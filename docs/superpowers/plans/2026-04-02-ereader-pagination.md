# E-Reader Pagination Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace pixel-based page turn checks with deterministic line-count thresholds for user navigation, keep pixel-based checks for audio sync, and optimize `update_highlight` to only dim visible lines.

**Architecture:** Three changes to `src/input/navigation.rs`: (1) `scroll_after_jump_forward` and `scroll_after_jump_backward` use `page_top_line + lines_per_page` arithmetic instead of `is_line_fully_visible`/`is_line_on_screen` pixel checks, (2) `update_highlight` restricts dim tag operations to a range around the visible page, (3) dead code cleanup.

**Tech Stack:** Rust, GTK4, sourceview5

---

### Task 1: Replace forward page turn with line-count threshold

**Files:**
- Modify: `src/input/navigation.rs` — `scroll_after_jump_forward`, `ensure_cursor_on_page`

Currently `scroll_after_jump_forward` calls `ensure_cursor_on_page` which uses pixel-based `needs_page_turn_down`. Replace with a simple line-count check: page turn when `current_line >= page_top_line + lines_per_page - 2`.

- [ ] **Step 1: Replace `scroll_after_jump_forward`**

Replace the current implementation:

```rust
/// Mode-aware scroll after a forward jump (`q` / next paragraph or dialogue).
/// In e-reader mode, page-turns when cursor reaches the last two lines of the page.
fn scroll_after_jump_forward(state: &mut AppState) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            let lpp = lines_per_page(state);
            let threshold = state.page_top_line + lpp.saturating_sub(2);
            if state.current_line >= threshold {
                set_page(state, state.current_line);
            }
        }
    }
}
```

- [ ] **Step 2: Update `ensure_cursor_on_page` to use line-count for forward case**

Replace the current `ensure_cursor_on_page` which uses pixel-based `needs_page_turn_down`:

```rust
fn ensure_cursor_on_page(state: &mut AppState) {
    let lpp = lines_per_page(state);
    let page_end = state.page_top_line + lpp.saturating_sub(2);

    crate::logging::log(&format!(
        "ENSURE: cursor={} page_top={} lpp={} page_end={}",
        state.current_line, state.page_top_line, lpp, page_end
    ));

    if state.current_line < state.page_top_line {
        // Went above — new page with cursor near bottom
        let new_top = state.current_line.saturating_sub(lpp.saturating_sub(1));
        set_page(state, new_top);
    } else if state.current_line >= page_end {
        // At or past threshold — new page with this line at top
        set_page(state, state.current_line);
    }
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo build`
Expected: Compiles with warnings only (no errors)

- [ ] **Step 4: Commit**

```bash
git add src/input/navigation.rs
git commit -m "Replace pixel-based forward page turn with line-count threshold"
```

---

### Task 2: Replace backward page turn with line-count threshold

**Files:**
- Modify: `src/input/navigation.rs` — `scroll_after_jump_backward`

Currently uses `is_line_on_screen` to check if the line above is off-screen. Replace with line-count: page turn when `current_line <= page_top_line`.

- [ ] **Step 1: Replace `scroll_after_jump_backward`**

```rust
/// Mode-aware scroll after a backward jump (`,` / prev paragraph or dialogue).
/// In e-reader mode, page-turns when cursor reaches the top line of the page.
fn scroll_after_jump_backward(state: &mut AppState) {
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if state.current_line <= state.page_top_line {
                let lpp = lines_per_page(state);
                let new_top = state.current_line.saturating_sub(lpp.saturating_sub(1));
                set_page(state, new_top);
            }
        }
    }
}
```

- [ ] **Step 2: Update `move_cursor` for e-reader mode**

Currently `move_cursor` calls `scroll_to_cursor` (which calls `center_cursor`). In e-reader mode, it should use the same line-count thresholds:

```rust
pub fn move_cursor(state: &mut AppState, delta: i32) {
    // ... existing translation-skip logic unchanged ...

    if new_line == state.current_line {
        return;
    }

    state.current_line = new_line;
    update_highlight(state);

    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => scroll_to_cursor(state),
        crate::config::NavigationMode::EReader => {
            let lpp = lines_per_page(state);
            let threshold = state.page_top_line + lpp.saturating_sub(2);
            if state.current_line >= threshold {
                set_page(state, state.current_line);
            } else if state.current_line <= state.page_top_line {
                let new_top = state.current_line.saturating_sub(lpp.saturating_sub(1));
                set_page(state, new_top);
            }
        }
    }

    auto_show_vocab_popup(state);
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo build`
Expected: Compiles with warnings only

