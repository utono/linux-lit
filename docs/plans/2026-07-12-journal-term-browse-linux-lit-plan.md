# Journal Term-Browse (linux-lit reader slice) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Inside the journal overlay, let the user press `f`, type a term, and browse the journal Q&A entries that discuss it across ALL works — landing each match in a read-only overlay view (no work switch), with `Ctrl+n`/`Ctrl+p` stepping only the matching subset and `Esc` clearing the filter then closing.

**Architecture:** A new cross-work DB query `find_pages_by_term` returns matching entries (tags-first, FTS5-fallback) carrying their `work_abbrev`. A new `filter` field on `JournalState` holds the active term + its ordered match list + position. A new **read-only render path** (`render_filtered_match`) drives the overlay's pure `show_page` directly with a fetched entry and a `<abbrev> <div1>.<div2> · match N of M` footer — bypassing the band/`current_work`-bound `render_current`, so an entry from another work displays without reloading the reader. The `f` key opens a `JournalTermInput` (a GTK Entry + a suggestion ListBox, modeled on `JournalMovePicker`): the user **types a term freely**, and existing distinct tags appear as live-filtered suggestions beneath. **Enter searches whatever is in the box** (a highlighted suggestion if one is selected, else the raw typed text) — so freely-typed terms reach the FTS5 fallback and the feature works even when `journal_tags` is empty. `nav_page` branches to walk the filter's match list when a filter is active.

**Tech Stack:** Rust, gtk4-rs, rusqlite (SQLite + FTS5), cargo test, headless `cage`+`grim` verification.

## Global Constraints

- The reader's journal action layer is hardwired to `s.current_work`; the term-browse feature must NOT switch `current_work` (decision: cross-work read-only overlay). Matches from other works render via a dedicated read-only path, not `render_current`.
- Matching is **tags-first, FTS5-fallback, all works**: query `journal_tags.term = ?` first; if zero rows, fall back to `journal_fts MATCH ?` (phrase-quoted). Both span every work.
- The litdb migration already added `journal_tags(entry_id, term, source)` and `journal_fts(question, answer)` (external-content, rowid=`journal_entries.id`) to `lit.db`. The reader is the FIRST FTS5 consumer; no `MATCH` exists in `src/db/` yet. `journal_tags` may be EMPTY at runtime (backfill is token-gated) — the FTS fallback must carry the feature.
- New DB queries take `conn: &Connection` and are opened by the caller via `crate::db::queries::open_db()` (read-only). Query result ordering for the match list must match `find_all_pages_ordered`'s `ORDER BY (scope = 'work') DESC, div1 ASC, div2 ASC, timestamp ASC, id ASC`, but WITHOUT the `WHERE work_abbrev = ?` restriction.
- `f` is an in-overlay key added to `handle_journal_key`'s plain-key match (`src/input/keymap.rs`), NOT a `keymap_config.rs` Action. There is no existing `f` binding in the overlay.
- Pure logic (the query, the step/clamp) is unit-tested with an in-memory DB (`Connection::open_in_memory()` + schema) or as pure functions like `flat_step`. GTK-touching functions are verified headlessly, not unit-tested.
- `JournalPage` currently has NO `work_abbrev` field; the new query needs it for the footer, so a parallel struct or an extended row carries it (do NOT add `work_abbrev` to `JournalPage` itself — 60+ call sites select `JOURNAL_PAGE_COLUMNS` without it; use a wrapper).
- Test command: `cd ~/utono/linux-lit && cargo test <name>`. Build: `cargo build`. Headless UI check: the protocol in `~/utono/linux-lit/CLAUDE.md` (Headless Verification), or the `test-headless-navigation` skill.

---

## File Structure

- Modify `src/db/journal.rs` — add `TermMatch` (a `JournalPage` + `work_abbrev`), `find_pages_by_term`, and an `ensure_journal_tags` migration guard (idempotent CREATE for `journal_tags`/`journal_fts` so in-memory test DBs and any un-migrated lit.db have them). One responsibility: journal DB access.
- Modify `src/input/actions/journal.rs` — add `JournalFilter` state on `JournalState`, `render_filtered_match` (read-only cross-work render), `open_term_input`/`confirm_term_input`/`activate_filter`/`clear_filter`, and the `nav_page` filter branch. Reuses the existing pure `flat_step` helper for the subset walk (no new stepper).
- Create `src/ui/journal_term_input.rs` — `JournalTermInput` (GTK Entry the user types into + a ListBox of tag suggestions), modeled on `src/ui/journal_move_picker.rs`. Enter searches the typed text or the highlighted suggestion.
- Modify `src/app/mod.rs` — `InputMode::JournalTermInput` variant, `journal_term_input` AppState field (import/construct/attach/assemble), and the Entry `connect_changed` wiring block.
- Modify `src/input/picker_dispatch.rs` — register `JournalTermInput` in `impl_picker!` + `picker_for_mode`.
- Modify `src/input/keymap.rs` — batch `JournalTermInput` into `handle_picker_key`, add its Hide/Confirm arms, add the `f` arm to `handle_journal_key`, and the `Esc`-clears-filter logic.
- Modify `src/ui/journal_overlay.rs` — none required if the footer string is built by the caller and passed to `show_page`; confirm during Task 3.

