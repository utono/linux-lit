# Page-Turn Lock + Single `visible_range` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring linux-lit's pagination structure into alignment with foliate-js on two pivots — (1) a single `PageTurnLock` mirroring foliate's `#locked` so page-mutating entry points share one race-prevention idiom, and (2) a single `visible_range()` function mirroring foliate's `getVisibleRange` so the four current height-summing loops collapse to one source of truth.

**Architecture:**

*F1 — turn lock.* Add a `PageTurnLock` (Cell-backed) wrapped in `Rc` on AppState. `set_page` acquires it on entry and releases from each animation's `connect_done` (and from the Instant fallthrough). `scroll_paragraph_to_top` (the MPV re-entry path) consults `is_locked` and short-circuits. `page_forward` and `page_backward` go through `set_page` already, so they inherit the guard transitively.

*F2 — visible_range.* Introduce `pub(crate) fn visible_range(text_view, buffer, page_top, line_count) -> VisibleRange { last_fit, total_height, count }` taking raw widget refs (so `update_bottom_clip` — which is called from an idle closure that holds raw refs only — can call it directly). The trim-trailing-speakers step becomes `trim_trailing_speakers(buffer, range, page_top, text_view) -> VisibleRange` so callers compose it when they want it. Replace all four call sites: `last_fully_visible_line` (uses raw → trim → returns `last_fit`), `is_line_fully_visible` (uses raw, short-circuits on `last_fit >= line`), `update_bottom_clip` (uses raw → trim → computes `clip` from `total_height`), `lines_per_page` (uses raw → no trim → returns `count`). Caller-specific short-circuits (`loading_work`, `widget_height <= 0`, empty-buffer) stay in the callers — they're not part of the shared kernel.

The cache (`#lastVisibleRange`) named in F2's "Refactor toward reference" is **not** in this plan. It belongs with F4 (synchronous clip update) so the cache and the synchronous write land together in a separate plan that builds on this consolidation.

**Tech Stack:** Rust 2021, GTK4 0.9 + libadwaita 0.7 + sourceview5 0.9, single-threaded GTK main loop with Tokio runtime in a separate thread bridged via `glib::spawn_future_local`. AppState lives in `Rc<RefCell<AppState>>`. Animations are `adw::TimedAnimation` with `connect_done` callbacks.

**Source of findings:** `docs/reviews/2026-04-28-pagination-vs-references.md` F1 (page-turn lock) and F2 (single `visible_range`).

**Verification model:** linux-lit's project convention (per CLAUDE.md) is `cargo build` + manual smoke test by the user. Pure-Rust unit tests cover (a) the `PageTurnLock` state machine and (b) the `trim_trailing_speakers` transform on synthetic line lists. The GTK-bound `visible_range` (calls `text_view.line_yrange`, `text_view.height()`) is verified by `cargo build` + the existing `page_turn_tests` mod (which exercises page-forward/backward over real text via simulation helpers) + the manual smoke-test protocol.

**Out of scope (each gets its own plan):**
- F3 (post-scroll `relocate` event)
- F4 (synchronous clip update + `#lastVisibleRange` cache) — will build on F2's `visible_range`
- F5 (descender guard via Pango font metrics)
- F6 (resize observer)
- F7 (backward fallback `prev_page_top`)
- F8 (page-top index cache)
- F9 (block-atom rule)
- F10 (view-trait dispatch)

---

## File Map

- **Modify:** `src/app.rs` — add `page_turn_lock: Rc<PageTurnLock>` field to `AppState` struct (around line 43); initialize in the constructor (around line 748). No other AppState changes in this plan.
- **Modify:** `src/input/navigation.rs`:
  - Add `PageTurnLock` struct + `VisibleRange` struct + `visible_range` function + `trim_trailing_speakers` function near the existing `PageDirection` enum (around line 982).
  - Replace bodies of `last_fully_visible_line` (lines 119–152), `is_line_fully_visible` (lines 836–863), `update_bottom_clip` (lines 1235–1318), and `lines_per_page` (lines 1669–1702) to call `visible_range` (and `trim_trailing_speakers` where applicable).
  - Gate `set_page` (line 1034) and `scroll_paragraph_to_top` (line 1370) on the lock.
- **No new files.** Both new structs and both new functions live in `navigation.rs` next to their only callers.
- **Tests:** Append `#[cfg(test)] mod page_turn_lock_tests` and `#[cfg(test)] mod visible_range_helpers_tests` to `src/input/navigation.rs` after the existing `page_turn_tests` mod.

---

## Manual Verification Protocol

Tasks 3, 4, and 6 end with the user running this protocol. Until the user confirms success, do not proceed. The plan executor must paste this protocol into the chat at the verification step and stop, waiting for the user.

