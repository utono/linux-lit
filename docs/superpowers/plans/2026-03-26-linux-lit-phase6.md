# Phase 6: Search + Tab Seek Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add vim-style `/` search with live results, match highlighting, `n`/`N` cycling, and change Tab to seek+resume at the current line's timestamp.

**Architecture:** A `SearchBar` widget (bottom-anchored `GtkBox` with `Entry` + `Label`) provides live search input. Search results are stored as `Vec<SearchMatch>` in `AppState` with two `TextTag`s for all-match and current-match highlighting. Smart-case matching uses `Line.normalized` for case-insensitive and `Line.text` for case-sensitive. Tab is changed globally from toggle-pause to seek+resume via a new `MpvCommand::ResumeAndSeek(f64)`.

**Tech Stack:** gtk4-rs (TextTag, TextIter, Entry, Box, Label), Rust std

---

### Task 1: Add `MpvCommand::ResumeAndSeek` and Handle It

**Files:**
- Modify: `src/mpv/commands.rs:6-17`
- Modify: `src/mpv/client.rs:84-99`

- [ ] **Step 1: Add the new command variant**

In `src/mpv/commands.rs`, add `ResumeAndSeek(f64)` to the `MpvCommand` enum:

```rust
pub enum MpvCommand {
    Seek(f64),
    TogglePause,
    ResumeAndSeek(f64),
    SetSpeed(f64),
    LoadFile(String),
    Connect(String),
    Disconnect,
    SetTimestamps {
        timestamps: Vec<(i64, f64, f64)>,
        line_id_to_index: HashMap<i64, usize>,
    },
}
```

- [ ] **Step 2: Handle `ResumeAndSeek` in the client**

In `src/mpv/client.rs`, add a match arm after the `Seek` arm (after line 100):

```rust
MpvCommand::ResumeAndSeek(time) => {
    if let Some(w) = writer.as_mut() {
        let cmd = format!(r#"{{"command":["set_property","time-pos",{}]}}"#, time);
        let _ = send_command(w, &cmd).await;
        let _ = send_command(w, r#"{"command":["set_property","pause",false]}"#).await;
    }
}
```

- [ ] **Step 3: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add src/mpv/commands.rs src/mpv/client.rs
git commit -m "feat: add MpvCommand::ResumeAndSeek for seek+unpause"
```

---

### Task 2: Change Tab Keybind to Seek+Resume

**Files:**
- Modify: `src/input/keymap.rs:180-184`

- [ ] **Step 1: Replace the Tab handler**

In `src/input/keymap.rs`, replace the `"Tab"` match arm (lines 180-184) with:

```rust
"Tab" => {
    let s = state.borrow();
    if let Some(ref work) = s.current_work {
        if let Some(ts) = &work.lines[s.current_line].timestamp {
            let seek_time = (ts.start - crate::input::navigation::SEEK_PREROLL).max(0.0);
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::ResumeAndSeek(seek_time));
        }
    }
    true
}
```

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat: change Tab to seek+resume at current line timestamp"
```

---

### Task 3: Create `SearchBar` Widget

**Files:**
- Create: `src/ui/search_bar.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Add module declaration**

In `src/ui/mod.rs`, add:

```rust
pub mod search_bar;
```

- [ ] **Step 2: Create `src/ui/search_bar.rs`**

```rust
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Entry, Label, Orientation};

pub struct SearchBar {
    pub container: GtkBox,
    entry: Entry,
    counter: Label,
}

impl SearchBar {
    pub fn new() -> Self {
        let container = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .halign(gtk4::Align::Fill)
            .valign(gtk4::Align::End)
            .build();
        container.add_css_class("search-bar");

        let slash_label = Label::builder().label("/").build();
        slash_label.add_css_class("search-slash");

        let entry = Entry::builder()
            .hexpand(true)
            .build();
        entry.add_css_class("search-entry");

        let counter = Label::builder()
            .label("")
            .build();
        counter.add_css_class("search-counter");

        container.append(&slash_label);
        container.append(&entry);
        container.append(&counter);
        container.set_visible(false);

        SearchBar {
            container,
            entry,
            counter,
        }
    }

