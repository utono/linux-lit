//! Word-level diff between a previous and new rewrite version. Pure, GTK-free:
//! returns CHARACTER-offset spans within `new` for the words that changed or were
//! added relative to `old`. Mirrors the pure/`gtk_ops` split of `overlay_search`.

/// Character-offset span of every whitespace-delimited word in `text`, in order.
fn word_spans(text: &str) -> Vec<(i32, i32, &str)> {
    let mut out = Vec::new();
    let mut start: Option<(usize, i32)> = None; // (byte, char) of current word start
    let mut char_idx = 0i32;
    let mut byte_idx = text.len();
    for (b, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some((sb, sc)) = start.take() {
                out.push((sc, char_idx, &text[sb..b]));
            }
        } else if start.is_none() {
            start = Some((b, char_idx));
        }
        char_idx += 1;
        byte_idx = b + c.len_utf8();
    }
    if let Some((sb, sc)) = start.take() {
        out.push((sc, char_idx, &text[sb..byte_idx]));
    }
    out
}

/// For each `new` word index, the `old` word index it is paired with in the LCS
/// (i.e. UNCHANGED), or `None` when the word is changed/added. Unpaired `new`
/// words are the substituted/inserted content; a paired word may still be
/// flagged "reflowed" by the caller if its surrounding paragraph structure
/// changed.
fn lcs_pairing(old_words: &[&str], new_words: &[&str]) -> Vec<Option<usize>> {
    let n = old_words.len();
    let m = new_words.len();
    // dp[i][j] = LCS length of old[i..] and new[j..]
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old_words[i] == new_words[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut pair = vec![None; m];
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if old_words[i] == new_words[j] {
            pair[j] = Some(i);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pair
}

/// Does the whitespace run immediately BEFORE the word at `spans[idx]` contain a
/// paragraph break (a blank line, i.e. two+ newlines)? Index 0 (first word) has
/// no leading gap, treated as no-break. `text` is the source the spans index.
fn leading_gap_has_break(text: &str, spans: &[(i32, i32, &str)], idx: usize) -> bool {
    if idx == 0 {
        return false;
    }
    let prev_end = spans[idx - 1].1; // char offset (exclusive) of previous word end
    let this_start = spans[idx].0; // char offset of this word start
    // The gap text is text[prev_end..this_start] in CHAR offsets; count newlines.
    text.chars()
        .skip(prev_end as usize)
        .take((this_start - prev_end) as usize)
        .filter(|c| *c == '\n')
        .count()
        >= 2
}

/// A paragraph of `text`: its char-offset span `[start, end)` and its trimmed
/// text (the run between blank lines). Paragraphs are separated by a blank line
/// (a run of whitespace containing 2+ newlines), matching how the reader splits
/// entries into display blocks.
struct Para {
    start: i32,
    end: i32,
    text: String,
}

/// Split `text` into paragraphs on blank lines. The span covers the paragraph's
/// own words (leading/trailing blank-line whitespace excluded); `text` is the
/// normalized (whitespace-collapsed) paragraph content, used for equality.
fn paragraphs(text: &str) -> Vec<Para> {
    let spans = word_spans(text);
    let mut out: Vec<Para> = Vec::new();
    let mut cur_start: Option<i32> = None;
    let mut cur_words: Vec<&str> = Vec::new();
    let mut cur_end = 0i32;
    for (idx, (s, e, w)) in spans.iter().enumerate() {
        if idx > 0 && leading_gap_has_break(text, &spans, idx) {
            // Close the current paragraph before starting the next.
            if let Some(start) = cur_start.take() {
                out.push(Para { start, end: cur_end, text: cur_words.join(" ") });
            }
            cur_words.clear();
        }
        if cur_start.is_none() {
            cur_start = Some(*s);
        }
        cur_words.push(w);
        cur_end = *e;
    }
    if let Some(start) = cur_start.take() {
        out.push(Para { start, end: cur_end, text: cur_words.join(" ") });
    }
    out
}

/// Character-offset spans within `new` covering what changed relative to `old`.
///
/// Two levels compose:
/// - **Paragraph level:** any `new` paragraph that is not identical (word-for-
///   word) to some `old` paragraph via a paragraph LCS is "changed". A paragraph
///   split therefore flags BOTH resulting paragraphs (neither equals the original
///   merged one), and a merge flags the merged paragraph — each tinted in FULL.
/// - **Word level:** an unchanged-boundary paragraph that only had words
///   substituted/added is NOT flagged whole; instead the changed words inside it
///   are tinted precisely.
///
/// Empty only when the texts are word-for-word identical AND identically
/// paragraphed.
pub fn changed_ranges(old: &str, new: &str) -> Vec<(i32, i32)> {
    let old_paras = paragraphs(old);
    let new_paras = paragraphs(new);
    let old_ptext: Vec<&str> = old_paras.iter().map(|p| p.text.as_str()).collect();
    let new_ptext: Vec<&str> = new_paras.iter().map(|p| p.text.as_str()).collect();
    // Paragraphs that survived unchanged (identical text) via the paragraph LCS.
    let para_pair = lcs_pairing(&old_ptext, &new_ptext);

    let mut ranges: Vec<(i32, i32)> = Vec::new();
    for (i, np) in new_paras.iter().enumerate() {
        if para_pair[i].is_some() {
            continue; // this paragraph is identical to an old one — no change
        }
        // A changed paragraph. If its words all still exist in `old` (a pure
        // reflow — split/merge with no wording change), tint the WHOLE paragraph
        // so the user sees the reshaped block. Otherwise tint just the word-level
        // changes within it (substitutions/additions).
        let word_ranges = word_level_changes(old, np.start, np.end, new);
        if word_ranges.is_empty() {
            // Pure reflow: no word changed, but the paragraph boundary did.
            ranges.push((np.start, np.end));
        } else {
            ranges.extend(word_ranges);
        }
    }
    ranges
}

/// Word-level changed spans WITHIN the `new` paragraph `[para_start, para_end)`,
/// diffed against the whole `old` text. Returns empty when every word in the
/// paragraph is matched in `old` (a pure reflow — the caller then tints the
/// whole paragraph). Adjacent changed words separated only by intra-paragraph
/// whitespace merge into one range.
fn word_level_changes(old: &str, para_start: i32, para_end: i32, new: &str) -> Vec<(i32, i32)> {
    let old_spans = word_spans(old);
    let new_spans = word_spans(new);
    let old_words: Vec<&str> = old_spans.iter().map(|(_, _, w)| *w).collect();
    let new_words: Vec<&str> = new_spans.iter().map(|(_, _, w)| *w).collect();
    let pair = lcs_pairing(&old_words, &new_words);

    let in_para = |idx: usize| -> bool {
        let (s, _, _) = new_spans[idx];
        s >= para_start && s < para_end
    };

    let mut ranges: Vec<(i32, i32)> = Vec::new();
    let mut prev_changed = false;
    for idx in 0..new_spans.len() {
        if !in_para(idx) {
            prev_changed = false;
            continue;
        }
        let changed = pair[idx].is_none();
        if !changed {
            prev_changed = false;
            continue;
        }
        let (s, e, _) = new_spans[idx];
        if prev_changed && !leading_gap_has_break(new, &new_spans, idx) {
            if let Some(last) = ranges.last_mut() {
                last.1 = e;
                prev_changed = true;
                continue;
            }
        }
        ranges.push((s, e));
        prev_changed = true;
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_has_no_changes() {
        assert!(changed_ranges("the cat sat", "the cat sat").is_empty());
    }

    #[test]
    fn single_word_substitution() {
        // "cat" -> "dog": the paragraph boundary is unchanged, so word-level:
        // only the middle word is tinted. "the dog sat": d=4..7
        assert_eq!(changed_ranges("the cat sat", "the dog sat"), vec![(4, 7)]);
    }

    #[test]
    fn appended_words_are_ranges() {
        // "the cat" -> "the cat sat down": same single paragraph, word-level —
        // "sat down" is new (chars 8..16)
        assert_eq!(changed_ranges("the cat", "the cat sat down"), vec![(8, 16)]);
    }

    #[test]
    fn adjacent_changed_words_merge_across_whitespace() {
        // both new words changed, same paragraph, separated only by a space -> one range
        assert_eq!(changed_ranges("a b", "a X Y"), vec![(2, 5)]);
    }

    #[test]
    fn char_offsets_not_byte_offsets() {
        // leading multibyte char: "é the cat" -> "é the dog" (one paragraph)
        // char offsets: é=0 sp=1 t=2 h=3 e=4 sp=5 d=6 -> dog = 6..9
        assert_eq!(changed_ranges("\u{e9} the cat", "\u{e9} the dog"), vec![(6, 9)]);
    }

    #[test]
    fn intra_paragraph_whitespace_change_has_no_ranges() {
        // Double space -> single space keeps the same single paragraph with the
        // same words, so nothing is highlighted.
        assert!(changed_ranges("the  cat", "the cat").is_empty());
    }

    #[test]
    fn unchanged_words_between_changes_are_not_highlighted() {
        // Same single paragraph, word-level: "a b c d" -> "a X c Y": b->X (2..3)
        // and d->Y (6..7); c unchanged and not tinted.
        assert_eq!(changed_ranges("a b c d", "a X c Y"), vec![(2, 3), (6, 7)]);
    }

    #[test]
    fn paragraph_split_highlights_both_resulting_paragraphs_in_full() {
        // Splitting "a b c d" into "a b" + "c d": neither new paragraph equals
        // the old merged one, and both are pure reflows (no word changed), so
        // BOTH are tinted in FULL. "a b\n\nc d": "a b" = 0..3, "c d" = 5..8.
        assert_eq!(changed_ranges("a b c d", "a b\n\nc d"), vec![(0, 3), (5, 8)]);
    }

    #[test]
    fn paragraph_merge_highlights_the_merged_paragraph_in_full() {
        // Merging "a b" + "c d" into "a b c d": the merged paragraph matches
        // neither old paragraph and is a pure reflow, so the whole merged
        // paragraph is tinted. "a b c d" = 0..7.
        assert_eq!(changed_ranges("a b\n\nc d", "a b c d"), vec![(0, 7)]);
    }

    #[test]
    fn unchanged_paragraph_alongside_a_split_is_not_tinted() {
        // "intro\n\na b c d" -> "intro\n\na b\n\nc d": the "intro" paragraph is
        // unchanged (matched), so it is NOT tinted; only the two split paragraphs.
        // "intro\n\na b\n\nc d": intro=0..5, "a b"=7..10, "c d"=12..15.
        assert_eq!(
            changed_ranges("intro\n\na b c d", "intro\n\na b\n\nc d"),
            vec![(7, 10), (12, 15)]
        );
    }

    #[test]
    fn word_change_inside_a_reflowed_paragraph_tints_word_level() {
        // Split AND a word change in the second new paragraph: "a b c d" ->
        // "a b\n\nc Z". Para "a b" (0..3) is a pure reflow -> whole. Para "c Z"
        // (5..8) has a word change (d->Z), so word-level within it: "Z" only.
        // c=5..6 Z=7..8. Since "c" is unchanged (matched) it's not tinted; only Z.
        assert_eq!(changed_ranges("a b c d", "a b\n\nc Z"), vec![(0, 3), (7, 8)]);
    }
}