```
1. cargo build (must succeed with warnings only).
2. User starts the app: cargo run.
3. User opens a long prose work (e.g. Bleak House) via Ctrl+p.
4. User selects Crossfade transition in settings (Ctrl+,).
5. User pages forward several times (no playback yet) — confirm clean pages, no clipped descenders, no excess gap below text. Confirms visible_range parity for the dominant code path.
6. User starts MPV playback (s key) on a work with timestamps.
7. User holds Ctrl+d (or hammers x for page forward) for 5 seconds during playback.
8. Expected: every keypress lands on a clean page; no half-faded snapshot stuck on screen; no skipped lines on the line counter.
9. User checks ~/utono/linux-lit/linux-lit-dev.log for `PAGE_TURN: SKIPPED (locked=true)` and `PARA_SCROLL: SKIP (page_turn_locked)` lines — these are expected when the lock blocks a second turn during animation.
10. User repeats steps 4–9 with Slide transition.
11. User confirms in chat: "verified" or describes the failure mode.
```

If the user reports a regression: revert the failing task's changes (`git checkout src/input/navigation.rs src/app.rs`) and diagnose before continuing.

---

## Task 1: Add the `PageTurnLock` state machine with unit tests

**Files:**
- Modify: `src/input/navigation.rs` — add `PageTurnLock` struct near the existing `PageDirection` enum (around line 982); append `#[cfg(test)] mod page_turn_lock_tests` at the end of the file.

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
/// Mirrors foliate-js's `Paginator.#locked` (paginator.js:1060-1071).
///
/// Uses `Cell<bool>` rather than `bool` so a `&PageTurnLock` borrow can mutate
/// it from a `connect_done` closure without a `&mut AppState`.
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
cd ~/utono/linux-lit && git add src/input/navigation.rs && git commit -m "$(cat <<'EOF'
Add PageTurnLock state machine with unit tests

Mirrors foliate-js's Paginator.#locked (paginator.js:1060-1071) so future
foliate cross-references translate directly. Cell-backed so a clone-into-
closure release doesn't need &mut AppState.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Add `page_turn_lock` field to AppState

**Files:**
- Modify: `src/app.rs` — `AppState` struct (around line 43) and `AppState` constructor (around line 748).

`page_turn_lock` is the field that the navigation code in Tasks 3–4 will consult. The lock lives inside `Rc` so animation `connect_done` closures can clone-and-release it without holding a `&mut AppState`.

- [ ] **Step 1: Add field to the AppState struct**

In `src/app.rs`, locate the `AppState` struct (begins around line 43, includes `page_top_line: usize,` and `page_turn_anim: Option<adw::TimedAnimation>,`). Add the new field next to `page_turn_anim` for locality:

```rust
    /// Re-entrancy lock for animated page turns. `set_page` consults this to
    /// drop racing second turns (e.g. MPV CursorSync arriving mid-animation)
    /// instead of letting them compose with the in-flight turn. Cleared by the
    /// animation's connect_done callback. Wrapped in Rc so connect_done
    /// closures can clone-and-release without a &mut AppState borrow.
    /// Mirrors foliate-js Paginator.#locked.
    pub page_turn_lock: std::rc::Rc<crate::input::navigation::PageTurnLock>,
```

- [ ] **Step 2: Initialize the field in the constructor**

In `src/app.rs` AppState constructor (around line 748, the literal that initializes `page_top_line: 0,` and `page_turn_anim: None,`), add:

```rust
        page_turn_lock: std::rc::Rc::new(
            crate::input::navigation::PageTurnLock::new()
        ),
```

The lock does NOT need to be reset on work load — if it was held, that means an animation is mid-flight and `connect_done` will fire (or has fired) regardless of work load, and the work-load path uses `set_page_instant` which doesn't consult the lock.

- [ ] **Step 3: Build**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -20
```

Expected: compiles with warnings only. The new field is referenced from the existing initialization site; if any constructor literal was missed, the compiler will flag `missing field`.

- [ ] **Step 4: Commit**

```bash
cd ~/utono/linux-lit && git add src/app.rs && git commit -m "$(cat <<'EOF'
Add page_turn_lock to AppState

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Gate `set_page` on the lock and clear it from `connect_done`

**Files:**
- Modify: `src/input/navigation.rs` — `set_page` function (lines 1034–1175).

`set_page` is the only function that starts an animated turn. It must (1) early-return when the lock is held, (2) acquire the lock for animated transition styles only (`Crossfade`, `Slide`), (3) clear the lock from the existing `connect_done` callback for both branches. The `Instant` branch does no animation, so it acquires-then-releases inline.

