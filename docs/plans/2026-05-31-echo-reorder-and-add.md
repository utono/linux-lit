# Echo Reorder + Add-Echo Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In the echoes overlay, let Up/Down reorder the selected echo (reorder + auto-curate, persisted), and `A` open a line-search picker over all Shakespeare lines to add the chosen line as a curated echo at the top.

**Architecture:** Add three `echo_links` DB helpers (`set_echo_link_rank`, `search_lines`, `add_curated_echo_link`) with in-memory unit tests; a reorder routine and an add-flow in `src/input/actions/echoes.rs`; a new `EchoLinePicker` widget modeled on `concordance_word_picker`; and wiring through the shared `handle_picker_key` plus the echoes-overlay key handler.

**Tech Stack:** Rust, GTK4 (TextView/Entry/ListBox/ScrolledWindow/Overlay), rusqlite (SQLite).

**Testing note:** DB helpers are unit-tested with in-memory SQLite (pattern: `line_start_time_reads_stored_value` test in `queries.rs`). The picker/UI/reorder integration is compile-verified + user-verified in Task 9 (manual). Do NOT run `cargo run`. The 2 pre-existing `input::viewport::block_atom_tests` failures are known/unrelated.

---

## Reference facts (verified in source)

- `echo_links` columns: `id, turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank`. `load_echo_links(conn, turn_id)` orders `curated DESC, rank ASC` (`queries.rs:1130`). `insert_echo_links` uses `INSERT OR IGNORE` with `curated=0` literal. `toggle_echo_curated` flips curated. (`queries.rs:1156-1184`)
- `StoredEchoLink { link_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank }`.
- `line_mapping` columns include `work_abbrev, canonical_text, div1, div2, line_in_div`. `canonical_text` is the human-readable line.
- `load_work_titles(conn) -> HashMap<String,String>` (abbrev→title).
- Echoes overlay handler `handle_echoes_overlay_key` (`keymap.rs`): `a`/`n`/`p`/`s`/`R`/`Tab`/`Return`/`g`/`G`/`Ctrl+Up`/`Ctrl+Down` bound; plain `Up`/`Down` and `A` unbound. `toggle_curated` (`echoes.rs:743`) is the reload-and-keep-selection model.
- Shared `handle_picker_key` (`keymap.rs:216`) serves Bookmark/Media/Concordance(+Word/List/Works)/Authorship/Gloss pickers via `resolve_picker_key(key, is_ctrl)` → `PickerAction { Hide, Confirm, MoveDown, MoveUp }` (`src/input/picker_keys.rs`: Ctrl+n→MoveDown, Ctrl+p→MoveUp, Escape→Hide, Return→Confirm). It `match`es the action then `match`es `mode` to call each picker's `move_selection(±1)` etc. Dispatch list at `keymap.rs:61-68`.
- `ConcordanceWordPicker` (`src/ui/concordance_word_picker.rs`, 132 lines) is the picker model: `Overlay`+`picker_box`(Box)+`Entry`+`ListBox` in `ScrolledWindow`; methods `attach`, `show`, `hide`, `is_visible`, `set_words`, `filter_changed`, `populate_list`, `selected_word`, `move_selection`, `entry`. Rows built by clearing the list_box and appending `ListBoxRow`s; first row auto-selected.
- AppState: field `concordance_word_picker: ConcordanceWordPicker` (`app.rs:217`); constructed + `attach`ed into the overlay chain (`app.rs:767-773`); `InputMode` enum has `ConcordanceWordPicker` (`app.rs:53`). `open_concordance_word_picker` (`pickers.rs:747`): set data, `show()`, set input mode.
- `src/ui/mod.rs` registers picker modules (`pub mod concordance_word_picker;` etc.).
- Echoes overlay state: `echo_overlay_links: Vec<StoredEchoLink>`, `echo_overlay_index: usize`, `echo_overlay_turn_id: Option<i64>`, `echo_overlay_titles: HashMap`. `render_echoes`/`scroll_echo_into_view` operate on these.

---

## File Structure

