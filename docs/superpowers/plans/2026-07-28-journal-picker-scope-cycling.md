# Q&A Picker Scope Cycling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the Q&A picker cycle between scene, work, and author scope with Alt+t, opening on work (today's behavior).

**Architecture:** A `JournalPickerScope` enum mirroring the existing `GlossPickerFilter`, held on `AppState`; a new author-scope DB query (a UNION of cross-work entries and corpus notes); a scope-aware rebuild in `populate_and_show_picker`; an `InputMode::JournalPicker` arm beside the existing `GlossPicker` Alt+t arm. Author rows carry a work label, following `RecentQaPicker`'s existing cross-work convention.

**Tech Stack:** Rust, rusqlite (SQLite), GTK4.

**Spec:** `docs/superpowers/specs/2026-07-28-journal-picker-scope-cycling-design.md`

## Global Constraints

- Branch off `master`. Per CLAUDE.md this work gets a worktree under
  `~/utono/linux-lit-wt/<branch>`.
- **Never write to the shared lit.db** at `~/utono/litdb/data/lit.db`. Tests
  use `Connection::open_in_memory()`.
- Do NOT run `cargo run`. The user launches the app themselves.
- **linux-lit is a BIN-ONLY crate.** `cargo test --lib` fails. Use
  `cargo test --bins`. Baseline at branch start is whatever master reports
  after the band-refile branch merges — record it before Task 1 and use it as
  your arithmetic base.
- A PRE-EXISTING deny-level clippy error at `src/db/queries.rs:2456`
  (unrelated) makes `cargo clippy --all-targets` fail. Plain `cargo clippy`
  is the gate. Do not touch queries.rs.
- **The picker opens on Work in every case.** Scope is not persisted across
  opens, not stored in config, and not remembered per work.
- Keybind changes update every surface they touch in the SAME change (see
  Task 5). This is required, not optional.

---

### Task 1: The scope enum and the author query

Pure data layer, no UI. Testable in isolation.

**Files:**
- Modify: `src/input/actions/pickers.rs` (add the enum beside `GlossPickerFilter`)
- Modify: `src/db/journal.rs` (add the author query)
- Modify: `src/db/journal.rs` (`mod tests`)
- Modify: `src/app/mod.rs` (add the `AppState` field)

**Interfaces:**
- Consumes: `JOURNAL_PAGE_COLUMNS` and `map_journal_page_row`, both private to
  `src/db/journal.rs`; `JournalPage`.
- Produces, both used by Task 2:
  - `pub(crate) enum JournalPickerScope { Scene, Work, Author }` with
    `#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]`, `Work` marked
    `#[default]`, and methods `fn next(self) -> Self` and
    `fn label(self) -> &'static str`.
  - `pub fn find_author_all_pages(conn: &Connection, author: &str) -> Result<Vec<(JournalPage, String)>, rusqlite::Error>` —
    every entry from every work by `author`, plus that author's corpus notes,
    each paired with the `work_abbrev` it belongs to.
  - `AppState.journal_picker_scope: JournalPickerScope`.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src/db/journal.rs`:

```rust
    /// Author scope spans every work by the author AND that author's
    /// corpus notes. Corpus notes store the AUTHOR NAME in work_abbrev
    /// (see save_author_page / AUTHOR_DIV), so the query is a union of two
    /// different keying schemes — the thing most likely to be got wrong.
    #[test]
    fn author_scope_spans_works_and_corpus_notes() {
        let conn = mem();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS works (
                 abbrev TEXT PRIMARY KEY, title TEXT, author TEXT
             );
             INSERT INTO works (abbrev, title, author) VALUES
                 ('Ham', 'Hamlet', 'Shakespeare'),
                 ('Rom', 'Romeo and Juliet', 'Shakespeare'),
                 ('BH',  'Bleak House', 'Charles Dickens');",
        )
        .unwrap();

        save_journal_page(&conn, "Ham", 1, 2, "HamQ?", "A.", "m", "scene", "qa").unwrap();
        save_journal_page(&conn, "Rom", 2, 2, "RomQ?", "A.", "m", "scene", "qa").unwrap();
        save_author_page(&conn, "Shakespeare", "CorpusQ?", "A.", "m", "note").unwrap();
        // Another author's entry must NOT appear.
        save_journal_page(&conn, "BH", 1, 0, "DickensQ?", "A.", "m", "scene", "qa").unwrap();

        let rows = find_author_all_pages(&conn, "Shakespeare").unwrap();

        let questions: Vec<&str> = rows.iter().map(|(p, _)| p.question.as_str()).collect();
        assert_eq!(rows.len(), 3, "two work entries + one corpus note");
        assert!(questions.contains(&"HamQ?"));
        assert!(questions.contains(&"RomQ?"));
        assert!(questions.contains(&"CorpusQ?"));
        assert!(!questions.contains(&"DickensQ?"), "another author must be excluded");

        // Each row knows which work it came from — author rows are the only
        // cross-work list, so the picker labels them.
        let ham = rows.iter().find(|(p, _)| p.question == "HamQ?").unwrap();
        assert_eq!(ham.1, "Ham");
    }

    /// An author with no entries at all yields an empty vec, never an error.
    #[test]
    fn author_scope_empty_is_not_an_error() {
        let conn = mem();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS works (
                 abbrev TEXT PRIMARY KEY, title TEXT, author TEXT
             );",
        )
        .unwrap();

        assert!(find_author_all_pages(&conn, "Nobody").unwrap().is_empty());
    }
