# Two-Level Library Picker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the flat work list in Ctrl+p picker with a two-level author-then-works browser with theme-integrated styling and a dimming scrim.

**Architecture:** The existing `LibraryPicker` struct gains a `PickerLevel` enum to track whether the user is browsing authors or works within an author. Works are grouped into `AuthorGroup` structs at `set_works()` time. A scrim widget dims the text behind the picker. CSS switches from hardcoded dark colors to theme variables.

**Tech Stack:** GTK4, Rust

---

## File Map

- **Modify:** `src/ui/library_picker.rs` — add state machine, grouped data, scrim, two-level rendering
- **Modify:** `src/theme.rs:344-347` — update `.library-picker` CSS to use theme variables
- **Modify:** `src/input/keymap.rs:84-150` — handle Escape/Backspace level navigation
- **Modify:** `src/app.rs:468-475` — update `connect_changed` to call new populate method

---

### Task 1: Add Data Structures and Author Grouping

**Files:**
- Modify: `src/ui/library_picker.rs`

- [ ] **Step 1: Write failing test for author grouping**

Add this test at the bottom of the `#[cfg(test)] mod tests` block in `src/ui/library_picker.rs`:

```rust
#[test]
fn test_group_works_by_author() {
    let works = vec![
        make_work("Gen", "Genesis", "KJV"),
        make_work("Ham", "Hamlet", "Shakespeare"),
        make_work("Exo", "Exodus", "KJV"),
        make_work("Rom", "Romeo and Juliet", "Shakespeare"),
        make_work("BH", "Bleak House", "Dickens, Charles"),
        make_work("Doll", "A Doll's House", "Henrik Ibsen"),
    ];
    let groups = group_works(works);
    // Shakespeare pinned first
    assert_eq!(groups[0].author, "Shakespeare");
    assert_eq!(groups[0].works.len(), 2);
    // Dickens pinned second
    assert_eq!(groups[1].author, "Dickens, Charles");
    assert_eq!(groups[1].works.len(), 1);
    // Remaining alphabetical
    assert_eq!(groups[2].author, "Henrik Ibsen");
    assert_eq!(groups[3].author, "KJV");
    // Works within each group sorted alphabetically
    assert_eq!(groups[0].works[0].title, "Hamlet");
    assert_eq!(groups[0].works[1].title, "Romeo and Juliet");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_group_works_by_author`
Expected: FAIL — `group_works` not found

- [ ] **Step 3: Add PickerLevel enum, AuthorGroup struct, and group_works function**

Add these types and the `group_works` function above the `impl LibraryPicker` block in `src/ui/library_picker.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum PickerLevel {
    Authors,
    Works { author: String },
}

#[derive(Debug, Clone)]
pub struct AuthorGroup {
    pub author: String,
    pub works: Vec<WorkSummary>,
}

const PINNED_AUTHORS: &[&str] = &["Shakespeare", "Dickens, Charles"];

fn group_works(works: Vec<WorkSummary>) -> Vec<AuthorGroup> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, Vec<WorkSummary>> = BTreeMap::new();
    for w in works {
        map.entry(w.author.clone()).or_default().push(w);
    }
    // Sort works within each group alphabetically
    for group in map.values_mut() {
        group.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    }
    // Build result: pinned authors first, then remaining alphabetical
    let mut result = Vec::new();
    for &pinned in PINNED_AUTHORS {
        if let Some(works) = map.remove(pinned) {
            result.push(AuthorGroup {
                author: pinned.to_string(),
                works,
            });
        }
    }
    for (author, works) in map {
        result.push(AuthorGroup { author, works });
    }
    result
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_group_works_by_author`
Expected: PASS

- [ ] **Step 5: Update LibraryPicker struct fields**

Replace the `works` field in the `LibraryPicker` struct and add `level` and `groups`:

Change the struct definition from:

```rust
pub struct LibraryPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    works: Vec<WorkSummary>,
}
```

to:

```rust
pub struct LibraryPicker {
    pub overlay: Overlay,
    scrim: GtkBox,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    groups: Vec<AuthorGroup>,
    level: PickerLevel,
}
```

- [ ] **Step 6: Update LibraryPicker::new() to initialize new fields and create scrim**

Replace the `new()` function body. Add scrim creation and initialize the new fields:

```rust
pub fn new() -> Self {
    let overlay = Overlay::new();

    let scrim = GtkBox::new(Orientation::Vertical, 0);
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.add_css_class("library-picker-scrim");
    scrim.set_visible(false);

    let picker_box = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .halign(gtk4::Align::Center)
        .valign(gtk4::Align::Center)
        .width_request(400)
        .height_request(400)
        .build();
    picker_box.add_css_class("library-picker");

    let search_entry = Entry::builder().placeholder_text("Filter authors...").build();

    let list_box = ListBox::builder()
        .selection_mode(gtk4::SelectionMode::Single)
        .build();

    let scrolled = ScrolledWindow::builder()
        .child(&list_box)
        .vexpand(true)
        .build();

    picker_box.append(&search_entry);
    picker_box.append(&scrolled);

    LibraryPicker {
        overlay,
        scrim,
        picker_box,
        search_entry,
        list_box,
        groups: Vec::new(),
        level: PickerLevel::Authors,
    }
}
```

- [ ] **Step 7: Update set_works to group by author**

Replace the `set_works` method:

```rust
pub fn set_works(&mut self, works: Vec<WorkSummary>) {
    self.groups = group_works(works);
    self.level = PickerLevel::Authors;
    self.populate_list("");
}
```

- [ ] **Step 8: Update attach() to add scrim as overlay**

Replace the `attach` method:

```rust
pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
    self.overlay.set_child(Some(base));
    self.overlay.add_overlay(&self.scrim);
    self.overlay.add_overlay(&self.picker_box);
    self.picker_box.set_visible(false);
}
```

- [ ] **Step 9: Update show/hide to toggle scrim**

Replace `show` and `hide`:

```rust
pub fn show(&self) {
    self.scrim.set_visible(true);
    self.picker_box.set_visible(true);
    self.search_entry.set_text("");
    self.search_entry.grab_focus();
    self.populate_list("");
}

pub fn hide(&self) {
    self.scrim.set_visible(false);
    self.picker_box.set_visible(false);
}
```

- [ ] **Step 10: Verify it compiles**

Run: `cargo build`
Expected: Compilation errors from `populate_list` and callers that use `set_works` with `&mut self` — those will be fixed in the next task.

- [ ] **Step 11: Commit**

```bash
git add src/ui/library_picker.rs
git commit -m "feat: add author grouping data structures and scrim to library picker"
```

---

### Task 2: Implement Two-Level populate_list

**Files:**
- Modify: `src/ui/library_picker.rs`

- [ ] **Step 1: Write test for author-level population**

Add to the test module in `src/ui/library_picker.rs`:

```rust
#[test]
fn test_subsequence_match_author_name() {
    let group = AuthorGroup {
        author: "Shakespeare".to_string(),
        works: vec![make_work("Ham", "Hamlet", "Shakespeare")],
    };
    assert!(author_name_matches("shak", &group));
    assert!(author_name_matches("", &group));
    assert!(!author_name_matches("dickens", &group));
}

#[test]
fn test_filter_finds_works_across_authors() {
    let groups = vec![
        AuthorGroup {
            author: "Shakespeare".to_string(),
            works: vec![
                make_work("Ham", "Hamlet", "Shakespeare"),
                make_work("Rom", "Romeo and Juliet", "Shakespeare"),
            ],
        },
        AuthorGroup {
            author: "KJV".to_string(),
            works: vec![make_work("Gen", "Genesis", "KJV")],
        },
    ];
    // "hamlet" should match only Shakespeare's works
    let matching = find_matching_authors(&groups, "hamlet");
    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].0, "Shakespeare");
    assert_eq!(matching[0].1.len(), 1);
    assert_eq!(matching[0].1[0].title, "Hamlet");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_subsequence_match_author_name test_filter_finds_works_across_authors`
Expected: FAIL — functions not found

- [ ] **Step 3: Add helper functions for filtering**

Add these functions near the existing `subsequence_match` function:

```rust
/// Case-insensitive subsequence match against author name only.
fn author_name_matches(filter: &str, group: &AuthorGroup) -> bool {
    if filter.is_empty() {
        return true;
    }
    let target = group.author.to_lowercase();
    let mut target_chars = target.chars();
    for fc in filter.chars() {
        if !target_chars.any(|tc| tc == fc) {
            return false;
        }
    }
    true
}

/// Find authors whose name or works match the filter.
/// Returns vec of (author_name, matching_works) tuples.
fn find_matching_authors<'a>(
    groups: &'a [AuthorGroup],
    filter: &str,
) -> Vec<(&'a str, Vec<&'a WorkSummary>)> {
    let filter_lower = filter.to_lowercase();
    let mut result = Vec::new();
    for group in groups {
        let matching_works: Vec<&WorkSummary> = group
            .works
            .iter()
            .filter(|w| subsequence_match(&filter_lower, w))
            .collect();
        if !matching_works.is_empty() {
            result.push((group.author.as_str(), matching_works));
        }
    }
    result
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_subsequence_match_author_name test_filter_finds_works_across_authors`
Expected: PASS

