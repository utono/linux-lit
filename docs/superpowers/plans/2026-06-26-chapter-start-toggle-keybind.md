# Chapter-start Toggle Keybind Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a prose-only keybind that toggles `line_mapping.chapter_start` on the cursor's paragraph, re-derives the work's `(div1, div2)` chapter divisions by shelling out to `chapter_divisions.py derive`, and reloads the work in place (cursor preserved) so the boundary change is immediately visible.

**Architecture:** A new `Action::ToggleChapterStart` dispatches to an action fn in a new `src/input/actions/chapters.rs`. The fn gates on `is_prose_work`, resolves the cursor's `line_mapping.id` + current `chapter_start`, then in a `spawn_blocking` toggles the column (new `queries::toggle_chapter_start`) and runs `python3 .../chapter_divisions.py derive --work <abbrev>`. On the UI thread it reloads the work via the existing picker load flow with `target_line_id = Some(cursor line id)`. The stale snapshot self-invalidates because its fingerprint includes `div1/div2`.

**Tech Stack:** Rust, GTK4 (glib main loop, `Rc<RefCell<AppState>>`), rusqlite (fresh `Connection` per op, WAL), tokio `spawn_blocking`, `std::process::Command`. Pure-logic tests via `cargo test --bins`.

## Global Constraints

- Do NOT run the app (`cargo run`); only `cargo build` / `cargo test` / `cargo clippy`. The user runs the app. The agent MAY use the headless `cage` protocol in CLAUDE.md (Headless Verification) for the final visual check. (CLAUDE.md)
- `cargo test --bins` stays green; `cargo clippy` warning count must NOT increase (baseline **119**).
- Prose gate: `crate::db::line_types::is_prose_work(&work.work_type) -> bool`. Non-prose → no-op + debug log, no DB write.
- Cursor identity: `AppState.current_line` is a BUFFER line. Map to the `line_mapping.id` via `AppState::line_mapping_id_for_buffer(buffer_line) -> Option<i64>` (`src/app/mod.rs:615`). The current `chapter_start` and `line_in_div` come from `current_work.lines[work_idx]` where `work_idx = work_line_for_buffer(buffer_line)` (`src/app/mod.rs:575`).
- DB write: open with `crate::db::queries::open_db_rw()` (`src/db/queries.rs:647`, WAL). Toggle SQL: `UPDATE line_mapping SET chapter_start = 1 - COALESCE(chapter_start,0) WHERE id = ?1`. Model on `upsert_chapter` (`queries.rs:1363`) / `toggle_bookmark` (`queries.rs:1253`).
- Re-derivation: shell out — `python3 <litdb>/scripts/chapter_divisions.py derive --work <abbrev>`, where `<litdb>` is `~/utono/litdb` expanded from `$HOME`. Run AFTER the toggle connection is dropped (so Python sees the committed mark). `derive` uses only stdlib, so plain `python3` (no venv) works.
- Reload pattern: copy `src/input/actions/pickers.rs::load_selected_work` (line 44) — `spawn_blocking`(`open_db` + `load_work` + snapshot read/build) then UI-thread `clear_display` + `display_work_at_with_prepared(&mut s, work, Some(cursor_line_id), prepared)`.
- Action signature: async/DB-write actions take `state: &Rc<RefCell<AppState>>` + `tokio_handle: &tokio::runtime::Handle` and use `glib::spawn_future_local` + `handle.spawn_blocking` (see `bookmarks::toggle_bookmark`, `src/input/actions/bookmarks.rs:11`).
- Commit messages end with the Co-Authored-By / Claude-Session trailer per CLAUDE.md.

---

### Task 1: `queries::toggle_chapter_start` DB helper

**Files:**
- Modify: `src/db/queries.rs` — add `toggle_chapter_start` near `upsert_chapter` (~line 1363).
- Test: `src/db/queries.rs` `#[cfg(test)]` module (mirror existing query tests if present; else add a `mod chapter_start_tests`).

**Interfaces:**
- Produces: `pub fn toggle_chapter_start(conn: &Connection, line_mapping_id: i64) -> Result<bool, rusqlite::Error>` — flips `line_mapping.chapter_start` for the row and returns the NEW value (`true` = now a chapter start). Used by Task 4.

- [ ] **Step 1: Write the failing test.** Add to `src/db/queries.rs`:

```rust
#[cfg(test)]
mod chapter_start_tests {
    use super::*;
    use rusqlite::Connection;

    fn mk() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE line_mapping (id INTEGER PRIMARY KEY, chapter_start INTEGER DEFAULT 0);
             INSERT INTO line_mapping (id, chapter_start) VALUES (7, 0);",
        ).unwrap();
        c
    }

    #[test]
    fn toggle_sets_then_clears() {
        let c = mk();
        assert_eq!(toggle_chapter_start(&c, 7).unwrap(), true);
        let v: i64 = c.query_row("SELECT chapter_start FROM line_mapping WHERE id=7", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 1);
        assert_eq!(toggle_chapter_start(&c, 7).unwrap(), false);
        let v2: i64 = c.query_row("SELECT chapter_start FROM line_mapping WHERE id=7", [], |r| r.get(0)).unwrap();
        assert_eq!(v2, 0);
    }

    #[test]
    fn toggle_handles_null_as_zero() {
        let c = mk();
        c.execute("UPDATE line_mapping SET chapter_start = NULL WHERE id = 7", []).unwrap();
        assert_eq!(toggle_chapter_start(&c, 7).unwrap(), true);
    }
}
```

- [ ] **Step 2: Run test to verify it fails.**

Run: `cargo test --bins chapter_start_tests`
Expected: FAIL — `cannot find function toggle_chapter_start`.

- [ ] **Step 3: Implement.** Add near `upsert_chapter` in `src/db/queries.rs`:

```rust
/// Toggle line_mapping.chapter_start for one paragraph. Returns the new value
/// (true = now marks a chapter start). NULL is treated as 0.
pub fn toggle_chapter_start(conn: &Connection, line_mapping_id: i64) -> Result<bool, rusqlite::Error> {
    conn.execute(
        "UPDATE line_mapping SET chapter_start = 1 - COALESCE(chapter_start, 0) WHERE id = ?1",
        [line_mapping_id],
    )?;
    let v: i64 = conn.query_row(
        "SELECT COALESCE(chapter_start, 0) FROM line_mapping WHERE id = ?1",
        [line_mapping_id],
        |r| r.get(0),
    )?;
    Ok(v == 1)
}
```

- [ ] **Step 4: Run test to verify it passes.**

Run: `cargo test --bins chapter_start_tests`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit.**

```bash
git add src/db/queries.rs
git commit -m "feat(db): toggle_chapter_start query for prose chapter marks"
```

---

### Task 2: `Action::ToggleChapterStart` enum variant

**Files:**
- Modify: `src/input/actions/mod.rs` — `enum Action` (~36–169), `category()` (~172–288), `name()` (~291–395).

**Interfaces:**
- Produces: `Action::ToggleChapterStart`, with `name()` returning `"ToggleChapterStart"` and a `category()` arm. Consumed by Tasks 3 and 4.

- [ ] **Step 1: Add the variant.** In `enum Action` add:

```rust
    /// Toggle whether the cursor's paragraph begins a chapter (prose only).
    ToggleChapterStart,
```

- [ ] **Step 2: Add `category()` arm.** In the `category()` match, alongside the navigation/timestamp actions:

```rust
    Action::ToggleChapterStart => Category::Navigation,
```

(Use whichever `Category` variant the file defines for movement/structure actions; `Navigation` exists per the reference report. If the project groups chapter/timestamp actions under a `Timestamps` category, use that instead — match the neighbour `SetChapter` uses.)

- [ ] **Step 3: Add `name()` arm.** In the `name()` match:

```rust
    Action::ToggleChapterStart => "ToggleChapterStart",
```

- [ ] **Step 4: Verify it compiles.**

Run: `cargo build`
Expected: builds (an unused-variant / non-exhaustive-match error elsewhere means a `match action` is missing the arm — Task 3 adds the dispatch arm; if `cargo build` fails ONLY on `dispatch_action` non-exhaustiveness, proceed to Task 3 then re-build).

- [ ] **Step 5: Commit.**

```bash
git add src/input/actions/mod.rs
git commit -m "feat(input): add ToggleChapterStart action variant"
```

---

### Task 3: Default keybind + dispatch arm

**Files:**
- Modify: `src/input/keymap_config.rs` — add the default combo in `nav_bindings()` (~200–241) or `timestamp_bindings()` (~319–331).
- Modify: `src/input/keymap.rs` — add a dispatch arm in `dispatch_action` (match at ~2118).

