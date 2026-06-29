# Navigation Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Change default navigation to text-editor scroll (cursor centered) and offer e-reader page-turn mode as a setting.

**Architecture:** Add `NavigationMode` enum to config. Branch in `move_cursor` to either center the viewport on the cursor (Scroll mode) or use existing page-turn logic (EReader mode). Add "Navigation" as 5th row in settings overlay.

**Tech Stack:** Rust, GTK4, serde

---

### Task 1: Add NavigationMode enum and config field

**Files:**
- Modify: `src/config.rs:1-101`

- [ ] **Step 1: Add NavigationMode enum before the Config struct**

Add after the existing `use` statements (after line 3):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NavigationMode {
    Scroll,
    #[serde(rename = "ereader")]
    EReader,
}

impl Default for NavigationMode {
    fn default() -> Self {
        NavigationMode::Scroll
    }
}
```

- [ ] **Step 2: Add navigation_mode field to Config struct**

Add after the `text_margins` field (after line 15):

```rust
    #[serde(default)]
    pub navigation_mode: NavigationMode,
```

- [ ] **Step 3: Add navigation_mode to Config::default()**

Add after `text_margins: default_text_margins(),` (line 63):

```rust
            navigation_mode: NavigationMode::default(),
```

- [ ] **Step 4: Build to verify**

Run: `cargo build 2>&1`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat: add NavigationMode enum to config"
```

---

### Task 2: Add center_cursor and branch move_cursor by mode

**Files:**
- Modify: `src/input/navigation.rs:20-48,192-198`

- [ ] **Step 1: Add center_cursor helper**

Add after the `scroll_to_cursor` function (after line 276):

```rust
/// Scroll the viewport so the current line is vertically centered.
/// Near document edges, clamps so no blank space appears (scrolloff behavior).
fn center_cursor(state: &mut AppState) {
    let adj = state.scrolled_window.vadjustment();
    let max_scroll = adj.upper() - adj.page_size();
    if max_scroll <= 0.0 {
        return;
    }
    let line_y = scroll_value_for_line(state, state.current_line);
    let half_page = adj.page_size() / 2.0;
    let centered = (line_y - half_page).max(0.0).min(max_scroll);
    adj.set_value(centered);
}
```

- [ ] **Step 2: Branch move_cursor by navigation mode**

Replace the scrolling logic in `move_cursor` (lines 39-45):

Current code:
```rust
    if delta > 0 && needs_page_turn_down(state, new_line) {
        // Going down and line is at bottom edge — page turn with this line at top
        set_page(state, new_line);
    } else if delta < 0 {
        scroll_to_cursor(state);
    }
```

Replace with:
```rust
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => {
            center_cursor(state);
        }
        crate::config::NavigationMode::EReader => {
            if delta > 0 && needs_page_turn_down(state, new_line) {
                set_page(state, new_line);
            } else if delta < 0 {
                scroll_to_cursor(state);
            }
        }
    }
```

- [ ] **Step 3: Update restore_cursor to center in Scroll mode**

Replace `restore_cursor` (lines 193-198):

```rust
pub fn restore_cursor(state: &mut AppState) {
    update_highlight(state);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => {
            let adj = state.scrolled_window.vadjustment();
            let max_scroll = adj.upper() - adj.page_size();
            let line_y = scroll_value_for_line(state, state.current_line);
            let half_page = adj.page_size() / 2.0;
            let centered = (line_y - half_page).max(0.0).min(max_scroll.max(0.0));
            adj.set_value(centered);
        }
        crate::config::NavigationMode::EReader => {
            let new_top = state.current_line.saturating_sub(PAGE_OVERLAP);
            set_page_instant(state, new_top);
        }
    }
}
```

- [ ] **Step 4: Build to verify**

