# app.rs Carve-Up Phase 3 — Layout Module Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract the layout function cluster out of `src/app/mod.rs` into a new `src/app/layout.rs`, via pure code motion — the first tier-b slice.

**Architecture:** One new sibling module under `src/app/`. Nine layout functions (card/column sizing + tiled-mode) + `SONNET_BLOCK_SAMPLE` + two layout test modules move verbatim. Three widget-bound functions reverse-called from `mod.rs`/sibling modules get `pub(crate)` bumps. The two-column/spacer consts stay in `mod.rs` (shared with build_window/display_work), imported via `use super::`. No facade; call sites repathed directly. Because the moved widget-bound functions render to screen, the regression proof is a **user-run nav-fuzz**, not only the 413-test suite.

**Tech Stack:** Rust, GTK4 / sourceview5 / Pango, `cargo build` / `cargo test --bins` / `cargo clippy`, `scripts/e2e-env.sh` nav-fuzz.

## Global Constraints

- **Scope class: behavior-preserving code motion (tier-b).** No logic edits. Move bodies VERBATIM. The only signature changes are the three named `pub(crate)` bumps.
- **No facade.** No `pub use` re-export from `mod.rs`. A plain `use self::layout::{...}` for `mod.rs`'s own retained callers is correct, NOT a facade.
- **Test-count invariant: `cargo test --bins` must report 413** before and after. The moved `card_width_tests` + `column_default_tests` modules keep the count whole. Command: `cargo test --bins 2>&1 | rg 'test result'`.
- **Clippy baseline: 115.** No new warnings; remove now-unused `use` left in `mod.rs`.
- **TIER-B VERIFICATION DIFFERENCE:** `cargo test --bins` covers ONLY the pure sizing math (the moved unit tests). The widget-bound fns (`apply_tiled_mode`, `apply_card_sizing`, `apply_column_layout`, `current_block_text_width`) render to screen and are NOT covered by the unit suite. Per CLAUDE.md, an agent CANNOT launch the nav-fuzz (live dwl owns the seat). The agent builds + runs the unit gates, states plainly that runtime verification is blocked, and ASKS THE USER to run the nav-fuzz before merge (Task 2).
- **Consts that STAY in `mod.rs`** (imported via `use super::`): `TWO_COLUMN_WIDTH_FRACTION`, `MIN_TWO_COLUMN_COLUMN_WIDTH`, `SHOW_LINE_NUMBERS_TWO_COL`, `TOP_SPACER_HEIGHT`. Only `SONNET_BLOCK_SAMPLE` moves.
- **Locate items by NAME**, not absolute line number (`rg -n 'fn apply_tiled_mode' src/app/mod.rs`).
- Leave unrelated `use crate::app::AppState;` imports untouched.
- Already on branch `refactor/app-carve-up-phase3-layout` (the spec was committed there). Do NOT create a new branch.

---

### Task 0: Baseline

**Files:** none (verification only).

**Interfaces:**
- Consumes: branch `refactor/app-carve-up-phase3-layout` (already exists, spec committed).
- Produces: confirmed 413-test + clippy-115 baseline.

- [ ] **Step 1: Confirm branch + capture baselines**

