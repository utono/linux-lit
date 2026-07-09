//! Vocab-sentence loop mode: Ctrl+r drill mode that jumps between sentences
//! containing vocab words, loops each one gaplessly via MPV ab-loop, and
//! karaoke-highlights it (sentence tint + phrase sweep) until n/p/Escape.
//!
//! Pure helpers here are unit-tested; the impure enter/activate/advance/exit
//! functions (added in a later task) drive AppState, MPV, and the tags.
//! Design: docs/plans/2026-07-09-vocab-sentence-loop-design.md

use crate::app::VocabMatch;
use crate::db::queries::PhraseSpan;
use crate::input::phrase_highlight::sentence_bounds;

/// One sentence containing >=1 vocab word, with its resolved audio window.
/// Char offsets are unicode chars within `buffer_line`'s text (the same space
/// as VocabMatch and PhraseSpan offsets).
#[derive(Clone, Debug)]
pub struct VocabSentence {
    pub buffer_line: usize,
    pub sent_start_char: usize,
    pub sent_end_char: usize,
    pub start_time: f64,
    pub end_time: f64,
    pub words: Vec<String>,
}

/// Mode state held in AppState while the loop is active.
pub struct VocabLoopState {
    pub sentences: Vec<VocabSentence>,
    pub idx: usize,
}

/// Group vocab matches into sentence candidates: `(buffer_line, (sent_start,
/// sent_end), words)`, in buffer order (vocab_matches is built in buffer
/// order). Matches whose sentence bounds coincide merge into one entry; a
/// word repeated within one sentence is listed once. Lines whose text is
/// empty (out of range) are skipped.
pub fn group_matches_into_sentences(
    matches: &[VocabMatch],
    line_text_of: &dyn Fn(usize) -> String,
) -> Vec<(usize, (usize, usize), Vec<String>)> {
    let mut out: Vec<(usize, (usize, usize), Vec<String>)> = Vec::new();
    for m in matches {
        let text = line_text_of(m.line_index);
        if text.is_empty() {
            continue;
        }
        let (sc, ec) = sentence_bounds(&text, m.char_start, m.char_end);
        match out
            .iter_mut()
            .find(|(bl, (s, _), _)| *bl == m.line_index && *s == sc)
        {
            Some((_, _, words)) => {
                if !words.contains(&m.word) {
                    words.push(m.word.clone());
                }
            }
            None => out.push((m.line_index, (sc, ec), vec![m.word.clone()])),
        }
    }
    out
}

/// Audio window of the sentence `[sc, ec)`: start of the FIRST span
/// intersecting it through end of the LAST. None when no span intersects
/// (sentence has no phrase data — caller drops it).
pub fn sentence_time_range(spans: &[PhraseSpan], sc: usize, ec: usize) -> Option<(f64, f64)> {
    let mut it = spans.iter().filter(|sp| sp.start_char < ec && sp.end_char > sc);
    let first = it.next()?;
    let last = it.next_back().unwrap_or(first);
    Some((first.start_time, last.end_time))
}

/// Entry index: forward = first sentence at/after the cursor line (wraps to
/// 0); backward = last sentence strictly before it (wraps to the end).
pub fn start_index(sentences: &[VocabSentence], current_line: usize, forward: bool) -> usize {
    if forward {
        sentences
            .iter()
            .position(|s| s.buffer_line >= current_line)
            .unwrap_or(0)
    } else {
        sentences
            .iter()
            .rposition(|s| s.buffer_line < current_line)
            .unwrap_or(sentences.len().saturating_sub(1))
    }
}

/// Wrapping n/p step.
pub fn step_index(idx: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if forward {
        (idx + 1) % len
    } else {
        (idx + len - 1) % len
    }
}

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::{AppState, InputMode};
use crate::input::phrase_highlight::{apply_char_range_tag, buffer_line_text};
use crate::mpv::MpvCommand;

/// Build the work's vocab-sentence list for the active media: group matches
/// into sentences, resolve each sentence's audio window from its line's
/// phrase spans, drop sentences without phrase data. Spans are fetched once
/// per distinct line (one prose paragraph often holds many vocab sentences).
fn build_vocab_sentences(s: &AppState) -> Vec<VocabSentence> {
    let Some(media) = s.media_id else {
        return Vec::new();
    };
    let Ok(conn) = crate::db::queries::open_db() else {
        return Vec::new();
    };
    let grouped = group_matches_into_sentences(&s.vocab_matches, &|bl| buffer_line_text(s, bl));
    let mut spans_cache: std::collections::HashMap<i64, Vec<PhraseSpan>> =
        std::collections::HashMap::new();
    let mut out = Vec::new();
    for (bl, (sc, ec), words) in grouped {
        let Some(wi) = s.work_line_for_buffer(bl) else {
            continue;
        };
        let Some(line_id) = s
            .current_work
            .as_ref()
            .and_then(|w| w.lines.get(wi))
            .map(|l| l.id)
        else {
            continue;
        };
        let spans = spans_cache
            .entry(line_id)
            .or_insert_with(|| crate::db::queries::phrase_spans_for_line(&conn, line_id, media));
        let Some((start_time, end_time)) = sentence_time_range(spans, sc, ec) else {
            continue;
        };
        out.push(VocabSentence {
            buffer_line: bl,
            sent_start_char: sc,
            sent_end_char: ec,
            start_time,
            end_time,
            words,
        });
    }
    out
}

