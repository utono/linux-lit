# AppState Grouping Phase F — scansion cluster Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Group the three flat `scansion_*` fields of `AppState` into one `ScansionState` sub-struct (render-tier — requires a user render check before merge).

**Architecture:** Define `pub struct ScansionState` co-located in `src/app/mod.rs`, replace the three flat `AppState` fields with one `scansion: ScansionState`, init it with an **explicit nested literal** (`ScanLevel::Off` is non-default; the enum has no `Default`), and rewrite every `s.scansion_<x>` access to `s.scansion.<x>` across mod.rs / keymap.rs / navigation.rs. Behavior-CHANGING (access shape only). Because the grouped fields feed the *displayed* scansion overlay, the regression proof is a user render check, not only the 413 suite.

**Tech Stack:** Rust, `cargo build` / `cargo test --bins` / `cargo clippy`, `scripts/e2e-env.sh` nav-fuzz + a manual scansion-on launch.

## Global Constraints

- **Scope class: behavior-CHANGING field grouping.** Purely `s.scansion_x` → `s.scansion.x`. NO value/logic/control-flow change.
- **RENDER-TIER verification:** agent runs `cargo build` + `cargo test --bins` (must stay **413**) + `cargo clippy` (must stay **115**) — necessary but NOT sufficient. The widget-render path is proven only by a **user-run two-part gate** (Task 2): the standard nav-fuzz (scansion-off nav paths) AND a manual scansion-ON eyeball (the nav-fuzz does not toggle scansion). The agent cannot launch cage (dwl owns the seat) — it states this is blocked and asks the user.
- **Non-default init — explicit nested literal:** `scansion: ScansionState { label_starts: std::collections::HashMap::new(), level: crate::scansion::ScanLevel::Off, data: std::collections::HashMap::new() }`. Do NOT use `::default()`; do NOT add a `Default` derive to `ScansionState` or `ScanLevel`.
- **The ONLY `build_window` edit** is replacing the three inline `scansion_*: …` init lines with the one nested literal.
- **No facade / no accessor methods** — direct nested access.
- **Field mapping:** `scansion_label_starts`→`label_starts`, `scansion_level`→`level`, `scansion_data`→`data`.
- **Boundary — do NOT touch:** `scansion_label_tag` (a `gtk4::TextTag` field), and `s.config.scansion_level` (a `Config` field, not AppState). Only the three named AppState fields move.
- 21 access sites: `src/app/mod.rs` (13), `src/input/keymap.rs` (7), `src/input/navigation.rs` (1). `crate::scansion::ScanLevel` stays fully-qualified everywhere.
- Branch off `master`. Branch name: `refactor/appstate-grouping-scansion`.

---

### Task 0: Branch + baseline

**Files:** none.

- [ ] **Step 1: Branch + baselines**

```bash
cd ~/utono/linux-lit
git checkout master
git checkout -b refactor/appstate-grouping-scansion
cargo test --bins 2>&1 | rg 'test result'
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: `413 passed`; clippy `115`. Record both. (No commit.)

---

### Task 1: Group `scansion_*` into `ScansionState`

**Files:**
- Modify: `src/app/mod.rs` (define `ScansionState`; 3 fields → 1; 3 init lines → 1 nested literal; rewrite 13 mod.rs access sites)
- Modify: `src/input/keymap.rs` (rewrite 7 access sites)
- Modify: `src/input/navigation.rs` (rewrite 1 access site)

**Interfaces:**
- Consumes: `AppState` from Task 0.
- Produces: `pub struct ScansionState { label_starts: HashMap<usize,usize>, level: crate::scansion::ScanLevel, data: HashMap<i64, crate::scansion::LineScansion> }` in `crate::app`; `AppState.scansion: ScansionState`.

- [ ] **Step 1: Define `ScansionState` in `src/app/mod.rs`**

Add near the other small structs (e.g. above `AppState`, beside `SearchMatch`/`VocabMatch`/`PageImageState`):

```rust
/// Grouped state for the scansion-marks feature (the per-line scansion data,
/// the current display level, and the buffer-line→label-start map). Was three
/// flat `scansion_*` fields on AppState; grouped per the AppState god-struct
/// decomposition (render-tier cluster).
pub struct ScansionState {
    pub label_starts: std::collections::HashMap<usize, usize>,
    pub level: crate::scansion::ScanLevel,
    pub data: std::collections::HashMap<i64, crate::scansion::LineScansion>,
}
```

(No `#[derive(Default)]` — the init is an explicit literal, Step 3.)

