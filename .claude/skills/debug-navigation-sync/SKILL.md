---
name: debug-navigation-sync
description: Use when playback sync stops working or navigates to the wrong line after pressing comma, q, j, or k — highlight stalls, cursor jumps to wrong position, or audio seeks incorrectly after dialogue navigation keybinds
argument-hint: <screenshot-path>
---

# Debug Navigation Sync

Diagnose why playback sync breaks after using `,` (prev dialogue), `q` (next dialogue), `j`/`k` (viewport scroll), or `Shift+,` (page backward) keybinds.

## Core Principle

These keybinds call `seek_to_current_line`, which sets `suppress_sync_until` to block incoming `CursorSync` events from overriding the user's navigation. Most failures trace to **suppression that never clears** — especially the 86400s permanent suppression when the target line has no timestamp.

## Diagnostic Priority

1. **Screenshot** — identify the stalled/wrong line
2. **Log** — check for suppression and seek entries
3. **Code trace** — only if the log doesn't explain the failure
4. **Database** — only if timestamps at the boundary are suspect

## Step 1: Read the Screenshot

Read the screenshot argument with the Read tool. Identify:
- The **highlighted line** and whether it's correct for what was pressed
- Whether playback is active (playing vs paused)
- The **work** being read and approximate position

## Step 2: Enable Debug Mode and Read the Log

Tell the user to press **Ctrl+d** to enable debug logging before reproducing the issue. The log file will be empty unless debug mode is on.

```bash
cat ~/utono/linux-lit/linux-lit-dev.log
```

**Navigation log prefixes** (permanent, in `src/input/navigation.rs`):
- `NAV_PREV:` — comma key fired, shows `from`, `to`, `page_top`
- `NAV_NEXT:` — q key fired, shows `from`, `to`, `page_top`
- `NAV_BACK:` — Shift+comma fired, shows `prev_top`, `new_top`, `current_line`
- `NAV_PAGE_FWD:` — q triggered a page turn, shows `current`, `old_top`, `new_top`, `history_len`
- `NAV_PAGE_BACK:` — comma triggered a page turn, shows `current`, `old_top`, `new_top`, `lpp`, `history_len`
- `SEEK:` — `seek_to_current_line` fired, shows `line`, `work_idx`, timestamp or `NO_TIMESTAMP`, suppression duration

**Also check these existing prefixes:**
- `CURSOR_SYNC: SUPPRESSED` — sync events blocked, check remaining seconds (86400s = no timestamp)
- `CURSOR_SYNC:` — sync event reached handler, shows `line_idx`, `buffer_line`, `current`, `page_top`
- `PAGE_TURN:` — page turn executed, shows `new_top`, `old_top`
- `MPV playback: playing/paused` — confirm playback state

**Quick diagnosis from log patterns:**
- `SEEK: ... NO_TIMESTAMP suppress=86400s` after `,`/`q`: target line has no timestamp, suppression is permanent until next `toggle_playback` on a timestamped line
- `SEEK: ... suppress=500ms` but sync resumes on wrong line: seeked to wrong timestamp, or `current_line` was set incorrectly
- No `CURSOR_SYNC` entries at all after resuming play: suppression from a prior keybind was never cleared
- `CURSOR_SYNC` entries present but highlight doesn't move: `buffer_to_work` translation is silently dropping events
- `NAV_PAGE_FWD` or `NAV_PAGE_BACK` shows unexpected `new_top`: `page_turn_top` or `lines_per_page` computed wrong

## Step 3: Verify Navigation Logging is Present

The following log lines should already exist in `src/input/navigation.rs`. If any are missing, add them before debugging:
- `NAV_PREV:` in `jump_to_prev_dialogue`
- `NAV_NEXT:` in `jump_to_next_dialogue`
- `NAV_BACK:` in `page_backward_bottom`
- `NAV_PAGE_FWD:` in `scroll_after_jump_forward`
- `NAV_PAGE_BACK:` in `scroll_after_jump_backward`
- `SEEK:` in `seek_to_current_line` (both timestamped and untimestamped branches)

