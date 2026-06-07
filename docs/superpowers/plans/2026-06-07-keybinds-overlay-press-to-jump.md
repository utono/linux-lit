# Keybinds Overlay Press-to-Jump Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user jump the highlight in the Ctrl+/ keybinds overlay straight to a key's cap by pressing that key (jump mode), with Tab toggling to the existing arrow/j-k/n-p navigation (nav mode).

**Architecture:** Add pure glyph-resolution free functions (`key_name_to_glyph`, `find_cap`) and a `jump_mode` Cell to `KeybindsOverlay` in `src/ui/keybinds_overlay.rs`; the Cairo draw reads the mode for a mode-aware header/footer. `handle_keybinds_key` in `src/input/keymap.rs` branches on the mode: in jump mode every non-nav key calls `jump_to_key`, in nav mode `n/p/j/k` keep their current meaning. Arrows and Esc work in both modes; the gamepad handoff is unchanged.

**Tech Stack:** Rust, GTK4 (`gtk4`/`cairo`), inline `#[cfg(test)]` unit tests (`cargo test --bins`).

---

## File Structure

- `src/ui/keybinds_overlay.rs` — Cairo overlay. Gains: `key_name_to_glyph`, `find_cap` (free fns, unit-tested), `jump_mode` field, `jump_to_key`/`toggle_mode`/`is_jump_mode` methods, mode-aware header + footer, and a `#[cfg(test)] mod tests`.
- `src/input/keymap.rs` — `handle_keybinds_key` restructured to consult the mode; the gamepad-handoff row-cycle logic factored into two local helpers to avoid duplication.

No new files; no new `InputMode`.

---

## Task 1: Glyph resolution free functions + unit tests

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (add two free functions near the row helpers, ~after `first_bound`, around line 145)
- Test: `src/ui/keybinds_overlay.rs` (new `#[cfg(test)] mod tests` at end of file)

- [ ] **Step 1: Write the failing tests**

Add at the very end of `src/ui/keybinds_overlay.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_for_symbol_names() {
        assert_eq!(key_name_to_glyph("slash"), Some("/"));
        assert_eq!(key_name_to_glyph("comma"), Some(","));
        assert_eq!(key_name_to_glyph("period"), Some("."));
        assert_eq!(key_name_to_glyph("parenleft"), Some("("));
        assert_eq!(key_name_to_glyph("plus"), Some("+"));
        assert_eq!(key_name_to_glyph("backslash"), Some("\\"));
        assert_eq!(key_name_to_glyph("apostrophe"), Some("'"));
    }

    #[test]
    fn glyph_returns_none_for_letters() {
        // Letters are matched by identity in find_cap, not via this table.
        assert_eq!(key_name_to_glyph("h"), None);
        assert_eq!(key_name_to_glyph("g"), None);
    }

    #[test]
    fn find_cap_resolves_representative_keys() {
        // 'h' is on the home row (index 2).
        let (row, idx) = find_cap("h").expect("h has a cap");
        assert_eq!(row, 2);
        assert_eq!(row_keys(row)[idx].unshifted, "h");

        // '/' (slash) is on the upper row (index 1).
        let (row, idx) = find_cap("slash").expect("slash has a cap");
        assert_eq!(row, 1);
        assert_eq!(row_keys(row)[idx].unshifted, "/");

        // '+' (plus) is on the number row (index 0).
        let (row, idx) = find_cap("plus").expect("plus has a cap");
        assert_eq!(row, 0);
        assert_eq!(row_keys(row)[idx].unshifted, "+");
    }

    #[test]
    fn find_cap_none_for_unmapped() {
        assert_eq!(find_cap("F5"), None);
        assert_eq!(find_cap("Return"), None);
    }

    #[test]
    fn every_lettered_cap_is_findable() {
        // Every cap with a single-char ASCII-letter glyph must resolve to
        // itself via identity matching.
        for row in 0..ROW_COUNT {
            for def in row_keys(row) {
                let g = def.unshifted;
                if g.len() == 1 && g.chars().all(|c| c.is_ascii_alphabetic()) {
                    let (r, i) = find_cap(g).unwrap_or_else(|| panic!("no cap for {g}"));
                    assert_eq!(row_keys(r)[i].unshifted, g);
                }
            }
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin linux-lit keybinds_overlay::tests -- --nocapture`
Expected: FAIL — `cannot find function key_name_to_glyph` / `find_cap` in this scope.

- [ ] **Step 3: Write the free functions**

Insert after `first_bound` (after line ~145, before the `describe` doc comment) in `src/ui/keybinds_overlay.rs`:

```rust
/// Map a GTK keyval name for a symbol key to the cap glyph used in the row
/// tables (`unshifted` field). Single-character letter/digit names are NOT in
/// this table — `find_cap` matches those by identity. Returns `None` for names
/// with no symbol cap.
fn key_name_to_glyph(key_name: &str) -> Option<&'static str> {
    Some(match key_name {
        "slash" => "/",
        "comma" => ",",
        "period" => ".",
        "parenleft" => "(",
        "parenright" => ")",
        "ampersand" => "&",
        "bracketleft" => "[",
        "bracketright" => "]",
        "braceleft" => "{",
        "braceright" => "}",
        "backslash" => "\\",
        "minus" => "-",
        "apostrophe" => "'",
        "plus" => "+",
        "asterisk" => "*",
        "exclam" => "!",
        "bar" => "|",
        "at" => "@",
        "dollar" => "$",
        "equal" => "=",
        _ => return None,
    })
}

/// Resolve an incoming GTK key name to the `(row_idx, cap_idx)` of the first cap
/// whose `unshifted` glyph matches. Symbol names go through
/// `key_name_to_glyph`; everything else (letters/digits) is matched by identity.
/// Returns `None` when no cap matches (the caller consumes the key as a no-op).
fn find_cap(key_name: &str) -> Option<(usize, usize)> {
    let glyph = key_name_to_glyph(key_name).unwrap_or(key_name);
    for row in 0..ROW_COUNT {
        let keys = row_keys(row);
        if let Some(idx) = keys.iter().position(|d| d.unshifted == glyph) {
            return Some((row, idx));
        }
    }
    None
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin linux-lit keybinds_overlay::tests -- --nocapture`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "feat(keybinds-overlay): glyph resolution find_cap/key_name_to_glyph + tests

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: jump_mode field + jump_to_key / toggle_mode / is_jump_mode

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` — `KeybindsOverlay` struct (line ~844), `new` (line ~852), `show` (line ~876), `show_last_row` (line ~927), and add three methods.

- [ ] **Step 1: Add the field to the struct**

Modify the struct (around line 844):

```rust
pub struct KeybindsOverlay {
    pub overlay: Overlay,
    drawing_area: DrawingArea,
    row_index: Rc<std::cell::Cell<usize>>,
    selected: Rc<std::cell::Cell<usize>>,
    jump_mode: Rc<std::cell::Cell<bool>>,
}
```

- [ ] **Step 2: Initialize it in `new` and wire it into the draw closure**

In `new` (around line 852-873), after `let selected = …`, add the cell and pass it to the draw closure:

```rust
        let row_index = Rc::new(std::cell::Cell::new(0usize));
        let selected = Rc::new(std::cell::Cell::new(first_bound(&row_keys(0))));
        let jump_mode = Rc::new(std::cell::Cell::new(true));

        let row_draw = row_index.clone();
        let sel_draw = selected.clone();
        let jump_draw = jump_mode.clone();
        drawing_area.set_draw_func(move |_area, cr, w, h| {
            draw_row_screen(cr, row_draw.get(), sel_draw.get(), jump_draw.get(), w as f64, h as f64);
        });

        KeybindsOverlay { overlay, drawing_area, row_index, selected, jump_mode }
