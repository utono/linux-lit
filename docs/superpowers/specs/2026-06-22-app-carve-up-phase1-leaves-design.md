# Carve three leaf modules out of app.rs (Phase 1)

**Date:** 2026-06-22
**Status:** Design approved, pending spec review
**Scope class:** Safe-scope (behavior-preserving code motion). This is Phase 1
of the `app.rs module carve-up` "larger project" from
`docs/superpowers/audit-opportunities.md`. The audit flagged the *whole*
carve-up as behavior-risky because its named targets (`build_window`,
`display_work`, layout) are heavy widget + signal-wiring code. This phase
deliberately avoids all of those, extracting only three self-contained function
families that are clean `Rc<RefCell<AppState>>`-in / widgets-out (or pure)
leaves — the same kind of motion proven safe by the `gloss_overlay.rs` split.

## Problem

`src/app.rs` is 6,735 lines — the single largest file in the crate by a wide
margin and the biggest drag on working in this codebase. It is not one tangled
blob: it is ~70 free functions clustered into topical families (text-prep,
formatting, translations, font, vocab-popup, scene/synopsis, gutter,
image/calibration) plus the monolithic `AppState` struct, `build_window`, and
`display_work_at_with_prepared`. Most of the topical families are already
`Rc<RefCell<AppState>>`-in / widgets-out and do not call each other — exactly
the shape that makes leaf-first code motion safe.

This phase extracts the three cleanest leaf families, shrinking `app.rs` by
~860 lines and giving each family an honest, nameable module boundary. The
heavy carve-up targets (`build_window`, `display_work`, layout) and the other
three tier-a families (scene/synopsis, translations, formatting) are left for
separate later specs.

## Goals

- Move three self-contained function families (~860 lines total) out of
  `app.rs` into three new sibling modules under a new `src/app/` directory.
- **Behavior-preserving.** Pure code motion. No logic changes, no signature
  changes beyond the two visibility bumps named below.
- Honest module names — call sites are repathed to the new paths, not hidden
  behind a re-export facade (matches the `gloss_overlay.rs` decision).

## Non-goals

- Do **not** touch `build_window`, `display_work_at_with_prepared`, the layout
  functions (`apply_tiled_mode`, `apply_column_layout`, `apply_card_sizing`),
  or any formatting / translation / scene-synopsis family. Those are later
  phases (formatting/translations/scene-synopsis are tier-a; build_window /
  display_work / layout are tier-b, behavior-risky).
- No `AppState` field changes, no grouping of the god-struct (that is its own
  parked "larger project").
- No new behavior, no API redesign, no consolidation of algorithms, no deletion
  of dead code (see the `build_line_map_for_prepared` note).

## Directory conversion

`app.rs` is currently a single file, so it has no sibling-module slot. Convert
it to a directory module: `src/app.rs` → `src/app/mod.rs`, and add the three new
files alongside it under `src/app/`. The moved items become `use`-imported back
into `mod.rs` (mirroring how `gloss_overlay.rs` imports its three siblings). No
change to `src/main.rs`'s `mod app;` declaration — a directory module resolves
the same way.

## The three new modules (extraction order: cleanest leaf first)

### 1. `src/app/vocab_popup.rs` — the cleanest leaf (extract first)

The vocab-popup widget family. Verified to have **zero** app.rs-private
dependencies (no shared structs/consts/free-fns), zero cross-group calls, and
**no reverse dependency** from `build_window` — it touches only `AppState`
fields and the `work_line_for_buffer` method.

- **Functions:**
  - `open_vocab_popup` (`pub`)
  - `close_vocab_popup` (`pub`)
  - `refresh_vocab_popup` (`pub`)
  - `vocab_popup_next` (`pub`)
  - `vocab_popup_prev` (`pub`)
  - `vocab_popup_toggle_view` (`pub`)
  - `show_vocab_popup` (`pub`, no external callers but used internally)
  - `update_vocab_popup_margin` (private — no external callers)
  - `format_etymology` (private — no external callers)
