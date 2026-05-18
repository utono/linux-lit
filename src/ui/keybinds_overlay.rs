use gtk4::prelude::*;
use gtk4::{DrawingArea, Overlay};
use std::cell::RefCell;
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
    key("[", "2", "prev para", "2: prev ch", &[]),
    key("{", "3", "next para", "3: next ch", &[]),
    ub("(", "4"),
    ub("&", "5"),
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
    key(";", ":", "next bkmk", "prev bkmk", &[]),
    key(",", "<", "prev dlg", "", &[("C-,", "settings")]),
    bare(".", ">", "set chapter"),
    key("p", "P", "nudge \u{2212}0.2", "P: +0.2", &[("C-p", "picker")]),
    bare("y", "Y", "prev chunk"),
    key("f", "F", "font \u{2192}", "F: \u{2190}", &[("C-f", "pg fwd"), ("M-f", "font info")]),
    key("g", "G", "", "", &[("C-g", "gloss tog"), ("A-g", "gloss pick")]),
    ub("c", "C"),
    key("r", "R", "next vocab", "R: prev vocab", &[]),
    key("l", "L", "toggle signs", "", &[("C-M-l", "save+quit")]),
    key("/", "?", "search", "", &[("C-/", "keybinds")]),
    ub("@", "^"),
    key("\\", "#", "vocab ▶", "◀ vocab", &[("C-\\", "conc picker"), ("M-\\", "vocab hi")]),
];
const TAB_KEY: KeyDef = bare("Tab", "", "play/pause");

const HOME_ROW: &[KeyDef] = &[
    bare("a", "A", "play from ts"),
    key("o", "O", "seek \u{2212}3.5", "O: \u{2212}60", &[]),
    key("e", "E", "seek +3.5", "E: +60", &[]),
    key("u", "U", "start time", "", &[("C-u", "pg back")]),
    key("i", "I", "translations", "", &[("M-i", "set end time")]),
    key("d", "D", "", "", &[("C-d", "debug log"), ("M-d", "dim tog")]),
    key("h", "H", "auto vocab", "H: synopsis", &[]),
    key("t", "T", "", "", &[("M-t", "title tog")]),
    key("n", "N", "next match", "N: prev match", &[]),
    bare("s", "S", "sync tog"),
    key("-", "_", "", "", &[("C--", "recent")]),
];
const ESC_KEY: KeyDef = bare("Esc", "", "clear AB");

const BOTTOM_ROW: &[KeyDef] = &[
    ub("'", "\""),
    bare("q", "Q", "next dlg"),
    bare("j", "J", "cursor \u{2193}"),
    bare("k", "K", "cursor \u{2191}"),
    bare("x", "X", "next chunk"),
    key("b", "B", "", "", &[("C-b", "pg back")]),
    key("m", "M", "bookmark", "", &[("C-m", "bookmarks"), ("C-S-m", "media picker")]),
    key("w", "W", "copy word", "W: collect", &[]),
    key("v", "V", "", "V: visual mode", &[]),
    ub("z", "Z"),
];

const SHIFT_KEY: KeyDef = ub("Shift", "");
const SPACEBAR_ROW_CTRL: KeyDef = ub("Ctrl", "");
const SPACEBAR_ROW_FN: KeyDef = ub("Fn", "");
const SPACEBAR_ROW_WIN: KeyDef = ub("Win", "");
const SPACEBAR_ROW_ALT_L: KeyDef = ub("Alt", "");
const SPACEBAR_ROW_SPACE: KeyDef = key("Space", "", "page ↓", "page ↑", &[]);
const SPACEBAR_ROW_ALT_R: KeyDef = ub("Alt", "");
const SPACEBAR_ROW_CTRL_R: KeyDef = ub("Ctrl", "");

const SEQ_GG: KeyDef = bare("gg", "", "go to start");
const SEQ_G: KeyDef = key("G", "", "", "go to end", &[]);
const SEQ_G_SEMI: KeyDef = bare("g;", "", "latest bkmk");