```

Append inside `mod tests` in `src/input/actions/pickers.rs`:

```rust
    /// Tightest -> widest, wrapping. Mirrors GlossPickerFilter's cycle test.
    #[test]
    fn journal_picker_scope_cycles_scene_work_author() {
        use super::JournalPickerScope as S;
        assert_eq!(S::Scene.next(), S::Work);
        assert_eq!(S::Work.next(), S::Author);
        assert_eq!(S::Author.next(), S::Scene);
    }

    /// The picker always OPENS on Work — today's behavior, so existing
    /// muscle memory is untouched.
    #[test]
    fn journal_picker_scope_defaults_to_work() {
        assert_eq!(super::JournalPickerScope::default(), super::JournalPickerScope::Work);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test --bins author_scope journal_picker_scope 2>&1 | tail -20
```

Expected: FAIL TO COMPILE — `cannot find function find_author_all_pages` and
`cannot find type JournalPickerScope`.

- [ ] **Step 3: Write the enum**

Add to `src/input/actions/pickers.rs`, beside `GlossPickerFilter`:

```rust
/// Which slice of the journal the Q&A picker lists. Cycled in place with
/// Alt+t while the picker is open (the same bind the gloss picker uses for
/// its type filter). Tightest -> widest, wrapping.
///
/// NOT persisted: the picker always opens on `Work`, which is what it
/// listed before scopes existed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum JournalPickerScope {
    /// Entries anchored to the cursor's (div1, div2) band — scene + passage.
    Scene,
    /// Every entry for the current work. The default, and the pre-scope
    /// behavior.
    #[default]
    Work,
    /// Every entry from every work by this work's author, plus that
    /// author's corpus notes.
    Author,
}

impl JournalPickerScope {
    pub(crate) fn next(self) -> Self {
        match self {
            JournalPickerScope::Scene => JournalPickerScope::Work,
            JournalPickerScope::Work => JournalPickerScope::Author,
            JournalPickerScope::Author => JournalPickerScope::Scene,
        }
    }

