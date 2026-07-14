# E-Reader Pagination Design Spec (linux-lit)

**Date**: 2026-04-02
**Status**: Approved
**Source**: Brainstorming session adapting macOS-lit e-reader pagination spec for GTK4
**Depends on**: Existing e-reader mode (NavigationMode::EReader), scroll_to_iter-based page turns

## Overview

Improve the existing scroll-based e-reader pagination in linux-lit. The full document remains in the GTK4 sourceview5 buffer (no page-only content). Navigation uses deterministic line-count-based page turn thresholds for user input, and pixel-based checks for audio sync. Highlight performance is optimized by restricting dim tag operations to the visible range.

## Page Model

- `lines_per_page` counts buffer lines whose full `line_yrange` fits in the viewport, starting from `page_top_line`. Handles wrapped lines correctly since each buffer line may span multiple visual rows.
- `page_top_line` tracks the first content line of the current page. Updated on every page turn.
- Page turns use `scroll_to_iter` on the end of `page_top_line - 2` with `yalign=0.0`. This places the end of that line at the viewport top, so `page_top_line - 1` acts as an overlap/padding line and `page_top_line` is the first content line. This approach prevents top-line clipping because the target content line is always fully below the viewport top edge.
- Minimum 1 line per page.
- Recalculated on window resize, font change, or document load. The page adjusts to keep `current_line` visible.

## Navigation Behavior (Deterministic, Line-Count-Based)

User navigation (`q`, `,`, `j`, `k`, `Ctrl+d`, etc.) uses line-count thresholds to decide page turns. No pixel-based visibility checks for user input.

### Dialogue/paragraph jump forward (`q`)

1. Move cursor to next dialogue line (or next paragraph in prose mode)
2. If `current_line >= page_top_line + lines_per_page - 2`: page turn, cursor line becomes `page_top_line` of the new page
3. Otherwise: cursor moves within the page, no page turn

### Dialogue/paragraph jump backward (`,`)

1. Move cursor to previous dialogue line (or previous paragraph in prose mode)
2. If cursor is on the top visible line (the line above it is off-screen, or cursor is at line 0): page turn backward, cursor near bottom of the new page
3. Otherwise: cursor moves within the page, no page turn

### Line-by-line (`j`/`k`)

- Same threshold as `q`/`,`: page turn when cursor crosses `page_top_line + lines_per_page - 2` (forward) or reaches the top visible line (backward)

### Half-page (`Ctrl+d` / `Ctrl+u`)

- Advance `current_line` by `lines_per_page / 2` (clamped to document bounds)
- Page turn if the new cursor position is outside the current page

### Full-page (`Ctrl+f` / `Ctrl+b`)

- Advance `current_line` by `lines_per_page` (clamped to document bounds)
- Always triggers a page turn

### Jump (`gg` / `G`)

- Instant jump to first/last line
- Page turn with no transition

### Search (`n` / `N`)

- Move cursor to line containing match
- Page turn if match is on a different page

## Audio Sync Behavior (Independent, Pixel-Based)

Audio playback sync (CursorSync) uses pixel-based checks because it must handle arbitrary cursor positions from MPV time events. The page state is independent during playback — the page only turns when the cursor actually goes off-screen.

### CursorSync line-by-line

- Uses `is_line_on_screen` (checks `line_yrange` against `vadjustment` value and page_size, no padding requirement)
- Only page-turns when the cursor is off-screen or on the last/second-to-last visible line (`should_page_turn_forward`)
- No page turn for mid-page cursor movements

### Paragraph transition during playback

- `scroll_paragraph_to_top` only fires when the paragraph's first line is off-screen
- When it fires: `set_page(para_start)` puts the paragraph at the top with one line of overlap

## Performance Optimization

### Visible-range dim tags

Current behavior: `update_highlight` applies dim tags to the entire buffer (36K+ lines) on every cursor movement, taking ~786ms.

New behavior: only apply/remove dim tags in a range around the visible page:
- Range: `page_top_line.saturating_sub(5)` to `page_top_line + lines_per_page + 5` (small margin for scroll overshoot)
- On page turn: remove dim tags from old visible range, apply to new visible range
- The dim tag for lines outside this range is neither applied nor removed — they retain whatever state they had. Since they're off-screen, this is invisible.
- When dimming is toggled off (`Alt+d`), remove dim tags from the full buffer once, then only manage the cursor_line_tag in the visible range.

Expected improvement: highlight time drops from O(document_size) to O(page_size) — roughly 30-40 lines instead of 36K.

## Scroll Positioning

### `scroll_value_for_line` (used by manual adj.set_value callers)

Uses `line_yrange` which includes `pixels_above_lines` and `pixels_below_lines` in the y coordinate. Returns `line_yrange.y` as the scroll target.

### `set_page` (page turn)

Uses `scroll_to_iter` on the end of `page_top_line - 2` at `yalign=0.0`. This places content with one overlap line and prevents top-line clipping. See `docs/gtk4-textview-pagination.md` for research details.

### `set_page_instant` (gg/G/restore)

Same as `set_page` — uses `scroll_to_iter` for consistent positioning.

## Transitions

Instant only — no animation. The page content changes immediately on page turn. Crossfade/slide transitions can be added later without changing the page model (capture screenshot of current viewport, replace scroll position, animate overlay fade).

## Files Changed

- **Modify**: `src/input/navigation.rs` — replace pixel-based page turn checks in `scroll_after_jump_forward`/`scroll_after_jump_backward` with line-count thresholds; optimize `update_highlight` to visible range only; clean up unused functions (`crossfade_to`, `ensure_visible_no_highlight`, `scroll_value_for_line`)
- **Modify**: `src/app.rs` — no structural changes expected; possible minor adjustments to initial page setup
- **No new files**

## Edge Cases

- Window too small to show even one line: `lines_per_page` returns minimum 1
- Last page has fewer lines than `lines_per_page` — show what's available, top-aligned
- Document shorter than one page: single page, no page turns needed
- Font size change (via `[`/`]` keys): recalculate `lines_per_page`, adjust page to keep cursor visible
- Window resize: same recalculation
- Rapid `q`/`,` at page boundary: each press triggers at most one page turn since the threshold is re-evaluated after each cursor move
- Audio sync rapid updates: `is_line_on_screen` prevents unnecessary page turns; `should_page_turn_forward` limits forward turns to last/second-to-last line
- `Alt+d` dim toggle: when toggling off, clear dim tags from full buffer once; subsequent highlight updates only manage cursor_line_tag in visible range

## What Does Not Change

- Full-text search via `GtkSourceSearchContext` — operates on the full buffer
- Vocab highlighting — tags applied to the full buffer, persist across pages
- Gutter renderers (timestamp, chunk bar) — reference buffer line indices directly
- Line index system — `current_line`, dialogue jumps, timestamps all use buffer-global indices
- AB loop / chunk navigation
- Visual selection mode
