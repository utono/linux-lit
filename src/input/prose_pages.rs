//! Pure prose page-table types + invariant suite (GTK-free, unit-testable).
//! A prose page boundary is (buffer_line, row_offset_px); offsets are pixel
//! offsets from the buffer line's top, snapped to visual-row tops by the
//! GTK-bound generator (snapping itself is not re-checkable here).
//! `end` is EXCLUSIVE and must equal the next page's `start` exactly —
//! zero gaps, zero overlaps: the machine-checked no-text-loss guarantee.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProsePage {
    pub start_line: usize,
    pub start_off: i32,
    pub end_line: usize,
    pub end_off: i32,
}

pub struct ProseValidateCtx<'a> {
    pub line_count: usize,
    /// Per-buffer-line pixel heights (line_yrange), at generation layout.
    pub heights: &'a [i32],
    /// widget_height - descender_guard - BASE_BOTTOM_MARGIN at that layout.
    pub usable_height: i32,
}

/// Lexicographic order on (line, off).
fn pos_le(al: usize, ao: i32, bl: usize, bo: i32) -> bool {
    (al, ao) <= (bl, bo)
}

/// Pixel height of the half-open interval [start, end) given per-line heights.
fn page_px(p: &ProsePage, heights: &[i32]) -> i64 {
    let mut px: i64 = 0;
    for l in p.start_line..=p.end_line.min(heights.len().saturating_sub(1)) {
        px += heights[l] as i64;
    }
    px - p.start_off as i64 - (heights[p.end_line.min(heights.len() - 1)] - p.end_off) as i64
}

/// Invariant suite (design doc §2). Returns the FIRST violation as
/// "<name>: <details>".
pub fn validate_prose_pages(
    pages: &[ProsePage],
    ctx: &ProseValidateCtx,
) -> Result<(), String> {
    if pages.is_empty() {
        return Err("coverage: no pages".into());
    }
    if ctx.line_count == 0 || ctx.heights.len() < ctx.line_count {
        return Err("sanity: bad ctx".into());
    }
    let first = &pages[0];
    if first.start_line != 0 || first.start_off != 0 {
        return Err(format!(
            "coverage: first page starts at ({}, {}) not (0, 0)",
            first.start_line, first.start_off
        ));
    }
    for (i, p) in pages.iter().enumerate() {
        // sanity: offsets inside their lines, positions ordered.
        if p.start_line >= ctx.line_count || p.end_line >= ctx.line_count {
            return Err(format!("sanity: page {} line out of range", i + 1));
        }
        if p.start_off < 0 || p.start_off >= ctx.heights[p.start_line].max(1) {
            return Err(format!(
                "sanity: page {} start_off {} outside line {} height {}",
                i + 1, p.start_off, p.start_line, ctx.heights[p.start_line]
            ));
        }
        if p.end_off <= 0 && !(p.end_off == 0 && p.end_line > p.start_line) {
            return Err(format!("sanity: page {} end_off {}", i + 1, p.end_off));
        }
        if p.end_off > ctx.heights[p.end_line] {
            return Err(format!(
                "sanity: page {} end_off {} > line {} height {}",
                i + 1, p.end_off, p.end_line, ctx.heights[p.end_line]
            ));
        }
        if !pos_le(p.start_line, p.start_off + 1, p.end_line, p.end_off) {
            return Err(format!("ordering: page {} end not after start", i + 1));
        }
        // adjacency: exclusive end == next start. THE no-text-loss rule.
        if let Some(n) = pages.get(i + 1) {
            let matches_next = (p.end_line == n.start_line && p.end_off == n.start_off)
                // A boundary exactly at a line's full height is the same
                // position as the next line's top (normalized form).
                || (p.end_off == ctx.heights[p.end_line]
                    && n.start_line == p.end_line + 1
                    && n.start_off == 0);
            if !matches_next {
                return Err(format!(
                    "coverage: page {} ends at ({}, {}) but page {} starts at ({}, {})",
                    i + 1, p.end_line, p.end_off, i + 2, n.start_line, n.start_off
                ));
            }
        }
        // fit: the page's pixel height must fit the viewport.
        let px = page_px(p, ctx.heights);
        if px > ctx.usable_height as i64 {
            return Err(format!(
                "fit: page {} spans {}px > usable {}",
                i + 1, px, ctx.usable_height
            ));
        }
        if px <= 0 {
            return Err(format!("fit: page {} spans {}px (empty/negative)", i + 1, px));
        }
    }
    // tail: last page must reach the document's pixel end.
    let last = pages.last().unwrap();
    let last_line = ctx.line_count - 1;
    if !(last.end_line == last_line && last.end_off == ctx.heights[last_line]) {
        return Err(format!(
            "tail: last page ends at ({}, {}) not ({}, {})",
            last.end_line, last.end_off, last_line, ctx.heights[last_line]
        ));
    }
    Ok(())
}

