use super::{AppState, TWO_COLUMN_WIDTH_FRACTION, MIN_TWO_COLUMN_COLUMN_WIDTH, SHOW_LINE_NUMBERS_TWO_COL};
use gtk4::prelude::*;

/// (renderer width, trailing margin past the number, gap between text and
/// number) for the right-side line-number gutter. Two-column mode uses a
/// tighter text↔number gap (more room for the verse line) but keeps real
/// padding past the number so it doesn't crowd the column/card edge.
pub(crate) fn line_number_gutter_geometry(column_count: u8) -> (i32, i32, i32) {
    if column_count >= 2 {
        (
            crate::gutter::LINE_NUMBER_WIDTH_TWO_COL,
            crate::gutter::LINE_NUMBER_MARGIN_END_TWO_COL,
            crate::gutter::LINE_NUMBER_TEXT_GAP_TWO_COL,
        )
    } else {
        (
            crate::gutter::LINE_NUMBER_WIDTH,
            crate::gutter::LINE_NUMBER_MARGIN_END,
            crate::gutter::LINE_NUMBER_MARGIN_END,
        )
    }
}

pub fn verse_left_offset(window_width: i32, column_width: u32) -> i32 {
    let card_w = (column_width as i32).min(window_width.max(1));
    let slack = window_width - card_w;
    if slack >= 2 * super::VERSE_LEFT_OFFSET { super::VERSE_LEFT_OFFSET } else { 0 }
}

/// Worst-case verse line for sizing the sonnet reading column — the LONGEST line
/// across all 154 sonnets in the Folger-cleaned text (60 chars, sonnet 14
/// "Then, churls, their thoughts, although their eyes were kind,"). Sizing the
/// block to this guarantees NO sonnet line wraps, and keeps the centered left
/// edge STABLE across sonnets (no jitter as you page) while tracking the
/// configured font/size. If the source ever gains a longer line, widen this.
const SONNET_BLOCK_SAMPLE: &str = "Then, churls, their thoughts, although their eyes were kind,";

/// Outer margin between the reading card (`content_hbox`) and the window, on
/// the LEFT/RIGHT. Single source of truth for `apply_card_sizing` (which sets
/// start/end on the card). Vertical margins use `CARD_VERTICAL_OUTER_MARGIN`.
pub(crate) const CARD_OUTER_MARGIN: i32 = 24;

/// Outer margin between the card and the window on the TOP/BOTTOM. Smaller than
/// the horizontal `CARD_OUTER_MARGIN` so the card is slightly TALLER; the gained
/// height funds more breathing room INSIDE the card (a taller header band above
/// the first line, a larger bottom reserve below the last line) without costing
/// a text row. Set once on `content_hbox` (mod.rs) and mirrored in the
/// `main_card_rect` height fallback below. Vertical only — never applied to
/// start/end.
pub(crate) const CARD_VERTICAL_OUTER_MARGIN: i32 = 14;

/// Horizontal room the two-column reading block needs BESIDES the two columns
/// themselves: the centre divider plus its seams. Used to clamp each column's
/// width against the window in `apply_tiled_mode`.
///
/// Measured, not guessed: with two 760px columns `page_turn_overlay` reports a
/// 1537px minimum, i.e. 17px of chrome over the 1520px of columns. Rounded up
/// to 20 for headroom — it is only ever SUBTRACTED from the available budget,
/// so erring high costs a couple of px of column width and never lets the pair
/// overflow the window.
pub(crate) const TWO_COLUMN_CHROME_ALLOWANCE: i32 = 20;

/// Bottom margin on the shared ask/edit input card (`ui::ask_card`), lifting
/// its bottom border above the act/scene toast pill with breathing room while
/// leaving the reading text above the card in place. The pill (`chapter_toast`,
/// window-anchored) occupies roughly `32` (its own `margin_bottom`) + ~12
/// (padding) + ~18 (label height) ≈ 62px from the window bottom; this clears
/// that band plus a gap so the card's bottom border never sits under the pill.
pub(crate) const OVERLAY_BOTTOM_CLEARANCE: i32 = 80;

/// Pixel width of the sonnet reading block, measured with the text_view's Pango
/// context against `SONNET_BLOCK_SAMPLE`. Used to center the one-section-per-page
/// block in the card. Returns 0 if measurement isn't possible.
fn current_block_text_width(state: &AppState) -> i32 {
    let ctx = state.text_view.create_pango_context();
    let pango_layout = pango::Layout::new(&ctx);
    let font = pango::FontDescription::from_string(&crate::ui::font_string(
        &state.config.font_family,
        state.config.font_size as i32,
    ));
    pango_layout.set_font_description(Some(&font));
    pango_layout.set_text(SONNET_BLOCK_SAMPLE);
    let (w, _h) = pango_layout.pixel_size();
    w
}

// (No card width seed constant. `content_hbox` carries no width of its own at
// all now -- `CardLayout` owns it, driven by `apply_card_sizing`. The 320px seed
// that used to live here is what the 2026-08-03 collapsed card rendered at.)

/// Narrowest window that can host the two-column reading block.
///
/// Two verse-safe columns (`2 * MIN_TWO_COLUMN_COLUMN_WIDTH`) plus the divider
/// and seams between them plus the card's two outer margins. Measured against
/// the live widget tree: two 760px columns make `page_turn_overlay` report a
/// 1537px minimum (17px of chrome over the 1520px of columns), and 1585px once
/// `CARD_OUTER_MARGIN` is added on both sides. Rounded to 20px of chrome for
/// headroom.
pub(crate) const MIN_TWO_COLUMN_WINDOW_WIDTH: i32 =
    2 * MIN_TWO_COLUMN_COLUMN_WIDTH + 20 + 2 * CARD_OUTER_MARGIN;