---

## Task 1: Cross-work term query + migration guard (`find_pages_by_term`)

**Files:**
- Modify: `src/db/journal.rs` (add `TermMatch`, `ensure_journal_tags`, `find_pages_by_term`; call `ensure_journal_tags` from `ensure_journal_table`)
- Test: same file's `#[cfg(test)] mod tests`

**Interfaces:**
- Produces:
  - `pub struct TermMatch { pub page: JournalPage, pub work_abbrev: String }`
  - `pub fn ensure_journal_tags(conn: &Connection) -> Result<(), rusqlite::Error>` — idempotent `CREATE TABLE IF NOT EXISTS journal_tags(...)`, `CREATE VIRTUAL TABLE IF NOT EXISTS journal_fts USING fts5(question, answer, content='journal_entries', content_rowid='id')`, and the three sync triggers `IF NOT EXISTS`. (Mirrors the litdb migration so in-memory test DBs and any older lit.db get the tables.)
  - `pub fn find_pages_by_term(conn: &Connection, term: &str) -> Result<Vec<TermMatch>, rusqlite::Error>` — tags-first then FTS5-fallback, all works, ordered as `find_all_pages_ordered`.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/db/journal.rs` (the `mem()` helper already exists and calls `ensure_journal_table`; this task makes `ensure_journal_table` also create the tag/fts tables):

```rust
#[test]
fn term_query_tags_first_then_fts_fallback_across_works() {
    let conn = mem();
    // two works; the "fee simple" phrase lives only in an answer on Rom
    conn.execute(
        "INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope) \
         VALUES (20, 'Rom', 3, 1, 'q', 'A fee simple, in Elizabethan law, is absolute ownership.', 'scene')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope) \
         VALUES (7, '2H6', 1, 1, 'q', 'Nothing about property law here.', 'scene')",
        [],
    ).unwrap();

    // No tags yet -> FTS fallback finds the Rom entry, and carries its work_abbrev.
    let hits = find_pages_by_term(&conn, "fee simple").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page.id, 20);
    assert_eq!(hits[0].work_abbrev, "Rom");

    // Now tag the 2H6 entry with the term -> tags-first path takes precedence
    // and returns the tagged entry (not the FTS match).
    conn.execute(
        "INSERT INTO journal_tags (entry_id, term, source) VALUES (7, 'fee simple', 'backfill')",
        [],
    ).unwrap();
    let hits2 = find_pages_by_term(&conn, "Fee Simple").unwrap(); // case-insensitive
    assert_eq!(hits2.iter().map(|m| m.page.id).collect::<Vec<_>>(), vec![7]);
}

