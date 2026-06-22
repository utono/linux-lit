# Journal Whole-Work Pages + Card-Size Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Q&A journal overlay the same size as the main reading card, add whole-work pages reachable via a separate "Work" band, and confirm `2H6`/`2H6-Amb` share one journal.

**Architecture:** Three independent refinements to the already-shipped journal feature. (1) Drop a `0.8` size multiplier so the overlay container matches the gloss overlay's verbatim sizing. (2) Add a `scope` column ('scene'|'work') to `journal_entries`; whole-work pages get `scope='work'`, `div1=div2=-1`, and a prompt that sends only title+author. (3) Replace `journal_scene: (i64,i64)` with a `JournalBand` enum (`Work` | `Scene(i64,i64)`) so the same overlay shows either band; `Alt+w` enters the Work band, `Alt+n/p` returns to scenes, `a` follows the current band.

**Tech Stack:** Rust, GTK4 (`gtk4` crate), `rusqlite` (SQLite at `~/utono/litdb/data/lit.db`), Tokio + `glib` async bridge.

## Global Constraints

- Build check only: `cargo build`. **Do NOT run the app** (`cargo run`) — the user runs it. (CLAUDE.md)
- Pure-logic verification: `cargo test --bins` and `cargo clippy`. Visual/runtime acceptance is user-gated via `cage` (an agent cannot drive `cage` on the live dwl seat).
- All journal save/load keys on `crate::app::base_work_abbrev(&w.abbrev)` — never the raw abbrev. (Established convention; gloss.rs/synopsis.rs do the same.)
- All SQL parameterized (`rusqlite::params!` / positional `?`), never string-interpolated.
- US Central timestamps in commit bodies are not required; commit message footer per repo convention:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
  ```
- Never hold an `AppState` borrow across `.await` in the Claude bridge — extract owned values under a short borrow before the closure, re-borrow after the await.
- Branch: `feat/qa-journal-overlay` (continue on it; do not create a new branch).

---

### Task 1: Card-size parity with the main reading card

**Files:**
- Modify: `src/ui/journal_overlay.rs:149-157` (`size_card`)

**Interfaces:**
- Consumes: `JournalOverlay::show_page(scene_title, page_index, page_count, question, answer, card_width, card_height)` already passes `content_hbox.width()/height()` from `journal.rs:34`.
- Produces: overlay container sized to `card_width × card_height` exactly (no scaling), matching `GlossOverlay` (`gloss_overlay.rs:658-659`).

This is a pure GTK geometry change with no pure-logic test (the codebase has no unit tests for overlay sizing — it's a rendered-pixel property verified visually, like `column_split`). The deliverable is the one-line edit plus a clean build; visual acceptance is folded into the Task 6 user hand-off.

- [ ] **Step 1: Read the current `size_card`**

Current code (`src/ui/journal_overlay.rs:149-157`):

```rust
fn size_card(&self, card_width: i32, card_height: i32) {
    let w = (card_width as f64 * 0.8) as i32;
    let h = (card_height as f64 * 0.8) as i32;
    self.container.set_size_request(w, h);
    self.last_card_size.set((w, h));
    self.view.set_left_margin(self.text_margins);
    self.view.set_right_margin(self.text_margins);
    let _ = self.column_width;
}
```

- [ ] **Step 2: Drop the 0.8 multiplier**

Replace the body so the container is sized to the passed dimensions verbatim:

```rust
fn size_card(&self, card_width: i32, card_height: i32) {
    self.container.set_size_request(card_width, card_height);
    self.last_card_size.set((card_width, card_height));
    self.view.set_left_margin(self.text_margins);
    self.view.set_right_margin(self.text_margins);
    let _ = self.column_width;
}
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles clean (no warnings introduced).

- [ ] **Step 4: Commit**