/// Whether two columns actually FIT in `window_width`.
///
/// The two-column block sizes each column to `MIN_TWO_COLUMN_COLUMN_WIDTH` with
/// a hard `width_request`, and a child's width_request propagates up as the
/// WINDOW's minimum size, which GTK advertises to the compositor. A window whose
/// minimum exceeds the output cannot honour fullscreen — the 2026-08-01 bug,
/// where the reader opened 1585x1200 on a 1920x1200 output with a strip of
/// wallpaper down the right-hand side, and niri shrank its own TILE to match the
/// advertised minimum instead of the reverse. `window.fullscreen()` was called
/// and granted at startup; the minimum simply outvoted it.
///
/// So the two-column layout is only claimed when there is genuinely room for it.
/// Below that the reader falls back to single column — an already-supported mode
/// — rather than narrowing the columns, which would let verse wrap and defeat
/// the point of the verse-safe width. Guarded by
/// `window_can_shrink_below_its_card_width` (tests/niri_smoke.rs).
///
/// `window_width <= 0` means "not allocated yet"; treat that as fitting so the
/// pre-first-allocation pass still sets up the layout the work asked for.
pub(crate) fn two_columns_fit(window_width: i32) -> bool {
    window_width <= 0 || window_width >= MIN_TWO_COLUMN_WINDOW_WIDTH
}

pub fn is_tiled_layout(window_width: i32, column_width: u32) -> bool {
    let card_w = (column_width as i32).min(window_width.max(1));
    (window_width - card_w) < 2 * super::VERSE_LEFT_OFFSET
}