```

(Note: `draw_row_screen` gains a `jump_mode: bool` param in Task 3. Until then this will not compile — that is expected; Tasks 2 and 3 land together. Build verification is at the end of Task 3.)

- [ ] **Step 3: Force jump mode on open**

In `show` (around line 876), set jump mode true at the top:

```rust
    pub fn show(&self) {
        self.jump_mode.set(true);
        // Reopen on the previously viewed row (row_index/selected persist across
        // hide/show). Clamp the row in case ROW_COUNT changed.
        let row = self.row_index.get().min(ROW_COUNT - 1);
```

In `show_last_row` (around line 927), set jump mode true at the top:

```rust
    pub fn show_last_row(&self) {
        self.jump_mode.set(true);
        let last = ROW_COUNT - 1;
```

- [ ] **Step 4: Add the three methods**

Add inside `impl KeybindsOverlay` (e.g. after `move_selection`, around line 945):

```rust
    /// Jump the highlight to the cap for `key_name`, switching rows if the cap
    /// is on another row. Returns true if a cap matched, false otherwise.
    pub fn jump_to_key(&self, key_name: &str) -> bool {
        match find_cap(key_name) {
            Some((row, idx)) => {
                self.row_index.set(row);
                self.selected.set(idx);
                self.drawing_area.queue_draw();
                true
            }
            None => false,
        }
    }

    /// Flip between jump mode and nav mode and redraw.
    pub fn toggle_mode(&self) {
        self.jump_mode.set(!self.jump_mode.get());
        self.drawing_area.queue_draw();
    }

    /// Whether the overlay is currently in jump mode (vs nav mode).
    pub fn is_jump_mode(&self) -> bool {
        self.jump_mode.get()
    }
```

- [ ] **Step 5: (No separate commit — build happens in Task 3)**

This task does not compile on its own because `draw_row_screen` does not yet take the `jump_mode` parameter. Proceed directly to Task 3; commit there.

---

## Task 3: Mode-aware draw (header + footer), build green

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` — `draw_row_screen` signature (line ~557), header (line ~576-580), footer (line ~803-809).

- [ ] **Step 1: Add the `jump_mode` parameter to `draw_row_screen`**

Change the signature (around line 557):

```rust
fn draw_row_screen(
    cr: &gtk4::cairo::Context,
    row_idx: usize,
    selected: usize,
    jump_mode: bool,
    widget_w: f64,
    widget_h: f64,
) {
```

- [ ] **Step 2: Show the mode in the header**

Replace the header block (around line 576-580):

```rust
    let title = ROW_TITLES.get(row_idx).copied().unwrap_or("");
    let mode = if jump_mode { "JUMP" } else { "NAV" };
    let header = format!("Row {} of {}  —  {}  —  {}", row_idx + 1, ROW_COUNT + 1, title, mode);
    let he = cr.text_extents(&header).unwrap();
    let _ = cr.move_to((widget_w - he.width()) / 2.0, 48.0);
    let _ = cr.show_text(&header);
```

- [ ] **Step 3: Make the footer mode-aware**

Replace the footer block (around line 803-809):

```rust
    cr.select_font_face("sans-serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(14.0);
    cr.set_source_rgb(0.78, 0.76, 0.82);
    let foot = if jump_mode {
        "Esc close  \u{00b7}  Tab jump/nav  \u{00b7}  press a key to jump to its cap  \u{00b7}  \u{2190}/\u{2192} move  \u{00b7}  \u{2191}/\u{2193} rows"
    } else {
        "Esc close  \u{00b7}  Tab jump/nav  \u{00b7}  n/p or \u{2191}/\u{2193} rows  \u{00b7}  j/k or \u{2190}/\u{2192} move"
    };
    let fe = cr.text_extents(foot).unwrap();
    let _ = cr.move_to((widget_w - fe.width()) / 2.0, widget_h - 28.0);
    let _ = cr.show_text(foot);
```

- [ ] **Step 4: Build and run the unit suite**

Run: `cargo build`
Expected: compiles cleanly (Tasks 2 + 3 now consistent).

Run: `cargo test --bin linux-lit keybinds_overlay::tests`
Expected: PASS (5 tests still green).

- [ ] **Step 5: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "feat(keybinds-overlay): jump_mode state + jump_to_key/toggle_mode, mode-aware header/footer

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Route keys by mode in handle_keybinds_key

**Files:**
- Modify: `src/input/keymap.rs` — `handle_keybinds_key` (lines 1126-1170).

- [ ] **Step 1: Replace `handle_keybinds_key` with the mode-aware version**

Replace the whole function (lines 1126-1170) with:

```rust
fn handle_keybinds_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool {
    // Advance a row; past the last keyboard row hands off to the gamepad screen.
    fn next_row_or_gamepad(state: &Rc<RefCell<AppState>>) {
        let advanced = state.borrow().keybinds_overlay.next_row();
        if !advanced {
            let s = state.borrow();
            s.keybinds_overlay.hide();
            s.gamepad_overlay.show();
            drop(s);
            state.borrow_mut().input_mode = crate::app::InputMode::GamepadOverlay;
        }
    }
    // Previous row; before the first keyboard row hands off to the gamepad screen.
    fn prev_row_or_gamepad(state: &Rc<RefCell<AppState>>) {
        let moved = state.borrow().keybinds_overlay.prev_row();
        if !moved {
            let s = state.borrow();
            s.keybinds_overlay.hide();
            s.gamepad_overlay.show();
            drop(s);
            state.borrow_mut().input_mode = crate::app::InputMode::GamepadOverlay;
        }
    }

    match key_name {
        "Escape" => {
            state.borrow().keybinds_overlay.hide();
            state.borrow_mut().input_mode = crate::app::InputMode::Reader;
            return true;
        }
        "Tab" => {
            state.borrow().keybinds_overlay.toggle_mode();
            return true;
        }
        // Arrows navigate in BOTH modes.
        "Up" => {
            next_row_or_gamepad(state);
            return true;
        }
        "Down" => {
            prev_row_or_gamepad(state);
            return true;
        }
        "Right" => {
            state.borrow().keybinds_overlay.move_selection(1);
            return true;
        }
        "Left" => {
            state.borrow().keybinds_overlay.move_selection(-1);
            return true;
        }
        _ => {}
    }

    if state.borrow().keybinds_overlay.is_jump_mode() {
        // Jump mode: any other key jumps the highlight to its cap (no-op if no
        // matching cap). Always consume so nothing leaks to the reader.
        state.borrow().keybinds_overlay.jump_to_key(key_name);
        return true;
    }

    // Nav mode: the classic n/p rows, j/k highlight.
    match key_name {
        "n" => next_row_or_gamepad(state),
        "p" => prev_row_or_gamepad(state),
        "j" => {
            state.borrow().keybinds_overlay.move_selection(1);
        }
        "k" => {
            state.borrow().keybinds_overlay.move_selection(-1);
        }
        _ => {}
    }
    true // consume all other keys while keybinds visible
}
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles cleanly.

- [ ] **Step 3: Run the full bins test suite + clippy**

Run: `cargo test --bins`
Expected: PASS (no regressions; the 5 overlay tests included).

Run: `cargo clippy --bins 2>&1 | rg -i "warning|error" | head`
Expected: no new warnings introduced by these changes.

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat(keybinds-overlay): route keys by jump/nav mode; Tab toggles, arrows nav both modes

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Update the Cairo-overlay keybind skill doc

**Files:**
- Modify: `.claude/skills/update-cairo-keybinds-overlay/SKILL.md` (only if it documents footer/nav text or the draw signature; otherwise skip).

- [ ] **Step 1: Check whether the skill documents the now-changed bits**

Run: `cat .claude/skills/update-cairo-keybinds-overlay/SKILL.md`
Expected: read it; look for references to the footer string, `draw_row_screen` signature, or the n/p/j/k navigation that this change alters.

- [ ] **Step 2: If it does, update those references**

Edit the SKILL.md so any quoted footer text, draw-function signature (now takes `jump_mode: bool`), or navigation description matches the new jump/nav-mode behavior. If the skill does not mention any of these, make no change and note "no doc change needed" — do NOT invent content.

- [ ] **Step 3: Commit (only if changed)**

```bash
git add .claude/skills/update-cairo-keybinds-overlay/SKILL.md
git commit -m "docs(skill): update-cairo-keybinds-overlay reflects jump/nav modes

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Headless / visual verification (user-run)

The behavioral and rendered parts (mode toggle, auto-row-switch on jump,
mode-aware header/footer) are overlay-geometry changes. Per the project rule, an
agent cannot drive the live dwl session; ask the user to run the headless
overlay check and eyeball the result.

- [ ] **Step 1: Ask the user to launch the reader headless and open the overlay**

Provide these commands and ask the user to paste the screenshot / report:

```bash
cd ~/utono/linux-lit && cargo build
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
```

Then, once the window is focused (wait ~3s):

```bash
export WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000
wtype -M ctrl "/" -m ctrl     # open the keybinds overlay
grim /tmp/kb-jump.png         # should show JUMP in the header + new footer
wtype "h"                     # jump to the home row 'h' cap
grim /tmp/kb-h.png            # HOME ROW, 'h' highlighted, synopsis detail
wtype -k Tab                  # toggle to NAV
grim /tmp/kb-nav.png          # header shows NAV, nav footer
```

Cleanup:

```bash
pkill -f "cage -- ./target/debug/linux-lit"; pkill -f target/debug/linux-lit
```

- [ ] **Step 2: Confirm the acceptance criteria from the screenshots**

- `/tmp/kb-jump.png`: header ends `… — JUMP`; footer reads the jump-mode hint.
- `/tmp/kb-h.png`: shows HOME ROW with the `h` cap highlighted and the synopsis
  detail panel (auto-row-switch worked).
- `/tmp/kb-nav.png`: header ends `… — NAV`; footer reads the nav-mode hint.

If any criterion fails, treat it as a bug and return to the relevant task.

---

## Self-Review Notes

- **Spec coverage:** jump mode default-on-open (Task 2 Step 3) ✓; Tab toggle (Task 3 header/footer + Task 4 Tab arm) ✓; auto-row-switch (Task 2 `jump_to_key` via `find_cap` scanning all rows) ✓; unshifted-only matching (`find_cap` compares `unshifted`) ✓; arrows in both modes + Esc both modes (Task 4) ✓; gamepad handoff preserved (Task 4 helpers) ✓; mode indicator header + mode-aware footer (Task 3) ✓; pure-logic tests (Task 1) ✓; Space known-limitation (no special-case added — global handler untouched) ✓.
- **Placeholders:** none — every code step shows full code; Task 5 is explicitly conditional with a "no doc change needed" escape, not a vague TODO.
- **Type consistency:** `jump_mode: Rc<Cell<bool>>` matches `row_index`/`selected`; `find_cap`/`key_name_to_glyph` signatures match their call sites; `draw_row_screen`'s new `jump_mode: bool` param is added (Task 3) before its first use compiles (Task 2 note flags the cross-task dependency); method names `jump_to_key`/`toggle_mode`/`is_jump_mode` are used identically in keymap.rs.