#[test]
fn ensure_journal_tags_is_idempotent() {
    let conn = mem();
    ensure_journal_tags(&conn).unwrap(); // second call must not error
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('journal_tags','journal_fts')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 2);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd ~/utono/linux-lit && cargo test term_query_tags_first_then_fts_fallback_across_works ensure_journal_tags_is_idempotent`
Expected: FAIL to compile — `find_pages_by_term` / `ensure_journal_tags` / `TermMatch` not found.

- [ ] **Step 3: Implement the struct, migration guard, and query**

Add near `JournalPage` in `src/db/journal.rs`:

```rust
/// A journal page plus the work it belongs to — used by cross-work term browse
/// (the term query spans all works, so the abbrev is not a fixed parameter).
#[derive(Debug, Clone)]
pub struct TermMatch {
    pub page: JournalPage,
    pub work_abbrev: String,
}
```

Add the migration guard (call it from the end of `ensure_journal_table` so every code path that ensures the journal schema also ensures these):

```rust
pub fn ensure_journal_tags(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS journal_tags (
            entry_id INTEGER NOT NULL REFERENCES journal_entries(id) ON DELETE CASCADE,
            term   TEXT NOT NULL,
            source TEXT,
            PRIMARY KEY (entry_id, term)
        );
        CREATE INDEX IF NOT EXISTS idx_journal_tags_term ON journal_tags(term);
        CREATE VIRTUAL TABLE IF NOT EXISTS journal_fts USING fts5(
            question, answer, content='journal_entries', content_rowid='id'
        );
        CREATE TRIGGER IF NOT EXISTS journal_entries_ai AFTER INSERT ON journal_entries BEGIN
            INSERT INTO journal_fts(rowid, question, answer)
            VALUES (new.id, new.question, new.answer);
        END;
        CREATE TRIGGER IF NOT EXISTS journal_entries_ad AFTER DELETE ON journal_entries BEGIN
            INSERT INTO journal_fts(journal_fts, rowid, question, answer)
            VALUES ('delete', old.id, old.question, old.answer);
        END;
        CREATE TRIGGER IF NOT EXISTS journal_entries_au AFTER UPDATE ON journal_entries BEGIN
            INSERT INTO journal_fts(journal_fts, rowid, question, answer)
            VALUES ('delete', old.id, old.question, old.answer);
            INSERT INTO journal_fts(rowid, question, answer)
            VALUES (new.id, new.question, new.answer);
        END;",
    )
}
```

In `ensure_journal_table`, after its existing body (before `Ok(())`), add:

```rust
    ensure_journal_tags(conn)?;
```

Add the query. `JOURNAL_PAGE_COLUMNS` has no `work_abbrev`, so select it as an extra trailing column and map both:

```rust
pub fn find_pages_by_term(
    conn: &Connection,
    term: &str,
) -> Result<Vec<TermMatch>, rusqlite::Error> {
    let term_norm = term.trim().to_lowercase();
    if term_norm.is_empty() {
        return Ok(Vec::new());
    }
    let order = "ORDER BY (scope = 'work') DESC, div1 ASC, div2 ASC, timestamp ASC, id ASC";

    // 1) tags-first (case-insensitive exact term)
    let tag_sql = format!(
        "SELECT {JOURNAL_PAGE_COLUMNS}, work_abbrev \
         FROM journal_entries \
         WHERE id IN (SELECT entry_id FROM journal_tags WHERE LOWER(term) = ?1) \
         {order}"
    );
    let mut out = Vec::new();
    {
        let mut stmt = conn.prepare(&tag_sql)?;
        let rows = stmt.query_map([&term_norm], map_term_match_row)?;
        for r in rows {
            out.push(r?);
        }
    }
    if !out.is_empty() {
        return Ok(out);
    }

    // 2) FTS5 fallback (phrase-quoted so multi-word terms match as a phrase)
    let fts_sql = format!(
        "SELECT {JOURNAL_PAGE_COLUMNS}, work_abbrev \
         FROM journal_entries \
         WHERE id IN (SELECT rowid FROM journal_fts WHERE journal_fts MATCH ?1) \
         {order}"
    );
    let phrase = format!("\"{}\"", term_norm.replace('"', ""));
    let mut stmt = conn.prepare(&fts_sql)?;
    let rows = stmt.query_map([&phrase], map_term_match_row)?;
    let mut out2 = Vec::new();
    for r in rows {
        out2.push(r?);
    }
    Ok(out2)
}

