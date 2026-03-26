# E-Reader Pagination Design

**Date:** 2026-03-26
**Status:** Implemented

## Problem

The original navigation used pixel-based `iter_location()` coordinates to detect when the cursor went off-screen, then computed scroll targets in pixel space. This caused:

- **Oscillation:** lines at exact page boundaries triggered alternating page-up/page-down
- **Inconsistent positioning:** wrapped paragraphs (2-3 visual lines) had unpredictable heights, making pixel math fragile
- **Animation race conditions:** crossfade opacity callbacks from rapid key presses could hide the highlight
- **No reading continuity:** page turns showed entirely new content with no overlap

## Research

Studied pagination in three open-source e-readers:

- **Foliate** (GTK4, GJS): Uses WebKitGTK WebView with CSS multi-column layout. Pages are CSS columns, turns change `scrollLeft`. Not applicable — requires a browser engine.
- **KOReader** (Lua, C++): CREngine renders text into a continuous layout, then `splitToPages()` breaks it into discrete pages by accumulating line heights. For scroll mode, it subtracts N lines of overlap from the scroll distance. The overlap region is dimmed to show previously-seen content.
- **Readest** (TypeScript, Tauri): Wraps foliate-js, same CSS column approach as Foliate.

## Solution: Line-Index-Based Pagination

Adopted KOReader's scroll-mode approach, adapted for GtkTextView:

### Core Concept

Track the **line index** at the top of the current page (`page_top_line`), not pixel coordinates. The page is defined as a range of line indices: `page_top_line` to `page_top_line + lines_per_page`.

### How It Works

1. **`page_top_line: usize`** in AppState tracks which line is at the top of the current view
2. **`lines_per_page()`** estimates how many lines fit by sampling line heights near the current position
3. **j/k movement:** cursor moves within `[page_top_line, page_top_line + lines_per_page)`. No scrolling until cursor exits this range.
4. **Page turn trigger:** when cursor goes above `page_top_line` or at/past `page_top_line + lines_per_page`, a page turn happens
5. **Backward turn (k/comma):** new `page_top_line = cursor - (lines_per_page - 1)`, so cursor appears at the bottom of the new page
6. **Forward turn (j/q):** new `page_top_line = cursor - overlap`, so cursor appears near the top with overlap lines from the old page visible above it
7. **Scroll target:** `scroll_value_for_line(page_top_line)` uses `iter_location()` once to get the y-coordinate, then sets `vadjustment` to that value

### Page Overlap

`PAGE_OVERLAP = 1` line. On forward page turn, the last line of the old page becomes the first line of the new page. On backward turn, the first line of the old page becomes the last line of the new page. This provides reading continuity.

### Crossfade Animation

Page turns use a crossfade: fade out (~80ms), snap scroll position, fade in (~80ms). A generation counter prevents stale animation callbacks from stomping on the current state. When the cursor moves to a line already on the current page, any in-flight animation is cancelled and opacity is restored immediately.

### Why This is Better

- **No pixel math for page boundaries.** Page boundaries are line indices, not pixel offsets. No oscillation possible.
- **No race conditions.** The page state is a simple integer (`page_top_line`). Whether the cursor is on the page is a simple range check on line indices.
- **Predictable overlap.** Exactly 1 line of overlap, always. Not dependent on paragraph heights.
- **`iter_location()` called once per page turn** (to get the scroll target), not per cursor move. Layout timing issues are minimized.

## Keybindings

- **j/k** — move cursor within page, page turn when reaching edge
- **comma (,)** — previous dialogue line, page turn if off-page
- **q** — next dialogue line, page turn if off-page
- **Ctrl+d/f** — page forward (one page minus overlap)
- **Ctrl+u/b** — page backward (one page minus overlap)
- **gg** — jump to first line (instant)
- **G** — jump to last line (instant)

## Files

- `src/input/navigation.rs` — all pagination logic
- `src/app.rs` — `AppState.page_top_line` field
