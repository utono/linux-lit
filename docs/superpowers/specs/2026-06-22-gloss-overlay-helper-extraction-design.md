# Extract gloss_overlay pure helpers into focused modules

**Date:** 2026-06-22
**Status:** Design approved, pending spec review
**Scope class:** Safe-scope (behavior-preserving code motion). This is the one
"larger project" item from `docs/superpowers/audit-opportunities.md` that the
audit flagged as *possibly* safe — this design keeps it strictly safe by
excluding the GTK-touching buffer-population code.

## Problem

`src/ui/gloss_overlay.rs` is 3,606 lines — by far the largest file in
`src/ui/` (the next largest is `keybinds_overlay.rs` at 1,277). About 1,125
lines of that are pure, self-contained helper functions (text/block parsing,
OP-IPA markup processing, geometry/color math, citation formatting) plus ~750
lines of their unit tests. These have nothing to do with the `GlossOverlay`
widget's GTK lifecycle; they are pure functions that happen to live in the
same file. Several are already part of the crate's public-ish surface,
imported by `src/input/actions/gloss.rs` and `src/input/actions/settings.rs`.

Extracting them shrinks `gloss_overlay.rs` to ~1,730 lines (the `GlossOverlay`
struct + its single large impl block + the GTK buffer-population code that
genuinely belongs with the widget), and gives the pure helpers honest,
nameable module boundaries.

## Goals

- Move ~1,125 lines of pure helpers + ~750 lines of their tests out of
  `gloss_overlay.rs` into three new sibling modules.
- **Behavior-preserving.** Pure code motion. No logic changes, no signature
  changes beyond visibility adjustments needed for cross-module calls.
- Honest module names — call sites are updated to the new paths, not hidden
  behind a re-export facade.

## Non-goals

- Do **not** touch `populate_gloss_buffer` / `populate_gloss_buffer_ex`,
  `apply_bracket_styling`, or `line_is_speaker`. These drive `gtk4::TextView`
  / `&gtk4::TextBuffer` directly and are woven into the overlay's rendering —
  the audit flagged the buffer-population path as behavior-risky and out of
  safe-scope. They stay in `gloss_overlay.rs`.
- No `AppState` changes, no `app.rs` changes. This project is scoped to
  `src/ui/` plus the two `input/actions/` call-site files.
- No new behavior, no API redesign, no consolidation of the helpers'
  algorithms.

## The three new modules

All under `src/ui/`. Each is a flat module of pure functions + its own
`#[cfg(test)]` test module(s).

### 1. `src/ui/gloss_block.rs` — block model & text parsing

The "parse gloss/synopsis text into typed blocks" unit. Largest cluster,
widest external surface.

- **Types:** `GlossBlock` (`pub`), `BlockKind` (`pub`), `GlossElement` (private)
- **Functions:**
  - `gloss_blocks` (`pub`)
  - `synopsis_blocks` (`pub`)
  - `visual_block_range` (`pub`)
  - `selected_blocks_text` (`pub`)
  - `render_synopsis_with_labels` (`pub`)
  - `parse_gloss_tags` (private)
  - `carry_forward_block_speakers` (private)
  - `try_extract` (private)
  - `is_label_paragraph` (private)
- **Tests moved whole:** `block_tests`, `synopsis_blocks_tests`,
  `visual_range_tests`
- **Tests moved by split:** the synopsis-label assertions from the mislabeled
  `synopsis_label_tests` module (see "Test-module split" below)
- **External API used today:** `GlossBlock`, `BlockKind`, `gloss_blocks`,
  `synopsis_blocks` (imported by `gloss.rs` and `settings.rs`)

### 2. `src/ui/gloss_ipa.rs` — OP-IPA / bracket markup processing

The OP-IPA lexical-set markup path (the text linux-lit sends to ElevenLabs and
strips for display). Pure string transforms.

- **Functions:**
  - `ipa_for_tts` (`pub(crate)`)
  - `contains_ipa_span` (`pub(crate)`)
  - `replace_word_ipa` (`pub(crate)`)
  - `replace_word_ipa_in_source_block` (`pub(crate)`)
  - `strip_ipa` (private or `pub(super)` as callers require)
  - `strip_brackets` (private)
  - `normalize_ipa_whitespace` (private)
  - `is_ipa_span` (private)
  - `opener_on_boundary` (private)
- **Tests moved by split:** the IPA assertions from the mislabeled
  `synopsis_label_tests` module (see below) — `strip_ipa_*`, `ipa_for_tts_*`,
  `contains_ipa_span_*`, `replace_word_ipa*` tests
