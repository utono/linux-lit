# Prose over-tall paragraph: sub-line paging

_2026-06-29 (US Central). Design + plan. Execution: end-to-end, no review gates
(user-directed)._

## Problem (root cause — evidence-confirmed)

A prose paragraph is stored as ONE buffer line (e.g. Bleak House line 17 =
"On such an afternoon…", 2529 chars). Big paragraphs wrap **taller than the
viewport**: trace shows `top=17 h=1170` vs `usable=1067`. Pagination counts whole
buffer lines (`visible_range` walks `line_yrange`), so an over-tall paragraph at
`page_top` fits **zero** lines → `last_fully_visible_line == top`. `next_page_top`
then advances `new_top` to `top+1` = the **next paragraph**, dropping every
wrapped row of the current paragraph below the fold. `x` reads
paragraph-by-paragraph; the bottom ~100px (the un-shown rows) of a tall paragraph
appears on no page. Confirmed via `NEXT_PAGE_TOP`/`BOTTOM_CLIP_OVERTALL` traces:
`top==last_visible` for every over-tall paragraph; `total_h=1016/1170/1032` per
single buffer line.

The RENDER side already handles an over-tall paragraph: `update_bottom_clip`'s
`range.count==0` branch (scroll.rs ~745) reads the LIVE scroll value and clips at
a visual-row boundary via `display_rows`/`bottom_clip_height` — comment:
"Paging forward continues the paragraph from the next row." But page-forward never
continues by row; it jumps the line.

## Approach (user-approved): sub-line scroll within the paragraph

When `page_top`'s paragraph is taller than the viewport, advance the SCROLL by
~one `usable_height` WITHIN the same buffer line (a pixel offset, snapped to a
visual-row top), and only move `page_top_line` to the next paragraph once the
offset exhausts the paragraph. `y` reverses it. The render/clip path already
follows the scroll value, so it needs no change.

### State change

- `AppState.page_top_offset: i32` (NEW) — pixels scrolled PAST `page_top_line`'s
  pixel top. 0 for the normal (line-aligned) case. The viewport top is
  `line_yrange(page_top_line).y + page_top_offset`.
- `AppState.page_back_stack: Vec<usize>` → `Vec<(usize, i32)>` — each entry is
  `(page_top_line, page_top_offset)` so `y` round-trips a mid-paragraph position
  exactly. All ~32 push sites push `(state.page_top_line, state.page_top_offset)`;
  the 2 pop sites destructure `(line, offset)`.

### Scroll plumbing

- `snap_scroll_to_line(state, line)` → `snap_scroll_to_line(state, line, offset)`
  (3 call sites inside `set_page`/`set_page_instant`): `adj.set_value(y + offset)`,
  clamped. offset 0 = current behavior.
- `set_page_instant(state, new_top)` stays (passes offset 0). ADD
  `set_page_instant_offset(state, new_top, offset)` for the restore path — avoids
  editing all 21 `set_page_instant` callers.
- `set_page(state, new_top, dir)` keeps setting offset 0 on a normal turn (a
  whole-line turn resets the offset). The over-tall WITHIN-paragraph advance does
  NOT go through `set_page`'s line path — see below.

### Over-tall paging branch (the core)

In `page_forward`, BEFORE the existing `next_page_top` path, add a single-column
over-tall check:

1. Compute `usable_height` (same as `last_fully_visible_line`:
   `widget_height - descender_guard_px(top) - BASE_BOTTOM_MARGIN`).
2. `para_h = line_yrange(page_top_line).h` (full wrapped paragraph height).
3. `cur_off = page_top_offset`. If `para_h - cur_off > usable_height` the
   paragraph still has rows below the fold → advance WITHIN it:
   - `raw = y(page_top_line) + cur_off + usable_height` (next viewport down).
   - Snap `raw` DOWN to a visual-row top via a new main-card
     `snap_value_to_display_row(state, raw)` (mirrors the overlay's
     `snap_value_to_line` using `display_rows` — sanctioned for the main card by
     clip-prevention.md).
   - `new_off = snapped - y(page_top_line)`.
   - push `(page_top_line, cur_off)` to the back-stack; set
     `page_top_offset = new_off`; `adj.set_value(snapped)`; recompute bottom clip;
     update cursor/dim; return. `page_top_line` UNCHANGED.
4. Else (paragraph exhausted, or normal-height line): fall through to the existing
   line-advance path, which sets `page_top_offset = 0` and advances
   `page_top_line` normally.

