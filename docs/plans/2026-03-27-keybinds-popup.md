# Keybinds Popup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a keyboard-shaped keybinds reference popup toggled with Ctrl+/, showing all linux-lit keybinds positioned by RPD keyboard row.

**Architecture:** New `KeybindsOverlay` widget in `src/ui/keybinds_overlay.rs` following the same Overlay pattern as `SettingsOverlay`. Static keybind data defined as const arrays. CSS classes added to `theme.rs`. Key routing added to `keymap.rs`. Overlay inserted into the nesting chain in `app.rs`.

**Tech Stack:** GTK4, Rust, sourceview5

---

### Task 1: Create keybinds_overlay.rs with data model and widget

**Files:**
- Create: `src/ui/keybinds_overlay.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Add module declaration**

In `src/ui/mod.rs`, add the new module:

```rust
pub mod keybinds_overlay;
```

- [ ] **Step 2: Create keybinds_overlay.rs with data types and const keybind data**

Create `src/ui/keybinds_overlay.rs` with the following content:

```rust
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, Overlay};

#[derive(Clone, Copy)]
enum ModifierType {
    Bare,
    Ctrl,
    Alt,
    CtrlAlt,
}

struct KeybindEntry {
    key_label: &'static str,
    action: &'static str,
    modifier: ModifierType,
    sub_binds: &'static [(&'static str, &'static str, ModifierType)],
}

const fn kb(key_label: &'static str, action: &'static str) -> KeybindEntry {
    KeybindEntry { key_label, action, modifier: ModifierType::Bare, sub_binds: &[] }
}

const fn kb_sub(
    key_label: &'static str,
    action: &'static str,
    sub_binds: &'static [(&'static str, &'static str, ModifierType)],
) -> KeybindEntry {
    KeybindEntry { key_label, action, modifier: ModifierType::Bare, sub_binds }
}

const fn kb_unbound(key_label: &'static str) -> KeybindEntry {
    KeybindEntry { key_label, action: "", modifier: ModifierType::Bare, sub_binds: &[] }
}

const fn kb_mod(key_label: &'static str, action: &'static str, modifier: ModifierType) -> KeybindEntry {
    KeybindEntry { key_label, action, modifier, sub_binds: &[] }
}

// Number row: $ + [ { ( & = ) } ] * ! |
// Only bound keys: +, 0, !, |
const NUMBER_ROW: &[KeybindEntry] = &[
    kb_unbound("$"),
    kb("+", "toggle speed"),
    kb_unbound("["),
    kb_unbound("{"),
    kb_unbound("("),
    kb_unbound("&"),
    kb_unbound("="),
    kb_unbound(")"),
    kb_unbound("}"),
    kb_unbound("]"),
    kb("0", "reset font"),
    kb("! |", "font \u{2212} / +"),
];

// Upper row: ; , . p y f g c r l / @ \  (with tab indent)
const UPPER_ROW: &[KeybindEntry] = &[
    kb_unbound(";"),
    kb_sub(",", "prev dialogue", &[("C-,", "settings", ModifierType::Ctrl)]),
    kb(".", "set chapter"),
    kb_sub("p P", "nudge \u{2212}/+0.2s", &[("C-p", "picker", ModifierType::Ctrl)]),
    kb("y", "prev chunk"),
    kb_sub("f F", "cycle font \u{2192}/\u{2190}", &[("C-f", "pg fwd", ModifierType::Ctrl)]),
    kb_unbound("g"),
    kb_unbound("c"),
    kb_unbound("r"),
    kb("l", "toggle signs"),
    kb_sub("/", "search", &[("C-/", "keybinds", ModifierType::Ctrl)]),
];

// Home row: a o e u i d h t n s -
const HOME_ROW: &[KeybindEntry] = &[
    kb("a", "play from ts"),
    kb("o O", "seek \u{2212}3.5/\u{2212}60"),
    kb("e E", "seek +3.5/+60"),
    kb_sub("u", "start time/undo", &[("C-u", "pg back", ModifierType::Ctrl)]),
    kb("i", "set end time"),
    kb_mod("C-d", "pg fwd", ModifierType::Ctrl),
    kb_unbound("h"),
    kb("Tab", "play/pause"),
    kb("n N", "next/prev match"),
    kb_unbound("s"),
    kb_unbound("\u{2212}"),
];