- **Modify** `src/db/queries.rs` — add `set_echo_link_rank`, `search_lines`, `add_curated_echo_link` + in-memory unit tests.
- **Modify** `src/input/actions/echoes.rs` — add `reorder_selected_echo`, `add_echo_from_line`, and open-picker helper.
- **Create** `src/ui/echo_line_picker.rs` — `EchoLinePicker` widget.
- **Modify** `src/ui/mod.rs` — register the module.
- **Modify** `src/app.rs` — `InputMode::EchoLinePicker`, AppState field, construct + attach, plus a field to stash the pending turn_id for the add.
- **Modify** `src/input/keymap.rs` — `Up`/`Down`/`A` arms in `handle_echoes_overlay_key`; `EchoLinePicker` branch in the dispatch list and in `handle_picker_key`.

---

## Task 1: DB helper `set_echo_link_rank` (TDD)

**Files:**
- Modify: `src/db/queries.rs` (+ inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

Add inside the existing `#[cfg(test)] mod tests { … }`:

```rust
#[test]
fn set_echo_link_rank_updates_rank_and_curated() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE echo_links (
            id INTEGER PRIMARY KEY, turn_id INTEGER, echo_work_abbrev TEXT,
            echo_div1 INTEGER, echo_div2 INTEGER, echo_start_line INTEGER,
            echo_text TEXT, similarity REAL, curated INTEGER, rank INTEGER
         );
         INSERT INTO echo_links (id, turn_id, echo_work_abbrev, echo_div1, echo_div2,
            echo_start_line, echo_text, similarity, curated, rank)
            VALUES (1, 7, 'Ham', 1, 1, 1, 'x', 0.0, 0, 5);",
    ).unwrap();
    set_echo_link_rank(&conn, 1, 2, true).unwrap();
    let (rank, curated): (i64, i64) = conn.query_row(
        "SELECT rank, curated FROM echo_links WHERE id = 1", [],
        |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(rank, 2);
    assert_eq!(curated, 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test set_echo_link_rank 2>&1 | tail -12`
Expected: FAIL — `cannot find function set_echo_link_rank`.

- [ ] **Step 3: Implement**

Add near `toggle_echo_curated` in `src/db/queries.rs`:

```rust
/// Set a link's rank and curated flag.
pub fn set_echo_link_rank(conn: &Connection, link_id: i64, rank: i64, curated: bool) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE echo_links SET rank = ?2, curated = ?3 WHERE id = ?1",
        rusqlite::params![link_id, rank, curated as i64],
    )?;
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test set_echo_link_rank 2>&1 | tail -8`
Expected: PASS. (A `dead_code` warning on `set_echo_link_rank` is fine until Task 5.)

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "Add set_echo_link_rank query

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: DB helper `search_lines` (TDD)

**Files:**
- Modify: `src/db/queries.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn search_lines_matches_substring_case_insensitive_with_limit() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE line_mapping (
            id INTEGER PRIMARY KEY, work_abbrev TEXT, canonical_text TEXT,
            div1 INTEGER, div2 INTEGER, line_in_div INTEGER
         );
         INSERT INTO line_mapping (id, work_abbrev, canonical_text, div1, div2, line_in_div) VALUES
            (1, 'Ham', 'To be, or not to be', 3, 1, 56),
            (2, 'Mac', 'Tomorrow and tomorrow', 5, 5, 19),
            (3, 'Lr',  'Nothing will come of nothing', 1, 1, 92);",
    ).unwrap();
    // Case-insensitive substring.
    let hits = search_lines(&conn, "TOMORROW", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0], ("Mac".to_string(), 5, 5, 19, "Tomorrow and tomorrow".to_string()));
    // Limit caps results.
    let all = search_lines(&conn, "o", 2).unwrap();
    assert_eq!(all.len(), 2);
    // No match -> empty.
    assert!(search_lines(&conn, "zzzz", 10).unwrap().is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test search_lines 2>&1 | tail -12`
Expected: FAIL — `cannot find function search_lines`.

- [ ] **Step 3: Implement**

```rust
/// Search every line whose canonical text contains `query` (case-insensitive),
/// across all works. Returns (work_abbrev, div1, div2, line_in_div, text), capped.
pub fn search_lines(conn: &Connection, query: &str, limit: i64)
    -> Result<Vec<(String, i64, i64, i64, String)>, rusqlite::Error>
{
    let pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT work_abbrev, div1, div2, line_in_div, canonical_text \
         FROM line_mapping \
         WHERE canonical_text LIKE ?1 COLLATE NOCASE \
         ORDER BY work_abbrev, div1, div2, line_in_div \
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![pattern, limit], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    rows.collect()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test search_lines 2>&1 | tail -8`
Expected: PASS. (`dead_code` on `search_lines` is fine until Task 7.)

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "Add search_lines query for the add-echo line picker

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: DB helper `add_curated_echo_link` (TDD)

**Files:**
- Modify: `src/db/queries.rs`

Inserts a new curated link at rank 0, shifting existing curated ranks +1. Returns the new link's id.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn add_curated_echo_link_inserts_at_top_shifting_curated() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE echo_links (
            id INTEGER PRIMARY KEY AUTOINCREMENT, turn_id INTEGER, echo_work_abbrev TEXT,
            echo_div1 INTEGER, echo_div2 INTEGER, echo_start_line INTEGER,
            echo_text TEXT, similarity REAL, curated INTEGER, rank INTEGER
         );
         INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2,
            echo_start_line, echo_text, similarity, curated, rank) VALUES
            (7, 'Mac', 5, 5, 19, 'old curated', 0.0, 1, 0),
            (7, 'Lr', 1, 1, 92, 'noncurated', 0.0, 0, 0);",
    ).unwrap();
    let new_id = add_curated_echo_link(&conn, 7, "Ham", 3, 1, 56, "To be").unwrap();
    // New row: curated, rank 0.
    let (curated, rank): (i64, i64) = conn.query_row(
        "SELECT curated, rank FROM echo_links WHERE id = ?1", [new_id],
        |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((curated, rank), (1, 0));
    // The previously-curated row shifted to rank 1.
    let old_rank: i64 = conn.query_row(
        "SELECT rank FROM echo_links WHERE echo_text = 'old curated'", [],
        |r| r.get(0)).unwrap();
    assert_eq!(old_rank, 1);
    // The non-curated row is untouched (rank 0, curated 0).
    let nc: (i64, i64) = conn.query_row(
        "SELECT curated, rank FROM echo_links WHERE echo_text = 'noncurated'", [],
        |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(nc, (0, 0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test add_curated_echo_link 2>&1 | tail -12`
Expected: FAIL — `cannot find function add_curated_echo_link`.

- [ ] **Step 3: Implement**

```rust
/// Insert a manual curated echo link at the top of the curated group (rank 0),
/// shifting existing curated ranks down. Returns the new link's id.
pub fn add_curated_echo_link(
    conn: &Connection,
    turn_id: i64,
    work: &str,
    div1: i64,
    div2: i64,
    line_in_div: i64,
    text: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "UPDATE echo_links SET rank = rank + 1 WHERE turn_id = ?1 AND curated = 1",
        [turn_id],
    )?;
    conn.execute(
        "INSERT INTO echo_links \
         (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0.0, 1, 0)",
        rusqlite::params![turn_id, work, div1, div2, line_in_div, text],
    )?;
    Ok(conn.last_insert_rowid())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test add_curated_echo_link 2>&1 | tail -8`
Expected: PASS. (`dead_code` until Task 7.)

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "Add add_curated_echo_link query (insert at top of curated group)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: `EchoLinePicker` widget

**Files:**
- Create: `src/ui/echo_line_picker.rs`
- Modify: `src/ui/mod.rs`

Models `ConcordanceWordPicker`. Holds result rows as a `Vec<(String,i64,i64,i64,String)>` (work, div1, div2, line, text) and a titles map; the selected row index maps back into that Vec.

- [ ] **Step 1: Create the widget file**

Create `src/ui/echo_line_picker.rs`:

```rust
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Orientation, Overlay, ScrolledWindow};
use std::collections::HashMap;

/// A single search hit: (work_abbrev, div1, div2, line_in_div, text).
pub type LineHit = (String, i64, i64, i64, String);

/// Picker for adding an echo: fuzzy-search Shakespeare lines, pick one.
pub struct EchoLinePicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    results: Vec<LineHit>,
}

impl EchoLinePicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = GtkBox::new(Orientation::Vertical, 0);
        picker_box.set_halign(Align::Center);
        picker_box.set_valign(Align::Start);
        picker_box.set_margin_top(40);
        picker_box.set_width_request(600);
        picker_box.add_css_class("picker-box");

        let search_entry = Entry::new();
        search_entry.set_placeholder_text(Some("Search Shakespeare lines…"));
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

        Self { overlay, picker_box, search_entry, list_box, results: Vec::new() }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.picker_box);
        self.picker_box.set_visible(false);
    }

    pub fn show(&self) {
        self.search_entry.set_text("");
        self.picker_box.set_visible(true);
        self.search_entry.grab_focus();
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.picker_box.is_visible()
    }

    pub fn entry(&self) -> &Entry {
        &self.search_entry
    }

    /// Replace the result rows. `titles` maps work_abbrev -> display title for
    /// the "text — Title div1.div2" row label.
    pub fn set_results(&mut self, results: Vec<LineHit>, titles: &HashMap<String, String>) {
        self.results = results;
        while let Some(row) = self.list_box.first_child() {
            self.list_box.remove(&row);
        }
        for (work, div1, div2, _line, text) in &self.results {
            let title = titles.get(work).cloned().unwrap_or_else(|| work.clone());
            let label = Label::new(Some(&format!("{} — {} {}.{}", text, title, div1, div2)));
            label.set_halign(Align::Start);
            label.set_hexpand(true);
            label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            label.add_css_class("picker-item-title");
            let row = ListBoxRow::new();
            row.set_child(Some(&label));
            self.list_box.append(&row);
        }
        if let Some(row) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&row));
        }
    }

    pub fn move_selection(&self, delta: i32) {
        let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
        let next = current + delta;
        if let Some(row) = self.list_box.row_at_index(next) {
            self.list_box.select_row(Some(&row));
        }
    }

    /// The currently selected line hit, if any.
    pub fn selected_hit(&self) -> Option<LineHit> {
        let idx = self.list_box.selected_row()?.index();
        if idx < 0 { return None; }
        self.results.get(idx as usize).cloned()
    }
}
```

- [ ] **Step 2: Register the module**

In `src/ui/mod.rs`, add (alphabetically near the other `echo_*`/`concordance_*` entries):

```rust
pub mod echo_line_picker;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | tail -8`
Expected: builds clean (a `never constructed`/`never used` note on `EchoLinePicker` is fine until Task 6).

- [ ] **Step 4: Commit**

```bash
git add src/ui/echo_line_picker.rs src/ui/mod.rs
git commit -m "Add EchoLinePicker widget for add-echo line search

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: `reorder_selected_echo` (Up/Down reorder + auto-curate)

