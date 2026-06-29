# AppState Grouping Phase A — nav_test cluster Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Group the six flat `nav_test_*` fields of `AppState` into one `NavTestState` sub-struct, as the pilot for the AppState god-struct grouping project.

**Architecture:** Define `pub struct NavTestState` in `src/input/nav_test.rs` (beside its only consumer), replace the six flat `AppState` fields with one `nav_test: NavTestState` field, init it via `NavTestState::default()` in `build_window`'s `AppState { … }` literal, and rewrite every `s.nav_test_<x>` access to `s.nav_test.<x>`. This is behavior-CHANGING (field-access shape changes), but the change is mechanical — the compiler flags every missed site. Mirrors the existing `ab_repeat: AbRepeatState` idiom.

**Tech Stack:** Rust, `cargo build` / `cargo test --bins` / `cargo clippy`.

## Global Constraints

- **Scope class: behavior-CHANGING field grouping.** The rewrite is purely `s.nav_test_x` → `s.nav_test.x` — NO value changes, NO logic edits, NO control-flow changes. Runtime behavior is preserved; only the access shape changes.
- **Pure-tier verification** (per spec): `nav_test` is a test harness, provably cannot affect rendering. `cargo test --bins` (must stay **413**) + `cargo clippy` (must stay **115**) fully cover it. NO user nav-fuzz needed for this cluster.
- **The ONLY `build_window` edit** is replacing the six inline `nav_test_*: …` init lines with one `nav_test: crate::input::nav_test::NavTestState::default(),`. No structural/closure change to build_window.
- **No accessor indirection / no facade** — direct nested-field access (`s.nav_test.active`), matching the `s.ab_repeat.chunk_index` pattern.
- **Field name mapping (prefix stripped):** `nav_test_active`→`active`, `nav_test_step`→`step`, `nav_test_failures`→`failures`, `nav_test_prev_top`→`prev_top`, `nav_test_expect_return`→`expect_return`, `nav_test_fuzz`→`fuzz`.
- All `nav_test_*` access sites are in `src/input/nav_test.rs` (28 sites); `src/app/mod.rs` holds only the struct def + the init. Confirmed by `rg`.
- Already on branch `refactor/appstate-grouping` (spec committed there). Do NOT create a new branch.

---

### Task 0: Baseline

**Files:** none (verification only).

**Interfaces:**
- Consumes: branch `refactor/appstate-grouping` (exists, spec committed).
- Produces: confirmed 413-test + clippy-115 baseline.

- [ ] **Step 1: Confirm branch + baselines**

```bash
cd ~/utono/linux-lit
git branch --show-current   # expect: refactor/appstate-grouping
cargo test --bins 2>&1 | rg 'test result'
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: `413 passed`; clippy `115`. Record both. (No commit.)

---

### Task 1: Group `nav_test_*` into `NavTestState`

**Files:**
- Modify: `src/input/nav_test.rs` (define `NavTestState`; rewrite 28 access sites)
- Modify: `src/app/mod.rs` (replace 6 fields with 1; replace 6 init lines with 1)

**Interfaces:**
- Consumes: `AppState` from Task 0.
- Produces: `pub struct NavTestState { active: bool, step: usize, failures: usize, prev_top: usize, expect_return: Option<usize>, fuzz: bool }` in `crate::input::nav_test`; `AppState.nav_test: NavTestState` replacing the six flat fields.

- [ ] **Step 1: Define `NavTestState` in `src/input/nav_test.rs`**

Add near the top of `src/input/nav_test.rs` (after the existing `use` lines, before the first fn):

```rust
/// Grouped state for the in-app navigation test harness (Ctrl+Shift+T /
/// LIT_NAV_FUZZ). Was six flat `nav_test_*` fields on AppState; grouped per
/// the AppState god-struct decomposition (pure-tier cluster).
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

- [ ] **Step 2: Replace the six flat fields in `AppState`**

In `src/app/mod.rs`, find the six `nav_test_*` field declarations (currently lines ~441–448 — locate by `rg -n 'pub nav_test_' src/app/mod.rs`):

```rust
// remove these six lines:
pub nav_test_active: bool,
pub nav_test_step: usize,
pub nav_test_failures: usize,
pub nav_test_prev_top: usize,
pub nav_test_expect_return: Option<usize>,
pub nav_test_fuzz: bool,
```

Replace with one line (keep any surrounding doc comment that grouped them):

```rust
pub nav_test: crate::input::nav_test::NavTestState,
```

- [ ] **Step 3: Replace the six init lines in `build_window`**

In `src/app/mod.rs`, find the six `nav_test_*` init lines inside the `AppState { … }` literal (currently ~1577–1582 — locate by `rg -n 'nav_test_\w+:' src/app/mod.rs`, the ones with `: false`/`: 0`/`: None` values):

```rust
// remove these six lines:
nav_test_active: false,
nav_test_step: 0,
nav_test_failures: 0,
nav_test_prev_top: 0,
nav_test_expect_return: None,
nav_test_fuzz: false,
```

Replace with one line:

```rust
nav_test: crate::input::nav_test::NavTestState::default(),
```

(All six initial values are the `Default` — `false`/`0`/`None` — so `::default()` is exact.)

- [ ] **Step 4: Rewrite the 28 access sites in `src/input/nav_test.rs`**

Rewrite every `s.nav_test_<x>` / `state.nav_test_<x>` to `s.nav_test.<x>` / `state.nav_test.<x>` (prefix stripped per the mapping). The complete list of sites (run `rg -n 'nav_test_\w+' src/input/nav_test.rs` to confirm none missed):

