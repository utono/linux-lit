# Ctrl+/ echo keybinds overlay

**Date:** 2026-05-31
**Status:** Approved design, pending implementation plan

## Problem

`Ctrl+/` opens the reader's keybinds overlay (a Cairo physical-keyboard map). It
only works in Reader mode (dispatched via the keymap lookup), and its content is
reader-specific. When the echoes overlay is open, the user wants `Ctrl+/` to show
a legend of the **echoes-overlay** keybinds, dismissible back to the echoes
overlay.

## Decisions (from brainstorming)

- **Style:** a simple legend list (key — action pairs), NOT the keyboard-map
  style. The echoes overlay has only ~12 binds.
- **Dismiss:** `Esc` OR `Ctrl+/` (toggle) closes it and returns to the echoes
  overlay in the same state.

## Current behavior (verified in source)

- `Ctrl+/` = `KeyCombo::ctrl("slash")` → `Action::OpenKeybindsOverlay`
  (`src/input/keymap_config.rs:333`), dispatched via the keymap lookup in
  `handle_key` — which only runs in Reader mode. In the echoes overlay, keys go
  to `handle_echoes_overlay_key` (`src/input/keymap.rs`), which currently has no
  `slash` arm; its `is_ctrl` block handles `Ctrl+Up`/`Ctrl+Down` (volume).
- The reader's `KeybindsOverlay` (`src/ui/keybinds_overlay.rs`) is a Cairo
  `DrawingArea` rendering keyboard-row screens — NOT reused here.
- `concordance_works_picker` (`src/ui/concordance_works_picker.rs`) is the model
  for an overlay panel: a struct exposing `pub container: GtkBox` + `pub scrim:
  GtkBox`, both hidden by default, added via
  `authorship_picker.overlay.add_overlay(&…)` in `app.rs` — NOT inserted into the
  reader's size-bearing widget chain. (Inserting into that chain orphans the
  reader content and collapses the layout; see the picker-overlay rule.)
- The echoes overlay key handler `handle_echoes_overlay_key` is reached from the
  `InputMode::EchoesOverlay` arm of `handle_key`'s mode-dispatch match
  (`keymap.rs:77`).

## Design

### New widget: `EchoKeybindsOverlay` (`src/ui/echo_keybinds_overlay.rs`)

A static legend panel, modeled on `concordance_works_picker`:

- `pub scrim: gtk4::Box` — full-area dimming backdrop, css class `gloss-scrim`
  (reuse the existing scrim style), hidden by default.
- `pub container: gtk4::Box` — vertical legend panel, css class `picker-box`,
  `halign Center`, `valign Center`, hidden by default.
- Contents (built once in `new`):
  - A title `Label` "Echo keybinds" (css `gloss-title` or `picker-item-title`).
  - One row per bind: a horizontal `Box` with a key `Label` (left, css
    `picker-item-title` or a dedicated key style) and an action `Label`.
  - Static bind list (compiled-in) — matches `handle_echoes_overlay_key` exactly
    (verified against source):
    - `a` — play echo
    - `A` — add echo
    - `n` / `p` — next / prev echo (select + play)
    - `↑` / `↓` — reorder (curate)
    - `g g` / `G` — first / last echo
    - `j` / `k` — scroll list
    - `Tab` — play source turn
    - `Enter` — open echo's work
    - `c` — copy echo
    - `s` — toggle curate
    - `R` — refresh echoes
    - `Ctrl+↑` / `Ctrl+↓` — volume
    - `Esc` — close overlay
- Methods:
  - `new() -> Self` — build scrim + container + rows.
  - `attach_to(&self, overlay: &gtk4::Overlay)` — `overlay.add_overlay(&self.scrim);
    overlay.add_overlay(&self.container);` (NOT a chain link).
  - `show(&self)` — `scrim.set_visible(true); container.set_visible(true);`
  - `hide(&self)` — `scrim.set_visible(false); container.set_visible(false);`
  - `is_visible(&self) -> bool`

### AppState + wiring (`src/app.rs`)

- New field `pub echo_keybinds_overlay: crate::ui::echo_keybinds_overlay::EchoKeybindsOverlay,`.
- New `InputMode::EchoKeybindsOverlay` variant.
- Construct it and `attach_to(&authorship_picker.overlay)` alongside the other
  `add_overlay` panels (next to `echo_line_picker` / `concordance_works_picker`).
- Add `echo_keybinds_overlay` to the constructor field list.
- Register the module in `src/ui/mod.rs`.

### Keymap wiring (`src/input/keymap.rs`)

- **Open:** in `handle_echoes_overlay_key`, inside its `is_ctrl` block (where
  `Ctrl+Up`/`Ctrl+Down` volume is handled), add a `"slash"` case:
  ```rust
  "slash" => {
      let mut s = state.borrow_mut();
      s.echo_keybinds_overlay.show();
      s.input_mode = crate::app::InputMode::EchoKeybindsOverlay;
      return true;
  }
  ```
- **Dispatch:** add `crate::app::InputMode::EchoKeybindsOverlay =>
  handle_echo_keybinds_key(state, key_name, is_ctrl),` to the mode match in
  `handle_key`.
- **Handler:** new `fn handle_echo_keybinds_key(state, key_name, is_ctrl) -> bool`:
  ```rust
  // Esc or Ctrl+/ closes, returning to the echoes overlay.
  if key_name == "Escape" || (is_ctrl && key_name == "slash") {
      let mut s = state.borrow_mut();
      s.echo_keybinds_overlay.hide();
      s.input_mode = crate::app::InputMode::EchoesOverlay;
  }
  true // consume all keys while the legend is up (modal)
  ```

### Behavior

The echoes overlay (gloss_overlay) stays visible underneath; the scrim dims it.
Closing the legend only hides it and restores `EchoesOverlay` mode — the echoes
state (selected echo, scroll position, links) is untouched because nothing in the
legend flow mutates it.

## Out of scope

- The reader's `Ctrl+/` keybinds overlay (unchanged).
- Any dynamic/keymap-driven content — the echo legend is static (the echoes binds
  are fixed in `handle_echoes_overlay_key`).
- Reusing the Cairo keyboard-map rendering.

## Testing

- Manual (`cargo run`): open the echoes overlay (`i` / Visual `i`), press `Ctrl+/`
  → the legend appears over a dimmed echoes overlay; `Esc` and `Ctrl+/` each close
  it back to the echoes overlay with the same selected echo; other echo keys do
  nothing while the legend is up; the reader's `Ctrl+/` (from normal reading) is
  unaffected.
- `cargo build` + `cargo clippy` clean; tests show only the 2 known pre-existing
  `block_atom_tests` failures.

## Open items for the plan

- Exact CSS classes for the legend rows (reuse `picker-box` / `picker-item-title`
  / `gloss-scrim`, or add a small dedicated style). Implementer's call — reuse
  existing classes for consistency unless they clash.
- Row layout detail (two-column key/action alignment) — a simple two-Label
  horizontal Box per row with the key Label given a fixed min-width for alignment.