- [ ] **Step 4: Commit**

```bash
git add src/input/navigation.rs
git commit -m "Replace pixel-based backward page turn and move_cursor with line-count thresholds"
```

---

### Task 3: Optimize `update_highlight` to visible range only

**Files:**
- Modify: `src/input/navigation.rs` — `update_highlight`

Currently applies dim tags to the entire 36K-line buffer on every cursor movement (~786ms). Change to only apply/remove dim tags in a range around the visible page.

- [ ] **Step 1: Replace `update_highlight` with visible-range version**

```rust
fn update_highlight(state: &AppState) {
    let buffer = &state.buffer;
    let tag = &state.dim_tag;
    let cl_tag = &state.cursor_line_tag;

    // Compute visible range with margin for scroll overshoot
    let lpp = lines_per_page(state);
    let margin = 5;
    let vis_start = state.page_top_line.saturating_sub(margin);
    let vis_end = (state.page_top_line + lpp + margin)
        .min(state.effective_line_count());

    // Get iters for visible range
    let vis_start_iter = buffer.iter_at_line(vis_start as i32)
        .unwrap_or_else(|| buffer.start_iter());
    let vis_end_iter = buffer.iter_at_line(vis_end as i32)
        .unwrap_or_else(|| buffer.end_iter());

    // Clear cursor line tag in visible range only
    buffer.remove_tag(cl_tag, &vis_start_iter, &vis_end_iter);

    if !state.dim_enabled {
        // Remove dimming in visible range
        buffer.remove_tag(tag, &vis_start_iter, &vis_end_iter);
        // Apply cursor line background
        if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
            let mut line_end = line_start;
            if !line_end.ends_line() {
                line_end.forward_to_line_end();
            }
            buffer.apply_tag(cl_tag, &line_start, &line_end);
        }
        return;
    }

    // Dim visible range
    buffer.apply_tag(tag, &vis_start_iter, &vis_end_iter);

    // Undim the current line
    if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
        let mut line_end = line_start;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        buffer.remove_tag(tag, &line_start, &line_end);
    }

    // When a chunk is active, undim all lines within the chunk range
    // (only the portion that overlaps with visible range)
    if state.ab_repeat.chunk_index.is_some() {
        if let (Some(a), Some(b)) = (state.ab_repeat.a_line, state.ab_repeat.b_line) {
            let chunk_start = a.max(vis_start);
            let chunk_end = b.min(vis_end.saturating_sub(1));
            for line_idx in chunk_start..=chunk_end {
                if let Some(line_start) = buffer.iter_at_line(line_idx as i32) {
                    let mut line_end = line_start;
                    if !line_end.ends_line() {
                        line_end.forward_to_line_end();
                    }
                    buffer.remove_tag(tag, &line_start, &line_end);
                }
            }
        }
    }

    // When visual selection is active, clear stale highlight then re-apply
    crate::input::visual::clear_selection_highlight(state);
    crate::input::visual::apply_selection_highlight(state);
}
```

- [ ] **Step 2: Add full-buffer dim clear for page turns**

When a page turn happens, the old visible range still has dim tags applied. The new visible range needs fresh tags. Add a helper called from `set_page`:

```rust
/// Clear dim tags from the old visible range before a page turn.
/// Called before page_top_line is updated.
fn clear_old_page_dim(state: &AppState) {
    if !state.dim_enabled {
        return;
    }
    let buffer = &state.buffer;
    let tag = &state.dim_tag;
    let lpp = lines_per_page(state);
    let margin = 5;
    let old_start = state.page_top_line.saturating_sub(margin);
    let old_end = (state.page_top_line + lpp + margin)
        .min(state.effective_line_count());
    let start_iter = buffer.iter_at_line(old_start as i32)
        .unwrap_or_else(|| buffer.start_iter());
    let end_iter = buffer.iter_at_line(old_end as i32)
        .unwrap_or_else(|| buffer.end_iter());
    buffer.remove_tag(tag, &start_iter, &end_iter);
}
```

Update `set_page` to call it:

```rust
fn set_page(state: &mut AppState, new_top: usize) {
    clear_old_page_dim(state);
    state.page_top_line = new_top;
    let scroll_line = new_top.saturating_sub(2);
    if let Some(iter) = state.buffer.iter_at_line(scroll_line as i32) {
        let mut end = iter;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        state.text_view.scroll_to_iter(&mut end, 0.0, true, 0.0, 0.0);
    }
}
```