- `s.nav_test_active` → `s.nav_test.active` (lines ~282, 285, 297, 338, 347, 353)
- `s.nav_test_step` → `s.nav_test.step` (~283, 341, 350, 351, 373, 375, 861)
- `s.nav_test_failures` → `s.nav_test.failures` (~284, 352)
- `state.nav_test_failures` → `state.nav_test.failures` (~913)
- `s.nav_test_prev_top` → `s.nav_test.prev_top` (~300, 378)
- `s.nav_test_expect_return` → `s.nav_test.expect_return` (~301, 394, 397, 448, 469)
- `s.nav_test_fuzz` → `s.nav_test.fuzz` (~303, 306, 309, 331)

Note the compound forms work identically on the nested field: `s.nav_test.step += 1` (~861), `s.nav_test.failures += 1` (~913), `s.nav_test.expect_return.take()` (~469), `s.nav_test.active = false` (assignments).

The simplest reliable method: a scoped sed-style replacement of each `nav_test_<suffix>` → `nav_test.<suffix>` token in this ONE file, then `cargo build` to catch anything. Do NOT apply such a replacement to `src/app/mod.rs` (its `nav_test` references are already handled in Steps 2–3 and the field name there is now exactly `nav_test`).

- [ ] **Step 5: Build**

```bash
cargo build
```
Expected: clean. Any `no field nav_test_x on type AppState` error names a missed access site — rewrite it `nav_test_x` → `nav_test.x`. Any `no field nav_test_x on NavTestState` would mean a typo in the struct or a wrong suffix.

- [ ] **Step 6: Clippy**

```bash
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: `115`. (No new warnings — the grouping adds a struct + derive, removes 6 fields, adds 1.)

- [ ] **Step 7: Test-count invariant**

```bash
cargo test --bins 2>&1 | rg 'test result'
```
Expected: `413 passed`. The nav-test harness e2e is `#[ignore]`d so the count is unaffected; this proves the access rewrite compiles + the pure suite passes.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(app): group nav_test_* fields into NavTestState

First cluster of the AppState god-struct grouping (pure-tier pilot). The
six flat nav_test_* fields (active/step/failures/prev_top/expect_return/
fuzz) become one NavTestState sub-struct in src/input/nav_test.rs, held as
AppState.nav_test, inited via NavTestState::default() in build_window.
28 access sites in nav_test.rs rewritten s.nav_test_x -> s.nav_test.x.
Behavior-preserving: access shape only, no value/logic change. Mirrors the
existing ab_repeat: AbRepeatState idiom. 413 tests + clippy 115 unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Finish the branch

**Files:** none (git) + audit ledger update.

**Interfaces:**
- Consumes: the grouped `NavTestState` from Task 1.
- Produces: Phase A merged to master; ledger updated.

- [ ] **Step 1: Final unit gates on the branch**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result' && cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: clean build, `413 passed`, clippy `115`. Confirm `git status` clean.

(No user nav-fuzz gate — `nav_test` is pure-tier per the spec.)

- [ ] **Step 2: Merge to master**

```bash
git checkout master
git merge --no-ff refactor/appstate-grouping
```

- [ ] **Step 3: Re-verify on merged master**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result'
```
Expected: clean build, `413 passed`.

- [ ] **Step 4: Push, delete branch**

```bash
git push origin master
git branch -d refactor/appstate-grouping
```

- [ ] **Step 5: Update the audit ledger**

In `docs/superpowers/audit-opportunities.md`, update the `AppState god-struct` entry: record that the grouping project has STARTED (decomposed into contained clusters; core fields stay flat) and Phase A (`nav_test` → `NavTestState`) is DONE. Note the remaining contained clusters (journal, page_image, word_cycle, echo_overlay, scansion, vocab_popup) as the sequenced follow-on sub-projects. Commit:

```bash
git add docs/superpowers/audit-opportunities.md
git commit -m "docs(audit): mark AppState grouping Phase A (nav_test) DONE

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push origin master
```

---

## Self-Review

**Spec coverage:**
- NavTestState sub-struct in src/input/nav_test.rs, Default-derived (spec "Phase A → The sub-struct") → Task 1 Step 1 ✓
- 6 flat fields → 1 `nav_test: NavTestState` (spec "AppState change") → Task 1 Step 2 ✓
- build_window init → `NavTestState::default()`, the only build_window edit (spec "build_window init change" + Global Constraints) → Task 1 Step 3 ✓
- 28 access-site rewrites `s.nav_test_x` → `s.nav_test.x` (spec "Access-site rewrites") → Task 1 Step 4 ✓
- pure-tier verification: 413 + clippy 115, no nav-fuzz (spec "Verification (pure tier)") → Global Constraints + Task 1 Steps 6-7 + Task 2 Step 1 ✓
- no facade / direct nested access (spec Mechanics) → Global Constraints + Task 1 Step 4 ✓
- field-name mapping (spec Global Constraints) → Global Constraints + Task 1 Step 4 ✓

**Placeholder scan:** No TBD/TODO. Line numbers are given as "~" anchors with `rg` commands to locate exactly (they shift only trivially within mod.rs since this is the first edit on a clean branch). Every code block is literal.

**Type consistency:** `NavTestState` field names (active/step/failures/prev_top/expect_return/fuzz) are consistent across the struct def (Step 1), the mapping (Global Constraints), and the rewrite list (Step 4). The `AppState.nav_test` field name matches the init (`nav_test: …`) and every rewritten access (`s.nav_test.x`).
