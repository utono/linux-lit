# Word Cycle Copy (`w` keybind) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `w` keybind that cycles through words on the cursor line, copying each to the system clipboard and showing the word in a bottom-left status label.

**Architecture:** Three fields added to AppState (word cycle index/line, status label). One new handler function in navigation.rs. One match arm in keymap.rs. CSS styling follows the existing sync-off-icon overlay pattern.

**Tech Stack:** GTK4, glib timers, wl-copy (Wayland clipboard)

---

### Task 1: Add AppState fields and word status label

**Files:**
- Modify: `src/app.rs:34-126` (AppState struct)
- Modify: `src/app.rs:446-454` (label creation, near sync_icon)
- Modify: `src/app.rs:474-554` (AppState constructor)
- Modify: `src/theme.rs:402` (CSS near sync-off-icon)

- [ ] **Step 1: Add fields to AppState struct**

In `src/app.rs`, after `sync_icon: gtk4::Label` (line 125), add:

```rust
    pub word_status_label: gtk4::Label,
    pub word_cycle_line: Option<usize>,
    pub word_cycle_index: usize,
    pub word_status_timer: Rc<Cell<u64>>,
```

- [ ] **Step 2: Create the word status label widget**

In `src/app.rs`, after the sync_icon block (after line 454, before the concordance bar), add:

```rust
    // Word-copy status indicator (lower-left corner, hidden by default)
    let word_status_label = gtk4::Label::new(None);
    word_status_label.set_valign(gtk4::Align::End);
    word_status_label.set_halign(gtk4::Align::Start);
    word_status_label.set_margin_start(12);
    word_status_label.set_margin_bottom(40);
    word_status_label.add_css_class("word-status");
    word_status_label.set_visible(false);
    concordance_list_picker.overlay.add_overlay(&word_status_label);
```

Note: `margin_bottom: 40` places it above the sync_icon (which uses margin_bottom: 12).

- [ ] **Step 3: Initialize fields in AppState constructor**

In `src/app.rs`, after `sync_icon,` (line 553), add:

```rust
        word_status_label,
        word_cycle_line: None,
        word_cycle_index: 0,
        word_status_timer: Rc::new(Cell::new(0)),
```

- [ ] **Step 4: Add CSS styling**

In `src/theme.rs`, after the `.sync-off-icon` rule (line 402), add:

```
         .word-status {{ font-size: 16px; color: {fg}; opacity: 0.85; }} \
```

- [ ] **Step 5: Build to verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: successful compilation (warnings OK, no errors)

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/theme.rs
git commit -m "Add word_cycle state fields and word-status label to AppState"
```

---

### Task 2: Implement word_cycle_copy handler

**Files:**
- Modify: `src/input/navigation.rs` (add function at end of file)

- [ ] **Step 1: Add the word_cycle_copy function**

At the end of `src/input/navigation.rs`, add:

```rust
/// Cycle through words on the current line, copying each to the system clipboard.
/// Each press advances to the next word; wraps after the last word.
/// Shows the copied word in a status label that auto-hides after 2 seconds.
pub fn word_cycle_copy(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    // Map buffer line to work line index
    let work_line_idx = if let Some(ref lm) = state.line_map {
        match lm.buffer_to_work.get(state.current_line).copied().flatten() {
            Some(idx) => idx,
            None => return,
        }
    } else {
        state.current_line
    };

    let line = match work.lines.get(work_line_idx) {
        Some(l) => l,
        None => return,
    };

    // Extract words: split on whitespace, strip leading/trailing non-alphanumeric
    let words: Vec<String> = line
        .text
        .split_whitespace()
        .filter_map(|token| {
            let stripped: String = token
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_string();
            if stripped.is_empty() {
                None
            } else {
                Some(stripped)
            }
        })
        .collect();

    if words.is_empty() {
        return;
    }

    // Reset index if we moved to a different line
    let idx = if state.word_cycle_line == Some(state.current_line) {
        state.word_cycle_index % words.len()
    } else {
        0
    };

    let word = &words[idx];

    // Copy to clipboard via wl-copy
    use std::io::Write;
    use std::process::{Command, Stdio};
    match Command::new("wl-copy").stdin(Stdio::piped()).spawn() {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(word.as_bytes());
            }
            let _ = child.wait();
        }
        Err(e) => {
            log_fmt!("WORD_COPY: wl-copy failed: {}", e);
            return;
        }
    }

    log_fmt!("WORD_COPY: copied '{}' (word {}/{})", word, idx + 1, words.len());

    // Update cycle state
    state.word_cycle_line = Some(state.current_line);
    state.word_cycle_index = idx + 1; // next press gets the next word (mod len happens above)

    // Show status label
    state.word_status_label.set_label(word);
    state.word_status_label.set_visible(true);

    // Bump timer generation to cancel any pending hide
    let gen = state.word_status_timer.get() + 1;
    state.word_status_timer.set(gen);
    let timer_rc = state.word_status_timer.clone();
    let label = state.word_status_label.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
        if timer_rc.get() == gen {
            label.set_visible(false);
        }
    });
}
```

- [ ] **Step 2: Build to verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: successful compilation (warnings OK, no errors)

- [ ] **Step 3: Commit**

```bash
git add src/input/navigation.rs
git commit -m "Implement word_cycle_copy: cycle words, copy to clipboard, show status"
```

---

### Task 3: Wire up the `w` key in keymap

**Files:**
- Modify: `src/input/keymap.rs:930-1258` (single-key match block)

- [ ] **Step 1: Add "w" match arm**

In `src/input/keymap.rs`, in the single-key match block (after the `"V"` arm at line 1253-1256, before `_ => false` at line 1257), add:

```rust
        "w" => {
            navigation::word_cycle_copy(&mut state.borrow_mut());
            true
        }
```

- [ ] **Step 2: Build to verify compilation**

Run: `cargo build 2>&1 | head -20`
Expected: successful compilation, no errors

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Wire w key to word_cycle_copy in normal mode keymap"
```
