# F3 + F4 + F8: Relocate Event, Visible-Range Cache, Page-Top Index

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Three sequential phases (F3 → F4 → F8); each ends in a commit + manual verification gate.

**Goal:** Bring linux-lit's pagination structure into alignment with foliate-js on three more pivots: a single `after_page_change` rendezvous (mirrors `relocate`), a cached `last_visible_range` (mirrors `#lastVisibleRange`), and an indexed `page_tops` cache (replaces the O(line_count²) `viewport_page_for_line` walk). Doing F3 first establishes the rendezvous; F4 and F8 each register one new responsibility inside it.

**Architecture:**

*F3 — `after_page_change`.* A `PageChangeReason` enum + an `after_page_change(state, reason)` function called as the tail of every page-mutating operation. Consumers that today are scattered (page label, vocab popup auto-show, bookmark glyph repaint, MPV seek) move into this function in a deterministic order. The 21 page-mutating entry points each get one `after_page_change(state, REASON)` call instead of N scattered concerns.

*F4 — `last_visible_range` cache.* AppState gets `last_visible_range: Cell<Option<VisibleRange>>`. `snap_scroll_to_line` populates it synchronously immediately after `adj.set_value(y)`; `is_line_fully_visible` reads from cache when available (cold-start fallback recomputes). Cache invalidated by `after_page_change` writing `None` for any reason that shifts the viewport; refilled on the next `snap_scroll_to_line`. Eliminates the idle-callback gap where MPV sync handlers read stale state.

*F8 — `page_tops` index.* AppState gets `page_tops: RefCell<Option<Vec<usize>>>` — a sorted vector of viewport-page top line indices for the current work. Built lazily on first need (typically by `viewport_page_for_line`); invalidated via `after_page_change` when font or work changes; queried via `binary_search` in O(log n). Replaces the current O(n²) walk-from-line-0 every overlay-label refresh.

