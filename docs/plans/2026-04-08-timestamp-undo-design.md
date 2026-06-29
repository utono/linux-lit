# Timestamp Undo (U key)

## Summary

Single-level undo for the last timestamp database write. Pressing `U` reverses the most recent `u`, `.`, `i`, `BackSpace`, `p`, or `P` operation, regardless of current cursor position. Clears after use.

## Data Model

New struct on `AppState`:

```rust
pub struct TimestampSnapshot {
    pub citation: String,
    pub start_time: Option<f64>,
    pub end_time: Option<f64>,
    pub is_chapter: bool,
}

pub struct TimestampUndoState {
    pub line_mapping_id: i64,
    pub media_id: i64,
    /// None = row didn't exist before the operation (undo → DELETE)
    pub previous: Option<TimestampSnapshot>,
}
```

New field: `AppState.timestamp_undo: Option<TimestampUndoState>`

## Capture

Before every timestamp write operation, snapshot the current `line_timestamps` row state into `state.timestamp_undo`:

- Query the DB for the row matching `(line_mapping_id, media_id)`
- If row exists: capture `TimestampSnapshot { citation, start_time, end_time, is_chapter }`
- If no row: set `previous` to `None`

This applies to all six timestamp keybinds: `u`, `.`, `i`, `BackSpace`, `p`, `P`.

## Undo Logic (U key)

When `U` is pressed and `timestamp_undo` is `Some(undo_state)`:

- If `undo_state.previous` is `None`: the original operation was a fresh INSERT. Undo by calling `delete_timestamp(line_mapping_id, media_id)`.
- If `undo_state.previous` is `Some(snapshot)`: restore the row to its previous state via a new `restore_timestamp` query that does a full UPSERT with all fields.
- If the row was deleted (BackSpace) and `previous` is `Some`: re-INSERT the full snapshot.

After the DB write:
- Update the in-memory `line.timestamp` on the corresponding work line
- Refresh the sign column to reflect the restored state
- Set `state.timestamp_undo = None` (consumed, no re-undo)

When `U` is pressed and `timestamp_undo` is `None`: no-op.

## New DB Query

`restore_timestamp(line_mapping_id, media_id, citation, start_time, end_time, is_chapter)`:

```sql
INSERT INTO line_timestamps (citation, line_mapping_id, media_id, start_time, end_time, source, is_chapter)
VALUES (?1, ?2, ?3, ?4, ?5, 'manual', ?6)
ON CONFLICT(line_mapping_id, media_id)
DO UPDATE SET start_time = ?4, end_time = ?5, is_chapter = ?6, updated_at = CURRENT_TIMESTAMP
```

## Snapshot Query

`get_timestamp_snapshot(line_mapping_id, media_id) -> Option<TimestampSnapshot>`:

```sql
SELECT citation, start_time, end_time, is_chapter
FROM line_timestamps
WHERE line_mapping_id = ?1 AND media_id = ?2
```

## Keybind

`U` (Shift+u) in timestamp mode. Routes through `keymap.rs` to a new `undo_timestamp` handler in `timestamps.rs`.

## Sign Column

After undo, call the same sign column refresh used by the other timestamp handlers to update the gutter indicator.

## Scope

- Six capture points (one per timestamp keybind)
- Two new DB queries (`get_timestamp_snapshot`, `restore_timestamp`)
- One new handler (`undo_timestamp`)
- One new keybind (`U`)
- Two new structs (`TimestampSnapshot`, `TimestampUndoState`)
- One new `AppState` field