    pub fn show(&self) {
        self.entry.set_text("");
        self.counter.set_label("");
        self.container.set_visible(true);
        self.entry.grab_focus();
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    pub fn update_counter(&self, current: usize, total: usize) {
        if total == 0 {
            self.counter.set_label("[0/0]");
        } else {
            self.counter.set_label(&format!("[{}/{}]", current + 1, total));
        }
    }

    pub fn query(&self) -> String {
        self.entry.text().to_string()
    }
}
```

- [ ] **Step 3: Build to verify**

Run: `cargo build`
Expected: compiles (SearchBar unused warnings are fine)

- [ ] **Step 4: Commit**

```bash
git add src/ui/mod.rs src/ui/search_bar.rs
git commit -m "feat: add SearchBar widget with entry and match counter"
```

---

### Task 4: Add Search State to `AppState` and Wire SearchBar into Window

**Files:**
- Modify: `src/app.rs:1-8` (imports)
- Modify: `src/app.rs:15-34` (AppState struct)
- Modify: `src/app.rs:60-65` (tag creation)
- Modify: `src/app.rs:112-118` (overlay wiring)
- Modify: `src/app.rs:123-140` (state init)
- Modify: `src/app.rs:142-150` (search entry connect_changed)
- Modify: `src/theme.rs:223-239` (CSS)

- [ ] **Step 1: Add `SearchMatch` struct and imports**

At the top of `src/app.rs`, add the import for `search_bar`:

```rust
use crate::ui::search_bar::SearchBar;
```

Add above the `AppState` struct:

```rust
#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub line_index: usize,
    pub byte_start: usize,
    pub byte_end: usize,
}
```

- [ ] **Step 2: Add search fields to `AppState`**

Add these fields to `AppState` after `playback_speed`:

```rust
pub search_bar: SearchBar,
pub search_matches: Vec<SearchMatch>,
pub search_match_idx: usize,
pub search_tag: gtk4::TextTag,
pub search_current_tag: gtk4::TextTag,
pub search_active: bool,
```

- [ ] **Step 3: Create search tags after `dim_tag`**

After line 65 (`buffer.tag_table().add(&dim_tag);`), add:

```rust
let search_tag = gtk4::TextTag::builder()
    .name("search-match")
    .background(if theme.is_light {
        "rgba(255, 200, 0, 0.35)"
    } else {
        "rgba(255, 200, 0, 0.25)"
    })
    .build();
buffer.tag_table().add(&search_tag);

let search_current_tag = gtk4::TextTag::builder()
    .name("search-current")
    .background(if theme.is_light {
        "rgba(255, 140, 0, 0.55)"
    } else {
        "rgba(255, 140, 0, 0.45)"
    })
    .build();
buffer.tag_table().add(&search_current_tag);
```

- [ ] **Step 4: Wire SearchBar into the overlay layout**

The current layout is: `window → picker.overlay → scrolled`. The search bar needs to sit at the bottom of the window, below the overlay. Change the window child setup.

Replace lines 113-118 (from `let mut picker` through `window.set_child`) with:

```rust
// Library picker overlay
let mut picker = LibraryPicker::new();
picker.set_works(works);
picker.attach(&scrolled);

// Search bar at bottom
let search_bar = SearchBar::new();
let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
vbox.append(&picker.overlay);
vbox.append(&search_bar.container);

window.set_child(Some(&vbox));
```

Make `picker.overlay` vexpand so the search bar stays at the bottom:

After `picker.attach(&scrolled);` add:

```rust
picker.overlay.set_vexpand(true);
```

- [ ] **Step 5: Initialize search fields in AppState constructor**

In the `AppState` constructor (lines 123-140), add after `playback_speed: 1.0,`:

```rust
search_bar,
search_matches: Vec::new(),
search_match_idx: 0,
search_tag,
search_current_tag,
search_active: false,
```

- [ ] **Step 6: Add CSS for search bar**

In `src/theme.rs` `generate_css`, append to the format string before the closing `"`:

```
 .search-bar {{ background-color: {bg}; color: {fg}; padding: 4px 12px; }} \
 .search-entry {{ background: transparent; border: none; color: {fg}; }} \
 .search-slash {{ color: {fg}; opacity: 0.6; }} \
 .search-counter {{ color: {fg}; opacity: 0.6; }}
```

- [ ] **Step 7: Build to verify**

Run: `cargo build`
Expected: compiles (search module not wired yet, but AppState and tags compile)

- [ ] **Step 8: Commit**

```bash
git add src/app.rs src/ui/search_bar.rs src/theme.rs
git commit -m "feat: add search state to AppState and wire SearchBar into window"
```

---

### Task 5: Implement Search Logic

**Files:**
- Create: `src/input/search.rs`
- Modify: `src/input/mod.rs`

- [ ] **Step 1: Add module declaration**

In `src/input/mod.rs`, add:

```rust
pub mod search;
```

- [ ] **Step 2: Create `src/input/search.rs`**

