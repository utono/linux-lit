# Timestamp Keybinds Design

**Date:** 2026-03-26
**Status:** Draft

## Overview

Add keybindings to linux-lit for setting, deleting, and nudging timestamps on individual lines, writing changes back to the shared `lit.db` database. This mirrors the timestamp editing workflow in the Neovim lit plugin.

## Keybindings

- `u` / `Right` — Set start time from current MPV position. Creates a new timestamp row if none exists, otherwise updates. Does not seek MPV.
- `i` — Set end time from current MPV position. Updates existing row only. Seeks to start time and resumes playback.
- `BackSpace` — Delete timestamp for current line.
- `p` — Nudge start time backward by 0.2s. Seeks to new start time.
- `P` (Shift+p) — Nudge start time forward by 0.2s. Seeks to new start time.

### Guards

- `i`, `BackSpace`, `p`, `P` are no-ops if the current line has no existing timestamp.
- All timestamp binds are no-ops if `state.media_id` is `None` (no media loaded).

## Approach: Cache time-pos in AppState (Approach C)

The MPV client already receives `time-pos` property changes via `observe_property`. Currently this data only feeds `CursorSync`. The change: also emit `MpvEvent::TimePos(f64)` so the GTK event loop can update `AppState.current_time_pos`. Keybinds then read this field directly — no MPV round-trip needed.

## Data Model Changes

### AppState

- `current_time_pos: f64` — updated continuously from `MpvEvent::TimePos`
- `media_id: Option<i64>` — the active media file's ID, set when a work is loaded (highest-priority media file)

### Line model

- `citation: String` — new field on the `Line` struct in `models.rs`, constructed as `{work_abbrev}.{div1}.{div2}.{line_in_div}` at load time

**Note:** `Line.id` is the `line_mapping.id` value. The spec refers to this as `line_mapping_id` in SQL contexts. The `load_work()` query must be expanded to also SELECT `div1`, `div2`, `line_in_div` so the citation can be constructed at load time. The `work_abbrev` is available from the parent `Work.abbrev` (passed to `load_work()` as the `abbrev` parameter).

### MpvEvent

- `TimePos(f64)` — new variant, emitted alongside `CursorSync` from the MPV client

### DB access

- New `open_db_rw()` function that opens without `SQLITE_OPEN_READ_ONLY`
- The rw connection should set `PRAGMA journal_mode=WAL` for safe concurrent access with the Neovim lit plugin
- Write functions use their own connection
- DB writes are synchronous on the GTK thread (rusqlite on a local SQLite file completes in sub-millisecond time)

## DB Write Operations

Three new functions in `src/db/queries.rs`:

### upsert_start_time

```sql
INSERT INTO line_timestamps (citation, line_mapping_id, media_id, start_time, source)
VALUES (?1, ?2, ?3, ?4, 'manual')
ON CONFLICT(line_mapping_id, media_id)
DO UPDATE SET start_time = ?4, updated_at = CURRENT_TIMESTAMP
```

Handles both creation (new timestamp) and update (existing timestamp).

### update_end_time

```sql
UPDATE line_timestamps SET end_time = ?3, updated_at = CURRENT_TIMESTAMP
WHERE line_mapping_id = ?1 AND media_id = ?2
```

Row must already exist (created by `u`/`Right` first).

### delete_timestamp

```sql
DELETE FROM line_timestamps WHERE line_mapping_id = ?1 AND media_id = ?2
```

All three functions also update the in-memory `Line.timestamp` so the UI stays in sync without reloading.

### In-memory update semantics

- **upsert (new):** Set `Line.timestamp = Some(TimeRange { start: new_time, end: 0.0 })`
- **upsert (existing):** Update `start` field, preserve existing `end`
- **update_end_time:** Update `end` field, preserve existing `start`
- **delete:** Set `Line.timestamp = None`

### MPV-side timestamp cache

The MPV client holds its own copy of timestamps (via `SetTimestamps`). After any write, re-send `MpvCommand::SetTimestamps` with updated data so that `CursorSync` uses current values. Build the `SetTimestamps` data from the `Line.timestamp` fields on `work.lines` (the single source of truth that was just updated), not from the separate `Work.timestamps` vec.

### Initialization

`current_time_pos` initializes to `0.0`. The `media_id` guard (`None` = no-op) prevents writing a stale `0.0` position when MPV is not connected.

## MPV Client Changes

In `src/mpv/client.rs`, when `parse_time_pos` returns `Some(pos)`:

- Existing: `find_line_for_time` → `MpvEvent::CursorSync`
- New: also emit `MpvEvent::TimePos(pos)`

In the GTK event loop, new match arm: `MpvEvent::TimePos(pos)` sets `state.current_time_pos = pos`.

## media_id Resolution

When `display_work()` loads a work, it already queries `media_files` for paths. Expand the query to also SELECT `mf.id` and store the first result's ID as `state.media_id = Some(id)`. The query already orders by `priority DESC`, so index 0 is highest priority.

On work switch, `display_work()` must also reset `state.current_time_pos = 0.0` to prevent writing a stale position from the previous work's MPV session. On disconnect or when no work is loaded, `state.media_id` is `None`, which guards all timestamp writes.

**Timestamp load filtering:** For this phase, `load_work()` continues to load all timestamps regardless of `media_id` (existing behavior). Filtering timestamps by the selected `media_id` is deferred to the future media selection phase.

**Future phase:** Add a keybind to pop up a media file selection window from the list of media IDs associated with the work.

## Keybind Behavior Details

### u / Right (set start time)

1. Read `state.current_time_pos`
2. Get current line's `line_mapping_id` and `citation`
3. Call `upsert_start_time(line_mapping_id, media_id, citation, current_time_pos)`
4. Update in-memory `Line.timestamp`

### i (set end time)

1. Read `state.current_time_pos`
2. Get current line's `line_mapping_id`
3. Call `update_end_time(line_mapping_id, media_id, current_time_pos)`
4. Update in-memory `Line.timestamp`
5. Send `MpvCommand::ResumeAndSeek(line.timestamp.start)` to seek to the line's existing start time and play the segment

### BackSpace (delete timestamp)

1. Get current line's `line_mapping_id`
2. Call `delete_timestamp(line_mapping_id, media_id)`
3. Set in-memory `Line.timestamp = None`

### p (nudge start backward)

1. Get current line's `timestamp.start`
2. Compute `new_start = (start - 0.2).max(0.0)`
3. Call `upsert_start_time(line_mapping_id, media_id, citation, new_start)`
4. Update in-memory `Line.timestamp`
5. Send `MpvCommand::Seek(new_start)` to seek to new position

### P (nudge start forward)

1. Same as `p` but `new_start = start + 0.2`

## Database Schema Reference

```sql
CREATE TABLE line_timestamps (
  id INTEGER PRIMARY KEY,
  citation TEXT NOT NULL,
  line_mapping_id INTEGER NOT NULL REFERENCES line_mapping(id),
  media_id INTEGER REFERENCES media_files(id),
  start_time REAL,
  end_time REAL,
  source TEXT DEFAULT 'manual',
  is_chapter INTEGER DEFAULT 0,
  is_scene_start INTEGER DEFAULT 0,
  sentence_start_time REAL,
  sentence_end_time REAL,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(line_mapping_id, media_id)
);
```

The `citation` is not a column on `line_mapping` — it is constructed as `{work_abbrev}.{div1}.{div2}.{line_in_div}` from the `line_mapping` table.
