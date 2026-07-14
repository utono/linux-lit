# app.rs Carve-Up Phase 2 — Tier-a Families Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract the three remaining tier-a families (`formatting`, `scene_synopsis`, `translations`) out of `src/app/mod.rs` into sibling modules, via pure code motion with no behavior change, finishing the tier-a carve-up.

**Architecture:** Three new files under the existing `src/app/` directory, extracted in dependency order (formatting → scene_synopsis → translations) so each module's `pub` helpers exist before a later module needs them. Pure verbatim code motion; named visibility bumps only; no re-export facade — external call sites repathed directly from `crate::app::foo` to `crate::app::<module>::foo`. The existing 413-test suite is the regression check; there are no new tests.

**Tech Stack:** Rust, GTK4 / sourceview5, `cargo build` / `cargo test --bins` / `cargo clippy`.

## Global Constraints

- **Scope class: behavior-preserving code motion.** No logic edits. Move function/type/const bodies VERBATIM. The only permitted signature changes are the named visibility bumps per task.
- **No facade.** Do not re-`pub`-export moved items from `mod.rs`. A plain `use self::<module>::{...}` in `mod.rs` for `mod.rs`'s OWN retained callers (build_window, display_work, rebuild_buffer_text) is correct and is NOT a facade; a `pub use` re-export WOULD be.
- **Test-count invariant: `cargo test --bins` must report 413 passed before and after every task.** Command: `cargo test --bins 2>&1 | rg 'test result'`.
- **Clippy baseline: 115 warnings.** No new warnings vs baseline; remove any now-unused `use` left in `mod.rs`.
- **No e2e/cage run needed** — pure motion, "logic unchanged, still compiles/tests" (per CLAUDE.md).
- **Extraction order is mandatory:** formatting (Task 1) → scene_synopsis (Task 2) → translations (Task 3). translations' overlay cluster calls scene_synopsis helpers, so scene_synopsis must be extracted first.
- **One module per task = one PR.** Each task independently mergeable on the branch.
- **Locate items by NAME, not absolute line number.** Each task's deletions shift the line numbers of every item below it in `mod.rs`. Use `rg -n 'fn <name>|struct <name>|enum <name>' src/app/mod.rs` to find current positions.
- Leave the unrelated `use crate::app::AppState;` / `use crate::app::InputMode;` imports untouched.
- Branch off `master` (currently on `master`). Branch name: `refactor/app-carve-up-phase2`.

---

### Task 0: Branch + baseline

**Files:** none (git only).

**Interfaces:**
- Consumes: nothing.
- Produces: branch `refactor/app-carve-up-phase2` off master; confirmed 413-test baseline.

- [ ] **Step 1: Create the branch**

```bash
cd ~/utono/linux-lit
git checkout master
git checkout -b refactor/app-carve-up-phase2
```

- [ ] **Step 2: Capture the test-count baseline**

Run:
```bash
cargo test --bins 2>&1 | rg 'test result'
```
Expected: `413 passed`. Record it; it is the invariant for every later task.

- [ ] **Step 3: Capture the clippy baseline**

