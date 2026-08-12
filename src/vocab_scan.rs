//! Shared vocab-word scanner: tokenizes text lines against the lit.db word
//! set. Used by the main reading buffer, the gloss/journal overlay buffers,
//! and the chat panel's label specs. Word chars: alphanumeric, ' and '
//! (same rule build_vocab_matches always used).
//!
//! Multi-word headwords (Latin phrases, `lang='la'`) are matched by a
//! SECOND pass that runs after the single-token pass and suppresses any
//! token span it overlaps. With no phrases loaded the output is identical
//! to the single-token behaviour, so non-Latin works cannot regress.

use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub struct VocabSpan {
    pub word: String,
    pub line_index: usize,
    pub char_start: usize,
    pub char_end: usize,
}

/// Single words plus multi-word phrases, indexed by first token.
#[derive(Debug, Clone, Default)]
pub struct VocabSet {
    pub words: HashSet<String>,
    /// first token (lowercase) -> phrases, each as its token list.
    pub phrases: HashMap<String, Vec<Vec<String>>>,
}

impl VocabSet {
    pub fn new(words: HashSet<String>, phrase_strings: Vec<String>) -> Self {
        let mut phrases: HashMap<String, Vec<Vec<String>>> = HashMap::new();
        for p in phrase_strings {
            let toks: Vec<String> = tokenize(&p.to_lowercase());
            if toks.len() < 2 {
                continue; // single tokens belong in `words`
            }
            phrases.entry(toks[0].clone()).or_default().push(toks);
        }
        // Longest first, so a longer phrase wins over a shorter prefix.
        for v in phrases.values_mut() {
            v.sort_by(|a, b| b.len().cmp(&a.len()));
        }
        VocabSet { words, phrases }
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.phrases.is_empty()
    }
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '\'' || ch == '\u{2019}'
}

fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !is_word_char(c))
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

/// (token_lowercase, char_start, char_end) for every token in `text`.
fn token_positions(text: &str) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    let mut char_offset = 0usize;
    let mut in_word = false;
    let mut start = 0usize;
    let mut buf = String::new();
    for ch in text.chars() {
        if is_word_char(ch) {
            if !in_word {
                start = char_offset;
                buf.clear();
                in_word = true;
            }
            buf.push(ch);
        } else if in_word {
            out.push((buf.to_lowercase(), start, char_offset));
            in_word = false;
        }
        char_offset += 1;
    }
    if in_word {
        out.push((buf.to_lowercase(), start, char_offset));
    }
    out
}

pub fn scan_lines<'a, I>(lines: I, set: &VocabSet, skip_upper: bool) -> Vec<VocabSpan>
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
        scan_line(line_text, line_index, set, &mut out);
    }
    out
}

