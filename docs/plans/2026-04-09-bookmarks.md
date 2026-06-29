# Bookmarks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-work bookmarks that persist in lit.db, with keybinds to toggle, cycle, and jump to the most recent bookmark.

**Architecture:** New `bookmarks` table in lit.db, `is_bookmarked` vec in AppState (same pattern as `has_timestamp`/`is_chapter_line`), bookmark glyph in the existing timestamp gutter, and navigation functions following the `jump_to_next_chapter`/`jump_to_prev_chapter` pattern.

**Tech Stack:** Rust, GTK4/libadwaita, sourceview5, rusqlite

**Spec:** `docs/superpowers/specs/2026-04-09-bookmarks-design.md`

---

## File Map

- **Modify:** `src/db/queries.rs` — add `ensure_bookmarks_table()`, `load_bookmarks()`, `toggle_bookmark()`, `most_recent_bookmark()`
- **Modify:** `src/app.rs:35-152` — add `is_bookmarked` field to `AppState`
- **Modify:** `src/app.rs:598` — initialize `is_bookmarked` in struct literal
- **Modify:** `src/app.rs:1410-1468` — populate `is_bookmarked` during `display_work_at` alongside `has_timestamp`/`is_manual`/`is_chapter_line`
- **Modify:** `src/app.rs:1472-1482` — pass `is_bookmarked` to `setup_timestamp_gutter()`
- **Modify:** `src/gutter.rs:17-74` — add `is_bookmarked` parameter, add `★` glyph with highest priority
- **Modify:** `src/input/navigation.rs` — add `next_bookmark()`, `prev_bookmark()`, `jump_to_most_recent_bookmark()`
- **Modify:** `src/input/keymap.rs:907-917` — extend `pending_g` handler for `g'` sequence
- **Modify:** `src/input/keymap.rs:1316-1346` — move media picker from `m` to `Ctrl+m`
- **Modify:** `src/input/keymap.rs` (single keys section) — add `m`, `apostrophe`, `quotedbl` keybinds

---

### Task 1: Database — Create bookmarks table and query functions

**Files:**
- Modify: `src/db/queries.rs`

- [ ] **Step 1: Add `ensure_bookmarks_table()` function**

Add after the `open_db_rw()` function (line 396):

```rust
/// Ensure the bookmarks table exists in the database.
pub fn ensure_bookmarks_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS bookmarks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            work_abbrev TEXT NOT NULL,
            line_mapping_id INTEGER NOT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            FOREIGN KEY (work_abbrev) REFERENCES works(abbrev),
            FOREIGN KEY (line_mapping_id) REFERENCES line_mapping(id),
            UNIQUE(work_abbrev, line_mapping_id)
        );"
    )?;
    Ok(())
}
```

- [ ] **Step 2: Add `load_bookmarks()` function**

Add after `ensure_bookmarks_table`:

```rust
/// Load all bookmarked line_mapping_ids for a work.
pub fn load_bookmarks(conn: &Connection, work_abbrev: &str) -> Result<Vec<i64>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT line_mapping_id FROM bookmarks WHERE work_abbrev = ?1"
    )?;
    let rows = stmt.query_map([work_abbrev], |row| row.get::<_, i64>(0))?;
    rows.collect()
}
```

- [ ] **Step 3: Add `toggle_bookmark()` function**

```rust
/// Toggle a bookmark on a line. Returns true if added, false if removed.
pub fn toggle_bookmark(
    conn: &Connection,
    work_abbrev: &str,
    line_mapping_id: i64,
) -> Result<bool, rusqlite::Error> {
    let existing: Option<i64> = conn.query_row(
        "SELECT id FROM bookmarks WHERE work_abbrev = ?1 AND line_mapping_id = ?2",
        rusqlite::params![work_abbrev, line_mapping_id],
        |row| row.get(0),
    ).optional()?;

    if let Some(id) = existing {
        conn.execute("DELETE FROM bookmarks WHERE id = ?1", [id])?;
        Ok(false)
    } else {
        conn.execute(
            "INSERT INTO bookmarks (work_abbrev, line_mapping_id) VALUES (?1, ?2)",
            rusqlite::params![work_abbrev, line_mapping_id],
        )?;
        Ok(true)
    }
}
```

