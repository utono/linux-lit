# linux-lit Phase 3: Navigation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Vim-style cursor movement through loaded text with cursor line highlight, including j/k line movement, gg/G jump to start/end, Ctrl+d/u half-page scroll, Ctrl+f/b full-page scroll, and comma/q dialogue jumping.

**Architecture:** Navigation logic lives in a dedicated `input/` module. `AppState` gains `current_line: usize`. A `GtkTextTag` highlights the current line. The key handler in `app.rs` is extracted into `input/keymap.rs` which routes events to navigation functions. The `gg` two-key sequence uses a simple pending-key state with timeout.

**Tech Stack:** gtk4-rs (TextTag, TextIter, scroll_to_iter), glib (timeout_add_local_once)

**Depends on:** Phase 2 (complete) — database loading, work display, library picker

---

## File Structure

```
~/utono/linux-lit/src/
  input/
    mod.rs              # Re-exports
    keymap.rs           # Key event routing, gg state machine, delegates to navigation
    navigation.rs       # Cursor movement functions (j/k, gg/G, page scroll, dialogue jump)
  app.rs                # Modified: add current_line + highlight_tag to AppState, extract key handler
```

## Key Design Decisions

- **`current_line: usize`** tracks position as an index into the work's `lines` vec (0-based). This is the single source of truth for cursor position.
- **Cursor highlight** uses a named `GtkTextTag` ("current-line") created once and reused. On each move: remove tag from old range, apply to new range.
- **`scroll_to_iter` with `within_margin: 0.4`** keeps the cursor centered-ish (40% from edges).
- **`gg` detection:** first `g` sets a pending state + 500ms timeout. Second `g` within timeout triggers jump-to-start. Timeout or any other key cancels the pending state.
- **Dialogue jumping (`,`/`q`):** scans the `is_dialogue` field on loaded lines. After jumping, no MPV seek yet (that's Phase 5).

---

### Task 1: Create Navigation Module with Cursor Movement

**Files:**
- Create: `src/input/mod.rs`
- Create: `src/input/navigation.rs`
- Modify: `src/app.rs` — add `current_line`, `scrolled_window` to AppState
- Modify: `src/main.rs` — add `mod input;`

This task implements the core cursor movement functions and the line highlight. No key binding yet — that's Task 2.

- [ ] **Step 1: Add `current_line`, `scrolled_window`, and `highlight_tag` to AppState**

In `src/app.rs`, update `AppState`:

```rust
#[allow(dead_code)]
pub struct AppState {
    pub text_view: TextView,
    pub buffer: TextBuffer,
    pub picker: LibraryPicker,
    pub current_work: Option<Work>,
    pub current_line: usize,
    pub highlight_tag: gtk4::TextTag,
    pub scrolled_window: ScrolledWindow,
    pub window: ApplicationWindow,
}
```

Create the highlight tag after creating the buffer:

```rust
    let highlight_tag = gtk4::TextTag::builder()
        .name("current-line")
        .background("rgba(100, 140, 200, 0.3)")
        .build();
    buffer.tag_table().add(&highlight_tag);
```

Update the `AppState` initialization to include `current_line: 0`, `highlight_tag`, and `scrolled_window: scrolled.clone()`.

Update `display_work` to reset `current_line` to 0 and apply the initial highlight.

- [ ] **Step 2: Create `src/input/mod.rs`**

```rust
pub mod keymap;
pub mod navigation;
```

- [ ] **Step 3: Create `src/input/navigation.rs`**

```rust
use gtk4::prelude::*;
use crate::app::AppState;

/// Move cursor by `delta` lines (positive = down, negative = up).
pub fn move_cursor(state: &mut AppState, delta: i32) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };
    let line_count = work.lines.len();
    if line_count == 0 {
        return;
    }

    let new_line = (state.current_line as i32 + delta)
        .max(0)
        .min(line_count as i32 - 1) as usize;

    if new_line != state.current_line {
        state.current_line = new_line;
        update_highlight(state);
        scroll_to_current_line(state);
    }
}

/// Jump to the first line.
pub fn jump_to_start(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    state.current_line = 0;
    update_highlight(state);
    scroll_to_current_line(state);
}

/// Jump to the last line.
pub fn jump_to_end(state: &mut AppState) {
    let line_count = match &state.current_work {
        Some(w) => w.lines.len(),
        None => return,
    };
    if line_count == 0 {
        return;
    }
    state.current_line = line_count - 1;
    update_highlight(state);
    scroll_to_current_line(state);
}

/// Scroll by half a page (positive = down, negative = up).
pub fn scroll_half_page(state: &mut AppState, direction: i32) {
    // Approximate half-page as 15 lines
    move_cursor(state, direction * 15);
}

/// Scroll by a full page (positive = down, negative = up).
pub fn scroll_full_page(state: &mut AppState, direction: i32) {
    // Approximate full page as 30 lines
    move_cursor(state, direction * 30);
}

/// Jump to the previous dialogue line (`,` key).
pub fn jump_to_prev_dialogue(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };
    if state.current_line == 0 {
        return;
    }

    for i in (0..state.current_line).rev() {
        if work.lines[i].is_dialogue {
            state.current_line = i;
            update_highlight(state);
            scroll_to_current_line(state);
            return;
        }
    }
}

/// Jump to the next dialogue line (`q` key).
pub fn jump_to_next_dialogue(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };
    let line_count = work.lines.len();

    for i in (state.current_line + 1)..line_count {
        if work.lines[i].is_dialogue {
            state.current_line = i;
            update_highlight(state);
            scroll_to_current_line(state);
            return;
        }
    }
}

/// Remove highlight from old line, apply to new current line.
fn update_highlight(state: &AppState) {
    let buffer = &state.buffer;
    let tag = &state.highlight_tag;

    // Remove tag from entire buffer
    let (start, end) = buffer.bounds();
    buffer.remove_tag(tag, &start, &end);

    // Apply tag to current line
    let line_start = buffer.iter_at_line(state.current_line as i32);
    if let Some(mut iter) = line_start {
        let mut line_end = iter.clone();
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        buffer.apply_tag(tag, &iter, &line_end);
    }
}

/// Scroll the text view to keep the current line visible.
fn scroll_to_current_line(state: &AppState) {
    if let Some(iter) = state.buffer.iter_at_line(state.current_line as i32) {
        state.text_view.scroll_to_iter(
            &mut iter.clone(),
            0.0,    // within_margin (use mark-based scrolling margin instead)
            true,   // use_align
            0.0,    // xalign
            0.4,    // yalign — 40% from top
        );
    }
}
```

- [ ] **Step 4: Add `mod input;` to `src/main.rs`**

Add `mod input;` after `mod db;`.

- [ ] **Step 5: Verify compilation**

Run: `cd ~/utono/linux-lit && cargo check 2>&1 | tail -5`
Expected: `Finished`

- [ ] **Step 6: Commit**

```bash
git add src/input/ src/app.rs src/main.rs
git commit -m "feat: add navigation module with cursor movement and line highlight"
```

---

### Task 2: Extract Key Handler into Keymap Module

**Files:**
- Create: `src/input/keymap.rs`
- Modify: `src/app.rs` — replace inline key handler with call to keymap

Extract the key handling from `app.rs` into `input/keymap.rs`. Add the `gg` two-key state machine and route all navigation keys.

- [ ] **Step 1: Create `src/input/keymap.rs`**

```rust
use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;
use crate::input::navigation;

/// Pending key state for multi-key sequences (e.g., gg).
#[derive(Default)]
pub struct KeyState {
    pub pending_g: bool,
}

/// Handle a key press event. Returns `true` if the key was consumed.
pub fn handle_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    is_ctrl: bool,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    let picker_visible = state.borrow().picker.is_visible();

    // --- Picker-visible keys ---

    // Ctrl+n/Ctrl+p navigate picker list
    if picker_visible && is_ctrl {
        match key_name {
            "n" => {
                state.borrow().picker.move_selection(1);
                return true;
            }
            "p" => {
                state.borrow().picker.move_selection(-1);
                return true;
            }
            _ => {}
        }
    }

    // Ctrl+p: open library picker (only when not visible)
    if is_ctrl && key_name == "p" && !picker_visible {
        state.borrow().picker.show();
        return true;
    }

    // Picker-specific keys
    if picker_visible {
        match key_name {
            "Escape" => {
                state.borrow().picker.hide();
                return true;
            }
            "Return" => {
                let abbrev = state.borrow().picker.selected_abbrev();
                if let Some(abbrev) = abbrev {
                    let state_clone = Rc::clone(state);
                    let handle = tokio_handle.clone();
                    glib::spawn_future_local(async move {
                        let work = handle
                            .spawn_blocking(move || {
                                let conn =
                                    crate::db::queries::open_db().expect("Failed to open lit.db");
                                crate::db::queries::load_work(&conn, &abbrev)
                            })
                            .await;
                        match work {
                            Ok(Ok(work)) => {
                                let mut s = state_clone.borrow_mut();
                                s.picker.hide();
                                crate::app::display_work(&mut s, work);
                            }
                            Ok(Err(e)) => eprintln!("Failed to load work: {}", e),
                            Err(e) => eprintln!("Task join error: {}", e),
                        }
                    });
                }
                return true;
            }
            "Down" => {
                state.borrow().picker.move_selection(1);
                return true;
            }
            "Up" => {
                state.borrow().picker.move_selection(-1);
                return true;
            }
            "j" => {
                if !state.borrow().picker.search_entry().has_focus() {
                    state.borrow().picker.move_selection(1);
                    return true;
                }
            }
            "k" => {
                if !state.borrow().picker.search_entry().has_focus() {
                    state.borrow().picker.move_selection(-1);
                    return true;
                }
            }
            _ => {}
        }
        // Let other keys pass through to the search entry
        return false;
    }

    // --- Normal mode keys (no picker) ---

    // Check for pending g (gg sequence)
    if key_state.borrow().pending_g {
        key_state.borrow_mut().pending_g = false;
        if key_name == "g" {
            navigation::jump_to_start(&mut state.borrow_mut());
            return true;
        }
        // Not g — cancel pending state, fall through
    }

    // Ctrl combos
    if is_ctrl {
        match key_name {
            "d" => {
                navigation::scroll_half_page(&mut state.borrow_mut(), 1);
                return true;
            }
            "u" => {
                navigation::scroll_half_page(&mut state.borrow_mut(), -1);
                return true;
            }
            "f" => {
                navigation::scroll_full_page(&mut state.borrow_mut(), 1);
                return true;
            }
            "b" => {
                navigation::scroll_full_page(&mut state.borrow_mut(), -1);
                return true;
            }
            _ => return false,
        }
    }

    // Single keys
    match key_name {
        "j" => {
            navigation::move_cursor(&mut state.borrow_mut(), 1);
            true
        }
        "k" => {
            navigation::move_cursor(&mut state.borrow_mut(), -1);
            true
        }
        "g" => {
            key_state.borrow_mut().pending_g = true;
            // Set timeout to cancel pending_g after 500ms
            let ks = Rc::clone(key_state);
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                ks.borrow_mut().pending_g = false;
            });
            true
        }
        "G" => {
            navigation::jump_to_end(&mut state.borrow_mut());
            true
        }
        "comma" => {
            navigation::jump_to_prev_dialogue(&mut state.borrow_mut());
            true
        }
        "q" => {
            navigation::jump_to_next_dialogue(&mut state.borrow_mut());
            true
        }
        _ => false,
    }
}
```

- [ ] **Step 2: Refactor `app.rs` key controller to use keymap**

Replace the entire `key_controller.connect_key_pressed(...)` closure with:

```rust
    let state_for_keys = Rc::clone(&state);
    let key_state = Rc::new(RefCell::new(crate::input::keymap::KeyState::default()));
    let key_controller = EventControllerKey::new();
    key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
    key_controller.connect_key_pressed(move |_controller, keyval, _keycode, modifier| {
        let key_name = keyval.name().unwrap_or_default();
        let is_ctrl = modifier.contains(gtk4::gdk::ModifierType::CONTROL_MASK);

        let consumed = crate::input::keymap::handle_key(
            &state_for_keys,
            &key_state,
            &key_name,
            is_ctrl,
            &tokio_handle,
        );

        if consumed {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(key_controller);
```

Remove all the inline key handling code that was there before (lines 112-197 in current app.rs).

- [ ] **Step 3: Update `display_work` to reset cursor and apply initial highlight**

At the end of `display_work()`:

```rust
    state.current_line = 0;
    // Apply initial highlight
    let tag = &state.highlight_tag;
    if let Some(mut line_end) = state.buffer.iter_at_line(0) {
        let line_start = line_end.clone();
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        state.buffer.apply_tag(tag, &line_start, &line_end);
    }
```

- [ ] **Step 4: Verify compilation**

Run: `cd ~/utono/linux-lit && cargo check 2>&1 | tail -5`
Expected: `Finished`

- [ ] **Step 5: Run all tests**

Run: `cd ~/utono/linux-lit && cargo test 2>&1 | tail -10`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/input/keymap.rs src/app.rs
git commit -m "feat: extract key handler into keymap module with gg sequence and navigation routing"
```

---

### Task 3: Polish and Verify

**Files:**
- Modify: various (as needed for clippy/fmt fixes)

- [ ] **Step 1: Run `cargo clippy`**

Run: `cd ~/utono/linux-lit && cargo clippy 2>&1 | grep "warning:" | grep -v "generated"`
Fix any warnings.

- [ ] **Step 2: Run `cargo fmt`**

Run: `cd ~/utono/linux-lit && cargo fmt`

- [ ] **Step 3: Run all tests**

Run: `cd ~/utono/linux-lit && cargo test 2>&1 | tail -10`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: fix clippy warnings and format code for Phase 3"
```

---

## Phase 3 Acceptance Criteria

After completing all tasks:

1. `j`/`k` moves cursor one line down/up with highlight following
2. `gg` jumps to first line, `G` jumps to last line
3. `Ctrl+d`/`Ctrl+u` scrolls half page down/up
4. `Ctrl+f`/`Ctrl+b` scrolls full page down/up
5. `,` jumps to previous dialogue line (skips speakers, stage directions, markers)
6. `q` jumps to next dialogue line
7. Cursor line has visible background highlight
8. Text view scrolls to keep cursor visible (yalign 0.4)
9. Library picker still works (Ctrl+p opens, Ctrl+n/p navigate, Enter selects, Escape dismisses)
10. `cargo clippy` — no warnings
11. `cargo test` — all tests pass

## Notes for Phase 4

- The highlight tag currently uses a hardcoded color (`rgba(100, 140, 200, 0.3)`). Phase 4 will replace this with the theme's `CursorLine.guibg` color.
- The `current_line` state will be saved/restored in Phase 8 (config.json persistence).
- Phase 5 will add MPV seek after `,`/`q` dialogue jumps.
