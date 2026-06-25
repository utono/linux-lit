pub mod authorship_picker;
pub mod action_popup;
pub mod ask_card;
pub mod footer;
pub mod concordance_bar;
pub mod concordance_list_picker;
pub mod concordance_works_picker;
pub mod concordance_picker;
pub mod concordance_word_picker;
pub mod gloss_block;
pub mod gloss_ipa;
pub mod gloss_overlay;
pub(crate) mod gloss_render;
pub mod gloss_util;
pub mod journal_block;
pub mod journal_overlay;
pub mod journal_picker;
pub mod gloss_picker;
pub mod echo_picker;
pub mod echo_line_picker;
pub mod echo_turns_picker;
pub mod echo_keybinds_overlay;
pub mod vocab_popup;
pub mod gamepad_overlay;
pub mod keybinds_overlay;
pub mod library_picker;
pub mod bookmark_picker;
pub mod media_picker;
pub mod page_image_overlay;
pub mod picker_attach;
pub mod picker_filter;
pub mod picker_nav;
pub mod search_bar;
pub mod settings_overlay;
pub mod toast;
pub mod translation_overlay;
pub mod voice_picker;

/// The side margin (left and right) for the full-screen gloss / synopsis / ask
/// cards: a quarter of the *live* card width, which keeps the prose near the
/// ~65-char readability optimum on a wide (~1660px) card.
///
/// CRITICAL: this is anchored to the on-screen `card_width`, NOT the fixed
/// `column_width`. The echo view deliberately uses `column_width / 8` instead
/// (a different value and concept); do NOT route those sites through here —
/// conflating the two reintroduces the "tiny margin / edge-to-edge text on a
/// wide card" bug. See `gloss_overlay::show_gloss_with_color` and audit #27.
pub(crate) fn card_side_margin(card_width: i32) -> i32 {
    card_width / 4
}

/// Re-assert the italic verse tags (`gloss-stage`, `gloss-bracket`) to the top
/// of `table`'s priority order. An overlay's buffer-wide font tag is built with
/// `.font("Family Size")`, whose Pango description carries a regular (upright)
/// STYLE attribute; added last, it would override the italic tags by add-order
/// priority and flatten stage/bracket directions to upright. Call this AFTER
/// applying the font tag in each overlay's `apply_font`. The gloss and journal
/// overlays both render verse via `gloss_render::populate_verse_buffer`, so both
/// own these tag names and must stay in sync — hence one shared helper.
pub(crate) fn reassert_italic_tags(table: &gtk4::TextTagTable) {
    use gtk4::prelude::*;
    let top = table.size();
    for italic in ["gloss-stage", "gloss-bracket"] {
        if let Some(t) = table.lookup(italic) {
            if top > 0 {
                t.set_priority(top - 1);
            }
        }
    }
}

/// Pure core of the overlay bottom-clip calculation: given each visual row's
/// `(top, bottom)` in scroll-coordinate space, the viewport top (`top_y`), the
/// viewport height (`viewport_h`), and the total content height (`content_h`),
/// return the clip-box height that hides any partial last row straddling the
/// viewport bottom — INCLUDING its descenders, because the rows carry real
/// per-row heights (a `line_yrange`/uniform-row estimate clips the wrong amount
/// and cuts descenders; see docs/troubleshooting/page-turning-mechanics.md).
///
/// Clips from the bottom of the last row that fits ENTIRELY above the viewport
/// bottom down to the viewport bottom. Two guards: if the document ends inside
/// the viewport, cover only the slack below `content_h`; if a single row is
/// taller than the viewport (nothing fits), return 0 so that row isn't blanked.
pub(crate) fn bottom_clip_height(
    rows: &[(f64, f64)],
    top_y: f64,
    viewport_h: f64,
    content_h: f64,
) -> i32 {
    if viewport_h <= 0.0 {
        return 0;
    }
    let bottom_y = top_y + viewport_h;
    let mut last_full_bottom = top_y;
    let mut any_full = false;
    for (row_top, row_bottom) in rows {
        if *row_bottom <= bottom_y + 0.5 && *row_bottom > top_y {
            last_full_bottom = *row_bottom;
            any_full = true;
        }
        if *row_top >= bottom_y {
            break;
        }
    }
    let effective_bottom = if content_h <= bottom_y + 0.5 {
        content_h
    } else {
        last_full_bottom
    };
    if !any_full && content_h > bottom_y + 0.5 {
        0
    } else {
        (bottom_y - effective_bottom).max(0.0).round() as i32
    }
}

