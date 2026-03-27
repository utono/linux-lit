# Text File Display for Shakespeare Works

## Problem

Shakespeare works currently display as database lines joined by newlines — stripping blank lines, speaker names, act/scene headers, and stage directions. The result lacks the natural formatting of the Folger text files. The user wants Shakespeare works to display the raw `.txt` file content from `~/utono/literature/shakespeare-william/folger-txt/` while preserving timestamp sync, gutter marks, highlighting, and MPV integration.

## Prior Art

The Neovim `lit` project (`~/utono/lit`) already solves this with `citation_map.lua`:
- Loads the `.txt` file into a Neovim buffer directly
- Builds a bidirectional map between buffer lines and `line_mapping` DB rows
- Uses normalized text matching with a sliding-window parallel walk
- Both buffer lines and DB rows are in document order, enabling O(n) matching

## Design

### Database Change

Add a `text_file` column to the `works` table:

```sql
ALTER TABLE works ADD COLUMN text_file TEXT;
```

Populate for Shakespeare works with full paths to Folger `.txt` files (43 works). This is a manual DB migration outside the scope of this code change — linux-lit reads the column if present.

### New Data Structures

**`LineMap`** (new struct):
- `buffer_to_work: Vec<Option<usize>>` — for each buffer line index, the corresponding index into `work.lines` (None for unmapped lines: blanks, speaker names, headers, stage directions)
- `work_to_buffer: Vec<usize>` — for each `work.lines` index, the corresponding buffer line index
- `dialogue_buffer_lines: Vec<usize>` — precomputed list of buffer line indices that map to dialogue lines, for efficient dialogue jumping

### New Module: `src/text_file_map.rs`

Single public function:

```rust
pub fn build_line_map(file_lines: &[String], work: &Work) -> LineMap
```

**Algorithm** (port of `citation_map.lua:build_core`):

1. Normalize each file line: trim, lowercase, strip non-alphanumeric chars (`[^a-z0-9 ]`), collapse whitespace. This matches the DB `normalized_text` column which was populated by the same normalization during import.
2. Walk file lines and `work.lines` (which carry `normalized` text from DB) in parallel
3. Use a sliding window of 50 DB rows for matching
4. Confirmation check: when a match is found beyond the cursor position, verify the next non-empty file line also matches the next DB row to avoid false positives on short/common lines like "he" or "sir"
5. Build both mapping vectors and the precomputed dialogue line list
6. Log a warning if fewer than 80% of DB rows were matched (helps diagnose stale/wrong files)

Note: `work_to_buffer` entries are best-effort. Works with many repeated short lines may have occasional mismatches despite the confirmation check.

### Model Changes

**`Work` struct** — add field:
```rust
pub text_file: Option<String>,
```

**`AppState`** — add field:
```rust
pub line_map: Option<LineMap>,
```

`buffer_line_count` is derived from `state.buffer.line_count()` or `line_map` length rather than stored separately, avoiding sync bugs.

A helper method `effective_line_count(&self) -> usize` returns `line_map.buffer_to_work.len()` if present, otherwise `work.lines.len()`.

### Query Changes

**`load_work`** in `src/db/queries.rs`:
- Add `text_file` to the works query: `SELECT title, COALESCE(author, ''), work_type, text_file FROM works WHERE abbrev = ?1`

### Display Changes

**`rebuild_buffer_text`** in `src/app.rs`:
- If `work.text_file` is `Some(path)` and the file exists and is valid UTF-8:
  - Read the file into a `Vec<String>` of lines
  - Build a `LineMap` via `build_line_map(&file_lines, &work)`
  - Store `line_map` in `AppState`
  - Set the buffer text to the file contents (joined by newlines)
  - Log a message indicating text file mode and match count
- If `text_file` is set but file is missing or unreadable, log a warning and fall through
- Otherwise (no text_file):
  - Existing behavior: join `work.lines` text with newlines
  - Set `line_map` to `None`

### Navigation Changes

**All functions in `src/input/navigation.rs`** that use `work.lines.len()` for bounds:
- Use `state.effective_line_count()` instead
- Affected: `move_cursor`, `jump_to_start`, `jump_to_end`, `page_forward`, `page_backward`

