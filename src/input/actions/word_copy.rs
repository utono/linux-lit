use gtk4::prelude::*;

use crate::app::AppState;
use crate::log_fmt;

/// Grouped state for the word-copy / word-cycle feature (cursor-word cycling,
/// multi-word phrase collection, and the bold-highlight generation counter).
/// Was five flat `word_cycle_*` / `word_collect_*` / `word_bold_gen` fields on
/// AppState; grouped per the AppState god-struct decomposition (pure-tier
/// cluster).
#[derive(Default)]
pub struct WordCycleState {
    pub cycle_line: Option<usize>,
    pub cycle_index: usize,
    pub bold_gen: std::rc::Rc<std::cell::Cell<u64>>,
    pub collect_words: Vec<String>,
    pub collect_ranges: Vec<(usize, usize)>,
}

/// Cycle FORWARD through the words on the current line, copying each to the
/// system clipboard and leaving it underlined. Wraps to the line's first word
/// after its last. `Shift+-` (`word_cycle_prev_copy`) is the mirror image.
///
/// Scope is the LINE, not the sentence: a sentence-scoped variant was tried on
/// 2026-07-26 and reverted the same day at the user's direction. `Ctrl+-`
/// (`underline_next_sentence`) is the sentence-level bind on this cap; `-`
/// and `Shift+-` step words within the line regardless of which sentence they
/// are in.
pub fn word_cycle_copy(state: &mut AppState) {
    word_cycle_step(state, 1)
}

/// Cycle BACKWARD through the words on the current line — the mirror of `-`.
/// Wraps to the line's LAST word when the cursor word is its first.
pub fn word_cycle_prev_copy(state: &mut AppState) {
    word_cycle_step(state, -1)
}

/// Shared body of `-` (`delta` = 1) and `Shift+-` (`delta` = -1).
///
/// Both walk the same line-scoped word list through the same `cycle_index`, so
/// the two directions compose: stepping forward three then back one lands on
/// the second word, and either direction wraps at its own end of the line.
fn word_cycle_step(state: &mut AppState, delta: i32) {
    if state.current_work.is_none() {
        return;
    }

    let words = extract_buffer_line_words(state);
    if words.is_empty() {
        return;
    }

    let same_line = state.word_cycle.cycle_line == Some(state.current_line);
    let idx = next_word_index(
        words.len(),
        same_line.then_some(state.word_cycle.cycle_index),
        delta,
    );

    let (ref word, char_start, char_end) = words[idx];

    // Copy to clipboard via wl-copy
    use std::io::Write;
    use std::process::{Command, Stdio};
    match Command::new("wl-copy").stdin(Stdio::piped()).spawn() {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(word.as_bytes());
            }
            let _ = child.wait();
        }
        Err(e) => {
            log_fmt!("WORD_COPY: wl-copy failed: {}", e);
            return;
        }
    }

    log_fmt!(
        "WORD_COPY: copied '{}' (word {}/{}, dir {})",
        word,
        idx + 1,
        words.len(),
        if delta < 0 { "back" } else { "fwd" }
    );

    // Update cycle state
    state.word_cycle.cycle_line = Some(state.current_line);
    state.word_cycle.cycle_index = idx + 1;

    // `-`/`Shift+-` are single-word modes, but the ONE word they underline must
    // still be a valid diagram selection, so set the collection to exactly it
    // rather than emptying it. `Alt+-` keeps appending from here.
    state.word_cycle.collect_words.clear();
    state.word_cycle.collect_words.push(word.clone());
    state.word_cycle.collect_ranges.clear();
    state.word_cycle.collect_ranges.push((char_start, char_end));

    // Remove any previous underline tag, then apply to the current word.
    // Persistent: cleared by Escape, by leaving the line, or by the next underline bind.
    apply_word_underline(state, &[(char_start, char_end)], true);
}

