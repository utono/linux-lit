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
    // Trim the common prefix/suffix before the O(n·m) DP: a rewrite usually
    // touches one region of a long answer, so the quadratic table only spans
    // the changed middle. (The untrimmed DP over a whole ~1500-word answer is
    // a multi-MB allocation per call — the Ctrl+j landing-diff slowness.)
    let mut pair = vec![None; new_words.len()];
    let common_prefix = old_words
        .iter()
        .zip(new_words.iter())
        .take_while(|(a, b)| a == b)
        .count();
    for (j, p) in pair.iter_mut().enumerate().take(common_prefix) {
        *p = Some(j);
    }
    let rem_old = old_words.len() - common_prefix;
    let rem_new = new_words.len() - common_prefix;
    let common_suffix = old_words[common_prefix..]
        .iter()
        .rev()
        .zip(new_words[common_prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(rem_old.min(rem_new));
    for k in 0..common_suffix {
        pair[new_words.len() - 1 - k] = Some(old_words.len() - 1 - k);
    }

    let old_mid = &old_words[common_prefix..old_words.len() - common_suffix];
    let new_mid = &new_words[common_prefix..new_words.len() - common_suffix];
    let n = old_mid.len();
    let m = new_mid.len();
    // dp[i][j] = LCS length of old_mid[i..] and new_mid[j..]
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old_mid[i] == new_mid[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if old_mid[i] == new_mid[j] {
            pair[common_prefix + j] = Some(common_prefix + i);
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
/// - **Sentence level:** an unchanged-boundary paragraph that only had words
///   substituted/added is NOT flagged whole; instead each SENTENCE that contains
///   a changed word is tinted in full. Sentence granularity (not word) is
///   deliberate: it lets the reader locate the rewritten passage as a contiguous
///   run, rather than scattering the highlight across the shared function words a
///   word-level diff leaves untinted between edits.
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

    // Word-level LCS over the WHOLE texts, computed ONCE and shared by every
    // changed paragraph below. (It previously ran inside sentence_level_changes
    // per changed paragraph — O(paragraphs × words²), the dominant cost of the
    // journal landing diff.) Skipped entirely when no paragraph changed.
    let any_changed = (0..new_paras.len()).any(|i| para_pair[i].is_none());
    let (new_spans, word_pair) = if any_changed {
        let old_spans = word_spans(old);
        let new_spans = word_spans(new);
        let old_words: Vec<&str> = old_spans.iter().map(|(_, _, w)| *w).collect();
        let new_words: Vec<&str> = new_spans.iter().map(|(_, _, w)| *w).collect();
        let word_pair = lcs_pairing(&old_words, &new_words);
        (new_spans, word_pair)
    } else {
        (Vec::new(), Vec::new())
    };

    let mut ranges: Vec<(i32, i32)> = Vec::new();
    for (i, np) in new_paras.iter().enumerate() {
        if para_pair[i].is_some() {
            continue; // this paragraph is identical to an old one — no change
        }
        // A changed paragraph. If its words all still exist in `old` (a pure
        // reflow — split/merge with no wording change), tint the WHOLE paragraph
        // so the user sees the reshaped block. Otherwise tint the sentences that
        // contain substituted/added words.
        let sentence_ranges = sentence_level_changes(&new_spans, &word_pair, np.start, np.end);
        if sentence_ranges.is_empty() {
            // Pure reflow: no word changed, but the paragraph boundary did.
            ranges.push((np.start, np.end));
        } else {
            ranges.extend(sentence_ranges);
        }
    }
    ranges
}

/// True if `w` ends a sentence — its last non-quote/bracket char is `.`/`!`/`?`.
/// Trailing quotes and closing brackets after the terminator still count (e.g.
/// `door."`, `earth?)`).
fn ends_sentence(w: &str) -> bool {
    let trimmed = w.trim_end_matches(['"', '\'', ')', ']', '}', '»', '”', '’']);
    matches!(trimmed.chars().last(), Some('.') | Some('!') | Some('?'))
}

/// Sentence-level changed spans WITHIN the `new` paragraph `[para_start,
/// para_end)`. `new_spans`/`pair` are the whole-text word spans and word-level
/// LCS pairing against `old`, computed once by `changed_ranges` and shared
/// across every changed paragraph. Groups the paragraph's words into sentences
/// (split after a word whose text ends in `.`/`!`/`?`) and emits one span —
/// first word start to last word end — for each sentence that contains at
/// least one changed (LCS-unpaired) word. Returns empty when every word in the
/// paragraph is matched in `old` (a pure reflow — the caller then tints the
/// whole paragraph).
fn sentence_level_changes(
    new_spans: &[(i32, i32, &str)],
    pair: &[Option<usize>],
    para_start: i32,
    para_end: i32,
) -> Vec<(i32, i32)> {
    let in_para = |idx: usize| -> bool {
        let (s, _, _) = new_spans[idx];
        s >= para_start && s < para_end
    };

    let mut ranges: Vec<(i32, i32)> = Vec::new();
    // Accumulate the current sentence's span + whether any word in it changed.
    let mut sent_start: Option<i32> = None;
    let mut sent_end = 0i32;
    let mut sent_changed = false;

    let flush = |start: &mut Option<i32>, end: i32, changed: &mut bool, out: &mut Vec<(i32, i32)>| {
        if let Some(s) = start.take() {
            if *changed {
                out.push((s, end));
            }
        }
        *changed = false;
    };

    for idx in 0..new_spans.len() {
        if !in_para(idx) {
            continue;
        }
        let (s, e, w) = new_spans[idx];
        if sent_start.is_none() {
            sent_start = Some(s);
        }
        sent_end = e;
        if pair[idx].is_none() {
            sent_changed = true;
        }
        if ends_sentence(w) {
            flush(&mut sent_start, sent_end, &mut sent_changed, &mut ranges);
        }
    }
    // Trailing sentence with no terminal punctuation.
    flush(&mut sent_start, sent_end, &mut sent_changed, &mut ranges);
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_has_no_changes() {
        assert!(changed_ranges("the cat sat.", "the cat sat.").is_empty());
    }

    #[test]
    fn single_word_substitution_tints_the_whole_sentence() {
        // "cat" -> "dog" inside one sentence: the whole sentence is tinted so the
        // reader locates the rewritten sentence, not an isolated word.
        // "the dog sat." = 0..12.
        assert_eq!(changed_ranges("the cat sat.", "the dog sat."), vec![(0, 12)]);
    }

    #[test]
    fn only_the_changed_sentence_is_tinted() {
        // Two sentences in one paragraph; only the second is reworded. The first
        // stays clean; the second is tinted whole.
        // "One stays. Two changed." — "Two changed." starts at char 11, ends 23.
        assert_eq!(
            changed_ranges("One stays. Two stays.", "One stays. Two changed."),
            vec![(11, 23)]
        );
    }

    #[test]
    fn appended_words_tint_their_sentence() {
        // Appending to the lone sentence tints the whole (now-longer) sentence.
        // "the cat sat down." = 0..17.
        assert_eq!(
            changed_ranges("the cat.", "the cat sat down."),
            vec![(0, 17)]
        );
    }

    #[test]
    fn char_offsets_not_byte_offsets() {
        // leading multibyte char: "é the cat." -> "é the dog." (one sentence).
        // The whole sentence is tinted; it spans the entire string: 0..10.
        assert_eq!(
            changed_ranges("\u{e9} the cat.", "\u{e9} the dog."),
            vec![(0, 10)]
        );
    }

    #[test]
    fn intra_sentence_whitespace_change_has_no_ranges() {
        // Double space -> single space keeps the same paragraph with the same
        // words, so nothing is highlighted.
        assert!(changed_ranges("the  cat.", "the cat.").is_empty());
    }

    #[test]
    fn two_changed_sentences_each_tint_whole() {
        // "a b c d" one word per token, two sentences: "A b. C d." -> "A X. C Y."
        // Both sentences changed; each tinted whole. "A X." = 0..4, "C Y." = 5..9.
        assert_eq!(
            changed_ranges("A b. C d.", "A X. C Y."),
            vec![(0, 4), (5, 9)]
        );
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
    fn word_change_inside_a_reflowed_paragraph_tints_its_sentence() {
        // Split AND a word change in the second new paragraph: "a b c d" ->
        // "a b\n\nc Z". Para "a b" (0..3) is a pure reflow -> whole. Para "c Z"
        // (5..8) has a word change (d->Z); its lone sentence "c Z" is tinted
        // whole (5..8), not just the changed word.
        assert_eq!(changed_ranges("a b c d", "a b\n\nc Z"), vec![(0, 3), (5, 8)]);
    }

    #[test]
    fn heavy_rewrite_does_not_speckle() {
        // A fully reworded sentence sharing only function words must NOT produce
        // scattered per-word spans (the bug): the single reworded sentence is one
        // contiguous range covering the whole sentence.
        let old = "The cat sat on the mat by the door.";
        let new = "The dog ran across the field near the gate.";
        // One paragraph, one sentence, reworded -> whole sentence tinted: 0..len.
        let len = new.chars().count() as i32;
        assert_eq!(changed_ranges(old, new), vec![(0, len)]);
    }
}