**Files:**
- Modify: `src/input/actions/echoes.rs`

- [ ] **Step 1: Add the function**

Add to `src/input/actions/echoes.rs` (place near `toggle_curated`):

```rust
/// Reorder the selected echo within the curated group (delta -1 = up, +1 = down),
/// marking it curated. Curated items always sort above non-curated; this moves
/// the selection among them and persists sequential ranks. Mirrors toggle_curated's
/// reload-and-keep-selection pattern.
pub(crate) fn reorder_selected_echo(state_rc: &Rc<RefCell<AppState>>, delta: i32) {
    let (turn_id, sel_link_id, links) = {
        let s = state_rc.borrow();
        let link = match s.echo_overlay_links.get(s.echo_overlay_index) {
            Some(l) => l.clone(),
            None => return,
        };
        match s.echo_overlay_turn_id {
            Some(id) => (id, link.link_id, s.echo_overlay_links.clone()),
            None => return,
        }
    };

    // Curated prefix in current display order (links are loaded curated DESC, rank ASC).
    let mut curated: Vec<i64> = links.iter().filter(|l| l.curated).map(|l| l.link_id).collect();
    let sel_is_curated = links.iter().any(|l| l.link_id == sel_link_id && l.curated);

    // Index of the selected link within the curated order (curate-on-move if not).
    let from = if sel_is_curated {
        curated.iter().position(|&id| id == sel_link_id).unwrap_or(0)
    } else {
        // Not yet curated: append to the curated tail, then move from there.
        curated.push(sel_link_id);
        curated.len() - 1
    };
    let to = from as i32 + delta;
    if to < 0 || to >= curated.len() as i32 {
        // Already at an edge of the curated group. If we just curated it (was not
        // curated), still persist that; otherwise no-op.
        if sel_is_curated {
            return;
        }
    }
    let to = to.clamp(0, curated.len() as i32 - 1) as usize;
    curated.swap(from, to);

    // Persist sequential ranks for the curated order; all curated=true.
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        for (rank, link_id) in curated.iter().enumerate() {
            let _ = crate::db::queries::set_echo_link_rank(&conn, *link_id, rank as i64, true);
        }
    }

    // Reload, keep selection on the moved link.
    let links = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::queries::load_echo_links(&conn, turn_id).ok())
        .unwrap_or_default();
    let mut s = state_rc.borrow_mut();
    let new_idx = links.iter().position(|l| l.link_id == sel_link_id).unwrap_or(0);
    s.echo_overlay_links = links;
    s.echo_overlay_index = new_idx;
    render_echoes(&mut s);
    s.gloss_overlay.scroll_echo_into_view(new_idx);
    sync_session(&mut s);
    crate::logging::log("ECHOES: reordered echo");
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -10`
Expected: builds clean. The Task-1 `set_echo_link_rank` dead_code warning should now be gone. A `dead_code` warning on `reorder_selected_echo` is expected until Task 8. If a compile error appears (e.g. `open_db_rw`/`sync_session`/`render_echoes` path differs), grep the real names in `echoes.rs` and match.

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/echoes.rs
git commit -m "Add reorder_selected_echo (Up/Down reorder + auto-curate)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: AppState wiring for `EchoLinePicker`

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add the InputMode variant**

