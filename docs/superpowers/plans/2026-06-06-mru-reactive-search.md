# MRU-Reactive Search via n/N Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After the user cancels `/` search with Escape, `n`/`N` reactivate the most-recently-used search pattern, navigate to the canonical spread containing the next/previous match (highlighting the matched line), never wrap at the ends, and show a left/right edge toast at the boundaries.

**Architecture:** Add a session-scoped MRU pattern field to `AppState`; extract the shared match-navigation body in `search.rs` into `goto_match_idx` that lands on the canonical spread (mirroring `navigation::jump_to_line`); make `next_match`/`prev_match` stop at the ends with edge toasts instead of wrapping; add `reactivate_and_step` that re-runs the MRU search when no matches are live and seeds the target index from the cursor position; route `n`/`N` through it in `keymap.rs`.

**Tech Stack:** Rust, GTK4 (`gtk4`, `libadwaita`), existing linux-lit `AppState` / `TextBuffer` / `TextTag` machinery.

**Spec:** `docs/superpowers/specs/2026-06-06-mru-reactive-search-design.md`

---

## File Structure

- `src/app.rs` — add `last_search_query: Option<String>` to `AppState`; init it; build two edge-toast `Label` widgets (`search_edge_toast_left`, `search_edge_toast_right`) and assign them in the struct literal.
- `src/ui/search_bar.rs` — add `pub fn set_text(&self, text: &str)` so reactivation can pre-fill the entry (the `entry` field is private).
- `src/input/search.rs` — store MRU in `execute_search`; add `goto_match_idx`; rewrite `next_match`/`prev_match` to stop + toast; add `edge_toast` helper and `Side` enum; add `reactivate_and_step`.
- `src/input/keymap.rs` — route `SearchNextMatch`/`SearchPrevMatch` through `reactivate_and_step`.

Pagination/landing/toast positioning are visual-only (per `CLAUDE.md`) — there are no pure unit tests for them. The one piece with pure logic worth a unit test is the no-wrap boundary decision, but it is entangled with GTK `AppState`; we instead verify the build with `cargo build` + `cargo test --bins` and hand off the on-screen check to the user via the e2e/manual launch.

---

### Task 1: Add MRU field to AppState

**Files:**
- Modify: `src/app.rs:131-135` (struct field), `src/app.rs:1379-1381` (struct literal init)

- [ ] **Step 1: Add the struct field**

In `src/app.rs`, in the `AppState` struct, immediately after the existing
`pub search_match_idx: usize,` line (currently line 132), add:

```rust
    pub search_match_idx: usize,
    /// Most-recently-used non-empty search pattern. Persists for the session
    /// (survives Escape and work switches) so n/N can reactivate search. NOT
    /// cleared by clear_search.
    pub last_search_query: Option<String>,
```

- [ ] **Step 2: Initialize it in the struct literal**

In `src/app.rs`, in the `AppState { ... }` constructor literal, immediately
after `search_match_idx: 0,` (currently line 1380), add:

```rust
        search_match_idx: 0,
        last_search_query: None,
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds (a `field is never read` warning for `last_search_query` is
acceptable at this point — later tasks use it).

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat(search): add session MRU search pattern field"
```

---

### Task 2: Add edge-toast widgets to AppState

**Files:**
- Modify: `src/app.rs` (struct fields near line 303-304; widget construction near line 1252; struct literal near line 1498)

- [ ] **Step 1: Add the two struct fields**

In `src/app.rs`, find the existing toast fields:

```rust
    pub chapter_toast: gtk4::Label,
    pub speed_toast: gtk4::Label,
```

Add immediately after them:

```rust
    pub chapter_toast: gtk4::Label,
    pub speed_toast: gtk4::Label,
    /// Left/right edge toasts for search boundaries ("no earlier/later
    /// occurrence"). Separate from speed_toast so search messages never
    /// clobber the playback-speed toast text.
    pub search_edge_toast_left: gtk4::Label,
    pub search_edge_toast_right: gtk4::Label,
```

- [ ] **Step 2: Construct the widgets**

In `src/app.rs`, immediately after the `speed_toast` construction block (the
line `authorship_picker.overlay.add_overlay(&speed_toast);`, currently 1252),
add:

