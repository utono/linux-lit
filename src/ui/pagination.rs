//! Shared pagination helpers for the free-prose overlays (translation, journal).
//!
//! The robust answer to bottom/top clipping on a wrapping-text surface is to
//! PAGINATE — render only the whole blocks that fit, so no partial row is ever
//! rendered at either edge (see `docs/troubleshooting/clip-prevention.md` →
//! "Pagination instead of a mask"). These helpers are pure (and `pango`-only for
//! measurement — no widget allocation, so no GTK settle race), shared so the two
//! overlays use one proven implementation.

use gtk4::pango;

/// A contiguous run of blocks `[start, end)` that fit on one page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub start: usize,
    pub end: usize,
}

/// Greedily pack consecutive blocks into pages: a page accumulates blocks until
/// the next would exceed `page_height`. A block taller than a whole page gets a
/// page to itself (never dropped). `block_heights[i]` is block i's rendered
/// height. Pure — unit-tested.
pub fn paginate(block_heights: &[i32], page_height: i32) -> Vec<Page> {
    let mut pages: Vec<Page> = Vec::new();
    let mut start = 0usize;
    let mut acc = 0i32;
    let budget = page_height.max(1);
    for (i, &h) in block_heights.iter().enumerate() {
        // Would adding this block overflow a non-empty page? Close the page first.
        if i > start && acc + h > budget {
            pages.push(Page { start, end: i });
            start = i;
            acc = 0;
        }
        acc += h;
    }
    if start < block_heights.len() {
        pages.push(Page { start, end: block_heights.len() });
    }
    pages
}

/// Like `paginate`, but blocks are grouped into indivisible UNITS that must not
/// be split across a page. `group_start[i] == true` marks block i as the first
/// block of a new unit (the rest attach to the preceding unit). A page break can
/// only fall on a unit boundary, so e.g. a gloss's Source (speaker+verse) block
/// and the Explication that follows it stay on the same page (the "don't orphan a
/// gloss" rule). A unit taller than a whole page still gets its own page (never
/// dropped). `group_start[0]` is treated as true regardless. Pure — unit-tested.
pub fn paginate_grouped(block_heights: &[i32], group_start: &[bool], page_height: i32) -> Vec<Page> {
    let n = block_heights.len();
    if n == 0 {
        return Vec::new();
    }
    // Unit boundaries: indices where a new unit begins (always includes 0).
    let mut unit_starts: Vec<usize> = (0..n)
        .filter(|&i| i == 0 || group_start.get(i).copied().unwrap_or(true))
        .collect();
    unit_starts.push(n); // sentinel end
    // Sum each unit's height.
    let unit_h: Vec<i32> = unit_starts
        .windows(2)
        .map(|w| block_heights[w[0]..w[1]].iter().sum())
        .collect();
    // Pack whole units; translate unit-page boundaries back to block indices.
    let mut pages: Vec<Page> = Vec::new();
    let mut start_unit = 0usize;
    let mut acc = 0i32;
    let budget = page_height.max(1);
    for (u, &h) in unit_h.iter().enumerate() {
        if u > start_unit && acc + h > budget {
            pages.push(Page { start: unit_starts[start_unit], end: unit_starts[u] });
            start_unit = u;
            acc = 0;
        }
        acc += h;
    }
    pages.push(Page { start: unit_starts[start_unit], end: n });
    pages
}

/// The page index whose `[start, end)` range contains `block_idx`. Clamps to the
/// last page if `block_idx` is past the end; returns 0 when there are no pages.
pub fn page_containing_block(pages: &[Page], block_idx: usize) -> usize {
    for (i, p) in pages.iter().enumerate() {
        if block_idx >= p.start && block_idx < p.end {
            return i;
        }
    }
    pages.len().saturating_sub(1)
}

/// Footer page counter shared by every paginated overlay (gloss, synopsis,
/// journal). Returns `Some("<current> / <total>")` (1-based, `page_idx` clamped
/// into range) when there is more than one render page, and `None` for 0 or 1
/// page (nothing to count). Callers add their own prefix if they want one
/// (gloss/synopsis show the bare token; journal prefixes "page ").
///
/// Centralizing this is deliberate: the per-overlay copies of this computation
/// drifted once (one overlay lost the token while another kept it), so it lives
/// in one place to keep the three footers in sync.
pub fn page_token(page_idx: usize, n_pages: usize) -> Option<String> {
    if n_pages > 1 {
        let current = page_idx.min(n_pages - 1) + 1;
        Some(format!("{} / {}", current, n_pages))
    } else {
        None
    }
}

