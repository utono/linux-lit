# Co-Author Attribution Display Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Display collaborator-attributed lines in italics for co-authored Shakespeare works, on by default, with keybind toggle and attribution set picker.

**Architecture:** New `src/db/authorship.rs` module provides DB queries. `AppState` gains four fields to track authorship state. A new `authorship-italic` TextTag is applied per-line during work load, following the same pattern as `apply_dialogue_formatting`. A simple picker (reusing the generic picker infrastructure) lets users switch between scholarly attribution sets.

**Tech Stack:** Rust, GTK4 (TextTag, ListBox), rusqlite, pango

---

### Task 1: DB Module — AttributionSet Struct and Queries

**Files:**
- Create: `src/db/authorship.rs`
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Add module declaration**

In `src/db/mod.rs`, add the new module after the existing declarations:

```rust
pub mod authorship;
```

- [ ] **Step 2: Create `src/db/authorship.rs` with the struct and queries**

```rust
use rusqlite::Connection;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct AttributionSet {
    pub id: i64,
    pub work_abbrev: String,
    pub name: String,
    pub display_name: String,
    pub primary_author: String,
    pub secondary_author: String,
}

pub fn load_attribution_sets(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<AttributionSet>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, work_abbrev, name, display_name, primary_author, secondary_author
         FROM attribution_sets WHERE work_abbrev = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([work_abbrev], |row| {
        Ok(AttributionSet {
            id: row.get(0)?,
            work_abbrev: row.get(1)?,
            name: row.get(2)?,
            display_name: row.get(3)?,
            primary_author: row.get(4)?,
            secondary_author: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn load_secondary_line_ids(
    conn: &Connection,
    set_id: i64,
    work_abbrev: &str,
) -> Result<HashSet<i64>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT lm.id
         FROM line_authorship la
         JOIN line_mapping lm
           ON lm.work_abbrev = ?2
          AND la.citation = lm.work_abbrev || '.' || lm.div1 || '.' || lm.div2 || '.' || lm.line_in_div
         WHERE la.attribution_set_id = ?1",
    )?;
    let rows = stmt.query_map(rusqlite::params![set_id, work_abbrev], |row| {
        row.get::<_, i64>(0)
    })?;
    let mut ids = HashSet::new();
    for r in rows {
        ids.insert(r?);
    }
    Ok(ids)
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles with no errors related to `db::authorship`

- [ ] **Step 4: Commit**

```bash
git add src/db/authorship.rs src/db/mod.rs
git commit -m "feat(authorship): add attribution set queries and data structs"
```

---

### Task 2: Action Variants and Keybind Config

**Files:**
- Modify: `src/input/actions/mod.rs` (Action enum, around line 139)
- Modify: `src/input/keymap_config.rs` (display_bindings, around line 276)

- [ ] **Step 1: Add Action variants**

In `src/input/actions/mod.rs`, add two new variants in the Action enum. Place them after `ToggleTitleBar` and before `ShowFontInfo`, in the "Settings (in reader)" group:

```rust
    ToggleAuthorship,
    PickAttributionSet,
```

- [ ] **Step 2: Add keybinds in `display_bindings`**

In `src/input/keymap_config.rs`, add to the `display_bindings()` function's vec, after the `ToggleTitleBar` entry:

```rust
        (KeyCombo::ctrl("a"), Action::ToggleAuthorship),
        (KeyCombo::ctrl_shift("A"), Action::PickAttributionSet),
```

- [ ] **Step 3: Update test expectations**

In `src/input/keymap_config.rs`, in the `default_reader_bindings_contains_known_bindings` test, add:

```rust
        assert_eq!(m.get(&KeyCombo::ctrl("a")), Some(&Action::ToggleAuthorship));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("A")), Some(&Action::PickAttributionSet));
