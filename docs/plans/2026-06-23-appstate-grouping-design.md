# AppState god-struct grouping — contained clusters (decomposition + Phase A)

**Date:** 2026-06-23
**Status:** Design approved, pending spec review
**Scope class:** Behavior-CHANGING (not pure code motion). Grouping flat
`AppState` fields into sub-structs rewrites every `state.foo_x` access to
`state.foo.x`. This is the audit's parked **AppState god-struct** project
(`docs/superpowers/audit-opportunities.md`: "~217 fields, de-facto global.
Grouping into domain sub-structs touches nearly every `&mut AppState`
signature"). It is also the documented prerequisite for any real `build_window`
split.

## The decision that scopes this project

A blast-radius inventory of all 217 fields shows a sharp split:

- **Contained clusters** (single-file, ~3–7 fields each): `nav_test_*`,
  `word_*`, `journal_*`, `page_image_*`/calibration, `echo_overlay_*`,
  `scansion_*`, `voice/tts`, `vocab_popup_*`. Near-zero cross-module churn to
  group, and **idiomatic** — `AppState` already holds domain structs
  (`ab_repeat: crate::ab_repeat::AbRepeatState`, `echo_session`,
  `visual_selection`, etc.). What does not yet exist is a struct that *re-homes
  existing flat fields* — this project introduces that.
- **Core fields** (`buffer` 291 hits/20 files, `current_line` 263/22,
  `current_work` 196/23, `config` 167/21, `text_view` 108/12, `input_mode`
  109/12): grouping any one cascades across 8–23 files **and all 7 extracted
  sibling modules**, for little readability gain — `buffer`/`current_line` are
  already perfectly clear as flat fields.

**This project groups ONLY the contained clusters. The core fields stay flat,
deliberately.** That is the honest 80/20: meaningful struct shrinkage at bounded
risk, without the worst churn-to-value rewrites. Grouping the core fields is
explicitly out of scope (and arguably should never be done).

## Goals

- Group the contained single-file field clusters into domain sub-structs, one
  sub-project per cluster, sequenced lowest-blast-radius-first.
- Each sub-struct follows the existing idiom: a named struct (in its own module
  or co-located), held as one `AppState` field, initialized via a nested literal
  / `::default()` in `build_window`'s `AppState { … }` literal.
- **Behavior-preserving** at runtime: the grouping changes the *shape* of field
  access, not the values or control flow.

## Non-goals

- Do **not** group the core fields (`buffer`, `text_view`, `current_line`,
  `page_top_line`, `current_work`, `config`, `theme`, `input_mode`,
  `gloss_overlay`, the core widgets). They stay flat.
- Do **not** restructure `build_window`'s body, the closures, or
  `display_work` — the only build_window edit is replacing a cluster's inline
  field inits with one nested literal.
- Do **not** touch the 13 `impl AppState` methods' *signatures* — only rewrite
  the field accesses inside any method that reads a grouped cluster.

## Mechanics (applies to every cluster sub-project)

1. **Define the sub-struct.** A `pub struct <Cluster>State { … }` with the
   cluster's fields (prefix stripped: `nav_test_active` → `active`). Place it in
   the most natural home — a new small module (e.g. `src/input/nav_test.rs` for
   the nav-test harness, beside its only consumer) or co-located in `app/mod.rs`
   if it has no natural module. Derive `Default` when all fields are
   default-constructible (pure-data clusters); otherwise an explicit nested
   literal supplies the initial values.
2. **Replace the flat fields in `AppState`** (the struct definition) with one
   field: `pub <cluster>: <Cluster>State`.
3. **Rewrite the `build_window` init** — replace the cluster's N inline
   `field: value` lines in the `AppState { … }` literal with one
   `<cluster>: <Cluster>State { … }` nested literal (or `::default()`). This is
   the only build_window edit; it touches only the cluster's own init lines.
4. **Rewrite every field-access site** `state.<cluster>_<x>` →
   `state.<cluster>.<x>` (and `s.<cluster>_<x>` → `s.<cluster>.<x>`,
   `borrow().<cluster>_<x>`, etc.) across the cluster's access files. The
   compiler finds every site (it's a hard error otherwise) — this is the
   behavior-changing edit, but mechanical.
5. Update any `impl AppState` method that reads the cluster's fields.

No facade, no accessor indirection — direct nested-field access, matching the
`state.ab_repeat.chunk_index` pattern already in the codebase.

## Verification — per-cluster risk-tiered

Each cluster's sub-project declares its tier:

- **Pure-state clusters** (`nav_test_*`, `word_*`, `journal_*`,
  `page_image_*`/calibration, `echo_overlay_*`) — provably cannot affect
  rendering; fully covered by `cargo test --bins` (must stay **413**) + clippy
  (**115**). No e2e needed (tier-a style).
- **Render-touching clusters** (`vocab_popup_*` drives the vocab Popover;
  `scansion_*` affects the displayed scansion marks) — ALSO get a **user-run
  nav-fuzz/e2e gate before merge** (tier-b style; the agent cannot launch cage —
  dwl owns the seat — so it builds, runs the unit gates, states runtime
  verification is blocked, and asks the user to run the nav-fuzz).

The agent-runnable gates are identical across tiers: `cargo build` clean,
`cargo test --bins` = 413, `cargo clippy` = 115. The difference is whether a
user render-check gates the merge.

## Cluster decomposition (sequenced lowest-blast-radius-first)

Each is its own spec → plan → execute → merge sub-project. Field lists are the
grouping membership; the prefix is stripped in the sub-struct.

| # | Cluster | Fields | Access files | Tier |
|---|---|---|---|---|
| A | `nav_test` | active, step, failures, prev_top, expect_return, fuzz (6) | 2 (mod.rs, nav_test.rs) | pure |
| B | `journal` | pages, page_index, return_pos, prompt_mode (4) | ~2 | pure |
| C | `page_image` | page_images, image_dir, image_mode, current_page_order, calibration_index (5) | ~2 | pure |
| D | `word_cycle` | word_cycle_line, word_cycle_index, word_collect_words, word_collect_ranges, word_bold_gen (5) | ~2 | pure |
| E | `echo_overlay` | links, index, titles, source, turn_id, turn_key (6) | ~2 | pure |
| F | `scansion` | scansion_label_starts, scansion_level, scansion_data (3) | ~3 | **render** |
| G | `vocab_popup` | vocab_popup(widget), data, index, view, auto, line, fade_gen (7) | ~8 | **render** |

Notes:
- **Phase A (this spec) fully specs cluster A (`nav_test`)** — the pilot. The
  other clusters are scoped here for sequencing but each gets its own spec when
  reached (standard decomposition; estimates may refine on contact).
- `vocab_popup` (G) is last: 8 access files makes it the highest-churn of the
  contained set, and it holds a real widget (the `VocabPopup`), so it is
  render-tier and not `Default`-derivable. Doing it last lets the pattern settle
  on the trivial clusters first.
- Clusters NOT in this list (search, mpv/sync, translations, gloss-state,
  toasts, gutter) are either medium-spread or core-adjacent and are deferred —
  re-evaluate after the contained set ships, but they are not part of this
  project's committed scope.

## Phase A — cluster `nav_test` (the pilot, fully specified)

The lowest-risk cluster in the entire struct: 6 fields, accessed in exactly 2
files (`src/app/mod.rs` + `src/input/nav_test.rs`), all pure `bool`/`usize`/
`Option<usize>` (trivially `Default`-derivable). Provably cannot affect
rendering → pure tier.

### The sub-struct

Define in `src/input/nav_test.rs` (beside its only real consumer):

```rust
#[derive(Default)]
pub struct NavTestState {
    pub active: bool,
    pub step: usize,
    pub failures: usize,
    pub prev_top: usize,
    pub expect_return: Option<usize>,
    pub fuzz: bool,
}
```

### AppState change

Replace the 6 flat fields (`nav_test_active`, `nav_test_step`,
`nav_test_failures`, `nav_test_prev_top`, `nav_test_expect_return`,
`nav_test_fuzz`) with one field:

```rust
pub nav_test: crate::input::nav_test::NavTestState,
```

### build_window init change

Replace the 6 inline inits (`nav_test_active: false, … nav_test_fuzz: false,`)
in the `AppState { … }` literal with one line:

```rust
nav_test: crate::input::nav_test::NavTestState::default(),
```

(All six initial values are the `Default` — `false`/`0`/`None` — so
`::default()` is exact; no nested literal needed for this cluster.)

### Access-site rewrites

Every `state.nav_test_<x>` / `s.nav_test_<x>` / `borrow().nav_test_<x>` →
`state.nav_test.<x>` etc., in `src/app/mod.rs` (the `LIT_NAV_FUZZ` auto-start
timer + any reads) and `src/input/nav_test.rs` (the harness body). The compiler
flags every missed site.

### Verification (pure tier)

- `cargo build` — clean
- `cargo test --bins` — **413** (the nav-test harness is `#[ignore]`d e2e, so
  the count is unaffected; this proves the rewrite compiles + the pure suite
  passes): `cargo test --bins 2>&1 | rg 'test result'`
- `cargo clippy` — **115**, no new warnings

No user nav-fuzz needed for cluster A — it's a test harness, not a render path.
(The nav-fuzz *uses* `nav_test` state, but grouping its fields cannot change
what the harness asserts; the unit build proves the access rewrite is correct.)

## Risks & mitigations

- **Behavior change slips in during the access rewrite.** Mitigated by: the
  rewrite is purely `state.foo_x` → `state.foo.x` (the compiler rejects any
  typo), no value/logic edits; and the per-cluster verification tier.
- **A cluster turns out higher-spread than estimated.** Mitigated by the
  decomposition: each cluster is re-scoped in its own spec on contact; the table
  estimates are starting points. If a "pure" cluster is found to touch render
  state, bump it to render-tier.
- **build_window edit drifts into the parked tier-b work.** Mitigated by the
  hard rule: the ONLY build_window edit is the cluster's own init lines becoming
  one nested literal/`::default()` — no structural change, no closure edit.
- **The 13 `impl AppState` methods.** Most read core/flat fields, not the
  contained clusters; a method that does read a grouped cluster gets its field
  accesses rewritten in that cluster's sub-project. None change signature.

## Out of scope (explicitly deferred)

- **Grouping the core fields** (`buffer`, `text_view`, `current_line`,
  `page_top_line`, `current_work`, `config`, `theme`, `input_mode`, core
  widgets, `gloss_overlay`) — high churn, low value; stays flat, likely
  permanently.
- **Medium-spread clusters** (search, mpv/sync, translations, gloss-state,
  toasts, gutter) — re-evaluate after the contained set ships; not committed
  scope here.
- **The `build_window` body split / `display_work` extraction** — the rest of
  tier-b; this project unblocks them but does not perform them.
