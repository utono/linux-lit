//! Cairo-drawn 8BitDo Micro gamepad layout showing linux-lit button bindings.

use gtk4::prelude::*;
use gtk4::{DrawingArea, Overlay};

// ── Button definitions ───────────────────────────────────────────────

struct ButtonDef {
    label: &'static str,
    action: &'static str,
}

const fn btn(label: &'static str, action: &'static str) -> ButtonDef {
    ButtonDef { label, action }
}

const FACE_X: ButtonDef = btn("X", "prev dlg");
const FACE_A: ButtonDef = btn("A", "set start");
const FACE_B: ButtonDef = btn("B", "next dlg");
const FACE_Y: ButtonDef = btn("Y", "set chpt");

const DPAD_UP: ButtonDef = btn("", "nudge \u{2212}");
const DPAD_DOWN: ButtonDef = btn("", "nudge +");
const DPAD_LEFT: ButtonDef = btn("", "pg back");
const DPAD_RIGHT: ButtonDef = btn("", "pg fwd");

const BTN_L: ButtonDef = btn("L", "play/pause");
const BTN_R: ButtonDef = btn("R", "del ts");
const BTN_L2: ButtonDef = btn("L2", "");
const BTN_R2: ButtonDef = btn("R2", "play/pause");

const BTN_MINUS: ButtonDef = btn("\u{2212}", "play line");
const BTN_PLUS: ButtonDef = btn("+", "prev chpt");
const BTN_STAR: ButtonDef = btn("\u{2217}", "");
const BTN_HOME: ButtonDef = btn("H", "next chpt");

// ── Layout ──────────────────────────────────────────────────────────

const BODY_W: f64 = 800.0;
const BODY_H: f64 = 380.0;
const BODY_R: f64 = 30.0;

const SHOULDER_W: f64 = 130.0;
const SHOULDER_H: f64 = 28.0;

const DPAD_ARM: f64 = 30.0;
const DPAD_THICK: f64 = 30.0;

const FACE_R: f64 = 22.0;
const FACE_SPREAD: f64 = 36.0;

const MENU_R: f64 = 13.0;

const PAD: f64 = 30.0;
const ACTION_FONT: f64 = 15.0;
const GLYPH_FONT: f64 = 21.0;
const GLYPH_FONT_SM: f64 = 15.0;

const LEGEND_H: f64 = 44.0;
const TOTAL_W: f64 = BODY_W + 2.0 * PAD;
const TOTAL_H: f64 = BODY_H + LEGEND_H + 2.0 * PAD;

// ── Geometry ────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Shape { Shoulder, Circle, DpadArm(Dir) }

#[derive(Clone, Copy)]
enum Dir { Up, Down, Left, Right }

struct Btn {
    x: f64, y: f64, w: f64, h: f64,
    shape: Shape,
    def: &'static ButtonDef,
}

