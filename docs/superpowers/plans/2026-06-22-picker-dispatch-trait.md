# Picker Dispatch Trait Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collapse the duplicated `InputMode → picker field` dispatch (nav `j`/`k` + the 7 plain Escape-hide arms) behind a `Picker` trait and one `picker_for_mode` accessor.

**Architecture:** A new `src/input/picker_dispatch.rs` defines `trait Picker { move_selection; hide }`, a trivial forwarding `impl` per dispatched picker, and `picker_for_mode(&AppState, InputMode) -> Option<&dyn Picker>`. The keymap's two nav match blocks and seven plain Escape arms route through it; the `(mode → field)` map lives in exactly one place.

**Tech Stack:** Rust, GTK4 (gtk4-rs), trait objects (`&dyn`).

See spec: `docs/superpowers/specs/2026-06-22-picker-dispatch-trait-design.md`.

## Global Constraints

- **Zero behavior change.** Each picker's `move_selection` body (the #6-preserved index variants) is untouched; only the dispatch routes through the trait.
- **Rust/CLI rules (CLAUDE.md):** `rg`/`fd` not `grep`/`find`. Do not run the app — only `cargo build` / `cargo test --bins`; the user runs `cargo run`. For headless verification use `./scripts/e2e-env.sh`.
- **Verification is NOT build-only.** Dispatch wiring is runtime behavior; the final task REQUIRES a headless cage e2e (or, if seat-blocked, asking the user to run it).
- **Branch + merge per CLAUDE.md:** branch `refactor/picker-dispatch-trait` off `master`; finish with `git merge --no-ff`, re-verify, push, delete branch.

---

### Task 1: Add the `Picker` trait, forwarding impls, and `picker_for_mode`

**Files:**
- Create: `src/input/picker_dispatch.rs`
- Modify: `src/input/mod.rs` (register `pub mod picker_dispatch;`)

**Interfaces:**
- Consumes: the 10 picker types on `AppState` (exact paths in Step 1).
- Produces:
  - `pub trait Picker { fn move_selection(&self, delta: i32); fn hide(&self); }`
  - `pub(crate) fn picker_for_mode(s: &crate::app::AppState, mode: crate::app::InputMode) -> Option<&dyn Picker>`

- [ ] **Step 1: Create the module with trait, impls, and accessor**

Create `src/input/picker_dispatch.rs` with exactly this content:

```rust
use crate::app::{AppState, InputMode};

/// The uniform slice of picker behavior the keymap dispatches by InputMode.
/// Per-picker index math stays inside each `move_selection` impl (the variants
/// audit #6 preserved); this trait only routes to it.
pub trait Picker {
    fn move_selection(&self, delta: i32);
    fn hide(&self);
}

macro_rules! impl_picker {
    ($ty:ty) => {
        impl Picker for $ty {
            fn move_selection(&self, delta: i32) {
                <$ty>::move_selection(self, delta);
            }
            fn hide(&self) {
                <$ty>::hide(self);
            }
        }
    };
}

impl_picker!(crate::ui::bookmark_picker::BookmarkPicker);
impl_picker!(crate::ui::media_picker::MediaPicker);
impl_picker!(crate::ui::concordance_picker::ConcordancePicker);
impl_picker!(crate::ui::concordance_word_picker::ConcordanceWordPicker);
impl_picker!(crate::ui::concordance_list_picker::ConcordanceListPicker);
impl_picker!(crate::ui::concordance_works_picker::ConcordanceWorksPicker);
impl_picker!(crate::ui::gloss_picker::GlossPicker);
impl_picker!(crate::ui::authorship_picker::AuthorshipPicker);
impl_picker!(crate::ui::journal_picker::JournalQaPicker);
impl_picker!(crate::ui::echo_line_picker::EchoLinePicker);

/// The single source of truth for "which picker is active in this mode".
/// Returns None for non-picker modes (caller no-ops, matching the old `_ => {}`).
pub(crate) fn picker_for_mode(s: &AppState, mode: InputMode) -> Option<&dyn Picker> {
    match mode {
        InputMode::BookmarkPicker => Some(&s.bookmark_picker),
        InputMode::MediaPicker => Some(&s.media_picker),
        InputMode::ConcordancePicker => Some(&s.concordance_picker),
        InputMode::ConcordanceWordPicker => Some(&s.concordance_word_picker),
        InputMode::ConcordanceListPicker => Some(&s.concordance_list_picker),
        InputMode::ConcordanceWorksPicker => Some(&s.concordance_works_picker),
        InputMode::GlossPicker => Some(&s.gloss_picker),
        InputMode::AuthorshipPicker => Some(&s.authorship_picker),
        InputMode::JournalPicker => Some(&s.journal_picker),
        InputMode::EchoLinePicker => Some(&s.echo_line_picker),
        _ => None,
    }
}
```

