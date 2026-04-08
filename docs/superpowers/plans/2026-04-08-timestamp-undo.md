# Timestamp Undo (U key) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add single-level undo for the last timestamp database write, triggered by `U` (Shift+u).

**Architecture:** A new `Option<TimestampUndoState>` field on `AppState` captures a snapshot of the `line_timestamps` row before each write. The `U` keybind restores that snapshot (or deletes the row if it was a fresh insert). Two new DB queries: one to read the snapshot, one to restore it.

**Tech Stack:** Rust, rusqlite, GTK4/sourceview5

---

### Task 1: Add snapshot structs and AppState field

**Files:**
- Modify: `src/input/timestamps.rs:1-6` (add structs at top)
- Modify: `src/app.rs:149` (add field before closing brace)
- Modify: `src/app.rs:653` (add field init)

- [ ] **Step 1: Add structs to timestamps.rs**

Add after the existing imports at the top of `src/input/timestamps.rs`:

```rust
#[derive(Debug, Clone)]
pub struct TimestampSnapshot {
    pub citation: String,
    pub start_time: Option<f64>,
    pub end_time: Option<f64>,
    pub is_chapter: bool,
}

#[derive(Debug, Clone)]
pub struct TimestampUndoState {
    pub line_mapping_id: i64,
    pub media_id: i64,
    /// None = row didn't exist before the operation (undo → DELETE)
    pub previous: Option<TimestampSnapshot>,
}
```

- [ ] **Step 2: Add field to AppState struct**

In `src/app.rs`, add before the closing `}` of `AppState` (after `loading_work` at line 149):

```rust
    pub timestamp_undo: Option<crate::input::timestamps::TimestampUndoState>,
```

- [ ] **Step 3: Initialize field in AppState constructor**

In `src/app.rs`, add after `loading_work: Rc::new(Cell::new(false)),` (line 653):

```rust
        timestamp_undo: None,
```

- [ ] **Step 4: Build to verify**

Run: `cargo build`
Expected: compiles with no new errors

- [ ] **Step 5: Commit**

```bash
git add src/input/timestamps.rs src/app.rs
git commit -m "Add TimestampUndoState structs and AppState field"
```

---

### Task 2: Add DB queries for snapshot and restore

**Files:**
- Modify: `src/db/queries.rs:458` (add after `delete_timestamp`)

- [ ] **Step 1: Add get_timestamp_snapshot query**

In `src/db/queries.rs`, add after the `delete_timestamp` function (after line 458):

```rust
pub fn get_timestamp_snapshot(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
) -> Result<Option<crate::input::timestamps::TimestampSnapshot>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT citation, start_time, end_time, is_chapter \
         FROM line_timestamps \
         WHERE line_mapping_id = ?1 AND media_id = ?2",
    )?;
    let result = stmt.query_row(rusqlite::params![line_mapping_id, media_id], |row| {
        Ok(crate::input::timestamps::TimestampSnapshot {
            citation: row.get(0)?,
            start_time: row.get(1)?,
            end_time: row.get(2)?,
            is_chapter: row.get::<_, bool>(3).unwrap_or(false),
        })
    });
    match result {
        Ok(snap) => Ok(Some(snap)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}
```

- [ ] **Step 2: Add restore_timestamp query**

Add immediately after `get_timestamp_snapshot`:

```rust
pub fn restore_timestamp(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    citation: &str,
    start_time: Option<f64>,
    end_time: Option<f64>,
    is_chapter: bool,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO line_timestamps (citation, line_mapping_id, media_id, start_time, end_time, source, is_chapter) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'manual', ?6) \
         ON CONFLICT(line_mapping_id, media_id) \
         DO UPDATE SET start_time = ?4, end_time = ?5, is_chapter = ?6, updated_at = CURRENT_TIMESTAMP",
        rusqlite::params![citation, line_mapping_id, media_id, start_time, end_time, is_chapter],
    )?;
    Ok(())
}
```

- [ ] **Step 3: Build to verify**

Run: `cargo build`
Expected: compiles (queries are not called yet, but types must resolve)

- [ ] **Step 4: Commit**

```bash
git add src/db/queries.rs
git commit -m "Add get_timestamp_snapshot and restore_timestamp DB queries"
```

---

### Task 3: Add snapshot capture helper