/// Page containing position (line, off). A position exactly at a page's start
/// resolves to THAT page (page tops are canonical — same convention as
/// play `page_for_line`). Adjacency is exact, so there is no overlap case.
pub fn prose_page_for_position(
    pages: &[ProsePage],
    line: usize,
    off: i32,
) -> Option<usize> {
    let idx = pages.partition_point(|p| pos_le(p.start_line, p.start_off, line, off));
    if idx == 0 {
        return None;
    }
    let i = idx - 1;
    let p = &pages[i];
    // Inside [start, end)?
    let before_end = (line, off) < (p.end_line, p.end_off)
        || (p.end_off == 0 && line < p.end_line); // normalized-end form
    before_end.then_some(i)
}

/// Page whose interval contains buffer line `line`'s FIRST row (off = 0).
/// The design's "a line maps to the page containing its first row" rule.
pub fn prose_page_for_line(pages: &[ProsePage], line: usize) -> Option<usize> {
    prose_page_for_position(pages, line, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 4 paragraphs, heights 100/250/40/60, usable 120. Paragraph 1 (250px)
    // straddles three boundaries. Pages tile the pixel space exactly.
    fn heights() -> Vec<i32> { vec![100, 250, 40, 60] }

    fn ok_pages() -> Vec<ProsePage> {
        vec![
            ProsePage { start_line: 0, start_off: 0,   end_line: 1, end_off: 20 },
            ProsePage { start_line: 1, start_off: 20,  end_line: 1, end_off: 140 },
            ProsePage { start_line: 1, start_off: 140, end_line: 2, end_off: 10 },
            ProsePage { start_line: 2, start_off: 10,  end_line: 3, end_off: 60 },
        ]
    }

    fn ctx(h: &[i32]) -> ProseValidateCtx<'_> {
        ProseValidateCtx { line_count: h.len(), heights: h, usable_height: 120 }
    }

    #[test]
    fn valid_pages_pass() {
        let h = heights();
        assert_eq!(validate_prose_pages(&ok_pages(), &ctx(&h)), Ok(()));
    }

    #[test]
    fn gap_between_pages_fails_coverage() {
        let h = heights();
        let mut p = ok_pages();
        p[2].start_off = 150; // 10px of paragraph 1's rows on no page
        let err = validate_prose_pages(&p, &ctx(&h)).unwrap_err();
        assert!(err.contains("coverage"), "got: {err}");
    }

    #[test]
    fn overlap_fails_coverage() {
        let h = heights();
        let mut p = ok_pages();
        p[2].start_off = 130; // re-shows 10px already on page 2
        let err = validate_prose_pages(&p, &ctx(&h)).unwrap_err();
        assert!(err.contains("coverage"), "got: {err}");
    }

    #[test]
    fn overfull_page_fails_fit() {
        let h = heights();
        let mut p = ok_pages();
        p[0].end_off = 40; // page 1 = 100 + 40 = 140px > 120
        // keep adjacency so ONLY fit fails
        p[1].start_off = 40;
        let err = validate_prose_pages(&p, &ctx(&h)).unwrap_err();
        assert!(err.contains("fit"), "got: {err}");
    }

    #[test]
    fn short_tail_fails() {
        let h = heights();
        let p = &ok_pages()[..3];
        let err = validate_prose_pages(p, &ctx(&h)).unwrap_err();
        assert!(err.contains("tail"), "got: {err}");
    }

    #[test]
    fn first_page_must_start_at_origin() {
        let h = heights();
        let mut p = ok_pages();
        p[0].start_off = 5;
        let err = validate_prose_pages(&p, &ctx(&h)).unwrap_err();
        assert!(err.contains("coverage"), "got: {err}");
    }

    #[test]
    fn position_lookup_resolves_pages_and_tops() {
        let p = ok_pages();
        assert_eq!(prose_page_for_position(&p, 0, 0), Some(0));
        assert_eq!(prose_page_for_position(&p, 1, 19), Some(0));
        assert_eq!(prose_page_for_position(&p, 1, 20), Some(1), "page top is canonical");
        assert_eq!(prose_page_for_position(&p, 1, 200), Some(2));
        assert_eq!(prose_page_for_position(&p, 3, 59), Some(3));
        assert_eq!(prose_page_for_position(&p, 3, 60), None, "past document end");
        // line -> page containing its FIRST row
        assert_eq!(prose_page_for_line(&p, 1), Some(0));
        assert_eq!(prose_page_for_line(&p, 2), Some(2));
    }

    #[test]
    fn normalized_full_height_end_matches_next_line_top() {
        // Page ends at exactly line 0's full height; next starts at (1, 0).
        let h = vec![100, 100];
        let p = vec![
            ProsePage { start_line: 0, start_off: 0, end_line: 0, end_off: 100 },
            ProsePage { start_line: 1, start_off: 0, end_line: 1, end_off: 100 },
        ];
        let c = ProseValidateCtx { line_count: 2, heights: &h, usable_height: 120 };
        assert_eq!(validate_prose_pages(&p, &c), Ok(()));
    }
}
