# Per-work vocab highlighting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make inline vocab-word coloring a per-work setting stored in the `lit.db` `works.vocab_highlight` column instead of a single global config flag.

**Architecture:** `load_work` reads the per-work `vocab_highlight` column into the `Work` struct; `display_work` seeds the runtime `vocab_highlight_visible` flag from it; **Alt+\\** flips the flag and persists it back to the work's column (read-write); the global `config.vocab_highlight_visible` is removed. An idempotent startup migration adds the column on a fresh DB (defaulting new works OFF) but never backfills or resets existing values.

**Tech Stack:** Rust, GTK4, rusqlite (SQLite), serde.

## Global Constraints

- **No backfill / no reset in the app migration.** Existing per-work values are preserved. The migration only ADDs the column when absent.
- **New / unset works default OFF** (`unwrap_or(0)` → `false`; fresh-add column `DEFAULT 0`).
- **Per-work column is the single source of truth.** Global config flag retired.
- Column is keyed by the row's exact `abbrev` (the same string passed to `load_work`). Writes use `work.abbrev` directly, NOT `base_work_abbrev`.
- Map column value to bool: `1` → true; everything else (`0`, NULL) → false.
- Migration registered in the `BOOKMARKS_INIT` `ensure_*` block in `src/app/mod.rs` (~line 2382), using `open_db_rw`.
- Spec: `docs/superpowers/specs/2026-06-27-per-work-vocab-highlight-design.md`.

---

### Task 1: Read `vocab_highlight` into the `Work` struct

**Files:**
- Modify: `src/db/models.rs:9-21` (add field to `Work`)
- Modify: `src/db/queries.rs:91-104` (read column in `load_work`), `src/db/queries.rs:221-232` (construct field)
- Test: `src/db/queries.rs` (tests module, `#[cfg(test)] mod tests` at line 2289)

**Interfaces:**
- Produces: `Work.vocab_highlight: bool` — true when the work's `works.vocab_highlight` column is `1`, false when `0`/NULL/absent.

- [ ] **Step 1: Add the field to the `Work` struct**

In `src/db/models.rs`, add `vocab_highlight` to `Work` (after `text_file`):

```rust
pub struct Work {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
    pub text_file: Option<String>,
    pub vocab_highlight: bool,
    pub lines: Vec<Line>,
    pub timestamps: Vec<Timestamp>,
    pub media_paths: Vec<String>,
    /// Parallel to media_paths: the media_id for each path.
    pub media_ids: Vec<i64>,
    pub media_id: Option<i64>,
}
```

- [ ] **Step 2: Read the column in `load_work`**

In `src/db/queries.rs`, right after the `text_file` block (ends at line 104, the `.unwrap_or(None);`), add:

```rust
    // vocab_highlight column may be absent on older/other DBs — graceful
    // fallback to OFF. 1 => on; 0/NULL/absent => off.
    let vocab_highlight: bool = conn.query_row(
        "SELECT vocab_highlight FROM works WHERE abbrev = ?1",
        [abbrev],
        |row| row.get::<_, Option<i64>>(0),
    ).unwrap_or(None).unwrap_or(0) == 1;
```

- [ ] **Step 3: Add the field to the constructed `Work`**

In `src/db/queries.rs`, in the `Ok(Work { ... })` at line 221, add `vocab_highlight,` after `text_file,`:

```rust
    Ok(Work {
        abbrev: abbrev.to_string(),
        title,
        author,
        work_type,
        text_file,
        vocab_highlight,
        lines,
        timestamps,
        media_paths,
        media_ids,
        media_id,
    })
```

- [ ] **Step 4: Write the failing test**

`load_work` issues hard-`?` `prepare`s against many companion tables
(`line_timestamps`, `media_files`, `work_media_associations`, `page_images`,
translations, `scene_synopses`, ...), so a hand-built minimal in-memory schema
is brittle. Mirror the existing `test_load_work_hamlet` (line 2966), which loads
a real work from `open_db()`. Assert the **mapping is consistent** with the raw
column — this holds regardless of the value Task 5 sets, so the test never goes
stale:

```rust
    #[test]
    fn load_work_vocab_highlight_matches_column() {
        let conn = open_db().unwrap();
        // Read the raw column for a work known to exist in lit.db.
        let raw: Option<i64> = conn
            .query_row("SELECT vocab_highlight FROM works WHERE abbrev = 'Ham'", [], |r| r.get(0))
            .unwrap();
        let expected = raw.unwrap_or(0) == 1;
        let work = load_work(&conn, "Ham").unwrap();
        assert_eq!(
            work.vocab_highlight, expected,
            "Work.vocab_highlight must mirror the works.vocab_highlight column",
        );
    }
```

This test depends on the real `~/utono/litdb/data/lit.db` (same as
`test_load_work_hamlet`). It fails to compile until the field exists (Step 1) and
fails at runtime until the read is wired (Steps 2-3).

- [ ] **Step 5: Run the test to verify it passes**

Run:
```bash
cargo test --bins load_work_vocab_highlight_matches_column -- --nocapture
```
Expected: PASS once Steps 1-3 are in. If it fails with a compile error elsewhere (e.g. another `Work { .. }` literal missing the new field), fix those literals — see Step 6, then re-run.

- [ ] **Step 6: Fix any other `Work { .. }` constructors**

Adding a non-`Option` field breaks any other struct-literal construction of `Work`. Find them:
```bash
rg -n "Work \{" src/ | rg -v "WorkSummary|pub struct Work|//"
```
Add `vocab_highlight: false,` (or a sensible value) to each literal the compiler flags. Run `cargo build` until clean:
```bash
cargo build 2>&1 | rg "^error" | head
```
Expected: no `error` lines.

- [ ] **Step 7: Commit**

```bash
git add src/db/models.rs src/db/queries.rs
git commit -m "feat(vocab): load per-work vocab_highlight column into Work

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QaK4WwWvULH1JFtXsoYKcE"
```

---

### Task 2: `set_vocab_highlight` writer + idempotent migration

**Files:**
- Modify: `src/db/queries.rs` (add `set_vocab_highlight` and `ensure_vocab_highlight_column` near `ensure_claude_model_columns`, ~line 687)
- Test: `src/db/queries.rs` (tests module)

**Interfaces:**
- Consumes: `column_exists(conn, table, col) -> Result<bool, rusqlite::Error>` (queries.rs:650), `open_db_rw`.
- Produces:
  - `pub fn set_vocab_highlight(conn: &Connection, abbrev: &str, on: bool) -> Result<(), rusqlite::Error>`
  - `pub fn ensure_vocab_highlight_column(conn: &Connection) -> Result<(), rusqlite::Error>`

- [ ] **Step 1: Write the failing test**

In `src/db/queries.rs` `mod tests`, add:

```rust
    #[test]
    fn vocab_highlight_migration_and_writer() {
        let conn = Connection::open_in_memory().unwrap();
        // A works table WITHOUT the vocab_highlight column (legacy/fresh).
        conn.execute_batch(
            "CREATE TABLE works (
                abbrev TEXT UNIQUE NOT NULL, title TEXT NOT NULL,
                author TEXT, work_type TEXT NOT NULL);
             INSERT INTO works (abbrev,title,work_type) VALUES ('W1','One','prose');",
        ).unwrap();

        // Migration adds the column (DEFAULT 0 => existing/new rows read off).
        ensure_vocab_highlight_column(&conn).unwrap();
        let v: Option<i64> = conn
            .query_row("SELECT vocab_highlight FROM works WHERE abbrev='W1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, Some(0), "fresh-added column defaults rows to 0 (off)");

        // Writer flips the per-work value.
        set_vocab_highlight(&conn, "W1", true).unwrap();
        let v2: Option<i64> = conn
            .query_row("SELECT vocab_highlight FROM works WHERE abbrev='W1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v2, Some(1), "writer sets the column to 1");

        // Idempotent: a second ensure is a no-op and does NOT reset the value.
        ensure_vocab_highlight_column(&conn).unwrap();
        let v3: Option<i64> = conn
            .query_row("SELECT vocab_highlight FROM works WHERE abbrev='W1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v3, Some(1), "second ensure must not backfill/reset existing values");

        set_vocab_highlight(&conn, "W1", false).unwrap();
        let v4: Option<i64> = conn
            .query_row("SELECT vocab_highlight FROM works WHERE abbrev='W1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v4, Some(0), "writer clears the column to 0");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run:
```bash
cargo test --bins vocab_highlight_migration_and_writer -- --nocapture
```
Expected: FAIL — `cannot find function ensure_vocab_highlight_column` / `set_vocab_highlight`.

- [ ] **Step 3: Implement the migration and writer**

In `src/db/queries.rs`, after `ensure_claude_model_columns` (ends ~line 687), add:

```rust
/// Ensure `works.vocab_highlight` exists. Per-work flag: `1` colors inline vocab
/// words in the reading card, `0` does not. The column is part of the external
/// lit.db core schema on the user's DB (already present with curated per-work
/// values); this migration only matters on a fresh/other DB that lacks it.
///
/// CRITICAL: this NEVER backfills or resets existing values — the user's
/// 199-work DB carries an intentional split and a blanket UPDATE would destroy
/// it. When the column is absent we ADD it with `DEFAULT 0` so genuinely-new
/// works are off by default; when it is present we do nothing.
pub fn ensure_vocab_highlight_column(conn: &Connection) -> Result<(), rusqlite::Error> {
    if !column_exists(conn, "works", "vocab_highlight")? {
        conn.execute_batch(
            "ALTER TABLE works ADD COLUMN vocab_highlight INTEGER DEFAULT 0;",
        )?;
    }
    Ok(())
}