```bash
git add src/ui/journal_overlay.rs
git commit -m "fix(journal): size overlay card to the main card, not 0.8x

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 2: `scope` column + scope-aware queries

**Files:**
- Modify: `src/db/journal.rs` (whole file — table DDL, query signatures, new `find_work_pages`, tests)

**Interfaces:**
- Consumes: `rusqlite::Connection`.
- Produces (signatures later tasks rely on):
  - `ensure_journal_table(conn: &Connection) -> Result<(), rusqlite::Error>` — now also adds `scope` column (idempotent).
  - `save_journal_page(conn, work_abbrev: &str, div1: i64, div2: i64, question: &str, answer: &str, claude_model: &str, scope: &str) -> Result<i64, rusqlite::Error>` — **new trailing `scope` arg**.
  - `update_journal_page(conn, id: i64, question: &str, answer: &str, claude_model: &str) -> Result<(), rusqlite::Error>` — unchanged.
  - `find_journal_pages(conn, work_abbrev: &str, div1: i64, div2: i64) -> Result<Vec<JournalPage>, rusqlite::Error>` — now filters `scope='scene'`.
  - `find_work_pages(conn, work_abbrev: &str) -> Result<Vec<JournalPage>, rusqlite::Error>` — **new**; `scope='work'`.
  - `find_journal_scenes(conn, work_abbrev: &str) -> Result<Vec<(i64,i64)>, rusqlite::Error>` — now filters `scope='scene'`.
  - `delete_journal_page(conn, id: i64) -> Result<(), rusqlite::Error>` — unchanged.
  - `JournalPage` struct — unchanged (no `scope` field; scope is implicit in which query returned the row).

- [ ] **Step 1: Write the failing tests**

Replace the existing `#[cfg(test)] mod tests` block at the bottom of `src/db/journal.rs` with this expanded version (covers scene/work isolation, `find_work_pages` roundtrip, scenes-excludes-work, and the shared-`-Amb` contract via the base abbrev):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_journal_table(&conn).unwrap();
        conn
    }

    #[test]
    fn scene_pages_roundtrip_and_exclude_work() {
        let conn = mem();
        save_journal_page(&conn, "Ham", 1, 2, "Q1?", "A1.", "claude-opus-4-8", "scene").unwrap();
        save_journal_page(&conn, "Ham", 1, 2, "Q2?", "A2.", "claude-opus-4-8", "scene").unwrap();
        // A work page in the same work must NOT appear in scene queries.
        save_journal_page(&conn, "Ham", -1, -1, "WQ?", "WA.", "claude-opus-4-8", "work").unwrap();

        let scene_pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(scene_pages.len(), 2);
        assert_eq!(scene_pages[0].question, "Q1?");
        assert_eq!(scene_pages[1].question, "Q2?");

        // find_journal_scenes lists only scene-scoped rows.
        let scenes = find_journal_scenes(&conn, "Ham").unwrap();
        assert_eq!(scenes, vec![(1, 2)]);
    }

    #[test]
    fn work_pages_roundtrip_and_exclude_scene() {
        let conn = mem();
        save_journal_page(&conn, "Ham", -1, -1, "WQ1?", "WA1.", "claude-opus-4-8", "work").unwrap();
        save_journal_page(&conn, "Ham", -1, -1, "WQ2?", "WA2.", "claude-opus-4-8", "work").unwrap();
        save_journal_page(&conn, "Ham", 3, 1, "SQ?", "SA.", "claude-opus-4-8", "scene").unwrap();

        let work_pages = find_work_pages(&conn, "Ham").unwrap();
        assert_eq!(work_pages.len(), 2);
        assert_eq!(work_pages[0].question, "WQ1?");
        assert_eq!(work_pages[1].question, "WQ2?");

        // A scene query must NOT return work pages.
        assert!(find_journal_pages(&conn, "Ham", -1, -1).unwrap().is_empty());
    }

    #[test]
    fn update_and_delete_still_work() {
        let conn = mem();
        let id1 = save_journal_page(&conn, "Ham", 1, 2, "Q1?", "A1.", "m", "scene").unwrap();
        save_journal_page(&conn, "Ham", 1, 2, "Q2?", "A2.", "m", "scene").unwrap();

        update_journal_page(&conn, id1, "Q1b?", "A1b.", "m").unwrap();
        let pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(pages[0].question, "Q1b?");
        assert_eq!(pages[0].answer, "A1b.");

        delete_journal_page(&conn, id1).unwrap();
        let pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].question, "Q2?");
    }

    #[test]
    fn shared_base_abbrev_contract() {
        // 2H6 and 2H6-Amb share a journal because callers always pass
        // base_work_abbrev (== "2H6"). This test documents that contract at the
        // DB layer: a page saved under "2H6" is found when querying "2H6".
        let conn = mem();
        save_journal_page(&conn, "2H6", 4, 8, "Q?", "A.", "m", "scene").unwrap();
        let pages = find_journal_pages(&conn, "2H6", 4, 8).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].question, "Q?");
    }

    #[test]
    fn ensure_table_is_idempotent_and_adds_scope() {
        let conn = mem();
        // Calling again must not error (idempotent ALTER guard).
        ensure_journal_table(&conn).unwrap();
        let has_scope: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='scope'")
            .unwrap()
            .exists([])
            .unwrap();
        assert!(has_scope);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bins journal`
Expected: FAIL to **compile** — `save_journal_page` is called with 8 args (the 7-arg version exists), and `find_work_pages` is not defined. (Compilation failure counts as the red state for a signature-changing TDD step.)

- [ ] **Step 3: Update `ensure_journal_table` to add the `scope` column**

Replace `ensure_journal_table` (`src/db/journal.rs:14-29`) with:

```rust
pub fn ensure_journal_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS journal_entries (
            id          INTEGER PRIMARY KEY,
            work_abbrev TEXT    NOT NULL,
            div1        INTEGER NOT NULL,
            div2        INTEGER NOT NULL,
            question    TEXT    NOT NULL,
            answer      TEXT    NOT NULL,
            claude_model TEXT,
            scope       TEXT    NOT NULL DEFAULT 'scene',
            timestamp   TEXT    NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_journal_work_scene
            ON journal_entries(work_abbrev, div1, div2, timestamp);",
    )?;
    // Idempotent migration for any DB whose table predates the scope column.
    let has_scope = conn
        .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='scope'")?
        .exists([])?;
    if !has_scope {
        conn.execute_batch(
            "ALTER TABLE journal_entries ADD COLUMN scope TEXT NOT NULL DEFAULT 'scene';",
        )?;
    }
    Ok(())
}
```

- [ ] **Step 4: Add `scope` arg to `save_journal_page`**

Replace `save_journal_page` (`src/db/journal.rs:31-47`) with:

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
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO journal_entries
            (work_abbrev, div1, div2, question, answer, claude_model, scope, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))",
        rusqlite::params![work_abbrev, div1, div2, question, answer, claude_model, scope],
    )?;
    Ok(conn.last_insert_rowid())
}
```

