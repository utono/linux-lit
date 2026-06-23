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

    // Clear multi-word collect state (w is single-word mode)
    state.word_cycle.collect_words.clear();
    state.word_cycle.collect_ranges.clear();

    // Remove any previous underline tag, then apply to the current word
    apply_word_underline(state, &[(char_start, char_end)]);
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
    apply_word_underline(state, &ranges);
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
/// removing any previous underline first. Auto-removes after 2 seconds.
fn apply_word_underline(state: &mut AppState, ranges: &[(usize, usize)]) {
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

    // Auto-remove underline after 2 seconds
    let gen = state.word_cycle.bold_gen.get() + 1;
    state.word_cycle.bold_gen.set(gen);
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
