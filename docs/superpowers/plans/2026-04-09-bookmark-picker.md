# Bookmark Picker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a bookmark picker UI (Ctrl+m) that lists bookmarks for the current work with line text and relative timestamps, supports filtering, jumping, and deleting bookmarks.

**Architecture:** New `BookmarkPicker` widget following the `MediaPicker` pattern (flat list, overlay, subsequence filtering). New DB queries to load bookmark details and delete bookmarks. Keybind changes to move media picker to Ctrl+Shift+M and wire bookmark picker to Ctrl+m.

**Tech Stack:** Rust, GTK4/libadwaita, sourceview5, rusqlite

**Spec:** `docs/superpowers/specs/2026-04-09-bookmark-picker-design.md`

---

## File Map

- **Create:** `src/ui/bookmark_picker.rs` — new picker widget (~180 lines, modeled on media_picker.rs)
- **Modify:** `src/ui/mod.rs` — add `pub mod bookmark_picker;`
- **Modify:** `src/db/models.rs` — add `BookmarkItem` struct
- **Modify:** `src/db/queries.rs` — add `load_bookmarks_with_details()`, `delete_bookmark()`
- **Modify:** `src/app.rs:86` — add `bookmark_picker` field to AppState
- **Modify:** `src/app.rs:448-460` — insert bookmark picker into overlay chain
- **Modify:** `src/app.rs:610` — initialize `bookmark_picker` in struct literal
- **Modify:** `src/app.rs:672-683` — wire search signal for bookmark picker
- **Modify:** `src/input/keymap.rs:293-432` — add bookmark picker key handling block (before media picker block)
- **Modify:** `src/input/keymap.rs:497-526` — change Ctrl+m to open bookmark picker, add Ctrl+Shift+M for media picker

---

### Task 1: Data layer — BookmarkItem model and DB queries

**Files:**
- Modify: `src/db/models.rs`
- Modify: `src/db/queries.rs`

- [ ] **Step 1: Add `BookmarkItem` struct to models.rs**

Add at the end of `src/db/models.rs`:

```rust
#[derive(Debug, Clone)]
pub struct BookmarkItem {
    pub line_mapping_id: i64,
    pub line_text: String,
    pub created_at: String,
}
```

- [ ] **Step 2: Add `load_bookmarks_with_details()` to queries.rs**

Add after the existing `most_recent_bookmark()` function in `src/db/queries.rs`:

```rust
/// Load bookmarks with line text for the picker, sorted by most recent first.
pub fn load_bookmarks_with_details(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<super::models::BookmarkItem>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT b.line_mapping_id, lm.canonical_text, b.created_at \
         FROM bookmarks b \
         JOIN line_mapping lm ON b.line_mapping_id = lm.id \
         WHERE b.work_abbrev = ?1 \
         ORDER BY b.created_at DESC"
    )?;
    let rows = stmt.query_map([work_abbrev], |row| {
        Ok(super::models::BookmarkItem {
            line_mapping_id: row.get(0)?,
            line_text: row.get(1)?,
            created_at: row.get(2)?,
        })
    })?;
    rows.collect()
}
```

- [ ] **Step 3: Add `delete_bookmark()` to queries.rs**

```rust
/// Delete a bookmark by work and line_mapping_id.
pub fn delete_bookmark(
    conn: &Connection,
    work_abbrev: &str,
    line_mapping_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM bookmarks WHERE work_abbrev = ?1 AND line_mapping_id = ?2",
        rusqlite::params![work_abbrev, line_mapping_id],
    )?;
    Ok(())
}
```

- [ ] **Step 4: Add test for load_bookmarks_with_details**

Add to the `#[cfg(test)] mod tests` block in queries.rs:

```rust
#[test]
fn test_load_bookmarks_with_details() {
    let conn = open_db_rw().expect("Failed to open lit.db rw");
    ensure_bookmarks_table(&conn).expect("Failed to create bookmarks table");

    let work_abbrev = "Ham";
    let line_id: i64 = conn.query_row(
        "SELECT id FROM line_mapping WHERE work_abbrev = ?1 LIMIT 1",
        [work_abbrev],
        |row| row.get(0),
    ).expect("Hamlet should have lines");

    // Clean up
    let _ = conn.execute(
        "DELETE FROM bookmarks WHERE work_abbrev = ?1 AND line_mapping_id = ?2",
        rusqlite::params![work_abbrev, line_id],
    );

    // Add a bookmark
    toggle_bookmark(&conn, work_abbrev, line_id).unwrap();

    // Load with details
    let items = load_bookmarks_with_details(&conn, work_abbrev).unwrap();
    let found = items.iter().find(|i| i.line_mapping_id == line_id);
    assert!(found.is_some(), "Should find the bookmarked line");
    let item = found.unwrap();
    assert!(!item.line_text.is_empty(), "Line text should not be empty");
    assert!(!item.created_at.is_empty(), "created_at should not be empty");

    // Delete it
    delete_bookmark(&conn, work_abbrev, line_id).unwrap();
    let items = load_bookmarks_with_details(&conn, work_abbrev).unwrap();
    assert!(
        !items.iter().any(|i| i.line_mapping_id == line_id),
        "Bookmark should be deleted"
    );
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test test_load_bookmarks_with_details -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/db/models.rs src/db/queries.rs
git commit -m "feat: add BookmarkItem model and picker DB queries"
```

---

### Task 2: BookmarkPicker widget

**Files:**
- Create: `src/ui/bookmark_picker.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Add module declaration**

Add to `src/ui/mod.rs`:

```rust
pub mod bookmark_picker;
```

- [ ] **Step 2: Create `src/ui/bookmark_picker.rs`**

This file follows the `MediaPicker` pattern closely. Full contents:

```rust
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Orientation, Overlay, ScrolledWindow,
};

use crate::db::models::BookmarkItem;

pub struct BookmarkPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    items: Vec<BookmarkItem>,
}

impl BookmarkPicker {
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
            .placeholder_text("Filter bookmarks...")
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

        BookmarkPicker {
            overlay,
            picker_box,
            search_entry,
            list_box,
            items: Vec::new(),
        }
    }

    pub fn set_items(&mut self, items: Vec<BookmarkItem>) {
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

        for item in &self.items {
            if !filter.is_empty() {
                let target = item.line_text.to_lowercase();
                if !subsequence_match(&filter_lower, &target) {
                    continue;
                }
            }

            let text = if item.line_text.len() > 80 {
                format!("{}...", &item.line_text[..item.line_text.char_indices().nth(80).map(|(i, _)| i).unwrap_or(item.line_text.len())])
            } else {
                item.line_text.clone()
            };

            let time_label = format_relative_time(&item.created_at);

            let text_label = Label::builder()
                .label(&text)
                .halign(gtk4::Align::Start)
                .hexpand(true)
                .ellipsize(gtk4::pango::EllipsizeMode::End)
                .build();

            let time_lbl = Label::builder()
                .label(&time_label)
                .halign(gtk4::Align::End)
                .build();
            time_lbl.add_css_class("picker-item-detail");

            let hbox = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(8)
                .build();
            hbox.append(&text_label);
            hbox.append(&time_lbl);

            let row = ListBoxRow::builder().child(&hbox).build();
            row.set_widget_name(&item.line_mapping_id.to_string());
            self.list_box.append(&row);
        }

        if let Some(first) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&first));
        }
    }

    pub fn selected_line_mapping_id(&self) -> Option<i64> {
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

    /// Remove the selected bookmark from the internal items list and the ListBox.
    /// Returns the line_mapping_id of the removed item, or None if nothing selected.
    pub fn remove_selected(&mut self) -> Option<i64> {
        let row = self.list_box.selected_row()?;
        let lm_id: i64 = row.widget_name().to_string().parse().ok()?;
        let idx = row.index();

        // Remove from internal items
        self.items.retain(|i| i.line_mapping_id != lm_id);

        // Remove the row from the list
        self.list_box.remove(&row);

        // Select the next row (or previous if it was the last)
        let next = self.list_box.row_at_index(idx)
            .or_else(|| self.list_box.row_at_index((idx - 1).max(0)));
        if let Some(r) = next {
            self.list_box.select_row(Some(&r));
        }

        Some(lm_id)
    }

    pub fn has_items(&self) -> bool {
        !self.items.is_empty()
    }
}