Note: `optional()` requires `use rusqlite::OptionalExtension;` — check if already imported. If not, add it to the `use` statement at the top of queries.rs.

- [ ] **Step 4: Add `most_recent_bookmark()` function**

```rust
/// Get the line_mapping_id of the most recently created bookmark for a work.
pub fn most_recent_bookmark(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT line_mapping_id FROM bookmarks WHERE work_abbrev = ?1 ORDER BY created_at DESC LIMIT 1",
        [work_abbrev],
        |row| row.get(0),
    ).optional()
}
```

- [ ] **Step 5: Add test for bookmark queries**

Add to the `#[cfg(test)] mod tests` block at the end of queries.rs:

```rust
#[test]
fn test_bookmark_toggle() {
    let conn = open_db_rw().expect("Failed to open lit.db rw");
    ensure_bookmarks_table(&conn).expect("Failed to create bookmarks table");

    // Use a known work and line
    let work_abbrev = "Ham";
    let line_id: i64 = conn.query_row(
        "SELECT id FROM line_mapping WHERE work_abbrev = ?1 LIMIT 1",
        [work_abbrev],
        |row| row.get(0),
    ).expect("Hamlet should have lines");

    // Clean up any leftover test bookmark
    let _ = conn.execute(
        "DELETE FROM bookmarks WHERE work_abbrev = ?1 AND line_mapping_id = ?2",
        rusqlite::params![work_abbrev, line_id],
    );

    // Toggle on
    let added = toggle_bookmark(&conn, work_abbrev, line_id).unwrap();
    assert!(added, "First toggle should add bookmark");

    // Should appear in load_bookmarks
    let bookmarks = load_bookmarks(&conn, work_abbrev).unwrap();
    assert!(bookmarks.contains(&line_id));

    // Should be the most recent
    let recent = most_recent_bookmark(&conn, work_abbrev).unwrap();
    assert_eq!(recent, Some(line_id));

    // Toggle off
    let removed = toggle_bookmark(&conn, work_abbrev, line_id).unwrap();
    assert!(!removed, "Second toggle should remove bookmark");

    // Should no longer appear
    let bookmarks = load_bookmarks(&conn, work_abbrev).unwrap();
    assert!(!bookmarks.contains(&line_id));
}
```

- [ ] **Step 6: Run tests to verify**

Run: `cargo test test_bookmark_toggle -- --nocapture`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat: add bookmarks table and query functions (load, toggle, most_recent)"
```

---

### Task 2: AppState — Add `is_bookmarked` field and populate on work load

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add `is_bookmarked` field to `AppState` struct**

At `src/app.rs:77` (after `is_chapter_line`), add:

```rust
    pub is_bookmarked: Rc<RefCell<Vec<bool>>>,
```

- [ ] **Step 2: Initialize `is_bookmarked` in the AppState struct literal**

At `src/app.rs:600` (after `is_chapter_line: Rc::new(RefCell::new(Vec::new())),`), add:

```rust
        is_bookmarked: Rc::new(RefCell::new(Vec::new())),
```

- [ ] **Step 3: Populate `is_bookmarked` in `display_work_at`**

After the `is_chapter_line` population block (after line 1468 `*state.is_chapter_line.borrow_mut() = new_is_ch;`), add:

```rust
        // Populate bookmark flags
        let bookmark_ids: std::collections::HashSet<i64> = {
            let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
            crate::db::queries::load_bookmarks(&conn, &work_abbrev)
                .unwrap_or_default()
                .into_iter()
                .collect()
        };
        let new_is_bookmarked: Vec<bool> = if let Some(ref lm) = state.line_map {
            lm.buffer_to_work
                .iter()
                .map(|opt_idx| {
                    opt_idx
                        .and_then(|idx| Some(bookmark_ids.contains(&state.current_work.as_ref()?.lines.get(idx)?.id)))
                        .unwrap_or(false)
                })
                .collect()
        } else {
            state
                .current_work
                .as_ref()
                .map(|w| w.lines.iter().map(|l| bookmark_ids.contains(&l.id)).collect())
                .unwrap_or_default()
        };
        *state.is_bookmarked.borrow_mut() = new_is_bookmarked;