const ARROW_UP: KeyDef = key("\u{2191}", "", "cursor \u{2191}", "", &[("C-\u{2191}", "vol +")]);
const ARROW_DOWN: KeyDef = key("\u{2193}", "", "cursor \u{2193}", "", &[("C-\u{2193}", "vol \u{2212}")]);
const ARROW_LEFT: KeyDef = bare("\u{2190}", "", "seek \u{2212}3.5");
const ARROW_RIGHT: KeyDef = bare("\u{2192}", "", "start time");

// ── Layout constants ─────────────────────────────────────────────────

const KEY_W: f64 = 68.0;
const KEY_H: f64 = 66.0;
const GAP: f64 = 4.0;
const TAB_W: f64 = 102.0;
const ESC_W: f64 = 120.0;
const BKSP_W: f64 = 94.0;
const ARROW_W: f64 = 54.0;
const ARROW_H: f64 = 50.0;
const CORNER_R: f64 = 5.0;
const PAD: f64 = 20.0; // outer padding

// Row x-offsets (left edge of first alpha key, simulating physical stagger)
// Number row: starts at 0
// Upper row: Tab key then keys
// Home row: Esc key then keys
// Bottom row: shifted right past Esc+a

/// Total width of the keyboard area (number row = 13 keys + bksp + gaps)
const KB_W: f64 = 13.0 * KEY_W + BKSP_W + 13.0 * GAP;
/// Total rows: 4 main + spacebar row + gap + seq/arrow rows
const KB_H: f64 = 4.0 * KEY_H + 3.0 * GAP  // main rows
    + GAP + KEY_H                              // spacebar row
    + 12.0                                     // gap before seq row
    + KEY_H                                    // seq row
    + GAP + ARROW_H                            // arrow bottom row
    + 30.0;                                    // legend

// ── Computed key rectangles ──────────────────────────────────────────

struct KeyRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

struct AllKeys {
    defs: Vec<&'static KeyDef>,
    rects: Vec<KeyRect>,
}

