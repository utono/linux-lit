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
/// gloss" rule). A multi-block unit taller than a whole page is released into
/// singleton blocks — an orphaned block on the next page beats one clipped
/// invisibly past the viewport bottom (same degradation `paginate_note_blocks`
/// uses; a no-Source gloss glues EVERY paragraph to unit 0, which otherwise
/// rendered one over-tall clipped page). A single block taller than a page still
/// gets its own page (never dropped). `group_start[0]` is treated as true
/// regardless. Pure — unit-tested.
pub fn paginate_grouped(block_heights: &[i32], group_start: &[bool], page_height: i32) -> Vec<Page> {
    let n = block_heights.len();
    if n == 0 {
        return Vec::new();
    }
    let budget = page_height.max(1);
    // Unit boundaries: indices where a new unit begins (always includes 0).
    let mut unit_starts: Vec<usize> = (0..n)
        .filter(|&i| i == 0 || group_start.get(i).copied().unwrap_or(true))
        .collect();
    unit_starts.push(n); // sentinel end
    // Units as block ranges [start, end); an oversize multi-block unit splits
    // into singletons so its tail blocks can flow onto following pages.
    let mut units: Vec<(usize, usize)> = Vec::new();
    for w in unit_starts.windows(2) {
        let (s, e) = (w[0], w[1]);
        let h: i32 = block_heights[s..e].iter().sum();
        if h > budget && e - s > 1 {
            units.extend((s..e).map(|i| (i, i + 1)));
        } else {
            units.push((s, e));
        }
    }
    // Pack whole units; translate unit-page boundaries back to block indices.
    let mut pages: Vec<Page> = Vec::new();
    let mut start_unit = 0usize;
    let mut acc = 0i32;
    for (u, &(us, ue)) in units.iter().enumerate() {
        let h: i32 = block_heights[us..ue].iter().sum();
        if u > start_unit && acc + h > budget {
            pages.push(Page { start: units[start_unit].0, end: us });
            start_unit = u;
            acc = 0;
        }
        acc += h;
    }
    pages.push(Page { start: units[start_unit].0, end: n });
    pages
}

