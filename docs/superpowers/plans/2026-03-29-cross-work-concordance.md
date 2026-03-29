# Cross-Work Concordance Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add cross-work concordance mode to linux-lit: pick a vocab word via Ctrl+Shift+p, navigate all its occurrences across works with r/R, pre-load next work in background, show position in a bottom status bar.

**Architecture:** New `ConcordanceState` in `AppState` holds the word, occurrence list, current index, and preloaded work. Two new picker overlays (word picker + occurrence list), a bottom status bar widget, a new DB query module, and modified r/R key handling that branches on concordance mode. Pre-loading uses the existing Tokio background thread via `spawn_blocking`.

**Tech Stack:** Rust, GTK4 0.9, rusqlite 0.33, Tokio 1.x

**Spec:** `docs/superpowers/specs/2026-03-29-cross-work-concordance-design.md`

---

## File Structure

**New files:**
- `src/concordance.rs` -- `ConcordanceState`, `ConcordanceHit`, `PreloadedWork` structs and state management logic
- `src/db/concordance.rs` -- cross-work occurrence query
- `src/ui/concordance_word_picker.rs` -- Ctrl+Shift+p word picker overlay
- `src/ui/concordance_list_picker.rs` -- Ctrl+Alt+p occurrence list overlay
- `src/ui/concordance_bar.rs` -- bottom status bar widget

**Modified files:**
- `src/app.rs` -- add `ConcordanceState` to `AppState`, add bar + pickers to window
- `src/input/keymap.rs` -- Ctrl+Shift+p, Ctrl+Alt+p bindings, r/R concordance branching
- `src/input/navigation.rs` -- concordance jump functions
- `src/db/mod.rs` -- add concordance module
- `src/ui/mod.rs` -- add new UI modules
- `src/main.rs` -- (no changes needed -- existing Tokio setup sufficient)

---

### Task 1: Add concordance data model and DB query

**Files:**
- Create: `src/concordance.rs`
- Create: `src/db/concordance.rs`
- Modify: `src/db/mod.rs`
- Modify: `src/main.rs` (add module declaration)

- [ ] **Step 1: Create the concordance data model**

Create `src/concordance.rs`:

```rust
use crate::db::models::Work;

/// A single occurrence of a word in a work's line_mapping.
#[derive(Debug, Clone)]
pub struct ConcordanceHit {
    pub work_abbrev: String,
    pub work_title: String,
    pub author: String,
    pub line_mapping_id: i64,
    pub div1: i64,
    pub div2: i64,
    pub line_in_div: i64,
    pub canonical_text: String,
    pub has_audio: bool,
}

/// Pre-loaded work data, ready to swap into AppState.
pub struct PreloadedWork {
    pub work_abbrev: String,
    pub work: Work,
}

/// Cross-work concordance navigation state.
pub struct ConcordanceState {
    pub word: String,
    pub occurrences: Vec<ConcordanceHit>,
    pub current_index: usize,
    pub preloaded_work: Option<PreloadedWork>,
}

impl ConcordanceState {
    pub fn new(word: String, occurrences: Vec<ConcordanceHit>) -> Self {
        Self {
            word,
            occurrences,
            current_index: 0,
            preloaded_work: None,
        }
    }

    /// Work abbreviation of the current occurrence.
    pub fn current_work_abbrev(&self) -> Option<&str> {
        self.occurrences.get(self.current_index).map(|h| h.work_abbrev.as_str())
    }

    /// Work abbreviation of the next occurrence in a given direction.
    /// direction: 1 for forward, -1 for backward.
    pub fn next_work_abbrev(&self, direction: i32) -> Option<&str> {
        let next = if direction > 0 {
            if self.current_index + 1 < self.occurrences.len() {
                self.current_index + 1
            } else {
                return None;
            }
        } else {
            if self.current_index > 0 {
                self.current_index - 1
            } else {
                return None;
            }
        };
        self.occurrences.get(next).map(|h| h.work_abbrev.as_str())
    }

    /// Advance index forward. Returns false if already at the end.
    pub fn advance(&mut self) -> bool {
        if self.current_index + 1 < self.occurrences.len() {
            self.current_index += 1;
            true
        } else {
            false
        }
    }

    /// Move index backward. Returns false if already at the start.
    pub fn retreat(&mut self) -> bool {
        if self.current_index > 0 {
            self.current_index -= 1;
            true
        } else {
            false
        }
    }

    /// Current hit, if any.
    pub fn current_hit(&self) -> Option<&ConcordanceHit> {
        self.occurrences.get(self.current_index)
    }

    /// Format status bar text: "disapprobation [3/13]"
    pub fn status_label(&self) -> String {
        format!(
            "{} [{}/{}]",
            self.word,
            self.current_index + 1,
            self.occurrences.len(),
        )
    }

    /// Format status bar work info: "Boswell, Life of Johnson"
    pub fn status_work(&self) -> String {
        match self.current_hit() {
            Some(hit) => {
                let author = shorten_author(&hit.author);
                let title = shorten_title(&hit.work_title);
                format!("{}, {}", author, title)
            }
            None => String::new(),
        }
    }
}

fn shorten_author(author: &str) -> &str {
    if let Some(idx) = author.find(',') {
        &author[..idx]
    } else {
        author.rsplit_once(' ').map(|(_, last)| last).unwrap_or(author)
    }
}

fn shorten_title(title: &str) -> &str {
    let t = title.split(':').next().unwrap_or(title).trim();
    let t = t.strip_prefix("The ").unwrap_or(t);
    if t.len() > 25 {
        &t[..t[..25].rfind(' ').unwrap_or(25)]
    } else {
        t
    }
}
```