/// Enter the loop mode at the first vocab sentence at/after (forward) or
/// before (backward) the cursor. Returns false when the mode cannot start —
/// the caller falls back to the plain vocab jump. Requires connected MPV,
/// an active media id, sync on, and translations hidden (inflated buffer
/// misaligns char offsets, same gate as the phrase sweep).
pub fn enter_vocab_loop(state: &Rc<RefCell<AppState>>, forward: bool) -> bool {
    let mut s = state.borrow_mut();
    if !s.mpv_connected || !s.sync_enabled || s.translations_visible || s.media_id.is_none() {
        return false;
    }
    // Most of the library has no phrase_timestamps at all for the active
    // media. Gate on that cheaply before grouping matches into sentences and
    // querying per-line spans, and fall back to the plain jump silently —
    // no misleading "no vocab sentences" toast on a work that simply has no
    // phrase data to drill with.
    let Some(media) = s.media_id else { return false };
    let Ok(conn) = crate::db::queries::open_db() else {
        return false;
    };
    if !crate::db::queries::media_has_phrase_data(&conn, media) {
        return false;
    }
    let sentences = build_vocab_sentences(&s);
    if sentences.is_empty() {
        if !s.vocab_matches.is_empty() {
            crate::input::navigation::show_chapter_toast(&s, "no vocab sentences with audio");
        }
        return false;
    }
    let idx = start_index(&sentences, s.current_line, forward);
    s.vocab_loop = Some(VocabLoopState { sentences, idx });
    s.input_mode = InputMode::VocabLoop;
    crate::logging::log("VOCAB_LOOP: enter");
    activate_current(&mut s);
    true
}

/// n/p inside the mode: step the index (wrapping) and re-activate.
pub fn advance(state: &Rc<RefCell<AppState>>, forward: bool) {
    let mut s = state.borrow_mut();
    remove_sentence_tag(&s);
    {
        let Some(vl) = s.vocab_loop.as_mut() else {
            return;
        };
        vl.idx = step_index(vl.idx, vl.sentences.len(), forward);
    }
    activate_current(&mut s);
}

/// Land on, tint, and start looping the current sentence. One funnel for
/// entry and n/p so the ab-loop, tint, toast, and sync suppression can never
/// drift apart.
fn activate_current(s: &mut AppState) {
    let (sentence, idx, len) = {
        let Some(vl) = s.vocab_loop.as_ref() else {
            return;
        };
        (vl.sentences[vl.idx].clone(), vl.idx, vl.sentences.len())
    };
    crate::input::navigation::land_cursor_on_line(s, sentence.buffer_line);
    // A loop never coexists with a scheduled sync page turn or line advance.
    s.pending_prose_cross = None;
    s.pending_advance = None;
    // Gapless native loop; ResumeAndSeek unpauses and jumps to the start.
    let _ = s.cmd_tx.try_send(MpvCommand::SetAbLoop {
        a: sentence.start_time,
        b: sentence.end_time,
    });
    let _ = s.cmd_tx.try_send(MpvCommand::ResumeAndSeek(sentence.start_time));
    s.suppress_sync_until = Some(
        std::time::Instant::now() + crate::input::navigation::SYNC_SUPPRESS_SEEK,
    );
    // Paint the first phrase immediately (same pattern as do_mpv_seek) so the
    // sweep shows before live TimePos ticks arrive.
    if crate::input::phrase_highlight::paint_pending_phrase(s, sentence.start_time) {
        s.phrase_paint_hold = s.suppress_sync_until;
    }
    apply_char_range_tag(
        s,
        &s.vocab_sentence_tag.clone(),
        sentence.buffer_line,
        sentence.sent_start_char,
        sentence.sent_end_char,
    );
    crate::input::navigation::show_chapter_toast(
        s,
        &format!("vocab {}/{} — {}", idx + 1, len, sentence.words.join(", ")),
    );
    crate::logging::log(&format!(
        "VOCAB_LOOP: {}/{} line={} chars=[{},{}) t=[{:.2},{:.2}] words={:?}",
        idx + 1,
        len,
        sentence.buffer_line,
        sentence.sent_start_char,
        sentence.sent_end_char,
        sentence.start_time,
        sentence.end_time,
        sentence.words
    ));
}

