# AppState Grouping Phase C — word_cycle cluster Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Group the five flat `word_cycle_*` / `word_collect_*` / `word_bold_gen` fields of `AppState` into one `WordCycleState` sub-struct.

**Architecture:** Define `pub struct WordCycleState` (with `#[derive(Default)]`) in `src/input/actions/word_copy.rs`, replace the five flat `AppState` fields with one `word_cycle: WordCycleState`, init it via `WordCycleState::default()` in `build_window` (all five inits are the type Default), and rewrite every `state.word_<x>` access to `state.word_cycle.<x>`. Behavior-CHANGING (access shape only); the compiler flags every missed site. Follows the proven Phase A (`nav_test`, all-Default `::default()`) pattern.

**Tech Stack:** Rust, `cargo build` / `cargo test --bins` / `cargo clippy`.

## Global Constraints

- **Scope class: behavior-CHANGING field grouping.** The rewrite is purely `state.word_x` → `state.word_cycle.x` — NO value/logic/control-flow change. Runtime behavior preserved; only access shape changes.
- **Pure-tier verification:** `cargo test --bins` (must stay **413**) + `cargo clippy` (must stay **115**). NO user nav-fuzz (word_cycle is pure-state, can't affect rendering).
- **All-Default init — `::default()`** (the Phase A variant): build_window inits `word_cycle: crate::input::actions::word_copy::WordCycleState::default()`. `WordCycleState` gets `#[derive(Default)]`. All five originals are the Default (`None`/`0`/`Rc::new(Cell::new(0))`==`Rc<Cell<u64>>::default()`/`Vec::new()`/`Vec::new()`).
- **The ONLY `build_window` edit** is replacing the five inline `word_*: …` init lines with the one `::default()` line.
- **No facade / no accessor methods** — direct nested access (`state.word_cycle.cycle_line`), matching the `s.ab_repeat.chunk_index` idiom.
- **Field mapping (strip leading `word_` per full name):** `word_cycle_line`→`cycle_line`, `word_cycle_index`→`cycle_index`, `word_bold_gen`→`bold_gen`, `word_collect_words`→`collect_words`, `word_collect_ranges`→`collect_ranges`.
- All cluster access sites are in `src/input/actions/word_copy.rs` (20 sites); `mod.rs` holds only the struct def + init. Confirmed by `rg`.
- **Do NOT touch `word_status_timer` / `word_status_label` / `word_bold_tag`** — separate fields. In particular `word_bold_gen` (in cluster) is distinct from `word_bold_tag` (NOT in cluster); rewrite by full name `word_bold_gen` so `word_bold_tag` is never matched.
- Branch off `master`. Branch name: `refactor/appstate-grouping-word-cycle`.

---

### Task 0: Branch + baseline

**Files:** none.

- [ ] **Step 1: Branch + baselines**

```bash
cd ~/utono/linux-lit
git checkout master
git checkout -b refactor/appstate-grouping-word-cycle
cargo test --bins 2>&1 | rg 'test result'
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: `413 passed`; clippy `115`. Record both. (No commit.)

---

### Task 1: Group `word_cycle_*` into `WordCycleState`

**Files:**
- Modify: `src/input/actions/word_copy.rs` (define `WordCycleState`; rewrite 20 access sites)
- Modify: `src/app/mod.rs` (5 fields → 1; 5 init lines → 1 `::default()`)

**Interfaces:**
- Consumes: `AppState` from Task 0.
- Produces: `pub struct WordCycleState { cycle_line: Option<usize>, cycle_index: usize, bold_gen: Rc<Cell<u64>>, collect_words: Vec<String>, collect_ranges: Vec<(usize,usize)> }` (Default-derived) in `crate::input::actions::word_copy`; `AppState.word_cycle: WordCycleState`.

- [ ] **Step 1: Define `WordCycleState` in `src/input/actions/word_copy.rs`**

Add after the `use` lines, before the first fn. Use whatever `Rc`/`Cell` path is already in scope in this file; if neither is imported, the fully-qualified path shown is correct:

```rust
/// Grouped state for the word-copy / word-cycle feature (cursor-word cycling,
/// multi-word phrase collection, and the bold-highlight generation counter).
/// Was five flat `word_cycle_*` / `word_collect_*` / `word_bold_gen` fields on
/// AppState; grouped per the AppState god-struct decomposition (pure-tier
/// cluster).
#[derive(Default)]
pub struct WordCycleState {
    pub cycle_line: Option<usize>,
    pub cycle_index: usize,
    pub bold_gen: std::rc::Rc<std::cell::Cell<u64>>,
    pub collect_words: Vec<String>,
    pub collect_ranges: Vec<(usize, usize)>,
}
```

- [ ] **Step 2: Replace the five flat fields in `AppState`**

In `src/app/mod.rs`, find the five field declarations (locate by `rg -n 'pub word_cycle_line|pub word_cycle_index|pub word_bold_gen|pub word_collect_words|pub word_collect_ranges' src/app/mod.rs`):

```rust
// remove these five lines:
pub word_cycle_line: Option<usize>,
pub word_cycle_index: usize,
pub word_bold_gen: Rc<Cell<u64>>,
pub word_collect_words: Vec<String>,
pub word_collect_ranges: Vec<(usize, usize)>,
```

Replace with one line:

```rust
pub word_cycle: crate::input::actions::word_copy::WordCycleState,
```

- [ ] **Step 3: Replace the five init lines in `build_window`**

In `src/app/mod.rs`, find the five init lines in the `AppState { … }` literal (locate by `rg -n 'word_cycle_line:|word_cycle_index:|word_bold_gen:|word_collect_words:|word_collect_ranges:' src/app/mod.rs`, the ones with `: None`/`: 0`/`: Rc::new`/`: Vec::new` values):

```rust
// remove these five lines:
word_cycle_line: None,
word_cycle_index: 0,
word_bold_gen: Rc::new(Cell::new(0)),
word_collect_words: Vec::new(),
word_collect_ranges: Vec::new(),
```

Replace with one line:

```rust
word_cycle: crate::input::actions::word_copy::WordCycleState::default(),
```

(All five initial values are the `Default` — `None`/`0`/`Rc::new(Cell::new(0))`==`Rc<Cell<u64>>::default()`/`Vec::new()` — so `::default()` is exact.)

- [ ] **Step 4: Rewrite the 20 access sites in `src/input/actions/word_copy.rs`**

Rewrite every `state.word_<full-name>` to `state.word_cycle.<stripped>` per the mapping:
- `state.word_cycle_line` → `state.word_cycle.cycle_line`
- `state.word_cycle_index` → `state.word_cycle.cycle_index`
- `state.word_bold_gen` → `state.word_cycle.bold_gen`
- `state.word_collect_words` → `state.word_cycle.collect_words`
- `state.word_collect_ranges` → `state.word_cycle.collect_ranges`

Compound forms carry over identically: `state.word_cycle.collect_words.clear()`, `.push(...)`, `.join(" ")`, `.len()`, `state.word_cycle.collect_ranges.clone()`, `.push((char_start, char_end))`, `state.word_cycle.bold_gen.get()`, `.set(gen)`, `.clone()`, `state.word_cycle.cycle_line == Some(state.current_line)`, `state.word_cycle.cycle_index % words.len()`.

Scope the token replacement to `src/input/actions/word_copy.rs` ONLY. Do NOT apply it to `mod.rs` (its word_cycle field/init are handled in Steps 2–3 and the field is named exactly `word_cycle`). Do NOT touch `word_status_timer`/`word_status_label`/`word_bold_tag` — rewrite by FULL field name (`word_bold_gen`, not the `word_bold` prefix), so `word_bold_tag` (a separate tag field, e.g. word_copy.rs `let tag = &state.word_bold_tag;`) is never matched.

- [ ] **Step 5: Build**

```bash
cargo build
```
Expected: clean. `no field word_x on AppState` names a missed site — rewrite it. `no field x on WordCycleState` means a typo/wrong stripped name.

- [ ] **Step 6: Clippy**

```bash
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: `115`.

- [ ] **Step 7: Test-count invariant**

```bash
cargo test --bins 2>&1 | rg 'test result'
```
Expected: `413 passed`.

- [ ] **Step 8: Verify zero old flat forms remain**

```bash
rg -n 'word_cycle_line|word_cycle_index|word_bold_gen|word_collect_words|word_collect_ranges' src/
```
Expected: zero hits (all rewritten; the struct uses bare `cycle_line`/`bold_gen`/etc., not the flat names). `word_bold_tag` / `word_status_*` may still appear — those are NOT cluster fields and are expected to remain.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "refactor(app): group word_cycle_* fields into WordCycleState

Third cluster of the AppState god-struct grouping (pure-tier). The five flat
word_cycle_* / word_collect_* / word_bold_gen fields become one WordCycleState
sub-struct in src/input/actions/word_copy.rs, held as AppState.word_cycle,
inited via WordCycleState::default() in build_window (all five originals are
the type Default). 20 access sites in word_copy.rs rewritten state.word_x ->
state.word_cycle.x. word_bold_tag (separate tag field) untouched.
Behavior-preserving: access shape only. 413 tests + clippy 115 unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Finish the branch

**Files:** none (git) + audit ledger update.

- [ ] **Step 1: Final unit gates**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result' && cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: clean build, `413 passed`, clippy `115`. `git status` clean. (No user nav-fuzz — pure tier.)

- [ ] **Step 2: Merge to master**

```bash
git checkout master
git merge --no-ff refactor/appstate-grouping-word-cycle
```

- [ ] **Step 3: Re-verify on merged master**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result'
```
Expected: clean build, `413 passed`.

- [ ] **Step 4: Push, delete branch**

```bash
git push origin master
git branch -d refactor/appstate-grouping-word-cycle
```

- [ ] **Step 5: Update the audit ledger**

In `docs/superpowers/audit-opportunities.md`, update the `AppState god-struct` entry: mark Phase C (`word_cycle` → `WordCycleState`) DONE, and that the remaining contained clusters are `page_image`, `echo_overlay` (pure-tier), `scansion`, `vocab_popup` (render-tier). Commit + push:

```bash
git add docs/superpowers/audit-opportunities.md
git commit -m "docs(audit): mark AppState grouping Phase C (word_cycle) DONE

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push origin master
```

---

## Self-Review

**Spec coverage:**
- WordCycleState (5 fields, `#[derive(Default)]`) in word_copy.rs (spec "The sub-struct") → Task 1 Step 1 ✓
- 5 flat fields → 1 `word_cycle: WordCycleState` (spec "AppState change") → Task 1 Step 2 ✓
- `::default()` init, the only build_window edit (spec "Init variant" + Global Constraints) → Task 1 Step 3 + Global Constraints ✓
- 20 access-site rewrites `state.word_x` → `state.word_cycle.x` (spec "Access-site rewrites") → Task 1 Step 4 ✓
- pure-tier: 413 + clippy 115, no nav-fuzz (spec "Verification") → Global Constraints + Task 1 Steps 6-7 + Task 2 Step 1 ✓
- don't touch word_status_*/word_bold_tag; word_bold_gen vs word_bold_tag distinction (spec "Access-site rewrites" + Risks) → Global Constraints + Task 1 Step 4 ✓
- no facade (spec Out-of-band) → Global Constraints ✓

**Placeholder scan:** No TBD/TODO. Line numbers given as `rg` locators. Every code block literal.

**Type consistency:** `WordCycleState` field names (cycle_line/cycle_index/bold_gen/collect_words/collect_ranges) consistent across struct def (Step 1), mapping (Global Constraints), and rewrite list (Step 4). The `AppState.word_cycle` field name matches init (`word_cycle:`) and every rewritten access (`state.word_cycle.x`).