- [ ] **Step 2: Create the DB query module**

Create `src/db/concordance.rs`:

```rust
use rusqlite::Connection;

use super::models::Work;

/// A hit from the cross-work concordance search.
#[derive(Debug, Clone)]
pub struct ConcordanceRow {
    pub line_mapping_id: i64,
    pub work_abbrev: String,
    pub title: String,
    pub author: String,
    pub div1: i64,
    pub div2: i64,
    pub line_in_div: i64,
    pub canonical_text: String,
    pub has_audio: bool,
}

/// Find all lines containing `word` across all works with line_mapping entries.
/// Results ordered by author, work, position.
pub fn find_word_occurrences(
    conn: &Connection,
    word: &str,
) -> Result<Vec<ConcordanceRow>, rusqlite::Error> {
    let pattern = format!("%{}%", word.to_lowercase());
    let mut stmt = conn.prepare(
        "SELECT lm.id, lm.work_abbrev, w.title, w.author,
                lm.div1, COALESCE(lm.div2, 0), lm.line_in_div, lm.canonical_text,
                EXISTS(
                    SELECT 1 FROM line_timestamps lt WHERE lt.line_mapping_id = lm.id
                ) AS has_audio
         FROM line_mapping lm
         JOIN works w ON w.abbrev = lm.work_abbrev
         WHERE lm.normalized_text LIKE ?1
         ORDER BY w.author, lm.work_abbrev, lm.div1, COALESCE(lm.div2, 0), lm.line_in_div",
    )?;
    let rows = stmt.query_map([&pattern], |row| {
        Ok(ConcordanceRow {
            line_mapping_id: row.get(0)?,
            work_abbrev: row.get(1)?,
            title: row.get(2)?,
            author: row.get(3)?,
            div1: row.get(4)?,
            div2: row.get(5)?,
            line_in_div: row.get(6)?,
            canonical_text: row.get(7)?,
            has_audio: row.get::<_, i64>(8)? != 0,
        })
    })?;
    rows.collect()
}

/// Load all vocab words globally (for the cross-work concordance word picker).
/// Returns (word, total_occurrence_count) across all works.
pub fn load_global_vocab_words(
    conn: &Connection,
) -> Result<Vec<(String, usize)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT vw.word, COUNT(DISTINCT lm.id) AS cnt
         FROM vocab_words vw
         JOIN line_mapping lm ON lm.normalized_text LIKE '%' || LOWER(vw.word) || '%'
         GROUP BY vw.word
         ORDER BY vw.word",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
    })?;
    rows.collect()
}
```

- [ ] **Step 3: Register the new modules**

Add to `src/db/mod.rs` after the existing module declarations:

```rust
pub mod concordance;
```

Add to `src/main.rs` after existing module declarations (near the top, where `mod app;`, `mod config;`, etc. are listed):

```rust
mod concordance;
```

- [ ] **Step 4: Verify it compiles**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -5`
Expected: `Finished` with no errors (warnings about unused imports/functions are OK at this stage)

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit
git add src/concordance.rs src/db/concordance.rs src/db/mod.rs src/main.rs
git commit -m "Add concordance data model and cross-work DB query"
```

---

### Task 2: Add concordance status bar widget

**Files:**
- Create: `src/ui/concordance_bar.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create the status bar widget**

Create `src/ui/concordance_bar.rs`:

```rust
use gtk4::prelude::*;
use gtk4::{Align, GtkBox, Label, Orientation};

