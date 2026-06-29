# Page-Turn Lock + Synchronous Clip Update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent race conditions where a second animated page-turn (typically MPV-driven `scroll_paragraph_to_top`) mutates `state.page_top_line` while the previous turn's snapshot animation is still on screen, and eliminate stale-state reads where consumers query `bottom_clip` height_request between scroll and the deferred `update_bottom_clip` idle callback.

**Architecture:** Two paired changes. (1) Add a `page_turn_locked: bool` flag to AppState, set true at the start of `set_page` and cleared in the animation's `connect_done`. The four `set_page` callers and any other animated-turn entry points consult the flag and early-return when locked. The instant-turn path (`set_page_instant`) is unaffected — no animation, no race. (2) Run `update_bottom_clip` synchronously inside `snap_scroll_to_line` after `adj.set_value(y)` instead of only via `glib::idle_add_local_once`. Cache the computed last-visible line on AppState as `last_visible_line: Cell<Option<usize>>` so MPV sync handlers read from cache instead of recomputing through three duplicate height-summing loops. Keep the idle re-run as a correctness backstop.

**Tech Stack:** Rust 2021, GTK4 0.9 + libadwaita 0.7 + sourceview5 0.9, single-threaded GTK main loop with Tokio runtime in a separate thread bridged via `glib::spawn_future_local`. AppState lives in `Rc<RefCell<AppState>>`. Animations are `adw::TimedAnimation` with `connect_done` callbacks.

**Source of findings:** `docs/reviews/2026-04-28-pagination-vs-references.md` F1 (re-entrancy lock) and F2 (synchronous clip + cache).

**Verification model:** linux-lit's project convention (per CLAUDE.md) is `cargo build` + manual smoke test by the user. The pure-logic helper added in Task 1 (the `PageTurnLock` state machine) gets a `#[cfg(test)]` unit test alongside the existing `page_turn_tests` mod. The GTK-integration changes in Tasks 2–5 use `cargo build` plus an explicit manual reproduction protocol the user runs.

**Out of scope:** F3 (descender guard via Pango), F4 (resize observer), F5 (backward fallback), F6 (block atoms), F7 (consolidate four height-summing loops), F8 (relocate event), F9 (page-top cache), F10 (view trait). Each is independent and should get its own plan.

---

## File Map

- **Modify:** `src/app.rs` — add `page_turn_locked: bool` and `last_visible_line: std::cell::Cell<Option<usize>>` to `AppState`; initialize both in the `AppState` constructor.
- **Modify:** `src/input/navigation.rs` — gate `set_page` on the lock; clear the lock from `connect_done` for both Crossfade and Slide branches; gate `set_page_instant` does NOT need the lock (instant); update `is_line_on_screen` to consult the cache; update `snap_scroll_to_line` to call `update_bottom_clip` synchronously and write to the cache; update `update_bottom_clip` to write to the cache.
- **No new files.** No new modules. Lock state machine is a small struct in `src/input/navigation.rs` so it lives next to the only code that uses it.
- **Test:** `#[cfg(test)] mod page_turn_lock_tests` appended to `src/input/navigation.rs` after the existing `page_turn_tests` mod.

---

## Manual Verification Protocol

Tasks 2–5 end with the user running this protocol. Until the user confirms success, do not proceed. The plan executor must paste this protocol into the chat at the verification step and stop, waiting for the user.

```
1. cargo build (must succeed with warnings only).
2. User starts the app: `cargo run`.
3. User opens a long prose work (e.g. Bleak House) via Ctrl+p.
4. User selects Crossfade transition in settings (Ctrl+,).
5. User starts MPV playback (s key) on a work with timestamps.
6. User holds Ctrl+d (or hammers x for page forward) for 5 seconds during playback.
7. Expected: every keypress lands on a clean page; no half-faded snapshot stuck on screen; no skipped lines on the line counter.
8. User checks `~/utono/linux-lit/linux-lit-dev.log` for `PAGE_TURN: SKIPPED (locked=true)` lines — these are expected when the lock blocks a second turn during animation.
9. User repeats steps 4–7 with Slide transition.
10. User confirms in chat: "verified" or describes the failure mode.
```

If the user reports a regression during the protocol, fix it before proceeding to the next task.

---

## Task 1: Add the `PageTurnLock` state machine with unit tests

