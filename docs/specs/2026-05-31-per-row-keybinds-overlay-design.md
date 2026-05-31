# Per-Row Keybinds Overlay

**Date:** 2026-05-31

## Problem

The Ctrl+/ keybinds overlay (`src/ui/keybinds_overlay.rs`) draws the entire RPD
keyboard at once. With ~50 keys crammed into the window, action labels truncate:
"toggle spe[ed]", "media picke[r]", "set end tim[e]", "next mat[ch]". The full
layout sacrifices legibility for completeness.

## Solution

Split the overlay into **one screen per physical keyboard row**, each shown full
width so labels have room. The user cycles rows with `n`/`p`. Each row screen
uses **Approach C**: a faithful keycap strip across the top with one key
highlighted, and a detail panel below showing the highlighted key's full
bindings (bare, shift, Ctrl, Alt) in complete text.

## Row Breakdown (5 row-screens)

1. **Number / symbol** — `$ + [ { ( & = ) } ] * ! |` · Backspace
2. **Upper (qwerty)** — `; , . p y f g c r l / @ \` · Tab
3. **Home** — `a o e u i d h t n s -` · Esc
4. **Bottom** — `' q j k x b m w v z` · Shift
5. **Modifiers & sequences** — Space · gg · g · g; · zt · arrows

The existing row constants (`NUMBER_ROW`, `UPPER_ROW`, `HOME_ROW`,
`BOTTOM_ROW`, plus the spacebar/sequence/arrow keys) map directly to these
screens — no key data changes, only how they are presented.

## Presentation (Approach C)

Each row-screen has two parts:

- **Keycap strip:** the row's keys as large keycaps, left-to-right, faithful to
  the physical row. The currently-highlighted key gets a blue glow.
- **Detail panel:** below the strip, a panel for the highlighted key showing
  every binding in full text:
  - bare key → action (green)
  - Shift+key → shift action (blue)
  - each modifier combo (Ctrl/Alt/Ctrl-Shift) → action (one per line)

  Keys with no bindings still show their keycap but the detail panel reads
  "(unbound)".

## Navigation

- **Ctrl+/** — open the overlay (row 1, first bound key highlighted)
- **n / p** — cycle to the next / previous row-screen (wraps)
- **j / k** or **← / →** — move the highlight within the current row
- **Esc** — close, return to Reader

### Interaction with the existing gamepad overlay

Today `n`/`p` in the keybinds overlay toggles to the **gamepad overlay** (a
separate full overlay). That two-way toggle is replaced: `n`/`p` now cycles the
five keybinds row-screens. The gamepad overlay is appended as a **sixth screen**
in the same cycle (row 6 = "Gamepad"), so `n`/`p` walks
number → upper → home → bottom → mod/seq → gamepad → (wrap to number). This
preserves access to the gamepad reference without a separate toggle path.

(If the gamepad overlay is better kept separate, the alternative is to leave it
on its own Ctrl+/-equivalent and have keybinds `n`/`p` cycle only the five rows.
Default: fold it in as row 6.)

## Implementation

`src/ui/keybinds_overlay.rs` is reworked from "draw the whole keyboard" to
"draw one row-screen by index":

- A `RowScreen` enum / index (0..=5) held in the overlay (`Rc<Cell<usize>>`).
- A `selected_key` index within the current row (`Rc<Cell<usize>>`).
- `build_row_screens()` groups the existing `KeyDef` row constants into the five
  row lists (reusing `NUMBER_ROW`, `UPPER_ROW`, `HOME_ROW`, `BOTTOM_ROW`, and a
  new `MOD_SEQ_ROW` assembled from the spacebar/sequence/arrow consts).
- `draw_row_screen(cr, row, selected, w, h)` replaces `draw_keyboard`:
  - draws the keycap strip centered, full width, scaling keycap size to fit
  - draws the blue highlight on `selected`
  - draws the detail panel below with the highlighted key's full bindings
  - draws a header ("Row N of 6 — HOME ROW") and the footer hint
- The mouse-motion hover/tooltip logic is replaced by click/hover to set
  `selected` (hovering a keycap updates the detail panel). Mouse is optional;
  keyboard j/k is primary.

`src/input/keymap.rs` `handle_keybinds_key`:
- `n` → advance row index (wrap), redraw
- `p` → previous row index (wrap), redraw
- `j`/`Right` → `selected` += 1 (clamp/wrap within row)
- `k`/`Left` → `selected` -= 1
- `Escape` → hide, Reader mode
- Remove the keybinds↔gamepad toggle branch (gamepad becomes row 6).

New `KeybindsOverlay` methods: `next_row()`, `prev_row()`, `move_selection(±1)`,
each updating state and calling `queue_draw()`.

## Out of Scope

- No change to any actual keybindings or `KeyDef` data — purely presentation.
- No new config; the overlay always opens at row 1.
- No search/filter within the overlay.

## Risks

- **Gamepad-as-row-6 fit:** the gamepad overlay has a different shape than a
  keyboard row. It may render as its own non-keycap screen within the cycle.
  Acceptable — it is just one more screen reached by `n`/`p`.
- **Keycap strip width:** the number row (13 keys + Backspace) is the widest. At
  full window width the keycaps should still fit; if not, the strip scales down
  (keycaps shrink) while the detail panel stays full-size — labels live in the
  panel, not on the caps, so shrinking caps is harmless.
- **`update-cairo-keybinds-overlay` skill:** that skill documents editing the
  per-key `KeyDef`s. It stays valid — the `KeyDef` constants are unchanged; only
  the draw/layout functions change. The skill's "Drawing" and "Layout builder"
  sections need a note that rendering is now per-row.