```rust
    let search_edge_toast_left = gtk4::Label::new(None);
    search_edge_toast_left.set_valign(gtk4::Align::End);
    search_edge_toast_left.set_halign(gtk4::Align::Start);
    search_edge_toast_left.set_margin_bottom(32);
    search_edge_toast_left.set_margin_start(24);
    search_edge_toast_left.add_css_class("chapter-toast");
    search_edge_toast_left.set_visible(false);
    authorship_picker.overlay.add_overlay(&search_edge_toast_left);

    let search_edge_toast_right = gtk4::Label::new(None);
    search_edge_toast_right.set_valign(gtk4::Align::End);
    search_edge_toast_right.set_halign(gtk4::Align::End);
    search_edge_toast_right.set_margin_bottom(32);
    search_edge_toast_right.set_margin_end(24);
    search_edge_toast_right.add_css_class("chapter-toast");
    search_edge_toast_right.set_visible(false);
    authorship_picker.overlay.add_overlay(&search_edge_toast_right);
```

- [ ] **Step 3: Assign them in the struct literal**

In `src/app.rs`, in the `AppState { ... }` literal, immediately after
`speed_toast,` (currently line 1498), add:

```rust
        chapter_toast,
        speed_toast,
        search_edge_toast_left,
        search_edge_toast_right,
```

(Leave the existing `chapter_toast,` line in place — shown here only for
position. Do not duplicate it.)

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: builds (unused-field warnings acceptable; later tasks use them).

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(search): add left/right edge-toast widgets"
```

---

### Task 3: Add SearchBar::set_text

**Files:**
- Modify: `src/ui/search_bar.rs` (add method in the `impl SearchBar` block)

- [ ] **Step 1: Add the method**

In `src/ui/search_bar.rs`, inside `impl SearchBar`, immediately after the
`query` method (currently ends line 66), add:

```rust
    /// Set the entry text without showing/hiding the bar. Used to pre-fill the
    /// MRU pattern when n/N reactivates search outside search mode.
    pub fn set_text(&self, text: &str) {
        self.entry.set_text(text);
    }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: builds (unused-method warning acceptable until Task 6 uses it).

- [ ] **Step 3: Commit**

```bash
git add src/ui/search_bar.rs
git commit -m "feat(search): add SearchBar::set_text for MRU prefill"
```

---

### Task 4: Store MRU in execute_search

**Files:**
- Modify: `src/input/search.rs:11-22` (the `execute_search` query handling)

- [ ] **Step 1: Store the MRU pattern on non-empty query**

In `src/input/search.rs`, the start of `execute_search` currently reads:

```rust
    let mut state = state_rc.borrow_mut();
    let query = state.search_bar.query();

    clear_highlights(&state);
    state.search_matches.clear();
    state.search_match_idx = 0;

    if query.is_empty() {
        state.search_bar.update_counter(0, 0);
        return;
    }
```

Replace it with (add the `last_search_query` store before the empty check):

```rust
    let mut state = state_rc.borrow_mut();
    let query = state.search_bar.query();

    clear_highlights(&state);
    state.search_matches.clear();
    state.search_match_idx = 0;

    if query.is_empty() {
        state.search_bar.update_counter(0, 0);
        return;
    }

    // Remember the pattern as MRU so n/N can reactivate search after Escape.
    state.last_search_query = Some(query.to_string());
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add src/input/search.rs
git commit -m "feat(search): record MRU pattern in execute_search"
```

---

### Task 5: Extract goto_match_idx with canonical landing; stop at ends with edge toasts

**Files:**
- Modify: `src/input/search.rs` (`next_match` lines 152-166, `prev_match` lines 168-183; add helpers)

- [ ] **Step 1: Add the Side enum and edge_toast helper**

In `src/input/search.rs`, in the `// --- internal helpers ---` section (after
`push_page_back_dedup`, currently around line 211), add:

```rust
#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

/// Show the left/right search-edge toast for 3s ("no earlier/later
/// occurrence"), mirroring show_chapter_toast's auto-hide.
fn edge_toast(state: &AppState, side: Side, query: &str) {
    let (label, text) = match side {
        Side::Left => (
            &state.search_edge_toast_left,
            format!("No earlier occurrence of \u{201c}{}\u{201d}", query),
        ),
        Side::Right => (
            &state.search_edge_toast_right,
            format!("No later occurrence of \u{201c}{}\u{201d}", query),
        ),
    };
    label.set_text(&text);
    label.set_visible(true);
    let toast = label.clone();
    gtk4::glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
        toast.set_visible(false);
    });
}
```

- [ ] **Step 2: Add goto_match_idx (canonical landing)**

In `src/input/search.rs`, in the same helpers section, add:

