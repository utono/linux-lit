# Green theme ladder (green-light / green-lightest) — design

Date: 2026-07-10
Status: approved (brainstormed with user; see decisions inline)

## Problem

The sepia family has a validated three-step lightness ladder (kindle-sepia →
sepia-light → sepia-lightest). The green family has only kindle-green. Add the
matching two lighter steps so the green side of the cycle degrades the same
way.

## Decision summary (user-approved)

- Two new reader-only themes: `green-light`, `green-lightest`.
- Derivation: SAME recipe as the sepia ladder — apply the per-key lightness
  deltas sepia-light/sepia-lightest apply to kindle-sepia, but starting from
  kindle-green's palette (hue stays green). Hand-tuning deferred until after
  a first visual pass, if needed.
- Alt+t cycle: both inserted immediately AFTER kindle-green
  (…kindle-green, green-light, green-lightest…).
- Default theme unchanged (`sepia-lightest`).

## Details

### Palettes (themes repo, not linux-lit)

- File: `~/utono/themes/.config/themes/themes-unified.json`
  (kindle-green ~line 4165-region alongside the sepia entries).
- Karaoke highlight alphas mirror the ladder: 0.28 (kindle-green, unchanged)
  → 0.18 (green-light) → 0.14 (green-lightest) — highlight prominence
  degrades identically as the background lightens.

### Root-color variants (Ctrl+t)

Mirror the sepia pair's mechanism exactly: explicit
`dwl.rootcolor_candidates` (5, lightest → darkest) if sepia-light/lightest
carry them; otherwise rely on the existing computed lighter/darker fallback.
No new `apply_theme_to_state` call sites are added, so there is no
`root_variant_for` restoration hazard.

### Cycle wiring (linux-lit)

- Stored configs: edit `theme_cycle` in BOTH `~/.config/linux-lit/config.json`
  and `config-dev.json`, while NO instance is running (running instances
  rewrite their config on exit).
- Compiled default cycle in `src/config.rs`: insert the two names after
  kindle-green so fresh configs match. (Stored values override the compiled
  default; both must change.)

### Code changes

None in linux-lit beyond the `src/config.rs` default-cycle constant — the
theme loader (`src/theme.rs`) reads themes-unified.json generically, so the
new entries resolve without code.

## Verification

- Headless cage launch pinned to each new theme; screenshot both.
- Ctrl+t through the 5 root variants on each new theme (needs
  `LIT_NO_MPV=1` + `wlr-randr` 1920x1200 to expose the root field, per the
  root-variant-cycling notes).
- Final eyeball by the user on the live GL renderer via Alt+t.

## Out of scope

- Changing the default theme.
- Hand-tuned green palettes (only if the mirrored deltas look washed out in
  the visual pass).
- Any dark-side green variants.