/// Apply tiled-vs-monocle visual state: verse left offset and root-color
/// wallpaper masking via the `tiled` CSS class.
/// Called from both the resize tick and load_work so the initial render
/// picks up the right state before the first resize notification.
pub(crate) fn apply_tiled_mode(state: &mut AppState, root_box: &gtk4::Box, window_width: i32) {
    let cw = state.config.column_width;
    let tiled = is_tiled_layout(window_width, cw);

    // Root-color masking: paint the vbox with the card bg so no wallpaper
    // shows through when the card fills the tile.
    if tiled {
        root_box.add_css_class("tiled");
    } else {
        root_box.remove_css_class("tiled");
    }

    // Compute and apply the text_view left margin first. Verse works get
    // the +120 offset only when untiled — that means in tile mode the text
    // column starts at text_margins (e.g. 48) while in monocle it sits at
    // text_margins + 120 (e.g. 168). Page-label positioning depends on this
    // value, so derive it up-front.
    let work_type = state.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or("").to_string();
    let is_verse = !crate::db::line_types::is_prose_work(&work_type);
    // The full verse/prose left offset is a monocle (single wide column)
    // aesthetic. In two-column mode each column is narrow, so we use a small
    // offset instead: enough to give the sign-column gutter padding to the left
    // of its glyphs, but not so much that verse lines wrap.
    // Downgrade to one column when two will not fit: see `two_columns_fit`.
    // Enforced here (not at the callers) for the same reason as in
    // `apply_card_sizing` — this is where the 760px column `width_request`s that
    // CREATE the window minimum are set, so the rule belongs where it cannot be
    // bypassed.
    let two_col = state.column_count() == 2 && two_columns_fit(window_width);
    // In two-column mode the left column's verse line numbers sit in its LEFT
    // gutter (book foliation), outside the sign column, so the left margin must
    // reserve room for them on top of the normal offset. Prose has no numbers.
    let left_number_allowance = if two_col && !tiled && is_verse && SHOW_LINE_NUMBERS_TWO_COL {
        crate::gutter::LINE_NUMBER_WIDTH_TWO_COL + crate::gutter::LINE_NUMBER_LEFT_GAP_TWO_COL
    } else {
        0
    };
    let left_bump = if state.translations_visible {
        // Translation view: the card is now sized like the two-column layout
        // (wide), so inset the text like the gloss/synopsis cards (~card_width/4
        // from the card edge) instead of hugging the left edge. Use the ACTUAL
        // on-screen card width (clamped to the window) so the inset degrades to
        // 0 when the card fills a narrow window — this runs even when `tiled`
        // (which is computed against column_width, not the wide translation
        // card) would otherwise be true. Subtract the base text_margins so
        // logical_left lands at ~card_width/4 overall.
        let target = target_card_width(
            window_width, state.config.column_width, state.column_count(), true,
        );
        let card_w = target.min(window_width.max(1));
        (crate::ui::card_side_margin(card_w) - state.config.text_margins as i32).max(0)
    } else if state.one_section_per_page() {
        // One section per page (sonnet_sequence): center the sonnet BLOCK in the
        // card — verse lines stay left-aligned to a common edge, but that edge is
        // placed so the widest line is centered. The number heading is then
        // center-justified over the block (see apply_one_section_centering).
        let card_w = state.config.column_width as i32;
        // +16px slack so a line measured at exactly block_w never wraps at the
        // text-region boundary (Pango layout width can differ a hair from the
        // standalone measure).
        let block_w = current_block_text_width(state) + 16;
        if block_w > 0 && card_w > block_w {
            ((card_w - block_w) / 2 - state.config.text_margins as i32).max(0)
        } else {
            super::VERSE_LEFT_OFFSET
        }
    } else if tiled {
        0
    } else if two_col {
        super::TWO_COLUMN_LEFT_OFFSET + left_number_allowance
    } else if is_verse {
        super::VERSE_LEFT_OFFSET
    } else {
        // Prose monocle: a centered NYTimes-style column. Inset is a fraction of
        // the ACTUAL on-screen card width (clamped to the window). Uses the
        // tighter prose_reading_card_margin (card/8) so prose text fills more of
        // the card with less left/right padding. Subtract the base text_margins
        // so logical_left lands exactly at that inset.
        // Mirrors the translations_visible branch's card-relative inset.
        let card_w = target_card_width(
            window_width, effective_column_width(state), state.column_count(), false,
        ).min(window_width.max(1));
        (crate::ui::prose_reading_card_margin(card_w) - state.config.text_margins as i32).max(0)
    };
    let logical_left = state.config.text_margins as i32 + left_bump;
    let gutter_active = state.gutter_renderer.is_some();
    if gutter_active {
        // Gutter's baked-in width only matches its creation-time logical left.
        // If the layout changed, tear down and rebuild so the gutter fits the
        // new column geometry.
        if state.gutter_logical_left.get() != logical_left {
            if let Some(old) = state.gutter_renderer.take() {
                crate::gutter::remove_gutter_renderer(&state.text_view, old);
            }
            state.text_view.set_left_margin(logical_left);
            // Adopt the NEW logical margin as the gutter's restore point BEFORE
            // rebuilding: setup_gutter() re-applies gutter_logical_left first
            // (its idempotence guard), so leaving the stale value here would
            // clobber the margin just set and pin the gutter at its original
            // creation-time geometry forever (the lost-left-padding bug).
            state.gutter_logical_left.set(logical_left);
            if state.dialogue_formatting_active {
                crate::app::formatting::apply_dialogue_formatting(state);
            }
            super::setup_gutter(state);
        }
    } else if state.sign_column_visible.get() {
        // Sign column is shown by default — create the gutter on the first
        // layout pass after a work loads. Margin is at logical_left here, so
        // setup_gutter() computes its width correctly.
        if state.text_view.left_margin() != logical_left {
            state.text_view.set_left_margin(logical_left);
            if state.dialogue_formatting_active {
                crate::app::formatting::apply_dialogue_formatting(state);
            }
        }
        // Keep the restore point in sync (a stale value from a previous work's
        // gutter would otherwise override this pass's margin in setup_gutter).
        state.gutter_logical_left.set(logical_left);
        super::setup_gutter(state);
    } else if state.text_view.left_margin() != logical_left {
        state.text_view.set_left_margin(logical_left);
        if state.dialogue_formatting_active {
            crate::app::formatting::apply_dialogue_formatting(state);
        }
    }

    // Right margin = gap between the text and its line number. In two-column
    // mode this must stay the tight line-number gap (set by the gutter setup),
    // NOT the wide single-column EXTRA_RIGHT_MARGIN — otherwise the left
    // column loses ~90px of text width and verse lines wrap. The right view's
    // margin is set in the gutter setup and never touched here, so without
    // this guard the two columns would also be asymmetric (left narrower).
    if two_col {
        state.text_view.set_right_margin(crate::gutter::LINE_NUMBER_TEXT_GAP_TWO_COL);
    } else if state.one_section_per_page() {
        // One section per page: set the right margin symmetric to the centered
        // left margin so the text region equals the sonnet block. Then a
        // center-justified number heading centers exactly over the block, and the
        // block stays centered in the card.
        state.text_view.set_right_margin(logical_left.max(state.config.text_margins as i32));
    } else if state.translations_visible {
        // Translation view: inset the right edge like the gloss/synopsis cards
        // (~card_width/4) so the reading block is symmetric within the wide card.
        let target = target_card_width(
            window_width, state.config.column_width, state.column_count(), true,
        );
        let card_w = target.min(window_width.max(1));
        state.text_view.set_right_margin(crate::ui::card_side_margin(card_w).max(crate::gutter::LINE_NUMBER_TEXT_GAP_TWO_COL));
    } else if !is_verse {
        // Prose monocle: symmetric right margin == the centered left inset, so
        // the column is centered in the card (NYTimes body look). Recompute the
        // same card-relative value used for logical_left above (card/8).
        let card_w = target_card_width(
            window_width, effective_column_width(state), state.column_count(), false,
        ).min(window_width.max(1));
        state.text_view.set_right_margin(crate::ui::prose_reading_card_margin(card_w));
    } else {
        let logical_right = state.config.text_margins as i32
            + crate::config::EXTRA_RIGHT_MARGIN;
        state.text_view.set_right_margin(logical_right);
    }

    // Book-spine symmetry. In two-column mode each half of the card is wider
    // than the verse needs, so left-aligned text in both columns drifts toward
    // the left of the card. To make the layout hug the center divider
    // symmetrically, size each column's scrolled window to the text width and
    // align it toward the divider: left column → right-aligned, right column →
    // left-aligned. Equal leftover then falls on the two outer edges.
    if two_col && !tiled {
        // Center the two-column BLOCK in the card rather than letting each
        // column fill half of it. Each column is fixed to its natural width
        // (the verse-safe column width); the block [col | divider | col] then
        // sizes to content and is centered, so the card's slack becomes equal
        // outer margins on both sides and the divider stays centered.
        let col_w = MIN_TWO_COLUMN_COLUMN_WIDTH;
        state.columns_hbox.set_hexpand(false);
        state.columns_hbox.set_halign(gtk4::Align::Center);
        state.scrolled_overlay.set_margin_start(0);
        state.scrolled_overlay.set_hexpand(false);
        state.scrolled_overlay.set_width_request(col_w);
        state.right_scrolled_overlay.set_hexpand(false);
        state.right_scrolled_overlay.set_width_request(col_w);
        // Each scrolled window fills its fixed-width column overlay; text is
        // left-aligned inside as usual.
        state.scrolled_window.set_hexpand(true);
        state.scrolled_window.set_halign(gtk4::Align::Fill);
        state.scrolled_window.set_width_request(-1);
        state.right_scrolled_window.set_hexpand(true);
        state.right_scrolled_window.set_halign(gtk4::Align::Fill);
        state.right_scrolled_window.set_width_request(-1);
    } else {
        // Restore single-column fill behavior.
        state.columns_hbox.set_hexpand(true);
        state.columns_hbox.set_halign(gtk4::Align::Fill);
        state.scrolled_overlay.set_margin_start(0);
        state.scrolled_overlay.set_hexpand(true);
        state.scrolled_overlay.set_width_request(-1);
        state.right_scrolled_overlay.set_hexpand(true);
        state.right_scrolled_overlay.set_width_request(-1);
        state.scrolled_window.set_hexpand(true);
        state.scrolled_window.set_halign(gtk4::Align::Fill);
        state.scrolled_window.set_width_request(-1);
        state.right_scrolled_window.set_hexpand(true);
        state.right_scrolled_window.set_halign(gtk4::Align::Fill);
        state.right_scrolled_window.set_width_request(-1);
    }

    state.top_spacer.set_height_request(super::TOP_SPACER_HEIGHT);
}