fn map_term_match_row(row: &rusqlite::Row<'_>) -> Result<TermMatch, rusqlite::Error> {
    let page = map_journal_page_row(row)?;
    // work_abbrev is the column AFTER the JOURNAL_PAGE_COLUMNS list (index 11).
    let work_abbrev: String = row.get(11)?;
    Ok(TermMatch { page, work_abbrev })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd ~/utono/linux-lit && cargo test term_query_tags_first_then_fts_fallback_across_works ensure_journal_tags_is_idempotent`
Expected: both pass.

- [ ] **Step 5: Run the existing journal DB tests to confirm no regression**

Run: `cd ~/utono/linux-lit && cargo test --lib db::journal`
Expected: all pass (the pre-existing `all_pages_ordered_work_first_then_scenes` still passes — `ensure_journal_table` now also creates the tag/fts tables, which must not perturb existing queries).

- [ ] **Step 6: Commit**

```bash
cd ~/utono/linux-lit
git add src/db/journal.rs
git commit -m "feat(journal): cross-work find_pages_by_term (tags-first, FTS5 fallback)"
```

---

## Task 2: Filter state + read-only cross-work render (reusing `flat_step`)

**Files:**
- Modify: `src/input/actions/journal.rs` (add `JournalFilter`, extend `JournalState`, add `render_filtered_match`, `activate_filter`, `clear_filter`, and the `nav_page` filter branch)
- Test: same file's `#[cfg(test)] mod tests` (a semantics-lock test over the existing `flat_step`; the render/activate/clear are GTK and verified headlessly)

**Interfaces:**
- Consumes: `find_pages_by_term`, `TermMatch` (Task 1); the overlay's `show_page` (pure display); the existing pure `flat_step` (`src/input/actions/journal.rs:471`).
- Produces:
  - a `pub struct JournalFilter { pub term: String, pub matches: Vec<crate::db::journal::TermMatch>, pub pos: usize }` and a `pub filter: Option<JournalFilter>` field on `JournalState`.
  - `pub(crate) fn render_filtered_match(s: &mut AppState)` — renders `s.journal.filter`'s current match via `show_page` with a `<abbrev> <div1>.<div2> · match N of M` footer, no work switch.
  - `pub(crate) fn activate_filter(state: &Rc<RefCell<AppState>>, term: &str) -> bool` — fetches matches, stores filter state, renders match 1; false (+ toast) if none.
  - `pub(crate) fn clear_filter(state: &Rc<RefCell<AppState>>)` — drops the filter, re-renders the current band via `render_current`.

Note: the subset walk in `nav_page` reuses the existing `flat_step` (already tested by `flat_step_clamps_and_steps`); this task adds NO new stepper function.

- [ ] **Step 1: Write the failing test**

The render is GTK (not unit-tested) and `flat_step` is already tested, so this task's automated test asserts the filter state shape compiles and `flat_step` gives the subset-walk behavior the filter relies on. Add to the tests module:

```rust
#[test]
fn filter_walk_uses_flat_step_over_match_list() {
    // A 3-match subset: stepping forward from 0 -> 1 -> 2 -> clamps (None at end).
    assert_eq!(flat_step(0, 1, 3), Some(1));
    assert_eq!(flat_step(1, 1, 3), Some(2));
    assert_eq!(flat_step(2, 1, 3), None); // at last match, no wrap
    assert_eq!(flat_step(0, -1, 3), None); // at first match, no wrap
    // empty subset never steps
    assert_eq!(flat_step(0, 1, 0), None);
}
```

- [ ] **Step 2: Run test to verify it fails/passes appropriately**

Run: `cd ~/utono/linux-lit && cargo test filter_walk_uses_flat_step_over_match_list`
Expected: PASS immediately (it exercises the existing `flat_step`). This is a guard test locking the semantics the filter depends on; if `flat_step` is ever changed, this fails. (Its purpose is documentation + regression lock, so it does not follow the fail-first cycle — note this explicitly in the commit.)

- [ ] **Step 3: Add the filter state**

Extend `JournalState` (`src/input/actions/journal.rs`, the struct at lines 72-97) with a new field:

```rust
    pub filter: Option<JournalFilter>,
```

Add the struct above `JournalState`:

```rust
/// Active term-browse filter: the term, the ordered cross-work match list, and
/// the current position within it. When set, `nav_page` walks these matches
/// instead of the current work's `find_all_pages_ordered`.
#[derive(Debug, Clone, Default)]
pub struct JournalFilter {
    pub term: String,
    pub matches: Vec<crate::db::journal::TermMatch>,
    pub pos: usize,
}
```

Ensure `JournalState`'s `Default`/construction initializes `filter: None` (find where `JournalState` is constructed — if it derives `Default`, add `#[derive(Default)]`-compatible `Option` which defaults to `None` automatically; otherwise add `filter: None` to the constructor).

- [ ] **Step 4: Add the read-only render + activate/clear**

Add these functions to `src/input/actions/journal.rs`:

```rust
/// Render the filter's current match in the overlay WITHOUT switching
/// current_work. Drives the pure `show_page` directly with the fetched entry;
/// bypasses the band-driven `render_current` so an entry from another work
/// displays in place. Footer reads "<abbrev> <div1>.<div2> · match N of M".
pub(crate) fn render_filtered_match(s: &mut AppState) {
    let Some(filter) = s.journal.filter.as_ref() else { return; };
    let Some(m) = filter.matches.get(filter.pos) else { return; };
    let p = &m.page;
    let footer_left = format!(
        "{} {}.{} \u{00b7} match {} of {}",
        m.work_abbrev, p.div1, p.div2, filter.pos + 1, filter.matches.len()
    );
    let (cw, h) = crate::input::actions::journal::card_size(s); // reuse existing sizing helper
    // NOTE: render as a single page — filtered view shows one entry at a time.
    s.journal_overlay.show_page(
        &footer_left,
        0,            // page_index within this single entry's pagination
        1,            // page_count (the overlay repaginates internally by height)
        &p.question,
        &p.answer,
        &p.kind,
        cw,
        h,
    );
}
```

(During implementation, confirm the exact sizing helper `render_current` uses to compute `(cw, h)` — replicate that call. If it is inline rather than a helper, extract a small `card_size(&AppState) -> (i32, i32)` and use it in both places.)

```rust
/// Activate a term filter: fetch matches, store filter state, render the first.
/// Returns false (with a toast) if nothing matches.
pub(crate) fn activate_filter(state: &Rc<RefCell<AppState>>, term: &str) -> bool {
    let matches = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_pages_by_term(&conn, term).ok())
        .unwrap_or_default();
    let mut s = state.borrow_mut();
    if matches.is_empty() {
        crate::ui::toast::show(&s, &format!("No entries mention \u{201c}{}\u{201d}", term));
        return false;
    }
    s.journal.filter = Some(JournalFilter { term: term.to_string(), matches, pos: 0 });
    render_filtered_match(&mut s);
    true
}

/// Clear the active filter and return to the normal band view.
pub(crate) fn clear_filter(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.journal.filter.is_none() {
        return;
    }
    s.journal.filter = None;
    render_current(&mut s); // restore the band the user was in
}
```

(Confirm the toast helper's real path during implementation — the report shows toasts used elsewhere in this file; match that call, e.g. `crate::ui::toast::show` or the existing in-file toast idiom.)

- [ ] **Step 5: Branch `nav_page` on the active filter**

Modify `nav_page` (`src/input/actions/journal.rs:518`). At the top, before the existing `find_all_pages_ordered` logic, add:

```rust
    // Filtered subset walk: step within the term matches, render read-only.
    {
        let mut s = state.borrow_mut();
        if let Some(filter) = s.journal.filter.as_mut() {
            let len = filter.matches.len();
            if let Some(next) = flat_step(filter.pos, delta, len) {
                filter.pos = next;
                render_filtered_match(&mut s);
            }
            return;
        }
    }
```

(The existing unfiltered body follows unchanged.)

- [ ] **Step 6: Build + run the pure test**

Run: `cd ~/utono/linux-lit && cargo build && cargo test filter_walk_uses_flat_step_over_match_list`
Expected: builds clean; test passes.

- [ ] **Step 7: Commit**

```bash
cd ~/utono/linux-lit
git add src/input/actions/journal.rs
git commit -m "feat(journal): term-filter state + read-only cross-work render + nav_page branch"
```

---

## Task 3: `JournalTermInput` widget (typed box + tag suggestions)

**Files:**
- Create: `src/ui/journal_term_input.rs`
- Modify: `src/ui/mod.rs` (add `pub mod journal_term_input;`)

**Interfaces:**
- Consumes: `picker_nav` helpers, `picker_filter::subsequence_match` (as `JournalMovePicker` does).
- Produces: `pub struct JournalTermInput` with `new()`, `attach(&Overlay)`, `set_suggestions(Vec<String>)` (distinct tags shown beneath), `show()`, `hide()`, `search_entry() -> &Entry`, `populate_list(&str)`, `move_selection(i32)`, `selected_index() -> Option<usize>`, and — the key method — `query_term() -> Option<String>`: returns the highlighted suggestion if a suggestion row is selected AND the typed text is empty, else the **typed entry text** (trimmed); `None` only if both are empty. This is what makes freely-typed terms (not just tags) reach the search.

- [ ] **Step 1: Copy the move-picker template and adapt for typed input**

Read `src/ui/journal_move_picker.rs` in full. Create `src/ui/journal_term_input.rs` as a near-copy where the ListBox holds distinct-tag suggestion strings and the Entry is the primary input:

```rust
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Entry, ListBox, ListBoxRow, Overlay};

pub struct JournalTermInput {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    pub suggestions: Vec<String>,
}

impl JournalTermInput {
    pub fn new() -> Self {
        let overlay = Overlay::new();
        let picker_box = crate::ui::picker_nav::build_picker_card();
        let search_entry = Entry::builder()
            .placeholder_text("Browse journal by term (type; existing tags suggested)…")
            .build();
        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();
        picker_box.append(&search_entry);
        picker_box.append(&scrolled);
        JournalTermInput { overlay, picker_box, search_entry, list_box, suggestions: Vec::new() }
    }

    pub fn attach(&self, host: &Overlay) {
        crate::ui::picker_attach::attach_panel(host, &self.overlay, &self.picker_box);
    }

    pub fn set_suggestions(&mut self, suggestions: Vec<String>) {
        self.suggestions = suggestions;
        self.search_entry.set_text("");
        self.populate_list("");
    }

    pub fn show(&self) { self.overlay.set_visible(true); self.search_entry.grab_focus(); }
    pub fn hide(&self) { self.overlay.set_visible(false); }
    pub fn search_entry(&self) -> &Entry { &self.search_entry }
    pub fn move_selection(&self, delta: i32) {
        crate::ui::picker_nav::move_selection_clamped(&self.list_box, delta);
    }
    pub fn selected_index(&self) -> Option<usize> {
        crate::ui::picker_nav::selected_index(&self.list_box)
    }

    /// The term to search: the typed text if any (trimmed), else the highlighted
    /// suggestion. This ordering means a freely-typed term always wins — so the
    /// FTS fallback is reachable even with zero tags. None only if both empty.
    pub fn query_term(&self) -> Option<String> {
        let typed = self.search_entry.text().trim().to_string();
        if !typed.is_empty() {
            return Some(typed);
        }
        self.selected_index()
            .and_then(|i| self.suggestions.get(i).cloned())
    }

    pub fn populate_list(&self, filter: &str) {
        crate::ui::picker_nav::clear_list(&self.list_box);
        let filter_lower = filter.to_lowercase();
        for (idx, term) in self.suggestions.iter().enumerate() {
            if !filter.is_empty()
                && !crate::ui::picker_filter::subsequence_match(&filter_lower, &term.to_lowercase())
            {
                continue;
            }
            let hbox = crate::ui::picker_nav::two_label_row(term, "");
            let row = ListBoxRow::builder().child(&hbox).build();
            row.set_widget_name(&idx.to_string());
            self.list_box.append(&row);
        }
        crate::ui::picker_nav::select_first_row(&self.list_box);
    }
}
```

(During implementation, match the EXACT helper signatures/visibility from `journal_move_picker.rs` — e.g. `attach_panel`, `two_label_row` arity. If `two_label_row` requires a non-empty second label, pass an empty-safe variant or the single-label helper the move picker uses. Confirm `Entry::text()` returns a `glib::GString` that `.trim()` works on via `AsRef<str>`.)

Add to `src/ui/mod.rs`:

```rust
pub mod journal_term_input;
```

- [ ] **Step 2: Build to verify it compiles**

Run: `cd ~/utono/linux-lit && cargo build`
Expected: compiles (the widget is not yet wired into AppState; this step only proves the module is valid).

- [ ] **Step 3: Commit**

```bash
cd ~/utono/linux-lit
git add src/ui/journal_term_input.rs src/ui/mod.rs
git commit -m "feat(ui): JournalTermInput (typed term box + tag suggestions)"
```

---

## Task 4: Wire the input into AppState + dispatch + the `f` key + Esc-clears

**Files:**
- Modify: `src/app/mod.rs` (import, field, construct/attach/assemble, Entry `connect_changed`)
- Modify: `src/input/picker_dispatch.rs` (`impl_picker!` + `picker_for_mode`)
- Modify: `src/app/mod.rs` `InputMode` enum (add `JournalTermInput`)
- Modify: `src/input/keymap.rs` (batch into `handle_picker_key`, Hide/Confirm arms, `f` arm in `handle_journal_key`, Esc-clears-filter)
- Modify: `src/input/actions/journal.rs` (add `open_term_input`, `confirm_term_input`)

**Interfaces:**
- Consumes: `JournalTermInput` (Task 3), `activate_filter`/`clear_filter` (Task 2), `find_distinct_terms` (add a tiny query — see Step 1).

Note on mode naming: the `InputMode` variant is `JournalTermInput` (the typed box), reusing the picker plumbing (`handle_picker_key`, `picker_for_mode`, Ctrl+n/p/Up/Down/Enter/Esc) because it is structurally a filter-Entry + ListBox like the other pickers. Its Confirm arm searches the typed term rather than a fixed selection.

- [ ] **Step 1: Add a distinct-terms query**

Add to `src/db/journal.rs` (+ a `mem()` test asserting it returns sorted distinct terms):

```rust
pub fn find_distinct_terms(conn: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT term FROM journal_tags ORDER BY term ASC",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}
```

Test:

```rust
#[test]
fn distinct_terms_sorted() {
    let conn = mem();
    conn.execute("INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope) VALUES (1,'Rom',1,1,'q','a','scene')", []).unwrap();
    conn.execute("INSERT INTO journal_tags (entry_id, term) VALUES (1,'freehold'),(1,'fee simple')", []).unwrap();
    assert_eq!(find_distinct_terms(&conn).unwrap(), vec!["fee simple".to_string(), "freehold".to_string()]);
}
```

Run: `cd ~/utono/linux-lit && cargo test distinct_terms_sorted` → PASS.

- [ ] **Step 2: Add the InputMode variant + AppState field**

In `src/app/mod.rs`: add `JournalTermInput,` to `InputMode` beside `JournalMovePicker` (line ~128). Add the field beside `journal_move_picker` (line ~472): `pub journal_term_input: crate::ui::journal_term_input::JournalTermInput,`. Mirror the move-picker's construct/attach block (lines ~1405-1407) and struct-assembly (line ~1792): construct with `JournalTermInput::new()`, `attach(&journal_move_picker.overlay)` (chain onto the existing overlay stack), and add `journal_term_input,` to the assembly.

- [ ] **Step 3: Register in picker_dispatch**

In `src/input/picker_dispatch.rs`: add an `impl_picker!(JournalTermInput);`-style line (match the macro's real form for the move picker — `JournalTermInput` exposes `move_selection`/`hide`, satisfying the `Picker` trait), and add the arm `InputMode::JournalTermInput => Some(&s.journal_term_input),` to `picker_for_mode`.

- [ ] **Step 4: Entry filter wiring (live suggestions)**

In `src/app/mod.rs`, mirror the journal_move_picker `connect_changed` block (lines ~2396-2404): on the term input's `search_entry().connect_changed`, call `populate_list(&entry.text())` so the suggestion list live-filters as the user types (via the shared `AppState` clone pattern used by the sibling blocks).

- [ ] **Step 5: Add the picker key arms + the `f` key + Esc-clears**

In `src/input/keymap.rs`:

(a) Batch `JournalTermInput` into the `handle_picker_key` `|`-group (line ~175). This gives it Ctrl+n/p + Up/Down (move through suggestions), Return (confirm), Escape (hide) for free via `resolve_picker_key`. Typed characters flow to the focused Entry naturally (the picker key handler returns unhandled for non-nav keys, so GTK routes them to the Entry).

(b) Add its Hide and Confirm arms (mirroring the move picker's at lines ~435 and ~570):

```rust
// Hide arm (Escape in the term input): back to the overlay, no filter set
InputMode::JournalTermInput => {
    s.journal_term_input.hide();
    s.input_mode = InputMode::JournalOverlay;
}
```
```rust
// Confirm arm (Return): search the typed (or highlighted) term
InputMode::JournalTermInput => {
    crate::input::actions::journal::confirm_term_input(state);
}
```

(c) In `handle_journal_key`'s plain-key match (line ~1378-1513), add before the `_ => false` arm:

```rust
"f" => {
    crate::input::actions::journal::open_term_input(state);
    true
}
```

(d) Change the overlay's `Escape` arm (lines ~1509-1513) so a first Esc clears an active filter (staying open), and a second Esc closes:

```rust
"Escape" => {
    if state.borrow().journal.filter.is_some() {
        crate::input::actions::journal::clear_filter(state);
    } else {
        crate::input::actions::journal::close_overlay(state);
    }
    true
}
```

- [ ] **Step 6: Add the open/confirm actions**

In `src/input/actions/journal.rs`:

```rust
/// Open the term input box (with distinct-tag suggestions) from inside the
/// overlay (the `f` key). The user types a term freely; existing tags are
/// suggested beneath.
pub(crate) fn open_term_input(state: &Rc<RefCell<AppState>>) {
    let terms = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_distinct_terms(&conn).ok())
        .unwrap_or_default();
    let mut s = state.borrow_mut();
    s.journal_term_input.set_suggestions(terms);
    s.journal_term_input.show();
    s.input_mode = InputMode::JournalTermInput;
}

/// Confirm the entered term: hide the box, activate the filter (lands on
/// match 1). The term is the typed text (else the highlighted suggestion);
/// a freely-typed term reaches the FTS fallback even with zero tags.
pub(crate) fn confirm_term_input(state: &Rc<RefCell<AppState>>) {
    let term = {
        let s = state.borrow();
        s.journal_term_input.query_term()
    };
    {
        let s = state.borrow();
        s.journal_term_input.hide();
    }
    state.borrow_mut().input_mode = InputMode::JournalOverlay;
    if let Some(term) = term {
        activate_filter(state, &term);
    }
}
```

(Note: `activate_filter` re-borrows `state`, so release the borrow before calling it — the code above scopes the borrows. Confirm no double-borrow panic during build/headless test.)

- [ ] **Step 7: Build**

Run: `cd ~/utono/linux-lit && cargo build`
Expected: compiles clean.

- [ ] **Step 8: Full test suite (no regressions)**

Run: `cd ~/utono/linux-lit && cargo test`
Expected: all pass (including Task 1/2/4 additions).

- [ ] **Step 9: Commit**

```bash
cd ~/utono/linux-lit
git add src/app/mod.rs src/input/picker_dispatch.rs src/input/keymap.rs src/input/actions/journal.rs src/db/journal.rs
git commit -m "feat(journal): wire JournalTermInput + f key + Esc-clears-filter"
```

---

## Task 5: Headless end-to-end verification

**Files:** none (verification).

- [ ] **Step 1: Confirm the tag/fts tables exist on the real lit.db**

```bash
litecli ~/utono/litdb/data/lit.db -e "SELECT rowid FROM journal_fts WHERE journal_fts MATCH '\"fee simple\"';"
```
Expected: `20` (the litdb slice already applied the migration).

- [ ] **Step 2: Headless drive the `f` flow (free-typed term, FTS path)**

Follow `~/utono/linux-lit/CLAUDE.md` "Headless Verification" (or the `test-headless-navigation` skill) to launch the reader in the isolated `cage` compositor on a work whose scene has journal entries, then:
- open the journal (`Ctrl+j`),
- press `f` — confirm the term-input box appears (with any distinct tags suggested beneath; the suggestion list may be empty since `journal_tags` is empty, and that is FINE — the box still accepts typed input),
- **type `fee simple`** and press Return.

Screenshot each state; confirm it lands on the `Rom 3.1` entry with footer `Rom 3.1 · match N of M` (the `fee simple` case reaches the FTS fallback with zero tags), `Ctrl+n`/`Ctrl+p` step within the subset, first `Esc` clears (footer returns to the band's `Q&A N of M`), second `Esc` closes.

- [ ] **Step 3: Verify the tag-suggestion path (optional, if tags exist)**

If the litdb `tag-journal` backfill has run (or you seed a row), confirm typing a prefix filters the suggestion list and that selecting a suggestion with an empty typed box searches that tag. Zero-cost seed to exercise the suggestion UI:
```bash
litecli ~/utono/litdb/data/lit.db -e "INSERT INTO journal_tags (entry_id, term, source) VALUES (20, 'fee simple', 'manual-test');"
# ... press f, see "fee simple" suggested, clear the box, arrow to it, Return; confirm it lands on Rom 3.1 ...
litecli ~/utono/litdb/data/lit.db -e "DELETE FROM journal_tags WHERE source='manual-test';"
```

- [ ] **Step 4: Confirm no work switch**

After browsing to a cross-work match (e.g. reading 2H6, landing on a Rom entry) and pressing `Esc` twice, confirm the reader is still on 2H6 at the original position (the read-only overlay did not switch `current_work`). Screenshot-verify.

---

## Self-Review notes

- **Cross-work constraint honored:** Task 2's `render_filtered_match` drives `show_page` directly (no `render_current`, no `current_work` switch), per the locked decision. Verified feasible because `show_page` is a pure display call taking all data as arguments.
- **Empty-tags reality (resolved):** `f` opens a **typed input box** (Task 3, `JournalTermInput`), so a freely-typed term reaches the FTS fallback even when `journal_tags` is empty — the `fee simple` case works today. Existing distinct tags appear as live-filtered suggestions beneath but do not gate the search. `query_term()` prefers typed text, falling back to a highlighted suggestion. Task 5 Step 2 verifies the free-typed path; Step 3 (optional) verifies the suggestion path.
- **No `JournalPage` schema churn:** `work_abbrev` rides in the `TermMatch` wrapper, not the 60-callsite `JournalPage`.
- **Reuse over duplication:** the filter walk reuses `flat_step` (not a new stepper); the input box reuses `picker_nav`/`picker_filter` and the picker key plumbing; the wiring mirrors `JournalMovePicker` end-to-end.
- **Type consistency:** `find_pages_by_term -> Vec<TermMatch>`, `JournalFilter { term, matches: Vec<TermMatch>, pos }`, `render_filtered_match` reads `filter.matches[filter.pos]`, `JournalTermInput::query_term() -> Option<String>` — consistent across tasks.