- [ ] **Step 5: Filter `find_journal_pages` to scene scope**

Replace the SQL in `find_journal_pages` (`src/db/journal.rs:55-60`) so the `WHERE` adds `AND scope = 'scene'`:

```rust
    let mut stmt = conn.prepare(
        "SELECT id, div1, div2, question, answer, COALESCE(claude_model, ''), timestamp
         FROM journal_entries
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 AND scope = 'scene'
         ORDER BY timestamp ASC, id ASC",
    )?;
```

(The `query_map` body that builds `JournalPage` is unchanged.)

- [ ] **Step 6: Add `find_work_pages`**

Insert this function immediately after `find_journal_pages` (after `src/db/journal.rs:73`):

```rust
pub fn find_work_pages(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, div1, div2, question, answer, COALESCE(claude_model, ''), timestamp
         FROM journal_entries
         WHERE work_abbrev = ?1 AND scope = 'work'
         ORDER BY timestamp ASC, id ASC",
    )?;
    let rows = stmt.query_map([work_abbrev], |row| {
        Ok(JournalPage {
            id: row.get(0)?,
            div1: row.get(1)?,
            div2: row.get(2)?,
            question: row.get(3)?,
            answer: row.get(4)?,
            claude_model: row.get(5)?,
            timestamp: row.get(6)?,
        })
    })?;
    rows.collect()
}
```

- [ ] **Step 7: Filter `find_journal_scenes` to scene scope**

Replace the SQL in `find_journal_scenes` (`src/db/journal.rs:79-83`):