**Files:**
- Modify: `src/input/navigation.rs` — add `PageTurnLock` struct near the existing `PageDirection` enum; add `#[cfg(test)] mod page_turn_lock_tests` at the end of the file.

The lock is a tiny state machine with three operations: `try_acquire` (true if it took the lock), `release` (clears it), and `is_locked` (peek). Keeping it as a dedicated struct rather than a bare `bool` makes the invariants testable in isolation and prevents AppState callers from forgetting to clear it on the failure path.

- [ ] **Step 1: Write the failing test**

Append to the very end of `src/input/navigation.rs`:

```rust
#[cfg(test)]
mod page_turn_lock_tests {
    use super::PageTurnLock;

    #[test]
    fn try_acquire_succeeds_when_unlocked() {
        let lock = PageTurnLock::new();
        assert!(lock.try_acquire(), "first acquire should succeed");
        assert!(lock.is_locked(), "lock should be held after acquire");
    }

    #[test]
    fn try_acquire_fails_when_locked() {
        let lock = PageTurnLock::new();
        assert!(lock.try_acquire());
        assert!(!lock.try_acquire(), "second acquire should fail");
        assert!(lock.is_locked(), "lock should still be held after rejected acquire");
    }

    #[test]
    fn release_clears_the_lock() {
        let lock = PageTurnLock::new();
        lock.try_acquire();
        lock.release();
        assert!(!lock.is_locked(), "release should clear the lock");
        assert!(lock.try_acquire(), "acquire should succeed after release");
    }

    #[test]
    fn release_when_unlocked_is_a_noop() {
        // Defensive: a stray release (e.g. animation done firing twice) must not panic.
        let lock = PageTurnLock::new();
        lock.release();
        lock.release();
        assert!(!lock.is_locked());
        assert!(lock.try_acquire());
    }

    #[test]
    fn double_release_does_not_re_lock() {
        let lock = PageTurnLock::new();
        lock.try_acquire();
        lock.release();
        lock.release();
        assert!(!lock.is_locked());
    }
}
```

- [ ] **Step 2: Run the test and verify it fails**

```bash
cd ~/utono/linux-lit && cargo test page_turn_lock_tests 2>&1 | tail -20
```

Expected: compilation error `cannot find type 'PageTurnLock' in this scope`.

- [ ] **Step 3: Implement `PageTurnLock`**

In `src/input/navigation.rs`, immediately after the existing `PageDirection` enum (around line 987), add:

```rust
/// Re-entrancy lock for animated page turns.
///
/// `set_page` calls `try_acquire` before mutating `page_top_line` or starting
/// an animation. The animation's `connect_done` callback calls `release`. While
/// locked, secondary turn requests (typically from MPV `CursorSync` arriving
/// mid-animation) are dropped so they don't compose with the in-flight turn.
///
/// `set_page_instant` does NOT consult the lock — it has no animation, so the
/// re-entrancy window doesn't exist for that path.
///
/// Uses `Cell<bool>` rather than `bool` so a `&PageTurnLock` borrow (which is
/// what AppState consumers will hold through `state.page_turn_lock`) can mutate
/// it without a `&mut AppState`.
pub(crate) struct PageTurnLock {
    locked: std::cell::Cell<bool>,
}

impl PageTurnLock {
    pub(crate) fn new() -> Self {
        Self { locked: std::cell::Cell::new(false) }
    }

    /// Attempt to take the lock. Returns true if acquired, false if already held.
    pub(crate) fn try_acquire(&self) -> bool {
        if self.locked.get() {
            false
        } else {
            self.locked.set(true);
            true
        }
    }

    /// Release the lock. Idempotent — releasing when unlocked is a no-op.
    pub(crate) fn release(&self) {
        self.locked.set(false);
    }

    /// Peek without mutating.
    pub(crate) fn is_locked(&self) -> bool {
        self.locked.get()
    }
}
```

- [ ] **Step 4: Run the test and verify it passes**

```bash
cd ~/utono/linux-lit && cargo test page_turn_lock_tests 2>&1 | tail -15
```