    /// Suffix for the picker header, so the active scope is always visible.
    pub(crate) fn label(self) -> &'static str {
        match self {
            JournalPickerScope::Scene => "SCENE",
            JournalPickerScope::Work => "WORK",
            JournalPickerScope::Author => "AUTHOR",
        }
    }
}
```

Add the field to `AppState` in `src/app/mod.rs`, beside
`gloss_picker_filter`, and initialize it with
`JournalPickerScope::default()` in the same place `gloss_picker_filter` is
initialized.

- [ ] **Step 4: Write the author query**

Add to `src/db/journal.rs`, after `find_author_pages`:

```rust
/// Every journal entry attributable to `author`, paired with the
/// `work_abbrev` it belongs to: entries from ALL of the author's works, plus
/// that author's corpus notes.
///
/// Two keying schemes are unioned here. Ordinary entries store a WORK ABBREV
/// in `work_abbrev` and are joined to `works` on the author; corpus notes
/// (`scope='author'`, see `save_author_page`) store the AUTHOR NAME in that
/// same column and are selected directly. Missing the second half silently
/// drops corpus notes from author scope.
pub fn find_author_all_pages(
    conn: &Connection,
    author: &str,
) -> Result<Vec<(JournalPage, String)>, rusqlite::Error> {
    let sql = format!(
        "SELECT {JOURNAL_PAGE_COLUMNS}, j.work_abbrev \
           FROM journal_entries j \
           JOIN works w ON w.abbrev = j.work_abbrev \
          WHERE w.author = ?1 AND j.scope != 'author' \
         UNION ALL \
         SELECT {JOURNAL_PAGE_COLUMNS}, j.work_abbrev \
           FROM journal_entries j \
          WHERE j.work_abbrev = ?1 AND j.scope = 'author' \
         ORDER BY timestamp ASC, id ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([author], |row| {
        let page = map_journal_page_row(row)?;
        let work: String = row.get(11)?;
        Ok((page, work))
    })?;
    rows.collect()
}
```

`JOURNAL_PAGE_COLUMNS` yields 11 columns (indices 0-10), so the appended
`work_abbrev` is index 11. If a column is ever added to that const, this
index moves — verify it against the const rather than trusting this comment.

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test --bins author_scope journal_picker_scope 2>&1 | tail -10
cargo test --bins 2>&1 | tail -3
```

Expected: the four new tests PASS; full suite still green (baseline + 4).

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/pickers.rs src/db/journal.rs src/app/mod.rs
git commit -m "feat(journal): JournalPickerScope + author-scope query

The picker listed exactly one thing: every entry for the current work.
Adds the scope enum (scene -> work -> author, defaulting to work) and the
author query, which unions cross-work entries with the author's corpus
notes — those store the AUTHOR NAME in work_abbrev, so a single-select
query would silently drop them.

Data layer only; nothing consumes these yet.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 2: Scope-aware picker rows

Teach the picker to build its list from any of the three scopes, and to
label author rows with their work.

**Files:**
- Modify: `src/ui/journal_picker.rs` (`JournalRow`, header label retention, `populate_list`)
- Modify: `src/input/actions/journal.rs` (`populate_and_show_picker`, ~line 3014)

**Interfaces:**
- Consumes: `JournalPickerScope` and `find_author_all_pages` from Task 1;
  `find_scene_band_pages` and `find_all_pages_ordered`, both existing.
- Produces, used by Task 3:
  - `JournalRow.work_label: Option<String>`
  - `JournalQaPicker::set_header_scope(&self, scope_label: &str)`
  - `populate_and_show_picker(s: &mut AppState) -> bool` reads
    `s.journal_picker_scope` instead of always using the work query.

- [ ] **Step 1: Retain the header label**

In `src/ui/journal_picker.rs`, the header's `Label` is currently discarded:

```rust
        let (header_box, _header_title) =
            crate::ui::picker_nav::build_picker_header("Q&A PAGES");
```

Bind it, store it on the struct (add `header_title: Label` to
`JournalQaPicker` and to the constructor's initializer — `library_picker.rs`
already does exactly this at its line 118), and add:

```rust
    /// Retitle the header so the active scope is always visible. Three
    /// different list contents behind one unlabeled title is unreadable.
    pub fn set_header_scope(&self, scope_label: &str) {
        self.header_title.set_label(&format!("Q&A PAGES — {scope_label}"));
    }
```

Import `Label` in that file's `gtk4::{...}` use list.

- [ ] **Step 2: Add the work label to rows**

In `src/ui/journal_picker.rs`, add to `JournalRow`:

```rust
    /// `Some(work title)` in AUTHOR scope only — it is the one cross-work
    /// list, where two identically-worded questions from different works are
    /// otherwise indistinguishable. `None` in scene/work scope leaves the
    /// row rendering exactly as before.
    pub work_label: Option<String>,
```

In `populate_list`, prefix the primary label when present, and include it in
the filter target so typing a work name narrows by work:

```rust
        for (idx, item) in self.items.iter().enumerate() {
            let primary = match &item.work_label {
                Some(w) => format!("{} · {}", w, item.question_prefix),
                None => item.question_prefix.clone(),
            };
            if !filter.is_empty() {
                let target = format!("{} {}", item.scene_label, primary).to_lowercase();
                if !crate::ui::picker_filter::subsequence_match(&filter_lower, &target) {
                    continue;
                }
            }

            let hbox = crate::ui::picker_nav::two_label_row(&primary, &item.scene_label);
            let row = ListBoxRow::builder().child(&hbox).build();
            row.set_widget_name(&idx.to_string());
            self.list_box.append(&row);
        }
```

- [ ] **Step 3: Add the empty-state row**

Scene scope is legitimately empty on most chapters, so this path is common,
not exceptional. At the top of `populate_list`, after `clear_list`, follow
`RecentQaPicker::populate_list` (`src/ui/recent_qa_picker.rs:91-101`):

```rust
        if self.items.is_empty() {
            let hbox = crate::ui::picker_nav::two_label_row(
                "No Q&A in this scope — Alt+t to widen.",
                "",
            );
            let row = ListBoxRow::builder().child(&hbox).build();
            row.set_selectable(false);
            row.set_activatable(false);
            self.list_box.append(&row);
            return;
        }
```

A non-selectable row means `selected_index()` returns `None`, which
`confirm_picker` already handles by re-rendering rather than crashing —
confirm that branch still reads correctly before moving on.

- [ ] **Step 4: Make `populate_and_show_picker` scope-aware**

In `src/input/actions/journal.rs`, `populate_and_show_picker` currently
always calls `find_all_pages_ordered`. Restructure so the page list is chosen
by `s.journal_picker_scope`, keeping the existing row-mapping logic (band
resolution, `is_passage` labeling, `first_passage_line`, the 80-char prefix)
intact for all three scopes — only the SOURCE of pages changes, plus
`work_label`:

- `Scene` — `current_scene_divs(s)` then
  `find_scene_band_pages(conn, &work_abbrev, d1, d2)`; `work_label: None`.
- `Work` — `find_all_pages_ordered(conn, &work_abbrev)`, exactly as today;
  `work_label: None`.
- `Author` — `find_author_all_pages(conn, &author)` where `author` is
  `s.current_work.as_ref().map(|w| w.author.clone())`; `work_label:
  Some(title)` resolved from the returned `work_abbrev`.

Keep `work_abbrev` as `current_work_abbrev(s)` (the canonical abbrev) for the
scene and work scopes — every journal path keys by canonical abbrev.

The empty case no longer returns `false` with a toast: an empty scope now
shows the empty-state row so Alt+t can widen from it. Return `true` in every
scope and let the picker open. **Check both callers** —
`open_picker_from_reader` uses the `false` return to roll back
`return_pos`/`picker_from_reader`; that rollback is now dead for the empty
case and must be removed or the state will be left half-set. `open_picker`
ignores the return value.

Call `s.journal_picker.set_header_scope(s.journal_picker_scope.label())`
before `show()`.

Reset the scope to `Work` on every open (both entry points), since the spec
says the picker always opens on Work.

- [ ] **Step 5: Verify the build and suite**

```bash
cargo build 2>&1 | tail -2
cargo test --bins 2>&1 | tail -3
cargo clippy 2>&1 | rg -c "^error" || echo "0 clippy errors"
```

Expected: clean build, suite green, 0 clippy errors. The picker still opens
on Work and behaves exactly as before — no bind exists yet to reach the other
scopes.

- [ ] **Step 6: Commit**