- **Visibility changes:** none.
- **External call sites to repath:**
  - `src/input/keymap.rs` — `open_vocab_popup` (:2057, :2337), `close_vocab_popup`
    (:2059), `vocab_popup_next` (:2332), `vocab_popup_prev` (:2334),
    `vocab_popup_toggle_view` (:2261)
  - `src/input/highlight.rs` — `open_vocab_popup` (:168), `refresh_vocab_popup` (:166)
  - `src/main.rs` — `refresh_vocab_popup` (:315)

### 2. `src/app/font.rs` — font / spacing family

Font-size and line-number-gutter rebuild family. No cross-group calls. One
reverse dependency forces one visibility bump.

- **Functions:**
  - `adjust_font_size` (`pub`)
  - `reset_font_size` (`pub`)
  - `cycle_font` (`pub`)
  - `show_font_info` (`pub`)
  - `reapply_font` (private → **`pub(crate)`**, see below)
  - `update_spacer_heights` (private — used only by `reapply_font`)
  - `rebuild_line_number_gutter` (private — used only by `adjust_font_size`/`reset_font_size`)
- **Visibility change:** `reapply_font` is currently private but is also called
  by `build_window` (app.rs:2452), which stays in `mod.rs`. It must become
  `pub(crate)` so `build_window` can still reach it across the module boundary.
  This is a visibility bump, not a logic change.
- **Deps that stay in `mod.rs` and are reached via `use`:**
  - `line_number_gutter_geometry` (already `pub(crate)`) — out-of-group helper
  - consts `TOP_SPACER_HEIGHT`, `SHOW_LINE_NUMBERS_TWO_COL` (already `pub`)
- **External call sites to repath:** `src/input/keymap.rs` only —
  `adjust_font_size` / `show_font_info` (:2111, :2112, :2193), `reset_font_size`
  (:2113), `cycle_font` (:2114, :2115).

### 3. `src/app/text_prep.rs` — pure text preparation (GTK-free)

The off-thread-safe text-prep family. Pure (no GTK), the most valuable to
isolate, but the most entangled with `build_window` — one shared private enum
forces one visibility bump.

- **Types:**
  - `PreparedTextOnly` (`pub`)
  - `PreparedText` (`pub`)
  - `SnapshotOrPrep` (private enum → **`pub(crate)`**, see below)
- **Functions:**
  - `prepare_text_for_display` (`pub`)
  - `prepare_text_only` (`pub`)
  - `build_line_map_for_prepared` (`pub`, dead — see note)
  - `clean_file_lines` (private — no external callers)
- **Visibility change:** `SnapshotOrPrep` is private but is constructed (via
  `prepare_text_only`) and pattern-matched by `build_window`, which stays in
  `mod.rs`. It must become `pub(crate)` and `build_window` repathed to it.
  `PreparedTextOnly` / `PreparedText` are already `pub` and also constructed by
  `build_window` — repathed via `use`, no visibility change.
- **Dead-code note:** `build_line_map_for_prepared` is `pub` but has zero
  callers anywhere (only a doc-comment reference). It moves with the group
  **as-is** — this is pure code motion; deleting it is out of scope and can be a
  trivial separate change later if desired.
- **External call sites to repath:**
  - `src/input/actions/pickers.rs` — `prepare_text_for_display` (:102, :281),
    `PreparedText` construction (:83, :262)
  - `src/input/actions/concordance.rs` — `prepare_text_for_display` (:402)
  - `src/input/actions/echoes.rs` — `prepare_text_for_display` (:1457)

## Module dependency note

The three new modules are independent leaves — none calls into another, and
none of the three calls any out-of-group `app.rs` free function (font reaches
`line_number_gutter_geometry`, which is a `pub(crate)` helper that stays in
`mod.rs` and is imported via `use`, not a peer module). The only edges are the
two reverse dependencies from `build_window`/`mod.rs` back into the moved code,
handled by the two `pub(crate)` visibility bumps:

- `vocab_popup` — leaf, no edges in or out (cleanest).
- `font` — leaf; `mod.rs::build_window` calls `font::reapply_font`
  (`pub(crate)`).
