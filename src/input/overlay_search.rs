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

// --- GTK-touching helpers: apply/clear/(re)build the match set on an overlay
// buffer. `prelude` is scoped to this block (fn-local `use`) so the pure
// collect/step core above stays GTK-free.
mod gtk_ops {
    use super::{collect, OverlaySearch};
    use gtk4::prelude::*;

    fn char_iter(buffer: &gtk4::TextBuffer, off: i32) -> gtk4::TextIter {
        buffer.iter_at_offset(off)
    }

    /// Remove both search tags over the whole buffer.
    pub fn clear(buffer: &gtk4::TextBuffer, tag: &gtk4::TextTag, current_tag: &gtk4::TextTag) {
        let (start, end) = buffer.bounds();
        buffer.remove_tag(tag, &start, &end);
        buffer.remove_tag(current_tag, &start, &end);
    }

    /// Clear old tags, re-tag every `s.matches` span with `tag`, and tag the
    /// current match (`s.matches[s.current]`) with `current_tag`.
    pub fn apply(
        buffer: &gtk4::TextBuffer,
        tag: &gtk4::TextTag,
        current_tag: &gtk4::TextTag,
        s: &OverlaySearch,
    ) {
        clear(buffer, tag, current_tag);
        for (a, b) in &s.matches {
            buffer.apply_tag(tag, &char_iter(buffer, *a), &char_iter(buffer, *b));
        }
        if let Some((a, b)) = s.matches.get(s.current) {
            buffer.apply_tag(current_tag, &char_iter(buffer, *a), &char_iter(buffer, *b));
        }
    }

    /// The full text of `buffer`, start to end.
    pub fn buffer_text(buffer: &gtk4::TextBuffer) -> String {
        let (start, end) = buffer.bounds();
        buffer.text(&start, &end, false).to_string()
    }

    /// Build a fresh `OverlaySearch` for `pattern` against `buffer`'s current
    /// text, apply the tags, and return it.
    pub fn set_from_text(
        buffer: &gtk4::TextBuffer,
        tag: &gtk4::TextTag,
        current_tag: &gtk4::TextTag,
        pattern: &str,
    ) -> OverlaySearch {
        let matches = collect(&buffer_text(buffer), pattern);
        let s = OverlaySearch { pattern: pattern.to_string(), matches, current: 0 };
        apply(buffer, tag, current_tag, &s);
        s
    }

    /// Re-collect `s.pattern` against the buffer's CURRENT text (e.g. the
    /// entry text changed), clamp `s.current` into range, and re-apply tags.
    pub fn reapply(
        buffer: &gtk4::TextBuffer,
        tag: &gtk4::TextTag,
        current_tag: &gtk4::TextTag,
        s: &mut OverlaySearch,
    ) {
        s.matches = collect(&buffer_text(buffer), &s.pattern);
        if s.current >= s.matches.len() {
            s.current = s.matches.len().saturating_sub(1);
        }
        apply(buffer, tag, current_tag, s);
    }
}

pub use gtk_ops::{apply, buffer_text, clear, reapply, set_from_text};

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