```

Note: `work_abbrev` should already be in scope — it's the abbrev of the work being loaded. Check what variable name is used at this point in `display_work_at`. It's `work.abbrev` before the work is moved into `state.current_work`, or `state.current_work.as_ref().unwrap().abbrev` after. Find the right variable by reading the surrounding code.

- [ ] **Step 4: Ensure bookmarks table exists on RW open**

In the `display_work_at` function, before the bookmark population block, ensure the table exists. Since `load_bookmarks` uses a read-only connection, and the table may not exist yet, add this call once during app startup. Find where `open_db_rw()` is first called in the app lifecycle (e.g., in `main.rs` or early in `app.rs` setup) and add:

```rust
{
    let conn = crate::db::queries::open_db_rw().expect("Failed to open lit.db rw");
    crate::db::queries::ensure_bookmarks_table(&conn).expect("Failed to create bookmarks table");
}
```

If no obvious startup point exists, add it at the top of `display_work_at` with a `std::sync::Once` guard:

```rust
static BOOKMARKS_INIT: std::sync::Once = std::sync::Once::new();
BOOKMARKS_INIT.call_once(|| {
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::queries::ensure_bookmarks_table(&conn);
    }
});
```

- [ ] **Step 5: Build to verify**

Run: `cargo build`
Expected: Compiles with no errors (warnings about unused `is_bookmarked` are fine at this stage)

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat: add is_bookmarked field to AppState, populate on work load"
```

---

### Task 3: Gutter — Add bookmark `★` glyph

**Files:**
- Modify: `src/gutter.rs:17-74`
- Modify: `src/app.rs:1472-1482` (caller that passes args to `setup_timestamp_gutter`)

- [ ] **Step 1: Add `is_bookmarked` parameter to `setup_timestamp_gutter`**

Change the function signature at `src/gutter.rs:17` to add the parameter after `is_chapter`:

```rust
pub fn setup_timestamp_gutter(
    view: &View,
    visible: Rc<Cell<bool>>,
    has_timestamp: Rc<RefCell<Vec<bool>>>,
    is_manual: Rc<RefCell<Vec<bool>>>,
    is_chapter: Rc<RefCell<Vec<bool>>>,
    is_bookmarked: Rc<RefCell<Vec<bool>>>,
    a_line: Rc<Cell<Option<usize>>>,
    b_line: Rc<Cell<Option<usize>>>,
    left_margin: i32,
    root_color: &str,
) -> sourceview5::GutterRendererText {
```

- [ ] **Step 2: Add bookmark glyph logic in the `connect_query_data` closure**

Replace the glyph selection block (lines 54-68) with bookmark as highest priority:

```rust
        let bm = is_bookmarked.borrow();
        let is_bm = idx < bm.len() && bm[idx];
        let glyph = if is_bm {
            "\u{2605}" // ★
        } else if is_ch {
            "\u{25B8}" // ▸
        } else if idx < ts.len() && ts[idx] {
            if a_line.get() == Some(idx) {
                "\u{25D0}" // ◐
            } else if b_line.get() == Some(idx) {
                "\u{25D1}" // ◑
            } else if is_man {
                "\u{2500}" // ─
            } else {
                "\u{2022}" // •
            }
        } else if is_bm {
            // This branch is unreachable (handled above), kept for clarity
            "\u{2605}" // ★
        } else {
            text_renderer.set_markup("");
            return;
        };
```

Wait — the bookmark glyph should show even when the line has no timestamp. Restructure the logic:

```rust
        let bm = is_bookmarked.borrow();
        let is_bm = idx < bm.len() && bm[idx];
        let has_ts = idx < ts.len() && ts[idx];
        let glyph = if is_bm {
            "\u{2605}" // ★
        } else if is_ch {
            "\u{25B8}" // ▸
        } else if has_ts {
            if a_line.get() == Some(idx) {
                "\u{25D0}" // ◐
            } else if b_line.get() == Some(idx) {
                "\u{25D1}" // ◑
            } else if is_man {
                "\u{2500}" // ─
            } else {
                "\u{2022}" // •
            }
        } else {
            text_renderer.set_markup("");
            return;
        };
```

- [ ] **Step 3: Update the caller in `src/app.rs`**

At the `setup_timestamp_gutter` call site (around line 1472), add `state.is_bookmarked.clone()` after `state.is_chapter_line.clone()`:

```rust
    let renderer = crate::gutter::setup_timestamp_gutter(
        &state.text_view,
        state.sign_column_visible.clone(),
        state.has_timestamp.clone(),
        state.is_manual.clone(),
        state.is_chapter_line.clone(),
        state.is_bookmarked.clone(),
        state.ab_a_line.clone(),
        state.ab_b_line.clone(),
        left_margin,
        &state.theme.root_color,
    );
```

- [ ] **Step 4: Build to verify**

Run: `cargo build`
Expected: Compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add src/gutter.rs src/app.rs
git commit -m "feat: add bookmark star glyph to timestamp gutter"
```

---

### Task 4: Navigation — Add bookmark cycling and jump functions

**Files:**
- Modify: `src/input/navigation.rs`

- [ ] **Step 1: Add `next_bookmark()` function**

Add after the existing `jump_to_next_chapter` function. Follow the same pattern (check `line_map` for text_file works, else iterate directly):

```rust
/// Jump to the next bookmarked line (wraps around).
pub fn next_bookmark(state: &mut AppState) {
    let is_bm = state.is_bookmarked.borrow();
    if is_bm.is_empty() || !is_bm.iter().any(|&b| b) {
        return;
    }
    let line_count = is_bm.len();
    // Search forward from current_line + 1, wrapping around
    for offset in 1..=line_count {
        let idx = (state.current_line + offset) % line_count;
        if is_bm[idx] {
            drop(is_bm);
            state.current_line = idx;
            update_highlight(state);
            let top = page_turn_top(&state.buffer, idx);
            match state.config.navigation_mode {
                crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
                crate::config::NavigationMode::EReader => {
                    set_page_instant(state, top);
                }
            }
            seek_to_current_line(state);
            return;
        }
    }
}
```

- [ ] **Step 2: Add `prev_bookmark()` function**

```rust
/// Jump to the previous bookmarked line (wraps around).
pub fn prev_bookmark(state: &mut AppState) {
    let is_bm = state.is_bookmarked.borrow();
    if is_bm.is_empty() || !is_bm.iter().any(|&b| b) {
        return;
    }
    let line_count = is_bm.len();
    // Search backward from current_line - 1, wrapping around
    for offset in 1..=line_count {
        let idx = (state.current_line + line_count - offset) % line_count;
        if is_bm[idx] {
            drop(is_bm);
            state.current_line = idx;
            update_highlight(state);
            let top = page_turn_top(&state.buffer, idx);
            match state.config.navigation_mode {
                crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
                crate::config::NavigationMode::EReader => {
                    set_page_instant(state, top);
                }
            }
            seek_to_current_line(state);
            return;
        }
    }
}
```

- [ ] **Step 3: Add `jump_to_most_recent_bookmark()` function**

This function needs a DB query, so it takes the tokio handle and state as `Rc<RefCell<AppState>>` (like the media picker pattern in keymap.rs). However, to keep navigation.rs consistent (all functions take `&mut AppState`), we'll make it take a `buffer_line: usize` target — the keymap handler will do the DB query and call a simpler jump function:

```rust
/// Jump to a specific buffer line (used by bookmark jump-to-recent).
pub fn jump_to_line(state: &mut AppState, buffer_line: usize) {
    let line_count = state.effective_line_count();
    if buffer_line >= line_count {
        return;
    }
    state.current_line = buffer_line;
    update_highlight(state);
    let top = page_turn_top(&state.buffer, buffer_line);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
        crate::config::NavigationMode::EReader => {
            set_page_instant(state, top);
        }
    }
    seek_to_current_line(state);
}
```

- [ ] **Step 4: Build to verify**

Run: `cargo build`
Expected: Compiles (warnings about unused functions are fine at this stage)

- [ ] **Step 5: Commit**

```bash
git add src/input/navigation.rs
git commit -m "feat: add bookmark navigation functions (next, prev, jump_to_line)"
```

---

### Task 5: Keybinds — Wire up bookmark keys and move media picker

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Move media picker from `m` to `Ctrl+m`**

Find the Ctrl key handling section (around lines 451-501 where other `is_ctrl && key_name` checks live). Add the media picker logic there:

```rust
    if is_ctrl && key_name == "m" {
        let abbrev = state
            .borrow()
            .current_work
            .as_ref()
            .map(|w| w.abbrev.clone());
        if let Some(abbrev) = abbrev {
            let state_clone = Rc::clone(state);
            let handle = tokio_handle.clone();
            glib::spawn_future_local(async move {
                let items = handle
                    .spawn_blocking(move || {
                        let conn =
                            crate::db::queries::open_db().expect("Failed to open lit.db");
                        crate::db::queries::list_media_for_work(&conn, &abbrev)
                            .unwrap_or_default()
                    })
                    .await
                    .unwrap_or_default();
                {
                    let mut s = state_clone.borrow_mut();
                    s.correction_overlay.hide();
                    s.media_picker.set_items(items);
                }
                state_clone.borrow().media_picker.show();
            });
        }
        return true;
    }