`page_backward` mirror:
- Pop `(line, off)`. If `line == page_top_line` and `off < page_top_offset` (we're
  stepping back WITHIN the same paragraph), restore via
  `set_page_instant_offset(state, line, off)` (no line change). Else normal
  `set_page(line, Backward)` with the popped offset threaded through.
- Empty-stack fallback (`prev_page_top`): offset 0 (a recomputed previous page is
  line-aligned; mid-paragraph history only exists via the stack).

### What does NOT change

- Render/clip: `update_bottom_clip` over-tall branch already reads `scroll_val`.
- Playback sync: the over-tall guard `current_line > last_vis` is already false
  when the cursor is the same buffer line as `page_top` (over-tall →
  `last_vis == page_top`), so sync does not spuriously page-turn. Sync keeps
  offset 0 (paragraph-start scroll); sub-line offset is manual-paging only.
- Jumps (gg/G/scene/search/bookmarks): all set `page_top_offset = 0` (via the
  normal `set_page_instant`, which we leave at offset 0). They already clear/push
  the stack; the push now records offset 0.
- Dimming (`clear_old_page_dim`): per-buffer-line, unaffected.
- Two-column plays: the over-tall branch is single-column-only
  (`column_count() == 1`); plays are unaffected.

## Tests (TDD)

The existing `test_page_forward_prose_bleak_house` models a fixed 30-line page and
never sees an over-tall single buffer line — it is blind to this bug. Add a pure
headless test that models pixel heights:

- `test_prose_overtall_paragraph_no_skipped_rows` — model a prose buffer where one
  "line" is an over-tall paragraph (height > usable). Simulate the over-tall
  forward branch (offset stepping) and assert: every visual row of the paragraph
  is covered by some page (forward offsets tile `[0, para_h)` with step
  `usable_height`, last step reaches `para_h`), and the next page_top is the next
  paragraph only AFTER the offset exhausts the current one.
- `test_prose_overtall_x_y_roundtrip` — forward through an over-tall paragraph then
  back; assert `(page_top_line, page_top_offset)` returns to each prior value
  exactly (the `Vec<(usize,i32)>` stack round-trips).

Extract the offset-stepping decision into a PURE helper so it's unit-testable
without GTK:

```
/// Given the current (offset, paragraph height, usable height), return the next
/// forward step: Some(new_offset) to advance within the paragraph, or None to
/// advance to the next buffer line. Pure.
fn overtall_next_offset(cur_off: i32, para_h: i32, usable: i32) -> Option<i32>
```
`Some((cur_off + usable))` when `para_h - cur_off > usable`, else `None`. (The GTK
caller snaps `cur_off + usable` to a real row top; the pure test uses the raw
step, which is the upper bound — snapping only reduces it, still > cur_off, still
< para_h, so coverage holds.)

Visual acceptance (user, no-cargo-run rule): on Bleak House, page through the
"On such an afternoon…" paragraph with `x` and confirm NO text is skipped between
pages (the "…so overthrows the brain and breaks the heart… come here!" tail now
appears), and `y` returns to the exact prior scroll position.

## Files

- `src/app/mod.rs` — add `page_top_offset: i32` field (+ init 0); change
  `page_back_stack` type to `Vec<(usize, i32)>`.
- `src/input/scroll.rs` — `snap_scroll_to_line` gains `offset`;
  `set_page`/`set_page_instant` pass offset; add `set_page_instant_offset`; add
  `snap_value_to_display_row` (main-card per-visual-row snap); `set_page` resets
  `page_top_offset = 0` on a line turn.
- `src/input/viewport.rs` — `overtall_next_offset` pure helper (+ tests).
- `src/input/navigation.rs` — over-tall branch in `page_forward`; mirror in
  `page_backward`/`page_backward_dialogue`; all `page_back_stack.push` →
  `push((line, offset))`; pops destructure.
- `src/input/highlight.rs`, `src/input/search.rs`, `src/input/nav_test.rs`,
  `src/main.rs` — any `page_back_stack.push(x)` → `push((x, 0))` (these push from
  non-over-tall contexts, offset 0).

## Removal

Delete the TEMP `NEXT_PAGE_TOP` trace (viewport.rs) and the TEMP normal-path
`PAGE_FWD` trace (navigation.rs) added during diagnosis, once verified.

## Doc update (after fix verified — user-requested)

Update `docs/troubleshooting/page-turning-mechanics.md`: add a "Prose over-tall
paragraph (sub-line paging)" subsection documenting `page_top_offset`, the
`Vec<(usize,i32)>` stack, the over-tall forward/back branch, and that render/clip
already follow the scroll. Note the test that guards it.