/// Reconfigure the column layout to match the current `column_count()`:
/// re-run `apply_tiled_mode` (margins/widths/gutter) and show or hide the
/// right column + divider. Use after anything that changes `column_count()`
/// at runtime — e.g. toggling translations, which forces a single column.
pub(crate) fn apply_column_layout(state: &mut AppState) {
    let vbox = state.vbox.clone();
    let ww = state.window.width();
    // Resize the card to match the current layout: the narrow centered
    // translation card, the configured single-column width, or the wide
    // two-column card. apply_tiled_mode only sets margins/gutters, not the
    // card's width_request, so this must run too or the card keeps its old
    // (wrong) width.
    let cw = effective_column_width(state);
    // Downgrade to one column when two will not fit (see `two_columns_fit`), and
    // feed the DOWNGRADED count to card sizing too — `target_card_width` floors
    // the card at `2 * MIN_TWO_COLUMN_COLUMN_WIDTH + 8` whenever it is told
    // "2 columns", which would re-impose the very minimum this avoids.
    let two_col = state.column_count() == 2 && two_columns_fit(ww);
    let cc = if two_col { 2 } else { 1 };
    let tr = state.translations_visible;
    apply_card_sizing(&state.content_hbox, ww, cw, cc, tr, state.chat_pinned());
    apply_tiled_mode(state, &vbox, ww);
    state.right_scrolled_overlay.set_visible(two_col);
    state.column_divider.set_visible(two_col);
    if !two_col {
        state.right_bottom_clip.set_height_request(0);
        state.next_division_watermark.set_visible(false);
    }
}

/// Max chars per source line across the Dickens prepared texts (the longest
/// line in `bleak-house-prepared.txt` is 78 chars; averages run ~59). The
/// prose card is widened so a line of this many average-width chars fits on
/// ONE rendered visual row of the centered 60% measure.
pub(crate) const PROSE_MEASURE_CHARS: i32 = 78;

/// Pure math for the prose card width: the card whose centered prose measure
/// holds `chars` average-width characters. The prose reading card is inset by
/// `prose_reading_card_margin` (card/D, D = `PROSE_READING_CARD_MARGIN_DIVISOR`
/// = 8) on BOTH sides, so measure = card - 2*(card/8) = 0.75*card; invert with
/// ceil(measure * 4 / 3). Never narrower than the configured base column width.
/// MUST stay in sync with `prose_reading_card_margin`'s divisor.
pub(crate) fn prose_card_width_px(chars: i32, avg_char_w: i32, base: u32) -> u32 {
    let measure = chars.max(0) * avg_char_w.max(0);
    // measure = card * (D - 2) / D  ⇒  card = ceil(measure * D / (D - 2)).
    let d = crate::ui::PROSE_READING_CARD_MARGIN_DIVISOR;
    let card = (measure * d + (d - 2 - 1)) / (d - 2);
    (card.max(0) as u32).max(base)
}

/// Effective column (card) width for the CURRENT work. Prose works at the
/// single-column layout widen so `PROSE_MEASURE_CHARS` average chars of the
/// current font fit on one rendered row (font-adaptive — tracks font cycling
/// and size changes); everything else uses the configured `column_width`.
/// Pre-load (no current work) also uses the configured width so a play never
/// flashes wide before its layout settles.
pub(crate) fn effective_column_width(state: &AppState) -> u32 {
    use gtk4::prelude::{TextBufferExt, TextTagExt, TextViewExt, WidgetExt};
    if state.current_work.is_none() || !state.is_prose() || state.column_count() != 1 {
        return state.config.column_width;
    }
    let ctx = state.text_view.pango_context();
    let font_desc = state
        .text_view
        .buffer()
        .tag_table()
        .lookup("font-size")
        .and_then(|tag| tag.font_desc());
    let metrics = ctx.metrics(font_desc.as_ref(), None);
    let avg = metrics.approximate_char_width() / pango::SCALE;
    prose_card_width_px(PROSE_MEASURE_CHARS, avg, state.config.column_width)
}

