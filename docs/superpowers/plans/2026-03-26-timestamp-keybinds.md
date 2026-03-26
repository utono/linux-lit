# Timestamp Keybinds Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add keybindings (`u`/`Right`, `i`, `BackSpace`, `p`, `P`) for setting, deleting, and nudging timestamps, writing changes back to the shared `lit.db`.

**Architecture:** Cache MPV's `time-pos` in AppState via a new `MpvEvent::TimePos` variant. Keybinds read this cached value and perform synchronous SQLite writes on the GTK thread. After each write, update in-memory `Line.timestamp` and re-sync the MPV client's timestamp cache.

**Tech Stack:** Rust, GTK4, rusqlite, tokio mpsc channels

**Spec:** `docs/superpowers/specs/2026-03-26-timestamp-keybinds-design.md`

---

## File Map

- **Modify:** `src/db/models.rs` — add `citation` field to `Line`
- **Modify:** `src/db/queries.rs` — expand `load_work()` query, add `open_db_rw()`, add `upsert_start_time()`, `update_end_time()`, `delete_timestamp()`
- **Modify:** `src/mpv/commands.rs` — add `MpvEvent::TimePos(f64)`
- **Modify:** `src/mpv/client.rs` — emit `TimePos` event
- **Modify:** `src/app.rs` — add `current_time_pos` and `media_id` to AppState, load `media_id` in `display_work()`, reset on work switch
- **Modify:** `src/main.rs` — handle `MpvEvent::TimePos` in event loop
- **Create:** `src/input/timestamps.rs` — timestamp keybind handlers
- **Modify:** `src/input/mod.rs` — add `pub mod timestamps;`
- **Modify:** `src/input/keymap.rs` — route `u`, `Right`, `i`, `BackSpace`, `p`, `P` to timestamp handlers

---

### Task 1: Add `citation` field to Line model and expand load_work() query

**Files:**
- Modify: `src/db/models.rs:13-22`
- Modify: `src/db/queries.rs:41-60`

- [ ] **Step 1: Add `citation` field to `Line` struct**

In `src/db/models.rs`, add `citation: String` to the `Line` struct:

```rust
pub struct Line {
    pub id: i64,
    pub citation: String,
    pub text: String,
    pub normalized: String,
    pub speaker: Option<String>,
    pub is_dialogue: bool,
    pub timestamp: Option<TimeRange>,
}
```

- [ ] **Step 2: Expand load_work() SQL to include div1, div2, line_in_div**

In `src/db/queries.rs`, change the line_stmt query (line 41-44) to:

```rust
let mut line_stmt = conn.prepare(
    "SELECT id, canonical_text, normalized_text, speaker, div1, div2, line_in_div \
     FROM line_mapping WHERE work_abbrev = ?1 \
     ORDER BY div1, div2, line_in_div",
)?;
```

And update the query_map closure (line 47-59) to construct citation:

