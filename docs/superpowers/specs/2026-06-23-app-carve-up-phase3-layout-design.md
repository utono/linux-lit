# Carve the layout functions out of app.rs (Phase 3 / tier-b start)

**Date:** 2026-06-23
**Status:** Design approved, pending spec review
**Scope class:** Behavior-preserving code motion — but **tier-b**, so the moved
functions are widget-bound (live GTK/Pango measurement), and the regression
proof is **e2e/nav-fuzz on a rendered spread**, not only the 413-test suite.
This is the first, lowest-risk slice of the audit's parked tier-b carve-up
(`docs/superpowers/audit-opportunities.md`: "behavior-risky tier-b targets —
`build_window` (~1419 lines), `display_work`, layout — which need e2e/nav-fuzz
verification").

## Problem & what tier-b actually is

After Phases 1+2, `src/app/mod.rs` is 4,360 lines. The audit named three tier-b
targets — `build_window`, `display_work`, layout. A structural inventory shows
they are **not** equally tractable:

- **`build_window`** (1,141–2,556) is dominated by the **~218-field `AppState`
  struct literal** (1,733–1,956) and by closures (RESIZE_TICK tick callback,
  reveal timers, 9 picker handlers, key controller, startup async) that
  **capture `state` created at line 1,733**. None can move before the literal,
  and the literal *is* the god-struct. `build_window` cannot be split by pure
  code motion without first grouping `AppState` into sub-structs — a separate,
  behavior-changing project, explicitly out of scope here.
- **`display_work_at_with_prepared`** (2,724–3,404) is a large self-contained
  function touching every rendering path — extractable, but the highest
  e2e-verification burden. Deferred to a later Phase.
- **The layout free functions** (785–1,140) are *callable functions*, not
  closures — `&mut AppState`-in / widgets-out (or pure). This is exactly the
  shape Phases 1+2 proved safe to move, modulo the tier-b verification rule.

This phase extracts **only the layout cluster** into `src/app/layout.rs`. It is
the lowest-risk real tier-b win and leaves the genuinely-blocked
`build_window` body and `display_work` in `mod.rs`.

## Goals

- Move the layout function cluster into a new `src/app/layout.rs` via pure code
  motion.
- **Behavior-preserving.** No logic edits. Only the named visibility bumps and
  necessary `use` adjustments.
- No re-export facade — external call sites repathed directly to
  `crate::app::layout::<fn>`.

## Non-goals

- Do **not** touch `build_window`'s body, the `AppState` struct literal, any
  closure, `display_work_at_with_prepared`, or `rebuild_buffer_text`. They stay
  in `mod.rs`.
- No `AppState` field changes; the god-struct grouping stays parked (and is what
  blocks the real `build_window` split — a separate project).
- No const consolidation, no new behavior, no dead-code deletion.

## The module: `src/app/layout.rs`

Move these items **verbatim** (all currently in `mod.rs`, lines ~785–1,140 +
the two layout test modules). Group = "card/column sizing + tiled-mode layout".

- **Functions:**
  - `line_number_gutter_geometry` (`pub(crate)`, 785) — pure. Reverse-called by
    `display_work` (3213) and by `font.rs` (via `use super::`).
  - `verse_left_offset` (`pub`, 801) — pure. External callers in `settings.rs`.
  - `current_block_text_width` (private, 818) — widget-bound. Called only by
    `apply_tiled_mode` (in-cluster) → stays private.
  - `is_tiled_layout` (`pub`, 832) — pure. Called only by `apply_tiled_mode`
    (in-cluster, same module after the move). It has no external callers, so it
    *could* narrow to private — but this phase is pure motion and the house rule
    is "change visibility only when a cross-module call forces it." Since both
    caller and callee land in `layout.rs`, no bump is forced; **keep its current
    `pub`** verbatim (narrowing is a separate cleanup, out of scope). Same
    reasoning leaves `current_block_text_width` private (already private,
    in-cluster caller).
  - `apply_tiled_mode` (`pub` → **`pub(crate)`**, 841) — widget-bound.
    Reverse-called by `build_window`'s tick closure (2128, 2195), `display_work`
    (2952), and `apply_column_layout` (in-cluster).
  - `apply_column_layout` (`pub` → **`pub(crate)`**, 1025) — widget-bound.
    External caller: `translations.rs` (via `use super::apply_column_layout`).
    Calls `apply_card_sizing` + `apply_tiled_mode` (in-cluster).
  - `target_card_width` (`pub(crate)`, 1080) — pure. Called by `apply_tiled_mode`,
    `apply_card_sizing`, `overlay_card_size` (all in-cluster) + its own tests.
  - `apply_card_sizing` (`pub` → **`pub(crate)`**, 1101) — widget-bound.
    Reverse-called by `build_window`'s tick (2110/2127/2194), `apply_column_layout`
    (in-cluster). External callers: `navigation.rs`, `settings.rs`.
  - `overlay_card_size` (`pub(crate)`, 1129) — widget-bound (pure read of
    `&AppState`). External callers: `scene_synopsis.rs`, `translations.rs` (via
    `use super::overlay_card_size`) + its own tests.
- **Const moved:** `SONNET_BLOCK_SAMPLE` (813) — used only by
  `current_block_text_width`. Moves with the cluster.
- **Test modules moved:** `card_width_tests` and `column_default_tests` (~4268+)
  — they exercise `target_card_width` / `overlay_card_size`. These run in
  `cargo test --bins`, so part of this module IS unit-tested (the pure sizing
  math), which the 413 count still proves; the widget-bound parts need e2e.
- **Visibility bumps (3):** `apply_tiled_mode`, `apply_column_layout`,
  `apply_card_sizing` `pub` → `pub(crate)` (reverse-called by `mod.rs` /
  sibling modules; never bare `pub` for an internal item). The pure functions
  already-`pub` with external callers (`verse_left_offset`) and the
  already-`pub(crate)`/`pub` ones (`line_number_gutter_geometry`,
  `target_card_width`, `overlay_card_size`, `is_tiled_layout`) keep their
  current visibility.

## Consts that STAY in `mod.rs` (imported via `use super::`)

These are used by layout functions **and** by code that stays in `mod.rs`
(build_window tick / display_work) and by sibling modules — so they stay put,
exactly as `DIALOGUE_INDENT` did in Phase 2. `layout.rs` imports them via
`use super::{...}`:

- `TWO_COLUMN_WIDTH_FRACTION` (1049), `MIN_TWO_COLUMN_COLUMN_WIDTH` (1068) — used
  by `target_card_width`/`apply_tiled_mode` AND build_window tick (2052/2142/2159).
- `SHOW_LINE_NUMBERS_TWO_COL` (1055) — used by `apply_tiled_mode` AND display_work
  (3195/3659) AND `font.rs` (`use super::SHOW_LINE_NUMBERS_TWO_COL`).
- `TOP_SPACER_HEIGHT` (757) — used by `font.rs`; not by the layout cluster, leave
  it (do not move).

## Sibling-module import repaths

Three already-extracted sibling modules currently reach layout fns via
`use super::`. Those imports must repath to the new module:

- `src/app/font.rs:1` — `use super::{AppState, line_number_gutter_geometry, ...}`
  → move `line_number_gutter_geometry` to `use crate::app::layout::line_number_gutter_geometry;`
  (keep `AppState`, `TOP_SPACER_HEIGHT`, `SHOW_LINE_NUMBERS_TWO_COL` on the
  `super::` import — they stay in `mod.rs`).
- `src/app/scene_synopsis.rs:2` — `use super::{AppState, InputMode, SidebarMode, overlay_card_size}`
  → move `overlay_card_size` to `use crate::app::layout::overlay_card_size;`.
- `src/app/translations.rs:1` — `use super::{AppState, apply_column_layout, overlay_card_size}`
  → move both to `use crate::app::layout::{apply_column_layout, overlay_card_size};`.

## External call sites to repath (`crate::app::X` → `crate::app::layout::X`)

- `apply_card_sizing` — `src/input/navigation.rs:542`,
  `src/input/actions/settings.rs:29, 316, 434`
- `verse_left_offset` — `src/input/actions/settings.rs:35, 319, 437`

(No other layout fn has an external `crate::app::` caller.)

## `mod.rs` wiring

- Add `pub mod layout;` near the top.
- Add `use self::layout::{...}` for the names `mod.rs`'s retained code calls:
  `build_window`'s tick + `apply_column_layout` need `apply_tiled_mode`,
  `apply_card_sizing`; `display_work` needs `apply_tiled_mode`,
  `line_number_gutter_geometry`. So:
  `use self::layout::{apply_tiled_mode, apply_card_sizing, line_number_gutter_geometry};`
  (add any other name the compiler reports used unqualified in `mod.rs` —
  e.g. `target_card_width`/`overlay_card_size` if a retained helper calls them).
  These are internal imports, **not** a `pub use` facade.

## `layout.rs` own imports

Start with:

```rust
use super::{AppState, TWO_COLUMN_WIDTH_FRACTION, MIN_TWO_COLUMN_COLUMN_WIDTH, SHOW_LINE_NUMBERS_TWO_COL};
use crate::logging::log;
```

Then `cargo build` and add EXACTLY what the compiler names (`gtk4::prelude::*`
and any `sourceview5`/`pango` traits for the widget-bound fns,
`crate::db::line_types::*` if referenced, etc.). Remove any starter `super::`
import reported unused. Goal: zero unused-import warnings.

## Verification — THIS IS TIER-B, so it differs from Phases 1+2

The pure sizing math (`target_card_width`, `overlay_card_size`,
`line_number_gutter_geometry`, `verse_left_offset`, `is_tiled_layout`) IS
covered by the moved unit tests + the 413 count. But `apply_tiled_mode`,
`apply_card_sizing`, `apply_column_layout`, `current_block_text_width` are
**widget-bound** — they mutate live GTK widgets and read Pango geometry. Per
CLAUDE.md, their acceptance criterion is "it renders correctly on screen," so
the real proof is a **rendered spread**, which an agent cannot launch (the live
dwl session owns the seat).

Agent-runnable gates (necessary, not sufficient):
- `cargo build` — clean
- `cargo test --bins` — total stays **413** (proves the pure-math motion +
  the moved tests still pass): `cargo test --bins 2>&1 | rg 'test result'`
- `cargo clippy` — no new warnings vs baseline (115)

**User-run gate (REQUIRED before merge — the tier-b difference):** the
**nav-fuzz**, which drives tiled/two-column layout, card sizing, and spread
balance — exactly the functions moved here:

```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work <ABBR>
```

Run it on at least one two-column play (e.g. a Folger work like `H8-Amb` or
`Ham`) AND a one-section-per-page sequence (a sonnet sequence, to exercise
`current_block_text_width`/`SONNET_BLOCK_SAMPLE` centering). Expect: no new
UNBALANCED-SPREAD / clipping / card-width failures vs a pre-change baseline.
The agent will build, run the unit gates, state plainly that runtime
verification is blocked, and **ask the user to run this** and paste the result.

## Mechanics

1. Create `src/app/layout.rs`. Move the listed fns + `SONNET_BLOCK_SAMPLE` +
   the two test modules **verbatim**.
2. Apply the 3 visibility bumps (`apply_tiled_mode`, `apply_column_layout`,
   `apply_card_sizing` → `pub(crate)`).
3. Add `pub mod layout;` + the `use self::layout::{...}` internal import to
   `mod.rs`; add `layout.rs`'s own `use super::`/`use crate::` imports
   (compiler-driven).
4. Repath the three sibling-module `use super::` imports (font, scene_synopsis,
   translations).
5. Repath the external call sites (navigation.rs, settings.rs).
6. Remove now-unused `use` left in `mod.rs`.

## Risks & mitigations

- **A widget-bound fn's motion subtly changes rendering.** This is the tier-b
  risk the unit suite can't catch. Mitigated by the mandatory user nav-fuzz on
  both a two-column play and a sonnet sequence before merge.
- **Const moved that's still needed in mod.rs → build break.** Mitigated by
  keeping `TWO_COLUMN_WIDTH_FRACTION`/`MIN_TWO_COLUMN_COLUMN_WIDTH`/
  `SHOW_LINE_NUMBERS_TWO_COL`/`TOP_SPACER_HEIGHT` in `mod.rs` (only
  `SONNET_BLOCK_SAMPLE` moves) and importing via `use super::`.
- **Visibility too narrow → build break.** The 3 named `pub(crate)` bumps cover
  the known reverse/sibling callers; bump further only if the compiler demands a
  call the inventory missed.
- **Sibling-import repath missed → build break.** Caught immediately by
  `cargo build`; the three files (font, scene_synopsis, translations) are named.

## Out of scope (explicitly deferred)

- **`build_window` body / `AppState` struct literal / the closures** — blocked
  on the god-struct grouping; a separate behavior-changing project.
- **`display_work_at_with_prepared` / `rebuild_buffer_text`** — a later tier-b
  Phase, highest e2e burden.
- **The `AppState` god-struct grouping** — its own parked "larger project," and
  the prerequisite for any real `build_window` split.