/// Target card width before clamping to the window.
///
/// - One column: the configured `column_width` (unchanged).
/// - Two columns: the larger of `column_width` and 85% of the window, so the
///   card grows on wide screens instead of squeezing two columns into one
///   column's worth of space. Never narrower than the single-column floor.
/// Tighter card width for the single-column translation view: a comfortable
/// reading measure (the verse-safe column width) plus room for the line-number
/// gutter, so the block centers in a wide window and the numbers hug the text
/// ends rather than sitting at the far card edge.
pub(crate) fn target_card_width(
    window_width: i32,
    column_width: u32,
    column_count: u8,
    translations: bool,
) -> i32 {
    let cw_cfg = column_width as i32;
    // Translation mode renders two logical columns (original + translation), so
    // size its card identically to the two-column layout — same width whether or
    // not translations are visible.
    if column_count >= 2 || translations {
        let proportional = (window_width as f32 * TWO_COLUMN_WIDTH_FRACTION) as i32;
        // Never narrow a column below the verse-safe floor: two columns plus a
        // few px for the divider. Also never below the single-column floor.
        let two_col_floor = 2 * MIN_TWO_COLUMN_COLUMN_WIDTH + 8;
        proportional.max(cw_cfg).max(two_col_floor)
    } else {
        cw_cfg
    }
}

/// Hand `content_hbox`'s width to its [`CardLayout`](crate::app::card_layout).
///
/// `width <= 0` means "fill whatever you are given" (the chat branch, where the
/// margins already carve out the card's region).
///
/// Silently does nothing if the box is not running a `CardLayout`. That cannot
/// happen for the real `content_hbox` -- `build_window` always installs one --
/// and `card_sizing_drives_the_layout_manager_not_a_width_request` fails the
/// build if this stops being the only width path.
fn set_card_layout_width(content_hbox: &gtk4::Box, width: i32) {
    if let Some(lm) = content_hbox.layout_manager() {
        if let Ok(card) = lm.downcast::<crate::app::card_layout::CardLayout>() {
            card.set_card_width(width);
        }
    }
}

/// Side margin for the card, given the window width and the card's drawn width.
///
/// Capped at `CARD_OUTER_MARGIN`. Margins count toward a widget's minimum size,
/// so margins that absorb ALL the slack make the minimum equal the current width
/// and pin the window exactly as an oversized `width_request` does -- measured at
/// min=1937 on a 1912 output. The card is centred by `halign`, so the margins only
/// need to be the small visual inset they were always meant to be.
pub(crate) fn card_side_margin(window_width: i32, card_w: i32) -> i32 {
    let slack = window_width - card_w;
    (slack / 2).clamp(0, CARD_OUTER_MARGIN)
}

pub(crate) fn apply_card_sizing(
    content_hbox: &gtk4::Box,
    window_width: i32,
    column_width: u32,
    column_count: u8,
    translations: bool,
    chat_open: bool,
) {
    let ww = window_width.max(0);
    // The two-column downgrade and the margin clamp both live in
    // `computed_card_width`, which `main_card_rect` also calls -- the two must
    // agree on the card's width, and sharing the function is what guarantees it.
    // The downgrade in particular has to be enforced somewhere central:
    // `apply_card_sizing` has eleven callers, each computing `column_count`
    // independently, and gating them individually already missed one (the resize
    // tick in mod.rs kept passing 2 and re-imposed the floor).
    let card_w = computed_card_width(ww, column_width, column_count, translations);
    let column_count = if two_columns_fit(ww) { column_count } else { 1 };
    let target = target_card_width(ww, column_width, column_count, translations);
    let slack = ww - card_w;
    if chat_open {
        // Chat layout: pin the card flush left. content_hbox is halign:Center,
        // so to land its fixed-width card flush against the left margin we must
        // RESERVE the panel's region as margin_end — otherwise the centered card
        // floats into the middle of the window and the panel overlaps (clips)
        // its right half. The panel abuts the card's right edge 1px away
        // (size_panel), so margin_end is exactly the panel-plus-seam region and
        // nothing but the hairline shows between card and panel.
        let start = (slack / 2).clamp(0, CARD_OUTER_MARGIN).min(slack.max(0));
        let end = (slack - start).max(0);
        // The margins reserve the panel's region, so what remains for the box
        // IS the card -- let `CardLayout` fill it rather than re-centring
        // `card_w` inside it (that would float the card back toward the middle,
        // the very thing the asymmetric margins exist to prevent).
        content_hbox.set_hexpand(true);
        set_card_layout_width(content_hbox, 0);
        content_hbox.set_margin_start(start);
        content_hbox.set_margin_end(end);
        crate::log_fmt!(
            "CARD_SIZING: chat ww={} card_w={} start={} end={}",
            ww, card_w, start, end
        );
        return;
    }
    // The card's width goes to `CardLayout`, never to `set_width_request`.
    //
    // The history is two bugs that look like opposites but are one knob:
    //
    // - An UNCLAMPED request (`config.column_width`, 1050) propagates up as the
    //   window's minimum and pins it -- niri shrinks its TILE to the minimum
    //   rather than granting fullscreen (2026-08-01).
    // - Removing the request entirely left NOTHING setting the width. A
    //   `WrapMode::Word` text view's minimum width is only its margins
    //   (gtk_text_view_measure never consults the layout horizontally), so the
    //   card collapsed to 281px and prose rendered one word per line
    //   (2026-08-03).
    //
    // In GTK a `width_request` IS the minimum, so no value satisfies both --
    // measured in cage, every route to a 1050 allocation (this box,
    // `scrolled_overlay`, `min-content-width`, margins) advertised ~1050 as the
    // minimum too. `CardLayout` exists precisely to separate those numbers: it
    // advertises a tiny minimum and allocates `card_w`. See
    // src/app/card_layout.rs.
    let margin = card_side_margin(ww, card_w);
    content_hbox.set_hexpand(true);
    set_card_layout_width(content_hbox, card_w);
    content_hbox.set_margin_start(margin);
    content_hbox.set_margin_end(margin);
    crate::log_fmt!(
        "CARD_SIZING: ww={} col_cfg={} cols={} target={} card_w={} margin={}",
        ww, column_width as i32, column_count, target, card_w, margin
    );
}