Run: `cargo build 2>&1`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add src/input/navigation.rs
git commit -m "feat: branch move_cursor and restore_cursor by navigation mode"
```

---

### Task 3: Add Navigation row to settings overlay

**Files:**
- Modify: `src/ui/settings_overlay.rs:1-229`
- Modify: `src/input/keymap.rs:219-227,232-260,297-319,658-691`

- [ ] **Step 1: Update NUM_SETTINGS and add navigation_mode to snapshot**

In `src/ui/settings_overlay.rs`, change line 6:

```rust
const NUM_SETTINGS: usize = 5;
```

Add `navigation_mode` field to `SettingsSnapshot` (after line 13):

```rust
    navigation_mode: crate::config::NavigationMode,
```

Add `navigation_mode` field to `SettingsOverlay` struct (after line 24, the `theme_index` field):

```rust
    navigation_mode: crate::config::NavigationMode,
```

- [ ] **Step 2: Add "Navigation" to the settings row names and constructor**

Change the names array (line 48):

```rust
        let names = ["Line Spacing", "Column Width", "Text Margins", "Theme", "Navigation"];
```

Update the constructor return value — add `navigation_mode` after `theme_index` (line 105):

```rust
            navigation_mode: crate::config::NavigationMode::default(),
```

Update the default snapshot to include navigation_mode (after line 101, inside the SettingsSnapshot literal):

```rust
                navigation_mode: crate::config::NavigationMode::default(),
```

- [ ] **Step 3: Update show() to accept and snapshot navigation_mode**

Change the `show` method signature and body (lines 109-120):

```rust
    pub fn show(&mut self, line_spacing: u32, column_width: u32, text_margins: u32, navigation_mode: crate::config::NavigationMode) {
        self.snapshot = SettingsSnapshot {
            line_spacing,
            column_width,
            text_margins,
            theme_index: self.theme_index,
            navigation_mode,
        };
        self.navigation_mode = navigation_mode;
        self.selected = 0;
        self.update_displayed_values(line_spacing, column_width, text_margins, navigation_mode);
        self.update_row_highlight();
        self.container.set_visible(true);
    }
```

- [ ] **Step 4: Update update_displayed_values to show navigation mode**

Change signature and add the 5th label (lines 203-210):

```rust
    pub fn update_displayed_values(&self, line_spacing: u32, column_width: u32, text_margins: u32, navigation_mode: crate::config::NavigationMode) {
        self.value_labels[0].set_label(&format!("\u{25C0} {}px \u{25B6}", line_spacing));
        self.value_labels[1].set_label(&format!("\u{25C0} {}px \u{25B6}", column_width));
        self.value_labels[2].set_label(&format!("\u{25C0} {}px \u{25B6}", text_margins));
        if let Some(theme) = self.themes.get(self.theme_index) {
            self.value_labels[3].set_label(&format!("\u{25C0} {} \u{25B6}", theme.display_name));
        }
        let nav_label = match navigation_mode {
            crate::config::NavigationMode::Scroll => "Scroll",
            crate::config::NavigationMode::EReader => "E-Reader",
        };
        self.value_labels[4].set_label(&format!("\u{25C0} {} \u{25B6}", nav_label));
    }
