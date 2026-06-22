# Shared picker-attach helper — implementation plan

Audit opportunity #13. See
`docs/superpowers/specs/2026-06-22-picker-attach-helper-design.md`.

## Task 1 — add the helper

- New `src/ui/picker_attach.rs` with
  `pub(crate) fn attach_panel(&Overlay, base, Option<&Box> scrim, &Box panel)`.
- Register `pub mod picker_attach;` in `src/ui/mod.rs`.
- `cargo build`.

## Task 2 — the 7 plain pickers

bookmark, concordance_list, concordance_picker, concordance_word, gloss, journal,
media: replace the 3-line `attach` body with
`attach_panel(&self.overlay, base, None, &self.picker_box);`.

## Task 3 — variants

- echo_picker: `attach_panel(&self.overlay, base, Some(&self.scrim), &self.picker_box);`
- authorship_picker: `attach_panel(&self.overlay, base, None, &self.container);`
- library_picker: replace leading 4 lines only; keep the resize block.

## Guard — do NOT touch (EXCLUDED)

All `*_overlay.rs` attach/attach_to; library_picker's resize block; pickers with
no attach.

## Verify

`cargo build` + `cargo test --bins`. Grep: no remaining 3-line
`set_child(Some(base)); add_overlay(&self.picker_box); ...set_visible(false)`
picker body. Headless cage launch: open a picker (Ctrl+p library, etc.) over the
reader — if seat-blocked, ask the user to run the e2e.
