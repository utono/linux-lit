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
    let rh = row_height(rows, budget);

    for b in &a.bands {
        let (sx, sy) = pos_of(b.start_char);
        let (ex, ey) = pos_of(b.end_char);
        // Deeper bands sit higher: row 0 (outermost) is the bottom rule.
        let row_y = y + (rows as f64 - 1.0 - b.depth as f64) * rh;
        // A band crossing a line wrap draws one segment per visual row.
        let segments: Vec<(f64, f64)> = if (sy - ey).abs() < 1.0 {
            vec![(sx, ex)]
        } else {
            vec![(sx, x0 + content_w), (x0, ex)]
        };
        // Tint by depth, from the theme accent.
        let fade = 1.0 - (b.depth as f64 * 0.15).min(0.6);
        cr.set_source_rgba(inner.accent.0, inner.accent.1, inner.accent.2, fade);
        cr.set_line_width(2.0);
        for (a_x, b_x) in &segments {
            cr.move_to(*a_x, row_y);
            cr.line_to(*b_x, row_y);
            let _ = cr.stroke();
        }
        // Label, centered on the first segment.
        let (lx0, lx1) = segments[0];
        let (ll, lw, _) = layout_text(area, &b.label, "Sans 10", None);
        cr.move_to(((lx0 + lx1) / 2.0 - lw / 2.0).max(x0), row_y - 14.0);
        pangocairo::functions::show_layout(cr, &ll);
    }
    y += rows as f64 * rh + 16.0;

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
}