**Interfaces:**
- Consumes: `Action::ToggleChapterStart` (Task 2), `chapters::toggle_chapter_start` (Task 4 — wire the arm now; the fn lands in Task 4, so this task's build completes only after Task 4. If executing strictly in order, stub the call as noted).

- [ ] **Step 1: Pick a non-colliding combo.** Confirm `Ctrl+c` is free in the defaults:

Run: `rg -n 'KeyCombo::ctrl\("c"\)' src/input/keymap_config.rs`
Expected: no match → `Ctrl+c` is free. (`c` plain is `SetChapter`, the audio chapter — different action.) If `Ctrl+c` is taken, choose another free combo and use it consistently below.

- [ ] **Step 2: Add the default binding.** In `keymap_config.rs`, in the chosen bindings fn:

```rust
        (KeyCombo::ctrl("c"), Action::ToggleChapterStart),
```

- [ ] **Step 3: Add the dispatch arm.** In `src/input/keymap.rs` `dispatch_action`'s match:

```rust
        Action::ToggleChapterStart => {
            crate::input::actions::chapters::toggle_chapter_start(state, tokio_handle);
        }
```

(`state: &Rc<RefCell<AppState>>` and `tokio_handle` are in scope here — same as the `ToggleBookmark` arm at `keymap.rs:2140`.)

- [ ] **Step 4: Build is expected to fail here** (the `chapters` module/fn doesn't exist yet) — that is fine; Task 4 creates it. Do NOT commit a non-building tree on its own; combine Step 5 with Task 4's commit, OR temporarily point the arm at a `todo!()` you replace in Task 4. Preferred: proceed directly to Task 4, then build+commit Tasks 3+4 together.

- [ ] **Step 5: (Deferred to Task 4 commit.)**

---

### Task 4: The action fn — gate, resolve, toggle+derive, reload

**Files:**
- Create: `src/input/actions/chapters.rs`.
- Modify: `src/input/actions/mod.rs` — add `pub mod chapters;`.
- Test: `src/input/actions/chapters.rs` `#[cfg(test)]` — a pure helper test (see Interfaces).

**Interfaces:**
- Consumes: `queries::toggle_chapter_start` (Task 1), `Action::ToggleChapterStart` dispatch (Task 3), `AppState::line_mapping_id_for_buffer` / `work_line_for_buffer`, `is_prose_work`, `pickers::load_selected_work`-style reload.
- Produces:
  - `pub fn toggle_chapter_start(state: &Rc<RefCell<AppState>>, tokio_handle: &tokio::runtime::Handle)` — the dispatched action.
  - `pub(crate) fn litdb_derive_command(home: &std::path::Path, abbrev: &str) -> std::process::Command` — builds the derive subprocess (pure, testable: asserts program + args).

- [ ] **Step 1: Write the failing test** for the pure command builder. In `src/input/actions/chapters.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::litdb_derive_command;
    use std::path::Path;

    #[test]
    fn builds_derive_command() {
        let cmd = litdb_derive_command(Path::new("/home/u"), "Cromwell");
        assert_eq!(cmd.get_program(), "python3");
        let args: Vec<_> = cmd.get_args().map(|a| a.to_str().unwrap().to_string()).collect();
        assert_eq!(
            args,
            vec![
                "/home/u/utono/litdb/scripts/chapter_divisions.py".to_string(),
                "derive".to_string(),
                "--work".to_string(),
                "Cromwell".to_string(),
            ]
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails.**

Run: `cargo test --bins chapters`
Expected: FAIL — module/function not found.

- [ ] **Step 3: Implement `chapters.rs`.** Create the file:

```rust
//! Prose chapter-start toggle: flip line_mapping.chapter_start on the cursor's
//! paragraph, re-derive (div1,div2) via the litdb tool, reload the work in
//! place. Prose-only. See docs/superpowers/specs/2026-06-26-chapter-start-toggle-keybind-design.md.

use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::cell::RefCell;

use crate::app::AppState;
use crate::db::line_types::is_prose_work;
use crate::log_fmt;

/// Build the `chapter_divisions.py derive` subprocess for a work. Pure so it is
/// unit-testable; the litdb checkout is assumed at <home>/utono/litdb.
pub(crate) fn litdb_derive_command(home: &Path, abbrev: &str) -> Command {
    let script = home.join("utono/litdb/scripts/chapter_divisions.py");
    let mut cmd = Command::new("python3");
    cmd.arg(script).arg("derive").arg("--work").arg(abbrev);
    cmd
}

/// Toggle whether the cursor's paragraph begins a chapter, then re-derive the
/// work's chapter divisions and reload in place (prose only).
pub fn toggle_chapter_start(state: &Rc<RefCell<AppState>>, tokio_handle: &tokio::runtime::Handle) {
    // --- resolve everything from a short borrow ---
    let resolved = {
        let s = state.borrow();
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return,
        };
        if !is_prose_work(&work.work_type) {
            log_fmt!("chapter_start: ignored (work '{}' is not prose)", work.abbrev);
            return;
        }
        let buffer_line = s.current_line;
        let lm_id = match s.line_mapping_id_for_buffer(buffer_line) {
            Some(id) => id,
            None => {
                log_fmt!("chapter_start: no line_mapping id for buffer line {}", buffer_line);
                return;
            }
        };
        (work.abbrev.clone(), lm_id)
    };
    let (abbrev, lm_id) = resolved;

    let handle = tokio_handle.clone();
    let state_rc = state.clone();
    glib::spawn_future_local(async move {
        let abbrev_for_blocking = abbrev.clone();
        let derive_result = handle
            .spawn_blocking(move || {
                // 1. toggle the column (own connection, dropped before derive)
                let new_state = {
                    let conn = crate::db::queries::open_db_rw()
                        .map_err(|e| format!("open_db_rw: {e}"))?;
                    let v = crate::db::queries::toggle_chapter_start(&conn, lm_id)
                        .map_err(|e| format!("toggle: {e}"))?;
                    v
                };
                // 2. re-derive divisions via the litdb tool
                let home = std::env::var("HOME").map_err(|e| format!("HOME: {e}"))?;
                let out = litdb_derive_command(Path::new(&home), &abbrev_for_blocking)
                    .output()
                    .map_err(|e| format!("spawn derive: {e}"))?;
                if !out.status.success() {
                    return Err(format!(
                        "derive failed: {}",
                        String::from_utf8_lossy(&out.stderr)
                    ));
                }
                Ok::<bool, String>(new_state)
            })
            .await;

        match derive_result {
            Ok(Ok(now_marked)) => {
                log_fmt!("chapter_start: {} -> {}", abbrev, if now_marked { "set" } else { "cleared" });
                reload_in_place(&state_rc, &abbrev, lm_id, &handle).await;
            }
            Ok(Err(e)) => log_fmt!("chapter_start: {}", e),
            Err(e) => log_fmt!("chapter_start: join error: {e}"),
        }
    });
}

/// Reload the work after a divisions change, restoring the cursor to the same
/// line_mapping row. Mirrors pickers::load_selected_work's load+prepare+display.
async fn reload_in_place(
    state: &Rc<RefCell<AppState>>,
    abbrev: &str,
    cursor_line_id: i64,
    handle: &tokio::runtime::Handle,
) {
    let abbrev = abbrev.to_string();
    let loaded = handle
        .spawn_blocking(move || {
            let conn = crate::db::queries::open_db().ok()?;
            let work = crate::db::queries::load_work(&conn, &abbrev).ok()?;
            // snapshot read (auto-invalidates on div1/div2 fingerprint change) or rebuild
            let prepared = crate::app::prepare_or_snapshot(&work); // see Step 3b
            Some((work, prepared))
        })
        .await
        .ok()
        .flatten();

    if let Some((work, prepared)) = loaded {
        let mut s = state.borrow_mut();
        crate::app::clear_display(&mut s);
        crate::app::display_work_at_with_prepared(&mut s, work, Some(cursor_line_id), prepared);
    }
}
```

- [ ] **Step 3b: Match the real reload helpers.** The names `prepare_or_snapshot`, `clear_display`, `display_work_at_with_prepared`, `open_db`, `load_work` must match the actual symbols. Confirm and adjust:

Run:
```bash
rg -n "pub fn display_work_at_with_prepared|pub fn clear_display|fn prepare_text_for_display|pub fn load_work|pub fn open_db\b" src/app/mod.rs src/db/queries.rs
rg -n "snapshot::read|snapshot::write|prepare_text_for_display" src/input/actions/pickers.rs
```
Expected: shows the exact signatures used by `pickers::load_selected_work`. Replace the `prepare_or_snapshot(&work)` placeholder with the SAME snapshot-read-else-`prepare_text_for_display`(-then-`snapshot::write`) sequence `load_selected_work` uses (the reference report describes it at `pickers.rs:71–137`). Keep the cursor restore via `target_line = Some(cursor_line_id)` exactly as `display_work_at_with_prepared` expects (its 3rd arg is `target_line_id: Option<i64>` per the reference report). If `load_selected_work` exposes a reusable inner helper, call THAT instead of duplicating.

- [ ] **Step 4: Register the module.** In `src/input/actions/mod.rs` add (alphabetically near `bookmarks`):

```rust
pub mod chapters;
```

- [ ] **Step 5: Build + run the pure test.**

Run: `cargo build && cargo test --bins chapters`
Expected: builds; the `builds_derive_command` test PASSES.

- [ ] **Step 6: Clippy gate.**

Run: `cargo clippy 2>&1 | rg -c '^warning'`
Expected: ≤ 119 (baseline must not increase). Fix any new warnings in the added code.

- [ ] **Step 7: Commit Tasks 3 + 4 together** (first building tree with the keybind wired):

```bash
git add src/input/actions/chapters.rs src/input/actions/mod.rs src/input/keymap_config.rs src/input/keymap.rs
git commit -m "feat(input): Ctrl+c toggles prose chapter-start + re-derives divisions

Resolves the cursor paragraph's line_mapping.id, toggles chapter_start,
shells out to chapter_divisions.py derive, and reloads the work in place
(cursor preserved). Prose-only. Snapshot self-invalidates on div1/div2.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01M4eSF8LVwLbcs49hzJD35M"
```

---

### Task 5: Keybinds overlay entry (if applicable)

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` — the on-screen keybind cheat-sheet, IF it is generated from a static list rather than from the live `Keymap`.

**Interfaces:** none (display only).

- [ ] **Step 1: Determine if the overlay needs a manual entry.**

Run: `rg -n "ToggleBookmark|SetChapter|Action::" src/ui/keybinds_overlay.rs | head`
Expected: if the overlay iterates the live `Keymap`/`Action` set, the new bind appears automatically → SKIP this task. If it has a hand-maintained list of `(key, description)` rows, add one for the chapter-start toggle next to the bookmark/chapter rows.

- [ ] **Step 2: If manual, add the row** matching the file's existing row format, e.g. a `("Ctrl+c", "Toggle chapter start (prose)")` entry. Build:

Run: `cargo build`
Expected: builds.

- [ ] **Step 3: Commit (only if changed).**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(overlay): list Ctrl+c chapter-start toggle"
```

---

### Task 6: Headless visual verification + first-multi-div1-prose check

**Files:** none (verification only).

**Interfaces:** none.

- [ ] **Step 1: Build.**

Run: `cargo build`
Expected: clean build.

- [ ] **Step 2: Seed Cromwell marks from litdb** (so there is structure to see). In a shell:

```bash
~/utono/litdb/.venv/bin/python ~/utono/litdb/scripts/chapter_divisions.py auto-detect --work Cromwell --apply
~/utono/litdb/.venv/bin/python ~/utono/litdb/scripts/chapter_divisions.py derive --work Cromwell
```

(Verifies the litdb side independently and gives Cromwell ~15 real chapters.)

- [ ] **Step 3: Headless toggle check** (per CLAUDE.md Headless Verification). Launch Cromwell in `cage`, screenshot, navigate to a paragraph that should open an undetected chapter, press `Ctrl+c`, screenshot again. Confirm:
  - a new section boundary appears at that paragraph (re-derive + reload worked),
  - the cursor stays on the same paragraph,
  - `~/utono/linux-lit/linux-lit-dev.log` shows `chapter_start: Cromwell -> set`.

Then press `Ctrl+c` again on the same paragraph → boundary disappears (`-> cleared`).

- [ ] **Step 4: First-ever multi-`div1` prose smoke check.** While Cromwell is open and divided, exercise the `2`/`3` scene-jump keys and the synopsis card on a chapter — confirm no panic, pagination is sane, and the synopsis overlay keys to the chapter `div1`. Capture any anomaly in the log for a follow-up (rendering polish is out of THIS plan's scope; the toggle mechanism is what's under test).

- [ ] **Step 5: Report.** Summarize the screenshots/log evidence. No commit (verification only). Hand back to the user for the final live-session confirmation.

---

## Notes for the implementer

- **`log_fmt!`** is the logging macro (`src/logging.rs`); the dev log is cleared each launch and lives at `~/utono/linux-lit/linux-lit-dev.log`.
- **Do not** reuse the `c` plain key — it is `SetChapter` (per-media AUDIO chapter), a different concept from the structural `chapter_start`.
- **WAL safety:** the toggle's `open_db_rw` connection is dropped before the Python `derive` runs, so there is no writer-writer overlap; `derive` opens its own connection.
- **Cursor restore depends on `line_in_div` staying global** (litdb keeps it so) — the same `line_mapping.id` survives the derive (only `div1/div2` change), so `target_line_id = Some(cursor_line_id)` lands on the same paragraph.
- If `display_work_at_with_prepared`'s cursor-restore arg is by `line_mapping.id`, pass `cursor_line_id`; if it restores by buffer line, translate after load. Confirm against the signature in Step 3b before finalizing.
