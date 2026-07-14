# Visual Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add vim-style linewise visual selection mode with copy, merge, and external command pipe actions.

**Architecture:** Visual mode is a state machine layered into the existing key routing chain. A `SelectionState` in AppState tracks anchor/cursor. Actions operate on both lit.db and text files. An undo stack captures pre-action state for reversal.

**Tech Stack:** Rust, GTK4, sourceview5, rusqlite

---

### Task 1: Config — Add visual_mode_commands

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add VisualModeCommand struct and config field**

In `src/config.rs`, add the struct and field:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualModeCommand {
    pub name: String,
    pub command: String,
}
```

Add to the `Config` struct after `last_line`:

```rust
    #[serde(default)]
    pub visual_mode_commands: Vec<VisualModeCommand>,
```

Add to `Default for Config` impl:

```rust
    visual_mode_commands: Vec::new(),
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: success (serde defaults handle missing field in existing configs)

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "feat: add visual_mode_commands to config"
```

---

### Task 2: SelectionState and AppState fields

**Files:**
- Create: `src/input/visual.rs`
- Modify: `src/input/mod.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Create src/input/visual.rs with SelectionState**

```rust
use crate::db::models::Line;

/// Tracks the visual selection range (anchor..cursor).
pub struct SelectionState {
    pub anchor_line: usize,
    pub cursor_line: usize,
}

impl SelectionState {
    pub fn new(line: usize) -> Self {
        Self {
            anchor_line: line,
            cursor_line: line,
        }
    }

    /// Returns (start, end) as an inclusive range, regardless of direction.
    pub fn range(&self) -> (usize, usize) {
        let start = self.anchor_line.min(self.cursor_line);
        let end = self.anchor_line.max(self.cursor_line);
        (start, end)
    }
}

/// A snapshot of state before a destructive action, for undo.
pub struct UndoEntry {
    pub db_lines: Vec<Line>,
    pub file_backup: Option<(String, String)>,
    pub cursor_line: usize,
}
```

- [ ] **Step 2: Register module in src/input/mod.rs**

Add to `src/input/mod.rs`:

```rust
pub mod visual;
```

- [ ] **Step 3: Add visual mode fields to AppState**

In `src/app.rs`, add to AppState struct (after `pending_advance`):

```rust
    pub visual_selection: Option<crate::input::visual::SelectionState>,
    pub undo_stack: Vec<crate::input::visual::UndoEntry>,
    pub selection_tag: gtk4::TextTag,
```

- [ ] **Step 4: Create selection_tag in build_window**

In `src/app.rs` `build_window()`, after the `translation_text_tag` block (after line 166), add:

```rust
    let selection_tag = gtk4::TextTag::builder()
        .name("visual-selection")
        .background(if theme.is_light {
            "rgba(38, 109, 211, 0.15)"
        } else {
            "rgba(68, 138, 255, 0.25)"
        })
        .build();
    buffer.tag_table().add(&selection_tag);
```

- [ ] **Step 5: Initialize new fields in AppState constructor**

In the `AppState { ... }` initializer (around line 250-293), add:

```rust
        visual_selection: None,
        undo_stack: Vec::new(),
        selection_tag,
```

- [ ] **Step 6: Update selection_tag in apply_theme_to_state**

In `src/input/keymap.rs` `apply_theme_to_state()` (around line 715), after the translation_dim_tag update, add:

```rust
    state.selection_tag.set_property(
        "background",
        if theme.is_light {
            "rgba(38, 109, 211, 0.15)"
        } else {
            "rgba(68, 138, 255, 0.25)"
        },
    );
```

- [ ] **Step 7: Clear visual state on work switch**

In `src/app.rs` `display_work()` (around line 459, near `state.current_line = 0`), add:

```rust
    state.visual_selection = None;
    state.undo_stack.clear();
```

- [ ] **Step 8: Verify it compiles**

Run: `cargo build`
Expected: success

- [ ] **Step 9: Commit**

```bash
git add src/input/visual.rs src/input/mod.rs src/app.rs src/input/keymap.rs
git commit -m "feat: add SelectionState, UndoEntry, and selection_tag to AppState"
```

---

### Task 3: Visual selection highlighting

**Files:**
- Modify: `src/input/visual.rs`
- Modify: `src/input/navigation.rs`

- [ ] **Step 1: Add selection highlight functions to visual.rs**

Append to `src/input/visual.rs`:

```rust
use gtk4::prelude::*;
use crate::app::AppState;

/// Apply the selection_tag to all lines in the visual selection range.
/// Also removes dim_tag from those lines so they appear at full brightness.
pub fn apply_selection_highlight(state: &AppState) {
    let selection = match &state.visual_selection {
        Some(s) => s,
        None => return,
    };
    let (start, end) = selection.range();
    let buffer = &state.buffer;

    for line_idx in start..=end {
        if let Some(line_start) = buffer.iter_at_line(line_idx as i32) {
            let mut line_end = line_start;
            if !line_end.ends_line() {
                line_end.forward_to_line_end();
            }
            buffer.remove_tag(&state.dim_tag, &line_start, &line_end);
            buffer.apply_tag(&state.selection_tag, &line_start, &line_end);
        }
    }
}

/// Remove the selection_tag from the entire buffer.
pub fn clear_selection_highlight(state: &AppState) {
    let (buf_start, buf_end) = state.buffer.bounds();
    state.buffer.remove_tag(&state.selection_tag, &buf_start, &buf_end);
}
```

- [ ] **Step 2: Integrate selection highlight into update_highlight**

In `src/input/navigation.rs`, at the end of the `update_highlight()` function (after the chunk undim block ending around line 495), add:

```rust
    // When visual selection is active, undim and highlight selected lines
    crate::input::visual::apply_selection_highlight(state);
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add src/input/visual.rs src/input/navigation.rs
git commit -m "feat: add visual selection highlighting integrated with update_highlight"
```

---

### Task 4: Enter/exit visual mode and extend selection (keymap)

**Files:**
- Modify: `src/input/keymap.rs`
- Modify: `src/input/visual.rs`

- [ ] **Step 1: Add visual mode movement function to visual.rs**

Append to `src/input/visual.rs`:

```rust
/// Move the visual selection cursor by delta lines.
/// Does NOT skip translation lines (visual mode selects all visible lines).
pub fn move_selection_cursor(state: &mut AppState, delta: i32) {
    let selection = match &mut state.visual_selection {
        Some(s) => s,
        None => return,
    };
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }
    let new_cursor = (selection.cursor_line as i32 + delta)
        .max(0)
        .min(line_count as i32 - 1) as usize;
    selection.cursor_line = new_cursor;
    state.current_line = new_cursor;

    crate::input::navigation::update_highlight_and_ensure_visible(state);
}

/// Enter visual mode: set anchor at current line.
pub fn enter_visual_mode(state: &mut AppState) {
    state.visual_selection = Some(SelectionState::new(state.current_line));
    crate::input::navigation::update_highlight_and_ensure_visible(state);
    crate::logging::log(&format!("VISUAL: entered at line {}", state.current_line));
}

/// Exit visual mode: clear selection and highlighting.
pub fn exit_visual_mode(state: &mut AppState) {
    if state.visual_selection.is_some() {
        state.visual_selection = None;
        clear_selection_highlight(state);
        crate::input::navigation::update_highlight_and_ensure_visible(state);
        crate::logging::log("VISUAL: exited");
    }
}

/// Extend visual selection to the first line (gg equivalent).
pub fn extend_to_start(state: &mut AppState) {
    if let Some(ref mut sel) = state.visual_selection {
        sel.cursor_line = 0;
        state.current_line = 0;
        crate::input::navigation::update_highlight_and_ensure_visible(state);
    }
}

/// Extend visual selection to the last line (G equivalent).
pub fn extend_to_end(state: &mut AppState) {
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }
    if let Some(ref mut sel) = state.visual_selection {
        sel.cursor_line = line_count - 1;
        state.current_line = line_count - 1;
        crate::input::navigation::update_highlight_and_ensure_visible(state);
    }
}
```

- [ ] **Step 2: Add visual mode key routing to keymap.rs**

In `src/input/keymap.rs`, insert a new section **after** the search bar block (after line 348, `}` closing `if search_visible`) and **before** the `// --- Normal mode (no picker) ---` comment (line 350):

```rust
    // --- Action popup (when visible) ---
    let action_popup_visible = state.borrow().action_popup.is_some();
    if action_popup_visible {
        // Will be implemented in Task 6
        return true;
    }

    // --- Visual mode ---
    let in_visual = state.borrow().visual_selection.is_some();
    if in_visual {
        match key_name {
            "j" => {
                crate::input::visual::move_selection_cursor(&mut state.borrow_mut(), 1);
                return true;
            }
            "k" => {
                crate::input::visual::move_selection_cursor(&mut state.borrow_mut(), -1);
                return true;
            }
            "G" => {
                crate::input::visual::extend_to_end(&mut state.borrow_mut());
                return true;
            }
            "g" => {
                // In visual mode, 'g' starts gg sequence to extend to start
                key_state.borrow_mut().pending_g = true;
                let ks = Rc::clone(key_state);
                glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                    ks.borrow_mut().pending_g = false;
                });
                return true;
            }
            "Escape" | "V" => {
                crate::input::visual::exit_visual_mode(&mut state.borrow_mut());
                return true;
            }
            "Return" => {
                // Open action popup — implemented in Task 6
                return true;
            }
            _ => {
                // Consume all other keys in visual mode
                return true;
            }
        }
    }
```

