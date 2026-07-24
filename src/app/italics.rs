//! Inline `_word_` italic parsing (Phase B). Pure, buffer-agnostic.

pub struct ItalicParse {
    pub stripped_text: String,
    pub spans: Vec<(usize, usize)>,
    pub removed_positions: Vec<usize>,
}

/// Parse paired `_..._` runs in a line. `None` when there is no `_` or an ODD
/// number of `_` (unpaired — the caller renders the line verbatim and logs it,
/// so a stray `_` never italicizes to end-of-line). Offsets are UNICODE CHAR
/// indices. Non-greedy left-to-right pairing: `_` opens, next `_` closes.
pub fn parse_italic_spans(line: &str) -> Option<ItalicParse> {
    // char-index positions of every `_`
    let underscores: Vec<usize> = line
        .chars()
        .enumerate()
        .filter(|(_, c)| *c == '_')
        .map(|(i, _)| i)
        .collect();
    if underscores.is_empty() || underscores.len() % 2 != 0 {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    let mut stripped: Vec<char> = Vec::with_capacity(chars.len() - underscores.len());
    let mut spans = Vec::new();
    let removed_positions = underscores.clone(); // already sorted ascending

    // Walk source chars; drop `_` at paired positions; record span bounds in the
    // STRIPPED coordinate space. Pairs are (underscores[2k], underscores[2k+1]).
    let mut pair_iter = underscores.chunks_exact(2);
    let mut next_pair = pair_iter.next();
    let mut span_open_display: Option<usize> = None;
    for (src_i, &c) in chars.iter().enumerate() {
        if let Some(&[open, close]) = next_pair {
            if src_i == open {
                // opening delimiter: drop it; the span begins at the current
                // stripped length.
                span_open_display = Some(stripped.len());
                continue;
            }
            if src_i == close {
                // closing delimiter: drop it; close the span.
                if let Some(start) = span_open_display.take() {
                    spans.push((start, stripped.len()));
                }
                next_pair = pair_iter.next();
                continue;
            }
        }
        stripped.push(c);
    }
    Some(ItalicParse {
        stripped_text: stripped.into_iter().collect(),
        spans,
        removed_positions,
    })
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

    #[test]
    fn odd_count_is_none_verbatim() {
        assert!(p("a stray _ underscore").is_none()); // 1 `_`
        // NOTE: brief's original string here ("_open but _no close_ here _")
        // has 4 underscores (positions 0,10,19,26) — an EVEN count, so per
        // the pairing rule it legitimately parses to Some(...) with pairs
        // (0,10) and (19,26); it is NOT odd/unpaired despite the test's
        // name. Corrected here by adding one more trailing `_` so the count
        // is genuinely ODD (5), which is what this test is meant to exercise.
        assert!(p("_open but _no close_ here _ _").is_none()); // 5 `_` -> odd -> None
    }

    #[test]
    fn multibyte_before_span_offsets_are_char_not_byte() {
        // "café _x_" — é is 2 bytes but 1 char; span must be char-indexed
        let r = p("café _x_").unwrap();
        assert_eq!(r.stripped_text, "café x");
        assert_eq!(r.spans, vec![(5, 6)]); // "café " = 5 chars; x at 5..6
    }
}