Expected output ends with: `test result: ok. 5 passed; 0 failed; 0 ignored`.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/navigation.rs && git commit -m "Add PageTurnLock state machine with unit tests"
```

---

## Task 2: Add `page_turn_lock` and `last_visible_line` fields to AppState

**Files:**
- Modify: `src/app.rs` — `AppState` struct (around line 43) and `AppState` constructor (around line 748).

`page_turn_lock` is the field that the navigation code in Tasks 3–4 will consult. `last_visible_line` is the cache the synchronous clip update in Task 5 will write to and that `is_line_on_screen` will read from to avoid recomputing through the height-summing loops on every MPV `time-pos` tick.

- [ ] **Step 1: Add fields to the AppState struct**

In `src/app.rs`, locate the `AppState` struct (begins around line 43, includes `page_top_line: usize,` and `page_turn_anim: Option<adw::TimedAnimation>,`). Add two new fields. Place them next to the existing pagination fields for locality:

```rust
    /// Re-entrancy lock for animated page turns. `set_page` consults this to
    /// drop racing second turns (e.g. MPV CursorSync arriving mid-animation)
    /// instead of letting them compose with the in-flight turn. Cleared by the
    /// animation's connect_done callback. Lives outside RefCell because
    /// connect_done callbacks borrow only the lock, not all of AppState.
    pub page_turn_lock: std::rc::Rc<crate::input::navigation::PageTurnLock>,

    /// Cached last fully visible buffer line for the current page. Written by
    /// snap_scroll_to_line and update_bottom_clip; read by is_line_on_screen
    /// and MPV sync handlers so they don't recompute through height-summing
    /// loops on every time-pos tick. None until the first scroll completes.
    pub last_visible_line: std::cell::Cell<Option<usize>>,
```

- [ ] **Step 2: Initialize the fields in the constructor**

In `src/app.rs` AppState constructor (around line 748, the literal that initializes `page_top_line: 0,` and `page_turn_anim: None,`), add:

```rust
        page_turn_lock: std::rc::Rc::new(
            crate::input::navigation::PageTurnLock::new()
        ),
        last_visible_line: std::cell::Cell::new(None),
```

- [ ] **Step 3: Reset `last_visible_line` on work load**

In `src/app.rs` `display_work`, locate the line `state.page_top_line = 0;` (around line 1314). Immediately after it, add:

```rust
    state.last_visible_line.set(None);
```

The lock does NOT need to be reset on work load — if it was held, that means an animation is mid-flight and `connect_done` will fire (or has fired) regardless of work load, and the work-load path uses `set_page_instant` which doesn't consult the lock anyway.

- [ ] **Step 4: Build**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -20
```

Expected: compiles with warnings only. The new fields are referenced from the existing initialization sites; if any constructor literal was missed, the compiler will flag `missing field`.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit && git add src/app.rs && git commit -m "Add page_turn_lock and last_visible_line cache to AppState"
```

---

## Task 3: Gate `set_page` on the lock and clear it from `connect_done`

**Files:**
- Modify: `src/input/navigation.rs` — `set_page` function (lines 1034–1175).

`set_page` is the only function that starts an animated turn. It must (1) early-return when the lock is held, (2) acquire the lock for animated transition styles only (`Crossfade`, `Slide`), (3) clear the lock from the existing `connect_done` callback for both branches. The `Instant` branch does no animation, so it neither acquires nor releases the lock.

The skip path (`if loading_work`) returns before any acquire so it does not need a release. The capture-snapshot fallback paths (lines 1054–1057, 1107–1112 — when `capture_page_snapshot` returns `None`) downgrade to instant; those paths must NOT release the lock because they never acquired it.

- [ ] **Step 1: Add the early-return at the top of `set_page`**

In `src/input/navigation.rs`, locate `set_page` (line 1034). After the existing `loading_work` guard (lines 1036–1039) and BEFORE the `log_fmt!` that announces the turn, insert:

```rust
    // F1: drop racing turns so MPV CursorSync arriving mid-animation can't
    // compose with a key-driven turn. Instant transitions don't go through
    // here — they call set_page_instant.
    if !state.page_turn_lock.try_acquire() {
        log_fmt!(
            "PAGE_TURN: SKIPPED (locked=true) new_top={} old_top={} requested_dir={:?}",
            new_top, state.page_top_line, direction
        );
        return;
    }
```

- [ ] **Step 2: Release the lock on the Instant fallthrough path**

The current `Instant` branch (lines 1046–1050) doesn't start an animation, so it must release immediately after the synchronous scroll. Replace:

```rust
        crate::config::TransitionStyle::Instant => {
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);
        }
```

with:

```rust
        crate::config::TransitionStyle::Instant => {
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);
            state.page_turn_lock.release();
        }
