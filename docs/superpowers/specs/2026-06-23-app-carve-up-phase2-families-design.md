# Carve the three tier-a families out of app.rs (Phase 2)

**Date:** 2026-06-23
**Status:** Design approved, pending spec review
**Scope class:** Safe-scope (behavior-preserving code motion). Phase 2 of the
`app.rs module carve-up` (`docs/superpowers/audit-opportunities.md`). Phase 1
(merge 1bd1df3) extracted the three cleanest leaves (vocab_popup, font,
text_prep) and converted `app.rs` → `src/app/mod.rs`. Phase 2 finishes the
**tier-a** (safe, behavior-preserving) carve-up by extracting the remaining
three topical families. The behavior-risky **tier-b** targets (`build_window`,
`display_work`, layout) stay parked for a separate, e2e-verified effort.

## Problem

After Phase 1, `src/app/mod.rs` is 6,105 lines. Three more cohesive topical
families still live in it, each a `&AppState`/`&mut AppState`-in, widgets-out
(or pure) cluster with well-understood boundaries:

- **formatting** (~560 lines) — the per-line typographers that tag the reader
  buffer (dialogue/verse indent, BCP liturgical layout, scansion marks, stanza
  centering, authorship italics).
- **scene/synopsis** (~790 lines) — scene-boundary derivation, synopsis keys/
  labels, the synopsis sidebar + overlay, scene title-bar updates.
- **translations** (~520 lines) — the inline-gloss interleave path and the
  two-column translation overlay.

Extracting all three shrinks `mod.rs` by ~1,870 lines (to ~4,200) and gives each
family an honest module boundary, matching the Phase 1 pattern.

## Goals

- Move the three families into three new sibling modules under `src/app/`, via
  pure code motion.
- **Behavior-preserving.** No logic edits. The only signature-level changes are
  the named visibility bumps below (all `pub(crate)`/`pub(super)`, never bare
  `pub` for an internal item).
- No re-export facade — external call sites repathed directly to
  `crate::app::<module>::<fn>`.

## Non-goals

- Do **not** touch `build_window`, `display_work_at_with_prepared`, or the
  layout functions — they stay in `mod.rs` (tier-b, separate effort).
- No `AppState` field changes; the god-struct grouping stays parked.
- No consolidation of algorithms, no new behavior, no dead-code deletion.

## Extraction order (dependency-driven)

The cross-family dependency graph (verified by inventory) dictates the order:

- **formatting** — zero cross-family edges in either direction. Fully
  independent. Extract **first**.
- **scene/synopsis** — depends on nothing in the other two; is depended *upon*
  by translations' overlay cluster. Extract **second** so its `pub` helpers
  exist before translations needs them.
- **translations** — its overlay cluster calls scene/synopsis
  (`current_scene_divs`, `synopsis_label`). Extract **last**.

Each family is one task = one PR. Order: formatting → scene_synopsis →
translations.

## Module 1: `src/app/formatting.rs` (~560 lines)

The per-line reader-buffer typographers.

- **Functions moved:**
  - `apply_dialogue_formatting` (`pub` → **`pub(crate)`**) — reverse-called by
    `build_window` and `display_work_at_with_prepared` in `mod.rs`.
  - `apply_authorship_formatting` (`pub` → **`pub(crate)`**) — reverse-called by
    `display_work_at_with_prepared`.
  - `apply_scansion_marks` (private → **`pub(crate)`**) — reverse-called by
    `rebuild_buffer_text` (`pub(crate)`, stays in `mod.rs`).
  - `apply_bcp_formatting` (`pub` → **`pub(crate)`**) — no external callers;
    called only by `apply_dialogue_formatting` (in-family). Narrowed from the
    currently over-broad `pub`.
  - `apply_stanza_number_centering` (private — stays private; in-family only).
  - `char_offset` (private — stays private; pure byte→char helper, called only
    inside `apply_bcp_formatting`).