/// Bottom status bar showing concordance mode state.
pub struct ConcordanceBar {
    pub container: GtkBox,
    word_label: Label,
    position_label: Label,
    hint_label: Label,
}

impl ConcordanceBar {
    pub fn new() -> Self {
        let container = GtkBox::new(Orientation::Horizontal, 0);
        container.set_hexpand(true);
        container.set_visible(false);
        container.add_css_class("concordance-bar");

        let word_label = Label::new(None);
        word_label.set_halign(Align::Start);
        word_label.set_hexpand(true);
        word_label.add_css_class("concordance-bar-word");

        let position_label = Label::new(None);
        position_label.set_halign(Align::Center);
        position_label.set_hexpand(true);
        position_label.add_css_class("concordance-bar-position");

        let hint_label = Label::new(Some("r/R: next/prev | Esc: exit"));
        hint_label.set_halign(Align::End);
        hint_label.set_hexpand(true);
        hint_label.add_css_class("concordance-bar-hint");

        container.append(&word_label);
        container.append(&position_label);
        container.append(&hint_label);

        Self {
            container,
            word_label,
            position_label,
            hint_label,
        }
    }

    pub fn update(&self, word: &str, position: &str) {
        self.word_label.set_markup(&format!(
            "concordance: <span foreground=\"#fabd2f\">{}</span>",
            glib::markup_escape_text(word),
        ));
        self.position_label.set_markup(&format!(
            "<span foreground=\"#83a598\">{}</span>",
            glib::markup_escape_text(position),
        ));
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    /// Generate CSS for the bar based on theme colors.
    pub fn css(bg: &str, fg: &str) -> String {
        format!(
            ".concordance-bar {{ background: {}; padding: 4px 12px; }}
             .concordance-bar-word {{ color: {}; font-family: monospace; font-size: 12px; }}
             .concordance-bar-position {{ color: {}; font-family: monospace; font-size: 12px; }}
             .concordance-bar-hint {{ color: {}; font-family: monospace; font-size: 12px; opacity: 0.6; }}",
            bg, fg, fg, fg,
        )
    }
}
```

- [ ] **Step 2: Register the module**

Add to `src/ui/mod.rs`:

```rust
pub mod concordance_bar;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -5`
Expected: `Finished` with no errors

- [ ] **Step 4: Commit**

```bash
cd ~/utono/linux-lit
git add src/ui/concordance_bar.rs src/ui/mod.rs
git commit -m "Add concordance status bar widget"
```

---

### Task 3: Add concordance word picker (Ctrl+Shift+p)

**Files:**
- Create: `src/ui/concordance_word_picker.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create the word picker**

Create `src/ui/concordance_word_picker.rs`. This follows the same pattern as `library_picker.rs` and `concordance_picker.rs`:

```rust
use gtk4::prelude::*;
use gtk4::{Align, Entry, GtkBox, Label, ListBox, ListBoxRow, Orientation, Overlay, ScrolledWindow};

/// Picker for selecting a vocab word for cross-work concordance.
pub struct ConcordanceWordPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    words: Vec<(String, usize)>,
}

impl ConcordanceWordPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = GtkBox::new(Orientation::Vertical, 0);
        picker_box.set_halign(Align::Center);
        picker_box.set_valign(Align::Start);
        picker_box.set_margin_top(40);
        picker_box.set_width_request(450);
        picker_box.add_css_class("picker-box");

        let search_entry = Entry::new();
        search_entry.set_placeholder_text(Some("Search vocab words..."));
        search_entry.add_css_class("picker-entry");
        picker_box.append(&search_entry);

        let scrolled = ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_max_content_height(400);
        scrolled.set_propagate_natural_height(true);

        let list_box = ListBox::new();
        list_box.add_css_class("picker-list");
        scrolled.set_child(Some(&list_box));
        picker_box.append(&scrolled);

        Self {
            overlay,
            picker_box,
            search_entry,
            list_box,
            words: Vec::new(),
        }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.picker_box);
        self.picker_box.set_visible(false);
    }

    pub fn show(&self) {
        self.search_entry.set_text("");
        self.populate_list("");
        self.picker_box.set_visible(true);
        self.search_entry.grab_focus();
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.picker_box.is_visible()
    }

    pub fn set_words(&mut self, words: Vec<(String, usize)>) {
        self.words = words;
    }

    pub fn filter_changed(&self) {
        let filter = self.search_entry.text().to_string();
        self.populate_list(&filter);
    }

    fn populate_list(&self, filter: &str) {
        // Remove existing rows
        while let Some(row) = self.list_box.first_child() {
            self.list_box.remove(&row);
        }

        let filter_lower = filter.to_lowercase();
        for (word, count) in &self.words {
            if !filter_lower.is_empty() && !word.contains(&filter_lower) {
                continue;
            }

            let row_box = GtkBox::new(Orientation::Horizontal, 8);
            row_box.set_margin_start(8);
            row_box.set_margin_end(8);
            row_box.set_margin_top(2);
            row_box.set_margin_bottom(2);

            let word_label = Label::new(Some(word));
            word_label.set_halign(Align::Start);
            word_label.set_hexpand(true);
            word_label.add_css_class("picker-item-title");

            let count_label = Label::new(Some(&format!("{} across all works", count)));
            count_label.set_halign(Align::End);
            count_label.add_css_class("picker-item-detail");

            row_box.append(&word_label);
            row_box.append(&count_label);

            let row = ListBoxRow::new();
            row.set_child(Some(&row_box));
            row.set_widget_name(word);
            self.list_box.append(&row);
        }

        // Select first row
        if let Some(row) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&row));
        }
    }

    pub fn selected_word(&self) -> Option<String> {
        self.list_box
            .selected_row()
            .map(|row| row.widget_name().to_string())
    }

    pub fn move_selection(&self, delta: i32) {
        let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
        let next = current + delta;
        if let Some(row) = self.list_box.row_at_index(next) {
            self.list_box.select_row(Some(&row));
        }
    }

    pub fn entry(&self) -> &Entry {
        &self.search_entry
    }
}
```

- [ ] **Step 2: Register the module**

Add to `src/ui/mod.rs`:

```rust
pub mod concordance_word_picker;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -5`
Expected: `Finished`

- [ ] **Step 4: Commit**

```bash
cd ~/utono/linux-lit
git add src/ui/concordance_word_picker.rs src/ui/mod.rs
git commit -m "Add concordance word picker for Ctrl+Shift+p"
```

---

### Task 4: Add concordance occurrence list picker (Ctrl+Alt+p)

**Files:**
- Create: `src/ui/concordance_list_picker.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create the occurrence list picker**

Create `src/ui/concordance_list_picker.rs`:

```rust
use gtk4::prelude::*;
use gtk4::{Align, GtkBox, Label, ListBox, ListBoxRow, Orientation, Overlay, ScrolledWindow};

use crate::concordance::ConcordanceHit;

/// Picker for jumping to a specific occurrence in cross-work concordance.
pub struct ConcordanceListPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    list_box: ListBox,
}