Note: the `<$ty>::move_selection(self, delta)` form calls the *inherent* method (not the trait method) — avoids infinite recursion. All 10 types have inherent `pub fn move_selection(&self, i32)` and `pub fn hide(&self)` (verified).

- [ ] **Step 2: Register the module**

In `src/input/mod.rs`, add `pub mod picker_dispatch;` alongside the other `pub mod` lines (keep alphabetical if the file is ordered; otherwise append).

- [ ] **Step 3: Build to verify it compiles (unused-fn warnings OK until Task 2)**

Run: `cargo build 2>&1 | rg -i 'error|warning: unused.*picker_for_mode|Finished'`
Expected: `Finished`. A `function picker_for_mode is never used` dead-code warning is expected (adopted in Task 2). NO errors — especially no "method move_selection not found" (means an inherent method is missing) and no orphan-rule error.

- [ ] **Step 4: Commit**

```bash
git add src/input/picker_dispatch.rs src/input/mod.rs
git commit -m "feat(input): Picker trait + picker_for_mode accessor (unused)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 2: Route the nav (`j`/`k`) dispatch through the accessor

**Files:**
- Modify: `src/input/keymap.rs:449-480` (the `PickerAction::MoveDown` / `MoveUp` 10-arm blocks)

**Interfaces:**
- Consumes: `crate::input::picker_dispatch::picker_for_mode` (Task 1).

- [ ] **Step 1: Replace the two match blocks**

The current code (keymap.rs, the block that contains 20 `move_selection` arms — `PickerAction::MoveDown` at ~449 and `PickerAction::MoveUp` at ~465) reads:

```rust
        PickerAction::MoveDown => {
            match mode {
                InputMode::BookmarkPicker => state.borrow().bookmark_picker.move_selection(1),
                InputMode::MediaPicker => state.borrow().media_picker.move_selection(1),
                InputMode::ConcordancePicker => state.borrow().concordance_picker.move_selection(1),
                InputMode::ConcordanceWordPicker => state.borrow().concordance_word_picker.move_selection(1),
                InputMode::ConcordanceListPicker => state.borrow().concordance_list_picker.move_selection(1),
                InputMode::ConcordanceWorksPicker => state.borrow().concordance_works_picker.move_selection(1),
                InputMode::GlossPicker => state.borrow().gloss_picker.move_selection(1),
                InputMode::AuthorshipPicker => state.borrow().authorship_picker.move_selection(1),
                InputMode::JournalPicker => state.borrow().journal_picker.move_selection(1),
                InputMode::EchoLinePicker => state.borrow().echo_line_picker.move_selection(1),
                _ => {}
            }
            true
        }
        PickerAction::MoveUp => {
            match mode {
                InputMode::BookmarkPicker => state.borrow().bookmark_picker.move_selection(-1),
                InputMode::MediaPicker => state.borrow().media_picker.move_selection(-1),
                InputMode::ConcordancePicker => state.borrow().concordance_picker.move_selection(-1),
                InputMode::ConcordanceWordPicker => state.borrow().concordance_word_picker.move_selection(-1),
                InputMode::ConcordanceListPicker => state.borrow().concordance_list_picker.move_selection(-1),
                InputMode::ConcordanceWorksPicker => state.borrow().concordance_works_picker.move_selection(-1),
                InputMode::GlossPicker => state.borrow().gloss_picker.move_selection(-1),
                InputMode::AuthorshipPicker => state.borrow().authorship_picker.move_selection(-1),
                InputMode::JournalPicker => state.borrow().journal_picker.move_selection(-1),
                InputMode::EchoLinePicker => state.borrow().echo_line_picker.move_selection(-1),
                _ => {}
            }
            true
        }