/// Authoritative card size for the full-screen overlays (synopsis, gloss,
/// translation). Mirrors the width `apply_card_sizing` requests for the reading
/// card so the overlays match the card instead of inheriting `content_hbox`'s
/// *allocated* width — which can exceed the card's `width_request` (a child's
/// natural width can stretch the hbox), making the overlay span edge to edge.
/// SINGLE SOURCE OF TRUTH for the dimensions of the VISIBLE main reading card.
///
/// Every full-screen overlay (gloss / journal / synopsis / echo) must match the
/// card the reader actually sees, for every work type. Rather than each overlay
/// re-deriving the size from a different widget with hand-tuned offsets (the bug
/// class this consolidates), they all read THIS rect.
///
/// The visible cream card is `content_hbox` (transparent, no margins of its own
/// once `apply_card_sizing` runs — its outer margins are between it and the
/// window, NOT inside the card) wrapping `page_turn_overlay`/`card_vbox`. So the
/// card's on-screen allocation is exactly `content_hbox`'s allocation. We read
/// that allocation directly when it's settled (post-first-layout), and fall back
/// to the computed width + window-minus-chrome height before first allocation.
/// Column-float geometry shared by the vocab float and the chat float: the
/// (x, width) of the reading column the float covers, extended to the column
/// divider so the float's border sits exactly on the divider instead of
/// leaving a sliver of card (the column stops 8px short of it — the divider's
/// CSS margin). `over_right` selects the right column.
pub(crate) fn column_float_rect(state: &AppState, over_right: bool) -> (i32, i32) {
    let col = if over_right {
        &state.right_scrolled_overlay
    } else {
        &state.scrolled_overlay
    };
    let (mut x, mut w) = col
        .compute_bounds(&state.window)
        .map(|b| (b.x() as i32, b.width() as i32))
        .unwrap_or((24, MIN_TWO_COLUMN_COLUMN_WIDTH));
    if let Some(d) = state.column_divider.compute_bounds(&state.window) {
        let (d_left, d_right) = (d.x() as i32, (d.x() + d.width()) as i32);
        if over_right {
            let new_x = d_left.min(x);
            w += x - new_x;
            x = new_x;
        } else {
            w = w.max(d_right - x);
        }
    }
    (x, w)
}

/// The card's drawn width, computed rather than measured.
///
/// SINGLE SOURCE OF TRUTH, shared with `apply_card_sizing` so the two can never
/// disagree. Reading `content_hbox`'s allocation instead is wrong: the box
/// deliberately carries no `width_request` (one would propagate up as the
/// WINDOW's minimum size and pin it below the output -- see
/// `card_width_request`), so its allocation reflects its children's natural
/// width, not the card's.
pub(crate) fn computed_card_width(
    window_width: i32,
    column_width: u32,
    column_count: u8,
    translations: bool,
) -> i32 {
    let ww = window_width.max(0);
    // Same two-column downgrade as `apply_card_sizing`, for the same reason:
    // `target_card_width` floors the card at the 1528 two-column measure when
    // told "2 columns", which would re-impose a width the window cannot host.
    let column_count = if two_columns_fit(ww) { column_count } else { 1 };
    let target = target_card_width(ww, column_width, column_count, translations);
    target.min((ww - 2 * CARD_OUTER_MARGIN).max(1))
}

pub(crate) fn main_card_rect(s: &AppState) -> (i32, i32) {
    // Width is COMPUTED, never read from `content_hbox`'s allocation.
    //
    // The box carries no `width_request` -- one would propagate up as the
    // WINDOW's minimum size and pin it below the output, which is the
    // 2026-08-01 fullscreen bug (see `card_width_request`). Without it the
    // allocation is the children's natural width, not the card's, so measuring
    // it would report the wrong rectangle: `overlay_clipping` failed with "text
    // region has no area" for exactly that reason.
    let card_w = computed_card_width(
        s.window.width().max(0),
        effective_column_width(s),
        s.column_count(),
        s.translations_visible,
    );
    // Height still comes from the allocation when settled -- nothing constrains
    // it the way width is constrained, so the measurement is trustworthy.
    let alloc_h = s.content_hbox.height();
    let card_h = if alloc_h > 0 {
        alloc_h
    } else {
        (s.window.height() - 2 * CARD_VERTICAL_OUTER_MARGIN).max(0)
    };
    (card_w, card_h)
}

/// Width + height an answer overlay (journal Q&A, gloss, synopsis) should
/// request. Intentionally does NOT alias [`main_card_rect`]'s width: the
/// overlay uses the **1-column reading measure** so it presents at the same
/// width regardless of the reader's real `column_count()` (a 2-column play
/// spread and a 1-column prose work behind it yield an identically-sized
/// overlay). Forcing `column_count = 1` and `translations = false` makes
/// `target_card_width` return the 1-col measure (`effective_column_width`,
/// prose-adaptive) rather than the wide 2-col proportional width.
///
/// Height is unchanged — the reader card's settled/allocated height, which is
/// already column-count-invariant.
pub(crate) fn overlay_card_size(s: &AppState) -> (i32, i32) {
    let ww = s.window.width().max(0);
    let width = overlay_card_width(ww, effective_column_width(s));
    (width, main_card_rect(s).1)
}