```rust
let lines: Vec<Line> = line_stmt
    .query_map([abbrev], |row| {
        let text: String = row.get(1)?;
        let normalized: String = row.get(2)?;
        let speaker: Option<String> = row.get(3)?;
        let div1: i64 = row.get(4)?;
        let div2: i64 = row.get(5)?;
        let line_in_div: i64 = row.get(6)?;
        let citation = format!("{}.{}.{}.{}", abbrev, div1, div2, line_in_div);
        Ok(Line {
            id: row.get(0)?,
            citation,
            is_dialogue: line_types::is_dialogue(&text, is_prose),
            text,
            normalized,
            speaker,
            timestamp: None,
        })
    })?
    .collect::<Result<_, _>>()?;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 4: Run existing tests**

Run: `cargo test`
Expected: All tests pass (existing tests don't check citation field)

- [ ] **Step 5: Commit**

```bash
git add src/db/models.rs src/db/queries.rs
git commit -m "feat: add citation field to Line model, expand load_work query"
```

---

### Task 2: Add open_db_rw() and timestamp write functions

**Files:**
- Modify: `src/db/queries.rs`

- [ ] **Step 1: Add open_db_rw() function**

After `open_db()` in `src/db/queries.rs`, add:

```rust
pub fn open_db_rw() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(db_path())?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    Ok(conn)
}
```

- [ ] **Step 2: Add upsert_start_time()**

```rust
pub fn upsert_start_time(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    citation: &str,
    start_time: f64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO line_timestamps (citation, line_mapping_id, media_id, start_time, source) \
         VALUES (?1, ?2, ?3, ?4, 'manual') \
         ON CONFLICT(line_mapping_id, media_id) \
         DO UPDATE SET start_time = ?4, updated_at = CURRENT_TIMESTAMP",
        rusqlite::params![citation, line_mapping_id, media_id, start_time],
    )?;
    Ok(())
}
```

- [ ] **Step 3: Add update_end_time()**

```rust
pub fn update_end_time(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
    end_time: f64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE line_timestamps SET end_time = ?3, updated_at = CURRENT_TIMESTAMP \
         WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_mapping_id, media_id, end_time],
    )?;
    Ok(())
}
```

- [ ] **Step 4: Add delete_timestamp()**

```rust
pub fn delete_timestamp(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM line_timestamps WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_mapping_id, media_id],
    )?;
    Ok(())
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 6: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat: add open_db_rw and timestamp write functions"
```

---

### Task 3: Add MpvEvent::TimePos and emit from client

**Files:**
- Modify: `src/mpv/commands.rs:23-27`
- Modify: `src/mpv/client.rs:30-33`

- [ ] **Step 1: Add TimePos variant to MpvEvent**

In `src/mpv/commands.rs`, add to the `MpvEvent` enum:

```rust
pub enum MpvEvent {
    CursorSync(usize),
    ConnectionStatus(bool),
    PlaybackState(bool),
    TimePos(f64),
}
```

- [ ] **Step 2: Emit TimePos from MPV client**

In `src/mpv/client.rs`, in the `Ok(_)` arm where `parse_time_pos` is called (around line 30), add a `TimePos` emit. Change:

```rust
Ok(_) => {
    if let Some(pos) = parse_time_pos(&line_buf) {
        if let Some(idx) = find_line_for_time(pos, &timestamps, &line_id_to_index) {
            let _ = evt_tx.send(MpvEvent::CursorSync(idx)).await;
        }
    }
```

To:

```rust
Ok(_) => {
    if let Some(pos) = parse_time_pos(&line_buf) {
        let _ = evt_tx.send(MpvEvent::TimePos(pos)).await;
        if let Some(idx) = find_line_for_time(pos, &timestamps, &line_id_to_index) {
            let _ = evt_tx.send(MpvEvent::CursorSync(idx)).await;
        }
    }
```

- [ ] **Step 3: Handle TimePos in GTK event loop**

In `src/main.rs`, add a match arm in the event loop (after the `PlaybackState` arm, around line 76):

```rust
MpvEvent::TimePos(pos) => {
    state_for_events.borrow_mut().current_time_pos = pos;
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: Fails — `current_time_pos` doesn't exist on AppState yet. That's expected; Task 4 adds it.

- [ ] **Step 5: Commit (combined with Task 4)**

This step is deferred to after Task 4.

---

### Task 4: Add current_time_pos and media_id to AppState

**Files:**
- Modify: `src/app.rs:23-47` (AppState struct)
- Modify: `src/app.rs:163-185` (build_window state init)
- Modify: `src/app.rs:278-360` (display_work)
- Modify: `src/db/queries.rs:98-107` (media query)

- [ ] **Step 1: Add fields to AppState**

In `src/app.rs`, add to the `AppState` struct (after `search_current_tag`):

```rust
    pub current_time_pos: f64,
    pub media_id: Option<i64>,
```

- [ ] **Step 2: Initialize fields in build_window**

In `src/app.rs`, in the `Rc::new(RefCell::new(AppState { ... }))` block (around line 163), add:

```rust
        current_time_pos: 0.0,
        media_id: None,
```

- [ ] **Step 3: Expand media query to include mf.id**

In `src/db/queries.rs`, change the media_stmt query (line 99-103) to also select `mf.id`:

```rust
let mut media_stmt = conn.prepare(
    "SELECT mf.id, mf.path FROM media_files mf \
     JOIN work_media_associations wma ON wma.media_id = mf.id \
     WHERE wma.work_abbrev = ?1 \
     ORDER BY wma.priority DESC",
)?;
```

Change the `media_paths` collection to return tuples of `(id, path)`:

```rust
let media_rows: Vec<(i64, String)> = media_stmt
    .query_map([abbrev], |row| Ok((row.get(0)?, row.get(1)?)))?
    .collect::<Result<_, _>>()?;
let media_id = media_rows.first().map(|(id, _)| *id);
let media_paths: Vec<String> = media_rows.into_iter().map(|(_, path)| path).collect();
```

Return `media_id` alongside the Work. Add `media_id: Option<i64>` to the `Work` struct in `models.rs`:

```rust
pub struct Work {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
    pub lines: Vec<Line>,
    pub timestamps: Vec<Timestamp>,
    pub media_paths: Vec<String>,
    pub media_id: Option<i64>,
}
```

And set it in the `Ok(Work { ... })` return:

```rust
Ok(Work {
    abbrev: abbrev.to_string(),
    title,
    author,
    work_type,
    lines,
    timestamps,
    media_paths,
    media_id,
})
```

- [ ] **Step 4: Set media_id and reset current_time_pos in display_work()**

In `src/app.rs`, at the top of `display_work()` (after `clear_search` on line 279), add:

```rust
state.current_time_pos = 0.0;
state.media_id = work.media_id;
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully (including the Task 3 changes)

- [ ] **Step 6: Run tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 7: Commit Tasks 3 and 4 together**

```bash
git add src/mpv/commands.rs src/mpv/client.rs src/main.rs src/app.rs src/db/queries.rs src/db/models.rs
git commit -m "feat: add TimePos event, current_time_pos and media_id to AppState"
```

---

### Task 5: Create timestamp keybind handlers

**Files:**
- Create: `src/input/timestamps.rs`
- Modify: `src/input/mod.rs`

- [ ] **Step 1: Add module declaration**

In `src/input/mod.rs`, add:

```rust
pub mod timestamps;
```

- [ ] **Step 2: Create src/input/timestamps.rs with helper to re-sync MPV timestamps**

```rust
use crate::app::AppState;
use crate::db::models::TimeRange;

const NUDGE_STEP: f64 = 0.2;

/// Re-send timestamps to MPV client after a write, built from Line.timestamp (single source of truth).
fn resync_mpv_timestamps(state: &AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };
    let mut ts_data: Vec<(i64, f64, f64)> = Vec::new();
    let mut id_to_idx: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
    for (i, line) in work.lines.iter().enumerate() {
        id_to_idx.insert(line.id, i);
        if let Some(ts) = &line.timestamp {
            ts_data.push((line.id, ts.start, ts.end));
        }
    }
    ts_data.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let _ = state
        .cmd_tx
        .try_send(crate::mpv::MpvCommand::SetTimestamps {
            timestamps: ts_data,
            line_id_to_index: id_to_idx,
        });
}

/// Set start time on current line from MPV position (u / Right).
pub fn set_start_time(state: &mut AppState) -> bool {
    let media_id = match state.media_id {
        Some(id) => id,
        None => return false,
    };
    let time_pos = state.current_time_pos;
    let line_idx = state.current_line;

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
            None => line.timestamp = Some(TimeRange { start: time_pos, end: 0.0 }),
        }
    }
    crate::logging::log(&format!("TS: set start_time={:.2} line={}", time_pos, line_idx));

    resync_mpv_timestamps(state);
    true
}