```

- [ ] **Step 4: Verify it compiles and tests pass**

Run: `cargo test`
Expected: all tests pass (there will be warnings about unhandled match arms in `dispatch_action` — that's fine, we'll add them in Task 5)

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/mod.rs src/input/keymap_config.rs
git commit -m "feat(authorship): add ToggleAuthorship and PickAttributionSet actions and keybinds"
```

---

### Task 3: AppState Fields and Tag Creation

**Files:**
- Modify: `src/app.rs` (AppState struct ~line 63, build_window ~line 466, AppState construction ~line 831)

- [ ] **Step 1: Add AppState fields**

In the `AppState` struct definition (around line 128, near `dialogue_formatting_active`), add:

```rust
    pub authorship_tag: gtk4::TextTag,
    pub authorship_line_ids: std::collections::HashSet<i64>,
    pub authorship_enabled: bool,
    pub authorship_sets: Vec<crate::db::authorship::AttributionSet>,
    pub active_attribution_set_id: Option<i64>,
```

- [ ] **Step 2: Create the authorship-italic TextTag in `build_window`**

After the existing tag creation block (after `word_bold_tag` creation, around line 543), add:

```rust
        let authorship_tag = gtk4::TextTag::builder()
            .name("authorship-italic")
            .style(pango::Style::Italic)
            .build();
        buffer.tag_table().add(&authorship_tag);
```

- [ ] **Step 3: Initialize fields in AppState construction**

In the `AppState { ... }` construction block (around line 968, before `input_mode`), add:

```rust
        authorship_tag,
        authorship_line_ids: std::collections::HashSet::new(),
        authorship_enabled: true,
        authorship_sets: Vec::new(),
        active_attribution_set_id: None,
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: compiles cleanly

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(authorship): add AppState fields and authorship-italic TextTag"
```

---

### Task 4: Apply Authorship Formatting on Work Load

**Files:**
- Modify: `src/app.rs` (new function + call site in display_work, around line 1697)

- [ ] **Step 1: Add `apply_authorship_formatting` function**

Place this function near `apply_dialogue_formatting` (around line 2284, after that function ends):

```rust
pub fn apply_authorship_formatting(state: &mut AppState) {
    let tag_table = state.buffer.tag_table();
    if let Some(old) = tag_table.lookup("authorship-italic") {
        let (start, end) = state.buffer.bounds();
        state.buffer.remove_tag(&old, &start, &end);
    }

    if !state.authorship_enabled || state.authorship_line_ids.is_empty() {
        return;
    }

    let line_count = state.buffer.line_count() as usize;
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return,
    };

    for buf_line in 0..line_count {
        let work_idx = if let Some(ref lm) = state.line_map {
            match lm.buffer_to_work.get(buf_line).and_then(|o| *o) {
                Some(wi) => wi,
                None => continue,
            }
        } else {
            buf_line
        };

        let line = match work.lines.get(work_idx) {
            Some(l) => l,
            None => continue,
        };

        if state.authorship_line_ids.contains(&line.id) {
            let line_start = match state.buffer.iter_at_line(buf_line as i32) {
                Some(it) => it,
                None => continue,
            };
            let line_end = if buf_line + 1 < line_count {
                match state.buffer.iter_at_line((buf_line + 1) as i32) {
                    Some(it) => it,
                    None => {
                        let (_, e) = state.buffer.bounds();
                        e
                    }
                }
            } else {
                let (_, e) = state.buffer.bounds();
                e
            };
            state.buffer.apply_tag(&state.authorship_tag, &line_start, &line_end);
        }
    }
}
```

- [ ] **Step 2: Load authorship data and call formatting in display_work**

In `display_work_at_with_prepared`, immediately after the `apply_dialogue_formatting(state)` call (line 1697) and its timing log (line 1698), add:

```rust
    // Load and apply authorship formatting
    let t_auth = std::time::Instant::now();
    if let Some(ref work) = state.current_work {
        if let Ok(conn) = crate::db::queries::open_db() {
            state.authorship_sets = crate::db::authorship::load_attribution_sets(&conn, &work.abbrev)
                .unwrap_or_default();
            if let Some(first) = state.authorship_sets.first() {
                state.active_attribution_set_id = Some(first.id);
                state.authorship_line_ids = crate::db::authorship::load_secondary_line_ids(
                    &conn, first.id, &work.abbrev,
                ).unwrap_or_default();
            } else {
                state.active_attribution_set_id = None;
                state.authorship_line_ids.clear();
            }
        }
    }
    apply_authorship_formatting(state);
    crate::logging::log(&format!("TIMING: apply_authorship_formatting {:.0}ms", t_auth.elapsed().as_millis()));
```

- [ ] **Step 3: Clear authorship state in clear_display**

Find `clear_display` (or the reset block at the start of `display_work_at_with_prepared` where per-work state is cleared). Add near the other state resets:

```rust
    state.authorship_line_ids.clear();
    state.authorship_sets.clear();
    state.active_attribution_set_id = None;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: compiles cleanly

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(authorship): apply italic formatting to collaborator lines on work load"
```

---

### Task 5: Toggle and Picker Action Dispatch

**Files:**
- Modify: `src/input/keymap.rs` (dispatch_action, around line 760)

- [ ] **Step 1: Add match arms in dispatch_action**

In the `dispatch_action` function's match block, add after the existing display-related actions (near `ToggleDim`, `ToggleTitleBar`, etc.):

```rust
        ToggleAuthorship => {
            let mut s = state.borrow_mut();
            if s.authorship_sets.is_empty() {
                s.chapter_toast.set_text("No authorship data for this work");
                s.chapter_toast.set_visible(true);
                let toast = s.chapter_toast.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                    toast.set_visible(false);
                });
                return;
            }
            s.authorship_enabled = !s.authorship_enabled;
            crate::app::apply_authorship_formatting(&mut s);
            let label = if s.authorship_enabled { "Authorship: on" } else { "Authorship: off" };
            s.chapter_toast.set_text(label);
            s.chapter_toast.set_visible(true);
            let toast = s.chapter_toast.clone();
            glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                toast.set_visible(false);
            });
        }
        PickAttributionSet => {
            let s = state.borrow();
            if s.authorship_sets.is_empty() {
                s.chapter_toast.set_text("No authorship data for this work");
                s.chapter_toast.set_visible(true);
                let toast = s.chapter_toast.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                    toast.set_visible(false);
                });
                return;
            }
            if s.authorship_sets.len() == 1 {
                s.chapter_toast.set_text("Only one attribution set available");
                s.chapter_toast.set_visible(true);
                let toast = s.chapter_toast.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                    toast.set_visible(false);
                });
                return;
            }
            drop(s);
            crate::input::actions::authorship::open_attribution_picker(state);
        }
```

- [ ] **Step 2: Note — do not compile yet**

This task depends on Task 6 (`src/input/actions/authorship.rs`). The commit for this file is deferred to Task 6 Step 9, which commits both files together.

---

### Task 6: Attribution Set Picker

**Files:**
- Create: `src/ui/authorship_picker.rs`
- Modify: `src/ui/mod.rs`
- Create: `src/input/actions/authorship.rs`
- Modify: `src/input/actions/mod.rs`
- Modify: `src/app.rs` (AppState struct, build_window, construction)
- Modify: `src/input/keymap.rs` (InputMode variant, handle_picker_key arms)

- [ ] **Step 1: Add InputMode variant**

In `src/app.rs`, add to the `InputMode` enum (after `ConcordanceWorksPicker`):

```rust
    AuthorshipPicker,
```

- [ ] **Step 2: Create `src/ui/authorship_picker.rs`**

Follow the MediaPicker pattern — a simple ListBox inside a ScrolledWindow overlay:

```rust
use gtk4::prelude::*;
use crate::db::authorship::AttributionSet;

pub struct AuthorshipPicker {
    pub overlay: gtk4::Overlay,
    container: gtk4::Box,
    list_box: gtk4::ListBox,
    items: Vec<AttributionSet>,
}

impl AuthorshipPicker {
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        container.set_halign(gtk4::Align::Center);
        container.set_valign(gtk4::Align::Center);
        container.set_width_request(400);
        container.add_css_class("picker-container");

        let title = gtk4::Label::new(Some("Attribution Sets"));
        title.add_css_class("picker-title");
        container.append(&title);

        let list_box = gtk4::ListBox::new();
        list_box.set_selection_mode(gtk4::SelectionMode::Single);
        list_box.add_css_class("picker-list");

        let scroll = gtk4::ScrolledWindow::new();
        scroll.set_child(Some(&list_box));
        scroll.set_max_content_height(300);
        scroll.set_propagate_natural_height(true);
        container.append(&scroll);

        container.set_visible(false);

        let overlay = gtk4::Overlay::new();
        overlay.set_child(None::<&gtk4::Widget>);
        overlay.add_overlay(&container);

        Self { overlay, container, list_box, items: Vec::new() }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
    }

    pub fn set_items(&mut self, items: Vec<AttributionSet>) {
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }
        for (i, set) in items.iter().enumerate() {
            let label = gtk4::Label::new(Some(&set.display_name));
            label.set_halign(gtk4::Align::Start);
            label.set_margin_start(8);
            label.set_margin_end(8);
            label.set_margin_top(4);
            label.set_margin_bottom(4);
            self.list_box.append(&label);
            if i == 0 {
                if let Some(row) = self.list_box.row_at_index(0) {
                    self.list_box.select_row(Some(&row));
                }
            }
        }
        self.items = items;
    }

    pub fn show(&self) {
        self.container.set_visible(true);
        if let Some(row) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&row));
        }
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn move_selection(&self, delta: i32) {
        let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(0);
        let next = (current + delta).max(0);
        if let Some(row) = self.list_box.row_at_index(next) {
            self.list_box.select_row(Some(&row));
        }
    }

    pub fn selected_set(&self) -> Option<&AttributionSet> {
        let idx = self.list_box.selected_row()?.index();
        self.items.get(idx as usize)
    }
}
```

- [ ] **Step 3: Register the picker module**

In `src/ui/mod.rs`, add:

```rust
pub mod authorship_picker;
```

- [ ] **Step 4: Add AuthorshipPicker to AppState**

In the `AppState` struct, add:

```rust
    pub authorship_picker: crate::ui::authorship_picker::AuthorshipPicker,
```

In `build_window`, create the picker before the `AppState` construction:

```rust
    let authorship_picker = crate::ui::authorship_picker::AuthorshipPicker::new();
```

Wire it into the overlay chain — attach it so it wraps an existing overlay (follow the pattern used by other pickers — find where `concordance_list_picker.overlay.add_overlay` or similar chaining happens, and insert `authorship_picker` similarly).

In the `AppState { ... }` construction, add:

```rust
        authorship_picker,
```

- [ ] **Step 5: Create `src/input/actions/authorship.rs`**

