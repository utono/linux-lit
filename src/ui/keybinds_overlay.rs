use gtk4::prelude::*;
use gtk4::{DrawingArea, Overlay};
use std::rc::Rc;

/// Definition of a single key on the keyboard overlay.
struct KeyDef {
    unshifted: &'static str,
    shifted: &'static str,
    action: &'static str,
    shift_action: &'static str,
    modifiers: &'static [(&'static str, &'static str)],
}

const fn key(
    unshifted: &'static str,
    shifted: &'static str,
    action: &'static str,
    shift_action: &'static str,
    modifiers: &'static [(&'static str, &'static str)],
) -> KeyDef {
    KeyDef { unshifted, shifted, action, shift_action, modifiers }
}

const fn ub(unshifted: &'static str, shifted: &'static str) -> KeyDef {
    key(unshifted, shifted, "", "", &[])
}

const fn bare(unshifted: &'static str, shifted: &'static str, action: &'static str) -> KeyDef {
    key(unshifted, shifted, action, "", &[])
}

// ── Row definitions ──────────────────────────────────────────────────

const NUMBER_ROW: &[KeyDef] = &[
    ub("$", "~"),
    bare("+", "1", "toggle speed"),
    key("[", "2", "prev ch", "2: prev scene", &[]),
    key("{", "3", "next ch", "3: next scene", &[]),
    key("(", "4", "prev bkmk", "4: prev bkmk", &[]),
    bare("&", "5", "next bkmk"),
    ub("=", "6"),
    ub(")", "7"),
    ub("}", "8"),
    ub("]", "9"),
    key("*", "0", "", "reset font", &[]),
    bare("!", "%", "font \u{2212}"),
    bare("|", "`", "font +"),
];
const BACKSPACE: KeyDef = bare("\u{232b}", "", "delete ts");

const UPPER_ROW: &[KeyDef] = &[
    key(";", ":", "reopen echoes", "prev bkmk", &[]),
    key(",", "<", "prev dlg", "", &[("C-,", "settings")]),
    bare(".", ">", "set chapter"),
    key("p", "P", "nudge \u{2212}0.2", "P: +0.2", &[("C-p", "picker")]),
    bare("y", "Y", "prev chunk"),
    key("f", "F", "font \u{2192}", "F: \u{2190}", &[("C-f", "pg fwd"), ("M-f", "font info")]),
    key("g", "G", "", "", &[("C-g", "gloss tog"), ("M-g", "gloss pick"), ("S-C-g", "echo turns")]),
    ub("c", "C"),
    key("r", "R", "next vocab", "R: prev vocab", &[]),
    key("l", "L", "toggle signs", "", &[("S-C-l", "save+quit")]),
    key("/", "?", "search", "", &[("C-/", "keybinds")]),
    ub("@", "^"),
    key("\\", "#", "vocab ▶", "◀ vocab", &[("C-\\", "conc picker"), ("M-\\", "vocab hi")]),
];
const TAB_KEY: KeyDef = bare("Tab", "", "play/pause");

const HOME_ROW: &[KeyDef] = &[
    bare("a", "A", "play from ts"),
    key("o", "O", "seek \u{2212}3.5", "O: \u{2212}60", &[]),
    key("e", "E", "seek +3.5", "E: +60", &[]),
    key("u", "U", "start time", "", &[("C-u", "pg fwd"), ("M-u", "set end time")]),
    key("i", "I", "echoes", "I: reopen echoes", &[("M-i", "translations")]),
    key("d", "D", "", "", &[("C-d", "debug log"), ("M-d", "dim tog")]),
    key("h", "H", "synopsis", "H: auto vocab", &[]),
    key("t", "T", "", "", &[("M-t", "title tog")]),
    key("n", "N", "next match", "N: prev match", &[]),
    bare("s", "S", "sync tog"),
    key("-", "_", "", "", &[("C--", "recent")]),
];
const ESC_KEY: KeyDef = bare("Esc", "", "clear AB");

const BOTTOM_ROW: &[KeyDef] = &[
    bare("'", "\"", "reopen echoes"),
    bare("q", "Q", "next dlg"),
    bare("j", "J", "cursor \u{2193}"),
    bare("k", "K", "cursor \u{2191}"),
    bare("x", "X", "next chunk"),
    key("b", "B", "", "", &[("C-b", "pg back")]),
    key("m", "M", "bookmark", "", &[("C-m", "bookmarks"), ("C-S-m", "media picker")]),
    key("w", "W", "copy word", "W: collect", &[]),
    key("v", "V", "", "V: visual mode", &[]),
    bare("z", "Z", "zt…"),
];

const SHIFT_KEY: KeyDef = ub("Shift", "");