- **External API used today:** `ipa_for_tts`, `contains_ipa_span`,
  `replace_word_ipa_in_source_block` (imported by `gloss.rs`)

### 3. `src/ui/gloss_util.rs` — geometry, color, citation

Small impl-only helpers with no external callers. Folded together rather than
split into two ~150-line files that would not earn the indirection.

- **Types:** `CursorScrollGeom`
- **Functions:** `cursor_scroll_target`, `snap_up_to_row`, `parse_hex_color`,
  `build_diff_markup`, `split_echo`, `parse_citation`, `format_citation_range`
- **Visibility:** `pub(super)` (only `gloss_overlay`'s impl calls them)
- **Tests moved whole:** `snap_up_tests`, `cursor_scroll_tests`,
  `citation_range_tests`

## What stays in `gloss_overlay.rs`

- `struct GlossOverlay` + its full impl block (lines ~97–1730)
- `populate_gloss_buffer`, `populate_gloss_buffer_ex` (drive `gtk4::TextView`)
- `apply_bracket_styling`, `line_is_speaker` (take `&gtk4::TextBuffer`)
- Private structs `BarRange`, `LineNumber`, `BlockRange` (returned/consumed by
  the buffer-population functions)
- The font constants `GLOSS_DEFAULT_FONT_FAMILY`, `GLOSS_DEFAULT_FONT_SIZE`
  (used by the impl)

After extraction the file is ~1,730 lines: the widget and its rendering code.

## Test-module split (the one non-trivial step)

The module named `synopsis_label_tests` (current lines ~3148–3430) is
**mislabeled** — it contains both synopsis-label tests *and* a large block of
IPA tests (`strip_ipa_*`, `ipa_for_tts_*`, `contains_ipa_span_*`,
`replace_word_ipa*`, current lines ~3181–3424). When extracting:

- synopsis-label / `render_synopsis_with_labels` assertions → `gloss_block.rs`
  (merge into or alongside its test module)
- IPA assertions → `gloss_ipa.rs` test module

Every other test module moves whole to the module that owns the functions it
exercises. No test assertions change; they only move and re-import.

## Mechanics

1. Create the three new files. Move the listed items into them verbatim.
2. For each moved item the impl still calls, give it the minimum visibility
   the call requires (`pub(super)` for impl-only; keep `pub`/`pub(crate)` for
   the externally-used ones) and add `use crate::ui::gloss_block::...` /
   `gloss_ipa::...` / `gloss_util::...` imports at the top of
   `gloss_overlay.rs`.
3. Add three `pub mod gloss_block;`, `pub mod gloss_ipa;`, `pub mod
   gloss_util;` lines to `src/ui/mod.rs`.
4. Update the external call sites (decision: update paths, no facade):
   - `src/input/actions/gloss.rs` — change `crate::ui::gloss_overlay::{gloss_blocks, synopsis_blocks, ipa_for_tts, contains_ipa_span, replace_word_ipa_in_source_block, BlockKind}` references to `crate::ui::gloss_block::...` / `crate::ui::gloss_ipa::...`
   - `src/input/actions/settings.rs` — change `crate::ui::gloss_overlay::BlockKind` to `crate::ui::gloss_block::BlockKind`
5. Remove any now-unused `use` imports left behind in `gloss_overlay.rs`.

## Verification

Pure code motion of pure functions — no rendering path changes, so no e2e/cage
run is needed (per CLAUDE.md, e2e is for "renders correctly on screen" changes;
this is "the logic is unchanged and still compiles/tests").

- `cargo build` — clean
- `cargo test --bins` — every moved test passes unchanged (this is the real
  proof the motion preserved behavior)
- `cargo clippy` — no new warnings (watch for now-unused imports / visibility
  lints)

## Risks & mitigations

- **Visibility too narrow → build break.** Mitigated by `cargo build`; bump
  `pub(super)` → `pub(crate)` only where a cross-module call demands it.
- **Test-module split drops a test.** Mitigated by comparing the test count
  before/after: `cargo test --bins 2>&1 | rg 'test result'` — the total test
  count must be identical pre- and post-extraction.
- **A helper turns out not to be pure** (touches a private struct that stays
  behind). Mitigated by the coupling analysis already done: the four pure
  clusters were verified to not reference `BarRange`/`LineNumber`/`BlockRange`,
  which stay with the buffer-population code. If the compiler disagrees, that
  function is not safe-scope and stays in `gloss_overlay.rs`.

## Out of scope (explicitly deferred)

The audit's other "larger projects" remain untouched: the `AppState`
god-struct grouping and the broader `app.rs` module carve-up. This spec is
only the `gloss_overlay.rs` helper extraction.
