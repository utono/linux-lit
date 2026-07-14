# AppState Grouping Phase D — echo_overlay cluster Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Group the six flat `echo_overlay_*` fields of `AppState` into one `EchoOverlayState` sub-struct.

**Architecture:** Define `pub struct EchoOverlayState` (with `#[derive(Default)]`) in `src/input/actions/echoes.rs`, replace the six flat `AppState` fields with one `echo_overlay: EchoOverlayState`, init it via `EchoOverlayState::default()` in `build_window` (all six inits are the type Default), and rewrite every `s.echo_overlay_<x>` access to `s.echo_overlay.<x>` across `echoes.rs` (88 sites) + `keymap.rs` (3 sites). Behavior-CHANGING (access shape only); the compiler flags every missed site. Follows the proven Phase A (`nav_test`, all-Default `::default()`) pattern.

**Tech Stack:** Rust, `cargo build` / `cargo test --bins` / `cargo clippy`.

## Global Constraints

- **Scope class: behavior-CHANGING field grouping.** The rewrite is purely `s.echo_overlay_x` → `s.echo_overlay.x` — NO value/logic/control-flow change. Runtime behavior preserved; only access shape changes.
- **Pure-tier verification:** `cargo test --bins` (must stay **413**) + `cargo clippy` (must stay **115**). NO user nav-fuzz (echo_overlay is pure-state, can't affect rendering).
- **All-Default init — `::default()`** (the Phase A variant): build_window inits `echo_overlay: crate::input::actions::echoes::EchoOverlayState::default()`. `EchoOverlayState` gets `#[derive(Default)]`. All six originals are the Default (`Vec::new()`/`0`/`HashMap::new()`/`String::new()`/`None`/`None`).
- **The ONLY `build_window` edit** is replacing the six inline `echo_overlay_*: …` init lines with the one `::default()` line.
- **No facade / no accessor methods** — direct nested access (`s.echo_overlay.links`), matching the `s.ab_repeat.chunk_index` idiom.
- **Field mapping (strip `echo_overlay_` prefix):** `echo_overlay_links`→`links`, `echo_overlay_index`→`index`, `echo_overlay_titles`→`titles`, `echo_overlay_source`→`source`, `echo_overlay_turn_id`→`turn_id`, `echo_overlay_turn_key`→`turn_key`.
- Cluster access sites are in **two files**: `src/input/actions/echoes.rs` (88) + `src/input/keymap.rs` (3, lines ~1724-1726). `mod.rs` holds only the struct def + init. Confirmed by `rg`.
- **Do NOT touch** `echo_session`, `echo_add_turn_id`, `echo_picker`, `echo_turns_picker`, `echo_line_picker`, `echo_keybinds_overlay`, `pending_echo_context`, `pending_echo_scene_lines` — separate fields. Rewrite by full `echo_overlay_*` name so they are never matched.
- Branch off `master`. Branch name: `refactor/appstate-grouping-echo-overlay`.

---

### Task 0: Branch + baseline

**Files:** none.

- [ ] **Step 1: Branch + baselines**

```bash
cd ~/utono/linux-lit
git checkout master
git checkout -b refactor/appstate-grouping-echo-overlay
cargo test --bins 2>&1 | rg 'test result'
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: `413 passed`; clippy `115`. Record both. (No commit.)

---

### Task 1: Group `echo_overlay_*` into `EchoOverlayState`

**Files:**
- Modify: `src/input/actions/echoes.rs` (define `EchoOverlayState`; rewrite 88 access sites)
- Modify: `src/input/keymap.rs` (rewrite 3 access sites)
- Modify: `src/app/mod.rs` (6 fields → 1; 6 init lines → 1 `::default()`)

**Interfaces:**
- Consumes: `AppState` from Task 0.
- Produces: `pub struct EchoOverlayState { links: Vec<StoredEchoLink>, index: usize, titles: HashMap<String,String>, source: String, turn_id: Option<i64>, turn_key: Option<EchoTurnKey> }` (Default-derived) in `crate::input::actions::echoes`; `AppState.echo_overlay: EchoOverlayState`.

- [ ] **Step 1: Define `EchoOverlayState` in `src/input/actions/echoes.rs`**

`echoes.rs` already imports `EchoTurnKey` + `StoredEchoLink` at line 12 (`use crate::db::queries::{EchoTurnKey, StoredEchoLink};`), so use the bare names. Add after the `use` lines, before the first fn:

```rust
/// Grouped state for the echo overlay (the stored echo links for the current
/// turn, the navigation index into them, the work-id→title map, the source
/// label, and the current turn id/key). Was six flat `echo_overlay_*` fields on
/// AppState; grouped per the AppState god-struct decomposition (pure-tier
/// cluster).
#[derive(Default)]
pub struct EchoOverlayState {
    pub links: Vec<StoredEchoLink>,
    pub index: usize,
    pub titles: std::collections::HashMap<String, String>,
    pub source: String,
    pub turn_id: Option<i64>,
    pub turn_key: Option<EchoTurnKey>,
}
```

- [ ] **Step 2: Replace the six flat fields in `AppState`**

In `src/app/mod.rs`, find the six `echo_overlay_*` field declarations (locate by `rg -n 'pub echo_overlay_' src/app/mod.rs`):

```rust
// remove these six lines:
pub echo_overlay_links: Vec<crate::db::queries::StoredEchoLink>,
pub echo_overlay_index: usize,
pub echo_overlay_titles: std::collections::HashMap<String, String>,
pub echo_overlay_source: String,
pub echo_overlay_turn_id: Option<i64>,
pub echo_overlay_turn_key: Option<crate::db::queries::EchoTurnKey>,
```

Replace with one line:

```rust
pub echo_overlay: crate::input::actions::echoes::EchoOverlayState,
```

- [ ] **Step 3: Replace the six init lines in `build_window` (`::default()`)**

In `src/app/mod.rs`, find the six `echo_overlay_*` init lines in the `AppState { … }` literal (locate by `rg -n 'echo_overlay_links:|echo_overlay_index:|echo_overlay_titles:|echo_overlay_source:|echo_overlay_turn_id:|echo_overlay_turn_key:' src/app/mod.rs`, the ones with values not `pub`):

```rust
// remove these six lines:
echo_overlay_links: Vec::new(),
echo_overlay_index: 0,
echo_overlay_titles: std::collections::HashMap::new(),
echo_overlay_source: String::new(),
echo_overlay_turn_id: None,
echo_overlay_turn_key: None,
```

Replace with one line:

```rust
echo_overlay: crate::input::actions::echoes::EchoOverlayState::default(),
```

(All six originals are the `Default`, so `::default()` is exact.)

- [ ] **Step 4: Rewrite the 88 access sites in `src/input/actions/echoes.rs`**

Rewrite every `s.echo_overlay_<suffix>` (and any `state.echo_overlay_<suffix>`) to `s.echo_overlay.<suffix>`, prefix stripped:
- `s.echo_overlay_links` → `s.echo_overlay.links`
- `s.echo_overlay_index` → `s.echo_overlay.index`
- `s.echo_overlay_titles` → `s.echo_overlay.titles`
- `s.echo_overlay_source` → `s.echo_overlay.source`
- `s.echo_overlay_turn_id` → `s.echo_overlay.turn_id`
- `s.echo_overlay_turn_key` → `s.echo_overlay.turn_key`

Compound forms carry over identically (`.clear()`, `.push()`, `.len()`, `.is_empty()`, `[index]`, `.get(...)`, `.insert(...)`, `.take()`, `= None`, `= Some(...)`). Scope the token replacement to `echoes.rs` (Step 5 handles keymap.rs). Do NOT touch the non-cluster echo fields listed in Global Constraints.

- [ ] **Step 5: Rewrite the 3 access sites in `src/input/keymap.rs`**

Locate by `rg -n 'echo_overlay_links|echo_overlay_turn_id|echo_overlay_turn_key' src/input/keymap.rs` (~lines 1724-1726):
- `s.echo_overlay_links.clear()` → `s.echo_overlay.links.clear()`
- `s.echo_overlay_turn_id = None` → `s.echo_overlay.turn_id = None`
- `s.echo_overlay_turn_key = None` → `s.echo_overlay.turn_key = None`

- [ ] **Step 6: Build**

```bash
cargo build
```
Expected: clean. `no field echo_overlay_x on AppState` names a missed site — rewrite it. `no field echo_overlay_x on EchoOverlayState` means a typo/wrong suffix.

- [ ] **Step 7: Clippy**

```bash
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: `115`.

- [ ] **Step 8: Test-count invariant**

```bash
cargo test --bins 2>&1 | rg 'test result'
```
Expected: `413 passed`.

- [ ] **Step 9: Verify zero old flat forms remain**

```bash
rg -n 'echo_overlay_links|echo_overlay_index|echo_overlay_titles|echo_overlay_source|echo_overlay_turn_id|echo_overlay_turn_key' src/
```
Expected: zero hits (all rewritten to `echo_overlay.links` etc.; the struct def uses bare `links`/`index`/etc.).

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "refactor(app): group echo_overlay_* fields into EchoOverlayState

Cluster of the AppState god-struct grouping (pure-tier). The six flat
echo_overlay_* fields (links/index/titles/source/turn_id/turn_key) become
one EchoOverlayState sub-struct in src/input/actions/echoes.rs, held as
AppState.echo_overlay, inited via ::default() in build_window (all six
originals are the type Default). 91 access sites (88 echoes.rs + 3 keymap.rs)
rewritten s.echo_overlay_x -> s.echo_overlay.x. Behavior-preserving: access
shape only. Non-cluster echo fields (echo_session/echo_picker/etc) untouched.
413 tests + clippy 115 unchanged.

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
git merge --no-ff refactor/appstate-grouping-echo-overlay
```

- [ ] **Step 3: Re-verify on merged master**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result'
```
Expected: clean build, `413 passed`.

- [ ] **Step 4: Push, delete branch**

```bash
git push origin master
git branch -d refactor/appstate-grouping-echo-overlay
```

- [ ] **Step 5: Update the audit ledger**

In `docs/superpowers/audit-opportunities.md`, mark Phase D (`echo_overlay` → `EchoOverlayState`) DONE; note the remaining contained clusters are `scansion`, `vocab_popup` (render-tier). Commit + push:

```bash
git add docs/superpowers/audit-opportunities.md
git commit -m "docs(audit): mark AppState grouping Phase D (echo_overlay) DONE

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push origin master
```

---

## Self-Review

**Spec coverage:**
- EchoOverlayState (6 fields, Default-derived) in echoes.rs (spec "The sub-struct") → Task 1 Step 1 ✓
- 6 flat fields → 1 `echo_overlay: EchoOverlayState` (spec "AppState change") → Task 1 Step 2 ✓
- `::default()` init, the only build_window edit (spec "Init / build_window") → Task 1 Step 3 + Global Constraints ✓
- 91 access-site rewrites across echoes.rs (88) + keymap.rs (3) (spec "Access-site rewrites") → Task 1 Steps 4-5 ✓
- pure-tier: 413 + clippy 115, no nav-fuzz (spec "Verification") → Global Constraints + Task 1 Steps 7-8 + Task 2 Step 1 ✓
- don't touch non-cluster echo fields (spec "Access-site rewrites") → Global Constraints + Task 1 Step 4 ✓
- no facade (spec Mechanics) → Global Constraints ✓

**Placeholder scan:** No TBD/TODO. Line numbers given as `rg` locators. Every code block literal; `StoredEchoLink`/`EchoTurnKey` confirmed imported in echoes.rs:12.

**Type consistency:** `EchoOverlayState` field names (links/index/titles/source/turn_id/turn_key) consistent across struct def (Step 1), mapping (Global Constraints), and rewrite list (Steps 4-5). The `AppState.echo_overlay` field name matches init (`echo_overlay:`) and every rewritten access (`s.echo_overlay.x`).