Do the same for `set_page_instant`.

- [ ] **Step 3: Handle `Alt+d` dim toggle — clear full buffer once**

In `src/input/keymap.rs`, the `Alt+d` handler calls `update_highlight_only`. When toggling dim OFF, we need to clear dim tags from the entire buffer once (since only the visible range was managed). Add this to the keymap handler:

In the existing `Alt+d` match arm in `keymap.rs` (around line 808), after `s.dim_enabled = !s.dim_enabled`, add:

```rust
if !s.dim_enabled {
    // Clear dim from entire buffer since only visible range was managed
    let (start, end) = s.buffer.bounds();
    s.buffer.remove_tag(&s.dim_tag, &start, &end);
}
```

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: Compiles with warnings only

- [ ] **Step 5: Commit**

```bash
git add src/input/navigation.rs src/input/keymap.rs
git commit -m "Optimize update_highlight to only dim visible range instead of full buffer"
```

---

### Task 4: Clean up dead code

**Files:**
- Modify: `src/input/navigation.rs`

Remove functions that are no longer called after the refactoring.

- [ ] **Step 1: Remove unused functions**

Remove these functions:
- `crossfade_to` — was the old page-turn scroll setter, replaced by `scroll_to_iter`
- `is_line_fully_visible` — was the pixel-based visibility check with padding, replaced by line-count threshold
- `needs_page_turn_down` — was the pixel-based page turn check, replaced by line-count threshold
- `ensure_visible_no_highlight` — unused caller of pixel-based checks

Keep these (still used):
- `is_line_on_screen` — used by audio sync (`should_page_turn_forward`, `update_highlight_and_ensure_visible`, `scroll_paragraph_to_top`)
- `should_page_turn_forward` — used by audio sync
- `scroll_value_for_line` — used by `center_cursor` (scroll mode)

- [ ] **Step 2: Remove stale doc comment on `set_page`**

The `set_page` function has a duplicate doc comment line. Clean it up:

```rust
/// Set the page top line and scroll so the line is fully visible with
/// one line of padding above it. Scrolls to the END of the line two
/// before `new_top` at yalign=0.0, so one full line acts as top padding.
fn set_page(state: &mut AppState, new_top: usize) {
```

- [ ] **Step 3: Build and verify no warnings for removed code**

Run: `cargo build 2>&1 | grep "dead_code"`
Expected: No dead_code warnings for the removed functions. Other existing dead_code warnings (in app.rs, text_file_map.rs, etc.) are fine.

- [ ] **Step 4: Commit**

```bash
git add src/input/navigation.rs
git commit -m "Remove dead code: crossfade_to, is_line_fully_visible, needs_page_turn_down, ensure_visible_no_highlight"
```

---

### Task 5: Verify all navigation paths work correctly

**Files:**
- No changes — manual testing only

- [ ] **Step 1: Test `q` forward navigation**

Run `cargo run`, load a work. Press `q` repeatedly:
- Cursor should move down through dialogue lines within the page
- When cursor reaches the second-to-last line, next `q` should page-turn
- New page should show cursor at top with one overlap line above

- [ ] **Step 2: Test `,` backward navigation**

Press `,` repeatedly:
- Cursor should move up through dialogue lines within the page
- When cursor reaches the top line (`page_top_line`), next `,` should page-turn backward
- New page should show cursor near the bottom

- [ ] **Step 3: Test `j`/`k` line movement**

Press `j`/`k`:
- Same page turn behavior as `q`/`,` but for single-line movement
- Page turns at the same thresholds

- [ ] **Step 4: Test `Ctrl+d`/`Ctrl+f` page navigation**

- `Ctrl+d`: advance by half page, page turns
- `Ctrl+f`: advance by full page, always page turns

- [ ] **Step 5: Test audio sync**

Press `Tab` to start playback:
- Cursor should follow audio within the page without page turns
- Page should turn only when cursor reaches the last/second-to-last visible line
- No premature page turns mid-page

- [ ] **Step 6: Test `Alt+d` dim toggle**

- Toggle dim off: all lines should show at full color immediately (no dim remnants from off-screen lines)
- Toggle dim on: only visible lines should dim, cursor line should be undimmed

- [ ] **Step 7: Final commit**

```bash
git add -A
git commit -m "E-reader pagination: deterministic line-count page turns and visible-range dim optimization"
```