```rust
    let mut stmt = conn.prepare(
        "SELECT DISTINCT div1, div2 FROM journal_entries
         WHERE work_abbrev = ?1 AND scope = 'scene'
         ORDER BY div1 ASC, div2 ASC",
    )?;
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test --bins journal`
Expected: all five journal tests PASS.

- [ ] **Step 9: Commit**

```bash
git add src/db/journal.rs
git commit -m "feat(journal): scope column + find_work_pages; scene/work isolation

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 3: `JournalBand` state model

**Files:**
- Modify: `src/app.rs` (add enum near `JournalPromptMode` ~line 93-97; replace field `journal_scene` ~line 275; update initializer ~line 1800)

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `pub enum JournalBand { Work, Scene(i64, i64) }` (derives `Clone, Copy, Debug, PartialEq, Eq`).
  - `AppState.journal_band: JournalBand` replaces `AppState.journal_scene: (i64, i64)`.
  - Initializer sets `journal_band: JournalBand::Scene(0, 0)`.

This task only renames/replaces the field and adds the enum; it leaves the codebase **not compiling** until Task 4 updates the consumers in `journal.rs`. That is acceptable for a state-model task — verification is "the enum and field exist and `app.rs` itself has no *new* errors beyond the expected unresolved references in `journal.rs`". Run `cargo build` and confirm the only errors are in `src/input/actions/journal.rs` referencing `journal_scene`.

- [ ] **Step 1: Add the `JournalBand` enum**

Insert after the `JournalPromptMode` enum (after `src/app.rs:97`):

```rust
/// Which "band" of the journal is currently shown. The Work band holds
/// whole-work pages (scope='work'); a Scene band holds one (div1,div2)'s pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalBand {
    Work,
    Scene(i64, i64),
}
```

- [ ] **Step 2: Replace the `journal_scene` field**

In the `AppState` struct, replace (`src/app.rs:275`):

```rust
    pub journal_scene: (i64, i64),
```

with:

```rust
    pub journal_band: JournalBand,
```

- [ ] **Step 3: Update the initializer**

In the `AppState { ... }` construction, replace (`src/app.rs:1800`):

```rust
        journal_scene: (0, 0),
```

with:

```rust
        journal_band: JournalBand::Scene(0, 0),
