# Scene Scroll on Playback Sync

When playback sync is active and the cursor reaches the first dialogue line
of a new scene, scroll the viewport so the entire header block is at the top.

## Behavior

- **Trigger:** The synced line's `(div1, div2)` differs from the previous
  synced dialogue line's `(div1, div2)` — i.e., the cursor crossed a scene
  boundary
- **Action:** Compute the header-block top via `back_up_for_speaker` for the
  current line (the first dialogue of the new scene), then snap the viewport
  to that position with `set_page_instant`
- **Unconditional:** Always repositions viewport on scene change, even if the
  header block is already visible on the current page
- **Plays only:** Skip for prose works (`is_prose_work(work_type)` returns
  true). Scene-scroll only fires for plays/poems with act/scene structure

## State

Add `current_sync_scene: Option<(i64, i64)>` to `AppState`. Tracks the
`(div1, div2)` of the most recently synced dialogue line. Set to `None` on
work load and on sync disable.

## Integration point

In the CursorSync handler in `main.rs`, after `s.current_line = buffer_line`
and before the existing paragraph-transition detection:

1. Look up the new line's `(div1, div2)` from `work.lines[work_line_index]`
2. Compare against `s.current_sync_scene`
3. If changed and work is not prose:
   - Compute `top = back_up_for_speaker(&s.buffer, buffer_line)`
   - Call `set_page_instant(state, top)` to snap viewport
   - Update `s.current_sync_scene = Some((div1, div2))`
4. If same scene or prose: just update `s.current_sync_scene`

The subsequent `update_highlight_and_advance_page` call still runs — it will
see the cursor is on the newly positioned page and apply the highlight
without triggering another page turn.

## Why set_page_instant (not set_page)

This is a deliberate viewport reposition to show scene context, not a natural
page turn from reading flow. An instant snap avoids a crossfade/slide
animation that would feel jarring. It also avoids the `page_turn_lock`
contention that animated turns create.

## page_back_stack

No stack interaction. This is system-driven (MPV sync), not user navigation.
Matches the existing convention for `scroll_paragraph_to_top` and
`update_highlight_and_advance_page` under `MpvSync`.

## Edge cases

- **First CursorSync after work load:** `current_sync_scene` is `None`, so
  the first scene is recorded but doesn't trigger a scroll (no previous scene
  to compare against)
- **line_map active (text_file mode):** Use `work_line_for_buffer(buffer_line)`
  to get the work-line index for div1/div2 lookup
- **Untimestamped advance (pending_advance):** Scene detection only runs in
  the CursorSync handler, not the pending_advance path. If a scene boundary
  falls on an untimestamped line, the next CursorSync will catch it
- **page_turn_lock:** Check lock before snapping — if an animation is in
  flight, skip the scene-scroll (same pattern as `scroll_paragraph_to_top`)

## Files changed

- `src/app.rs` — add `current_sync_scene: Option<(i64, i64)>` to AppState,
  initialize to `None`
- `src/main.rs` — scene-transition detection in CursorSync handler