/// Remove the sentence-extent tint everywhere.
fn remove_sentence_tag(s: &AppState) {
    let (bs, be) = s.buffer.bounds();
    s.buffer.remove_tag(&s.vocab_sentence_tag, &bs, &be);
}

/// The ONE exit funnel: Escape/Ctrl+r in-mode, and defensively on work
/// switch. Clears the MPV ab-loop (a leaked loop would trap normal
/// playback), drops the state and tint, and returns to Reader. Playback
/// continues from wherever it is; normal sync resumes on the next TimePos.
/// No handling is needed for MPV quit/disconnect — the ab-loop lives in the
/// MPV process and dies with it.
pub fn exit_vocab_loop(s: &mut AppState) {
    if s.vocab_loop.take().is_none() {
        return;
    }
    let _ = s.cmd_tx.try_send(MpvCommand::ClearAbLoop);
    remove_sentence_tag(s);
    // The mode forced the sweep on; if the configured mode is Off or
    // playback is paused there is no next tick to clear it, and when
    // playing the next tick repaints per the configured mode.
    crate::input::phrase_highlight::clear_phrase_highlight(s);
    if s.input_mode == InputMode::VocabLoop {
        s.input_mode = InputMode::Reader;
    }
    crate::logging::log("VOCAB_LOOP: exit");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(line: usize, cs: usize, ce: usize, w: &str) -> VocabMatch {
        VocabMatch {
            word: w.to_string(),
            line_index: line,
            char_start: cs,
            char_end: ce,
        }
    }

    fn sp(st: f64, et: f64, sc: usize, ec: usize) -> PhraseSpan {
        PhraseSpan { start_time: st, end_time: et, start_char: sc, end_char: ec }
    }

    fn vs(bl: usize) -> VocabSentence {
        VocabSentence {
            buffer_line: bl,
            sent_start_char: 0,
            sent_end_char: 1,
            start_time: 0.0,
            end_time: 1.0,
            words: vec![],
        }
    }

    #[test]
    fn grouping_merges_same_sentence_and_splits_sentences() {
        // "One two. Three four." — sentence 1 = chars [0,8), sentence 2 = [9,20).
        let text = "One two. Three four.";
        let matches = vec![m(0, 4, 7, "two"), m(0, 9, 14, "three"), m(0, 15, 19, "four")];
        let out = group_matches_into_sentences(&matches, &|_| text.to_string());
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], (0, (0, 8), vec!["two".to_string()]));
        assert_eq!(
            out[1],
            (0, (9, 20), vec!["three".to_string(), "four".to_string()])
        );
    }

    #[test]
    fn grouping_dedupes_repeated_word_and_skips_empty_lines() {
        let text = "Fog here, fog there.";
        let matches = vec![m(3, 0, 3, "fog"), m(3, 10, 13, "fog"), m(99, 0, 3, "fog")];
        let out = group_matches_into_sentences(&matches, &|bl| {
            if bl == 3 { text.to_string() } else { String::new() }
        });
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], (3, (0, 20), vec!["fog".to_string()]));
    }

    #[test]
    fn time_range_spans_first_to_last_intersecting() {
        let spans = vec![
            sp(10.0, 11.0, 0, 8),
            sp(11.0, 12.5, 9, 14),
            sp(12.5, 14.0, 15, 20),
            sp(14.0, 15.0, 21, 30),
        ];
        assert_eq!(sentence_time_range(&spans, 9, 20), Some((11.0, 14.0)));
        assert_eq!(sentence_time_range(&spans, 0, 8), Some((10.0, 11.0)));
        assert_eq!(sentence_time_range(&spans, 40, 50), None);
        assert_eq!(sentence_time_range(&[], 0, 5), None);
    }

    #[test]
    fn start_index_forward_backward_and_wrap() {
        let ss = vec![vs(5), vs(10), vs(20)];
        assert_eq!(start_index(&ss, 0, true), 0);
        assert_eq!(start_index(&ss, 10, true), 1); // at/after cursor
        assert_eq!(start_index(&ss, 21, true), 0); // wrap to first
        assert_eq!(start_index(&ss, 21, false), 2); // last before cursor
        assert_eq!(start_index(&ss, 10, false), 0); // strictly before
        assert_eq!(start_index(&ss, 5, false), 2); // none before -> wrap to last
    }

    #[test]
    fn step_index_wraps_both_directions() {
        assert_eq!(step_index(1, 3, true), 2);
        assert_eq!(step_index(2, 3, true), 0);
        assert_eq!(step_index(0, 3, false), 2);
        assert_eq!(step_index(0, 0, true), 0);
    }
}
