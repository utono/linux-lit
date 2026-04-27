# Cursor Advancement and Dialogue Navigation

How the cursor moves between lines — manually via keybindings and automatically via MPV playback sync.

## Manual Navigation

### Line-by-line (j/k)

`move_cursor()` in `src/input/navigation.rs` advances by `delta` lines (typically +1 or -1). It moves to **any** line, not just dialogue. The only filtering: when translations are visible, it skips over translation lines.

### Dialogue jump (q forward, , backward)

`jump_to_next_dialogue()` and `jump_to_prev_dialogue()` skip non-dialogue lines to land on the next/previous dialogue line.

The target is found one of two ways:

- **With a line_map** (translations active): searches `line_map.dialogue_buffer_lines`, a precomputed vec of buffer-line indices that are dialogue
- **Without a line_map**: iterates `work.lines` checking `is_dialogue`

### What counts as dialogue

Defined in `src/db/line_types.rs`, `is_dialogue(text, is_prose)`:

- Blank lines are never dialogue
- **Prose works** (novel, essay_collection, prose_book, prose): every non-blank line is dialogue
- **Plays**: a line is dialogue unless it matches one of: speaker name (`HAMLET.`), stage direction (`[Exit]`), act/scene marker (`ACT 1`, `SCENE 2`, `PROLOGUE`, `EPILOGUE`), or separator (`====`)

### After landing on a line

After any cursor movement:

1. `current_line` is set to the target
2. The dim highlight updates (all lines dimmed except current)
3. The viewport adjusts — page turn in EReader mode if the line isn't visible, or center-scroll in Scroll mode
4. MPV seeks to the line's audio timestamp (with 0.2s preroll), if the line has one

## Automatic Advancement via Playback Sync

When MPV is connected and playing, the cursor follows the audio automatically. This involves three components.

### 1. MPV observes time-pos

On connection (`src/mpv/client.rs`), the client sends `observe_property` for `time-pos`. MPV then streams periodic position updates over the IPC socket.

### 2. time-pos maps to a line index

Each update runs through `find_line_for_time()`:

- Adds a 0.3s **sync preroll** (`SYNC_PREROLL`) to the raw time-pos so the cursor highlights a line slightly *before* its audio begins
- Binary-searches the sorted timestamp list to find which line's `start_time` the effective time falls within
- Maps the line's database ID back to a buffer-line index
- Sends a `CursorSync(buffer_line)` event

### 3. CursorSync moves the cursor

In the GTK event loop (`src/main.rs`), `CursorSync` events:

- Are **suppressed** when the user recently navigated manually (`suppress_sync_until`) — a 500ms window after seeking, or indefinitely when the user navigated to an untimestamped line
- Are **ignored** during active search mode
- Translate work-line indices to buffer-line indices when a line_map is present (translations mode)
- Set `current_line`, update highlight, ensure the line is visible (page turn if needed), and persist the position to config

### 4. Advancing past untimestamped lines

When `CursorSync` lands on a timestamped line, it checks whether the **next dialogue line** lacks a timestamp. If so, it sets `pending_advance = Some((end_time, next_buffer_line))`.

Then, on subsequent `TimePos` events, once `time_pos >= end_time` (the current line's audio has finished), the cursor advances to that untimestamped line. This advance also sets `suppress_sync_until` to 86400 seconds so the cursor stays put — no further CursorSync events will pull it away until the user manually navigates again.

This handles the case where some lines in a work have no audio timestamps but should still scroll past as playback continues.

## Suppression mechanism

`suppress_sync_until` is an `Option<Instant>` that gates CursorSync processing:

- **Manual seek to timestamped line**: suppressed for 500ms while MPV processes the seek, preventing the sync from snapping the cursor back to where it was
- **Manual seek to untimestamped line**: suppressed for 86400s (effectively forever) so the cursor stays where the user put it
- **Auto-advance to untimestamped line**: same 86400s suppression
- **Search exit**: suppression is cleared so sync resumes

## Constants

- `SEEK_PREROLL` = 0.2s — audio starts slightly before the line's timestamp when the user navigates
- `SYNC_PREROLL` = 0.3s — cursor highlights a line slightly before its audio begins during playback
- `PAGE_OVERLAP` = 1 line — overlap between pages on page turns for reading continuity