impl ConcordanceListPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = GtkBox::new(Orientation::Vertical, 0);
        picker_box.set_halign(Align::Center);
        picker_box.set_valign(Align::Start);
        picker_box.set_margin_top(40);
        picker_box.set_width_request(600);
        picker_box.add_css_class("picker-box");

        let header = Label::new(Some("Concordance occurrences"));
        header.add_css_class("picker-header");
        header.set_margin_top(8);
        header.set_margin_bottom(4);
        picker_box.append(&header);

        let scrolled = ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_max_content_height(500);
        scrolled.set_propagate_natural_height(true);

        let list_box = ListBox::new();
        list_box.add_css_class("picker-list");
        scrolled.set_child(Some(&list_box));
        picker_box.append(&scrolled);

        Self {
            overlay,
            picker_box,
            list_box,
        }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.picker_box);
        self.picker_box.set_visible(false);
    }

    pub fn show(&self, hits: &[ConcordanceHit], current_index: usize) {
        // Remove existing rows
        while let Some(row) = self.list_box.first_child() {
            self.list_box.remove(&row);
        }

        for (i, hit) in hits.iter().enumerate() {
            let row_box = GtkBox::new(Orientation::Vertical, 2);
            row_box.set_margin_start(8);
            row_box.set_margin_end(8);
            row_box.set_margin_top(4);
            row_box.set_margin_bottom(4);

            // Top line: author, title
            let author = shorten_author(&hit.author);
            let title = shorten_title(&hit.work_title);
            let header = Label::new(Some(&format!("{}, {} [{}.{}]", author, title, hit.div1, hit.line_in_div)));
            header.set_halign(Align::Start);
            header.add_css_class("picker-item-title");

            // Bottom line: snippet
            let snippet = truncate_around_center(&hit.canonical_text, 80);
            let detail = Label::new(Some(&snippet));
            detail.set_halign(Align::Start);
            detail.add_css_class("picker-item-detail");
            detail.set_ellipsize(pango::EllipsizeMode::End);

            row_box.append(&header);
            row_box.append(&detail);

            let row = ListBoxRow::new();
            row.set_child(Some(&row_box));
            // Store index as widget name for retrieval
            row.set_widget_name(&i.to_string());
            self.list_box.append(&row);
        }

        // Select current occurrence
        if let Some(row) = self.list_box.row_at_index(current_index as i32) {
            self.list_box.select_row(Some(&row));
        }

        self.picker_box.set_visible(true);
        self.list_box.grab_focus();
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.picker_box.is_visible()
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.list_box
            .selected_row()
            .and_then(|row| row.widget_name().parse::<usize>().ok())
    }

    pub fn move_selection(&self, delta: i32) {
        let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
        let next = current + delta;
        if let Some(row) = self.list_box.row_at_index(next) {
            self.list_box.select_row(Some(&row));
        }
    }
}

