# linux-lit Phase 2: Database & Work Loading

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Load literary works from SQLite, classify lines as dialogue/non-dialogue, display text in the GtkTextView, and provide a library picker overlay for selecting works.

**Architecture:** Database queries run via `tokio::task::spawn_blocking` on the Tokio runtime to avoid blocking the GTK loop. Work data (lines, timestamps, media paths) is loaded into memory structs. The library picker is a modal overlay widget with fuzzy filtering. On work selection, the GTK buffer is populated with all lines.

**Tech Stack:** rusqlite (read-only SQLite), regex (line classification), gtk4-rs (overlay/UI)

**Depends on:** Phase 1 (complete) — GTK4 window, text view, Tokio runtime, channel bridge

---

## File Structure

```
~/utono/linux-lit/src/
  db/
    mod.rs              # Re-exports
    models.rs           # Work, Line, TimeRange, Timestamp structs
    line_types.rs       # Dialogue classification (is_speaker, is_stage_direction, etc.)
    queries.rs          # SQLite connection, list_works, load_work
  ui/
    mod.rs              # Re-exports
    library_picker.rs   # Ctrl+p overlay with fuzzy filter, j/k navigation
  app.rs                # Modified: integrate library picker, populate buffer on work load
  main.rs               # Modified: wire database loading through Tokio runtime
```

## Key Design Decisions

- **`regex` crate** for line classification patterns — more reliable than manual string matching, and the patterns are compiled once at startup via `lazy_static` or `std::sync::OnceLock`.
- **All DB queries in `queries.rs`** — single `rusqlite::Connection` opened once, passed to query functions. Connection lives on the Tokio thread (used only from `spawn_blocking`).
- **Library picker** is a `gtk4::Box` overlaid on the window using `gtk4::Overlay`, not a separate dialog. Same pattern will be reused for theme picker and search in later phases.
- **Work data fully in memory** — after `load_work()`, the `Work` struct holds all lines, timestamps, and media paths. No further DB queries needed until a new work is loaded.

---

### Task 1: Add `regex` Dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add regex to Cargo.toml**

Add to `[dependencies]`:

```toml
regex = "1"
```

- [ ] **Step 2: Verify compilation**

Run: `cd ~/utono/linux-lit && cargo check 2>&1 | tail -3`
Expected: `Finished`

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat: add regex dependency for line classification"
```

---

### Task 2: Define Data Models

**Files:**
- Create: `src/db/mod.rs`
- Create: `src/db/models.rs`

These structs hold the in-memory representation of a loaded work. They match the spec exactly.

- [ ] **Step 1: Create `src/db/models.rs`**

```rust
#[derive(Debug, Clone)]
pub struct Work {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
    pub lines: Vec<Line>,
    pub timestamps: Vec<Timestamp>,
    pub media_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub id: i64,
    pub text: String,
    pub normalized: String,
    pub speaker: Option<String>,
    pub is_dialogue: bool,
    pub timestamp: Option<TimeRange>,
}

#[derive(Debug, Clone, Copy)]
pub struct TimeRange {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone)]
pub struct Timestamp {
    pub line_id: i64,
    pub start: f64,
    pub end: f64,
    pub media_id: i64,
}

/// Summary info for library picker listing.
#[derive(Debug, Clone)]
pub struct WorkSummary {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
}
```

- [ ] **Step 2: Create `src/db/mod.rs`**

```rust
pub mod line_types;
pub mod models;
pub mod queries;
```

- [ ] **Step 3: Add `mod db;` to `src/main.rs`**

Add `mod db;` after the existing `mod mpv;` line.

- [ ] **Step 4: Verify compilation**

Run: `cargo check 2>&1 | tail -3`
Expected: `Finished` (with warnings about unused items — that's fine, `queries.rs` and `line_types.rs` don't exist yet; create empty placeholder files if needed for the module declaration)

Note: You will need to create stub files for `line_types.rs` and `queries.rs` so the `mod` declarations in `db/mod.rs` compile. Empty files are fine.

- [ ] **Step 5: Commit**

```bash
git add src/db/ src/main.rs
git commit -m "feat: define Work, Line, Timestamp data models"
```

---

### Task 3: Implement Line Type Classification

**Files:**
- Create: `src/db/line_types.rs`

Port the dialogue classification from `lit`'s `line_types.lua`. This determines which lines are "dialogue" (for `,`/`q` navigation in Phase 3).

Reference: `/home/mlj/utono/lit/plugins/lua/lit_keymaps/line_types.lua`

- [ ] **Step 1: Create `src/db/line_types.rs`**

```rust
use std::sync::OnceLock;

