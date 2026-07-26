//! Sentence-boundary expansion for the word-underline syntax-diagram entry
//! point. Pure: takes a joined text window and char offsets into it, returns
//! the char span of the sentence(s) those offsets fall in.
//!
//! Char offsets into ONE joined string, not (line, char) pairs — a "line" is
//! not a unit here. A two-column play's buffer line is one verse line, but a
//! prose `line_mapping` row in BH-Barrett runs to 2,874 characters (a whole
//! paragraph holding many sentences).

/// Abbreviations whose trailing period is never a sentence boundary.
const ABBREVIATIONS: &[&str] = &[
    "Mr", "Mrs", "Ms", "Dr", "St", "Prof", "Rev", "Hon", "Sr", "Jr", "Capt", "Col", "Gen", "Lt",
    "Sgt", "Maj", "Esq", "vs", "etc", "No",
];

/// Characters that close a sentence.
const TERMINATORS: &[char] = &['.', '!', '?'];

/// Trailing characters that belong to the sentence they follow.
const TRAILERS: &[char] = &['"', '\'', ')', ']', '\u{201d}', '\u{2019}'];

/// Expand `ranges` (char offsets into `text`) outward to sentence boundaries.
///
/// `text` is the already-joined buffer region; the caller decides how much
/// context to hand in, so this function never touches lines, the buffer, or
/// GTK. Returns a half-open char span, or `None` when there is nothing to
/// expand (empty text or no ranges).
///
/// Out-of-bounds ranges are clamped rather than rejected: they mean the
/// caller's offsets went stale, and the useful answer is still "the sentence
/// nearest that position".
pub fn sentence_span(text: &str, ranges: &[(usize, usize)]) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || ranges.is_empty() {
        return None;
    }

    let last = chars.len() - 1;
    let lo = ranges.iter().map(|r| r.0).min()?.min(last);
    let hi = ranges.iter().map(|r| r.1).max()?.min(chars.len());

    let start = sentence_start(&chars, lo);
    let end = sentence_end(&chars, hi.saturating_sub(1).max(lo));
    Some((start, end))
}

/// Scan backwards from `from` for the first real terminator; the sentence
/// starts at the first non-space character after it.
fn sentence_start(chars: &[char], from: usize) -> usize {
    let mut i = from;
    while i > 0 {
        i -= 1;
        if is_boundary(chars, i) {
            let mut s = i + 1;
            // Step over the terminator's own trailing quotes/brackets, then
            // any whitespace, to land on the next sentence's first char.
            while s < chars.len() && TRAILERS.contains(&chars[s]) {
                s += 1;
            }
            while s < chars.len() && chars[s].is_whitespace() {
                s += 1;
            }
            return s;
        }
    }
    0
}

/// Scan forwards from `from` for the first real terminator; the sentence ends
/// after it plus any trailing quote/bracket that belongs to it.
fn sentence_end(chars: &[char], from: usize) -> usize {
    let mut i = from;
    while i < chars.len() {
        if is_boundary(chars, i) {
            let mut e = i + 1;
            while e < chars.len() && TRAILERS.contains(&chars[e]) {
                e += 1;
            }
            return e;
        }
        i += 1;
    }
    chars.len()
}