/// Scan one line, pushing matches. CHAR offsets, not bytes.
pub fn scan_line(text: &str, line_index: usize, set: &VocabSet, out: &mut Vec<VocabSpan>) {
    let tokens = token_positions(text);

    // Pass 2 first (compute only): find phrase spans so pass 1 can skip
    // any token they cover.
    let mut phrase_spans: Vec<(usize, usize, String)> = Vec::new();
    if !set.phrases.is_empty() {
        let mut i = 0usize;
        while i < tokens.len() {
            let mut matched = false;
            if let Some(cands) = set.phrases.get(&tokens[i].0) {
                for toks in cands {
                    if i + toks.len() > tokens.len() {
                        continue;
                    }
                    if (0..toks.len()).all(|k| tokens[i + k].0 == toks[k]) {
                        let start = tokens[i].1;
                        let end = tokens[i + toks.len() - 1].2;
                        phrase_spans.push((start, end, toks.join(" ")));
                        i += toks.len();
                        matched = true;
                        break;
                    }
                }
            }
            if !matched {
                i += 1;
            }
        }
    }

    // Pass 1: single tokens not covered by a phrase.
    for (tok, start, end) in &tokens {
        let covered = phrase_spans
            .iter()
            .any(|(ps, pe, _)| *start < *pe && *ps < *end);
        if covered {
            continue;
        }
        if set.words.contains(tok) {
            out.push(VocabSpan {
                word: tok.clone(),
                line_index,
                char_start: *start,
                char_end: *end,
            });
        }
    }

    for (start, end, word) in phrase_spans {
        out.push(VocabSpan { word, line_index, char_start: start, char_end: end });
    }
    out.sort_by_key(|s| s.char_start);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(words: &[&str], phrases: &[&str]) -> VocabSet {
        VocabSet::new(
            words.iter().map(|s| s.to_string()).collect(),
            phrases.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn finds_words_with_char_offsets() {
        let spans = scan_lines(
            [(0usize, "Should censure thus on lovely gentlemen.")].into_iter(),
            &set(&["censure"], &[]),
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
            &set(&["parle", "parle\u{2019}d"], &[]),
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
            &set(&["lucetta", "censure"], &[]),
            true,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].word, "censure");
    }

    #[test]
    fn trailing_word_at_end_of_line_is_flushed() {
        let spans = scan_lines(
            [(0usize, "with parle")].into_iter(),
            &set(&["parle"], &[]),
            false,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].char_end, 10);
    }

    #[test]
    fn phrase_matches_as_single_span() {
        let spans = scan_lines(
            [(0usize, "predilection for my natale solum, nay,")].into_iter(),
            &set(&[], &["natale solum"]),
            false,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].word, "natale solum");
        assert_eq!(spans[0].char_start, 20);
        assert_eq!(spans[0].char_end, 32);
    }

    #[test]
    fn phrase_suppresses_overlapping_single_word() {
        // `solum` is also a single entry; inside the phrase the phrase wins.
        let spans = scan_lines(
            [(0usize, "my natale solum, nay")].into_iter(),
            &set(&["solum"], &["natale solum"]),
            false,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].word, "natale solum");
    }

    #[test]
    fn single_word_still_matches_outside_a_phrase() {
        let spans = scan_lines(
            [(0usize, "the solum alone")].into_iter(),
            &set(&["solum"], &["natale solum"]),
            false,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].word, "solum");
    }

    #[test]
    fn longest_phrase_wins() {
        let spans = scan_lines(
            [(0usize, "Dulce et decorum est pro patria mori here")].into_iter(),
            &set(&[], &["Dulce et", "Dulce et decorum est pro patria mori"]),
            false,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].word, "dulce et decorum est pro patria mori");
    }

    #[test]
    fn phrase_matches_accented_text() {
        let spans = scan_lines(
            [(0usize, "pro patri\u{e2} mori")].into_iter(),
            &set(&[], &["pro patri\u{e2} mori"]),
            false,
        );
        // 15 CHARS, not bytes — `â` is 2 bytes but 1 char. Getting this
        // wrong is exactly the class of bug the offsets guard catches.
        assert_eq!(spans[0].char_end, 15);
    }

    #[test]
    fn phrase_offsets_never_exceed_text_length() {
        // Guards the translate_offset abort hazard.
        let text = "natale solum";
        let spans = scan_lines(
            [(0usize, text)].into_iter(),
            &set(&[], &["natale solum"]),
            false,
        );
        let len = text.chars().count();
        for s in &spans {
            assert!(s.char_end <= len, "char_end {} > len {}", s.char_end, len);
            assert!(s.char_start <= s.char_end);
        }
    }

    #[test]
    fn empty_phrase_set_is_identical_to_single_token_scan() {
        // REGRESSION GUARD: existing works must not change at all.
        let line = "Should censure thus on lovely gentlemen.";
        let spans = scan_lines(
            [(0usize, line)].into_iter(),
            &set(&["censure", "lovely"], &[]),
            false,
        );
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].word, "censure");
        assert_eq!(spans[0].char_start, 7);
        assert_eq!(spans[0].char_end, 14);
        assert_eq!(spans[1].word, "lovely");
    }
}