fn format_relative_time(iso: &str) -> String {
    // Parse ISO-8601: "2026-04-09T12:34:56.789Z"
    // Use a simple approach: parse year, month, day, hour, minute, second
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let created = parse_iso_to_unix(iso).unwrap_or(now);
    if created >= now {
        return "just now".to_string();
    }
    let diff = now - created;

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 86400 * 30 {
        format!("{}d ago", diff / 86400)
    } else {
        format!("{}mo ago", diff / (86400 * 30))
    }
}

fn parse_iso_to_unix(iso: &str) -> Option<u64> {
    // Minimal ISO-8601 parser for "YYYY-MM-DDTHH:MM:SS" (UTC assumed)
    // Handles optional fractional seconds and trailing Z
    let s = iso.trim_end_matches('Z');
    let (date_part, time_part) = s.split_once('T')?;
    let mut date_iter = date_part.split('-');
    let year: i64 = date_iter.next()?.parse().ok()?;
    let month: i64 = date_iter.next()?.parse().ok()?;
    let day: i64 = date_iter.next()?.parse().ok()?;

    // Split time on '.' to ignore fractional seconds
    let time_no_frac = time_part.split('.').next()?;
    let mut time_iter = time_no_frac.split(':');
    let hour: i64 = time_iter.next()?.parse().ok()?;
    let minute: i64 = time_iter.next()?.parse().ok()?;
    let second: i64 = time_iter.next().unwrap_or("0").parse().ok()?;

    // Days from epoch (1970-01-01) using a simplified calculation
    let mut days: i64 = 0;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    let month_days = [31, if is_leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for m in 0..(month - 1) as usize {
        days += month_days.get(m).copied().unwrap_or(30) as i64;
    }
    days += day - 1;

    let secs = days * 86400 + hour * 3600 + minute * 60 + second;
    Some(secs as u64)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
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

- [ ] **Step 3: Build to verify**

Run: `cargo build`
Expected: Compiles (warnings about unused struct/functions are fine)

- [ ] **Step 4: Commit**

```bash
git add src/ui/bookmark_picker.rs src/ui/mod.rs
git commit -m "feat: add BookmarkPicker widget with filtering and relative timestamps"
```

---

### Task 3: AppState integration — field, overlay chain, search signal

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add import and field**

Add import near the top of `src/app.rs` alongside the existing `use crate::ui::media_picker::MediaPicker;`:

```rust
use crate::ui::bookmark_picker::BookmarkPicker;
```

Add field to `AppState` struct (after `media_picker: MediaPicker`):

```rust
    pub bookmark_picker: BookmarkPicker,
```

- [ ] **Step 2: Insert into overlay chain**

In the overlay chain section (around line 448), the current order is:
1. library picker
2. media picker wraps library picker
3. settings wraps media picker

Insert bookmark picker between media picker and settings. After `media_picker.overlay.set_vexpand(true);` (line 450), add:

```rust
    // Bookmark picker overlay wraps the media picker overlay
    let bookmark_picker = BookmarkPicker::new();
    bookmark_picker.attach(&media_picker.overlay);
    bookmark_picker.overlay.set_vexpand(true);
```

Then change the settings overlay to attach to the bookmark picker instead of media picker. Change:
```rust
    settings_overlay.attach(&media_picker.overlay);
```
to:
```rust
    settings_overlay.attach(&bookmark_picker.overlay);
```

- [ ] **Step 3: Initialize in struct literal**

Add `bookmark_picker,` after `media_picker,` in the AppState struct literal (around line 610).

- [ ] **Step 4: Wire search signal**

After the media picker search signal connection (around line 683), add:

```rust
    // Connect bookmark picker search entry filter
    let state_for_bookmark_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.bookmark_picker.search_entry().connect_changed(move |entry| {
            let text = entry.text();
            state_for_bookmark_filter
                .borrow()
                .bookmark_picker
                .populate_list(&text);
        });
    }
```

- [ ] **Step 5: Build to verify**

Run: `cargo build`
Expected: Compiles with no errors

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat: integrate BookmarkPicker into AppState and overlay chain"
```

---

### Task 4: Keybinds — bookmark picker keys and media picker move

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Add bookmark picker key handling block**

Before the media picker handling block (before line 292 `// Media picker`), add a new block for the bookmark picker:

```rust
    // Bookmark picker
    let bookmark_picker_visible = state.borrow().bookmark_picker.is_visible();

    // Ctrl+n/Ctrl+p navigate bookmark picker list when visible
    if bookmark_picker_visible && is_ctrl {
        match key_name {
            "n" => {
                state.borrow().bookmark_picker.move_selection(1);
                return true;
            }
            "p" => {
                state.borrow().bookmark_picker.move_selection(-1);
                return true;
            }
            _ => {}
        }
    }

    if bookmark_picker_visible {
        match key_name {
            "Escape" => {
                state.borrow().bookmark_picker.hide();
                return true;
            }
            "Return" => {
                let selected_id = state.borrow().bookmark_picker.selected_line_mapping_id();
                if let Some(lm_id) = selected_id {
                    {
                        let s = state.borrow();
                        s.bookmark_picker.hide();
                    }
                    let mut s = state.borrow_mut();
                    let buffer_line = if let Some(ref lm) = s.line_map {
                        s.current_work.as_ref().and_then(|w| {
                            let work_idx = w.lines.iter().position(|l| l.id == lm_id)?;
                            Some(lm.work_to_buffer[work_idx])
                        })
                    } else {
                        s.current_work.as_ref().and_then(|w| {
                            w.lines.iter().position(|l| l.id == lm_id)
                        })
                    };
                    if let Some(bl) = buffer_line {
                        navigation::jump_to_line(&mut s, bl);
                    }
                }
                return true;
            }
            "Delete" | "d" => {
                let is_search_focused = state.borrow().bookmark_picker.search_entry().has_focus();
                if key_name == "Delete" || !is_search_focused {
                    let selected_id = state.borrow().bookmark_picker.selected_line_mapping_id();
                    let abbrev = state
                        .borrow()
                        .current_work
                        .as_ref()
                        .map(|w| w.abbrev.clone());
                    if let (Some(lm_id), Some(abbrev)) = (selected_id, abbrev) {
                        let state_clone = Rc::clone(state);
                        let handle = tokio_handle.clone();
                        glib::spawn_future_local(async move {
                            let result = handle
                                .spawn_blocking(move || {
                                    let conn = crate::db::queries::open_db_rw()
                                        .expect("Failed to open lit.db rw");
                                    crate::db::queries::delete_bookmark(&conn, &abbrev, lm_id)
                                })
                                .await;
                            if let Ok(Ok(())) = result {
                                let mut s = state_clone.borrow_mut();
                                // Update is_bookmarked vec
                                let buffer_line = if let Some(ref lm) = s.line_map {
                                    s.current_work.as_ref().and_then(|w| {
                                        let work_idx = w.lines.iter().position(|l| l.id == lm_id)?;
                                        Some(lm.work_to_buffer[work_idx])
                                    })
                                } else {
                                    s.current_work.as_ref().and_then(|w| {
                                        w.lines.iter().position(|l| l.id == lm_id)
                                    })
                                };
                                if let Some(bl) = buffer_line {
                                    let mut bm = s.is_bookmarked.borrow_mut();
                                    if bl < bm.len() {
                                        bm[bl] = false;
                                    }
                                }
                                if let Some(ref renderer) = s.gutter_renderer {
                                    renderer.queue_draw();
                                }
                                // Remove from picker list
                                s.bookmark_picker.remove_selected();
                                if !s.bookmark_picker.has_items() {
                                    s.bookmark_picker.hide();
                                }
                            }
                        });
                    }
                    return true;
                }
            }
            "Down" | "j" => {
                let is_search_focused = state.borrow().bookmark_picker.search_entry().has_focus();
                if key_name == "Down" || !is_search_focused {
                    state.borrow().bookmark_picker.move_selection(1);
                    return true;
                }
            }
            "Up" | "k" => {
                let is_search_focused = state.borrow().bookmark_picker.search_entry().has_focus();
                if key_name == "Up" || !is_search_focused {
                    state.borrow().bookmark_picker.move_selection(-1);
                    return true;
                }
            }
            _ => {}
        }
        // Let typed characters through to search entry
        return false;
    }
```

- [ ] **Step 2: Change Ctrl+m to open bookmark picker instead of media picker**

Replace the existing `// Ctrl+m: open media picker` block (lines 497-526) with:

```rust
    // Ctrl+Shift+M: open media picker
    if is_ctrl && is_shift && key_name == "M" {
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
                        crate::db::queries::list_media_for_work(&conn, &abbrev)
                            .unwrap_or_default()
                    })
                    .await
                    .unwrap_or_default();
                {
                    let mut s = state_clone.borrow_mut();
                    s.correction_overlay.hide();
                    s.media_picker.set_items(items);
                }
                state_clone.borrow().media_picker.show();
            });
        }
        return true;
    }

    // Ctrl+m: open bookmark picker
    if is_ctrl && !is_shift && key_name == "m" {
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
                        crate::db::queries::load_bookmarks_with_details(&conn, &abbrev)
                            .unwrap_or_default()
                    })
                    .await
                    .unwrap_or_default();
                {
                    let mut s = state_clone.borrow_mut();
                    s.correction_overlay.hide();
                    s.bookmark_picker.set_items(items);
                }
                state_clone.borrow().bookmark_picker.show();
            });
        }
        return true;
    }
```

Important: The `Ctrl+Shift+M` check must come **before** the `Ctrl+m` check because `Ctrl+Shift+M` would also match `is_ctrl && key_name == "M"` (uppercase). The `!is_shift` guard on the `Ctrl+m` handler prevents it from also matching Shift.

- [ ] **Step 3: Build to verify**

Run: `cargo build`
Expected: Compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat: wire bookmark picker keybinds (Ctrl+m open, d delete, Return jump, Ctrl+Shift+M media)"
```

---

### Task 5: Final verification

- [ ] **Step 1: Run all tests**

Run: `cargo test`
Expected: All bookmark-related tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: No errors

- [ ] **Step 3: Manual smoke test checklist**

The user will run `cargo run` and verify:
1. Open a work with bookmarks (add some with `m` if needed)
2. Press `Ctrl+m` — bookmark picker opens with bookmarks listed (line text + relative time)
3. Type in search — filters by line text
4. Press `j`/`k` or Up/Down — moves selection
5. Press `Return` — jumps to the bookmarked line, picker closes
6. Reopen picker, select a bookmark, press `d` — bookmark deleted, row removed, gutter updates
7. Press `Ctrl+Shift+M` — media picker opens (moved keybind)
8. Press `Escape` in any picker — closes it

- [ ] **Step 4: Final commit if any fixes needed**

```bash
git add -u
git commit -m "fix: address issues found during bookmark picker testing"
```
