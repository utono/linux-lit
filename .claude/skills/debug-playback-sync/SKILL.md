---
name: debug-playback-sync
description: Use when playback sync fails to turn the page, turns to the wrong line, turns at the wrong moment, or the highlight stops advancing during MPV audio playback in e-reader mode. Accepts a screenshot argument showing the page where sync failed.
argument-hint: <screenshot-path>
---

# Debug Playback Sync Page Turn

Diagnose why playback sync failed to turn the page or turned to the wrong line during MPV audio playback in e-reader mode.

## Core Principle

Most page-turn failures come down to one question: **which dialogue line should appear at the top of the next page?** The correct answer is always the dialogue line immediately following the last dialogue line on the current page. Start every investigation by identifying that expected next line, then work backward to find why the code didn't land there.

## Step 1: Read the Screenshot and Identify the Expected Next Page

Read the screenshot argument with the Read tool. Identify:
- The **highlighted line** (the line with the cursor/highlight background)
- The **last visible line** at the bottom of the page (may be clipped)
- Whether the highlighted line IS the last visible line (sync stalled at bottom)
- A **distinctive phrase** from the last visible line (for database lookup)

Then determine the **expected next page top**: the line of dialogue immediately after the last dialogue line visible on this page. For prose, every non-blank line is dialogue. For plays, dialogue excludes speaker names, stage directions, and act/scene headers.

## Step 2: Read the Log

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

**Key question from the log:** Does `SYNC_ADVANCE` show `current` reaching `last_vis` but never exceeding it? If so, the page-turn condition (`current_line > last_vis`) is never met — the cursor stalls at the bottom line.

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

The CursorSync handler in `src/main.rs` has two scroll paths. Understanding which one fires is critical:

**Path A — Paragraph changed** (`scroll_paragraph_to_top`):
- Fires when `current_paragraph_range().start` differs from the previous paragraph start
- In e-reader mode, only page-turns if `!is_line_on_screen(para_start)`
- **Failure mode:** paragraph start is still partially visible on the current page, so no page turn happens even though the current line is at the bottom

**Path B — Same paragraph** (`update_highlight_and_advance_page`):
- Fires when paragraph hasn't changed (within a long paragraph)
- Page-turns only if `current_line > last_fully_visible_line`
- **Failure mode:** for prose with long paragraphs, `current_line` equals but doesn't exceed `last_vis` — the highlight reaches the bottom line but the `>` check prevents the turn

**Which path should have fired?** If the log shows `CURSOR_SYNC: PARA_CHANGE` with `on_screen=true`, Path A fired but skipped the page turn. If `SYNC_ADVANCE` shows `current == last_vis` without ever showing `current > last_vis`, Path B's `>` condition was never met.

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
3. **Paragraph-change path skips turn:** `scroll_paragraph_to_top` checks `is_line_on_screen(para_start)` — if the old paragraph's start is still visible, it skips the page turn even though the cursor is at the bottom
4. **Suppression kills sync:** 86400s suppress from navigation/seek keybinds, never cleared by `toggle_playback`
5. **`pending_advance` cleared prematurely:** unconditional `None` on every `CursorSync` clears it before it can fire
6. **`last_fully_visible_line` miscounts:** includes a clipped bottom line as "fully visible", preventing the `>` condition
7. **`line_map` translation drops events:** `buffer_to_work` verification mismatch silently skips a `CursorSync`

## Step 7: Fix and Verify

Make the minimal fix addressing the root cause. The fix should ensure the next page starts with the correct line — the dialogue line immediately following the last dialogue line of the previous page, backed up to include its speaker name and blank separator via `page_turn_top`.

Rebuild with `cargo build`. The user will run `cargo run` to test.

Remove any temporary logging added in Step 3 after the fix is confirmed.