- **Deps that stay in `mod.rs`, reached via `use super::`:** consts
  `DIALOGUE_INDENT`, `TWO_COLUMN_DIALOGUE_INDENT`, `BCP_SENTENCE_GAP` (all
  already `pub const`; `DIALOGUE_INDENT`/`TWO_COLUMN_DIALOGUE_INDENT` are also
  used by `setup_gutter`, which stays in `mod.rs`, so they stay put — import,
  don't move). `AppState` and its methods `one_section_per_page`,
  `column_count`, `work_line_for_buffer`.
- **External (already `crate::`-pathed, no change):** `crate::db::line_types::*`,
  `crate::scansion::{mark_line, ScanLevel, LineScansion}`,
  `crate::text_file_map::LineMap`, `crate::db::models::Line`.
- **External call sites to repath** (`crate::app::X` → `crate::app::formatting::X`):
  - `apply_dialogue_formatting` — `src/input/actions/settings.rs` (×3: lines
    40, 329, 447)
  - `apply_authorship_formatting` — `src/input/keymap.rs:2299`,
    `src/input/actions/authorship.rs:31`
- **Excluded (out of family — leave in `mod.rs`):** `apply_vocab_highlighting`,
  `apply_reader_gloss_highlighting` (+ `apply_reader_gloss_tag_to_line`),
  `apply_ab_dim`/`remove_ab_dim`, `setup_gutter`. These are separate subsystems
  (vocab, gloss, AB-repeat, gutter) that merely sit nearby; `setup_gutter`
  shares the `dialogue-indent` consts/tag but is a gutter concern, so the consts
  stay in `mod.rs` and formatting imports them.

## Module 2: `src/app/scene_synopsis.rs` (~790 lines)

Scene-boundary derivation, synopsis keys/labels/overlay, scene title bar.

- **Functions moved** (free functions, no `AppState` methods): `is_chapter_work`,
  `current_chapter_number`, `chapter_number_from_flags`, `current_synopsis_key`,
  `whole_work_label`, `synopsis_label`, `current_scene_divs`,
  `divs_at_buffer_line`, `scene_text_for`, `is_first_line_of_scene`,
  `scene_heading_start`, `show_synopsis`, `toggle_synopsis`,
  `show_synopsis_overlay`, `scene_label`, `scene_label_for`, `prepend_whole_work`,
  `ordered_synopsis_scenes`, `clamp_synopsis_index`, `cycle_synopsis`,
  `update_title_bar_scene`.
- **Visibility:**
  - `scene_heading_start` (private → **`pub(crate)`**) — reverse-called by
    `display_work_at_with_prepared` in `mod.rs` (alongside `is_first_line_of_scene`,
    which is already `pub`).
  - All other currently-`pub` fns keep `pub` (they have external callers).
  - The currently-private helpers `whole_work_label`, `prepend_whole_work`,
    `ordered_synopsis_scenes`, `clamp_synopsis_index` stay private (in-family
    only).
- **Const:** move `SYNOPSIS_WHOLE_WORK` (`pub(crate)`) **with** the cluster
  (used only by this family). **Leave `JOURNAL_WORK_DIV` in `mod.rs`** — it sits
  in the same source region but is journal-owned (external caller
  `src/input/actions/journal.rs:271`), not a scene/synopsis const.
- **Cross-module visibility bump (required):** `update_vocab_popup_margin` in
  `src/app/vocab_popup.rs` is currently `pub(super)` and is called by
  `show_synopsis`. `pub(super)` on an item in `vocab_popup` grants access to the
  **parent module only** (`app`/`mod.rs`) — it does **not** grant access to
  *sibling* modules. Once `show_synopsis` moves into the sibling
  `scene_synopsis`, the call no longer compiles. So this bump is **required**,
  not a fallback: change `update_vocab_popup_margin` from `pub(super)` to
  **`pub(crate)`**. (This is a small edit to the Phase-1 `vocab_popup.rs`; note
  it as such in the plan so the reviewer expects a change outside the new
  module.)
- **Deps reached via `use super::`:** `overlay_card_size` (`pub(crate)`, stays
  in `mod.rs`); enums `InputMode`, `SidebarMode` (`pub`, stay in `mod.rs`); the
  vocab_popup fns `open_vocab_popup`/`close_vocab_popup` (via
  `crate::app::vocab_popup::*`). External `crate::input::actions::gloss::recolor_cached_blocks`
  (already pathed).
- **External call sites to repath** (`crate::app::X` → `crate::app::scene_synopsis::X`):
  - `synopsis_label` — `journal.rs:43,348`, `synopsis.rs:137,261`
  - `current_scene_divs` — `main.rs:220,381`, `keymap.rs:1143`,
    `navigation.rs:1535,1561`, `journal.rs:89`
  - `divs_at_buffer_line` — `scroll.rs:401`
  - `scene_text_for` — `journal.rs:223`
  - `toggle_synopsis` — `keymap.rs:2285`
  - `show_synopsis_overlay` — `keymap.rs:2286`
  - `scene_label` — `journal.rs:256`
  - `scene_label_for` — `scroll.rs:402`, `navigation.rs:1536,1562`
  - `cycle_synopsis` — `keymap.rs:1252,1256`
  - `update_title_bar_scene` — `keymap.rs:2189`, `highlight.rs:277,317`
  - (`is_first_line_of_scene` stays in mod.rs's reverse-call; repath that call to
    `crate::app::scene_synopsis::is_first_line_of_scene` inside `mod.rs`, or import
    via `use self::scene_synopsis::is_first_line_of_scene`.)
- **NOTE on `sync_translation_overlay`:** it is a **translations** function, not
  scene/synopsis — it moves to Module 3, not here. (The scene inventory
  over-collected it because it sits in the same source region.)

## Module 3: `src/app/translations.rs` (~520 lines)

The inline-gloss interleave path + the two-column translation overlay. One
module (the two sub-clusters share the translations concept and AppState
translation fields).

- **Functions moved:** `toggle_translations` (`pub`), `show_translations`
  (private), `hide_translations` (private), `hide_translations_for_navigation`
  (`pub`), `strip_translation_lines` (private), `map_line_after_insert`
  (private, pure), `map_line_before_insert` (private, pure),
  `show_translation_overlay` (`pub`), `sync_translation_overlay` (`pub`),
  `rebuild_translation_overlay` (`pub`).
- **Visibility:** no reverse-dep bumps — neither `build_window` nor
  `display_work_at_with_prepared` calls any translation fn (they only touch
  `state.translations_visible` as a field). Keep the four `pub` fns `pub`; keep
  the six private fns private. `rebuild_translation_overlay` is `pub` but has no
  external caller — it MAY narrow to `pub(crate)`/private; keep its current
  `pub` for pure-motion simplicity (narrowing is optional and not required).
- **Cross-family dep (resolved by extraction order):** the overlay cluster
  (`sync_translation_overlay`, `rebuild_translation_overlay`) calls scene/synopsis
  `current_scene_divs` and `synopsis_label` — both `pub`, already extracted into
  `scene_synopsis.rs` (Module 2). Reach them via
  `use crate::app::scene_synopsis::{current_scene_divs, synopsis_label};`.
- **Deps reached via `use super::`:** `apply_column_layout` (`pub`, stays in
  `mod.rs`), `overlay_card_size` (`pub(crate)`), and the font helpers
  `reapply_font`/`rebuild_line_number_gutter` (already imported into `mod.rs`
  from `self::font`; translations imports them via `crate::app::font::*`).
  `AppState` + its methods/fields.
- **External (already `crate::`-pathed):**
  `crate::ui::translation_overlay::group_scene_into_blocks`,
  `crate::input::timestamps::redraw_sign_gutters`,
  `crate::input::navigation::{invalidate_page_tops, update_highlight_only, refresh_bottom_clip}`,
  `crate::input::scroll::scrolloff_bottom_clip_widgets`, `crate::logging::log`.
- **External call sites to repath** (`crate::app::X` → `crate::app::translations::X`):
  - `toggle_translations` — `keymap.rs:2107`, `gamepad.rs:174`,
    `actions/escape.rs:14`
  - `hide_translations_for_navigation` — `search.rs:91`, `navigation.rs:958,1017`
  - `show_translation_overlay` — `keymap.rs:2287`
  - `sync_translation_overlay` — `main.rs:353,395`, `keymap.rs:1145`

## Module dependency note

After Phase 2 the `src/app/` graph is:

- `vocab_popup`, `font`, `text_prep` (Phase 1 leaves) — unchanged.
- `formatting` — leaf (no edges to the other five new modules; imports only
  `super::` consts/`AppState` + external crates).
- `scene_synopsis` — depends on `vocab_popup` (via `crate::app::vocab_popup::*`)
  and `super::` items; leaf w.r.t. formatting/translations.
- `translations` — depends on `scene_synopsis` (the one new inter-module edge:
  `translations → scene_synopsis::{current_scene_divs, synopsis_label}`) and
  `font` + `super::` items.

The only new inter-module edge is `translations → scene_synopsis`, which is why
scene_synopsis is extracted first. `mod.rs` retains reverse edges into
formatting and scene_synopsis (the documented `pub(crate)` bumps).

## What stays in `src/app/mod.rs`

`AppState` + impl, all type/enum/const definitions not named for moving, the
layout fns, `build_window`, the `display_work*` chain, `rebuild_buffer_text`,
`setup_gutter`, the gutter/vocab/gloss/AB-repeat/image-calibration families, and
the test modules. After Phase 2, `mod.rs` is ~4,200 lines.

## Mechanics (per module, repeated for all three)

1. Create `src/app/<module>.rs`. Move the listed items **verbatim**.
2. Apply the named visibility bumps for that module.
3. Add `mod <module>;` (or `pub mod <module>;` where external call sites use the
   path — formatting/scene_synopsis/translations all have external callers, so
   `pub mod`) to `mod.rs`, plus `use self::<module>::{...}` for the names
   `mod.rs`'s own retained code calls (e.g. `build_window`/`display_work` →
   `formatting::{apply_dialogue_formatting, apply_authorship_formatting}`,
   `rebuild_buffer_text` → `formatting::apply_scansion_marks`,
   `display_work` → `scene_synopsis::{is_first_line_of_scene, scene_heading_start}`).
   These are internal imports, **not** a `pub use` facade.