**Files:**
- Modify: `src/input/timestamps.rs` (add helper function after `resync_mpv_timestamps`)

- [ ] **Step 1: Add capture_undo_snapshot helper**

In `src/input/timestamps.rs`, add after `resync_mpv_timestamps` (after line 29):

```rust
/// Capture the current state of a timestamp row before mutating it.
/// Stores the snapshot in state.timestamp_undo for single-level undo.
fn capture_undo_snapshot(state: &mut AppState, line_mapping_id: i64, media_id: i64) {
    let conn = match crate::db::queries::open_db_rw() {
        Ok(c) => c,
        Err(_) => return,
    };
    let previous = crate::db::queries::get_timestamp_snapshot(&conn, line_mapping_id, media_id)
        .unwrap_or(None);
    state.timestamp_undo = Some(TimestampUndoState {
        line_mapping_id,
        media_id,
        previous,
    });
}
```

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: compiles (helper is not called yet)

- [ ] **Step 3: Commit**

```bash
git add src/input/timestamps.rs
git commit -m "Add capture_undo_snapshot helper for timestamp undo"
```

---

### Task 4: Wire snapshot capture into all six timestamp handlers

**Files:**
- Modify: `src/input/timestamps.rs` (add one `capture_undo_snapshot` call into each handler)

Each handler already has `line.id` (the `line_mapping_id`) and `media_id` available. Insert the capture call right before the DB write in each handler.

- [ ] **Step 1: Wire into set_start_time**

`capture_undo_snapshot` takes `&mut AppState`, so it must be called before the mutable borrow of `state.current_work`. Extract `line_id` with a read-only borrow first, call `capture_undo_snapshot`, then do the mutable borrow.

In `set_start_time`, replace the block from line 52 (the `{` before `let work = match &mut state.current_work`) through line 77 (closing `}` after the in-memory update) with:

```rust
    let line_id = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        work.lines[line_idx].id
    };

    capture_undo_snapshot(state, line_id, media_id);

    {
        let work = match &mut state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line = &mut work.lines[line_idx];

        // DB write
        let conn = match crate::db::queries::open_db_rw() {
            Ok(c) => c,
            Err(e) => {
                crate::logging::log(&format!("TS: open_db_rw failed: {}", e));
                return false;
            }
        };
        if let Err(e) = crate::db::queries::upsert_start_time(&conn, line.id, media_id, &line.citation, time_pos) {
            crate::logging::log(&format!("TS: upsert_start_time failed: {}", e));
            return false;
        }

        // Update in-memory
        match &mut line.timestamp {
            Some(ts) => ts.start = time_pos,
            None => line.timestamp = Some(TimeRange { start: time_pos, end: 0.0, sentence_start: None }),
        }
    }
```

- [ ] **Step 2: Wire into set_chapter**

In `set_chapter`, the nearby-chapter check already uses a read-only borrow of `state.current_work`. After the nearby-chapter check block (line 151) and before the mutable borrow block (line 153), add:

```rust
    let line_id = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        work.lines[line_idx].id
    };

    capture_undo_snapshot(state, line_id, media_id);
```

Then the existing mutable borrow block at line 153 stays as-is.

- [ ] **Step 3: Wire into set_end_time**

In `set_end_time`, before the existing mutable borrow block (line 219), add:

```rust
    let line_id = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        work.lines[line_idx].id
    };

    capture_undo_snapshot(state, line_id, media_id);
```

Then the existing block starting `let start_time = {` stays as-is.

- [ ] **Step 4: Wire into delete_timestamp**

In `delete_timestamp`, before the mutable borrow block (line 268), add:

```rust
    let line_id = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        work.lines[line_idx].id
    };

    capture_undo_snapshot(state, line_id, media_id);
```

Then the existing block stays as-is.

- [ ] **Step 5: Wire into nudge_start_time**

In `nudge_start_time`, before the mutable borrow block (line 311), add:

```rust
    let line_id = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        work.lines[line_idx].id
    };

    capture_undo_snapshot(state, line_id, media_id);
```

Then the existing block stays as-is. This covers both `nudge_start_backward` and `nudge_start_forward` since they delegate to `nudge_start_time`.

- [ ] **Step 6: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 7: Commit**

```bash
git add src/input/timestamps.rs
git commit -m "Capture undo snapshot before all timestamp DB writes"
```

