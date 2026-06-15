# Gloss Loading-Card Scrolling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the "Glossing…" loading card scrollable so a long source passage can be read while the gloss generates.

**Architecture:** The loading card (`GlossOverlay::show_glossing`) already renders the passage into the same scrolling viewport the result gloss uses, but clears the block list, so the block-cursor nav keys (`j`/`k`/`gg`/`G`) no-op. We route navigation keys in `handle_gloss_key` to the existing direct-viewport scroll methods (`scroll_gloss`, `scroll_gloss_to_top`, `scroll_gloss_to_bottom`) **only when the card has no blocks** (the loading state), and add one thin `scroll_gloss_page` method for page-sized scrolls. The result-gloss block navigation is untouched.

**Tech Stack:** Rust, GTK4 (gtk4-rs), `cargo`.

---

## File structure

- **Modify** `src/ui/gloss_overlay.rs` — add `scroll_gloss_page(delta: i32)`, a page-sized sibling of `scroll_gloss` (same snap + bottom-clip logic, stepping by the viewport `page_size()`).
- **Modify** `src/input/keymap.rs` — in `handle_gloss_key`, gate `j`/`k`/`gg`/`G` on whether the card has blocks; add `x`/`y` page scroll. When `current_block()` is `None` (loading state), scroll the viewport; otherwise keep block-cursor nav.

No `keymap.json` or Ctrl+/ overlay change: these keys keep their gloss-overlay identity; they only gain a scroll behavior in the loading sub-state.

---

## Background the implementer needs

- `GlossOverlay::current_block()` (`src/ui/gloss_overlay.rs`) returns
  `Option<(BlockKind, i32)>`; it is `None` exactly when `self.blocks` is empty —
  which is the loading state (`show_glossing` clears `blocks`). This is the gate.
- Existing scroll methods on `GlossOverlay`:
  - `scroll_gloss(delta: i32)` — steps `row_step() * 3.0 * delta`, snaps the
    viewport top to a whole visual row (`snap_value_to_line`), updates the bottom
    clip, repaints the bar. **Already used by the echoes overlay.**
  - `scroll_gloss_to_top()` / `scroll_gloss_to_bottom()` — jump to extremes,
    snapping + clipping the same way.
- The echoes overlay precedent is `handle_echoes_overlay_key` in
  `src/input/keymap.rs` (`"j" => scroll_gloss(1)`, `"k" => scroll_gloss(-1)`).
- Page keys: in the reader, `x` = PageForward, `y` = PageBackward (literal GTK
  key names). The overlay key handlers match literal key names, so we add `"x"`
  / `"y"` arms in `handle_gloss_key`.
- `gg` is a chord: the gloss handler already has a `ChordState::PendingG` branch
  that, on the second `g`, calls `cursor_first_block()`. We make that branch
  scroll-to-top when there are no blocks. `G` is the `"G"` match arm.
- These scroll methods all call `mark_cursor_block()` internally, which is a safe
  no-op when `blocks` is empty — so calling them in the loading state is fine.

---

### Task 1: Add `scroll_gloss_page` to `GlossOverlay`

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (insert after `scroll_gloss`, before `scroll_gloss_to_top`, around line 1602)

There is no pure-logic unit test for the scroll methods (they read live GTK
adjustment/Pango geometry — the same reason `scroll_gloss` has none). Verify by
compile + the runtime check in Task 3. Keep the method a faithful page-sized
mirror of `scroll_gloss`.

- [ ] **Step 1: Add the method**

Insert this immediately after the closing brace of `scroll_gloss` (currently line 1602):

```rust
    /// Page-sized sibling of `scroll_gloss`: step the viewport by one page
    /// (`page_size()`) per press in `delta`'s direction, then snap the top to a
    /// whole visual row and re-size the bottom clip — same invariants as
    /// `scroll_gloss` (no fractional top line under the title rule; partial
    /// bottom row masked by the clip box). Used by the loading card's page keys.
    pub fn scroll_gloss_page(&self, delta: i32) {
        let adj = self.gloss_scrolled.vadjustment();
        // One page per press, less a row of overlap so the line at the fold
        // stays visible across the turn (matches a reader's page step).
        let overlap = self.row_step();
        let page = (adj.page_size() - overlap).max(overlap);
        let raw_target = adj.value() + page * delta as f64;
        let target = self.snap_value_to_line(raw_target);
        adj.set_value(target);
        self.update_bottom_clip();
        self.bar_drawing.queue_draw();
        self.mark_cursor_block();
    }
```

- [ ] **Step 2: Build to verify it compiles**

Run: `cargo build`
Expected: builds clean (a warning that `scroll_gloss_page` is never used is
acceptable here — Task 2 wires it up; if `-D warnings` is in effect, proceed to
Task 2 before treating the unused-method warning as a failure).

- [ ] **Step 3: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): add scroll_gloss_page for page-sized viewport scroll

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Route loading-card keys to viewport scroll in `handle_gloss_key`

**Files:**
- Modify: `src/input/keymap.rs` (`handle_gloss_key`, the `ChordState::PendingG` branch ~line 705, the `j`/`k` arms ~812-821, the `G` arm ~799-803, and add `x`/`y` arms)

The gate is `state.borrow().gloss_overlay.current_block().is_none()` — true in
the loading state (no blocks), false for the result gloss.

- [ ] **Step 1: Gate the `gg` chord on block presence**

Find this branch (currently ~line 705):

```rust
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            state.borrow().gloss_overlay.cursor_first_block();
        }
        return true;
    }
```

Replace its body with a block-presence gate:

```rust
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            // Loading card (no blocks): scroll the viewport to the top.
            // Result gloss: jump the block cursor to the first block.
            let has_blocks = state.borrow().gloss_overlay.current_block().is_some();
            if has_blocks {
                state.borrow().gloss_overlay.cursor_first_block();
            } else {
                state.borrow().gloss_overlay.scroll_gloss_to_top();
            }
        }
        return true;
    }
```

- [ ] **Step 2: Gate the `G` arm**

Find (currently ~line 799):

```rust
        "G" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            state.borrow().gloss_overlay.cursor_last_block();
            true
        }
```

Replace with:

```rust
        "G" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            // Loading card (no blocks): scroll the viewport to the bottom.
            // Result gloss: jump the block cursor to the last block.
            if state.borrow().gloss_overlay.current_block().is_some() {
                state.borrow().gloss_overlay.cursor_last_block();
            } else {
                state.borrow().gloss_overlay.scroll_gloss_to_bottom();
            }
            true
        }
```

- [ ] **Step 3: Gate the `j` and `k` arms**

Find (currently ~line 812):

```rust
        "j" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            state.borrow().gloss_overlay.cursor_next_block();
            true
        }
        "k" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            state.borrow().gloss_overlay.cursor_prev_block();
            true
        }
```

Replace with:

```rust
        "j" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            // Loading card (no blocks): scroll the viewport down.
            // Result gloss: step the block cursor to the next block.
            if state.borrow().gloss_overlay.current_block().is_some() {
                state.borrow().gloss_overlay.cursor_next_block();
            } else {
                state.borrow().gloss_overlay.scroll_gloss(1);
            }
            true
        }
        "k" => {
            crate::input::actions::gloss::stop_all_gloss_audio(state);
            if state.borrow().gloss_overlay.current_block().is_some() {
                state.borrow().gloss_overlay.cursor_prev_block();
            } else {
                state.borrow().gloss_overlay.scroll_gloss(-1);
            }
            true
        }
```

- [ ] **Step 4: Add `x` / `y` page-scroll arms (loading card only)**

Add these two arms in the same `match key_name { … }` block (e.g. right after
the `k` arm). They page-scroll only in the loading state; in the result gloss
they fall through to a no-op so they don't disturb block navigation:

```rust
        "x" => {
            // Loading card (no blocks): page the viewport forward.
            if state.borrow().gloss_overlay.current_block().is_none() {
                state.borrow().gloss_overlay.scroll_gloss_page(1);
            }
            true
        }
        "y" => {
            // Loading card (no blocks): page the viewport backward.
            if state.borrow().gloss_overlay.current_block().is_none() {
                state.borrow().gloss_overlay.scroll_gloss_page(-1);
            }
            true
        }
```

Note: the gloss handler's `Escape | n` arm closes the overlay via `"n"`, not
`"y"`, so adding `"y"` here does not collide with the cancel key.

- [ ] **Step 5: Build to verify it compiles**

Run: `cargo build`
Expected: builds clean, no unused-method warning for `scroll_gloss_page` (now used).

- [ ] **Step 6: Run the pure-logic test suite**

Run: `cargo test --bins`
Expected: PASS (this change adds no pure-logic helpers; the suite stays green).

- [ ] **Step 7: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat(gloss): scroll the Glossing loading card with j/k/x/y/gg/G

Gate gloss-overlay nav keys on block presence: with no blocks (the
loading card) j/k/x/y/gg/G scroll the passage viewport via the existing
scroll_gloss* methods; with blocks (the result gloss) they keep stepping
the block cursor unchanged.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Runtime verification (user-run, visual)

**Files:** none (verification only).

The acceptance criterion is visual — "the loading card scrolls" — so per the
project's headless-test rule it must be confirmed by launching the app. An agent
generally cannot launch cage from the live dwl session (seat busy / SIGTERM), so
this task is handed to the user.

- [ ] **Step 1: Ask the user to verify**

Give the user these steps to run:

1. `cargo run`
2. Navigate to a long passage (e.g. 2H6, York's "Anjou and Maine…" speech) and
   start a gloss (`Ctrl+g`) on it so the "Glossing…" card appears.
3. While the card is up, confirm:
   - `j` / `k` scroll the passage down / up by a few lines,
   - `x` / `y` page forward / back,
   - `gg` jumps to the top, `G` to the bottom,
   - the top line is never clipped under the title rule and the bottom line is
     not bisected by the footer rule (whole rows only).
4. After the gloss arrives, confirm the result gloss still navigates by block
   (`j`/`k`/`gg`/`G` move the accent bar between blocks as before) — i.e. the
   block navigation was NOT regressed.

- [ ] **Step 2: Address any issue the user reports, else mark complete.**

---

## Self-review

- **Spec coverage:** loading-card scroll keys (`j`/`k`/`x`/`y`/`gg`/`G`) → Task 2;
  page-step method → Task 1; empty-blocks gate → Task 2; result-gloss path
  untouched → Task 2 (the `if has_blocks` branches preserve it); visual
  verification → Task 3. No spec requirement is unaddressed.
- **Placeholder scan:** none — every code step shows the full replacement.
- **Type consistency:** `current_block()` returns `Option<(BlockKind, i32)>`
  (used only via `.is_some()`/`.is_none()`); `scroll_gloss`,
  `scroll_gloss_page`, `scroll_gloss_to_top`, `scroll_gloss_to_bottom`,
  `cursor_first_block`, `cursor_last_block`, `cursor_next_block`,
  `cursor_prev_block` are all existing/added `pub` methods on `GlossOverlay`
  with the signatures used here.
