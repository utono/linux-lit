# AppState Grouping Phase B — journal cluster Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Group the four flat `journal_*` fields of `AppState` into one `JournalState` sub-struct.

**Architecture:** Define `pub struct JournalState` in `src/input/actions/journal.rs`, replace the four flat `AppState` fields with one `journal: JournalState`, init it with an **explicit nested literal** in `build_window` (NOT `::default()` — `journal_prompt_mode: JournalPromptMode::Ask` is non-default and the enum has no `Default`), and rewrite every `s.journal_<x>` access to `s.journal.<x>`. Behavior-CHANGING (access shape only); the compiler flags every missed site. Follows the proven Phase A (`nav_test`) pattern.

**Tech Stack:** Rust, `cargo build` / `cargo test --bins` / `cargo clippy`.

## Global Constraints

- **Scope class: behavior-CHANGING field grouping.** The rewrite is purely `s.journal_x` → `s.journal.x` — NO value/logic/control-flow change. Runtime behavior preserved; only access shape changes.
- **Pure-tier verification:** `cargo test --bins` (must stay **413**) + `cargo clippy` (must stay **115**). NO user nav-fuzz (journal is pure-state, can't affect rendering).
- **Non-default init — explicit nested literal** (the variant from Phase A): build_window inits `journal: crate::input::actions::journal::JournalState { pages: Vec::new(), page_index: 0, return_pos: None, prompt_mode: JournalPromptMode::Ask }`. Do NOT add `#[derive(Default)]` to `JournalState` or `JournalPromptMode`; do NOT use `::default()`.
- **The ONLY `build_window` edit** is replacing the four inline `journal_*: …` init lines with the one nested literal above.
- **No facade / no accessor methods** — direct nested access (`s.journal.pages`), matching the `s.ab_repeat.chunk_index` idiom.
- **Field mapping (prefix stripped):** `journal_pages`→`pages`, `journal_page_index`→`page_index`, `journal_return_pos`→`return_pos`, `journal_prompt_mode`→`prompt_mode`.
- All `journal_*` access sites are in `src/input/actions/journal.rs` (33 sites); `mod.rs` holds only the struct def + init. Confirmed by `rg`.
- **Do NOT touch `journal_overlay` / `journal_picker` / `journal_band`** — separate fields, not this cluster.
- Branch off `master`. Branch name: `refactor/appstate-grouping-journal`.

---

### Task 0: Branch + baseline

**Files:** none.

- [ ] **Step 1: Branch + baselines**

```bash
cd ~/utono/linux-lit
git checkout master
git checkout -b refactor/appstate-grouping-journal
cargo test --bins 2>&1 | rg 'test result'
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: `413 passed`; clippy `115`. Record both. (No commit.)

---

### Task 1: Group `journal_*` into `JournalState`

**Files:**
- Modify: `src/input/actions/journal.rs` (define `JournalState`; rewrite 33 access sites)
- Modify: `src/app/mod.rs` (4 fields → 1; 4 init lines → 1 nested literal)

**Interfaces:**
- Consumes: `AppState` from Task 0.
- Produces: `pub struct JournalState { pages: Vec<crate::db::journal::JournalPage>, page_index: usize, return_pos: Option<(usize,usize)>, prompt_mode: JournalPromptMode }` in `crate::input::actions::journal`; `AppState.journal: JournalState`.

- [ ] **Step 1: Define `JournalState` in `src/input/actions/journal.rs`**

`JournalPromptMode` is already imported at line 1 of this file, so use the bare name. Add after the `use` lines, before the first fn:

```rust
/// Grouped state for the journal feature (band pages + viewer index + the
/// return-to-reader position + the add/edit prompt mode). Was four flat
/// `journal_*` fields on AppState; grouped per the AppState god-struct
/// decomposition (pure-tier cluster).
pub struct JournalState {
    pub pages: Vec<crate::db::journal::JournalPage>,
    pub page_index: usize,
    pub return_pos: Option<(usize, usize)>,
    pub prompt_mode: JournalPromptMode,
}
```

(No `#[derive(Default)]` — the init is an explicit literal, Step 3.)

- [ ] **Step 2: Replace the four flat fields in `AppState`**

In `src/app/mod.rs`, find the four `journal_*` field declarations (locate by `rg -n 'pub journal_pages|pub journal_page_index|pub journal_return_pos|pub journal_prompt_mode' src/app/mod.rs`):

```rust
// remove these four lines:
pub journal_pages: Vec<crate::db::journal::JournalPage>,
pub journal_page_index: usize,
pub journal_return_pos: Option<(usize, usize)>,
pub journal_prompt_mode: JournalPromptMode,
```

Replace with one line:

```rust
pub journal: crate::input::actions::journal::JournalState,
```

- [ ] **Step 3: Replace the four init lines in `build_window` (explicit nested literal)**

In `src/app/mod.rs`, find the four `journal_*` init lines in the `AppState { … }` literal (locate by `rg -n 'journal_pages:|journal_page_index:|journal_return_pos:|journal_prompt_mode:' src/app/mod.rs`):

```rust
// remove these four lines:
journal_pages: Vec::new(),
journal_page_index: 0,
journal_return_pos: None,
journal_prompt_mode: JournalPromptMode::Ask,
```

Replace with one nested literal (preserves the exact `Ask` init):

```rust
journal: crate::input::actions::journal::JournalState {
    pages: Vec::new(),
    page_index: 0,
    return_pos: None,
    prompt_mode: JournalPromptMode::Ask,
},
```

(`JournalPromptMode` is already in scope in mod.rs since it's defined there. Do NOT use `::default()` — `JournalPromptMode` has no `Default`.)

- [ ] **Step 4: Rewrite the 33 access sites in `src/input/actions/journal.rs`**

Rewrite every `s.journal_<suffix>` to `s.journal.<suffix>` (and any `state.journal_<suffix>`), prefix stripped per the mapping:
- `s.journal_pages` → `s.journal.pages`
- `s.journal_page_index` → `s.journal.page_index`
- `s.journal_return_pos` → `s.journal.return_pos`
- `s.journal_prompt_mode` → `s.journal.prompt_mode`

Compound forms carry over identically: `s.journal.return_pos.take()`, `s.journal.page_index -= 1`, `s.journal.pages.is_empty()`, `s.journal.pages[s.journal.page_index].id`, `s.journal.pages.iter().position(...)`.

Scope the token replacement to `src/input/actions/journal.rs` ONLY. Do NOT apply it to `mod.rs` (its journal field/init are handled in Steps 2–3 and the field is named exactly `journal`). Do NOT touch `journal_overlay`/`journal_picker`/`journal_band` (different fields — their names contain `journal` but are not `journal_pages/page_index/return_pos/prompt_mode`).

- [ ] **Step 5: Build**

```bash
cargo build
```
Expected: clean. `no field journal_x on AppState` names a missed site — rewrite it. `no field journal_x on JournalState` means a typo/wrong suffix.

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
rg -n 'journal_pages|journal_page_index|journal_return_pos|journal_prompt_mode' src/
```
Expected: zero hits (all rewritten to `journal.pages` etc.; the struct def uses bare `pages`/`page_index`/etc., not the flat names).

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "refactor(app): group journal_* fields into JournalState

Second cluster of the AppState god-struct grouping (pure-tier). The four
flat journal_* fields (pages/page_index/return_pos/prompt_mode) become one
JournalState sub-struct in src/input/actions/journal.rs, held as
AppState.journal. Init is an explicit nested literal (NOT ::default()) to
preserve journal_prompt_mode: JournalPromptMode::Ask, which is non-default
and the enum has no Default. 33 access sites in journal.rs rewritten
s.journal_x -> s.journal.x. Behavior-preserving: access shape only. 413
tests + clippy 115 unchanged.

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
git merge --no-ff refactor/appstate-grouping-journal
```

- [ ] **Step 3: Re-verify on merged master**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result'
```
Expected: clean build, `413 passed`.

- [ ] **Step 4: Push, delete branch**

```bash
git push origin master
git branch -d refactor/appstate-grouping-journal
```

- [ ] **Step 5: Update the audit ledger**

In `docs/superpowers/audit-opportunities.md`, update the `AppState god-struct` entry: mark Phase B (`journal` → `JournalState`) DONE, note the non-default explicit-literal init precedent it established, and that the remaining contained clusters are `page_image`, `word_cycle`, `echo_overlay` (pure-tier), `scansion`, `vocab_popup` (render-tier). Commit + push:

```bash
git add docs/superpowers/audit-opportunities.md
git commit -m "docs(audit): mark AppState grouping Phase B (journal) DONE

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push origin master
```

---

## Self-Review

**Spec coverage:**
- JournalState (4 fields, no Default derive) in journal.rs (spec "The sub-struct") → Task 1 Step 1 ✓
- 4 flat fields → 1 `journal: JournalState` (spec "AppState change") → Task 1 Step 2 ✓
- explicit nested literal init preserving `JournalPromptMode::Ask`, the only build_window edit (spec "non-Default init") → Task 1 Step 3 + Global Constraints ✓
- 33 access-site rewrites `s.journal_x` → `s.journal.x` (spec "Access-site rewrites") → Task 1 Step 4 ✓
- pure-tier: 413 + clippy 115, no nav-fuzz (spec "Verification") → Global Constraints + Task 1 Steps 6-7 + Task 2 Step 1 ✓
- don't touch journal_overlay/picker/band (spec "Access-site rewrites") → Global Constraints + Task 1 Step 4 ✓
- no facade (spec Mechanics) → Global Constraints ✓

**Placeholder scan:** No TBD/TODO. Line numbers given as `rg` locators. Every code block literal; the `prompt_mode` type is the concrete bare `JournalPromptMode` (confirmed in scope via journal.rs:1).

**Type consistency:** `JournalState` field names (pages/page_index/return_pos/prompt_mode) consistent across struct def (Step 1), mapping (Global Constraints), init literal (Step 3), and rewrite list (Step 4). The `AppState.journal` field name matches init (`journal:`) and every rewritten access (`s.journal.x`).
