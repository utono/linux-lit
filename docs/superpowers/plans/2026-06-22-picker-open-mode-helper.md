# Picker Open-Mode Helper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an `open_picker_mode(&mut AppState, InputMode)` helper for the uniform "enter picker mode" tail of every picker-open path, and normalize the 4 sites that redundantly take two RefCell borrows down to one `borrow_mut`.

**Architecture:** A trivial free fn `open_picker_mode` in `src/input/actions/pickers.rs` sets `s.input_mode = mode`. Each picker-open site keeps its own `show(...)` (the show signature varies per picker) and routes the trailing mode-set through the helper. Four double-borrow sites collapse `borrow()`+`borrow_mut()` into one `borrow_mut`.

**Tech Stack:** Rust, `Rc<RefCell<AppState>>` interior mutability.

See spec: `docs/superpowers/specs/2026-06-22-picker-open-mode-helper-design.md`.

## Global Constraints

- **Zero observable behavior change.** Same `show()` call, same final `input_mode`. The only structural change is borrow-count at 4 sites.
- **Borrow-merge safety:** at each of the 4 normalized sites, the implementer MUST confirm there is no intervening `state.borrow*()` / `state_clone.borrow*()` between the `show()` and the mode-set before merging into one borrow. (They are adjacent statements today.) If any site has an intervening borrow, keep it two-borrow and only swap the mode-set line through the helper.
- **EXCLUDED (do not touch):** the Confirm dispatch block in `handle_picker_key`; the library_picker open paths (`pickers.rs` `show_prepare*`/`show_finish`); all `set_items`/`set_words` calls.
- **Rust/CLI rules (CLAUDE.md):** `rg`/`fd` not grep/find; do NOT run the app — only `cargo build` / `cargo test --bins`; use `./scripts/e2e-env.sh` for any headless check; suppress verbose Bash output.
- **Branch + merge per CLAUDE.md:** branch `refactor/picker-open-mode-helper` off `master`; finish with `git merge --no-ff`, re-verify, push, delete branch.

---

### Task 1: Add the `open_picker_mode` helper

**Files:**
- Modify: `src/input/actions/pickers.rs` (add a top-level `pub(crate) fn`; `use crate::app::AppState;` is already present at line 6)

**Interfaces:**
- Produces: `pub(crate) fn open_picker_mode(s: &mut crate::app::AppState, mode: crate::app::InputMode)`

- [ ] **Step 1: Add the helper near the top of pickers.rs (after the imports / first use block)**

Insert this free function at module top level (not inside any `impl` or `fn`), e.g. just after the `use` lines:

```rust
/// Enter the given picker `mode`. The uniform tail of every picker-open path;
/// the caller does its own `show(...)` first (the show signature varies per
/// picker, so only the mode-set is shared).
pub(crate) fn open_picker_mode(s: &mut AppState, mode: crate::app::InputMode) {
    s.input_mode = mode;
}
```

- [ ] **Step 2: Build (unused-fn warning expected until Task 2)**

Run: `cargo build 2>&1 | rg -i 'error|warning: .*open_picker_mode|Finished'`
Expected: `Finished`. A `function open_picker_mode is never used` dead-code warning is expected (adopted in Task 2). No errors.

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/pickers.rs
git commit -m "feat(pickers): add open_picker_mode helper (unused)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 2: Adopt the helper at all 8 sites (and normalize the 4 double-borrow sites)

**Files:**
- Modify: `src/input/actions/pickers.rs` (6 sites)
- Modify: `src/input/actions/concordance.rs` (2 sites)

**Interfaces:**
- Consumes: `open_picker_mode(&mut AppState, InputMode)` from Task 1 (same-module in pickers.rs → bare `open_picker_mode`; from concordance.rs → `crate::input::actions::pickers::open_picker_mode`).

- [ ] **Step 1: Normalize the 3 plain double-borrow sites in pickers.rs (bookmark, media, gloss)**

Each is two adjacent lines `state_clone.borrow().<picker>.show(); state_clone.borrow_mut().input_mode = InputMode::X;` with NO intervening borrow (verify by reading the two lines — they are adjacent). Replace.

