//! Full-screen Cairo diagram of a selection's grammatical structure.
//!
//! Geometry follows `keybinds_overlay.rs` (hexpand/vexpand + Align::Fill,
//! everything computed against the widget_w/widget_h handed to set_draw_func)
//! so the diagram fills the WINDOW, never the reading card — it is unaffected
//! by column count, card margins, or an open two-column play.
//!
//! Two deliberate departures from that precedent:
//!   * Pango, not cr.show_text — this renders the work's own early modern
//!     English, which the toy text API cannot shape or fall back for.
//!   * Theme colors, not hardcoded literals — this is a reading surface, so it
//!     follows the theme cycle.

use gtk4::prelude::*;
use gtk4::{DrawingArea, Overlay};
use std::cell::RefCell;
use std::rc::Rc;

use crate::syntax_diagram::SyntaxAnalysis;

/// Natural height of one band row.
const BAND_ROW_H: f64 = 26.0;
/// Never shrink a band row below this — past it, labels stop being legible.
const MIN_BAND_ROW_H: f64 = 12.0;
/// Window margin around the content column.
const MARGIN: f64 = 48.0;
/// Content column cap, so text does not run edge to edge on a wide display.
const MAX_CONTENT_W: f64 = 1240.0;

/// Height per band row: natural, shrunk to fit `available`, floored at
/// `MIN_BAND_ROW_H` so a pathological stack degrades rather than vanishing.
fn row_height(rows: usize, available: f64) -> f64 {
    if rows == 0 {
        return BAND_ROW_H;
    }
    let fitted = available / rows as f64;
    fitted.min(BAND_ROW_H).max(MIN_BAND_ROW_H)
}

/// Height per band row for a line that has ANOTHER line of passage text
/// directly beneath it — i.e. every wrapped line except the last.
///
/// The depth stack drawn under such a line must fit inside the real gap to
/// the next line's own text (`line_h`), never `budget` (leftover space below
/// the whole passage block, which is unrelated to inter-line spacing and was
/// the root cause: `(rows - 1) * rh` derived from `budget` could exceed
/// `line_h` and push a shallow band's rule into the following line of text).
///
/// `clearance` reserves headroom above the rule stack for the line's own
/// descender and the band label floated above the topmost rule
/// (`row_y - 14.0` at the call site), so the stack never starts flush against
/// the glyphs. Shrinks like `row_height`, floored at `MIN_BAND_ROW_H` so deep
/// nesting degrades (overflows slightly into the floor) rather than
/// vanishing or clipping.
fn interior_row_height(rows: usize, line_h: f64, clearance: f64) -> f64 {
    let available = (line_h - clearance).max(0.0);
    row_height(rows, available)
}

/// Bottom of the drawn band stack: the maximum `row_y` actually reached by
/// any drawn segment, i.e. `max(line_bottoms[line_index] + depth_offset)`
/// over every `(line_index, depth)` pair a band segment was drawn at.
///
/// Pure and Cairo/Pango-free — `line_bottoms[i]` is each visual line's own
/// `line_y(i) + line_h` (already resolved to a plain number at the call
/// site), `rows`/`last_line_index`/`rh`/`interior_rh` are the same values
/// `draw_analysis` computes for row placement, and `entries` is one
/// `(line_index, depth)` pair per segment actually drawn (mirrors
/// `band_line_spans`' output, minus the byte range, which does not affect
/// height).
///
/// Replaces the old `y += rows as f64 * rh + 16.0` global estimate, which
/// was correct only when every line used the same (`rh`) row height — once
/// interior lines switched to `interior_row_height`, that global model no
/// longer matched what was actually drawn, and the commentary crept up over
/// the diagram on wrapped text. Returns `f64::MIN` when `entries` is empty
/// (no bands drawn at all) so the caller can fall back to a sane default
/// rather than treating 0.0 as a real height.
fn band_stack_bottom(
    line_bottoms: &[f64],
    entries: &[(usize, u8)],
    rows: usize,
    last_line_index: usize,
    rh: f64,
    interior_rh: f64,
) -> f64 {
    entries
        .iter()
        .filter_map(|&(line_index, depth)| {
            let base = *line_bottoms.get(line_index)?;
            let row_h = if line_index == last_line_index { rh } else { interior_rh };
            let depth_offset = (rows as f64 - 1.0 - depth as f64) * row_h;
            Some(base + depth_offset)
        })
        .fold(f64::MIN, f64::max)
}