// Bottom row: ' q j k x b m w v z
const BOTTOM_ROW: &[KeybindEntry] = &[
    kb_unbound("'"),
    kb("q", "next dialogue"),
    kb("j", "cursor \u{2193}"),
    kb("k", "cursor \u{2191}"),
    kb("x", "next chunk"),
    kb_mod("C-b", "pg back", ModifierType::Ctrl),
    kb("m", "media picker"),
    kb_unbound("w"),
    kb("V", "visual mode"),
    kb_unbound("z"),
];

// Other: gg, G, arrow, backspace, esc, ctrl+arrows, alt combos
const OTHER_ROW: &[KeybindEntry] = &[
    kb("g g", "go to start"),
    kb("G", "go to end"),
    kb("\u{2192}", "set start time"),
    kb("\u{232b}", "delete ts"),
    kb("Esc", "clear AB loop"),
    kb_mod("C-\u{2191} C-\u{2193}", "vol +/\u{2212}", ModifierType::Ctrl),
    kb_mod("M-f", "font info", ModifierType::Alt),
    kb_mod("M-i", "translations", ModifierType::Alt),
    kb_mod("C-M-l", "save + quit", ModifierType::CtrlAlt),
];

pub struct KeybindsOverlay {
    pub overlay: Overlay,
    container: GtkBox,
}

impl KeybindsOverlay {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(820)
            .build();
        container.add_css_class("keybinds-overlay");

        let rows: &[(&str, &[KeybindEntry], i32)] = &[
            ("Number Row", NUMBER_ROW, 0),
            ("Upper Row", UPPER_ROW, 16),
            ("Home Row", HOME_ROW, 24),
            ("Bottom Row", BOTTOM_ROW, 36),
            ("Other", OTHER_ROW, 0),
        ];

        for &(title, entries, indent) in rows {
            let header = Label::builder()
                .label(title)
                .halign(gtk4::Align::Start)
                .css_classes(vec!["keybind-row-header"])
                .build();
            container.append(&header);

            let row_box = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(3)
                .margin_start(indent)
                .margin_bottom(10)
                .build();
            if title == "Other" {
                row_box.set_halign(gtk4::Align::Start);
            }

            for entry in entries {
                let cell = GtkBox::builder()
                    .orientation(Orientation::Vertical)
                    .css_classes(vec!["keybind-key"])
                    .build();

                if entry.action.is_empty() {
                    cell.add_css_class("keybind-key-unbound");
                }

                let modifier_class = match entry.modifier {
                    ModifierType::Bare => "keybind-label-bare",
                    ModifierType::Ctrl => "keybind-label-ctrl",
                    ModifierType::Alt => "keybind-label-alt",
                    ModifierType::CtrlAlt => "keybind-label-ctrlalt",
                };

                let key_label = Label::builder()
                    .label(entry.key_label)
                    .halign(gtk4::Align::Start)
                    .css_classes(vec![modifier_class])
                    .build();
                cell.append(&key_label);

                if !entry.action.is_empty() {
                    let action_label = Label::builder()
                        .label(entry.action)
                        .halign(gtk4::Align::Start)
                        .css_classes(vec!["keybind-action"])
                        .build();
                    cell.append(&action_label);
                }

                for &(sub_key, sub_action, sub_mod) in entry.sub_binds {
                    let sub_class = match sub_mod {
                        ModifierType::Bare => "keybind-label-bare",
                        ModifierType::Ctrl => "keybind-label-ctrl",
                        ModifierType::Alt => "keybind-label-alt",
                        ModifierType::CtrlAlt => "keybind-label-ctrlalt",
                    };
                    let sub_label = Label::builder()
                        .label(&format!("{} {}", sub_key, sub_action))
                        .halign(gtk4::Align::Start)
                        .css_classes(vec![sub_class, "keybind-action"])
                        .build();
                    cell.append(&sub_label);
                }

                row_box.append(&cell);
            }

            container.append(&row_box);
        }

        // Legend bar
        let legend = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(16)
            .css_classes(vec!["keybind-legend"])
            .build();

