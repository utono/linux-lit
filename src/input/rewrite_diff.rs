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

/// Indices (into `new_words`) of tokens that are part of the LCS with `old_words`
/// (i.e. UNCHANGED). Everything else in `new` is changed/added.
fn lcs_matched_new_indices(old_words: &[&str], new_words: &[&str]) -> Vec<bool> {
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
    let mut matched = vec![false; m];
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if old_words[i] == new_words[j] {
            matched[j] = true;
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    matched
}

/// Character-offset spans within `new` covering words that changed or were added
/// relative to `old`. Adjacent changed words (separated only by whitespace) merge
/// into one range. Empty when the texts are word-for-word identical.
pub fn changed_ranges(old: &str, new: &str) -> Vec<(i32, i32)> {
    let old_spans = word_spans(old);
    let new_spans = word_spans(new);
    let old_words: Vec<&str> = old_spans.iter().map(|(_, _, w)| *w).collect();
    let new_words: Vec<&str> = new_spans.iter().map(|(_, _, w)| *w).collect();
    let matched = lcs_matched_new_indices(&old_words, &new_words);

    let mut ranges: Vec<(i32, i32)> = Vec::new();
    for (idx, (s, e, _)) in new_spans.iter().enumerate() {
        if matched[idx] {
            continue;
        }
        // Merge with the previous range if this changed word is the very next
        // token (previous range's end..this start is whitespace only).
        if let Some(last) = ranges.last_mut() {
            if idx > 0 && !matched[idx - 1] {
                last.1 = *e;
                continue;
            }
        }
        ranges.push((*s, *e));
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
    fn whitespace_only_change_has_no_ranges() {
        assert!(changed_ranges("the  cat", "the cat").is_empty());
    }

    #[test]
    fn unchanged_words_between_changes_are_not_highlighted() {
        // "a b c d" -> "a X c Y": b->X (2..3) and d->Y (6..7); c unchanged
        assert_eq!(changed_ranges("a b c d", "a X c Y"), vec![(2, 3), (6, 7)]);
    }
}