```

- [ ] **Step 5: Add Navigation case to adjust_value**

Add `navigation_mode` parameter to `adjust_value` signature and add case for index 4. Change the method (lines 144-184):

```rust
    pub fn adjust_value(
        &mut self,
        delta: i32,
        line_spacing: u32,
        column_width: u32,
        text_margins: u32,
        navigation_mode: crate::config::NavigationMode,
    ) -> SettingsChange {
        match self.selected {
            0 => {
                let new_val = (line_spacing as i32 + delta).clamp(0, 20) as u32;
                self.value_labels[0].set_label(&format!("\u{25C0} {}px \u{25B6}", new_val));
                SettingsChange::LineSpacing(new_val)
            }
            1 => {
                let new_val = (column_width as i32 + delta * 50).clamp(400, 1200) as u32;
                self.value_labels[1].set_label(&format!("\u{25C0} {}px \u{25B6}", new_val));
                SettingsChange::ColumnWidth(new_val)
            }
            2 => {
                let new_val = (text_margins as i32 + delta * 4).clamp(8, 96) as u32;
                self.value_labels[2].set_label(&format!("\u{25C0} {}px \u{25B6}", new_val));
                SettingsChange::TextMargins(new_val)
            }
            3 => {
                let len = self.themes.len();
                if len == 0 {
                    return SettingsChange::None;
                }
                let new_idx = (self.theme_index as i32 + delta).rem_euclid(len as i32) as usize;
                self.theme_index = new_idx;
                let theme = &self.themes[new_idx];
                self.value_labels[3].set_label(&format!("\u{25C0} {} \u{25B6}", theme.display_name));
                SettingsChange::Theme(Box::new(theme.clone()))
            }
            4 => {
                let new_mode = match navigation_mode {
                    crate::config::NavigationMode::Scroll => crate::config::NavigationMode::EReader,
                    crate::config::NavigationMode::EReader => crate::config::NavigationMode::Scroll,
                };
                self.navigation_mode = new_mode;
                let label = match new_mode {
                    crate::config::NavigationMode::Scroll => "Scroll",
                    crate::config::NavigationMode::EReader => "E-Reader",
                };
                self.value_labels[4].set_label(&format!("\u{25C0} {} \u{25B6}", label));
                SettingsChange::Navigation(new_mode)
            }
            _ => SettingsChange::None,
        }
    }
```

- [ ] **Step 6: Update snapshot() to return navigation_mode**

Change `snapshot` method (lines 186-193):

```rust
    pub fn snapshot(&self) -> (u32, u32, u32, usize, crate::config::NavigationMode) {
        (
            self.snapshot.line_spacing,
            self.snapshot.column_width,
            self.snapshot.text_margins,
            self.snapshot.theme_index,
            self.snapshot.navigation_mode,
        )
    }
```

- [ ] **Step 7: Add Navigation variant to SettingsChange enum**

Change the enum (lines 223-229):

```rust
pub enum SettingsChange {
    LineSpacing(u32),
    ColumnWidth(u32),
    TextMargins(u32),
    Theme(Box<Theme>),
    Navigation(crate::config::NavigationMode),
    None,
}
```

- [ ] **Step 8: Add public navigation_mode getter**

Add after the `themes()` method (after line 201):

```rust
    pub fn navigation_mode(&self) -> crate::config::NavigationMode {
        self.navigation_mode
    }
```

- [ ] **Step 9: Build to verify settings_overlay compiles**

Run: `cargo build 2>&1`
Expected: errors in `keymap.rs` because `show()`, `adjust_value()`, `snapshot()`, and `update_displayed_values()` signatures changed. That's expected — Task 4 fixes those.

- [ ] **Step 10: Commit settings_overlay changes**

```bash
git add src/ui/settings_overlay.rs
```

(Don't commit yet — Task 4 will fix keymap.rs and commit together.)

---

### Task 4: Update keymap.rs to wire up the new settings

**Files:**
- Modify: `src/input/keymap.rs:219-227,232-260,279-296,297-319,658-691`

- [ ] **Step 1: Update Ctrl+comma to pass navigation_mode to show()**

Change lines 219-227:

```rust
    if is_ctrl && key_name == "comma" && !settings_visible && !picker_visible {
        let s = state.borrow();
        let ls = s.config.line_spacing;
        let cw = s.config.column_width;
        let tm = s.config.text_margins;
        let nm = s.config.navigation_mode;
        drop(s);
        state.borrow_mut().settings_overlay.show(ls, cw, tm, nm);
        return true;
    }
