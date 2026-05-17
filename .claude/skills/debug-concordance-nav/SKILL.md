---
name: debug-concordance-nav
description: Use when r or R concordance navigation jumps to the wrong line, fails to seek MPV, or cross-work media loading fails. Also use when the concordance picker selects a word but the cursor doesn't move or MPV doesn't play from the correct position.
argument-hint: <screenshot-path>
---

# Debug Concordance Navigation

Diagnose issues with r/R concordance jumping, cross-work media loading, and concordance picker word selection.

## Log File

```bash
cat ~/utono/linux-lit/linux-lit-dev.log
```

## Diagnostic Steps

### 1. Read the log

Filter for concordance events:

```bash
grep "CONC_\|PENDING_SEEK\|MEDIA_\|MPV.*Seek\|MPV.*Resume\|MPV.*loadfile\|MPV.*connect\|MPV playback\|MPV connection" ~/utono/linux-lit/linux-lit-dev.log
```

### 2. Check each r/R jump

Every r/R press produces a `CONC_NEXT` or `CONC_PREV` log line followed by a `CONC_POS` line. Verify:

- **`contains_word=true`** — cursor landed on a line containing the concordance word. If `false`, the hit list or line resolution is wrong.
- **`seek_time=NONE`** — the hit line has no timestamp for the active media_id. MPV won't seek. This is expected for unrecorded lines.
- **`seek_time=<number>`** — MPV should seek to this time. Check that a matching `MPV: Seek time=<number>` line follows.
- **`cross_work=true`** — a work switch is happening. Check for the full cross-work sequence below.

### 3. Check cross-work jump sequence

When `cross_work=true`, the expected log sequence is:

```
CONC_NEXT: [N/total]->[M/total] work='OLD'->'NEW' cross_work=true
CONC_JUMP: CROSS-WORK from 'OLD' to 'NEW' — saving position
CONC_JUMP: loading work 'NEW' from database...
CONC_JUMP: CROSS-WORK loaded 'Title' lines=N timestamps=N — opening media picker
```

Then either auto-select (single media) or user picks from media picker:

```
MEDIA_PICKER: auto-selecting single media: id=N path='...'
MEDIA_PICKER: loadfile '...' into existing MPV
MPV: loadfile replace '...'
CONC_SEEK_CURRENT: line_id=N seek_time=N.N text='...' — pending until file loaded
PENDING_SEEK: file loaded, seeking to N.N resume=true
MPV: ResumeAndSeek time=N.N
```

### 4. Common failure patterns

**Cursor on wrong line (contains_word=false):**
- Check `line_mapping_id` in the CONC_POS log against the database:
  ```bash
  sqlite3 ~/utono/litdb/data/lit.db "SELECT id, normalized_text FROM line_mapping WHERE id = <line_id>"
  ```
- If the DB row contains the word but `contains_word=false`, the `line.text` field differs from `normalized_text` (display text vs search text).

**MPV doesn't seek after cross-work jump:**
- Check that `PENDING_SEEK` appears after `loadfile`. If missing, `pending_loadfile_seek` was not set — check `CONC_SEEK_CURRENT` log.
- Check that `MPV playback: paused` or a `TimePos` event appears between `loadfile` and `PENDING_SEEK`. The seek fires on the first `TimePos` after loadfile.
- If `PENDING_SEEK` appears but MPV plays from the start, the seek command may have arrived before MPV finished loading. Check the time gap between `loadfile` and `PENDING_SEEK`.

**MPV seeks to wrong time (sentence start instead of hit line):**
- `concordance_seek` should use the hit line's own `line_mapping_id` to find the timestamp, not the sentence start. Check `seek_time` in `CONC_POS` against the line's actual timestamp:
  ```bash
  sqlite3 ~/utono/litdb/data/lit.db "SELECT lt.start_time FROM line_timestamps lt WHERE lt.line_mapping_id = <line_id> AND lt.media_id = <media_id>"
  ```

**Media picker shows for single-media work:**
- Check `MEDIA_PICKER: work='X' found N media files`. If N=1, it should auto-select. If the picker still showed, the auto-select code path was skipped.

**No concordance active toast on r/R:**
- Expected when `concordance_state` is None. User needs to pick a word with Ctrl+\.

### 5. Key source files

- `src/input/actions/concordance.rs` — r/R handlers, cross-work jump, seek logic
- `src/input/actions/pickers.rs` — media picker open/confirm, auto-select
- `src/mpv/client.rs` — LoadFile, Seek, ResumeAndSeek commands
- `src/main.rs` — PENDING_SEEK handler in TimePos event loop
- `src/concordance.rs` — ConcordanceState, advance/retreat
- `src/db/concordance.rs` — find_word_occurrences query

### 6. If a screenshot was provided

Read the screenshot to identify:
- Which work is shown (check status bar at bottom)
- Which line is highlighted (the cursor line)
- Whether the highlighted line contains the concordance word
- Cross-reference with the log's `CONC_POS` entries
