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
- `MOD_SEQ_ROW` — modifiers/sequences/arrows row (Space, gg, G, g;, zt, arrows) as one screen

The row-leader keys `BACKSPACE`, `TAB_KEY`, `ESC_KEY`, `SHIFT_KEY` are still
separate consts, prepended/appended to their row in `row_keys()`.

## Per-Row Rendering (current)

The overlay is **per-row** — one physical keyboard row per screen. Ctrl+/ opens
row 1; `n`/`p` cycle rows (the gamepad overlay is the 6th screen); `j`/`k` or
`←`/`→` move the key highlight; Esc closes.

### `row_keys(idx)`

Returns the `KeyDef`s for screen `idx` (0..`ROW_COUNT`). `ROW_TITLES` holds each
screen's header text. No pixel layout — keys are laid out at draw time.

### Drawing (`draw_row_screen`)

Draws one row-screen: a **keycap strip** across the top (one cap per key, the
`selected` cap highlighted blue, with a truncated bare-action hint under each
glyph) and a **detail panel** below listing the highlighted key's full
bindings — bare (green/pine), Shift (iris), Ctrl (gold), Ctrl+Shift (green),
Alt (rose), one row each. Colors are inlined in `draw_row_screen`; there is no
separate `key_colors`/`build_layout`/`draw_keyboard`/`hit_test` (removed).

## Steps

1. Read `src/ui/keybinds_overlay.rs`
2. Find the key's `KeyDef` in the appropriate row constant (`NUMBER_ROW`,
   `UPPER_ROW`, `HOME_ROW`, `BOTTOM_ROW`, `MOD_SEQ_ROW`, or a row-leader const)
3. Update the `action`, `shift_action`, or `modifiers` field — the detail panel
   and keycap hint pick it up automatically; no layout edit needed
4. For a new key: add a `KeyDef` to the appropriate row constant (it renders in
   that row's keycap strip automatically)
5. `cargo build` to verify