Bookmark (`pickers.rs` ~342-343):
```rust
            state_clone.borrow().bookmark_picker.show();
            state_clone.borrow_mut().input_mode = crate::app::InputMode::BookmarkPicker;
```
→
```rust
            let mut s = state_clone.borrow_mut();
            s.bookmark_picker.show();
            open_picker_mode(&mut s, crate::app::InputMode::BookmarkPicker);
```

Media (`pickers.rs` ~447-448): same shape, `media_picker` / `MediaPicker`:
```rust
            let mut s = state_clone.borrow_mut();
            s.media_picker.show();
            open_picker_mode(&mut s, crate::app::InputMode::MediaPicker);
```

Gloss (`pickers.rs` ~903-904): same shape, `gloss_picker` / `GlossPicker`:
```rust
            let mut s = state_clone.borrow_mut();
            s.gloss_picker.show();
            open_picker_mode(&mut s, crate::app::InputMode::GlossPicker);
```

- [ ] **Step 2: Normalize the concordance_word site (3 borrows → fold the show+mode tail)**

`pickers.rs` ~828-830:
```rust
    state.borrow_mut().concordance_word_picker.set_words(words);
    state.borrow().concordance_word_picker.show();
    state.borrow_mut().input_mode = crate::app::InputMode::ConcordanceWordPicker;
```
→ (keep `set_words` as-is; fold the `show()` + mode-set into one borrow — they are adjacent, no intervening borrow):
```rust
    state.borrow_mut().concordance_word_picker.set_words(words);
    let mut s = state.borrow_mut();
    s.concordance_word_picker.show();
    open_picker_mode(&mut s, crate::app::InputMode::ConcordanceWordPicker);
```

- [ ] **Step 3: Swap the mode-set line at the 2 arg-show sites (concordance_list, concordance_works)**

These keep their `show(&args)` + `drop(s)`; only the trailing mode-set line changes.

concordance_list (`pickers.rs` ~840), currently `state.borrow_mut().input_mode = crate::app::InputMode::ConcordanceListPicker;`:
```rust
    open_picker_mode(&mut state.borrow_mut(), crate::app::InputMode::ConcordanceListPicker);
```

concordance_works (`pickers.rs` ~862), currently `state.borrow_mut().input_mode = crate::app::InputMode::ConcordanceWorksPicker;`:
```rust
    open_picker_mode(&mut state.borrow_mut(), crate::app::InputMode::ConcordanceWorksPicker);
```

- [ ] **Step 4: Swap the mode-set line at the 2 concordance.rs sites (already single-borrow)**

Both are inside an existing `let mut s = state.borrow_mut();` and read `s.input_mode = crate::app::InputMode::ConcordancePicker;`.

`concordance.rs` ~316 (inside the cached branch, before `drop(s)`):
```rust
        s.concordance_picker.show();
        s.input_mode = crate::app::InputMode::ConcordancePicker;
```
→
```rust
        s.concordance_picker.show();
        crate::input::actions::pickers::open_picker_mode(&mut s, crate::app::InputMode::ConcordancePicker);
```

`concordance.rs` ~337 (inside the async block):
```rust
                s.concordance_picker.show();
                s.input_mode = crate::app::InputMode::ConcordancePicker;
```
→
```rust
                s.concordance_picker.show();
                crate::input::actions::pickers::open_picker_mode(&mut s, crate::app::InputMode::ConcordancePicker);
```

- [ ] **Step 5: Build**

Run: `cargo build 2>&1 | rg -i 'error|warning: .*open_picker_mode|Finished'`
Expected: `Finished`, no errors, and the `open_picker_mode never used` warning from Task 1 is gone.

If a borrow error appears (`cannot borrow ... as mutable/immutable`), it means a site had an intervening borrow that the merge violated — revert THAT site to its original two-borrow form and only swap the mode-set line through the helper (`open_picker_mode(&mut state.borrow_mut(), ...)`), then rebuild. Report which site.

- [ ] **Step 6: Verify all 8 sites adopted; excluded sites untouched**

