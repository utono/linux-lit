# Settings Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a modal settings overlay (Ctrl+,) for adjusting line spacing, column width, text margins, and theme with live preview, confirm/revert semantics, and config persistence.

**Architecture:** A new `SettingsOverlay` widget in `src/ui/settings_overlay.rs` following the `LibraryPicker` pattern. Config gains three new fields with serde defaults. Keymap routes Ctrl+, and overlay-visible keys to the overlay. All changes apply live; Enter persists, Escape reverts.

**Tech Stack:** Rust, GTK4, sourceview5, serde_json

**Spec:** `docs/superpowers/specs/2026-03-26-settings-overlay-design.md`

---

### Task 1: Add config fields for line_spacing, column_width, text_margins

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add fields and defaults to Config**

In `src/config.rs`, add three new fields to the `Config` struct and their default functions:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: u32,
    #[serde(default = "default_line_spacing")]
    pub line_spacing: u32,
    #[serde(default = "default_column_width")]
    pub column_width: u32,
    #[serde(default = "default_text_margins")]
    pub text_margins: u32,
    #[serde(default)]
    pub last_work: Option<String>,
    #[serde(default)]
    pub last_line: usize,
}

fn default_line_spacing() -> u32 {
    4
}

fn default_column_width() -> u32 {
    950
}

fn default_text_margins() -> u32 {
    48
}
```

Update `Default` impl:

```rust
impl Default for Config {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            line_spacing: default_line_spacing(),
            column_width: default_column_width(),
            text_margins: default_text_margins(),
            last_work: None,
            last_line: 0,
        }
    }
}
```

- [ ] **Step 2: Use config values in app.rs instead of hardcoded constants**

In `src/app.rs`, in `build_window`, replace the hardcoded values. Change line spacing (around line 163):

```rust
    // Line spacing
    text_view.set_pixels_above_lines(config.line_spacing as i32);
    text_view.set_pixels_below_lines(config.line_spacing as i32);
```

Change text margins (around line 167):

```rust
    // Text area padding (inside the text background)
    text_view.set_left_margin(config.text_margins as i32);
    text_view.set_right_margin(config.text_margins as i32);
```

Change column width (around line 179):

```rust
        .width_request(config.column_width as i32)
```

- [ ] **Step 3: Build and verify**

Run: `cargo build`
Expected: compiles with no errors (warnings OK)

- [ ] **Step 4: Commit**

```bash
git add src/config.rs src/app.rs
git commit -m "feat: add line_spacing, column_width, text_margins to config"
```

---

### Task 2: Create SettingsOverlay widget

**Files:**
- Create: `src/ui/settings_overlay.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Add module declaration**

In `src/ui/mod.rs`, add:

```rust
pub mod library_picker;
pub mod search_bar;
pub mod settings_overlay;
```

- [ ] **Step 2: Create the SettingsOverlay struct and constructor**

Create `src/ui/settings_overlay.rs`:

```rust
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, Overlay};

use crate::theme::Theme;

const NUM_SETTINGS: usize = 4;

#[derive(Clone)]
struct SettingsSnapshot {
    line_spacing: u32,
    column_width: u32,
    text_margins: u32,
    theme_index: usize,
}

pub struct SettingsOverlay {
    pub overlay: Overlay,
    container: GtkBox,
    rows: Vec<GtkBox>,
    value_labels: Vec<Label>,
    selected: usize,
    snapshot: SettingsSnapshot,
    themes: Vec<Theme>,
    theme_index: usize,
}

impl SettingsOverlay {
    pub fn new(themes: Vec<Theme>, current_theme_name: &str) -> Self {
        let overlay = Overlay::new();

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(500)
            .build();
        container.add_css_class("settings-overlay");

        // Title
        let title = Label::builder()
            .label("Settings")
            .css_classes(vec!["settings-title"])
            .build();
        container.append(&title);

        // Setting names
        let names = ["Line Spacing", "Column Width", "Text Margins", "Theme"];

        let mut rows = Vec::new();
        let mut value_labels = Vec::new();

        for name in &names {
            let row = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(0)
                .css_classes(vec!["settings-row"])
                .build();

            let name_label = Label::builder()
                .label(*name)
                .halign(gtk4::Align::Start)
                .hexpand(true)
                .build();

            let value_label = Label::builder()
                .label("")
                .halign(gtk4::Align::End)
                .build();

            row.append(&name_label);
            row.append(&value_label);
            container.append(&row);

            rows.push(row);
            value_labels.push(value_label);
        }

        // Footer
        let footer = Label::builder()
            .label("j/k navigate · h/l adjust · Enter confirm · Esc revert")
            .css_classes(vec!["settings-footer"])
            .build();
        container.append(&footer);

        // Find current theme index
        let theme_index = themes
            .iter()
            .position(|t| t.name == current_theme_name)
            .unwrap_or(0);

        SettingsOverlay {
            overlay,
            container,
            rows,
            value_labels,
            selected: 0,
            snapshot: SettingsSnapshot {
                line_spacing: 4,
                column_width: 950,
                text_margins: 48,
                theme_index,
            },
            themes,
            theme_index,
        }
    }

    pub fn show(&mut self, line_spacing: u32, column_width: u32, text_margins: u32) {
        self.snapshot = SettingsSnapshot {
            line_spacing,
            column_width,
            text_margins,
            theme_index: self.theme_index,
        };
        self.selected = 0;
        self.update_value_labels(line_spacing, column_width, text_margins);
        self.update_row_highlight();
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

    pub fn move_selection(&mut self, delta: i32) {
        let new = (self.selected as i32 + delta).rem_euclid(NUM_SETTINGS as i32) as usize;
        self.selected = new;
        self.update_row_highlight();
    }

    /// Adjust the currently selected setting. Returns the new values to apply live.
    /// For theme changes, returns the new Theme to apply.
    pub fn adjust_value(
        &mut self,
        delta: i32,
        line_spacing: u32,
        column_width: u32,
        text_margins: u32,
    ) -> SettingsChange {
        match self.selected {
            0 => {
                // Line Spacing: 0–20, step 1
                let new_val = (line_spacing as i32 + delta).clamp(0, 20) as u32;
                self.value_labels[0].set_label(&format!("◀ {}px ▶", new_val));
                SettingsChange::LineSpacing(new_val)
            }
            1 => {
                // Column Width: 400–1200, step 50
                let new_val = (column_width as i32 + delta * 50).clamp(400, 1200) as u32;
                self.value_labels[1].set_label(&format!("◀ {}px ▶", new_val));
                SettingsChange::ColumnWidth(new_val)
            }
            2 => {
                // Text Margins: 8–96, step 4
                let new_val = (text_margins as i32 + delta * 4).clamp(8, 96) as u32;
                self.value_labels[2].set_label(&format!("◀ {}px ▶", new_val));
                SettingsChange::TextMargins(new_val)
            }
            3 => {
                // Theme: cycle through loaded themes
                let len = self.themes.len();
                if len == 0 {
                    return SettingsChange::None;
                }
                let new_idx = (self.theme_index as i32 + delta).rem_euclid(len as i32) as usize;
                self.theme_index = new_idx;
                let theme = &self.themes[new_idx];
                self.value_labels[3].set_label(&format!("◀ {} ▶", theme.display_name));
                SettingsChange::Theme(theme.clone())
            }
            _ => SettingsChange::None,
        }
    }

    pub fn snapshot(&self) -> (u32, u32, u32, usize) {
        (
            self.snapshot.line_spacing,
            self.snapshot.column_width,
            self.snapshot.text_margins,
            self.snapshot.theme_index,
        )
    }

    pub fn set_theme_index(&mut self, idx: usize) {
        self.theme_index = idx;
    }

    pub fn themes(&self) -> &[Theme] {
        &self.themes
    }

    fn update_value_labels(&self, line_spacing: u32, column_width: u32, text_margins: u32) {
        self.value_labels[0].set_label(&format!("◀ {}px ▶", line_spacing));
        self.value_labels[1].set_label(&format!("◀ {}px ▶", column_width));
        self.value_labels[2].set_label(&format!("◀ {}px ▶", text_margins));
        if let Some(theme) = self.themes.get(self.theme_index) {
            self.value_labels[3].set_label(&format!("◀ {} ▶", theme.display_name));
        }
    }

    fn update_row_highlight(&self) {
        for (i, row) in self.rows.iter().enumerate() {
            if i == self.selected {
                row.add_css_class("settings-row-selected");
            } else {
                row.remove_css_class("settings-row-selected");
            }
        }
    }
}

pub enum SettingsChange {
    LineSpacing(u32),
    ColumnWidth(u32),
    TextMargins(u32),
    Theme(Theme),
    None,
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo build`
Expected: compiles (warnings about unused code OK at this stage)