fn shorten_author(author: &str) -> &str {
    if let Some(idx) = author.find(',') {
        &author[..idx]
    } else {
        author.rsplit_once(' ').map(|(_, last)| last).unwrap_or(author)
    }
}

fn shorten_title(title: &str) -> &str {
    let t = title.split(':').next().unwrap_or(title).trim();
    t.strip_prefix("The ").unwrap_or(t)
}

fn truncate_around_center(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}...", &text[..max_len])
    }
}
```

- [ ] **Step 2: Register the module**

Add to `src/ui/mod.rs`:

```rust
pub mod concordance_list_picker;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -5`
Expected: `Finished`

- [ ] **Step 4: Commit**

```bash
cd ~/utono/linux-lit
git add src/ui/concordance_list_picker.rs src/ui/mod.rs
git commit -m "Add concordance occurrence list picker for Ctrl+Alt+p"
```

---

### Task 5: Integrate into AppState and window layout

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add fields to AppState**

In `src/app.rs`, add these fields to the `AppState` struct (after the `concordance_picker` field, which is the last field):

```rust
    pub concordance_state: Option<crate::concordance::ConcordanceState>,
    pub concordance_word_picker: crate::ui::concordance_word_picker::ConcordanceWordPicker,
    pub concordance_list_picker: crate::ui::concordance_list_picker::ConcordanceListPicker,
    pub concordance_bar: crate::ui::concordance_bar::ConcordanceBar,
```

- [ ] **Step 2: Create widgets and wire into overlay chain in build_window**

In `build_window`, the overlay chain currently ends with `ConcordancePicker` wrapping `CorrectionOverlay`. The new pickers need to be inserted into the chain. Find the section where overlays are stacked (around lines 267-316).

After the existing `concordance_picker` creation and before the action popup, create the new widgets:

```rust
    let concordance_word_picker = crate::ui::concordance_word_picker::ConcordanceWordPicker::new();
    concordance_word_picker.attach(&concordance_picker.overlay);

    let concordance_list_picker = crate::ui::concordance_list_picker::ConcordanceListPicker::new();
    concordance_list_picker.attach(&concordance_word_picker.overlay);

    let concordance_bar = crate::ui::concordance_bar::ConcordanceBar::new();
```

Update the action popup to attach to `concordance_list_picker.overlay` instead of `concordance_picker.overlay`.

Add the concordance bar to the main VBox layout (the container that holds the scrolled window and search bar). Insert the bar between the scrolled window area and the search bar:

```rust
    main_vbox.append(&concordance_bar.container);
```

- [ ] **Step 3: Initialize fields in AppState construction**

In the `AppState { ... }` struct literal inside `build_window`, add:

```rust
    concordance_state: None,
    concordance_word_picker,
    concordance_list_picker,
    concordance_bar,
```

- [ ] **Step 4: Verify it compiles**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -5`
Expected: `Finished`

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit
git add src/app.rs
git commit -m "Integrate concordance state, pickers, and bar into AppState"
```

---

### Task 6: Add Ctrl+Shift+p key binding (open word picker)

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Add the Ctrl+Shift+p handler**

In `keymap.rs`, find the section where Ctrl key combinations are handled. Near the existing `Ctrl+p` handler (around line 43), add a handler for Ctrl+Shift+p. The key name for `p` with Shift held is still `"p"` -- you distinguish via `is_shift`:

```rust
    // Ctrl+Shift+p — open concordance word picker
    if is_ctrl && is_shift && key_name == "p" {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let words = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::concordance::load_global_vocab_words(&conn)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            let mut s = state_clone.borrow_mut();
            s.concordance_word_picker.set_words(words);
            s.concordance_word_picker.show();
        });
        return true;
    }