- [ ] **Step 2: Replace the three flat fields in `AppState`**

In `src/app/mod.rs`, find the three field declarations (locate by `rg -n 'pub scansion_label_starts|pub scansion_level|pub scansion_data' src/app/mod.rs`):

```rust
// remove these three lines:
pub scansion_label_starts: std::collections::HashMap<usize, usize>,
pub scansion_level: crate::scansion::ScanLevel,
pub scansion_data: std::collections::HashMap<i64, crate::scansion::LineScansion>,
```

Replace with one line:

```rust
pub scansion: ScansionState,
```

Do NOT touch `pub scansion_label_tag: gtk4::TextTag,` (separate field — leave it).

- [ ] **Step 3: Replace the three init lines in `build_window` (explicit nested literal)**

In `src/app/mod.rs`, find the three init lines in the `AppState { … }` literal (locate by `rg -n 'scansion_label_starts:|scansion_level:|scansion_data:' src/app/mod.rs`, the ones with `: std::collections::HashMap::new()` / `: crate::scansion::ScanLevel::Off`):

```rust
// remove these three lines:
scansion_label_starts: std::collections::HashMap::new(),
scansion_level: crate::scansion::ScanLevel::Off,
scansion_data: std::collections::HashMap::new(),
```

Replace with one nested literal (preserves the exact `Off` init):

```rust
scansion: ScansionState {
    label_starts: std::collections::HashMap::new(),
    level: crate::scansion::ScanLevel::Off,
    data: std::collections::HashMap::new(),
},
```

(Do NOT use `::default()` — `ScanLevel` has no `Default`.)

- [ ] **Step 4: Rewrite the 13 access sites in `src/app/mod.rs`**

Rewrite every `state.scansion_<suffix>` / `s.scansion_<suffix>` → `state.scansion.<suffix>` / `s.scansion.<suffix>` (mapping: `scansion_label_starts`→`scansion.label_starts`, `scansion_level`→`scansion.level`, `scansion_data`→`scansion.data`). These are in `display_work`'s buffer-build (`scansion.level != Off`, `scansion.data.is_empty()`, `apply_scansion_marks(..., &state.scansion.data, state.scansion.level)`, `state.scansion.label_starts = label_starts`) and the sign-gutter scan-text path (`state.scansion.label_starts.get(&line_idx)`).

Do NOT rewrite `scansion_label_tag` (no `_starts`/`_level`/`_data` suffix — different field).

- [ ] **Step 5: Rewrite the 7 access sites in `src/input/keymap.rs`**

The `s` scansion-toggle handler (locate by `rg -n 's\.scansion_' src/input/keymap.rs`):
- `s.scansion_data.is_empty()` → `s.scansion.data.is_empty()` (×2)
- `s.scansion_data = map` → `s.scansion.data = map`
- `s.scansion_level = crate::scansion::ScanLevel::Off` → `s.scansion.level = crate::scansion::ScanLevel::Off`
- `s.scansion_level = s.scansion_level.next()` → `s.scansion.level = s.scansion.level.next()`
- `s.scansion_level.as_str()` → `s.scansion.level.as_str()`
- `s.scansion_level` (the `{:?}` log) → `s.scansion.level`

Do NOT touch `s.config.scansion_level` (Config field — stays `s.config.scansion_level`).

- [ ] **Step 6: Rewrite the 1 access site in `src/input/navigation.rs`**

`state.scansion_level != crate::scansion::ScanLevel::Off` → `state.scansion.level != crate::scansion::ScanLevel::Off`.

- [ ] **Step 7: Build**

```bash
cargo build
```
Expected: clean. `no field scansion_x on AppState` names a missed site → rewrite it. `no field scansion_x on ScansionState` means a wrong suffix.

- [ ] **Step 8: Clippy + test invariant + zero-flat-form check**

```bash
cargo clippy 2>&1 | rg -c 'warning:'
cargo test --bins 2>&1 | rg 'test result'
rg -n 's\.scansion_label_starts|s\.scansion_level|s\.scansion_data|state\.scansion_label_starts|state\.scansion_level|state\.scansion_data' src/
```
Expected: clippy `115`; `413 passed`; the `rg` returns ZERO hits (all rewritten; the struct uses bare `label_starts`/`level`/`data`; `scansion_label_tag` and `config.scansion_level` are NOT matched by these patterns).

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "refactor(app): group scansion_* fields into ScansionState