use regex::Regex;

/// Prose work types — all non-blank lines are dialogue in these.
const PROSE_TYPES: &[&str] = &["novel", "essay_collection", "prose_book", "prose"];

fn speaker_simple_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Z][A-Z\s.\-']+\.?$").unwrap())
}

fn speaker_with_direction_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Z][A-Z\s\-']*,?\s*\[.*\]\.?$").unwrap())
}

fn stage_direction_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\[.*\]$").unwrap())
}

pub fn is_prose_work(work_type: &str) -> bool {
    PROSE_TYPES.contains(&work_type)
}

pub fn is_blank(text: &str) -> bool {
    text.trim().is_empty()
}

pub fn is_speaker(text: &str) -> bool {
    let trimmed = text.trim();
    // Must be at least 2 chars after stripping trailing period
    let stripped = trimmed.trim_end_matches('.');
    if stripped.len() < 2 {
        return false;
    }
    speaker_simple_re().is_match(trimmed) || speaker_with_direction_re().is_match(trimmed)
}

pub fn is_stage_direction(text: &str) -> bool {
    stage_direction_re().is_match(text.trim())
}

pub fn is_act_scene_marker(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with("ACT ")
        || trimmed.starts_with("SCENE ")
        || trimmed.starts_with("PROLOGUE")
        || trimmed.starts_with("EPILOGUE")
}

pub fn is_separator(text: &str) -> bool {
    text.trim().starts_with('=')
}