fn build_layout() -> AllKeys {
    let mut defs: Vec<&'static KeyDef> = Vec::new();
    let mut rects: Vec<KeyRect> = Vec::new();

    // Helper: add a key
    let add = |x: f64, y: f64, w: f64, h: f64, def: &'static KeyDef,
                   defs: &mut Vec<&'static KeyDef>, rects: &mut Vec<KeyRect>| {
        defs.push(def);
        rects.push(KeyRect { x, y, w, h });
    };

    // Row 0: Number row
    let y0 = 0.0;
    let mut x = 0.0;
    for def in NUMBER_ROW {
        add(x, y0, KEY_W, KEY_H, def, &mut defs, &mut rects);
        x += KEY_W + GAP;
    }
    add(x, y0, BKSP_W, KEY_H, &BACKSPACE, &mut defs, &mut rects);

    // Row 1: Upper row (Tab + 13 keys)
    let y1 = y0 + KEY_H + GAP;
    x = 0.0;
    add(x, y1, TAB_W, KEY_H, &TAB_KEY, &mut defs, &mut rects);
    x += TAB_W + GAP;
    for def in UPPER_ROW {
        add(x, y1, KEY_W, KEY_H, def, &mut defs, &mut rects);
        x += KEY_W + GAP;
    }

    // Row 2: Home row (Esc + 11 keys)
    let y2 = y1 + KEY_H + GAP;
    x = 0.0;
    add(x, y2, ESC_W, KEY_H, &ESC_KEY, &mut defs, &mut rects);
    x += ESC_W + GAP;
    for def in HOME_ROW {
        add(x, y2, KEY_W, KEY_H, def, &mut defs, &mut rects);
        x += KEY_W + GAP;
    }

    // Row 3: Bottom row (Shift + alpha keys)
    let y3 = y2 + KEY_H + GAP;
    // 'a' starts at ESC_W + GAP = 124. Shift fills from 0 to just before '
    let bottom_alpha_offset = ESC_W + GAP + KEY_W * 0.45;
    let shift_w = bottom_alpha_offset - GAP;
    add(0.0, y3, shift_w, KEY_H, &SHIFT_KEY, &mut defs, &mut rects);
    x = bottom_alpha_offset;
    for def in BOTTOM_ROW {
        add(x, y3, KEY_W, KEY_H, def, &mut defs, &mut rects);
        x += KEY_W + GAP;
    }

    // Row 4: Spacebar row
    // Ctrl + Fn below Shift (equal width, splitting Shift's width)
    // Then Win, Alt, Space, Alt, Ctrl
    let y4 = y3 + KEY_H + GAP;
    let half_shift = (shift_w - GAP) / 2.0;
    add(0.0, y4, half_shift, KEY_H, &SPACEBAR_ROW_CTRL, &mut defs, &mut rects);
    add(half_shift + GAP, y4, half_shift, KEY_H, &SPACEBAR_ROW_FN, &mut defs, &mut rects);

    // Win and Alt between Shift area and spacebar
    let win_x = bottom_alpha_offset;
    add(win_x, y4, KEY_W, KEY_H, &SPACEBAR_ROW_WIN, &mut defs, &mut rects);
    add(win_x + KEY_W + GAP, y4, KEY_W, KEY_H, &SPACEBAR_ROW_ALT_L, &mut defs, &mut rects);

    // Spacebar: left edge under 'j' (bottom row index 2), right edge under 'm' (bottom row index 6)
    let j_x = bottom_alpha_offset + 2.0 * (KEY_W + GAP);                // left edge of 'j'
    let m_right = bottom_alpha_offset + 6.0 * (KEY_W + GAP) + KEY_W;    // right edge of 'm'
    add(j_x, y4, m_right - j_x, KEY_H, &SPACEBAR_ROW_SPACE, &mut defs, &mut rects);

    // Alt and Ctrl after spacebar
    add(m_right + GAP, y4, KEY_W, KEY_H, &SPACEBAR_ROW_ALT_R, &mut defs, &mut rects);
    add(m_right + GAP + KEY_W + GAP, y4, KEY_W, KEY_H, &SPACEBAR_ROW_CTRL_R, &mut defs, &mut rects);

    // Row 5: Sequences (gg, G, g;) + up arrow
    let y5 = y4 + KEY_H + 12.0;
    add(0.0, y5, KEY_W * 1.4, KEY_H, &SEQ_GG, &mut defs, &mut rects);
    add(KEY_W * 1.4 + GAP, y5, KEY_W, KEY_H, &SEQ_G, &mut defs, &mut rects);
    add(KEY_W * 1.4 + GAP + KEY_W + GAP, y5, KEY_W, KEY_H, &SEQ_G_SEMI, &mut defs, &mut rects);

    // Arrow keys — inverted T on far right
    // Bottom row of arrows: left, down, right
    let arrow_y_bottom = y5 + KEY_H + GAP;
    let arrow_right_edge = KB_W;
    let arrow_left_x = arrow_right_edge - 3.0 * ARROW_W - 2.0 * GAP;
    add(arrow_left_x, arrow_y_bottom, ARROW_W, ARROW_H, &ARROW_LEFT, &mut defs, &mut rects);
    add(arrow_left_x + ARROW_W + GAP, arrow_y_bottom, ARROW_W, ARROW_H, &ARROW_DOWN, &mut defs, &mut rects);
    add(arrow_left_x + 2.0 * (ARROW_W + GAP), arrow_y_bottom, ARROW_W, ARROW_H, &ARROW_RIGHT, &mut defs, &mut rects);

    // Up arrow: centered above down arrow
    let up_x = arrow_left_x + ARROW_W + GAP;
    add(up_x, y5, ARROW_W, ARROW_H, &ARROW_UP, &mut defs, &mut rects);

    AllKeys { defs, rects }
}

// ── Colors ───────────────────────────────────────────────────────────

fn key_colors(def: &KeyDef) -> ((f64, f64, f64), (f64, f64, f64), (f64, f64, f64)) {
    // Returns (bg, border, char_color)
    let has_bare = !def.action.is_empty();
    let has_shift = !def.shift_action.is_empty();
    let has_mod = !def.modifiers.is_empty();
    let bound = has_bare || has_shift || has_mod;
    if bound {
        ((0.949, 0.914, 0.882), (0.875, 0.855, 0.851), (0.341, 0.322, 0.475))
    } else {
        ((1.0, 0.98, 0.953), (0.949, 0.914, 0.882), (0.596, 0.576, 0.647))
    }
}