```

- [ ] **Step 2: Update Escape revert to include navigation_mode**

Change the Escape handler (lines 232-260). The snapshot destructure changes:

```rust
            "Escape" => {
                let (snap_ls, snap_cw, snap_tm, snap_ti, snap_nm) = state.borrow().settings_overlay.snapshot();
                {
                    let mut s = state.borrow_mut();
                    if s.dialogue_formatting_active {
                        let tag_table = s.buffer.tag_table();
                        if let Some(tag) = tag_table.lookup("speaker-gap") {
                            tag.set_property("pixels-above-lines", snap_ls.max(1) as i32 * 5);
                        }
                    } else {
                        s.text_view.set_pixels_above_lines(snap_ls as i32);
                        s.text_view.set_pixels_below_lines(snap_ls as i32);
                    }
                    s.scrolled_window.set_width_request(snap_cw as i32);
                    s.text_view.set_left_margin(snap_tm as i32);
                    s.text_view.set_right_margin(snap_tm as i32);
                    s.config.line_spacing = snap_ls;
                    s.config.column_width = snap_cw;
                    s.config.text_margins = snap_tm;
                    s.config.navigation_mode = snap_nm;
                    if let Some(snap_theme) = s.settings_overlay.themes().get(snap_ti) {
                        let snap_theme = snap_theme.clone();
                        s.settings_overlay.set_theme_index(snap_ti);
                        apply_theme_to_state(&mut s, &snap_theme);
                    }
                    s.settings_overlay.hide();
                }
                return true;
            }
```

- [ ] **Step 3: Update h/l adjust_value calls to pass navigation_mode**

Change the "h" | "Left" handler (lines 279-287):

```rust
            "h" | "Left" => {
                let (ls, cw, tm, nm) = {
                    let s = state.borrow();
                    (s.config.line_spacing, s.config.column_width, s.config.text_margins, s.config.navigation_mode)
                };
                let change = state.borrow_mut().settings_overlay.adjust_value(-1, ls, cw, tm, nm);
                apply_settings_change(state, change);
                return true;
            }
```

Change the "l" | "Right" handler (lines 288-296):

```rust
            "l" | "Right" => {
                let (ls, cw, tm, nm) = {
                    let s = state.borrow();
                    (s.config.line_spacing, s.config.column_width, s.config.text_margins, s.config.navigation_mode)
                };
                let change = state.borrow_mut().settings_overlay.adjust_value(1, ls, cw, tm, nm);
                apply_settings_change(state, change);
                return true;
            }
```

- [ ] **Step 4: Update "r" reset handler to include navigation_mode**

Change the "r" handler (lines 297-319). Add after `s.config.text_margins = tm;`:

```rust
                s.config.navigation_mode = crate::config::NavigationMode::default();
                s.settings_overlay.update_displayed_values(ls, cw, tm, crate::config::NavigationMode::default());
```

(Replace the existing `s.settings_overlay.update_displayed_values(ls, cw, tm);` call.)

- [ ] **Step 5: Handle Navigation variant in apply_settings_change**

Add the Navigation arm to the match in `apply_settings_change` (around line 688, before `SettingsChange::None`):

```rust
        SettingsChange::Navigation(mode) => {
            s.config.navigation_mode = mode;
        }
```

- [ ] **Step 6: Build to verify everything compiles**

Run: `cargo build 2>&1`
Expected: compiles with no errors

- [ ] **Step 7: Commit all remaining changes**

```bash
git add src/ui/settings_overlay.rs src/input/keymap.rs
git commit -m "feat: add Navigation setting to settings overlay"
```

---

### Task 5: Run clippy and final verification

**Files:**
- None (verification only)

- [ ] **Step 1: Run clippy**

Run: `cargo clippy 2>&1`
Expected: no errors, no new warnings related to navigation mode

- [ ] **Step 2: Run tests**

Run: `cargo test 2>&1`
Expected: all tests pass

- [ ] **Step 3: Build release**

Run: `cargo build 2>&1`
Expected: compiles clean