```rust
/// Move the current match to `new_idx` and land on the canonical spread for
/// that line. Mirrors navigation::jump_to_line: if the target line is already
/// fully visible on the current spread, move the cursor/highlight only (no
/// re-pagination); otherwise land on canonical_page_top_for. Also seeks +
/// resumes MPV at the matched line.
fn goto_match_idx(state: &mut AppState, new_idx: usize) {
    let total = state.search_matches.len();
    if total == 0 {
        return;
    }
    remove_current_highlight(state);
    state.search_match_idx = new_idx.min(total - 1);
    let line = state.search_matches[state.search_match_idx].line_index;
    state.current_line = line;
    apply_current_highlight(state);
    state
        .search_bar
        .update_counter(state.search_match_idx, total);
    push_page_back_dedup(state);

    if crate::input::viewport::is_line_fully_visible(state, line) {
        // Already on the current spread — move cursor/highlight only, no flash.
        crate::input::highlight::update_highlight(state);
    } else {
        match state.config.navigation_mode {
            crate::config::NavigationMode::Scroll => {
                crate::input::scroll::center_cursor(state)
            }
            crate::config::NavigationMode::EReader => {
                let top = crate::input::navigation::canonical_page_top_for(state, line);
                crate::input::scroll::set_page_instant(state, top);
            }
        }
    }
    seek_and_resume(state);
}
```

- [ ] **Step 3: Rewrite next_match to stop at the end**

In `src/input/search.rs`, replace the entire `next_match` function (currently
lines 152-166) with:

```rust
/// Jump to next match. Does NOT wrap: at the last match, show the right edge
/// toast and stay put.
pub fn next_match(state: &mut AppState) {
    let total = state.search_matches.len();
    if total == 0 {
        return;
    }
    if state.search_match_idx + 1 >= total {
        let q = state.last_search_query.clone().unwrap_or_default();
        edge_toast(state, Side::Right, &q);
        return;
    }
    goto_match_idx(state, state.search_match_idx + 1);
}
```

- [ ] **Step 4: Rewrite prev_match to stop at the start**

In `src/input/search.rs`, replace the entire `prev_match` function (currently
lines 168-183) with:

```rust
/// Jump to previous match. Does NOT wrap: at the first match, show the left
/// edge toast and stay put.
pub fn prev_match(state: &mut AppState) {
    let total = state.search_matches.len();
    if total == 0 {
        return;
    }
    if state.search_match_idx == 0 {
        let q = state.last_search_query.clone().unwrap_or_default();
        edge_toast(state, Side::Left, &q);
        return;
    }
    goto_match_idx(state, state.search_match_idx - 1);
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: builds clean (no more wrap modulo; `update_highlight_and_center`
import in `next_match`/`prev_match` is gone but still used by `execute_search`,
so no unused-import error).

- [ ] **Step 6: Run the pure-logic test suite**

Run: `cargo test --bins`
Expected: PASS (no behavior covered here regresses; this confirms the crate
still builds under the test profile).

- [ ] **Step 7: Commit**

```bash
git add src/input/search.rs
git commit -m "feat(search): no-wrap n/N with edge toasts + canonical-spread landing"
```

---

### Task 6: Add reactivate_and_step and route n/N through it

**Files:**
- Modify: `src/input/search.rs` (add `reactivate_and_step`)
- Modify: `src/input/keymap.rs:1536-1549` (SearchNextMatch / SearchPrevMatch arms)

- [ ] **Step 1: Add reactivate_and_step**

In `src/input/search.rs`, after `prev_match` (and before `seek_and_resume`),
add:

```rust
/// Entry point for n / N pressed in reader mode (concordance already handled by
/// the caller). If matches are live, step within them. Otherwise, if an MRU
/// pattern exists, reactivate search against the current work and land on the
/// first match at/after (n) or last match at/before (N) the cursor.
pub fn reactivate_and_step(state_rc: &Rc<RefCell<AppState>>, forward: bool) {
    // Live matches: just step (handles its own end-of-list edge toasts).
    if !state_rc.borrow().search_matches.is_empty() {
        let mut state = state_rc.borrow_mut();
        if forward {
            next_match(&mut state);
        } else {
            prev_match(&mut state);
        }
        return;
    }

    // No live matches — try to reactivate from MRU.
    let mru = state_rc.borrow().last_search_query.clone();
    let mru = match mru {
        Some(p) if !p.is_empty() => p,
        _ => return, // nothing to reactivate
    };

    // Pre-fill the entry so execute_search's query() reads the MRU pattern,
    // then collect matches + apply full highlights WITHOUT execute_search's
    // auto-navigate (we seed the index from the cursor ourselves below).
    state_rc.borrow().search_bar.set_text(&mru);
    collect_matches(state_rc);

    let mut state = state_rc.borrow_mut();
    let total = state.search_matches.len();
    if total == 0 {
        // Pattern has no matches in this work; empty counter is the feedback.
        state.search_bar.update_counter(0, 0);
        return;
    }

    let cur = state.current_line;
    if forward {
        match state
            .search_matches
            .iter()
            .position(|m| m.line_index >= cur)
        {
            Some(idx) => goto_match_idx(&mut state, idx),
            None => {
                let q = state.last_search_query.clone().unwrap_or_default();
                edge_toast(&state, Side::Right, &q);
            }
        }
    } else {
        match state
            .search_matches
            .iter()
            .rposition(|m| m.line_index <= cur)
        {
            Some(idx) => goto_match_idx(&mut state, idx),
            None => {
                let q = state.last_search_query.clone().unwrap_or_default();
                edge_toast(&state, Side::Left, &q);
            }
        }
    }
}
```

- [ ] **Step 2: Add collect_matches (match collection + highlights, no navigate)**

This is the match-collection + `apply_highlights` portion of `execute_search`,
factored so reactivation can reuse it without the auto-navigate/pause. In
`src/input/search.rs`, in the helpers section, add:

```rust
/// Collect matches for the current search_bar query into state.search_matches
/// and apply the dim "all matches" highlight. Does NOT navigate, set the
/// current-match highlight, or touch MPV. Used by reactivate_and_step.
fn collect_matches(state_rc: &Rc<RefCell<AppState>>) {
    let mut state = state_rc.borrow_mut();
    let query = state.search_bar.query();

    clear_highlights(&state);
    state.search_matches.clear();
    state.search_match_idx = 0;

    if query.is_empty() {
        state.search_bar.update_counter(0, 0);
        return;
    }
    state.last_search_query = Some(query.to_string());

    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };
    let case_sensitive = query.chars().any(|c| c.is_uppercase());
    let mut new_matches: Vec<SearchMatch> = Vec::new();

    if state.line_map.is_some() {
        let text = state
            .buffer
            .text(&state.buffer.start_iter(), &state.buffer.end_iter(), false);
        for (line_idx, line_text) in text.as_str().lines().enumerate() {
            collect_line(line_text, &query, case_sensitive, line_idx, &mut new_matches);
        }
    } else {
        for (line_idx, line) in work.lines.iter().enumerate() {
            collect_line(&line.text, &query, case_sensitive, line_idx, &mut new_matches);
        }
    }

    state.search_matches = new_matches;
    apply_highlights(&state);
    state
        .search_bar
        .update_counter(0, state.search_matches.len());
}