- [ ] **Step 5: Replace populate_list with two-level rendering**

Replace the `populate_list` method entirely:

```rust
pub fn populate_list(&self, filter: &str) {
    while let Some(child) = self.list_box.first_child() {
        self.list_box.remove(&child);
    }

    match &self.level {
        PickerLevel::Authors => {
            if filter.is_empty() {
                // Show all authors with work counts
                for group in &self.groups {
                    self.add_author_row(&group.author, group.works.len());
                }
            } else {
                // Check if filter matches works across authors
                let matching = find_matching_authors(&self.groups, filter);
                if matching.len() == 1 {
                    // Single author match — auto-drill into their works
                    for work in &matching[0].1 {
                        self.add_work_row(work);
                    }
                } else {
                    // Multiple or zero author matches — show filtered author list
                    for (author, works) in &matching {
                        self.add_author_row(author, works.len());
                    }
                }
            }
        }
        PickerLevel::Works { author } => {
            let group = self.groups.iter().find(|g| g.author == *author);
            if let Some(group) = group {
                let filter_lower = filter.to_lowercase();
                for work in &group.works {
                    if filter.is_empty() || subsequence_match(&filter_lower, work) {
                        self.add_work_row(work);
                    }
                }
            }
        }
    }

    if let Some(first) = self.list_box.row_at_index(0) {
        self.list_box.select_row(Some(&first));
    }
}

fn add_author_row(&self, author: &str, count: usize) {
    let hbox = GtkBox::new(Orientation::Horizontal, 8);

    let name_label = Label::builder()
        .label(author)
        .halign(gtk4::Align::Start)
        .build();

    let count_label = Label::builder()
        .label(&format!("({})", count))
        .halign(gtk4::Align::End)
        .hexpand(true)
        .build();
    count_label.add_css_class("picker-item-detail");

    hbox.append(&name_label);
    hbox.append(&count_label);

    let row = ListBoxRow::builder().child(&hbox).build();
    // Store author name with "author:" prefix to distinguish from work abbrevs
    row.set_widget_name(&format!("author:{}", author));
    self.list_box.append(&row);
}

fn add_work_row(&self, work: &WorkSummary) {
    let hbox = GtkBox::new(Orientation::Horizontal, 8);

    let title_label = Label::builder()
        .label(&work.title)
        .halign(gtk4::Align::Start)
        .build();

    let abbrev_label = Label::builder()
        .label(&format!("({})", work.abbrev))
        .halign(gtk4::Align::End)
        .hexpand(true)
        .build();
    abbrev_label.add_css_class("picker-item-detail");

    hbox.append(&title_label);
    hbox.append(&abbrev_label);

    let row = ListBoxRow::builder().child(&hbox).build();
    row.set_widget_name(&work.abbrev);
    self.list_box.append(&row);
}
```

- [ ] **Step 6: Add methods for level navigation**

Add these methods to `impl LibraryPicker`:

```rust
pub fn level(&self) -> &PickerLevel {
    &self.level
}

pub fn enter_author(&mut self) {
    if let Some(row) = self.list_box.selected_row() {
        let name = row.widget_name().to_string();
        if let Some(author) = name.strip_prefix("author:") {
            self.level = PickerLevel::Works {
                author: author.to_string(),
            };
            self.search_entry.set_placeholder_text(Some("Filter works..."));
            self.search_entry.set_text("");
            self.populate_list("");
            self.search_entry.grab_focus();
        }
    }
}

pub fn go_back_to_authors(&mut self) {
    self.level = PickerLevel::Authors;
    self.search_entry.set_placeholder_text(Some("Filter authors..."));
    self.search_entry.set_text("");
    self.populate_list("");
    self.search_entry.grab_focus();
}

pub fn show(&self) {
    self.scrim.set_visible(true);
    self.picker_box.set_visible(true);
    self.search_entry.set_text("");
    self.search_entry.grab_focus();
    self.populate_list("");
}
```