```rust
use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::{AppState, SearchMatch};

/// Run search against loaded work, update highlights and counter.
/// Called on every keystroke in the search entry.
pub fn execute_search(state_rc: &Rc<RefCell<AppState>>) {
    let mut state = state_rc.borrow_mut();
    let query = state.search_bar.query();

    clear_highlights(&state);
    state.search_matches.clear();
    state.search_match_idx = 0;

    if query.is_empty() {
        state.search_bar.update_counter(0, 0);
        return;
    }

    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    // Smart-case: if query has uppercase, match case-sensitively;
    // otherwise match case-insensitively.
    // Always search in line.text to keep byte offsets consistent with the buffer.
    let case_sensitive = query.chars().any(|c| c.is_uppercase());

    for (line_idx, line) in work.lines.iter().enumerate() {
        if case_sensitive {
            let mut search_start = 0;
            while let Some(pos) = line.text[search_start..].find(&*query) {
                let byte_start = search_start + pos;
                let byte_end = byte_start + query.len();
                state.search_matches.push(SearchMatch {
                    line_index: line_idx,
                    byte_start,
                    byte_end,
                });
                search_start = byte_end;
            }
        } else {
            // Case-insensitive: lowercase both sides, but track byte positions in original text
            let text_lower = line.text.to_lowercase();
            let query_lower = query.to_lowercase();
            let mut search_start = 0;
            while let Some(pos) = text_lower[search_start..].find(&*query_lower) {
                let byte_start = search_start + pos;
                let byte_end = byte_start + query_lower.len();
                state.search_matches.push(SearchMatch {
                    line_index: line_idx,
                    byte_start,
                    byte_end,
                });
                search_start = byte_end;
            }
        }
    }

    apply_highlights(&state);

    let total = state.search_matches.len();
    if total > 0 {
        // Jump to first match at or after current_line
        let idx = state
            .search_matches
            .iter()
            .position(|m| m.line_index >= state.current_line)
            .unwrap_or(0);
        state.search_match_idx = idx;
        apply_current_highlight(&state);
        state.search_bar.update_counter(idx, total);
    } else {
        state.search_bar.update_counter(0, 0);
    }
}

/// Jump to next match, wrapping around.
pub fn next_match(state: &mut AppState) {
    let total = state.search_matches.len();
    if total == 0 {
        return;
    }
    remove_current_highlight(state);
    state.search_match_idx = (state.search_match_idx + 1) % total;
    let m = &state.search_matches[state.search_match_idx];
    state.current_line = m.line_index;
    apply_current_highlight(state);
    state.search_bar.update_counter(state.search_match_idx, total);
    crate::input::navigation::update_highlight_and_ensure_visible(state);
}

/// Jump to previous match, wrapping around.
pub fn prev_match(state: &mut AppState) {
    let total = state.search_matches.len();
    if total == 0 {
        return;
    }
    remove_current_highlight(state);
    state.search_match_idx = (state.search_match_idx + total - 1) % total;
    let m = &state.search_matches[state.search_match_idx];
    state.current_line = m.line_index;
    apply_current_highlight(state);
    state.search_bar.update_counter(state.search_match_idx, total);
    crate::input::navigation::update_highlight_and_ensure_visible(state);
}

/// Clear all search state: highlights, matches, active flag.
pub fn clear_search(state: &mut AppState) {
    clear_highlights(state);
    state.search_matches.clear();
    state.search_match_idx = 0;
    state.search_active = false;
}

// --- internal helpers ---

fn clear_highlights(state: &AppState) {
    let (start, end) = state.buffer.bounds();
    state.buffer.remove_tag(&state.search_tag, &start, &end);
    state
        .buffer
        .remove_tag(&state.search_current_tag, &start, &end);
}

fn apply_highlights(state: &AppState) {
    for m in &state.search_matches {
        let Some(line_start) = state.buffer.iter_at_line(m.line_index as i32) else {
            continue;
        };
        let mut start = line_start;
        start.set_line_offset(0);
        // Convert byte offset to char offset for GTK TextIter
        let line_text = &state.current_work.as_ref().unwrap().lines[m.line_index].text;
        let char_start = line_text[..m.byte_start].chars().count() as i32;
        let char_end = line_text[..m.byte_end].chars().count() as i32;
        let mut match_start = line_start;
        match_start.forward_chars(char_start);
        let mut match_end = line_start;
        match_end.forward_chars(char_end);
        state
            .buffer
            .apply_tag(&state.search_tag, &match_start, &match_end);
    }
}

fn apply_current_highlight(state: &AppState) {
    if state.search_matches.is_empty() {
        return;
    }
    let m = &state.search_matches[state.search_match_idx];
    let Some(line_start) = state.buffer.iter_at_line(m.line_index as i32) else {
        return;
    };
    let line_text = &state.current_work.as_ref().unwrap().lines[m.line_index].text;
    let char_start = line_text[..m.byte_start].chars().count() as i32;
    let char_end = line_text[..m.byte_end].chars().count() as i32;
    let mut match_start = line_start;
    match_start.forward_chars(char_start);
    let mut match_end = line_start;
    match_end.forward_chars(char_end);
    state
        .buffer
        .apply_tag(&state.search_current_tag, &match_start, &match_end);
}

fn remove_current_highlight(state: &AppState) {
    if state.search_matches.is_empty() {
        return;
    }
    let m = &state.search_matches[state.search_match_idx];
    let Some(line_start) = state.buffer.iter_at_line(m.line_index as i32) else {
        return;
    };
    let line_text = &state.current_work.as_ref().unwrap().lines[m.line_index].text;
    let char_start = line_text[..m.byte_start].chars().count() as i32;
    let char_end = line_text[..m.byte_end].chars().count() as i32;
    let mut match_start = line_start;
    match_start.forward_chars(char_start);
    let mut match_end = line_start;
    match_end.forward_chars(char_end);
    state
        .buffer
        .remove_tag(&state.search_current_tag, &match_start, &match_end);
}
```