/// Is `chars[i]` a real sentence terminator?
///
/// `.` is the ambiguous one — `!` and `?` are unconditional. A period is NOT a
/// boundary when it ends a known abbreviation, ends a single-letter initial,
/// or is followed by a lowercase word (which means the sentence continued).
fn is_boundary(chars: &[char], i: usize) -> bool {
    if !TERMINATORS.contains(&chars[i]) {
        return false;
    }
    if chars[i] != '.' {
        return true;
    }

    // The word immediately before the period.
    let mut w_start = i;
    while w_start > 0 && chars[w_start - 1].is_alphanumeric() {
        w_start -= 1;
    }
    let word: String = chars[w_start..i].iter().collect();

    if ABBREVIATIONS.iter().any(|a| a.eq_ignore_ascii_case(&word)) {
        return false;
    }
    // A single-letter word before a period is an initial: "J. R. Smith".
    if word.chars().count() == 1 && word.chars().next().is_some_and(|c| c.is_alphabetic()) {
        return false;
    }

    // Look past the period (and any closing quote) for the next letter. A
    // lowercase one means the sentence did not actually end.
    let mut j = i + 1;
    while j < chars.len() && (chars[j].is_whitespace() || TRAILERS.contains(&chars[j])) {
        j += 1;
    }
    match chars.get(j) {
        Some(c) if c.is_lowercase() => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: char-slice a span for readable assertions.
    fn slice(text: &str, span: (usize, usize)) -> String {
        text.chars().skip(span.0).take(span.1 - span.0).collect()
    }

    #[test]
    fn expands_to_surrounding_sentence() {
        let text = "First one. The second sentence here. Third one.";
        // "second" starts at char 15.
        let span = sentence_span(text, &[(15, 21)]).unwrap();
        assert_eq!(slice(text, span), "The second sentence here.");
    }

    #[test]
    fn does_not_break_on_mister_abbreviation() {
        let text = "Mr. Bucket looked at him. Then he left.";
        let span = sentence_span(text, &[(4, 10)]).unwrap();
        assert_eq!(slice(text, span), "Mr. Bucket looked at him.");
    }

    #[test]
    fn does_not_break_on_initials() {
        let text = "It was J. R. Smith who spoke. Nobody answered.";
        let span = sentence_span(text, &[(13, 18)]).unwrap();
        assert_eq!(slice(text, span), "It was J. R. Smith who spoke.");
    }

    #[test]
    fn does_not_break_on_lowercase_after_period() {
        // A period followed by space + lowercase is not a boundary.
        let text = "He went to No. five and waited. Then home.";
        let span = sentence_span(text, &[(0, 2)]).unwrap();
        assert_eq!(slice(text, span), "He went to No. five and waited.");
    }

    #[test]
    fn includes_closing_quote_in_span() {
        let text = "She asked, \"What's that?\" He said nothing.";
        let span = sentence_span(text, &[(11, 16)]).unwrap();
        assert_eq!(slice(text, span), "She asked, \"What's that?\"");
    }

    #[test]
    fn spans_a_line_break_inside_the_window() {
        let text = "To be or not to be,\nthat is the question. Next.";
        let span = sentence_span(text, &[(23, 27)]).unwrap();
        assert_eq!(slice(text, span), "To be or not to be,\nthat is the question.");
    }

    #[test]
    fn union_when_ranges_cross_two_sentences() {
        let text = "First one here. Second one there. Third.";
        // "one" in sentence 1 (6..9) and "there" in sentence 2 (27..32).
        let span = sentence_span(text, &[(6, 9), (27, 32)]).unwrap();
        assert_eq!(slice(text, span), "First one here. Second one there.");
    }

    #[test]
    fn whole_window_when_no_boundary_present() {
        let text = "no terminator anywhere in this window";
        let span = sentence_span(text, &[(3, 13)]).unwrap();
        assert_eq!(slice(text, span), text);
    }

    #[test]
    fn start_of_window_is_a_valid_start() {
        let text = "Opening sentence. Second.";
        let span = sentence_span(text, &[(0, 7)]).unwrap();
        assert_eq!(slice(text, span), "Opening sentence.");
    }

    #[test]
    fn end_of_window_is_a_valid_end() {
        let text = "First. Trailing sentence with no period";
        let span = sentence_span(text, &[(16, 24)]).unwrap();
        assert_eq!(slice(text, span), "Trailing sentence with no period");
    }

    #[test]
    fn empty_ranges_returns_none() {
        assert_eq!(sentence_span("Anything at all.", &[]), None);
    }

    #[test]
    fn empty_text_returns_none() {
        assert_eq!(sentence_span("", &[(0, 1)]), None);
    }

    #[test]
    fn out_of_bounds_range_is_clamped_not_panicking() {
        let text = "Short sentence.";
        let span = sentence_span(text, &[(900, 950)]).unwrap();
        assert_eq!(slice(text, span), "Short sentence.");
    }

    #[test]
    fn handles_multibyte_text_by_chars_not_bytes() {
        // Every char here is multi-byte; offsets must be char-based.
        let text = "Æsop wrote it. Naïve reader—café.";
        let span = sentence_span(text, &[(15, 20)]).unwrap();
        assert_eq!(slice(text, span), "Naïve reader—café.");
    }
}
