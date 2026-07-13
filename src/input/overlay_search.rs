//! Regex/literal search over an overlay's TextView buffer. Unlike reader
//! search (line-index over work.lines / state.buffer), this collects CHAR-offset
//! spans in an arbitrary buffer's text and is applied to the OVERLAY buffer's
//! own search TextTag. Reuses search::build_matcher for the regex + smart-case +
//! literal-fallback semantics. No AppState, no GTK types in the pure core.

/// A live search over one overlay buffer: the pattern, the char-offset spans of
/// every match in that buffer (in document order), and the current index.
#[derive(Debug, Clone, Default)]
pub struct OverlaySearch {
    pub pattern: String,
    pub matches: Vec<(i32, i32)>,
    pub current: usize,
}

/// Char-offset (start, end) spans of every non-empty match of `pattern` in
/// `text`, in document order. `pattern` is a regex (smart-cased); an invalid
/// regex degrades to a literal search. Empty pattern → no matches. Offsets are
/// CHARACTER offsets (GTK TextBuffer indexes by char), computed from the byte
/// offsets `regex` returns.
pub fn collect(text: &str, pattern: &str) -> Vec<(i32, i32)> {
    if pattern.is_empty() {
        return Vec::new();
    }
    let re = crate::input::search::build_matcher(pattern);
    let mut out = Vec::new();
    for m in re.find_iter(text) {
        if m.start() == m.end() {
            continue; // skip zero-width
        }
        // byte offset -> char offset
        let start_char = text[..m.start()].chars().count() as i32;
        let end_char = text[..m.end()].chars().count() as i32;
        out.push((start_char, end_char));
    }
    out
}

/// Step `cur` by ±1 within `len`, clamped, no wrap. None if it can't move.
pub fn step(cur: usize, len: usize, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if forward {
        if cur + 1 < len { Some(cur + 1) } else { None }
    } else if cur > 0 {
        Some(cur - 1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_char_offsets_regex_and_literal() {
        // two occurrences of "fee"
        let spans = collect("a fee and a fee simple", "fee");
        assert_eq!(spans, vec![(2, 5), (12, 15)]);
    }

    #[test]
    fn collect_char_offsets_are_char_not_byte() {
        // a leading multibyte char shifts byte offsets but not char offsets
        let spans = collect("\u{00e9} fee", "fee"); // é + space + fee
        assert_eq!(spans, vec![(2, 5)]); // char offsets: é=0, space=1, f=2
    }

    #[test]
    fn collect_smart_case_and_bad_regex_literal_fallback() {
        assert_eq!(collect("Fee fee", "fee").len(), 2); // lowercase query = case-insensitive
        // invalid regex "(" degrades to literal — matches the literal "("
        assert_eq!(collect("a ( b", "(").len(), 1);
    }

    #[test]
    fn collect_empty_pattern_is_empty() {
        assert!(collect("anything", "").is_empty());
    }

    #[test]
    fn step_clamps_no_wrap() {
        assert_eq!(step(0, 3, true), Some(1));
        assert_eq!(step(2, 3, true), None); // last, forward
        assert_eq!(step(0, 3, false), None); // first, back
        assert_eq!(step(0, 0, true), None); // empty
    }
}