4. Add the new module's own `use super::...` / `use crate::...` imports; let the
   compiler name each missing one (don't bulk-guess) so no unused-import lint
   appears.
5. Repath the external call sites listed for that module (no facade).
6. Remove any now-unused `use` left in `mod.rs`.

## Verification (per module + at merge)

Pure code motion — no rendering-path logic change, so no e2e/cage run is needed
(per CLAUDE.md, e2e is for "renders correctly on screen" changes; this is
"logic unchanged, still compiles/tests").

- `cargo build` — clean
- `cargo test --bins` — total must stay **413** before and after every module:
  `cargo test --bins 2>&1 | rg 'test result'`
- `cargo clippy` — no new warnings vs the Phase-1 baseline (115); watch for
  now-unused imports / visibility lints.

## Risks & mitigations

- **Visibility too narrow → build break.** Mitigated by `cargo build`; the
  required bumps are named per module. The `update_vocab_popup_margin`
  `pub(super)` → `pub(crate)` bump is **required** (sibling modules can't see a
  `pub(super)` item) and edits the Phase-1 `vocab_popup.rs` — the scene_synopsis
  task must include it. Bump anything further only where the compiler demands a
  cross-module call the inventory missed.
- **A "leaf" has a hidden edge.** Mitigated by the inventory: formatting has
  zero cross-family edges; the only new inter-module edge is
  `translations → scene_synopsis`, handled by extraction order. If the compiler
  surfaces another edge, that function is re-examined before forcing a bump.
- **`sync_translation_overlay` mis-assigned.** It moves with translations
  (Module 3), not scene_synopsis (Module 2). The plan must place it explicitly.
- **`JOURNAL_WORK_DIV` dragged along.** It stays in `mod.rs` (journal-owned);
  only `SYNOPSIS_WHOLE_WORK` moves with scene_synopsis.
- **Order violation.** translations must not be extracted before scene_synopsis
  (its overlay cluster needs the scene helpers). The plan sequences them.

## Out of scope (explicitly deferred)

- **Tier-b, behavior-risky:** `build_window` (~1,419 lines), `display_work`,
  layout — a separate, e2e-verified effort.
- **The `AppState` god-struct grouping** — its own parked "larger project."
- Narrowing `rebuild_translation_overlay`/`apply_bcp_formatting` beyond the named
  bumps, and deleting any dead code — out of scope for pure motion.