The `loading_work` skip path returns before any acquire so it does not need a release. The capture-snapshot fallback paths (when `capture_page_snapshot` returns `None`) downgrade to instant; those paths must release the lock because they DID acquire it at the top of `set_page`.

- [ ] **Step 1: Add the early-return at the top of `set_page`**

In `src/input/navigation.rs`, locate `set_page` (line 1034). After the existing `loading_work` guard (lines 1036–1039) and BEFORE the `log_fmt!` that announces the turn, insert:

```rust
    // F1: drop racing turns so MPV CursorSync arriving mid-animation can't
    // compose with a key-driven turn. set_page_instant does not go through
    // here — it has no animation window.
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

- [ ] **Step 3: Release the lock from the Crossfade `connect_done` and snapshot fallthrough**

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
            let lock = std::rc::Rc::clone(&state.page_turn_lock);
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
            let lock = std::rc::Rc::clone(&state.page_turn_lock);
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

Expected: compiles with warnings only.

- [ ] **Step 6: Run all tests**

```bash
cd ~/utono/linux-lit && cargo test 2>&1 | tail -15
```

Expected: all existing tests pass plus the 5 `page_turn_lock_tests` from Task 1.

- [ ] **Step 7: Manual verification**

Paste the Manual Verification Protocol (top of plan) into chat. Stop and wait for the user. Do not proceed to commit until the user replies "verified".

If the user reports a regression: revert this task's changes (`git checkout src/input/navigation.rs`), diagnose, fix, repeat.

- [ ] **Step 8: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/navigation.rs && git commit -m "$(cat <<'EOF'
Gate set_page on PageTurnLock to prevent animated-turn re-entrancy

Mirrors foliate-js Paginator.#turnPage (paginator.js:1060-1071): acquire
the lock before mutating page_top_line, release in connect_done. Snapshot-
capture fallthroughs and the Instant transition release inline since they
do no animation. set_page_instant does not consult the lock — no animation
window, no race.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Gate `scroll_paragraph_to_top` (the MPV re-entry path) on the lock

**Files:**
- Modify: `src/input/navigation.rs` — `scroll_paragraph_to_top` function (around line 1370).

The lock added in Task 3 already prevents `set_page` from running re-entrantly — that's correctness. But `scroll_paragraph_to_top` is called from the MPV CursorSync handler (`src/main.rs:177`) on every paragraph transition; when the lock blocks `set_page`, the handler still pays the cost of `is_line_on_screen` and the page direction calculation. Cheaper to short-circuit at the entry to `scroll_paragraph_to_top` so the MPV path doesn't even try.

This matches foliate's `goTo` early-return on `#locked` (paginator.js:1023) — the entry point checks the lock, not just the inner `#turnPage`.

- [ ] **Step 1: Add the lock check at the top of `scroll_paragraph_to_top`**

In `src/input/navigation.rs` (line 1370), locate `scroll_paragraph_to_top`. Before its existing match expression on `state.config.navigation_mode`, insert:

```rust
    // F1: if a page-turn animation is in flight, drop this sync request.
    // The next CursorSync after release will pick up the new state.
    // Mirrors foliate Paginator.goTo's #locked early-return (paginator.js:1023).
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

- [ ] **Step 3: Run all tests**

```bash
cd ~/utono/linux-lit && cargo test 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 4: Manual verification**

Paste the Manual Verification Protocol (top of plan) into chat. Stop and wait for the user. The user's expected log signature for this task: `PARA_SCROLL: SKIP (page_turn_locked)` lines appearing during the hammer-Ctrl+d test, alongside the `PAGE_TURN: SKIPPED (locked=true)` lines from Task 3.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/navigation.rs && git commit -m "$(cat <<'EOF'
Short-circuit scroll_paragraph_to_top when page-turn lock is held

Mirrors foliate Paginator.goTo's #locked early-return (paginator.js:1023):
gate at the entry point, not just at the inner mutator. MPV CursorSync
mid-animation no longer pays the cost of is_line_on_screen + direction calc
just to be rejected by set_page.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Add `VisibleRange`, `visible_range`, and `trim_trailing_speakers` with unit tests

