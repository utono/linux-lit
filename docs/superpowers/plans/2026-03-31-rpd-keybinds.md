# rpd-keybinds Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a standalone GTK4 Rust app that renders Cairo-drawn RPD keyboard layouts from TOML config files, with keybind cycling between drawings.

**Architecture:** RPD physical layout as Rust constants, per-app keybind definitions in TOML files under `configs/`. A generic Cairo renderer merges the two. GTK4 window with keyboard navigation to cycle drawings.

**Tech Stack:** Rust, GTK4 (0.9), Cairo (via gtk4), toml + serde for config parsing

---

### Task 1: Create repo, Cargo project, and GitHub remote

**Files:**
- Create: `~/utono/rpd-keybinds/Cargo.toml`
- Create: `~/utono/rpd-keybinds/src/main.rs`
- Create: `~/utono/rpd-keybinds/CLAUDE.md`
- Create: `~/utono/rpd-keybinds/.gitignore`

- [ ] **Step 1: Create the Cargo project**

```bash
cd ~/utono
cargo init rpd-keybinds
cd rpd-keybinds
```

- [ ] **Step 2: Set up Cargo.toml**

Replace `~/utono/rpd-keybinds/Cargo.toml` with:

```toml
[package]
name = "rpd-keybinds"
version = "0.1.0"
edition = "2021"

[dependencies]
gtk4 = { version = "0.9", features = ["v4_12"] }
glib = "0.20"
toml = "0.8"
serde = { version = "1", features = ["derive"] }
```

- [ ] **Step 3: Write minimal main.rs that opens a GTK4 window**

Write `src/main.rs`:

```rust
use gtk4::prelude::*;
use gtk4::Application;

fn main() {
    let app = Application::builder()
        .application_id("com.utono.rpd-keybinds")
        .build();

    app.connect_activate(|app| {
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("rpd-keybinds")
            .default_width(1100)
            .default_height(600)
            .build();
        window.present();
    });

    app.run();
}
```

- [ ] **Step 4: Build and verify the window opens**

```bash
cargo build
cargo run
```

Expected: An empty GTK4 window titled "rpd-keybinds" opens. Close it manually.

- [ ] **Step 5: Write CLAUDE.md**

Create `~/utono/rpd-keybinds/CLAUDE.md`:

```markdown
# rpd-keybinds

Standalone GTK4 Rust app that renders Cairo-drawn RPD keyboard layouts showing keybindings for multiple apps.

## Build & Run

```bash
cargo build
cargo run
```

## Testing

```bash
cargo test
cargo clippy
```

## Key Files

- `src/main.rs` — entry point, GTK4 app setup
- `src/config.rs` — TOML parsing, keybind loading
- `src/layout.rs` — RPD physical layout constants (rows, positions, sizes)
- `src/renderer.rs` — Cairo drawing, merges layout + keybinds
- `src/app.rs` — GTK4 window, DrawingArea, cycling logic
- `configs/` — per-app TOML keybind definitions

## Keyboard Layout

Real Programmers Dvorak. Physical key positions are in `src/layout.rs`.

## Configs

Each TOML file in `configs/` defines one keyboard drawing. See any file for the format.
```

- [ ] **Step 6: Create .gitignore**

Create `~/utono/rpd-keybinds/.gitignore`:

```
/target
```

- [ ] **Step 7: Create configs directory**

```bash
mkdir -p ~/utono/rpd-keybinds/configs
```

- [ ] **Step 8: Init git repo and create private GitHub remote**

```bash
cd ~/utono/rpd-keybinds
git init
git add .
git commit -m "Initial Cargo project with GTK4 window"
gh repo create utono/rpd-keybinds --private --source=. --push
```

---

### Task 2: TOML config parsing (config.rs)

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs` (add `mod config;`)

- [ ] **Step 1: Write test for config parsing**

Create `src/config.rs`:

```rust
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct DrawingMeta {
    pub name: String,
    pub app: String,
    #[serde(default = "default_order")]
    pub order: u32,
}

fn default_order() -> u32 {
    1
}

#[derive(Debug, Deserialize, Default)]
pub struct KeyBinding {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub shift: String,
    #[serde(default)]
    pub modifiers: Vec<[String; 2]>,
}

#[derive(Debug, Deserialize)]
pub struct DrawingToml {
    pub drawing: DrawingMeta,
    #[serde(default)]
    pub keys: HashMap<String, KeyBinding>,
}

#[derive(Debug)]
pub struct Drawing {
    pub name: String,
    pub app: String,
    pub order: u32,
    pub keys: HashMap<String, KeyBinding>,
}

impl From<DrawingToml> for Drawing {
    fn from(t: DrawingToml) -> Self {
        Drawing {
            name: t.drawing.name,
            app: t.drawing.app,
            order: t.drawing.order,
            keys: t.keys,
        }
    }
}

pub fn load_drawing(path: &Path) -> Result<Drawing, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let parsed: DrawingToml = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;
    Ok(parsed.into())
}