```bash
cd ~/utono/linux-lit
git branch --show-current   # expect: refactor/app-carve-up-phase3-layout
cargo test --bins 2>&1 | rg 'test result'
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: `413 passed`; clippy `115`. Record both. (No commit — verification only.)

---

### Task 1: Extract `src/app/layout.rs`

**Files:**
- Create: `src/app/layout.rs`
- Modify: `src/app/mod.rs` (remove 9 fns + `SONNET_BLOCK_SAMPLE` + 2 test modules; add `pub mod layout;` + `use self::layout::{...}`; 3 visibility bumps)
- Modify: `src/app/font.rs` (repath `line_number_gutter_geometry` import)
- Modify: `src/app/scene_synopsis.rs` (repath `overlay_card_size` import)
- Modify: `src/app/translations.rs` (repath `apply_column_layout` + `overlay_card_size` imports)
- Modify: `src/input/navigation.rs` (repath 1 call)
- Modify: `src/input/actions/settings.rs` (repath 7 calls: `apply_card_sizing` ×3, `verse_left_offset` ×3 — wait, ×4 and ×3, see Step 5)

**Interfaces:**
- Consumes: `src/app/mod.rs` from Task 0.
- Produces: module `crate::app::layout` exposing `pub(crate) fn apply_tiled_mode(state: &mut AppState, root_box: &gtk4::Box, window_width: i32)`, `pub(crate) fn apply_card_sizing(...)`, `pub(crate) fn apply_column_layout(state: &mut AppState)`, `pub(crate) fn line_number_gutter_geometry(column_count: u8) -> (i32,i32,i32)`, `pub(crate) fn overlay_card_size(s: &AppState) -> (i32,i32)`, `pub(crate) fn target_card_width(...)`, `pub fn verse_left_offset(window_width: i32, column_width: u32) -> i32`, `pub fn is_tiled_layout(window_width: i32, column_width: u32) -> bool`. `current_block_text_width` stays private.

- [ ] **Step 1: Create `src/app/layout.rs` with the moved code**

Move these items **verbatim** from `src/app/mod.rs` into a new `src/app/layout.rs` (locate each by name):

- `line_number_gutter_geometry` (keep `pub(crate)`)
- `verse_left_offset` (keep `pub`)
- `current_block_text_width` (keep PRIVATE)
- `is_tiled_layout` (keep `pub` — verbatim; in-cluster caller, no forced bump)
- `apply_tiled_mode` (**bump `pub` → `pub(crate)`**)
- `apply_column_layout` (**bump `pub` → `pub(crate)`**)
- `target_card_width` (keep `pub(crate)`)
- `apply_card_sizing` (**bump `pub` → `pub(crate)`**)
- `overlay_card_size` (keep `pub(crate)`)
- the const `SONNET_BLOCK_SAMPLE` (private — used only by `current_block_text_width`)
- the two test modules `card_width_tests` and `column_default_tests` (move whole; they test `target_card_width`/`overlay_card_size`).

At the top of `src/app/layout.rs`, start with:

```rust
use super::{AppState, TWO_COLUMN_WIDTH_FRACTION, MIN_TWO_COLUMN_COLUMN_WIDTH, SHOW_LINE_NUMBERS_TWO_COL};
use crate::logging::log;
```

Then `cargo build` (Step 3) and add EXACTLY the `use` lines the compiler names: `gtk4::prelude::*`, any `sourceview5`/`pango` traits the widget-bound fns need, `crate::db::line_types::*` if referenced. Do NOT bulk-guess. If a `super::` const is unused by the moved fns, remove it from the import. The moved test modules may need their own `use super::*;` inside the `#[cfg(test)] mod` — keep whatever they had verbatim.

- [ ] **Step 2: Wire `mod.rs`**

In `src/app/mod.rs`:
- Delete the 9 fns + `SONNET_BLOCK_SAMPLE` + the 2 test modules you moved.
- Keep the consts `TWO_COLUMN_WIDTH_FRACTION`, `MIN_TWO_COLUMN_COLUMN_WIDTH`, `SHOW_LINE_NUMBERS_TWO_COL`, `TOP_SPACER_HEIGHT` in place (do NOT move them).
- Add `pub mod layout;` near the top.
- Add the internal import for the fns mod.rs's retained code (build_window tick, display_work) calls:

```rust
use self::layout::{apply_tiled_mode, apply_card_sizing, line_number_gutter_geometry};
```

If `cargo build` reports another moved fn used unqualified inside mod.rs (e.g. `target_card_width` or `overlay_card_size` from a retained helper, or `apply_column_layout`), add only the name(s) the compiler names to this `use self::layout::{...}` line.

- [ ] **Step 3: Build (resolve layout.rs imports)**

```bash
cargo build
```
Resolve any import error in `layout.rs` by adding the exact `use` the compiler names. If a bumped fn is unreachable from mod.rs, confirm the `pub(crate)` bump + the `use self::layout` line.

- [ ] **Step 4: Repath the sibling-module `use super::` imports**