/// Yield `(row_top, row_bottom)` for each visual (wrapped) row of `view`'s
/// buffer, in vadjustment / scroll-coordinate space (top_margin included so the
/// values compare against `adj.value()`). Steps `forward_display_line` and reads
/// each row's real rect via `iter_location` — so wrapped paragraphs contribute
/// one entry per visual row at its TRUE height (`line_yrange` would collapse them
/// and a uniform-row estimate clips descenders; see the gloss overlay and
/// docs/troubleshooting/page-turning-mechanics.md).
pub(crate) fn display_rows(view: &gtk4::TextView) -> Vec<(f64, f64)> {
    use gtk4::prelude::*;
    let mut rows: Vec<(f64, f64)> = Vec::new();
    let top_margin = view.top_margin() as f64;
    let buffer = view.buffer();
    let mut iter = buffer.start_iter();
    let end = buffer.end_iter();
    for _ in 0..8192 {
        let rect = view.iter_location(&iter);
        if rect.height() > 0 {
            let top = rect.y() as f64 + top_margin;
            rows.push((top, top + rect.height() as f64));
        }
        if iter == end || !view.forward_display_line(&mut iter) {
            break;
        }
    }
    rows
}

/// Logical-line `(row_top, row_bottom)` pairs in vadjustment/scroll coordinate
/// space, from the line at `top_val` down to the first line whose top reaches
/// `top_val + viewport_h`. The logical-line analog of `display_rows` (which
/// walks visual/wrapped rows): scroll-mode and the translation-follow path size
/// their bottom clip from whole-line `line_yrange` geometry, NOT wrapped rows.
/// Feed the result to `bottom_clip_height` so scroll-mode shares the overlays'
/// single covering algorithm instead of re-implementing it.
pub(crate) fn line_yrange_rows(
    view: &gtk4::TextView,
    top_val: f64,
    viewport_h: f64,
) -> Vec<(f64, f64)> {
    use gtk4::prelude::*;
    let bottom_y = top_val + viewport_h;
    let mut rows: Vec<(f64, f64)> = Vec::new();
    let (mut iter, _) = view.line_at_y(top_val.max(0.0) as i32);
    loop {
        let (ly, lh) = view.line_yrange(&iter);
        let row_top = ly as f64;
        let row_bottom = (ly + lh) as f64;
        if row_top >= bottom_y {
            break;
        }
        rows.push((row_top, row_bottom));
        if !iter.forward_line() {
            break;
        }
    }
    rows
}

/// Set `clip`'s height to hide any partial last row straddling the bottom of
/// `scrolled`'s viewport in `view` — the descender-correct bottom clip both the
/// gloss and journal overlays use. Reads real row geometry via `display_rows`
/// and the pure `bottom_clip_height`.
pub(crate) fn recompute_overlay_bottom_clip(
    view: &gtk4::TextView,
    clip: &gtk4::Box,
    scrolled: &gtk4::ScrolledWindow,
) {
    use gtk4::prelude::*;
    let adj = scrolled.vadjustment();
    let viewport_h = adj.page_size();
    let clip_h = bottom_clip_height(&display_rows(view), adj.value(), viewport_h, adj.upper());
    if clip.height_request() != clip_h {
        clip.set_height_request(clip_h);
    }
}

#[cfg(test)]
mod bottom_clip_tests {
    use super::bottom_clip_height;