In `src/app.rs`, in the `InputMode` enum (which has `ConcordanceWordPicker` around line 53), add:

```rust
    EchoLinePicker,
```

- [ ] **Step 2: Add the AppState field + a pending-turn stash**

In `src/app.rs`, near `pub concordance_word_picker: crate::ui::concordance_word_picker::ConcordanceWordPicker,` (around line 217), add:

```rust
    pub echo_line_picker: crate::ui::echo_line_picker::EchoLinePicker,
    /// turn_id the add-echo picker will attach the chosen line to.
    pub echo_add_turn_id: Option<i64>,
```

- [ ] **Step 3: Construct + attach in the overlay chain**

In `src/app.rs`, the picker overlay chain (around lines 762-795) layers each picker onto the previous one's `.overlay`. It is NOT strictly linear at the tail: both `concordance_works_picker` and `vocab_popup` attach onto `authorship_picker.overlay`. Insert `echo_line_picker` by attaching it onto `authorship_picker.overlay` (the same base `vocab_popup`/`concordance_works_picker` use), AFTER `authorship_picker` is constructed and attached. Add:

```rust
    let echo_line_picker = crate::ui::echo_line_picker::EchoLinePicker::new();
    echo_line_picker.attach(&authorship_picker.overlay);
    echo_line_picker.overlay.set_vexpand(true);
```

