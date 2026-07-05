---
name: debug-playback-sync
description: Use when playback sync fails to turn the page, turns to the wrong line, turns at the wrong moment, or the highlight stops advancing during MPV audio playback in e-reader mode. Accepts a screenshot argument showing the page where sync failed.
argument-hint: <screenshot-path>
---

# Debug Playback Sync Page Turn

Diagnose why playback sync failed to turn the page or turned to the wrong line during MPV audio playback in e-reader mode.

## Core Principle

Most page-turn failures come down to one question: **which dialogue line should appear at the top of the next page?** The correct answer is always the dialogue line immediately following the last dialogue line on the current page. Start every investigation by identifying that expected next line, then work backward to find why the code didn't land there.

## Diagnostic Priority

The log almost always contains enough information to diagnose the issue. Follow this order:
1. **Screenshot** — identify the stalled page and expected next page
2. **Log** — read it carefully; the log line patterns below usually pinpoint the root cause directly
3. **Code trace** — only if the log doesn't explain the failure
4. **Database queries** — only if timestamps are suspect (missing start/end times at the boundary)

Skip steps 3-4 if the log already reveals the root cause. Most bugs are visible from the log alone.

## Step 1: Read the Screenshot and Identify the Expected Next Page

Read the screenshot argument with the Read tool. Identify:
- The **highlighted line** (the line with the cursor/highlight background)
- The **last visible line** at the bottom of the page (may be clipped)
- Whether the highlighted line IS the last visible line (sync stalled at bottom)
- A **distinctive phrase** from the last visible line (for database lookup)

Then determine the **expected next page top**: the line of dialogue immediately after the last dialogue line visible on this page. For prose, every non-blank line is dialogue. For plays, dialogue excludes speaker names, stage directions, and act/scene headers.

## Step 2: Read the Log

Critical sync and page-turn log lines (`CURSOR_SYNC:`, `SYNC_ADVANCE:`, `SYNC_PAGE_TURN:`, `PAGE_TURN:`) are always written regardless of debug mode. For additional detail (e.g. `CURSOR_LINE:`, `SEEK:`), tell the user to press **Ctrl+d** to enable debug logging before reproducing.

```bash
cat ~/utono/linux-lit/linux-lit-dev.log
```

Look for:
- `CURSOR_SYNC:` — shows `line_idx`, `buffer_line`, `current`, `page_top` for every sync event that reaches the handler. If absent between `playing` and `paused`, events are being suppressed or filtered
- `CURSOR_SYNC: SUPPRESSED` — events killed by `suppress_sync_until`, shows remaining seconds
- `CURSOR_SYNC: PARA_CHANGE` — paragraph transitions, shows whether `para_start` was on-screen
- `SYNC_ADVANCE:` — the page-turn decision point, shows `current`, `last_vis`, `page_top`
- `PAGE_TURN:` — confirms a page turn happened, shows `new_top` and `old_top`
- `PARA_SCROLL:` — paragraph-path page turn
- `MPV playback: playing/paused` — confirm playback was active during the failure
- `BOTTOM_CLIP:` — extract `page_top` value for boundary queries

**Quick diagnosis from log patterns:**
- `PARA_CHANGE on_screen=true` followed by `SYNC_ADVANCE` → `PAGE_TURN`: working correctly (unified flow catches it)
- `PARA_CHANGE on_screen=true` with NO subsequent `SYNC_ADVANCE`: `update_highlight_and_advance_page` is not being called after `scroll_paragraph_to_top` — check the unified flow in `src/main.rs`
- `SYNC_ADVANCE` shows `current == last_vis` repeatedly but never `current > last_vis`: timestamp gap — no CursorSync event lands on the line past `last_vis`
- `SUPPRESSED` entries spanning the entire `playing`→`paused` window: suppression duration too long, check Step 6
- `CURSOR_SYNC:` entries present but no `SYNC_ADVANCE` at all: `current_line` never changes (buffer_line equals current on every event)

## Step 3: Enable Sync Logging (if not present)

Check `src/main.rs` in the `MpvEvent::CursorSync` handler for `CURSOR_SYNC:` log lines. If missing, add them. There should be logging:
- At entry (after suppress check): line_idx, buffer_line, current_line, page_top_line
- On suppression: line_idx, remaining seconds
- On paragraph change: para_start, on_screen status
- In `update_highlight_and_advance_page` (navigation.rs): current, last_vis, page_top

Rebuild: `cargo build`

## Step 4: Identify the Page Boundary Lines

Use the distinctive phrase from the screenshot to find the line in the database.

