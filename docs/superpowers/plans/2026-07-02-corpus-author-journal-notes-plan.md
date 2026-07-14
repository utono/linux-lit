# Corpus/author-scope journal notes + Markdown — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add author/corpus-scope journal entries (`scope='author'`), an explicit `kind` flag (`qa`|`note`) that suppresses the `Q:` prefix for imported notes, raw-Markdown storage rendered via pulldown-cmark → GTK TextTags, an `Alt+a` jump to the author band, and an import skill — so `loading-the-cry.md` files in.

**Architecture:** Reuse the existing `journal_entries` table (Approach 1): author rows key by the author string in `work_abbrev` with a new `div1=div2=-2` sentinel. A new `kind` column (idempotent `ADD COLUMN`, default `'qa'`) drives display. A new `src/ui/markdown.rs` module parses CommonMark and applies TextTags to the journal `TextView`. Author-band nav mirrors the existing `Alt+w` → work-band pattern; it is a jump target, not part of the sequential band walk.

**Tech Stack:** Rust, GTK4 (`gtk4`, `sourceview5`), `rusqlite`, `pulldown-cmark` (new dep). SQLite at `~/utono/litdb/data/lit.db`.

## Global Constraints

- **Do NOT run the app** — verify with `cargo build` / `cargo test --bins`; the user runs `cargo run` and the e2e harness. (linux-lit CLAUDE.md)
- **Author rows key by the author string** in `work_abbrev`, sentinel `div1=div2=-2` = new const `JOURNAL_AUTHOR_DIV`. Existing `JOURNAL_WORK_DIV=(-1,-1)`.
- **`kind` column:** `TEXT NOT NULL DEFAULT 'qa'`, values `'qa'` | `'note'`. Added via the idempotent migration in `ensure_journal_table`.
- **`answer` stores raw Markdown.** Never store rendered/tagged form.
- **Journal-overlay modal keys live in `handle_journal_key` (`src/input/keymap.rs`), NOT in `keymap_config.rs`/`keymap.json`.** So the `Alt+a` bind needs NO keymap.json/stow change — only the two keybind legends.
- **Any keybind change updates BOTH the journal legend (`src/ui/journal_keybinds_overlay.rs` `GROUPS`) and the Ctrl+/ reader overlay.** (linux-lit CLAUDE.md; use the `update-cairo-keybinds-overlay` skill for the latter.)
- **Timestamps:** US Central — `TZ='America/Chicago' date +"%Y-%m-%dT%H:%M:%SZ"`.
- Commit messages end with the Co-Authored-By + Claude-Session trailer (see repo convention). After each commit, this plan's caller updates `ac`.

## File Structure

- `Cargo.toml` — add `pulldown-cmark` dep. (Task 1)
- `src/db/journal.rs` — `kind` column + migration; `JournalPage.kind`; `save_journal_page` gains `kind`; new `save_author_page` / `find_author_pages`; `move_journal_page` author arm is caller-side (no change here). (Tasks 1–3)
- `src/app/mod.rs` — `JournalBand::Author(String)`; `JOURNAL_AUTHOR_DIV`. (Task 4)
- `src/input/actions/journal.rs` — `band_for_page` author arm; `footer_left_text` author arm; `nav_to_author_band`; `render_current` author arm; `ask_claude` author save; `move_journal_page` target mapping author arm; thread `kind` into `show_page`/`enter_edit_buffer`. (Tasks 4–7)
- `src/ui/markdown.rs` — NEW: CommonMark → TextTag renderer. (Task 8)
- `src/ui/journal_overlay.rs` — `kind`-driven prefix; render bodies through markdown. (Tasks 6, 9)
- `src/input/vim/journal_doc.rs` — note raw-MD buffer round-trip. (Task 7)
- `src/input/keymap.rs` — `Alt+a` → `nav_to_author_band` in `handle_journal_key`. (Task 5)
- `src/ui/journal_keybinds_overlay.rs` + Ctrl+/ reader overlay — legends. (Task 5)
- `.claude/skills/import-corpus-note/SKILL.md` — import skill. (Task 10)

---

### Task 1: Add `kind` column + `pulldown-cmark` dep

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/db/journal.rs` (JOURNAL_PAGE_COLUMNS, map_journal_page_row, JournalPage, ensure_journal_table, save_journal_page)
- Test: `src/db/journal.rs` `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `JournalPage.kind: String`; `save_journal_page(conn, work_abbrev, div1, div2, question, answer, claude_model, scope, kind)` (new trailing `kind: &str` param).

- [ ] **Step 1: Add the dependency.** In `Cargo.toml`, under `[dependencies]`, add:

```toml
pulldown-cmark = { version = "0.12", default-features = false }
```

Run: `cargo fetch` — Expected: resolves `pulldown-cmark v0.12.x`.

- [ ] **Step 2: Write a failing test for the `kind` column round-trip.** Add to `src/db/journal.rs` tests module:

```rust
#[test]
fn kind_defaults_to_qa_and_roundtrips() {
    let conn = mem();
    // Old-style insert path (scene) must default kind to 'qa'.
    let id = save_journal_page(&conn, "Ham", 1, 2, "Q?", "A.", "m", "scene", "qa").unwrap();
    let pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].id, id);
    assert_eq!(pages[0].kind, "qa");
}
```