Note: `show` is being redefined here — remove the earlier version from Task 1 Step 9 if not already replaced.

- [ ] **Step 7: Update selected_abbrev to handle both levels**

Replace the `selected_abbrev` method:

```rust
pub fn selected_abbrev(&self) -> Option<String> {
    let row = self.list_box.selected_row()?;
    let name = row.widget_name().to_string();
    // Author rows have "author:" prefix — not a valid abbrev
    if name.starts_with("author:") {
        None
    } else {
        Some(name)
    }
}
```

- [ ] **Step 8: Verify it compiles**

Run: `cargo build`
Expected: May have warnings but should compile. If `show` has a duplicate, remove the older definition.

- [ ] **Step 9: Run all tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 10: Commit**

```bash
git add src/ui/library_picker.rs
git commit -m "feat: implement two-level author/works rendering in library picker"
```

---

### Task 3: Update Theme CSS

**Files:**
- Modify: `src/theme.rs:344-347`

- [ ] **Step 1: Replace hardcoded library-picker CSS with theme variables**

In `src/theme.rs`, in the `generate_css` function, replace these three lines:

```
         .library-picker {{ background-color: rgba(40, 40, 40, 0.95); color: white; \
           padding: 16px; border-radius: 8px; }} \
         .library-picker entry {{ margin-bottom: 8px; }} \
         .library-picker row:selected {{ background-color: rgba(100, 140, 200, 0.8); }} \
```

with:

```
         .library-picker {{ background-color: {bg}; color: {fg}; \
           padding: 16px; border-radius: 12px; border: 1px solid {dim}; }} \
         .library-picker entry {{ margin-bottom: 8px; }} \
         .library-picker row:selected {{ background-color: {cursor_bg}; color: {cursor_fg}; }} \
         .library-picker-scrim {{ background-color: rgba(0, 0, 0, 0.3); }} \
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/theme.rs
git commit -m "feat: use theme colors for library picker styling, add scrim CSS"
```

---

### Task 4: Update Key Handling for Two-Level Navigation

**Files:**
- Modify: `src/input/keymap.rs:84-150`

- [ ] **Step 1: Update Escape handling for level navigation**

In `src/input/keymap.rs`, replace the picker-visible `Escape` match arm (lines 87-90):

```rust
            "Escape" => {
                state.borrow().picker.hide();
                return true;
            }
```

with:

```rust
            "Escape" => {
                let level = state.borrow().picker.level().clone();
                match level {
                    crate::ui::library_picker::PickerLevel::Works { .. } => {
                        state.borrow_mut().picker.go_back_to_authors();
                    }
                    crate::ui::library_picker::PickerLevel::Authors => {
                        state.borrow().picker.hide();
                    }
                }
                return true;
            }
```

- [ ] **Step 2: Update Return handling to drill into authors or load works**

Replace the `Return` match arm (lines 91-137) in the picker-visible block:

```rust
            "Return" => {
                let level = state.borrow().picker.level().clone();
                match level {
                    crate::ui::library_picker::PickerLevel::Authors => {
                        // Check if the selected row is an author or a work
                        // (works appear when filter auto-drills into single author)
                        let selected_name = state
                            .borrow()
                            .picker
                            .list_box()
                            .selected_row()
                            .map(|r| r.widget_name().to_string());
                        if let Some(name) = selected_name {
                            if name.starts_with("author:") {
                                state.borrow_mut().picker.enter_author();
                            } else {
                                // Auto-drilled work row — load it
                                load_selected_work(state, tokio_handle);
                            }
                        }
                        return true;
                    }
                    crate::ui::library_picker::PickerLevel::Works { .. } => {
                        load_selected_work(state, tokio_handle);
                        return true;
                    }
                }
            }
```

- [ ] **Step 3: Add Backspace handling for back navigation**

Add a new match arm in the picker-visible block, after the `"Up"` arm and before the `_ => {}` fallthrough:

```rust
            "BackSpace" => {
                let level = state.borrow().picker.level().clone();
                if let crate::ui::library_picker::PickerLevel::Works { .. } = level {
                    let text = state.borrow().picker.search_entry().text().to_string();
                    if text.is_empty() {
                        state.borrow_mut().picker.go_back_to_authors();
                        return true;
                    }
                }
                // Let GTK handle the backspace in the entry when text is non-empty
                return false;
            }
```

- [ ] **Step 4: Extract load_selected_work helper function**

