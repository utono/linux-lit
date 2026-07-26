//! Syntax diagram: build the request, dispatch it, route the reply into the
//! full-screen Cairo surface.
//!
//! The `line_syntax` enrichment is optional in exactly ONE place — the parse
//! table is empty for the 301 unparsed works and the prompt simply omits that
//! section. There is no second code path and no coverage gate.

use crate::app::AppState;
use gtk4::prelude::TextBufferExt;
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
        let mut s = state_rc.borrow_mut();
        // Capture the mode we're opening FROM (Reader, or whichever overlay
        // mode the caller already restored before calling us) so every exit
        // path — Escape, bad-JSON, API-error — returns here instead of
        // hard-coding Reader. `SyntaxOverlay`'s methods are `&self` on their
        // own interior `RefCell` and never touch `AppState`, so calling
        // `show_loading` under this same `borrow_mut` is safe: single scope,
        // no double borrow.
        s.syntax_return_mode = Some(s.input_mode);
        // Loading state BEFORE the request — run_claude_request's contract.
        s.syntax_overlay.show_loading(&text, &s.theme);
        s.input_mode = crate::app::InputMode::SyntaxDiagram;
        s.config.claude_model.clone()
    };

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
                    s.input_mode = s
                        .syntax_return_mode
                        .take()
                        .unwrap_or(crate::app::InputMode::Reader);
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
            s.input_mode = s
                .syntax_return_mode
                .take()
                .unwrap_or(crate::app::InputMode::Reader);
            crate::input::navigation::show_chapter_toast_secs(&s, msg, 3);
        },
    );
}

/// Open the diagram for the sentence containing the currently underlined
/// words (`-` / `_` then `Return`).
///
/// Window: the cursor's buffer line plus one either side. That covers a
/// sentence spanning a verse break without risking a whole-chapter scan.
///
/// The window is joined from BUFFER text, not `work.lines[..].text`, because
/// `collect_ranges` are char offsets into the BUFFER line (see
/// `extract_buffer_line_words`). Phase B's inline italics delete `_`
/// delimiters from the buffer, so on a work with italics the buffer and DB
/// strings differ in length and the offsets do not transfer. Work lines are
/// consulted ONLY for `line_mapping` ids, which feed `line_syntax` enrichment.
pub fn open_syntax_diagram_for_underlined(state_rc: &Rc<RefCell<AppState>>) {
    let (text, line_ids) = {
        let state = state_rc.borrow();

        let ranges: Vec<(usize, usize)> =
            crate::input::actions::word_copy::active_underline(&state).to_vec();
        if ranges.is_empty() {
            return;
        }

        let cursor = state.current_line;
        let last_line = (state.buffer.line_count().max(1) as usize) - 1;
        let first = cursor.saturating_sub(1);
        let last = (cursor + 1).min(last_line);

        // Join the window from buffer text, recording where the cursor's own
        // line starts so the underline offsets can be rebased into it.
        let mut window = String::new();
        let mut cursor_line_offset = 0usize;
        for bl in first..=last {
            if bl == cursor {
                cursor_line_offset = window.chars().count();
            }
            let start = match state.buffer.iter_at_line(bl as i32) {
                Some(it) => it,
                None => continue,
            };
            let mut end = start;
            if !end.ends_line() {
                end.forward_to_line_end();
            }
            window.push_str(state.buffer.text(&start, &end, false).as_str());
            if bl != last {
                window.push('\n');
            }
        }

        let rebased: Vec<(usize, usize)> = ranges
            .iter()
            .map(|&(s, e)| (s + cursor_line_offset, e + cursor_line_offset))
            .collect();

        let span = match crate::input::sentence::sentence_span(&window, &rebased) {
            Some(sp) => sp,
            None => return,
        };
        let text: String = window.chars().skip(span.0).take(span.1 - span.0).collect();

        // Ids for enrichment. The sentence can only touch lines inside the
        // window, so mapping the whole window is correct and cheap.
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        let ids: Vec<i64> = (first..=last)
            .filter_map(|bl| {
                state
                    .work_line_for_buffer(bl)
                    .and_then(|wi| work.lines.get(wi))
                    .map(|l| l.id)
            })
            .collect();

        crate::logging::log(&format!(
            "SYNTAX_UNDERLINE: {} range(s) -> span {}..{} over {} line(s)",
            ranges.len(),
            span.0,
            span.1,
            ids.len()
        ));
        (text, ids)
    };

    // `open_syntax_diagram` already guards empty/whitespace-only text
    // (`if text.trim().is_empty()` → log, return), so do not duplicate it here.
    open_syntax_diagram(state_rc, text, line_ids);
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
