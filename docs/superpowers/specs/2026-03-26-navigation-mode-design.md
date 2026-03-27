# Navigation Mode Design

## Summary

Change the default navigation from e-reader page-turn style to text-editor scroll style with the cursor kept centered on screen. Offer the e-reader mode as a toggle in the settings overlay.

## Navigation Modes

### Scroll (default)

- j/k moves the cursor and scrolls the viewport so the cursor line stays at the vertical midpoint
- Near document edges (within half a viewport of the top or bottom), the scroll clamps so no blank space appears. The cursor naturally drifts from center toward the edge, matching vim's `scrolloff` behavior
- Page turns (Ctrl+d/f, Ctrl+u/b) unchanged
- Jump commands (gg, G, dialogue jumps, search) unchanged
- `restore_cursor` centers the restored line on screen

### E-Reader

- Current behavior: j triggers page turn when cursor reaches the bottom visible line; k scrolls minimally to keep cursor visible
- Page turns, jumps, restore all unchanged

## Config

Add to `Config` struct:

- `navigation_mode: NavigationMode` with `#[serde(default)]`
- `NavigationMode` enum: `Scroll` (default), `EReader`
- Serializes as `"scroll"` / `"ereader"` in config.json

## Navigation Changes

### `move_cursor` in `src/input/navigation.rs`

After updating `current_line`, branch on `state.config.navigation_mode`:

- `Scroll`: call `center_cursor(state)` which computes the scroll offset to place the cursor line at the viewport midpoint, clamped to `[0, max_scroll]` to prevent blank space at document boundaries
- `EReader`: existing page-turn-at-edge logic

### New helper: `center_cursor`

```
fn center_cursor(state: &mut AppState) {
    let target_y = scroll_value_for_line(state, state.current_line);
    let adj = state.scrolled_window.vadjustment();
    let half_page = adj.page_size() / 2.0;
    let centered = (target_y - half_page).max(0.0).min(adj.upper() - adj.page_size());
    adj.set_value(centered);
    state.page_top_line = ... // derive from scroll position
}
```

### `restore_cursor`

In `Scroll` mode, center the cursor instead of placing it near the top.

## Settings Overlay

- Add "Navigation" as 5th row (after Theme)
- `NUM_SETTINGS` becomes 5
- h/l cycles between `Scroll` and `E-Reader` display values
- New `SettingsChange::Navigation(NavigationMode)` variant
- Snapshot includes `navigation_mode` for Escape revert
- Enter persists to config

## Files Changed

- `src/config.rs` — `NavigationMode` enum, new field on `Config`
- `src/input/navigation.rs` — branch in `move_cursor`, new `center_cursor` helper, update `restore_cursor`
- `src/ui/settings_overlay.rs` — 5th row, snapshot field, adjust logic
- `src/input/keymap.rs` — handle `SettingsChange::Navigation`