```

Replace BOTH arms with:

```rust
        PickerAction::MoveDown => {
            let s = state.borrow();
            if let Some(p) = crate::input::picker_dispatch::picker_for_mode(&s, mode) {
                p.move_selection(1);
            }
            true
        }
        PickerAction::MoveUp => {
            let s = state.borrow();
            if let Some(p) = crate::input::picker_dispatch::picker_for_mode(&s, mode) {
                p.move_selection(-1);
            }
            true
        }
```

(`mode` is the local `let mode = s.input_mode;` already in scope; `InputMode` is `Copy`.)

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | rg -i 'error|Finished'`
Expected: `Finished`, no errors. The `picker_for_mode never used` warning from Task 1 is now gone.

- [ ] **Step 3: Verify the nav arms are gone, accessor is sole map**

Run: `rg -n 'InputMode::\w+ => state.borrow\(\).\w+_picker.move_selection' src/input/keymap.rs`
Expected: NO output (all nav-mode arms removed).
Run: `rg -c 'InputMode::\w+ => Some\(&s\.' src/input/picker_dispatch.rs`
Expected: `10`.

- [ ] **Step 4: Run the pure test suite**

Run: `cargo test --bins 2>&1 | rg -i 'test result|FAILED'`
Expected: `test result: ok. 413 passed` (count may differ if suite grew; 0 failed is the gate).

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "refactor(keymap): route picker nav dispatch via picker_for_mode

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 3: Collapse the 7 plain Escape-hide arms

**Files:**
- Modify: `src/input/keymap.rs` (the `PickerAction::Hide` match block, ~282-308)

**Interfaces:**
- Consumes: `crate::input::picker_dispatch::picker_for_mode` (Task 1).

- [ ] **Step 1: Replace the Hide match block**

The current `PickerAction::Hide` block reads (the 7 plain `→ Reader` arms + 3 special arms + `_ => {}`):

```rust
        PickerAction::Hide => {
            let mut s = state.borrow_mut();
            match mode {
                InputMode::BookmarkPicker => { s.bookmark_picker.hide(); s.input_mode = InputMode::Reader; }
                InputMode::MediaPicker => { s.media_picker.hide(); s.input_mode = InputMode::Reader; }
                InputMode::ConcordancePicker => { s.concordance_picker.hide(); s.input_mode = InputMode::Reader; }
                InputMode::ConcordanceWordPicker => { s.concordance_word_picker.hide(); s.input_mode = InputMode::Reader; }
                InputMode::ConcordanceListPicker => { s.concordance_list_picker.hide(); s.input_mode = InputMode::Reader; }
                InputMode::ConcordanceWorksPicker => { s.concordance_works_picker.hide(); s.input_mode = InputMode::Reader; }
                InputMode::GlossPicker => {
                    s.gloss_picker.hide();
                    // If the picker was opened from within the gloss overlay
                    // (Alt+g), the overlay is still visible behind it — return to
                    // it rather than dropping to the reader.
                    if s.gloss_picker_from_overlay {
                        s.gloss_picker_from_overlay = false;
                        s.input_mode = InputMode::GlossOverlay;
                    } else {
                        s.input_mode = InputMode::Reader;
                    }
                }
                InputMode::AuthorshipPicker => { s.authorship_picker.hide(); s.input_mode = InputMode::Reader; }
                InputMode::JournalPicker => { s.journal_picker.hide(); s.input_mode = InputMode::JournalOverlay; }
                InputMode::EchoLinePicker => { drop(s); crate::input::actions::echoes::cancel_add_echo(state); }
                _ => {}
            }
            true
        }
```