/// Set a work's per-work vocab-highlight flag (`1` on / `0` off), keyed by the
/// exact `abbrev` row. Call on a read-write connection (`open_db_rw`).
pub fn set_vocab_highlight(
    conn: &Connection,
    abbrev: &str,
    on: bool,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE works SET vocab_highlight = ?2 WHERE abbrev = ?1",
        rusqlite::params![abbrev, on as i64],
    )?;
    Ok(())
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run:
```bash
cargo test --bins vocab_highlight_migration_and_writer -- --nocapture
```
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat(vocab): add set_vocab_highlight writer + idempotent column migration

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QaK4WwWvULH1JFtXsoYKcE"
```

---

### Task 3: Register the migration + seed runtime flag from the work

**Files:**
- Modify: `src/app/mod.rs:2382-2391` (register migration in `BOOKMARKS_INIT`)
- Modify: `src/app/mod.rs:2593` (seed `vocab_highlight_visible` from `work.vocab_highlight` in `display_work_at_with_prepared`)

**Interfaces:**
- Consumes: `ensure_vocab_highlight_column` (Task 2); `Work.vocab_highlight` (Task 1).

- [ ] **Step 1: Register the migration**

In `src/app/mod.rs`, in the `BOOKMARKS_INIT.call_once` block, add the new ensure after `ensure_claude_model_columns` (line 2389):

```rust
            let _ = crate::db::queries::ensure_claude_model_columns(&conn);
            let _ = crate::db::queries::ensure_vocab_highlight_column(&conn);
            let _ = crate::db::journal::ensure_journal_table(&conn);
```

- [ ] **Step 2: Seed the runtime flag from the loaded work**

In `src/app/mod.rs`, `work` is moved into `state.current_work` at line 2593. Capture the per-work flag and set the runtime flag immediately before the move:

```rust
    state.visual_selection = None;
    // Per-work vocab coloring: the loaded work's column is the source of truth.
    // Capture before `work` is moved into current_work; the gate further down
    // (`if state.vocab_highlight_visible { apply_vocab_highlighting }`) reads it.
    state.vocab_highlight_visible = work.vocab_highlight;
    state.current_work = Some(work);