fn layout() -> Vec<Btn> {
    let mut v = Vec::new();

    // Shoulders — single row: L, L2 (adjacent) ... R2, R (adjacent)
    let sy = 20.0;
    let sx_start = 28.0;
    let small_w = SHOULDER_W / 3.0;
    let pair_gap = 4.0;
    let left_group_w = SHOULDER_W + pair_gap + small_w;
    let right_group_w = small_w + pair_gap + SHOULDER_W;
    let center_gap = BODY_W - 2.0 * sx_start - left_group_w - right_group_w;
    let mut sx = sx_start;
    v.push(Btn { x: sx, y: sy, w: SHOULDER_W, h: SHOULDER_H, shape: Shape::Shoulder, def: &BTN_L });
    sx += SHOULDER_W + pair_gap;
    v.push(Btn { x: sx, y: sy, w: small_w, h: SHOULDER_H, shape: Shape::Shoulder, def: &BTN_L2 });
    sx += small_w + center_gap;
    v.push(Btn { x: sx, y: sy, w: small_w, h: SHOULDER_H, shape: Shape::Shoulder, def: &BTN_R2 });
    sx += small_w + pair_gap;
    v.push(Btn { x: sx, y: sy, w: SHOULDER_W, h: SHOULDER_H, shape: Shape::Shoulder, def: &BTN_R });

    // D-pad — left side, vertically centered in lower 2/3 of body
    // Arms are positioned flush against the center square (no overlap needed —
    // the cross is drawn as a single filled shape in draw()).
    let dpad_cx = 160.0;
    let dpad_cy = BODY_H * 0.56;
    for &(dir, dx, dy) in &[
        (Dir::Up, 0.0, -(DPAD_THICK / 2.0 + DPAD_ARM)),
        (Dir::Down, 0.0, DPAD_THICK / 2.0),
        (Dir::Left, -(DPAD_THICK / 2.0 + DPAD_ARM), 0.0),
        (Dir::Right, DPAD_THICK / 2.0, 0.0),
    ] {
        let def = match dir { Dir::Up => &DPAD_UP, Dir::Down => &DPAD_DOWN, Dir::Left => &DPAD_LEFT, Dir::Right => &DPAD_RIGHT };
        let (w, h) = match dir { Dir::Up | Dir::Down => (DPAD_THICK, DPAD_ARM), Dir::Left | Dir::Right => (DPAD_ARM, DPAD_THICK) };
        v.push(Btn { x: dpad_cx - DPAD_THICK / 2.0 + dx, y: dpad_cy - DPAD_THICK / 2.0 + dy, w, h, shape: Shape::DpadArm(dir), def });
    }

    // Face buttons — right side, inset enough for "set chpt" label
    let face_cx = BODY_W - 160.0;
    let face_cy = BODY_H * 0.56;
    for &(def, dx, dy) in &[
        (&FACE_X as &ButtonDef, 0.0, -FACE_SPREAD),
        (&FACE_A, FACE_SPREAD, 0.0),
        (&FACE_B, 0.0, FACE_SPREAD),
        (&FACE_Y, -FACE_SPREAD, 0.0),
    ] {
        v.push(Btn { x: face_cx + dx, y: face_cy + dy, w: FACE_R * 2.0, h: FACE_R * 2.0, shape: Shape::Circle, def });
    }

    // Menu buttons — shifted left of center to avoid face button labels
    let mcx = BODY_W * 0.50;
    let mcy = BODY_H * 0.56;
    let mgx = 40.0;
    let mgy = 32.0;
    for &(def, dx, dy) in &[
        (&BTN_MINUS as &ButtonDef, -mgx, -mgy),
        (&BTN_PLUS, mgx, -mgy),
        (&BTN_STAR, -mgx, mgy),
        (&BTN_HOME, mgx, mgy),
    ] {
        v.push(Btn { x: mcx + dx, y: mcy + dy, w: MENU_R * 2.0, h: MENU_R * 2.0, shape: Shape::Circle, def });
    }

    v
}

// ── Colors ──────────────────────────────────────────────────────────

const BG: (f64, f64, f64, f64) = (0.341, 0.322, 0.475, 0.95);
const BODY_FILL: (f64, f64, f64) = (0.88, 0.86, 0.86);
const BODY_STROKE: (f64, f64, f64) = (0.35, 0.33, 0.38);
const PART_BOUND: (f64, f64, f64) = (0.949, 0.914, 0.882);
const PART_BOUND_BORDER: (f64, f64, f64) = (0.475, 0.459, 0.576);
const PART_UNBOUND: (f64, f64, f64) = (0.78, 0.76, 0.78);
const PART_UNBOUND_BORDER: (f64, f64, f64) = (0.60, 0.58, 0.62);
const LABEL_BOUND: (f64, f64, f64) = (0.341, 0.322, 0.475);
const LABEL_UNBOUND: (f64, f64, f64) = (0.45, 0.44, 0.50);
const ACTION_COL: (f64, f64, f64) = (0.157, 0.412, 0.514);