/// Collect words on the current line, accumulating across presses.
/// Each press (Alt+- / Alt+i) advances to the next word, appends it to the collection,
/// and copies all collected words (space-separated) to the clipboard.
/// Underlines all collected words. Resets on line change.
///
/// Scope stays the LINE (unlike `-`, which walks one sentence): `_` exists to
/// grab a whole line's worth of words, so a sentence limit would defeat it.
pub fn word_collect_copy(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }

    let words = extract_buffer_line_words(state);
    if words.is_empty() {
        return;
    }

    // Reset if we moved to a different line
    let idx = if state.word_cycle.cycle_line == Some(state.current_line) {
        state.word_cycle.cycle_index % words.len()
    } else {
        state.word_cycle.collect_words.clear();
        state.word_cycle.collect_ranges.clear();
        0
    };

    let (ref word, char_start, char_end) = words[idx];

    // Append to collection
    state.word_cycle.collect_words.push(word.clone());
    state.word_cycle.collect_ranges.push((char_start, char_end));

    // Copy all collected words to clipboard
    let phrase = state.word_cycle.collect_words.join(" ");
    use std::io::Write;
    use std::process::{Command, Stdio};
    match Command::new("wl-copy").stdin(Stdio::piped()).spawn() {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(phrase.as_bytes());
            }
            let _ = child.wait();
        }
        Err(e) => {
            log_fmt!("WORD_COLLECT: wl-copy failed: {}", e);
            return;
        }
    }

    log_fmt!("WORD_COLLECT: copied '{}' ({} words)", phrase, state.word_cycle.collect_words.len());

    // Update cycle state
    state.word_cycle.cycle_line = Some(state.current_line);
    state.word_cycle.cycle_index = idx + 1;

    // Underline all collected words
    let ranges: Vec<(usize, usize)> = state.word_cycle.collect_ranges.clone();
    // Persistent: cleared by Escape, by leaving the line, or by the next underline bind.
    apply_word_underline(state, &ranges, true);
}

/// Underline the FIRST WORD of the NEXT sentence (Ctrl+-; dup Ctrl+i).
///
/// Steps sentence by sentence through the cursor's buffer line, writing the
/// same `collect_ranges` the other underline binds write — so `Return`
/// (`OpenSyntaxDiagramForUnderlined`) opens a syntax gloss for the sentence
/// this marked, with no separate plumbing. Every bind in the family builds
/// ONE shared underline set.
///
/// Scope is the cursor's own line, matching the rest of this module: the
/// underline ranges are char offsets into `current_line` and
/// `active_underline` treats them as gone once the cursor leaves it (see
/// `underline_is_active`). A cross-line stepper would need a different
/// representation; `syntax.rs` widens to a 3-line window only when READING the
/// span, never when storing it.
///
/// With nothing underlined on this line yet, the first press marks the first
/// sentence. At the last sentence it wraps to the first, mirroring
/// `word_cycle_copy`'s wrap.
pub fn underline_next_sentence(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }

    let line_text = current_line_text(state);
    if line_text.trim().is_empty() {
        return;
    }

    // Where the current underline sits, if it is still this line's.
    let anchor = active_underline(state).iter().map(|r| r.1).max();

    let Some((start, end)) = next_sentence_first_word(&line_text, anchor) else {
        return;
    };

    state.word_cycle.cycle_line = Some(state.current_line);
    // Hand the walk over to `-`/`Shift+-`, which step the LINE's word list.
    // `cycle_index` is stored as "index of the underlined word + 1", so find
    // where the word we just underlined sits in that list and record it that
    // way — otherwise the next `-` would resume from wherever the previous
    // cycle stopped instead of continuing from this sentence.
    let line_words = extract_buffer_line_words(state);
    let word_pos = line_words.iter().position(|&(_, s, _)| s == start);
    state.word_cycle.cycle_index = word_pos.map(|i| i + 1).unwrap_or(0);
    let word: String = line_text.chars().skip(start).take(end - start).collect();
    state.word_cycle.collect_words.clear();
    state.word_cycle.collect_words.push(word.clone());
    state.word_cycle.collect_ranges.clear();
    state.word_cycle.collect_ranges.push((start, end));

    log_fmt!("SENTENCE_STEP: underlined '{}' at chars {}..{}", word, start, end);
    apply_word_underline(state, &[(start, end)], true);
}

/// Index of the word a `-` / `Shift+-` press should land on.
///
/// `n` is the line's word count, `stored` the `cycle_index` carried over from
/// the previous press on THIS line (`None` after a line change), and `delta`
/// the direction (+1 forward, -1 back).
///
/// `cycle_index` is stored as "underlined index + 1", so the currently
/// underlined word is `stored - 1` and stepping happens relative to that.
/// Both directions wrap: forward past the last word lands on the first, and
/// backward from the first lands on the LAST. A fresh line starts at the first
/// word going forward and the last going backward, so either key does
/// something useful as the first press on a line.
///
/// Pure because the wrap arithmetic (signed, modular) is the part worth
/// testing without a GTK buffer.
fn next_word_index(n: usize, stored: Option<usize>, delta: i32) -> usize {
    if n == 0 {
        return 0;
    }
    match stored {
        Some(stored) => {
            let current = stored as isize - 1;
            (current + delta as isize).rem_euclid(n as isize) as usize
        }
        None if delta < 0 => n - 1,
        None => 0,
    }
}