        let legend_items: &[(&str, &str)] = &[
            ("keybind-label-bare", "key"),
            ("keybind-label-ctrl", "Ctrl+"),
            ("keybind-label-alt", "Alt+"),
            ("keybind-label-ctrlalt", "Ctrl+Alt+"),
        ];
        for &(class, text) in legend_items {
            let item = Label::builder()
                .label(&format!("\u{25a0} {}", text))
                .css_classes(vec![class])
                .build();
            legend.append(&item);
        }

        let close_hint = Label::builder()
            .label("Esc to close \u{00b7} Ctrl+/ to toggle")
            .halign(gtk4::Align::End)
            .hexpand(true)
            .css_classes(vec!["keybind-action"])
            .build();
        legend.append(&close_hint);

        container.append(&legend);

        KeybindsOverlay { overlay, container }
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

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.container);
        self.container.set_visible(false);
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | head -20`
Expected: Compilation succeeds (new module is declared but not yet used, so no errors)

- [ ] **Step 4: Commit**

```bash
git add src/ui/keybinds_overlay.rs src/ui/mod.rs
git commit -m "feat: add keybinds overlay widget with RPD keyboard layout data"
```

---

### Task 2: Add CSS classes for keybinds overlay

**Files:**
- Modify: `src/theme.rs:222-260` (inside `generate_css()`)

- [ ] **Step 1: Add keybinds CSS classes to generate_css()**

In `src/theme.rs`, inside the `generate_css()` function, add the keybinds overlay CSS classes at the end of the format string (before the closing `"`):

Add after the `.action-separator` line (line 253):

```rust
         .keybinds-overlay {{ background-color: rgba(40, 40, 40, 0.95); color: white; \
           padding: 16px; border-radius: 8px; }} \
         .keybind-key {{ background-color: rgba(255, 255, 255, 0.08); \
           border-radius: 3px; min-width: 68px; padding: 3px 6px; }} \
         .keybind-key-unbound {{ opacity: 0.25; }} \
         .keybind-label-bare {{ color: #7db8f0; font-size: 11px; font-weight: bold; }} \
         .keybind-label-ctrl {{ color: #d4a052; font-size: 11px; font-weight: bold; }} \
         .keybind-label-alt {{ color: #c47dd4; font-size: 11px; font-weight: bold; }} \
         .keybind-label-ctrlalt {{ color: #d45050; font-size: 11px; font-weight: bold; }} \
         .keybind-action {{ color: rgba(255, 255, 255, 0.5); font-size: 9px; }} \
         .keybind-row-header {{ font-size: 9px; letter-spacing: 2px; \
           color: rgba(255, 255, 255, 0.35); margin-bottom: 4px; }} \
         .keybind-legend {{ border-top: 1px solid rgba(255, 255, 255, 0.1); \
           margin-top: 12px; padding-top: 8px; }}",
```

The full closing of the format string changes from:

```rust
         .action-separator {{ opacity: 0.3; margin: 4px 12px; }}",
```

to:

```rust
         .action-separator {{ opacity: 0.3; margin: 4px 12px; }} \
         .keybinds-overlay {{ background-color: rgba(40, 40, 40, 0.95); color: white; \
           padding: 16px; border-radius: 8px; }} \
         .keybind-key {{ background-color: rgba(255, 255, 255, 0.08); \
           border-radius: 3px; min-width: 68px; padding: 3px 6px; }} \
         .keybind-key-unbound {{ opacity: 0.25; }} \
         .keybind-label-bare {{ color: #7db8f0; font-size: 11px; font-weight: bold; }} \
         .keybind-label-ctrl {{ color: #d4a052; font-size: 11px; font-weight: bold; }} \
         .keybind-label-alt {{ color: #c47dd4; font-size: 11px; font-weight: bold; }} \
         .keybind-label-ctrlalt {{ color: #d45050; font-size: 11px; font-weight: bold; }} \
         .keybind-action {{ color: rgba(255, 255, 255, 0.5); font-size: 9px; }} \
         .keybind-row-header {{ font-size: 9px; letter-spacing: 2px; \
           color: rgba(255, 255, 255, 0.35); margin-bottom: 4px; }} \
         .keybind-legend {{ border-top: 1px solid rgba(255, 255, 255, 0.1); \
           margin-top: 12px; padding-top: 8px; }}",
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | head -20`
Expected: Compilation succeeds