```

- [ ] **Step 3: Build**

Run:
```bash
cargo build 2>&1 | rg "^error" | head
```
Expected: no `error` lines. (This wiring has no pure unit test — it's exercised by the runtime gate at mod.rs:2767. Coverage is the e2e run in Task 6.)

- [ ] **Step 4: Commit**

```bash
git add src/app/mod.rs
git commit -m "feat(vocab): register migration and seed runtime flag from work column

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QaK4WwWvULH1JFtXsoYKcE"
```

---

### Task 4: Alt+\\ persists per-work + retire global config flag

**Files:**
- Modify: `src/input/keymap.rs:2209-2220` (`ToggleVocabHighlight` arm)
- Modify: `src/config.rs:87-88, 169-173, 225` (remove the global flag)
- Modify: `src/app/mod.rs:1368, 1521` (remove the build-time seed from config)
- Modify: `src/ui/keybinds_overlay.rs:463-465` (describe text)

**Interfaces:**
- Consumes: `set_vocab_highlight` (Task 2), `open_db_rw`.

- [ ] **Step 1: Rewrite the `ToggleVocabHighlight` arm to persist per-work**

In `src/input/keymap.rs`, replace the arm at lines 2209-2220:

```rust
        ToggleVocabHighlight => {
            let mut s = state.borrow_mut();
            s.vocab_highlight_visible = !s.vocab_highlight_visible;
            if s.vocab_highlight_visible {
                crate::app::apply_vocab_highlighting(&s);
            } else {
                crate::app::remove_vocab_highlighting(&s);
            }
            // Persist per-work to lit.db (the column keyed by this work's abbrev),
            // not to config. Source of truth is now per-work.
            if let Some(abbrev) = s.current_work.as_ref().map(|w| w.abbrev.clone()) {
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let _ = crate::db::queries::set_vocab_highlight(
                        &conn, &abbrev, s.vocab_highlight_visible,
                    );
                }
            }
            crate::logging::log(&format!("VOCAB: highlighting {}", if s.vocab_highlight_visible { "on" } else { "off" }));
        }
```

- [ ] **Step 2: Remove the global config flag — struct field**

In `src/config.rs`, delete lines 87-88:

```rust
    #[serde(default = "default_vocab_highlight_visible")]
    pub vocab_highlight_visible: bool,
```

- [ ] **Step 3: Remove the default fn**

In `src/config.rs`, delete the `default_vocab_highlight_visible` fn (lines 169-173):

```rust
fn default_vocab_highlight_visible() -> bool {
    // Off by default — inline vocab-word coloring is opt-in via Alt+\\
    // (ToggleVocabHighlight), which persists the choice.
    false
}
```

- [ ] **Step 4: Remove the Default initializer**

In `src/config.rs`, delete line 225:

```rust
            vocab_highlight_visible: default_vocab_highlight_visible(),
```

- [ ] **Step 5: Remove the build-time seed in app/mod.rs**

In `src/app/mod.rs`, delete line 1368:

```rust
    let vocab_highlight_visible = config.vocab_highlight_visible;
```

And in the `AppState { .. }` construction, change line 1521 from `vocab_highlight_visible,` (shorthand, now undefined) to an explicit default — the value is overwritten by Task 3's per-work seed on the first `display_work`, so the initial value only matters before any work loads:

```rust
        dim_enabled,
        vocab_highlight_visible: false,
```

- [ ] **Step 6: Update the keybinds-overlay describe text**

In `src/ui/keybinds_overlay.rs`, the `"vocab hi"` arm (lines 463-465) says "state saved to config". Change to reflect per-work persistence:

```rust
        "vocab hi" => "Toggle highlighting of vocabulary words in the text (state \
saved per-work in lit.db). -> ToggleVocabHighlight arm -> app::apply_vocab_highlighting / \
app::remove_vocab_highlighting — src/input/keymap.rs, src/app.rs",
```

- [ ] **Step 7: Build and confirm the global flag is gone**

Run:
```bash
cargo build 2>&1 | rg "^error" | head
```
Expected: no `error` lines. Then confirm no lingering references:
```bash
rg -n "vocab_highlight_visible" src/config.rs && echo "STILL PRESENT (bad)" || echo "config clean"
rg -n "config.vocab_highlight_visible" src/ && echo "STILL REFERENCED (bad)" || echo "no config refs"
```
Expected: "config clean" and "no config refs".

- [ ] **Step 8: Run the full pure-logic suite**

Run:
```bash
cargo test --bins 2>&1 | rg "test result"
```
Expected: `test result: ok.` with 0 failed (count will be ~477 + the 2 new tests).

- [ ] **Step 9: Commit**

```bash
git add src/input/keymap.rs src/config.rs src/app/mod.rs src/ui/keybinds_overlay.rs
git commit -m "feat(vocab): Alt+backslash persists per-work; retire global config flag

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QaK4WwWvULH1JFtXsoYKcE"
```

---

### Task 5: One-time data edit — Shakespeare + Dickens OFF

**Files:**
- None in the repo. This edits the user's `~/utono/litdb/data/lit.db` directly. It is NOT the app migration and is NOT committed to the repo.

**Interfaces:** none (data-only).

- [ ] **Step 1: Close any running linux-lit instance**

A running instance does not clobber lit.db on exit (only config), but close it anyway so the next launch re-reads the new values cleanly:
```bash
pgrep -af "target/debug/linux-lit" | rg -v "pgrep|zsh -c" || echo "none running"
```
If one is running, ask the user to quit it (the agent must not kill the user's reader).

- [ ] **Step 2: Show the before-state (for the record)**

Run:
```bash
sqlite3 ~/utono/litdb/data/lit.db \
  "SELECT author, vocab_highlight, COUNT(*) FROM works
   WHERE author IN ('Shakespeare','Charles Dickens')
   GROUP BY author, vocab_highlight ORDER BY author;"
