//! OP-IPA / bracket markup processing extracted from `gloss_overlay`.
//! Pure string transforms. `pub(crate)` items are consumed by
//! `input::actions::gloss`; `strip_ipa` is also called by `gloss_block`.

use crate::ui::gloss_overlay::{GlossElement, parse_gloss_tags};
use crate::ui::gloss_util::split_echo;

pub(crate) fn strip_brackets(text: &str) -> String {
    let mut result = String::new();
    let mut in_bracket = false;
    for ch in text.chars() {
        match ch {
            '[' => in_bracket = true,
            ']' => { in_bracket = false; }
            _ if !in_bracket => result.push(ch),
            _ => {}
        }
    }
    result
}

/// Remove inline `/IPA/` pronunciation spans for DISPLAY. Mirrors
/// `strip_brackets`. An IPA span is `/…/` whose contents contain at least one
/// non-ASCII-letter / IPA-class character (length marks, stress marks, schwa,
/// etc.), so a bare literal slash between plain words ("and/or") is NOT treated
/// as a span and survives. The raw, IPA-bearing text is what TTS gets; this is
/// the reader-facing form. See the gloss-IPA spec, §4.
///
/// The prompt appends IPA after a word (`Dread /drɛːd/ sovereign`), so naively
/// dropping the span leaves the two flanking spaces collapsed into a doubled
/// gap (`Dread  sovereign`), and IPA before punctuation leaves a space before it
/// (`good /gʊd/,` → `good ,`). After removing spans we therefore normalize
/// whitespace: collapse internal space runs to one, drop any space immediately
/// before `,;:.!?`, and trim the ends. This is display-only (every caller is a
/// render path); the stored gloss text is untouched.
/// Decide whether a `/…/` run is an inline IPA span (vs. a literal slash like
/// `and/or`). Two signals, either of which marks it IPA:
///
/// 1. **inner has a non-ASCII-letter char** — length `ː`, stress `ˈ`, schwa `ə`,
///    etc. Catches the overwhelming majority of OP IPA.
/// 2. **the opening `/` sits on a word boundary** (preceded by whitespace, start
///    of text, or punctuation) — catches the all-ASCII IPA spans the prompt
///    still emits, e.g. `have /hav/`, where the inner is `hav` (all ASCII
///    letters) and signal 1 alone would misread it as a literal slash. A literal
///    `and/or` fails this test because its slash is glued to a letter on the left.
///
/// Without signal 2, an all-ASCII span like `/hav/` was left unstripped, and its
/// two slashes then mis-paired with the NEXT span's slash, swallowing the text
/// between them (the `have /hav/ been. 'Tis a cruelty /ˈkruːəltɪ/` →
/// `have /havˈkruːəltɪ/` corruption seen in the reader).
fn is_ipa_span(inner: &[char], opener_on_boundary: bool) -> bool {
    if inner.is_empty() {
        return false;
    }
    inner.iter().any(|&c| !c.is_ascii_alphabetic()) || opener_on_boundary
}

/// True if the char before an opening `/` (or its absence at index 0) marks a
/// word boundary — so a free-standing `/word/` token is distinguished from a
/// letter-glued literal like `and/or`.
fn opener_on_boundary(chars: &[char], slash_idx: usize) -> bool {
    match slash_idx.checked_sub(1).map(|p| chars[p]) {
        None => true, // start of text
        Some(c) => c.is_whitespace() || matches!(c, ',' | ';' | ':' | '.' | '!' | '?' | '(' | '['),
    }
}

pub(crate) fn strip_ipa(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut stripped = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' {
            if let Some(close_rel) = chars[i + 1..].iter().position(|&c| c == '/') {
                let close = i + 1 + close_rel;
                let inner = &chars[i + 1..close];
                if is_ipa_span(inner, opener_on_boundary(&chars, i)) {
                    i = close + 1; // skip the whole /…/ span
                    continue;
                }
            }
        }
        stripped.push(chars[i]);
        i += 1;
    }
    normalize_ipa_whitespace(&stripped)
}

