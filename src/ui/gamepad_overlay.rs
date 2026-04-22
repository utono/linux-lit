//! Cairo-drawn 8BitDo Micro gamepad layout showing linux-lit button bindings.
//!
//! Matches the physical layout in the official manual:
//! - Horizontal rectangular body.
//! - D-pad (plus shape) on the left.
//! - Face buttons Y/A/B/X in a Switch-style diamond on the right
//!   (Y = top, A = right, B = bottom, X = left).
//! - Four small buttons between the D-pad and the face diamond:
//!   minus / plus / star / home.
//! - L and R shoulders above the body, with L2/R2 triggers behind them.
//! - Power-status LED at the top center.
//! - Mode switch and pair button on the bottom edge (drawn but unbound).

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

// Face buttons. Physical positions match the Switch layout in the manual.
// evdev labels (Xbox convention) do not match physical labels:
//   BTN_NORTH → physical X (top)
//   BTN_EAST  → physical A (right)
//   BTN_SOUTH → physical B (bottom)
//   BTN_WEST  → physical Y (left)
// But the 8BitDo Micro manual shows Y/A/B/X positions as a Nintendo
// Switch pad, which is what this overlay renders.
const FACE_X: ButtonDef = btn("X", "prev dlg");      // top (BTN_NORTH)
const FACE_A: ButtonDef = btn("A", "set chapter");   // right (BTN_SOUTH physical = bottom but see below)
const FACE_B: ButtonDef = btn("B", "next dlg");      // bottom (BTN_EAST physical = right)
const FACE_Y: ButtonDef = btn("Y", "play/pause");    // left (BTN_WEST)

// D-pad
// Arrow glyphs (U+2190..2193) don't render in the sans-serif font used
// here, so we leave the D-pad arms blank — the plus shape already conveys
// direction, and action labels sit just outside each arm.
const DPAD_UP: ButtonDef = btn("", "toggle speed");
const DPAD_DOWN: ButtonDef = btn("", "translations");
const DPAD_LEFT: ButtonDef = btn("", "seek \u{2212}3.5");
const DPAD_RIGHT: ButtonDef = btn("", "start ts");

// Shoulders / triggers.
const BTN_L: ButtonDef = btn("L", "");
const BTN_R: ButtonDef = btn("R", "");
const BTN_L2: ButtonDef = btn("L2", "");
const BTN_R2: ButtonDef = btn("R2", "");

// Menu buttons (small, between d-pad and face diamond)
const BTN_MINUS: ButtonDef = btn("-", "sync tog");      // Select
const BTN_PLUS: ButtonDef = btn("+", "prev chapter");   // Start
const BTN_STAR: ButtonDef = btn("*", "");               // no evdev event
const BTN_HOME: ButtonDef = btn("H", "next chapter");

// ── Layout constants ────────────────────────────────────────────────

const BODY_W: f64 = 820.0;
const BODY_H: f64 = 270.0;
const BODY_CORNER: f64 = 20.0;

const SHOULDER_W: f64 = 110.0;
const SHOULDER_H: f64 = 34.0;
const SHOULDER_GAP: f64 = 6.0;

const DPAD_ARM: f64 = 44.0;       // length of each arm of the plus
const DPAD_THICK: f64 = 54.0;     // width of the crossbar of the plus

const FACE_R: f64 = 36.0;         // face button radius
const FACE_OFFSET: f64 = 52.0;    // distance from diamond center to face button center

const MENU_R: f64 = 14.0;         // radius of small round menu buttons

const CORNER_R: f64 = 4.0;
const PAD: f64 = 24.0;

const KB_W: f64 = BODY_W;
const KB_H: f64 = BODY_H + SHOULDER_H * 2.0 + SHOULDER_GAP + 48.0; // + legend

// ── Button geometry ──────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum ButtonShape {
    Rect,
    RectShoulder,
    Circle,
    DpadArm(DpadDir),
}

#[derive(Clone, Copy)]
enum DpadDir {
    Up,
    Down,
    Left,
    Right,
}

struct ButtonRect {
    x: f64, // top-left for Rect shapes, center for Circle
    y: f64,
    w: f64,
    h: f64,
    shape: ButtonShape,
    def: &'static ButtonDef,
}

