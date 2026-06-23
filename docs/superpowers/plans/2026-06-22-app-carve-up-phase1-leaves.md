# app.rs Carve-Up Phase 1 — Leaf Modules Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract three self-contained leaf families (`vocab_popup`, `font`, `text_prep`) out of `src/app.rs` into sibling modules under a new `src/app/` directory, via pure code motion with no behavior change.

**Architecture:** Convert `src/app.rs` → `src/app/mod.rs` (a directory module; `src/main.rs` keeps `mod app;` unchanged). Move each family verbatim into `src/app/<name>.rs`. Two private items that `build_window` still uses become `pub(crate)`. No re-export facade: every external call site is repathed directly from `crate::app::foo` to `crate::app::<module>::foo`. The existing 413-test suite is the regression check — there are no new tests.

**Tech Stack:** Rust, GTK4 / sourceview5, `cargo build` / `cargo test --bins` / `cargo clippy`.

## Global Constraints

- **Scope class: behavior-preserving code motion.** No logic edits. The only signature-level changes permitted are the two named visibility bumps (`font::reapply_font` → `pub(crate)`, `text_prep::SnapshotOrPrep` → `pub(crate)`).
- **No facade.** Do not re-`pub`-export moved items from `mod.rs`. External call sites are repathed directly.
- **Test-count invariant: `cargo test --bins` must report the same total before and after every task.** Baseline is 413; capture the real baseline in Task 0 and hold it. Command: `cargo test --bins 2>&1 | rg 'test result'`.
- **No e2e/cage run needed** — this is "logic unchanged, still compiles/tests", not a "renders on screen" change (per CLAUDE.md).
- **One module per task = one PR**, in order: vocab_popup → font → text_prep. Each task is independently mergeable.
- Leave the unrelated `use crate::app::AppState;` / `use crate::app::InputMode;` imports untouched — they do not import any moving name.
- Branch off `master` (current branch is `master`, so branch without asking). Suggested branch name: `refactor/app-carve-up-phase1`.

---

### Task 0: Branch + directory conversion + baseline

**Files:**
- Rename: `src/app.rs` → `src/app/mod.rs` (via `git mv`)

**Interfaces:**
- Consumes: nothing.
- Produces: `src/app/mod.rs` exists; `src/main.rs`'s `mod app;` resolves to it unchanged. The new `src/app/` directory is the slot for the three new modules.

- [ ] **Step 1: Create the branch**

```bash
cd ~/utono/linux-lit
git checkout -b refactor/app-carve-up-phase1
```

- [ ] **Step 2: Capture the test-count baseline**

Run:
```bash
cargo test --bins 2>&1 | rg 'test result'
```
Record the total number of passed tests (expected `413`; use the real number if it differs — that becomes the invariant for every later task).

- [ ] **Step 3: Convert app.rs to a directory module**

```bash
git mv src/app.rs src/app/mod.rs
```

- [ ] **Step 4: Verify the build is unchanged**

Run:
```bash
cargo build
```
Expected: clean build. (`mod app;` in `src/main.rs` resolves `src/app/mod.rs` identically to the old `src/app.rs` — no source edit required.)

- [ ] **Step 5: Verify the test count is unchanged**

Run:
```bash
cargo test --bins 2>&1 | rg 'test result'
```
Expected: same total as Step 2 (413).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(app): convert app.rs to directory module (no code change)

Pure rename src/app.rs -> src/app/mod.rs to open a slot for sibling
leaf modules. mod app; in main.rs is unchanged. No logic change.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 1: Extract `src/app/vocab_popup.rs` (cleanest leaf)

**Files:**
- Create: `src/app/vocab_popup.rs`
- Modify: `src/app/mod.rs` (remove the moved fns; add `mod vocab_popup;` + `use`)
- Modify: `src/input/keymap.rs` (repath 6 call lines)
- Modify: `src/input/highlight.rs` (repath 2 call lines)
- Modify: `src/main.rs` (repath 1 call line)