pub fn load_all_drawings(config_dir: &Path) -> Vec<Drawing> {
    let mut drawings = Vec::new();
    let entries = match std::fs::read_dir(config_dir) {
        Ok(e) => e,
        Err(_) => return drawings,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "toml").unwrap_or(false) {
            match load_drawing(&path) {
                Ok(d) => drawings.push(d),
                Err(e) => eprintln!("{}", e),
            }
        }
    }
    drawings.sort_by(|a, b| a.app.cmp(&b.app).then(a.order.cmp(&b.order)));
    drawings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_toml() {
        let toml_str = r#"
[drawing]
name = "test app normal"
app = "test-app"
order = 1

[keys]
a = { action = "do thing" }
o = { action = "seek", shift = "O: big seek" }
u = { action = "start", modifiers = [["C-u", "pg back"]] }
"#;
        let parsed: DrawingToml = toml::from_str(toml_str).unwrap();
        let drawing: Drawing = parsed.into();

        assert_eq!(drawing.name, "test app normal");
        assert_eq!(drawing.app, "test-app");
        assert_eq!(drawing.order, 1);
        assert_eq!(drawing.keys["a"].action, "do thing");
        assert_eq!(drawing.keys["o"].shift, "O: big seek");
        assert_eq!(drawing.keys["u"].modifiers[0][0], "C-u");
        assert_eq!(drawing.keys["u"].modifiers[0][1], "pg back");
    }

    #[test]
    fn test_missing_keys_section() {
        let toml_str = r#"
[drawing]
name = "empty"
app = "test"
"#;
        let parsed: DrawingToml = toml::from_str(toml_str).unwrap();
        let drawing: Drawing = parsed.into();
        assert!(drawing.keys.is_empty());
        assert_eq!(drawing.order, 1); // default
    }
}
```

- [ ] **Step 2: Add module to main.rs**

Add to the top of `src/main.rs`:

```rust
mod config;
```

- [ ] **Step 3: Run tests**

```bash
cargo test
```

Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "Add TOML config parsing with tests"
```

---

### Task 3: RPD physical layout constants (layout.rs)

**Files:**
- Create: `src/layout.rs`
- Modify: `src/main.rs` (add `mod layout;`)

- [ ] **Step 1: Write layout.rs with RPD physical key positions**

