# Startup Bottom Clip: Partial Line Visible on Launch

## Symptom

When launching the app and loading a work, the last line at the bottom of the page is partially visible (clipped). After pressing `q` (page forward) and navigating, the clipping disappears because `update_bottom_clip` runs correctly on subsequent pages.

## Root Cause

Two issues in `update_highlight_and_show` (navigation.rs):

1. **Wrong anchor line**: `update_bottom_clip` was called with `current_line` (the highlighted line) instead of `page_top_line` (the first visible line). On startup these differ because the cursor is placed on the first dialogue line while `page_top_line` may be one line above (to show the speaker). The clip height was calculated from the wrong starting position.

2. **Premature layout query**: `update_bottom_clip` was called in the same idle tick that made the scrolled window visible. GTK hadn't completed its layout pass yet, so `line_yrange` returned stale pixel heights. After page turns this isn't a problem because the widget is already visible and laid out.

## Fix

- Pass `page_top_line` (not `current_line`) to `update_bottom_clip`
- Call `scrolled_window.set_visible(true)` first, then defer `update_bottom_clip` to a nested `glib::idle_add_local_once` so GTK has one frame to lay out the now-visible widget before heights are queried

## Key Insight

`snap_scroll_to_line` (used by page turns) already passes the correct `page_top` and runs after the widget is visible, which is why clipping only appeared on initial launch.