**Interfaces:**
- Consumes: `src/app/mod.rs` from Task 0.
- Produces: module `crate::app::vocab_popup` exposing `pub` fns `open_vocab_popup`, `close_vocab_popup`, `refresh_vocab_popup`, `vocab_popup_next`, `vocab_popup_prev`, `vocab_popup_toggle_view`, `show_vocab_popup`. Signatures unchanged from their current `app.rs` definitions. `update_vocab_popup_margin` and `format_etymology` stay private to the module.

- [ ] **Step 1: Create the new module file with the moved code**

Move these items **verbatim** from `src/app/mod.rs` into a new file `src/app/vocab_popup.rs` (current source ranges: `open_vocab_popup` 6324–6384, `update_vocab_popup_margin` 6385–6402, `close_vocab_popup` 6403–6407, `show_vocab_popup` 6408–6430, `refresh_vocab_popup` 6431–6490, `vocab_popup_next` 6491–6498, `vocab_popup_prev` 6499–6511, `vocab_popup_toggle_view` 6512–6521, `format_etymology` 6522–6559):

- `open_vocab_popup`, `close_vocab_popup`, `refresh_vocab_popup`, `vocab_popup_next`, `vocab_popup_prev`, `vocab_popup_toggle_view`, `show_vocab_popup` — keep their existing `pub` visibility.
- `update_vocab_popup_margin`, `format_etymology` — keep them private (no `pub`).

At the top of `src/app/vocab_popup.rs` add the imports these functions need. They reference `AppState`, GTK/glib types, `crate::db::queries::*`, and `crate::ui::vocab_popup::*`. Start with:

```rust
use super::AppState;
use crate::logging::log;
```

Then run `cargo build` (next step) and let the compiler name every missing import — add each `use` it reports (GTK prelude items, `glib`, `crate::db::queries::...`, `crate::ui::vocab_popup::...`). Do **not** guess-and-bulk-import; add exactly what the errors demand so no unused-import clippy lint appears.

- [ ] **Step 2: Wire the module into `mod.rs`**

In `src/app/mod.rs`, delete the nine functions you just moved (lines 6324–6559). Add the module declaration and a `use` so the retained code in `mod.rs` (if any calls these) still resolves. Place near the other `mod`/`use` lines at the top of `mod.rs`:

```rust
mod vocab_popup;
```

`mod.rs` itself does not call any vocab_popup fn (verified: all callers are external), so no `use self::vocab_popup::...` is required. If `cargo build` later reports an unresolved `vocab_popup` name inside `mod.rs`, add the specific `use self::vocab_popup::<name>;` it asks for.

- [ ] **Step 3: Repath the external call sites**

In `src/input/keymap.rs`, change the `crate::app::` prefix to `crate::app::vocab_popup::` on these exact lines:

- L2057: `crate::app::open_vocab_popup(&mut s);` → `crate::app::vocab_popup::open_vocab_popup(&mut s);`
- L2059: `crate::app::close_vocab_popup(&mut s);` → `crate::app::vocab_popup::close_vocab_popup(&mut s);`
- L2261: `crate::app::vocab_popup_toggle_view(&mut state.borrow_mut());` → `crate::app::vocab_popup::vocab_popup_toggle_view(&mut state.borrow_mut());`
- L2332: `crate::app::vocab_popup_next(&mut state.borrow_mut());` → `crate::app::vocab_popup::vocab_popup_next(&mut state.borrow_mut());`
- L2334: `crate::app::vocab_popup_prev(&mut state.borrow_mut());` → `crate::app::vocab_popup::vocab_popup_prev(&mut state.borrow_mut());`
- L2337: `crate::app::open_vocab_popup(&mut state.borrow_mut());` → `crate::app::vocab_popup::open_vocab_popup(&mut state.borrow_mut());`

In `src/input/highlight.rs`:

- L166: `crate::app::refresh_vocab_popup(state);` → `crate::app::vocab_popup::refresh_vocab_popup(state);`
- L168: `crate::app::open_vocab_popup(state);` → `crate::app::vocab_popup::open_vocab_popup(state);`

In `src/main.rs`:

- L315: `crate::app::refresh_vocab_popup(&mut s);` → `crate::app::vocab_popup::refresh_vocab_popup(&mut s);`

