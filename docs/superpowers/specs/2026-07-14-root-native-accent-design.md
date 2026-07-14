# Root variants from theme-native accent candidates — design

**Date:** 2026-07-14
**Branch:** `feat/root-native-accent`
**Status:** approved, pending implementation plan

## Goal

Make linux-lit's root-color variants (cycled by Ctrl+`$` /
`RootVariantNext`/`Prev`) be the theme's own **native accent family** — the
`dwl.rootcolor_candidates` authored by the dwl-mlj tooling — used **verbatim**:
no computed white-blend/darken colors injected, no re-sorting.

## Background: how candidates are authored (dwl-mlj)

`rootcolor_candidates` in `~/utono/themes/.config/themes/themes-unified.json`
are not arbitrary. The dwl-mlj `dwl-wallpaper` skill (commit `dca1cbb`) builds
them as a **hue-locked OKLCH family** anchored on the theme's signature accent:
lightness stepped up/down with hue and chroma held, sorted, each candidate
given its own `rootcolor_borders`. The `align-theme-accent` skill (commit
`8cae238`) governs choosing that signature accent per theme (rose-pine-dawn
composition model) and proposes complementary wallpaper candidates.

So the candidates **already are** "a color accent native to the theme." The
tooling is the source of truth for their values and order.

## Problem

linux-lit's `root_variant_color` (`src/theme.rs`) does **not** honor that
native family. With a fixed `ROOT_VARIANT_COUNT = 5` it:

- takes only two candidates (`slot(0)`, `slot(1)`),
- injects two **computed** white-blends (50% / 70% toward white) plus the
  designed root,
- then **re-sorts** the pool by WCAG luminance.

Result: most of a theme's native candidates are discarded, and non-native
synthetic colors appear in the cycle. The root variants the user cycles are
therefore not purely the theme's native accent family.

## Survey (empirical)

Every theme in `themes-unified.json` that defines `dwl.rootcolor` also has a
non-empty `rootcolor_candidates`, and all nine themes in the current
`theme_cycle` have candidates (3 or 6). The no-candidates case is a defensive
edge (the built-in `default_theme()`, or a future theme), not a live one.

## Design

### Core change (`src/theme.rs`)

Replace `root_variant_color`'s pool-building + WCAG re-sort with a direct read
of `rootcolor_candidates`:

- The ordered variant list **is** `dwl.rootcolor_candidates`, in authored order.
- Candidates absent/empty → the list is `[dwl.rootcolor]` (single element).
- Index maps straight into the list; **index 0 = `candidates[0]`** (the
  launch / theme-switch default).
- No candidate-equals-root skipping, no `blend_colors`/`darken_color` in this
  path.

### Per-theme variant count

`ROOT_VARIANT_COUNT` (fixed `5`) is removed; the count becomes a **per-theme**
value = candidate list length (3, 6, or 1 for the fallback).

- Add `root_variant_count: u8` to the `Theme` struct, set in
  `resolve_theme_variant` from the candidate list length (min 1).
- `cycle_root_variant` (`src/input/actions/settings.rs`) reads
  `s.theme.root_variant_count` for wraparound instead of the constant, and the
  toast shows `Root [n/N]` with that N.

### Persistence & clamping

`config.root_variants` persists a saved index per theme. With per-theme counts
a saved index can exceed a theme's count (saved 5, theme now has 3 candidates).

- **Clamp at resolve time:** `resolve_theme_variant` sets
  `idx = if variant < count { variant } else { 0 }` — out-of-range **resets to
  0** (first candidate).
- `config.root_variant_for(name)` stops doing `% ROOT_VARIANT_COUNT` (the
  constant is gone); it returns the raw saved index and lets
  `resolve_theme_variant` clamp.

### No-candidates fallback

Candidates absent/empty → variant list `= [dwl.rootcolor]`, count 1. Ctrl+`$`
becomes a no-op (wraps to itself). No computed colors are ever produced. The
built-in `default_theme()` sets `root_variant: 0`, `root_variant_count: 1`.

## Data flow

**Resolution (read)** — `load_theme_with_fallback(name, variant)` →
`resolve_theme_variant(name, val, variant)`:

1. `candidates = dwl.rootcolor_candidates` (empty → `[dwl.rootcolor]`).
2. `count = candidates.len()` (min 1).
3. `idx = if variant < count { variant } else { 0 }`.
4. `root_color = candidates[idx]`; `scrim_bg = darken(root_color, 0.80)`
   (unchanged — scrim still derives from the active root).
5. `theme.root_variant = idx`; `theme.root_variant_count = count`.

**Cycle (write)** — `cycle_root_variant`:

- `count = s.theme.root_variant_count`.
- `next = forward ? (cur+1)%count : (cur+count-1)%count`.
- Persist `config.root_variants[name] = next`, re-resolve, apply, toast
  `Root [next+1/count]`.
- `count == 1` → no-op cycle (candidate-less themes).

**Unaffected** (verified this session): the card surface is byte-identical
across variants (only `root_color` + `scrim_bg` vary); `vocab_popup_ink`,
`reader_gloss`, cursor colors, and the toast background all derive from the
active `root_color`/`text_bg` and continue to track it. The Ctrl+`$`
clipboard-copy + screenshot flow is unchanged.

## Removed / changed symbols

- `pub const ROOT_VARIANT_COUNT: u8 = 5` — **removed**; replaced by
  `Theme.root_variant_count`.
- `root_variant_color` — **simplified** to the candidate lookup (no
  pool-building, blends, or re-sort).
- `config.root_variant_for` — **drops** `% ROOT_VARIANT_COUNT`.
- `default_theme()` — sets `root_variant_count: 1`.

## Testing

New tests (theme.rs `#[cfg(test)]`):

- `root_variants_are_the_candidates_verbatim` — fixture candidates `[a,b,c]`;
  assert `resolve_theme_variant(_, i).root_color == candidates[i]` for
  `i in 0..3`, in authored order (no re-sort).
- `root_variant_count_matches_candidate_list` — 6-candidate → count 6;
  3-candidate → 3.
- `out_of_range_variant_clamps_to_zero` — variant 5 on a 3-candidate theme →
  `candidates[0]`, `root_variant == 0`.
- `no_candidates_falls_back_to_designed_root` — fixture with `rootcolor`, no
  candidates → single variant = `dwl.rootcolor`, count 1.

Keep (retargeted to iterate `0..count`):

- `card_surface_is_identical_across_root_variants`.
- The vocab / reader-gloss contrast-across-variants invariants.

Delete (assert removed behavior):

- `all_five_roots_sorted_lightest_to_darkest_including_base`
- `computed_fallback_without_candidates`
- `short_candidate_list_fills_remaining_computed`

Verification: `cargo build` + `cargo test --bins`, then headless cage — launch
a 6-candidate theme (kindle-sepia), press Ctrl+`$` repeatedly, screenshot to
confirm the root walks the native candidate family and wraps at 6; confirm the
`Root [n/N]` toast shows the right N; confirm a 3-candidate theme wraps at 3.

## Risk

The contrast-across-variants invariants (vocab tint, reader-gloss tint
legibility) were previously guaranteed partly by the computed white-blends
staying light. Native candidates can be darker / more saturated (e.g. the
crimson `#8d1637` family). If a candidate defeats a contrast floor, that is a
**theme-authoring** issue — fix in dwl-mlj's candidate generation, not in
linux-lit — and the retained contrast tests will surface it.