- [ ] **Step 3: Handle gg in visual mode**

In the existing gg sequence check block (around line 353), modify it to handle visual mode:

Replace:
```rust
    // gg sequence check
    if key_state.borrow().pending_g {
        key_state.borrow_mut().pending_g = false;
        if key_name == "g" {
            navigation::jump_to_start(&mut state.borrow_mut());
            return true;
        }
    }
```

With:
```rust
    // gg sequence check
    if key_state.borrow().pending_g {
        key_state.borrow_mut().pending_g = false;
        if key_name == "g" {
            if state.borrow().visual_selection.is_some() {
                crate::input::visual::extend_to_start(&mut state.borrow_mut());
            } else {
                navigation::jump_to_start(&mut state.borrow_mut());
            }
            return true;
        }
    }
```

- [ ] **Step 4: Add V key to normal mode to enter visual mode**

In the normal mode single keys `match` block (around line 407), add before the closing `_ => false`:

```rust
        "V" => {
            crate::input::visual::enter_visual_mode(&mut state.borrow_mut());
            true
        }
```

- [ ] **Step 5: Add action_popup field to AppState**

In `src/app.rs`, add to AppState struct (after `visual_selection`):

```rust
    pub action_popup: Option<crate::input::visual::ActionPopupState>,
```

Add `ActionPopupState` to `src/input/visual.rs`:

```rust
/// Tracks which action is highlighted in the popup menu.
pub struct ActionPopupState {
    pub selected_index: usize,
}
```

Initialize in AppState constructor:

```rust
        action_popup: None,
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo build`
Expected: success

- [ ] **Step 7: Commit**

```bash
git add src/input/keymap.rs src/input/visual.rs src/app.rs
git commit -m "feat: add visual mode enter/exit, j/k/G/gg selection extension"
```

---

### Task 5: Action popup GTK overlay

**Files:**
- Create: `src/ui/action_popup.rs`
- Modify: `src/ui/mod.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Check current ui module structure**

Read `src/ui/mod.rs` to see existing modules.

- [ ] **Step 2: Create src/ui/action_popup.rs**

```rust
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};

pub struct ActionPopup {
    pub container: GtkBox,
    rows: Vec<GtkBox>,
    selected: usize,
}

impl ActionPopup {
    pub fn new() -> Self {
        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(350)
            .build();
        container.add_css_class("action-popup");
        container.set_visible(false);

        // Title
        let title = Label::builder()
            .label("Action")
            .css_classes(vec!["settings-title"])
            .build();
        container.append(&title);

        ActionPopup {
            container,
            rows: Vec::new(),
            selected: 0,
        }
    }

    /// Populate the popup with action names. Built-in actions come first,
    /// then a separator, then external commands.
    pub fn show_actions(&mut self, builtin: &[&str], external: &[(String, String)]) {
        // Clear existing rows
        while let Some(child) = self.container.last_child() {
            // Keep the title (first child)
            if self.container.first_child().as_ref() == Some(&child) {
                break;
            }
            self.container.remove(&child);
        }
        self.rows.clear();

        for name in builtin {
            let row = self.make_row(name);
            self.container.append(&row);
            self.rows.push(row);
        }

        if !external.is_empty() {
            let sep = Label::builder()
                .label("───────────────")
                .css_classes(vec!["action-separator"])
                .build();
            self.container.append(&sep);

            for (name, _cmd) in external {
                let row = self.make_row(name);
                self.container.append(&row);
                self.rows.push(row);
            }
        }

        // Footer
        let footer = Label::builder()
            .label("Ctrl+n/p navigate · Enter confirm · Esc cancel")
            .css_classes(vec!["settings-footer"])
            .build();
        self.container.append(&footer);

        self.selected = 0;
        self.update_row_highlight();
        self.container.set_visible(true);
    }