/// Given a band's byte span `[band_start, band_end)` and the byte span of
/// every visual line in the Pango layout (in line order, each
/// `(start_byte, end_byte)` exclusive at the end — i.e. `lines[i].1 ==
/// lines[i + 1].0` for contiguous wrapped lines), return the `(line_index,
/// seg_start_byte, seg_end_byte)` triple for every line the band touches,
/// clipped to that line's own span.
///
/// Pure and Cairo/Pango-free so the segment-selection logic — the actual
/// defect being fixed — is unit-testable without a display. A band spanning
/// N visual lines yields exactly N triples, one per line, in line order.
fn band_line_spans(
    band_start: usize,
    band_end: usize,
    lines: &[(usize, usize)],
) -> Vec<(usize, usize, usize)> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, &(line_start, line_end))| {
            let seg_start = band_start.max(line_start);
            let seg_end = band_end.min(line_end);
            (seg_start < seg_end).then_some((i, seg_start, seg_end))
        })
        .collect()
}

/// What the surface is currently showing.
enum View {
    Loading,
    Analysis(SyntaxAnalysis),
}

struct Inner {
    view: View,
    /// Commentary hidden by default; a key toggles it.
    show_note: bool,
    /// Theme colors, resolved at show time (r, g, b) in 0..1.
    ink: (f64, f64, f64),
    dim: (f64, f64, f64),
    accent: (f64, f64, f64),
    scrim: (f64, f64, f64),
}

pub struct SyntaxOverlay {
    drawing_area: DrawingArea,
    inner: Rc<RefCell<Inner>>,
}

impl SyntaxOverlay {
    pub fn new() -> Self {
        let drawing_area = DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .halign(gtk4::Align::Fill)
            .valign(gtk4::Align::Fill)
            .visible(false)
            .build();

        let inner = Rc::new(RefCell::new(Inner {
            view: View::Loading,
            show_note: false,
            ink: (0.96, 0.94, 0.90),
            dim: (0.70, 0.68, 0.66),
            accent: (0.80, 0.60, 0.40),
            scrim: (0.10, 0.10, 0.12),
        }));

        let draw_inner = inner.clone();
        drawing_area.set_draw_func(move |area, cr, w, h| {
            draw(area, cr, &draw_inner.borrow(), w as f64, h as f64);
        });

        SyntaxOverlay { drawing_area, inner }
    }

    /// Add to the window-filling outer overlay (the same layer the vocab popup
    /// and toasts use), so the diagram floats above the whole reader chain.
    pub fn attach_to(&self, overlay: &Overlay) {
        overlay.add_overlay(&self.drawing_area);
        self.drawing_area.set_visible(false);
    }

    /// Show the loading state. MUST be called before dispatching the Claude
    /// request — `run_claude_request`'s contract.
    pub fn show_loading(&self, theme: &crate::theme::Theme) {
        {
            let mut i = self.inner.borrow_mut();
            i.view = View::Loading;
            i.show_note = false;
            apply_theme(&mut i, theme);
        }
        self.drawing_area.set_visible(true);
        self.drawing_area.queue_draw();
    }

    pub fn show_analysis(&self, analysis: SyntaxAnalysis, theme: &crate::theme::Theme) {
        {
            let mut i = self.inner.borrow_mut();
            i.view = View::Analysis(analysis);
            apply_theme(&mut i, theme);
        }
        self.drawing_area.set_visible(true);
        self.drawing_area.queue_draw();
    }

    pub fn hide(&self) {
        self.drawing_area.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.drawing_area.is_visible()
    }

    /// Toggle the prose commentary under the diagram.
    pub fn toggle_note(&self) {
        {
            let mut i = self.inner.borrow_mut();
            i.show_note = !i.show_note;
        }
        self.drawing_area.queue_draw();
    }
}