```bash
git add src/ui/journal_picker.rs src/input/actions/journal.rs
git commit -m "feat(journal): picker builds its rows from any scope

populate_and_show_picker now reads AppState.journal_picker_scope and sources
pages from the scene band, the work, or the author. Author rows carry a work
label (the one cross-work list) following RecentQaPicker's convention, and
the header shows the active scope. An empty scope shows a non-selectable
empty-state row instead of refusing to open, so Alt+t can widen from it.

Still opens on Work; no bind reaches the other scopes yet.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 3: The Alt+t bind

**Files:**
- Modify: `src/input/actions/pickers.rs` (the cycle handler)
- Modify: `src/input/keymap.rs` (the `InputMode::JournalPicker` arm, beside `GlossPicker` at ~line 1027)

**Interfaces:**
- Consumes: `JournalPickerScope::next`/`label` (Task 1),
  `set_header_scope` and the scope-aware `populate_and_show_picker` (Task 2).
- Produces: `pub(crate) fn cycle_journal_picker_scope(state: &Rc<RefCell<AppState>>)`.

- [ ] **Step 1: Write the cycle handler**

Add to `src/input/actions/pickers.rs`. Unlike `toggle_gloss_picker_type` this
needs no async work — all three queries are small and synchronous, and the
picker is already open:

```rust
/// Alt+t inside the Q&A picker: advance the scope, rebuild the list, retitle
/// the header. Selection resets to the first row and the filter is cleared —
/// the row sets differ between scopes, so preserving either is meaningless,
/// and a stale filter silently hiding a scope's contents reads as "empty".
pub(crate) fn cycle_journal_picker_scope(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    s.journal_picker_scope = s.journal_picker_scope.next();
    crate::input::actions::journal::repopulate_picker_for_scope(&mut s);
}
```

`repopulate_picker_for_scope` is the rebuild half of
`populate_and_show_picker` — extract it in `journal.rs` as
`pub(crate) fn repopulate_picker_for_scope(s: &mut AppState)` so both the
open path and the cycle path share one implementation rather than diverging.
It must: rebuild `items` for the current scope, call `set_header_scope`,
clear the search entry text, and re-`populate_list("")`.

Clearing the entry via `set_text("")` fires `connect_changed` synchronously,
which borrows `AppState` — the `project_picker_signal_refcell_crash` class,
a non-unwinding abort if the handler used a plain `borrow()` under an active
`borrow_mut`. **Already verified safe for this picker:** the handler at
`src/app/mod.rs:3014` uses `if let Ok(st) = ...try_borrow()`, so clearing
under `borrow_mut` is a no-op re-populate rather than an abort. Do not
"simplify" that `try_borrow` to a `borrow`.

- [ ] **Step 2: Wire the key**

In `src/input/keymap.rs`, beside the existing `InputMode::GlossPicker` arm:

```rust
                InputMode::JournalPicker => {
                    // Alt+t cycles the scope (scene -> work -> author), the
                    // same bind and the same reason as the gloss picker's
                    // type filter: Alt combos don't type into the search
                    // entry, so no focus guard is needed.
                    if is_alt && key_name == "t" {
                        crate::input::actions::pickers::cycle_journal_picker_scope(state);
                        return true;
                    }
                }
