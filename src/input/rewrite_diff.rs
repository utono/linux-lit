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

/// Character-offset spans within `new` covering words that changed, were added,
/// OR were reflowed (a paragraph break was inserted/removed immediately before
/// the word). Adjacent changed words (separated only by whitespace) merge into
/// one range. Empty only when the texts are word-for-word identical AND
/// identically paragraphed — a pure paragraph split therefore tints the word at
/// the new break, so a reformatting rewrite still shows what moved.
pub fn changed_ranges(old: &str, new: &str) -> Vec<(i32, i32)> {
    let old_spans = word_spans(old);
    let new_spans = word_spans(new);
    let old_words: Vec<&str> = old_spans.iter().map(|(_, _, w)| *w).collect();
    let new_words: Vec<&str> = new_spans.iter().map(|(_, _, w)| *w).collect();
    let pair = lcs_pairing(&old_words, &new_words);

    // A word counts as changed if it is unpaired (substituted/inserted) OR it is
    // paired but its leading paragraph-break status differs from its old
    // counterpart (reflowed — a `\n\n` appeared or vanished before it).
    let is_changed = |idx: usize| -> bool {
        match pair[idx] {
            None => true,
            Some(old_idx) => {
                leading_gap_has_break(new, &new_spans, idx)
                    != leading_gap_has_break(old, &old_spans, old_idx)
            }
        }
    };

    let mut ranges: Vec<(i32, i32)> = Vec::new();
    for idx in 0..new_spans.len() {
        if !is_changed(idx) {
            continue;
        }
        let (s, e, _) = new_spans[idx];
        // Merge with the previous range if this changed word is the very next
        // token AND is separated from it only by intra-paragraph whitespace (no
        // paragraph break) — so a highlight never spans a blank line.
        if let Some(last) = ranges.last_mut() {
            if idx > 0 && is_changed(idx - 1) && !leading_gap_has_break(new, &new_spans, idx) {
                last.1 = e;
                continue;
            }
        }
        ranges.push((s, e));
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
        // "cat" -> "dog": only the middle word changed.
        // "the dog sat": offsets d=4..7
        assert_eq!(changed_ranges("the cat sat", "the dog sat"), vec![(4, 7)]);
    }

    #[test]
    fn appended_words_are_ranges() {
        // "the cat" -> "the cat sat down": "sat down" is new (chars 8..16)
        assert_eq!(changed_ranges("the cat", "the cat sat down"), vec![(8, 16)]);
    }

    #[test]
    fn adjacent_changed_words_merge_across_whitespace() {
        // both new words changed and are separated only by a space -> one range
        assert_eq!(changed_ranges("a b", "a X Y"), vec![(2, 5)]);
    }

    #[test]
    fn char_offsets_not_byte_offsets() {
        // leading multibyte char: "é the cat" -> "é the dog"
        // char offsets: é=0 sp=1 t=2 h=3 e=4 sp=5 d=6 -> dog = 6..9
        assert_eq!(changed_ranges("\u{e9} the cat", "\u{e9} the dog"), vec![(6, 9)]);
    }

    #[test]
    fn intra_paragraph_whitespace_change_has_no_ranges() {
        // Double space -> single space is NOT a paragraph-break change, so no
        // word is reflowed and nothing is highlighted.
        assert!(changed_ranges("the  cat", "the cat").is_empty());
    }

    #[test]
    fn unchanged_words_between_changes_are_not_highlighted() {
        // "a b c d" -> "a X c Y": b->X (2..3) and d->Y (6..7); c unchanged
        assert_eq!(changed_ranges("a b c d", "a X c Y"), vec![(2, 3), (6, 7)]);
    }

    #[test]
    fn paragraph_split_highlights_the_reflowed_word() {
        // Same words, but a paragraph break is inserted before "c": the word "c"
        // now starts a new paragraph where it didn't before, so it is tinted.
        // "a b\n\nc d": chars a=0 sp=1 b=2 \n=3 \n=4 c=5 sp=6 d=7 -> "c" = 5..6
        assert_eq!(changed_ranges("a b c d", "a b\n\nc d"), vec![(5, 6)]);
    }

    #[test]
    fn paragraph_merge_highlights_the_reflowed_word() {
        // Removing a paragraph break also reflows the word that followed it.
        // "a b c d": "c" no longer starts a paragraph (it did in old) -> tinted.
        // chars: a=0 sp=1 b=2 sp=3 c=4 sp=5 d=6 -> "c" = 4..5
        assert_eq!(changed_ranges("a b\n\nc d", "a b c d"), vec![(4, 5)]);
    }

    #[test]
    fn reflow_and_word_change_both_highlight() {
        // "b"->"X" (word change) AND a break before "c" (reflow).
        // "a X\n\nc d": a=0 sp=1 X=2 \n=3 \n=4 c=5 sp=6 d=7 -> X=2..3, c=5..6
        assert_eq!(changed_ranges("a b c d", "a X\n\nc d"), vec![(2, 3), (5, 6)]);
    }
}