- [ ] **Step 3: Run it — expect a COMPILE failure** (missing `kind` field/param), which is the failing state.

Run: `cargo test --bins db::journal 2>&1 | tail -20`
Expected: compile error — `save_journal_page` takes 8 args / no field `kind`.

- [ ] **Step 4: Add the `kind` column to the schema + migration.** In `ensure_journal_table`, add `kind TEXT NOT NULL DEFAULT 'qa',` to the `CREATE TABLE` (after `source_text TEXT,`), and add an idempotent migration alongside the existing ones:

```rust
    let has_kind: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='kind'")?
        .exists([])?;
    if !has_kind {
        conn.execute_batch(
            "ALTER TABLE journal_entries ADD COLUMN kind TEXT NOT NULL DEFAULT 'qa';",
        )?;
    }
```

- [ ] **Step 5: Add `kind` to the column list, row struct, and mapper.**
  - In `JournalPage` (after `source_text`): `pub kind: String,`
  - In `JOURNAL_PAGE_COLUMNS`, append `, COALESCE(kind, 'qa')` (last column).
  - In `map_journal_page_row`, add `kind: row.get(10)?,` (index 10 = the new last column).

- [ ] **Step 6: Thread `kind` through `save_journal_page`.** Change its signature to add a trailing `kind: &str`, and update the INSERT:

```rust
pub fn save_journal_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    question: &str,
    answer: &str,
    claude_model: &str,
    scope: &str,
    kind: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO journal_entries
            (work_abbrev, div1, div2, question, answer, claude_model, scope, kind, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'))",
        rusqlite::params![work_abbrev, div1, div2, question, answer, claude_model, scope, kind],
    )?;
    Ok(conn.last_insert_rowid())
}
```

- [ ] **Step 7: Fix existing `save_journal_page` callers.** In the SAME test module, update the two existing calls in `scene_pages_roundtrip_and_exclude_work` (and any other test) to pass a trailing `"qa"`. Then update the production callers found in Task 6 later — but for THIS task, just add `"qa"` to every existing `save_journal_page(` call so the crate compiles. Search:

Run: `rg -n "save_journal_page\(" src/ | rg -v "pub fn"`
Add `, "qa"` before the closing `)` of each call whose scope arg is present.

- [ ] **Step 8: Run tests — expect PASS.**

Run: `cargo test --bins db::journal 2>&1 | tail -20`
Expected: `kind_defaults_to_qa_and_roundtrips` and the existing journal tests PASS.

- [ ] **Step 9: Full build.**

Run: `cargo build 2>&1 | tail -5`
Expected: builds clean (warnings ok).

- [ ] **Step 10: Commit.**

```bash
git add Cargo.toml Cargo.lock src/db/journal.rs
git commit -m "feat(journal): add kind column (qa|note) + pulldown-cmark dep"
```

---

### Task 2: `save_author_page` + `find_author_pages`

**Files:**
- Modify: `src/db/journal.rs`
- Test: `src/db/journal.rs` tests

**Interfaces:**
- Consumes: `JournalPage.kind`, `save_journal_page` (from Task 1).
- Produces: `save_author_page(conn, author, question, answer, claude_model, kind) -> Result<i64>`; `find_author_pages(conn, author) -> Result<Vec<JournalPage>>`.

- [ ] **Step 1: Write the failing test.** Add to the tests module:

```rust
#[test]
fn author_pages_roundtrip_and_exclude_work_scene() {
    let conn = mem();
    let nid = save_author_page(&conn, "Shakespeare", "", "## Cry\n\n**load** it", "m", "note").unwrap();
    save_author_page(&conn, "Shakespeare", "Corpus Q?", "Corpus A.", "m", "qa").unwrap();
    // A scene page for an actual work must NOT appear in author queries.
    save_journal_page(&conn, "Ham", 1, 2, "SQ?", "SA.", "m", "scene", "qa").unwrap();

    let pages = find_author_pages(&conn, "Shakespeare").unwrap();
    assert_eq!(pages.len(), 2);
    let note = pages.iter().find(|p| p.id == nid).unwrap();
    assert_eq!(note.kind, "note");
    assert_eq!(note.question, "");
    assert_eq!(note.answer, "## Cry\n\n**load** it");
    assert_eq!(note.div1, -2);
    assert_eq!(note.div2, -2);
}
```

- [ ] **Step 2: Run it — expect compile failure** (`save_author_page`/`find_author_pages` undefined).

Run: `cargo test --bins db::journal::tests::author_pages 2>&1 | tail -15`
Expected: FAIL — cannot find function `save_author_page`.

- [ ] **Step 3: Implement both functions.** Add near `save_journal_page` in `src/db/journal.rs`:

```rust
/// The (div1, div2) sentinel that marks an author/corpus-scope journal row.
/// Distinct from JOURNAL_WORK_DIV (-1,-1) so author rows never collide with
/// whole-work rows. `work_abbrev` holds the AUTHOR string for these rows.
pub const AUTHOR_DIV: (i64, i64) = (-2, -2);

pub fn save_author_page(
    conn: &Connection,
    author: &str,
    question: &str,
    answer: &str,
    claude_model: &str,
    kind: &str,
) -> Result<i64, rusqlite::Error> {
    save_journal_page(
        conn, author, AUTHOR_DIV.0, AUTHOR_DIV.1, question, answer, claude_model, "author", kind,
    )
}

pub fn find_author_pages(
    conn: &Connection,
    author: &str,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {JOURNAL_PAGE_COLUMNS} \
         FROM journal_entries \
         WHERE work_abbrev = ?1 AND scope = 'author' \
         ORDER BY timestamp ASC, id ASC",
    ))?;
    let rows = stmt.query_map(rusqlite::params![author], map_journal_page_row)?;
    rows.collect()
}
```

- [ ] **Step 4: Run tests — expect PASS.**

Run: `cargo test --bins db::journal 2>&1 | tail -15`
Expected: `author_pages_roundtrip_and_exclude_work_scene` PASS; all journal tests PASS.

- [ ] **Step 5: Commit.**

```bash
git add src/db/journal.rs
git commit -m "feat(journal): save_author_page + find_author_pages (scope='author')"
```

---

### Task 3: `move_journal_page` supports the author target (no code change; verify)

**Files:**
- Test: `src/db/journal.rs` tests

**Interfaces:**
- Consumes: existing `move_journal_page(conn, id, scope, div1, div2)`.