fn colors(bound: bool) -> ((f64, f64, f64), (f64, f64, f64), (f64, f64, f64)) {
    if bound { (PART_BOUND, PART_BOUND_BORDER, LABEL_BOUND) }
    else { (PART_UNBOUND, PART_UNBOUND_BORDER, LABEL_UNBOUND) }
}

// ── Drawing ─────────────────────────────────────────────────────────

fn draw(cr: &gtk4::cairo::Context, btns: &[Btn], ww: f64, wh: f64) {
    let pi2 = 2.0 * std::f64::consts::PI;

    // Full-screen backdrop
    cr.set_source_rgba(BG.0, BG.1, BG.2, BG.3);
    cr.rectangle(0.0, 0.0, ww, wh);
    let _ = cr.fill();

    // Scale and center
    let sx = ww / TOTAL_W;
    let sy = wh / TOTAL_H;
    let s = sx.min(sy) * 0.98;
    cr.translate((ww - TOTAL_W * s) / 2.0, (wh - TOTAL_H * s) / 2.0);
    cr.scale(s, s);
    cr.translate(PAD, PAD);

    // Body chassis
    cr.set_source_rgb(BODY_FILL.0, BODY_FILL.1, BODY_FILL.2);
    rrect(cr, 0.0, 0.0, BODY_W, BODY_H, BODY_R);
    let _ = cr.fill();
    cr.set_source_rgb(BODY_STROKE.0, BODY_STROKE.1, BODY_STROKE.2);
    rrect(cr, 0.5, 0.5, BODY_W - 1.0, BODY_H - 1.0, BODY_R);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    // Power LED (top center)
    cr.set_source_rgb(0.55, 0.58, 0.62);
    cr.arc(BODY_W / 2.0, 14.0, 3.5, 0.0, pi2);
    let _ = cr.fill();

    // Mode switch (bottom-left)
    cr.set_source_rgb(0.60, 0.58, 0.62);
    rrect(cr, 36.0, BODY_H - 14.0, 50.0, 8.0, 2.0);
    let _ = cr.fill();

    // Pair button (bottom center)
    cr.set_source_rgb(0.60, 0.58, 0.62);
    cr.arc(BODY_W / 2.0, BODY_H - 10.0, 4.0, 0.0, pi2);
    let _ = cr.fill();

    // D-pad — draw as a single filled cross shape
    let dpad_cx = 160.0;
    let dpad_cy = BODY_H * 0.56;
    let half_t = DPAD_THICK / 2.0;
    let arm_len = DPAD_ARM;
    let (cfill, cborder, _) = colors(true);
    cr.set_source_rgb(cfill.0, cfill.1, cfill.2);
    cr.new_path();
    // Start at top-left of the up arm, go clockwise
    cr.move_to(dpad_cx - half_t, dpad_cy - half_t - arm_len);
    cr.line_to(dpad_cx + half_t, dpad_cy - half_t - arm_len);
    cr.line_to(dpad_cx + half_t, dpad_cy - half_t);
    cr.line_to(dpad_cx + half_t + arm_len, dpad_cy - half_t);
    cr.line_to(dpad_cx + half_t + arm_len, dpad_cy + half_t);
    cr.line_to(dpad_cx + half_t, dpad_cy + half_t);
    cr.line_to(dpad_cx + half_t, dpad_cy + half_t + arm_len);
    cr.line_to(dpad_cx - half_t, dpad_cy + half_t + arm_len);
    cr.line_to(dpad_cx - half_t, dpad_cy + half_t);
    cr.line_to(dpad_cx - half_t - arm_len, dpad_cy + half_t);
    cr.line_to(dpad_cx - half_t - arm_len, dpad_cy - half_t);
    cr.line_to(dpad_cx - half_t, dpad_cy - half_t);
    cr.close_path();
    let _ = cr.fill_preserve();
    cr.set_source_rgb(cborder.0, cborder.1, cborder.2);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    // Buttons
    for b in btns {
        let bound = !b.def.action.is_empty();
        let (fill, border, lcol) = colors(bound);

        match b.shape {
            Shape::Shoulder => {
                cr.set_source_rgb(fill.0, fill.1, fill.2);
                rrect(cr, b.x, b.y, b.w, b.h, 8.0);
                let _ = cr.fill();
                cr.set_source_rgb(border.0, border.1, border.2);
                rrect(cr, b.x + 0.5, b.y + 0.5, b.w - 1.0, b.h - 1.0, 8.0);
                cr.set_line_width(1.0);
                let _ = cr.stroke();
            }
            Shape::Circle => {
                let r = b.w / 2.0;
                cr.set_source_rgb(fill.0, fill.1, fill.2);
                cr.arc(b.x, b.y, r, 0.0, pi2);
                let _ = cr.fill();
                cr.set_source_rgb(border.0, border.1, border.2);
                cr.arc(b.x, b.y, r - 0.5, 0.0, pi2);
                cr.set_line_width(1.0);
                let _ = cr.stroke();
            }
            Shape::DpadArm(_) => {
                // Cross shape already drawn; individual arms are just for arrows + labels
            }
        }

        // Glyph / label for shoulders: show action inside when bound, else glyph
        if matches!(b.shape, Shape::Shoulder) {
            if bound {
                cr.set_source_rgb(ACTION_COL.0, ACTION_COL.1, ACTION_COL.2);
                cr.set_font_size(ACTION_FONT);
                cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
                let e = cr.text_extents(b.def.action).unwrap();
                let gx = b.x + b.w / 2.0 - e.width() / 2.0 - e.x_bearing();
                let gy = b.y + b.h / 2.0 - (e.y_bearing() + e.height() / 2.0);
                let _ = cr.move_to(gx, gy);
                let _ = cr.show_text(b.def.action);
            } else if !b.def.label.is_empty() {
                cr.set_source_rgb(lcol.0, lcol.1, lcol.2);
                cr.set_font_size(GLYPH_FONT);
                cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
                let e = cr.text_extents(b.def.label).unwrap();
                let gx = b.x + b.w / 2.0 - e.width() / 2.0 - e.x_bearing();
                let gy = b.y + b.h / 2.0 - (e.y_bearing() + e.height() / 2.0);
                let _ = cr.move_to(gx, gy);
                let _ = cr.show_text(b.def.label);
            }
        } else if matches!(b.shape, Shape::DpadArm(_)) {
            // no glyph — the cross shape is enough
        } else if !b.def.label.is_empty() {
            cr.set_source_rgb(lcol.0, lcol.1, lcol.2);
            let fs = if matches!(b.shape, Shape::Circle) && b.w < 40.0 { GLYPH_FONT_SM } else { GLYPH_FONT };
            cr.set_font_size(fs);
            cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
            let e = cr.text_extents(b.def.label).unwrap();
            let (gx, gy) = (
                b.x - e.width() / 2.0 - e.x_bearing(),
                b.y - (e.y_bearing() + e.height() / 2.0),
            );
            let _ = cr.move_to(gx, gy);
            let _ = cr.show_text(b.def.label);
        }

        // Action label (skip shoulders — handled above)
        if bound && !matches!(b.shape, Shape::Shoulder) {
            cr.set_source_rgb(ACTION_COL.0, ACTION_COL.1, ACTION_COL.2);
            cr.set_font_size(ACTION_FONT);
            cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
            let ae = cr.text_extents(b.def.action).unwrap();
            let (ax, ay) = action_pos(b, ae.width());
            let _ = cr.move_to(ax, ay);
            let _ = cr.show_text(b.def.action);
        }
    }

    // Legend
    let ly = BODY_H + 16.0;
    cr.set_font_size(13.0);
    cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    let mut lx = 0.0;
    for &(bound, label) in &[(true, "bound"), (false, "unbound")] {
        let (sw, _, _) = colors(bound);
        cr.set_source_rgb(sw.0, sw.1, sw.2);
        rrect(cr, lx, ly, 14.0, 14.0, 3.0);
        let _ = cr.fill();
        cr.set_source_rgb(0.93, 0.93, 0.96);
        let _ = cr.move_to(lx + 20.0, ly + 12.0);
        let _ = cr.show_text(label);
        lx += 20.0 + cr.text_extents(label).unwrap().width() + 20.0;
    }
    cr.set_source_rgb(0.75, 0.74, 0.83);
    let t = "8BitDo Micro";
    let te = cr.text_extents(t).unwrap();
    let _ = cr.move_to(BODY_W - te.width(), ly + 12.0);
    let _ = cr.show_text(t);
    let h = "Esc to close \u{00b7} n/p cycle overlays";
    let he = cr.text_extents(h).unwrap();
    let _ = cr.move_to(BODY_W - he.width(), ly + 30.0);
    let _ = cr.show_text(h);
}