```

- [ ] **Step 4: Build (expect errors only in journal.rs)**

Run: `cargo build 2>&1 | rg -n "journal_scene|error\[" | head`
Expected: errors reference `journal_scene` in `src/input/actions/journal.rs` only (consumers fixed in Task 4). `src/app.rs` itself compiles its new enum/field. Do **not** commit yet — Task 4 makes the tree compile; commit there.

(No commit this task — the tree does not build standalone. Task 4 finishes the unit.)

---

### Task 4: Band-aware action layer

**Files:**
- Modify: `src/input/actions/journal.rs` (whole file — `render_current`, `toggle_overlay`, `nav_scene`, new `nav_to_work_band`, `begin_ask`, `ask_claude`)

**Interfaces:**
- Consumes: `JournalBand` (Task 3), `find_work_pages`/`find_journal_pages`/`find_journal_scenes`/`save_journal_page(.., scope)` (Task 2), `crate::app::{base_work_abbrev, synopsis_label, scene_label, scene_text_for, current_scene_divs}`, `crate::gloss::JOURNAL_QA_PROMPT`.
- Produces:
  - `pub(crate) fn nav_to_work_band(state: &Rc<RefCell<AppState>>)` — used by keymap (Task 5).
  - Existing `pub(crate)` fns keep their signatures: `toggle_overlay`, `close_overlay`, `nav_page`, `nav_scene`, `begin_ask`, `begin_edit`, `close_prompt`, `submit_prompt`, `delete_current`.

- [ ] **Step 1: Update `render_current` to branch on band**

Replace `render_current` (`src/input/actions/journal.rs:9-44`) with:

```rust
/// Load the current band's pages from the DB into `journal_pages`, clamp the
/// index, and render the current page (or the empty-band card).
fn render_current(s: &mut AppState) {
    let work_abbrev = s
        .current_work
        .as_ref()
        .map(|w| crate::app::base_work_abbrev(&w.abbrev).to_string())
        .unwrap_or_default();

    let conn = crate::db::queries::open_db().ok();
    let (pages, scene_title) = match s.journal_band {
        JournalBand::Work => {
            let pages = conn
                .and_then(|c| crate::db::journal::find_work_pages(&c, &work_abbrev).ok())
                .unwrap_or_default();
            let title = format!(
                "{} — whole work",
                s.current_work.as_ref().map(|w| w.title.as_str()).unwrap_or(""),
            );
            (pages, title)
        }
        JournalBand::Scene(d1, d2) => {
            let pages = conn
                .and_then(|c| crate::db::journal::find_journal_pages(&c, &work_abbrev, d1, d2).ok())
                .unwrap_or_default();
            let title = format!(
                "{} — {}",
                s.current_work.as_ref().map(|w| w.title.as_str()).unwrap_or(""),
                crate::app::synopsis_label(s, d1, d2),
            );
            (pages, title)
        }
    };

    let count = pages.len();
    if count == 0 {
        s.journal_page_index = 0;
    } else if s.journal_page_index >= count {
        s.journal_page_index = count - 1;
    }

    let cw = s.content_hbox.width();
    let h = s.content_hbox.height();
    let (q, a) = if count == 0 {
        (String::new(), String::new())
    } else {
        let p = &pages[s.journal_page_index];
        (p.question.clone(), p.answer.clone())
    };
    s.journal_overlay
        .show_page(&scene_title, s.journal_page_index, count, &q, &a, cw, h);
    s.journal_pages = pages;
}
```

- [ ] **Step 2: Add the `JournalBand` import**

At the top of `src/input/actions/journal.rs`, replace the `use crate::app::{...}` line (`src/input/actions/journal.rs:1`):

```rust
use crate::app::{AppState, InputMode, JournalPromptMode};
```

with:

```rust
use crate::app::{AppState, InputMode, JournalBand, JournalPromptMode};
```

- [ ] **Step 3: Update `toggle_overlay` to set a Scene band on open**

In `toggle_overlay`, replace the open line (`src/input/actions/journal.rs:66`):

```rust
    s.journal_scene = crate::app::current_scene_divs(&s);
```

with:

```rust
    let (d1, d2) = crate::app::current_scene_divs(&s);
    s.journal_band = JournalBand::Scene(d1, d2);