/// Pure 1-column overlay-width computation, factored out for unit testing.
/// `one_col_measure` is the reader's `effective_column_width` (the configured
/// column width, or the prose-adaptive measure for 1-col prose). Returns the
/// 1-col `target_card_width` clamped to the window, matching `main_card_rect`.
pub(crate) fn overlay_card_width(window_width: i32, one_col_measure: u32) -> i32 {
    let target = target_card_width(window_width, one_col_measure, 1, false);
    target.min(window_width.max(1))
}

/// Height an overlay should request to match the visible main card.
pub(crate) fn overlay_card_height(s: &AppState) -> i32 {
    main_card_rect(s).1
}

#[cfg(test)]
mod column_default_tests {
    use super::super::default_column_count_for_parts;

    #[test]
    fn shakespeare_play_defaults_to_two() {
        assert_eq!(default_column_count_for_parts("Shakespeare", "play"), 2);
    }
    #[test]
    fn shakespeare_poem_defaults_to_two() {
        assert_eq!(default_column_count_for_parts("Shakespeare", "poem"), 2);
        assert_eq!(default_column_count_for_parts("Shakespeare", "narrative_poem"), 2);
    }
    #[test]
    fn sonnet_sequence_defaults_to_one() {
        // Each sonnet is its own (div1,div2) section; two columns would push every
        // sonnet to the right column and leave the left empty. See
        // default_column_count_for_parts.
        assert_eq!(default_column_count_for_parts("Shakespeare", "sonnet_sequence"), 1);
    }
    #[test]
    fn non_shakespeare_play_defaults_to_two() {
        assert_eq!(default_column_count_for_parts("Marlowe", "play"), 2);
    }
    #[test]
    fn prose_work_types_default_to_one() {
        // Prose renders through the single-column prose visual-row pagination
        // engine (gated on column_count()==1); two columns would route it
        // through the play engine and disable prose pagination. Covers every
        // line_types::PROSE_TYPES value.
        assert_eq!(default_column_count_for_parts("Dickens", "novel"), 1);
        assert_eq!(default_column_count_for_parts("Dickens", "prose"), 1);
        assert_eq!(default_column_count_for_parts("Churchill", "prose_book"), 1);
        assert_eq!(default_column_count_for_parts("Emerson", "essay_collection"), 1);
    }
    #[test]
    fn anthology_defaults_to_two() {
        // An anthology deliberately packs two columns; it is NOT a prose type,
        // so the prose 1-col rule must not catch it.
        assert_eq!(default_column_count_for_parts("Crystal", "anthology"), 2);
    }
}

#[cfg(test)]
mod card_width_tests {
    use super::{overlay_card_width, prose_card_width_px, target_card_width, PROSE_MEASURE_CHARS};

    #[test]
    fn prose_card_width_inverts_the_75_percent_measure() {
        // 78 chars at 9px avg = 702px measure; card = ceil(702*8/6) = 936.
        assert_eq!(prose_card_width_px(PROSE_MEASURE_CHARS, 9, 900), 936);
        // The resulting card's actual measure (card - 2*(card/8), the
        // prose_reading_card_margin insets) must hold the requested chars.
        let card = prose_card_width_px(PROSE_MEASURE_CHARS, 9, 900) as i32;
        assert!(card - 2 * (card / 8) >= PROSE_MEASURE_CHARS * 9);
    }

    #[test]
    fn prose_card_width_never_narrower_than_configured() {
        // Small font: 78 * 6 = 468px measure -> 624px card, below base 1050.
        assert_eq!(prose_card_width_px(PROSE_MEASURE_CHARS, 6, 1050), 1050);
        // Degenerate inputs clamp safely to base.
        assert_eq!(prose_card_width_px(0, 9, 1050), 1050);
        assert_eq!(prose_card_width_px(PROSE_MEASURE_CHARS, 0, 1050), 1050);
    }

    #[test]
    fn one_column_keeps_configured_width() {
        // Single column always uses column_width regardless of window size.
        assert_eq!(target_card_width(1920, 1050, 1, false), 1050);
        assert_eq!(target_card_width(800, 1050, 1, false), 1050);
    }

    #[test]
    fn two_columns_fill_fraction_of_wide_window() {
        // TWO_COLUMN_WIDTH_FRACTION (0.68) of 1920 = 1305, below the wrap-safe
        // two-column floor (2*760+8 = 1528), so clamp up to 1528.
        assert_eq!(target_card_width(1920, 1050, 2, false), 1528);
    }

    #[test]
    fn two_columns_use_proportional_when_above_floor() {
        // On a very wide window the proportional width wins: 0.68 * 2400 = 1632.
        assert_eq!(target_card_width(2400, 1050, 2, false), 1632);
    }

    #[test]
    fn two_columns_never_below_verse_safe_floor() {
        // Narrow window: proportional (0.68*1300=884) and column_width (1050)
        // are both below the 1528 two-column floor → clamp up to 1528.
        assert_eq!(target_card_width(1300, 1050, 2, false), 1528);
    }

    #[test]
    fn translations_match_two_column_width() {
        // Translation mode (column_count forced to 1) sizes like two columns.
        assert_eq!(
            target_card_width(2400, 1050, 1, true),
            target_card_width(2400, 1050, 2, false),
        );
    }

    #[test]
    fn overlay_width_is_one_col_invariant_over_two_column_reader() {
        // A 2-column play behind the overlay: the reader card is the wide 2-col
        // proportional width, but the overlay must present at the 1-col measure.
        // (effective_column_width would return the configured 1050 for a play,
        // so the overlay sees one_col_measure = 1050.)
        let window = 2400;
        let one_col_measure = 1050;
        let overlay = overlay_card_width(window, one_col_measure);
        // Overlay equals the 1-col target, NOT the wider 2-col proportional one.
        assert_eq!(overlay, target_card_width(window, one_col_measure, 1, false));
        assert_eq!(overlay, one_col_measure as i32);
        assert_ne!(overlay, target_card_width(window, one_col_measure, 2, false));
        assert!(overlay < target_card_width(window, one_col_measure, 2, false));
    }