- `text_prep` — leaf; `mod.rs::build_window` constructs/matches
  `text_prep::SnapshotOrPrep` (`pub(crate)`) and `PreparedText*`.

## What stays in `src/app/mod.rs`

Everything else, unchanged — the `AppState` struct + its `impl`, all type
definitions (`InputMode`, `SearchMatch`, etc.), constants, the layout functions,
`build_window`, the whole `display_work*` chain, all formatting/translation/
gutter/scene-synopsis/image-calibration/title-bar families, and the existing
`#[cfg(test)]` modules. This phase removes only the three families above.

## Mechanics

1. `git mv src/app.rs src/app/mod.rs` (preserves history).
2. Create `src/app/vocab_popup.rs`, `src/app/font.rs`, `src/app/text_prep.rs`.
   Move the listed items into them verbatim.
3. Apply the two visibility bumps: `font::reapply_font` → `pub(crate)`;
   `text_prep::SnapshotOrPrep` → `pub(crate)`.
4. Add `mod vocab_popup;`, `mod font;`, `mod text_prep;` to `mod.rs`, and add
   `use` imports inside `mod.rs` for the moved items its retained code still
   calls (e.g. `build_window` → `text_prep::SnapshotOrPrep`,
   `font::reapply_font`). These are internal imports for code that stays in the
   `app` module — **not** a re-export facade; nothing is re-`pub`-exported from
   `mod.rs`, and external crates' call sites are repathed directly to
   `crate::app::vocab_popup::*` etc. in step 5. Prefer explicit item imports
   over globs to keep the boundary legible. Add the `use crate::app::...`
   imports the new modules need for the helpers/consts that stay in `mod.rs`
   (`line_number_gutter_geometry`, `TOP_SPACER_HEIGHT`,
   `SHOW_LINE_NUMBERS_TWO_COL`).
5. Repath the external call sites listed per module (no facade) —
   `keymap.rs`, `highlight.rs`, `main.rs`, `pickers.rs`, `concordance.rs`,
   `echoes.rs`.
6. Remove any now-unused `use` imports left behind in `mod.rs`.

Each module is one independently-mergeable PR; ship in order
(`vocab_popup` → `font` → `text_prep`).

## Verification

Pure code motion of functions whose logic is unchanged — no rendering path
changes, so no e2e/cage run is needed (per CLAUDE.md, e2e is for "renders
correctly on screen" changes; this is "the logic is unchanged and still
compiles/tests").

- `cargo build` — clean
- `cargo test --bins` — every test passes unchanged; the total count must stay
  **413** before and after (this is the real proof the motion preserved
  behavior): `cargo test --bins 2>&1 | rg 'test result'`
- `cargo clippy` — no new warnings (watch for now-unused imports / visibility
  lints)

## Risks & mitigations

- **Visibility too narrow → build break.** Mitigated by `cargo build`; the two
  required bumps (`reapply_font`, `SnapshotOrPrep` → `pub(crate)`) are named
  above. Bump further only if the compiler demands a cross-module call we
  missed.
- **A "leaf" turns out to have a hidden edge** (calls another moved family or an
  out-of-group helper that also needs moving). Mitigated by the dependency
  analysis already done: all three families were verified to have zero
  cross-group calls and zero out-of-group app.rs free-fn calls except
  `font`→`line_number_gutter_geometry` (a `pub(crate)` helper that stays put).
  If the compiler disagrees, that function is not safe-scope for this phase and
  stays in `mod.rs`.
- **Directory conversion loses git history.** Mitigated by `git mv` for the
  `app.rs` → `app/mod.rs` rename.

## Out of scope (explicitly deferred)

- **Tier-a, later phases:** the `scene_synopsis`, `translations`, and
  `formatting` families — same safe code-motion style, separate specs.
- **Tier-b, behavior-risky:** the `build_window`, `display_work`, and layout
  extractions named in the audit ledger — a separate, e2e-verified effort.
- **The `AppState` god-struct grouping** — its own parked "larger project."
- Deleting the dead `build_line_map_for_prepared` — moves as-is this phase.
