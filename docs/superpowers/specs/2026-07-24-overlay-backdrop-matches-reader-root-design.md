# Overlay backdrop matches reader root exactly

**Date:** 2026-07-24
**Status:** approved

## Problem

The gloss, journal, and translation overlays paint their full-bleed backdrop
(the area around the overlay card) with `scrim_bg`, which is
`darken_color(root_color, 0.80)` — about 20% darker than the reader's plain
window backdrop (`window { background-color: {root} }`). That intentional
"dimmed frame" reads as a root-color mismatch: the overlay backdrop does not
match the reader's wallpaper.

Requirement: the card overlays' backdrop should always equal the reader's
**live** root color, including after a root-variant cycle (`Ctrl+$`).

## Change

Single-function change in `src/theme.rs`:

- `scrim_bg(theme)` currently returns `darken_color(&theme.root_color, 0.80)`.
  Change it to return `theme.root_color.clone()` — the scrim becomes the live
  root, identical to the reader window background.

No per-overlay code changes are needed. The `.gloss-scrim` CSS class already
interpolates `{scrim}` (= `theme.scrim_bg`), and `scrim_bg` is re-derived from
`theme.root_color` inside `resolve_theme_variant`; the CSS reloads on every
`apply_theme_to_state`. So the overlay backdrop tracks root-variant cycling in
lockstep with the reader automatically.

The scrim stays an **opaque solid color** (root instead of darkened-root), so
the main reading card behind the overlay remains fully hidden — no bleed-through.

## Scope

- **In scope:** the three card overlays that use `.gloss-scrim` —
  `src/ui/gloss_overlay.rs`, `src/ui/journal_overlay.rs`,
  `src/ui/translation_overlay.rs`. Gloss also backs synopsis and echo, so those
  inherit the change.
- **Out of scope:** the library/legend/settings pickers, which use the separate
  `build_picker_scrim` → `.library-picker-scrim` = `rgba(0, 0, 0, 0.3)` path.
  They keep their current translucent treatment (user's call).

## Tests

Two existing tests in `theme.rs` encode the OLD (darker-than-root) contract:

1. `scrim_bg_is_a_small_darkening_of_root` — asserts the scrim is a small
   darkening of root. This **inverts**: rewrite (and rename) it to assert
   `scrim_bg(&theme) == theme.root_color` for the sampled themes — the new
   contract.
2. The variant test asserting `v0.scrim_bg != v1.scrim_bg` stays green: the
   root still differs across variants, so a root-equal scrim still differs.

## Verification

Headless before/after screenshot of a gloss or journal overlay (via
`scripts/land-on.sh WORK div1.div2 gloss|journal`), pixel-comparing the
backdrop shade against the reader wallpaper to confirm they now match. Cage is
software rendering, so hand the user the same command for a final eyeball on
the real GL renderer if the shade is subtle.