Sixth cluster of the AppState god-struct grouping (render-tier). The three
flat scansion_* fields (label_starts/level/data) become one ScansionState
sub-struct in src/app/mod.rs, held as AppState.scansion. Init is an explicit
nested literal (NOT ::default()) to preserve scansion_level: ScanLevel::Off,
which is non-default and the enum has no Default. 21 access sites across
mod.rs/keymap.rs/navigation.rs rewritten s.scansion_x -> s.scansion.x.
Boundary fields scansion_label_tag + config.scansion_level untouched.
Behavior-preserving: access shape only. 413 tests + clippy 115 unchanged.

Render-tier: the grouped fields feed the displayed scansion overlay, so a
user render check (nav-fuzz + scansion-on eyeball) gates the merge (Task 2).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

- [ ] **Step 10: Report runtime verification is blocked**

State in the report: unit gates (build/413/clippy 115) pass and prove the rewrite compiles + the scansion-*off* logic, BUT the grouped fields feed the rendered scansion overlay and the agent cannot launch the render check (dwl owns the seat). The user must run the two-part gate (Task 2) before merge.

---

### Task 2: User render verification + finish the branch

**Files:** none (git) + audit ledger update.

- [ ] **Step 1: Final unit gates on the branch**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result' && cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: clean build, `413 passed`, clippy `115`. `git status` clean.

- [ ] **Step 2: ASK THE USER to run the two-part render gate (REQUIRED — render-tier)**

The agent CANNOT run these. Give the user both and wait for confirmation. The nav-fuzz does NOT toggle scansion, so BOTH parts are needed:

Part 1 — standard nav-fuzz on a verse work (proves no regression in the scansion-off nav paths):
```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work Son
```

Part 2 — manual scansion-ON render check (the part that exercises the grouped fields' render path): launch a verse work that has `syllable_scan` data (e.g. `TN` = Twelfth Night, used by the scansion DB tests; or another Folger verse work), press `s` to cycle `scansion_level` on, and confirm the scansion marks render over the verse exactly as before. (If the user has a preferred manual-launch command from the headless-verification docs, use that; otherwise they run `cargo run` and press `s`.)

**Do NOT merge until the user confirms BOTH are clean.** A render regression → systematic debugging, do NOT merge.

- [ ] **Step 3: Merge to master (only after user confirms both clean)**

```bash
git checkout master
git merge --no-ff refactor/appstate-grouping-scansion
```

- [ ] **Step 4: Re-verify unit gates on merged master**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result'
```
Expected: clean build, `413 passed`.

- [ ] **Step 5: Push, delete branch**

```bash
git push origin master
git branch -d refactor/appstate-grouping-scansion
```

- [ ] **Step 6: Update the audit ledger**

In `docs/superpowers/audit-opportunities.md`, mark Phase F (`scansion` → `ScansionState`) DONE, noting it was the first render-tier cluster (user nav-fuzz + scansion-on check gated the merge), and that only `vocab_popup` (the hardest contained cluster) remains. Commit + push:

```bash
git add docs/superpowers/audit-opportunities.md
git commit -m "docs(audit): mark AppState grouping Phase F (scansion) DONE

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push origin master
```

---

## Self-Review

**Spec coverage:**
- ScansionState (3 fields, no Default derive) in mod.rs (spec "The sub-struct") → Task 1 Step 1 ✓
- 3 flat fields → 1 `scansion: ScansionState` (spec "AppState change") → Task 1 Step 2 ✓
- explicit nested literal init preserving `ScanLevel::Off`, only build_window edit (spec "Non-default init") → Task 1 Step 3 + Global Constraints ✓
- 21 access-site rewrites across mod.rs/keymap.rs/navigation.rs (spec "Access-site rewrites") → Task 1 Steps 4-6 ✓
- boundary: don't touch scansion_label_tag or config.scansion_level (spec "Boundary") → Global Constraints + Task 1 Steps 2/5/8 ✓
- render-tier verification: unit gates + TWO-part user gate (nav-fuzz + scansion-on), nav-fuzz doesn't toggle scansion (spec "Verification") → Global Constraints + Task 1 Step 10 + Task 2 Step 2 ✓
- no facade (spec Mechanics) → Global Constraints ✓

**Placeholder scan:** No TBD/TODO. Line numbers are `rg` locators. Every code block literal; `ScanLevel`/`LineScansion` paths are the concrete `crate::scansion::*`.

**Type consistency:** `ScansionState` field names (label_starts/level/data) consistent across struct def (Step 1), mapping (Global Constraints), init literal (Step 3), and the three rewrite steps (4-6). The `AppState.scansion` field matches init (`scansion:`) and every rewritten access (`s.scansion.x`).
