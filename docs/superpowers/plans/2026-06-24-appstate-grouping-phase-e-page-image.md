# AppState Grouping Phase E — page_image cluster Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Group the five flat page-image / calibration fields of `AppState` into one `PageImageState` sub-struct.

**Architecture:** Define `pub struct PageImageState` co-located **in `src/app/mod.rs`** (the only consumer — all 43 access sites are mod.rs-internal image/calibration free functions), replace the five flat `AppState` fields with one `page_image: PageImageState`, init it with `PageImageState::default()` in `build_window` (all five originals are Default), and rewrite every `state.<field>` access to `state.page_image.<sub>`. Behavior-CHANGING (access shape only); the compiler flags every missed site. The entire change is confined to `src/app/mod.rs`. Follows the proven Phase A (`nav_test`) all-Default pattern.

**Tech Stack:** Rust, `cargo build` / `cargo test --bins` / `cargo clippy`.

## Global Constraints

- **Scope class: behavior-CHANGING field grouping.** The rewrite is purely `state.<field>` → `state.page_image.<sub>` — NO value/logic/control-flow change. Runtime behavior preserved; only access shape changes.
- **Pure-tier verification:** `cargo test --bins` (must stay **413**) + `cargo clippy` (must stay **115**). NO user nav-fuzz (the grouping is an access-shape change to data fields; it cannot change values, control flow, or any render invariant in scope).
- **All-Default init — `::default()`** (the variant from Phase A, NOT journal's explicit literal): every original init value is the type Default (`Vec::new()`/`None`/`false`/`None`/`0`), so `PageImageState` derives `Default` and `build_window` inits `page_image: PageImageState::default(),`.
- **The ONLY change is in `src/app/mod.rs`** — the new struct, the field decl, the init, and the 43 access rewrites are all in this one file. No other file is touched.
- **No facade / no accessor methods** — direct nested access (`state.page_image.images`), matching the `s.ab_repeat.chunk_index` idiom.
- **Field mapping:** `page_images`→`images`, `image_dir`→`dir`, `image_mode`→`mode`, `current_page_order`→`page_order`, `calibration_index`→`calibration_index`. The `AppState` field is the **singular `page_image`** (so `state.page_image.images`).
- **BOUNDARIES — do NOT touch (substring matches that are NOT cluster fields):** `page_image_overlay` (separate overlay field), the method NAME `page_image_for_line_id` (only its body's `self.page_images` read changes), the free fn NAME `refresh_page_image` (only its body's `state.X` accesses change). The `works.image_dir` doc-comment mentions in `db/models.rs`/`db/queries.rs` are false positives — not touched.
- Branch off `master`. Branch name: `refactor/appstate-grouping-page-image`.

---

### Task 0: Branch + baseline

**Files:** none.

- [ ] **Step 1: Branch + baselines**

```bash
cd ~/utono/linux-lit
git checkout master
git checkout -b refactor/appstate-grouping-page-image
cargo test --bins 2>&1 | rg 'test result'
cargo clippy 2>&1 | rg -c 'warning:'
```
Expected: `413 passed`; clippy `115`. Record both. (No commit.)

---

### Task 1: Group page-image fields into `PageImageState`

**Files:**
- Modify: `src/app/mod.rs` ONLY (define `PageImageState`; 5 fields → 1; 5 init lines → 1; rewrite 43 access sites)

**Interfaces:**
- Consumes: `AppState` from Task 0.
- Produces: `#[derive(Default)] pub struct PageImageState { images: Vec<crate::db::models::PageImage>, dir: Option<String>, mode: bool, page_order: Option<i64>, calibration_index: usize }` in `src/app/mod.rs`; `AppState.page_image: PageImageState`.

- [ ] **Step 1: Define `PageImageState` in `src/app/mod.rs`**

Add near the other small state structs (`SearchMatch` / `VocabMatch`) or just above `AppState`:

```rust
/// Grouped state for the page-scan image view + calibration mode. Was five flat
/// fields on AppState (`page_images`/`image_dir`/`image_mode`/`current_page_order`/
/// `calibration_index`); grouped per the AppState god-struct decomposition
/// (pure-tier cluster). All accesses are mod.rs-internal (the image/calibration
/// free functions).
#[derive(Default)]
pub struct PageImageState {
    pub images: Vec<crate::db::models::PageImage>,
    pub dir: Option<String>,
    pub mode: bool,
    pub page_order: Option<i64>,
    pub calibration_index: usize,
}
```

- [ ] **Step 2: Replace the five flat fields in `AppState`**

In `src/app/mod.rs`, find the five field declarations (locate by `rg -n 'pub page_images:|pub image_dir:|pub image_mode:|pub current_page_order:|pub calibration_index:' src/app/mod.rs`). They currently sit around lines 306–316, interleaved with doc comments — keep any unrelated doc comments for neighboring fields, but remove these five `pub <field>: T,` lines:

```rust
// remove these five field declarations:
pub page_images: Vec<crate::db::models::PageImage>,
pub image_dir: Option<String>,
pub image_mode: bool,
pub current_page_order: Option<i64>,
pub calibration_index: usize,
```

Replace with one line:

```rust
pub page_image: PageImageState,
```

- [ ] **Step 3: Replace the five init lines in `build_window` (`::default()`)**

In `src/app/mod.rs`, find the five init lines in the `AppState { … }` literal (locate by `rg -n 'page_images: Vec::new\(\)|image_dir: None|image_mode: false|current_page_order: None|calibration_index: 0' src/app/mod.rs`):

```rust
// remove these five init lines:
page_images: Vec::new(),
image_dir: None,
image_mode: false,
current_page_order: None,
calibration_index: 0,
```

Replace with one line:

```rust
page_image: PageImageState::default(),
```

(All five originals are the type Default, so `::default()` is behavior-identical.)

- [ ] **Step 4: Rewrite the 43 access sites in `src/app/mod.rs`**

Rewrite every access of the five fields (both `state.` and `s.` receiver forms), in the image-view / calibration free functions (`enter_page_calibration`, `refresh_page_image`, `calibration_show_page`, `calibration_jump_page`, `toggle_image_view`, `exit_page_calibration`, etc.):

- `state.page_images` → `state.page_image.images`
- `state.image_dir` → `state.page_image.dir`
- `state.image_mode` → `state.page_image.mode`
- `state.current_page_order` → `state.page_image.page_order`
- `state.calibration_index` → `state.page_image.calibration_index`

Also the `impl AppState` method `page_image_for_line_id` body: `self.page_images` → `self.page_image.images` (the method NAME stays).

Compound forms carry over identically: `state.page_image.images.is_empty()`, `state.page_image.images.len()`, `state.page_image.images[i]`, `state.page_image.page_order = Some(...)`, `state.page_image.calibration_index += 1`.

**DO NOT** do a blind token replace over `mod.rs`. Rewrite only the five exact field-access patterns. **DO NOT** touch:
- `page_image_overlay` (separate field),
- the method NAME `page_image_for_line_id`,
- the free fn NAME `refresh_page_image`,
- the `page_image: PageImageState` field/init you just created in Steps 2–3 (already correct).

- [ ] **Step 5: Build**

```bash
cargo build
```
Expected: clean. `no field page_images on AppState` (etc.) names a missed site — rewrite it. `no field images on PageImageState` means a typo/wrong sub-name.

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
rg -n 'state\.page_images|state\.image_dir|state\.image_mode|state\.current_page_order|state\.calibration_index|s\.page_images|s\.image_dir|s\.image_mode|s\.current_page_order|s\.calibration_index|self\.page_images' src/
```
Expected: zero hits (all rewritten to `…page_image.images` etc.). Note: `page_image_overlay`, `page_image_for_line_id`, `refresh_page_image`, and the `works.image_dir` doc comments are NOT matched by these patterns and correctly remain.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "refactor(app): group page-image fields into PageImageState

Contained cluster of the AppState god-struct grouping (pure-tier). The five
flat page-image/calibration fields (page_images/image_dir/image_mode/
current_page_order/calibration_index) become one PageImageState sub-struct,
held as AppState.page_image. All-Default init via PageImageState::default().
Unusual: all 43 access sites are mod.rs-internal (the image/calibration
free functions), so the whole change is confined to src/app/mod.rs. Access
rewritten state.page_images -> state.page_image.images etc. Behavior-
preserving: access shape only. Boundary substrings (page_image_overlay,
page_image_for_line_id, refresh_page_image) untouched. 413 tests + clippy
115 unchanged.

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
git merge --no-ff refactor/appstate-grouping-page-image
```

- [ ] **Step 3: Re-verify on merged master**

```bash
cargo build && cargo test --bins 2>&1 | rg 'test result'
```
Expected: clean build, `413 passed`.

- [ ] **Step 4: Push, delete branch**

```bash
git push origin master
git branch -d refactor/appstate-grouping-page-image
```

- [ ] **Step 5: Update the audit ledger**

In `docs/superpowers/audit-opportunities.md`, update the `AppState god-struct` entry: mark the page_image cluster (`page_image` → `PageImageState`) DONE, note the mod.rs-internal nature (all access in one file), and that the remaining contained clusters are `word_cycle`, `echo_overlay` (pure-tier), `scansion`, `vocab_popup` (render-tier). Commit + push:

```bash
git add docs/superpowers/audit-opportunities.md
git commit -m "docs(audit): mark AppState grouping page_image cluster DONE

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push origin master
```

---

## Self-Review

**Spec coverage:**
- PageImageState (5 fields, `#[derive(Default)]`) in mod.rs (spec "The sub-struct") → Task 1 Step 1 ✓
- 5 flat fields → 1 `page_image: PageImageState` (spec "AppState change") → Task 1 Step 2 ✓
- all-Default `::default()` init, the only build_window edit (spec "All-Default init variant") → Task 1 Step 3 + Global Constraints ✓
- 43 access-site rewrites `state.<field>` → `state.page_image.<sub>` in mod.rs only (spec "Access-site rewrites") → Task 1 Step 4 ✓
- boundary substrings untouched: page_image_overlay / page_image_for_line_id name / refresh_page_image name / works.image_dir doc comments (spec "Boundaries") → Global Constraints + Task 1 Step 4 + Step 8 ✓
- pure-tier: 413 + clippy 115, no nav-fuzz (spec "Verification") → Global Constraints + Task 1 Steps 6-7 + Task 2 Step 1 ✓
- no facade (spec out-of-scope/idiom) → Global Constraints ✓
- mod.rs-internal: whole change in one file (spec "UNUSUAL") → Global Constraints + Task 1 Files ✓

**Placeholder scan:** No TBD/TODO. Line numbers given as `rg` locators (mod.rs edits shift internal lines as the rewrite proceeds, so name-based location is used). Every code block literal.

**Type consistency:** `PageImageState` sub-field names (images/dir/mode/page_order/calibration_index) consistent across struct def (Step 1), mapping (Global Constraints), and rewrite list (Step 4). The `AppState.page_image` field name (singular) matches init (`page_image:`) and every rewritten access (`state.page_image.<sub>`), and is distinct from the untouched `page_image_overlay`.