/// Set end time on current line from MPV position (i).
pub fn set_end_time(state: &mut AppState) -> bool {
    let media_id = match state.media_id {
        Some(id) => id,
        None => return false,
    };
    let time_pos = state.current_time_pos;
    let line_idx = state.current_line;

    let start_time = {
        let work = match &mut state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line = &mut work.lines[line_idx];

        // Guard: must have existing timestamp
        let start_time = match &line.timestamp {
            Some(ts) => ts.start,
            None => return false,
        };

        let conn = match crate::db::queries::open_db_rw() {
            Ok(c) => c,
            Err(e) => {
                crate::logging::log(&format!("TS: open_db_rw failed: {}", e));
                return false;
            }
        };
        if let Err(e) = crate::db::queries::update_end_time(&conn, line.id, media_id, time_pos) {
            crate::logging::log(&format!("TS: update_end_time failed: {}", e));
            return false;
        }

        // Update in-memory
        line.timestamp.as_mut().unwrap().end = time_pos;
        start_time
    };
    crate::logging::log(&format!("TS: set end_time={:.2} line={}", time_pos, line_idx));

    resync_mpv_timestamps(state);

    // Seek to start and resume playback
    let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::ResumeAndSeek(start_time));
    true
}

/// Delete timestamp on current line (BackSpace).
pub fn delete_timestamp(state: &mut AppState) -> bool {
    let media_id = match state.media_id {
        Some(id) => id,
        None => return false,
    };
    let line_idx = state.current_line;

    {
        let work = match &mut state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line = &mut work.lines[line_idx];

        // Guard: must have existing timestamp
        if line.timestamp.is_none() {
            return false;
        }

        let conn = match crate::db::queries::open_db_rw() {
            Ok(c) => c,
            Err(e) => {
                crate::logging::log(&format!("TS: open_db_rw failed: {}", e));
                return false;
            }
        };
        if let Err(e) = crate::db::queries::delete_timestamp(&conn, line.id, media_id) {
            crate::logging::log(&format!("TS: delete_timestamp failed: {}", e));
            return false;
        }

        line.timestamp = None;
    }
    crate::logging::log(&format!("TS: deleted timestamp line={}", line_idx));

    resync_mpv_timestamps(state);
    true
}