fn build_layout() -> Vec<ButtonRect> {
    let mut v: Vec<ButtonRect> = Vec::new();

    let body_y = SHOULDER_H * 2.0 + SHOULDER_GAP;

    // Shoulders + triggers. Row 0 (top) = L2 / R2, row 1 (bottom) = L / R.
    // In the manual L2 sits behind/above L (printed as overlapping tabs), so
    // we stack them vertically here.
    v.push(ButtonRect {
        x: 46.0, y: 0.0,
        w: SHOULDER_W, h: SHOULDER_H,
        shape: ButtonShape::RectShoulder, def: &BTN_L2,
    });
    v.push(ButtonRect {
        x: 46.0, y: SHOULDER_H + SHOULDER_GAP,
        w: SHOULDER_W, h: SHOULDER_H,
        shape: ButtonShape::RectShoulder, def: &BTN_L,
    });
    v.push(ButtonRect {
        x: BODY_W - SHOULDER_W - 46.0, y: 0.0,
        w: SHOULDER_W, h: SHOULDER_H,
        shape: ButtonShape::RectShoulder, def: &BTN_R2,
    });
    v.push(ButtonRect {
        x: BODY_W - SHOULDER_W - 46.0, y: SHOULDER_H + SHOULDER_GAP,
        w: SHOULDER_W, h: SHOULDER_H,
        shape: ButtonShape::RectShoulder, def: &BTN_R,
    });

    // D-pad plus on the left of the body.
    let dpad_cx = 140.0;
    let dpad_cy = body_y + BODY_H / 2.0;
    // Up arm
    v.push(ButtonRect {
        x: dpad_cx - DPAD_THICK / 2.0,
        y: dpad_cy - DPAD_THICK / 2.0 - DPAD_ARM,
        w: DPAD_THICK, h: DPAD_ARM,
        shape: ButtonShape::DpadArm(DpadDir::Up), def: &DPAD_UP,
    });
    // Down arm
    v.push(ButtonRect {
        x: dpad_cx - DPAD_THICK / 2.0,
        y: dpad_cy + DPAD_THICK / 2.0,
        w: DPAD_THICK, h: DPAD_ARM,
        shape: ButtonShape::DpadArm(DpadDir::Down), def: &DPAD_DOWN,
    });
    // Left arm
    v.push(ButtonRect {
        x: dpad_cx - DPAD_THICK / 2.0 - DPAD_ARM,
        y: dpad_cy - DPAD_THICK / 2.0,
        w: DPAD_ARM, h: DPAD_THICK,
        shape: ButtonShape::DpadArm(DpadDir::Left), def: &DPAD_LEFT,
    });
    // Right arm
    v.push(ButtonRect {
        x: dpad_cx + DPAD_THICK / 2.0,
        y: dpad_cy - DPAD_THICK / 2.0,
        w: DPAD_ARM, h: DPAD_THICK,
        shape: ButtonShape::DpadArm(DpadDir::Right), def: &DPAD_RIGHT,
    });

    // Face buttons in Switch-style diamond on the right of the body.
    let face_cx = BODY_W - 140.0;
    let face_cy = body_y + BODY_H / 2.0;
    // Y top (BTN_WEST in evdev, but physically drawn at top per manual).
    // Wait — the manual shows Y at top-LEFT of the diamond. Re-read carefully.
    // Switch Pro Controller diamond: X top, Y left, A right, B bottom.
    // The 8BitDo Micro manual diagram shows: Y top, X right, A bottom, B left.
    // Our evtest labels came out: BTN_NORTH=X, BTN_EAST=B, BTN_SOUTH=A, BTN_WEST=Y.
    // To be consistent with the evdev feedback (which is what the code binds),
    // draw: X top, B right, A bottom, Y left.
    v.push(ButtonRect {
        x: face_cx, y: face_cy - FACE_OFFSET,
        w: FACE_R * 2.0, h: FACE_R * 2.0,
        shape: ButtonShape::Circle, def: &FACE_X,
    });
    v.push(ButtonRect {
        x: face_cx + FACE_OFFSET, y: face_cy,
        w: FACE_R * 2.0, h: FACE_R * 2.0,
        shape: ButtonShape::Circle, def: &FACE_A,
    });
    v.push(ButtonRect {
        x: face_cx, y: face_cy + FACE_OFFSET,
        w: FACE_R * 2.0, h: FACE_R * 2.0,
        shape: ButtonShape::Circle, def: &FACE_B,
    });
    v.push(ButtonRect {
        x: face_cx - FACE_OFFSET, y: face_cy,
        w: FACE_R * 2.0, h: FACE_R * 2.0,
        shape: ButtonShape::Circle, def: &FACE_Y,
    });

    // Menu buttons in the center. Top row: minus, plus. Bottom row: star, home.
    let menu_cx = BODY_W / 2.0;
    let menu_row_top = body_y + BODY_H / 2.0 - 28.0;
    let menu_row_bot = body_y + BODY_H / 2.0 + 28.0;
    let menu_dx = 34.0;
    v.push(ButtonRect {
        x: menu_cx - menu_dx, y: menu_row_top,
        w: MENU_R * 2.0, h: MENU_R * 2.0,
        shape: ButtonShape::Circle, def: &BTN_MINUS,
    });
    v.push(ButtonRect {
        x: menu_cx + menu_dx, y: menu_row_top,
        w: MENU_R * 2.0, h: MENU_R * 2.0,
        shape: ButtonShape::Circle, def: &BTN_PLUS,
    });
    v.push(ButtonRect {
        x: menu_cx - menu_dx, y: menu_row_bot,
        w: MENU_R * 2.0, h: MENU_R * 2.0,
        shape: ButtonShape::Circle, def: &BTN_STAR,
    });
    v.push(ButtonRect {
        x: menu_cx + menu_dx, y: menu_row_bot,
        w: MENU_R * 2.0, h: MENU_R * 2.0,
        shape: ButtonShape::Circle, def: &BTN_HOME,
    });

    v
}