```rust
use std::cell::RefCell;
use std::rc::Rc;
use crate::app::AppState;

pub fn open_attribution_picker(state: &Rc<RefCell<AppState>>) {
    let sets = state.borrow().authorship_sets.clone();
    {
        let mut s = state.borrow_mut();
        s.authorship_picker.set_items(sets);
        s.authorship_picker.show();
        s.input_mode = crate::app::InputMode::AuthorshipPicker;
    }
}

pub fn confirm_attribution_selection(state: &Rc<RefCell<AppState>>) {
    let selected = state.borrow().authorship_picker.selected_set().cloned();
    if let Some(set) = selected {
        {
            let s = state.borrow();
            s.authorship_picker.hide();
        }
        let mut s = state.borrow_mut();
        s.input_mode = crate::app::InputMode::Reader;
        s.active_attribution_set_id = Some(set.id);
        if let Ok(conn) = crate::db::queries::open_db() {
            s.authorship_line_ids = crate::db::authorship::load_secondary_line_ids(
                &conn, set.id, &set.work_abbrev,
            ).unwrap_or_default();
        }
        s.authorship_enabled = true;
        crate::app::apply_authorship_formatting(&mut s);

        let msg = format!("Authorship: {}", set.display_name);
        s.chapter_toast.set_text(&msg);
        s.chapter_toast.set_visible(true);
        let toast = s.chapter_toast.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            toast.set_visible(false);
        });
    }
}
```

- [ ] **Step 6: Register the actions module**

In `src/input/actions/mod.rs`, add:

```rust
pub mod authorship;
```

- [ ] **Step 7: Add AuthorshipPicker to handle_picker_key**

In `src/input/keymap.rs`:

1. Add `AuthorshipPicker` to the mode dispatch at line 68 (the `handle_picker_key` arm):

```rust
            | crate::app::InputMode::AuthorshipPicker
```

2. In `handle_picker_key`, add `AuthorshipPicker` arms in each `PickerAction` match:

In `PickerAction::Hide`:
```rust
                InputMode::AuthorshipPicker => { s.authorship_picker.hide(); s.input_mode = InputMode::Reader; }
```

In `PickerAction::Confirm`:
```rust
                InputMode::AuthorshipPicker => {
                    crate::input::actions::authorship::confirm_attribution_selection(state);
                    true
                }
```

In `PickerAction::MoveDown`:
```rust
                InputMode::AuthorshipPicker => state.borrow().authorship_picker.move_selection(1),
```

In `PickerAction::MoveUp`:
```rust
                InputMode::AuthorshipPicker => state.borrow().authorship_picker.move_selection(-1),
```

- [ ] **Step 8: Verify it compiles**

Run: `cargo build`
Expected: compiles cleanly

- [ ] **Step 9: Commit**

```bash
git add src/ui/authorship_picker.rs src/ui/mod.rs src/input/actions/authorship.rs src/input/actions/mod.rs src/input/keymap.rs src/app.rs
git commit -m "feat(authorship): add attribution set picker and action handlers"
```

---

### Task 7: Keymap JSON Update

**Files:**
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`

- [ ] **Step 1: Add the two keybinds to keymap.json**

Add to the JSON array:

```json
  {"key": "a", "ctrl": true, "action": "ToggleAuthorship"},
  {"key": "A", "ctrl": true, "shift": true, "action": "PickAttributionSet"}
```

- [ ] **Step 2: Restow**

```bash
cd ~/tty-dotfiles && stow linux-lit
```

- [ ] **Step 3: Commit**

```bash
cd ~/tty-dotfiles && git add linux-lit/.config/linux-lit/keymap.json && git commit -m "feat(linux-lit): add authorship keybinds Ctrl+a and Ctrl+Shift+A"
```

---

### Task 8: Final Build and Manual Test

- [ ] **Step 1: Full build and test**

Run: `cargo build && cargo test && cargo clippy`
Expected: all pass cleanly

- [ ] **Step 2: Manual verification**

Run the app (`cargo run`), open Henry VIII (H8). Verify:
1. Collaborator lines (Fletcher) appear in italics automatically
2. `Ctrl+a` toggles italics off/on with toast
3. `Ctrl+Shift+A` shows "Only one attribution set available" toast (H8 has one set)
4. Open a non-co-authored work — `Ctrl+a` shows "No authorship data" toast
5. Check `linux-lit-dev.log` for `TIMING: apply_authorship_formatting` line

- [ ] **Step 3: Final commit if any fixups needed**

```bash
git add -A && git commit -m "fix(authorship): address review feedback"
```