Run: `rg -n 'input_mode = crate::app::InputMode::\w+Picker' src/input/actions/pickers.rs src/input/actions/concordance.rs`
Expected: NO matches for Bookmark/Media/ConcordanceWord/Concordance/ConcordanceList/ConcordanceWorks/Gloss Picker (all routed through the helper). `LibraryPicker` mode-sets (pickers.rs ~725, ~747) MUST still be present (excluded).
Run: `rg -c 'open_picker_mode\(' src/input/actions/pickers.rs src/input/actions/concordance.rs`
Expected: pickers.rs `7` (1 def + 6 calls), concordance.rs `2`.

- [ ] **Step 7: Run the pure test suite**

Run: `cargo test --bins 2>&1 | rg -i 'test result|FAILED'`
Expected: `test result: ok. ... 0 failed` (413 expected).

- [ ] **Step 8: Commit**

```bash
git add src/input/actions/pickers.rs src/input/actions/concordance.rs
git commit -m "refactor(pickers): route picker-open mode-set via open_picker_mode

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 3: Headless boot smoke + finish the branch

**Files:** none (verification + merge).

- [ ] **Step 1: Headless boot/render smoke (low-risk; open paths unchanged in behavior)**

Run: `./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture 2>&1 | rg -i 'test result|FAILED|panic'`
Expected: `test result: ok. 1 passed` — confirms the app boots/renders with the adopted open paths. (If the cage is seat-blocked / SIGTERM, do NOT claim runtime-verified — note that build + 413 tests pass and that picker-open behavior is unchanged structurally; the prior dispatch round user-verified picker open/nav/close, and this change does not alter what `show()` does.)

- [ ] **Step 2: Confirm clean tree + tests**

Run: `git status --short` (only committed work) and `cargo test --bins 2>&1 | rg 'test result'` (0 failed).

- [ ] **Step 3: Merge to master with --no-ff**

```bash
git checkout master
git merge --no-ff refactor/picker-open-mode-helper -m "Merge refactor/picker-open-mode-helper: open_picker_mode helper + borrow normalization"
```

- [ ] **Step 4: Re-verify on the merged result**

Run: `cargo build 2>&1 | rg -i 'error|Finished'`
Expected: `Finished`.

- [ ] **Step 5: Push and delete the branch**

```bash
git push origin master
git branch -d refactor/picker-open-mode-helper
```

- [ ] **Step 6: Update the audit ledger**

In `docs/superpowers/audit-opportunities.md`, under the "Larger projects" → picker-dispatch DONE note, update the deferred follow-on line: the `show()`/open pairs are now done via `open_picker_mode`; only the Confirm dispatch remains deferred (bespoke per arm). Commit:
```bash
git add docs/superpowers/audit-opportunities.md
git commit -m "docs(ledger): picker open-mode helper done; only Confirm dispatch deferred

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
git push origin master
```

---

## Self-Review

**Spec coverage:**
- `open_picker_mode` helper → Task 1. ✓
- 4 borrow-normalized sites (bookmark, media, gloss, concordance_word) → Task 2 Steps 1-2. ✓
- 2 arg-show mode-set swaps (concordance_list, works) → Task 2 Step 3. ✓
- 2 concordance.rs single-borrow swaps → Task 2 Step 4. ✓ (8 sites total)
- Borrow-merge safety check + fallback → Global Constraints + Task 2 Step 5. ✓
- Exclusions (Confirm, library_picker, set_items/words) → Global Constraints + grep gate Task 2 Step 6. ✓
- Verification (build, tests, headless smoke) → Task 2 Steps 5-7, Task 3 Step 1. ✓
- Branch/merge per CLAUDE.md → Task 3. ✓

**Placeholder scan:** No "TBD"/"add error handling"/"similar to Task N". Every site's before/after shown in full. ✓

**Type consistency:** `open_picker_mode(&mut AppState, InputMode)` named identically in Task 1 (def) and Task 2 (all call sites). Same-module calls use bare `open_picker_mode`; cross-module (concordance.rs) use the full `crate::input::actions::pickers::` path — consistent throughout. ✓
