# Gloss overlay visual-selection mode

## Goal

In the gloss overlay, `Shift+V` enters a visual block-selection mode that mirrors
the synopsis overlay's visual mode (`j`/`k` extend, `gg`/`G` to ends, `y` yank,
`Esc`/`V` exit). The current gloss `Shift+V` (cycle active TTS voice) moves to
`Ctrl+V`. Lowercase `v` (open voice picker) is unchanged.

## Current behaviour

- `handle_gloss_key` (`src/input/keymap.rs`): plain `"V"` calls
  `gloss::cycle_active_voice`; `"v"` opens the voice picker.
- Synopsis visual mode already exists: `handle_synopsis_overlay_key`'s `"V"`
  calls `gloss_overlay.enter_visual()` + sets `InputMode::SynopsisVisual` +
  `set_synopsis_visual_hint()`. `handle_synopsis_visual_key` handles
  `j/k/gg/G/y/Esc/V` and **hardcodes returning to `SynopsisOverlay`** and the
  synopsis hint.
- The visual primitives (`enter_visual`, `exit_visual`, `visual_step`,
  `visual_to_end`) operate on the shared `self.blocks` and work for any block
  list (gloss or synopsis). Only the yank text source differs:
  `visual_selection_text()` reads `current_synopsis`, which is empty in a gloss.

## Design

### 1. New input mode

Add `InputMode::GlossVisual`. A separate mode (not reusing `SynopsisVisual`) is
required because the synopsis visual handler returns to `SynopsisOverlay`;
routing gloss `V` through it would dump the user into the synopsis overlay.

### 2. Key remap in `handle_gloss_key`

- Move `cycle_active_voice` into the existing `is_ctrl` match block so
  `Ctrl+V` cycles voice (alongside `Ctrl+n`/`Ctrl+p`).
- Replace the plain `"V"` arm with the synopsis-style enter-visual: call
  `gloss_overlay.enter_visual()`; on success set `input_mode = GlossVisual`
  and `gloss_overlay.set_gloss_visual_hint()`.

### 3. New `handle_gloss_visual_key`

A near-copy of `handle_synopsis_visual_key`, differing only in:
- `y`/`Esc`/`V` return to `InputMode::GlossOverlay` and call
  `set_gloss_hint()` (not the synopsis hint).
- `y` yanks **full block text**: a new
  `gloss_overlay.visual_selection_buffer_text()` reads the `gloss_view`
  buffer from the first selected block's `start_line` to the last selected
  block's `end_line` (source line + its gloss together, as displayed). This
  does not depend on `current_synopsis`.

Wire the new mode into the mode-dispatch `match` in `handle_reader_key`.

### 4. Hint methods on `GlossOverlay`

- `set_gloss_hint()` — sets the existing gloss hint text plus a `· ⇧V select`
  suffix. Call it from the gloss render method (replacing the inline
  `self.hint.set_text(...)` at line ~671) so normal and exit-visual paths share
  one string.
- `set_gloss_visual_hint()` — `"⇧V/Esc exit · j/k extend · gg/G ends · y yank"`
  (same as synopsis visual hint).

### 5. Yank toast

The `y` yank "Copied" toast reuses the existing synopsis-visual pattern (direct
`timeout_add_local_once` on `chapter_toast`). The generation-guard fix applied
to `show_chapter_toast` is NOT applied here — out of scope for this change.

### 6. Ctrl+/ keybinds overlay sync

Run the `update-cairo-keybinds-overlay` skill: the gloss-context `V`/`Ctrl+V`/`v`
caps and `describe()` arms must reflect `Shift+V` select / `Ctrl+V` cycle voice /
`v` voice picker.

## Out of scope

- Generation-guarded yank toast.
- Any change to synopsis visual mode.

## Verification

- `cargo build` clean; `cargo test --bins` green (no pure-logic tests cover GTK
  key routing, so no new unit test).
- Runtime (user-run, per CLAUDE.md): open a gloss (`Ctrl+g`), press `Shift+V` —
  selection bar appears and the voice toast does NOT show. `j`/`k` extend,
  `y` copies the selected blocks (verify with `wl-paste`), `Esc` returns to the
  gloss overlay. `Ctrl+V` cycles voice; `v` opens the voice picker.
