//! Syntax diagram: build the request, dispatch it, route the reply into the
//! full-screen Cairo surface.
//!
//! The `line_syntax` enrichment is optional in exactly ONE place — the parse
//! table is empty for the 301 unparsed works and the prompt simply omits that
//! section. There is no second code path and no coverage gate.

use crate::app::AppState;
use std::cell::RefCell;
use std::rc::Rc;

/// Compiled fallback, used when lit.db has no `syntax.diagram` row.
const FALLBACK_PROMPT: &str = "\
You analyze the grammatical structure of a passage of literature and return \
ONLY a JSON object — no prose, no markdown fence, no commentary outside the \
JSON.

The JSON object has exactly these keys:

{
  \"bands\": [{\"start_char\": 0, \"end_char\": 19, \"label\": \"subject\", \"depth\": 0}],
  \"pos\": [{\"start_char\": 0, \"end_char\": 1, \"pos\": \"DET\"}],
  \"note\": \"Two or three sentences on what this structure is doing.\"
}

`bands` mark what each stretch of the passage grammatically IS — \"main \
clause\", \"relative clause\", \"appositive\", \"subject\", \"predicate\", \
\"participial modifier\". `depth` is nesting depth: 0 is the outermost span, \
and a band at depth N+1 must be fully CONTAINED in a band at depth N. Bands at \
the same depth must not overlap. Partially overlapping bands are discarded, so \
never emit them.

`start_char` and `end_char` are BYTE offsets into the passage exactly as given, \
counted from 0. Be precise: an offset that lands mid-word or past the end of \
the text discards that band.

`pos` gives a part-of-speech tag per word, using the coarse Universal \
Dependencies set (ADJ, ADP, ADV, AUX, CCONJ, DET, INTJ, NOUN, NUM, PART, PRON, \
PROPN, PUNCT, SCONJ, VERB).

`note` is two or three sentences on what the structure is DOING rhetorically — \
why a modifier is set off, what an inversion delays, how the subordination \
shapes the reading. Write for a thoughtful reader. No markdown. Set any work \
title in quotation marks, never asterisks.

The passage may be early modern English. Analyze the syntax as it actually \
stands, not as modern English would render it.";

/// The system prompt: lit.db `api_prompts` row `syntax.diagram`, else the
/// compiled fallback. Mirrors `gloss.rs`'s prompt-or-fallback pattern.
fn system_prompt() -> String {
    crate::db::prompts::active_prompt("syntax.diagram").unwrap_or_else(|| {
        crate::logging::log(
            "SYNTAX PROMPT: syntax.diagram missing from api_prompts; using compiled fallback",
        );
        FALLBACK_PROMPT.to_string()
    })
}

/// The user message: the passage, plus the parse table when the work has one.
/// `parse_table` is empty for unparsed works, which omits the section entirely.
fn build_user_message(text: &str, parse_table: &str) -> String {
    let mut msg = format!("Passage:\n{text}\n");
    if !parse_table.is_empty() {
        msg.push_str(
            "\nA dependency parse of this passage is available. Use it to \
             anchor your analysis; where it disagrees with your own reading of \
             the syntax (it was produced by a model trained on modern English \
             and misparses archaic constructions), trust your reading.\n\n",
        );
        msg.push_str(parse_table);
    }
    msg
}

/// Open the diagram for `text`. `line_ids` are `line_mapping` row ids for the
/// selection — empty for overlay selections (gloss/journal text has no
/// line_mapping rows), which simply means no enrichment.
pub fn open_syntax_diagram(
    state_rc: &Rc<RefCell<AppState>>,
    text: String,
    line_ids: Vec<i64>,
) {
    if text.trim().is_empty() {
        crate::logging::log("SYNTAX: empty selection, not opening");
        return;
    }

    let parse_table = if line_ids.is_empty() {
        String::new()
    } else {
        match crate::db::queries::open_db() {
            Ok(conn) => {
                let toks = crate::db::syntax::load_line_syntax(&conn, &line_ids);
                crate::logging::log(&format!(
                    "SYNTAX: {} parsed tokens for {} lines",
                    toks.len(),
                    line_ids.len()
                ));
                crate::db::syntax::tokens_as_table(&toks)
            }
            Err(_) => String::new(),
        }
    };

    let model = {
        let s = state_rc.borrow();
        // Loading state BEFORE the request — run_claude_request's contract.
        s.syntax_overlay.show_loading(&s.theme);
        s.config.claude_model.clone()
    };
    {
        let mut s = state_rc.borrow_mut();
        s.input_mode = crate::app::InputMode::SyntaxDiagram;
    }

    let user_msg = build_user_message(&text, &parse_table);
    let text_for_parse = text.clone();

    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        system_prompt(),
        user_msg,
        model,
        move |st, reply| {
            match crate::syntax_diagram::parse_analysis(&reply, &text_for_parse) {
                Ok(analysis) => {
                    crate::logging::log(&format!(
                        "SYNTAX: {} bands, {} pos tags",
                        analysis.bands.len(),
                        analysis.pos.len()
                    ));
                    let s = st.borrow();
                    s.syntax_overlay.show_analysis(analysis, &s.theme);
                }
                Err(e) => {
                    crate::logging::log(&format!("SYNTAX: {e}"));
                    let mut s = st.borrow_mut();
                    s.syntax_overlay.hide();
                    s.input_mode = crate::app::InputMode::Reader;
                    crate::input::navigation::show_chapter_toast_secs(
                        &s,
                        "Could not analyze syntax",
                        3,
                    );
                }
            }
        },
        move |st, msg| {
            let mut s = st.borrow_mut();
            s.syntax_overlay.hide();
            s.input_mode = crate::app::InputMode::Reader;
            crate::input::navigation::show_chapter_toast_secs(&s, msg, 3);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_includes_the_selection() {
        let msg = build_user_message("A touch, irresolute, makes him start", "");
        assert!(msg.contains("A touch, irresolute, makes him start"));
    }

    #[test]
    fn user_message_embeds_the_parse_when_present() {
        let table = "word\tPOS\tdep\thead\ntouch\tNOUN\tnsubj\t2\n";
        let msg = build_user_message("A touch", table);
        assert!(msg.contains("nsubj"), "parse table must be embedded");
        assert!(msg.contains("dependency parse"), "must label the table");
    }

    #[test]
    fn user_message_omits_the_parse_section_when_absent() {
        let msg = build_user_message("A touch", "");
        assert!(!msg.contains("dependency parse"),
            "no parse section when the work is unparsed");
    }
}
