# Translation overlay: center cursor on show, restore exact spread on hide

## Problem

Two defects in the `i` / Escape translation (interlinear) overlay, observed on a
two-column work (Henry VIII):

1. **No centering on show.** Pressing `i` with a line highlighted (e.g. "A
   Marshalsea shall hold you play these two months." at the bottom of the left
   column) opens the single-column translation overlay anchored to the top of
   the *chapter*, not to the cursor. The highlighted line is off-screen; the user
   has to scroll to find where they were.

2. **Non-canonical spread on hide.** Escaping out of the overlay does not return
   to the exact two-column spread that was showing when `i` was pressed. It lands
   one spread earlier (left column starting "place. At length they came to th'
   broomstaff…" instead of "and fight for bitten apples…"). The cursor line is
   still correct, but the page boundary differs — a different, non-canonical
   foliation.

## Root causes

### 1. Show anchors to `page_top_line`, never centers the cursor

`show_translations` (`src/app.rs`) defers its viewport anchor to an idle
callback that snaps the scroll to `page_top_line`'s *exact pixel top* (the
"idle snap to page_top" block, ~line 3868). That is correct for keeping a page
boundary aligned, but it ignores the cursor entirely. The translation overlay
scrolls continuously (cursor-following, vim `scrolloff`) — there is no reason to
pin it to the old two-column page top. The highlighted line should be centered
the way `center_cursor` centers it elsewhere (`page_size * 0.25` from the top).

### 2. Hide lets `snap_near_end_to_canonical` re-derive the page

`show_translations` saves the faithful pre-toggle spread:
`pre_translation_page = Some((old_current, old_top))` (pre-insert indices). The
two-column `hide_translations` branch restores exactly those
(`current_line = cur; page_top_line = top`) and then sets
`needs_layout_refresh = true`, routing the re-snap through the shared
`RESIZE_TICK` layout-refresh path.

That path unconditionally calls `snap_near_end_to_canonical(&mut s)`
(`src/app.rs` ~line 1861). For the restored spread, the saved cursor ("A
Marshalsea") is the **last** dialogue line of the left column. `snap_near_end…`
recomputes the page from the cursor via `page_top_containing(current_line)`;
when the cursor sits on the last line of a spread the page index resolves to the
*previous* boundary, so it overwrites the faithfully-restored `page_top_line`
with the earlier spread. Result: Image #3's wrong foliation.

The saved page is already the canonical spread the user quit on — it must not be
second-guessed.

## Design

### 1. Center the cursor on show

In `show_translations`, replace the deferred "snap to `page_top`'s exact pixel
top" anchor with a deferred **center-on-cursor** anchor that mirrors
`center_cursor`'s math:

- target scroll = `cursor_line_y - (page_size * 0.25)`, clamped to
  `[0, upper - page_size]`.
- still snap the result to a whole-line top (`snap_value_to_line_top`) so the
  continuously-scrolling overlay never lands between line boundaries (the
  existing top/bottom clip reasoning is unchanged).
- keep the existing `scrolloff_bottom_clip_widgets` call afterward so the
  partial bottom line is covered, and keep `page_top_line` consistent with the
  resulting scroll value (set it from `line_at_value` like the scrolloff path
  does) so overlay j/k start from the right place.

Because the overlay is single-column and cursor-following, anchoring to the
cursor (not the old two-column page top) is the correct model. No change to the
buffer-insert / tag / section-remap logic above it.

### 2. Restore the exact pre-toggle spread on hide

Add a one-shot `AppState` flag, `trust_restored_page: Rc<Cell<bool>>` (default
false). The two-column `hide_translations` branch sets it to `true` right after
restoring `(cur, top)` from `pre_translation_page`.

In the `RESIZE_TICK` layout-refresh path, guard the canonical re-derivation:

```rust
if s.trust_restored_page.replace(false) {
    // faithfully-restored translation-hide spread — do not second-guess it
} else {
    snap_near_end_to_canonical(&mut s);
}
```

`replace(false)` consumes the flag so it only suppresses the *one* re-snap
triggered by this hide; subsequent resizes behave normally. `snap_scroll_to_line`
still runs with the restored `page_top_line`, painting the exact pre-toggle
spread (Image #1).

This keeps the canonical-snap behavior for work-load and resize (where the
pre-layout `current_line - 1` guess genuinely needs correcting) and only opts
out for the translation-hide case, where the saved page is known-good.

## Scope / non-goals

- No change to the single-column (non-two-column) hide branch; it already
  anchors the cursor by saved screen-y and never routes through
  `snap_near_end_to_canonical`.
- No change to `snap_near_end_to_canonical`'s own logic — only whether the tick
  calls it after a translation hide.
- No DB / `.txt` / `LineMap` shape change; no `SNAPSHOT_VERSION` bump.
- Does not touch the deferred translation lockstep-scroll work.

## Verification

- `cargo build`, `cargo test --bins`.
- Visual ("renders on screen") criterion → ask the user to run the headless
  launch of the two-column work, press `i` with a bottom-of-column line
  highlighted (confirm it centers ~¼ down the overlay), then Escape (confirm the
  exact pre-toggle two-column spread returns — left column starting at the same
  line as before the toggle).
