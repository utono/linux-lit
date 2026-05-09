# Gloss Picker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an Alt+g gloss picker that lists all glossed passages in the current work, with fuzzy filtering, and navigates to the selected passage's gloss overlay on confirm.

**Architecture:** New `GlossPicker` widget in `src/ui/gloss_picker.rs` following the `BookmarkPicker` flat-list pattern. Wired into the existing `handle_picker_key()` dispatcher. Opener function in `pickers.rs` spawns an async DB query and populates the picker.

**Tech Stack:** GTK4 (Overlay, ListBox, Entry), rusqlite (existing `find_glossed_passages` query), Rust

---

### Task 1: Create the GlossPicker widget

**Files:**
- Create: `src/ui/gloss_picker.rs`
- Modify: `src/ui/mod.rs:6` (add module declaration)

- [ ] **Step 1: Add module declaration to `src/ui/mod.rs`**

Insert after line 6 (`pub mod gloss_overlay;`):

```rust
pub mod gloss_picker;
```

- [ ] **Step 2: Create `src/ui/gloss_picker.rs`**

```rust
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Orientation, Overlay, ScrolledWindow,
};

use crate::db::queries::GlossedPassage;

pub struct GlossPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    items: Vec<GlossedPassage>,
}

impl GlossPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(600)
            .height_request(400)
            .build();
        picker_box.add_css_class("library-picker");

        let search_entry = Entry::builder()
            .placeholder_text("Filter glosses...")
            .build();

        let list_box = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .build();

        let scrolled = ScrolledWindow::builder()
            .child(&list_box)
            .vexpand(true)
            .build();

        picker_box.append(&search_entry);
        picker_box.append(&scrolled);

        GlossPicker {
            overlay,
            picker_box,
            search_entry,
            list_box,
            items: Vec::new(),
        }
    }

    pub fn set_items(&mut self, items: Vec<GlossedPassage>) {
        self.items = items;
        self.populate_list("");
    }

    pub fn show(&self) {
        self.picker_box.set_visible(true);
        self.search_entry.set_text("");
        self.search_entry.grab_focus();
        self.populate_list("");
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.picker_box.is_visible()
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.picker_box);
        self.picker_box.set_visible(false);
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    pub fn populate_list(&self, filter: &str) {
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        let filter_lower = filter.to_lowercase();

        for (idx, item) in self.items.iter().enumerate() {
            if !filter.is_empty() {
                let target = format!("{} {}", item.speaker, item.source_text).to_lowercase();
                if !subsequence_match(&filter_lower, &target) {
                    continue;
                }
            }

            let first_line = item.source_text.lines().next().unwrap_or("");
            let display = if item.speaker.is_empty() {
                first_line.to_string()
            } else {
                format!("{}: {}", item.speaker, first_line)
            };

            let text_label = Label::builder()
                .label(&display)
                .halign(gtk4::Align::Start)
                .hexpand(true)
                .ellipsize(gtk4::pango::EllipsizeMode::End)
                .build();

            let citation_label = Label::builder()
                .label(&item.start_citation)
                .halign(gtk4::Align::End)
                .build();
            citation_label.add_css_class("picker-item-detail");

            let hbox = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(8)
                .build();
            hbox.append(&text_label);
            hbox.append(&citation_label);

            let row = ListBoxRow::builder().child(&hbox).build();
            row.set_widget_name(&idx.to_string());
            self.list_box.append(&row);
        }

        if let Some(first) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&first));
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.list_box
            .selected_row()
            .and_then(|row| row.widget_name().to_string().parse().ok())
    }

    pub fn move_selection(&self, delta: i32) {
        if let Some(current) = self.list_box.selected_row() {
            let idx = current.index();
            let new_idx = (idx + delta).max(0);
            if let Some(row) = self.list_box.row_at_index(new_idx) {
                self.list_box.select_row(Some(&row));
            }
        }
    }
}

fn subsequence_match(filter: &str, target: &str) -> bool {
    let mut target_chars = target.chars();
    for fc in filter.chars() {
        if !target_chars.any(|tc| tc == fc) {
            return false;
        }
    }
    true
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | head -5`
Expected: May warn about dead code (GlossPicker not used yet), but no errors.