// ── Drawing ──────────────────────────────────────────────────────────

fn draw_keyboard(cr: &gtk4::cairo::Context, layout: &AllKeys, tooltip_idx: Option<usize>, widget_w: f64, widget_h: f64) {
    // Full-screen background
    cr.set_source_rgba(0.341, 0.322, 0.475, 0.95);
    cr.rectangle(0.0, 0.0, widget_w, widget_h);
    let _ = cr.fill();

    // Scale and center the keyboard within the full window
    let base_w = KB_W + 2.0 * PAD;
    let base_h = KB_H + 2.0 * PAD;
    let scale_x = widget_w / base_w;
    let scale_y = widget_h / base_h;
    let scale = scale_x.min(scale_y) * 0.92;
    let scaled_w = base_w * scale;
    let scaled_h = base_h * scale;
    let x_offset = (widget_w - scaled_w) / 2.0;
    let y_offset = (widget_h - scaled_h) / 2.0;
    cr.translate(x_offset, y_offset);
    cr.scale(scale, scale);

    cr.translate(PAD, PAD);

    for (i, rect) in layout.rects.iter().enumerate() {
        let def = layout.defs[i];
        let (bg, border, char_col) = key_colors(def);

        // Key background
        cr.set_source_rgb(bg.0, bg.1, bg.2);
        rounded_rect(cr, rect.x, rect.y, rect.w, rect.h, CORNER_R);
        let _ = cr.fill();

        // Key border
        cr.set_source_rgb(border.0, border.1, border.2);
        rounded_rect(cr, rect.x + 0.5, rect.y + 0.5, rect.w - 1.0, rect.h - 1.0, CORNER_R);
        cr.set_line_width(1.0);
        let _ = cr.stroke();

        // Unshifted character (bold, left side)
        cr.set_source_rgb(char_col.0, char_col.1, char_col.2);
        cr.set_font_size(22.0);
        cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
        let _ = cr.move_to(rect.x + 7.0, rect.y + 24.0);
        let _ = cr.show_text(def.unshifted);

        // Shifted character (small, top-right)
        if !def.shifted.is_empty() {
            let shifted_col = if !def.shift_action.is_empty() || (!def.action.is_empty() && !def.modifiers.is_empty()) {
                (0.565, 0.478, 0.663) // iris
            } else {
                (0.596, 0.576, 0.647) // muted/text_unbound
            };
            cr.set_source_rgb(shifted_col.0, shifted_col.1, shifted_col.2);
            cr.set_font_size(14.0);
            cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
            let extents = cr.text_extents(def.shifted).unwrap();
            let _ = cr.move_to(rect.x + rect.w - extents.width() - 7.0, rect.y + 17.0);
            let _ = cr.show_text(def.shifted);
        }

        // Action label (bare key)
        if !def.action.is_empty() {
            cr.set_source_rgb(0.157, 0.412, 0.514);
            cr.set_font_size(12.0);
            cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
            let _ = cr.move_to(rect.x + 7.0, rect.y + rect.h - 8.0);
            let _ = cr.show_text(def.action);
        }

        // Shift action label
        if !def.shift_action.is_empty() {
            cr.set_source_rgb(0.565, 0.478, 0.663);
            cr.set_font_size(11.0);
            cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
            let y_pos = if !def.action.is_empty() {
                rect.y + rect.h - 22.0
            } else {
                rect.y + rect.h - 8.0
            };
            let _ = cr.move_to(rect.x + 7.0, y_pos);
            let _ = cr.show_text(def.shift_action);
        }

        // Modifier action labels — rendered on the key face so the user
        // doesn't need to hover. Stack upward from the bare/shift labels.
        if !def.modifiers.is_empty() {
            cr.set_font_size(11.0);
            cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
            let base_slot = match (!def.action.is_empty(), !def.shift_action.is_empty()) {
                (true, true) => 2,
                (true, false) | (false, true) => 1,
                (false, false) => 0,
            };
            for (mi, (combo, act)) in def.modifiers.iter().enumerate() {
                let slot = base_slot + mi;
                let y_pos = rect.y + rect.h - 8.0 - slot as f64 * 14.0;
                if combo.starts_with("M-") && !combo.contains("C-") {
                    cr.set_source_rgb(0.706, 0.388, 0.478);
                } else {
                    cr.set_source_rgb(0.557, 0.420, 0.208);
                }
                let _ = cr.move_to(rect.x + 7.0, y_pos);
                let _ = cr.show_text(act);
            }
        }
    }

    // Draw tooltip if hovering a key with modifiers
    if let Some(idx) = tooltip_idx {
        if idx < layout.defs.len() {
            let def = layout.defs[idx];
            if !def.modifiers.is_empty() {
                let rect = &layout.rects[idx];
                draw_tooltip(cr, rect, def);
            }
        }
    }

    // Legend
    let legend_y = KB_H - 20.0;
    let legend_items: &[((f64, f64, f64), &str)] = &[
        ((0.949, 0.914, 0.882), "bare key"),
        ((0.949, 0.914, 0.882), "shift only"),
        ((0.949, 0.914, 0.882), "both / modifier"),
        ((1.0, 0.98, 0.953), "unbound"),
    ];
    let legend_colors: &[(f64, f64, f64)] = &[
        (0.157, 0.412, 0.514),  // pine
        (0.565, 0.478, 0.663),  // iris
        (0.475, 0.459, 0.576),  // subtle
        (0.596, 0.576, 0.647),  // muted
    ];

    let mut lx = 0.0;
    for (i, &(bg, label)) in legend_items.iter().enumerate() {
        // Swatch
        cr.set_source_rgb(bg.0, bg.1, bg.2);
        rounded_rect(cr, lx, legend_y, 16.0, 16.0, 3.0);
        let _ = cr.fill();

        // Label
        let tc = legend_colors[i];
        cr.set_source_rgb(tc.0, tc.1, tc.2);
        cr.set_font_size(13.0);
        cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
        let _ = cr.move_to(lx + 20.0, legend_y + 13.0);
        let _ = cr.show_text(label);
        let extents = cr.text_extents(label).unwrap();
        lx += 20.0 + extents.width() + 24.0;
    }

    // Ctrl+ indicator
    cr.set_source_rgb(0.557, 0.420, 0.208);
    cr.set_font_size(13.0);
    let _ = cr.move_to(lx, legend_y + 13.0);
    let _ = cr.show_text("\u{2022} Ctrl+");
    let extents = cr.text_extents("\u{2022} Ctrl+").unwrap();
    lx += extents.width() + 16.0;

    // Alt+ indicator
    cr.set_source_rgb(0.706, 0.388, 0.478);
    cr.set_font_size(13.0);
    let _ = cr.move_to(lx, legend_y + 13.0);
    let _ = cr.show_text("\u{2022} Alt+");

    // Close hint (right side)
    cr.set_source_rgb(0.475, 0.459, 0.576);
    cr.set_font_size(13.0);
    cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    let hint = "Esc to close \u{00b7} C-/ to toggle";
    let extents = cr.text_extents(hint).unwrap();
    let _ = cr.move_to(KB_W - extents.width(), legend_y + 13.0);
    let _ = cr.show_text(hint);
}