    #[test]
    fn clips_partial_last_row_with_descenders_to_last_full_row() {
        // 4 rows of height 20 (tops 0,20,40,60). Viewport is 70 tall starting at
        // 0, so rows 0..=2 fit (bottoms 20,40,60 <= 70) but row 3 (60..80)
        // straddles the bottom — its descenders would poke under the footer.
        // The clip must cover from the last FULL row bottom (60) to the viewport
        // bottom (70) = 10px, hiding the partial row.
        let rows = [(0.0, 20.0), (20.0, 40.0), (40.0, 60.0), (60.0, 80.0)];
        assert_eq!(bottom_clip_height(&rows, 0.0, 70.0, 80.0), 10);
    }

    #[test]
    fn no_clip_when_document_ends_inside_viewport() {
        // 2 rows totalling 40px content, viewport 100 tall — only 60px of slack
        // below the content, no partial row. Clip covers the slack (100-40=60).
        let rows = [(0.0, 20.0), (20.0, 40.0)];
        assert_eq!(bottom_clip_height(&rows, 0.0, 100.0, 40.0), 60);
    }

    #[test]
    fn no_clip_when_rows_land_exactly_on_viewport_bottom() {
        // 3 rows of 20 = 60px, viewport 60 — last row bottom == viewport bottom,
        // no partial row, content fits exactly. Clip 0.
        let rows = [(0.0, 20.0), (20.0, 40.0), (40.0, 60.0)];
        assert_eq!(bottom_clip_height(&rows, 0.0, 60.0, 60.0), 0);
    }

    #[test]
    fn single_row_taller_than_viewport_is_not_blanked() {
        // One 100px row, 50px viewport: nothing fits entirely. Guard returns 0
        // so the row stays visible rather than being fully clipped.
        let rows = [(0.0, 100.0)];
        assert_eq!(bottom_clip_height(&rows, 0.0, 50.0, 100.0), 0);
    }

    #[test]
    fn scrolled_viewport_uses_top_y() {
        // Scrolled down: top_y=30, viewport 50 -> bottom 80. Rows at 30,50,70,90.
        // Rows with bottom 50,70 fit; row 70..90 straddles 80. Last full bottom
        // 70, clip = 80-70 = 10.
        let rows = [(30.0, 50.0), (50.0, 70.0), (70.0, 90.0)];
        assert_eq!(bottom_clip_height(&rows, 30.0, 50.0, 90.0), 10);
    }

    #[test]
    fn nonuniform_rows_clip_correctly_where_uniform_step_would_cut_descenders() {
        // THE BUG CASE. Real journal prose has non-uniform rows: a tall title/
        // first row + a paragraph gap, then 18px body rows. Here the first row is
        // 30px (0..30), then body rows of 18 (30..48, 48..66, 66..84). Viewport is
        // 70 tall from y=0 -> bottom 70. Rows fully fitting: bottoms 30,48,66 (all
        // <= 70); row 66..84 straddles 70 and its DESCENDERS sit below the clip.
        // Correct clip = 70 - 66 = 4 (cover the partial row).
        //
        // The OLD buggy journal math used a UNIFORM step from the FIRST row
        // (30px): remainder = 70 - floor(70/30)*30 = 70 - 2*30 = 10. That 10px
        // clip starts at y=60, slicing into the LAST FULL row (66 bottom) — i.e.
        // it clips 6px too much OR (depending on step) leaves the partial row's
        // descenders showing. Either way the uniform estimate != 4. This test
        // pins the real (per-row) answer.
        let rows = [(0.0, 30.0), (30.0, 48.0), (48.0, 66.0), (66.0, 84.0)];
        assert_eq!(bottom_clip_height(&rows, 0.0, 70.0, 84.0), 4);

        // Demonstrate the divergence explicitly: the old uniform-step formula
        // (first-row height as the step) gives a different, wrong value.
        let step = 30.0_f64; // first row height, what row_step() returned
        let page = 70.0_f64;
        let uniform_remainder = (page - (page / step).floor() * step).round() as i32;
        assert_ne!(
            uniform_remainder, 4,
            "uniform-step estimate must differ from the correct per-row clip"
        );
    }
}