```

**Important**: This must come **before** the existing `Ctrl+p` handler so that `Ctrl+Shift+p` doesn't fall through to the library picker. The `is_shift` check distinguishes them.

- [ ] **Step 2: Add word picker navigation keys**

Add a guard block early in `handle_key` (near the existing picker-visible guards) for when the word picker is visible:

```rust
    let conc_word_picker_visible = state.borrow().concordance_word_picker.is_visible();
    if conc_word_picker_visible {
        match key_name {
            "Escape" => {
                state.borrow().concordance_word_picker.hide();
                return true;
            }
            "Return" => {
                let selected = state.borrow().concordance_word_picker.selected_word();
                state.borrow().concordance_word_picker.hide();
                if let Some(word) = selected {
                    let state_clone = Rc::clone(state);
                    let handle = tokio_handle.clone();
                    let word_clone = word.clone();
                    glib::spawn_future_local(async move {
                        let hits = handle
                            .spawn_blocking(move || {
                                let conn = crate::db::queries::open_db()
                                    .expect("Failed to open lit.db");
                                crate::db::concordance::find_word_occurrences(&conn, &word_clone)
                                    .unwrap_or_default()
                            })
                            .await
                            .unwrap_or_default();
                        if hits.is_empty() {
                            return;
                        }
                        let conc_hits: Vec<crate::concordance::ConcordanceHit> = hits
                            .into_iter()
                            .map(|h| crate::concordance::ConcordanceHit {
                                work_abbrev: h.work_abbrev,
                                work_title: h.title,
                                author: h.author,
                                line_mapping_id: h.line_mapping_id,
                                div1: h.div1,
                                div2: h.div2,
                                line_in_div: h.line_in_div,
                                canonical_text: h.canonical_text,
                                has_audio: h.has_audio,
                            })
                            .collect();
                        let conc_state = crate::concordance::ConcordanceState::new(
                            word.clone(),
                            conc_hits,
                        );
                        let mut s = state_clone.borrow_mut();
                        // Update bar
                        s.concordance_bar.update(&conc_state.status_label(), &conc_state.status_work());
                        s.concordance_state = Some(conc_state);
                        // Jump to first occurrence
                        drop(s);
                        navigation::concordance_jump_to_current(&state_clone, &handle);
                    });
                }
                return true;
            }
            _ => {
                if is_ctrl && key_name == "n" {
                    state.borrow().concordance_word_picker.move_selection(1);
                    return true;
                }
                if is_ctrl && key_name == "p" {
                    state.borrow().concordance_word_picker.move_selection(-1);
                    return true;
                }
                // Let entry handle text input
                return false;
            }
        }
    }
```

- [ ] **Step 3: Connect search entry changed signal**

In `build_window` in `app.rs`, after creating the concordance word picker, connect the entry's changed signal:

```rust
    {
        let state_ref = Rc::clone(&state_rc);
        concordance_word_picker.entry().connect_changed(move |_| {
            state_ref.borrow().concordance_word_picker.filter_changed();
        });
    }
```

(Where `state_rc` is the `Rc<RefCell<AppState>>` — follow the pattern used by the existing library picker's entry connection.)

- [ ] **Step 4: Verify it compiles**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -5`
Expected: `Finished`

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit
git add src/input/keymap.rs src/app.rs
git commit -m "Add Ctrl+Shift+p binding to open concordance word picker"
```

---

### Task 7: Add concordance navigation (r/R overload)

**Files:**
- Modify: `src/input/navigation.rs`
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Add concordance jump functions to navigation.rs**

Add to `src/input/navigation.rs`:

```rust
use crate::concordance::ConcordanceState;