**`seek_to_current_line`**:
- If `line_map` is present: look up `line_map.buffer_to_work[current_line]`
  - If `Some(idx)`: access `work.lines[idx].timestamp` and seek
  - If `None`: skip seek (unmapped line — blank, speaker name, header)
- If no `line_map`: existing behavior (direct index into `work.lines`)

**Dialogue jumping** (`jump_to_next_dialogue`, `jump_to_prev_dialogue`):
- If `line_map` is present: use `dialogue_buffer_lines` precomputed list for O(log n) lookup via binary search
- If no `line_map`: existing behavior

### Gutter Changes

**`place_timestamp_marks`** in `src/gutter.rs`:
- Currently takes `has_timestamp: Vec<bool>` with one entry per work line
- When `line_map` is present: build the `has_timestamp` vec with one entry per buffer line, mapping through `buffer_to_work` to check timestamps
- The vec length must match buffer line count

**`setup_chunk_gutter`** / `build_chunk_positions`**:
- Currently builds a positions vec indexed by `work.lines.len()`
- When `line_map` is present: vec must be `buffer_line_count` long
- Chunk `a_line`/`b_line` reference `div1`/`div2`/`line_in_div` which map to work-line indices — use `work_to_buffer` to translate to buffer line indices for gutter rendering

### Highlight/Dim Changes

**`update_highlight_and_ensure_visible`**:
- Currently dims all lines except the current one, using work line indices
- With `line_map`: operates on buffer line indices (which it already does via the GTK buffer). No change needed — the highlighting already works on buffer lines.

### AB Repeat Changes

**`apply_ab_dim`** and AB repeat logic:
- `ab_repeat.a_line` and `ab_repeat.b_line` are set from `current_line` (cursor position), which is a buffer line index in text file mode
- The dim logic operates on GTK buffer lines, so it works correctly with buffer line indices
- When AB timestamps are needed (for MPV looping), translate buffer line indices through `buffer_to_work` to get the work line's timestamp

### MPV Sync Changes

**`display_work`** timestamp data sent to MPV:
- The `line_id_to_index` map sent to the MPV client remains unchanged — it maps `line_id` to work-lines index
- The MPV client runs on the Tokio runtime and has no access to `LineMap`

**GTK event handler** in `src/main.rs`:
- When `MpvEvent::CursorSync(work_line_idx)` is received:
  - If `line_map` is present: translate via `line_map.work_to_buffer[work_line_idx]` to get the buffer line index before updating `current_line`
  - If no `line_map`: use `work_line_idx` directly (existing behavior)

### Cursor Restore

**Startup cursor restore** in `src/app.rs`:
- `last_cursor_line` stores a line index. When text file mode is active, this is a buffer line index.
- The text file must be read and `LineMap` built before clamping `last_cursor_line` — move text file loading into `display_work` before the clamping logic, not deferred to `rebuild_buffer_text`
- Clamp against `effective_line_count()` instead of `work.lines.len()`

### Search

Search matches are built from GTK buffer content, so match line indices are naturally buffer line indices. No changes needed — works correctly in both modes.

## Scope Boundary

- The `text_file` DB column migration and population is done manually by the user, not by this code
- If `text_file` is NULL or the file doesn't exist, behavior is unchanged
- This design is general enough for any work with a text file, not just Shakespeare
- File change detection is out of scope for v1

## Files to Create or Modify

- **Create**: `src/text_file_map.rs` — line mapping module
- **Modify**: `src/db/models.rs` — add `text_file` field to `Work`
- **Modify**: `src/db/queries.rs` — query `text_file` column
- **Modify**: `src/app.rs` — `rebuild_buffer_text`, `display_work`, `AppState`, `effective_line_count`
- **Modify**: `src/input/navigation.rs` — use `effective_line_count`, map through `line_map`
- **Modify**: `src/gutter.rs` — build gutter/chunk marks for buffer line count
- **Modify**: `src/main.rs` — add `mod text_file_map`, translate `CursorSync` through `line_map`