fn draw_tooltip(cr: &gtk4::cairo::Context, rect: &KeyRect, def: &KeyDef) {
    let lines: Vec<(&str, &str)> = def.modifiers.iter().map(|&(combo, action)| {
        (combo, action)
    }).collect();

    cr.set_font_size(13.0);
    cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);

    // Measure tooltip size
    let mut max_w: f64 = 0.0;
    for &(_, act) in &lines {
        let ext = cr.text_extents(act).unwrap();
        if ext.width() > max_w { max_w = ext.width(); }
    }
    let tt_w = max_w + 20.0;
    let tt_h = lines.len() as f64 * 18.0 + 12.0;
    let tt_x = rect.x + rect.w / 2.0 - tt_w / 2.0;
    let tt_y = rect.y - tt_h - 6.0;

    // Background
    cr.set_source_rgba(0.341, 0.322, 0.475, 0.95);
    rounded_rect(cr, tt_x, tt_y, tt_w, tt_h, 4.0);
    let _ = cr.fill();

    // Border
    cr.set_source_rgb(0.475, 0.459, 0.576);
    rounded_rect(cr, tt_x + 0.5, tt_y + 0.5, tt_w - 1.0, tt_h - 1.0, 4.0);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    // Text
    for (i, &(combo, act)) in lines.iter().enumerate() {
        if combo.starts_with("M-") && !combo.contains("C-") {
            cr.set_source_rgb(0.706, 0.388, 0.478);
        } else {
            cr.set_source_rgb(0.557, 0.420, 0.208);
        }
        let _ = cr.move_to(tt_x + 10.0, tt_y + 18.0 + i as f64 * 18.0);
        let _ = cr.show_text(act);
    }
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

