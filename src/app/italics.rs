//! Inline `_word_` italic parsing (Phase B). Pure, buffer-agnostic.

pub struct ItalicParse {
    /// The line with paired `_` removed. Load-bearing: `strip_italics_for_fill`
    /// reads this directly to build the buffer-fill text (the fill-time
    /// stripped text), and the unit tests assert on it as a cross-check that
    /// the parser strips correctly.
    pub stripped_text: String,
    pub spans: Vec<(usize, usize)>,
    pub removed_positions: Vec<usize>,
}

/// Translate a SOURCE char offset to the DISPLAY (stripped) offset by
/// subtracting the number of removed `_` at-or-before it. Identity when
/// `removed` is empty (non-italic line → zero cost, no shift).
/// `removed` must be sorted ascending (as `ItalicParse.removed_positions` is).
pub fn translate_offset(removed: &[usize], source_offset: usize) -> usize {
    // `removed` is sorted ascending; count entries <= source_offset.
    let n = removed.partition_point(|&p| p <= source_offset);
    source_offset - n
}

/// Give an odd-count line's orphan `_` the sibling it lacks, returning the
/// full list of pair boundaries (always even-length).
///
/// Which `_` is the orphan: pairing is greedy left-to-right, so with an odd
/// count the LAST `_` is the one left over. Its missing sibling is placed from
/// **local context**, never at a line edge far away — LoJ line 1.1316 is 1,535
/// chars with 21 underscores, where a "close at end-of-line" rule would
/// italicize 1,251 characters (its real defect is Gutenberg's stray opener in
/// `_The _Tatler Revived_`).
///
/// The rule, derived from the real corpus shapes (docs/loj/history.md §4):
///
/// - Orphan **attached to the text on its right** (`_word…`) — it opens a span
///   whose closer was lost. Close at the end of that word run.
/// - Orphan **attached to the text on its left** (`…word_`) — it closes a span
///   whose opener was lost. Open at the start of that word run.
/// - Orphan **touching text on neither side** (a lone `_` between spaces) —
///   there is no span to recover. Pair it with itself (an empty span), so the
///   character is still deleted from the display and nothing is italicized.
///
/// "Word run" = the maximal stretch of non-space, non-`_` characters adjacent
/// to the orphan. That keeps the repair local: it can never span a space, so
/// it cannot swallow the rest of a line.
fn repair_orphan(chars: &[char], underscores: &[usize]) -> Vec<usize> {
    let orphan = *underscores.last().expect("odd count implies non-empty");
    let paired = &underscores[..underscores.len() - 1];

    let is_word = |i: usize| -> bool {
        chars.get(i).is_some_and(|c| !c.is_whitespace() && *c != '_')
    };

    let attached_right = is_word(orphan + 1);
    let attached_left = orphan > 0 && is_word(orphan - 1);

    let (open, close) = if attached_right && !attached_left {
        // `_word` — opener; close after the word run to its right.
        let mut end = orphan + 1;
        while is_word(end) {
            end += 1;
        }
        (orphan, end)
    } else if attached_left && !attached_right {
        // `word_` — closer; open at the start of the word run to its left.
        let mut start = orphan;
        while start > 0 && is_word(start - 1) {
            start -= 1;
        }
        (start, orphan)
    } else if attached_left && attached_right {
        // Word-internal (`Reviews_ are` splits as left-attached; this arm is
        // `a_b`). Treat as a closer of the left run — the shape Gutenberg's
        // `2_d_.` currency italics take when one delimiter is lost.
        let mut start = orphan;
        while start > 0 && is_word(start - 1) {
            start -= 1;
        }
        (start, orphan)
    } else {
        // Isolated `_`: empty span at the orphan. Deletes the char, italicizes
        // nothing.
        (orphan, orphan)
    };

    let mut bounds = paired.to_vec();
    bounds.push(open);
    bounds.push(close);
    bounds.sort_unstable();
    bounds
}

