# Translation Lockstep Two-Column Scroll Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** While translations are visible in two-column mode, scroll the left column continuously and re-point the right column each tick so the spread reads contiguously (left bottom → right top) with no clipping; restore the pre-toggle page on exit.

**Architecture:** Reuse the existing two-view, one-buffer renderer. Add a `translation_scroll_active` flag, a saved pre-toggle page, and a re-entrancy guard to `AppState`. A single `value-changed` handler on the left `ScrolledWindow`'s vadjustment computes the left column's last-fully-visible line and re-points the right view's scroll + bottom clip. Navigation keys (j/k/q/x) branch into a small `scroll_mode` helper module when the flag is set; e-reader pagination is untouched otherwise.

**Tech Stack:** Rust, GTK4 (gtk4-rs), sourceview5 (`View`, `Buffer`), libadwaita. Binary-only crate — tests run via `cargo test` (no lib target).

**Reference:** Design spec at `docs/superpowers/specs/2026-06-02-translation-lockstep-scroll-design.md`.

---

## File Structure

- **Create** `src/input/scroll_mode.rs` — all continuous-scroll-mode logic: the
  pure split helper, the scroll-sync routine, and the key-handler branches
  (scroll by line, scroll by spread, make-cursor-visible). One responsibility:
  "how the two columns behave while translations scroll."
- **Modify** `src/app.rs` — add three `AppState` fields + initializers; capture
  pre-toggle page and enter scroll mode in `show_translations`; restore + exit
  in `hide_translations`; connect the left vadjustment `value-changed` handler
  after the `state` Rc is built.
- **Modify** `src/input/mod.rs` — declare `pub(crate) mod scroll_mode;`.
- **Modify** `src/input/navigation.rs` — early-branch `page_forward`,
  `cursor_next_dialogue`, `cursor_prev_line`, `jump_to_next_dialogue`,
  `jump_to_prev_dialogue` into `scroll_mode` when `translation_scroll_active`.

---

## Task 1: AppState fields for scroll mode

**Files:**
- Modify: `src/app.rs` (struct fields near `translations_visible` ~line 173; initializer near line 1269)

- [ ] **Step 1: Add the three fields to the AppState struct**

In `src/app.rs`, find the block (currently around line 173):

```rust
    pub translations_visible: bool,
    /// Sign-column visibility saved when translations are shown, so it can be
    /// restored when translations are hidden. `None` when not in translation
    /// mode. Signs are hidden while translations are visible.
    pub sign_visible_before_translations: Option<bool>,
```

Add immediately after it:

```rust
    /// True while continuous-scroll translation mode is active. Only ever set
    /// when `column_count() == 2`. Drives the left-vadjustment scroll-sync
    /// handler and the navigation key branches in `input::scroll_mode`.
    pub translation_scroll_active: bool,
    /// `(current_line, page_top_line)` captured before `show_translations`
    /// mutates them, so exit restores the exact pre-toggle page. `None` when
    /// not in translation mode.
    pub pre_translation_page: Option<(usize, usize)>,
    /// Re-entrancy guard set while the scroll-sync handler writes the right
    /// view's adjustment, so that write cannot recurse back into the handler.
    pub right_scroll_syncing: std::cell::Cell<bool>,
```

- [ ] **Step 2: Add the initializers**

In `src/app.rs`, find (around line 1269):

```rust
        translations_visible: false,
        sign_visible_before_translations: None,
        translation_lines: Vec::new(),
```

Change to:

```rust
        translations_visible: false,
        sign_visible_before_translations: None,
        translation_scroll_active: false,
        pre_translation_page: None,
        right_scroll_syncing: std::cell::Cell::new(false),
        translation_lines: Vec::new(),
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | grep -E "^error|Finished" | tail -3`
Expected: `Finished ...` with no `error` lines.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "scroll-mode: add AppState fields for translation lockstep scroll"
```

---

## Task 2: Pure split helper + unit test

This is the only unit-testable kernel: given line heights, a scroll offset, and
the left view height, return the left column's last-fully-visible line index.
The right column top is always that + 1 (the contiguity invariant).

**Files:**
- Create: `src/input/scroll_mode.rs`
- Modify: `src/input/mod.rs`

- [ ] **Step 1: Declare the module**

In `src/input/mod.rs`, add alongside the other `mod` declarations:

```rust
pub(crate) mod scroll_mode;
```

- [ ] **Step 2: Write the failing test**

Create `src/input/scroll_mode.rs` with ONLY the pure helper signature and tests:

```rust
//! Continuous two-column scroll for translation mode. The left column scrolls
//! freely; the right column is re-pointed each scroll tick so its top line is
//! the line just after the left column's last fully visible line. Active only
//! while translations are visible in two-column mode.