/// Row 5: modifiers, sequences, and arrows gathered into one screen.
const MOD_SEQ_ROW: &[KeyDef] = &[
    key("Space", "", "page \u{2193}", "page \u{2191}", &[]),
    bare("gg", "", "go to start"),
    key("G", "", "", "go to end", &[]),
    bare("g;", "", "latest bookmark"),
    bare("zt", "", "scroll cursor top"),
    key("\u{2191}", "", "cursor up", "", &[("C-\u{2191}", "volume +")]),
    key("\u{2193}", "", "cursor down", "", &[("C-\u{2193}", "volume \u{2212}")]),
    bare("\u{2190}", "", "seek \u{2212}3.5"),
    bare("\u{2192}", "", "set start time"),
];

// ── Per-row screens ──────────────────────────────────────────────────

/// Title shown for each keyboard-row screen, plus the gamepad screen.
const ROW_TITLES: &[&str] = &[
    "NUMBER / SYMBOL ROW",
    "UPPER ROW",
    "HOME ROW",
    "BOTTOM ROW",
    "MODIFIERS & SEQUENCES",
];

/// Number of keyboard-row screens (the gamepad is a 6th screen handled by the
/// gamepad overlay, reached by cycling past the last row).
pub const ROW_COUNT: usize = 5;

/// The keys shown on row screen `idx` (0..ROW_COUNT). The row-leader key
/// (Backspace/Tab/Esc/Shift) is appended so every key in the physical row is
/// represented.
fn row_keys(idx: usize) -> Vec<&'static KeyDef> {
    match idx {
        0 => NUMBER_ROW.iter().chain(std::iter::once(&BACKSPACE)).collect(),
        1 => std::iter::once(&TAB_KEY).chain(UPPER_ROW.iter()).collect(),
        2 => std::iter::once(&ESC_KEY).chain(HOME_ROW.iter()).collect(),
        3 => std::iter::once(&SHIFT_KEY).chain(BOTTOM_ROW.iter()).collect(),
        _ => MOD_SEQ_ROW.iter().collect(),
    }
}

/// Index of the first bound key in a row (so the highlight starts on something
/// useful), else 0.
fn first_bound(keys: &[&KeyDef]) -> usize {
    keys.iter()
        .position(|d| !d.action.is_empty() || !d.shift_action.is_empty() || !d.modifiers.is_empty())
        .unwrap_or(0)
}


// ── Drawing (per-row screen) ─────────────────────────────────────────