// ── Colors ───────────────────────────────────────────────────────────

const BG: (f64, f64, f64, f64) = (0.341, 0.322, 0.475, 0.95);      // backdrop
const BODY_FILL: (f64, f64, f64) = (0.88, 0.86, 0.86);             // pad chassis
const BODY_STROKE: (f64, f64, f64) = (0.35, 0.33, 0.38);
const PART_BOUND: (f64, f64, f64) = (0.949, 0.914, 0.882);
const PART_BOUND_BORDER: (f64, f64, f64) = (0.475, 0.459, 0.576);
const PART_UNBOUND: (f64, f64, f64) = (0.78, 0.76, 0.78);
const PART_UNBOUND_BORDER: (f64, f64, f64) = (0.60, 0.58, 0.62);
const LABEL_BOUND: (f64, f64, f64) = (0.341, 0.322, 0.475);
const LABEL_UNBOUND: (f64, f64, f64) = (0.45, 0.44, 0.50);
const ACTION_COLOR: (f64, f64, f64) = (0.157, 0.412, 0.514);

fn part_colors(bound: bool) -> ((f64, f64, f64), (f64, f64, f64), (f64, f64, f64)) {
    if bound {
        (PART_BOUND, PART_BOUND_BORDER, LABEL_BOUND)
    } else {
        (PART_UNBOUND, PART_UNBOUND_BORDER, LABEL_UNBOUND)
    }
}

// ── Drawing ──────────────────────────────────────────────────────────