Replace with (keep the 3 special arms verbatim; collapse the 7 plain arms into the fallback):

```rust
        PickerAction::Hide => {
            let mut s = state.borrow_mut();
            match mode {
                InputMode::GlossPicker => {
                    s.gloss_picker.hide();
                    // If the picker was opened from within the gloss overlay
                    // (Alt+g), the overlay is still visible behind it — return to
                    // it rather than dropping to the reader.
                    if s.gloss_picker_from_overlay {
                        s.gloss_picker_from_overlay = false;
                        s.input_mode = InputMode::GlossOverlay;
                    } else {
                        s.input_mode = InputMode::Reader;
                    }
                }
                InputMode::JournalPicker => { s.journal_picker.hide(); s.input_mode = InputMode::JournalOverlay; }
                InputMode::EchoLinePicker => { drop(s); crate::input::actions::echoes::cancel_add_echo(state); }
                _ => {
                    if let Some(p) = crate::input::picker_dispatch::picker_for_mode(&s, mode) {
                        p.hide();
                        s.input_mode = InputMode::Reader;
                    }
                }
            }
            true
        }
```

Important: the fallback arm now also catches `GlossPicker`/`JournalPicker`/`EchoLinePicker`? No — they are matched by their explicit arms ABOVE, so they never reach `_`. The fallback's `picker_for_mode` will return `Some` for the 7 plain modes (Bookmark, Media, Concordance, ConcordanceWord, ConcordanceList, ConcordanceWorks, Authorship) and `None` for every non-picker mode — so a non-picker Hide no-ops exactly as the old `_ => {}` did.

- [ ] **Step 2: Build — resolve the borrow nuance if it errors**

Run: `cargo build 2>&1 | rg -i 'error|Finished'`
Expected: `Finished`.

IF (and only if) the build errors with `cannot borrow *s as mutable because it is also borrowed as immutable` on the `s.input_mode = ...` line, apply this fallback form instead (scope the immutable borrow so it ends before the mutable assignment):

```rust
                _ => {
                    let hid = crate::input::picker_dispatch::picker_for_mode(&s, mode)
                        .map(|p| p.hide())
                        .is_some();
                    if hid {
                        s.input_mode = InputMode::Reader;
                    }
                }
```

Then re-run the build; expect `Finished`. (NLL usually accepts the first form because `p.hide()` ends the borrow; this is the documented backup.)

- [ ] **Step 3: Verify the plain arms are gone, special arms remain**

Run: `rg -n 'InputMode::\w+ => \{ s.\w+_picker.hide\(\); s.input_mode = InputMode::Reader; \}' src/input/keymap.rs`
Expected: NO output (the 7 plain arms removed).
Run: `rg -c 'InputMode::GlossPicker =>|InputMode::JournalPicker =>|InputMode::EchoLinePicker =>' src/input/keymap.rs`
Expected: at least `3` (the special arms still present in the Hide block).

- [ ] **Step 4: Run the pure test suite**

Run: `cargo test --bins 2>&1 | rg -i 'test result|FAILED'`
Expected: `test result: ok. ... 0 failed`.

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "refactor(keymap): collapse plain Escape-hide arms via picker_for_mode

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 4: Headless verification gate (REQUIRED — runtime behavior)

**Files:** none (verification only).

This refactor changed dispatch *wiring*. Build + unit tests passing is necessary but NOT sufficient — the acceptance criterion is "j/k still moves the right picker and Escape still closes it on screen."

- [ ] **Step 1: Attempt the headless cage e2e**

Run the standing automated harness (boots the app in an isolated cage):
```bash
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture 2>&1 | rg -i 'test result|FAILED|panic'
```
Expected: `test result: ok. 1 passed` — confirms the app boots/renders with the new dispatch wired in.

- [ ] **Step 2: Drive a picker open + nav + close (manual cage, if the seat is free)**

