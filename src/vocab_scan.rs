//! Shared vocab-word scanner: tokenizes text lines against the lit.db word
//! set. Used by the main reading buffer, the gloss/journal overlay buffers,
//! and the chat panel's label specs. Word chars: alphanumeric, ' and '
//! (same rule build_vocab_matches always used).

use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct VocabSpan {
    pub word: String,
    pub line_index: usize,
    pub char_start: usize,
    pub char_end: usize,
}

pub fn scan_lines<'a, I>(lines: I, words: &HashSet<String>, skip_upper: bool) -> Vec<VocabSpan>
where
    I: Iterator<Item = (usize, &'a str)>,
{
    let mut out = Vec::new();
    for (line_index, line_text) in lines {
        let trimmed = line_text.trim();
        if skip_upper
            && !trimmed.is_empty()
            && trimmed.chars().any(|c| c.is_alphabetic())
            && trimmed == trimmed.to_uppercase()
        {
            continue;
        }
        scan_line(line_text, line_index, words, &mut out);
    }
    out
}

/// Scan one line, pushing matches. CHAR offsets, not bytes.
pub fn scan_line(text: &str, line_index: usize, words: &HashSet<String>, out: &mut Vec<VocabSpan>) {
    let mut char_offset = 0usize;
    let mut in_word = false;
    let mut word_start = 0usize;
    let mut word_buf = String::new();
    let flush = |buf: &str, start: usize, end: usize, out: &mut Vec<VocabSpan>| {
        let lower = buf.to_lowercase();
        if words.contains(&lower) {
            out.push(VocabSpan { word: lower, line_index, char_start: start, char_end: end });
        }
    };
    for ch in text.chars() {
        let is_word_char = ch.is_alphanumeric() || ch == '\'' || ch == '\u{2019}';
        if is_word_char {
            if !in_word {
                word_start = char_offset;
                word_buf.clear();
                in_word = true;
            }
            word_buf.push(ch);
        } else if in_word {
            flush(&word_buf, word_start, char_offset, out);
            in_word = false;
        }
        char_offset += 1;
    }
    if in_word {
        flush(&word_buf, word_start, char_offset, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> HashSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn finds_words_with_char_offsets() {
        let spans = scan_lines(
            [(0usize, "Should censure thus on lovely gentlemen.")].into_iter(),
            &words(&["censure"]),
            false,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].word, "censure");
        assert_eq!(spans[0].char_start, 7);
        assert_eq!(spans[0].char_end, 14);
    }

    #[test]
    fn matches_are_case_insensitive_and_apostrophe_aware() {
        let spans = scan_lines(
            [(3usize, "PARLE and parle\u{2019}d")].into_iter(),
            &words(&["parle", "parle\u{2019}d"]),
            false,
        );
        // skip_upper=false: both tokens scanned; "PARLE" lowercases to a hit.
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].line_index, 3);
    }

    #[test]
    fn skip_upper_skips_speaker_header_lines() {
        let spans = scan_lines(
            [(0usize, "LUCETTA"), (1usize, "censure me")].into_iter(),
            &words(&["lucetta", "censure"]),
            true,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].word, "censure");
    }

    #[test]
    fn trailing_word_at_end_of_line_is_flushed() {
        let spans = scan_lines(
            [(0usize, "with parle")].into_iter(),
            &words(&["parle"]),
            false,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].char_end, 10);
    }
}