fn draw_gamepad(cr: &gtk4::cairo::Context, layout: &[ButtonRect]) {
    let total_w = KB_W + 2.0 * PAD;
    let total_h = KB_H + 2.0 * PAD;

    // Backdrop
    cr.set_source_rgba(BG.0, BG.1, BG.2, BG.3);
    rounded_rect(cr, 0.0, 0.0, total_w, total_h, 14.0);
    let _ = cr.fill();

    cr.translate(PAD, PAD);

    // Pad chassis
    let body_y = SHOULDER_H * 2.0 + SHOULDER_GAP;
    cr.set_source_rgb(BODY_FILL.0, BODY_FILL.1, BODY_FILL.2);
    rounded_rect(cr, 0.0, body_y, BODY_W, BODY_H, BODY_CORNER);
    let _ = cr.fill();
    cr.set_source_rgb(BODY_STROKE.0, BODY_STROKE.1, BODY_STROKE.2);
    rounded_rect(cr, 0.5, body_y + 0.5, BODY_W - 1.0, BODY_H - 1.0, BODY_CORNER);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    // Power LED (top center of body)
    let led_x = BODY_W / 2.0;
    let led_y = body_y + 16.0;
    cr.set_source_rgb(0.55, 0.58, 0.62);
    cr.arc(led_x, led_y, 4.0, 0.0, 2.0 * std::f64::consts::PI);
    let _ = cr.fill();

    // Mode switch (bottom-left edge)
    cr.set_source_rgb(0.60, 0.58, 0.62);
    rounded_rect(
        cr, 40.0, body_y + BODY_H - 14.0, 70.0, 10.0, 2.0,
    );
    let _ = cr.fill();

    // Pair button (bottom center)
    cr.set_source_rgb(0.60, 0.58, 0.62);
    cr.arc(BODY_W / 2.0, body_y + BODY_H - 12.0, 5.0, 0.0, 2.0 * std::f64::consts::PI);
    let _ = cr.fill();

    // All interactive parts.
    for part in layout {
        let bound = !part.def.action.is_empty();
        let (fill, border, label_col) = part_colors(bound);

        match part.shape {
            ButtonShape::Rect => {
                cr.set_source_rgb(fill.0, fill.1, fill.2);
                rounded_rect(cr, part.x, part.y, part.w, part.h, CORNER_R);
                let _ = cr.fill();
                cr.set_source_rgb(border.0, border.1, border.2);
                rounded_rect(cr, part.x + 0.5, part.y + 0.5, part.w - 1.0, part.h - 1.0, CORNER_R);
                cr.set_line_width(1.0);
                let _ = cr.stroke();
            }
            ButtonShape::RectShoulder => {
                // Shoulders use a wider corner radius to suggest the curved
                // top edge of the physical pad.
                cr.set_source_rgb(fill.0, fill.1, fill.2);
                rounded_rect(cr, part.x, part.y, part.w, part.h, 10.0);
                let _ = cr.fill();
                cr.set_source_rgb(border.0, border.1, border.2);
                rounded_rect(cr, part.x + 0.5, part.y + 0.5, part.w - 1.0, part.h - 1.0, 10.0);
                cr.set_line_width(1.0);
                let _ = cr.stroke();
            }
            ButtonShape::Circle => {
                let cx = part.x;
                let cy = part.y;
                let r = part.w / 2.0;
                cr.set_source_rgb(fill.0, fill.1, fill.2);
                cr.arc(cx, cy, r, 0.0, 2.0 * std::f64::consts::PI);
                let _ = cr.fill();
                cr.set_source_rgb(border.0, border.1, border.2);
                cr.arc(cx, cy, r - 0.5, 0.0, 2.0 * std::f64::consts::PI);
                cr.set_line_width(1.0);
                let _ = cr.stroke();
            }
            ButtonShape::DpadArm(_dir) => {
                // Draw each arm as a plain rounded rect; when combined
                // visually with the other three arms they form a plus.
                cr.set_source_rgb(fill.0, fill.1, fill.2);
                rounded_rect(cr, part.x, part.y, part.w, part.h, 4.0);
                let _ = cr.fill();
                cr.set_source_rgb(border.0, border.1, border.2);
                rounded_rect(cr, part.x + 0.5, part.y + 0.5, part.w - 1.0, part.h - 1.0, 4.0);
                cr.set_line_width(1.0);
                let _ = cr.stroke();
            }
        }

        // Glyph on the button face.
        let (is_circle, cx, cy, r) = match part.shape {
            ButtonShape::Circle => (true, part.x, part.y, part.w / 2.0),
            _ => (false, 0.0, 0.0, 0.0),
        };
        cr.set_source_rgb(label_col.0, label_col.1, label_col.2);
        let glyph_size = if matches!(part.shape, ButtonShape::Circle) && part.w < 40.0 {
            16.0
        } else {
            20.0
        };
        cr.set_font_size(glyph_size);
        cr.select_font_face(
            "sans-serif",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        let ext = cr.text_extents(part.def.label).unwrap();
        let (lx, ly) = if is_circle {
            (cx - ext.width() / 2.0 - ext.x_bearing(),
             cy - (ext.y_bearing() + ext.height() / 2.0))
        } else {
            match part.shape {
                ButtonShape::DpadArm(DpadDir::Up) => (
                    part.x + part.w / 2.0 - ext.width() / 2.0,
                    part.y + part.h - 8.0,
                ),
                ButtonShape::DpadArm(DpadDir::Down) => (
                    part.x + part.w / 2.0 - ext.width() / 2.0,
                    part.y + 20.0,
                ),
                ButtonShape::DpadArm(DpadDir::Left) => (
                    part.x + part.w - ext.width() - 8.0,
                    part.y + part.h / 2.0 + 6.0,
                ),
                ButtonShape::DpadArm(DpadDir::Right) => (
                    part.x + 8.0,
                    part.y + part.h / 2.0 + 6.0,
                ),
                _ => (
                    part.x + part.w / 2.0 - ext.width() / 2.0,
                    part.y + part.h / 2.0 + 6.0,
                ),
            }
        };
        let _ = cr.move_to(lx, ly);
        let _ = cr.show_text(part.def.label);

        // Action label — placed outside the button so nothing overlaps.
        if bound {
            cr.set_source_rgb(ACTION_COLOR.0, ACTION_COLOR.1, ACTION_COLOR.2);
            cr.set_font_size(11.0);
            cr.select_font_face(
                "sans-serif",
                gtk4::cairo::FontSlant::Normal,
                gtk4::cairo::FontWeight::Normal,
            );
            let a_ext = cr.text_extents(part.def.action).unwrap();
            let (ax, ay) = action_label_pos(part, a_ext.width());
            let _ = cr.move_to(ax, ay);
            let _ = cr.show_text(part.def.action);
        }
        let _ = (r,); // silence unused when we don't need it
    }

    // Legend bar.
    let legend_y = body_y + BODY_H + 22.0;
    let legend_items: &[(bool, &str)] = &[(true, "bound"), (false, "unbound")];
    let mut lx = 0.0;
    for &(bound, label) in legend_items {
        let (sw, _, _) = part_colors(bound);
        cr.set_source_rgb(sw.0, sw.1, sw.2);
        rounded_rect(cr, lx, legend_y, 16.0, 16.0, 3.0);
        let _ = cr.fill();

        cr.set_source_rgb(0.93, 0.93, 0.96);
        cr.set_font_size(13.0);
        cr.select_font_face(
            "sans-serif",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        let _ = cr.move_to(lx + 22.0, legend_y + 13.0);
        let _ = cr.show_text(label);
        let extents = cr.text_extents(label).unwrap();
        lx += 22.0 + extents.width() + 22.0;
    }

    // Title on the right of the legend.
    cr.set_source_rgb(0.75, 0.74, 0.83);
    cr.set_font_size(13.0);
    let title = "8BitDo Micro gamepad";
    let ext = cr.text_extents(title).unwrap();
    let _ = cr.move_to(KB_W - ext.width(), legend_y + 13.0);
    let _ = cr.show_text(title);
}

fn action_label_pos(part: &ButtonRect, label_w: f64) -> (f64, f64) {
    match part.shape {
        ButtonShape::Circle => {
            let r = part.w / 2.0;
            // Face-button diamond: push the label outward so the four
            // circles' labels don't overlap. Detect face buttons by radius
            // (menu buttons are smaller).
            if r >= FACE_R - 0.1 {
                // Diamond positions: X=top, A=right, B=bottom, Y=left.
                match part.def.label {
                    "X" => (part.x - label_w / 2.0, part.y - r - 8.0),
                    "B" => (part.x - label_w / 2.0, part.y + r + 18.0),
                    "Y" => (part.x - r - label_w - 8.0, part.y + 4.0),
                    "A" => (part.x + r + 8.0, part.y + 4.0),
                    _ => (part.x - label_w / 2.0, part.y + r + 14.0),
                }
            } else {
                // Menu buttons: label below.
                (part.x - label_w / 2.0, part.y + r + 14.0)
            }
        }
        ButtonShape::DpadArm(DpadDir::Up) => (
            part.x + part.w / 2.0 - label_w / 2.0,
            part.y - 6.0,
        ),
        ButtonShape::DpadArm(DpadDir::Down) => (
            part.x + part.w / 2.0 - label_w / 2.0,
            part.y + part.h + 14.0,
        ),
        ButtonShape::DpadArm(DpadDir::Left) => (
            part.x - label_w - 6.0,
            part.y + part.h / 2.0 + 4.0,
        ),
        ButtonShape::DpadArm(DpadDir::Right) => (
            part.x + part.w + 6.0,
            part.y + part.h / 2.0 + 4.0,
        ),
        ButtonShape::Rect | ButtonShape::RectShoulder => (
            part.x + part.w / 2.0 - label_w / 2.0,
            part.y + part.h + 14.0,
        ),
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

// ── Public API ───────────────────────────────────────────────────────

pub struct GamepadOverlay {
    pub overlay: Overlay,
    drawing_area: DrawingArea,
}

fn compute_scale(widget_w: i32) -> f64 {
    let base_w = KB_W + 2.0 * PAD;
    let target = widget_w as f64 * 0.92;
    (target / base_w).max(0.5)
}

impl GamepadOverlay {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let drawing_area = DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .halign(gtk4::Align::Fill)
            .valign(gtk4::Align::Center)
            .visible(false)
            .build();
        drawing_area.add_css_class("gamepad-overlay-canvas");

        drawing_area.set_draw_func(move |area, cr, w, _h| {
            let scale = compute_scale(w);
            let base_w = KB_W + 2.0 * PAD;
            let base_h = KB_H + 2.0 * PAD;

            let scaled_w = base_w * scale;
            let x_offset = (w as f64 - scaled_w) / 2.0;
            cr.translate(x_offset, 0.0);
            cr.scale(scale, scale);

            area.set_content_height((base_h * scale) as i32);

            let layout = build_layout();
            draw_gamepad(cr, &layout);
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