Run:
```bash
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: `115`. Record it.

(No commit — this task only branches and records baselines.)

---

### Task 1: Extract `src/app/formatting.rs`

**Files:**
- Create: `src/app/formatting.rs`
- Modify: `src/app/mod.rs` (remove 6 fns; add `pub mod formatting;` + `use self::formatting::{...}`)
- Modify: `src/input/actions/settings.rs` (repath 3 call lines)
- Modify: `src/input/keymap.rs` (repath 1 call line)
- Modify: `src/input/actions/authorship.rs` (repath 1 call line)

**Interfaces:**
- Consumes: `src/app/mod.rs` from Task 0.
- Produces: module `crate::app::formatting` exposing `pub(crate) fn apply_dialogue_formatting(state: &mut AppState)`, `pub(crate) fn apply_authorship_formatting(state: &mut AppState)`, `pub(crate) fn apply_scansion_marks(...)`, `pub(crate) fn apply_bcp_formatting(state: &mut AppState)`. `apply_stanza_number_centering` and `char_offset` stay private to the module. `mod.rs`'s build_window/display_work/rebuild_buffer_text call the first three via `use self::formatting::{...}`.

- [ ] **Step 1: Create the new module file with the moved code**

Move these items **verbatim** from `src/app/mod.rs` into a new file `src/app/formatting.rs` (locate by name with `rg -n 'fn apply_dialogue_formatting|fn apply_bcp_formatting|fn apply_scansion_marks|fn apply_stanza_number_centering|fn apply_authorship_formatting|fn char_offset' src/app/mod.rs`):

- `apply_scansion_marks` — **bump private → `pub(crate)`** (reverse-called by `rebuild_buffer_text` in mod.rs).
- `apply_stanza_number_centering` — keep PRIVATE.
- `apply_dialogue_formatting` — **bump `pub` → `pub(crate)`** (reverse-called by build_window + display_work_at_with_prepared; external callers also use it via the new path).
- `apply_bcp_formatting` — **bump `pub` → `pub(crate)`** (no external callers; called only by apply_dialogue_formatting — narrowing the over-broad pub).
- `char_offset` — keep PRIVATE.
- `apply_authorship_formatting` — **bump `pub` → `pub(crate)`** (reverse-called by display_work; external callers use the new path).

At the top of `src/app/formatting.rs`, add the imports these need. They reference `AppState` + its methods (one_section_per_page, column_count, work_line_for_buffer), the consts that STAY in mod.rs (`DIALOGUE_INDENT`, `TWO_COLUMN_DIALOGUE_INDENT`, `BCP_SENTENCE_GAP` — all already `pub const`), and external crates. Start with:

```rust
use super::{AppState, DIALOGUE_INDENT, TWO_COLUMN_DIALOGUE_INDENT, BCP_SENTENCE_GAP};
use crate::logging::log;
```

Then run `cargo build` (Step 3) and add EXACTLY the `use` lines the compiler names: `gtk4::prelude::{...}` for buffer/tag/iter trait methods, `crate::db::line_types::{...}` (the is_speaker / is_stage_direction / is_bcp_* / divine_name_spans / bcp_smallcaps_spans / is_stanza_number family), `crate::scansion::{mark_line, ScanLevel, LineScansion}`, `crate::text_file_map::LineMap`, `crate::db::models::Line`. Do NOT bulk-guess — add only what the compiler reports, so no unused-import warning appears. If one of the three `super::` consts turns out unused by these fns, remove it from the import.

- [ ] **Step 2: Wire the module into `mod.rs`**

In `src/app/mod.rs`:
- Delete the 6 functions you moved.
- Add `pub mod formatting;` near the top (pub, because settings.rs/keymap.rs/authorship.rs reference `crate::app::formatting::...`).
- Add an internal import so mod.rs's retained callers (build_window, display_work_at_with_prepared, rebuild_buffer_text) still resolve the moved fns unqualified:

```rust
use self::formatting::{apply_dialogue_formatting, apply_authorship_formatting, apply_scansion_marks};
```

(`apply_bcp_formatting` is NOT imported here — it has no mod.rs caller; it's called only from inside `apply_dialogue_formatting`, which now lives in formatting.rs. If `cargo build` reports otherwise, add it.)

- [ ] **Step 3: Build**

Run:
```bash
cargo build
```
Expected: clean. Resolve import errors by adding the exact `use` the compiler names (Step 1). If a private-visibility error reaches `apply_dialogue_formatting`/`apply_authorship_formatting`/`apply_scansion_marks` from mod.rs, confirm the `pub(crate)` bumps and the `use self::formatting::{...}` line.

- [ ] **Step 4: Repath the external call sites**

In `src/input/actions/settings.rs` (3 sites — lines ~40, ~329, ~447, locate by `rg -n 'crate::app::apply_dialogue_formatting' src/input/actions/settings.rs`):
- `crate::app::apply_dialogue_formatting(` → `crate::app::formatting::apply_dialogue_formatting(` (each occurrence)

In `src/input/keymap.rs` (locate by `rg -n 'crate::app::apply_authorship_formatting' src/input/keymap.rs`, ~line 2299):
- `crate::app::apply_authorship_formatting(` → `crate::app::formatting::apply_authorship_formatting(`

In `src/input/actions/authorship.rs` (~line 31):
- `crate::app::apply_authorship_formatting(` → `crate::app::formatting::apply_authorship_formatting(`

Verify zero un-repathed sites remain:
```bash
rg -n 'crate::app::(apply_dialogue_formatting|apply_authorship_formatting|apply_scansion_marks|apply_bcp_formatting)\b' src/ | rg -v 'formatting::'
```
Expected: no output.

- [ ] **Step 5: Rebuild after repath**

Run:
```bash
cargo build
```
Expected: clean.

- [ ] **Step 6: Clippy**

Run:
```bash
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: 115 (baseline). If higher, the most likely cause is an unused `use` left in mod.rs — remove it.

- [ ] **Step 7: Test-count invariant**

Run:
```bash
cargo test --bins 2>&1 | rg 'test result'
```
Expected: `413 passed`.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(app): extract formatting family into src/app/formatting.rs

Pure code motion: the per-line reader-buffer typographers (dialogue/BCP/
scansion/stanza-centering/authorship + char_offset helper) move out of
app.rs into a sibling module. apply_dialogue_formatting/apply_authorship_
formatting/apply_scansion_marks bumped to pub(crate) (build_window/
display_work/rebuild_buffer_text reverse-call them); apply_bcp_formatting
narrowed pub -> pub(crate); stanza-centering + char_offset stay private.
dialogue-indent consts stay in mod.rs (shared with setup_gutter), imported
via use super. Call sites in settings/keymap/authorship repathed (no
facade). 413 tests unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Extract `src/app/scene_synopsis.rs`

**Files:**
- Create: `src/app/scene_synopsis.rs`
- Modify: `src/app/mod.rs` (remove ~21 fns + `SYNOPSIS_WHOLE_WORK`; add `pub mod scene_synopsis;` + `use`; repath the in-mod.rs reverse calls)
- Modify: `src/app/vocab_popup.rs` (bump `update_vocab_popup_margin` `pub(super)` → `pub(crate)`)
- Modify: `src/main.rs`, `src/input/keymap.rs`, `src/input/navigation.rs`, `src/input/scroll.rs`, `src/input/highlight.rs`, `src/input/actions/journal.rs`, `src/input/actions/synopsis.rs` (repath call lines)

**Interfaces:**
- Consumes: `src/app/mod.rs` + `src/app/vocab_popup.rs` from Task 1.
- Produces: module `crate::app::scene_synopsis` exposing the scene/synopsis fns. Externally-used `pub`: `synopsis_label`, `current_scene_divs`, `divs_at_buffer_line`, `scene_text_for`, `toggle_synopsis`, `show_synopsis_overlay`, `scene_label`, `scene_label_for`, `cycle_synopsis`, `update_title_bar_scene`, `is_first_line_of_scene`. `scene_heading_start` is `pub(crate)` (reverse-called by display_work). Translations (Task 3) will import `current_scene_divs` + `synopsis_label` from here.

- [ ] **Step 1: Bump the vocab_popup helper (required cross-module fix)**

In `src/app/vocab_popup.rs`, change `update_vocab_popup_margin` from `pub(super)` to `pub(crate)`:

```rust
// before:
pub(super) fn update_vocab_popup_margin(state: &AppState) {
// after:
pub(crate) fn update_vocab_popup_margin(state: &AppState) {
```

Why: `show_synopsis` (moving into the sibling `scene_synopsis` module this task) calls it. `pub(super)` grants access to the PARENT module (`app`/mod.rs) only — NOT to sibling modules — so the call would not compile without this bump.

- [ ] **Step 2: Create the new module file with the moved code**

Move these items **verbatim** from `src/app/mod.rs` into a new file `src/app/scene_synopsis.rs` (locate each by name). Functions: `is_chapter_work`, `current_chapter_number`, `chapter_number_from_flags`, `current_synopsis_key`, `whole_work_label`, `synopsis_label`, `current_scene_divs`, `divs_at_buffer_line`, `scene_text_for`, `is_first_line_of_scene`, `scene_heading_start`, `show_synopsis`, `toggle_synopsis`, `show_synopsis_overlay`, `scene_label`, `scene_label_for`, `prepend_whole_work`, `ordered_synopsis_scenes`, `clamp_synopsis_index`, `cycle_synopsis`, `update_title_bar_scene`. Plus the const `SYNOPSIS_WHOLE_WORK`.

**Do NOT move `sync_translation_overlay`, `show_translation_overlay`, or `rebuild_translation_overlay`** — those are translation functions (Task 3), even though they sit in the same source region. **Do NOT move `JOURNAL_WORK_DIV`** — it is journal-owned (external caller in journal.rs) and stays in mod.rs.

Visibility:
- `scene_heading_start` — **bump private → `pub(crate)`** (reverse-called by display_work_at_with_prepared in mod.rs).
- Keep `whole_work_label`, `prepend_whole_work`, `ordered_synopsis_scenes`, `clamp_synopsis_index` PRIVATE (in-family only).
- Keep every currently-`pub` fn `pub`.
- `SYNOPSIS_WHOLE_WORK` keeps its `pub(crate)`.

Top-of-file imports for `scene_synopsis.rs` — start with:

```rust
use super::{AppState, InputMode, SidebarMode, overlay_card_size};
use crate::app::vocab_popup::{open_vocab_popup, close_vocab_popup, update_vocab_popup_margin};
use crate::logging::log;
```

Then `cargo build` and add EXACTLY what the compiler names (GTK prelude items, `crate::input::actions::gloss::recolor_cached_blocks`, db/text_file_map types, etc.). Remove any of the starter `super::` imports the compiler reports unused.

- [ ] **Step 3: Wire the module into `mod.rs`**

In `src/app/mod.rs`:
- Delete the ~21 functions + `SYNOPSIS_WHOLE_WORK` you moved.
- Add `pub mod scene_synopsis;` near the top.
- Add an internal import for the two fns mod.rs's display_work reverse-calls:

```rust
use self::scene_synopsis::{is_first_line_of_scene, scene_heading_start};
```

(If `cargo build` reports any other scene fn used unqualified inside mod.rs — e.g. by a retained helper — add it to this list. Only the ones the compiler names.)

- [ ] **Step 4: Build**

Run:
```bash
cargo build
```
Expected: clean. If `update_vocab_popup_margin` still errors as unreachable, confirm Step 1's `pub(crate)` bump. If `scene_heading_start`/`is_first_line_of_scene` error from mod.rs, confirm the `use self::scene_synopsis::{...}` and the `pub(crate)` bump.

- [ ] **Step 5: Repath the external call sites**

Repath each `crate::app::X` → `crate::app::scene_synopsis::X` (locate each with `rg -n`). Sites:
- `synopsis_label` — `src/input/actions/journal.rs` (×2), `src/input/actions/synopsis.rs` (×2)
- `current_scene_divs` — `src/main.rs` (×2), `src/input/keymap.rs` (×1), `src/input/navigation.rs` (×2), `src/input/actions/journal.rs` (×1)
- `divs_at_buffer_line` — `src/input/scroll.rs` (×1)
- `scene_text_for` — `src/input/actions/journal.rs` (×1)
- `toggle_synopsis` — `src/input/keymap.rs` (×1)
- `show_synopsis_overlay` — `src/input/keymap.rs` (×1)
- `scene_label` — `src/input/actions/journal.rs` (×1)
- `scene_label_for` — `src/input/scroll.rs` (×1), `src/input/navigation.rs` (×2)
- `cycle_synopsis` — `src/input/keymap.rs` (×2)
- `update_title_bar_scene` — `src/input/keymap.rs` (×1), `src/input/highlight.rs` (×2)

Verify zero un-repathed sites remain:
```bash
rg -n 'crate::app::(synopsis_label|current_scene_divs|divs_at_buffer_line|scene_text_for|toggle_synopsis|show_synopsis_overlay|scene_label|scene_label_for|cycle_synopsis|update_title_bar_scene)\b' src/ | rg -v 'scene_synopsis::'
```
Expected: no output. (Note: `scene_label_for` and `scene_label` — the regex `scene_label\b` will not match `scene_label_for` because of the word boundary after `scene_label`; both are listed explicitly so check both.)

- [ ] **Step 6: Rebuild**

Run:
```bash
cargo build
```
Expected: clean.

- [ ] **Step 7: Clippy + test invariant**

Run:
```bash
cargo clippy 2>&1 | rg -c 'warning:'
cargo test --bins 2>&1 | rg 'test result'
```
Expected: clippy 115; `413 passed`.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(app): extract scene_synopsis family into src/app/scene_synopsis.rs

Pure code motion: ~21 scene-boundary / synopsis-key / synopsis-overlay /
title-bar fns + SYNOPSIS_WHOLE_WORK move out of app.rs into a sibling
module. scene_heading_start bumped private -> pub(crate) (display_work
reverse-calls it). vocab_popup::update_vocab_popup_margin bumped pub(super)
-> pub(crate) (show_synopsis, now a sibling module, calls it — siblings
can't see pub(super)). JOURNAL_WORK_DIV + the translation-overlay fns left
in mod.rs. Call sites in main/keymap/navigation/scroll/highlight/journal/
synopsis repathed (no facade). 413 tests unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Extract `src/app/translations.rs`

**Files:**
- Create: `src/app/translations.rs`
- Modify: `src/app/mod.rs` (remove 10 fns; add `pub mod translations;`)
- Modify: `src/input/keymap.rs`, `src/input/gamepad.rs`, `src/input/actions/escape.rs`, `src/input/search.rs`, `src/input/navigation.rs`, `src/main.rs` (repath call lines)

**Interfaces:**
- Consumes: `src/app/mod.rs`, `src/app/scene_synopsis.rs`, `src/app/font.rs` from Task 2/Phase 1.
- Produces: module `crate::app::translations` exposing `pub` fns `toggle_translations`, `hide_translations_for_navigation`, `show_translation_overlay`, `sync_translation_overlay` (and `rebuild_translation_overlay` kept `pub`, no external caller). The six other fns private. No reverse-dep bump (build_window/display_work don't call any translation fn).

- [ ] **Step 1: Create the new module file with the moved code**

Move these items **verbatim** from `src/app/mod.rs` into a new file `src/app/translations.rs` (locate each by name): `toggle_translations`, `show_translations`, `hide_translations`, `hide_translations_for_navigation`, `strip_translation_lines`, `map_line_after_insert`, `map_line_before_insert`, `show_translation_overlay`, `sync_translation_overlay`, `rebuild_translation_overlay`.

Visibility — keep exactly as-is (no bumps): `toggle_translations` `pub`, `hide_translations_for_navigation` `pub`, `show_translation_overlay` `pub`, `sync_translation_overlay` `pub`, `rebuild_translation_overlay` `pub`; the other five PRIVATE (`show_translations`, `hide_translations`, `strip_translation_lines`, `map_line_after_insert`, `map_line_before_insert`).

Top-of-file imports for `translations.rs` — the overlay cluster needs the scene helpers from Task 2 (now reachable as siblings). Start with:

```rust
use super::{AppState, apply_column_layout, overlay_card_size};
use crate::app::scene_synopsis::{current_scene_divs, synopsis_label};
use crate::app::font::{reapply_font, rebuild_line_number_gutter};
use crate::logging::log;
```

Then `cargo build` and add EXACTLY what the compiler names: `gtk4::prelude::{...}`, `crate::ui::translation_overlay::group_scene_into_blocks`, `crate::input::timestamps::redraw_sign_gutters`, `crate::input::navigation::{invalidate_page_tops, update_highlight_only, refresh_bottom_clip}`, `crate::input::scroll::scrolloff_bottom_clip_widgets`. Remove any starter import the compiler reports unused.

- [ ] **Step 2: Wire the module into `mod.rs`**

In `src/app/mod.rs`:
- Delete the 10 functions you moved.
- Add `pub mod translations;` near the top.
- No `use self::translations::{...}` is needed UNLESS mod.rs's retained code calls a translation fn unqualified. (Inventory says build_window/display_work only touch `state.translations_visible` as a field, not the fns.) If `cargo build` reports an unqualified translation-fn use in mod.rs, add it to a `use self::translations::{...}` line.

- [ ] **Step 3: Build**

Run:
```bash
cargo build
```
Expected: clean. If `current_scene_divs`/`synopsis_label` error, confirm the `use crate::app::scene_synopsis::{...}` import (Task 2 already made them reachable as `pub`).

- [ ] **Step 4: Repath the external call sites**

Repath each `crate::app::X` → `crate::app::translations::X` (locate with `rg -n`):
- `toggle_translations` — `src/input/keymap.rs` (×1), `src/input/gamepad.rs` (×1), `src/input/actions/escape.rs` (×1)
- `hide_translations_for_navigation` — `src/input/search.rs` (×1), `src/input/navigation.rs` (×2)
- `show_translation_overlay` — `src/input/keymap.rs` (×1)
- `sync_translation_overlay` — `src/main.rs` (×2), `src/input/keymap.rs` (×1)

Verify zero un-repathed sites remain:
```bash
rg -n 'crate::app::(toggle_translations|hide_translations_for_navigation|show_translation_overlay|sync_translation_overlay)\b' src/ | rg -v 'translations::'
```
Expected: no output.

- [ ] **Step 5: Rebuild**

Run:
```bash
cargo build
```
Expected: clean.

- [ ] **Step 6: Clippy + test invariant**

Run:
```bash
cargo clippy 2>&1 | rg -c 'warning:'
cargo test --bins 2>&1 | rg 'test result'
```
Expected: clippy 115; `413 passed`.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor(app): extract translations family into src/app/translations.rs

Pure code motion: the inline-gloss interleave path + the two-column
translation overlay (10 fns incl. sync_translation_overlay) move out of
app.rs into a sibling module. No reverse-dep bumps (build_window/
display_work only read the translations_visible field). The overlay
cluster's current_scene_divs/synopsis_label deps resolve via
use crate::app::scene_synopsis (extracted first). Call sites in keymap/
gamepad/escape/search/navigation/main repathed (no facade). 413 tests
unchanged. Completes the tier-a carve-up.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Finish the branch (merge to master)

**Files:** none (git only) + audit ledger update.

**Interfaces:**
- Consumes: all three extracted modules from Tasks 1–3.
- Produces: Phase 2 merged to master and pushed; branch deleted; audit ledger updated.

- [ ] **Step 1: Final full verification on the branch**

Run:
```bash
cargo build && cargo test --bins 2>&1 | rg 'test result' && cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: clean build, `413 passed`, clippy 115. Confirm `git status` is clean. Confirm `mod.rs` shrank:
```bash
wc -l src/app/*.rs
```
Expected: `mod.rs` ~4,200; new files formatting.rs ~560, scene_synopsis.rs ~790, translations.rs ~520.

- [ ] **Step 2: Merge to master (per CLAUDE.md "Finishing a Branch")**

```bash
git checkout master
git merge --no-ff refactor/app-carve-up-phase2
```

- [ ] **Step 3: Re-verify on the merged result**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result'
```
Expected: clean build, `413 passed`.

- [ ] **Step 4: Push and delete the branch**

```bash
git push origin master
git branch -d refactor/app-carve-up-phase2
```

- [ ] **Step 5: Update the audit ledger**

In `docs/superpowers/audit-opportunities.md`, update the `app.rs module carve-up` entry to record Phase 2 DONE: the three families (formatting, scene_synopsis, translations) extracted, the new mod.rs line count, and that the entire **tier-a** carve-up is now complete — only the behavior-risky **tier-b** targets (build_window/display_work/layout) and the AppState god-struct remain parked. Commit:

```bash
git add docs/superpowers/audit-opportunities.md
git commit -m "docs(audit): mark app.rs carve-up Phase 2 (tier-a families) DONE

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push origin master
```

---

## Self-Review

**Spec coverage:**
- formatting module + 4 visibility bumps (spec Module 1) → Task 1 ✓
- scene_synopsis module + scene_heading_start bump + SYNOPSIS_WHOLE_WORK moves / JOURNAL_WORK_DIV stays + sync_translation_overlay stays out (spec Module 2) → Task 2 ✓
- update_vocab_popup_margin pub(super)→pub(crate) required bump (spec Module 2 "Cross-module visibility bump (required)") → Task 2 Step 1 ✓
- translations module, no reverse bumps, overlay deps via scene_synopsis (spec Module 3) → Task 3 ✓
- Extraction order formatting → scene_synopsis → translations (spec "Extraction order") → Task numbering + Global Constraints ✓
- No facade / repath each call site (spec Goals, Mechanics) → each task's repath step + the `rg` verification ✓
- 413-test + clippy-115 invariants (spec Verification) → Global Constraints + each task ✓
- No e2e (spec Verification) → Global Constraints ✓
- Tier-b / AppState out of scope (spec Out of scope) → no task touches build_window/display_work/layout/AppState fields ✓

**Placeholder scan:** No TBD/TODO. Every repath is exact (file + fn + count + `rg` verify). The "locate by name" instructions are deliberate (line numbers shift across tasks).

**Type consistency:** Module paths consistent — `crate::app::formatting::*`, `crate::app::scene_synopsis::*`, `crate::app::translations::*`. The one inter-module edge (`translations` imports `scene_synopsis::{current_scene_divs, synopsis_label}`) matches the extraction order. The `update_vocab_popup_margin` bump is spelled identically in spec and plan. `sync_translation_overlay` is assigned to translations (Task 3) in both, with an explicit "do not move" guard in Task 2.