fn action_pos(b: &Btn, lw: f64) -> (f64, f64) {
    match b.shape {
        Shape::Circle => {
            let r = b.w / 2.0;
            if r >= FACE_R - 0.1 {
                match b.def.label {
                    "X" => (b.x - lw / 2.0, b.y - r - 8.0),
                    "B" => (b.x - lw / 2.0, b.y + r + 18.0),
                    "Y" => (b.x - r - lw - 10.0, b.y + 5.0),
                    "A" => (b.x + r + 10.0, b.y + 5.0),
                    _ => (b.x - lw / 2.0, b.y + r + 16.0),
                }
            } else {
                (b.x - lw / 2.0, b.y + r + 16.0)
            }
        }
        Shape::DpadArm(Dir::Up) => (b.x + b.w / 2.0 - lw / 2.0, b.y - 10.0),
        Shape::DpadArm(Dir::Down) => (b.x + b.w / 2.0 - lw / 2.0, b.y + b.h + DPAD_THICK / 2.0 + 18.0),
        Shape::DpadArm(Dir::Left) => (b.x - lw - 10.0, b.y + b.h / 2.0 + DPAD_THICK / 4.0 + 5.0),
        Shape::DpadArm(Dir::Right) => (b.x + b.w + DPAD_THICK / 2.0 + 10.0, b.y + b.h / 2.0 + DPAD_THICK / 4.0 + 5.0),
        Shape::Shoulder => {
            (b.x + b.w / 2.0 - lw / 2.0, b.y + b.h / 2.0 + 5.0)
        }
    }
}

fn rrect(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let pi = std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
    cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
    cr.arc(x + r, y + r, r, pi, 3.0 * pi / 2.0);
    cr.close_path();
}

// ── Public API ──────────────────────────────────────────────────────

pub struct GamepadOverlay {
    pub overlay: Overlay,
    drawing_area: DrawingArea,
}

impl GamepadOverlay {
    pub fn new() -> Self {
        let overlay = Overlay::new();
        let drawing_area = DrawingArea::builder()
            .hexpand(true).vexpand(true)
            .halign(gtk4::Align::Fill).valign(gtk4::Align::Fill)
            .visible(false).build();
        drawing_area.add_css_class("gamepad-overlay-canvas");
        drawing_area.set_draw_func(move |_a, cr, w, h| {
            let l = layout();
            draw(cr, &l, w as f64, h as f64);
        });
        GamepadOverlay { overlay, drawing_area }
    }

    pub fn show(&self) { self.drawing_area.set_visible(true); }
    pub fn hide(&self) { self.drawing_area.set_visible(false); }
    pub fn is_visible(&self) -> bool { self.drawing_area.is_visible() }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.drawing_area);
    }
}