First read lines ~760-795 to confirm `authorship_picker` is the current shared base for trailing overlays; if the structure differs, attach onto whatever overlay `vocab_popup`/`concordance_works_picker` attach to, and report the actual base used.

- [ ] **Step 4: Add field initializers to the AppState constructor**

In the `AppState { … }` construction, add (field shorthand) wherever the other picker fields are listed:

```rust
        echo_line_picker,
        echo_add_turn_id: None,
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build 2>&1 | tail -10`
Expected: builds clean. The `EchoLinePicker` "never constructed" warning is now gone. If the attach-chain wiring is wrong you'll get an overlay/borrow error — re-read the existing chain and mirror it precisely.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "Wire EchoLinePicker into AppState and the overlay chain

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Open-picker + add-flow actions

**Files:**
- Modify: `src/input/actions/echoes.rs`

- [ ] **Step 1: Add the open + add functions**

Add to `src/input/actions/echoes.rs`:

```rust
/// `A` in the echoes overlay: open the line-search picker to add an echo to the
/// current turn. Stashes the turn_id for the deferred add.
pub(crate) fn open_add_echo_picker(state_rc: &Rc<RefCell<AppState>>) {
    let turn_id = state_rc.borrow().echo_overlay_turn_id;
    if turn_id.is_none() {
        return;
    }
    let mut s = state_rc.borrow_mut();
    s.echo_add_turn_id = turn_id;
    s.echo_line_picker.set_results(Vec::new(), &s.echo_overlay_titles);
    s.echo_line_picker.show();
    s.input_mode = crate::app::InputMode::EchoLinePicker;
    crate::logging::log("ECHOES: opened add-echo line picker");
}

/// Re-run the line search for the picker's current entry text (called on each
/// keystroke). Empty query clears the list.
pub(crate) fn refresh_add_echo_search(state_rc: &Rc<RefCell<AppState>>) {
    let query = state_rc.borrow().echo_line_picker.entry().text().to_string();
    let results = if query.trim().is_empty() {
        Vec::new()
    } else {
        crate::db::queries::open_db()
            .ok()
            .and_then(|conn| crate::db::queries::search_lines(&conn, query.trim(), 200).ok())
            .unwrap_or_default()
    };
    let mut s = state_rc.borrow_mut();
    let titles = s.echo_overlay_titles.clone();
    s.echo_line_picker.set_results(results, &titles);
}

/// Confirm the selected line in the add-echo picker: add it as a curated echo at
/// the top of the rankings (or promote an existing matching echo), then return
/// to the echoes overlay.
pub(crate) fn confirm_add_echo(state_rc: &Rc<RefCell<AppState>>) {
    let hit = match state_rc.borrow().echo_line_picker.selected_hit() {
        Some(h) => h,
        None => {
            cancel_add_echo(state_rc);
            return;
        }
    };
    let turn_id = match state_rc.borrow().echo_add_turn_id {
        Some(id) => id,
        None => {
            cancel_add_echo(state_rc);
            return;
        }
    };
    let (work, div1, div2, line_in_div, text) = hit;

    // Existing match in this turn? Promote it; else insert new at top.
    let existing_id = state_rc.borrow().echo_overlay_links.iter()
        .find(|l| l.echo_work_abbrev == work && l.echo_div1 == div1
                  && l.echo_div2 == div2 && l.echo_start_line == line_in_div)
        .map(|l| l.link_id);

    let new_link_id = if let Ok(conn) = crate::db::queries::open_db_rw() {
        if let Some(id) = existing_id {
            // Promote: shift other curated +1, set this to curated rank 0.
            let _ = conn.execute(
                "UPDATE echo_links SET rank = rank + 1 WHERE turn_id = ?1 AND curated = 1",
                [turn_id],
            );
            let _ = crate::db::queries::set_echo_link_rank(&conn, id, 0, true);
            Some(id)
        } else {
            crate::db::queries::add_curated_echo_link(&conn, turn_id, &work, div1, div2, line_in_div, &text).ok()
        }
    } else {
        None
    };

    let links = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::queries::load_echo_links(&conn, turn_id).ok())
        .unwrap_or_default();
    let mut s = state_rc.borrow_mut();
    s.echo_line_picker.hide();
    s.echo_add_turn_id = None;
    let new_idx = new_link_id
        .and_then(|id| links.iter().position(|l| l.link_id == id))
        .unwrap_or(0);
    s.echo_overlay_links = links;
    s.echo_overlay_index = new_idx;
    s.input_mode = crate::app::InputMode::EchoesOverlay;
    render_echoes(&mut s);
    s.gloss_overlay.scroll_echo_into_view(new_idx);
    sync_session(&mut s);
    crate::logging::log("ECHOES: added echo from line picker");
}

/// Cancel the add-echo picker, returning to the echoes overlay.
pub(crate) fn cancel_add_echo(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    s.echo_line_picker.hide();
    s.echo_add_turn_id = None;
    s.input_mode = crate::app::InputMode::EchoesOverlay;
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -12`
Expected: builds clean. The `search_lines`/`add_curated_echo_link` dead_code warnings are now gone. `dead_code` warnings on the four new fns are expected until Task 8. If a method/field name differs (e.g. `selected_hit`, `set_results`, `entry`), match the Task-4 widget API and the Task-6 field names exactly.

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/echoes.rs
git commit -m "Add open/refresh/confirm/cancel add-echo picker actions

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Keymap wiring (Up/Down/A + picker dispatch)

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Add Up/Down/A arms in the echoes overlay handler**

