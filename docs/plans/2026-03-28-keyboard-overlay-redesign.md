# Keyboard Overlay Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the keybinds popup with a visual RPD keyboard layout showing all bindings with inline actions and hover tooltips for modifier combos.

**Architecture:** Complete rewrite of `src/ui/keybinds_overlay.rs` with new `KeyDef` data model, physical keyboard widget layout using GtkBox nesting, and CSS color-coded key states. CSS updates in `src/theme.rs`. GTK4 native tooltips (`set_tooltip_markup`) for modifier combos.

**Tech Stack:** GTK4, Rust, CSS

---

### File Map

- **Rewrite:** `src/ui/keybinds_overlay.rs` — new KeyDef struct, const row arrays, keyboard widget builder, arrow cluster, legend
- **Modify:** `src/theme.rs` — replace old `.keybind-*` CSS classes with new `.kb-*` classes for keyboard layout

---

### Task 1: Replace CSS classes in theme.rs

**Files:**
- Modify: `src/theme.rs:368-381`

- [ ] **Step 1: Replace keybind CSS block**

In `src/theme.rs`, find the block from `.keybinds-overlay` through `.keybind-legend` (lines ~368-381) and replace with:

```rust
         .keybinds-overlay {{ background-color: rgba(26, 26, 26, 0.95); color: white; \
           padding: 20px; border-radius: 10px; }} \
         .kb-row {{ }} \
         .kb-key {{ background-color: #2a2a2a; border: 1px solid #444444; \
           border-radius: 5px; padding: 3px 5px; min-width: 52px; min-height: 50px; }} \
         .kb-key-bound {{ background-color: #1a3a2a; border-color: #3a6a4a; }} \
         .kb-key-bound-shift {{ background-color: #1a2a3a; border-color: #3a4a6a; }} \
         .kb-key-bound-both {{ background-color: #1a3a2a; border-color: #3a6a4a; }} \
         .kb-key-unbound {{ opacity: 0.5; }} \
         .kb-char {{ font-size: 40px; font-weight: bold; color: #888888; }} \
         .kb-char-bound {{ color: #88ff88; }} \
         .kb-char-shift {{ color: #88aaff; }} \
         .kb-char-both {{ color: #88ff88; }} \
         .kb-shifted {{ font-size: 28px; color: #666666; }} \
         .kb-shifted-active {{ color: #6688cc; }} \
         .kb-action {{ font-size: 24px; color: #66cc66; }} \
         .kb-shift-action {{ font-size: 22px; color: #6688cc; }} \
         .kb-arrow {{ background-color: #2a2a2a; border: 1px solid #444444; \
           border-radius: 4px; padding: 2px 4px; min-width: 38px; min-height: 36px; }} \
         .kb-arrow-bound {{ background-color: #1a3a2a; border-color: #3a6a4a; }} \
         .kb-arrow-bound-both {{ background-color: #1a3a2a; border-color: #3a6a4a; }} \
         .kb-arrow-char {{ font-size: 34px; color: #88ff88; }} \
         .kb-arrow-action {{ font-size: 20px; color: #66cc66; }} \
         .kb-legend {{ border-top: 1px solid rgba(255, 255, 255, 0.1); \
           margin-top: 12px; padding-top: 8px; }} \
         .kb-legend-swatch {{ min-width: 14px; min-height: 14px; \
           border-radius: 3px; border: 1px solid #555555; }} \
         .kb-legend-bound {{ background-color: #1a3a2a; border-color: #3a6a4a; }} \
         .kb-legend-shift {{ background-color: #1a2a3a; border-color: #3a4a6a; }} \
         .kb-legend-both {{ background-color: #1a3a2a; border-color: #3a6a4a; }} \
         .kb-legend-unbound {{ background-color: #2a2a2a; border-color: #444444; }} \
```

- [ ] **Step 2: Build and verify no regressions**

Run: `cargo build`
Expected: compiles (old CSS class names still referenced in keybinds_overlay.rs — that's fine, they'll be replaced in Task 2)

- [ ] **Step 3: Commit**