- [ ] **Step 3: Commit**

```bash
git add src/theme.rs
git commit -m "feat: add CSS classes for keybinds overlay"
```

---

### Task 3: Integrate keybinds overlay into AppState and overlay chain

**Files:**
- Modify: `src/app.rs:26-75` (AppState struct), `src/app.rs:244-264` (overlay chain setup)

- [ ] **Step 1: Add keybinds_overlay field to AppState**

In `src/app.rs`, add the field to the `AppState` struct after `action_popup_widget`:

```rust
    pub keybinds_overlay: crate::ui::keybinds_overlay::KeybindsOverlay,
```

- [ ] **Step 2: Create KeybindsOverlay and insert into overlay chain**

In `src/app.rs`, in the `build_ui` function, after the settings overlay setup (after line 252 `settings_overlay.overlay.set_vexpand(true);`), add:

```rust
    // Keybinds overlay wraps the settings overlay
    let keybinds_overlay = crate::ui::keybinds_overlay::KeybindsOverlay::new();
    keybinds_overlay.attach(&settings_overlay.overlay);
    keybinds_overlay.overlay.set_vexpand(true);
```

Then update the action popup to be added to the keybinds overlay instead of settings overlay. Change:

```rust
    settings_overlay.overlay.add_overlay(&action_popup_widget.container);
```

to:

```rust
    keybinds_overlay.overlay.add_overlay(&action_popup_widget.container);
```

Then update the vbox to use keybinds_overlay instead of settings_overlay. Change:

```rust
    vbox.append(&settings_overlay.overlay);
```

to:

```rust
    vbox.append(&keybinds_overlay.overlay);
```

- [ ] **Step 3: Add field to AppState initialization**

In the `AppState { ... }` struct literal (around line 269), add after `action_popup_widget,`:

```rust
        keybinds_overlay,
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | head -20`
Expected: Compilation succeeds

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat: integrate keybinds overlay into AppState and overlay chain"
```

---

### Task 4: Add Ctrl+/ toggle and key routing in keymap.rs

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Add keybinds overlay visibility check and key routing**

In `src/input/keymap.rs`, in `handle_key()`, add the keybinds overlay handling **after the search bar handling** (after the `if search_visible { ... }` block, around line 348) and **before the action popup handling**:

```rust
    // --- Keybinds overlay (when visible) ---
    let keybinds_visible = state.borrow().keybinds_overlay.is_visible();
    if keybinds_visible {
        match key_name {
            "Escape" => {
                state.borrow().keybinds_overlay.hide();
                return true;
            }
            _ => return true, // consume all keys when keybinds visible
        }
    }
```

- [ ] **Step 2: Add Ctrl+/ toggle keybind**

In the `if is_ctrl { ... }` block (around line 469), add a new match arm **before** the existing `"d" | "f"` arm:

```rust
            "slash" => {
                let s = state.borrow();
                if s.keybinds_overlay.is_visible() {
                    s.keybinds_overlay.hide();
                } else {
                    s.keybinds_overlay.show();
                }
                return true;
            }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | head -20`
Expected: Compilation succeeds

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat: add Ctrl+/ toggle and key routing for keybinds overlay"
```

---

### Task 5: Manual verification

- [ ] **Step 1: Run the app and test**

Run: `cargo run` (user runs this)

Test checklist:
1. Press `Ctrl+/` — keybinds popup appears centered over the text
2. All four keyboard rows visible without scrolling, with staggered indents
3. Unbound keys shown faded
4. Color coding: blue for bare keys, gold for Ctrl, purple for Alt, red for Ctrl+Alt
5. Sub-binds (e.g., "C-p picker" under "p") visible below their parent key
6. Legend bar at bottom with color swatches
7. Press `Esc` — popup closes
8. Press `Ctrl+/` again — popup toggles back on, then off
9. While popup is visible, other keys (j, k, /, etc.) are consumed and don't affect the text
10. Other overlays still work: `Ctrl+,` (settings), `Ctrl+p` (picker), `m` (media)

- [ ] **Step 2: Final commit if any fixes needed**

```bash
git add -u
git commit -m "fix: keybinds overlay adjustments from manual testing"
```