In `handle_echoes_overlay_key`, in the main `match key_name { … }` (NOT under the `is_ctrl` block — Ctrl+Up/Down stay volume), add:

```rust
        "Up" => {
            crate::input::actions::echoes::reorder_selected_echo(state, -1);
            true
        }
        "Down" => {
            crate::input::actions::echoes::reorder_selected_echo(state, 1);
            true
        }
        "A" => {
            crate::input::actions::echoes::open_add_echo_picker(state);
            true
        }
```

(`A` is the shifted form; GTK delivers Shift+a as key_name `"A"`, distinct from `"a"` = play echo.)

- [ ] **Step 2: Add EchoLinePicker to the picker dispatch list**

In `handle_key`'s mode-dispatch match (the `|`-joined list at ~`keymap.rs:61-68` that routes to `handle_picker_key`), add `EchoLinePicker` to the joined arm:

```rust
            | crate::app::InputMode::AuthorshipPicker
            | crate::app::InputMode::EchoLinePicker
            | crate::app::InputMode::GlossPicker => handle_picker_key(state, key_name, is_ctrl, tokio_handle, mode),
```

- [ ] **Step 3: Handle EchoLinePicker inside handle_picker_key**

In `handle_picker_key`, add `EchoLinePicker` branches to the `mode` matches:
- In the `PickerAction::MoveDown` arm's `match mode { … }` (the block calling `move_selection(1)` per mode), add:
  ```rust
                InputMode::EchoLinePicker => state.borrow().echo_line_picker.move_selection(1),
  ```