Create `src/layout.rs`. This is ported directly from linux-lit's `keybinds_overlay.rs` but uses owned Strings instead of `&'static str` so it can merge with TOML data:

```rust
/// Physical key on the RPD keyboard layout.
pub struct PhysicalKey {
    pub unshifted: &'static str,
    pub shifted: &'static str,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

// ── Layout constants ─────────────────────────────────────────────────

pub const KEY_W: f64 = 68.0;
pub const KEY_H: f64 = 66.0;
pub const GAP: f64 = 4.0;
pub const TAB_W: f64 = 102.0;
pub const ESC_W: f64 = 120.0;
pub const BKSP_W: f64 = 94.0;
pub const ARROW_W: f64 = 54.0;
pub const ARROW_H: f64 = 50.0;
pub const CORNER_R: f64 = 5.0;
pub const PAD: f64 = 20.0;

pub const KB_W: f64 = 13.0 * KEY_W + BKSP_W + 13.0 * GAP;
pub const KB_H: f64 = 4.0 * KEY_H + 3.0 * GAP  // main rows
    + GAP + KEY_H                                 // spacebar row
    + 12.0                                        // gap before seq row
    + KEY_H                                       // seq row
    + GAP + ARROW_H                               // arrow bottom row
    + 30.0;                                       // legend

// ── Row data ─────────────────────────────────────────────────────────

const NUMBER_ROW: &[(&str, &str)] = &[
    ("$", "~"), ("+", "1"), ("[", "2"), ("{", "3"),
    ("(", "4"), ("&", "5"), ("=", "6"), (")", "7"),
    ("}", "8"), ("]", "9"), ("*", "0"), ("!", "%"), ("|", "`"),
];

const UPPER_ROW: &[(&str, &str)] = &[
    (";", ":"), (",", "<"), (".", ">"), ("p", "P"),
    ("y", "Y"), ("f", "F"), ("g", "G"), ("c", "C"),
    ("r", "R"), ("l", "L"), ("/", "?"), ("@", "^"), ("\\", "#"),
];

const HOME_ROW: &[(&str, &str)] = &[
    ("a", "A"), ("o", "O"), ("e", "E"), ("u", "U"),
    ("i", "I"), ("d", "D"), ("h", "H"), ("t", "T"),
    ("n", "N"), ("s", "S"), ("-", "_"),
];

const BOTTOM_ROW: &[(&str, &str)] = &[
    ("'", "\""), ("q", "Q"), ("j", "J"), ("k", "K"),
    ("x", "X"), ("b", "B"), ("m", "M"), ("w", "W"),
    ("v", "V"), ("z", "Z"),
];

/// Build all physical key positions. Returns a Vec of PhysicalKey with
/// coordinates relative to the top-left of the keyboard area (inside padding).
pub fn build_layout() -> Vec<PhysicalKey> {
    let mut keys = Vec::new();

    // Row 0: Number row
    let y0 = 0.0;
    let mut x = 0.0;
    for &(u, s) in NUMBER_ROW {
        keys.push(PhysicalKey { unshifted: u, shifted: s, x, y: y0, w: KEY_W, h: KEY_H });
        x += KEY_W + GAP;
    }
    // Backspace
    keys.push(PhysicalKey { unshifted: "\u{232b}", shifted: "", x, y: y0, w: BKSP_W, h: KEY_H });

    // Row 1: Upper row (Tab + 13 keys)
    let y1 = y0 + KEY_H + GAP;
    x = 0.0;
    keys.push(PhysicalKey { unshifted: "Tab", shifted: "", x, y: y1, w: TAB_W, h: KEY_H });
    x += TAB_W + GAP;
    for &(u, s) in UPPER_ROW {
        keys.push(PhysicalKey { unshifted: u, shifted: s, x, y: y1, w: KEY_W, h: KEY_H });
        x += KEY_W + GAP;
    }

    // Row 2: Home row (Esc + 11 keys)
    let y2 = y1 + KEY_H + GAP;
    x = 0.0;
    keys.push(PhysicalKey { unshifted: "Esc", shifted: "", x, y: y2, w: ESC_W, h: KEY_H });
    x += ESC_W + GAP;
    for &(u, s) in HOME_ROW {
        keys.push(PhysicalKey { unshifted: u, shifted: s, x, y: y2, w: KEY_W, h: KEY_H });
        x += KEY_W + GAP;
    }

    // Row 3: Bottom row (Shift + 10 keys)
    let y3 = y2 + KEY_H + GAP;
    let bottom_alpha_offset = ESC_W + GAP + KEY_W * 0.45;
    let shift_w = bottom_alpha_offset - GAP;
    keys.push(PhysicalKey { unshifted: "Shift", shifted: "", x: 0.0, y: y3, w: shift_w, h: KEY_H });
    x = bottom_alpha_offset;
    for &(u, s) in BOTTOM_ROW {
        keys.push(PhysicalKey { unshifted: u, shifted: s, x, y: y3, w: KEY_W, h: KEY_H });
        x += KEY_W + GAP;
    }

    // Row 4: Spacebar row
    let y4 = y3 + KEY_H + GAP;
    let half_shift = (shift_w - GAP) / 2.0;
    keys.push(PhysicalKey { unshifted: "Ctrl", shifted: "", x: 0.0, y: y4, w: half_shift, h: KEY_H });
    keys.push(PhysicalKey { unshifted: "Fn", shifted: "", x: half_shift + GAP, y: y4, w: half_shift, h: KEY_H });

    let win_x = bottom_alpha_offset;
    keys.push(PhysicalKey { unshifted: "Win", shifted: "", x: win_x, y: y4, w: KEY_W, h: KEY_H });
    keys.push(PhysicalKey { unshifted: "Alt", shifted: "", x: win_x + KEY_W + GAP, y: y4, w: KEY_W, h: KEY_H });

    let j_x = bottom_alpha_offset + 2.0 * (KEY_W + GAP);
    let m_right = bottom_alpha_offset + 6.0 * (KEY_W + GAP) + KEY_W;
    keys.push(PhysicalKey { unshifted: "Space", shifted: "", x: j_x, y: y4, w: m_right - j_x, h: KEY_H });

    keys.push(PhysicalKey { unshifted: "Alt", shifted: "", x: m_right + GAP, y: y4, w: KEY_W, h: KEY_H });
    keys.push(PhysicalKey { unshifted: "Ctrl", shifted: "", x: m_right + GAP + KEY_W + GAP, y: y4, w: KEY_W, h: KEY_H });

    // Row 5: Sequences (gg, G)
    let y5 = y4 + KEY_H + 12.0;
    keys.push(PhysicalKey { unshifted: "gg", shifted: "", x: 0.0, y: y5, w: KEY_W * 1.4, h: KEY_H });
    keys.push(PhysicalKey { unshifted: "G", shifted: "", x: KEY_W * 1.4 + GAP, y: y5, w: KEY_W, h: KEY_H });

    // Arrow keys — inverted T on far right
    let arrow_y_bottom = y5 + KEY_H + GAP;
    let arrow_left_x = KB_W - 3.0 * ARROW_W - 2.0 * GAP;
    keys.push(PhysicalKey { unshifted: "\u{2190}", shifted: "", x: arrow_left_x, y: arrow_y_bottom, w: ARROW_W, h: ARROW_H });
    keys.push(PhysicalKey { unshifted: "\u{2193}", shifted: "", x: arrow_left_x + ARROW_W + GAP, y: arrow_y_bottom, w: ARROW_W, h: ARROW_H });
    keys.push(PhysicalKey { unshifted: "\u{2192}", shifted: "", x: arrow_left_x + 2.0 * (ARROW_W + GAP), y: arrow_y_bottom, w: ARROW_W, h: ARROW_H });
    // Up arrow above down arrow
    let up_x = arrow_left_x + ARROW_W + GAP;
    keys.push(PhysicalKey { unshifted: "\u{2191}", shifted: "", x: up_x, y: y5, w: ARROW_W, h: ARROW_H });

    keys
}

/// Map from TOML key names to the unshifted label used in the layout.
/// Most keys map directly (e.g., "a" -> "a"), but some need translation.
pub fn toml_key_to_layout(key: &str) -> &str {
    match key {
        "Tab" => "Tab",
        "Esc" | "Escape" => "Esc",
        "Space" => "Space",
        "BackSpace" => "\u{232b}",
        "Up" => "\u{2191}",
        "Down" => "\u{2193}",
        "Left" => "\u{2190}",
        "Right" => "\u{2192}",
        "slash" => "/",
        "backslash" => "\\",
        "comma" => ",",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_has_expected_key_count() {
        let keys = build_layout();
        // 13 number + bksp + tab + 13 upper + esc + 11 home + shift + 10 bottom
        // + 7 spacebar row + 2 seq + 4 arrows = 62
        assert_eq!(keys.len(), 62);
    }

    #[test]
    fn test_toml_key_mapping() {
        assert_eq!(toml_key_to_layout("a"), "a");
        assert_eq!(toml_key_to_layout("Tab"), "Tab");
        assert_eq!(toml_key_to_layout("Up"), "\u{2191}");
        assert_eq!(toml_key_to_layout("slash"), "/");
        assert_eq!(toml_key_to_layout("gg"), "gg");
    }
}
```

- [ ] **Step 2: Add module to main.rs**

Add to `src/main.rs`:

```rust
mod layout;
```

- [ ] **Step 3: Run tests**

```bash
cargo test
```

Expected: 4 tests pass (2 config + 2 layout).

- [ ] **Step 4: Commit**

```bash
git add src/layout.rs src/main.rs
git commit -m "Add RPD physical layout constants with tests"
```

---

### Task 4: Cairo renderer (renderer.rs)

**Files:**
- Create: `src/renderer.rs`
- Modify: `src/main.rs` (add `mod renderer;`)

- [ ] **Step 1: Write renderer.rs**

Create `src/renderer.rs`:

```rust
use crate::config::Drawing;
use crate::layout::{self, PhysicalKey, KB_W, KB_H, PAD, CORNER_R};

// ── Rose Pine Dawn palette ──────────────────────────────────────────

const BG: (f64, f64, f64) = (0.341, 0.322, 0.475);           // #575279
const BOUND_BG: (f64, f64, f64) = (0.949, 0.914, 0.882);     // #f2e9e1
const BOUND_BORDER: (f64, f64, f64) = (0.875, 0.855, 0.851); // #dfdad9
const UNBOUND_BG: (f64, f64, f64) = (1.0, 0.98, 0.953);      // #fffaf3
const UNBOUND_BORDER: (f64, f64, f64) = (0.949, 0.914, 0.882); // #f2e9e1
const TEXT_BOUND: (f64, f64, f64) = (0.341, 0.322, 0.475);    // #575279
const TEXT_UNBOUND: (f64, f64, f64) = (0.596, 0.576, 0.647);  // #9893a5
const PINE: (f64, f64, f64) = (0.157, 0.412, 0.514);          // #286983
const IRIS: (f64, f64, f64) = (0.565, 0.478, 0.663);          // #907aa9
const LOVE: (f64, f64, f64) = (0.706, 0.388, 0.478);          // #b4637a
const ROSE: (f64, f64, f64) = (0.843, 0.51, 0.494);           // #d7827e
const MUTED: (f64, f64, f64) = (0.596, 0.576, 0.647);        // #9893a5
const SUBTLE: (f64, f64, f64) = (0.475, 0.459, 0.576);       // #797593

fn is_bound(key: &PhysicalKey, drawing: &Drawing) -> bool {
    let lookup = layout::toml_key_to_layout(key.unshifted);
    if let Some(kb) = drawing.keys.get(lookup) {
        !kb.action.is_empty() || !kb.shift.is_empty() || !kb.modifiers.is_empty()
    } else {
        // Check if the unshifted label itself matches
        if let Some(kb) = drawing.keys.get(key.unshifted) {
            !kb.action.is_empty() || !kb.shift.is_empty() || !kb.modifiers.is_empty()
        } else {
            false
        }
    }
}

fn lookup_binding<'a>(key: &PhysicalKey, drawing: &'a Drawing) -> Option<&'a crate::config::KeyBinding> {
    let lookup = layout::toml_key_to_layout(key.unshifted);
    drawing.keys.get(lookup).or_else(|| drawing.keys.get(key.unshifted))
}