---

### Task 5: Implement undo_timestamp handler

**Files:**
- Modify: `src/input/timestamps.rs` (add `undo_timestamp` function at the end, before the nudge helpers)

- [ ] **Step 1: Add undo_timestamp function**

Add before `nudge_start_backward` in `src/input/timestamps.rs`:

```rust
/// Undo the last timestamp operation (U).
pub fn undo_timestamp(state: &mut AppState) -> bool {
    let undo = match state.timestamp_undo.take() {
        Some(u) => u,
        None => return false,
    };

    let conn = match crate::db::queries::open_db_rw() {
        Ok(c) => c,
        Err(e) => {
            crate::logging::log(&format!("TS: undo open_db_rw failed: {}", e));
            return false;
        }
    };

    match &undo.previous {
        None => {
            // Row didn't exist before — delete it
            if let Err(e) = crate::db::queries::delete_timestamp(&conn, undo.line_mapping_id, undo.media_id) {
                crate::logging::log(&format!("TS: undo delete failed: {}", e));
                return false;
            }
        }
        Some(snap) => {
            // Restore the previous row state
            if let Err(e) = crate::db::queries::restore_timestamp(
                &conn,
                undo.line_mapping_id,
                undo.media_id,
                &snap.citation,
                snap.start_time,
                snap.end_time,
                snap.is_chapter,
            ) {
                crate::logging::log(&format!("TS: undo restore failed: {}", e));
                return false;
            }
        }
    }

    // Update in-memory state, then extract values for sign column update.
    // Must drop the mutable borrow of current_work before accessing
    // state.line_map, state.has_timestamp, etc.
    let (buffer_line, has_ts, is_ch) = {
        let work = match &mut state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line = match work.lines.iter_mut().find(|l| l.id == undo.line_mapping_id) {
            Some(l) => l,
            None => return false,
        };

        match &undo.previous {
            None => {
                line.timestamp = None;
                line.is_chapter = false;
            }
            Some(snap) => {
                match (snap.start_time, snap.end_time) {
                    (Some(start), end) => {
                        line.timestamp = Some(TimeRange {
                            start,
                            end: end.unwrap_or(0.0),
                            sentence_start: None,
                        });
                    }
                    (None, _) => {
                        line.timestamp = None;
                    }
                }
                line.is_chapter = snap.is_chapter;
            }
        }

        let has_ts = line.timestamp.is_some();
        let is_ch = line.is_chapter;
        let work_idx = work.lines.iter().position(|l| l.id == undo.line_mapping_id);
        (work_idx, has_ts, is_ch)
    };
    // buffer_line here is the work_idx; resolve to actual buffer line
    let buffer_line = match buffer_line {
        Some(idx) => {
            if let Some(ref lm) = state.line_map {
                lm.work_to_buffer.get(idx).copied()
            } else {
                Some(idx)
            }
        }
        None => None,
    };

    crate::logging::log(&format!(
        "TS: undo line_mapping_id={} restored={}", undo.line_mapping_id, undo.previous.is_some()
    ));

    resync_mpv_timestamps(state);

    // Update sign column
    if let Some(bl) = buffer_line {
        {
            let mut ht = state.has_timestamp.borrow_mut();
            if bl < ht.len() {
                ht[bl] = has_ts;
            }
        }
        {
            let mut ch = state.is_chapter_line.borrow_mut();
            if bl < ch.len() {
                ch[bl] = is_ch;
            }
        }
    }

    if let Some(ref renderer) = state.gutter_renderer {
        renderer.queue_draw();
    }

    true
}
```

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: compiles (handler is not called yet from keybind)

- [ ] **Step 3: Commit**

```bash
git add src/input/timestamps.rs
git commit -m "Add undo_timestamp handler"
```

---

### Task 6: Wire U keybind in keymap.rs

**Files:**
- Modify: `src/input/keymap.rs:1298-1300` (add `U` case near the other timestamp keybinds)

- [ ] **Step 1: Add U keybind**

In `src/input/keymap.rs`, add after the `"P"` case (after line 1300):

```rust
        "U" => {
            crate::input::timestamps::undo_timestamp(&mut state.borrow_mut())
        }
```

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all previously passing tests still pass

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Wire U keybind to undo_timestamp handler"
```