    #[test]
    fn overlay_width_clamps_to_window() {
        // Degenerate pre-allocation window: never exceed window.max(1).
        assert_eq!(overlay_card_width(0, 1050), 1); // clamps to 1
        // Narrow window smaller than the configured measure clamps down.
        assert_eq!(overlay_card_width(800, 1050), 800);
    }

    #[test]
    fn card_sizing_drives_the_layout_manager_not_a_width_request() {
        // `content_hbox` must carry NO `width_request` -- its width comes from
        // `CardLayout` (src/app/card_layout.rs), which advertises a small
        // minimum and allocates the card. A `width_request` here IS the
        // window's minimum and pins the tile (2026-08-01); the previous form of
        // this guard demanded one, which was correct only while the layout
        // manager did not exist.
        let src = include_str!("layout.rs");
        for (i, line) in src.lines().enumerate() {
            let t = line.trim();
            if t.starts_with("content_hbox.set_width_request(") && t.ends_with(");") {
                panic!(
                    "layout.rs:{}: `{}` -- content_hbox's width belongs to \
                     CardLayout via `set_card_layout_width`. A width_request \
                     becomes the WINDOW's minimum width and stops the \
                     compositor tiling the reader narrow \
                     (window_can_shrink_below_its_card_width).",
                    i + 1,
                    t
                );
            }
        }

        // ...and the layout manager must actually be driven, in BOTH branches
        // of `apply_card_sizing`. A branch that returns without setting a width
        // leaves the card at whatever the previous tick left behind.
        let calls = src
            .lines()
            .filter(|l| l.trim().starts_with("set_card_layout_width(content_hbox,"))
            .count();
        assert_eq!(
            calls, 2,
            "expected exactly 2 `set_card_layout_width(content_hbox, ...)` \
             calls (the chat branch and the normal branch), found {calls}"
        );
    }

    #[test]
    fn card_layout_reports_a_small_minimum_but_allocates_the_card() {
        // The invariant the whole manager exists for, as arithmetic: the width
        // it ADVERTISES must not track the width it ALLOCATES. Verified against
        // the real widget in `single_column_card_fills_its_computed_width`
        // (allocation) and `window_can_shrink_below_its_card_width` (minimum).
        use crate::app::card_layout::ADVERTISED_MIN_WIDTH;

        // A 1050 card must not advertise anything near 1050, or the window's
        // minimum pins the tile exactly as a width_request did.
        assert!(
            ADVERTISED_MIN_WIDTH < 1050 / 4,
            "ADVERTISED_MIN_WIDTH ({ADVERTISED_MIN_WIDTH}) is creeping toward a \
             real card width; at that point CardLayout is an expensive \
             width_request and the niri shrink guard will fail again"
        );
        // ...but not zero, which invites a degenerate window size.
        assert!(ADVERTISED_MIN_WIDTH > 0);
    }


    #[test]
    fn computed_card_width_matches_what_apply_card_sizing_draws() {
        use super::{computed_card_width, CARD_OUTER_MARGIN};

        // `main_card_rect` must not read `content_hbox`'s ALLOCATION: the box
        // carries no `width_request` (that would propagate up as the window's
        // minimum and pin it -- see `card_width_request`), so its allocation is
        // its children's natural width, not the card's. The card's width is
        // whatever `apply_card_sizing` computed, so compute it the same way.
        //
        // Identical clamp to `apply_card_sizing`: window MINUS both outer
        // margins, because the margins sit outside the card.
        assert_eq!(computed_card_width(1920, 1050, 1, false), 1050);
        assert_eq!(computed_card_width(1920, 1050, 2, false), 1528);

        // Narrow window: the card gives up width to keep its margins.
        assert_eq!(computed_card_width(1000, 1050, 1, false), 1000 - 2 * CARD_OUTER_MARGIN);

        // Two columns that do not fit are downgraded to one first, exactly as
        // `apply_card_sizing` does -- otherwise the 1528 floor reappears here.
        // At ww=1100 the downgrade yields the 1-col measure (1050), which is
        // already under the 1052 margin clamp, so 1050 is the answer.
        assert_eq!(computed_card_width(1100, 1050, 2, false), 1050);

        // Degenerate pre-allocation window clamps to a sane positive width.
        assert!(computed_card_width(0, 1050, 1, false) >= 1);
    }

    #[test]
    fn card_margins_never_pad_the_window_minimum_up_to_its_own_width() {
        use super::{card_side_margin, CARD_OUTER_MARGIN};

        // Margins DO count toward a widget's minimum size (measured: with the
        // card at 1528 and margins at 204 each, GTK reported the window's
        // minimum as 1937 -- its own current width, on a 1912 output). So
        // absorbing all the slack in margins just recreates the bug that
        // absorbing it in `width_request` caused: the minimum tracks the current
        // width and the window can never shrink.
        //
        // Cap them. Centering is `halign`'s job, not padding's.
        let ww = 1937;
        let card_w = 1528;
        let m = card_side_margin(ww, card_w);
        assert!(
            card_w + 2 * m < ww,
            "card {card_w} + 2*{m} margin must stay under the window {ww}, \
             else the minimum equals the current width and pins it"
        );
        assert_eq!(m, CARD_OUTER_MARGIN);

        // Narrow window: margins shrink with the slack rather than going negative.
        assert_eq!(card_side_margin(1000, 990), 5);
        assert_eq!(card_side_margin(1000, 1000), 0);
    }
}
