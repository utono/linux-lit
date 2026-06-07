# Keybinds overlay: press-to-jump

Date: 2026-06-07
Branch: `feat/keybinds-overlay-press-to-jump`

## Goal

While the Ctrl+/ keybinds overlay is open, let the user jump the highlight
directly to a key's cap by pressing that physical key, instead of only the
arrow / `j`-`k` / `n`-`p` navigation that exists today.

## Current behavior (baseline)

- `Ctrl+/` opens `KeybindsOverlay` (`src/ui/keybinds_overlay.rs`), a Cairo-drawn
  per-row keyboard legend. Five keyboard-row screens plus a 6th gamepad screen.
- Keys are routed by `handle_keybinds_key` in `src/input/keymap.rs`:
  - `Esc` closes (back to Reader).
  - `n` / `Up` next row; past the last row hands off to the gamepad overlay.
  - `p` / `Down` prev row; before the first row hands off to the gamepad overlay.
  - `j` / `Right` move highlight forward within the row.
  - `k` / `Left` move highlight backward within the row.
  - everything else is consumed (no-op).
- `row_index` and `selected` are `Rc<Cell<usize>>` on the overlay and persist
  across hide/show.

## New behavior

Two modes, toggled by **Tab**:

- **Jump mode** — the default each time the overlay opens. Any printable key
  jumps the highlight to that key's cap. If the cap is on a different row, the
  overlay auto-switches to that row first, then highlights the cap (its detail
  panel renders). Matching is **unshifted-glyph only**: e.g. `?` (Shift+/)
  jumps to the same cap as `/`. Keys with no matching cap are consumed with no
  effect.
- **Nav mode** — the existing behavior: `n`/`p` cycle rows (with the gamepad
  handoff intact), `j`/`k` move the highlight.

In **both** modes:

- **Esc** closes the overlay.
- **Tab** toggles jump <-> nav and redraws.
- **Arrow keys** (←→↑↓) navigate: ←/→ move the highlight, ↑/↓ cycle rows (with
  the gamepad handoff). Arrows never collide with a cap, so they stay live in
  jump mode as an always-available fallback.

The gamepad screen is reached only via row cycling (nav-mode `n`/`p` or arrows),
exactly as today. Jump mode does not target the gamepad — it is a controller
legend, not a keyboard cap.

Reset rule: every open via `Ctrl+/` starts in **jump mode** (predictable;
matches the headline feature). `row_index`/`selected` still persist as today.

## Components

### `src/ui/keybinds_overlay.rs`

- Add `jump_mode: Rc<Cell<bool>>` to `KeybindsOverlay`, defaulting to `true` in
  `new()` and forced to `true` in `show()` / `show_last_row()` (always-jump on
  open).
- The draw closure reads `jump_mode` so the header and footer can show the mode.
- New: `key_name_to_glyph(key_name: &str) -> Option<&'static str>` — maps a GTK
  keyval name to the cap's unshifted glyph string for the symbol keys that don't
  match by identity. Table (non-exhaustive, covers every symbol cap on screen):
  `slash`→`/`, `comma`→`,`, `period`→`.`, `parenleft`→`(`, `ampersand`→`&`,
  `bracketleft`→`[`, `braceleft`→`{`, `backslash`→`\`, `minus`→`-`,
  `apostrophe`→`'`, `plus`→`+`, `asterisk`→`*`, `exclam`→`!`, `bar`→`|`,
  `at`→`@`, `dollar`→`$`, `equal`→`=`, `parenright`→`)`, `braceright`→`}`,
  `bracketright`→`]`. Single-character names (`h`, `o`, `g`, …) fall through and
  match by identity.
  - **Known limitation — Space:** the `space` keyval is intercepted globally at
    the top of `handle_key` (MPV play/pause) and returns before mode dispatch,
    so it never reaches `handle_keybinds_key`. The Space cap (mod/seq row) is
    therefore not jump-targetable; reach it with arrows or by jumping to a
    nearby key then ←/→. We deliberately do NOT special-case the overlay in the
    global Space handler. (`gg`, `g;`, `zt` on the mod/seq row are multi-char
    sequence pseudo-caps and likewise not single-key jump targets.)
- New: `find_cap(key_name: &str) -> Option<(usize, usize)>` — resolves the
  glyph (via the table or identity), then scans rows `0..ROW_COUNT` and their
  `row_keys(idx)` for the first `KeyDef` whose `unshifted` equals that glyph.
  Returns `(row_idx, cap_idx)`.
- New public method `jump_to_key(&self, key_name: &str) -> bool` — calls
  `find_cap`; on hit, sets `row_index` + `selected`, queues a redraw, returns
  `true`; on miss returns `false`.