/// Nudge start time by delta seconds (p = -0.2, P = +0.2).
pub fn nudge_start_time(state: &mut AppState, delta: f64) -> bool {
    let media_id = match state.media_id {
        Some(id) => id,
        None => return false,
    };
    let line_idx = state.current_line;

    let new_start = {
        let work = match &mut state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line = &mut work.lines[line_idx];

        // Guard: must have existing timestamp
        let current_start = match &line.timestamp {
            Some(ts) => ts.start,
            None => return false,
        };

        let new_start = (current_start + delta).max(0.0);

        let conn = match crate::db::queries::open_db_rw() {
            Ok(c) => c,
            Err(e) => {
                crate::logging::log(&format!("TS: open_db_rw failed: {}", e));
                return false;
            }
        };
        if let Err(e) = crate::db::queries::upsert_start_time(&conn, line.id, media_id, &line.citation, new_start) {
            crate::logging::log(&format!("TS: nudge upsert failed: {}", e));
            return false;
        }

        // Update in-memory
        line.timestamp.as_mut().unwrap().start = new_start;
        new_start
    };
    crate::logging::log(&format!("TS: nudge start_time={:.2} delta={:.1} line={}", new_start, delta, line_idx));

    resync_mpv_timestamps(state);

    // Seek to new position
    let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::Seek(new_start));
    true
}

/// Nudge start backward by 0.2s.
pub fn nudge_start_backward(state: &mut AppState) -> bool {
    nudge_start_time(state, -NUDGE_STEP)
}

/// Nudge start forward by 0.2s.
pub fn nudge_start_forward(state: &mut AppState) -> bool {
    nudge_start_time(state, NUDGE_STEP)
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 4: Commit**

```bash
git add src/input/mod.rs src/input/timestamps.rs
git commit -m "feat: add timestamp keybind handlers"
```

---

### Task 6: Wire keybinds in keymap.rs

**Files:**
- Modify: `src/input/keymap.rs:172-244`

- [ ] **Step 1: Add timestamp keybinds to the single-key match block**

In `src/input/keymap.rs`, add these arms before the `_ => false` fallthrough (line 244):

```rust
        "u" | "Right" => {
            crate::input::timestamps::set_start_time(&mut state.borrow_mut())
        }
        "i" => {
            crate::input::timestamps::set_end_time(&mut state.borrow_mut())
        }
        "BackSpace" => {
            crate::input::timestamps::delete_timestamp(&mut state.borrow_mut())
        }
        "p" => {
            crate::input::timestamps::nudge_start_backward(&mut state.borrow_mut())
        }
        "P" => {
            crate::input::timestamps::nudge_start_forward(&mut state.borrow_mut())
        }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 4: Run clippy**

Run: `cargo clippy`
Expected: No warnings

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat: wire timestamp keybinds (u/Right, i, BackSpace, p, P)"
```

---

### Task 7: Final verification

- [ ] **Step 1: Full build**

Run: `cargo build`
Expected: Clean compile

- [ ] **Step 2: Full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 3: Clippy**

Run: `cargo clippy`
Expected: No warnings