```

- [ ] **Step 3: Release the lock from the Crossfade `connect_done`**

In the Crossfade branch (lines 1051–1104), the existing `connect_done` (lines 1097–1100) currently only removes the snapshot overlay. Replace:

```rust
            let snap_cleanup = snapshot_pic.clone();
            anim.connect_done(move |_| {
                overlay.remove_overlay(&snap_cleanup);
            });
```

with:

```rust
            let snap_cleanup = snapshot_pic.clone();
            let lock = state.page_turn_lock.clone();
            anim.connect_done(move |_| {
                overlay.remove_overlay(&snap_cleanup);
                lock.release();
            });
```

Also handle the snapshot-capture-failure fallthrough at lines 1053–1058. Replace:

```rust
            let Some(snapshot_pic) = capture_page_snapshot(state) else {
                clear_old_page_dim(state);
                state.page_top_line = new_top;
                snap_scroll_to_line(state, new_top);
                return;
            };
```

with:

```rust
            let Some(snapshot_pic) = capture_page_snapshot(state) else {
                clear_old_page_dim(state);
                state.page_top_line = new_top;
                snap_scroll_to_line(state, new_top);
                state.page_turn_lock.release();
                return;
            };
```

- [ ] **Step 4: Release the lock from the Slide `connect_done` and snapshot fallthrough**

In the Slide branch (lines 1105–1173), do the equivalent. Replace the `connect_done`:

```rust
            let snap_cleanup = snapshot_pic.clone();
            let card_cleanup = state.card_vbox.clone();
            anim.connect_done(move |_| {
                overlay.remove_overlay(&snap_cleanup);
                card_cleanup.set_margin_start(0);
                card_cleanup.set_margin_end(0);
            });
```

with:

```rust
            let snap_cleanup = snapshot_pic.clone();
            let card_cleanup = state.card_vbox.clone();
            let lock = state.page_turn_lock.clone();
            anim.connect_done(move |_| {
                overlay.remove_overlay(&snap_cleanup);
                card_cleanup.set_margin_start(0);
                card_cleanup.set_margin_end(0);
                lock.release();
            });
```

And the Slide snapshot-capture fallthrough (lines 1107–1112):

```rust
            let Some(snapshot_pic) = capture_page_snapshot(state) else {
                clear_old_page_dim(state);
                state.page_top_line = new_top;
                snap_scroll_to_line(state, new_top);
                return;
            };
```

with:

```rust
            let Some(snapshot_pic) = capture_page_snapshot(state) else {
                clear_old_page_dim(state);
                state.page_top_line = new_top;
                snap_scroll_to_line(state, new_top);
                state.page_turn_lock.release();
                return;
            };
```

- [ ] **Step 5: Build**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -20
```

Expected: compiles with warnings only. If the compiler complains about `state.page_turn_lock.clone()` requiring `Rc::clone`, fix the call site to use `std::rc::Rc::clone(&state.page_turn_lock)` — `Rc::clone` is the conventional form even though `.clone()` works on `Rc<T>`.

- [ ] **Step 6: Run the manual verification protocol**

Paste the Manual Verification Protocol (top of plan) into chat. Stop and wait for the user. Do not proceed to commit until the user replies "verified".

If the user reports a regression: revert this task's changes (`git checkout src/input/navigation.rs`), diagnose, fix, repeat.

- [ ] **Step 7: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/navigation.rs && git commit -m "Gate set_page on PageTurnLock to prevent animated-turn re-entrancy"
```

---

## Task 4: Gate the MPV sync re-entry path explicitly

**Files:**
- Modify: `src/input/navigation.rs` — `scroll_paragraph_to_top` function (around line 1370).

The lock added in Task 3 already prevents `set_page` from running re-entrantly — that's correctness. But `scroll_paragraph_to_top` is called from the MPV CursorSync handler (`src/main.rs:177`) on every paragraph transition; when the lock blocks `set_page`, the handler still pays the cost of `is_line_on_screen` and the page direction calculation. Cheaper to short-circuit at the entry to `scroll_paragraph_to_top` so the MPV path doesn't even try.

This is also load-bearing for F2: if the MPV handler short-circuits, it can't read stale state because it doesn't read at all.

- [ ] **Step 1: Add the lock check at the top of `scroll_paragraph_to_top`**

In `src/input/navigation.rs` (line 1370), locate `scroll_paragraph_to_top`. Before its existing match expression on `state.config.navigation_mode`, insert:

```rust
    // If a page-turn animation is in flight, drop this sync request.
    // The next CursorSync after release will pick up the new state.
    if state.page_turn_lock.is_locked() {
        crate::logging::log(&format!(
            "PARA_SCROLL: SKIP (page_turn_locked) para_start={} page_top={}",
            para_start, state.page_top_line
        ));
        return;
    }
