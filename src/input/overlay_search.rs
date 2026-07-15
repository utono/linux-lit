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

// The per-page GTK apply/clear/reapply of the search highlights now lives on the
// overlays themselves (`JournalOverlay`/`GlossOverlay`: `set_search_matches`,
// `clear_search_tags`, `reapply_search`), because a paginated entry's matches are
// whole-body char offsets that must be clipped to the shown page — the same model
// the rewrite-diff highlight uses. This module keeps only the pure, GTK-free
// `collect`/`step` core, which those overlay methods and the handlers feed the
// WHOLE-ENTRY text (every page), not just the rendered buffer.

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

    // --- Cross-page whole-entry-offset invariant (models the paragraph basis the
    // journal overlay's `whole_entry_text` / `page_char_span` / `page_for_whole_offset`
    // use). The overlays compute these GTK-side, but the ARITHMETIC contract they
    // rely on is pure and testable here: given paragraphs joined by "\n\n" and a
    // pagination that splits them into page ranges, every collected match offset
    // (into the whole-entry text) must map back — via the per-page (start,len)
    // spans — onto exactly its own page and the correct page-local substring.

    /// Char (start, len) span of each page in the whole-entry basis, mirroring
    /// `JournalOverlay::page_char_span`: paragraphs joined by "\n\n" (2 join chars
    /// between blocks and between pages). `page_ranges` are [start_para, end_para).
    fn page_spans(paras: &[&str], page_ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
        let clen = |s: &str| s.chars().count();
        page_ranges
            .iter()
            .map(|&(ps, pe)| {
                let start: usize = paras[..ps].iter().map(|p| clen(p) + 2).sum();
                let mut len = 0usize;
                for (i, p) in paras[ps..pe].iter().enumerate() {
                    if i > 0 {
                        len += 2;
                    }
                    len += clen(p);
                }
                (start, len)
            })
            .collect()
    }

    fn page_for_offset(spans: &[(usize, usize)], off: usize) -> usize {
        for (i, &(s, l)) in spans.iter().enumerate() {
            if off >= s && off < s + l {
                return i;
            }
        }
        spans.len().saturating_sub(1)
    }

    #[test]
    fn whole_entry_len_equals_sum_of_page_spans_plus_joins() {
        let paras = ["Q: alpha beta", "gamma delta", "epsilon zeta", "eta theta"];
        let whole = paras.join("\n\n");
        // Two pages: [0,2) and [2,4).
        let ranges = [(0usize, 2usize), (2usize, 4usize)];
        let spans = page_spans(&paras, &ranges);
        let sum_len: usize = spans.iter().map(|&(_, l)| l).sum();
        // whole = Σ page_len + one "\n\n" (2 chars) join BETWEEN the two pages.
        assert_eq!(whole.chars().count(), sum_len + 2 * (ranges.len() - 1));
    }

    #[test]
    fn match_offset_maps_to_correct_page_and_local_substring() {
        let paras = ["Q: find the needle", "hay hay hay", "here is a needle again"];
        let whole = paras.join("\n\n");
        let ranges = [(0usize, 1usize), (1usize, 3usize)]; // page 0 = para 0; page 1 = paras 1..3
        let spans = page_spans(&paras, &ranges);

        let matches = collect(&whole, "needle");
        assert_eq!(matches.len(), 2); // one on page 0, one on page 1

        // Match 0 (page 0), match 1 (page 1). For each, the page-local slice of
        // the page's own body text must equal "needle".
        let page_body = |pi: usize| -> String {
            let (ps, pe) = ranges[pi];
            paras[ps..pe].join("\n\n")
        };
        for (mi, expect_page) in [(0usize, 0usize), (1usize, 1usize)] {
            let (a, _b) = matches[mi];
            let pi = page_for_offset(&spans, a as usize);
            assert_eq!(pi, expect_page, "match {mi} should be on page {expect_page}");
            let (pstart, _) = spans[pi];
            let local = a as usize - pstart;
            let body = page_body(pi);
            let slice: String = body.chars().skip(local).take(6).collect();
            assert_eq!(slice, "needle");
        }
    }
}