Do **not** touch `gloss_overlay.adjust_font_size(...)`, `handle_vocab_popup_key`, `auto_show_vocab_popup`, the `s.vocab_popup*` fields, or the `VocabPopup*`/`ToggleVocabPopup` enum variants — none are moving functions.

- [ ] **Step 4: Build**

Run:
```bash
cargo build
```
Expected: clean. Resolve any import errors by adding the exact `use` the compiler names (Step 1 note). If it reports a private-visibility error reaching one of the moved `pub` fns, confirm you kept the `pub` keyword on it.

- [ ] **Step 5: Clippy**

Run:
```bash
cargo clippy 2>&1 | rg -i 'warning|error' || echo 'clippy clean'
```
Expected: no new warnings. Most likely culprit is an unused `use` left behind in `mod.rs` — remove any import that only served the moved fns.

- [ ] **Step 6: Test-count invariant**

Run:
```bash
cargo test --bins 2>&1 | rg 'test result'
```
Expected: same total as Task 0 baseline (413). A drop means a `#[cfg(test)]` block referencing a moved fn failed to compile — fix the path, don't delete the test.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor(app): extract vocab_popup family into src/app/vocab_popup.rs

Pure code motion: the vocab-popup widget family (open/close/refresh/
next/prev/toggle/show + 2 private helpers) moves out of app.rs into a
sibling module. Call sites in keymap.rs/highlight.rs/main.rs repathed
directly (no facade). No visibility changes, no logic change. 413 tests
unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Extract `src/app/font.rs`

**Files:**
- Create: `src/app/font.rs`
- Modify: `src/app/mod.rs` (remove moved fns; add `mod font;` + `use self::font::reapply_font;`; bump `reapply_font` visibility)
- Modify: `src/input/keymap.rs` (repath 6 call lines / 8 calls)

**Interfaces:**
- Consumes: `src/app/mod.rs` from Task 1; the `pub(crate)` helper `line_number_gutter_geometry` and `pub` consts `TOP_SPACER_HEIGHT`, `SHOW_LINE_NUMBERS_TWO_COL` that remain in `mod.rs`.
- Produces: module `crate::app::font` exposing `pub` fns `adjust_font_size`, `reset_font_size`, `cycle_font`, `show_font_info`, and `pub(crate) fn reapply_font` (consumed by `mod.rs::build_window`). `update_spacer_heights`, `rebuild_line_number_gutter` stay private.

- [ ] **Step 1: Create the new module file with the moved code**