**Database schema reference:**
- Table: `line_mapping` (columns: `id`, `work_abbrev`, `div1`, `div2`, `line_in_div`, `canonical_text`, `speaker`, `tln`)
- Table: `line_timestamps` (columns: `line_mapping_id`, `media_id`, `start_time`, `end_time`)
- Table: `media_files` (columns: `id`, `work_abbrev`, `path`)

Search by text to find the line:

```bash
sqlite3 ~/utono/litdb/data/lit.db "
SELECT id, div1, div2, line_in_div, substr(canonical_text, 1, 60)
FROM line_mapping
WHERE work_abbrev = '<WORK_ABBREV>'
  AND canonical_text LIKE '%distinctive phrase%';
"
```

Then query that line and its neighbors with timestamps:

```bash
sqlite3 ~/utono/litdb/data/lit.db "
SELECT lm.id, lm.line_in_div, substr(lm.canonical_text, 1, 60),
       lt.start_time, lt.end_time
FROM line_mapping lm
LEFT JOIN line_timestamps lt ON lt.line_mapping_id = lm.id
WHERE lm.work_abbrev = '<WORK_ABBREV>'
  AND lm.div1 = <DIV1> AND lm.div2 = <DIV2>
  AND lm.line_in_div BETWEEN <LINE - 2> AND <LINE + 5>
ORDER BY lm.line_in_div;
"
```

Replace `<WORK_ABBREV>` with the work abbreviation from the log (e.g. `BH` for Bleak House). Replace `<DIV1>`, `<DIV2>`, `<LINE>` from the search results.

**What to check at the boundary:**
- Does the last visible dialogue line have both `start_time` and `end_time`?
- Does the NEXT dialogue line after the page boundary have a `start_time`?
- If yes to both: `CursorSync` should handle the transition directly
- If the next line lacks a timestamp: `pending_advance` fires when the current line's `end_time` is reached

**Identify the expected next page top:** The next dialogue line's `line_in_div` tells you where the page should land. Use `page_turn_top` logic: from that dialogue line, walk backward over blank lines and speaker names to find the actual top. The page top should be the blank separator or speaker name above the next dialogue line, not the dialogue line itself.

## Step 5: Trace the Page-Turn Decision

The CursorSync handler in `src/main.rs` uses a unified flow:

1. **Paragraph-change check:** If `current_paragraph_range().start` differs from previous, `scroll_paragraph_to_top` runs first. In e-reader mode, this only page-turns if `!is_line_on_screen(para_start)`. If para_start is on-screen, it does nothing.

2. **Advance check (always runs):** `update_highlight_and_advance_page` runs unconditionally after the paragraph check. Page-turns if `current_line > last_fully_visible_line`.

This means every CursorSync event that changes `current_line` will check the advance condition, regardless of whether a paragraph change occurred.

**Log pattern for paragraph-change page turns:**
- `CURSOR_SYNC: PARA_CHANGE para_start=N on_screen=false` → `PARA_SCROLL` (paragraph path turned the page)
- `CURSOR_SYNC: PARA_CHANGE para_start=N on_screen=true` → `SYNC_ADVANCE` → `PAGE_TURN` (paragraph path skipped, advance path caught it)

**Log pattern for same-paragraph page turns:**
- `CURSOR_SYNC:` (no PARA_CHANGE) → `SYNC_ADVANCE` → `PAGE_TURN`

**Failure mode to watch for:** `SYNC_ADVANCE` shows `current == last_vis` without ever exceeding it. The `>` condition means the cursor must advance one line PAST the last visible line to trigger the turn. If timestamps are spaced such that sync never lands on that line, the page stalls.

Also check the `TimePos` handler for `pending_advance` logic — this fires for untimestamped lines when the previous line's audio ends. Same `update_highlight_and_advance_page` is called, same `>` condition applies.

## Step 6: Trace the Suppression Chain (if sync events are missing)

If the log shows `CURSOR_SYNC: SUPPRESSED` or no `CURSOR_SYNC` entries at all during playback, trace `suppress_sync_until`.

Key suppression sources and durations:
- `seek_to_current_line` — 500ms (timestamped) or **86400s** (untimestamped)
- `toggle_playback` — 500ms (timestamped) or clears suppression (untimestamped)
- `display_work` — 5s (loading guard)
- Seek keybinds (o/e/O/E, Left/Right) — 86400s (permanent until cleared)
- Search navigation — 500ms
- `cursor_to_page_bottom` (Q key) — calls `seek_to_current_line`, so 500ms or 86400s

**Common failure pattern:** User navigates with `q`/`,` while paused, lands on untimestamped line (86400s suppress), then presses Tab to play. If `toggle_playback` doesn't clear the suppression (because current line also lacks a timestamp), sync stays dead.

