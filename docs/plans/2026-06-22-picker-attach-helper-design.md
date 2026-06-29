# Shared picker-attach helper — design

## Goal

Remove the duplicated `attach()` body that overlay-panel pickers repeat
(set the base as the overlay child, add the panel as an overlay, hide it), via one
free helper — with **zero behavior change**, preserving the scrim and field-name
differences at each site. Audit opportunity #13, scoped to the picker `attach`
shape (NOT the structurally-different `*_overlay` attach/attach_to).

## The duplication

10 picker `attach(&self, base: &impl IsA<Widget>)` methods. They fall into:

- **Plain (7 sites — byte-identical 3 lines):** bookmark, concordance_list,
  concordance_picker, concordance_word, gloss, journal, media.
  ```rust
  self.overlay.set_child(Some(base));
  self.overlay.add_overlay(&self.picker_box);
  self.picker_box.set_visible(false);
  ```
- **Scrim (1 clean site):** echo_picker — inserts `add_overlay(&self.scrim)`
  before the picker_box overlay, then the same hide.
- **Scrim + extra (1 site):** library_picker — same scrim+picker_box+hide leading
  4 lines, THEN a responsive-resize tick block. Only the leading 4 lines match.
- **Field-name variant (1 site):** authorship_picker — same 3-line shape but the
  panel field is `container`, not `picker_box`.

All `overlay: gtk4::Overlay`, all panels/scrims are `gtk4::Box`.

## Component

A `pub mod picker_attach;` at `src/ui/picker_attach.rs` (registered in
`src/ui/mod.rs`), mirroring `picker_filter` (#10) / `picker_nav` (#6). Pure GTK:

```rust
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Overlay, Widget};

/// Attach an overlay-panel picker: set `base` as the overlay's child, add the
/// optional `scrim` then `panel` as overlays, and hide `panel`. The shared body
/// of every picker's `attach`; callers that need extra setup (e.g. responsive
/// resize) call this first, then do their own work.
pub(crate) fn attach_panel(
    overlay: &Overlay,
    base: &impl IsA<Widget>,
    scrim: Option<&GtkBox>,
    panel: &GtkBox,
) {
    overlay.set_child(Some(base));
    if let Some(scrim) = scrim {
        overlay.add_overlay(scrim);
    }
    overlay.add_overlay(panel);
    panel.set_visible(false);
}
```

## Call-site changes

- **7 plain pickers:**
  `crate::ui::picker_attach::attach_panel(&self.overlay, base, None, &self.picker_box);`
- **echo_picker:**
  `attach_panel(&self.overlay, base, Some(&self.scrim), &self.picker_box);`
- **authorship_picker:**
  `attach_panel(&self.overlay, base, None, &self.container);`
- **library_picker:** replace its leading 4 lines with
  `attach_panel(&self.overlay, base, Some(&self.scrim), &self.picker_box);`
  then keep the responsive-resize block unchanged.

## Explicitly EXCLUDED (structurally different — leave untouched)

- **All `*_overlay.rs` `attach`/`attach_to`** (gloss_overlay, journal_overlay,
  translation_overlay, settings_overlay, gamepad_overlay, keybinds_overlay,
  echo_keybinds_overlay, page_image_overlay, vocab_popup) — different signatures
  (`child`, or `attach_to(&Overlay)`) and bodies; not the picker_box shape.
- **library_picker's responsive-resize block** — stays inline after the helper call.
- Pickers with no `attach` (voice, echo_line, echo_turns, concordance_works) —
  nothing to change.

## Verification

Pure widget-construction extraction; no control-flow change. `cargo build` +
`cargo test --bins`. A headless launch (cage) confirms pickers still open over the
reader — the overlay wiring is what `attach` sets up, so a render smoke is the
right check. Ask the user to run the e2e if the agent's cage is seat-blocked.