pub fn draw_keyboard(cr: &gtk4::cairo::Context, keys: &[PhysicalKey], drawing: &Drawing, hover_idx: Option<usize>, drawing_label: &str) {
    let total_w = KB_W + 2.0 * PAD;
    let total_h = KB_H + 2.0 * PAD;

    // Background
    cr.set_source_rgb(BG.0, BG.1, BG.2);
    rounded_rect(cr, 0.0, 0.0, total_w, total_h, 10.0);
    let _ = cr.fill();

    cr.translate(PAD, PAD);

    for (i, key) in keys.iter().enumerate() {
        let bound = is_bound(key, drawing);
        let binding = lookup_binding(key, drawing);

        let (bg, border) = if bound {
            (BOUND_BG, BOUND_BORDER)
        } else {
            (UNBOUND_BG, UNBOUND_BORDER)
        };

        let char_col = if bound { TEXT_BOUND } else { TEXT_UNBOUND };

        // Key background
        cr.set_source_rgb(bg.0, bg.1, bg.2);
        rounded_rect(cr, key.x, key.y, key.w, key.h, CORNER_R);
        let _ = cr.fill();

        // Key border (highlight on hover if has modifiers)
        let is_hovered = hover_idx == Some(i);
        let has_modifiers = binding.map(|b| !b.modifiers.is_empty()).unwrap_or(false);
        if is_hovered && has_modifiers {
            cr.set_source_rgb(ROSE.0, ROSE.1, ROSE.2);
            cr.set_line_width(2.0);
        } else {
            cr.set_source_rgb(border.0, border.1, border.2);
            cr.set_line_width(1.0);
        }
        rounded_rect(cr, key.x + 0.5, key.y + 0.5, key.w - 1.0, key.h - 1.0, CORNER_R);
        let _ = cr.stroke();

        // Unshifted character (bold, top-left)
        cr.set_source_rgb(char_col.0, char_col.1, char_col.2);
        cr.set_font_size(22.0);
        cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
        let _ = cr.move_to(key.x + 7.0, key.y + 24.0);
        let _ = cr.show_text(key.unshifted);

        // Shifted character (small, top-right)
        if !key.shifted.is_empty() {
            let has_shift_action = binding.map(|b| !b.shift.is_empty()).unwrap_or(false);
            let shifted_col = if has_shift_action { IRIS } else { TEXT_UNBOUND };
            cr.set_source_rgb(shifted_col.0, shifted_col.1, shifted_col.2);
            cr.set_font_size(14.0);
            cr.select_font_face("monospace", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
            let extents = cr.text_extents(key.shifted).unwrap();
            let _ = cr.move_to(key.x + key.w - extents.width() - 7.0, key.y + 17.0);
            let _ = cr.show_text(key.shifted);
        }

        if let Some(kb) = binding {
            // Bare action label (pine, bottom-left)
            if !kb.action.is_empty() {
                cr.set_source_rgb(PINE.0, PINE.1, PINE.2);
                cr.set_font_size(12.0);
                cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
                let _ = cr.move_to(key.x + 7.0, key.y + key.h - 8.0);
                let _ = cr.show_text(&kb.action);
            }

            // Shift action label (iris)
            if !kb.shift.is_empty() {
                cr.set_source_rgb(IRIS.0, IRIS.1, IRIS.2);
                cr.set_font_size(11.0);
                cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
                let y_pos = if !kb.action.is_empty() {
                    key.y + key.h - 22.0
                } else {
                    key.y + key.h - 8.0
                };
                let _ = cr.move_to(key.x + 7.0, y_pos);
                let _ = cr.show_text(&kb.shift);
            }
        }
    }

    // Draw tooltip if hovering a key with modifiers
    if let Some(idx) = hover_idx {
        if idx < keys.len() {
            if let Some(kb) = lookup_binding(&keys[idx], drawing) {
                if !kb.modifiers.is_empty() {
                    draw_tooltip(cr, &keys[idx], kb);
                }
            }
        }
    }

    // Legend
    let legend_y = KB_H - 20.0;

    // Bound swatch + "bare key"
    let mut lx = 0.0;
    cr.set_source_rgb(BOUND_BG.0, BOUND_BG.1, BOUND_BG.2);
    rounded_rect(cr, lx, legend_y, 16.0, 16.0, 3.0);
    let _ = cr.fill();
    cr.set_source_rgb(PINE.0, PINE.1, PINE.2);
    cr.set_font_size(13.0);
    cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    let _ = cr.move_to(lx + 20.0, legend_y + 13.0);
    let _ = cr.show_text("bare key");
    let ext = cr.text_extents("bare key").unwrap();
    lx += 20.0 + ext.width() + 24.0;

    // Bound swatch + "shift only"
    cr.set_source_rgb(BOUND_BG.0, BOUND_BG.1, BOUND_BG.2);
    rounded_rect(cr, lx, legend_y, 16.0, 16.0, 3.0);
    let _ = cr.fill();
    cr.set_source_rgb(IRIS.0, IRIS.1, IRIS.2);
    let _ = cr.move_to(lx + 20.0, legend_y + 13.0);
    let _ = cr.show_text("shift only");
    let ext = cr.text_extents("shift only").unwrap();
    lx += 20.0 + ext.width() + 24.0;

    // Unbound swatch + "unbound"
    cr.set_source_rgb(UNBOUND_BG.0, UNBOUND_BG.1, UNBOUND_BG.2);
    rounded_rect(cr, lx, legend_y, 16.0, 16.0, 3.0);
    let _ = cr.fill();
    cr.set_source_rgb(MUTED.0, MUTED.1, MUTED.2);
    let _ = cr.move_to(lx + 20.0, legend_y + 13.0);
    let _ = cr.show_text("unbound");
    let ext = cr.text_extents("unbound").unwrap();
    lx += 20.0 + ext.width() + 24.0;

    // Modifier bullet
    cr.set_source_rgb(LOVE.0, LOVE.1, LOVE.2);
    let _ = cr.move_to(lx, legend_y + 13.0);
    let _ = cr.show_text("\u{2022} Ctrl/Alt");

    // Drawing label + hint (right side)
    cr.set_source_rgb(SUBTLE.0, SUBTLE.1, SUBTLE.2);
    let hint = &format!("{} \u{00b7} j/k cycle \u{00b7} q quit", drawing_label);
    let ext = cr.text_extents(hint).unwrap();
    let _ = cr.move_to(KB_W - ext.width(), legend_y + 13.0);
    let _ = cr.show_text(hint);
}

fn draw_tooltip(cr: &gtk4::cairo::Context, key: &PhysicalKey, kb: &crate::config::KeyBinding) {
    let lines: Vec<String> = kb.modifiers.iter().map(|m| {
        format!("{} \u{2192} {}", m[0], m[1])
    }).collect();

    cr.set_font_size(13.0);
    cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);

    let mut max_w: f64 = 0.0;
    for line in &lines {
        let ext = cr.text_extents(line).unwrap();
        if ext.width() > max_w { max_w = ext.width(); }
    }
    let tt_w = max_w + 20.0;
    let tt_h = lines.len() as f64 * 18.0 + 12.0;
    let tt_x = key.x + key.w / 2.0 - tt_w / 2.0;
    let tt_y = key.y - tt_h - 6.0;

    // Background
    cr.set_source_rgba(BG.0, BG.1, BG.2, 0.95);
    rounded_rect(cr, tt_x, tt_y, tt_w, tt_h, 4.0);
    let _ = cr.fill();

    // Border
    cr.set_source_rgb(SUBTLE.0, SUBTLE.1, SUBTLE.2);
    rounded_rect(cr, tt_x + 0.5, tt_y + 0.5, tt_w - 1.0, tt_h - 1.0, 4.0);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    // Text
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("M-") {
            cr.set_source_rgb(IRIS.0, IRIS.1, IRIS.2);
        } else {
            cr.set_source_rgb(LOVE.0, LOVE.1, LOVE.2);
        }
        let _ = cr.move_to(tt_x + 10.0, tt_y + 18.0 + i as f64 * 18.0);
        let _ = cr.show_text(line);
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

/// Hit-test: find which key index the mouse is over (for tooltip).
pub fn hit_test(keys: &[PhysicalKey], mx: f64, my: f64) -> Option<usize> {
    let x = mx - PAD;
    let y = my - PAD;
    for (i, key) in keys.iter().enumerate() {
        if x >= key.x && x <= key.x + key.w && y >= key.y && y <= key.y + key.h {
            return Some(i);
        }
    }
    None
}
```

- [ ] **Step 2: Add module to main.rs**

Add to `src/main.rs`:

```rust
mod renderer;
```

- [ ] **Step 3: Build to verify compilation**

```bash
cargo build
```

Expected: Compiles with no errors (warnings about unused are fine).

- [ ] **Step 4: Commit**

```bash
git add src/renderer.rs src/main.rs
git commit -m "Add Cairo renderer with Rose Pine Dawn theme"
```

---

### Task 5: GTK4 app window with cycling (app.rs)

**Files:**
- Create: `src/app.rs`
- Modify: `src/main.rs` (replace activate handler, add `mod app;`)

- [ ] **Step 1: Write app.rs**

Create `src/app.rs`:

```rust
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, DrawingArea};
use std::cell::RefCell;
use std::rc::Rc;