/// Push every occurrence of `query` in `line_text` onto `out`, smart-cased.
fn collect_line(
    line_text: &str,
    query: &str,
    case_sensitive: bool,
    line_idx: usize,
    out: &mut Vec<SearchMatch>,
) {
    if case_sensitive {
        let mut search_start = 0;
        while let Some(pos) = line_text[search_start..].find(query) {
            let byte_start = search_start + pos;
            let byte_end = byte_start + query.len();
            out.push(SearchMatch { line_index: line_idx, byte_start, byte_end });
            search_start = byte_end;
        }
    } else {
        let text_lower = line_text.to_lowercase();
        let query_lower = query.to_lowercase();
        let mut search_start = 0;
        while let Some(pos) = text_lower[search_start..].find(&query_lower) {
            let byte_start = search_start + pos;
            let byte_end = byte_start + query_lower.len();
            out.push(SearchMatch { line_index: line_idx, byte_start, byte_end });
            search_start = byte_end;
        }
    }
}
```

- [ ] **Step 3: Refactor execute_search to reuse collect_line (DRY)**

The byte-offset find loops in `execute_search` (lines 41-92) now duplicate
`collect_line`. Replace the two inner `if case_sensitive { ... } else { ... }`
blocks in `execute_search` with calls to `collect_line`. The `execute_search`
collection section becomes:

```rust
    if state.line_map.is_some() {
        // Text file mode: search the buffer text directly
        let text = state.buffer.text(&state.buffer.start_iter(), &state.buffer.end_iter(), false);
        for (line_idx, line_text) in text.as_str().lines().enumerate() {
            collect_line(line_text, &query, case_sensitive, line_idx, &mut new_matches);
        }
    } else {
        // Original: search work.lines
        for (line_idx, line) in work.lines.iter().enumerate() {
            collect_line(&line.text, &query, case_sensitive, line_idx, &mut new_matches);
        }
    }
