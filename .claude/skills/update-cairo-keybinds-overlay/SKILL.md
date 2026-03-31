---
name: update-cairo-keybinds-overlay
description: Use when adding, removing, or changing a keybinding that should be reflected in the Ctrl+/ keyboard overlay Cairo drawing
---

# Update Keybinds Overlay

When a keybinding is added, removed, or changed in `src/input/keymap.rs`, update the corresponding Cairo-drawn keyboard overlay in `src/ui/keybinds_overlay.rs`.

## File

`src/ui/keybinds_overlay.rs` — Cairo-drawn RPD keyboard layout shown via Ctrl+/

## Structure

### Key definitions (top of file)

Each key is a `KeyDef` with fields:
- `unshifted` — character shown on the key
- `shifted` — shifted character (top-right of key)
- `action` — bare key action label (green, bottom-left)
- `shift_action` — shift action label (blue)
- `modifiers` — slice of `(combo, action)` tuples for Ctrl/Alt combos, shown as tooltips on hover

Helper constructors:
- `ub(unshifted, shifted)` — unbound key, no actions
- `bare(unshifted, shifted, action)` — bare key action only
- `key(unshifted, shifted, action, shift_action, modifiers)` — full definition

### Row constants

- `NUMBER_ROW` — top row (`$+[{(&=)}]*!|`) + `BACKSPACE`
- `UPPER_ROW` — after Tab (`;,.pyfgcrl/@\`) + `TAB_KEY`
- `HOME_ROW` — after Esc (`aoeuidhtns-`) + `ESC_KEY`
- `BOTTOM_ROW` — after Shift (`'qjkxbmwvz`) + `SHIFT_KEY`
- Spacebar row: `SPACEBAR_ROW_CTRL`, `_FN`, `_WIN`, `_ALT_L`, `_SPACE`, `_ALT_R`, `_CTRL_R`
- `SEQ_GG`, `SEQ_G` — sequence keys
- `ARROW_UP/DOWN/LEFT/RIGHT` — arrow keys

### Layout builder (`build_layout`)

Computes pixel positions for each key. Row y-positions cascade downward. The bottom row has a Shift key at x=0 before the alpha keys. The spacebar row has Ctrl/Fn below Shift, then Win, Alt, Space, Alt, Ctrl.

### Drawing (`draw_keyboard`)

Iterates all keys and draws: background, border, unshifted char (bold, top-left), shifted char (small, top-right), action label (green, bottom), shift action (blue). Tooltips appear on hover for keys with modifiers.

### Colors (`key_colors`)

Key background/border/text colors are chosen based on which actions are defined:
- Green: has bare action and/or modifiers
- Blue: shift action only
- Dark gray: unbound

## Steps

1. Read `src/ui/keybinds_overlay.rs`
2. Find the key's `KeyDef` in the appropriate row constant
3. Update the `action`, `shift_action`, or `modifiers` field
4. For new keys: add a `KeyDef` constant and add it to the layout in `build_layout`
5. `cargo build` to verify