/// Classify whether a line is dialogue.
/// For prose works: any non-blank line is dialogue.
/// For non-prose: dialogue if none of the skip patterns match.
pub fn is_dialogue(text: &str, is_prose: bool) -> bool {
    if is_blank(text) {
        return false;
    }
    if is_prose {
        return true;
    }
    !is_speaker(text)
        && !is_stage_direction(text)
        && !is_act_scene_marker(text)
        && !is_separator(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prose_types() {
        assert!(is_prose_work("novel"));
        assert!(is_prose_work("essay_collection"));
        assert!(is_prose_work("prose_book"));
        assert!(is_prose_work("prose"));
        assert!(!is_prose_work("play"));
        assert!(!is_prose_work("poem"));
    }

    #[test]
    fn test_blank() {
        assert!(is_blank(""));
        assert!(is_blank("   "));
        assert!(is_blank("\t\n"));
        assert!(!is_blank("text"));
    }

    #[test]
    fn test_speaker_simple() {
        assert!(is_speaker("HAMLET"));
        assert!(is_speaker("HAMLET."));
        assert!(is_speaker("FIRST GENTLEMAN"));
        assert!(is_speaker("FIRST GENTLEMAN."));
        assert!(is_speaker("KING HENRY"));
        // Single char should fail
        assert!(!is_speaker("A"));
        assert!(!is_speaker("A."));
        // Lowercase should fail
        assert!(!is_speaker("hamlet"));
        assert!(!is_speaker("Hamlet"));
    }

    #[test]
    fn test_speaker_with_direction() {
        assert!(is_speaker("LUCIANA, [to Adriana]"));
        assert!(is_speaker("PRINCE HENRY [aside]"));
    }

    #[test]
    fn test_stage_direction() {
        assert!(is_stage_direction("[Exit]"));
        assert!(is_stage_direction("[Exeunt all but HAMLET]"));
        assert!(!is_stage_direction("Not a direction"));
        assert!(!is_stage_direction("[partial"));
    }

    #[test]
    fn test_act_scene_marker() {
        assert!(is_act_scene_marker("ACT 1"));
        assert!(is_act_scene_marker("SCENE 2"));
        assert!(is_act_scene_marker("PROLOGUE"));
        assert!(is_act_scene_marker("EPILOGUE"));
        assert!(!is_act_scene_marker("Action"));
    }

    #[test]
    fn test_separator() {
        assert!(is_separator("===="));
        assert!(is_separator("= Chapter"));
        assert!(!is_separator("not a separator"));
    }

    #[test]
    fn test_dialogue_play() {
        // Dialogue lines for a play
        assert!(is_dialogue("Who's there?", false));
        assert!(is_dialogue("Nay, answer me. Stand and unfold yourself.", false));
        // Non-dialogue
        assert!(!is_dialogue("HAMLET.", false));
        assert!(!is_dialogue("[Exit]", false));
        assert!(!is_dialogue("ACT 1", false));
        assert!(!is_dialogue("", false));
    }

    #[test]
    fn test_dialogue_prose() {
        // Everything non-blank is dialogue for prose
        assert!(is_dialogue("Any text at all.", true));
        assert!(is_dialogue("HAMLET.", true)); // even speaker-like text
        assert!(is_dialogue("[Exit]", true));   // even stage direction-like text
        assert!(!is_dialogue("", true));         // blank still not dialogue
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cd ~/utono/linux-lit && cargo test db::line_types 2>&1 | tail -15`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/db/line_types.rs
git commit -m "feat: implement line type classification with tests"
```

---

### Task 4: Implement Database Queries

**Files:**
- Create: `src/db/queries.rs`

Opens a read-only SQLite connection and provides `list_works()` and `load_work()`. All functions are synchronous (designed to run inside `spawn_blocking`).

- [ ] **Step 1: Create `src/db/queries.rs`**

```rust
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;

use super::line_types;
use super::models::{Line, TimeRange, Timestamp, Work, WorkSummary};

fn db_path() -> String {
    let home = std::env::var("HOME").expect("HOME environment variable not set");
    format!("{}/utono/litdb/data/lit.db", home)
}

pub fn open_db() -> Result<Connection, rusqlite::Error> {
    Connection::open_with_flags(db_path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
}

pub fn list_works(conn: &Connection) -> Result<Vec<WorkSummary>, rusqlite::Error> {
    let mut stmt =
        conn.prepare("SELECT abbrev, title, author, work_type FROM works ORDER BY title")?;
    let rows = stmt.query_map([], |row| {
        Ok(WorkSummary {
            abbrev: row.get(0)?,
            title: row.get(1)?,
            author: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            work_type: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn load_work(conn: &Connection, abbrev: &str) -> Result<Work, rusqlite::Error> {
    // 1. Get work metadata
    let (title, author, work_type): (String, String, String) = conn.query_row(
        "SELECT title, COALESCE(author, ''), work_type FROM works WHERE abbrev = ?1",
        [abbrev],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    let is_prose = line_types::is_prose_work(&work_type);

    // 2. Load all lines
    let mut line_stmt = conn.prepare(
        "SELECT id, canonical_text, normalized_text, speaker \
         FROM line_mapping WHERE work_abbrev = ?1 \
         ORDER BY div1, div2, line_in_div",
    )?;
    let lines: Vec<Line> = line_stmt
        .query_map([abbrev], |row| {
            let text: String = row.get(1)?;
            let normalized: String = row.get(2)?;
            let speaker: Option<String> = row.get(3)?;
            Ok(Line {
                id: row.get(0)?,
                is_dialogue: line_types::is_dialogue(&text, is_prose),
                text,
                normalized,
                speaker,
                timestamp: None, // filled in below
            })
        })?
        .collect::<Result<_, _>>()?;

    // 3. Load timestamps
    let mut ts_stmt = conn.prepare(
        "SELECT lt.line_mapping_id, lt.start_time, lt.end_time, lt.media_id \
         FROM line_timestamps lt \
         JOIN line_mapping lm ON lt.line_mapping_id = lm.id \
         WHERE lm.work_abbrev = ?1",
    )?;
    let timestamps: Vec<Timestamp> = ts_stmt
        .query_map([abbrev], |row| {
            Ok(Timestamp {
                line_id: row.get(0)?,
                start: row.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                end: row.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                media_id: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
            })
        })?
        .collect::<Result<_, _>>()?;

    // 4. Build timestamp lookup: line_id -> TimeRange
    let mut ts_map: HashMap<i64, TimeRange> = HashMap::new();
    for ts in &timestamps {
        ts_map.entry(ts.line_id).or_insert(TimeRange {
            start: ts.start,
            end: ts.end,
        });
    }

    // 5. Attach timestamps to lines
    let lines: Vec<Line> = lines
        .into_iter()
        .map(|mut line| {
            line.timestamp = ts_map.get(&line.id).copied();
            line
        })
        .collect();

    // 6. Load media paths
    let mut media_stmt = conn.prepare(
        "SELECT mf.path FROM media_files mf \
         JOIN work_media_associations wma ON wma.media_id = mf.id \
         WHERE wma.work_abbrev = ?1 \
         ORDER BY wma.priority DESC",
    )?;
    let media_paths: Vec<String> = media_stmt
        .query_map([abbrev], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    Ok(Work {
        abbrev: abbrev.to_string(),
        title,
        author,
        work_type,
        lines,
        timestamps,
        media_paths,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_db() {
        let conn = open_db().expect("Failed to open lit.db");
        let count: i64 = conn
            .query_row("SELECT count(*) FROM works", [], |row| row.get(0))
            .unwrap();
        assert!(count > 0, "works table should have rows");
    }

    #[test]
    fn test_list_works() {
        let conn = open_db().unwrap();
        let works = list_works(&conn).unwrap();
        assert!(works.len() > 100, "Should have 100+ works");
        // Check Hamlet exists
        assert!(works.iter().any(|w| w.abbrev == "Ham"));
    }

    #[test]
    fn test_load_work_hamlet() {
        let conn = open_db().unwrap();
        let work = load_work(&conn, "Ham").unwrap();
        assert_eq!(work.title, "Hamlet");
        assert_eq!(work.work_type, "play");
        assert!(work.lines.len() > 4000, "Hamlet should have 4000+ lines");
        // First line should be dialogue
        assert_eq!(work.lines[0].text, "Who's there?");
        assert!(work.lines[0].is_dialogue);
        // Check some lines have timestamps
        let with_ts = work.lines.iter().filter(|l| l.timestamp.is_some()).count();
        assert!(with_ts > 0, "Some lines should have timestamps");
    }
}
```

**Note:** `open_db()` uses `std::env::var("HOME")` at runtime to locate the database.

- [ ] **Step 2: Run tests**

Run: `cd ~/utono/linux-lit && cargo test db::queries 2>&1 | tail -15`
Expected: All 3 tests pass (requires `~/utono/litdb/data/lit.db` to exist).

- [ ] **Step 3: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat: implement SQLite queries for works, lines, timestamps"
```

---

### Task 5: Create Library Picker Overlay

**Files:**
- Create: `src/ui/mod.rs`
- Create: `src/ui/library_picker.rs`

A modal overlay widget listing all works, with type-to-filter fuzzy matching, j/k navigation, Enter to select, Escape to dismiss.

- [ ] **Step 1: Create `src/ui/mod.rs`**

```rust
pub mod library_picker;
```

- [ ] **Step 2: Create `src/ui/library_picker.rs`**

This is the most complex file in Phase 2. It creates an overlay with a search entry and a scrollable list of works. Key behaviors:

- Typing in the search entry filters works by case-insensitive subsequence matching on title, author, and abbrev
- j/k (when search entry not focused) or Up/Down arrows navigate the list
- Enter selects the highlighted work
- Escape dismisses the picker

```rust
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Overlay, Orientation, ScrolledWindow,
};

use crate::db::models::WorkSummary;

/// Result of showing the library picker: either a selected work abbreviation or dismissal.
pub struct LibraryPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    works: Vec<WorkSummary>,
}

impl LibraryPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(500)
            .height_request(400)
            .build();
        picker_box.add_css_class("library-picker");

        let search_entry = Entry::builder()
            .placeholder_text("Filter works...")
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

        LibraryPicker {
            overlay,
            picker_box,
            search_entry,
            list_box,
            works: Vec::new(),
        }
    }

    /// Load works into the picker and populate the list.
    pub fn set_works(&mut self, works: Vec<WorkSummary>) {
        self.works = works;
        self.populate_list("");
    }

    /// Show the picker overlay.
    pub fn show(&self) {
        self.picker_box.set_visible(true);
        self.search_entry.set_text("");
        self.search_entry.grab_focus();
        self.populate_list("");
    }

    /// Hide the picker overlay.
    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    /// Check if the picker is currently visible.
    pub fn is_visible(&self) -> bool {
        self.picker_box.is_visible()
    }

    /// Add the picker as an overlay on a base widget.
    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.picker_box);
        self.picker_box.set_visible(false);
    }

    /// Get the search entry for connecting signals.
    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    /// Get the list box for connecting signals.
    pub fn list_box(&self) -> &ListBox {
        &self.list_box
    }

    /// Get the picker box for styling.
    pub fn picker_box(&self) -> &GtkBox {
        &self.picker_box
    }

    /// Populate the list with filtered works.
    pub fn populate_list(&self, filter: &str) {
        // Remove all existing rows
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        let filter_lower = filter.to_lowercase();

        for work in &self.works {
            if !filter.is_empty() && !subsequence_match(&filter_lower, work) {
                continue;
            }

            let label = Label::builder()
                .label(&format!("{} — {} ({})", work.title, work.author, work.abbrev))
                .halign(gtk4::Align::Start)
                .build();

            let row = ListBoxRow::builder().child(&label).build();
            // Store abbrev as widget name for retrieval on selection
            row.set_widget_name(&work.abbrev);
            self.list_box.append(&row);
        }

        // Select first row
        if let Some(first) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&first));
        }
    }

    /// Get the abbreviation of the currently selected work.
    pub fn selected_abbrev(&self) -> Option<String> {
        self.list_box
            .selected_row()
            .map(|row| row.widget_name().to_string())
    }

    /// Move selection up or down by delta rows.
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