/// Draw one row-screen: a keycap strip across the top (one key highlighted)
/// and a detail panel below listing the highlighted key's full bindings.
fn draw_row_screen(
    cr: &gtk4::cairo::Context,
    row_idx: usize,
    selected: usize,
    widget_w: f64,
    widget_h: f64,
) {
    // Full-screen scrim
    cr.set_source_rgba(0.341, 0.322, 0.475, 0.95);
    cr.rectangle(0.0, 0.0, widget_w, widget_h);
    let _ = cr.fill();

    let keys = row_keys(row_idx);
    let sel = selected.min(keys.len().saturating_sub(1));

    // ── Header ──
    cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
    cr.set_font_size(20.0);
    cr.set_source_rgb(0.96, 0.94, 0.90);
    let title = ROW_TITLES.get(row_idx).copied().unwrap_or("");
    let header = format!("Row {} of {}  —  {}", row_idx + 1, ROW_COUNT + 1, title);
    let he = cr.text_extents(&header).unwrap();
    let _ = cr.move_to((widget_w - he.width()) / 2.0, 48.0);
    let _ = cr.show_text(&header);

    // ── Keycap strip ──
    // Fit `n` caps across the available width.
    let margin = 40.0;
    let avail_w = widget_w - 2.0 * margin;
    let n = keys.len().max(1) as f64;
    let cap_gap = 8.0;
    let cap_w = ((avail_w - (n - 1.0) * cap_gap) / n).min(110.0).max(36.0);
    let cap_h = 60.0;
    let strip_w = n * cap_w + (n - 1.0) * cap_gap;
    let strip_x = (widget_w - strip_w) / 2.0;
    let strip_y = 78.0;

    for (i, def) in keys.iter().enumerate() {
        let x = strip_x + i as f64 * (cap_w + cap_gap);
        let bound = !def.action.is_empty() || !def.shift_action.is_empty() || !def.modifiers.is_empty();
        let is_sel = i == sel;

        // Selected glow
        if is_sel {
            cr.set_source_rgba(0.227, 0.353, 0.616, 0.30);
            rounded_rect(cr, x - 3.0, strip_y - 3.0, cap_w + 6.0, cap_h + 6.0, 9.0);
            let _ = cr.fill();
        }

        // Cap background
        if is_sel {
            cr.set_source_rgb(0.875, 0.910, 0.957);
        } else if bound {
            cr.set_source_rgb(0.949, 0.914, 0.882);
        } else {
            cr.set_source_rgb(1.0, 0.98, 0.953);
        }
        rounded_rect(cr, x, strip_y, cap_w, cap_h, 7.0);
        let _ = cr.fill();

        // Cap border
        if is_sel {
            cr.set_source_rgb(0.227, 0.353, 0.616);
            cr.set_line_width(2.0);
        } else {
            cr.set_source_rgb(0.72, 0.66, 0.66);
            cr.set_line_width(1.0);
        }
        rounded_rect(cr, x + 0.5, strip_y + 0.5, cap_w - 1.0, cap_h - 1.0, 7.0);
        let _ = cr.stroke();

        // Unshifted glyph (centered-ish)
        cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
        cr.set_font_size(if def.unshifted.chars().count() > 1 { 18.0 } else { 22.0 });
        if is_sel {
            cr.set_source_rgb(0.149, 0.251, 0.478);
        } else if bound {
            cr.set_source_rgb(0.341, 0.322, 0.475);
        } else {
            cr.set_source_rgb(0.596, 0.576, 0.647);
        }
        let ge = cr.text_extents(def.unshifted).unwrap();
        let _ = cr.move_to(x + (cap_w - ge.width()) / 2.0 - ge.x_bearing(), strip_y + 28.0);
        let _ = cr.show_text(def.unshifted);

        // Shifted glyph (small, top-right)
        if !def.shifted.is_empty() {
            cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
            cr.set_font_size(12.0);
            cr.set_source_rgb(0.565, 0.478, 0.663);
            let se = cr.text_extents(def.shifted).unwrap();
            let _ = cr.move_to(x + cap_w - se.width() - 6.0, strip_y + 16.0);
            let _ = cr.show_text(def.shifted);
        }

        // Tiny bare-action hint under the glyph (truncated; full text is in panel)
        if !def.action.is_empty() {
            cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
            cr.set_font_size(10.0);
            cr.set_source_rgb(0.157, 0.412, 0.514);
            let hint = truncate_to_width(cr, def.action, cap_w - 10.0);
            let _ = cr.move_to(x + 5.0, strip_y + cap_h - 8.0);
            let _ = cr.show_text(&hint);
        }
    }

    // ── Detail panel ──
    let panel_x = margin;
    let panel_y = strip_y + cap_h + 28.0;
    let panel_w = widget_w - 2.0 * margin;
    let def = keys[sel];

    let mut rows: Vec<(&str, String, (f64, f64, f64))> = Vec::new();
    if !def.action.is_empty() {
        rows.push((def.unshifted, def.action.to_string(), (0.157, 0.412, 0.514))); // pine
    }
    if !def.shift_action.is_empty() {
        rows.push(("Shift", def.shift_action.to_string(), (0.565, 0.478, 0.663))); // iris
    }
    for &(combo, act) in def.modifiers {
        let (label, col) = if combo.starts_with("M-") && !combo.contains("C-") {
            ("Alt", (0.706, 0.388, 0.478)) // rose
        } else if combo.contains("S-") {
            ("Ctrl+Shift", (0.204, 0.506, 0.341)) // green
        } else {
            ("Ctrl", (0.557, 0.420, 0.208)) // gold
        };
        rows.push((label, act.to_string(), col));
    }
    if rows.is_empty() {
        rows.push(("", "(unbound)".to_string(), (0.596, 0.576, 0.647)));
    }

    let line_h = 30.0;
    let panel_h = 56.0 + rows.len() as f64 * line_h;

    // Panel background
    cr.set_source_rgb(0.965, 0.949, 0.925);
    rounded_rect(cr, panel_x, panel_y, panel_w, panel_h, 10.0);
    let _ = cr.fill();
    cr.set_source_rgb(0.886, 0.847, 0.784);
    rounded_rect(cr, panel_x + 0.5, panel_y + 0.5, panel_w - 1.0, panel_h - 1.0, 10.0);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    // Panel title: the key glyph
    cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
    cr.set_font_size(24.0);
    cr.set_source_rgb(0.149, 0.251, 0.478);
    let _ = cr.move_to(panel_x + 22.0, panel_y + 36.0);
    let mut ttl = def.unshifted.to_string();
    if !def.shifted.is_empty() {
        ttl.push_str(&format!("   ({} = shift)", def.shifted));
    }
    let _ = cr.show_text(&ttl);

    // Binding rows
    for (i, (label, act, col)) in rows.iter().enumerate() {
        let ry = panel_y + 60.0 + i as f64 * line_h;
        cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
        cr.set_font_size(15.0);
        cr.set_source_rgb(0.4, 0.38, 0.45);
        let _ = cr.move_to(panel_x + 24.0, ry);
        let _ = cr.show_text(label);
        cr.set_source_rgb(col.0, col.1, col.2);
        cr.set_font_size(16.0);
        let _ = cr.move_to(panel_x + 180.0, ry);
        let _ = cr.show_text(act);
    }

    // ── Footer hint ──
    cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(14.0);
    cr.set_source_rgb(0.78, 0.76, 0.82);
    let foot = "Esc close  \u{00b7}  n/p cycle rows  \u{00b7}  j/k or \u{2190}/\u{2192} move highlight";
    let fe = cr.text_extents(foot).unwrap();
    let _ = cr.move_to((widget_w - fe.width()) / 2.0, widget_h - 28.0);
    let _ = cr.show_text(foot);
}

