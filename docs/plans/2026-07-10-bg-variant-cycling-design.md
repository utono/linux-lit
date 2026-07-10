# Ctrl+t Root-Color-Variant Cycling — Design

Date: 2026-07-10
Status: approved (brainstormed with user; PIVOTED same day — see below)

## Pivot note (2026-07-10)

The first shipped version (merged 2084486) varied the **main card's
background**. That mischaracterized the request: the user wants variants for
the **root color** — the field OUTSIDE the main card (`theme.root_color`,
sourced from `dwl.rootcolor`) — and explicitly NO variants for the card
background. This doc describes the corrected feature; the refit reuses the
shipped machinery (per-theme persisted index, Ctrl+t action, restore on
every theme-apply path, overlay docs) and changes only what varies.

## Problem

For every theme, Ctrl+t cycles three root colors — each a variant of the
theme's root-color family. The main card's background, text colors, and all
reading-surface tints (cursor line, karaoke) are untouched by the cycle.

## Decisions (from brainstorming)

- **Every theme has 5 variants** (grown from 3 on 2026-07-10 — user: "all
  of the themes need two more variants that are brighter").
- **Variant 0 is always the theme's designed `dwl.rootcolor`** — resolution
  byte-identical to no-variant behavior.
- **The four alternates (slots 1–4) are ordered LIGHTEST to DARKEST**
  (2026-07-10, user request) — Ctrl+t walks a predictable brightness
  ladder after the designed color. The pool before sorting:
  - Two from `dwl.rootcolor_candidates` where present (e.g. kindle-sepia's
    `#41819b`, `#286983`; entries equal to the designed rootcolor are
    skipped). Themes without (enough) candidates fill computed: root
    blended 25% toward `#ffffff`, and root darkened via
    `darken_color(root, 0.7)`.
  - Two brighter computed steps for every theme: root blended 50% and 70%
    toward `#ffffff`.
  Sorting is by WCAG relative luminance, descending.
- The chosen variant **persists per theme** in the app config
  (`root_variants: {theme_name: 0|1|2}`), ours-wins on merge like `theme`.

## What re-derives, what is pinned

- **Varies:** `root_color`; `scrim_bg` (derived from root) re-derives; the
  journal overlay bar color follows via `apply_theme_to_state`.
- **Pinned (never varies):** `text_bg`, `text_fg`, `cursor_line_bg`,
  `phrase_highlight_bg` (karaoke), `dim_fg`, `sign_fg`, `cursor_bg/fg`,
  `vocab_fg`, `reader_gloss*`, `overlay_panel_bg`, `focus_color`.
  (`rootcolor_borders`-driven focuscolor coupling is out of scope — the
  reader keeps the theme's single designed `focuscolor`.)

## Keybind and dispatch

- `Action::RootVariantNext` on **Ctrl+t** — cycles 0→1→2→0 — in BOTH
  `src/input/keymap_config.rs` and the stowed
  `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`.
- Handler `cycle_root_variant` in `src/input/actions/settings.rs`: insert
  the new index into `config.root_variants` BEFORE `apply_theme_to_state`
  (which saves config); toast title `Root [n/3]`, body = resolved
  `root_color` hex.
- Every theme-apply path restores the target theme's saved index via
  `config.root_variant_for(name)`: Alt+t cycle, startup, SIGUSR1 reload
  (which also adopts `root_variants` from disk), settings-overlay change
  arm, and settings-overlay Escape revert.

## Data

- kindle-sepia already carries `rootcolor_candidates` — no seeding needed.
- sepia-light / sepia-lightest have no candidates → computed teal
  variants (user can author candidates later).
- The obsolete `sepia-lightest."linux-lit".bg_variants` key is removed from
  themes-unified.json.

## Testing

- Unit (theme.rs): variant-0 identity; candidates used in order with the
  designed root skipped; short/absent candidate lists fall back computed
  per-slot; `text_bg` + both tints byte-identical across all variants;
  modulo wrap.
- Config: `root_variants` roundtrip + wrap-on-corrupt.
- Headless cage acceptance: the outer field changes color across three
  Ctrl+t presses while the card stays cream; wrap on the fourth.

## Out of scope

- `rootcolor_borders` focus/border coupling (dwl-side concern).
- Reverse-cycling bind.
- The retired card-bg variant behavior (removed, not kept behind a flag).