Rebuild if any were added: `cargo build`

## Step 4: Check Timestamp Coverage at the Boundary

Use the highlighted line's text to find it in the database:

```bash
sqlite3 ~/utono/litdb/data/lit.db "
SELECT lm.id, lm.line_in_div, substr(lm.canonical_text, 1, 60),
       lt.start_time, lt.end_time
FROM line_mapping lm
LEFT JOIN line_timestamps lt ON lt.line_mapping_id = lm.id
WHERE lm.work_abbrev = '<WORK_ABBREV>'
  AND lm.canonical_text LIKE '%distinctive phrase%';
"
```

Then query neighbors to check for timestamp gaps:

```bash
sqlite3 ~/utono/litdb/data/lit.db "
SELECT lm.id, lm.line_in_div, substr(lm.canonical_text, 1, 60),
       lt.start_time, lt.end_time
FROM line_mapping lm
LEFT JOIN line_timestamps lt ON lt.line_mapping_id = lm.id
WHERE lm.work_abbrev = '<WORK_ABBREV>'
  AND lm.div1 = <DIV1> AND lm.div2 = <DIV2>
  AND lm.line_in_div BETWEEN <LINE - 3> AND <LINE + 3>
ORDER BY lm.line_in_div;
"
```

**What to check:**
- Does the target line (where `,`/`q` jumped to) have a `start_time`? If not, `seek_to_current_line` sets 86400s suppression
- Is there a timestamp gap between the current and target lines? Gaps can cause seeks to wrong positions

## Step 5: Trace the Keybind Code Path

### `,` (jump_to_prev_dialogue)
1. `prev_dialogue_line()` finds target → sets `current_line`
2. `update_highlight()` → `scroll_after_jump_backward()` — page turns if `current_line < page_top_line`
3. `seek_to_current_line()` — seeks MPV, sets suppression

### `q` (jump_to_next_dialogue)
1. `next_dialogue_line()` finds target → sets `current_line`
2. `update_highlight()` → `scroll_after_jump_forward()` — page turns if target not fully visible
3. `seek_to_current_line()` — seeks MPV, sets suppression

### `j`/`k` (scroll_viewport)
- Pure viewport scroll — does NOT move `current_line`, does NOT call `seek_to_current_line`
- If sync breaks after `j`/`k`, the issue is that the highlighted line scrolled off-screen but `current_line` didn't change, so when `CursorSync` fires it may page-turn unexpectedly

### `Shift+,` (page_backward_bottom)
1. Pops `page_history` → `set_page()` to previous page
2. Sets `current_line = last_fully_visible_line`
3. `seek_to_current_line()` — seeks MPV, sets suppression

## Common Root Causes (ordered by frequency)

1. **86400s suppression from untimestamped line:** `,`/`q` jumped to a line without timestamps → `seek_to_current_line` sets permanent suppression → `toggle_playback` doesn't clear it if the line still lacks a timestamp when play resumes
2. **Wrong dialogue line found:** `prev_dialogue_line`/`next_dialogue_line` skips over or lands on the wrong line due to `line_types` classification (speaker names, stage directions misclassified as dialogue or vice versa)
3. **Page turn to wrong position:** `scroll_after_jump_forward`/`scroll_after_jump_backward` computes wrong `page_turn_top`, landing the page at the wrong spot
4. **`j`/`k` viewport drift:** User scrolls viewport with `j`/`k` while playing, then `CursorSync` fires and triggers an unexpected page turn because `current_line` is now off-screen relative to the viewport
5. **`page_history` corruption:** Multiple rapid `,`/`q` presses push duplicate entries onto `page_history`, causing `Shift+,` to go to unexpected pages
6. **Seek preroll mismatch:** `SEEK_PREROLL` causes MPV to land at a different line's audio range, so the first `CursorSync` after suppression expires moves the cursor to an unexpected line

## Step 6: Fix and Verify

Make the minimal fix addressing the root cause. Rebuild with `cargo build`. The user will run `cargo run` to test. The navigation logging is permanent — do not remove it.