In `src/app/font.rs` line 1 — `line_number_gutter_geometry` moved out of `super::`:
```rust
// before:
use super::{AppState, line_number_gutter_geometry, TOP_SPACER_HEIGHT, SHOW_LINE_NUMBERS_TWO_COL};
// after:
use super::{AppState, TOP_SPACER_HEIGHT, SHOW_LINE_NUMBERS_TWO_COL};
use crate::app::layout::line_number_gutter_geometry;
```

In `src/app/scene_synopsis.rs` line 2 — `overlay_card_size` moved:
```rust
// before:
use super::{AppState, InputMode, SidebarMode, overlay_card_size};
// after:
use super::{AppState, InputMode, SidebarMode};
use crate::app::layout::overlay_card_size;
```

In `src/app/translations.rs` line 1 — `apply_column_layout` + `overlay_card_size` moved:
```rust
// before:
use super::{AppState, apply_column_layout, overlay_card_size};
// after:
use super::AppState;
use crate::app::layout::{apply_column_layout, overlay_card_size};
```

- [ ] **Step 5: Repath the external call sites**

In `src/input/navigation.rs:542` — `crate::app::apply_card_sizing(` → `crate::app::layout::apply_card_sizing(`.

In `src/input/actions/settings.rs` — `apply_card_sizing` at 29, 316, 434 and `verse_left_offset` at 35, 319, 437:
- `crate::app::apply_card_sizing(` → `crate::app::layout::apply_card_sizing(` (×3)
- `crate::app::verse_left_offset(` → `crate::app::layout::verse_left_offset(` (×3)

Verify zero un-repathed sites remain:
```bash
rg -n 'crate::app::(apply_tiled_mode|apply_card_sizing|apply_column_layout|target_card_width|is_tiled_layout|current_block_text_width|verse_left_offset|overlay_card_size|line_number_gutter_geometry)\b' src/ | rg -v 'layout::'
```
Expected: no output.

- [ ] **Step 6: Build after repath**

```bash
cargo build
```
Expected: clean.

- [ ] **Step 7: Clippy**

```bash
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: 115. If higher, remove the now-unused `use` left in `mod.rs` (the moved fns' names) or in the sibling files.

- [ ] **Step 8: Test-count invariant**

```bash
cargo test --bins 2>&1 | rg 'test result'
```
Expected: `413 passed`. (A drop means a moved test module failed to compile — fix its `use` path, don't delete tests.)

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "refactor(app): extract layout family into src/app/layout.rs

Pure code motion (tier-b): the card/column-sizing + tiled-mode layout fns
(apply_tiled_mode, apply_card_sizing, apply_column_layout, target_card_width,
is_tiled_layout, current_block_text_width, verse_left_offset,
overlay_card_size, line_number_gutter_geometry) + SONNET_BLOCK_SAMPLE + the
card_width/column_default test modules move out of app.rs into a sibling
module. apply_tiled_mode/apply_card_sizing/apply_column_layout bumped pub ->
pub(crate) (build_window tick + display_work + sibling modules reverse-call
them). Two-column/spacer consts stay in mod.rs (shared), imported via use
super. Sibling imports (font/scene_synopsis/translations) + external call
sites (navigation/settings) repathed (no facade). 413 tests unchanged.

Widget-bound fns render to screen; nav-fuzz verification pending (Task 2).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

- [ ] **Step 10: Report runtime verification is blocked**

State plainly in the task report: the unit gates (build, 413, clippy 115) pass and cover the pure sizing math, BUT the widget-bound layout fns (`apply_tiled_mode`/`apply_card_sizing`/`apply_column_layout`/`current_block_text_width`) render to screen and are NOT exercised by `cargo test --bins`. Runtime verification (nav-fuzz) is blocked for the agent (live dwl owns the seat) and must be run by the user in Task 2 before merge.

---

### Task 2: User nav-fuzz verification + finish the branch

**Files:** none (verification + git) + audit ledger update.

**Interfaces:**
- Consumes: the extracted `layout.rs` from Task 1.
- Produces: user-confirmed nav-fuzz clean; Phase 3 merged to master; ledger updated.

- [ ] **Step 1: Agent unit gates (final, on the committed branch)**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result' && cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: clean build, `413 passed`, clippy `115`. Confirm `git status` clean.

- [ ] **Step 2: ASK THE USER to run the nav-fuzz (REQUIRED — tier-b gate)**

The agent CANNOT run this (live dwl owns the seat). Give the user these exact commands and wait for the result. Run on a two-column play AND a sonnet sequence (to exercise `current_block_text_width`/`SONNET_BLOCK_SAMPLE` centering):

```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work H8-Amb
```

```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work Son
```

(`Son` = "The Sonnets", confirmed in lit.db — the one-section-per-page case that exercises `current_block_text_width`/`SONNET_BLOCK_SAMPLE` centering.) Expected: no new UNBALANCED-SPREAD / clipping / card-width failures vs a pre-change baseline. The FAIL summary prints to the terminal; full log at `/tmp/fuzz-nav.log`.

**Do NOT proceed to merge until the user confirms the nav-fuzz is clean.** If it surfaces failures, treat as a tier-b regression: return to systematic-debugging, do NOT merge.

- [ ] **Step 3: Merge to master (only after user confirms clean)**

```bash
git checkout master
git merge --no-ff refactor/app-carve-up-phase3-layout
```

- [ ] **Step 4: Re-verify unit gates on merged master**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result'
```
Expected: clean build, `413 passed`. Confirm `mod.rs` shrank:
```bash
wc -l src/app/mod.rs src/app/layout.rs
```
Expected: `mod.rs` ~4,000; `layout.rs` ~400.