```

- [ ] **Step 4: Rewrite `nav_scene` for band entry/exit**

Replace `nav_scene` (`src/input/actions/journal.rs:95-123`) with:

```rust
/// Jump to the next/prev scene that has pages (skips empty scenes). Lands on
/// that scene's first page. From the Work band, delta>0 lands on the first
/// scene with pages, delta<0 on the last (the Work band sorts before scenes).
pub(crate) fn nav_scene(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    let work_abbrev = s
        .current_work
        .as_ref()
        .map(|w| crate::app::base_work_abbrev(&w.abbrev).to_string())
        .unwrap_or_default();
    let scenes = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_journal_scenes(&conn, &work_abbrev).ok())
        .unwrap_or_default();
    if scenes.is_empty() {
        return;
    }

    let target_idx: i64 = match s.journal_band {
        // From the Work band, enter the scene list at the appropriate end.
        JournalBand::Work => {
            if delta > 0 { 0 } else { scenes.len() as i64 - 1 }
        }
        JournalBand::Scene(d1, d2) => {
            match scenes.iter().position(|&sc| sc == (d1, d2)) {
                Some(i) => (i as i64 + delta as i64).clamp(0, scenes.len() as i64 - 1),
                None => {
                    if delta > 0 { 0 } else { scenes.len() as i64 - 1 }
                }
            }
        }
    };

    let target = JournalBand::Scene(scenes[target_idx as usize].0, scenes[target_idx as usize].1);
    if target != s.journal_band {
        s.journal_band = target;
        s.journal_page_index = 0;
        render_current(&mut s);
    }
}
```

- [ ] **Step 5: Add `nav_to_work_band`**

Insert immediately after `nav_scene` (after the function added in Step 4):

```rust
/// Switch to the Work band (whole-work pages) and render it.
pub(crate) fn nav_to_work_band(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.journal_band == JournalBand::Work {
        return;
    }
    s.journal_band = JournalBand::Work;
    s.journal_page_index = 0;
    render_current(&mut s);
}
```

- [ ] **Step 6: Make `begin_ask` label follow the band**

Replace `begin_ask` (`src/input/actions/journal.rs:125-130`) with:

```rust
pub(crate) fn begin_ask(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    s.journal_prompt_mode = JournalPromptMode::Ask;
    let title = match s.journal_band {
        JournalBand::Work => "Ask a question about the whole work",
        JournalBand::Scene(_, _) => "Ask a question about this scene",
    };
    s.journal_overlay
        .open_ask_card(title, "Ctrl+Enter to ask · Esc to cancel");
}
```

- [ ] **Step 7: Make `ask_claude` band-aware (scope + prompt + reload)**

Replace `ask_claude` (`src/input/actions/journal.rs:158-264`) with the band-aware version. For the Work band: skip `scene_text_for`, omit the scene-text block from the user message, write `scope='work'` with `div1=div2=-1`, and reload via `find_work_pages`. For a Scene band: unchanged behavior with explicit `scope='scene'`.

```rust
fn ask_claude(state_rc: &Rc<RefCell<AppState>>, question: &str, mode: JournalPromptMode) {
    let (work_title, work_author, work_abbrev, band, scene_text, model, tokio_handle) = {
        let s = state_rc.borrow();
        let band = s.journal_band;
        let (title, author, abbrev) = match s.current_work.as_ref() {
            Some(w) => (
                w.title.clone(),
                w.author.clone(),
                crate::app::base_work_abbrev(&w.abbrev).to_string(),
            ),
            None => return,
        };
        let scene_text = match band {
            JournalBand::Work => String::new(),
            JournalBand::Scene(d1, d2) => crate::app::scene_text_for(&s, d1, d2),
        };
        (
            title,
            author,
            abbrev,
            band,
            scene_text,
            s.config.claude_model.clone(),
            s.tokio_handle.clone(),
        )
    };

    state_rc.borrow().journal_overlay.show_loading();

    let edit_id: i64 = if mode == JournalPromptMode::Edit {
        let s = state_rc.borrow();
        s.journal_pages
            .get(s.journal_page_index)
            .map(|p| p.id)
            .unwrap_or(-1)
    } else {
        -1
    };

    let user_msg = match band {
        JournalBand::Work => format!(
            "Work: {} by {}\n\nReader's question about the play as a whole:\n{}",
            work_title, work_author, question,
        ),
        JournalBand::Scene(d1, d2) => format!(
            "Work: {} by {}\nScene: {}\n\nScene text:\n{}\n\nReader's question:\n{}",
            work_title,
            work_author,
            crate::app::scene_label(d1, d2),
            scene_text,
            question,
        ),
    };
    let question_owned = question.to_string();
    let state_for_result = Rc::clone(state_rc);

    glib::spawn_future_local(async move {
        let model_for_db = model.clone();
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(
                    &crate::gloss::JOURNAL_QA_PROMPT,
                    &user_msg,
                    &model,
                )
                .await
            })
            .await;

        match result {
            Ok(Ok(answer)) => {
                // For a save, the scope and (div1,div2) come from the band.
                let (scope, sdiv1, sdiv2) = match band {
                    JournalBand::Work => ("work", -1_i64, -1_i64),
                    JournalBand::Scene(d1, d2) => ("scene", d1, d2),
                };
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let write_result = if mode == JournalPromptMode::Edit && edit_id >= 0 {
                        crate::db::journal::update_journal_page(
                            &conn, edit_id, &question_owned, &answer, &model_for_db,
                        )
                    } else {
                        crate::db::journal::save_journal_page(
                            &conn, &work_abbrev, sdiv1, sdiv2, &question_owned, &answer,
                            &model_for_db, scope,
                        )
                        .map(|_| ())
                    };
                    if let Err(e) = write_result {
                        crate::logging::log(&format!("JOURNAL: db write failed: {}", e));
                    }
                }
                let pages = crate::db::queries::open_db()
                    .ok()
                    .and_then(|conn| match band {
                        JournalBand::Work => {
                            crate::db::journal::find_work_pages(&conn, &work_abbrev).ok()
                        }
                        JournalBand::Scene(d1, d2) => {
                            crate::db::journal::find_journal_pages(&conn, &work_abbrev, d1, d2).ok()
                        }
                    })
                    .unwrap_or_default();
                let new_index = if mode == JournalPromptMode::Edit && edit_id >= 0 {
                    pages.iter().position(|p| p.id == edit_id).unwrap_or(0)
                } else {
                    pages.len().saturating_sub(1)
                };
                let mut s = state_for_result.borrow_mut();
                s.journal_band = band;
                s.journal_page_index = new_index;
                render_current(&mut s);
                crate::logging::log("JOURNAL: saved page");
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.journal_overlay.show_message(&format!("Error: {}", e));
                crate::logging::log(&format!("JOURNAL: claude error: {}", e));
            }
            Err(e) => {
                let s = state_for_result.borrow();
                s.journal_overlay.show_message("Internal error — try again.");
                crate::logging::log(&format!("JOURNAL: tokio join error: {}", e));
            }
        }
    });
}
```

- [ ] **Step 8: Build the whole tree**

Run: `cargo build`
Expected: compiles clean. (`delete_current` is untouched — it deletes by `id`, scope-independent — and `nav_page`/`begin_edit`/`submit_prompt`/`close_prompt`/`close_overlay`/`toggle_overlay` close/open are unaffected by the rename beyond Step 3.)

- [ ] **Step 9: Run the full bin test suite**

Run: `cargo test --bins`
Expected: all pass (the journal DB tests from Task 2 plus the pre-existing suite). Note the journal action layer itself has no unit tests (it's GTK + async; covered by the user-run visual check in Task 6).

- [ ] **Step 10: Commit (Tasks 3 + 4 together — the unit that compiles)**

```bash
git add src/app.rs src/input/actions/journal.rs
git commit -m "feat(journal): JournalBand (Work|Scene) + whole-work ask path

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 5: Keymap — Alt+w enters the Work band