Move these items **verbatim** from `src/app/mod.rs` into a new file `src/app/font.rs` (current source ranges before Task 1's deletions shift them — locate by name, not absolute line: `update_spacer_heights` ~5095–5098, `reapply_font` ~5099–5136, `rebuild_line_number_gutter` ~5137–5221, `adjust_font_size` ~5222–5235, `reset_font_size` ~5236–5249, `cycle_font` ~5250–5273, `show_font_info` ~5274–5285):

- `adjust_font_size`, `reset_font_size`, `cycle_font`, `show_font_info` — keep `pub`.
- `reapply_font` — **change its visibility from private to `pub(crate)`** (it is called by `build_window`, which stays in `mod.rs`).
- `update_spacer_heights`, `rebuild_line_number_gutter` — keep private.

At the top of `src/app/font.rs` add the imports these functions need. They reference `AppState`, GTK/sourceview/glib, `crate::input::navigation::{resnap_page, invalidate_page_tops}`, `crate::config::save`, `crate::gutter::*`, `crate::theme::generate_css`, and the items that stay in `mod.rs`:

```rust
use super::{AppState, line_number_gutter_geometry, TOP_SPACER_HEIGHT, SHOW_LINE_NUMBERS_TWO_COL};
use crate::logging::log;
```

Then `cargo build` and add each remaining `use` the compiler names (Step 4). Confirm `line_number_gutter_geometry` is reachable — it is already `pub(crate)` in `mod.rs`, so the `use super::line_number_gutter_geometry;` above resolves.

- [ ] **Step 2: Wire the module into `mod.rs` and fix `build_window`'s call**

In `src/app/mod.rs`:
- Delete the seven functions you moved.
- Add `mod font;` near the top.
- Add `use self::font::reapply_font;` so `build_window`'s existing call `reapply_font(...)` (app.rs ~2452) still resolves with no edit to the call line. (Alternatively repath that single call to `font::reapply_font(...)` — either is fine; the `use` keeps the diff to `build_window` zero.)

- [ ] **Step 3: Repath the external call sites in `src/input/keymap.rs`**

Change the `crate::app::` prefix to `crate::app::font::` on these exact lines (note L2111/L2112 each have two calls):

- L2111: `{ crate::app::adjust_font_size(&mut state.borrow_mut(), 1); crate::app::show_font_info(&state.borrow()); }` → `{ crate::app::font::adjust_font_size(&mut state.borrow_mut(), 1); crate::app::font::show_font_info(&state.borrow()); }`
- L2112: `{ crate::app::adjust_font_size(&mut state.borrow_mut(), -1); crate::app::show_font_info(&state.borrow()); }` → `{ crate::app::font::adjust_font_size(&mut state.borrow_mut(), -1); crate::app::font::show_font_info(&state.borrow()); }`
- L2113: `crate::app::reset_font_size(&mut state.borrow_mut())` → `crate::app::font::reset_font_size(&mut state.borrow_mut())`
- L2114: `crate::app::cycle_font(&mut state.borrow_mut(), true)` → `crate::app::font::cycle_font(&mut state.borrow_mut(), true)`
- L2115: `crate::app::cycle_font(&mut state.borrow_mut(), false)` → `crate::app::font::cycle_font(&mut state.borrow_mut(), false)`
- L2193: `crate::app::show_font_info(&state.borrow())` → `crate::app::font::show_font_info(&state.borrow())`

Do **not** touch the `gloss_overlay.adjust_font_size(...)` calls at L962/966/1244/1248 — those are a method on `GlossOverlay`, not the moved `crate::app` fn.

- [ ] **Step 4: Build**

Run:
```bash
cargo build
```
Expected: clean. If `reapply_font` triggers a private-in-public or unresolved error from `build_window`, confirm the `pub(crate)` bump (Step 1) and the `use self::font::reapply_font;` (Step 2) are both in place.

- [ ] **Step 5: Clippy**

Run:
```bash
cargo clippy 2>&1 | rg -i 'warning|error' || echo 'clippy clean'
```
Expected: no new warnings. Remove any now-unused `use` left in `mod.rs`.

- [ ] **Step 6: Test-count invariant**

Run:
```bash
cargo test --bins 2>&1 | rg 'test result'
```
Expected: same total as the Task 0 baseline (413).

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor(app): extract font family into src/app/font.rs

Pure code motion: the font-size/line-number-gutter-rebuild family moves
out of app.rs into a sibling module. reapply_font bumped private ->
pub(crate) because build_window still calls it. Call sites in keymap.rs
repathed directly (no facade). 413 tests unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Extract `src/app/text_prep.rs` (pure / GTK-free)

**Files:**
- Create: `src/app/text_prep.rs`
- Modify: `src/app/mod.rs` (remove moved items; add `mod text_prep;` + `use`; bump `SnapshotOrPrep` visibility; repath `build_window`'s uses of the moved types)
- Modify: `src/input/actions/pickers.rs` (repath 4 lines)
- Modify: `src/input/actions/concordance.rs` (repath 1 line)
- Modify: `src/input/actions/echoes.rs` (repath 1 line)

**Interfaces:**
- Consumes: `src/app/mod.rs` from Task 2.
- Produces: module `crate::app::text_prep` exposing `pub struct PreparedTextOnly`, `pub struct PreparedText`, `pub(crate) enum SnapshotOrPrep`, and `pub` fns `prepare_text_for_display`, `prepare_text_only`, `build_line_map_for_prepared`. `clean_file_lines` stays private. `build_window` (in `mod.rs`) constructs `prepare_text_only`/`SnapshotOrPrep`/`PreparedText` via the new module path.

- [ ] **Step 1: Create the new module file with the moved code**

Move these items **verbatim** from `src/app/mod.rs` into a new file `src/app/text_prep.rs` (locate by name; ranges shift after Tasks 1–2: `PreparedTextOnly` ~3403–3418, `PreparedText` ~3419–3433, `SnapshotOrPrep` ~3434–3456, `clean_file_lines` ~3457–3520, `prepare_text_only` ~3521–3552, `build_line_map_for_prepared` ~3553–3567, `prepare_text_for_display` ~3568–3733):

- `PreparedTextOnly`, `PreparedText` — keep `pub`.
- `SnapshotOrPrep` — **change its visibility from private to `pub(crate)`** (constructed and matched by `build_window` in `mod.rs`).
- `prepare_text_for_display`, `prepare_text_only`, `build_line_map_for_prepared` — keep `pub`. (`build_line_map_for_prepared` is dead/unused but moves as-is — do not delete it.)
- `clean_file_lines` — keep private.

At the top of `src/app/text_prep.rs` add the imports. These are GTK-free: they reference `crate::db::line_types::*`, `crate::text_file_map::*` (incl. `LineMap`), `crate::db::models::Line`, the `Work` type, and `crate::logging::log`. Start with:

```rust
use crate::logging::log;
```

Then `cargo build` and add each `use` the compiler names. There is no `use super::AppState` needed unless the compiler asks — these functions are pure and take `&Work` / slices, not `AppState`.

- [ ] **Step 2: Wire the module into `mod.rs` and repath `build_window`'s uses**

In `src/app/mod.rs`:
- Delete the moved structs/enum/fns.
- Add `mod text_prep;` near the top.
- Add the imports `build_window` and any other retained code need:

```rust
use self::text_prep::{PreparedText, PreparedTextOnly, SnapshotOrPrep, prepare_text_only};
```

`build_window` (~lines 2412–2508) constructs `SnapshotOrPrep`, calls `prepare_text_only`, and builds `PreparedText` inline. With the `use` above, those references resolve unqualified and `build_window`'s body needs no per-line edit. If `cargo build` reports any of these still unresolved in `mod.rs`, add the missing name to the `use self::text_prep::{...}` list.

- [ ] **Step 3: Repath the external call sites**

In `src/input/actions/pickers.rs`:
- L83: `let prep = crate::app::PreparedText {` → `let prep = crate::app::text_prep::PreparedText {`
- L102: `(crate::app::prepare_text_for_display(&work), true)` → `(crate::app::text_prep::prepare_text_for_display(&work), true)`
- L262: `let prep = crate::app::PreparedText {` → `let prep = crate::app::text_prep::PreparedText {`
- L281: `(crate::app::prepare_text_for_display(&work), true)` → `(crate::app::text_prep::prepare_text_for_display(&work), true)`

In `src/input/actions/concordance.rs`:
- L402: `let prepared = crate::app::prepare_text_for_display(&work);` → `let prepared = crate::app::text_prep::prepare_text_for_display(&work);`

In `src/input/actions/echoes.rs`:
- L1457: `let prepared = crate::app::prepare_text_for_display(&work);` → `let prepared = crate::app::text_prep::prepare_text_for_display(&work);`

- [ ] **Step 4: Build**

Run:
```bash
cargo build
```
Expected: clean. If `SnapshotOrPrep` triggers a private-type error from `build_window`, confirm the `pub(crate)` bump and the `use self::text_prep::{... SnapshotOrPrep ...}`. Note `build_line_map_for_prepared` is unused — if clippy/​rustc flags it `dead_code`, that is pre-existing (it was already unused before the move); keep it and, only if a hard error blocks the build, add `#[allow(dead_code)]` matching its prior state. (It was `pub`, so it should not warn.)

- [ ] **Step 5: Clippy**

Run:
```bash
cargo clippy 2>&1 | rg -i 'warning|error' || echo 'clippy clean'
```
Expected: no new warnings beyond any that existed pre-refactor. Remove now-unused `use` lines left in `mod.rs`.

- [ ] **Step 6: Test-count invariant**

Run:
```bash
cargo test --bins 2>&1 | rg 'test result'
```
Expected: same total as the Task 0 baseline (413).

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor(app): extract text_prep family into src/app/text_prep.rs

Pure code motion: the GTK-free text-preparation family (PreparedText*,
SnapshotOrPrep, clean_file_lines, prepare_text_only/for_display,
build_line_map_for_prepared) moves out of app.rs into a sibling module.
SnapshotOrPrep bumped private -> pub(crate) because build_window
constructs/matches it. External call sites in pickers/concordance/echoes
repathed directly (no facade). 413 tests unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Finish the branch (merge to master)

**Files:** none (git only).

**Interfaces:**
- Consumes: all three extracted modules from Tasks 1–3.
- Produces: the carve-up merged to `master` and pushed; branch deleted.

- [ ] **Step 1: Final full verification on the branch**

Run:
```bash
cargo build && cargo test --bins 2>&1 | rg 'test result' && cargo clippy 2>&1 | rg -i 'warning|error' || echo 'clippy clean'
```
Expected: clean build, 413 tests, no new clippy warnings. Confirm `git status` is clean.

- [ ] **Step 2: Merge to master (per CLAUDE.md "Finishing a Branch")**

```bash
git checkout master
git merge --no-ff refactor/app-carve-up-phase1
```

- [ ] **Step 3: Re-verify on the merged result**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result'
```
Expected: clean build, 413 tests.

- [ ] **Step 4: Push and delete the branch**

```bash
git push origin master
git branch -d refactor/app-carve-up-phase1
```

- [ ] **Step 5: Update the audit ledger**

In `docs/superpowers/audit-opportunities.md`, under "Larger projects (not safe-scope)", update the `app.rs module carve-up` entry to record Phase 1 DONE: note the three leaf modules (`vocab_popup`, `font`, `text_prep`) extracted, the new `src/app/` directory, the resulting `app/mod.rs` line count, and that build_window/display_work/layout (tier-b) plus scene_synopsis/translations/formatting (tier-a, later phases) remain. Commit:

```bash
git add docs/superpowers/audit-opportunities.md
git commit -m "docs(audit): mark app.rs carve-up Phase 1 (leaf modules) DONE

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push origin master
```

---

## Self-Review

**Spec coverage:**
- vocab_popup module (spec §"1. vocab_popup") → Task 1 ✓
- font module + `reapply_font` pub(crate) bump (spec §"2. font") → Task 2 ✓
- text_prep module + `SnapshotOrPrep` pub(crate) bump + dead `build_line_map_for_prepared` moves as-is (spec §"3. text_prep") → Task 3 ✓
- `src/app.rs` → `src/app/mod.rs` directory conversion (spec §"Directory conversion") → Task 0 ✓
- No facade / repath each call site (spec §Goals, §Mechanics step 5) → Tasks 1–3 Step 3, all repaths are `crate::app::X` → `crate::app::<mod>::X` ✓
- 413-test invariant (spec §Verification) → Global Constraints + every task's Step 6 ✓
- `cargo build` / `cargo clippy` clean (spec §Verification) → every task ✓
- No e2e needed (spec §Verification) → Global Constraints ✓
- Out-of-scope items untouched (spec §Out of scope) → no task touches build_window logic, display_work, layout, scene_synopsis, translations, formatting, or AppState fields ✓

**Placeholder scan:** No TBD/TODO. Every repath shows exact before→after text. The only "locate by name, not absolute line" instructions (Tasks 2–3 Step 1) are deliberate — earlier deletions shift absolute line numbers, so the function *name* is the stable anchor; the approximate ranges are given as a finding aid.

**Type consistency:** Module paths are consistent across tasks — `crate::app::vocab_popup::*`, `crate::app::font::*`, `crate::app::text_prep::*`. The two visibility bumps (`reapply_font` → `pub(crate)`, `SnapshotOrPrep` → `pub(crate)`) are named identically in spec and plan. `build_line_map_for_prepared` spelled consistently and flagged dead-but-moves-as-is in both.