/// Paginate a Markdown NOTE's blocks with the **no-orphaned-heading rule**:
/// chrome blocks (headings / rules, `stoppable[i] == false`) glue FORWARD onto
/// the stoppable block that follows them, so a page never ends on a heading
/// whose content moved to the next page. Degrades gracefully when a glued
/// unit exceeds the page:
/// - first retry with just `[immediately-preceding chrome…, stoppable]` where
///   the leading chrome blocks are released as singletons (keeps a section
///   heading with its first paragraph while letting a long title header stay
///   behind);
/// - if even the final `[heading, block]` pair is oversize, fall back to fully
///   independent blocks (an orphan beats a clipped page).
///
/// All-stoppable input (no chrome) degenerates to plain `paginate`. Pure.
pub fn paginate_note_blocks(
    block_heights: &[i32],
    stoppable: &[bool],
    page_height: i32,
) -> Vec<Page> {
    let n = block_heights.len();
    if n == 0 {
        return Vec::new();
    }
    let budget = page_height.max(1);
    let is_stop = |i: usize| stoppable.get(i).copied().unwrap_or(true);

    // Build indivisible units as block ranges [start, end).
    let mut units: Vec<(usize, usize)> = Vec::new();
    let mut i = 0usize;
    while i < n {
        // Collect the chrome run (possibly empty) + its following stoppable.
        let run_start = i;
        while i < n && !is_stop(i) {
            i += 1;
        }
        if i >= n {
            // Trailing chrome with no content after it — singletons.
            for j in run_start..n {
                units.push((j, j + 1));
            }
            break;
        }
        let stop = i;
        i += 1;
        let group_h: i32 = block_heights[run_start..=stop].iter().sum();
        if group_h <= budget {
            units.push((run_start, stop + 1));
            continue;
        }
        // Oversize: release leading chrome as singletons, keep the LAST chrome
        // block (the section heading) glued to its content if that pair fits.
        let pair_start = stop.saturating_sub(1).max(run_start);
        let keep_pair = pair_start < stop
            && !is_stop(pair_start)
            && block_heights[pair_start..=stop].iter().sum::<i32>() <= budget;
        let single_end = if keep_pair { pair_start } else { stop };
        for j in run_start..single_end {
            units.push((j, j + 1));
        }
        units.push((single_end, stop + 1));
    }

    // Greedy-pack units into pages (an oversize unit gets its own page).
    let mut pages: Vec<Page> = Vec::new();
    let mut start_unit = 0usize;
    let mut acc = 0i32;
    for (u, &(us, ue)) in units.iter().enumerate() {
        let h: i32 = block_heights[us..ue].iter().sum();
        if u > start_unit && acc + h > budget {
            pages.push(Page { start: units[start_unit].0, end: us });
            start_unit = u;
            acc = 0;
        }
        acc += h;
    }
    pages.push(Page { start: units[start_unit].0, end: n });
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

/// The end-of-content marker glyph for a paginated overlay page, or `None` when
/// nothing should show. Floating bottom-center marker shared by gloss / synopsis
/// / journal:
/// - `⌄` (U+2304) when another page follows (`page_idx + 1 < n_pages`) — "more".
/// - `•` (U+2022) on the LAST page of paginated content — "end / no more pages".
/// - `None` for single-page (or empty) content — no marker at all.
pub fn page_marker(page_idx: usize, n_pages: usize) -> Option<&'static str> {
    if n_pages <= 1 {
        None
    } else if page_idx + 1 < n_pages {
        Some("\u{2304}")
    } else {
        Some("\u{2022}")
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
    // Trailing comma (see `ui::font_string`): parsed bare, a family whose last
    // word is a Pango style keyword ("Gentium Book") loses that word and
    // measures in a FALLBACK face, silently diverging from what the view
    // renders — under-filling or over-packing every page.
    let mut desc = pango::FontDescription::from_string(&format!("{},", family.trim()));
    desc.set_size(size_pt * pango::SCALE);
    layout.set_font_description(Some(&desc));
    layout.set_width(width_px.max(1) * pango::SCALE);
    layout.set_wrap(pango::WrapMode::WordChar);
    layout.set_text(text);
    layout.pixel_size().1
}

/// `measure_text_height` for a view that renders with
/// `set_pixels_inside_wrap(ui::OVERLAY_LINE_LEADING)` (the gloss/journal
/// reading views): adds the same per-line leading via pango layout spacing so
/// the measured wrap height matches the render. Do NOT use for surfaces
/// without the leading (the translation overlay) — they would over-count.
pub fn measure_text_height_leaded(
    pctx: &pango::Context,
    text: &str,
    size_pt: i32,
    family: &str,
    width_px: i32,
) -> i32 {
    let layout = pango::Layout::new(pctx);
    // Trailing comma (see `ui::font_string`): parsed bare, a family whose last
    // word is a Pango style keyword ("Gentium Book") loses that word and
    // measures in a FALLBACK face, silently diverging from what the view
    // renders — under-filling or over-packing every page.
    let mut desc = pango::FontDescription::from_string(&format!("{},", family.trim()));
    desc.set_size(size_pt * pango::SCALE);
    layout.set_font_description(Some(&desc));
    layout.set_width(width_px.max(1) * pango::SCALE);
    layout.set_wrap(pango::WrapMode::WordChar);
    layout.set_spacing(crate::ui::OVERLAY_LINE_LEADING * pango::SCALE);
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
    fn paginate_grouped_oversize_multiblock_unit_splits() {
        // A multi-block unit taller than a page (blocks 1+2 = 120 > 100) is
        // released into singleton blocks — an orphaned block on the next page
        // beats one clipped invisibly past the viewport bottom.
        let h = vec![40, 70, 50];
        let starts = vec![true, true, false];
        let pages = paginate_grouped(&h, &starts, 100);
        assert_eq!(pages, vec![
            Page { start: 0, end: 1 },
            Page { start: 1, end: 2 },
            Page { start: 2, end: 3 },
        ]);
    }

    #[test]
    fn paginate_grouped_all_blocks_one_oversize_unit_paginates() {
        // A gloss with NO Source blocks (e.g. a prose gloss of only <gloss>
        // paragraphs) has group_start all-false: every block glues to unit 0.
        // When that single unit exceeds the page it must split into per-block
        // pages, not render one over-tall clipped page (gloss 21776).
        let h = vec![30, 30, 30, 30, 30, 30, 30];
        let starts = vec![false; 7];
        let pages = paginate_grouped(&h, &starts, 100);
        assert_eq!(pages, paginate(&h, 100));
    }

    #[test]
    fn paginate_grouped_oversize_single_block_keeps_own_page() {
        // A SINGLE block taller than a page cannot be split further; it still
        // gets its own page (never dropped), same as plain paginate.
        let h = vec![40, 250, 50];
        let starts = vec![true, true, false];
        let pages = paginate_grouped(&h, &starts, 100);
        assert_eq!(pages, vec![
            Page { start: 0, end: 1 },
            Page { start: 1, end: 2 },
            Page { start: 2, end: 3 },
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
    fn note_heading_travels_with_its_paragraph() {
        // [para, H2, para]: page can hold 2 blocks of 100 but the H2 (40) +
        // para (100) exceed the remaining space after block 0 — the H2 must
        // MOVE to page 2 with its paragraph, never end page 1 orphaned.
        let h = [100, 40, 100];
        let stop = [true, false, true];
        let pages = paginate_note_blocks(&h, &stop, 180);
        assert_eq!(pages, vec![Page { start: 0, end: 1 }, Page { start: 1, end: 3 }]);
    }

    #[test]
    fn note_oversize_title_run_releases_leading_chrome() {
        // [H1, H3, Rule, H2, big-para] where the whole glued run exceeds the
        // page: leading chrome (H1/H3/Rule) become singletons on page 1, the
        // final [H2, para] pair stays glued on page 2.
        let h = [46, 81, 59, 64, 290];
        let stop = [false, false, false, false, true];
        let pages = paginate_note_blocks(&h, &stop, 486);
        assert_eq!(pages, vec![Page { start: 0, end: 3 }, Page { start: 3, end: 5 }]);
    }

    #[test]
    fn note_all_stoppable_matches_plain_paginate() {
        let h = [30, 30, 30, 30, 30];
        let stop = [true; 5];
        assert_eq!(paginate_note_blocks(&h, &stop, 100), paginate(&h, 100));
    }

    #[test]
    fn note_oversize_pair_falls_back_to_orphan_not_clip() {
        // Heading + para together exceed the page: fall back to independent
        // blocks (orphan allowed) rather than an overflowing glued unit.
        let h = [40, 200];
        let stop = [false, true];
        let pages = paginate_note_blocks(&h, &stop, 210);
        assert_eq!(pages, vec![Page { start: 0, end: 1 }, Page { start: 1, end: 2 }]);
    }

    #[test]
    fn note_trailing_chrome_is_kept() {
        // A rule at the very end (no content after) still renders on a page.
        let h = [100, 30];
        let stop = [true, false];
        let pages = paginate_note_blocks(&h, &stop, 200);
        assert_eq!(pages, vec![Page { start: 0, end: 2 }]);
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

    #[test]
    fn page_marker_none_for_single_or_empty() {
        assert_eq!(page_marker(0, 0), None);
        assert_eq!(page_marker(0, 1), None);
    }

    #[test]
    fn page_marker_chevron_then_bullet() {
        // 3 pages: pages 0,1 show the "more" chevron; page 2 (last) shows the bullet.
        assert_eq!(page_marker(0, 3), Some("\u{2304}"));
        assert_eq!(page_marker(1, 3), Some("\u{2304}"));
        assert_eq!(page_marker(2, 3), Some("\u{2022}"));
    }
}