/// Collapse the spacing artifacts left behind when `strip_ipa` removes an inline
/// IPA span: runs of spaces become one, a space directly before `,;:.!?` is
/// dropped, and leading/trailing spaces are trimmed. Only ASCII space `' '` is
/// collapsed — newlines and other whitespace are preserved verbatim so verse
/// line structure (the gloss text is single-line per block here, but be safe)
/// and the seek-matcher key stay intact.
fn normalize_ipa_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = false;
    for c in text.chars() {
        if c == ' ' {
            // Defer emitting the space: collapse runs and allow it to be
            // dropped if the next char is punctuation.
            prev_space = true;
            continue;
        }
        if prev_space {
            // Emit a single space unless it would sit before close punctuation
            // or a newline (so a deferred space never trails a line).
            if !matches!(c, ',' | ';' | ':' | '.' | '!' | '?' | '\n') {
                out.push(' ');
            }
            prev_space = false;
        }
        out.push(c);
    }
    // A trailing run of spaces (prev_space == true) is intentionally dropped.
    out.trim().to_string()
}

/// Build the TTS-facing form of a verse block: REPLACE each appended `word
/// /IPA/` pair with just `/IPA/`, so ElevenLabs `eleven_v3` voices the word once
/// (via the IPA) instead of saying the plain word AND the IPA (the doubling
/// bug). The prompt emits `take /tɛːk/` — word then IPA — which the reader path
/// strips back to `take` (see `strip_ipa`); for audio we do the opposite and
/// drop the preceding word, leaving `/tɛːk/`, the docs' single-pronunciation
/// form. Words with NO following IPA span are left untouched (sparse tagging:
/// only operative words carry OP). Detection of an IPA span matches `strip_ipa`
/// (the shared [`is_ipa_span`] heuristic: non-ASCII inner OR a boundary-anchored
/// `/word/`), so an all-ASCII span like `/hav/` is handled while a letter-glued
/// literal `and/or` is not. This is applied ONLY on the TTS path; the stored
/// gloss text and the reader display are unchanged.
pub(crate) fn ipa_for_tts(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' {
            if let Some(close_rel) = chars[i + 1..].iter().position(|&c| c == '/') {
                let close = i + 1 + close_rel;
                let inner = &chars[i + 1..close];
                if is_ipa_span(inner, opener_on_boundary(&chars, i)) {
                    // Drop the word the IPA annotates: remove a single run of
                    // spaces then the immediately-preceding word from `out`, so
                    // `take /tɛːk/` becomes `/tɛːk/`. Stop at the word boundary
                    // (space) or punctuation so we never eat earlier words or
                    // the punctuation between them.
                    while matches!(out.last(), Some(' ')) {
                        out.pop();
                    }
                    while let Some(&c) = out.last() {
                        // Stop at a word boundary: space, punctuation, newline,
                        // or a prior IPA span's closing `/` (so two adjacent
                        // spans never eat each other — defensive; the prompt
                        // always separates spans with a plain word).
                        if c == ' '
                            || c == '/'
                            || matches!(c, ',' | ';' | ':' | '.' | '!' | '?' | '\n')
                        {
                            break;
                        }
                        out.pop();
                    }
                    // Re-insert a separating space if the IPA now abuts a prior
                    // word/punctuation (so `good, take /tɛːk/` -> `good, /tɛːk/`,
                    // not `good,/tɛːk/`).
                    if matches!(out.last(), Some(c) if *c != ' ' && *c != '\n') {
                        out.push(' ');
                    }
                    // Copy the IPA span verbatim.
                    out.extend(&chars[i..=close]);
                    i = close + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    let s: String = out.into_iter().collect();
    // Reuse the same whitespace normalization the display path uses, so any
    // doubled gaps / pre-punctuation spaces left by the word removal are cleaned.
    normalize_ipa_whitespace(&s)
}

/// True if `s` contains an inline IPA span. Uses the shared [`is_ipa_span`]
/// heuristic (non-ASCII inner OR a boundary-anchored `/word/`), so it agrees
/// with `strip_ipa`/`ipa_for_tts` and recognizes all-ASCII spans like `/hav/`
/// while still rejecting a letter-glued literal `and/or`. Used to decide whether
/// a fix-IPA input is a literal `/IPA/` or a plain hint.
pub(crate) fn contains_ipa_span(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' {
            if let Some(rel) = chars[i + 1..].iter().position(|&c| c == '/') {
                let close = i + 1 + rel;
                let inner = &chars[i + 1..close];
                if is_ipa_span(inner, opener_on_boundary(&chars, i)) {
                    return true;
                }
                i = close + 1;
                continue;
            }
        }
        i += 1;
    }
    false
}

