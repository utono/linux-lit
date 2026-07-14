# Backslash Segment-Overlay Cycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Plain `\` cycles the per-segment overlays — journal Q&A → gloss → synopsis → journal (wraps) — anchored to the reader position where the lap started.

**Architecture:** A thin dispatcher module `src/input/actions/overlay_cycle.rs` composes the existing open/close handlers: a new `Action::CycleSegmentOverlays` (reader `\`) opens the journal stop; a plain-`\` arm in each of the three overlay modal handlers closes the current overlay by *restoring* its saved pre-open position (never the jump-to-source close) and opens the next stop. No new AppState fields: each stop's open already records `return_pos` = the reader position, which is the anchor as long as cycle closes always restore it. Empty stops keep their standalone fallbacks (journal → work-wide picker, gloss/synopsis → toast).

**Tech Stack:** Rust, GTK4. Repo: linux-lit, worktree `~/utono/linux-lit-wt/feat-backslash-overlay-cycle`, branch `feat/backslash-overlay-cycle`.

**Spec:** `docs/superpowers/specs/2026-07-12-backslash-overlay-cycle-design.md` (same directory).

## Global Constraints

- Work ONLY in the worktree `~/utono/linux-lit-wt/feat-backslash-overlay-cycle` (own `target/`; never set a shared `CARGO_TARGET_DIR`). All `cd` in this plan means the worktree.
- Verify with `cargo build` / `cargo test --bins`; do NOT run the app live — headless cage only (Task 5). Cleanup is ONLY `pkill -f "cage -- ./target/debug/linux-lit"` (a bare pattern kills the user's live instance).
- Plain `\` emits keysym `backslash` unshifted on RPD (the keycap strip shows `\` / `#`); no shift-state ambiguity.
- `Alt+\` (ToggleVocabHighlight) and `Ctrl+\` (OpenLibraryPicker in reader; work-wide Q&A picker inside the journal overlay) must keep working unchanged — every new `\` match must exclude ctrl and alt.
- Keybind mirrors change in the SAME task as the bind: stowed keymap.json (separate repo `~/tty-dotfiles` — its edit gets its own commit there), Ctrl+/ reader overlay, and the three overlay legends.
- Commit messages end with the Co-Authored-By/Claude-Session trailer the session already uses.

---

### Task 1: `Action::CycleSegmentOverlays` + reader `\` bind + dispatcher module

**Files:**
- Modify: `src/input/actions/mod.rs` (variant ~line 143, Category match ~line 293, `name()` ~line 439, module decls lines 5–20)
- Create: `src/input/actions/overlay_cycle.rs`
- Modify: `src/input/keymap_config.rs` (vocab_bindings ~line 332, test ~line 498)
- Modify: `src/input/keymap.rs` (dispatch_action match, after the `ToggleLastOverlay` arm ending ~line 3321)
- Test: existing `#[test]` in `src/input/keymap_config.rs` (the fn containing line 498)

**Interfaces:**
- Consumes: `crate::input::actions::journal::open_journal_scene(state: &Rc<RefCell<AppState>>)` (existing, already `pub(crate)`).
- Produces: `Action::CycleSegmentOverlays`; `overlay_cycle::cycle_from_reader(state: &Rc<RefCell<AppState>>)`. Task 3 adds three more fns to the same module.

- [ ] **Step 1: Write the failing test**

In `src/input/keymap_config.rs`, replace line 498:

```rust
        assert_eq!(m.get(&KeyCombo::plain("backslash")), None);
```

with:

```rust
        // `\` cycles the segment overlays (journal Q&A → gloss → synopsis).
        assert_eq!(
            m.get(&KeyCombo::plain("backslash")),
            Some(&Action::CycleSegmentOverlays)
        );
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd ~/utono/linux-lit-wt/feat-backslash-overlay-cycle && cargo test --bins keymap_config 2>&1 | tail -20
```

Expected: compile error `no variant named CycleSegmentOverlays` (a compile failure is the failing state here).

- [ ] **Step 3: Implement**

3a. `src/input/actions/mod.rs` — add the variant directly after `ToggleLastOverlay,` (line 143):

```rust
    /// Plain `\`: cycle the per-segment overlays — journal Q&A → gloss →
    /// synopsis → journal (wraps). From the reader it opens the journal stop;
    /// inside each overlay `\` advances (handled in the overlay modal
    /// handlers, not this reader action). See input/actions/overlay_cycle.rs.
    CycleSegmentOverlays,
```

3b. Same file, Category match — add `| Action::CycleSegmentOverlays` after `| Action::ToggleLastOverlay` (line 293) in the `Category::Vocab` arm.

3c. Same file, `name()` — after the `ToggleLastOverlay` arm (line 439):

```rust
            Action::CycleSegmentOverlays => "CycleSegmentOverlays",
```

(`Action` derives Serialize/Deserialize, so keymap.json's `"CycleSegmentOverlays"` parses via `parse_action` with no further work.)

3d. Same file, module decls — after `pub mod journal;` (line 14):

```rust
pub mod overlay_cycle;
```

3e. Create `src/input/actions/overlay_cycle.rs`:

```rust
//! Plain `\` segment-overlay cycle: journal Q&A → gloss → synopsis → journal
//! (wraps). The lap is anchored to the reader position where it started —
//! each advance closes the current overlay by RESTORING its saved pre-open
//! position (never the jump-to-source close), so every stop shows the same
//! segment even after Ctrl+n/p traversal inside an overlay. Empty stops keep
//! their standalone fallbacks: journal → work-wide Q&A picker (which ends the
//! lap), gloss/synopsis → toast, landing back in the reader at the anchor.
//! Escape and each overlay's own close/flip keys are untouched.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;

/// Reader `\` (Action::CycleSegmentOverlays): start the lap at the journal
/// Q&A stop. `open_journal_scene` records `journal.return_pos` = the current
/// reader position, which becomes the lap's anchor.
pub(crate) fn cycle_from_reader(state: &Rc<RefCell<AppState>>) {
    crate::input::actions::journal::open_journal_scene(state);
}
```

3f. `src/input/keymap.rs` — in the dispatch_action match, after the `ToggleLastOverlay => { ... }` arm (ends ~line 3321):

```rust
        CycleSegmentOverlays => crate::input::actions::overlay_cycle::cycle_from_reader(state),
```

3g. `src/input/keymap_config.rs` — in `vocab_bindings()`, after `(KeyCombo::ctrl("Tab"), Action::ToggleLastOverlay),` (line 332):

```rust
        // `\` cycles the segment overlays: journal Q&A → gloss → synopsis
        // (wraps; advance arms live in the overlay modal handlers).
        (KeyCombo::plain("backslash"), Action::CycleSegmentOverlays),
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd ~/utono/linux-lit-wt/feat-backslash-overlay-cycle && cargo test --bins 2>&1 | tail -5
```

Expected: all tests pass (the Action enum's matches are exhaustive — a missed arm fails compile here, which is the point of running the full bin suite).

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit-wt/feat-backslash-overlay-cycle && git add src/input/actions/mod.rs src/input/actions/overlay_cycle.rs src/input/keymap_config.rs src/input/keymap.rs && git commit -m "feat: bind \\ to CycleSegmentOverlays (reader stop: journal Q&A)"
```

---

### Task 2: Factor the gloss open half into `open_gloss_at_cursor`

Pure refactor so Task 3 can open the gloss stop without toggling.

**Files:**
- Modify: `src/input/actions/gloss.rs:2634-2740` (`toggle_overlay`)

**Interfaces:**
- Produces: `pub(crate) fn open_gloss_at_cursor(state: &Rc<RefCell<AppState>>)` in `src/input/actions/gloss.rs` — resolves the cursor line's covering glossed passage and opens the overlay; toasts "No gloss on this line" (and opens nothing) when there is none. Saves `gloss_return_pos` itself.
- Consumes: nothing new.

- [ ] **Step 1: Move the open half**

In `src/input/actions/gloss.rs`, `toggle_overlay` currently ends its close half with `return;` (~line 2652) and continues into the open logic (`const GLOSS_TYPES` at line 2655 through the `open_gloss_overlay(...)` call at line 2739). Move that entire open block into a new function placed directly after `toggle_overlay`, and call it from the toggle:

```rust
    open_gloss_at_cursor(state);
}

/// Open the gloss overlay for the passage covering the reader cursor line
/// (the open half of `toggle_overlay`, shared with the `\` segment-overlay
/// cycle). Toasts "No gloss on this line" and opens nothing when no glossed
/// passage covers the cursor. Saves `gloss_return_pos` from the current
/// reader position so Escape/cycle-advance can restore it.
pub(crate) fn open_gloss_at_cursor(state: &Rc<RefCell<AppState>>) {
    const GLOSS_TYPES: &[&str] = &["teacher-generic", "inner-monologue", "reader-gloss"];
    // ... body moved VERBATIM from toggle_overlay (lines 2655-2739):
    // cursor→(abbrev, triple) resolution, find_glossed_passages, covering
    // passage lookup, find_glosses_by_start, gloss_return_pos save,
    // open_gloss_overlay(&mut s, passages, passage_index, passage,
    //                    all_glosses, false, None);
}
```

The moved body is copied verbatim — no logic changes, same toasts, same `open_gloss_overlay(..., false, None)` tail.

- [ ] **Step 2: Build and test**

```bash
cd ~/utono/linux-lit-wt/feat-backslash-overlay-cycle && cargo build 2>&1 | tail -3 && cargo test --bins 2>&1 | tail -3
```

Expected: clean build, tests pass.

- [ ] **Step 3: Commit**

```bash
cd ~/utono/linux-lit-wt/feat-backslash-overlay-cycle && git add src/input/actions/gloss.rs && git commit -m "refactor: factor gloss open half into open_gloss_at_cursor"
```

---

### Task 3: Cycle-advance functions + `\` arms in the three overlay handlers

**Files:**
- Modify: `src/input/actions/overlay_cycle.rs`
- Modify: `src/input/keymap.rs` (`handle_journal_key` plain match ~line 1473; `handle_gloss_key` plain match ~line 1752; `handle_synopsis_overlay_key` plain match ~line 2076)

**Interfaces:**
- Produces: `cycle_from_journal`, `cycle_from_gloss`, `cycle_from_synopsis` — all `pub(crate) fn (state: &Rc<RefCell<AppState>>)` in `overlay_cycle.rs`.
- Consumes: `journal::open_journal_scene`, `gloss::open_gloss_at_cursor` (Task 2), `crate::app::scene_synopsis::show_synopsis_overlay`, `crate::app::return_to_reader_mode(&mut AppState)`, `crate::app::restore_saved_position_resnap(&mut AppState, Option<(usize, usize, i32)>)`.

- [ ] **Step 1: Add the advance functions**

Append to `src/input/actions/overlay_cycle.rs`:

```rust
/// Journal-overlay `\`: close restoring the anchor, then open the gloss stop
/// for the anchor line.
pub(crate) fn cycle_from_journal(state: &Rc<RefCell<AppState>>) {
    {
        let mut s = state.borrow_mut();
        s.journal_overlay.hide();
        // Recolor BEFORE restore's update_highlight, matching
        // journal::toggle_overlay's close half.
        crate::app::return_to_reader_mode(&mut s);
        // Take entry_page_id/return_pos so they don't leak into the next
        // open, but ALWAYS restore the saved position — never
        // jump_to_journal_source_start — so the lap stays on its entry
        // segment even after Ctrl+n/p traversal.
        s.journal.entry_page_id.take();
        let pos = s.journal.return_pos.take();
        crate::app::restore_saved_position_resnap(&mut s, pos);
    }
    crate::input::actions::gloss::open_gloss_at_cursor(state);
}

/// Gloss-overlay `\`: close restoring the anchor, then open the synopsis stop.
pub(crate) fn cycle_from_gloss(state: &Rc<RefCell<AppState>>) {
    {
        let mut s = state.borrow_mut();
        s.tts.stop();
        s.gloss_overlay.hide();
        s.gloss_opened_from_picker = false;
        crate::app::return_to_reader_mode(&mut s);
        // Restore, never jump_to_gloss_source_start — see module doc.
        let pos = s.gloss_return_pos.take();
        crate::app::restore_saved_position_resnap(&mut s, pos);
    }
    crate::app::scene_synopsis::show_synopsis_overlay(state);
}

/// Synopsis-overlay `\`: wrap back to the journal Q&A stop. The synopsis
/// never moves the reader cursor, so its close (hide + return to reader,
/// mirroring its `h`/Escape arms) already leaves the anchor current.
pub(crate) fn cycle_from_synopsis(state: &Rc<RefCell<AppState>>) {
    {
        let mut s = state.borrow_mut();
        s.gloss_overlay.hide();
        crate::app::return_to_reader_mode(&mut s);
    }
    crate::input::actions::journal::open_journal_scene(state);
}
```

Note: `show_synopsis_overlay` short-circuits (closes and returns) when `gloss_overlay.is_visible()` — `cycle_from_gloss` hides it first, so the real synopsis open runs. Do not reorder.

- [ ] **Step 2: Add the `\` arms**

2a. `handle_journal_key` — in the plain `match key_name` block, directly BEFORE the `"Escape"` arm (line 1473). Ctrl+`\` already returned earlier (line 1251, work-wide picker); guard alt so `Alt+\` stays a no-op here:

```rust
        // `\`: advance the segment-overlay cycle → gloss for the lap's entry
        // segment (Ctrl+\ = work-wide picker, handled above; Alt+\ excluded).
        "backslash" if !is_ctrl && !is_alt => {
            crate::input::actions::overlay_cycle::cycle_from_journal(state);
            true
        }
```

2b. `handle_gloss_key` — in its plain `match key_name` block, directly BEFORE the `"Escape" | "n"` arm (line 1752), same arm shape:

```rust
        // `\`: advance the segment-overlay cycle → synopsis for the lap's
        // entry segment (restores the pre-open page, unlike Escape's
        // jump-to-source close).
        "backslash" if !is_ctrl && !is_alt => {
            crate::input::actions::overlay_cycle::cycle_from_gloss(state);
            true
        }
```

2c. `handle_synopsis_overlay_key` — in the plain `match key_name` block, directly AFTER the `"h"` arm (lines 2076–2081):

```rust
        // `\`: advance the segment-overlay cycle → wrap to the journal Q&A
        // stop for the lap's entry segment.
        "backslash" if !is_ctrl && !is_alt => {
            crate::input::actions::overlay_cycle::cycle_from_synopsis(state);
            true
        }
```

(All three handlers reach their plain match with ctrl/alt fall-through, so the guards are required, mirroring the risk noted in Global Constraints.)

- [ ] **Step 3: Build and test**

```bash
cd ~/utono/linux-lit-wt/feat-backslash-overlay-cycle && cargo build 2>&1 | tail -3 && cargo test --bins 2>&1 | tail -3 && cargo clippy 2>&1 | tail -3
```

Expected: clean build, tests pass, no new clippy warnings.

- [ ] **Step 4: Commit**

```bash
cd ~/utono/linux-lit-wt/feat-backslash-overlay-cycle && git add src/input/actions/overlay_cycle.rs src/input/keymap.rs && git commit -m "feat: \\ advances the segment-overlay cycle inside journal/gloss/synopsis"
```

---

### Task 4: Keybind mirrors — stowed keymap.json, Ctrl+/ overlay, three legends

**Files:**
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (SEPARATE repo — own commit)
- Modify: `src/ui/keybinds_overlay.rs` (keycap strip line 64; describe() "Gloss / echo system" section ~line 267)
- Modify: `src/ui/journal_keybinds_overlay.rs` ("Cross-reference" group)
- Modify: `src/ui/gloss_keybinds_overlay.rs` ("View" group)
- Modify: `src/ui/synopsis_keybinds_overlay.rs` ("View" group)

**Interfaces:** none — display data only. Consult the `update-cairo-keybinds-overlay` skill's three-pass cross-reference when editing `keybinds_overlay.rs`.

- [ ] **Step 1: stowed keymap.json**

In `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`, the reader array already has the two modified `\` binds at lines 15–16. Add the plain bind beside them (JSON array — mind the commas):

```json
    {"key": "backslash", "action": "CycleSegmentOverlays"},
```

This file is stowed to `~/.config/linux-lit/keymap.json` and SILENTLY SHADOWS compiled defaults — without this line the new bind never fires on the user's machine.

- [ ] **Step 2: Ctrl+/ reader overlay**

In `src/ui/keybinds_overlay.rs` line 64, fill the empty plain-action label:

```rust
    key("\\", "#", "cycle overlays", "", &[("C-\\", "lib picker"), ("M-\\", "vocab hi")]),
```

In `describe()`, add to the "Gloss / echo system" section after the `"last overlay"` arm (line 267):

```rust
        "cycle overlays" => "Action::CycleSegmentOverlays (journal Q&A → gloss \
→ synopsis, wraps; segment fixed at lap entry) — src/input/actions/overlay_cycle.rs",
```

Then run the skill's three passes over the strip: every keycap label has a describe() arm; every arm is reachable; no bind changed in Tasks 1–3 is missing from the strip.

- [ ] **Step 3: the three overlay legends**

`src/ui/journal_keybinds_overlay.rs`, "Cross-reference" group, after `("Ctrl+g", "view gloss for passage"),`:

```rust
        ("\\", "cycle: → gloss (same segment)"),
```

`src/ui/gloss_keybinds_overlay.rs`, "View" group, before `("Esc / n / Ctrl+g", "close (jump to source)"),`:

```rust
        ("\\", "cycle: → synopsis (same segment)"),
```

`src/ui/synopsis_keybinds_overlay.rs`, "View" group, before `("h / Esc / Ctrl+g", "close"),`:

```rust
        ("\\", "cycle: → journal Q&A (same segment)"),
```

- [ ] **Step 4: Build and test**

```bash
cd ~/utono/linux-lit-wt/feat-backslash-overlay-cycle && cargo build 2>&1 | tail -3 && cargo test --bins 2>&1 | tail -3
```

Expected: clean build, tests pass (keybinds_overlay.rs has its own glyph tests — they must stay green).

- [ ] **Step 5: Commit both repos**

```bash
cd ~/utono/linux-lit-wt/feat-backslash-overlay-cycle && git add src/ui/keybinds_overlay.rs src/ui/journal_keybinds_overlay.rs src/ui/gloss_keybinds_overlay.rs src/ui/synopsis_keybinds_overlay.rs && git commit -m "docs(ui): mirror \\ segment-overlay cycle in Ctrl+/ overlay and legends"
cd ~/tty-dotfiles && git add linux-lit/.config/linux-lit/keymap.json && git commit -m "linux-lit: add plain backslash CycleSegmentOverlays bind"
```

---

### Task 5: Headless e2e acceptance

**Files:** none created (screenshots to the scratchpad). Read `CLAUDE.md`'s Headless Verification section first; cleanup is ONLY the scoped pkill.

- [ ] **Step 1: Pick a target segment**

Find a (work, div1, div2) that has scene Q&A, a glossed passage, and a synopsis, so the happy-path lap has all three stops. Read the SQL in `src/db/journal.rs::find_scene_band_pages`, `src/db/queries.rs::find_glossed_passages`, and the synopsis loader in `src/app/scene_synopsis.rs` to get the exact tables/filters, then query `~/utono/litdb/data/lit.db` with sqlite3 for a triple present in all three. Note the work's abbrev and a line inside that scene. Also note one work/line with NO gloss coverage (for the empty-stop check).

- [ ] **Step 2: Launch headless**

```bash
cd ~/utono/linux-lit-wt/feat-backslash-overlay-cycle && cargo build
LIT_NO_MPV=1 GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
sleep 3
ls /run/user/1000/wayland-*   # export WAYLAND_DISPLAY to the new socket
```

Confirm current binds in `src/input/keymap_config.rs` before scripting; drive keys with wtype (`wtype '\'` sends backslash; give the window ~3s to map; an ~2-byte PNG from grim means not-mapped — sleep and retry). Navigate to the target work/scene (library picker or search), confirming position with a grim screenshot.

- [ ] **Step 3: Drive the lap**

Press `\` four times, taking a grim screenshot after each press into the scratchpad. Per the UI review protocol, OPEN every PNG and report inline:

- Shot 1: journal overlay, scene-band Q&A for the cursor's scene.
- Shot 2: gloss overlay for a passage in the same scene.
- Shot 3: synopsis overlay for the same scene.
- Shot 4: journal overlay again (wrap), SAME scene band as shot 1.

Then press Escape and screenshot: the reader is back at the pre-lap position (same visible page as the pre-lap screenshot from Step 2).

- [ ] **Step 4: Anchor-hold check**

Start a lap, press Ctrl+n twice inside the journal overlay (traverse Q&A pages), then `\`. Screenshot: the gloss stop still shows the ENTRY segment's passage, not the traversed page's scene.

- [ ] **Step 5: Empty-stop check**

Navigate to the no-gloss line from Step 1, start a lap (`\` opens journal or the work-wide picker fallback — if the picker opens, Escape, and use a scene WITH Q&A but whose cursor line has no gloss), press `\` from the journal: expect the "No gloss on this line" toast and the READER visible at the anchor position (screenshot). Press `\` again: a fresh lap starts at the journal stop.

- [ ] **Step 6: Cleanup + full suite**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
cd ~/utono/linux-lit-wt/feat-backslash-overlay-cycle && cargo test --bins 2>&1 | tail -3 && cargo clippy 2>&1 | tail -3
```

Expected: tests pass, no new clippy warnings. Report every screenshot observation inline; a passing exit code is not acceptance.

---

### Task 6: Finish the branch

- [ ] **Step 1:** Invoke `superpowers:finishing-a-branch`. House default: merge back to master with `--no-ff` FROM THE MAIN CHECKOUT `~/utono/linux-lit` (git refuses master in two worktrees), re-verify `cargo build` on master, push `origin master`, then `git worktree remove ~/utono/linux-lit-wt/feat-backslash-overlay-cycle` and delete the branch. Remind the user to restart `crll` (running instances predate the rebuild) and that the tty-dotfiles commit ships the keymap.json half.