/// Parse paired `_..._` runs in a line. `None` only when the line contains no
/// `_` at all. Offsets are UNICODE CHAR indices. Non-greedy left-to-right
/// pairing: `_` opens, next `_` closes.
///
/// ODD (unpaired) counts are REPAIRED rather than rejected. Every `_` is
/// removed from the displayed text, so a stray delimiter never reaches the
/// screen. The leftover orphan is given the sibling it lacks, inferred from
/// its LOCAL context (see `repair_orphan`) — never by extending a span to the
/// end of the line. The caller still logs the repair (`ITALIC_UNPAIRED`).
pub fn parse_italic_spans(line: &str) -> Option<ItalicParse> {
    // char-index positions of every `_`
    let underscores: Vec<usize> = line
        .chars()
        .enumerate()
        .filter(|(_, c)| *c == '_')
        .map(|(i, _)| i)
        .collect();
    if underscores.is_empty() {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    // Repair an odd count by synthesising the orphan's missing sibling. Returns
    // the pair boundaries to use; `removed_positions` stays the REAL `_`
    // positions (only real chars are deleted from the display text).
    let pair_bounds = if underscores.len() % 2 == 0 {
        underscores.clone()
    } else {
        repair_orphan(&chars, &underscores)
    };
    let mut stripped: Vec<char> = Vec::with_capacity(chars.len() - underscores.len());
    let mut spans = Vec::new();
    let removed_positions = underscores.clone(); // already sorted ascending

    // Walk source chars; drop `_` at paired positions; record span bounds in the
    // STRIPPED coordinate space. Pairs are (pair_bounds[2k], pair_bounds[2k+1]).
    // A synthetic boundary from `repair_orphan` is a position where no `_`
    // actually sits, so the walk must open/close a span there WITHOUT deleting
    // a character — handled by the `is_real` checks below.
    let mut pair_iter = pair_bounds.chunks_exact(2);
    let mut next_pair = pair_iter.next();
    let mut span_open_display: Option<usize> = None;
    for (src_i, &c) in chars.iter().enumerate() {
        if let Some(&[open, close]) = next_pair {
            if src_i == open && open == close {
                // Degenerate repair (isolated orphan): open and close coincide,
                // so emit an EMPTY span here and consume the pair in one step.
                // Without this the `else if` below never runs and the span
                // would stay open to end-of-line.
                spans.push((stripped.len(), stripped.len()));
                next_pair = pair_iter.next();
                if c == '_' {
                    continue;
                }
            } else if src_i == open {
                // Opening boundary: the span begins at the current stripped
                // length. Drop the char only if a real `_` sits here (a
                // synthetic opener must not eat the word it precedes).
                span_open_display = Some(stripped.len());
                if c == '_' {
                    continue;
                }
            } else if src_i == close {
                // Closing boundary: close the span. Same real-vs-synthetic
                // rule — a synthetic closer keeps the char it precedes.
                if let Some(start) = span_open_display.take() {
                    spans.push((start, stripped.len()));
                }
                next_pair = pair_iter.next();
                if c == '_' {
                    continue;
                }
            }
        }
        stripped.push(c);
    }
    // A synthetic closer at end-of-line lands past the last char, so the loop
    // never reaches it; close any span still open at the buffer's end.
    if let Some(start) = span_open_display.take() {
        spans.push((start, stripped.len()));
    }
    Some(ItalicParse {
        stripped_text: stripped.into_iter().collect(),
        spans,
        removed_positions,
    })
}

pub struct ItalicStripResult {
    pub stripped_lines: Vec<String>,
    pub line_spans: std::collections::HashMap<usize, Vec<(usize, usize)>>,
    pub line_removed: std::collections::HashMap<usize, Vec<usize>>,
}

/// Strip paired `_` from each line for buffer-fill. Output index = buffer line
/// index. `parse_italic_spans` returns None only for a line with no `_` at
/// all; an odd (unpaired) count is REPAIRED, and still logged so the upstream
/// data defect stays visible.
pub fn strip_italics_for_fill(lines: &[String]) -> ItalicStripResult {
    let mut stripped_lines = Vec::with_capacity(lines.len());
    let mut line_spans = std::collections::HashMap::new();
    let mut line_removed = std::collections::HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        // Fast path: no `_` -> verbatim, no entry.
        if !line.contains('_') {
            stripped_lines.push(line.clone());
            continue;
        }
        // Log BEFORE parsing: an odd count means this line carried an orphan
        // that the parser repaired. Keeping the log (rather than dropping it
        // with the old verbatim path) is what keeps the upstream Gutenberg /
        // cross-row-split defects auditable — see docs/loj/history.md §4.
        if line.chars().filter(|c| *c == '_').count() % 2 != 0 {
            crate::log_fmt!(
                "ITALIC_UNPAIRED: line {} odd `_` count, orphan repaired: {:?}",
                i,
                line.chars().take(60).collect::<String>()
            );
        }
        match parse_italic_spans(line) {
            Some(parse) => {
                stripped_lines.push(parse.stripped_text);
                line_spans.insert(i, parse.spans);
                line_removed.insert(i, parse.removed_positions);
            }
            None => {
                // Unreachable for a line containing `_` (None means no `_`),
                // but keep the arm total rather than panicking.
                stripped_lines.push(line.clone());
            }
        }
    }
    ItalicStripResult { stripped_lines, line_spans, line_removed }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Option<ItalicParse> {
        parse_italic_spans(s)
    }

    #[test]
    fn no_underscore_is_none() {
        assert!(p("plain roman text").is_none());
    }

    #[test]
    fn single_pair_strips_and_spans() {
        let r = p("he wrote _London_ later").unwrap();
        assert_eq!(r.stripped_text, "he wrote London later");
        // "he wrote " = 9 chars; "London" = chars 9..15
        assert_eq!(r.spans, vec![(9, 15)]);
        // `_` removed at source offsets 9 and 16 ("he wrote _" -> _ at 9; close after London at 16)
        assert_eq!(r.removed_positions, vec![9, 16]);
    }

    #[test]
    fn two_adjacent_pairs_are_two_spans_not_one_run() {
        // _A_, _B_  -> italic A and italic B, comma+space roman between
        let r = p("_A_, _B_").unwrap();
        assert_eq!(r.stripped_text, "A, B");
        assert_eq!(r.spans, vec![(0, 1), (3, 4)]);
        assert_eq!(r.removed_positions, vec![0, 2, 5, 7]);
    }

    #[test]
    fn word_internal_weld_italicizes_inner() {
        // John_son_ -> "Johnson" with "son" italic (pairing the two `_`)
        let r = p("John_son_").unwrap();
        assert_eq!(r.stripped_text, "Johnson");
        assert_eq!(r.spans, vec![(4, 7)]); // "son" at chars 4..7 of "Johnson"
        assert_eq!(r.removed_positions, vec![4, 8]);
    }

    #[test]
    fn currency_measure_italic_letter() {
        // 120_l_.  -> "120l." with "l" italic
        let r = p("120_l_.").unwrap();
        assert_eq!(r.stripped_text, "120l.");
        assert_eq!(r.spans, vec![(3, 4)]);
        assert_eq!(r.removed_positions, vec![3, 5]);
    }

    // ---- orphan repair (odd `_` count) --------------------------------
    //
    // Historically an odd count returned None and the line rendered verbatim,
    // showing a literal `_`. It now REPAIRS: the orphan is given the sibling
    // it is missing, inferred from its local context. Every case below is
    // taken from real LoJ data (see docs/loj/history.md §4).

    #[test]
    fn orphan_opener_italicizes_its_word_run_only() {
        // LoJ 1.805 shape: `_` opens and its closer was lost to the row split.
        // The repair is deliberately LOCAL — it italicizes the adjacent word
        // run ("The"), NOT the rest of the line. Under-italicizing a
        // cross-row span is the accepted cost of never over-italicizing; the
        // alternative (close at EOL) is what makes LoJ 1.1316 catastrophic.
        let r = p("authour of _The Tears of").unwrap();
        assert_eq!(r.stripped_text, "authour of The Tears of");
        assert_eq!(r.spans, vec![(11, 14)]); // "The"
    }

    #[test]
    fn orphan_closer_opens_at_start_of_line() {
        // LoJ 1.1275 shape: `Poets_.` — the opener was on the PREVIOUS row.
        // Open at start of line so the leading text is italic.
        let r = p("Poets_.").unwrap();
        assert_eq!(r.stripped_text, "Poets.");
        assert_eq!(r.spans, vec![(0, 5)]);
    }

    #[test]
    fn orphan_repair_is_local_not_whole_line() {
        // LoJ 1.1316 shape (the case that rules out a naive whole-line rule):
        // a line whose OTHER spans pair correctly, plus one stray opener from
        // Gutenberg's own corruption (`_The _Tatler Revived_`). The repair
        // must not swallow the rest of the line.
        let r = p("the title of _The _Tatler Revived_ was").unwrap();
        // Greedy pairing takes (13,18) = "The "; the stray is repaired
        // locally, never extending to end-of-line.
        assert_eq!(r.stripped_text, "the title of The Tatler Revived was");
        assert!(
            r.spans.iter().all(|&(_, end)| end <= 31),
            "no span may run to end-of-line: {:?}",
            r.spans
        );
    }

    #[test]
    fn single_stray_underscore_mid_sentence_is_dropped_not_italicized() {
        // LoJ 1.242 shape: `Reviews_ are of the following Books:` — Gutenberg
        // corruption with no real italic intent. Repairing must never
        // italicize the whole tail; here the orphan closes a leading span.
        let r = p("his Reviews_ are of").unwrap();
        assert_eq!(r.stripped_text, "his Reviews are of");
        // The `_` is gone from the DISPLAYED text — that is the contract.
        assert!(!r.stripped_text.contains('_'));
    }

    #[test]
    fn lone_underscore_with_no_word_context_still_strips() {
        // Degenerate: a single `_` surrounded by spaces. No sensible span,
        // but the display must not show a literal `_`.
        let r = p("a stray _ underscore").unwrap();
        assert!(!r.stripped_text.contains('_'));
    }

    #[test]
    fn no_underscore_still_none() {
        // Unchanged: a line with zero `_` is not an italic line at all.
        assert!(p("no underscores here").is_none());
    }

    #[test]
    fn multibyte_before_span_offsets_are_char_not_byte() {
        // "café _x_" — é is 2 bytes but 1 char; span must be char-indexed
        let r = p("café _x_").unwrap();
        assert_eq!(r.stripped_text, "café x");
        assert_eq!(r.spans, vec![(5, 6)]); // "café " = 5 chars; x at 5..6
    }

    #[test]
    fn translate_identity_when_empty() {
        assert_eq!(translate_offset(&[], 0), 0);
        assert_eq!(translate_offset(&[], 42), 42);
    }

    #[test]
    fn translate_subtracts_removed_before_offset() {
        // removed `_` at source 9 and 16 (the _London_ case)
        let removed = vec![9usize, 16];
        assert_eq!(translate_offset(&removed, 5), 5);    // before both -> unchanged
        assert_eq!(translate_offset(&removed, 10), 9);   // 1 removed (<=10) -> -1
        assert_eq!(translate_offset(&removed, 20), 18);  // 2 removed (<=20) -> -2
    }

    #[test]
    fn translate_offset_exactly_at_removed_position_counts_it() {
        // a `_` exactly AT the offset is at-or-before -> counted
        assert_eq!(translate_offset(&[9, 16], 9), 8);   // one removed <= 9
        assert_eq!(translate_offset(&[9, 16], 16), 14); // two removed <= 16
    }

    #[test]
    fn strip_for_fill_mixed_lines() {
        let lines = vec![
            "plain roman".to_string(),          // 0: no `_`
            "he wrote _London_ later".to_string(), // 1: one span
            "(_Page_ 115, _note_ 4.)".to_string(), // 2: two spans
            "a stray _ underscore".to_string(), // 3: odd -> REPAIRED (was verbatim)
        ];
        let r = strip_italics_for_fill(&lines);
        // stripped text: `_` removed on 1 & 2, and now on 3 as well — an odd
        // count is repaired rather than left verbatim, so no literal `_` ever
        // reaches the display. Line 3's orphan is isolated (spaces both
        // sides), so it strips to an empty span and italicizes nothing.
        assert_eq!(r.stripped_lines, vec![
            "plain roman",
            "he wrote London later",
            "(Page 115, note 4.)",
            "a stray  underscore",             // `_` gone; note the double space
        ]);
        // spans: 1 & 2 as before; 3 has an entry but no visible span.
        assert_eq!(r.line_spans.get(&0), None);
        assert_eq!(r.line_spans.get(&1), Some(&vec![(9, 15)]));   // "London"
        assert_eq!(r.line_spans.get(&2), Some(&vec![(1, 5), (11, 15)])); // "Page","note"
        assert_eq!(r.line_spans.get(&3), Some(&vec![(8, 8)]));    // empty span
        // removed: source `_` positions — line 3 now records its orphan, so
        // karaoke offset translation stays correct on repaired lines too.
        assert_eq!(r.line_removed.get(&1), Some(&vec![9, 16]));
        assert_eq!(r.line_removed.get(&2), Some(&vec![1, 6, 13, 18]));
        assert_eq!(r.line_removed.get(&3), Some(&vec![8]));
    }

    #[test]
    fn strip_for_fill_preserves_line_count_and_indices() {
        let lines: Vec<String> = (0..5).map(|i| format!("_x{i}_ tail")).collect();
        let r = strip_italics_for_fill(&lines);
        assert_eq!(r.stripped_lines.len(), 5);          // 1:1 with input
        for i in 0..5 { assert!(r.line_spans.contains_key(&i)); } // each has a span
    }
}