```bash
git add src/theme.rs
git commit -m "feat: add keyboard layout CSS classes for keybinds overlay redesign"
```

---

### Task 2: Rewrite keybinds_overlay.rs — data model and key row definitions

**Files:**
- Rewrite: `src/ui/keybinds_overlay.rs` (top half — structs and const arrays)

- [ ] **Step 1: Replace the entire top section** (structs, consts, helper fns) of `keybinds_overlay.rs` with:

```rust
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, CssProvider, Label, Orientation, Overlay};

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

/// Shorthand: unbound key
const fn ub(unshifted: &'static str, shifted: &'static str) -> KeyDef {
    key(unshifted, shifted, "", "", &[])
}

/// Shorthand: bare-key only
const fn bare(unshifted: &'static str, shifted: &'static str, action: &'static str) -> KeyDef {
    key(unshifted, shifted, action, "", &[])
}

// Number row: $ + [ { ( & = ) } ] * ! | Bksp
const NUMBER_ROW: &[KeyDef] = &[
    ub("$", "~"),
    bare("+", "1", "toggle speed"),
    bare("[", "2", "prev ch"),
    bare("{", "3", "next ch"),
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

// Upper row: ; , . p y f g c r l / @ '\'
const UPPER_ROW: &[KeyDef] = &[
    ub(";", ":"),
    key(",", "<", "prev dlg", "", &[("C-,", "settings")]),
    bare(".", ">", "set chapter"),
    key("p", "P", "nudge \u{2212}0.2", "P: +0.2", &[("C-p", "picker")]),
    bare("y", "Y", "prev chunk"),
    key("f", "F", "font \u{2192}", "F: \u{2190}", &[("C-f", "pg fwd"), ("M-f", "font info")]),
    ub("g", "G"),
    ub("c", "C"),
    ub("r", "R"),
    key("l", "L", "toggle signs", "", &[("C-M-l", "save+quit")]),
    key("/", "?", "search", "", &[("C-/", "keybinds")]),
    ub("@", "^"),
    ub("\\", "#"),
];

const TAB_KEY: KeyDef = bare("Tab", "", "play/pause");

// Home row: a o e u i d h t n s -
const HOME_ROW: &[KeyDef] = &[
    bare("a", "A", "play from ts"),
    key("o", "O", "seek \u{2212}3.5", "O: \u{2212}60", &[]),
    key("e", "E", "seek +3.5", "E: +60", &[]),
    key("u", "U", "start time", "", &[("C-u", "pg back")]),
    key("i", "I", "set end time", "", &[("M-i", "translations")]),
    key("d", "D", "", "", &[("C-d", "pg fwd")]),
    ub("h", "H"),
    ub("t", "T"),
    key("n", "N", "next match", "N: prev match", &[]),
    ub("s", "S"),
    ub("-", "_"),
];

const ESC_KEY: KeyDef = bare("Esc", "", "clear AB");

// Bottom row: ' q j k x b m w v z
const BOTTOM_ROW: &[KeyDef] = &[
    ub("'", "\""),
    bare("q", "Q", "next dlg"),
    bare("j", "J", "cursor \u{2193}"),
    bare("k", "K", "cursor \u{2191}"),
    bare("x", "X", "next chunk"),
    key("b", "B", "", "", &[("C-b", "pg back")]),
    bare("m", "M", "media picker"),
    ub("w", "W"),
    key("v", "V", "", "V: visual mode", &[]),
    ub("z", "Z"),
];

// Multi-key sequences
const SEQ_GG: KeyDef = bare("gg", "", "go to start");
const SEQ_G: KeyDef = key("G", "", "", "go to end", &[]);

// Arrow keys
const ARROW_UP: KeyDef = key("\u{2191}", "", "", "", &[("C-\u{2191}", "vol +")]);
const ARROW_DOWN: KeyDef = key("\u{2193}", "", "", "", &[("C-\u{2193}", "vol \u{2212}")]);
const ARROW_LEFT: KeyDef = bare("\u{2190}", "", "delete ts");
const ARROW_RIGHT: KeyDef = bare("\u{2192}", "", "start time");
```

