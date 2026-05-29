---
name: debug-sync-page-turn
description: Use when playback sync turns the page too early, too late, or to the wrong line — cursor jumps to a new page before the current page's dialogue is finished, or the page stalls when it should have turned. Also use when the cursor lands on a non-dialogue line (stage direction, speaker name) during sync.
argument-hint: <screenshot-path>
---

# Debug Sync Page Turn

Diagnose premature, late, or incorrect page turns triggered by playback sync.

## Always-On Logging

These log prefixes are always written regardless of debug mode (`Ctrl+d`):

- `CURSOR_SYNC:` — every sync event that changes `current_line` (line_idx, buffer_line, current, page_top)
- `SYNC_ADVANCE:` — the page-turn decision point (current, last_vis, page_top)
- `SYNC_PAGE_TURN:` — confirms a page turn fired (current, last_vis, old_top, new_top)
- `PAGE_TURN:` — the actual `set_page` call (new_top, old_top, current_line, transition)

No special setup is needed. Read the log directly:

```bash
cat ~/utono/linux-lit/linux-lit-dev.log
```

## Step 1: Read the Screenshot

Read the screenshot with the Read tool. Identify:
- The **highlighted line** (cursor background)
- The **last visible dialogue line** on the page
- Whether the page turned when it shouldn't have (premature) or didn't turn when it should have (stall)

## Step 2: Check the Log for the Turn Decision

Search for `SYNC_ADVANCE` near the timestamp of the failure:

- **Premature turn:** `current > last_vis` evaluated true when the cursor was still visible. Check if `last_vis` is correct — it should be the raw last visible line, not trimmed for pagination. If `last_vis` is lower than expected, check `last_raw_visible_line` in `src/input/viewport.rs`.
- **Stall (no turn):** `SYNC_ADVANCE` shows `current <= last_vis` repeatedly. Either timestamps are gapped (no CursorSync event lands past `last_vis`) or suppression is killing events.
- **Wrong target:** `SYNC_PAGE_TURN` shows `new_top` that doesn't match the expected next dialogue line. Check `page_turn_top` in `src/input/viewport.rs`.

## Step 3: Check for Non-Dialogue Cursor Landing

If the cursor landed on a stage direction, speaker name, or blank line:

1. Check that `SetTimestamps` filters to dialogue-only lines. Three build sites:
   - `src/app.rs` — `display_work` (primary) and media discovery callback
   - `src/input/timestamps.rs` — `resync_mpv_timestamps`
2. Each site must filter `ts_data` to only include lines where `is_dialogue == true`
3. Verify with: `rg "dialogue_ids\|is_dialogue" src/app.rs src/input/timestamps.rs`

## Key Functions

| Function | File | Role |
|----------|------|------|
| `last_raw_visible_line` | `src/input/viewport.rs` | Raw last visible line (no trim) — used by sync |
| `last_fully_visible_line` | `src/input/viewport.rs` | Trimmed last visible (for pagination, NOT sync) |
| `update_highlight_and_advance_page` | `src/input/highlight.rs` | Page-turn decision: `current > last_vis` |
| `page_turn_top` | `src/input/viewport.rs` | Computes new page top (backs up for speaker/blank) |
| `find_line_for_time` | `src/mpv/client.rs` | Maps MPV time to line index |

## Common Root Causes

1. **`last_vis` trimmed too aggressively:** `last_fully_visible_line` trims trailing speakers/stage directions for pagination — if sync used this instead of `last_raw_visible_line`, the page turns before the cursor reaches the actual bottom
2. **Non-dialogue timestamps in SetTimestamps:** Stage direction lines with timestamps cause `CursorSync` to land on non-dialogue lines
3. **Timestamp gap at page boundary:** No timestamp spans the transition from last visible to first off-screen line, so `current > last_vis` never fires
4. **Suppression blocks sync:** 86400s suppress from navigation keybinds not cleared on playback resume

### Previously Fixed

- **Premature turn from trimmed last_vis (fixed 2026-05):** `update_highlight_and_advance_page` used `last_fully_visible_line` (which trims trailing speakers/blanks for pagination). Sync saw `last_vis` as ~10 lines before the actual bottom, causing early turns. Fix: switched to `last_raw_visible_line`.
- **Stage direction cursor landing (fixed 2026-05):** `SetTimestamps` included all lines regardless of `is_dialogue`. Stage directions with timestamps caused `CursorSync` to highlight them. Fix: filter `ts_data` to dialogue-only at all three build sites.

## Step 4: Fix and Verify

Make the minimal fix. Rebuild with `cargo build`. The user runs `cargo run` to test.
