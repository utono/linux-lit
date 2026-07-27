//! The `-` family (`-`, `Shift+-`, `Alt+-`, `Ctrl+-`) inside the gloss and
//! journal Q&A overlays — the overlay counterpart of
//! `crate::input::actions::word_copy`, which serves reader mode.
//!
//! SCOPE IS THE CURSOR BLOCK, not a visual line. The reader's version walks
//! the cursor LINE of `state.buffer`; an overlay has no line cursor — it has a
//! BLOCK cursor (`cursor_block` into `blocks`), and its blocks wrap across
//! several display lines, so a line scope would be arbitrary. This matches the
//! scope the `rr` vocab popup already uses on these surfaces
//! (`gloss_overlay_scope_words` / `journal_overlay_scope_words`).
//!
//! `Return` is deliberately NOT bound on either overlay: in the reader it
//! opens a syntax gloss for the underlined span
//! (`OpenSyntaxDiagramForUnderlined`), which must not fire from inside a
//! gloss.
//!
//! The word-stepping arithmetic itself is NOT duplicated — `next_word_index`
//! and `next_sentence_first_word` live in `word_copy` and are reused here, so
//! the two surfaces wrap identically.

use gtk4::prelude::*;

use crate::log_fmt;

/// Per-surface word-cycle state, mirroring the reader's `WordCycleState` but
/// keyed by BLOCK index instead of line index.
#[derive(Default)]
pub struct OverlayWordCycle {
    /// Block index the current cycle belongs to; a different block resets it.
    pub cycle_block: Option<usize>,
    /// Index of the underlined word + 1 (same encoding as the reader's).
    pub cycle_index: usize,
    /// Words accumulated by `Alt+-`, space-joined on copy.
    pub collect_words: Vec<String>,
}

/// One overlay's text surface: the buffer to tag and the cursor block's
/// inclusive buffer-line span. Both overlays expose `start_line`/`end_line`
/// per block, so this is all the adapter needs.
pub struct BlockTarget {
    pub buffer: gtk4::TextBuffer,
    pub tag: gtk4::TextTag,
    pub start_line: i32,
    pub end_line: i32,
    /// Index of the cursor block, for the reset-on-block-change check.
    pub block_index: usize,
}

/// Which direction / mode a keypress requested.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// `-`: next word, replacing the selection.
    Forward,
    /// `Shift+-`: previous word, replacing the selection.
    Back,
    /// `Alt+-`: next word, APPENDING to the collection.
    Collect,
    /// `Ctrl+-`: first word of the next sentence, replacing the selection.
    NextSentence,
}

/// Run one `-`-family step over `target`, updating `cycle` and the underline.
///
/// Returns the text copied to the clipboard, or `None` when the block has no
/// words (nothing is copied and no tag is applied).
pub fn step(
    cycle: &mut OverlayWordCycle,
    target: &BlockTarget,
    step: Step,
) -> Option<String> {
    let block_text = block_text(target);
    if block_text.trim().is_empty() {
        return None;
    }

    // A different block than last time restarts the cycle.
    let same_block = cycle.cycle_block == Some(target.block_index);
    if !same_block {
        cycle.collect_words.clear();
    }

    let words = words_with_offsets(&block_text);
    if words.is_empty() {
        return None;
    }

    let (idx, ranges) = match step {
        Step::NextSentence => {
            // Anchor on the END of the current underline, matching the
            // reader's `underline_next_sentence`.
            let anchor = same_block
                .then(|| cycle.cycle_index.checked_sub(1))
                .flatten()
                .and_then(|i| words.get(i))
                .map(|&(_, _, end)| end);
            let (start, end) =
                crate::input::actions::word_copy::next_sentence_first_word(
                    &block_text,
                    anchor,
                )?;
            // Report the sentence's first word as the cycle position, so a
            // following `-` continues from it (same handover the reader does).
            let idx = words
                .iter()
                .position(|&(_, s, _)| s == start)
                .unwrap_or(0);
            (idx, vec![(start, end)])
        }
        Step::Collect => {
            // `Alt+-` advances one word and APPENDS. On a fresh block it
            // starts at the first word.
            let idx = if same_block {
                cycle.cycle_index % words.len()
            } else {
                0
            };
            let mut ranges: Vec<(usize, usize)> = Vec::new();
            // Rebuild the collected ranges from the stored count: the words
            // collected so far are the run ending at `idx`.
            let collected = cycle.collect_words.len();
            let first = idx.saturating_sub(collected);
            for w in words.iter().take(idx + 1).skip(first) {
                ranges.push((w.1, w.2));
            }
            (idx, ranges)
        }
        Step::Forward | Step::Back => {
            let delta = if step == Step::Back { -1 } else { 1 };
            let idx = crate::input::actions::word_copy::next_word_index(
                words.len(),
                same_block.then_some(cycle.cycle_index),
                delta,
            );
            (idx, vec![(words[idx].1, words[idx].2)])
        }
    };

    let word = words.get(idx).map(|w| w.0.clone())?;

    // The clipboard payload: the whole collection for Alt+-, else the word.
    let payload = if step == Step::Collect {
        cycle.collect_words.push(word.clone());
        cycle.collect_words.join(" ")
    } else {
        cycle.collect_words.clear();
        cycle.collect_words.push(word.clone());
        word.clone()
    };

    if !copy_to_clipboard(&payload) {
        return None;
    }

    cycle.cycle_block = Some(target.block_index);
    cycle.cycle_index = idx + 1;

    apply_underline(target, &ranges);
    log_fmt!(
        "OVERLAY_WORD_COPY: copied '{}' (word {}/{}, {:?})",
        payload,
        idx + 1,
        words.len(),
        step
    );
    Some(payload)
}