- [ ] **Step 4: Commit**

```bash
git add src/ui/gloss_picker.rs src/ui/mod.rs
git commit -m "feat: add GlossPicker widget (flat-list with fuzzy filter)"
```

---

### Task 2: Add OpenGlossPicker action variant

**Files:**
- Modify: `src/input/actions/mod.rs:87` (add variant after ToggleGlossOverlay)
- Modify: `src/input/actions/mod.rs:175-178` (add to Vocab category match)
- Modify: `src/input/actions/mod.rs:269` (add to name() match)

- [ ] **Step 1: Add the Action variant**

In `src/input/actions/mod.rs`, add `OpenGlossPicker` after `ToggleGlossOverlay` (line 87):

```rust
    ToggleGlossOverlay,
    OpenGlossPicker,
```

- [ ] **Step 2: Add to category() match**

In the `Vocab` arm (around line 175-178), add `OpenGlossPicker` before the `=> Category::Vocab` line. The arm should end:

```rust
            | Action::ToggleGlossOverlay
            | Action::OpenGlossPicker
            | Action::OpenConcordancePicker
```

- [ ] **Step 3: Add to name() match**

After the `ToggleGlossOverlay` line (line 269), add:

```rust
            Action::OpenGlossPicker => "OpenGlossPicker",
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | head -10`
Expected: Compile error in `keymap.rs` `dispatch_action` — the match is non-exhaustive because `OpenGlossPicker` has no arm yet. This is expected and will be fixed in Task 4.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/mod.rs
git commit -m "feat: add OpenGlossPicker action variant"
```

---

### Task 3: Add default Alt+g keybinding

**Files:**
- Modify: `src/input/keymap_config.rs:263` (add binding to vocab_bindings)

- [ ] **Step 1: Add the binding**

In `src/input/keymap_config.rs`, in `vocab_bindings()` (after the `ctrl_alt("p")` line, around line 267), add:

```rust
        (KeyCombo::alt("g"), Action::OpenGlossPicker),
```

- [ ] **Step 2: Commit**

```bash
git add src/input/keymap_config.rs
git commit -m "feat: bind Alt+g to OpenGlossPicker"
```

---

### Task 4: Add GlossPicker InputMode and wire into app.rs

**Files:**
- Modify: `src/app.rs:36-52` (add GlossPicker to InputMode enum)
- Modify: `src/app.rs:55` (add gloss_picker field to AppState)
- Modify: `src/app.rs:658-661` (overlay nesting — insert between gloss_overlay and concordance_picker)
- Modify: `src/app.rs:824` (AppState constructor — add gloss_picker field)
- Modify: `src/app.rs:1068` (signal wiring — add search_entry changed handler)

- [ ] **Step 1: Add GlossPicker to InputMode enum**

In `src/app.rs`, add `GlossPicker` after `GlossPrompt` (line 44):

```rust
    GlossPrompt,
    GlossPicker,
```

- [ ] **Step 2: Add import for GlossPicker**

After the existing `use crate::ui::bookmark_picker::BookmarkPicker;` line (line 16), add:

```rust
use crate::ui::gloss_picker::GlossPicker;
```

- [ ] **Step 3: Add gloss_picker field to AppState**

In the `AppState` struct, after the `gloss_prompt_textview` field (line 151), add:

```rust
    pub gloss_picker: GlossPicker,
```

- [ ] **Step 4: Insert overlay nesting**

In the overlay chain (around line 658-661), the current code is:

```rust
    // Concordance picker wraps the gloss overlay
    let concordance_picker = crate::ui::concordance_picker::ConcordancePicker::new();
    concordance_picker.attach(&gloss_overlay.overlay);
```

Change to:

```rust
    // Gloss picker wraps the gloss overlay
    let gloss_picker = GlossPicker::new();
    gloss_picker.attach(&gloss_overlay.overlay);
    gloss_picker.overlay.set_vexpand(true);

    // Concordance picker wraps the gloss picker
    let concordance_picker = crate::ui::concordance_picker::ConcordancePicker::new();
    concordance_picker.attach(&gloss_picker.overlay);