// ── Hit testing ──────────────────────────────────────────────────────

fn hit_test(layout: &AllKeys, mx: f64, my: f64) -> Option<usize> {
    // Adjust for padding
    let x = mx - PAD;
    let y = my - PAD;
    for (i, rect) in layout.rects.iter().enumerate() {
        if x >= rect.x && x <= rect.x + rect.w && y >= rect.y && y <= rect.y + rect.h {
            if !layout.defs[i].modifiers.is_empty() {
                return Some(i);
            }
        }
    }
    None
}

// ── Public API ───────────────────────────────────────────────────────

pub struct KeybindsOverlay {
    pub overlay: Overlay,
    drawing_area: DrawingArea,
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

        let layout = Rc::new(build_layout());
        let hover_idx: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));

        // Draw function — fills entire widget, centers keyboard
        let layout_draw = layout.clone();
        let hover_draw = hover_idx.clone();
        drawing_area.set_draw_func(move |_area, cr, w, h| {
            let idx = *hover_draw.borrow();
            draw_keyboard(cr, &layout_draw, idx, w as f64, h as f64);
        });

        // Mouse motion for tooltips — un-scale coordinates
        let motion = gtk4::EventControllerMotion::new();
        let hover_motion = hover_idx.clone();
        let layout_motion = layout.clone();
        let da_motion = drawing_area.clone();
        motion.connect_motion(move |_controller, x, y| {
            let w = da_motion.width() as f64;
            let h = da_motion.height() as f64;
            let base_w = KB_W + 2.0 * PAD;
            let base_h = KB_H + 2.0 * PAD;
            let scale_x = w / base_w;
            let scale_y = h / base_h;
            let scale = scale_x.min(scale_y) * 0.92;
            let scaled_w = base_w * scale;
            let scaled_h = base_h * scale;
            let x_offset = (w - scaled_w) / 2.0;
            let y_offset = (h - scaled_h) / 2.0;

            // Convert mouse coords to layout space
            let lx = (x - x_offset) / scale;
            let ly = (y - y_offset) / scale;

            let new_idx = hit_test(&layout_motion, lx, ly);
            let old_idx = *hover_motion.borrow();
            if new_idx != old_idx {
                *hover_motion.borrow_mut() = new_idx;
                da_motion.queue_draw();
            }
        });

        let hover_leave = hover_idx;
        let da_leave = drawing_area.clone();
        motion.connect_leave(move |_controller| {
            *hover_leave.borrow_mut() = None;
            da_leave.queue_draw();
        });

        drawing_area.add_controller(motion);

        KeybindsOverlay { overlay, drawing_area }
    }

    pub fn show(&self) {
        self.drawing_area.set_visible(true);
    }

    pub fn hide(&self) {
        self.drawing_area.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.drawing_area.is_visible()
    }

    pub fn adjust_scale(&mut self, _delta: i32) {
        // Fixed size — no-op
    }

    pub fn reset_scale(&mut self) {
        // Fixed size — no-op
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.drawing_area);
    }
}