```

- [ ] **Step 3: Verify**

```bash
cargo build 2>&1 | tail -2
cargo test --bins 2>&1 | tail -3
cargo clippy 2>&1 | rg -c "^error" || echo "0 clippy errors"
```

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/pickers.rs src/input/keymap.rs src/input/actions/journal.rs
git commit -m "feat(journal): Alt+t cycles the Q&A picker scope

Same bind, same modal placement, and the same no-focus-guard-needed reason
as the gloss picker's Alt+t type filter. Cycling clears the filter text and
resets the selection: the row sets differ between scopes, and a stale filter
silently hiding a scope reads as 'this scope is empty'.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 4: Cross-work confirm from author scope

In author scope the selected entry may belong to another work, which
`confirm_picker` does not currently handle — it assumes the current work and
would land the overlay on a band of the wrong work.

**Files:**
- Modify: `src/input/actions/journal.rs` (`confirm_picker`, ~line 3110)
- Modify: `src/input/keymap.rs` (the `JournalPicker` confirm arm, ~line 972, if it must pass `tokio_handle`)

**Interfaces:**
- Consumes: `JournalRow.work_label` and the row's owning work (Task 2);
  `crate::input::actions::pickers::load_arkangel_edition_then` and
  `crate::input::actions::corpus_search::open_journal_hit`, both used by
  `confirm_recent_qa_picker` (`journal.rs:3187`) for exactly this purpose.
- Produces: no new public surface.

- [ ] **Step 1: Carry the owning work on the row**

`JournalRow` needs the entry's `work_abbrev` (not just the display label) so
confirm can load it. Add `pub work_abbrev: Option<String>` — `None` when the
row belongs to the current work (scene/work scope, and same-work rows in
author scope), `Some(abbrev)` for a cross-work row. Populate it in Task 2's
author branch.

- [ ] **Step 2: Branch confirm on the owning work**

In `confirm_picker`, after resolving the selected row: when `work_abbrev` is
`None`, keep today's exact path (`land_on_page(&mut s, band, target_id)`).
When it is `Some(other)`, follow `confirm_recent_qa_picker`'s shape — hide
the picker, drop to `InputMode::Reader`, then:

```rust
    crate::input::actions::pickers::load_arkangel_edition_then(
        state,
        tokio_handle,
        other_abbrev,
        current_abbrev,
        move |state| crate::input::actions::corpus_search::open_journal_hit(state, target_id, ""),
    );
```

Do NOT duplicate that loader's logic — it already handles the same-work skip,
MPV media discovery, and the error toast. `confirm_picker` will need
`tokio_handle` threaded in from its `keymap.rs` call site, which
`confirm_recent_qa_picker` already receives at the adjacent arm.

- [ ] **Step 3: Verify**

```bash
cargo build 2>&1 | tail -2
cargo test --bins 2>&1 | tail -3
cargo clippy 2>&1 | rg -c "^error" || echo "0 clippy errors"
```

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/journal.rs src/input/keymap.rs
git commit -m "feat(journal): picker confirm loads the entry's work in author scope

Author scope is cross-work, so a confirmed entry may live in another work.
Reuses load_arkangel_edition_then + open_journal_hit — the path
confirm_recent_qa_picker already uses for the same problem — rather than
duplicating the loader. Same-work rows keep today's exact land_on_page path.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 5: Keybind surfaces + consistency log

Required in the same branch as the bind, per CLAUDE.md.

**Files:**
- Modify: `src/ui/journal_keybinds_overlay.rs` (`GROUPS`)
- Modify: `docs/guides/keybind-consistency-guide.md` (change log)

**Interfaces:** none — documentation and legend text.

- [ ] **Step 1: Add the bind to the journal overlay legend**

`Ctrl+\` opens this picker from the journal overlay, so that overlay's Ctrl+/
legend must list the new bind. Add to the appropriate group in `GROUPS` (the
file's existing rows read like
`("Alt+s", "JournalBand::Scene (cursor scene)")`):

```rust
        ("Alt+t", "Q&A picker: cycle scope (scene → work → author)"),
```

Two surfaces deliberately NOT touched — do not add them:
- `src/ui/keybinds_overlay.rs` (main-card Ctrl+/) lists main-card binds only;
  Alt+t here is picker-modal.
- `~/.config/linux-lit/keymap.json` — this bind lives in the picker's modal
  arm in `keymap.rs`, not in `keymap_config.rs`, exactly like the gloss
  picker's Alt+t.

- [ ] **Step 2: Run the cross-reference self-check**

Run the `update-cairo-keybinds-overlay` skill's three-pass cross-reference to
confirm nothing drifted between the handler, the legends, and the compiled
defaults.

- [ ] **Step 3: Record the consistency decision**

Append to the change log in `docs/guides/keybind-consistency-guide.md`: Alt+t
now means "cycle this picker's filter/scope" in both the gloss picker and the
Q&A picker — one concept, one key, across two surfaces.

- [ ] **Step 4: Commit**

```bash
git add src/ui/journal_keybinds_overlay.rs docs/guides/keybind-consistency-guide.md
git commit -m "docs(keybinds): Alt+t Q&A picker scope in the journal legend