/// Replace the `/IPA/` that immediately follows each whole-word, case-insensitive
/// occurrence of `word` in `text` with `new_ipa` (which includes its slashes,
/// e.g. `"/ˈdeɪli/"`). Returns the rewritten text, or `None` if no
/// `word /IPA/` pair was found (nothing changed). A match requires the word as a
/// whole token (not a substring) directly followed (after one run of spaces) by
/// an IPA span. Used by the gloss-overlay `i` (fix-IPA) flow on a source block's
/// text.
pub(crate) fn replace_word_ipa(text: &str, word: &str, new_ipa: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let wlc: Vec<char> = word.to_ascii_lowercase().chars().collect();
    if wlc.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut replaced = false;
    while i < chars.len() {
        let at_word_boundary = i == 0 || !chars[i - 1].is_alphanumeric();
        let word_matches = at_word_boundary
            && i + wlc.len() <= chars.len()
            && chars[i..i + wlc.len()]
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .eq(wlc.iter().copied())
            && chars
                .get(i + wlc.len())
                .map_or(true, |c| !c.is_alphanumeric());
        if word_matches {
            // word, then a run of spaces, then an IPA span -> replace the span.
            let mut k = i + wlc.len();
            while k < chars.len() && chars[k] == ' ' {
                k += 1;
            }
            if k < chars.len() && chars[k] == '/' {
                if let Some(rel) = chars[k + 1..].iter().position(|&c| c == '/') {
                    let close = k + 1 + rel;
                    let inner = &chars[k + 1..close];
                    let is_ipa =
                        !inner.is_empty() && inner.iter().any(|&c| !c.is_ascii_alphabetic());
                    if is_ipa {
                        out.extend(&chars[i..k]); // word + original spacing verbatim
                        out.push_str(new_ipa);
                        i = close + 1;
                        replaced = true;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    if replaced {
        Some(out)
    } else {
        None
    }
}

/// Rewrite the `/IPA/` after `word` (whole-word, all occurrences) within ONLY
/// the source block at `source_index`, operating on the TAGGED `gloss_text`
/// (each verse line is wrapped in `<verse>…</verse>`). Returns the full updated
/// gloss_text, or None if that block has no `word /IPA/` pair. Other blocks are
/// untouched even if they contain the same word.
///
/// Scoped by POSITION, not text: each `<verse>` is identified by its
/// document-order ordinal, and only verses belonging to the target source run
/// (per `gloss_blocks`' exact flush rule) are rewritten. This distinguishes
/// byte-identical verse lines that appear in different source blocks (e.g. a
/// repeated refrain) — a text-membership match would wrongly rewrite both.
pub(crate) fn replace_word_ipa_in_source_block(
    gloss_text: &str,
    source_index: i32,
    word: &str,
    new_ipa: &str,
) -> Option<String> {
    // Phase 1: which verse ORDINALS (0-based, document order) belong to the
    // target source block? Mirror gloss_blocks' flush rule exactly: a non-echo
    // <gloss> flushes the pending source run, and the source index advances
    // ONLY when that pending run is non-empty (matching flush_source).
    let mut target_ordinals: std::collections::HashSet<usize> = std::collections::HashSet::new();
    {
        let mut cur_source = 0i32;
        let mut verse_ord = 0usize;
        let mut pending_ords: Vec<usize> = Vec::new();
        for el in parse_gloss_tags(gloss_text) {
            match el {
                GlossElement::Verse(_) => {
                    pending_ords.push(verse_ord);
                    verse_ord += 1;
                }
                GlossElement::Gloss(text) => {
                    if split_echo(&text).is_some() {
                        continue; // echo bracket: does not flush
                    }
                    // non-echo gloss flushes the current source run (if non-empty)
                    if !pending_ords.is_empty() {
                        if cur_source == source_index {
                            target_ordinals.extend(pending_ords.iter().copied());
                        }
                        cur_source += 1;
                        pending_ords.clear();
                    }
                }
                GlossElement::Speaker(_) | GlossElement::Pron(_) => {}
            }
        }
        // trailing run (gloss that ends on verse)
        if !pending_ords.is_empty() && cur_source == source_index {
            target_ordinals.extend(pending_ords.iter().copied());
        }
    }
    if target_ordinals.is_empty() {
        return None; // no such source block / no verses
    }

    // Phase 2: walk raw <verse>…</verse> spans by ordinal (same document order
    // as Phase 1, since parse_gloss_tags emits one Verse per tag in order);
    // rewrite only target ones, copy everything else verbatim.
    let mut out = String::with_capacity(gloss_text.len());
    let mut rest = gloss_text;
    let mut ord = 0usize;
    let mut any = false;
    while let Some(open) = rest.find("<verse>") {
        let after_open = open + "<verse>".len();
        out.push_str(&rest[..after_open]);
        let tail = &rest[after_open..];
        if let Some(close_rel) = tail.find("</verse>") {
            let inner = &tail[..close_rel];
            if target_ordinals.contains(&ord) {
                if let Some(fixed) = replace_word_ipa(inner, word, new_ipa) {
                    out.push_str(&fixed);
                    any = true;
                } else {
                    out.push_str(inner);
                }
            } else {
                out.push_str(inner);
            }
            out.push_str("</verse>");
            rest = &tail[close_rel + "</verse>".len()..];
            ord += 1;
        } else {
            // malformed: no closing tag — copy the remainder and stop
            out.push_str(tail);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    if any {
        Some(out)
    } else {
        None
    }
}

#[cfg(test)]
mod ipa_tests {
    use super::*;

    #[test]
    fn strip_ipa_removes_tagged_words() {
        // The removed span must not leave a doubled gap, and a trailing span
        // must not leave a trailing space.
        assert_eq!(strip_ipa("To /biː/ or not to /biː/"), "To or not to");
    }

    #[test]
    fn strip_ipa_keeps_literal_slash() {
        // a bare slash between ordinary words is NOT an IPA span
        assert_eq!(strip_ipa("read and/or write"), "read and/or write");
    }

    #[test]
    fn strip_ipa_strips_all_ascii_span() {
        // `/hav/` is valid OP IPA whose inner is all ASCII letters. The old
        // non-ASCII-only heuristic missed it, so its slashes mis-paired with the
        // NEXT span and swallowed the text between — the reader saw
        // `have /havˈkruːəltɪ/`. The boundary signal (space before the opening
        // `/`) now marks it IPA and strips it cleanly.
        assert_eq!(
            strip_ipa("For what they have /hav/ been. 'Tis a cruelty /ˈkruːəltɪ/"),
            "For what they have been. 'Tis a cruelty"
        );
    }

    #[test]
    fn strip_ipa_all_ascii_span_does_not_eat_following_text() {
        // Minimal form of the corruption: an all-ASCII span must not swallow the
        // words up to the next span.
        assert_eq!(strip_ipa("a /hav/ b /biː/"), "a b");
    }

    #[test]
    fn strip_ipa_all_ascii_span_does_not_collapse_newlines() {
        // The accent-bar bug: a Source block's `display` is strip_ipa(verses
        // joined by '\n'). The old heuristic let an all-ASCII `/hav/` span
        // mis-pair across the '\n', collapsing a 5-line block into 4 lines whose
        // last line no longer matched any buffer line — so the bar's end_line
        // matcher failed and the bar shrank to one line ([1,1] in the log).
        // strip must preserve every newline so the block keeps its line count.
        let block = "However faulty, yet should find /fəɪnd/ respect\n\
                     For what they have /hav/ been. 'Tis a cruelty /ˈkruːəltɪ/\n\
                     To load /loːd/ a falling /ˈfɑlɪn/ man.";
        let stripped = strip_ipa(block);
        assert_eq!(stripped.lines().count(), 3, "newlines must survive stripping");
        assert_eq!(
            stripped.lines().last().unwrap(),
            "To load a falling man.",
            "last line must stay matchable for the accent-bar end_line lookup"
        );
        assert!(!stripped.contains('/'), "no slash should remain: {stripped:?}");
    }

    #[test]
    fn strip_ipa_no_tags_is_identity() {
        assert_eq!(strip_ipa("plain modern line"), "plain modern line");
    }

    #[test]
    fn strip_ipa_handles_stress_marks() {
        assert_eq!(strip_ipa("the /ˈsʊfər/ of it"), "the of it");
    }

    #[test]
    fn strip_ipa_removes_leaked_prose_ipa() {
        // IPA the LLM might leak into explication prose must be strippable,
        // with no doubled/trailing spaces.
        assert_eq!(
            strip_ipa("the modern diphthong /eɪ/ vs the older /eː/"),
            "the modern diphthong vs the older"
        );
    }

    #[test]
    fn strip_ipa_no_space_before_punctuation() {
        // IPA appended to a word before punctuation must not leave a space
        // before the comma/semicolon/period (the screenshot's "good , wise").
        assert_eq!(
            strip_ipa("Not only good /gʊd/, but most religious /rɪˈlɪdʒəs/;"),
            "Not only good, but most religious;"
        );
        assert_eq!(
            strip_ipa("this great offender /ɒˈfɛndər/."),
            "this great offender."
        );
    }

    #[test]
    fn strip_ipa_real_verse_line_single_spaced() {
        // The reported bug: a full verse line with several appended IPA spans
        // renders with clean single spacing.
        assert_eq!(
            strip_ipa("Dread /drɛːd/ sovereign, how much are we bound /baʊnd/ to heaven /ˈhɛvn̩/"),
            "Dread sovereign, how much are we bound to heaven"
        );
    }

    #[test]
    fn ipa_for_tts_replaces_word_with_its_ipa() {
        // 'take /tɛːk/' -> '/tɛːk/' (word dropped) so v3 voices it once.
        assert_eq!(ipa_for_tts("take /tɛːk/"), "/tɛːk/");
        assert_eq!(ipa_for_tts("To take /tɛːk/ arms"), "To /tɛːk/ arms");
    }

    #[test]
    fn ipa_for_tts_all_ascii_span_replaces_its_word() {
        // The TTS twin of strip_ipa_strips_all_ascii_span: `have /hav/` must
        // become `/hav/` (word dropped, span kept), not get mis-paired with the
        // next span.
        assert_eq!(
            ipa_for_tts("they have /hav/ been. 'Tis a cruelty /ˈkruːəltɪ/"),
            "they /hav/ been. 'Tis a /ˈkruːəltɪ/"
        );
    }

    #[test]
    fn ipa_for_tts_keeps_untagged_words() {
        // Sparse tagging: words with no following IPA span are spoken as-is.
        assert_eq!(
            ipa_for_tts("Dread /drɛːd/ sovereign, how much are we bound /baʊnd/ to heaven /ˈhɛvn̩/"),
            "/drɛːd/ sovereign, how much are we /baʊnd/ to /ˈhɛvn̩/"
        );
    }

    #[test]
    fn ipa_for_tts_before_punctuation() {
        // 'good /gʊd/,' -> '/gʊd/,' — the IPA replaces the word but the comma
        // stays attached, no doubled word.
        assert_eq!(
            ipa_for_tts("Not only good /gʊd/, but most religious /rɪˈlɪdʒəs/;"),
            "Not only /gʊd/, but most /rɪˈlɪdʒəs/;"
        );
    }

    #[test]
    fn ipa_for_tts_ipa_at_line_start_is_kept() {
        // A leading IPA span with no preceding word stays as-is (nothing to drop).
        assert_eq!(ipa_for_tts("/biː/ or not"), "/biː/ or not");
        // Each IPA span replaces the word immediately before it: here the second
        // /biː/ follows "to", so "to" is the word it replaces.
        assert_eq!(ipa_for_tts("/biː/ or not to /biː/"), "/biː/ or not /biː/");
    }

    #[test]
    fn ipa_for_tts_keeps_literal_slash_and_plain() {
        // 'and/or' is not IPA; a no-IPA line is identity.
        assert_eq!(ipa_for_tts("read and/or write"), "read and/or write");
        assert_eq!(ipa_for_tts("plain modern line"), "plain modern line");
    }

    #[test]
    fn ipa_for_tts_adjacent_spans_dont_eat_each_other() {
        // Defensive: the prompt never emits two IPA spans with no word between,
        // but if it did, the word-pop must stop at the prior span's closing `/`
        // and not delete it. Each span survives.
        assert_eq!(ipa_for_tts("/biː/ /tuː/"), "/biː/ /tuː/");
        // 'be' precedes the first span and is dropped; the second span has only a
        // prior IPA span before it, so the guard keeps that span intact.
        assert_eq!(ipa_for_tts("To be /biː/ /tuː/"), "To /biː/ /tuː/");
    }

    #[test]
    fn contains_ipa_span_detects_real_ipa() {
        assert!(contains_ipa_span("/ˈdeɪli/"));
        assert!(contains_ipa_span("daily /ˈdeɪli/"));
        assert!(!contains_ipa_span("hard a"));          // plain hint, no slashes
        assert!(!contains_ipa_span("and/or"));          // glued literal slash, not on a boundary
        // A boundary-anchored slash token IS IPA even with an all-ASCII inner —
        // the same rule that lets strip_ipa handle `/hav/`. A user who wraps a
        // token in slashes means it as a pronunciation, and an all-ASCII OP span
        // (e.g. `/hav/`) must be recognized, not mistaken for a plain hint.
        assert!(contains_ipa_span("/word/"));
        assert!(contains_ipa_span("have /hav/"));
        assert!(!contains_ipa_span(""));
    }

    #[test]
    fn replace_word_ipa_swaps_the_words_ipa() {
        assert_eq!(
            replace_word_ipa("In daily /ˈdɛːli/ thanks, that gave /gɛːv/ us", "daily", "/ˈdeɪli/"),
            Some("In daily /ˈdeɪli/ thanks, that gave /gɛːv/ us".to_string())
        );
    }

    #[test]
    fn replace_word_ipa_all_occurrences() {
        assert_eq!(
            replace_word_ipa("good /gʊd/ and more good /gʊd/", "good", "/guːd/"),
            Some("good /guːd/ and more good /guːd/".to_string())
        );
    }

    #[test]
    fn replace_word_ipa_is_whole_word() {
        assert_eq!(replace_word_ipa("daily /ˈdɛːli/ here", "day", "/deɪ/"), None);
    }

    #[test]
    fn replace_word_ipa_word_without_following_ipa_is_none() {
        assert_eq!(replace_word_ipa("In daily /ˈdɛːli/ thanks", "thanks", "/θaŋks/"), None);
    }

    #[test]
    fn replace_word_ipa_case_insensitive_word_match() {
        assert_eq!(
            replace_word_ipa("Daily /ˈdɛːli/ thanks", "daily", "/ˈdeɪli/"),
            Some("Daily /ˈdeɪli/ thanks".to_string())
        );
    }

    #[test]
    fn replace_in_source_block_rewrites_multiline_verse() {
        let g = "<speaker>GARDINER</speaker>\n<verse>In daily /ˈdɛːli/ thanks</verse>\n<verse>that gave /gɛːv/ us</verse>\n<gloss>note</gloss>";
        let out = replace_word_ipa_in_source_block(g, 0, "daily", "/ˈdeɪli/").unwrap();
        assert!(out.contains("daily /ˈdeɪli/"));
        assert!(out.contains("gave /gɛːv/")); // other word untouched
        assert!(out.contains("<gloss>note</gloss>")); // tags intact
        assert!(out.contains("<verse>"));
    }

    #[test]
    fn replace_in_source_block_none_when_word_absent() {
        let g = "<verse>In daily /ˈdɛːli/ thanks</verse>";
        assert!(replace_word_ipa_in_source_block(g, 0, "missing", "/x/").is_none());
    }

    #[test]
    fn replace_in_source_block_scopes_to_the_block() {
        // 'good' appears in TWO source blocks; fixing block 1 must not touch block 0.
        let g = "<verse>good /gʊd/ first</verse>\n<gloss>a</gloss>\n<verse>good /gʊd/ second</verse>\n<gloss>b</gloss>";
        let out = replace_word_ipa_in_source_block(g, 1, "good", "/guːd/").unwrap();
        // block 0 (index 0) keeps old IPA; block 1 (index 1) gets new.
        let first = out.find("first").unwrap();
        let second = out.find("second").unwrap();
        assert!(out[..first].contains("good /gʊd/")); // block 0 unchanged
        assert!(out[..second].contains("good /guːd/")); // block 1 changed
    }

    #[test]
    fn replace_in_source_block_distinguishes_identical_lines_across_blocks() {
        // Same verse line text in TWO source blocks; fixing block 1 must leave
        // block 0's identical line untouched (position-scoped, not text-scoped).
        let g = "<verse>good /gʊd/ same</verse>\n<gloss>a</gloss>\n<verse>good /gʊd/ same</verse>\n<gloss>b</gloss>";
        let out = replace_word_ipa_in_source_block(g, 1, "good", "/guːd/").unwrap();
        // exactly ONE rewrite: block 1's. Block 0 keeps /gʊd/.
        assert_eq!(out.matches("good /guːd/ same").count(), 1);
        assert_eq!(out.matches("good /gʊd/ same").count(), 1);
    }
}