- New public method `toggle_mode(&self)` — flips `jump_mode` and redraws.
- New public accessor `is_jump_mode(&self) -> bool`.
- Header: append the mode to the existing centered header line, e.g.
  `Row 3 of 6  —  HOME ROW  —  JUMP` / `… —  NAV`.
- Footer rewritten to be mode-aware, e.g.
  jump: `Esc close · Tab jump/nav · press a key to jump · ←→ move · ↑↓ rows`
  nav:  `Esc close · Tab jump/nav · n/p rows · j/k move · ←→↑↓ also`.

### `src/input/keymap.rs` — `handle_keybinds_key`

Restructure the match so mode is consulted:

```
Escape            -> close (both modes)
Tab               -> overlay.toggle_mode()
Up / Down         -> row cycle (+ gamepad handoff), both modes
Left              -> move_selection(-1), both modes
Right             -> move_selection(1),  both modes
otherwise:
  if jump_mode    -> overlay.jump_to_key(key_name)   // consume regardless
  else (nav mode):
    n             -> next_row (+ gamepad handoff)
    p             -> prev_row (+ gamepad handoff)
    j             -> move_selection(1)
    k             -> move_selection(-1)
    _             -> consume
```

The two row-cycle handoff blocks (next_row/prev_row returning `false` → switch
to the gamepad overlay and set `InputMode::GamepadOverlay`) are factored so both
the arrow paths and the nav-mode `n`/`p` paths reuse them rather than
duplicating the borrow dance four times.

## Data flow

Key press → `handle_key` → mode dispatch → `handle_keybinds_key` →
(jump) `keybinds_overlay.jump_to_key(name)` sets `row_index`/`selected` →
`queue_draw` → `draw_row_screen` reads `jump_mode`, `row_index`, `selected` and
repaints the strip + detail panel + mode-aware header/footer.

No new `InputMode` is introduced — jump/nav is internal to the overlay, so the
existing `InputMode::KeybindsOverlay` dispatch is unchanged.

## Error handling / edge cases

- Key with no matching cap (e.g. a function key, or `z`-with-no-cap): consumed,
  no movement. The overlay never lets a key fall through to the reader.
- Pressing a key already selected: re-selects the same cap (idempotent redraw).
- `find_cap` clamps nothing — `selected` is set to a valid in-row index by
  construction (it comes from iterating that row's keys).
- Tab in either mode only toggles; it is never treated as a cap (`Tab` key cap
  exists on the upper row as the row-leader, but Tab is intercepted before the
  jump branch).
- Gamepad handoff unchanged; jump mode cannot reach it (intended).

## Testing

Pure-logic (cargo `--bins`, no GTK):

- `key_name_to_glyph` returns the right glyph for each symbol name and `None`
  is never needed for a single-char letter (those go through identity in
  `find_cap`).
- `find_cap` resolves every cap that has a non-empty `unshifted` glyph to a
  valid `(row, idx)` within `ROW_COUNT`, and that `row_keys(row)[idx].unshifted`
  equals the resolved glyph.
- `find_cap` for a few representative names: `"h"` → home row, `"slash"` →
  upper row, `"plus"` → number row, `"G"`/`"g"` → mod/seq or upper row caps.
- `find_cap` for an unmapped name (`"F5"`) returns `None`.

Note: `find_cap`/`jump_to_key` logic should be testable without constructing the
GTK widget — the cap tables and resolution are free functions; the unit tests
target those. `jump_to_key` itself touches a `Cell` + `queue_draw`, so the test
coverage is on the free `find_cap`/`key_name_to_glyph` functions it delegates to.

Rendered / behavioral (per project e2e rule — agent asks the user to run):

- Open `Ctrl+/`: starts in jump mode (header shows JUMP, new footer).
- Press `h` from the number row → switches to HOME ROW, `h` highlighted.
- Press `/` → switches to UPPER ROW, `/` highlighted (keybinds cap).
- `Tab` → header/footer flip to NAV; `n`/`p` cycle rows, `j`/`k` move; `Tab`
  back to JUMP.
- Arrows work in both modes; gamepad handoff still reachable via nav `n`/`p`
  and via ↑/↓.
- `Esc` closes from either mode.

## Out of scope (YAGNI)

- Shifted-glyph jump targets (only unshifted matching this round).
- Executing a key's real action from the overlay (launcher behavior) — this is
  jump-to-highlight only.
- Remembering the last mode across opens (always reset to jump).
- Any change to the gamepad screen's own input handling.
- Reworking the `describe()` blurbs or row groupings.