`move_journal_page` is already generic (writes whatever `scope`/`div1`/`div2` it's given), so moving an entry TO the author band needs no DB change — only the caller-side band→(scope,div) mapping in Task 6. Lock that in with a test.

- [ ] **Step 1: Write the test.**

```rust
#[test]
fn move_to_author_band_sets_scope_and_sentinel() {
    let conn = mem();
    let id = save_journal_page(&conn, "Ham", 1, 2, "Q?", "A.", "m", "scene", "qa").unwrap();
    move_journal_page(&conn, id, "author", AUTHOR_DIV.0, AUTHOR_DIV.1).unwrap();
    // NOTE: move keeps work_abbrev; author-band lookups key by work_abbrev, so a
    // moved-from-a-work page keys under the WORK abbrev, not the author. That's
    // acceptable: the move picker is out of scope for author here (Task 6 does
    // not add an Author move target). This test documents move_journal_page is
    // scope-agnostic and needs no change.
    let n: i64 = conn
        .query_row("SELECT div1 FROM journal_entries WHERE id=?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(n, -2);
}
```

- [ ] **Step 2: Run — expect PASS** (no impl change needed).

Run: `cargo test --bins db::journal::tests::move_to_author 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 3: Commit.**

```bash
git add src/db/journal.rs
git commit -m "test(journal): confirm move_journal_page is scope-agnostic for author"
```

---

### Task 4: `JournalBand::Author` + band mapping + footer

**Files:**
- Modify: `src/app/mod.rs` (JournalBand enum, JOURNAL_AUTHOR_DIV const)
- Modify: `src/input/actions/journal.rs` (band_for_page, footer_left_text)
- Test: `src/input/actions/journal.rs` tests (band_for_page test)

**Interfaces:**
- Produces: `JournalBand::Author(String)`; `JOURNAL_AUTHOR_DIV: (i64,i64) = (-2,-2)`.

- [ ] **Step 1: Add the enum variant + const.** In `src/app/mod.rs`, extend the enum (keep existing derives `#[derive(Clone, Debug, PartialEq, Eq)]`):

```rust
pub enum JournalBand {
    Work,
    Scene(i64, i64),
    Passage { div1: i64, div2: i64, start: String, end: String },
    /// Author/corpus band: holds scope='author' pages keyed by the author name.
    Author(String),
}
```

And beside `JOURNAL_WORK_DIV` (line ~3945):

```rust
pub(crate) const JOURNAL_AUTHOR_DIV: (i64, i64) = (-2, -2);
```

- [ ] **Step 2: Write the failing band-mapping test.** In `src/input/actions/journal.rs` tests, extend `band_for_page_classifies_work_and_scene_passages_share_scene_band` OR add:

```rust
#[test]
fn band_for_page_classifies_author() {
    // An author page arrives with div1=div2=-2. band_for_page must classify it
    // as Author using the page's work_abbrev — but band_for_page only sees a
    // JournalPage (no work_abbrev field), so author classification keys on the
    // -2 sentinel and the Author name is supplied by the caller (render_current).
    // Here we assert the sentinel routes to the Work-vs-Author branch correctly.
    assert_eq!(band_for_page(&page(-2, -2, None, None)), JournalBand::Author(String::new()));
}
```

- [ ] **Step 3: Run — expect FAIL** (band_for_page has no Author arm).

Run: `cargo test --bins actions::journal::tests::band_for_page_classifies_author 2>&1 | tail -12`
Expected: FAIL (assertion / non-exhaustive match compile error).

- [ ] **Step 4: Add the `band_for_page` author arm.** `JournalPage` has no `work_abbrev`, so `band_for_page` returns `Author(String::new())` on the `-2` sentinel; the real name is filled by `render_current` (which knows the author). Update `band_for_page` (around line 90):

```rust
fn band_for_page(p: &crate::db::journal::JournalPage) -> JournalBand {
    if p.div1 == crate::app::JOURNAL_AUTHOR_DIV.0 && p.div2 == crate::app::JOURNAL_AUTHOR_DIV.1 {
        JournalBand::Author(String::new())
    } else if p.div1 < 0 {
        JournalBand::Work
    } else if let (Some(start), Some(end)) = (p.start_citation.clone(), p.end_citation.clone()) {
        JournalBand::Passage { div1: p.div1, div2: p.div2, start, end }
    } else {
        JournalBand::Scene(p.div1, p.div2)
    }
}
```

(Order matters: the `-2` check MUST precede `p.div1 < 0`, since -2 < 0.)

- [ ] **Step 5: Add the `footer_left_text` author arm.** Around line 115:

```rust
        JournalBand::Author(name) => format!("{} \u{00b7} corpus", name),
```

Add this arm to the `match band` in `footer_left_text`. (The `abbrev` param is ignored for the author arm — the name comes from the band.)

- [ ] **Step 6: Fix any now-non-exhaustive matches on `JournalBand`.** Build and follow the compiler:

Run: `cargo build 2>&1 | rg -n "non-exhaustive|not covered|JournalBand" | head`
For each site the compiler flags (e.g. `target_bands`, `band_for_rewrite`, `move_target_rows`, the `move_journal_page` mapping in journal.rs ~line 1173, `begin_ask` prompt text ~line 493), add an `Author` arm. For arms where author is not a valid target (the sequential band walk in `target_bands`, the move picker rows), simply skip/ignore it:
  - `target_bands`: no `Author` push (author is jump-only) — add `JournalBand::Author(_) => {}` if the match is exhaustive, else leave (it starts from `Work`).
  - `band_for_rewrite`: `JournalBand::Author(name) => (author rewrite)` — mirror Work: `if p.div1 == JOURNAL_AUTHOR_DIV.0 ... => JournalBand::Author(...)`. Since band_for_rewrite is built from a page too, reuse the same sentinel check as band_for_page.
  - The `move_journal_page` scope mapping (~1173): add `JournalBand::Author(_) => ("author", crate::app::JOURNAL_AUTHOR_DIV.0, crate::app::JOURNAL_AUTHOR_DIV.1),`.
  - `begin_ask` prompt (~493): `JournalBand::Author(_) => "Ask a question about this author's corpus",`.

- [ ] **Step 7: Run the band test — expect PASS + clean build.**

Run: `cargo test --bins actions::journal 2>&1 | tail -15 && cargo build 2>&1 | tail -3`
Expected: author band test PASS; build clean.

- [ ] **Step 8: Commit.**

```bash
git add src/app/mod.rs src/input/actions/journal.rs
git commit -m "feat(journal): JournalBand::Author variant + band/footer mapping"
```

---

### Task 5: `Alt+a` jump to author band + legends

**Files:**
- Modify: `src/input/actions/journal.rs` (nav_to_author_band)
- Modify: `src/input/keymap.rs` (handle_journal_key alt branch)
- Modify: `src/ui/journal_keybinds_overlay.rs` (GROUPS legend)
- Modify: Ctrl+/ reader overlay (via update-cairo-keybinds-overlay skill)
- Test: `src/input/actions/journal.rs` tests

**Interfaces:**
- Consumes: `JournalBand::Author`, `render_current` (Task 6 provides the author render arm; until then `nav_to_author_band` sets the band and calls `render_current`, which Task 6 completes).
- Produces: `pub(crate) fn nav_to_author_band(state: &Rc<RefCell<AppState>>)`.

- [ ] **Step 1: Implement `nav_to_author_band`** next to `nav_to_work_band` (~line 417):

```rust
pub(crate) fn nav_to_author_band(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    let author = s
        .current_work
        .as_ref()
        .map(|w| w.author.clone())
        .unwrap_or_default();
    if author.is_empty() {
        return;
    }
    if s.journal_band == JournalBand::Author(author.clone()) {
        return;
    }
    s.journal_band = JournalBand::Author(author);
    s.journal.page_index = 0;
    render_current(&mut s);
}
```

- [ ] **Step 2: Wire `Alt+a` in `handle_journal_key`.** In the `if is_alt { match key_name {` block in `src/input/keymap.rs` (after the `"w"` arm), add:

```rust
            // Alt+a: jump to the author/corpus band (scope='author' pages for
            // the current work's author). A jump target, not part of the
            // sequential band walk (Alt+n/p scenes, Alt+w work).
            "a" => {
                crate::input::actions::journal::nav_to_author_band(state);
                return true;
            }
```

- [ ] **Step 3: Add a test that `nav_to_author_band` sets the band.** If a headless AppState harness exists in the journal tests, assert `journal_band == Author(author)`. If not (GTK state), skip a unit test here and rely on the build + the e2e check; note that inline. Prefer:

Run: `rg -n "fn .*AppState.*test|test_state|mk_state" src/input/actions/journal.rs`
If a constructor exists, write the assertion; otherwise add a comment `// nav_to_author_band verified via e2e (needs GTK AppState)` and move on.

- [ ] **Step 4: Update the journal legend.** In `src/ui/journal_keybinds_overlay.rs`, find the `GROUPS` const row containing `Alt+w` / "whole work" and add, in the same group, a row:

```rust
    ("Alt+a", "author corpus band"),
```

(Match the exact tuple shape used by the surrounding rows.)

- [ ] **Step 5: Update the Ctrl+/ reader overlay.** Invoke the `update-cairo-keybinds-overlay` skill to add/describe the `Alt+a` journal-band key in `src/ui/keybinds_overlay.rs` (keycap + `describe()` arm pointing to `nav_to_author_band — src/input/actions/journal.rs`). Follow that skill's three cross-reference passes.

- [ ] **Step 6: Build + test.**

Run: `cargo build 2>&1 | tail -3 && cargo test --bins 2>&1 | tail -5`
Expected: clean build; bins tests pass.

- [ ] **Step 7: Commit.**

```bash
git add src/input/actions/journal.rs src/input/keymap.rs src/ui/journal_keybinds_overlay.rs src/ui/keybinds_overlay.rs
git commit -m "feat(journal): Alt+a jumps to author/corpus band + legends"
```

---

### Task 6: `render_current` + `ask_claude` author arms; thread `kind` to display

**Files:**
- Modify: `src/input/actions/journal.rs` (render_current, ask_claude, show_page call, enter_edit_buffer call)
- Modify: `src/ui/journal_overlay.rs` (show_page + enter_edit_buffer signatures gain `kind`)
- Test: none new (integration; covered by build + e2e)

**Interfaces:**
- Consumes: `find_author_pages`, `save_author_page` (Tasks 1–2), `JournalBand::Author` (Task 4).
- Produces: `show_page(..., kind: &str)` and `enter_edit_buffer(..., kind: &str)` signatures.

- [ ] **Step 1: Add the `render_current` author arm.** In the `match s.journal_band.clone()` (~line 189), add:

```rust
        JournalBand::Author(ref name) => conn
            .and_then(|c| crate::db::journal::find_author_pages(&c, name).ok())
            .unwrap_or_default(),
```

- [ ] **Step 2: Fill the Author name after loading.** `band_for_page` returns `Author(String::new())`; when `render_current` lands a page (the `land_on_page` / `show_page` caller and the ~1021 `band = band_for_page(p)` site), if the resulting band is `Author("")`, replace it with the current author:

At the ~1021 site and any `band_for_page` consumer that stores the band, add right after:

```rust
        let band = match band_for_page(p) {
            JournalBand::Author(_) => JournalBand::Author(
                s.current_work.as_ref().map(|w| w.author.clone()).unwrap_or_default(),
            ),
            other => other,
        };
```

- [ ] **Step 3: Thread `kind` into `show_page`.** Change `show_page` in `src/ui/journal_overlay.rs` to take `kind: &str` (add after `answer: &str`), and change the body build:

```rust
            let full = if kind == "note" {
                answer.to_string()
            } else {
                format!("{}\n\n{}", prefix_question(question), answer)
            };
```

Update the `render_current` call (~line 258) to pass the current page's `kind`:

```rust
        .show_page(&footer_left, s.journal.page_index, count, &q, &a, &kind, cw, h);
```

where `kind` is read from the current page (`pages[s.journal.page_index].kind.clone()`, or `"qa".to_string()` when the band is empty).

- [ ] **Step 4: Add the `ask_claude` author save.** In `ask_claude` (~line 945) where the band match writes the row, add an `Author` arm mirroring the Work arm but calling `save_author_page(&conn, &author, &question, &answer, &model, "qa")` where `author = s.current_work...author`. (Find the existing `JournalBand::Work =>` save arm and add `JournalBand::Author(name) =>` next to it.)

- [ ] **Step 5: Build.**

Run: `cargo build 2>&1 | tail -6`
Expected: clean (fix any missed `show_page`/`enter_edit_buffer` caller — the `show_loading` path also calls a builder; ensure it still compiles, passing `"qa"` where a kind is needed).

- [ ] **Step 6: Commit.**

```bash
git add src/input/actions/journal.rs src/ui/journal_overlay.rs
git commit -m "feat(journal): render + ask at author scope; kind-driven Q: prefix"
```

---

### Task 7: Note raw-Markdown vim round-trip

**Files:**
- Modify: `src/input/vim/journal_doc.rs`
- Modify: `src/ui/journal_overlay.rs` (enter_edit_buffer gains `kind`)
- Modify: `src/input/actions/journal.rs` (begin_edit passes kind; :w save routes note→answer only)
- Test: `src/input/vim/journal_doc.rs` tests

**Interfaces:**
- Consumes: `JournalPage.kind`.
- Produces: `build_note_buffer(answer) -> String`, `parse_note_back(buffer) -> String`.

- [ ] **Step 1: Write failing tests.** In `journal_doc.rs` tests:

```rust
#[test]
fn note_buffer_is_raw_markdown_roundtrip() {
    let md = "## Cry\n\n- load it\n- **then** drop it";
    let b = build_note_buffer(md);
    assert_eq!(b, md); // no Q: seed, verbatim
    assert_eq!(parse_note_back(&b), md);
}
```

- [ ] **Step 2: Run — expect FAIL** (functions undefined).

Run: `cargo test --bins vim::journal_doc 2>&1 | tail -12`
Expected: FAIL — cannot find `build_note_buffer`.

- [ ] **Step 3: Implement.** Add to `journal_doc.rs`:

```rust
/// A `note` entry has no question and stores raw Markdown; its editor buffer is
/// the raw Markdown verbatim (no `Q:` seed line). Round-trips losslessly.
pub fn build_note_buffer(answer: &str) -> String {
    answer.to_string()
}

pub fn parse_note_back(buffer: &str) -> String {
    buffer.to_string()
}
```

- [ ] **Step 4: Thread `kind` into `enter_edit_buffer`** (`src/ui/journal_overlay.rs`): add `kind: &str`, and choose the buffer:

```rust
        let buf = if kind == "note" {
            crate::input::vim::journal_doc::build_note_buffer(answer)
        } else {
            crate::input::vim::journal_doc::build_buffer(question, answer)
        };
```

- [ ] **Step 5: Route the `:w` save for notes.** In the journal-edit save path (`submit_edit_rewrite` / the JournalEdit `:w` handler in `src/input/actions/journal.rs`), when the current page `kind == "note"`, parse with `parse_note_back` → the new answer, keep `question` empty, and UPDATE the row's `answer` only (reuse the existing update fn, or `save`/`update` path already used for edits). Mirror the existing qa branch; add a `kind`-gated split.

Run: `rg -n "enter_edit_buffer|parse_back|submit_edit_rewrite|fn begin_edit" src/input/actions/journal.rs`
Update `begin_edit` to pass the page's `kind` into `enter_edit_buffer`, and the save path to branch on it.

- [ ] **Step 6: Run tests + build.**

Run: `cargo test --bins vim::journal_doc 2>&1 | tail -8 && cargo build 2>&1 | tail -3`
Expected: note round-trip PASS; clean build.

- [ ] **Step 7: Commit.**

```bash
git add src/input/vim/journal_doc.rs src/ui/journal_overlay.rs src/input/actions/journal.rs
git commit -m "feat(journal): note entries edit raw Markdown (no Q: seed)"
```

---

### Task 8: `src/ui/markdown.rs` — CommonMark → TextTag renderer

**Files:**
- Create: `src/ui/markdown.rs`
- Modify: `src/ui/mod.rs` (add `pub mod markdown;`)
- Test: `src/ui/markdown.rs` tests (pure: assert the tag-plan, not GTK)

**Interfaces:**
- Produces: `pub fn plan_markdown(src: &str) -> Vec<Span>` where `Span { text: String, style: Style }` and `Style` is an enum `{ Body, H1, H2, H3, Bold, Italic, BlockQuote, ListItem, Rule, Mono }`; and `pub fn apply_markdown(buffer: &gtk4::TextBuffer, src: &str, tags: &MarkdownTags)`. The pure `plan_markdown` is unit-tested; `apply_markdown` walks the plan and inserts text + applies the matching `TextTag`.

- [ ] **Step 1: Write the failing pure test.** Create `src/ui/markdown.rs` with only the test + type stubs’ signatures, then:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_and_emphasis_plan() {
        let spans = plan_markdown("## Cry\n\nload **it** now");
        // The heading text appears as an H2 span; "it" appears as a Bold span.
        assert!(spans.iter().any(|s| s.text.contains("Cry")
            && matches!(s.style, Style::H2)));
        assert!(spans.iter().any(|s| s.text == "it" && matches!(s.style, Style::Bold)));
    }

    #[test]
    fn bullet_list_items_are_listitem() {
        let spans = plan_markdown("- one\n- two");
        let items: Vec<_> = spans.iter().filter(|s| matches!(s.style, Style::ListItem)).collect();
        assert!(items.iter().any(|s| s.text.contains("one")));
        assert!(items.iter().any(|s| s.text.contains("two")));
    }

    #[test]
    fn table_becomes_mono() {
        let spans = plan_markdown("| a | b |\n|---|---|\n| 1 | 2 |");
        assert!(spans.iter().any(|s| matches!(s.style, Style::Mono)));
    }
}
```

- [ ] **Step 2: Run — expect FAIL** (undefined).

Run: `cargo test --bins ui::markdown 2>&1 | tail -15`
Expected: FAIL — `plan_markdown` / `Style` not found.

- [ ] **Step 3: Implement `plan_markdown`** using `pulldown-cmark`'s pull parser. Walk `Event`s, maintain a small style stack, and emit `Span`s. Map: `Heading(H1|H2|H3+)` → `H1/H2/H3`; `Strong` → `Bold`; `Emphasis` → `Italic`; `BlockQuote` → `BlockQuote`; list `Item` text → `ListItem` (prefix `• ` for bullets, `N. ` for ordered); `Rule` → a `Rule` span with text `"─".repeat(40)`; `Table`/`CodeBlock` regions → collect their raw text into a single `Mono` span. Paragraph text with no active emphasis → `Body`. (Full code block — write the actual parser here, ~80–120 lines. No placeholder: implement the event loop, the style stack push/pop on `Start`/`End`, and the table/code raw-capture flag.)

- [ ] **Step 4: Run pure tests — expect PASS.**

Run: `cargo test --bins ui::markdown 2>&1 | tail -10`
Expected: the three plan tests PASS.

- [ ] **Step 5: Implement `apply_markdown` + `MarkdownTags` — match the claude.ai artifact styling** (design "Target styling"; reference screenshot of `loading-the-cry.md`). Define `MarkdownTags` holding `gtk4::TextTag`s tuned to the reader's **serif** look (family = the reading card's Charter, NOT a new font), generous leading, comfortable left measure:
  - `H1` (title): `weight=Bold`, `scale ≈ 2.0`, `pixels-below-lines` for space under.
  - `H3` (subtitle): `weight=Bold`, `scale ≈ 1.15`, small space under.
  - `H2` (section): `weight=Bold`, `scale ≈ 1.3`, `pixels-above-lines` for space above.
  - `Bold`: `weight=700`. `Italic`: `style=Italic`. (Both inherit the serif body.)
  - `Body`: serif, `pixels-below-lines` ≈ paragraph leading of the reading card; `left-margin` for the comfortable measure (reuse the overlay's existing side margin, not edge-to-edge).
  - `ListItem`: hanging indent — `left-margin` = indent, `indent` = negative marker width so wrapped lines align under the text, not the `1.`/`•`.
  - `BlockQuote`: `left-margin` bump + a muted `foreground`.
  - `Rule`: render as a hairline, NOT dashes — apply a tag with `paragraph-background` off and instead a light `foreground` on a single `─`-run sized to the measure, OR (preferred if it renders cleanly) an empty paragraph carrying a bottom border via CSS on the view for the rule class. Choose whichever reads as a thin grey line with vertical margin above/below; it must not look like literal dashes/box-drawing.
  - `Mono` (tables/code only): `family=JetBrainsMono`.
  `apply_markdown` iterates `plan_markdown(src)`, inserts each span's text at the buffer end iter, and applies the matching tag over the inserted range. Register the tags once against the buffer's tag table (guard against double-register by name). The visible result should read like the claude.ai render in the reference screenshot.

- [ ] **Step 6: Register the module.** In `src/ui/mod.rs` add `pub mod markdown;`.

- [ ] **Step 7: Build.**

Run: `cargo build 2>&1 | tail -4`
Expected: clean.

- [ ] **Step 8: Commit.**

```bash
git add src/ui/markdown.rs src/ui/mod.rs
git commit -m "feat(ui): CommonMark -> TextTag renderer (plan_markdown + apply_markdown)"
```

---

### Task 9: Render journal bodies through the Markdown renderer

**Files:**
- Modify: `src/ui/journal_overlay.rs` (the render_page / paragraph rendering path)
- Test: none new (visual; e2e)

**Interfaces:**
- Consumes: `apply_markdown`, `MarkdownTags` (Task 8).

The journal body currently sets plain text into the buffer and paginates by paragraph. Route the per-page text through `apply_markdown` so the visible buffer carries TextTags. Keep pagination working: the `paragraph_texts`/block model splits on blank lines; markdown spans must be applied to the RENDERED page buffer, not the pagination source.

- [ ] **Step 1: Locate the buffer-set in `render_page`.** 

Run: `rg -n "buffer\(\).set_text|fn render_page|apply_font" src/ui/journal_overlay.rs | head`

- [ ] **Step 2: Replace the plain `set_text` for the page body** with: clear the buffer, then `crate::ui::markdown::apply_markdown(&self.view.buffer(), &page_text, &self.md_tags)` (store a `MarkdownTags` on the overlay struct, built once in the overlay constructor against `self.view.buffer()`). Keep the block-cursor / font application after apply.

- [ ] **Step 3: Guard the note vs qa header.** Since Task 6 already builds `full` without the `Q:` line for notes, the page text handed to `apply_markdown` is correct for both; the `Q: ` line (qa) renders as `Body`. No extra branch here.

- [ ] **Step 4: Build.**

Run: `cargo build 2>&1 | tail -4`
Expected: clean.

- [ ] **Step 5: Commit.**

```bash
git add src/ui/journal_overlay.rs
git commit -m "feat(journal): render Q&A + note bodies as Markdown in the overlay"
```

- [ ] **Step 6: Ask the user to run the visual e2e** (agent cannot launch cage on the live seat). Provide:

```bash
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
```

and the single-work launch to eyeball a rendered note (after Task 10 imports one). **Visual acceptance criterion: the rendered note must look like the claude.ai artifact view in the reference screenshot** — serif body, large bold title, bold section headings with space above, hairline `---` rules (not dashes), true italic/bold runs, and hanging-indent numbered/bulleted lists. State plainly that Markdown rendering + author-jump are screenshot-level acceptance and are NOT verified until the user runs this and confirms it matches the reference.

---

### Task 10: `import-corpus-note` skill

**Files:**
- Create: `~/utono/linux-lit/.claude/skills/import-corpus-note/SKILL.md`

**Interfaces:** none (data-mutation skill; invokes `sqlite3`).

- [ ] **Step 1: Write `SKILL.md`.** Frontmatter:

```yaml
---
name: import-corpus-note
description: Use when importing a .md file (e.g. from claude.ai) into linux-lit's journal as an author/corpus-scope note entry (scope='author', kind='note'), keyed by author name, so it renders for every work by that author with no Q: prefix
argument-hint: <path.md> <author>
---
```

Body documents the exact insert (bypassing the interactive `cp`/`rm` alias rules is N/A here; this is a DB insert):

```bash
# args: MD_PATH (a .md file), AUTHOR (e.g. "Shakespeare")
MD_PATH="$1"; AUTHOR="$2"
DB=~/utono/litdb/data/lit.db
# Dedup: warn if an identical (author, answer) note already exists.
ANSWER="$(cat "$MD_PATH")"
python3 - "$DB" "$AUTHOR" "$MD_PATH" <<'PY'
import sqlite3, sys
db, author, path = sys.argv[1], sys.argv[2], sys.argv[3]
answer = open(path, encoding="utf-8").read()
c = sqlite3.connect(db)
dup = c.execute(
  "SELECT id FROM journal_entries WHERE scope='author' AND work_abbrev=? AND answer=?",
  (author, answer)).fetchone()