- [ ] **Step 3: Wire search entry `connect_changed` in `src/app.rs`**

After the picker's `connect_changed` block (after line 150 in `app.rs`), add:

```rust
// Connect search entry for live search
let state_for_search = Rc::clone(&state);
{
    let s = state.borrow();
    s.search_bar.entry().connect_changed(move |_entry| {
        crate::input::search::execute_search(&state_for_search);
    });
}
```

- [ ] **Step 4: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add src/input/mod.rs src/input/search.rs src/app.rs
git commit -m "feat: implement search logic with smart-case matching and highlighting"
```

---

### Task 6: Wire Search Keybinds into Keymap

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Add search bar visible guard**

After the picker-visible block (after line 104), before the `// --- Normal mode` comment, add:

```rust
// Search bar visible — route keys to search entry
let search_visible = state.borrow().search_bar.is_visible();
if search_visible {
    match key_name {
        "Escape" => {
            let mut s = state.borrow_mut();
            s.search_bar.hide();
            s.search_active = !s.search_matches.is_empty();
            return true;
        }
        "Return" => {
            let mut s = state.borrow_mut();
            s.search_bar.hide();
            s.search_active = !s.search_matches.is_empty();
            // Jump cursor to current match
            if !s.search_matches.is_empty() {
                let m = &s.search_matches[s.search_match_idx];
                s.current_line = m.line_index;
                crate::input::navigation::update_highlight_and_ensure_visible(&mut s);
            }
            return true;
        }
        _ => return false, // let GTK route to the Entry
    }
}
```

- [ ] **Step 2: Add search-active clear guard**

After the search-visible guard (and after the `gg` sequence check), before the Ctrl+Shift block, add:

```rust
// Clear search highlights on any key that isn't n/N (and search bar is not visible)
let search_active = state.borrow().search_active;
if search_active && key_name != "n" && key_name != "N" {
    crate::input::search::clear_search(&mut state.borrow_mut());
}
```

- [ ] **Step 3: Add `/` keybind and `n`/`N` keybinds**

In the single-key `match` block (around line 151), add before the `_ => false` arm:

```rust
"slash" => {
    let mut s = state.borrow_mut();
    crate::input::search::clear_search(&mut s);
    s.search_bar.show();
    true
}
"n" => {
    let active = state.borrow().search_active;
    if active {
        crate::input::search::next_match(&mut state.borrow_mut());
        true
    } else {
        false
    }
}
"N" => {
    let active = state.borrow().search_active;
    if active {
        crate::input::search::prev_match(&mut state.borrow_mut());
        true
    } else {
        false
    }
}
```

- [ ] **Step 4: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat: wire search keybinds (slash, n, N, Escape, Return)"
```

---

### Task 7: Clear Search on Work Load

**Files:**
- Modify: `src/app.rs` — `display_work` function

- [ ] **Step 1: Clear search state at start of `display_work`**

At the start of `display_work` (after `pub fn display_work(state: &mut AppState, work: Work) {`), add:

```rust
crate::input::search::clear_search(state);
state.search_bar.hide();
```

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: clear search state when loading a new work"
```

---

### Task 8: Build + Clippy + Manual Test

- [ ] **Step 1: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: no warnings

- [ ] **Step 2: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 3: Fix any issues from clippy/tests**

- [ ] **Step 4: Final commit if fixes needed**

```bash
git add -A
git commit -m "fix: address clippy warnings and test failures"
```