```

- [ ] **Step 2: Build**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: compiles with warnings only.

- [ ] **Step 3: Manual smoke test**

Paste the Manual Verification Protocol (top of plan) into chat. Stop and wait for the user. The user's expected log signature for this task: `PARA_SCROLL: SKIP (page_turn_locked)` lines appearing during the hammer-Ctrl+d test, alongside the `PAGE_TURN: SKIPPED (locked=true)` lines from Task 3.

- [ ] **Step 4: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/navigation.rs && git commit -m "Short-circuit scroll_paragraph_to_top when page-turn lock is held"
```

---

## Task 5: Synchronous clip update + cache the last visible line (F2)

**Files:**
- Modify: `src/input/navigation.rs` — `snap_scroll_to_line` (lines 1192–1217), `update_bottom_clip` (lines 1235–1318), `is_line_on_screen` / `is_line_fully_visible` (lines 829–863).

Currently `snap_scroll_to_line` schedules `update_bottom_clip` via `glib::idle_add_local_once`. Between the scroll mutation and the idle callback firing, MPV `time-pos` events can call `is_line_on_screen` / `is_line_fully_visible`, which recompute the visible range from `text_view.line_yrange` against the *previous* scroll's layout. The fix has two parts:

1. **Run `update_bottom_clip` synchronously** after the `adj.set_value(y)` call inside `snap_scroll_to_line`, AND keep the idle callback as a backstop (GTK occasionally finalizes layout in the next frame; the synchronous call is correct most of the time but the idle re-run catches the stragglers).
2. **Cache the last fully visible line** from `update_bottom_clip` into `state.last_visible_line`, and have `is_line_on_screen` consult the cache before falling back to recompute. The cache is invalidated by writing `None` whenever the layout might be stale — we set it on every successful `update_bottom_clip` run.

- [ ] **Step 1: Refactor `update_bottom_clip` to also write the cache**

`update_bottom_clip` currently takes raw GTK widget references because it's called from a `glib::idle_add_local_once` closure that can only borrow values it captured. To make it write the cache, give it a third option: take an extra `Option<&std::cell::Cell<Option<usize>>>` parameter so the synchronous call site can pass the cache and the idle call site can pass `None` (the idle recompute is purely a clip backstop and doesn't need to update the cache, which the synchronous call already populated).

In `src/input/navigation.rs`, change `update_bottom_clip`'s signature (line 1235) from:

```rust
fn update_bottom_clip(
    text_view: &sourceview5::View,
    bottom_clip: &gtk4::Box,
    scrolled_window: &gtk4::ScrolledWindow,
    page_top: usize,
    line_count: usize,
) {
```

to:

```rust
fn update_bottom_clip(
    text_view: &sourceview5::View,
    bottom_clip: &gtk4::Box,
    scrolled_window: &gtk4::ScrolledWindow,
    page_top: usize,
    line_count: usize,
    last_visible_cache: Option<&std::cell::Cell<Option<usize>>>,
) {
```

Inside `update_bottom_clip`, after the existing trailing-speaker-trim block (after line 1302, just before the `let clip = ...` calculation on line 1304), insert:

```rust
    // F2: cache the last visible line so MPV sync handlers don't recompute
    // through this loop on every time-pos tick. Writing only when we have
    // any non-zero height — `!any_nonzero` already short-circuited above.
    if let Some(cache) = last_visible_cache {
        cache.set(Some(trim));
    }
```

