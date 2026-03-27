# Keybinds Popup Design

## Overview

A keyboard-shaped reference popup showing all linux-lit keybinds, organized by RPD keyboard row with keys positioned to mirror their physical location. Toggled with Ctrl+/.

## Trigger

- `Ctrl+/` toggles the popup open/closed
- `Esc` closes it
- All other keys consumed while visible

## Layout

The popup renders a visual keyboard layout with four rows (number, upper, home, bottom) plus an "Other" section for keys that don't map to a single physical position.

Each keyboard row is a horizontal flex container with increasing left margin to mimic the physical stagger of a keyboard:

- Number row: 0px indent
- Upper row: 16px indent
- Home row: 24px indent
- Bottom row: 36px indent

Each key is a small cell containing:

- Key label (e.g., "a", "o O", "C-d") — color-coded by modifier type
- Action description below (e.g., "play from ts", "seek -3.5/-60")
- For keys with both bare and modifier bindings, the modifier binding appears as a second line within the same cell

Unbound keys are shown at reduced opacity in their correct physical position to preserve spatial context.

The "Other" section uses the same cell style in a wrapping flex row for: gg, G, arrow keys, Backspace, Esc, Ctrl+arrows, Alt combos.

A legend bar at the bottom shows the color mapping.

## Color Coding

- Blue (#7db8f0): bare key
- Gold (#d4a052): Ctrl+ modifier
- Purple (#c47dd4): Alt+ modifier
- Red (#d45050): Ctrl+Alt+ modifier

## Data Model

A const array in keybinds_overlay.rs defines all keybind entries:

```rust
struct KeybindEntry {
    key_label: &'static str,      // "a", "o O", "C-d"
    action: &'static str,         // "play from ts"
    modifier: ModifierType,       // None, Ctrl, Alt, CtrlAlt
    sub_binds: &'static [(        // modifier combos on same physical key
        &'static str,             // "C-u"
        &'static str,             // "pg back"
        ModifierType,
    )],
}
```

Keybind entries are grouped into const arrays per row (NUMBER_ROW, UPPER_ROW, HOME_ROW, BOTTOM_ROW, OTHER), ordered left-to-right per RPD layout. Unbound keys included with empty action.

## Widget Structure

New file: `src/ui/keybinds_overlay.rs`

```
KeybindsOverlay
├── overlay: Overlay
├── container: GtkBox (vertical, 820px wide, centered)
│   ├── Label "Number Row" (row header)
│   ├── GtkBox (horizontal, flex, gap 3px)
│   │   ├── GtkBox (key cell) → Label (key) + Label (action)
│   │   ├── GtkBox (key cell, unbound, faded)
│   │   └── ...
│   ├── Label "Upper Row"
│   ├── GtkBox (horizontal, margin-left 16px)
│   │   └── ...
│   ├── Label "Home Row"
│   ├── GtkBox (horizontal, margin-left 24px)
│   │   └── ...
│   ├── Label "Bottom Row"
│   ├── GtkBox (horizontal, margin-left 36px)
│   │   └── ...
│   ├── Label "Other"
│   ├── GtkBox (horizontal, wrapping)
│   │   └── ...
│   └── GtkBox (legend bar)
```

## CSS Classes

Added to `generate_css()` in theme.rs:

- `.keybinds-overlay` — rgba(40,40,40,0.95), centered, padding 16px, border-radius 8px
- `.keybind-key` — rgba(255,255,255,0.08) background, border-radius 3px, min-width 68px, padding 3px 6px
- `.keybind-key-unbound` — opacity 0.25
- `.keybind-label-bare` — color #7db8f0, font-size ~11px, bold
- `.keybind-label-ctrl` — color #d4a052
- `.keybind-label-alt` — color #c47dd4
- `.keybind-label-ctrlalt` — color #d45050
- `.keybind-action` — color rgba(255,255,255,0.5), font-size ~9px
- `.keybind-row-header` — uppercase, letter-spacing 2px, dim, font-size 9px
- `.keybind-legend` — border-top, flex row with color swatches

## Integration

- Add `keybinds_overlay: KeybindsOverlay` to `AppState`
- In `keymap.rs`:
  - Check `keybinds_overlay.is_visible()` early in `handle_key`
  - `Ctrl+/` toggles visibility
  - When visible: Esc closes, all other keys consumed
- Overlay attachment: The keybinds overlay wraps the settings overlay, becoming the new outermost overlay in the chain: `picker → media_picker → settings_overlay → keybinds_overlay`. The vbox appends `keybinds_overlay.overlay` instead of `settings_overlay.overlay`. Action popup is added as a sibling overlay on the keybinds overlay.

## Keybind Inventory

All keybinds from keymap.rs, ordered by RPD row position:

**Number Row:** +, 0, !, |
**Upper Row:** , (comma), . (period), p, P, y, f, F, l, /
**Home Row:** a, o, O, e, E, u, i, n, N, Tab
**Bottom Row:** q, j, k, x, m, V
**Ctrl combos:** C-p, C-,, C-/, C-d, C-f, C-u, C-b, C-Up, C-Down
**Alt combos:** M-f, M-i
**Ctrl+Alt:** C-M-l
**Other:** gg, G, Right arrow, Backspace, Escape