/// Resolve theme colors into the (r,g,b) floats Cairo wants.
fn apply_theme(inner: &mut Inner, theme: &crate::theme::Theme) {
    inner.ink = crate::theme::hex_to_rgb(&crate::theme::vocab_popup_fg(theme));
    inner.accent = crate::theme::hex_to_rgb(&crate::theme::vocab_popup_accent(theme));
    // NOTE: the field is `root_color`, not `root`.
    inner.scrim = crate::theme::hex_to_rgb(&theme.root_color);
    // Dim = ink pulled toward the scrim, for secondary labels.
    inner.dim = (
        inner.ink.0 * 0.65 + inner.scrim.0 * 0.35,
        inner.ink.1 * 0.65 + inner.scrim.1 * 0.35,
        inner.ink.2 * 0.65 + inner.scrim.2 * 0.35,
    );
}

/// Lay out a Pango layout and return it plus its pixel size.
fn layout_text(
    area: &DrawingArea,
    text: &str,
    font: &str,
    width: Option<f64>,
) -> (gtk4::pango::Layout, f64, f64) {
    let layout = area.create_pango_layout(Some(text));
    layout.set_font_description(Some(&gtk4::pango::FontDescription::from_string(font)));
    if let Some(w) = width {
        layout.set_width((w * gtk4::pango::SCALE as f64) as i32);
        layout.set_wrap(gtk4::pango::WrapMode::Word);
    }
    let (pw, ph) = layout.pixel_size();
    (layout, pw as f64, ph as f64)
}

fn draw(area: &DrawingArea, cr: &gtk4::cairo::Context, inner: &Inner, w: f64, h: f64) {
    // Full-window scrim.
    cr.set_source_rgba(inner.scrim.0, inner.scrim.1, inner.scrim.2, 0.97);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();

    let content_w = (w - 2.0 * MARGIN).min(MAX_CONTENT_W);
    let x0 = (w - content_w) / 2.0;

    match &inner.view {
        View::Loading => {
            cr.set_source_rgb(inner.dim.0, inner.dim.1, inner.dim.2);
            let (layout, tw, th) = layout_text(area, "Analyzing syntax…", "Sans 16", None);
            cr.move_to((w - tw) / 2.0, (h - th) / 2.0);
            pangocairo::functions::show_layout(cr, &layout);
        }
        View::Analysis(a) => draw_analysis(area, cr, inner, a, x0, content_w, h),
    }
}