```
Expected (before): Charles Dickens|1|5 ; Shakespeare|0|42 ; Shakespeare|1|43.

- [ ] **Step 3: Apply the two UPDATEs**

Run:
```bash
sqlite3 ~/utono/litdb/data/lit.db \
  "UPDATE works SET vocab_highlight = 0 WHERE author = 'Shakespeare';
   UPDATE works SET vocab_highlight = 0 WHERE author = 'Charles Dickens';"
```

- [ ] **Step 4: Verify the after-state**

Run:
```bash
sqlite3 ~/utono/litdb/data/lit.db \
  "SELECT author, vocab_highlight, COUNT(*) FROM works
   WHERE author IN ('Shakespeare','Charles Dickens')
   GROUP BY author, vocab_highlight ORDER BY author;"
```
Expected (after): Charles Dickens|0|5 ; Shakespeare|0|85.

Then confirm nothing else moved (the other 109 ON works untouched):
```bash
sqlite3 ~/utono/litdb/data/lit.db \
  "SELECT vocab_highlight, COUNT(*) FROM works GROUP BY vocab_highlight;"
```
Expected after: **0|90 ; 1|109**.

Arithmetic: before = 42 off / 157 on (199 total). Shakespeare flips 43 on→off,
Dickens flips 5 on→off = 48 flipped. After = 42+48 = 90 off, 157−48 = 109 on.
The 85 OFF Shakespeare rows = 42 already-off + 43 newly-flipped; all 5 Dickens
were on and are now off.

- [ ] **Step 5: No commit (data edit, not repo).** Done.

---

### Task 6: Runtime verification (user-run e2e)

**Files:** none — verification only.

**Interfaces:** none.

The render gate (mod.rs:2767) and Alt+\\ persistence are GUI behaviors; per the project's CLAUDE.md the agent generally cannot drive cage from its own shell (the live dwl owns the seat). After Tasks 1-5, hand the user these checks.

- [ ] **Step 1: Build**

```bash
cargo build 2>&1 | rg "^error" | head
```
Expected: no errors.

- [ ] **Step 2: Ask the user to verify on screen**

Tell the user to `cargo run` and confirm:
- Open a Shakespeare play (e.g. `Ham`) or Dickens (`BH`): vocab words are NOT colored.
- Open a still-on work (e.g. a bible book, or any of the 109 ON works): vocab words ARE colored.
- On an ON work, press **Alt+\\**: coloring turns off; switch to another work and back: it stays off (persisted to lit.db). Press Alt+\\ again: on; relaunch the app: still on.

- [ ] **Step 3: (Optional) headless clipping invariant unaffected**

```bash
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
```
Expected: PASS (this change does not touch layout, but it confirms the build runs headless).

---

## Notes for the implementer

- The two new pure tests (`load_work_vocab_highlight_matches_column`, `vocab_highlight_migration_and_writer`) are the only automated coverage. The state-wiring (Task 3) and keybind persistence (Task 4) have no pure test — they're verified by Task 6.
- Do not hand-edit `config-dev.json` / `config.json` to remove the stale `vocab_highlight_visible` key — serde ignores unknown keys on read and stops writing the key once the field is gone. The keys become inert automatically.
- If `cargo build` flags a `Work { .. }` literal in a test or fixture missing the new field, add `vocab_highlight: false,` to it (Task 1 Step 6).