if dup:
    print(f"Already imported as id {dup[0]}; skipping.")
else:
    c.execute(
      "INSERT INTO journal_entries "
      "(work_abbrev, div1, div2, question, answer, claude_model, scope, kind, timestamp) "
      "VALUES (?, -2, -2, '', ?, 'claude.ai', 'author', 'note', datetime('now'))",
      (author, answer))
    c.commit()
    print(f"Imported note id {c.execute('SELECT last_insert_rowid()').fetchone()[0]} for {author}.")
PY
```

Include the `loading-the-cry.md` worked example:

```bash
# Import the "Loading the Cry" finding-aid as a Shakespeare corpus note:
#   import-corpus-note ~/Downloads/loading-the-cry.md Shakespeare
# Then in linux-lit: open the journal on any Shakespeare work, press Alt+a.
```

Note the `-2,-2` sentinel = `JOURNAL_AUTHOR_DIV`, `scope='author'`, `kind='note'`, `work_abbrev=<author>`; and that linux-lit must be closed or the note only appears after its next launch (DB has no hot reload).

- [ ] **Step 2: Verify the skill frontmatter loads** (no code test). Confirm the file exists and the YAML is valid:

Run: `head -6 ~/utono/linux-lit/.claude/skills/import-corpus-note/SKILL.md`
Expected: the 4-line frontmatter block.

- [ ] **Step 3: Commit.**

```bash
git add .claude/skills/import-corpus-note/SKILL.md
git commit -m "feat(skill): import-corpus-note (.md -> author-scope journal note)"
```

---

### Task 11: End-to-end import + verification handoff

**Files:** none (verification).

- [ ] **Step 1: Import the example note** (linux-lit closed):

```bash
~/utono/linux-lit/.claude/skills/import-corpus-note/... # per SKILL.md, or run the python inline
```

Verify the row:

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT id, work_abbrev, div1, div2, scope, kind, substr(answer,1,30) FROM journal_entries WHERE scope='author';"
```