```

Then remove the old `"m" =>` arm (lines 1316-1346) from the single keys match block.

- [ ] **Step 2: Add `m` keybind for toggle bookmark**

Replace the removed `"m" =>` arm in the single keys `match` block with:

```rust
        "m" => {
            let (abbrev, line_mapping_id, buffer_line) = {
                let s = state.borrow();
                let abbrev = s.current_work.as_ref().map(|w| w.abbrev.clone());
                let lm_id = s.current_work.as_ref().and_then(|w| {
                    let work_idx = if let Some(ref lm) = s.line_map {
                        lm.buffer_to_work.get(s.current_line)?.as_ref().copied()
                    } else {
                        Some(s.current_line)
                    };
                    work_idx.and_then(|wi| w.lines.get(wi).map(|l| l.id))
                });
                (abbrev, lm_id, s.current_line)
            };
            if let (Some(abbrev), Some(lm_id)) = (abbrev, line_mapping_id) {
                let state_clone = Rc::clone(state);
                let handle = tokio_handle.clone();
                glib::spawn_future_local(async move {
                    let result = handle
                        .spawn_blocking(move || {
                            let conn = crate::db::queries::open_db_rw()
                                .expect("Failed to open lit.db rw");
                            crate::db::queries::toggle_bookmark(&conn, &abbrev, lm_id)
                        })
                        .await;
                    if let Ok(Ok(added)) = result {
                        let mut s = state_clone.borrow_mut();
                        let mut bm = s.is_bookmarked.borrow_mut();
                        if buffer_line < bm.len() {
                            bm[buffer_line] = added;
                        }
                        // Trigger gutter redraw
                        drop(bm);
                        s.text_view.queue_draw();
                    }
                });
            }
            true
        }
```

- [ ] **Step 3: Add `apostrophe` keybind for next bookmark**

Add in the single keys `match` block:

```rust
        "apostrophe" => {
            if !is_shift {
                navigation::next_bookmark(&mut state.borrow_mut());
            }
            true
        }
```

Note: verify the GTK key name for `'` is `"apostrophe"`. You can check by adding a temporary `log_fmt!("KEY: {}", key_name)` in the key handler if unsure. On most GTK setups it's `"apostrophe"`.

- [ ] **Step 4: Add `quotedbl` keybind for previous bookmark**