- [ ] **Step 5: Push, delete branch**

```bash
git push origin master
git branch -d refactor/app-carve-up-phase3-layout
```

- [ ] **Step 6: Update the audit ledger**

In `docs/superpowers/audit-opportunities.md`, update the `app.rs module carve-up` entry: record Phase 3 (layout.rs) DONE with the new mod.rs line count and the nav-fuzz result; note that tier-b is now PARTIALLY done (layout extracted) but `build_window`'s body + `display_work` + the AppState god-struct remain parked (build_window blocked on the god-struct). Commit:

```bash
git add docs/superpowers/audit-opportunities.md
git commit -m "docs(audit): mark app.rs carve-up Phase 3 (layout module) DONE

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push origin master
```

---

## Self-Review

**Spec coverage:**
- layout.rs module, 9 fns + SONNET_BLOCK_SAMPLE + 2 test modules (spec "The module") → Task 1 Step 1 ✓
- 3 visibility bumps apply_tiled_mode/apply_column_layout/apply_card_sizing → pub(crate) (spec "Visibility bumps") → Task 1 Step 1 ✓
- is_tiled_layout/current_block_text_width keep current visibility, no narrowing (spec note) → Task 1 Step 1 ✓
- consts stay in mod.rs via use super (spec "Consts that STAY") → Global Constraints + Task 1 Step 1/2 ✓
- sibling-import repaths font/scene_synopsis/translations (spec "Sibling-module import repaths") → Task 1 Step 4 ✓
- external call-site repaths navigation/settings (spec "External call sites") → Task 1 Step 5 ✓
- mod.rs wiring pub mod + use self::layout, no facade (spec "mod.rs wiring") → Task 1 Step 2 ✓
- tier-b verification: unit gates cover pure math, user nav-fuzz for widget-bound fns (spec "Verification") → Global Constraints + Task 1 Step 10 + Task 2 Step 2 ✓
- build_window/display_work/god-struct out of scope (spec "Out of scope") → no task touches them ✓

**Placeholder scan:** No TBD/TODO. The one soft spot is the sonnet-sequence abbrev in Task 2 Step 2 (`Son`) — flagged explicitly as "ask the user / pick a one-section-per-page work," not a silent placeholder. All repaths are exact with `rg`-verify.

**Type consistency:** Module path `crate::app::layout::*` consistent across tasks. The 3 bumped fns named identically in spec and plan. `line_number_gutter_geometry` (pub(crate), font.rs import), `overlay_card_size` (pub(crate), scene_synopsis+translations imports), `apply_column_layout` (pub(crate), translations import) — the sibling repaths in Step 4 match the moved fns' names exactly.