/// Case-insensitive subsequence match against title, author, and abbrev.
fn subsequence_match(filter: &str, work: &WorkSummary) -> bool {
    let target = format!("{} {} {}", work.title, work.author, work.abbrev).to_lowercase();
    let mut target_chars = target.chars();
    for fc in filter.chars() {
        if !target_chars.any(|tc| tc == fc) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_work(abbrev: &str, title: &str, author: &str) -> WorkSummary {
        WorkSummary {
            abbrev: abbrev.to_string(),
            title: title.to_string(),
            author: author.to_string(),
            work_type: "play".to_string(),
        }
    }

    #[test]
    fn test_subsequence_match_exact() {
        let w = make_work("Ham", "Hamlet", "Shakespeare");
        assert!(subsequence_match("hamlet", &w));
    }

    #[test]
    fn test_subsequence_match_partial() {
        let w = make_work("Ham", "Hamlet", "Shakespeare");
        assert!(subsequence_match("hml", &w));
        assert!(subsequence_match("ham", &w));
    }

    #[test]
    fn test_subsequence_match_no_match() {
        let w = make_work("Ham", "Hamlet", "Shakespeare");
        assert!(!subsequence_match("xyz", &w));
    }

    #[test]
    fn test_subsequence_match_author() {
        let w = make_work("Ham", "Hamlet", "Shakespeare");
        assert!(subsequence_match("shk", &w));
    }
}
```

- [ ] **Step 3: Add `mod ui;` to `src/main.rs`**

Add `mod ui;` after `mod db;`.

- [ ] **Step 4: Run tests**

Run: `cd ~/utono/linux-lit && cargo test ui::library_picker 2>&1 | tail -10`
Expected: All subsequence match tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/ui/ src/main.rs
git commit -m "feat: add library picker with fuzzy subsequence filtering"
```

---

### Task 6: Wire Everything Together

**Files:**
- Modify: `src/app.rs`
- Modify: `src/main.rs`

Connect the database, library picker, and text view. On startup, load the works list and show the picker. On selection, load the work and populate the text buffer.

- [ ] **Step 1: Refactor `app.rs` to support overlay and work loading**

The `build_window` function needs to:
1. Wrap the ScrolledWindow in an Overlay (for the library picker)
2. Accept a works list to populate the picker
3. Provide a way to populate the buffer with a loaded work's lines

Rewrite `src/app.rs`:

```rust
use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, CssProvider, EventControllerKey, ScrolledWindow, TextBuffer, TextView,
    WrapMode,
};

use crate::db::models::{Work, WorkSummary};
use crate::ui::library_picker::LibraryPicker;

/// Shared application state accessible from callbacks.
pub struct AppState {
    pub text_view: TextView,
    pub buffer: TextBuffer,
    pub picker: LibraryPicker,
    pub current_work: Option<Work>,
    pub window: ApplicationWindow,
}

pub fn build_window(
    app: &gtk4::Application,
    works: Vec<WorkSummary>,
    tokio_handle: tokio::runtime::Handle,
) -> Rc<RefCell<AppState>> {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("linux-lit")
        .default_width(1000)
        .default_height(800)
        .build();

    let buffer = TextBuffer::new(None);
    let text_view = TextView::builder()
        .buffer(&buffer)
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(WrapMode::Word)
        .build();

    // Apply serif font via CSS
    let css_provider = CssProvider::new();
    css_provider.load_from_string(&format!(
        "textview {{ font-family: Georgia, 'Noto Serif', 'Liberation Serif', 'DejaVu Serif'; font-size: {}pt; }}",
        18
    ));
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css_provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Line spacing
    text_view.set_pixels_above_lines(14);
    text_view.set_pixels_below_lines(14);

    // Initial margins
    text_view.set_left_margin(150);
    text_view.set_right_margin(150);

    // Scrolled window — hide scrollbar
    let scrolled = ScrolledWindow::builder()
        .child(&text_view)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::External)
        .vexpand(true)
        .hexpand(true)
        .build();

    // Recalculate margins on resize
    let text_view_for_resize = text_view.clone();
    scrolled.connect_notify_local(Some("width"), move |scrolled, _| {
        let width = scrolled.width();
        let margin = ((width - 700) / 2).max(20);
        text_view_for_resize.set_left_margin(margin);
        text_view_for_resize.set_right_margin(margin);
    });

    // Library picker overlay
    let mut picker = LibraryPicker::new();
    picker.set_works(works);
    picker.attach(&scrolled);

    window.set_child(Some(&picker.overlay));

    let state = Rc::new(RefCell::new(AppState {
        text_view,
        buffer,
        picker,
        current_work: None,
        window: window.clone(),
    }));

    // Connect picker search entry filter
    let state_for_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.picker.search_entry().connect_changed(move |entry| {
            let text = entry.text();
            state_for_filter.borrow().picker.populate_list(&text);
        });
    }

    // Key event controller — single controller handles all keys
    let state_for_keys = Rc::clone(&state);
    let key_controller = EventControllerKey::new();
    key_controller.connect_key_pressed(move |_controller, keyval, _keycode, modifier| {
        let key_name = keyval.name().unwrap_or_default();

        // Ctrl+p: toggle library picker
        if modifier.contains(gtk4::gdk::ModifierType::CONTROL_MASK) && key_name == "p" {
            let state = state_for_keys.borrow();
            if state.picker.is_visible() {
                state.picker.hide();
            } else {
                state.picker.show();
            }
            return glib::Propagation::Stop;
        }

        // When picker is visible, handle picker keys
        let picker_visible = state_for_keys.borrow().picker.is_visible();
        if picker_visible {
            match key_name.as_str() {
                "Escape" => {
                    state_for_keys.borrow().picker.hide();
                    return glib::Propagation::Stop;
                }
                "Return" => {
                    let abbrev = state_for_keys.borrow().picker.selected_abbrev();
                    if let Some(abbrev) = abbrev {
                        let state_clone = Rc::clone(&state_for_keys);
                        let handle = tokio_handle.clone();
                        // Load work asynchronously to avoid blocking GTK thread
                        glib::spawn_future_local(async move {
                            let work = handle
                                .spawn_blocking(move || {
                                    let conn = crate::db::queries::open_db()
                                        .expect("Failed to open lit.db");
                                    crate::db::queries::load_work(&conn, &abbrev)
                                })
                                .await;
                            match work {
                                Ok(Ok(work)) => {
                                    let mut s = state_clone.borrow_mut();
                                    s.picker.hide();
                                    display_work(&mut s, work);
                                }
                                Ok(Err(e)) => eprintln!("Failed to load work: {}", e),
                                Err(e) => eprintln!("Task join error: {}", e),
                            }
                        });
                    }
                    return glib::Propagation::Stop;
                }
                "Down" => {
                    state_for_keys.borrow().picker.move_selection(1);
                    return glib::Propagation::Stop;
                }
                "Up" => {
                    state_for_keys.borrow().picker.move_selection(-1);
                    return glib::Propagation::Stop;
                }
                "j" => {
                    if !state_for_keys.borrow().picker.search_entry().has_focus() {
                        state_for_keys.borrow().picker.move_selection(1);
                        return glib::Propagation::Stop;
                    }
                }
                "k" => {
                    if !state_for_keys.borrow().picker.search_entry().has_focus() {
                        state_for_keys.borrow().picker.move_selection(-1);
                        return glib::Propagation::Stop;
                    }
                }
                _ => {}
            }
        }

        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    window.present();

    // Show picker on startup
    state.borrow().picker.show();

    state
}

/// Populate the text buffer with a loaded work's lines.
pub fn display_work(state: &mut AppState, work: Work) {
    let text: String = work.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
    state.buffer.set_text(&text);
    state
        .window
        .set_title(Some(&format!("{} — linux-lit", work.title)));
    state.current_work = Some(work);
}
```

- [ ] **Step 2: Update `src/main.rs` to wire database and picker selection**

```rust
mod app;
mod db;
mod mpv;
mod ui;

use gtk4::prelude::*;
use mpv::{MpvCommand, MpvEvent};

fn main() {
    let application = gtk4::Application::builder()
        .application_id("com.utono.linux-lit")
        .build();

    application.connect_activate(|gtk_app| {
        // Channel: GTK → Tokio (commands)
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<MpvCommand>(32);

        // Channel: Tokio → GTK (events)
        let (evt_tx, mut evt_rx) = tokio::sync::mpsc::channel::<MpvEvent>(32);

        // Create Tokio runtime, clone handle for GTK thread, then move runtime to background
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        let tokio_handle = rt.handle().clone();
        std::thread::spawn(move || {
            rt.block_on(async move {
                while let Some(cmd) = cmd_rx.recv().await {
                    eprintln!("Tokio received command: {:?}", cmd);
                }
                let _ = evt_tx;
            });
        });

        // Load works list from database (blocking is OK during startup — 133 works, sub-ms)
        let works = {
            let conn = db::queries::open_db().expect("Failed to open lit.db");
            db::queries::list_works(&conn).expect("Failed to list works")
        };

        // Build the window with works list and Tokio handle for async DB operations
        // All key handling (including picker Enter) is inside build_window's key controller
        let _state = app::build_window(gtk_app, works, tokio_handle);

        // Attach event receiver to GTK main loop
        glib::spawn_future_local(async move {
            while let Some(event) = evt_rx.recv().await {
                eprintln!("GTK received event: {:?}", event);
            }
        });

        let _ = cmd_tx;
    });

    application.run();
}
```

- [ ] **Step 3: Verify compilation**

Run: `cd ~/utono/linux-lit && cargo check 2>&1 | tail -5`
Expected: `Finished`

- [ ] **Step 4: Run all tests**

Run: `cd ~/utono/linux-lit && cargo test 2>&1 | tail -15`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "feat: wire database loading, library picker, and work display"
```

---

### Task 7: Add Picker Styling via CSS

**Files:**
- Modify: `src/app.rs`

The picker needs basic styling to be visually distinct from the text background — a semi-opaque panel with padding and rounded corners.

- [ ] **Step 1: Add picker CSS to the CSS provider in `app.rs`**

Extend the CSS string in `build_window()` to include picker styling:

```rust
    css_provider.load_from_string(&format!(
        "textview {{ font-family: Georgia, 'Noto Serif', 'Liberation Serif', 'DejaVu Serif'; font-size: {}pt; }} \
         .library-picker {{ background-color: rgba(40, 40, 40, 0.95); color: white; padding: 16px; border-radius: 8px; }} \
         .library-picker entry {{ margin-bottom: 8px; }} \
         .library-picker row:selected {{ background-color: rgba(100, 140, 200, 0.8); }}",
        18
    ));
```

Note: The `.library-picker` CSS class comes from `add_css_class("library-picker")` set on the picker box in Task 5.

- [ ] **Step 2: Run the application**

Run: `cd ~/utono/linux-lit && cargo run`
Expected:
- App launches and shows the library picker overlay (dark semi-transparent panel)
- Search entry has focus
- Type "ham" — list filters to show Hamlet
- Press j/k to navigate, Enter to select
- Hamlet text fills the text view in serif font
- Window title changes to "Hamlet — linux-lit"
- Ctrl+p reopens the picker

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: add CSS styling for library picker overlay"
```

---

### Task 8: Clean Up and Final Verification

**Files:**
- Modify: various (as needed)

- [ ] **Step 1: Run `cargo clippy`**

Run: `cd ~/utono/linux-lit && cargo clippy 2>&1 | grep -E "warning|error" | grep -v "generated"`
Fix any warnings.

- [ ] **Step 2: Run `cargo fmt`**

Run: `cd ~/utono/linux-lit && cargo fmt`

- [ ] **Step 3: Run all tests**

Run: `cd ~/utono/linux-lit && cargo test 2>&1 | tail -15`
Expected: All tests pass.

- [ ] **Step 4: Final build verification**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -3`
Expected: Clean build, no warnings.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: fix clippy warnings and format code for Phase 2"
```

---

## Phase 2 Acceptance Criteria

After completing all tasks:

1. `cargo run` opens the app and shows the library picker overlay
2. Typing in the picker filters works by subsequence match
3. j/k and arrow keys navigate the picker list
4. Enter loads the selected work — full text appears in the serif text view
5. Window title shows `<title> — linux-lit`
6. Ctrl+p reopens the library picker to select a different work
7. Escape dismisses the picker
8. Line classification: speaker lines, stage directions, and act/scene markers are correctly classified as non-dialogue (verified by unit tests)
9. `cargo test` — all tests pass (line_types, queries, subsequence matching)
10. `cargo clippy` — no warnings

## Notes for Phase 3

- The `AppState` struct and `Rc<RefCell<_>>` pattern will be extended with `current_line: usize` for cursor tracking
- The `is_dialogue` field on each `Line` will be used by `,`/`q` dialogue navigation
- The key event controller in `app.rs` will be expanded into a proper keymap state machine in `src/input/keymap.rs`
- The text buffer population approach (joining lines with `\n`) will need to track line-to-buffer-position mapping for cursor highlight