use crate::config::Drawing;
use crate::layout::{self, PhysicalKey, KB_W, KB_H, PAD};
use crate::renderer;

struct AppState {
    drawings: Vec<Drawing>,
    current: usize,
    keys: Vec<PhysicalKey>,
    hover_idx: Option<usize>,
}

pub fn build_ui(app: &gtk4::Application, drawings: Vec<Drawing>) {
    if drawings.is_empty() {
        eprintln!("No drawings found in configs/");
        std::process::exit(1);
    }

    let keys = layout::build_layout();
    let first_name = drawings[0].name.clone();

    let state = Rc::new(RefCell::new(AppState {
        drawings,
        current: 0,
        keys,
        hover_idx: None,
    }));

    let window = ApplicationWindow::builder()
        .application(app)
        .title(&first_name)
        .default_width(1100)
        .default_height(600)
        .build();

    let drawing_area = DrawingArea::builder()
        .hexpand(true)
        .vexpand(true)
        .build();

    // Draw function
    let state_draw = Rc::clone(&state);
    drawing_area.set_draw_func(move |area, cr, w, _h| {
        let s = state_draw.borrow();
        let base_w = KB_W + 2.0 * PAD;
        let base_h = KB_H + 2.0 * PAD;
        let scale = (w as f64 * 0.92) / base_w;
        let scaled_w = base_w * scale;
        let x_offset = (w as f64 - scaled_w) / 2.0;

        cr.translate(x_offset, 0.0);
        cr.scale(scale, scale);
        area.set_content_height((base_h * scale) as i32);

        let drawing = &s.drawings[s.current];
        let label = &drawing.name;
        renderer::draw_keyboard(cr, &s.keys, drawing, s.hover_idx, label);
    });

    // Mouse motion for tooltips
    let motion = gtk4::EventControllerMotion::new();
    let state_motion = Rc::clone(&state);
    let da_motion = drawing_area.clone();
    motion.connect_motion(move |_ctrl, x, y| {
        let w = da_motion.width();
        let base_w = KB_W + 2.0 * PAD;
        let scale = (w as f64 * 0.92) / base_w;
        let scaled_w = base_w * scale;
        let x_offset = (w as f64 - scaled_w) / 2.0;

        let lx = (x - x_offset) / scale;
        let ly = y / scale;

        let new_idx = renderer::hit_test(&state_motion.borrow().keys, lx, ly);
        let old_idx = state_motion.borrow().hover_idx;
        if new_idx != old_idx {
            state_motion.borrow_mut().hover_idx = new_idx;
            da_motion.queue_draw();
        }
    });

    let state_leave = Rc::clone(&state);
    let da_leave = drawing_area.clone();
    motion.connect_leave(move |_ctrl| {
        state_leave.borrow_mut().hover_idx = None;
        da_leave.queue_draw();
    });
    drawing_area.add_controller(motion);

    // Key handler for cycling and quit
    let key_ctrl = gtk4::EventControllerKey::new();
    let state_key = Rc::clone(&state);
    let da_key = drawing_area.clone();
    let win_key = window.clone();
    key_ctrl.connect_key_pressed(move |_ctrl, keyval, _keycode, _mods| {
        let key_name = keyval.name().unwrap_or_default();
        match key_name.as_str() {
            "j" | "n" => {
                let mut s = state_key.borrow_mut();
                s.current = (s.current + 1) % s.drawings.len();
                s.hover_idx = None;
                let title = s.drawings[s.current].name.clone();
                drop(s);
                win_key.set_title(Some(&title));
                da_key.queue_draw();
                glib::Propagation::Stop
            }
            "k" | "p" => {
                let mut s = state_key.borrow_mut();
                let len = s.drawings.len();
                s.current = (s.current + len - 1) % len;
                s.hover_idx = None;
                let title = s.drawings[s.current].name.clone();
                drop(s);
                win_key.set_title(Some(&title));
                da_key.queue_draw();
                glib::Propagation::Stop
            }
            "q" | "Escape" => {
                win_key.close();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(key_ctrl);

    window.set_child(Some(&drawing_area));
    window.present();
}
```

- [ ] **Step 2: Replace main.rs**

Replace `src/main.rs` with:

```rust
mod app;
mod config;
mod layout;
mod renderer;

use gtk4::prelude::*;
use std::path::PathBuf;

fn main() {
    let gtk_app = gtk4::Application::builder()
        .application_id("com.utono.rpd-keybinds")
        .build();

    gtk_app.connect_activate(|app| {
        // Find configs dir relative to the executable or CWD
        let config_dir = find_configs_dir();
        let drawings = config::load_all_drawings(&config_dir);
        app::build_ui(app, drawings);
    });

    gtk_app.run();
}

fn find_configs_dir() -> PathBuf {
    // Try CWD/configs first, then exe dir/configs
    let cwd = std::env::current_dir().unwrap_or_default();
    let cwd_configs = cwd.join("configs");
    if cwd_configs.is_dir() {
        return cwd_configs;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let exe_configs = dir.join("configs");
            if exe_configs.is_dir() {
                return exe_configs;
            }
        }
    }
    cwd_configs // fallback, will just find no files
}
```

- [ ] **Step 3: Build**

```bash
cargo build
```

Expected: Compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "Add GTK4 window with drawing cycling and keyboard navigation"
```

---

### Task 6: Create initial TOML config for linux-lit normal mode

**Files:**
- Create: `configs/linux-lit-normal.toml`

- [ ] **Step 1: Write the config file**

Create `configs/linux-lit-normal.toml`:

```toml
[drawing]
name = "linux-lit normal"
app = "linux-lit"
order = 1

[keys]
"$" = {}
"+" = { action = "toggle speed" }
"[" = { action = "prev ch" }
"{" = { action = "next ch" }
"*" = { shift = "reset font" }
"!" = { action = "font \u2212" }
"|" = { action = "font +" }
BackSpace = { action = "delete ts" }

Tab = { action = "play/pause" }
"," = { action = "prev dlg", modifiers = [["C-,", "settings"]] }
"." = { action = "set chapter" }
p = { action = "nudge \u22120.2", shift = "P: +0.2", modifiers = [["C-p", "picker"]] }
y = { action = "prev chunk" }
f = { action = "font \u2192", shift = "F: \u2190", modifiers = [["C-f", "pg fwd"], ["M-f", "font info"]] }
l = { action = "toggle signs", modifiers = [["C-M-l", "save+quit"]] }
slash = { action = "search", modifiers = [["C-/", "keybinds"]] }

Esc = { action = "clear AB" }
a = { action = "play from ts" }
o = { action = "seek \u22123.5", shift = "O: \u221260" }
e = { action = "seek +3.5", shift = "E: +60" }
u = { action = "start time", modifiers = [["C-u", "pg back"]] }
i = { action = "set end time", modifiers = [["M-i", "translations"]] }
d = { modifiers = [["C-d", "pg fwd"]] }
n = { action = "next match", shift = "N: prev match" }

"'" = {}
q = { action = "next dlg" }
j = { action = "cursor \u2193" }
k = { action = "cursor \u2191" }
x = { action = "next chunk" }
b = { modifiers = [["C-b", "pg back"]] }
m = { action = "media picker", modifiers = [["p", "set default"]] }
v = { shift = "V: visual mode" }

Space = { action = "vocab popup" }

gg = { action = "go to start" }
G = { shift = "go to end" }

Up = { modifiers = [["C-Up", "vol +"]] }
Down = { modifiers = [["C-Down", "vol \u2212"]] }
Left = { action = "delete ts" }
Right = { action = "start time" }
```

- [ ] **Step 2: Run the app to verify rendering**

```bash
cargo run
```

Expected: A window opens showing the linux-lit normal keyboard overlay rendered with Rose Pine Dawn colors. Press `q` to close.

- [ ] **Step 3: Commit**

```bash
git add configs/linux-lit-normal.toml
git commit -m "Add linux-lit normal mode keybind config"
```

---

### Task 7: Push to GitHub

**Files:** None (git operations only)

- [ ] **Step 1: Run tests and clippy**

```bash
cargo test && cargo clippy
```

Expected: All tests pass, no clippy errors.

- [ ] **Step 2: Push**

```bash
git push
```

---

### Task 8: Update linux-lit overlay to Rose Pine Dawn colors

**Files:**
- Modify: `~/utono/linux-lit/src/ui/keybinds_overlay.rs`

- [ ] **Step 1: Replace the color constants and `key_colors` function**

In `~/utono/linux-lit/src/ui/keybinds_overlay.rs`, replace the `key_colors` function and the color usage in `draw_keyboard`.

Replace the `key_colors` function (around line 221):

```rust
fn key_colors(def: &KeyDef) -> ((f64, f64, f64), (f64, f64, f64), (f64, f64, f64)) {
    let has_bare = !def.action.is_empty();
    let has_shift = !def.shift_action.is_empty();
    let has_mod = !def.modifiers.is_empty();

    let bound = has_bare || has_shift || has_mod;
    if bound {
        // Rose Pine Dawn: overlay bg, dfdad9 border, text color
        ((0.949, 0.914, 0.882), (0.875, 0.855, 0.851), (0.341, 0.322, 0.475))
    } else {
        // Unbound: surface bg, overlay border, muted text
        ((1.0, 0.98, 0.953), (0.949, 0.914, 0.882), (0.596, 0.576, 0.647))
    }
}
```

- [ ] **Step 2: Update draw_keyboard background and label colors**

In `draw_keyboard`, replace the overall background color:

```rust
    cr.set_source_rgba(0.341, 0.322, 0.475, 0.95);  // #575279
```

Replace action label color (bare key):
```rust
    // Action label (bare key) — pine
    cr.set_source_rgb(0.157, 0.412, 0.514);
```

Replace shift action label color:
```rust
    // Shift action label — iris
    cr.set_source_rgb(0.565, 0.478, 0.663);
```

Replace shifted character active color:
```rust
    let shifted_col = if !def.shift_action.is_empty() || (!def.action.is_empty() && !def.modifiers.is_empty()) {
        (0.565, 0.478, 0.663) // iris
    } else {
        (0.596, 0.576, 0.647) // muted
    };
```

Replace legend colors — update the `legend_items` array backgrounds to use bound/unbound colors, and the `legend_colors` array to use pine/iris/muted/muted:

```rust
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
```

Replace Alt+ indicator color:
```rust
    cr.set_source_rgb(0.565, 0.478, 0.663);  // iris for Alt+
```

Replace close hint color:
```rust
    cr.set_source_rgb(0.475, 0.459, 0.576);  // subtle
```

- [ ] **Step 3: Update tooltip colors**

In `draw_tooltip`, replace background:
```rust
    cr.set_source_rgba(0.341, 0.322, 0.475, 0.95);  // #575279
```

Replace border:
```rust
    cr.set_source_rgb(0.475, 0.459, 0.576);  // subtle
```

Replace modifier text colors:
```rust
        if line.starts_with("M-") {
            cr.set_source_rgb(0.565, 0.478, 0.663); // iris for Alt
        } else {
            cr.set_source_rgb(0.706, 0.388, 0.478); // love for Ctrl
        }
```

- [ ] **Step 4: Build linux-lit**

```bash
cd ~/utono/linux-lit && cargo build
```

Expected: Compiles successfully.

- [ ] **Step 5: Commit in linux-lit**

```bash
cd ~/utono/linux-lit
git add src/ui/keybinds_overlay.rs
git commit -m "Update keybinds overlay to Rose Pine Dawn color scheme"
```