(Note: `trim` is the variable from the existing trailing-speaker-trim loop, which holds the post-trim last visible line. If the variable is named differently after the trim block, use that name. As of the read taken for this plan, the trim loop assigns to `trim` and the loop's exit value is the cached value we want.)

- [ ] **Step 2: Update both `update_bottom_clip` call sites**

There are two callers. The synchronous call inside the idle closure (line 1215) passes `None` because the synchronous-from-`snap_scroll_to_line` call (added in Step 3 below) will already have populated the cache. Update line 1215:

From:

```rust
    glib::idle_add_local_once(move || {
        update_bottom_clip(&text_view, &bottom_clip, &scrolled_window, page_top, line_count);
    });
```

To:

```rust
    glib::idle_add_local_once(move || {
        update_bottom_clip(&text_view, &bottom_clip, &scrolled_window, page_top, line_count, None);
    });
```

- [ ] **Step 3: Add the synchronous call inside `snap_scroll_to_line`**

In `snap_scroll_to_line` (line 1192), after the `adj.set_value(y as f64);` line (line 1198) and BEFORE the page-label update block (line 1201), insert:

```rust
    // F2: run update_bottom_clip synchronously so MPV sync handlers reading
    // last_visible_line right after this call don't see stale state. The
    // idle re-run scheduled below is a backstop for GTK frames that finalize
    // layout in the next tick.
    update_bottom_clip(
        &state.text_view,
        &state.bottom_clip,
        &state.scrolled_window,
        line,
        state.effective_line_count(),
        Some(&state.last_visible_line),
    );
```

- [ ] **Step 4: Have `is_line_fully_visible` consult the cache**

Currently `is_line_fully_visible` (line 836) recomputes the visible range from scratch on every call. After the F2 fix, the cache is authoritative for the *current* page; we still fall back to recompute when the cache is empty (cold start, post-resize, post-work-load — exactly the cases the cache was reset to `None`).

In `src/input/navigation.rs`, replace `is_line_fully_visible`'s body with:

```rust
fn is_line_fully_visible(state: &AppState, line: usize) -> bool {
    // During work loading, GTK layout is stale — report all lines as visible
    // to prevent bogus page turns that crash the app.
    if state.loading_work.get() {
        return true;
    }
    if line < state.page_top_line {
        return false;
    }
    // F2: fast path — consult the cache populated by update_bottom_clip.
    if let Some(last_visible) = state.last_visible_line.get() {
        return line <= last_visible;
    }
    // Cold-start fallback: recompute. Mirror of the original loop.
    let descender_guard = descender_guard_px(&state.text_view, state.page_top_line);
    let bottom_margin = state.text_view.bottom_margin();
    let usable_height = state.text_view.height() - descender_guard - bottom_margin;
    let buf = &state.buffer;
    let mut total_height = 0;
    for i in state.page_top_line..=line {
        let Some(iter) = buf.iter_at_line(i as i32) else { return false };
        let (_y, h) = state.text_view.line_yrange(&iter);
        total_height += h;
        if total_height > usable_height {
            return false;
        }
    }
    true
}
```

- [ ] **Step 5: Build**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -20
```

Expected: compiles with warnings only. The new `last_visible_cache` parameter is the only signature change; the two callers are explicitly updated above.

- [ ] **Step 6: Run unit tests**

```bash
cd ~/utono/linux-lit && cargo test 2>&1 | tail -10
```

Expected: all existing tests pass (including `page_turn_tests` and `page_turn_lock_tests`). No new unit tests in this task — `update_bottom_clip` and `snap_scroll_to_line` are GTK-widget-bound and not unit-testable in the linux-lit convention.

- [ ] **Step 7: Manual verification — extended protocol for F2**

Paste this expanded protocol into chat. Stop and wait for the user.

```
1. cargo build (must succeed).
2. User starts the app: cargo run.
3. User opens a long prose work via Ctrl+p.
4. User pages forward several times (no playback yet) — confirm no clipped descenders or excess gap below text. This validates the synchronous clip path on the dominant code path.
5. User starts MPV playback (s key) on a work with timestamps.
6. User watches a paragraph transition turn the page during playback. Confirm the page turn lands cleanly with no double-render.
7. User checks ~/utono/linux-lit/linux-lit-dev.log for `BOTTOM_CLIP:` lines — should appear immediately after every `PAGE_TURN:` line, NOT after a delay.
8. User confirms in chat: "verified" or describes the failure mode.
```

- [ ] **Step 8: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/navigation.rs && git commit -m "Run update_bottom_clip synchronously and cache last_visible_line for sync handlers"
```

---

## Task 6: Final verification and merge readiness

**Files:** none modified.

This task confirms the F1+F2 batch is done and the working tree is clean.

- [ ] **Step 1: Confirm clean tree**

```bash
cd ~/utono/linux-lit && git status
```

Expected: `nothing to commit, working tree clean`. If anything is dirty, investigate before proceeding.

- [ ] **Step 2: Confirm test suite passes**

```bash
cd ~/utono/linux-lit && cargo test 2>&1 | tail -10
```

Expected: all tests pass. Specifically the new `page_turn_lock_tests` mod (5 tests) and the existing `page_turn_tests` mod.

- [ ] **Step 3: Confirm build is warning-clean (or warning-comparable to baseline)**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | grep -c "^warning"
```

Compare to the warning count from `git stash; cargo build 2>&1 | grep -c warning; git stash pop` if the executor wants to verify no new warnings. Otherwise: confirm no `error:` lines.

- [ ] **Step 4: Confirm commit log**

```bash
cd ~/utono/linux-lit && git log --oneline -5
```

Expected: the most recent four commits are this plan's commits in order:
1. `Add PageTurnLock state machine with unit tests`
2. `Add page_turn_lock and last_visible_line cache to AppState`
3. `Gate set_page on PageTurnLock to prevent animated-turn re-entrancy`
4. `Short-circuit scroll_paragraph_to_top when page-turn lock is held`
5. `Run update_bottom_clip synchronously and cache last_visible_line for sync handlers`

(Five commits total. Order matters: Task 2's struct field change must come before Tasks 3–5 reference it; Task 4 depends on Task 3's lock semantics; Task 5 is independent of Tasks 3–4 but easier to verify after them.)

- [ ] **Step 5: Final user signoff**

Output to chat:

> "F1+F2 implementation complete. Five commits on the current branch. Manual verification protocols passed. Ready to merge to master via /git:merge, or continue with another finding (F3 descender, F4 resize, F5 backward fallback) — your call."

Do not invoke `/git:merge` or any other command. Wait for the user.

---

## Self-Review

**Spec coverage:**
- F1 (re-entrancy lock): Tasks 1, 2, 3, 4. ✓
- F2 (synchronous clip + cache): Tasks 2 (cache field), 5 (sync update + cache wiring + fast-path read). ✓
- The "Hypothesis / improvement" sentence in F1 says "Early-return from `page_forward`, `page_backward`, `set_page`, `scroll_paragraph_to_top` when locked." Tasks 3 and 4 implement set_page and scroll_paragraph_to_top. `page_forward` and `page_backward` always go through `set_page` — covered transitively. ✓
- F2's hypothesis says "Store the computed clip and last-visible line in AppState." Task 2 adds `last_visible_line`; the bottom_clip height_request is already stored on the widget itself (`bottom_clip.set_height_request`), so no separate field. ✓

**Placeholder scan:** No "TBD", "TODO", "implement later", or "similar to Task N" present. Every code block contains the actual code to insert. The Manual Verification Protocol is reproduced inline rather than referenced abstractly. ✓

**Type / API consistency:**
- `PageTurnLock::try_acquire` / `release` / `is_locked` — Task 1 defines, Task 2 wraps in `Rc`, Task 3 calls `try_acquire` and `release` (via clone-into-closure), Task 4 calls `is_locked`. Names match throughout. ✓
- `state.page_turn_lock` — added in Task 2, referenced in Tasks 3, 4. Type is `Rc<PageTurnLock>` so `.clone()` produces another `Rc` (cheap, expected). ✓
- `state.last_visible_line` — added in Task 2 as `Cell<Option<usize>>`. Task 5 reads via `.get()` and writes via `.set(Some(trim))`. Reset to `None` in Task 2 Step 3. ✓
- `update_bottom_clip` signature change — Task 5 adds `Option<&Cell<Option<usize>>>` parameter; both call sites (idle-callback and new synchronous call) updated in the same task. ✓

**Notes for the executor:**
- The skill document says "TDD". This plan applies TDD to the unit-testable PageTurnLock (Task 1) and uses `cargo build` + manual smoke test for the GTK-widget-bound code (Tasks 2–5), per linux-lit's CLAUDE.md convention. The skill's Instruction Priority section ("user instructions always take precedence") authorizes this divergence.
- Tasks 3–5 each include a manual verification step that requires the user to run the app and report back. Do NOT skip these. Do NOT proceed to the commit step until the user confirms.
- The `trim` variable referenced in Task 5 Step 1 exists in the current `update_bottom_clip` (line 1284) — confirm by re-reading the function before inserting the cache write.