```

- [ ] **Step 5: Add gloss_picker to AppState constructor**

In the `AppState { ... }` initializer (around line 831), after `gloss_prompt_textview: None,`, add:

```rust
        gloss_picker,
```

- [ ] **Step 6: Wire the search_entry changed signal**

After the bookmark picker filter connection block (around line 1068), add:

```rust
    // Connect gloss picker search entry filter
    let state_for_gloss_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.gloss_picker.search_entry().connect_changed(move |entry| {
            let text = entry.text();
            state_for_gloss_filter
                .borrow()
                .gloss_picker
                .populate_list(&text);
        });
    }
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo build 2>&1 | head -10`
Expected: Still a compile error in `dispatch_action` for the missing `OpenGlossPicker` arm. Fixed in Task 5.

- [ ] **Step 8: Commit**

```bash
git add src/app.rs
git commit -m "feat: add GlossPicker to InputMode, AppState, overlay chain, and signal wiring"
```

---

### Task 5: Add opener function and dispatch arm

**Files:**
- Modify: `src/input/actions/pickers.rs` (add `open_gloss_picker` function)
- Modify: `src/input/keymap.rs:784` (add dispatch arm for OpenGlossPicker)

- [ ] **Step 1: Add open_gloss_picker function**

At the end of `src/input/actions/pickers.rs` (after the `delete_bookmark` function), add:

```rust
/// Open the gloss picker, querying glossed passages for the current work.
/// Spawns an async task to query the DB.
pub(crate) fn open_gloss_picker(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());
    if let Some(abbrev) = abbrev {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let items = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::queries::find_glossed_passages(&conn, &abbrev)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            {
                let mut s = state_clone.borrow_mut();
                s.gloss_overlay.hide();
                s.gloss_picker.set_items(items);
            }
            state_clone.borrow().gloss_picker.show();
            state_clone.borrow_mut().input_mode = crate::app::InputMode::GlossPicker;
        });
    }
}
```

- [ ] **Step 2: Add dispatch arm in keymap.rs**

In `src/input/keymap.rs`, in `dispatch_action()`, after the `ToggleGlossOverlay` arm (line 784):

```rust
        ToggleGlossOverlay => crate::input::actions::gloss::toggle_overlay(state),
```

Add:

```rust
        OpenGlossPicker => crate::input::actions::pickers::open_gloss_picker(state, tokio_handle),
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | head -10`
Expected: Still a compile error in `handle_key` — `GlossPicker` InputMode is not matched in mode dispatch. Fixed in Task 6.

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/pickers.rs src/input/keymap.rs
git commit -m "feat: add open_gloss_picker opener and dispatch arm"
```

---

### Task 6: Wire GlossPicker into handle_picker_key

**Files:**
- Modify: `src/input/keymap.rs:60-66` (add GlossPicker to mode dispatch)
- Modify: `src/input/keymap.rs:221-229` (add GlossPicker Hide arm)
- Modify: `src/input/keymap.rs:233-298` (add GlossPicker Confirm arm)
- Modify: `src/input/keymap.rs:300-309` (add GlossPicker MoveDown arm)
- Modify: `src/input/keymap.rs:311-320` (add GlossPicker MoveUp arm)

- [ ] **Step 1: Add GlossPicker to mode dispatch in handle_key**

In `src/input/keymap.rs`, in the mode dispatch match (around line 62-66), add `InputMode::GlossPicker` to the existing `handle_picker_key` branch:

```rust
            crate::app::InputMode::BookmarkPicker
            | crate::app::InputMode::MediaPicker
            | crate::app::InputMode::ConcordancePicker
            | crate::app::InputMode::ConcordanceWordPicker
            | crate::app::InputMode::ConcordanceListPicker
            | crate::app::InputMode::GlossPicker => handle_picker_key(state, key_name, is_ctrl, tokio_handle, mode),
```

- [ ] **Step 2: Add GlossPicker Hide arm**

In `handle_picker_key`, in the `PickerAction::Hide` match (around line 221-229), add after the `ConcordanceListPicker` arm:

```rust
                InputMode::GlossPicker => { s.gloss_picker.hide(); s.input_mode = InputMode::Reader; }
```