Per `CLAUDE.md` "Headless Verification", in a throwaway cage:
```bash
cd ~/utono/linux-lit && cargo build
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
# wait for the new socket, then (export WAYLAND_DISPLAY to it):
# open a picker, send nav, screenshot, send Escape, screenshot.
# e.g. concordance picker: wtype "$(printf '\\')"  (Ctrl+\ opens it — check keymap),
# then wtype "j" / "k" to move, grim /tmp/shot.png, wtype Escape, grim /tmp/shot2.png
pkill -f "cage -- ./target/debug/linux-lit"; pkill -f target/debug/linux-lit
```
Then `Read` the PNGs: the picker's selection highlight must move on `j`/`k`, and `Escape` must return to the reader.

- [ ] **Step 3: If the cage is seat-blocked, STOP and ask the user**

Per `CLAUDE.md` "When to ASK THE USER to run e2e-env.sh": if cage dies with SIGTERM (exit 144) or the dev log never updates (live dwl owns the seat), do NOT claim verified. Tell the user plainly that build + 413 bin tests pass but runtime dispatch verification is blocked, and ask them to run:
```bash
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
```
and to manually confirm in their session that `j`/`k` navigate a picker and `Escape` closes it (open e.g. the library picker `Ctrl+p`, a concordance picker `Ctrl+\`, and the bookmark/media pickers). Paste the result before marking this task done.

- [ ] **Step 4: UI review (per CLAUDE.md "UI review protocol")**

If an e2e ran, open every PNG in `target/ui/` (and any `_clip.png`) and report inline what you see — quote the on-screen text, confirm picker selection moved and the reader returned after Escape. A passing exit code alone is not sufficient.

---

### Task 5: Finish the branch (merge, re-verify, push)

**Files:** none.

- [ ] **Step 1: Confirm clean tree + tests**

Run: `git status --short` (expect only committed work) and `cargo test --bins 2>&1 | rg 'test result'` (0 failed).

- [ ] **Step 2: Merge to master with --no-ff**

```bash
git checkout master
git merge --no-ff refactor/picker-dispatch-trait -m "Merge refactor/picker-dispatch-trait: Picker trait + picker_for_mode dispatch"
```

- [ ] **Step 3: Re-verify on the merged result**

Run: `cargo build 2>&1 | rg -i 'error|Finished'`
Expected: `Finished`.

- [ ] **Step 4: Push and delete the branch**

```bash
git push origin master
git branch -d refactor/picker-dispatch-trait
```

- [ ] **Step 5: Update the audit ledger**

In `docs/superpowers/audit-opportunities.md`, under "## Larger projects (not safe-scope)", move the `InputMode → picker dispatch accessor` bullet to a DONE note (cite the merge commit) — it is no longer a parked project for the nav + plain-hide scope. Note the still-deferred follow-on (Confirm + open paths). Commit:
```bash
git add docs/superpowers/audit-opportunities.md
git commit -m "docs(ledger): mark picker-dispatch trait done (nav + plain-hide scope)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
git push origin master
```

---

## Self-Review

**Spec coverage:**
- Trait + forwarding impls + accessor → Task 1. ✓
- Nav dispatch collapse → Task 2. ✓
- Escape plain-hide collapse, 3 special arms kept → Task 3. ✓
- Borrow nuance with documented fallback → Task 3 Step 2. ✓
- Mandatory headless gate + ask-user-if-blocked → Task 4. ✓
- Exclusions (Confirm, show/open, settings/action_popup) → not touched by any task; noted in spec + Task 5 ledger follow-on. ✓
- Branch/merge per CLAUDE.md → Task 5. ✓

**Placeholder scan:** No "TBD"/"add error handling"/"similar to". All code shown in full; both borrow forms given verbatim. ✓

**Type consistency:** `picker_for_mode(&AppState, InputMode) -> Option<&dyn Picker>` and `Picker { move_selection(&self, i32); hide(&self) }` named identically in Tasks 1/2/3. The 10 module paths match `AppState`'s field types (verified against app.rs). `JournalQaPicker` (not `JournalRow`) used. ✓