The three caches share one invalidation rule: anything that breaks current viewport metrics (font/size change, work load) clears them; everything else (page turns) updates `last_visible_range` (correct value known) but is a no-op for `page_tops` (the index doesn't change as the user pages — it's a property of the work's geometry, not the current page).

**Tech Stack:** Rust 2021, GTK4 0.9 + libadwaita 0.7 + sourceview5 0.9. AppState in `Rc<RefCell<AppState>>`, single-threaded GTK main loop. New types: `PageChangeReason` enum, expansions to AppState fields.

**Source of findings:** `docs/reviews/2026-04-28-pagination-vs-references.md` F3, F4, F8.

**Verification model:** Pure-Rust unit tests for the `PageChangeReason` enum and `page_tops` index helpers. `cargo build` + manual smoke test for the GTK-bound integration. Each phase ends in a commit and a manual verification gate; no F4 work begins until F3 is verified, no F8 until F4 is verified.

**Out of scope (each gets its own future plan):**
- F5 already shipped; F6 closed-without-action; F7 (backward fallback `prev_page_top`) — independent
- F9 (block-atom rule for verse stanzas) — depends on F2's `visible_range` but not F3/F4/F8
- F10 (view-trait dispatch refactor) — keymap, not pagination

---

## File Map

- **Modify:** `src/app.rs` — add `last_visible_range` and `page_tops` fields to `AppState` struct; initialize in constructor; reset them in `display_work` when work changes.
- **Modify:** `src/input/navigation.rs`:
  - Add `PageChangeReason` enum and `after_page_change(state, reason)` function near `PageTurnLock` (around line 1003 area).
  - Add `pub fn invalidate_page_tops(state)` and `pub fn page_tops(state) -> Ref<Vec<usize>>` (or pure helpers + caller wrappers).
  - Modify the 21 page-mutating entry points to end in `after_page_change(state, reason)`.
  - Modify `snap_scroll_to_line` to write `state.last_visible_range` synchronously (don't depend on the idle callback for cache population).
  - Modify `is_line_fully_visible` to consult `state.last_visible_range` before recomputing.
  - Modify `viewport_page_for_line` to use `page_tops` binary search.
  - Modify `update_bottom_clip` to write the cache too (the idle path also populates it as a backstop).
- **No new files.** All additions live in `navigation.rs` next to their existing peers.
- **Tests:** Append `#[cfg(test)] mod after_page_change_tests` and `#[cfg(test)] mod page_tops_tests` after the existing `visible_range_helpers_tests` mod.

---

## Manual Verification Protocol (used after each phase)

```
1. cargo build (must succeed; warnings only).
2. cargo run.
3. Open a long prose work via Ctrl+p (Bleak House preferred).
4. Page through 10–20 pages with x.
   - Page label updates correctly on every turn.
   - No skipped or duplicate pages.
   - Vocab popup auto-shows still works.
   - Bookmark glyph (★) still appears for bookmarked lines.
5. Toggle a bookmark on (m), navigate away, navigate back via Ctrl+m picker.
   - Bookmark jump lands correctly; page label refreshes.
6. Open a play (Troilus and Cressida) via Ctrl+p.
   - Cycle scenes (2 / 3) — page label shows act.scene.line.
7. Start MPV playback (s) and verify sync still drives page turns cleanly.
8. Cycle font (f / F) and adjust size (Ctrl+= / Ctrl+-).
   - Pages re-paginate; descenders clean.
   - Page label still shows correct page-of-N for prose.
9. Confirm: 'verified' or describe any regression.
```

After each commit in each phase, paste this protocol and stop.

---

# Phase 1 — F3: `after_page_change` rendezvous

## Task 1.1: Add `PageChangeReason` enum and `after_page_change` function (no callers yet)

**Files:**
- Modify: `src/input/navigation.rs` — add enum + function near `PageTurnLock`. Append `#[cfg(test)] mod after_page_change_tests` at end of file.

The new function consolidates four concerns currently scattered across 21 callers:
1. **Page label** — `page_label_text_for_buffer + set_text + set_visible(true)`. Currently inlined in `set_page_instant`, `snap_scroll_to_line`, and `jump_to_line`.
2. **Vocab popup auto-show** — `auto_show_vocab_popup(state)`. Currently inlined in 12+ places.
3. **MPV seek** — `seek_to_current_line(state)` for navigation that should drag audio. Currently in 14+ places.
4. **Highlight repaint** — `update_highlight(state)`. Currently in 17+ places.

The reason enum lets each consumer opt out for kinds of page change where it shouldn't fire (e.g., `Resnap` doesn't reset MPV; `MpvSync` shouldn't trigger another seek; `WorkLoad` skips most consumers because the work is being torn down and rebuilt).

- [ ] **Step 1: Write the failing test**

Append to `src/input/navigation.rs` after the existing `visible_range_helpers_tests` mod:

```rust
#[cfg(test)]
mod after_page_change_tests {
    use super::PageChangeReason;

    #[test]
    fn reason_drives_seek_for_user_navigation() {
        assert!(PageChangeReason::Forward.should_seek());
        assert!(PageChangeReason::Backward.should_seek());
        assert!(PageChangeReason::JumpToLine.should_seek());
        assert!(PageChangeReason::JumpToBookmark.should_seek());
        assert!(PageChangeReason::Chapter.should_seek());
        assert!(PageChangeReason::Scene.should_seek());
    }

    #[test]
    fn reason_skips_seek_for_system_driven_changes() {
        assert!(!PageChangeReason::MpvSync.should_seek(),
            "MPV-driven page change must not re-seek MPV");
        assert!(!PageChangeReason::Resnap.should_seek(),
            "resnap is a layout refresh, not a navigation");
        assert!(!PageChangeReason::WorkLoad.should_seek(),
            "work load drives its own seek separately");
    }

    #[test]
    fn reason_drives_vocab_popup_for_user_navigation() {
        assert!(PageChangeReason::Forward.should_show_vocab());
        assert!(PageChangeReason::JumpToBookmark.should_show_vocab());
    }

    #[test]
    fn reason_skips_vocab_for_system_changes() {
        assert!(!PageChangeReason::MpvSync.should_show_vocab());
        assert!(!PageChangeReason::Resnap.should_show_vocab());
        assert!(!PageChangeReason::WorkLoad.should_show_vocab());
    }

    #[test]
    fn reason_always_updates_label_except_workload() {
        // Page label updates for every navigation; only work-load skips
        // (display_work owns label setup).
        assert!(PageChangeReason::Forward.should_update_label());
        assert!(PageChangeReason::MpvSync.should_update_label());
        assert!(PageChangeReason::Resnap.should_update_label());
        assert!(!PageChangeReason::WorkLoad.should_update_label());
    }
}
```

- [ ] **Step 2: Run test, verify it fails to compile**

```bash
cd /home/mlj/utono/linux-lit && cargo test after_page_change_tests 2>&1 | tail -10
```

Expected: `cannot find type 'PageChangeReason' in this scope`.

- [ ] **Step 3: Implement `PageChangeReason` and `after_page_change`**

In `src/input/navigation.rs`, after the `PageTurnLock` impl block (around line 1031), add:

```rust
/// Why the viewport's page changed. Drives which consumers fire inside
/// `after_page_change`. Mirrors the `reason` field on foliate-js's `relocate`
/// CustomEvent (paginator.js:952-969).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PageChangeReason {
    /// User pressed page-forward (x, Ctrl+d, Space).
    Forward,
    /// User pressed page-backward (y, Shift+,).
    Backward,
    /// User jumped to a specific line (gg, G, jump-to-bookmark via picker).
    JumpToLine,
    /// User toggled a bookmark and we're refreshing the cursor on it.
    JumpToBookmark,
    /// User jumped to a chapter via [ ] keys.
    Chapter,
    /// User jumped to a scene via 2 / 3 keys (plays).
    Scene,
    /// User jumped to a vocab match.
    Vocab,
    /// User pressed comma/q/j/k for dialogue navigation.
    Dialogue,
    /// User pressed [ or { for paragraph navigation.
    Paragraph,
    /// MPV CursorSync drove the cursor to a new line; do NOT re-seek MPV.
    MpvSync,
    /// Layout refresh after font/size/translation change. Not a navigation.
    Resnap,
    /// Work just loaded; AppState is being initialized. Skip most consumers.
    WorkLoad,
}

impl PageChangeReason {
    /// Whether to call `seek_to_current_line` after the page change. False for
    /// MPV-driven changes (would loop) and pure layout refreshes.
    pub(crate) fn should_seek(self) -> bool {
        !matches!(self, Self::MpvSync | Self::Resnap | Self::WorkLoad)
    }

    /// Whether to call `auto_show_vocab_popup` after the page change. False
    /// for system-driven changes that the user didn't request.
    pub(crate) fn should_show_vocab(self) -> bool {
        !matches!(self, Self::MpvSync | Self::Resnap | Self::WorkLoad)
    }

    /// Whether to refresh the page label. True for everything except WorkLoad
    /// (display_work handles label setup itself).
    pub(crate) fn should_update_label(self) -> bool {
        !matches!(self, Self::WorkLoad)
    }
}

/// Single rendezvous called at the tail of every page-mutating function.
/// Mirrors the listener pattern around foliate-js's `relocate` CustomEvent
/// (paginator.js:952-969): one canonical "page changed" signal that all
/// consumers (page label, vocab popup, MPV seek) project from in a
/// deterministic order.
///
/// Each consumer consults the reason flags so the function shape is
/// the same for every caller — the differences are in the reason, not in
/// scattered if/else around the call sites.
pub(crate) fn after_page_change(state: &mut AppState, reason: PageChangeReason) {
    if reason.should_update_label() {
        if let Some(text) = state.page_label_text_for_buffer(state.page_top_line) {
            state.page_line_label.set_text(&text);
            state.page_line_label.set_visible(true);
        }
    }

    // Highlight always repaints — consumer order matters: highlight first so
    // downstream consumers (vocab popup positioning) see the new cursor.
    update_highlight(state);

    if reason.should_seek() {
        seek_to_current_line(state);
    }

    if reason.should_show_vocab() {
        auto_show_vocab_popup(state);
    }
}
```

- [ ] **Step 4: Run tests, verify pass**

```bash
cd /home/mlj/utono/linux-lit && cargo test after_page_change_tests 2>&1 | tail -10
```

Expected: `test result: ok. 5 passed`.

- [ ] **Step 5: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: compiles; `after_page_change` and `PageChangeReason` warn as `dead_code` (will clear in Task 1.2 when call sites switch to use them).

- [ ] **Step 6: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/navigation.rs && git commit -m "$(cat <<'EOF'
Add PageChangeReason enum + after_page_change rendezvous

Mirrors foliate-js's `relocate` CustomEvent (paginator.js:952-969):
single canonical "page changed" signal with a reason flag that drives
which consumers fire. Currently unused; Task 1.2 wires the 21 page-
mutating entry points to call it instead of inlining their own page-
label / seek / vocab / highlight logic.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 1.2: Wire the 21 entry points to call `after_page_change`

**Files:**
- Modify: `src/input/navigation.rs` — replace the scattered `update_highlight + scroll + seek + auto_show_vocab_popup + page_label` blocks at the tail of each page-mutating function with a single `after_page_change(state, REASON)` call.
- Modify: `src/main.rs` — the MPV CursorSync handler currently calls `update_highlight_and_ensure_visible` and friends inline; route through `after_page_change(state, MpvSync)`.

Each entry point currently ends with some subset of:
```rust
update_highlight(state);
scroll_after_jump_forward(state, prev_line);  // or center_cursor / set_page_instant
seek_to_current_line(state);
auto_show_vocab_popup(state);
```

The `scroll/page` part stays caller-specific (different navigations want different scroll behaviors). The other four — highlight, label, seek, vocab — move inside `after_page_change`.

The function-by-function transformation is mechanical but extensive. Each entry point gets its old tail (everything from `update_highlight` onward) replaced with the appropriate `after_page_change` call.

- [ ] **Step 1: Update `page_forward` (lines 207–229)**

Find it:
```bash
cd /home/mlj/utono/linux-lit && grep -n "^pub fn page_forward" src/input/navigation.rs
```

The function currently ends:
```rust
    state.current_line = next_dialogue;
    update_highlight(state);
    seek_to_current_line(state);
    set_page(state, new_top, PageDirection::Forward);
    auto_show_vocab_popup(state);
}
```

Replace those last 5 lines with:
```rust
    state.current_line = next_dialogue;
    set_page(state, new_top, PageDirection::Forward);
    after_page_change(state, PageChangeReason::Forward);
}
```

The order shift (set_page before after_page_change) is intentional: page_top_line must be the new top before page_label_text_for_buffer reads it.

- [ ] **Step 2: Update `page_backward` (lines 236–270)**

Currently ends with:
```rust
    state.current_line = next;
    update_highlight(state);
    seek_to_current_line(state);
    set_page(state, new_top, PageDirection::Backward);
    auto_show_vocab_popup(state);
}
```

Replace with:
```rust
    state.current_line = next;
    set_page(state, new_top, PageDirection::Backward);
    after_page_change(state, PageChangeReason::Backward);
}
```

- [ ] **Step 3: Update `cursor_to_page_bottom` (lines 272–286)**

Currently ends with `update_highlight + seek + auto_show_vocab_popup`. Replace:

```rust
pub fn cursor_to_page_bottom(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let last_vis = last_fully_visible_line(state, state.page_top_line);
    if state.current_line != last_vis {
        state.current_line = last_vis;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        after_page_change(state, PageChangeReason::Dialogue);
    }
}
```

- [ ] **Step 4: Update `page_backward_bottom` (lines 288–309)**

Replace tail (`update_highlight + seek + auto_show_vocab_popup`) with `after_page_change(state, PageChangeReason::Backward)`.

- [ ] **Step 5: Update `jump_to_prev_dialogue` (lines 311–329)**

Replace tail (`update_highlight + scroll_after_jump_backward + seek + auto_show_vocab_popup`) with:
```rust
        scroll_after_jump_backward(state);
        after_page_change(state, PageChangeReason::Dialogue);
```

`scroll_after_jump_backward` stays — it's the caller-specific scroll concern.

- [ ] **Step 6: Update `jump_to_next_dialogue` (lines 331–349)**

Same pattern as Step 5: replace `update_highlight + scroll_after_jump_forward + seek + auto_show_vocab_popup` with `scroll_after_jump_forward(state, prev_line); after_page_change(state, PageChangeReason::Dialogue);`.

- [ ] **Step 7: Update `cursor_prev_line` (lines 351–368)**

This one currently does `update_highlight + scroll_after_jump_backward + auto_show_vocab_popup` (NO seek — the function-name suggests cursor-only). Use `Dialogue` reason but verify in the function body what it does. The reason-flag-driven `after_page_change` will skip seek if reason says so — but for cursor-only navigation we DO want seek (it drags audio). Compare the original behavior: this one doesn't call `seek_to_current_line`. So we need a new reason `CursorOnly` that doesn't seek? Actually look at the function's current body — it skips seek deliberately. Use a new `Cursor` reason that doesn't seek.

**Add a `Cursor` variant to `PageChangeReason`** (in `src/input/navigation.rs` where you defined the enum in Task 1.1 Step 3). Insert after `Dialogue`:

```rust
    /// User pressed k/K for cursor-only movement (no audio seek).
    Cursor,
```

And update `should_seek` to:
```rust
    pub(crate) fn should_seek(self) -> bool {
        !matches!(self, Self::MpvSync | Self::Resnap | Self::WorkLoad | Self::Cursor)
    }
```

(This is a real plan-time discovery — the spec didn't anticipate it. Adding the variant is the right call.)

Add a unit test after the existing tests:
```rust
    #[test]
    fn reason_skips_seek_for_cursor_only_navigation() {
        assert!(!PageChangeReason::Cursor.should_seek(),
            "cursor-only navigation must not drag audio");
        assert!(PageChangeReason::Cursor.should_show_vocab(),
            "cursor navigation still shows vocab");
        assert!(PageChangeReason::Cursor.should_update_label());
    }
```

Now the `cursor_prev_line` rewrite:
```rust
pub fn cursor_prev_line(state: &mut AppState) {
    if state.current_line == 0 {
        return;
    }
    let buffer = &state.buffer;
    let Some(target) = prev_dialogue_line(buffer, &state.translation_lines, state.current_line)
    else {
        return;
    };
    state.current_line = target;
    state.pending_advance = None;
    state.pending_advance_ignore_bl = None;
    state.prev_highlight_line.set(None);
    scroll_after_jump_backward(state);
    after_page_change(state, PageChangeReason::Cursor);
}
```

- [ ] **Step 8: Update `cursor_next_dialogue` (lines 370–387)**

Same `Cursor` reason. Replace tail with `scroll_after_jump_forward(state, prev_line); after_page_change(state, PageChangeReason::Cursor);`.

- [ ] **Step 9: Update `jump_to_prev_paragraph` (lines 389–435) and `jump_to_next_paragraph` (lines 437–464)**

Both end with the same scroll/seek/vocab pattern. Replace tails with `after_page_change(state, PageChangeReason::Paragraph)`. Keep the caller-specific scroll calls (`set_page_instant`, `scroll_to_cursor`, `scroll_after_jump_forward`) — those are mode-aware decisions that don't belong in the rendezvous.

- [ ] **Step 10: Update `jump_to_prev_chapter` (lines 565–609) and `jump_to_next_chapter` (lines 611–659)**

Replace tails with `after_page_change(state, PageChangeReason::Chapter)`.

- [ ] **Step 11: Update `jump_to_prev_scene` (lines 661–711) and `jump_to_next_scene` (lines 713–744)**

Replace tails with `after_page_change(state, PageChangeReason::Scene)`.

- [ ] **Step 12: Update `next_bookmark` (lines 746–761) and `prev_bookmark` (lines 763–778)**

Both call `jump_to_line(state, idx)` which already does its own consumers. After Task 1.2's full pass, `jump_to_line` will end in `after_page_change(state, PageChangeReason::JumpToBookmark)` — no extra change needed in next/prev_bookmark themselves.

- [ ] **Step 13: Update `jump_to_line` (lines 780–800)**

This function:
```rust
pub fn jump_to_line(state: &mut AppState, buffer_line: usize) {
    let line_count = state.effective_line_count();
    if buffer_line >= line_count {
        return;
    }
    state.current_line = buffer_line;
    update_highlight(state);
    let top = page_turn_top(&state.buffer, buffer_line);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            state.page_history.push(state.page_top_line);
            set_page_instant(state, top);
        }
    }
    // Update page label with the target line (page top may be a blank spacer)
    if let Some(text) = state.page_label_text_for_buffer(buffer_line) {
        state.page_line_label.set_text(&text);
        state.page_line_label.set_visible(true);
    }
    seek_to_current_line(state);
}
```

Replace with:
```rust
pub fn jump_to_line(state: &mut AppState, buffer_line: usize) {
    let line_count = state.effective_line_count();
    if buffer_line >= line_count {
        return;
    }
    state.current_line = buffer_line;
    let top = page_turn_top(&state.buffer, buffer_line);
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            state.page_history.push(state.page_top_line);
            set_page_instant(state, top);
        }
    }
    // Page label uses the target line, not page_top, because page_top may
    // be a blank spacer. Override the label after after_page_change runs.
    after_page_change(state, PageChangeReason::JumpToBookmark);
    if let Some(text) = state.page_label_text_for_buffer(buffer_line) {
        state.page_line_label.set_text(&text);
        state.page_line_label.set_visible(true);
    }
}
```

The label override after `after_page_change` is intentional — `jump_to_line` has a different "what to show in the label" rule than the default page-top-based one.

- [ ] **Step 14: Update `jump_to_start` (lines 23–43) and `jump_to_end` (lines 46–60)**

Both end with `update_highlight + set_page_instant + seek_to_current_line`. Replace tails with `after_page_change(state, PageChangeReason::JumpToLine)`. Keep the `set_page_instant` calls — those are the caller-specific page positioning.

- [ ] **Step 15: Update `jump_to_next_vocab` and `jump_to_prev_vocab`**

```bash
cd /home/mlj/utono/linux-lit && grep -n "^pub fn jump_to_next_vocab\|^pub fn jump_to_prev_vocab" src/input/navigation.rs
```

Both end with the standard tail. Use `PageChangeReason::Vocab`. Apply the same pattern.

- [ ] **Step 16: Update the MPV sync path in `src/main.rs`**

Find the MPV CursorSync handler:
```bash
cd /home/mlj/utono/linux-lit && grep -n "update_highlight_and_ensure_visible\|scroll_paragraph_to_top\|MpvEvent::CursorSync" src/main.rs
```

After `scroll_paragraph_to_top` (around line 177) and the existing inline highlight/seek logic, the handler ends with calls to `update_highlight_and_ensure_visible` and friends. Route the post-sync work through `after_page_change(s, PageChangeReason::MpvSync)` instead of the existing `update_highlight_and_ensure_visible`. The `scroll_paragraph_to_top` call stays — that's the caller-specific scroll decision; only the post-scroll consumer cleanup moves into `after_page_change`.

Read the handler before editing — there's some translation-skip logic and `pending_advance_ignore_bl` handling that must stay where it is.

- [ ] **Step 17: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -15
```

Expected: compiles. Many `auto_show_vocab_popup`, `seek_to_current_line`, page-label-set-text inline calls should be GONE from the entry points (now centralized in `after_page_change`); `cargo build` should NOT warn about either function being dead — they're called from `after_page_change`.

- [ ] **Step 18: Run all tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -8
```

Expected: 88 pass + 6 new `after_page_change_tests` = 94 total. Same `mpv::client::tests::test_find_line_for_time` pre-existing failure.

The existing `page_turn_tests` mod simulates page-forward/backward against real Troilus text — those tests do NOT call `after_page_change` (they re-implement helpers). They MUST still pass — any regression means navigation behavior diverged.

- [ ] **Step 19: Manual verification (FIRST GATE)**

Paste the Manual Verification Protocol (top of plan) into chat. Stop and wait for the user.

Critical things to test:
- Page label updates on every navigation
- Vocab popup auto-shows on j/k/x/y page changes
- Bookmark jump still updates the page label correctly
- MPV playback sync still drives page turns and the page label updates correctly during playback
- Cursor-only navigation (k/K) does NOT drag MPV audio

If user reports a regression: revert with `git checkout src/input/navigation.rs src/main.rs`, diagnose. Common causes:
- Wrong reason at a call site (e.g., used `JumpToLine` where `Cursor` was correct, causing audio drag)
- Forgot a `set_page_instant` or `scroll_to_cursor` call (the scroll behavior is caller-specific, not part of `after_page_change`)
- Reason flag should_X function returns wrong value for some new edge case

- [ ] **Step 20: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/navigation.rs src/main.rs && git commit -m "$(cat <<'EOF'
Route 21 page-mutating entry points through after_page_change

Replaces the scattered update_highlight + page_label + seek + vocab_popup
tails at every page-mutating function with a single after_page_change call
keyed on a PageChangeReason. The reason drives which consumers fire so
the rendezvous shape is uniform: differences live in the reason, not
in scattered if/else around 20+ call sites.

Adds PageChangeReason::Cursor for k/K cursor-only nav (no audio seek)
which the original spec didn't anticipate.

The MPV CursorSync handler in main.rs routes through after_page_change
with reason=MpvSync; should_seek = false prevents the sync->seek loop.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Phase 2 — F4: `last_visible_range` cache

After Phase 1 commits and verifies, proceed to Phase 2.

## Task 2.1: Add `last_visible_range` to AppState; populate from `snap_scroll_to_line`

**Files:**
- Modify: `src/app.rs` — add `last_visible_range: std::cell::Cell<Option<crate::input::navigation::VisibleRange>>` field; initialize in constructor; reset to `None` in `display_work` and from `after_page_change` for any reason that shifts the viewport.
- Modify: `src/input/navigation.rs` — `snap_scroll_to_line` writes the cache synchronously; `is_line_fully_visible` reads it; `update_bottom_clip` writes it as a backstop.

The cache is `Cell<Option<VisibleRange>>` (not `RefCell`) because `VisibleRange: Copy` and writes are point-replacements, not in-place mutations. `Option` lets cold start return None; readers fall back to recompute.

- [ ] **Step 1: Add the field to AppState**

In `src/app.rs`, locate the `AppState` struct (around line 43, near `page_turn_lock`). Add:

```rust
    /// Cached last visible range from the most recent snap_scroll_to_line or
    /// update_bottom_clip. None during cold start, after work load, or after
    /// any after_page_change for a reason that shifts the viewport. Read by
    /// is_line_fully_visible to avoid recomputing through the height-summing
    /// loop on every MPV time-pos tick.
    /// Mirrors foliate-js Paginator.#lastVisibleRange.
    pub last_visible_range: std::cell::Cell<Option<crate::input::navigation::VisibleRange>>,
```

In the AppState constructor (around line 748), add:
```rust
        last_visible_range: std::cell::Cell::new(None),
```

In `display_work` (around line 1314 — find the line `state.page_top_line = 0;`), add right after:
```rust
    state.last_visible_range.set(None);
```

- [ ] **Step 2: Populate the cache from `snap_scroll_to_line`**

In `src/input/navigation.rs`, find `snap_scroll_to_line` (around line 1352). After the `adj.set_value(y as f64);` line and before the page label code, insert a synchronous `visible_range` call that populates the cache:

```rust
    // F4: populate the cache synchronously so MPV sync handlers reading
    // is_line_fully_visible right after this call see the new range, not
    // stale state. The idle-scheduled update_bottom_clip below ALSO writes
    // the cache as a backstop for layout-not-yet-flushed cases.
    let widget_height = state.text_view.height();
    if widget_height > 0 {
        let descender_guard = descender_guard_px(&state.text_view, line);
        let bottom_margin = state.text_view.bottom_margin();
        let usable_height = widget_height - descender_guard - bottom_margin;
        let line_count = state.effective_line_count();
        let range = visible_range(&state.text_view, &state.buffer, line, line_count, usable_height);
        let trimmed = trim_trailing_speakers(range, line, &state.text_view, &state.buffer);
        state.last_visible_range.set(Some(trimmed));
    } else {
        state.last_visible_range.set(None);
    }
```

- [ ] **Step 3: Have `update_bottom_clip` write the cache (backstop path)**

`update_bottom_clip` runs from idle/timeout backstops. It already computes `trimmed` internally. After the trim line and before the clip computation, add a cache write — but `update_bottom_clip` only has widget refs, not AppState. Two options:

**Option A:** Pass an optional cache writer through. Add a parameter `last_visible_cache: Option<&Cell<Option<VisibleRange>>>` to `update_bottom_clip`, default None. Update the three callers (the idle path in `snap_scroll_to_line`, the timeout path in `schedule_bottom_clip_update`, and `refresh_bottom_clip`) to pass `Some(&state.last_visible_range)` where they have AppState in scope.

**Option B:** Accept that the synchronous write in Step 2 is sufficient, and `update_bottom_clip` doesn't write the cache. Cache is populated synchronously; backstop only sizes the clip widget.

Go with **Option B** — simpler, fewer signature changes. The synchronous write in Step 2 covers the cache; the timeout backstop's job is just to resize the clip widget after layout flushes. They serve different purposes.

(No code change for this step beyond confirming Option B.)

- [ ] **Step 4: Have `is_line_fully_visible` read from cache**

In `src/input/navigation.rs`, find `is_line_fully_visible` (around line 836). Replace its body with:

```rust
fn is_line_fully_visible(state: &AppState, line: usize) -> bool {
    if state.loading_work.get() {
        return true;
    }
    if line < state.page_top_line {
        return false;
    }
    // F4: fast path — consult the cache populated by snap_scroll_to_line.
    if let Some(cached) = state.last_visible_range.get() {
        return line <= cached.last_fit && cached.count > 0;
    }
    // Cold-start fallback: recompute via visible_range.
    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return true;
    }
    let descender_guard = descender_guard_px(&state.text_view, state.page_top_line);
    let bottom_margin = state.text_view.bottom_margin();
    let usable_height = widget_height - descender_guard - bottom_margin;
    let line_count = state.effective_line_count();
    let range = visible_range(
        &state.text_view,
        &state.buffer,
        state.page_top_line,
        line_count,
        usable_height,
    );
    line <= range.last_fit && range.count > 0
}
```

- [ ] **Step 5: Invalidate cache from `after_page_change` for relevant reasons**

In `src/input/navigation.rs`, modify `after_page_change` (added in Task 1.1). Add at the top of the function, before the label check:

```rust
    // F4: cache shifts with viewport. Invalidate for any reason that
    // changes what's visible. snap_scroll_to_line repopulates after
    // any subsequent set_page / set_page_instant.
    if matches!(
        reason,
        PageChangeReason::Forward | PageChangeReason::Backward | PageChangeReason::JumpToLine
            | PageChangeReason::JumpToBookmark | PageChangeReason::Chapter
            | PageChangeReason::Scene | PageChangeReason::Vocab | PageChangeReason::Paragraph
            | PageChangeReason::MpvSync | PageChangeReason::Resnap
    ) {
        // Cache will be repopulated by the next snap_scroll_to_line call,
        // which most page-changing operations trigger via set_page or
        // set_page_instant. For Cursor and Dialogue (no page shift), keep
        // the cache.
    }
```

Wait — `Cursor` and `Dialogue` MAY shift the viewport (they call `scroll_after_jump_backward` / `scroll_after_jump_forward` which can page-turn). So the conditional is wrong. Simpler: **invalidate unconditionally**. The next `snap_scroll_to_line` (called from set_page_instant / set_page within the page-mutating function) repopulates. If no scroll happens, the next `is_line_fully_visible` recomputes via the cold-start fallback — slightly slower but correct.

Replace the `if matches!` block with simply:
```rust
    // F4: invalidate cache unconditionally; snap_scroll_to_line
    // repopulates if any scroll happened.
    state.last_visible_range.set(None);
```

Move this BEFORE the `should_update_label` check. That way the label code (which doesn't need the cache) and downstream consumers all run after invalidation.

- [ ] **Step 6: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: compiles. No new warnings.

- [ ] **Step 7: Run tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -5
```

Expected: 94 pass / 1 pre-existing fail.

- [ ] **Step 8: Manual verification (SECOND GATE)**

Paste the Manual Verification Protocol into chat. Stop. Test specifically:
- Pages turn correctly during playback (cache reads / writes via MPV sync path)
- No "is line on screen" misjudgments visible as missed page turns
- No descender clipping (cache value matches what the bottom clip uses)

- [ ] **Step 9: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/app.rs src/input/navigation.rs && git commit -m "$(cat <<'EOF'
Cache last_visible_range; populate synchronously from snap_scroll_to_line

Mirrors foliate-js Paginator.#lastVisibleRange (paginator.js:945-958):
the visible range is computed once after every scroll and cached on the
widget, so downstream consumers (is_line_fully_visible, MPV sync handlers)
read from cache instead of recomputing through the height-summing loop on
every time-pos tick.

Cache lives on AppState as Cell<Option<VisibleRange>>; populated
synchronously inside snap_scroll_to_line; invalidated by after_page_change.
Cold-start and post-invalidation reads fall back to visible_range.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Phase 3 — F8: `page_tops` index cache

After Phase 2 verifies, proceed to Phase 3.

## Task 3.1: Add `page_tops` index + `viewport_page_for_line` binary search

**Files:**
- Modify: `src/app.rs` — add `page_tops: std::cell::RefCell<Option<Vec<usize>>>` field; init to None; reset in `display_work`.
- Modify: `src/input/navigation.rs` — add `pub fn ensure_page_tops(state) -> impl Deref<Target=Vec<usize>>` (or similar) that computes-on-first-need; rewrite `viewport_page_for_line` to binary-search; add tests; invalidate from font/size/work-change.

`page_tops` is a property of the work's geometry at the current font/size, not the current scroll position. Once built, it doesn't change as the user pages — only when font/size changes or a new work loads.

- [ ] **Step 1: Write failing tests**

Append to `src/input/navigation.rs` after `after_page_change_tests`:

```rust
#[cfg(test)]
mod page_tops_tests {
    use super::page_for_line_in_index;

    #[test]
    fn page_for_line_returns_1_for_empty_index() {
        let tops: Vec<usize> = vec![];
        assert_eq!(page_for_line_in_index(&tops, 0), 1);
        assert_eq!(page_for_line_in_index(&tops, 100), 1);
    }

    #[test]
    fn page_for_line_returns_1_for_first_page() {
        let tops = vec![0, 35, 70, 105]; // page 1 starts at 0, page 2 at 35, etc.
        assert_eq!(page_for_line_in_index(&tops, 0), 1);
        assert_eq!(page_for_line_in_index(&tops, 10), 1);
        assert_eq!(page_for_line_in_index(&tops, 34), 1);
    }

    #[test]
    fn page_for_line_returns_2_for_second_page() {
        let tops = vec![0, 35, 70, 105];
        assert_eq!(page_for_line_in_index(&tops, 35), 2);
        assert_eq!(page_for_line_in_index(&tops, 50), 2);
        assert_eq!(page_for_line_in_index(&tops, 69), 2);
    }

    #[test]
    fn page_for_line_handles_target_past_end() {
        let tops = vec![0, 35, 70, 105];
        assert_eq!(page_for_line_in_index(&tops, 200), 4);
    }

    #[test]
    fn page_for_line_exact_top_match() {
        let tops = vec![0, 35, 70];
        // line 35 is the START of page 2 — partition_point gives index 2,
        // page = 2.
        assert_eq!(page_for_line_in_index(&tops, 35), 2);
    }
}
```

- [ ] **Step 2: Run tests, verify they fail**

```bash
cd /home/mlj/utono/linux-lit && cargo test page_tops_tests 2>&1 | tail -10
```

Expected: `cannot find function 'page_for_line_in_index'`.

- [ ] **Step 3: Implement the pure helper**

In `src/input/navigation.rs`, near `visible_range` (around line 1063), add:

```rust
/// Pure: given a sorted vec of viewport-page top line indices, return the
/// 1-indexed page that contains `target_line`. Empty index returns 1.
/// `target_line` past the last page-top returns `tops.len()`.
///
/// Mirrors what foliate-js paginator.js does in O(log n) via index lookup
/// (`atStart`/`atEnd` use page indices, paginator.js:1050-1054), replacing
/// linux-lit's previous O(n²) replay-from-line-0 walk.
pub(crate) fn page_for_line_in_index(tops: &[usize], target_line: usize) -> usize {
    if tops.is_empty() {
        return 1;
    }
    // partition_point returns the index of the first element > target_line.
    // Because tops[0] is always 0 (the first page), partition_point >= 1 for
    // any target_line >= 0 — that's exactly the page number we want.
    tops.partition_point(|&t| t <= target_line).max(1)
}
```

- [ ] **Step 4: Run tests, verify pass**

```bash
cd /home/mlj/utono/linux-lit && cargo test page_tops_tests 2>&1 | tail -10
```

Expected: 5 tests pass.

- [ ] **Step 5: Add the AppState field**

In `src/app.rs`, near `last_visible_range`, add:

```rust
    /// Cached vec of viewport-page top line indices for the current work at
    /// the current font/size. Built lazily on first need by ensure_page_tops;
    /// invalidated to None when font/size changes or a new work loads. The
    /// cache eliminates the O(line_count²) replay-from-line-0 walk that
    /// viewport_page_for_line used to do on every overlay-label refresh.
    pub page_tops: std::cell::RefCell<Option<Vec<usize>>>,
```

In the constructor:
```rust
        page_tops: std::cell::RefCell::new(None),
```

In `display_work` (right after `state.last_visible_range.set(None)`):
```rust
    *state.page_tops.borrow_mut() = None;
```

- [ ] **Step 6: Implement `ensure_page_tops` and rewrite `viewport_page_for_line`**

In `src/input/navigation.rs`, near `viewport_page_for_line` (around line 197), add:

```rust
/// Build the page_tops index by walking next_page_top from line 0 to the end
/// of the work. Result is the same as repeatedly calling next_page_top from
/// line 0; cost is O(line_count) once instead of O(line_count²) on every
/// overlay-label refresh.
fn build_page_tops(state: &AppState) -> Vec<usize> {
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return Vec::new();
    }
    let mut tops = vec![0usize];
    let mut top: usize = 0;
    while top < line_count {
        let next = next_page_top(state, top).new_top;
        if next <= top || next >= line_count {
            break;
        }
        tops.push(next);
        top = next;
    }
    tops
}
```

Replace `viewport_page_for_line` (around line 197) with:

```rust
/// Return the 1-indexed viewport page that contains `target_line`. Reads
/// from the page_tops cache; builds it on first need.
pub fn viewport_page_for_line(state: &AppState, target_line: usize) -> usize {
    {
        let cached = state.page_tops.borrow();
        if let Some(tops) = cached.as_ref() {
            return page_for_line_in_index(tops, target_line);
        }
    }
    // Cache miss — build, store, then look up.
    let tops = build_page_tops(state);
    let page = page_for_line_in_index(&tops, target_line);
    *state.page_tops.borrow_mut() = Some(tops);
    page
}

/// Drop the page_tops cache. Called when font/size changes invalidate page
/// boundaries (resnap_page) and when a new work loads (display_work).
pub fn invalidate_page_tops(state: &AppState) {
    *state.page_tops.borrow_mut() = None;
}
```

- [ ] **Step 7: Wire invalidation into font/size changes**

In `src/app.rs`, find `adjust_font_size`, `reset_font_size`, `cycle_font` (around lines 2286, 2298, 2310). After each `reapply_font(state); resnap_page(state);` pair, add:

```rust
    crate::input::navigation::invalidate_page_tops(state);
```

Also in `show_translations` and `hide_translations` (the translation toggle flows around lines 1940 and 2126), after each `reapply_font(state)`, add:
```rust
    crate::input::navigation::invalidate_page_tops(state);
```

(Translation toggle changes line heights → page tops shift.)

- [ ] **Step 8: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: compiles.

- [ ] **Step 9: Run all tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -5
```

Expected: 99 pass / 1 pre-existing fail. (94 from previous + 5 new `page_tops_tests`.)

- [ ] **Step 10: Manual verification (THIRD GATE)**

Paste the Manual Verification Protocol into chat. Stop. Test specifically:
- Open a long prose work (Bleak House) — confirm page label updates correctly on every turn.
- Cycle font sizes — confirm the page label updates to reflect the new pagination (e.g., page 1/100 → page 1/120 when font shrinks).
- Toggle translations — confirm page count updates.
- Confirm no perf regression: opening a long work and pressing x repeatedly should feel as snappy as before.

- [ ] **Step 11: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/app.rs src/input/navigation.rs && git commit -m "$(cat <<'EOF'
Cache page_tops index; viewport_page_for_line becomes O(log n) binary search

Replaces the O(line_count²) replay-from-line-0 walk inside
viewport_page_for_line with a binary search over a cached
Vec<usize> of page-top indices. Cache built lazily on first need;
invalidated when font/size changes or a new work loads.

Mirrors foliate-js Paginator's index-based atStart/atEnd checks
(paginator.js:1050-1054). Removes a perf cliff that scaled with work
length and inverse font size — overlay-label refresh on long prose at
small fonts is now constant-time.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Phase 4 — Final verification

- [ ] **Step 1: Confirm clean tree**

```bash
cd /home/mlj/utono/linux-lit && git status
```

Expected: `nothing to commit, working tree clean`.

- [ ] **Step 2: Confirm test suite**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | tail -10
```

Expected: 99 pass + 1 pre-existing fail.

- [ ] **Step 3: Confirm commit log**

```bash
cd /home/mlj/utono/linux-lit && git log --oneline -8
```

Expected order (most recent first):
1. `Cache page_tops index; viewport_page_for_line becomes O(log n) binary search`
2. `Cache last_visible_range; populate synchronously from snap_scroll_to_line`
3. `Route 21 page-mutating entry points through after_page_change`
4. `Add PageChangeReason enum + after_page_change rendezvous`
(plus prior session commits)

- [ ] **Step 4: User signoff**

Output to chat:

> "F3 + F4 + F8 implementation complete. Four commits on master. All three manual verification gates passed. Ready to push to origin, or continue with another finding (F7 backward fallback, F9 block-atom rule, F10 view-trait dispatch) — your call."

Do not push. Wait for the user.

---

## Self-Review

**Spec coverage:**
- F3: `PageChangeReason` enum + `after_page_change` rendezvous + 21 entry points wired through it + MPV sync handler routed via the rendezvous. ✓
- F4: `last_visible_range: Cell<Option<VisibleRange>>` field + synchronous write from `snap_scroll_to_line` + cache-read fast path in `is_line_fully_visible` + invalidation in `after_page_change`. ✓
- F8: `page_tops: RefCell<Option<Vec<usize>>>` field + `build_page_tops` + `viewport_page_for_line` rewritten to use `page_for_line_in_index` (binary search) + invalidation on font/size/translation changes. ✓

**Placeholder scan:** No "TBD" / "TODO" / "fill in later". Code blocks contain actual code. Manual Verification Protocol reproduced inline. ✓

**Type / API consistency:**
- `PageChangeReason` variants used: Forward, Backward, JumpToLine, JumpToBookmark, Chapter, Scene, Vocab, Dialogue, Cursor (added at Task 1.2 Step 7), Paragraph, MpvSync, Resnap, WorkLoad. 13 variants — each with consistent `should_*` semantics. ✓
- `VisibleRange` already exists from F2; `Cell<Option<VisibleRange>>` works because `VisibleRange: Copy`. ✓
- `page_tops: RefCell<Option<Vec<usize>>>` not Cell because Vec isn't Copy. ✓
- `page_for_line_in_index(&[usize], usize) -> usize` — pure function, testable on synthetic vecs. ✓
- `build_page_tops(&AppState) -> Vec<usize>` — uses `next_page_top` which already exists. ✓
- `invalidate_page_tops(&AppState)` and `viewport_page_for_line(&AppState, usize) -> usize` keep their public signatures. ✓

**Scope discipline:**
- Plan stays within F3+F4+F8. No drive-by refactors of unrelated functions.
- F4's cache write happens in one place (snap_scroll_to_line); F4's cache read happens in one place (is_line_fully_visible); cache invalidation happens in one place (after_page_change). Single-write-site / single-read-site / single-invalidation-site invariant holds.
- F8's index build happens in one place (build_page_tops); index read happens in one place (page_for_line_in_index); invalidation happens explicitly at the four font/size/translation/work-load sites. Single-build / single-read / four-invalidation-sites — explicitly counted.

**Notes for the executor:**
- Tasks 1.2 has 16 substeps because each entry point gets its own audit. DO NOT batch them — read each function's current tail before editing; the scroll/page calls vary and must stay caller-specific.
- Phase gates (Step 19 in Phase 1, Step 8 in Phase 2, Step 10 in Phase 3) are MANDATORY user verification. Do not proceed past a gate without explicit "verified".
- The new `Cursor` PageChangeReason variant in Task 1.2 Step 7 is a planning-time discovery, not in the original spec. The unit test added there documents the behavior. If you find more such variants needed during entry-point editing, add them with `should_*` definitions and a test, and document the discovery in your task report.
- Task 2.1 Step 5's "invalidate unconditionally" decision: discussed inline. The reasoning is that snap_scroll_to_line will repopulate, and if no scroll happened the cold-start path is correct (just a tiny perf cost). Don't second-guess this and try to be clever about which reasons need invalidation.
- Task 3.1 Step 6's `viewport_page_for_line` rewrite has a borrow-checker subtlety: the immutable borrow inside the `if let Some(tops)` block must end before the mutable borrow on the next line. The block scope `{ … }` handles this — don't simplify it to a one-liner.
