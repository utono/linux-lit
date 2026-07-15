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
row 1. There are no modes and no footer/pill: the header is just
`Row N of 6 — <TITLE>`.

Input model — **every key jumps the highlight to its own cap**, auto-switching
rows if the cap is on another row (arrows resolve to the `↑↓←→` caps, `Space` to
the Space cap, symbols to their symbol caps, `Tab` to the Tab cap, `j`/`k` to the
`j`/`k` caps). Matching is unshifted-glyph only (see `find_cap` /
`key_name_to_glyph`), so digit keys (`1`..`0`) and modified combos
(Ctrl/Alt/Shift chords) are NOT jump targets — only the bare unshifted glyph
printed on the cap is; those caps are reached by pressing a neighbour and reading
across, or via the `Ctrl+n`/`Ctrl+p` row navigation.

**Row navigation is `Ctrl+n` (next row) / `Ctrl+p` (previous row)** — the gamepad
overlay is the 6th screen, reached past the last/first row. Every UNMODIFIED key
still jumps to its own cap. `Esc` closes.

Key routing lives in `handle_keybinds_key` (`src/input/keymap.rs`, which takes
`is_ctrl`); `jump_to_key` / `next_row` / `prev_row` live on `KeybindsOverlay`.

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
5. Add/adjust the `describe()` arm for any label you introduced (step 6 verifies
   this is complete)
6. **Run the exhaustive cross-reference (below)** — this is mandatory, not
   optional. The overlay silently drifts: a blank detail row or a label naming
   the wrong action both render fine and compile clean.
7. `cargo build` to verify

## Exhaustive cross-reference (mandatory)

The overlay is a *hand-maintained mirror* of the keymap. Nothing enforces that
the mirror is accurate — a `KeyDef` can show a stale label, an empty slot for a
real binding, or a label with no `describe()` blurb, and it all compiles. After
ANY overlay edit, verify EVERY binding round-trips. Do not spot-check; check all.

**The two sources of truth:**

- **Bindings (what each key does):** `default_reader_bindings()` in
  `src/input/keymap_config.rs`. Each is `(KeyCombo::<MOD>("<key>"), Action::X)`.
  An **uppercase single letter** in `plain("X")` means **Shift+x** (GTK delivers
  shifted letters as the uppercase name; the shift flag is redundant). So
  `plain("U")` is Shift+u, `plain("Q")` is Shift+q. A few keys are handled
  *before* the keymap in `handle_key`/`handle_key_inner` (`src/input/keymap.rs`)
  — notably **Space = MPV play/pause** (not in `default_reader_bindings()`).
  Check there for any key whose overlay label looks unbound in the config.
- **Display (what the overlay claims):** the row `KeyDef` tables +
  `describe(label)`, both in `src/ui/keybinds_overlay.rs`. A `KeyDef` slot that
  is `""` renders a **blank** detail row. A non-empty label with no matching
  `describe()` arm renders the short label with **no long blurb**. Modifier glyph
  prefixes: `C-` Ctrl, `M-` Alt, `S-C-`/`C-S-` Ctrl+Shift, `C-M-` Ctrl+Alt.

**Run all three passes. Report each gap with file:line before fixing.**

- **Pass A — no blank slot hides a real binding.** For every binding in
  `default_reader_bindings()`, find its keycap and confirm the matching slot is
  populated: bare key → `action`; `plain("<UPPER>")` → `shift_action` on the
  lowercase cap; `ctrl`/`alt`/`ctrl_shift`/`ctrl_alt` → a `modifiers` entry with
  the right glyph prefix. A populated slot elsewhere counts (e.g. Shift+g's
  `JumpToEnd` shows on the standalone `G` cap in `MOD_SEQ_ROW`, so the `g` cap's
  empty `shift_action` is fine) — note such cases explicitly rather than
  "fixing" them into a duplicate.
- **Pass B — no label names the wrong action.** For every populated keycap slot
  (action, shift_action, every modifier label), confirm the keymap actually binds
  that key+modifier to an action whose meaning matches the label. Watch for: a
  bare label for a key that is only bound *with* a modifier (bare slot should be
  `""`), and labels left over from a since-moved binding.
- **Pass C — every label has a `describe()` arm.** Collect every non-empty label
  string used anywhere in the tables. For each, apply `strip_shift_prefix` (drops
  a leading `"<char>: "`) and confirm a `describe()` match arm exists — i.e. it
  does NOT fall through to `_ => return None`. Add an arm for any miss, ending
  with the `-> handler — src/path` reference like the existing arms.

For a large or post-refactor sweep, dispatch this as a single read-only
subagent task (give it the two file paths, the uppercase=Shift rule, the
`handle_key` Space caveat, and the three passes) and have it report
`key / claimed / actual / file:line` for every gap. Then apply fixes and
re-run `cargo build`.