/// Text of the cursor block — the region `words_with_offsets` tokenizes, so
/// char offsets are interchangeable with `apply_underline`'s.
fn block_text(target: &BlockTarget) -> String {
    let Some(start) = target.buffer.iter_at_line(target.start_line) else {
        return String::new();
    };
    let Some(mut end) = target.buffer.iter_at_line(target.end_line) else {
        return String::new();
    };
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    target.buffer.text(&start, &end, false).to_string()
}

/// Tokenize `text` into `(word, char_start, char_end)`, stripping surrounding
/// punctuation — the block-scoped twin of `word_copy`'s line tokenizer.
/// Offsets are char offsets from the START OF THE BLOCK (newlines included),
/// which is what `apply_underline` walks from.
fn words_with_offsets(text: &str) -> Vec<(String, usize, usize)> {
    let mut words = Vec::new();
    for token in text.split_whitespace() {
        let token_byte_start = token.as_ptr() as usize - text.as_ptr() as usize;
        let token_char_start = text[..token_byte_start].chars().count();
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

/// Underline `ranges` (char offsets from the block start), clearing any
/// previous underline first. Persistent — no auto-remove timer, matching the
/// reader's `persist: true` behavior.
fn apply_underline(target: &BlockTarget, ranges: &[(usize, usize)]) {
    let buf = &target.buffer;
    let (bs, be) = (buf.start_iter(), buf.end_iter());
    buf.remove_tag(&target.tag, &bs, &be);

    let Some(block_start) = buf.iter_at_line(target.start_line) else {
        return;
    };
    for &(char_start, char_end) in ranges {
        let mut ws = block_start;
        ws.forward_chars(char_start as i32);
        let mut we = ws;
        we.forward_chars((char_end - char_start) as i32);
        buf.apply_tag(&target.tag, &ws, &we);
    }
}

/// Copy `text` via wl-copy (Wayland-only, matching the reader's path).
fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    match Command::new("wl-copy").stdin(Stdio::piped()).spawn() {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            true
        }
        Err(e) => {
            log_fmt!("OVERLAY_WORD_COPY: wl-copy failed: {}", e);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_across_newlines_with_block_char_offsets() {
        // A block spans several buffer lines; offsets must be measured from
        // the block start WITH the newlines counted, or the underline lands
        // on the wrong words.
        let text = "First line here\nSecond line there";
        let words = words_with_offsets(text);
        let names: Vec<&str> = words.iter().map(|w| w.0.as_str()).collect();
        assert_eq!(names, vec!["First", "line", "here", "Second", "line", "there"]);
        // "Second" starts right after the newline at char 16.
        let second = words.iter().find(|w| w.0 == "Second").unwrap();
        assert_eq!(second.1, 16);
        assert_eq!(text.chars().skip(second.1).take(second.2 - second.1).collect::<String>(), "Second");
    }

    #[test]
    fn strips_surrounding_punctuation_but_keeps_apostrophes() {
        let words = words_with_offsets("\"Don't,\" he said.");
        let names: Vec<&str> = words.iter().map(|w| w.0.as_str()).collect();
        assert_eq!(names, vec!["Don't", "he", "said"]);
    }

    #[test]
    fn all_blank_block_yields_no_words() {
        assert!(words_with_offsets("   \n\n  ").is_empty());
    }

    #[test]
    fn offsets_are_chars_not_bytes_for_multibyte_text() {
        let text = "Æsop wrote\nNaïve café";
        let words = words_with_offsets(text);
        let naive = words.iter().find(|w| w.0 == "Naïve").unwrap();
        let sliced: String = text.chars().skip(naive.1).take(naive.2 - naive.1).collect();
        assert_eq!(sliced, "Naïve");
    }
}
