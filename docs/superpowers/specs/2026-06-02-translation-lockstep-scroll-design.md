# Translation lockstep two-column scroll — design

## Problem

In two-column mode, toggling translations ON interleaves a translation line
under every original line. The existing two-column renderer paginates against
the **buffer** (which now contains translation lines) while logical line
accounting uses `effective_line_count` (translations excluded). The mismatch
clips the top/bottom of columns and the right column overflows the visible
area, so the reader cannot see every line together with its translation.

Discrete page-turn pagination is the wrong model for reading interleaved
translations. The fix is a **continuous, lockstep two-column scroll** that is
active only while translations are visible.

## Goal

While translations are ON (and `column_count() == 2`):

- The **left column scrolls continuously** (line-by-line via j/k or wheel).
- The **right column "follows"**: each scroll tick it is re-pointed so its top
  line equals `(left column's last fully visible line) + 1`. The spread always
  reads contiguously: left bottom → right top with no gap, overlap, or clip.
- No clipping: the left column fills to the viewport bottom; the right column
  shows the contiguous continuation and clips only its own fitted end.

Toggling translations OFF returns to the normal e-reader two-column
pagination and restores the page to exactly what it was **before** translations
were toggled on.

Single-column works and e-reader two-column pagination (translations off) are
unchanged.

## Chosen approach

"Right follows left" via a scroll-sync callback. Reuse the existing two-view,
one-buffer architecture (`text_view` + `right_view` share `buffer`;
`column_split`/`visible_range` already compute column fits). Add a flag and a
single `value-changed` handler on the left adjustment. Rejected alternatives:
single composited column (user wants two side-by-side columns); custom drawn
widget (discards the working View/buffer/gutter/highlight stack).

## Section 1 — State & mode entry/exit

New `AppState` fields:

- `translation_scroll_active: bool` — true while continuous-scroll translation
  mode is on. Only ever set when `column_count() == 2`.
- `pre_translation_page: Option<(usize, usize)>` — `(current_line,
  page_top_line)` captured BEFORE `show_translations` mutates them, so exit
  restores the exact pre-toggle page.
- `right_scroll_syncing: Cell<bool>` — re-entrancy guard set while the sync
  callback writes the right adjustment.

Entry (`show_translations`):

1. Capture `pre_translation_page = Some((current_line, page_top_line))` at the
   top, before remapping for inserts.
2. After inserting translation lines and hiding signs (already implemented), if
   `column_count() == 2`: set `translation_scroll_active = true`, allow
   continuous scroll on the left `scrolled_window`, clear the left e-reader
   bottom clip, and run the initial scroll-sync once.

Exit (`hide_translations`, two-column branch):

1. Restore `current_line`/`page_top_line` from `pre_translation_page`
   (reverse-mapped through the strip), clear `translation_scroll_active` and
   `pre_translation_page`.
2. Re-tile via the normal e-reader two-column path (`set_page_instant` /
   `resnap_page`) so columns refill cleanly. Replaces the earlier
   partial-fill-on-hide patch.

## Section 2 — Scroll-sync callback (core mechanism)

Connect once, at startup, to the left `scrolled_window.vadjustment()`
`value-changed` signal. Early-return unless `translation_scroll_active`
(e-reader mode untouched). Each fire:

1. Read left adjustment value `v`.
2. Find the first buffer line whose top `y >= v` → left column effective top
   `lt`. Bounded scan from a cached `(v → lt)` hint; rescan only when `v` moves
   past the cached line bounds. O(lines on screen) per tick, not O(buffer).
3. Compute the left column's last fully visible line from `lt` with the
   existing left-fill logic (`visible_range`/`column_split`) against the left
   view height → `split`.
4. Set the right view scroll so the line at `split` is at the right column top:
   `right_adj.set_value(y_of(split))`, wrapped in the `right_scroll_syncing`
   guard.
5. Update the right bottom clip to the right column's fitted end.
6. Clear/zero the left bottom clip (left column fills to viewport bottom in
   scroll mode).

Invariant: `right_top == left_last_fully_visible + 1`, always.

Layout-not-ready guard: if view height ≤ 0, no-op this tick; the deferred idle
re-runs the initial sync once GTK lays out.

## Section 3 — Key handling & cursor

While `translation_scroll_active`, gate at the top of the relevant navigation
functions and branch into a small `scroll_mode` helper:

- **j / k / wheel:** adjust the left vadjustment by one line height (the
  current top line's `line_yrange` height), clamped to `[0, upper − page]`. The
  `value-changed` callback re-points the right column. The cursor highlight is
  not forcibly moved; it scrolls with the text.
- **q / comma (dialogue jumps):** move `current_line` to the next/prev dialogue
  as today, then scroll the left view so the cursor is visible: if the target
  is already in the left column's visible range, repaint highlight only; if it
  is below the left column, scroll the left view so the target sits in the left
  column. Cursor anchors to the left column when jumped to.
- **x / page keys:** scroll the left view by ~one viewport height, clamped.
- **Highlight:** `update_highlight_only` repaints the cursor-line tag without
  re-paginating. Cursor highlights in whichever column it currently falls.

## Section 4 — Edge cases & testing

Edge cases:

- Document start/end: left scroll clamps to `[0, upper − page]`. Near EOF the
  right column shows the tail with whitespace below; no wrap-around.
- Last spread: clamp `split`/`page_end` to `line_count`; no panic.
- Layout not ready (height ≤ 0): callback no-ops; initial sync re-runs on idle.
- Re-entrancy: `right_scroll_syncing` guard; left `value-changed` is the sole
  driver.
- Single-column works: flag never set; unchanged.
- Toggle off mid-scroll: exit restores `pre_translation_page` and re-tiles,
  independent of scroll position.
- MPV sync / search jumps in scroll mode: set `current_line`, then use the same
  "make cursor visible" path as q/comma.

Testing:

- Pure helper unit test: `left_last_visible_for_scroll(heights, scroll_v,
  view_h) -> split` and `right_top = split + 1` — contiguity invariant with
  synthetic line-height tables (mirrors `column_split_pure` tests in
  viewport.rs).
- Manual (user runs): toggle translations on a two-column play; scroll with
  j/k and wheel; confirm right always continues from left bottom + 1, no
  clipping; toggle off and confirm the page returns to the pre-toggle spread.
- Regression: `cargo test`. The 2 pre-existing `card_width_tests` failures are
  unrelated and known.

## Out of scope

- Continuous scroll for two-column mode when translations are OFF (stays
  discrete page-turn pagination).
- Single-column continuous translation scroll changes (single-column unchanged).