The `"` key on standard layouts is Shift+apostrophe, so it arrives as `key_name == "quotedbl"` or as `key_name == "apostrophe" && is_shift`. Handle the shift case in the apostrophe arm:

```rust
        "apostrophe" => {
            if is_shift {
                navigation::prev_bookmark(&mut state.borrow_mut());
            } else {
                navigation::next_bookmark(&mut state.borrow_mut());
            }
            true
        }
```

However, on Real Programmers Dvorak, `"` may be its own key. Check the RPD layout — if `"` sends `quotedbl` independently (not shift+apostrophe), add a separate arm:

```rust
        "quotedbl" => {
            navigation::prev_bookmark(&mut state.borrow_mut());
            true
        }
```

Add both arms to be safe — one will match depending on the layout.

- [ ] **Step 5: Add `g'` keybind for jump to most recent bookmark**

Modify the `pending_g` handler at lines 907-917. After the `if key_name == "g"` block, add an `else if` for apostrophe:

```rust
    if key_state.borrow().pending_g {
        key_state.borrow_mut().pending_g = false;
        if key_name == "g" {
            if state.borrow().visual_selection.is_some() {
                crate::input::visual::extend_to_start(&mut state.borrow_mut());
            } else {
                navigation::jump_to_start(&mut state.borrow_mut());
            }
            return true;
        } else if key_name == "apostrophe" {
            // g' — jump to most recently created bookmark
            let abbrev = state
                .borrow()
                .current_work
                .as_ref()
                .map(|w| w.abbrev.clone());
            if let Some(abbrev) = abbrev {
                let state_clone = Rc::clone(state);
                let handle = tokio_handle.clone();
                glib::spawn_future_local(async move {
                    let result = handle
                        .spawn_blocking(move || {
                            let conn = crate::db::queries::open_db()
                                .expect("Failed to open lit.db");
                            crate::db::queries::most_recent_bookmark(&conn, &abbrev)
                        })
                        .await;
                    if let Ok(Ok(Some(lm_id))) = result {
                        let mut s = state_clone.borrow_mut();
                        // Find the buffer line for this line_mapping_id
                        let buffer_line = if let Some(ref lm) = s.line_map {
                            s.current_work.as_ref().and_then(|w| {
                                let work_idx = w.lines.iter().position(|l| l.id == lm_id)?;
                                Some(lm.work_to_buffer[work_idx])
                            })
                        } else {
                            s.current_work.as_ref().and_then(|w| {
                                w.lines.iter().position(|l| l.id == lm_id)
                            })
                        };
                        if let Some(bl) = buffer_line {
                            navigation::jump_to_line(&mut s, bl);
                        }
                    }
                });
            }
            return true;
        }
        // If pending_g was set but the second key wasn't 'g' or apostrophe,
        // fall through to normal key handling
    }
```

- [ ] **Step 6: Build to verify**

Run: `cargo build`
Expected: Compiles with no errors

- [ ] **Step 7: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat: wire bookmark keybinds (m toggle, '/\" cycle, g' most recent, Ctrl+m media picker)"
```

---

### Task 6: Manual testing and verification

- [ ] **Step 1: Run all tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: No errors (warnings acceptable)

- [ ] **Step 3: Manual smoke test checklist**

The user will run `cargo run` and verify:
1. Open a work — no crash
2. Press `l` to show sign column — gutter displays as before (no bookmark glyphs yet)
3. Press `m` — bookmark toggled, `★` appears in gutter
4. Press `m` again on same line — bookmark removed, `★` disappears
5. Add 2+ bookmarks on different lines
6. Press `'` — jumps to next bookmark, wraps at end
7. Press `"` — jumps to previous bookmark, wraps at start
8. Press `g` then `'` — jumps to most recently created bookmark
9. Press `Ctrl+m` — media picker opens (moved from `m`)
10. Close and reopen the work — bookmarks persist (appear in gutter)

- [ ] **Step 4: Final commit if any fixes needed**

```bash
git add -u
git commit -m "fix: address issues found during bookmark testing"
```
