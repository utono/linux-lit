# Sync-Deferred Page Turn

## Summary

When playback sync is active and audio is playing, defer page turns in e-reader mode until the last fully visible line's `end_time` is reached. The new page starts with the next line as `page_top_line`.

## Current Behavior

During playback sync, `CursorSync` events move the cursor line-by-line. When the cursor moves to a line that isn't fully visible, `update_highlight_and_ensure_visible()` immediately triggers `set_page()` with a crossfade. The page turns at the `start_time` of the off-screen line.

## New Behavior

When all three conditions hold — `sync_enabled`, audio is playing, and e-reader mode — the page turn is deferred:

1. `CursorSync` moves cursor to the last fully visible line as normal
2. Instead of turning the page when the next sync event targets an off-screen line, store a `pending_page_turn: Option<(f64, usize)>` — the `end_time` of the last visible line and the target `page_top_line` (next line after it)
3. The `TimePos` handler checks: if `pos >= pending_page_turn.end_time`, execute `set_page(target_line, Forward)` with the crossfade
4. The cursor highlight stays on the last visible line until the page turns

## State Change

Add to `AppState`:

```rust
/// Deferred page turn during playback sync: (end_time, new_page_top_line)
pub pending_page_turn: Option<(f64, usize)>,
```

## Logic Changes

### CursorSync handler (main.rs ~line 131)

When sync moves the cursor to a line that is NOT fully visible:

- Look up the last fully visible line on the current page
- Get its `end_time` from the work's timestamp data
- If it has a timestamp: set `pending_page_turn = Some((end_time, target_line))` where `target_line` is the line after the last visible one. Do NOT call `update_highlight_and_ensure_visible`. Keep cursor on the last visible line.
- If no timestamp on the last visible line: fall back to current behavior (immediate page turn)

### TimePos handler (main.rs ~line 193)

After existing `pending_advance` check, add:

```
if let Some((end_time, new_top)) = s.pending_page_turn {
    if pos >= end_time {
        s.pending_page_turn = None;
        s.current_line = new_top;
        set_page(state, new_top, PageDirection::Forward);
        // highlight + config save
    }
}
```

### Cancellation

Clear `pending_page_turn` when:
- User manually navigates (j/k/gg/G/page keys) — in the suppress_sync_until path
- Sync is toggled off
- A new work is loaded

### Scope

- Only affects e-reader mode during active playback sync
- Manual j/k navigation: unchanged (immediate page turn)
- Scroll mode: unchanged (no page turns)
- Paragraph-aware scrolling during sync: the deferred page turn replaces the immediate turn; paragraph detection still applies to the new page after it lands

## Edge Cases

- Last visible line has no timestamp: immediate page turn (current behavior)
- User presses j/k while `pending_page_turn` is set: cancel pending, do normal manual page turn
- Multiple `CursorSync` events arrive while waiting: ignore them (cursor stays on last visible line, pending_page_turn already set)
- Audio seeks past the end_time: `TimePos` handler fires the turn on the next tick