- [ ] **Step 4: Commit**

```bash
git add src/ui/settings_overlay.rs src/ui/mod.rs
git commit -m "feat: create SettingsOverlay widget"
```

---

### Task 3: Wire SettingsOverlay into AppState and window

**Files:**
- Modify: `src/app.rs`
- Modify: `src/theme.rs` (add CSS for settings overlay)

- [ ] **Step 1: Add settings_overlay field to AppState**

In `src/app.rs`, add to the `AppState` struct:

```rust
    pub settings_overlay: crate::ui::settings_overlay::SettingsOverlay,
```

- [ ] **Step 2: Add CSS for settings overlay**

In `src/theme.rs`, in `generate_css`, append to the format string (before the closing `"`):

```rust
         .settings-overlay {{ background-color: rgba(40, 40, 40, 0.95); color: white; \
           padding: 16px; border-radius: 8px; }} \
         .settings-title {{ font-size: 18px; font-weight: bold; \
           margin-bottom: 12px; padding-bottom: 12px; \
           border-bottom: 1px solid rgba(255,255,255,0.2); }} \
         .settings-row {{ padding: 8px 12px; margin: 2px 0; border-radius: 4px; }} \
         .settings-row-selected {{ background-color: rgba(100, 140, 200, 0.8); \
           border-left: 3px solid rgba(100, 180, 255, 0.9); }} \
         .settings-footer {{ font-size: 11px; opacity: 0.6; margin-top: 12px; \
           text-align: center; }}"
```

- [ ] **Step 3: Construct and attach settings overlay in build_window**

In `src/app.rs`, in `build_window`, after the library picker section and before `window.set_child`:

Load all themes and create the settings overlay:

```rust
    // Settings overlay
    let all_themes = crate::theme::load_all_themes();
    let mut settings_overlay = crate::ui::settings_overlay::SettingsOverlay::new(
        all_themes,
        &theme.name,
    );
```

The widget tree needs to nest the settings overlay around the existing picker overlay. Replace the existing section that builds the vbox:

```rust
    // Settings overlay wraps the picker overlay
    settings_overlay.attach(&picker.overlay);
    settings_overlay.overlay.set_vexpand(true);

    // Search bar at bottom
    let search_bar = SearchBar::new();
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    vbox.append(&settings_overlay.overlay);
    vbox.append(&search_bar.container);
```

Add `settings_overlay` to the `AppState` initialization:

```rust
        settings_overlay,
```