/// Pixel height of `text` wrapped at `width_px` in `family` at `size_pt`, via a
/// `pango::Layout` on `pctx` (a widget's pango context). Used only for page-fit
/// math — no widget allocation, so no GTK settle race.
pub fn measure_text_height(
    pctx: &pango::Context,
    text: &str,
    size_pt: i32,
    family: &str,
    width_px: i32,
) -> i32 {
    let layout = pango::Layout::new(pctx);
    let mut desc = pango::FontDescription::from_string(family);
    desc.set_size(size_pt * pango::SCALE);
    layout.set_font_description(Some(&desc));
    layout.set_width(width_px.max(1) * pango::SCALE);
    layout.set_wrap(pango::WrapMode::WordChar);
    layout.set_text(text);
    layout.pixel_size().1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginate_packs_until_full() {
        // heights 30 each, page 100 -> 3 per page.
        let h = vec![30, 30, 30, 30, 30, 30, 30];
        let pages = paginate(&h, 100);
        assert_eq!(pages, vec![
            Page { start: 0, end: 3 },
            Page { start: 3, end: 6 },
            Page { start: 6, end: 7 },
        ]);
    }

    #[test]
    fn paginate_grouped_never_splits_a_unit() {
        // 4 blocks in 2 units: [0,1] and [2,3] (each unit 60 tall). Page budget
        // 100 fits one unit but not two, so the break falls on the unit boundary
        // (block 2), NOT between block 0 and 1 — the "don't orphan a gloss" rule.
        let h = vec![30, 30, 30, 30];
        let starts = vec![true, false, true, false];
        let pages = paginate_grouped(&h, &starts, 100);
        assert_eq!(pages, vec![
            Page { start: 0, end: 2 },
            Page { start: 2, end: 4 },
        ]);
    }

    #[test]
    fn paginate_grouped_oversize_unit_gets_own_page() {
        // A unit taller than a page (blocks 1+2 = 120 > 100) still gets its own
        // page, never dropped or split.
        let h = vec![40, 70, 50];
        let starts = vec![true, true, false];
        let pages = paginate_grouped(&h, &starts, 100);
        assert_eq!(pages, vec![
            Page { start: 0, end: 1 },
            Page { start: 1, end: 3 },
        ]);
    }

    #[test]
    fn paginate_grouped_matches_paginate_when_all_units_singleton() {
        // Every block its own unit -> identical to plain paginate.
        let h = vec![30, 30, 30, 30, 30, 30, 30];
        let starts = vec![true; 7];
        assert_eq!(paginate_grouped(&h, &starts, 100), paginate(&h, 100));
    }

    #[test]
    fn paginate_over_tall_block_alone_on_page() {
        // A 250-tall block can't fit a 100 page; it gets its own page, never dropped.
        let h = vec![30, 250, 30];
        let pages = paginate(&h, 100);
        assert_eq!(pages, vec![
            Page { start: 0, end: 1 },
            Page { start: 1, end: 2 },
            Page { start: 2, end: 3 },
        ]);
    }

    #[test]
    fn paginate_exact_fit_boundary() {
        // 50 + 50 == 100 fits one page; the third 50 starts a new page.
        let h = vec![50, 50, 50];
        let pages = paginate(&h, 100);
        assert_eq!(pages, vec![
            Page { start: 0, end: 2 },
            Page { start: 2, end: 3 },
        ]);
    }

    #[test]
    fn paginate_empty() {
        assert!(paginate(&[], 100).is_empty());
    }

    #[test]
    fn page_containing_block_finds_and_clamps() {
        let pages = vec![Page { start: 0, end: 3 }, Page { start: 3, end: 6 }];
        assert_eq!(page_containing_block(&pages, 0), 0);
        assert_eq!(page_containing_block(&pages, 2), 0);
        assert_eq!(page_containing_block(&pages, 3), 1);
        assert_eq!(page_containing_block(&pages, 5), 1);
        // Past the end clamps to the last page.
        assert_eq!(page_containing_block(&pages, 99), 1);
        // No pages -> 0.
        assert_eq!(page_containing_block(&[], 0), 0);
    }

    #[test]
    fn page_token_hidden_for_zero_or_one_page() {
        assert_eq!(page_token(0, 0), None);
        assert_eq!(page_token(0, 1), None);
    }

    #[test]
    fn page_token_counts_one_based() {
        assert_eq!(page_token(0, 2).as_deref(), Some("1 / 2"));
        assert_eq!(page_token(1, 2).as_deref(), Some("2 / 2"));
        assert_eq!(page_token(2, 5).as_deref(), Some("3 / 5"));
    }

    #[test]
    fn page_token_clamps_out_of_range_index() {
        // A stale page_idx past the last page clamps to the final page.
        assert_eq!(page_token(9, 3).as_deref(), Some("3 / 3"));
    }
}