- [ ] **Step 2: Verify it compiles** (the old `impl KeybindsOverlay` will be temporarily broken — that's expected, we fix it in Task 3)

This step is just to validate the data model compiles. Temporarily comment out the `impl KeybindsOverlay` block if needed to check.

---

### Task 3: Rewrite keybinds_overlay.rs — widget builder

**Files:**
- Rewrite: `src/ui/keybinds_overlay.rs` (bottom half — `impl KeybindsOverlay`)

- [ ] **Step 1: Write the key widget builder function**

Add this function above the `impl KeybindsOverlay`:

```rust
/// Build a single key widget from a KeyDef.
fn build_key(def: &KeyDef, css_width: &str) -> GtkBox {
    let cell = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .css_classes(vec!["kb-key"])
        .build();

    if !css_width.is_empty() {
        cell.set_width_request(css_width.parse::<i32>().unwrap_or(56));
    }

    // Determine color state
    let has_bare = !def.action.is_empty();
    let has_shift = !def.shift_action.is_empty();
    let has_mod = !def.modifiers.is_empty();

    if has_bare && (has_shift || has_mod) {
        cell.add_css_class("kb-key-bound-both");
    } else if has_bare {
        cell.add_css_class("kb-key-bound");
    } else if has_shift {
        cell.add_css_class("kb-key-bound-shift");
    } else if has_mod {
        cell.add_css_class("kb-key-bound-both");
    } else {
        cell.add_css_class("kb-key-unbound");
    }

    // Top area: unshifted char + shifted char overlay
    let top = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .build();

    let char_class = if has_bare && (has_shift || has_mod) {
        "kb-char-both"
    } else if has_bare {
        "kb-char-bound"
    } else if has_shift {
        "kb-char-shift"
    } else {
        "kb-char"
    };

    let char_label = Label::builder()
        .label(def.unshifted)
        .halign(gtk4::Align::Start)
        .hexpand(true)
        .css_classes(vec!["kb-char", char_class])
        .build();
    top.append(&char_label);

    if !def.shifted.is_empty() {
        let shifted_class = if has_shift || (has_bare && has_mod) {
            "kb-shifted-active"
        } else {
            "kb-shifted"
        };
        let shifted_label = Label::builder()
            .label(def.shifted)
            .halign(gtk4::Align::End)
            .valign(gtk4::Align::Start)
            .css_classes(vec!["kb-shifted", shifted_class])
            .build();
        top.append(&shifted_label);
    }

    cell.append(&top);

    // Action label (bare key)
    if !def.action.is_empty() {
        let action_label = Label::builder()
            .label(def.action)
            .halign(gtk4::Align::Start)
            .css_classes(vec!["kb-action"])
            .build();
        cell.append(&action_label);
    }

    // Shift action label
    if !def.shift_action.is_empty() {
        let shift_label = Label::builder()
            .label(def.shift_action)
            .halign(gtk4::Align::Start)
            .css_classes(vec!["kb-shift-action"])
            .build();
        cell.append(&shift_label);
    }

    // Tooltip for modifier bindings
    if !def.modifiers.is_empty() {
        let tooltip_lines: Vec<String> = def.modifiers.iter().map(|(combo, action)| {
            format!("{} \u{2192} {}", combo, action)
        }).collect();
        cell.set_tooltip_text(Some(&tooltip_lines.join("\n")));
    }

    cell
}

/// Build a single arrow key widget.
fn build_arrow(def: &KeyDef) -> GtkBox {
    let cell = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .css_classes(vec!["kb-arrow"])
        .halign(gtk4::Align::Center)
        .valign(gtk4::Align::Center)
        .build();

    let has_bare = !def.action.is_empty();
    let has_mod = !def.modifiers.is_empty();

    if has_bare && has_mod {
        cell.add_css_class("kb-arrow-bound-both");
    } else if has_bare {
        cell.add_css_class("kb-arrow-bound");
    } else if has_mod {
        cell.add_css_class("kb-arrow-bound-both");
    }

    let char_label = Label::builder()
        .label(def.unshifted)
        .css_classes(vec!["kb-arrow-char"])
        .build();
    cell.append(&char_label);

    if !def.action.is_empty() {
        let action_label = Label::builder()
            .label(def.action)
            .css_classes(vec!["kb-arrow-action"])
            .build();
        cell.append(&action_label);
    }

    if !def.modifiers.is_empty() {
        let tooltip_lines: Vec<String> = def.modifiers.iter().map(|(combo, action)| {
            format!("{} \u{2192} {}", combo, action)
        }).collect();
        cell.set_tooltip_text(Some(&tooltip_lines.join("\n")));
    }

    cell
}
```

- [ ] **Step 2: Rewrite the `impl KeybindsOverlay` block**

Replace the entire `impl KeybindsOverlay` with:

```rust
pub struct KeybindsOverlay {
    pub overlay: Overlay,
    container: GtkBox,
    scale_provider: CssProvider,
    scale: f64,
}

const DEFAULT_SCALE: f64 = 1.0;
const SCALE_STEP: f64 = 0.1;
const MIN_SCALE: f64 = 0.5;
const MAX_SCALE: f64 = 2.0;

impl KeybindsOverlay {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .build();
        container.add_css_class("keybinds-overlay");

        // --- Number row ---
        let num_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(4)
            .build();
        for def in NUMBER_ROW {
            num_row.append(&build_key(def, ""));
        }
        num_row.append(&build_key(&BACKSPACE, "78"));
        container.append(&num_row);

        // --- Upper row (Tab + keys) ---
        let upper_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(4)
            .build();
        upper_row.append(&build_key(&TAB_KEY, "86"));
        for def in UPPER_ROW {
            upper_row.append(&build_key(def, ""));
        }
        container.append(&upper_row);

        // --- Home row (Esc/Caps + keys) ---
        let home_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(4)
            .build();
        home_row.append(&build_key(&ESC_KEY, "96"));
        for def in HOME_ROW {
            home_row.append(&build_key(def, ""));
        }
        container.append(&home_row);

        // --- Bottom row (shifted right) ---
        let bottom_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(4)
            .margin_start(110)
            .build();
        for def in BOTTOM_ROW {
            bottom_row.append(&build_key(def, ""));
        }
        container.append(&bottom_row);

        // --- Sequences row + up arrow ---
        let seq_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(4)
            .margin_top(8)
            .build();
        seq_row.append(&build_key(&SEQ_GG, "78"));
        seq_row.append(&build_key(&SEQ_G, ""));

        // Spacer to push arrow right
        let spacer = GtkBox::builder().hexpand(true).build();
        seq_row.append(&spacer);

        // Up arrow centered above down arrow (width = 3 arrows + 2 gaps = 3*46 = 138)
        let up_container = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .halign(gtk4::Align::Center)
            .width_request(138)
            .build();
        up_container.append(&build_arrow(&ARROW_UP));
        seq_row.append(&up_container);

        container.append(&seq_row);

        // --- Arrow bottom row (left, down, right) ---
        let arrow_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(2)
            .halign(gtk4::Align::End)
            .build();
        arrow_row.append(&build_arrow(&ARROW_LEFT));
        arrow_row.append(&build_arrow(&ARROW_DOWN));
        arrow_row.append(&build_arrow(&ARROW_RIGHT));
        container.append(&arrow_row);

        // --- Legend ---
        let legend = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(16)
            .css_classes(vec!["kb-legend"])
            .build();

        let legend_items: &[(&str, &str, &str)] = &[
            ("kb-legend-bound", "#88ff88", "bare key"),
            ("kb-legend-shift", "#88aaff", "shift only"),
            ("kb-legend-both", "#aaaaaa", "both / modifier"),
            ("kb-legend-unbound", "#666666", "unbound"),
        ];
        for &(swatch_class, color, text) in legend_items {
            let item_box = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(4)
                .build();
            let swatch = GtkBox::builder()
                .css_classes(vec!["kb-legend-swatch", swatch_class])
                .build();
            item_box.append(&swatch);
            let label = Label::builder()
                .label(text)
                .build();
            label.set_markup(&format!("<span color=\"{}\">{}</span>", color, text));
            item_box.append(&label);
            legend.append(&item_box);
        }

        // Alt indicator
        let alt_label = Label::new(None);
        alt_label.set_markup("<span color=\"#cc88ff\">\u{2022} Alt+</span>");
        legend.append(&alt_label);

        let close_hint = Label::builder()
            .label("Esc to close \u{00b7} C-/ to toggle")
            .halign(gtk4::Align::End)
            .hexpand(true)
            .css_classes(vec!["kb-action"])
            .build();
        legend.append(&close_hint);

        container.append(&legend);

        // Scoped CSS provider for font scaling
        let scale_provider = CssProvider::new();
        gtk4::style_context_add_provider_for_display(
            &gtk4::gdk::Display::default().unwrap(),
            &scale_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER + 1,
        );

        KeybindsOverlay { overlay, container, scale_provider, scale: DEFAULT_SCALE }
    }

    pub fn show(&self) {
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    pub fn adjust_scale(&mut self, delta: i32) {
        let new_scale = (self.scale + delta as f64 * SCALE_STEP).clamp(MIN_SCALE, MAX_SCALE);
        self.scale = new_scale;
        self.apply_scale();
    }

    pub fn reset_scale(&mut self) {
        self.scale = DEFAULT_SCALE;
        self.apply_scale();
    }

    fn apply_scale(&self) {
        let char_size = (40.0 * self.scale) as u32;
        let shifted_size = (28.0 * self.scale) as u32;
        let action_size = (24.0 * self.scale) as u32;
        let shift_action_size = (22.0 * self.scale) as u32;
        let arrow_char_size = (34.0 * self.scale) as u32;
        let arrow_action_size = (20.0 * self.scale) as u32;
        let css = format!(
            ".kb-char, .kb-char-bound, .kb-char-shift, .kb-char-both \
             {{ font-size: {}px; }} \
             .kb-shifted, .kb-shifted-active {{ font-size: {}px; }} \
             .kb-action {{ font-size: {}px; }} \
             .kb-shift-action {{ font-size: {}px; }} \
             .kb-arrow-char {{ font-size: {}px; }} \
             .kb-arrow-action {{ font-size: {}px; }}",
            char_size, shifted_size, action_size, shift_action_size,
            arrow_char_size, arrow_action_size,
        );
        self.scale_provider.load_from_string(&css);
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.container);
        self.container.set_visible(false);
    }
}
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles successfully

- [ ] **Step 4: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "feat: rewrite keybinds overlay as visual RPD keyboard layout"
```

---

### Task 4: Visual verification and tuning

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (minor tweaks)
- Modify: `src/theme.rs` (CSS adjustments)

- [ ] **Step 1: Run the app and open the overlay**

Run: `cargo run` then press Ctrl+/ to toggle the keybinds overlay.

Verify:
- All 4 keyboard rows render with correct stagger
- Tab and Esc keys are visibly wider
- Backspace appears at end of number row
- Bottom row starts slightly right of `a`
- Arrow keys form inverted-T below the sequences row
- Color coding: green for bare, blue for shift-only, gradient for both, gray for unbound
- Hover keys with modifiers shows tooltip (e.g., hover `,` shows "C-, → settings")
- Legend bar displays correctly
- Ctrl+Up/Down scales the overlay
- Esc closes the overlay

- [ ] **Step 2: Adjust any sizing/spacing issues found**

Common adjustments:
- Key min-width/min-height in CSS if keys look too small or large
- Margin-start on bottom row if stagger doesn't look right
- Arrow cluster width if up arrow isn't centered above down
- Font sizes in CSS if text overflows keys

- [ ] **Step 3: Commit any adjustments**

```bash
git add src/ui/keybinds_overlay.rs src/theme.rs
git commit -m "fix: tune keyboard overlay sizing and spacing"
```