fn draw_analysis(
    area: &DrawingArea,
    cr: &gtk4::cairo::Context,
    inner: &Inner,
    a: &SyntaxAnalysis,
    x0: f64,
    content_w: f64,
    h: f64,
) {
    let mut y = MARGIN;

    // ── The selection text ──
    cr.set_source_rgb(inner.ink.0, inner.ink.1, inner.ink.2);
    let (layout, _tw, th) = layout_text(area, &a.text, "Serif 20", Some(content_w));
    cr.move_to(x0, y);
    pangocairo::functions::show_layout(cr, &layout);

    // Byte offset -> (x, y) of that character, via Pango's own index mapping,
    // so bands line up with wrapped text exactly. Anchored to the text
    // block's own top (`text_top`, a copy taken now) rather than the
    // running `y` cursor, which is mutated after this closure is built —
    // borrowing `y` itself would keep it borrowed across those later
    // `y += …` statements.
    let text_top = y;
    let pos_of = |byte: usize| -> (f64, f64) {
        let (rect, _) = layout.cursor_pos(byte as i32);
        (
            x0 + rect.x() as f64 / gtk4::pango::SCALE as f64,
            text_top + rect.y() as f64 / gtk4::pango::SCALE as f64,
        )
    };
    let line_h = {
        let (_, _, one_line_h) = layout_text(area, "X", "Serif 20", None);
        one_line_h
    };

    // Per visual line: its own byte span (start..end, exclusive) and, for
    // convenience, its baseline y (via `pos_of` at the line's own start
    // byte) — everything `band_line_spans` and the segment-drawing loop
    // below need, without either of them touching Pango types directly.
    let pango_lines: Vec<(usize, usize)> = (0..layout.line_count())
        .filter_map(|i| {
            let l = layout.line(i)?;
            let start = l.start_index() as usize;
            let end = start + l.length() as usize;
            Some((start, end))
        })
        .collect();
    let line_y = |line_index: usize| -> f64 {
        pango_lines
            .get(line_index)
            .map(|&(start, _)| pos_of(start).1)
            .unwrap_or(text_top)
    };
    // x-pixel range of a byte span, clipped to one visual line, via Pango's
    // own `x_ranges` (line-local pixel units — offset by the line's own x
    // start, which for this left-aligned, non-indented layout is x0).
    let line_x_range = |line_index: usize, seg_start: usize, seg_end: usize| -> (f64, f64) {
        match layout.line(line_index as i32) {
            Some(l) => {
                let ranges = l.x_ranges(seg_start as i32, seg_end as i32);
                let scale = gtk4::pango::SCALE as f64;
                match ranges.as_slice() {
                    [x0_px, x1_px, ..] => (x0 + *x0_px as f64 / scale, x0 + *x1_px as f64 / scale),
                    _ => (x0, x0),
                }
            }
            None => (x0, x0),
        }
    };

    y += th + 8.0;

    // ── POS row: each tag under its word ──
    cr.set_source_rgb(inner.dim.0, inner.dim.1, inner.dim.2);
    for p in &a.pos {
        let (px, py) = pos_of(p.start_char);
        let (pl, _, _) = layout_text(area, &p.pos, "Sans 9", None);
        cr.move_to(px, py + line_h);
        pangocairo::functions::show_layout(cr, &pl);
    }
    y += 16.0;

    // ── Band rows, stacked by depth ──
    let rows = crate::syntax_diagram::max_row(&a.bands) + 1;
    let note_reserve = if inner.show_note && a.note.is_some() { 160.0 } else { 40.0 };
    let budget = (h - y - note_reserve).max(MIN_BAND_ROW_H);
    // `rh` is the generous, budget-derived row height: correct ONLY for the
    // passage's last visual line, which has nothing but the note/margin
    // beneath it. Every interior line (one with another line of text right
    // under it) must instead use `interior_row_height`, fitted to the real
    // gap to that next line (`line_h`) — see its doc comment for why `rh`
    // alone was the defect.
    let rh = row_height(rows, budget);
    let last_line_index = pango_lines.len().saturating_sub(1);
    // Headroom the rule stack must leave above the line's own glyphs: the
    // band label floats 14px above its topmost rule (see `row_y - 14.0`
    // below), plus a little breathing room so the label isn't flush against
    // the line's descenders.
    const LABEL_CLEARANCE: f64 = 18.0;
    let interior_rh = interior_row_height(rows, line_h, LABEL_CLEARANCE);
    // Per-line row height: the last visual line keeps the generous
    // below-passage spacing; every other line uses the fitted interior
    // spacing so a shallow band's rule can never reach into the next line's
    // text. Same value for every interior line, so the stack still reads as
    // one coherent ladder rather than jittering line to line.
    let row_h_for_line = |line_index: usize| -> f64 {
        if line_index == last_line_index { rh } else { interior_rh }
    };
    // Each visual line's own bottom (`line_y(i) + line_h`), resolved to a
    // plain number up front so the pure `band_stack_bottom` helper below
    // never has to touch Pango types.
    let line_bottoms: Vec<f64> = (0..pango_lines.len()).map(|i| line_y(i) + line_h).collect();
    // One (line_index, depth) pair per segment actually drawn, fed to
    // `band_stack_bottom` after the loop to find the real bottom of the
    // stack — see the comment at that call site for why this replaced the
    // old `rows as f64 * rh` global estimate.
    let mut drawn_entries: Vec<(usize, u8)> = Vec::new();

    for b in &a.bands {
        // One triple per visual line the band touches — the fix for the
        // "3+ wrapped lines lose their middle segments" defect. Each triple
        // is a (line_index, seg_start_byte, seg_end_byte), already clipped
        // to that line's own span by the pure helper.
        let spans = band_line_spans(b.start_char, b.end_char, &pango_lines);
        if spans.is_empty() {
            continue;
        }
        let fade = 1.0 - (b.depth as f64 * 0.15).min(0.6);
        cr.set_source_rgba(inner.accent.0, inner.accent.1, inner.accent.2, fade);
        cr.set_line_width(2.0);

        let mut first_segment: Option<(f64, f64, f64)> = None; // (x0, x1, row_y)
        for (line_index, seg_start, seg_end) in &spans {
            let (sx, ex) = line_x_range(*line_index, *seg_start, *seg_end);
            // Deeper bands sit higher within their own line's row stack: row
            // 0 (outermost) is the bottom rule, same intent as before, just
            // measured from each touched line's own baseline and its own
            // (interior-vs-last) row height instead of one shared band-area
            // origin.
            let depth_offset =
                (rows as f64 - 1.0 - b.depth as f64) * row_h_for_line(*line_index);
            let row_y = line_y(*line_index) + line_h + depth_offset;
            cr.move_to(sx, row_y);
            cr.line_to(ex, row_y);
            let _ = cr.stroke();
            if first_segment.is_none() {
                first_segment = Some((sx, ex, row_y));
            }
        }

        // Label, centered on the first segment, drawn once per band.
        if let Some((lx0, lx1, row_y)) = first_segment {
            let (ll, lw, _) = layout_text(area, &b.label, "Sans 10", None);
            cr.move_to(((lx0 + lx1) / 2.0 - lw / 2.0).max(x0), row_y - 14.0);
            pangocairo::functions::show_layout(cr, &ll);
        }

        for &(line_index, _, _) in &spans {
            drawn_entries.push((line_index, b.depth));
        }
    }
    // Advance past the ACTUAL bottom of the drawn band geometry, not a
    // recomputed global estimate (`rows as f64 * rh`) — that global model
    // was correct only when every line shared one row height; once interior
    // lines switched to `interior_row_height` (per-line, generally smaller
    // than `rh`), it no longer matched what was actually drawn and could
    // leave the commentary overlapping the last band row on wrapped text.
    // Falls back to the pre-loop `y` (top of the band area) plus one `rh`
    // row when no band was drawn at all, so an analysis with an empty
    // `bands` list still reserves sane space for the note.
    let stack_bottom = band_stack_bottom(
        &line_bottoms,
        &drawn_entries,
        rows,
        last_line_index,
        rh,
        interior_rh,
    );
    y = if stack_bottom > f64::MIN {
        stack_bottom + 16.0
    } else {
        y + rh + 16.0
    };

    // ── Commentary (toggleable) ──
    if inner.show_note {
        if let Some(note) = &a.note {
            cr.set_source_rgb(inner.ink.0, inner.ink.1, inner.ink.2);
            let (nl, _, _) = layout_text(area, note, "Sans 12", Some(content_w));
            cr.move_to(x0, y);
            pangocairo::functions::show_layout(cr, &nl);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_row_height_shrinks_to_fit_available_space() {
        // 3 rows in generous space keeps the natural height.
        assert_eq!(row_height(3, 400.0), BAND_ROW_H);
        // 8 rows in 100px must shrink rather than overflow, while still
        // fitting comfortably above the legibility floor (100/8 = 12.5,
        // between MIN_BAND_ROW_H=12 and BAND_ROW_H=26 — both constraints are
        // simultaneously satisfiable here, unlike the 20-row/100px case
        // below).
        let h = row_height(8, 100.0);
        assert!(h < BAND_ROW_H, "expected shrink, got {h}");
        assert!(h * 8.0 <= 100.0 + f64::EPSILON, "must fit the budget");
        assert!(h >= MIN_BAND_ROW_H, "must not shrink below legibility floor");
    }

    #[test]
    fn zero_rows_is_safe() {
        assert_eq!(row_height(0, 100.0), BAND_ROW_H);
    }

    #[test]
    fn pathological_row_count_floors_rather_than_vanishes() {
        // 20 rows in 100px: fitting exactly would need 5px/row, below
        // MIN_BAND_ROW_H (12px). The two constraints ("shrink to fit" and
        // "never go under the legibility floor") are mutually unsatisfiable
        // for this input — 20 * 12 = 240 > 100 — so the floor wins and the
        // stack is allowed to overflow its budget rather than become
        // illegible. This is the documented degrade-gracefully behavior,
        // not a bug: a diagram with this many nested clauses scrolls/clips
        // rather than shows unreadable 5px bands.
        let h = row_height(20, 100.0);
        assert_eq!(h, MIN_BAND_ROW_H, "floor must win when it can't also fit");
    }

    // ── interior_row_height ──

    #[test]
    fn interior_row_height_fits_the_reported_regression() {
        // The exact numbers from the defect report: 1920x1200, "Serif 20"
        // line_h ~= 27px, BAND_ROW_H = 26.0, 3 nested bands (rows = 3). The
        // naive `(rows - 1) * rh` with `rh` sourced from `budget` could reach
        // BAND_ROW_H (26.0), so 2 * 26.0 = 52.0 — well past a 27px line_h.
        // The fitted interior height must keep the WHOLE stack — depth_offset
        // for the outermost row is `(rows - 1) * interior_rh` — within the
        // gap to the next line.
        let rows = 3;
        let line_h = 27.0;
        let clearance = 18.0;
        let interior_rh = interior_row_height(rows, line_h, clearance);
        let max_depth_offset = (rows as f64 - 1.0) * interior_rh;
        assert!(
            max_depth_offset < line_h,
            "stack of {max_depth_offset} must fit under a {line_h}px line, got interior_rh={interior_rh}"
        );
    }

    #[test]
    fn interior_row_height_single_visual_line_is_moot_but_safe() {
        // rows == 1 (no nesting): depth_offset is always 0 regardless of the
        // row height value, but the helper must still return something sane
        // (no div-by-zero, no NaN).
        let h = interior_row_height(1, 27.0, 18.0);
        assert!(h.is_finite() && h > 0.0);
    }

    #[test]
    fn interior_row_height_zero_rows_matches_row_height_contract() {
        // Mirrors `zero_rows_is_safe` for `row_height`: 0 rows is the "no
        // bands" case and must return the natural height, not 0 or NaN.
        assert_eq!(interior_row_height(0, 27.0, 18.0), BAND_ROW_H);
    }

    #[test]
    fn interior_row_height_pathological_rows_floors_rather_than_negative() {
        // A huge nesting depth against a normal line_h: `line_h - clearance`
        // is small (or the rows count alone demands sub-floor spacing), so
        // the legibility floor must win, exactly like `row_height`'s own
        // pathological case — never 0, never negative, never NaN.
        let h = interior_row_height(20, 27.0, 18.0);
        assert_eq!(h, MIN_BAND_ROW_H);
    }

    #[test]
    fn interior_row_height_clearance_exceeding_line_h_still_floors() {
        // A short line_h (tight wrap) with generous clearance can drive
        // `line_h - clearance` negative; the helper must clamp to a sane
        // floor rather than propagate a negative/NaN available space.
        let h = interior_row_height(3, 10.0, 18.0);
        assert_eq!(h, MIN_BAND_ROW_H);
    }

    // ── band_line_spans ──
    //
    // Three lines of 10 bytes each: [0,10), [10,20), [20,30).
    const THREE_LINES: [(usize, usize); 3] = [(0, 10), (10, 20), (20, 30)];

    #[test]
    fn single_line_band_yields_one_segment() {
        let spans = band_line_spans(2, 8, &THREE_LINES);
        assert_eq!(spans, vec![(0, 2, 8)]);
    }

    #[test]
    fn two_line_band_yields_two_segments_clipped_to_each_line() {
        // Band runs from mid-line-0 into mid-line-1.
        let spans = band_line_spans(5, 15, &THREE_LINES);
        assert_eq!(spans, vec![(0, 5, 10), (1, 10, 15)]);
    }

    #[test]
    fn three_line_band_does_not_lose_the_middle_segment() {
        // This is the defect under test: a band crossing 3 visual lines
        // must draw all 3, not just the first and last.
        let spans = band_line_spans(5, 25, &THREE_LINES);
        assert_eq!(spans, vec![(0, 5, 10), (1, 10, 20), (2, 20, 25)]);
    }

    #[test]
    fn many_line_band_yields_n_segments_for_every_n() {
        let lines: Vec<(usize, usize)> = (0..7).map(|i| (i * 10, i * 10 + 10)).collect();
        for n in 1..=7 {
            let end = n * 10 - 3; // stop mid-way through the nth line
            let spans = band_line_spans(0, end, &lines);
            assert_eq!(spans.len(), n, "band to byte {end} should touch {n} lines");
            // Every segment must land on the line it claims and lie inside
            // that line's own byte span.
            for &(line_index, seg_start, seg_end) in &spans {
                let (line_start, line_end) = lines[line_index];
                assert!(seg_start >= line_start && seg_end <= line_end);
                assert!(seg_start < seg_end);
            }
        }
    }

    #[test]
    fn band_exactly_covering_one_full_line() {
        let spans = band_line_spans(10, 20, &THREE_LINES);
        assert_eq!(spans, vec![(1, 10, 20)]);
    }

    #[test]
    fn band_touching_line_boundary_does_not_spill_into_next_line() {
        // end == a line boundary: the next line must not get a zero-width
        // spurious segment.
        let spans = band_line_spans(0, 10, &THREE_LINES);
        assert_eq!(spans, vec![(0, 0, 10)]);
    }

    #[test]
    fn empty_lines_list_yields_no_segments() {
        let spans = band_line_spans(0, 10, &[]);
        assert!(spans.is_empty());
    }

    // ── band_stack_bottom ──

    #[test]
    fn single_line_bottom_is_the_last_lines_outermost_row() {
        // One visual line, 3 rows (rows=3), only the outermost band (depth 0)
        // drawn on it — depth_offset for depth 0 is (rows - 1) * rh.
        let line_bottoms = [100.0];
        let entries = [(0usize, 0u8)];
        let bottom = band_stack_bottom(&line_bottoms, &entries, 3, 0, 26.0, 20.0);
        assert_eq!(bottom, 100.0 + 2.0 * 26.0);
    }

    #[test]
    fn interior_line_uses_interior_row_height_not_last_line_rh() {
        // Two visual lines; last_line_index = 1. A depth-0 band drawn on the
        // INTERIOR line (index 0) must use `interior_rh`, not `rh` — this is
        // exactly the composition defect: before the fix, the caller
        // advanced `y` by `rows * rh`, which is wrong once interior lines use
        // a different (smaller) row height.
        let line_bottoms = [50.0, 150.0];
        let entries = [(0usize, 0u8)]; // interior line, outermost band
        let rh = 26.0;
        let interior_rh = 10.0;
        let bottom = band_stack_bottom(&line_bottoms, &entries, 3, 1, rh, interior_rh);
        assert_eq!(bottom, 50.0 + 2.0 * interior_rh);
        assert_ne!(
            bottom,
            50.0 + 2.0 * rh,
            "must not fall back to the stale global rh model"
        );
    }

    #[test]
    fn bottom_is_the_max_across_multiple_drawn_segments() {
        // A band spanning 2 lines plus a deeper band only on the last line:
        // the max must win even though it's not the entry with the largest
        // line_index.
        let line_bottoms = [50.0, 150.0];
        let rh = 26.0;
        let interior_rh = 10.0;
        let entries = [
            (0usize, 0u8), // interior line, outermost: 50 + 2*10 = 70
            (1usize, 0u8), // last line, outermost: 150 + 2*26 = 202 <- max
            (1usize, 1u8), // last line, depth 1: 150 + 1*26 = 176
        ];
        let bottom = band_stack_bottom(&line_bottoms, &entries, 3, 1, rh, interior_rh);
        assert_eq!(bottom, 202.0);
    }

    #[test]
    fn no_entries_yields_f64_min_so_caller_can_fall_back() {
        let bottom = band_stack_bottom(&[100.0], &[], 3, 0, 26.0, 20.0);
        assert_eq!(bottom, f64::MIN);
    }

    #[test]
    fn out_of_range_line_index_is_skipped_not_panicking() {
        // Defensive: an entry referencing a line index past `line_bottoms`
        // must be ignored rather than panic (should not happen in practice,
        // since entries are derived from the same `pango_lines` the bottoms
        // come from, but the helper must not blow up if it ever does).
        let entries = [(5usize, 0u8)];
        let bottom = band_stack_bottom(&[100.0], &entries, 3, 0, 26.0, 20.0);
        assert_eq!(bottom, f64::MIN);
    }
}