/// Char span of the first word of the sentence AFTER the one containing
/// `anchor`, within `text`. `None` when there is no word to land on.
///
/// Pure so the wrap/boundary behavior is unit-testable without an `AppState`.
/// With `anchor` `None` this returns the first word of the whole text; past the
/// last sentence it wraps to the first.
fn next_sentence_first_word(text: &str, anchor: Option<usize>) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }

    // No live underline → start at the first sentence.
    let Some(anchor) = anchor else {
        return first_word_from(&chars, 0);
    };

    // Walk to the end of the anchor's sentence, then to the word after it.
    let from = anchor.min(chars.len().saturating_sub(1));
    let sentence_end = crate::input::sentence::sentence_span(text, &[(from, from + 1)])
        .map(|(_, e)| e)
        .unwrap_or(chars.len());

    // Past the last sentence → wrap to the first.
    match first_word_from(&chars, sentence_end) {
        Some(span) => Some(span),
        None => first_word_from(&chars, 0),
    }
}

/// First alphanumeric-bearing word at or after char index `from`.
fn first_word_from(chars: &[char], from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while i < chars.len() {
        // Skip anything that cannot start a word (space, quotes, dashes).
        if !chars[i].is_alphanumeric() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '\'') {
            i += 1;
        }
        return Some((start, i));
    }
    None
}

/// Text of the cursor's buffer line — the same region
/// `extract_buffer_line_words` tokenizes, so char offsets are interchangeable
/// between the two.
fn current_line_text(state: &AppState) -> String {
    let start = match state.buffer.iter_at_line(state.current_line as i32) {
        Some(it) => it,
        None => return String::new(),
    };
    let mut end = start;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    state.buffer.text(&start, &end, false).to_string()
}

/// Extract words from the current buffer line with their char offsets.
fn extract_buffer_line_words(state: &AppState) -> Vec<(String, usize, usize)> {
    let buf_line_start = state.buffer.iter_at_line(state.current_line as i32).unwrap();
    let buf_line_end = {
        let mut it = buf_line_start;
        if !it.ends_line() { it.forward_to_line_end(); }
        it
    };
    let buf_line_text = state.buffer.text(&buf_line_start, &buf_line_end, false).to_string();

    let mut words: Vec<(String, usize, usize)> = Vec::new();
    for token in buf_line_text.split_whitespace() {
        let token_byte_start = token.as_ptr() as usize - buf_line_text.as_ptr() as usize;
        let token_char_start = buf_line_text[..token_byte_start].chars().count();
        let stripped = token.trim_matches(|c: char| !c.is_alphanumeric());
        if stripped.is_empty() {
            continue;
        }
        let strip_byte_offset = stripped.as_ptr() as usize - token.as_ptr() as usize;
        let char_start = token_char_start + token[..strip_byte_offset].chars().count();
        let char_end = char_start + stripped.chars().count();
        words.push((stripped.to_string(), char_start, char_end));
    }
    words
}

/// Apply the underline tag to the given char ranges on the current line,
/// removing any previous underline first.
///
/// `persist`: when true the 2-second auto-remove timer is NOT armed, so the
/// underline stays until explicitly cleared (Escape, or another underline
/// bind that replaces it). `bold_gen` is still bumped either way, which
/// invalidates any timer already in flight from an earlier non-persistent
/// call.
fn apply_word_underline(state: &mut AppState, ranges: &[(usize, usize)], persist: bool) {
    let buf = &state.buffer;
    let tag = &state.word_bold_tag;
    let (buf_start, buf_end) = (buf.start_iter(), buf.end_iter());
    buf.remove_tag(tag, &buf_start, &buf_end);

    let line_start = buf.iter_at_line(state.current_line as i32).unwrap();
    for &(char_start, char_end) in ranges {
        let mut word_start = line_start;
        word_start.forward_chars(char_start as i32);
        let mut word_end = word_start;
        word_end.forward_chars((char_end - char_start) as i32);
        buf.apply_tag(tag, &word_start, &word_end);
    }

    // Bump the generation counter unconditionally: it is what makes any timer
    // still in flight a no-op.
    let gen = state.word_cycle.bold_gen.get() + 1;
    state.word_cycle.bold_gen.set(gen);
    if persist {
        return;
    }

    // Auto-remove underline after 2 seconds
    let gen_rc = state.word_cycle.bold_gen.clone();
    let buf_clone = buf.clone();
    let tag_clone = tag.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
        if gen_rc.get() == gen {
            let (s, e) = (buf_clone.start_iter(), buf_clone.end_iter());
            buf_clone.remove_tag(&tag_clone, &s, &e);
        }
    });
}