/// Jump to the current concordance occurrence.
/// Loads the work if different from current, positions cursor on the line.
pub fn concordance_jump_to_current(
    state: &Rc<RefCell<AppState>>,
    handle: &tokio::runtime::Handle,
) {
    let (target_abbrev, target_line_id) = {
        let s = state.borrow();
        let conc = match &s.concordance_state {
            Some(c) => c,
            None => return,
        };
        let hit = match conc.current_hit() {
            Some(h) => h,
            None => return,
        };
        (hit.work_abbrev.clone(), hit.line_mapping_id)
    };

    let current_abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());

    if current_abbrev.as_deref() != Some(&target_abbrev) {
        // Need to load a different work
        let state_clone = Rc::clone(state);
        let handle_clone = handle.clone();
        let abbrev = target_abbrev.clone();

        // Check if preloaded work matches
        let preloaded = {
            let mut s = state_clone.borrow_mut();
            if let Some(conc) = &mut s.concordance_state {
                if conc
                    .preloaded_work
                    .as_ref()
                    .map(|p| p.work_abbrev == abbrev)
                    .unwrap_or(false)
                {
                    conc.preloaded_work.take()
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(preloaded) = preloaded {
            // Use preloaded work
            let mut s = state_clone.borrow_mut();
            crate::app::display_work(&mut s, preloaded.work);
            concordance_position_cursor(&mut s, target_line_id);
            concordance_update_bar(&s);
            concordance_preload_next(&state_clone, &handle_clone);
        } else {
            // Synchronous load via spawn_blocking
            glib::spawn_future_local(async move {
                let work = handle_clone
                    .spawn_blocking(move || {
                        let conn =
                            crate::db::queries::open_db().expect("Failed to open lit.db");
                        crate::db::queries::load_work(&conn, &abbrev).ok()
                    })
                    .await
                    .unwrap_or(None);
                if let Some(work) = work {
                    let mut s = state_clone.borrow_mut();
                    crate::app::display_work(&mut s, work);
                    concordance_position_cursor(&mut s, target_line_id);
                    concordance_update_bar(&s);
                    drop(s);
                    concordance_preload_next(&state_clone, &handle_clone);
                }
            });
        }
    } else {
        // Same work, just move cursor
        let mut s = state.borrow_mut();
        concordance_position_cursor(&mut s, target_line_id);
        concordance_update_bar(&s);
        drop(s);
        concordance_preload_next(state, handle);
    }
}

/// Position cursor on the line with the given line_mapping_id.
fn concordance_position_cursor(state: &mut AppState, line_mapping_id: i64) {
    if let Some(work) = &state.current_work {
        if let Some(idx) = work.lines.iter().position(|l| l.id == line_mapping_id) {
            state.current_line = idx;
            update_highlight(state);
            center_cursor(state);
            seek_to_current_line(state);
        }
    }
}

/// Update the concordance status bar from current state.
fn concordance_update_bar(state: &AppState) {
    if let Some(conc) = &state.concordance_state {
        state
            .concordance_bar
            .update(&conc.status_label(), &conc.status_work());
    }
}

/// Kick off background preload of the next work in the concordance direction.
pub fn concordance_preload_next(
    state: &Rc<RefCell<AppState>>,
    handle: &tokio::runtime::Handle,
) {
    let next_abbrev = {
        let s = state.borrow();
        let conc = match &s.concordance_state {
            Some(c) => c,
            None => return,
        };
        // Preload in forward direction
        match conc.next_work_abbrev(1) {
            Some(a) if Some(a) != s.current_work.as_ref().map(|w| w.abbrev.as_str()) => {
                a.to_string()
            }
            _ => return,
        }
    };

    let state_clone = Rc::clone(state);
    let handle_clone = handle.clone();
    let abbrev = next_abbrev;
    glib::spawn_future_local(async move {
        let work = handle_clone
            .spawn_blocking(move || {
                let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                crate::db::queries::load_work(&conn, &abbrev).ok()
            })
            .await
            .unwrap_or(None);
        if let Some(work) = work {
            let mut s = state_clone.borrow_mut();
            if let Some(conc) = &mut s.concordance_state {
                conc.preloaded_work = Some(crate::concordance::PreloadedWork {
                    work_abbrev: work.abbrev.clone(),
                    work,
                });
            }
        }
    });
}
```

- [ ] **Step 2: Modify r/R handling in keymap.rs**

Replace the existing `"r"` and `"R"` handlers (around lines 870-876) with concordance-aware versions:

```rust
"r" => {
    let has_concordance = state.borrow().concordance_state.is_some();
    if has_concordance {
        let advanced = {
            let mut s = state.borrow_mut();
            s.concordance_state.as_mut().map(|c| c.advance()).unwrap_or(false)
        };
        if advanced {
            navigation::concordance_jump_to_current(state, tokio_handle);
        }
    } else {
        navigation::jump_to_next_vocab(&mut state.borrow_mut());
    }
    true
}
"R" => {
    let has_concordance = state.borrow().concordance_state.is_some();
    if has_concordance {
        let retreated = {
            let mut s = state.borrow_mut();
            s.concordance_state.as_mut().map(|c| c.retreat()).unwrap_or(false)
        };
        if retreated {
            navigation::concordance_jump_to_current(state, tokio_handle);
        }
    } else {
        navigation::jump_to_prev_vocab(&mut state.borrow_mut());
    }
    true
}
```

- [ ] **Step 3: Add Escape handler to clear concordance mode**

In the Escape key handler in `keymap.rs`, add concordance clearing. Find the existing Escape handling and add before other escape actions:

```rust
    // Clear concordance mode on Escape
    {
        let has_conc = state.borrow().concordance_state.is_some();
        if has_conc && key_name == "Escape" {
            let mut s = state.borrow_mut();
            s.concordance_state = None;
            s.concordance_bar.hide();
            return true;
        }
    }
```

- [ ] **Step 4: Clear concordance on Ctrl+p (library picker)**

In the Ctrl+p handler that opens the library picker, add concordance clearing:

```rust
    // When opening library picker, clear concordance mode
    let mut s = state.borrow_mut();
    s.concordance_state = None;
    s.concordance_bar.hide();
    drop(s);
```

- [ ] **Step 5: Verify it compiles**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -5`
Expected: `Finished`

- [ ] **Step 6: Commit**

```bash
cd ~/utono/linux-lit
git add src/input/navigation.rs src/input/keymap.rs
git commit -m "Add r/R concordance navigation with preloading"
```

---

### Task 8: Add Ctrl+Alt+p key binding (occurrence list picker)

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Add the Ctrl+Alt+p handler**

In `keymap.rs`, in the section where Ctrl+Alt combinations would be handled, add:

```rust
    // Ctrl+Alt+p — open concordance occurrence list
    if is_ctrl && is_alt && key_name == "p" {
        let s = state.borrow();
        if let Some(conc) = &s.concordance_state {
            s.concordance_list_picker
                .show(&conc.occurrences, conc.current_index);
        }
        return true;
    }
```

- [ ] **Step 2: Add list picker navigation keys**

Add a guard block for when the list picker is visible:

```rust
    let conc_list_picker_visible = state.borrow().concordance_list_picker.is_visible();
    if conc_list_picker_visible {
        match key_name {
            "Escape" => {
                state.borrow().concordance_list_picker.hide();
                return true;
            }
            "Return" => {
                let selected = state.borrow().concordance_list_picker.selected_index();
                state.borrow().concordance_list_picker.hide();
                if let Some(idx) = selected {
                    {
                        let mut s = state.borrow_mut();
                        if let Some(conc) = &mut s.concordance_state {
                            conc.current_index = idx;
                        }
                    }
                    navigation::concordance_jump_to_current(state, tokio_handle);
                }
                return true;
            }
            "j" | "n" => {
                state.borrow().concordance_list_picker.move_selection(1);
                return true;
            }
            "k" | "p" => {
                if !is_ctrl {
                    state.borrow().concordance_list_picker.move_selection(-1);
                    return true;
                }
                return false;
            }
            _ => {
                if is_ctrl && key_name == "n" {
                    state.borrow().concordance_list_picker.move_selection(1);
                    return true;
                }
                if is_ctrl && key_name == "p" {
                    state.borrow().concordance_list_picker.move_selection(-1);
                    return true;
                }
                return false;
            }
        }
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -5`
Expected: `Finished`

- [ ] **Step 4: Commit**

```bash
cd ~/utono/linux-lit
git add src/input/keymap.rs
git commit -m "Add Ctrl+Alt+p binding for concordance occurrence list"
```

---

### Task 9: Manual smoke test

**Files:** None (testing only)

- [ ] **Step 1: Build and run**

```bash
cd ~/utono/linux-lit && cargo build && cargo run
```

- [ ] **Step 2: Test Ctrl+Shift+p word picker**

1. Press Ctrl+Shift+p
2. Type "disapprobation" to filter
3. Press Enter to select

Expected: work loads, cursor positioned on first occurrence, bottom bar shows `concordance: disapprobation [1/N] Author, Title`

- [ ] **Step 3: Test r/R navigation**

1. Press `r` to advance to next occurrence
2. If next occurrence is in a different work, the work should load and cursor should jump to the line
3. Press `R` to go back
4. Status bar should update on each jump

- [ ] **Step 4: Test Ctrl+Alt+p occurrence list**

1. Press Ctrl+Alt+p
2. Scroll through the list of occurrences
3. Select one and press Enter
4. Should jump to that occurrence

- [ ] **Step 5: Test Escape to exit**

1. Press Escape
2. Status bar should disappear
3. r/R should revert to within-work vocab navigation

- [ ] **Step 6: Test Ctrl+p clears concordance**

1. Enter concordance mode again via Ctrl+Shift+p
2. Press Ctrl+p to open library picker
3. Concordance mode should be cleared
