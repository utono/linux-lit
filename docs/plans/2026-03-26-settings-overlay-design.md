# E-Reader Settings Overlay — Design Spec

**Date:** 2026-03-26
**Status:** Draft

## Overview

A modal overlay for adjusting e-reader display settings (line spacing, column width, text margins, theme). Opened with `Ctrl+,`. Vim-style keyboard navigation. All changes apply live. Enter confirms and persists; Escape reverts.

## Settings

Four settings, displayed as a flat list:

- **Line Spacing** — `pixels_above_lines` / `pixels_below_lines` on the GtkTextView. Range 0–20, step 1, default 4. Display: `"{value}px"`.
- **Column Width** — `width_request` on the ScrolledWindow. Range 400–1200, step 50, default 950. Display: `"{value}px"`.
- **Text Margins** — `left_margin` / `right_margin` on the GtkTextView. Range 8–96, step 4, default 48. Display: `"{value}px"`.
- **Theme** — cycles through all loaded themes from `themes-unified.json`. Display: theme `display_name`. Applies immediately on change (CSS + tags + cursor highlight refreshed).

## Interaction

- `Ctrl+,` — toggle overlay visibility
- `j` / `k` (or arrow down/up) — move highlight between settings rows
- `h` / `l` (or arrow left/right) — decrement/increment the selected setting's value
- `Enter` — confirm all changes, persist to `config.json`, close overlay
- `Escape` — revert all settings to values when overlay was opened, close overlay

All value changes apply live to the text view behind the overlay so the user sees the effect immediately.

When the overlay opens, it captures a snapshot of all current values. Escape restores this snapshot. Enter persists the current values.

## Config Persistence

Add three new fields to `Config` in `config.rs`:

```rust
pub line_spacing: u32,    // default 4
pub column_width: u32,    // default 950
pub text_margins: u32,    // default 48
```

On app startup, apply these values from config instead of hardcoded constants. Theme is already persisted via `.current_theme` file — no change needed for theme persistence.

## Visual Design

The overlay is a centered box (matching the library picker pattern):

- Width: 500px, auto height
- Semi-transparent dark backdrop behind the box (consistent with other overlays)
- Title "Settings" centered at top with bottom border
- Each row: label on the left, `◀ value ▶` on the right
- Highlighted row: accent background with left border (using theme's cursor line color or a fixed accent)
- Footer text: `j/k navigate · h/l adjust · Enter confirm · Esc revert`

## Implementation

### New file: `src/ui/settings_overlay.rs`

A `SettingsOverlay` struct following the `LibraryPicker` pattern:

- `overlay: Overlay` — the GTK overlay widget
- `container: GtkBox` — the visible settings box
- `selected: usize` — which row is highlighted (0-indexed)
- `snapshot: SettingsSnapshot` — values captured on open
- `themes: Vec<Theme>` — loaded theme list
- `theme_index: usize` — current position in theme list

Public API:
- `new(themes: Vec<Theme>) -> Self`
- `show(state: &AppState)` — capture snapshot, show overlay
- `hide()` — hide overlay
- `is_visible() -> bool`
- `move_selection(delta: i32)` — j/k navigation, wraps around
- `adjust_value(delta: i32, state: &mut AppState)` — h/l to change selected setting, applies live
- `confirm(state: &mut AppState)` — persist config, hide
- `revert(state: &mut AppState)` — restore snapshot values, hide
- `attach(base: &impl IsA<Widget>)` — attach overlay to widget tree

### SettingsSnapshot

```rust
struct SettingsSnapshot {
    line_spacing: u32,
    column_width: u32,
    text_margins: u32,
    theme_index: usize,
}
```

### Live apply

When `adjust_value` is called:
- **Line Spacing**: `text_view.set_pixels_above_lines(val)` and `set_pixels_below_lines(val)`
- **Column Width**: `scrolled_window.set_width_request(val as i32)`
- **Text Margins**: `text_view.set_left_margin(val as i32)` and `set_right_margin(val as i32)`
- **Theme**: call existing theme application code (`generate_css`, update `css_provider`, refresh `dim_tag` foreground, update cursor highlight). Write theme name to `.current_theme` file.

### Wiring

- `keymap.rs`: detect `Ctrl+,` → toggle settings overlay
- `keymap.rs`: when settings overlay is visible, route j/k/h/l/Enter/Escape to the overlay
- `app.rs`: add `settings_overlay: SettingsOverlay` to `AppState`
- `app.rs`: on startup, apply `config.line_spacing`, `config.column_width`, `config.text_margins` instead of hardcoded values
- `ui/mod.rs`: add `pub mod settings_overlay`

### Theme cycling detail

The overlay loads all themes on construction. When the user h/l's on the Theme row, it advances/retreats `theme_index`, applies the new theme immediately (same code path as the existing theme picker), and updates the overlay's own styling to match the new theme colors.

## What this does NOT change

- Font family cycling (`Ctrl+Shift+f`) — stays as keybind-only
- Font size adjustment (`Ctrl+Shift+!`/`|`) — stays as keybind-only
- Theme picker (`Ctrl+Shift+\`) — remains available as an alternative way to pick themes
- Top margin (24px) — not exposed as a setting