```

Leave the rest of `execute_search` (the `state.search_matches = new_matches;`,
`apply_highlights`, the auto-navigate to first match ≥ current_line, the MPV
pause) unchanged — live typing still auto-navigates as before.

- [ ] **Step 4: Route n/N through reactivate_and_step in keymap.rs**

In `src/input/keymap.rs`, replace the `SearchNextMatch` and `SearchPrevMatch`
arms (currently lines 1536-1549) with:

```rust
        // Search / concordance-in-work (n/p)
        SearchNextMatch => {
            if state.borrow().concordance_state.is_some() {
                crate::input::actions::concordance::concordance_next_in_work(state, tokio_handle);
            } else {
                crate::input::search::reactivate_and_step(state, true);
            }
        }
        SearchPrevMatch => {
            if state.borrow().concordance_state.is_some() {
                crate::input::actions::concordance::concordance_prev_in_work(state, tokio_handle);
            } else {
                crate::input::search::reactivate_and_step(state, false);
            }
        }
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 6: Run the pure-logic test suite + clippy**

Run: `cargo test --bins`
Expected: PASS.

Run: `cargo clippy`
Expected: no new errors (warnings tolerated if pre-existing in the file).

- [ ] **Step 7: Commit**

```bash
git add src/input/search.rs src/input/keymap.rs
git commit -m "feat(search): reactivate MRU search via n/N after Escape"
```

---

### Task 7: User on-screen verification (visual acceptance)

Per `CLAUDE.md`, landing/canonical-spread/toast positioning are visual-only and
cannot be verified by the agent from its own shell (the live dwl session owns
the seat). Hand off to the user.

- [ ] **Step 1: Confirm the build is green**

Run: `cargo build && cargo test --bins`
Expected: both succeed.

- [ ] **Step 2: Ask the user to verify on screen**

Ask the user to launch the reader (`cargo run`) and confirm:

1. `/` + a pattern present multiple times + Return, then `Escape`, then `n` →
   search reactivates, lands on the **canonical spread** containing the next
   match (same spread paging would show), with that match highlighted orange
   and all matches dim-yellow.
2. Press `n` repeatedly to the **last** match → a **right-aligned** toast "No
   later occurrence of …" appears and the view does **not** wrap to the top.
3. Press `N` to the **first** match → a **left-aligned** toast "No earlier
   occurrence of …" appears and the view does **not** wrap to the bottom.
4. With a match already visible on the current spread ahead of the cursor,
   pressing `n` moves the highlight to it **without a re-layout flash**.
5. Switch works with `Ctrl+p`, then press `n` → search re-runs the MRU pattern
   against the new work (or shows the empty `[0/0]` counter if absent).

Alternatively, the headless manual launch from `CLAUDE.md` (Headless
Verification) can capture a screenshot of the reactivated spread + toast.

- [ ] **Step 3: (no commit — verification only)**

---

## Self-Review

**Spec coverage:**
- MRU pattern persists session-wide → Task 1 (field), Task 4 (store), not cleared by `clear_search` (left untouched in Task 5/6). ✓
- n/N after Escape re-runs MRU + rebuilds highlights → Task 6 `reactivate_and_step` + `collect_matches` (calls `apply_highlights`). ✓
- No wrap; right toast on n-end, left toast on N-start; applies to active and reactivated → Task 5 (`next_match`/`prev_match`), Task 6 (reactivation None branches). ✓
- First target at/after cursor for n, at/before for N → Task 6 `position`/`rposition`. ✓
- Canonical-spread landing; same-spread move with no flash → Task 5 `goto_match_idx` (`is_line_fully_visible` branch + `canonical_page_top_for`). ✓
- Concordance priority unchanged → Task 6 Step 4 keeps the `concordance_state.is_some()` branch first. ✓
- MPV seek/resume unchanged → `goto_match_idx` calls `seek_and_resume`. ✓
- Edge case: MRU set, zero matches in work → Task 6 `total == 0` → empty counter, no toast. ✓
- Edge case: work switched → `collect_matches` re-runs against current work/buffer. ✓

**Placeholder scan:** No TBD/TODO; all code blocks complete. ✓

**Type consistency:** `goto_match_idx(state, new_idx)`, `edge_toast(state, side, query)`, `collect_line(line_text, query, case_sensitive, line_idx, out)`, `collect_matches(state_rc)`, `reactivate_and_step(state_rc, forward)`, `SearchBar::set_text(text)`, `Side::{Left,Right}`, fields `last_search_query` / `search_edge_toast_left` / `search_edge_toast_right` — used consistently across Tasks 1-6. `goto_match_idx` and `edge_toast` take `&mut AppState`/`&AppState` (borrowed inside `reactivate_and_step`'s `borrow_mut`), matching the `next_match`/`prev_match` `&mut AppState` signature. ✓