(Place it after the `line_map: None,` line.)

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: compiles successfully

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/theme.rs
git commit -m "feat: wire SettingsOverlay into AppState and window"
```

---

### Task 4: Add keybindings for settings overlay

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Add Ctrl+, to open/toggle settings overlay**

In `src/input/keymap.rs`, in `handle_key`, add after the picker-visible block (after line 104 `return false;`) and before the search bar visible check:

```rust
    // Settings overlay
    let settings_visible = state.borrow().settings_overlay.is_visible();

    // Ctrl+,: toggle settings overlay
    if is_ctrl && key_name == "comma" && !settings_visible && !picker_visible {
        let s = state.borrow();
        let ls = s.config.line_spacing;
        let cw = s.config.column_width;
        let tm = s.config.text_margins;
        drop(s);
        state.borrow_mut().settings_overlay.show(ls, cw, tm);
        return true;
    }

    // Settings overlay visible — route keys
    if settings_visible {
        match key_name {
            "Escape" => {
                // Revert to snapshot values
                let (snap_ls, snap_cw, snap_tm, snap_ti) = state.borrow().settings_overlay.snapshot();
                {
                    let mut s = state.borrow_mut();
                    s.text_view.set_pixels_above_lines(snap_ls as i32);
                    s.text_view.set_pixels_below_lines(snap_ls as i32);
                    s.scrolled_window.set_width_request(snap_cw as i32);
                    s.text_view.set_left_margin(snap_tm as i32);
                    s.text_view.set_right_margin(snap_tm as i32);
                    s.config.line_spacing = snap_ls;
                    s.config.column_width = snap_cw;
                    s.config.text_margins = snap_tm;
                    // Revert theme if changed
                    if let Some(snap_theme) = s.settings_overlay.themes().get(snap_ti) {
                        let snap_theme = snap_theme.clone();
                        s.settings_overlay.set_theme_index(snap_ti);
                        apply_theme_to_state(&mut s, &snap_theme);
                    }
                    s.settings_overlay.hide();
                }
                return true;
            }
            "Return" => {
                // Confirm: persist config and close
                {
                    let mut s = state.borrow_mut();
                    crate::config::save(&s.config);
                    s.settings_overlay.hide();
                }
                return true;
            }
            "j" | "Down" => {
                state.borrow_mut().settings_overlay.move_selection(1);
                return true;
            }
            "k" | "Up" => {
                state.borrow_mut().settings_overlay.move_selection(-1);
                return true;
            }
            "h" | "Left" => {
                let (ls, cw, tm) = {
                    let s = state.borrow();
                    (s.config.line_spacing, s.config.column_width, s.config.text_margins)
                };
                let change = state.borrow_mut().settings_overlay.adjust_value(-1, ls, cw, tm);
                apply_settings_change(state, change);
                return true;
            }
            "l" | "Right" => {
                let (ls, cw, tm) = {
                    let s = state.borrow();
                    (s.config.line_spacing, s.config.column_width, s.config.text_margins)
                };
                let change = state.borrow_mut().settings_overlay.adjust_value(1, ls, cw, tm);
                apply_settings_change(state, change);
                return true;
            }
            _ => return true, // consume all other keys when settings visible
        }
    }
```

- [ ] **Step 2: Add the apply_settings_change and apply_theme_to_state helper functions**

Add these at the bottom of `src/input/keymap.rs`:

```rust
fn apply_settings_change(
    state: &Rc<RefCell<crate::app::AppState>>,
    change: crate::ui::settings_overlay::SettingsChange,
) {
    use crate::ui::settings_overlay::SettingsChange;
    let mut s = state.borrow_mut();
    match change {
        SettingsChange::LineSpacing(val) => {
            s.text_view.set_pixels_above_lines(val as i32);
            s.text_view.set_pixels_below_lines(val as i32);
            s.config.line_spacing = val;
        }
        SettingsChange::ColumnWidth(val) => {
            s.scrolled_window.set_width_request(val as i32);
            s.config.column_width = val;
        }
        SettingsChange::TextMargins(val) => {
            s.text_view.set_left_margin(val as i32);
            s.text_view.set_right_margin(val as i32);
            s.config.text_margins = val;
        }
        SettingsChange::Theme(theme) => {
            apply_theme_to_state(&mut s, &theme);
        }
        SettingsChange::None => {}
    }
}

fn apply_theme_to_state(state: &mut crate::app::AppState, theme: &crate::theme::Theme) {
    let css = crate::theme::generate_css(theme, &state.config.font_family, state.config.font_size);
    state.css_provider.load_from_string(&css);

    // Update dim tag foreground
    state.dim_tag.set_property("foreground", &theme.dim_fg);
    state.ab_dim_tag.set_property("foreground", &theme.dim_fg);

    // Write .current_theme file
    let home = std::env::var("HOME").unwrap_or_default();
    let theme_path = std::path::PathBuf::from(&home)
        .join("utono/themes/.config/themes/.current_theme");
    let _ = std::fs::write(&theme_path, &theme.name);

    state.theme = theme.clone();

    crate::logging::log(&format!("SETTINGS: theme changed to {}", theme.display_name));
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo build`
Expected: compiles successfully

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat: add keybindings for settings overlay"
```

---

### Task 5: Manual test and polish

**Files:**
- Possibly: `src/ui/settings_overlay.rs`, `src/input/keymap.rs`, `src/theme.rs`

- [ ] **Step 1: Build final**

Run: `cargo build`
Expected: clean build

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: no errors (warnings acceptable)

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 4: Commit any fixes**

If clippy or tests required changes:

```bash
git add -u
git commit -m "fix: address clippy warnings and test issues"
```