Expected: one row, `Shakespeare | -2 | -2 | author | note | # Loading the Cry…`.

- [ ] **Step 2: Full test suite.**

Run: `cargo test --bins 2>&1 | tail -6`
Expected: all pass.

- [ ] **Step 3: Hand off the visual check to the user.** Provide the launch command from linux-lit CLAUDE.md *Headless Verification* to open a Shakespeare work, then `Alt+a`, then eyeball the rendered `loading-the-cry.md` **against the reference screenshot** (serif title/headings, hairline rules, italic/bold, hanging-indent list, the one table as monospace), confirming NO `Q:` prefix. State the change is not visually verified until they run it and confirm it replicates the claude.ai render.

---

## Self-Review

**Spec coverage:**
- §1 Schema (`kind` + `author` scope) → Tasks 1, 2. ✓
- §2 JournalBand::Author + footer → Task 4. ✓
- §3 DB layer (`save_author_page`/`find_author_pages`/`move` arm) → Tasks 2, 3, 4(step 6). ✓
- §4 Ask + import (author ask; import via skill) → Tasks 6, 10. ✓
- §5 Rendering + editing (markdown module, kind-driven prefix, note raw edit, Alt+a, legends) → Tasks 5, 7, 8, 9. ✓
- §6 Import skill → Task 10. ✓
- §7 Testing → unit tests in Tasks 1–4, 7, 8; e2e handoff in Tasks 9, 11. ✓
- Non-goals respected: no `scope='corpus'`, no NOT-NULL drop, monospace tables, jump-only band. ✓

**Placeholder scan:** Task 8 Step 3 says "write the actual parser (~80–120 lines)" — this is a real implementation instruction with the exact event→style mapping enumerated, not a TODO. All other steps carry concrete code/commands.

**Type consistency:** `save_journal_page` gains `kind: &str` (Task 1) and every later caller passes it. `save_author_page(conn, author, question, answer, model, kind)` used consistently (Tasks 2, 6, 10-python mirrors the same columns). `JournalBand::Author(String)` used in Tasks 4–7. `plan_markdown`/`Style`/`Span`/`apply_markdown`/`MarkdownTags` names consistent across Tasks 8–9. `JOURNAL_AUTHOR_DIV` (app) vs `AUTHOR_DIV` (db/journal): two consts with the same value in different modules — acceptable (db layer is self-contained), but implementers should not assume one imports the other.
