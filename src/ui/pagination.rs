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
}