- [ ] **Step 3: Add GlossPicker Confirm arm**

In `handle_picker_key`, in the `PickerAction::Confirm` match (around line 233-298), add a new arm before the `_ => true` fallthrough:

```rust
                InputMode::GlossPicker => {
                    let selected = state.borrow().gloss_picker.selected_index();
                    if let Some(idx) = selected {
                        let passage = state.borrow().gloss_picker.items[idx].clone();
                        {
                            let s = state.borrow();
                            s.gloss_picker.hide();
                        }

                        let all_glosses = crate::db::queries::open_db()
                            .ok()
                            .and_then(|conn| {
                                crate::db::queries::find_all_glosses(
                                    &conn, &passage.work_abbrev,
                                    &passage.start_citation, &passage.end_citation,
                                ).ok()
                            })
                            .unwrap_or_default();

                        if all_glosses.is_empty() {
                            state.borrow_mut().input_mode = InputMode::Reader;
                            return true;
                        }

                        let mut s = state.borrow_mut();
                        let work_title = s.current_work.as_ref()
                            .map(|w| w.title.clone()).unwrap_or_default();
                        let ctx = crate::gloss::GlossContext {
                            work_abbrev: passage.work_abbrev,
                            work_title,
                            start_citation: passage.start_citation,
                            end_citation: passage.end_citation,
                            act: passage.act,
                            scene: passage.scene,
                            speaker: passage.speaker,
                            source_text: passage.source_text,
                            source_line_numbers: Vec::new(),
                            hash: String::new(),
                        };

                        let h = s.scrolled_window.height();
                        let source_lines: Vec<(String, i64)> = Vec::new();
                        s.gloss_overlay.show_gloss_with_color(
                            &ctx.source_text, &all_glosses[0].gloss_text, h,
                            Some(&s.theme.root_color), &source_lines,
                        );
                        s.gloss_overlay.set_position(0, all_glosses.len());

                        s.gloss_passages = s.gloss_picker.items.clone();
                        s.gloss_passage_index = idx;
                        s.gloss_list = all_glosses;
                        s.gloss_index = 0;
                        s.gloss_context = Some(ctx);
                        s.input_mode = InputMode::GlossOverlay;
                    }
                    true
                }
```

- [ ] **Step 4: Add GlossPicker MoveDown arm**

In `handle_picker_key`, in the `PickerAction::MoveDown` match (around line 300-309), add:

```rust
                InputMode::GlossPicker => state.borrow().gloss_picker.move_selection(1),
```

- [ ] **Step 5: Add GlossPicker MoveUp arm**

In `handle_picker_key`, in the `PickerAction::MoveUp` match (around line 311-320), add:

```rust
                InputMode::GlossPicker => state.borrow().gloss_picker.move_selection(-1),
```

- [ ] **Step 6: Make gloss_picker.items pub(crate)**

The Confirm arm reads `state.borrow().gloss_picker.items[idx].clone()` and `s.gloss_picker.items.clone()`. The `items` field in `GlossPicker` is currently private. In `src/ui/gloss_picker.rs`, change:

```rust
    items: Vec<GlossedPassage>,
```

to:

```rust
    pub(crate) items: Vec<GlossedPassage>,
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo build 2>&1 | head -5`
Expected: Clean compile, no errors.

- [ ] **Step 8: Run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All existing tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/input/keymap.rs src/ui/gloss_picker.rs
git commit -m "feat: wire GlossPicker into handle_picker_key with confirm/hide/move"
```

---

### Task 7: Update keybinds overlay

**Files:**
- Modify: `src/ui/keybinds_overlay.rs:59` (add Alt+g entry)

- [ ] **Step 1: Add Alt+g to keybinds overlay**

In `src/ui/keybinds_overlay.rs`, find the line with the `g` key entry (line 59):

```rust
    key("g", "G", "", "", &[("C-g", "gloss tog")]),
```

Add `A-g` to the hints array:

```rust
    key("g", "G", "", "", &[("C-g", "gloss tog"), ("A-g", "gloss pick")]),
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | head -5`
Expected: Clean compile.

- [ ] **Step 3: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "feat: add Alt+g gloss picker to keybinds overlay"
```