Add this function near the top of `keymap.rs` (below the imports or just above `handle_key`):

```rust
fn load_selected_work(
    state: &Rc<RefCell<crate::app::AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let abbrev = state.borrow().picker.selected_abbrev();
    if let Some(abbrev) = abbrev {
        {
            let s = state.borrow();
            let _ = s.cmd_tx.try_send(crate::mpv::commands::MpvCommand::Pause);
            s.picker.hide();
            s.correction_overlay.show_loading_message("Loading...");
        }
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let work = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::queries::load_work(&conn, &abbrev)
                })
                .await;
            match work {
                Ok(Ok(work)) => {
                    {
                        let mut s = state_clone.borrow_mut();
                        s.correction_overlay.hide();
                        crate::app::display_work(&mut s, work);
                    }
                    glib::idle_add_local_once(move || {
                        crate::input::navigation::restore_cursor(
                            &mut state_clone.borrow_mut(),
                        );
                    });
                }
                Ok(Err(e)) => {
                    let s = state_clone.borrow();
                    s.correction_overlay.hide();
                    eprintln!("Failed to load work: {}", e);
                }
                Err(e) => {
                    let s = state_clone.borrow();
                    s.correction_overlay.hide();
                    eprintln!("Task join error: {}", e);
                }
            }
        });
    }
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat: add two-level navigation (Escape/Backspace) to library picker"
```

---

### Task 5: Update app.rs Filter Connection

**Files:**
- Modify: `src/app.rs:468-475`

- [ ] **Step 1: Update connect_changed to pass filter to picker**

In `src/app.rs`, the existing `connect_changed` closure (lines 472-475) already calls `populate_list(&text)` which is correct — the new `populate_list` handles both levels. No code change needed here.

However, we need to ensure that the picker's `show()` call on Ctrl+p resets to author level. Check that `show()` resets the level.

- [ ] **Step 2: Add level reset to show()**

In `src/ui/library_picker.rs`, update the `show` method to reset level:

```rust
pub fn show(&self) {
    // Note: level reset requires &mut self — we need to handle this.
    // Since show() takes &self, we reset level in the caller instead.
    self.scrim.set_visible(true);
    self.picker_box.set_visible(true);
    self.search_entry.set_placeholder_text(Some("Filter authors..."));
    self.search_entry.set_text("");
    self.search_entry.grab_focus();
    self.populate_list("");
}
```

We need `show` to also reset the level. Since `level` is not behind interior mutability, change `show` to take `&mut self`:

```rust
pub fn show(&mut self) {
    self.level = PickerLevel::Authors;
    self.scrim.set_visible(true);
    self.picker_box.set_visible(true);
    self.search_entry.set_placeholder_text(Some("Filter authors..."));
    self.search_entry.set_text("");
    self.search_entry.grab_focus();
    self.populate_list("");
}
```

Then update the call site in `src/input/keymap.rs` line 80 from:

```rust
state.borrow().picker.show();
```

to:

```rust
state.borrow_mut().picker.show();
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: PASS

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add src/ui/library_picker.rs src/input/keymap.rs src/app.rs
git commit -m "feat: reset picker to author level on show, wire up filter"
```

---

### Task 6: Final Integration and Cleanup

**Files:**
- Modify: `src/ui/library_picker.rs` (if needed)
- Modify: `src/input/keymap.rs` (if needed)

- [ ] **Step 1: Make PickerLevel derive Clone for keymap usage**

Ensure `PickerLevel` has `Clone` derived (already added in Task 1 Step 3 with `#[derive(Debug, Clone, PartialEq)]`). Verify the `level()` method returns a reference and keymap clones it.

- [ ] **Step 2: Ensure Ctrl+n/Ctrl+p still work for list navigation**

The existing Ctrl+n/Ctrl+p handling at lines 28-40 calls `move_selection()` which operates on the `list_box` directly — this works unchanged for both author and work rows.

- [ ] **Step 3: Ensure j/k navigation still works if applicable**

Check if j/k are handled for picker navigation. If not, no changes needed. The Down/Up arrow keys at lines 139-146 already call `move_selection()` which works unchanged.

- [ ] **Step 4: Run full test suite and clippy**

Run: `cargo test && cargo clippy`
Expected: All pass, no warnings

- [ ] **Step 5: Commit any final fixes**

```bash
git add -u
git commit -m "chore: final cleanup for two-level library picker"
```