/// Truncate `text` so it fits within `max_w` px, appending "…" if cut.
fn truncate_to_width(cr: &gtk4::cairo::Context, text: &str, max_w: f64) -> String {
    if cr.text_extents(text).map(|e| e.width()).unwrap_or(0.0) <= max_w {
        return text.to_string();
    }
    let mut s = String::new();
    for ch in text.chars() {
        let mut trial = s.clone();
        trial.push(ch);
        trial.push('\u{2026}');
        if cr.text_extents(&trial).map(|e| e.width()).unwrap_or(0.0) > max_w {
            s.push('\u{2026}');
            return s;
        }
        s.push(ch);
    }
    s
}

fn rounded_rect(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let pi = std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
    cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
    cr.arc(x + r, y + r, r, pi, 3.0 * pi / 2.0);
    cr.close_path();
}


// ── Public API ───────────────────────────────────────────────────────

pub struct KeybindsOverlay {
    pub overlay: Overlay,
    drawing_area: DrawingArea,
    row_index: Rc<std::cell::Cell<usize>>,
    selected: Rc<std::cell::Cell<usize>>,
}

impl KeybindsOverlay {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let drawing_area = DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .halign(gtk4::Align::Fill)
            .valign(gtk4::Align::Fill)
            .visible(false)
            .build();
        drawing_area.add_css_class("keybinds-overlay-canvas");

        let row_index = Rc::new(std::cell::Cell::new(0usize));
        let selected = Rc::new(std::cell::Cell::new(first_bound(&row_keys(0))));

        let row_draw = row_index.clone();
        let sel_draw = selected.clone();
        drawing_area.set_draw_func(move |_area, cr, w, h| {
            draw_row_screen(cr, row_draw.get(), sel_draw.get(), w as f64, h as f64);
        });

        KeybindsOverlay { overlay, drawing_area, row_index, selected }
    }

    pub fn show(&self) {
        // Always open at the first row.
        self.row_index.set(0);
        self.selected.set(first_bound(&row_keys(0)));
        self.drawing_area.set_visible(true);
        self.drawing_area.queue_draw();
    }

    pub fn hide(&self) {
        self.drawing_area.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.drawing_area.is_visible()
    }

    /// Cycle to the next row. Returns false when cycling past the last keyboard
    /// row (caller switches to the gamepad screen).
    pub fn next_row(&self) -> bool {
        let cur = self.row_index.get();
        if cur + 1 >= ROW_COUNT {
            return false; // caller should advance to the gamepad screen
        }
        let next = cur + 1;
        self.row_index.set(next);
        self.selected.set(first_bound(&row_keys(next)));
        self.drawing_area.queue_draw();
        true
    }

    /// Cycle to the previous row. Returns false when at the first row (caller
    /// wraps to the gamepad screen).
    pub fn prev_row(&self) -> bool {
        let cur = self.row_index.get();
        if cur == 0 {
            return false; // caller should wrap to the gamepad screen
        }
        let prev = cur - 1;
        self.row_index.set(prev);
        self.selected.set(first_bound(&row_keys(prev)));
        self.drawing_area.queue_draw();
        true
    }

    /// Jump directly to the last keyboard row (used when entering from the
    /// gamepad screen via `p`).
    pub fn show_last_row(&self) {
        let last = ROW_COUNT - 1;
        self.row_index.set(last);
        self.selected.set(first_bound(&row_keys(last)));
        self.drawing_area.set_visible(true);
        self.drawing_area.queue_draw();
    }

    /// Move the key highlight within the current row (wraps).
    pub fn move_selection(&self, delta: i32) {
        let len = row_keys(self.row_index.get()).len();
        if len == 0 {
            return;
        }
        let cur = self.selected.get() as i32;
        let next = (cur + delta).rem_euclid(len as i32) as usize;
        self.selected.set(next);
        self.drawing_area.queue_draw();
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.drawing_area);
    }
}