- In the `PickerAction::MoveUp` arm's `match mode { … }`, add:
  ```rust
                InputMode::EchoLinePicker => state.borrow().echo_line_picker.move_selection(-1),
  ```
- In the `PickerAction::Confirm` arm, add an `EchoLinePicker` case that calls `crate::input::actions::echoes::confirm_add_echo(state)`. (Read the Confirm arm's structure — it `match`es `mode`; add `InputMode::EchoLinePicker => crate::input::actions::echoes::confirm_add_echo(state),` following the shape of the other confirm cases. If confirm cases return early/true, match that control flow.)
- In the `PickerAction::Hide` arm, add `InputMode::EchoLinePicker => crate::input::actions::echoes::cancel_add_echo(state),` following the other hide cases.

Read `handle_picker_key` fully first (`keymap.rs:216` onward) and mirror the exact structure of each arm — the per-mode matches and their borrow/return patterns vary, so match the siblings precisely rather than assuming.

- [ ] **Step 4: Wire live search on entry change**

The established pattern is a `connect_changed` handler on the picker's entry in `src/app.rs` (see lines ~1232-1292, e.g. `s.concordance_word_picker.entry().connect_changed(move |_| { state_for_conc_word_filter.borrow().concordance_word_picker.filter_changed(); });` at ~1291). Mirror it: in `src/app.rs`, near those other `connect_changed` registrations, add a handler on `echo_line_picker.entry()` that calls the Task-7 `refresh_add_echo_search`. Follow the exact `Rc` clone pattern the sibling handlers use:

```rust
        let state_for_echo_line = std::rc::Rc::clone(&state);
        s.echo_line_picker.entry().connect_changed(move |_| {
            crate::input::actions::echoes::refresh_add_echo_search(&state_for_echo_line);
        });
```

Read the surrounding `connect_changed` block (~1232-1292) to confirm the exact variable name for the `Rc<RefCell<AppState>>` in scope (it may be `state` or a clone) and the borrow pattern, and match it. This file (`src/app.rs`) is already being edited in this task, so it's in scope.

- [ ] **Step 5: Verify it compiles + clippy + tests**

Run: `cargo build 2>&1 | tail -6 && cargo clippy 2>&1 | rg 'keymap.rs|echoes.rs|echo_line_picker.rs|app.rs' | rg -v 'CHUNK_PREROLL|activate_chunk|show_gloss|display_work_at|build_line_map|apply_ab_dim' | head && cargo test 2>&1 | tail -6`
Expected: builds clean; no new clippy warnings in the touched code; the 3 new DB tests pass; only the 2 known pre-existing `block_atom_tests` failures.

- [ ] **Step 6: Commit**

```bash
git add src/input/keymap.rs src/app.rs
git commit -m "Wire Up/Down reorder, A add-echo, and EchoLinePicker dispatch

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: Footer hint + manual verification

**Files:**
- Modify: `src/ui/gloss_overlay.rs`

- [ ] **Step 1: Update the echoes footer hint**

In `src/ui/gloss_overlay.rs`, in `show_echoes`, the hint text is currently:

```rust
        self.hint.set_text("Esc close · a play echo · Tab play turn · n/p select · Enter open work · c copy · s curate · R refresh");
```

Change it to include reorder + add:

```rust
        self.hint.set_text("Esc close · a play · A add · ↑/↓ reorder · n/p select · Tab play turn · Enter open · c copy · s curate · R refresh");
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -3`
Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "Update echoes hint for reorder and add-echo

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 4: Manual verification (user runs the app)**

Ask the user to `cargo run`, open the echoes overlay, and confirm:
1. `Up`/`Down` move the selected echo within the curated group and mark it ★ curated; ordering persists across close/reopen.
2. `A` opens the line picker; typing filters Shakespeare lines (rows show "text — Title act.scene"); Ctrl+n/p navigate; Enter adds the chosen line as the first (curated) echo; Escape cancels back to the overlay.
3. Adding a line already in the list promotes it to top (no duplicate).
4. `Ctrl+Up`/`Ctrl+Down` still adjust volume (not reorder).

---

## Self-Review

**Spec coverage:**
- `set_echo_link_rank` → Task 1. `search_lines` → Task 2. `add_curated_echo_link` (insert at rank 0, shift curated) → Task 3. ✓
- `EchoLinePicker` widget (Entry + list, "text — Title act.scene" rows, Ctrl+n/p nav via move_selection) → Task 4. ✓
- Up/Down reorder within curated group + auto-curate, persisted, reload-keep-selection → Task 5 + Task 8 Step 1. ✓
- `A` opens picker; live substring search; select adds curated at top; existing match promoted not duplicated → Task 7 + Task 8. ✓
- InputMode + AppState field + overlay attach → Task 6. ✓
- Picker wiring via shared handle_picker_key, Ctrl+n/p, Return=confirm, Escape=cancel-to-EchoesOverlay → Task 8. ✓
- Footer hint → Task 9. ✓
- Manual verification → Task 9 Step 4. ✓

**Placeholder scan:** Verified against source — Task 6 Step 3 now names the concrete attach base (`authorship_picker.overlay`, the same one `vocab_popup`/`concordance_works_picker` use) and Task 6 Step 4 / Task 8 Step 4 give the literal `connect_changed` registration mirroring `concordance_word_picker`'s at app.rs:~1291. The only remaining "read and mirror" is Task 8 Step 3 (the per-mode `Confirm`/`Hide`/`MoveDown`/`MoveUp` arms inside `handle_picker_key`), which is an explicit "add a sibling case matching the existing arms' control flow" instruction — appropriate, since the arms' borrow/return patterns vary and copying a wrong literal would be worse than directing to the authoritative sibling. All DB/widget/action code is complete and literal.

**Type consistency:** `set_echo_link_rank(&Connection, i64, i64, bool)`, `search_lines(&Connection, &str, i64) -> Vec<(String,i64,i64,i64,String)>`, `add_curated_echo_link(&Connection, i64, &str, i64, i64, i64, &str) -> i64` — used identically in tests (Tasks 1-3) and callers (Tasks 5, 7). `EchoLinePicker` methods (`set_results`, `move_selection`, `selected_hit`, `entry`, `show`, `hide`) defined in Task 4 and called in Tasks 6-8. `LineHit = (String,i64,i64,i64,String)` matches `search_lines` return and `selected_hit`. `echo_add_turn_id`/`echo_line_picker` fields (Task 6) used in Task 7. Actions `reorder_selected_echo`/`open_add_echo_picker`/`refresh_add_echo_search`/`confirm_add_echo`/`cancel_add_echo` (Tasks 5, 7) called in Task 8. ✓