**Files:**
- Modify: `src/input/navigation.rs` — add `VisibleRange` struct, `visible_range` function, and `trim_trailing_speakers` function near `PageTurnLock` (around line 990–1030 after Task 1's additions). Append `#[cfg(test)] mod visible_range_helpers_tests` after the existing `page_turn_lock_tests` mod.

This task adds the new abstractions WITHOUT changing any existing call site — that's Task 6's job. Splitting it lets us land the kernel function and its trim helper as a self-contained, testable commit, then collapse the four loops in a separate commit that's easy to revert if behavior diverges.

`visible_range` takes raw widget refs (not `&AppState`) because `update_bottom_clip` is called from a `glib::idle_add_local_once` closure that captures only `text_view`, `bottom_clip`, `scrolled_window`, `page_top`, `line_count` — not AppState. Designing for the most-constrained caller means the others can use it too via thin wrappers.

`trim_trailing_speakers` is a pure function over a buffer, range, and page_top — testable on synthetic input without GTK widgets.

- [ ] **Step 1: Write the failing tests**

Append to the end of `src/input/navigation.rs`, after the `page_turn_lock_tests` mod added in Task 1:

```rust
#[cfg(test)]
mod visible_range_helpers_tests {
    use super::{VisibleRange, trim_trailing_speakers_pure};

    // Speaker detection in the real trim_trailing_speakers depends on
    // crate::db::line_types and a sourceview5::Buffer. For unit tests we
    // exercise a pure variant trim_trailing_speakers_pure that takes a
    // closure for "is the line at index i a speaker-or-blank?" and a
    // closure for "what is the height of line i?". The production
    // trim_trailing_speakers wraps it with the GTK + line_types calls.

    fn line_classifier(speakers_or_blanks: &[usize]) -> impl Fn(usize) -> bool + '_ {
        move |i| speakers_or_blanks.contains(&i)
    }

    fn line_height(_i: usize) -> i32 {
        20
    }

    #[test]
    fn trim_with_no_trailing_speaker_is_identity() {
        let range = VisibleRange { last_fit: 5, total_height: 100, count: 6 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 5);
        assert_eq!(trimmed.total_height, 100);
        assert_eq!(trimmed.count, 6);
    }

    #[test]
    fn trim_drops_one_trailing_speaker() {
        let range = VisibleRange { last_fit: 5, total_height: 100, count: 6 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[5]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 4);
        assert_eq!(trimmed.total_height, 80);
        assert_eq!(trimmed.count, 5);
    }

    #[test]
    fn trim_drops_speaker_with_preceding_blanks() {
        // Lines 3 (blank), 4 (blank), 5 (speaker) — all trim.
        let range = VisibleRange { last_fit: 5, total_height: 120, count: 6 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[3, 4, 5]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 2);
        assert_eq!(trimmed.total_height, 60);
        assert_eq!(trimmed.count, 3);
    }

    #[test]
    fn trim_stops_at_dialogue_line() {
        // Line 5 is speaker, line 4 is dialogue (not in classifier), line 3 is blank.
        // Trim removes line 5 only — line 4 is dialogue, blocks further trim.
        let range = VisibleRange { last_fit: 5, total_height: 120, count: 6 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[3, 5]), // 4 is dialogue
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 4);
        assert_eq!(trimmed.total_height, 100);
        assert_eq!(trimmed.count, 5);
    }

    #[test]
    fn trim_does_not_cross_page_top() {
        // Every line is a speaker, but page_top is 3 — trim must not delete the
        // page top itself (would leave an empty page).
        let range = VisibleRange { last_fit: 5, total_height: 60, count: 3 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            3,
            &line_classifier(&[3, 4, 5]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 3, "must leave page_top in place");
        assert!(trimmed.total_height > 0);
        assert!(trimmed.count >= 1);
    }

    #[test]
    fn trim_with_empty_range_is_noop() {
        // last_fit == page_top, count == 1 — nothing to trim.
        let range = VisibleRange { last_fit: 0, total_height: 20, count: 1 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[0]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 0);
        assert_eq!(trimmed.total_height, 20);
        assert_eq!(trimmed.count, 1);
    }
}
```

- [ ] **Step 2: Run the tests and verify they fail**

```bash
cd ~/utono/linux-lit && cargo test visible_range_helpers_tests 2>&1 | tail -20
```

Expected: compilation error `cannot find type 'VisibleRange'` and `cannot find function 'trim_trailing_speakers_pure'`.

- [ ] **Step 3: Implement `VisibleRange`, `visible_range`, and the trim helpers**

In `src/input/navigation.rs`, immediately after the `PageTurnLock` impl block added in Task 1 (around line 1030 after Task 1's additions), add:

```rust
/// Result of a single height-summing walk over the buffer starting at a page
/// top: which line was the last to fully fit, the total pixel height consumed
/// by lines [page_top, last_fit], and the count of lines included.
///
/// Mirrors what foliate-js `getVisibleRange` returns (paginator.js:94-151) —
/// a single source of truth for "what's on screen right now" that all four
/// previous callers (`last_fully_visible_line`, `is_line_fully_visible`,
/// `update_bottom_clip`, `lines_per_page`) project from.
///
/// Convention: when the buffer is empty or no line fits, `count == 0` and
/// `total_height == 0`. `last_fit` is then equal to `page_top` but should be
/// treated as meaningless by callers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VisibleRange {
    pub(crate) last_fit: usize,
    pub(crate) total_height: i32,
    pub(crate) count: usize,
}

/// Walk the buffer from `page_top`, summing line heights against the viewport's
/// `usable_height` (caller computes that from `widget_height - descender_guard
/// - bottom_margin`). Returns the largest range that fully fits.
///
/// Caller-specific short-circuits (`loading_work`, `widget_height <= 0`, empty
/// buffer) stay in the callers — they're not part of this kernel.
///
/// Mirrors `getVisibleRange` in foliate-js paginator.js (lines 94-151) in
/// purpose: one canonical visibility computation. Future foliate reads of that
/// function map directly to this one.
pub(crate) fn visible_range(
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    page_top: usize,
    line_count: usize,
    usable_height: i32,
) -> VisibleRange {
    let mut total_height: i32 = 0;
    let mut count: usize = 0;
    let mut last_fit: usize = page_top;
    for i in page_top..line_count {
        let Some(iter) = buffer.iter_at_line(i as i32) else { break };
        let (_y, h) = text_view.line_yrange(&iter);
        if total_height + h > usable_height {
            break;
        }
        total_height += h;
        last_fit = i;
        count += 1;
    }
    VisibleRange { last_fit, total_height, count }
}

/// Trim trailing speaker-name and blank lines from a `VisibleRange` so a
/// dangling speaker doesn't appear at the bottom of a page without its
/// dialogue. Pure variant separated for unit testability — the GTK-bound
/// `trim_trailing_speakers` wraps this with line_types and line_yrange calls.
///
/// Stops at `page_top` so the trim never deletes the page top itself.
pub(crate) fn trim_trailing_speakers_pure<F, H>(
    mut range: VisibleRange,
    page_top: usize,
    is_speaker_or_blank: F,
    line_height: H,
) -> VisibleRange
where
    F: Fn(usize) -> bool,
    H: Fn(usize) -> i32,
{
    while range.last_fit > page_top && is_speaker_or_blank(range.last_fit) {
        range.total_height -= line_height(range.last_fit);
        range.last_fit -= 1;
        range.count = range.count.saturating_sub(1);
    }
    range
}

/// GTK-bound wrapper for `trim_trailing_speakers_pure`. Reads line text via
/// `buffer` and classifies via `crate::db::line_types`; reads heights via
/// `text_view.line_yrange`.
pub(crate) fn trim_trailing_speakers(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
) -> VisibleRange {
    use crate::db::line_types;
    let is_speaker_or_blank = |i: usize| -> bool {
        let text = {
            let Some(start) = buffer.iter_at_line(i as i32) else { return false };
            let mut end = start;
            if !end.ends_line() { end.forward_to_line_end(); }
            buffer.text(&start, &end, false).to_string()
        };
        line_types::is_speaker(&text) || line_types::is_blank(&text)
    };
    let line_height = |i: usize| -> i32 {
        let Some(iter) = buffer.iter_at_line(i as i32) else { return 0 };
        let (_y, h) = text_view.line_yrange(&iter);
        h
    };
    trim_trailing_speakers_pure(range, page_top, is_speaker_or_blank, line_height)
}
```

- [ ] **Step 4: Run the tests and verify they pass**

```bash
cd ~/utono/linux-lit && cargo test visible_range_helpers_tests 2>&1 | tail -15
```

Expected output ends with: `test result: ok. 6 passed; 0 failed; 0 ignored`.

- [ ] **Step 5: Build the whole crate**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: compiles with warnings only. The new `visible_range`, `VisibleRange`, `trim_trailing_speakers`, `trim_trailing_speakers_pure` will warn as `dead_code` — that's expected; Task 6 removes the warnings by wiring them to the four call sites.

- [ ] **Step 6: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/navigation.rs && git commit -m "$(cat <<'EOF'
Add VisibleRange + visible_range + trim_trailing_speakers helpers

Mirrors foliate-js getVisibleRange (paginator.js:94-151): single canonical
visibility kernel that all callers project from. Trim is split into a pure
testable variant + a GTK-bound wrapper. Unused until Task 6 wires the four
existing height-summing loops to call these.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Replace the four height-summing loops with `visible_range`

**Files:**
- Modify: `src/input/navigation.rs` — `last_fully_visible_line` (lines 119–152), `is_line_fully_visible` (lines 836–863), `update_bottom_clip` (lines 1235–1318), `lines_per_page` (lines 1669–1702).

This task is the consolidation. Each callsite keeps its caller-specific short-circuits (the things that are NOT part of the shared kernel) and delegates the loop to `visible_range`. Three of four also call `trim_trailing_speakers`; `lines_per_page` does not.

**Behavior must be identical.** The kernel is byte-equivalent to the existing inner loop; the only behavioral change is structural. After this task, `cargo test page_turn_tests` (which exercises page-forward/backward against real Troilus and Cressida text via simulation helpers) must still pass — those tests don't call `visible_range` directly but do exercise `last_fully_visible_line` indirectly via `next_page_top`.

- [ ] **Step 1: Replace `last_fully_visible_line` body**

In `src/input/navigation.rs`, replace the entire `last_fully_visible_line` function (lines 119–152, including the doc comment) with:

```rust
/// Find the last buffer line that fits within the viewport starting from
/// `top`, matching the bottom clip calculation exactly. A line is included
/// only if its full height fits in the remaining usable space (widget height
/// minus descender guard). This ensures page_forward doesn't count clipped
/// lines as "seen". Trailing speaker names and blanks are trimmed so a
/// dangling speaker at the bottom doesn't count as "visible" content.
fn last_fully_visible_line(state: &AppState, top: usize) -> usize {
    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return top;
    }
    let line_count = state.effective_line_count();
    let descender_guard = descender_guard_px(&state.text_view, top);
    let bottom_margin = state.text_view.bottom_margin();
    let usable_height = widget_height - descender_guard - bottom_margin;
    let range = visible_range(&state.text_view, &state.buffer, top, line_count, usable_height);
    let trimmed = trim_trailing_speakers(range, top, &state.text_view, &state.buffer);
    trimmed.last_fit
}
```

- [ ] **Step 2: Replace `is_line_fully_visible` body**

In `src/input/navigation.rs`, replace the entire `is_line_fully_visible` function (lines 836–863, including the doc comment) with:

```rust
/// Check whether a buffer line is fully visible in the viewport.
/// Sums line heights from page_top against widget_height minus a descender
/// guard and the text_view bottom margin (which GTK reserves for padding
/// and is not available for text rendering).
fn is_line_fully_visible(state: &AppState, line: usize) -> bool {
    // During work loading, GTK layout is stale — report all lines as visible
    // to prevent bogus page turns that crash the app.
    if state.loading_work.get() {
        return true;
    }
    if line < state.page_top_line {
        return false;
    }
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

Note: this version walks the full visible range every call (same as the current code does up to `line` and stops). After F4 (separate plan) introduces the cache, this becomes a cache lookup.

- [ ] **Step 3: Replace `update_bottom_clip` body**

In `src/input/navigation.rs`, replace the entire `update_bottom_clip` function (lines 1235–1318) with:

```rust
fn update_bottom_clip(
    text_view: &sourceview5::View,
    bottom_clip: &gtk4::Box,
    scrolled_window: &gtk4::ScrolledWindow,
    page_top: usize,
    line_count: usize,
) {
    let widget_height = text_view.height();
    if widget_height <= 0 {
        bottom_clip.set_height_request(0);
        return;
    }

    let buf = text_view.buffer();
    let buf_sv: sourceview5::Buffer = match buf.downcast::<sourceview5::Buffer>() {
        Ok(b) => b,
        Err(_) => {
            bottom_clip.set_height_request(0);
            return;
        }
    };

    let descender_guard = descender_guard_px(text_view, page_top);
    let bottom_margin = text_view.bottom_margin();
    let usable_height = widget_height - descender_guard - bottom_margin;

    let range = visible_range(text_view, &buf_sv, page_top, line_count, usable_height);

    if range.count == 0 || range.total_height == 0 {
        bottom_clip.set_height_request(0);
        return;
    }

    let trimmed = trim_trailing_speakers(range, page_top, text_view, &buf_sv);

    let clip = (widget_height - trimmed.total_height).max(0);
    let scroll_val = scrolled_window.vadjustment().value();
    let expected_y = if let Some(iter) = buf_sv.iter_at_line(page_top as i32) {
        let (y, _h) = text_view.line_yrange(&iter);
        y as f64
    } else {
        -1.0
    };
    let scroll_offset = scroll_val - expected_y;
    crate::logging::log(&format!(
        "BOTTOM_CLIP: widget_h={} total_h={} clip={} page_top={} scroll_val={:.1} expected_y={:.1} offset={:.1}",
        widget_height, trimmed.total_height, clip, page_top, scroll_val, expected_y, scroll_offset
    ));
    bottom_clip.set_height_request(clip);
}
```

Note the `buf.downcast::<sourceview5::Buffer>()` step. `text_view.buffer()` returns `gtk4::TextBuffer`; `visible_range` and `trim_trailing_speakers` take `&sourceview5::Buffer` because the rest of the file uses sourceview's Buffer (which derefs to TextBuffer). The downcast falls back gracefully if the buffer isn't actually a sourceview Buffer — that condition shouldn't happen in production but the bail keeps the function total.

The `any_nonzero` guard from the original code is replaced by `range.count == 0 || range.total_height == 0`, which is equivalent: count is 0 only when no line fit; total_height is 0 only when every fitting line had height 0 (the original code's `!any_nonzero` case).

- [ ] **Step 4: Replace `lines_per_page` body**

In `src/input/navigation.rs`, replace the entire `lines_per_page` function (lines 1669–1702) with:

```rust
/// Count how many buffer lines are fully visible starting from `page_top_line`.
/// Returns a calibrated estimate (35) during work load when GTK layout is
/// invalid, and a small fallback (15) for empty or past-end buffers.
fn lines_per_page(state: &AppState) -> usize {
    if state.loading_work.get() {
        return 35;
    }

    let line_count = state.effective_line_count();
    let start = state.page_top_line;

    if line_count == 0 || start >= line_count {
        return 15;
    }

    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return 15;
    }

    let descender_guard = descender_guard_px(&state.text_view, start);
    let bottom_margin = state.text_view.bottom_margin();
    let usable_height = widget_height - descender_guard - bottom_margin;
    let range = visible_range(
        &state.text_view,
        &state.buffer,
        start,
        line_count,
        usable_height,
    );
    range.count.max(1)
}
```

`lines_per_page` does NOT trim trailing speakers — that's the original behavior. It returns the raw fit count (so `page_backward`'s `current - lpp` fallback uses the full visual page count, not a trimmed count). Documenting this absence is implicit in the function's omission of `trim_trailing_speakers`.

- [ ] **Step 5: Build**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -20
```

Expected: compiles with warnings only. The previous `dead_code` warnings on `visible_range`, `trim_trailing_speakers`, `VisibleRange` should disappear.

- [ ] **Step 6: Run all tests, with focus on `page_turn_tests`**

```bash
cd ~/utono/linux-lit && cargo test 2>&1 | tail -30
```

Expected: all existing tests pass — especially the `page_turn_tests` mod (which simulates page-forward/backward over Troilus and Cressida). Any failure here means `visible_range`'s behavior diverged from one of the original four loops.

If `page_turn_tests` fails: revert with `git checkout src/input/navigation.rs`, diagnose the divergence (likely a missed `any_nonzero` heuristic or a height-edge case), and re-do the relevant inline replacement.

- [ ] **Step 7: Manual verification**

Paste the Manual Verification Protocol (top of plan) into chat. Stop and wait for the user.

For this task, the user should pay particular attention to:
- Pages display the same number of lines as before (no off-by-one)
- Trailing speaker names still hide at page bottoms (no dangling SPEAKER:)
- Last visible line is not clipped (descender guard still works)
- Bottom clip overlay sized correctly (no excess gap, no overlap with text)

- [ ] **Step 8: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/navigation.rs && git commit -m "$(cat <<'EOF'
Collapse four height-summing loops into one visible_range call

Replaces the inner loops of last_fully_visible_line, is_line_fully_visible,
update_bottom_clip, and lines_per_page with calls to the visible_range
kernel + trim_trailing_speakers (where applicable). Caller-specific short-
circuits (loading_work, widget_height<=0, empty buffer) stay in the
callers — they aren't part of the shared kernel.

Mirrors foliate-js getVisibleRange (paginator.js:94-151) being the single
visibility computation in that codebase. Future descender / speaker-trim /
block-atom rules now land in one place; closes the bug class behind
d7f34dd, 7559eb5, 5f6c475, 2467a01.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Final verification and merge readiness

**Files:** none modified.

This task confirms the F1+F2 batch is done and the working tree is clean.

- [ ] **Step 1: Confirm clean tree**

```bash
cd ~/utono/linux-lit && git status
```

Expected: `nothing to commit, working tree clean`.

- [ ] **Step 2: Confirm test suite passes**

```bash
cd ~/utono/linux-lit && cargo test 2>&1 | tail -15
```

Expected: all tests pass. New mods: `page_turn_lock_tests` (5 tests), `visible_range_helpers_tests` (6 tests). Existing `page_turn_tests` still green.

- [ ] **Step 3: Confirm no new warnings**

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | grep "^warning" | sort -u
```

Compare to the warning list from before this work. Expected: no new warnings introduced. The `dead_code` warnings on `visible_range` / `VisibleRange` / `trim_trailing_speakers` from after Task 5 should be gone after Task 6.

- [ ] **Step 4: Confirm commit log**

```bash
cd ~/utono/linux-lit && git log --oneline -7
```

Expected order (most recent first):
1. `Collapse four height-summing loops into one visible_range call`
2. `Add VisibleRange + visible_range + trim_trailing_speakers helpers`
3. `Short-circuit scroll_paragraph_to_top when page-turn lock is held`
4. `Gate set_page on PageTurnLock to prevent animated-turn re-entrancy`
5. `Add page_turn_lock to AppState`
6. `Add PageTurnLock state machine with unit tests`

(Six commits total. Order matters: Task 2's struct field must precede Tasks 3–4 that reference it; Task 4 depends on Task 3's lock semantics; Tasks 5 and 6 are independent of the lock work but depend on each other in that order.)

- [ ] **Step 5: Final user signoff**

Output to chat:

> "F1+F2 implementation complete. Six commits on the current branch. Manual verification protocols passed. Ready to merge to master via /git:merge, or continue with another finding (F3 relocate event, F4 sync clip + cache, F5 descender via Pango, F6 resize observer) — your call."

Do not invoke `/git:merge` or any other command. Wait for the user.

---

## Self-Review

**Spec coverage:**
- F1 (page-turn lock): Tasks 1, 2, 3, 4. Lock primitive + AppState field + `set_page` gate + `scroll_paragraph_to_top` gate. Mirrors foliate's `#turnPage`/`goTo` pattern. ✓
- F2 (single visible_range): Tasks 5, 6. Kernel function + trim helper + four call-site replacements. Cache (mentioned in F2's "Refactor toward reference" closing line) is deferred to F4 — explicitly noted in plan Architecture section. ✓

**Placeholder scan:** No "TBD", "TODO", "implement later", or "similar to Task N". Every code block contains the actual code. The Manual Verification Protocol is reproduced inline. The kernel and trim functions appear in full in Task 5; the four replacement bodies appear in full in Task 6 — even the duplicated boilerplate (`widget_height = state.text_view.height(); if widget_height <= 0 …`) is kept in callers as the plan specifies. ✓

**Type / API consistency:**
- `PageTurnLock::try_acquire` / `release` / `is_locked` — Task 1 defines, Task 2 wraps in `Rc`, Task 3 calls `try_acquire` and `release`, Task 4 calls `is_locked`. Names match. ✓
- `state.page_turn_lock: Rc<PageTurnLock>` — Task 2 adds, Tasks 3–4 reference. `Rc::clone` is the conventional form (e.g. `std::rc::Rc::clone(&state.page_turn_lock)` in connect_done closures). ✓
- `VisibleRange { last_fit: usize, total_height: i32, count: usize }` — Task 5 defines, Task 6 uses `range.last_fit`, `range.total_height`, `range.count`, `trimmed.total_height`, `trimmed.last_fit`. All access patterns match the struct fields. ✓
- `visible_range(text_view, buffer, page_top, line_count, usable_height) -> VisibleRange` — Task 5 defines, Task 6 calls with these exact arg names/order. ✓
- `trim_trailing_speakers(range, page_top, text_view, buffer) -> VisibleRange` — Task 5 defines, Task 6 calls with these args. The pure variant `trim_trailing_speakers_pure(range, page_top, is_speaker_or_blank, line_height) -> VisibleRange` is only used by tests. ✓

**Scope discipline:**
- Plan stays within F1+F2. Cache deferred to F4. Resize handling deferred to F6. Descender Pango query deferred to F5. Plan's Architecture section lists each deferred item by F-number with explanation. ✓
- No drive-by refactors to unrelated functions. The four replaced functions get equivalent behavior in fewer lines; their public/private visibility, signatures, and short-circuits are preserved. ✓

**Notes for the executor:**
- The skill's TDD requirement applies fully to Tasks 1 and 5 (pure-Rust units). Tasks 2, 3, 4, 6 use `cargo build` + `cargo test` (the existing `page_turn_tests` mod exercises real text) + manual smoke test, per linux-lit's CLAUDE.md convention. The skill's Instruction Priority section authorizes this divergence.
- Task 6's `update_bottom_clip` rewrite includes a `buf.downcast::<sourceview5::Buffer>()` step that wasn't in the original. If this introduces a runtime warning or panic in practice, the alternative is to change `visible_range` and `trim_trailing_speakers` to take `&gtk4::TextBuffer` instead — at the cost of losing `iter_at_line` returning `Option` (TextBuffer's iter_at_line returns the iter directly). The downcast keeps the kernel typed for sourceview's API, which is what every other existing call site uses.
- Manual verification on Tasks 3, 4, 6: do NOT skip. Do NOT proceed to commit without the user's "verified" reply.
- The previous version of this plan (now `2026-04-28-page-turn-lock-and-sync-clip.OLD.md`) bundled F1 with the *old* F2 (synchronous clip + cache, now F4). That work is now in scope of a future plan that will build on this one.