Alt+t now means 'cycle this picker's filter/scope' in both the gloss picker
and the Q&A picker — one concept, one key. Main-card overlay and keymap.json
deliberately untouched: this bind is picker-modal, handled in keymap.rs.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 6: Headless on-screen verification

Non-waivable. The spec's acceptance criterion is visual: the header changes,
the row set changes, and author rows carry a work label.

**Files:** none. This task produces evidence.

- [ ] **Step 1: Launch headless on a work with entries in all three scopes**

BH-Barrett chapter 2 works: chapter 2 has a scene-band entry, BH has 11
entries, and Charles Dickens has entries across 2 works.

```bash
export XDG_RUNTIME_DIR=$(mktemp -d)
cd ~/utono/linux-lit-wt/<branch>
./scripts/land-on.sh BH-Barrett 2.0
```

`land-on.sh` takes `WORK div1.div2 [journal|synopsis|gloss]`, sets its own
hermetic env (private DB copy, private log), and does NOT need `e2e-env.sh`.
Launch with the harness `run_in_background`. It prints `WAYLAND_DISPLAY=` and
`log=` — use those.

- [ ] **Step 2: Open the picker and capture each scope**

```bash
wtype -M ctrl -k j -m ctrl     # opens the picker (reader mode)
sleep 2 && grim "$SCRATCH/picker-work.png"
wtype -M alt -k t -m alt       # -> author
sleep 2 && grim "$SCRATCH/picker-author.png"
wtype -M alt -k t -m alt       # -> scene
sleep 2 && grim "$SCRATCH/picker-scene.png"
```

Confirm each `KEY:` line landed in the log before trusting the screenshot.
`Ctrl+j` in reader mode maps directly to `Action::OpenJournalPicker`
(verified in `keymap_config.rs:364`) — it always opens the picker, with no
segment-dependent branch.

- [ ] **Step 3: Open all three PNGs and report what you see**

Per the UI review protocol, report inline. Verify against the spec:
- Header reads `Q&A PAGES — WORK`, then `— AUTHOR`, then `— SCENE`.
- The row set genuinely differs between the three.
- Author rows carry a work label; work/scene rows do NOT.
- No clipping, no empty card where rows were expected.

- [ ] **Step 4: Test the empty scene scope**

Land on a chapter with no scene-band entry and cycle to Scene. **BH chapter 5
is verified to have zero** (`land-on.sh BH-Barrett 5.0`). Confirm the
empty-state row appears — "No Q&A in this scope — Alt+t to widen." — and that
Alt+t from there still widens rather than dismissing the picker.

- [ ] **Step 5: Test cross-work confirm**

In Author scope, select a row belonging to the OTHER Dickens work and press
Enter. Confirm the other work loads and the journal overlay opens on that
entry. This is Task 4's only real exercise — the unit tests cannot cover it.

- [ ] **Step 6: Clean up**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

Exactly that pattern — a bare `pkill -f target/debug/linux-lit` kills the
user's live instance.

- [ ] **Step 7: Report**

State what was observed, with the screenshots. If a launch genuinely fails
after a retry, hand off manual steps.

---

## Finishing

Per CLAUDE.md: merge back to master locally, then push — no PR, no asking.

1. `cargo build`, `cargo clippy`, `cargo test --bins` green; tree clean.
2. `git checkout master && git merge --no-ff <branch>`
3. Re-verify the build on master.
4. `git push origin master`
5. `git worktree remove` the worktree, then `git branch -d <branch>`.

**This branch DOES meet the spec threshold** (a new mode/axis, multiple
surfaces, a keybind), so `superpowers:requesting-code-review` runs before
merge — unless the user waives review, in which case the Task 6 on-screen run
becomes the only remaining gate and is mandatory.

## Follow-ups (NOT this branch)

- **Upstream litdb:** whatever rewrites `scope` and renumbers bands on
  re-import.
- Persisting the picker scope across opens, if the always-open-on-Work
  default proves annoying in use.