/// Given per-line pixel heights (indexed by buffer line), the current left
/// scroll offset in pixels, the line index at the very top of the buffer the
/// scan should start from (always 0 in practice; a parameter for testability),
/// and the left column's usable height in pixels, return the index of the last
/// line that fully fits in the left column.
///
/// `heights[i]` is the rendered height of buffer line `i`. The scan finds the
/// first line whose cumulative top `y >= scroll_v` (the effective top line),
/// then sums heights until the next line would overflow `usable_height`.
/// Returns the effective top line itself when nothing else fits.
pub(crate) fn left_last_visible(
    heights: &[i32],
    scroll_v: i32,
    usable_height: i32,
) -> usize {
    // Find effective top line: first line whose top y >= scroll_v.
    let mut top_line = 0usize;
    let mut y = 0i32;
    while top_line + 1 < heights.len() && y + heights[top_line] <= scroll_v {
        y += heights[top_line];
        top_line += 1;
    }
    // Sum from top_line until the next line would overflow usable_height.
    let mut used = 0i32;
    let mut last = top_line;
    let mut i = top_line;
    while i < heights.len() {
        if used + heights[i] > usable_height {
            break;
        }
        used += heights[i];
        last = i;
        i += 1;
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_last_visible_basic_fit() {
        // 10 lines of height 20; usable 100 -> 5 lines fit (0..=4) at scroll 0.
        let heights = vec![20; 10];
        assert_eq!(left_last_visible(&heights, 0, 100), 4);
    }

    #[test]
    fn left_last_visible_after_scroll() {
        // Scroll past 2 lines (40px): top line becomes 2; 5 lines fit -> 6.
        let heights = vec![20; 10];
        assert_eq!(left_last_visible(&heights, 40, 100), 6);
    }

    #[test]
    fn left_last_visible_partial_line_excluded() {
        // usable 90, height 20 -> only 4 full lines (80<=90, 100>90) -> 3.
        let heights = vec![20; 10];
        assert_eq!(left_last_visible(&heights, 0, 90), 3);
    }

    #[test]
    fn left_last_visible_uneven_heights() {
        // heights: 30,30,30,30; usable 70 -> lines 0,1 fit (60), line 2 (90)
        // overflows -> last = 1.
        let heights = vec![30, 30, 30, 30];
        assert_eq!(left_last_visible(&heights, 0, 70), 1);
    }

    #[test]
    fn left_last_visible_clamps_to_top_when_nothing_fits() {
        // First line taller than usable -> returns the top line itself.
        let heights = vec![200, 20, 20];
        assert_eq!(left_last_visible(&heights, 0, 100), 0);
    }
}
```

- [ ] **Step 3: Run the test to verify it passes**

Run: `cargo test scroll_mode::tests 2>&1 | grep -E "test result|FAILED" | tail -3`
Expected: `test result: ok. 5 passed; 0 failed; ...`

(The implementation is included with the test because the kernel is small and
self-contained; the test exercises five distinct branches.)

- [ ] **Step 4: Commit**

```bash
git add src/input/scroll_mode.rs src/input/mod.rs
git commit -m "scroll-mode: pure left_last_visible split kernel + tests"
```

---

## Task 3: Scroll-sync routine (right follows left)

Re-point the right view and clips from the left scroll offset. This is the
GTK-bound wrapper around the kernel; it is not unit-tested (it needs live
widgets) — it is verified manually in Task 7.

**Files:**
- Modify: `src/input/scroll_mode.rs`

- [ ] **Step 1: Add the sync routine**

Append to `src/input/scroll_mode.rs` (before the `#[cfg(test)]` module):

```rust
use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use sourceview5::prelude::*;

use crate::app::AppState;

/// Re-point the right column so its top line is the line just after the left
/// column's last fully visible line, and update both bottom clips. Called from
/// the left vadjustment `value-changed` handler and once on mode entry.
///
/// No-ops unless `translation_scroll_active` and the views have a real height.
/// Guards `right_scroll_syncing` so writing the right adjustment cannot recurse.
pub(crate) fn sync_right_to_left(state: &AppState) {
    if !state.translation_scroll_active {
        return;
    }
    let left_h = state.text_view.height();
    if left_h <= 0 {
        return;
    }
    let line_count = state.buffer.line_count() as usize;
    if line_count == 0 {
        return;
    }

    let scroll_v = state.scrolled_window.vadjustment().value() as i32;

    // Build per-line heights lazily by scanning from line 0. line_yrange gives
    // absolute y, so we derive heights as we go. For large buffers this is the
    // one O(n) cost per tick; acceptable for now (see plan note). Compute the
    // effective top line and left-column last-fit in a single pass mirroring
    // `left_last_visible`.
    let usable_left = left_h
        - crate::input::viewport::descender_guard_px(&state.text_view, 0)
        - crate::input::scroll::BASE_BOTTOM_MARGIN;

    let mut top_line = 0usize;
    let mut y = 0i32;
    while top_line + 1 < line_count {
        let Some(iter) = state.buffer.iter_at_line(top_line as i32) else { break };
        let (_y, h) = state.text_view.line_yrange(&iter);
        if y + h <= scroll_v {
            y += h;
            top_line += 1;
        } else {
            break;
        }
    }
    let mut used = 0i32;
    let mut split_last = top_line;
    let mut i = top_line;
    while i < line_count {
        let Some(iter) = state.buffer.iter_at_line(i as i32) else { break };
        let (_y, h) = state.text_view.line_yrange(&iter);
        if used + h > usable_left {
            break;
        }
        used += h;
        split_last = i;
        i += 1;
    }
    let split = (split_last + 1).min(line_count);

    // Right column starts at `split`. Scroll the right view there.
    state.right_scroll_syncing.set(true);
    if split < line_count {
        if let Some(iter) = state.buffer.iter_at_line(split as i32) {
            let (ry, _h) = state.right_view.line_yrange(&iter);
            let radj = state.right_scrolled_window.vadjustment();
            let rmax = (radj.upper() - radj.page_size()).max(0.0);
            radj.set_value((ry as f64).min(rmax));
        }
    }
    state.right_scroll_syncing.set(false);

    // Left column fills to the viewport bottom in scroll mode: no clip.
    state.bottom_clip.set_height_request(0);

    // Right column: fit from `split` against the right view height and clip the
    // remainder.
    let right_h = state.right_view.height().max(left_h);
    let usable_right = right_h
        - crate::input::viewport::descender_guard_px(&state.right_view, split)
        - crate::input::scroll::BASE_BOTTOM_MARGIN;
    let right = crate::input::viewport::visible_range(
        &state.right_view, &state.buffer, split, line_count, usable_right,
    );
    let right_end = (right.last_fit + 1).min(line_count);
    crate::input::scroll::update_bottom_clip_public(
        &state.right_view,
        &state.right_bottom_clip,
        &state.right_scrolled_window,
        split,
        right_end,
        state.is_prose(),
    );
}

/// Hold an `Rc<RefCell<AppState>>` purely to satisfy the signal closure shape;
/// see `connect_scroll_sync`.
pub(crate) fn connect_scroll_sync(state: &Rc<RefCell<AppState>>) {
    let adj = state.borrow().scrolled_window.vadjustment();
    let state_for_sync = Rc::clone(state);
    adj.connect_value_changed(move |_adj| {
        // Re-entrancy: ignore ticks caused by our own right-view writes.
        if state_for_sync.borrow().right_scroll_syncing.get() {
            return;
        }
        sync_right_to_left(&state_for_sync.borrow());
    });
}
```

- [ ] **Step 2: Verify `update_bottom_clip_public` signature matches**

The call above passes `(view, clip, scrolled_window, page_top, line_count,
is_prose)`. Confirm `src/input/scroll.rs` defines:

```rust
pub(crate) fn update_bottom_clip_public(
    text_view: &sourceview5::View,
    bottom_clip: &gtk4::Box,
    scrolled_window: &gtk4::ScrolledWindow,
    page_top: usize,
    line_count: usize,
    is_prose: bool,
)
```

Run: `rg -n "pub\(crate\) fn update_bottom_clip_public" src/input/scroll.rs`
Expected: one match. (If the parameter is named differently, adjust the call —
it already exists with this shape per the codebase.)

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | grep -E "^error|Finished" | tail -5`
Expected: `Finished ...` with no `error` lines. (`right` may warn unused if the
compiler can't see `right_end` usage — it is used; no warning expected.)

- [ ] **Step 4: Commit**

```bash
git add src/input/scroll_mode.rs
git commit -m "scroll-mode: sync_right_to_left + value-changed connector"
```

---

## Task 4: Wire the value-changed handler at startup

**Files:**
- Modify: `src/app.rs` (after the `state` Rc and other signal connections, near line 1656)

- [ ] **Step 1: Connect the handler**

In `src/app.rs`, find the block that ends the echo-line entry connection
(around line 1656):

```rust
    let state_for_echo_line = Rc::clone(&state);
    {
        let s = state.borrow();
        s.echo_line_picker.entry().connect_changed(move |_| {
            crate::input::actions::echoes::refresh_add_echo_search(&state_for_echo_line);
        });
    }
```

Add immediately after it:

```rust
    // Continuous translation scroll: re-point the right column whenever the
    // left column scrolls. No-ops unless translation_scroll_active.
    crate::input::scroll_mode::connect_scroll_sync(&state);
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | grep -E "^error|Finished" | tail -3`
Expected: `Finished ...`.

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "scroll-mode: connect left-vadjustment scroll-sync at startup"
```

---

## Task 5: Enter scroll mode on show, restore on hide

Replaces the interim partial-fill patch in `hide_translations` with the
spec'd exit (restore pre-toggle page + re-tile). Captures the pre-toggle page
in `show_translations` and enters scroll mode after inserts.

**Files:**
- Modify: `src/app.rs` (`show_translations`, `hide_translations`)

- [ ] **Step 1: Capture pre-toggle page at the top of `show_translations`**

In `src/app.rs`, find the start of `show_translations` after the
`current_work` guard (the function begins around line 3209). Immediately after
the `let work = match &state.current_work { ... };` block and before
`state.card_vbox.set_opacity(0.0);`, add:

```rust
    // Save the pre-toggle reader position so hide can restore it exactly.
    state.pre_translation_page = Some((state.current_line, state.page_top_line));
```

- [ ] **Step 2: Enter scroll mode after signs are hidden**

Still in `show_translations`, find the block added earlier:

```rust
    state.sign_column_visible.set(false);
    crate::input::timestamps::redraw_sign_gutters(state);

    reapply_font(state);
    crate::input::navigation::invalidate_page_tops(state);
```

Insert between the `redraw_sign_gutters(state);` line and `reapply_font(state);`:

```rust
    // Two-column: switch to continuous lockstep scroll so every line and its
    // translation are readable without clipping.
    if state.column_count() == 2 {
        state.translation_scroll_active = true;
        // Left column fills to the viewport bottom in scroll mode.
        state.bottom_clip.set_height_request(0);
    }
```

- [ ] **Step 3: Run the initial sync after layout settles**

Still in `show_translations`, find the existing idle callback that ends with
`vbox.set_opacity(1.0);` (around line 3397). It clones widgets for anchoring.
We need the initial right-column sync to run once after GTK lays out. Find the
`crate::input::navigation::refresh_bottom_clip(state);` line near the end of
`show_translations` (around line 3400) and add immediately after it:

```rust
    // Initial right-column sync for scroll mode (deferred so GTK has laid out).
    if state.translation_scroll_active {
        let state_weak = std::rc::Rc::downgrade(&state.window.clone()) ;
        let _ = state_weak; // window weak only to avoid keeping state alive here
    }
```

NOTE: The above placeholder cannot reach `state` from an idle closure because
`show_translations` takes `&mut AppState`, not the `Rc`. Instead, perform the
initial sync synchronously at the end of `show_translations` AND rely on the
`value-changed` handler firing when the left adjustment settles. Replace the
block you just added with a synchronous call:

```rust
    // Initial right-column sync for scroll mode.
    if state.translation_scroll_active {
        crate::input::scroll_mode::sync_right_to_left(state);
    }
```

(If the views report height 0 at this point, `sync_right_to_left` no-ops; the
left adjustment's later `value-changed` — fired as content lays out — runs it
again. The handler is idempotent.)

- [ ] **Step 4: Rewrite the two-column branch of `hide_translations`**

In `src/app.rs`, find the two-column branch added earlier in
`hide_translations` (around line 3488):

```rust
    if state.column_count() == 2 {
        // Two-column e-reader mode: scroll-anchoring leaves the column split
        // stale, so the left column underfills. Snap to a clean page top and
        // re-tile both columns instead.
        state.page_top_line = state.current_line;
        crate::input::navigation::resnap_page(state);
        state.card_vbox.set_opacity(1.0);
        rebuild_line_number_gutter(state);
        return;
    }
```

Replace it with:

```rust
    if state.column_count() == 2 {
        // Leave continuous scroll mode and restore the exact pre-toggle page.
        state.translation_scroll_active = false;
        let (cur, top) = state.pre_translation_page.take()
            .unwrap_or((state.current_line, state.page_top_line));
        state.current_line = cur;
        state.page_top_line = top;
        crate::input::scroll::set_page_instant(state, top);
        crate::input::navigation::update_highlight_only(state);
        state.card_vbox.set_opacity(1.0);
        rebuild_line_number_gutter(state);
        return;
    }
```

- [ ] **Step 5: Clear scroll state in the single-column strip path too**

In `src/app.rs`, find `strip_translation_lines` where it sets
`state.translations_visible = false;` (around line 3560) and the sign-restore
block. Immediately after `state.translations_visible = false;`, add:

```rust
    // Always clear scroll-mode flags on strip (covers navigation-driven hide
    // and single-column paths). The two-column hide branch already restored
    // the page; here we only ensure the flags do not leak.
    state.translation_scroll_active = false;
    state.pre_translation_page = None;
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo build 2>&1 | grep -E "^error|Finished" | tail -5`
Expected: `Finished ...`.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs
git commit -m "scroll-mode: enter on show, restore pre-toggle page on hide"
```

---

## Task 6: Navigation key branches for scroll mode

Branch the reader keys into `scroll_mode` when active: j/k scroll by a line,
x scrolls by a viewport, q/comma move the cursor and scroll it into view.

**Files:**
- Modify: `src/input/scroll_mode.rs` (add key helpers)
- Modify: `src/input/navigation.rs` (early-branch in the public nav fns)

- [ ] **Step 1: Add scroll-by-line / scroll-by-page / cursor helpers**

Append to `src/input/scroll_mode.rs` (before `#[cfg(test)]`):

```rust
/// Scroll the left column by one line height in the given direction
/// (`+1` down, `-1` up). The value-changed handler re-points the right column.
pub(crate) fn scroll_by_line(state: &AppState, dir: i32) {
    let adj = state.scrolled_window.vadjustment();
    // Use the line at the current top as the step size.
    let scroll_v = adj.value() as i32;
    let line_count = state.buffer.line_count() as usize;
    let mut top_line = 0usize;
    let mut y = 0i32;
    while top_line + 1 < line_count {
        let Some(iter) = state.buffer.iter_at_line(top_line as i32) else { break };
        let (_y, h) = state.text_view.line_yrange(&iter);
        if y + h <= scroll_v { y += h; top_line += 1; } else { break; }
    }
    let step = state.buffer.iter_at_line(top_line as i32)
        .map(|it| state.text_view.line_yrange(&it).1)
        .unwrap_or(20)
        .max(1);
    let max_value = (adj.upper() - adj.page_size()).max(0.0);
    let target = ((adj.value() + (dir * step) as f64)).clamp(0.0, max_value);
    adj.set_value(target);
}

/// Scroll the left column by ~one viewport height (a "spread"), `dir` +1/-1.
pub(crate) fn scroll_by_page(state: &AppState, dir: i32) {
    let adj = state.scrolled_window.vadjustment();
    let max_value = (adj.upper() - adj.page_size()).max(0.0);
    let target = (adj.value() + (dir as f64) * adj.page_size()).clamp(0.0, max_value);
    adj.set_value(target);
}

/// After `state.current_line` has been moved by a dialogue jump, scroll the
/// left view so the cursor line is visible in the spread. If it is already
/// within the left column's current visible range, repaint only.
pub(crate) fn ensure_cursor_visible(state: &AppState) {
    let line = state.current_line;
    if let Some(iter) = state.buffer.iter_at_line(line as i32) {
        let (y, h) = state.text_view.line_yrange(&iter);
        let adj = state.scrolled_window.vadjustment();
        let top = adj.value();
        let bottom = top + adj.page_size();
        if (y as f64) < top || ((y + h) as f64) > bottom {
            let max_value = (adj.upper() - adj.page_size()).max(0.0);
            adj.set_value((y as f64).min(max_value));
        }
    }
}
```

- [ ] **Step 2: Verify the helpers compile (no behavior change yet)**

Run: `cargo build 2>&1 | grep -E "^error|Finished" | tail -3`
Expected: `Finished ...` (the helpers are unused → expect `dead_code` warnings;
those clear in Step 3).

- [ ] **Step 3: Branch `page_forward` (x) into scroll-by-page**

In `src/input/navigation.rs`, at the very top of `page_forward` (after the
`current_work.is_none()` guard, before the lock check), add:

```rust
    if state.translation_scroll_active {
        crate::input::scroll_mode::scroll_by_page(state, 1);
        return;
    }
```

- [ ] **Step 4: Branch `page_backward` into scroll-by-page**

In `src/input/navigation.rs`, at the top of `page_backward` (after the
`current_work.is_none()` guard), add:

```rust
    if state.translation_scroll_active {
        crate::input::scroll_mode::scroll_by_page(state, -1);
        return;
    }
```

- [ ] **Step 5: Branch `cursor_prev_line` (k) into scroll-up**

In `src/input/navigation.rs`, find `pub fn cursor_prev_line` (the `k` handler).
At the top of the function body, add:

```rust
    if state.translation_scroll_active {
        crate::input::scroll_mode::scroll_by_line(state, -1);
        return;
    }
```

Run: `rg -n "pub fn cursor_prev_line" src/input/navigation.rs` to confirm the
function exists with that name. If named differently (e.g. `cursor_up`), use
that name; the `k` action is `CursorPrevLine` per keymap_config.rs.

- [ ] **Step 6: Branch dialogue jumps (j / q / comma) to move cursor + ensure visible**

In `src/input/navigation.rs`, at the top of each of `cursor_next_dialogue`,
`jump_to_next_dialogue`, and `jump_to_prev_dialogue`, add (after the
`current_work.is_none()` guard if present):

```rust
    if state.translation_scroll_active {
        // Move the cursor as usual, then scroll it into view instead of
        // paginating.
        // (Fall through to the normal cursor-move below, but replace the
        // page/scroll step with ensure_cursor_visible — see Step 7.)
    }
```

NOTE: These functions intermix cursor movement with pagination. Rather than
duplicate their cursor logic, do Step 7 instead of the placeholder above.

- [ ] **Step 7: Replace the placeholder with a clean branch in dialogue jumps**

For each of `cursor_next_dialogue`, `jump_to_next_dialogue`,
`jump_to_prev_dialogue`: locate the `match state.config.navigation_mode { ... }`
block that performs the scroll/page (the same pattern seen at
navigation.rs:606, 683, 803, etc.). Wrap that match so scroll mode takes a
different path. Concretely, replace:

```rust
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
            crate::config::NavigationMode::EReader => {
                // ... existing e-reader page logic ...
            }
        }
```

with:

```rust
        if state.translation_scroll_active {
            crate::input::scroll_mode::ensure_cursor_visible(state);
        } else {
            match state.config.navigation_mode {
                crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
                crate::config::NavigationMode::EReader => {
                    // ... existing e-reader page logic (unchanged) ...
                }
            }
        }
```

Apply this wrapping to each dialogue-jump function's nav-mode match. Do not
delete the placeholder from Step 6 — replace it entirely with this structure
(remove the empty `if state.translation_scroll_active { }` stub).

- [ ] **Step 8: Verify it compiles**

Run: `cargo build 2>&1 | grep -E "^error|Finished" | tail -5`
Expected: `Finished ...` with no `error` lines and no `dead_code` warnings for
the scroll_mode helpers.

- [ ] **Step 9: Run the full test suite**

Run: `cargo test 2>&1 | grep -E "test result|FAILED" | tail -8`
Expected: all pass EXCEPT the two known-unrelated failures
`app::card_width_tests::two_columns_fill_fraction_of_wide_window` and
`...two_columns_never_below_single_column_floor` (pre-existing). The
`scroll_mode::tests` (5) must pass.

- [ ] **Step 10: Commit**

```bash
git add src/input/scroll_mode.rs src/input/navigation.rs
git commit -m "scroll-mode: branch j/k/x/q/comma navigation while translations scroll"
```

---

## Task 7: Manual verification

**Files:** none (verification only).

- [ ] **Step 1: Build**

Run: `cargo build 2>&1 | grep -E "^error|Finished" | tail -3`
Expected: `Finished ...`.

- [ ] **Step 2: User runs the app and verifies**

(Per project convention, the USER runs `cargo run` — do not run it as the
agent.) Ask the user to verify, on a two-column play with translations:

1. Toggle translations ON → no clipping; signs hidden.
2. Scroll with the wheel and with j/k → the right column always continues from
   `(left column last visible line) + 1`; the spread reads contiguously with no
   gap, overlap, or clipped top/bottom line.
3. q / comma → cursor jumps to next/prev dialogue and scrolls into view.
4. x → scrolls about one viewport.
5. Toggle translations OFF → returns to the exact spread shown before the
   toggle, both columns fully filled, signs restored.

- [ ] **Step 3: Update CLAUDE.md note (optional, only if behavior is user-facing enough to document)**

If desired, add a one-line note under the project's two-column / MPV section in
`/home/mlj/utono/linux-lit/CLAUDE.md` describing that translations use
continuous lockstep scroll. Skip if the user prefers not to.

- [ ] **Step 4: Final commit (if CLAUDE.md changed)**

```bash
git add CLAUDE.md
git commit -m "docs: note continuous lockstep scroll in translation mode"
```

---

## Self-Review Notes

- **Spec coverage:** Section 1 (state/entry/exit) → Tasks 1, 5. Section 2
  (scroll-sync callback) → Tasks 2, 3, 4. Section 3 (key handling) → Task 6.
  Section 4 (edge cases/testing) → Task 2 unit tests + Task 7 manual + the
  height≤0/clamp guards inside `sync_right_to_left`.
- **Re-entrancy guard:** `right_scroll_syncing` set/cleared around the right
  adjustment write in Task 3; checked in the handler closure in Task 3 Step 1.
- **Interim patch removal:** Task 5 Step 4 explicitly replaces the earlier
  partial-fill-on-hide patch with the spec'd restore.
- **Known caveat (documented, not a blocker):** `sync_right_to_left` scans line
  heights from line 0 each tick (O(n)). The spec called for a cached
  `(v → lt)` hint to keep it O(screen). This plan ships the correct O(n)
  version first (simpler, verifiably correct); if scroll feels laggy on large
  works during Task 7, add the cache as a follow-up (store last `(scroll_v,
  top_line, y)` on AppState and resume the scan from the hint). Flagged here so
  the omission is explicit, per "no silent caps."
```