## Common Root Causes (ordered by frequency)

1. **Wrong next-page target:** `page_turn_top` doesn't back up far enough (misses speaker name or blank separator above the next dialogue line), or backs up too far (shows end of previous paragraph)
2. **Page turn never triggers:** `current_line > last_vis` uses strict `>`, so if the cursor reaches but equals `last_vis`, no turn happens. The highlight stalls at the bottom line
3. **Suppression kills sync:** 86400s suppress from navigation/seek keybinds, never cleared by `toggle_playback`
4. **`pending_advance` cleared prematurely:** unconditional `None` on every `CursorSync` clears it before it can fire
5. **`last_fully_visible_line` miscounts:** includes a clipped bottom line as "fully visible", preventing the `>` condition
6. **`line_map` translation drops events:** `buffer_to_work` verification mismatch silently skips a `CursorSync`

### Previously Fixed

- **Table-mode boundary read from the live engine (fixed 2026-07-04):** With a pinned `play_pages` table active, the render clips at the STORED spread end, but `last_raw_visible_line` / `is_line_fully_visible` asked the live `column_split` — which can disagree with the stored table at a matching fingerprint (the fp covers font metrics + window size, not text_view height or engine changes; a table generated in a cage run is ~1 line/column short of the real display). Sync and `j` walked the cursor past the rendered page end without turning (R2-Arkangel: highlight stalled on the page's last line, turn fired ~10s late when the cursor finally passed the LIVE boundary). Fix: `page_table::table_end_for_top` — the 2-col branches of `last_raw_visible_line`, `last_fully_visible_line`, and `is_line_fully_visible` read the active table's spread end (live fallback when `top` isn't a canonical stored top), and `scroll_after_jump_forward` lands on `table_top_for(current)` like sync does. Diagnostic signature: boundary `CURSOR_SYNC` shows `current` == rendered last line, no `SYNC_PAGE_TURN` ever logs, and `PAGES: table hit` appeared at load. Same-day follow-ups: the paragraph turn (`SYNC_PARA_TURN`), the scene snap, and the `,`/`k` backward turn now land via `table_top_for` (they landed on the live grid); and `page_table::resnap_to_table` runs after the deferred table load/gen and resize revalidation — a startup resume could snap to a stored table for ANOTHER fingerprint that gets dropped at settled geometry (window 1910x1190 vs stored 1920x1200), leaving an off-grid `page_top` whose first sync turn re-anchored with a visible mid-page cursor teleport (`SYNC_PAGE_TURN … old_top=492 new_top=496`, jump of only 4 lines).
- **Premature page turn from trimmed last_vis (fixed 2026-05):** `update_highlight_and_advance_page` used `last_fully_visible_line` (trims trailing speakers/blanks for pagination). Sync saw `last_vis` as ~10 lines before the actual bottom. Fix: switched to `last_raw_visible_line`.
- **Stage direction cursor landing (fixed 2026-05):** `SetTimestamps` included all lines regardless of `is_dialogue`. Stage directions with timestamps caused `CursorSync` to highlight them. Fix: filter `ts_data` to dialogue-only at all three build sites.
- **Paragraph-change path skipped advance check (fixed 2026-04):** Before the fix, the CursorSync handler had an if/else split — paragraph changes called `update_highlight_only` + `scroll_paragraph_to_top`, while same-paragraph called `update_highlight_and_advance_page`. If `para_start` was on-screen, neither path would page-turn. Fix: `update_highlight_and_advance_page` now runs unconditionally after both paths.

## Step 7: Fix and Verify

Make the minimal fix addressing the root cause. The fix should ensure the next page starts with the correct line — the dialogue line immediately following the last dialogue line of the previous page, backed up to include its speaker name and blank separator via `page_turn_top`.

Rebuild with `cargo build`.

### Tell the user how to reproduce

After rebuilding, give the user concrete reproduction steps:

1. **Which work to open** — name the play/book from the screenshot
2. **Where to navigate** — identify the specific line of dialogue that was on screen before the failure, using the log's buffer_line and the database text. Tell the user to navigate to that line (e.g. "press q until you reach 'I am come to survey the Tower this day'")
3. **What to do** — start playback with Tab, or press q/comma to advance, depending on whether the bug was sync-triggered or navigation-triggered
4. **What to watch for** — describe the expected correct behavior (e.g. "when sync crosses into Scene 3, the page should snap so 'Scene 3' and its separator are at the top")

The user runs `cargo run` to test. Remove any temporary logging added in Step 3 after the fix is confirmed.