    pub fn hide(&mut self) {
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    pub fn move_selection(&mut self, delta: i32) {
        if self.rows.is_empty() {
            return;
        }
        let new = (self.selected as i32 + delta).rem_euclid(self.rows.len() as i32) as usize;
        self.selected = new;
        self.update_row_highlight();
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    fn make_row(&self, label: &str) -> GtkBox {
        let row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(0)
            .css_classes(vec!["settings-row"])
            .build();
        let name_label = Label::builder()
            .label(label)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();
        row.append(&name_label);
        row
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
```

- [ ] **Step 3: Register module in src/ui/mod.rs**

Add to `src/ui/mod.rs`:

```rust
pub mod action_popup;
```

- [ ] **Step 4: Add ActionPopup to AppState and wire into overlay stack**

In `src/app.rs`, add to AppState struct:

```rust
    pub action_popup_widget: crate::ui::action_popup::ActionPopup,
```

In `build_window()`, after the settings overlay attach block (around line 237) and before the search bar setup (line 240), create and attach the action popup:

```rust
    // Action popup overlay for visual mode
    let mut action_popup_widget = crate::ui::action_popup::ActionPopup::new();
    settings_overlay.overlay.add_overlay(&action_popup_widget.container);
```

Initialize in AppState constructor:

```rust
        action_popup_widget,
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: success

- [ ] **Step 6: Commit**

```bash
git add src/ui/action_popup.rs src/ui/mod.rs src/app.rs
git commit -m "feat: add ActionPopup GTK overlay widget"
```

---

### Task 6: Action popup key routing and dispatch

**Files:**
- Modify: `src/input/keymap.rs`
- Modify: `src/input/visual.rs`

- [ ] **Step 1: Add action list builder to visual.rs**

Append to `src/input/visual.rs`:

```rust
/// Built-in action names, in display order.
pub const BUILTIN_ACTIONS: &[&str] = &["Copy", "Copy with metadata", "Merge lines"];

/// Determine which built-in actions are available for the current work.
/// Copy actions are always available. Merge requires either a text_file or DB lines.
pub fn available_builtin_actions(state: &AppState) -> Vec<&'static str> {
    // All built-in actions are always available
    BUILTIN_ACTIONS.to_vec()
}

/// Open the action popup menu.
pub fn open_action_popup(state: &mut AppState) {
    let builtins = available_builtin_actions(state);
    let externals: Vec<(String, String)> = state
        .config
        .visual_mode_commands
        .iter()
        .map(|c| (c.name.clone(), c.command.clone()))
        .collect();
    state.action_popup_widget.show_actions(
        &builtins.iter().map(|s| *s).collect::<Vec<_>>(),
        &externals,
    );
    state.action_popup = Some(ActionPopupState { selected_index: 0 });
    crate::logging::log("VISUAL: action popup opened");
}

/// Close the action popup without executing.
pub fn close_action_popup(state: &mut AppState) {
    state.action_popup = None;
    state.action_popup_widget.hide();
    crate::logging::log("VISUAL: action popup closed");
}
```

- [ ] **Step 2: Implement action popup key routing in keymap.rs**

Replace the placeholder action popup block (from Task 4 Step 2) with:

```rust
    // --- Action popup (when visible) ---
    let action_popup_visible = state.borrow().action_popup.is_some();
    if action_popup_visible {
        match key_name {
            "n" if is_ctrl => {
                let mut s = state.borrow_mut();
                s.action_popup_widget.move_selection(1);
                let idx = s.action_popup_widget.selected_index();
                if let Some(ref mut popup) = s.action_popup {
                    popup.selected_index = idx;
                }
                return true;
            }
            "p" if is_ctrl => {
                let mut s = state.borrow_mut();
                s.action_popup_widget.move_selection(-1);
                let idx = s.action_popup_widget.selected_index();
                if let Some(ref mut popup) = s.action_popup {
                    popup.selected_index = idx;
                }
                return true;
            }
            "Return" => {
                let selected_idx = state.borrow().action_popup_widget.selected_index();
                crate::input::visual::close_action_popup(&mut state.borrow_mut());
                crate::input::visual::execute_action(&mut state.borrow_mut(), selected_idx, tokio_handle);
                return true;
            }
            "Escape" => {
                crate::input::visual::close_action_popup(&mut state.borrow_mut());
                return true;
            }
            _ => return true, // consume all keys when popup visible
        }
    }
```

- [ ] **Step 3: Add execute_action stub to visual.rs**

Append to `src/input/visual.rs`:

```rust
/// Execute the action at the given index.
/// Indices 0..N are built-in actions, N.. are external commands.
pub fn execute_action(state: &mut AppState, index: usize, _tokio_handle: &tokio::runtime::Handle) {
    let builtin_count = available_builtin_actions(state).len();

    if index < builtin_count {
        match index {
            0 => action_copy(state, false),
            1 => action_copy(state, true),
            2 => action_merge(state),
            _ => {}
        }
    } else {
        let ext_index = index - builtin_count;
        let command = state.config.visual_mode_commands.get(ext_index).map(|c| c.command.clone());
        if let Some(cmd) = command {
            action_external_command(state, &cmd);
        }
    }

    // Exit visual mode after action
    exit_visual_mode(state);
}

fn action_copy(_state: &mut AppState, _with_metadata: bool) {
    // Implemented in Task 7
}

fn action_merge(_state: &mut AppState) {
    // Implemented in Task 8
}

fn action_external_command(_state: &mut AppState, _command: &str) {
    // Implemented in Task 9
}
```

- [ ] **Step 4: Wire Enter key in visual mode to open popup**

In the visual mode key routing block (Task 4 Step 2), replace the `"Return"` arm:

```rust
            "Return" => {
                crate::input::visual::open_action_popup(&mut state.borrow_mut());
                return true;
            }
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: success

- [ ] **Step 6: Commit**

```bash
git add src/input/keymap.rs src/input/visual.rs
git commit -m "feat: add action popup key routing and dispatch skeleton"
```

---

### Task 7: Copy and Copy with metadata actions

**Files:**
- Modify: `src/input/visual.rs`

- [ ] **Step 1: Implement action_copy**

Replace the `action_copy` stub in `src/input/visual.rs`:

```rust
fn action_copy(state: &mut AppState, with_metadata: bool) {
    let selection = match &state.visual_selection {
        Some(s) => s,
        None => return,
    };
    let (start, end) = selection.range();
    let buffer = &state.buffer;

    let mut lines_text = Vec::new();
    for line_idx in start..=end {
        if let Some(line_start) = buffer.iter_at_line(line_idx as i32) {
            let mut line_end = line_start;
            if !line_end.ends_line() {
                line_end.forward_to_line_end();
            }
            let text = buffer.text(&line_start, &line_end, false).to_string();

            if with_metadata {
                let meta = format_line_metadata(state, line_idx, &text);
                lines_text.push(meta);
            } else {
                lines_text.push(text);
            }
        }
    }

    let output = lines_text.join("\n");
    // Pipe to wl-copy
    use std::process::{Command, Stdio};
    use std::io::Write;
    match Command::new("wl-copy").stdin(Stdio::piped()).spawn() {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(output.as_bytes());
            }
            let _ = child.wait();
            crate::logging::log(&format!("VISUAL: copied {} lines to clipboard", end - start + 1));
        }
        Err(e) => {
            crate::logging::log(&format!("VISUAL: wl-copy failed: {}", e));
        }
    }
}

/// Format a line with metadata: [line_num] SPEAKER (start-end): text
fn format_line_metadata(state: &AppState, buffer_line: usize, text: &str) -> String {
    let work = match &state.current_work {
        Some(w) => w,
        None => return format!("[{}] {}", buffer_line + 1, text),
    };

    let work_idx = state.work_line_for_buffer(buffer_line);
    let line = work_idx.and_then(|i| work.lines.get(i));

    match line {
        Some(line) => {
            let mut parts = Vec::new();
            parts.push(format!("[{}]", buffer_line + 1));
            if let Some(ref speaker) = line.speaker {
                parts.push(speaker.clone());
            }
            if let Some(ref ts) = line.timestamp {
                parts.push(format!("({:.1}-{:.1})", ts.start, ts.end));
            }
            parts.push(format!(":{}", text));
            parts.join(" ")
        }
        None => format!("[{}] {}", buffer_line + 1, text),
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add src/input/visual.rs
git commit -m "feat: implement copy and copy-with-metadata visual mode actions"
```

---

### Task 8: DB write queries for merge and replace

**Files:**
- Modify: `src/db/queries.rs`

- [ ] **Step 1: Add merge_lines query**

Append to `src/db/queries.rs` (before the `#[cfg(test)]` block):

```rust
/// Merge multiple lines into one. Updates the first line's text and deletes the rest.
/// Returns the IDs of deleted lines.
pub fn merge_lines(
    conn: &Connection,
    first_line_id: i64,
    merged_text: &str,
    delete_ids: &[i64],
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE line_mapping SET canonical_text = ?2, normalized_text = ?2 WHERE id = ?1",
        rusqlite::params![first_line_id, merged_text],
    )?;
    for &id in delete_ids {
        conn.execute("DELETE FROM line_mapping WHERE id = ?1", [id])?;
    }
    Ok(())
}

/// Replace a set of lines with new text lines. Updates the first line,
/// deletes excess old lines, or inserts new lines if output has more.
/// `old_ids`: IDs of original lines (ordered).
/// `new_texts`: replacement texts (ordered).
pub fn replace_lines(
    conn: &Connection,
    work_abbrev: &str,
    old_ids: &[i64],
    new_texts: &[String],
) -> Result<(), rusqlite::Error> {
    if old_ids.is_empty() || new_texts.is_empty() {
        return Ok(());
    }

    // Update existing lines where we have both old and new
    let update_count = old_ids.len().min(new_texts.len());
    for i in 0..update_count {
        conn.execute(
            "UPDATE line_mapping SET canonical_text = ?2, normalized_text = ?2 WHERE id = ?1",
            rusqlite::params![old_ids[i], new_texts[i]],
        )?;
    }

    // Delete excess old lines
    for &id in &old_ids[update_count..] {
        conn.execute("DELETE FROM line_mapping WHERE id = ?1", [id])?;
    }

    // Insert new lines if output has more than old
    if new_texts.len() > old_ids.len() {
        // Get div info from the first old line to use for new inserts
        let (div1, div2, base_line_in_div): (i64, i64, i64) = conn.query_row(
            "SELECT div1, div2, line_in_div FROM line_mapping WHERE id = ?1",
            [old_ids[0]],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

        for (i, text) in new_texts[old_ids.len()..].iter().enumerate() {
            let new_line_in_div = base_line_in_div + (old_ids.len() + i) as i64;
            conn.execute(
                "INSERT INTO line_mapping (work_abbrev, canonical_text, normalized_text, div1, div2, line_in_div) \
                 VALUES (?1, ?2, ?2, ?3, ?4, ?5)",
                rusqlite::params![work_abbrev, text, div1, div2, new_line_in_div],
            )?;
        }
    }

    Ok(())
}

/// Restore lines from an undo entry. Deletes any lines that replaced them,
/// then re-inserts the originals.
pub fn restore_lines(
    conn: &Connection,
    work_abbrev: &str,
    original_lines: &[crate::db::models::Line],
) -> Result<(), rusqlite::Error> {
    if original_lines.is_empty() {
        return Ok(());
    }

    // Get current line IDs in the same div range to clean up
    let first = &original_lines[0];
    let last = &original_lines[original_lines.len() - 1];
    let mut stmt = conn.prepare(
        "SELECT id FROM line_mapping WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 \
         AND line_in_div >= ?4 AND line_in_div <= ?5",
    )?;
    let current_ids: Vec<i64> = stmt
        .query_map(
            rusqlite::params![work_abbrev, first.div1, first.div2, first.line_in_div, last.line_in_div],
            |row| row.get(0),
        )?
        .collect::<Result<_, _>>()?;

    // Delete current lines in range
    for id in &current_ids {
        conn.execute("DELETE FROM line_mapping WHERE id = ?1", [id])?;
    }

    // Re-insert originals with their original IDs
    for line in original_lines {
        conn.execute(
            "INSERT INTO line_mapping (id, work_abbrev, canonical_text, normalized_text, speaker, div1, div2, line_in_div) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                line.id,
                work_abbrev,
                line.text,
                line.normalized,
                line.speaker,
                line.div1,
                line.div2,
                line.line_in_div,
            ],
        )?;
    }

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat: add DB write queries for merge, replace, and restore lines"
```

---

### Task 9: Merge lines action

**Files:**
- Modify: `src/input/visual.rs`

- [ ] **Step 1: Implement action_merge**

Replace the `action_merge` stub in `src/input/visual.rs`:

```rust
fn action_merge(state: &mut AppState) {
    let selection = match &state.visual_selection {
        Some(s) => s,
        None => return,
    };
    let (start, end) = selection.range();
    if start == end {
        crate::logging::log("VISUAL: merge requires multiple lines");
        return;
    }

    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    // Collect the DB lines for the selection range
    let mut db_lines: Vec<crate::db::models::Line> = Vec::new();
    for buf_line in start..=end {
        if let Some(work_idx) = state.work_line_for_buffer(buf_line) {
            if let Some(line) = work.lines.get(work_idx) {
                db_lines.push(line.clone());
            }
        }
    }
    if db_lines.len() < 2 {
        return;
    }

    // Build merged text
    let merged_text: String = db_lines
        .iter()
        .map(|l| l.text.trim())
        .collect::<Vec<_>>()
        .join(" ");

    let first_id = db_lines[0].id;
    let delete_ids: Vec<i64> = db_lines[1..].iter().map(|l| l.id).collect();
    let abbrev = work.abbrev.clone();

    // Capture undo entry
    let file_backup = work.text_file.as_ref().and_then(|path| {
        std::fs::read_to_string(path).ok().map(|content| (path.clone(), content))
    });
    state.undo_stack.push(UndoEntry {
        db_lines: db_lines.clone(),
        file_backup,
        cursor_line: state.current_line,
    });

    // Write to DB
    match crate::db::queries::open_db_rw() {
        Ok(conn) => {
            if let Err(e) = crate::db::queries::merge_lines(&conn, first_id, &merged_text, &delete_ids) {
                crate::logging::log(&format!("VISUAL: merge DB error: {}", e));
                state.undo_stack.pop();
                return;
            }
        }
        Err(e) => {
            crate::logging::log(&format!("VISUAL: open_db_rw failed: {}", e));
            state.undo_stack.pop();
            return;
        }
    }

    // Update text file if it exists
    if let Some(ref path) = work.text_file.clone() {
        if let Ok(contents) = std::fs::read_to_string(path) {
            let mut file_lines: Vec<String> = contents.lines().map(|l| l.to_string()).collect();
            if end < file_lines.len() {
                // Replace the range with the merged line
                file_lines.splice(start..=end, std::iter::once(merged_text.clone()));
                let _ = std::fs::write(path, file_lines.join("\n"));
            }
        }
    }

    crate::logging::log(&format!("VISUAL: merged {} lines into 1", db_lines.len()));

    // Reload the work to refresh buffer
    reload_current_work(state);
}
```

- [ ] **Step 2: Add reload_current_work helper**

Append to `src/input/visual.rs`:

```rust
/// Reload the current work from DB and refresh the display.
fn reload_current_work(state: &mut AppState) {
    let abbrev = match &state.current_work {
        Some(w) => w.abbrev.clone(),
        None => return,
    };
    let saved_line = state.current_line;

    match crate::db::queries::open_db() {
        Ok(conn) => {
            match crate::db::queries::load_work(&conn, &abbrev) {
                Ok(work) => {
                    let line_count_before = state.effective_line_count();
                    crate::app::display_work(state, work);
                    let new_count = state.effective_line_count();
                    // Restore cursor to a valid position
                    state.current_line = saved_line.min(new_count.saturating_sub(1));
                    crate::input::navigation::update_highlight_and_ensure_visible(state);
                }
                Err(e) => crate::logging::log(&format!("VISUAL: reload work failed: {}", e)),
            }
        }
        Err(e) => crate::logging::log(&format!("VISUAL: open_db failed: {}", e)),
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add src/input/visual.rs
git commit -m "feat: implement merge lines action with DB write and file update"
```

---

### Task 10: External command pipe action

**Files:**
- Modify: `src/input/visual.rs`

- [ ] **Step 1: Implement action_external_command**

Replace the `action_external_command` stub in `src/input/visual.rs`:

```rust
fn action_external_command(state: &mut AppState, command: &str) {
    let selection = match &state.visual_selection {
        Some(s) => s,
        None => return,
    };
    let (start, end) = selection.range();

    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    // Collect buffer text for selection
    let mut selected_text = Vec::new();
    for buf_line in start..=end {
        if let Some(line_start) = state.buffer.iter_at_line(buf_line as i32) {
            let mut line_end = line_start;
            if !line_end.ends_line() {
                line_end.forward_to_line_end();
            }
            selected_text.push(state.buffer.text(&line_start, &line_end, false).to_string());
        }
    }
    let input = selected_text.join("\n");

    // Collect DB lines for undo
    let mut db_lines: Vec<crate::db::models::Line> = Vec::new();
    for buf_line in start..=end {
        if let Some(work_idx) = state.work_line_for_buffer(buf_line) {
            if let Some(line) = work.lines.get(work_idx) {
                db_lines.push(line.clone());
            }
        }
    }

    // Run external command
    use std::process::{Command, Stdio};
    use std::io::Write;
    let result = Command::new("sh")
        .args(["-c", command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(input.as_bytes())?;
            }
            // Drop stdin to signal EOF
            drop(child.stdin.take());
            child.wait_with_output()
        });

    let output = match result {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                crate::logging::log(&format!(
                    "VISUAL: command '{}' failed ({}): {}",
                    command, output.status, stderr
                ));
                return; // Don't modify anything on failure
            }
            String::from_utf8_lossy(&output.stdout).to_string()
        }
        Err(e) => {
            crate::logging::log(&format!("VISUAL: command '{}' spawn failed: {}", command, e));
            return;
        }
    };

    let new_lines: Vec<String> = output.lines().map(|l| l.to_string()).collect();
    if new_lines.is_empty() {
        crate::logging::log("VISUAL: command returned empty output, skipping");
        return;
    }

    let abbrev = work.abbrev.clone();
    let old_ids: Vec<i64> = db_lines.iter().map(|l| l.id).collect();

    // Capture undo entry
    let file_backup = work.text_file.as_ref().and_then(|path| {
        std::fs::read_to_string(path).ok().map(|content| (path.clone(), content))
    });
    state.undo_stack.push(UndoEntry {
        db_lines,
        file_backup,
        cursor_line: state.current_line,
    });

    // Write to DB
    match crate::db::queries::open_db_rw() {
        Ok(conn) => {
            if let Err(e) = crate::db::queries::replace_lines(&conn, &abbrev, &old_ids, &new_lines) {
                crate::logging::log(&format!("VISUAL: replace DB error: {}", e));
                state.undo_stack.pop();
                return;
            }
        }
        Err(e) => {
            crate::logging::log(&format!("VISUAL: open_db_rw failed: {}", e));
            state.undo_stack.pop();
            return;
        }
    }

    // Update text file if it exists
    if let Some(ref path) = work.text_file.clone() {
        if let Ok(contents) = std::fs::read_to_string(path) {
            let mut file_lines: Vec<String> = contents.lines().map(|l| l.to_string()).collect();
            if end < file_lines.len() {
                file_lines.splice(start..=end, new_lines.iter().cloned());
                let _ = std::fs::write(path, file_lines.join("\n"));
            }
        }
    }

    crate::logging::log(&format!(
        "VISUAL: command '{}' replaced {} lines with {} lines",
        command,
        old_ids.len(),
        new_lines.len(),
    ));

    reload_current_work(state);
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add src/input/visual.rs
git commit -m "feat: implement external command pipe action for visual mode"
```

---

### Task 11: Undo action

**Files:**
- Modify: `src/input/visual.rs`
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Add undo function to visual.rs**

Append to `src/input/visual.rs`:

```rust
/// Undo the last destructive visual mode action.
pub fn undo_last_action(state: &mut AppState) {
    let entry = match state.undo_stack.pop() {
        Some(e) => e,
        None => {
            crate::logging::log("VISUAL: nothing to undo");
            return;
        }
    };

    let abbrev = match &state.current_work {
        Some(w) => w.abbrev.clone(),
        None => return,
    };

    // Restore DB lines
    match crate::db::queries::open_db_rw() {
        Ok(conn) => {
            if let Err(e) = crate::db::queries::restore_lines(&conn, &abbrev, &entry.db_lines) {
                crate::logging::log(&format!("VISUAL: undo DB restore failed: {}", e));
                return;
            }
        }
        Err(e) => {
            crate::logging::log(&format!("VISUAL: undo open_db_rw failed: {}", e));
            return;
        }
    }

    // Restore file if backup exists
    if let Some((path, content)) = &entry.file_backup {
        if let Err(e) = std::fs::write(path, content) {
            crate::logging::log(&format!("VISUAL: undo file restore failed: {}", e));
        }
    }

    crate::logging::log("VISUAL: undo successful");

    // Reload and restore cursor
    let saved_cursor = entry.cursor_line;
    reload_current_work(state);
    let line_count = state.effective_line_count();
    state.current_line = saved_cursor.min(line_count.saturating_sub(1));
    crate::input::navigation::update_highlight_and_ensure_visible(state);
}
```

- [ ] **Step 2: Wire 'u' key to undo in normal mode**

In `src/input/keymap.rs`, the `"u"` key is currently bound to `set_start_time` (line 514). We need to change `u` to undo when there are undo entries, and keep the timestamp behavior otherwise.

Replace the `"u" | "Right"` arm (around line 514):

```rust
        "u" => {
            if !state.borrow().undo_stack.is_empty() {
                crate::input::visual::undo_last_action(&mut state.borrow_mut());
                true
            } else {
                crate::input::timestamps::set_start_time(&mut state.borrow_mut())
            }
        }
        "Right" => {
            crate::input::timestamps::set_start_time(&mut state.borrow_mut())
        }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add src/input/visual.rs src/input/keymap.rs
git commit -m "feat: implement undo for visual mode destructive actions"
```

---

### Task 12: Final integration and cleanup

**Files:**
- Modify: `src/input/visual.rs` (if needed)
- Modify: `src/input/keymap.rs` (if needed)

- [ ] **Step 1: Run clippy**

Run: `cargo clippy`
Expected: no new warnings from our changes

- [ ] **Step 2: Fix any clippy warnings**

Address any warnings in the new files.

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all existing tests pass

- [ ] **Step 4: Final build**

Run: `cargo build`
Expected: success

- [ ] **Step 5: Commit any fixes**

```bash
git add -A
git commit -m "chore: clippy fixes and cleanup for visual mode"
```
