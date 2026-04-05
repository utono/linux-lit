# Descender Clipping Fix

## Problem

In e-reader pagination mode, the last visible line on a page would have its descenders (g, p, y, j, etc.) clipped by the bottom clip overlay. During playback sync, the cursor would advance to a partially clipped line without triggering a page turn.

## Root Cause

The bottom clip overlay hides partial lines at the viewport bottom. Its height was calculated as `widget_height % line_height` — a modulo that assumes `scroll_to_iter` aligns the first line exactly to pixel 0. In practice, `scroll_to_iter` introduces sub-pixel offsets, so the modulo remainder didn't match the actual leftover space. A 7px clip couldn't cover a partial line that was 15-20px visible.

Additionally, `lines_per_page` counted visible lines by walking the buffer and calling `is_line_fully_visible`, which checked against the old clip height — creating a circular dependency where the clip was too small because the page showed too many lines, and the page showed too many lines because the clip was too small.

## Fix

Three components enforce a consistent 35-line page cap:

- **`update_bottom_clip`** sums actual `line_yrange` heights of 35 lines from `page_top`, then sets `clip = widget_height - total_height`. With 35 lines at 31px each (1085px) in a 1092px viewport, the clip is 7px. The height summation avoids the modulo alignment problem because `line_yrange` values are buffer-absolute, independent of scroll position.
- **`lines_per_page`** caps its loop at 35 iterations, preventing page turns from advancing past the clipped region.
- **`is_line_fully_visible`** uses a simple range check: `line >= page_top && line < page_top + 34`. The limit is 34 (one less than the clip cap) so that playback sync triggers a page turn as the last visible line plays, rather than waiting for the next off-screen line.

**Why not `bottom_margin`?** GTK's `set_bottom_margin()` adds padding inside the text view's content area. But the clip overlay sits on top of the scrolled window as a sibling overlay, so it covers the margin too. The margin approach doesn't work with the overlay-based clipping architecture.

**Why not `buffer_to_window_coords`?** Line position queries via `buffer_to_window_coords` return stale values in the `idle_add_local_once` callback where the clip is updated. The scroll from `scroll_to_iter` hasn't fully committed by the time the idle fires. Summing `line_yrange` heights avoids this because those are buffer-absolute values, independent of scroll position.

## Relevant code

- `src/input/navigation.rs`: `update_bottom_clip`, `lines_per_page`, `is_line_fully_visible`
- `src/app.rs`: `bottom_clip` widget setup (overlay on `scrolled_overlay`, CSS class `card-middle`)

## Reference

See `~/utono/text-viewer-gtk4-rs-example/docs/text-viewer.md` for GTK4 text view padding and Pango layout line documentation.