/// Pure predicate behind `active_underline`, split out so it is testable
/// without an `AppState`.
///
/// Clearing is LAZY, not event-driven: `current_line` has ~76 write sites
/// across 14 modules, so hooking every cursor-move path would be
/// unimplementable and would rot on the next navigation feature. Instead the
/// underline carries the line it belongs to and is treated as absent once the
/// cursor leaves.
pub fn underline_is_active(
    cycle_line: Option<usize>,
    current_line: usize,
    ranges_len: usize,
) -> bool {
    ranges_len > 0 && cycle_line == Some(current_line)
}

/// The underlined ranges, but ONLY while they still belong to the cursor's
/// line. Single source of truth for the `Return` / `Escape` guards.
///
/// A tag that briefly outlives its line is cosmetic, not a correctness
/// problem, because nothing can act on it — this returns empty and both
/// guards fall through.
pub fn active_underline(state: &AppState) -> &[(usize, usize)] {
    if underline_is_active(
        state.word_cycle.cycle_line,
        state.current_line,
        state.word_cycle.collect_ranges.len(),
    ) {
        &state.word_cycle.collect_ranges
    } else {
        &[]
    }
}

/// Remove the underline tag and forget the collected words.
pub fn clear_word_underline(state: &mut AppState) {
    let (s, e) = (state.buffer.start_iter(), state.buffer.end_iter());
    state.buffer.remove_tag(&state.word_bold_tag, &s, &e);
    state.word_cycle.bold_gen.set(state.word_cycle.bold_gen.get() + 1);
    state.word_cycle.collect_words.clear();
    state.word_cycle.collect_ranges.clear();
    state.word_cycle.cycle_line = None;
    crate::logging::log("WORD_UNDERLINE: cleared");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underline_active_on_its_own_line() {
        assert!(underline_is_active(Some(42), 42, 2));
    }

    #[test]
    fn underline_inactive_after_cursor_leaves_the_line() {
        // Lazy clearing: the ranges are still in state, but they no longer
        // belong to the cursor's line, so nothing may act on them.
        assert!(!underline_is_active(Some(42), 43, 2));
    }

    #[test]
    fn underline_inactive_when_no_ranges() {
        assert!(!underline_is_active(Some(42), 42, 0));
    }

    #[test]
    fn underline_inactive_when_never_cycled() {
        assert!(!underline_is_active(None, 42, 2));
    }

    /// Helper: char-slice a span for readable assertions.
    fn slice(text: &str, span: (usize, usize)) -> String {
        text.chars().skip(span.0).take(span.1 - span.0).collect()
    }

    #[test]
    fn first_press_marks_the_first_word() {
        let text = "First one here. Second one there. Third.";
        let span = next_sentence_first_word(text, None).unwrap();
        assert_eq!(slice(text, span), "First");
    }

    #[test]
    fn steps_to_the_next_sentence_first_word() {
        let text = "First one here. Second one there. Third.";
        // Anchored inside sentence 1 ("First").
        let span = next_sentence_first_word(text, Some(5)).unwrap();
        assert_eq!(slice(text, span), "Second");
    }

    #[test]
    fn steps_again_from_the_second_sentence() {
        let text = "First one here. Second one there. Third.";
        // Anchored on "Second" (starts at char 16).
        let span = next_sentence_first_word(text, Some(22)).unwrap();
        assert_eq!(slice(text, span), "Third");
    }

    #[test]
    fn wraps_to_the_first_sentence_past_the_last() {
        let text = "First one here. Second one there.";
        // Anchored in the LAST sentence — nothing follows, so wrap.
        let span = next_sentence_first_word(text, Some(20)).unwrap();
        assert_eq!(slice(text, span), "First");
    }

    #[test]
    fn does_not_split_on_an_abbreviation() {
        // Shares sentence.rs's boundary rules: "Mr." is not a sentence end.
        let text = "Mr. Bucket looked at him. Then he left.";
        let span = next_sentence_first_word(text, Some(1)).unwrap();
        assert_eq!(slice(text, span), "Then");
    }

    #[test]
    fn skips_leading_quotes_and_punctuation() {
        let text = "He spoke. \"Quoted words follow.\"";
        let span = next_sentence_first_word(text, Some(2)).unwrap();
        assert_eq!(slice(text, span), "Quoted");
    }

    #[test]
    fn keeps_internal_apostrophes_in_the_word() {
        let text = "One sentence. Don't split this.";
        let span = next_sentence_first_word(text, Some(3)).unwrap();
        assert_eq!(slice(text, span), "Don't");
    }

    #[test]
    fn handles_multibyte_text_by_chars_not_bytes() {
        let text = "Æsop wrote it. Naïve reader—café.";
        let span = next_sentence_first_word(text, Some(2)).unwrap();
        assert_eq!(slice(text, span), "Naïve");
    }

    #[test]
    fn empty_text_yields_nothing() {
        assert_eq!(next_sentence_first_word("", None), None);
    }

    #[test]
    fn text_with_no_words_yields_nothing() {
        assert_eq!(next_sentence_first_word("  ... --- ", None), None);
    }

    #[test]
    fn single_sentence_wraps_onto_itself() {
        let text = "Only one sentence here.";
        let span = next_sentence_first_word(text, Some(5)).unwrap();
        assert_eq!(slice(text, span), "Only");
    }

    #[test]
    fn out_of_bounds_anchor_is_clamped_not_panicking() {
        let text = "Short sentence.";
        // A stale anchor past the end must not panic; wrapping is fine.
        let span = next_sentence_first_word(text, Some(9000)).unwrap();
        assert_eq!(slice(text, span), "Short");
    }

    // ── `-` / `Shift+-` word stepping (line-scoped, wraps both ways) ──
    //
    // `stored` is the previous press's `cycle_index`, i.e. "underlined index
    // + 1"; `None` models the first press on a line.

    #[test]
    fn forward_first_press_on_a_line_starts_at_the_first_word() {
        assert_eq!(next_word_index(5, None, 1), 0);
    }

    #[test]
    fn backward_first_press_on_a_line_starts_at_the_last_word() {
        // Shift+- with no prior cycle: the useful landing is the line's end.
        assert_eq!(next_word_index(5, None, -1), 4);
    }

    #[test]
    fn forward_steps_to_the_next_word() {
        // Underlined word 0 (stored = 1) -> next is 1.
        assert_eq!(next_word_index(5, Some(1), 1), 1);
    }

    #[test]
    fn backward_steps_to_the_previous_word() {
        // Underlined word 3 (stored = 4) -> previous is 2.
        assert_eq!(next_word_index(5, Some(4), -1), 2);
    }

    #[test]
    fn forward_wraps_past_the_last_word_to_the_first() {
        // Underlined the last word (index 4, stored = 5) -> wrap to 0.
        assert_eq!(next_word_index(5, Some(5), 1), 0);
    }

    /// The wrap the user asked for: Shift+- on the line's FIRST word goes to
    /// its LAST word, not off the front.
    #[test]
    fn backward_wraps_from_the_first_word_to_the_last() {
        // Underlined word 0 (stored = 1) -> previous wraps to index 4.
        assert_eq!(next_word_index(5, Some(1), -1), 4);
    }

    #[test]
    fn stepping_the_two_directions_composes() {
        // Forward three times from a fresh line, then back once.
        let mut idx = next_word_index(6, None, 1);
        for _ in 0..2 {
            idx = next_word_index(6, Some(idx + 1), 1);
        }
        assert_eq!(idx, 2);
        assert_eq!(next_word_index(6, Some(idx + 1), -1), 1);
    }

    #[test]
    fn single_word_line_stays_put_in_both_directions() {
        assert_eq!(next_word_index(1, Some(1), 1), 0);
        assert_eq!(next_word_index(1, Some(1), -1), 0);
    }

    #[test]
    fn empty_word_list_does_not_panic() {
        // Guards the `% 0` / `n - 1` underflow paths.
        assert_eq!(next_word_index(0, None, 1), 0);
        assert_eq!(next_word_index(0, None, -1), 0);
        assert_eq!(next_word_index(0, Some(3), -1), 0);
    }

    #[test]
    fn a_stale_oversized_stored_index_is_wrapped_not_panicking() {
        // `cycle_index` can outlive a re-render that shortened the line.
        assert_eq!(next_word_index(3, Some(99), 1), (99 - 1 + 1) % 3);
    }
}