**Files:**
- Modify: `src/input/keymap.rs` (the `is_alt` match in `handle_journal_key`, ~line 681-693)

**Interfaces:**
- Consumes: `crate::input::actions::journal::nav_to_work_band` (Task 4).
- Produces: `Alt+w` handled inside the journal overlay.

- [ ] **Step 1: Add the `Alt+w` arm**

In `handle_journal_key`, replace the `is_alt` match (`src/input/keymap.rs:681-693`):

```rust
    if is_alt {
        match key_name {
            "n" => {
                crate::input::actions::journal::nav_scene(state, 1);
                return true;
            }
            "p" => {
                crate::input::actions::journal::nav_scene(state, -1);
                return true;
            }
            _ => {}
        }
    }
```

with:

```rust
    if is_alt {
        match key_name {
            "n" => {
                crate::input::actions::journal::nav_scene(state, 1);
                return true;
            }
            "p" => {
                crate::input::actions::journal::nav_scene(state, -1);
                return true;
            }
            "w" => {
                crate::input::actions::journal::nav_to_work_band(state);
                return true;
            }
            _ => {}
        }
    }
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat(journal): Alt+w enters the Work band in the overlay

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 6: Update the Ctrl+/ keybinds overlay + user hand-off

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (the `"journal tog"` describe arm, ~line 355-366)

**Interfaces:**
- Consumes: nothing (descriptive text only).
- Produces: the journal detail panel documents `Alt+w`.

The journal's in-overlay keys (`a/e/d/j/k/gg/G/Ctrl+n/Ctrl+p/Alt+n/Alt+p/Alt+w/Escape`) are handled in `handle_journal_key`, not as reader binds, so **`keymap.json` and `keymap_config.rs` are NOT touched** — only this descriptive overlay. The cap `("C-j", "journal tog")` (`src/ui/keybinds_overlay.rs:86`) is unchanged; only the description blurb adds `Alt+w`.

- [ ] **Step 1: Add `Alt+w` to the journal description**

Replace the `"journal tog"` arm (`src/ui/keybinds_overlay.rs:355-366`) with (adds the Work-band sentence; rest unchanged):

```rust
        "journal tog" => "Open or close the Q&A journal for the current scene. \
The journal is a per-work notebook: each scene holds zero or more \u{201c}pages,\u{201d} \
where a page is one question you asked and the answer Claude gave. It opens on the \
scene under the reading cursor; if that scene has no pages yet it shows an empty \
card prompting you to press a to ask. Inside the overlay: a asks a new question \
(Claude answers, drawing on its knowledge of the whole play), e edits the current \
page's question, d deletes the current page, j/k scroll the answer, gg/G jump to \
top/bottom, Ctrl+n / Ctrl+p flip pages within the band, Alt+n / Alt+p jump to the \
next/prev scene that has pages, and Alt+w switches to the Work band \u{2014} \
whole-work pages about the play as a whole (Claude is sent only the title and \
author, not a scene). Escape (or Ctrl+j) closes and returns the cursor to where \
you were reading. \
-> journal::toggle_overlay — src/input/actions/journal.rs (overlay keys: \
handle_journal_key in src/input/keymap.rs)",
```

- [ ] **Step 2: Run the overlay cross-reference skill**

Invoke the `update-cairo-keybinds-overlay` skill and run its three-pass cross-reference for the journal key (`C-j` cap → `"journal tog"` describe arm). Confirm: no blank detail slot, the label names the right action, and the describe arm exists and mentions every in-overlay key including `Alt+w`.

- [ ] **Step 3: Build + full check**

Run: `cargo build && cargo test --bins && cargo clippy 2>&1 | rg -i "journal" | head`
Expected: build clean, all tests pass, no journal-specific clippy warnings.

- [ ] **Step 4: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "feat(journal): document Alt+w Work band in Ctrl+/ overlay

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

- [ ] **Step 5: Hand off runtime verification to the user**

An agent cannot drive `cage` on the live dwl seat. Give the user the exact commands and the visual acceptance checklist:

```bash
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
```

Then a manual headless launch (from CLAUDE.md "Headless Verification") to eyeball, with `ANTHROPIC_API_KEY` set:
- `Ctrl+j` opens the journal **filling the same rectangle as the main reading card** (Task 1 acceptance).
- In a scene band, `a` asks a scene question (sends scene text); answer renders, `j`/`k` scroll without clipping.
- `Alt+w` switches to the **Work band** ("… — whole work" title); `a` there asks a whole-work question (title+author only, no scene text).
- `Alt+n`/`Alt+p` return from the Work band to a scene with pages and move between scenes; `Ctrl+n`/`Ctrl+p` flip pages within the band.
- Scene pages and work pages do **not** appear in each other's band.
- `e` edits within the same band, `d` deletes, `Escape` closes and restores the cursor.

---

## Self-Review

**Spec coverage:**
- Item 1 (card-size parity) → Task 1. ✓
- Item 2 data model (`scope` column, migration, queries) → Task 2. ✓
- Item 2 state (`JournalBand`) → Task 3. ✓
- Item 2 navigation + ask (`Alt+w`, band-following `a`, work prompt title+author only, scene/work isolation) → Tasks 4 + 5. ✓
- Item 2 overlay header (work title vs scene label) → Task 4 `render_current`. ✓
- Item 3 (shared `-Amb`, documenting test) → Task 2 `shared_base_abbrev_contract`. ✓
- Ctrl+/ overlay sync → Task 6. ✓

**Placeholder scan:** No TBD/TODO; every code step shows full code; every command has expected output. ✓

**Type consistency:**
- `save_journal_page(.., scope: &str)` defined in Task 2, called with 8 args in Task 4 Step 7. ✓
- `find_work_pages(conn, abbrev)` defined Task 2, called in Task 4 Steps 1 & 7. ✓
- `JournalBand { Work, Scene(i64,i64) }` defined Task 3, matched in Task 4 (`render_current`, `nav_scene`, `nav_to_work_band`, `begin_ask`, `ask_claude`). ✓
- `nav_to_work_band` defined Task 4 Step 5, called Task 5 Step 1. ✓
- `journal_band` field replaces `journal_scene` consistently across Tasks 3–4 (no remaining `journal_scene` reference). ✓
- `update_journal_page` / `delete_journal_page` signatures unchanged; callers unchanged. ✓
