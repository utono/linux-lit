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

/// Cycle through words on the current line, copying each to the system clipboard.
/// Each press advances to the next word; wraps after the last word.
/// Briefly bolds the word in the buffer for 2 seconds.
pub fn word_cycle_copy(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }

    let words = extract_buffer_line_words(state);
    if words.is_empty() {
        return;
    }

    // Reset index if we moved to a different line
    let idx = if state.word_cycle.cycle_line == Some(state.current_line) {
        state.word_cycle.cycle_index % words.len()
    } else {
        0
    };

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

    log_fmt!("WORD_COPY: copied '{}' (word {}/{})", word, idx + 1, words.len());

    // Update cycle state
    state.word_cycle.cycle_line = Some(state.current_line);
    state.word_cycle.cycle_index = idx + 1;

    // `-` is single-word mode, but the ONE word it underlines must still be a
    // valid diagram selection, so set the collection to exactly it rather than
    // emptying it. `_` keeps appending from here.
    state.word_cycle.collect_words.clear();
    state.word_cycle.collect_words.push(word.clone());
    state.word_cycle.collect_ranges.clear();
    state.word_cycle.collect_ranges.push((char_start, char_end));

    // Remove any previous underline tag, then apply to the current word.
    // Persistent: cleared by Escape, by leaving the line, or by the next -/_.
    apply_word_underline(state, &[(char_start, char_end)], true);
}

/// Collect words on the current line, accumulating across presses.
/// Each W press advances to the next word, appends it to the collection,
/// and copies all collected words (space-separated) to the clipboard.
/// Underlines all collected words. Resets on line change.
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
    // Persistent: cleared by Escape, by leaving the line, or by the next -/_.
    apply_word_underline(state, &ranges, true);
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
/// underline stays until explicitly cleared (Escape, or a `-`/`_` that
/// replaces it). `bold_gen` is still bumped either way, which invalidates any
/// timer already in flight from an earlier non-persistent call.
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
}
