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
    let last = it.last().unwrap_or(first);
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
