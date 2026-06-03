# Gap-Aware Sync Preroll Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** During MPV playback, advance the reading cursor to the next dialogue line 1 second early, but only when a silent gap longer than 2 seconds separates it from the current line.

**Architecture:** All logic lives in `find_line_for_time` in `src/mpv/client.rs` — the single function that maps audio `time_pos` to the active dialogue line index. After it picks the normal active line (`idx - 1`), it peeks at the next line (`idx`); if the gap from the current line's `end_time` to the next line's `start_time` exceeds a threshold and the audio is within the preroll window, it promotes the active line to the next one. Two tunable constants live beside the existing `SYNC_PREROLL` in `src/input/navigation.rs`. No other code changes — the `CursorSync` event handler in `src/main.rs` simply fires one second earlier for qualifying transitions.

**Tech Stack:** Rust, MPV IPC (`src/mpv/client.rs`), GTK4 (event handling, unchanged here). Tests are standard `#[cfg(test)]` unit tests run with `cargo test`.

---

## Context for the implementer

You do not need to understand the whole app. You only touch two files:

- `src/input/navigation.rs` — holds sync-related constants. You add two `pub const` lines near line 60.
- `src/mpv/client.rs` — holds `find_line_for_time` (around line 284) and its unit test `test_find_line_for_time` (around line 320). You modify the function and add a new test.

Background on the data: `timestamps` is a `Vec<(i64, f64, f64)>` of `(line_id, start_time, end_time)`, **sorted ascending by `start_time`**. `find_line_for_time` is called on every MPV time-position tick. It returns `Option<usize>` — the index into the work's line list for the line that should be highlighted now, or `None` before the first line.

The current function:

```rust
fn find_line_for_time(
    time_pos: f64,
    timestamps: &[(i64, f64, f64)],
    line_id_to_index: &HashMap<i64, usize>,
) -> Option<usize> {
    let effective_time = time_pos + crate::input::navigation::SYNC_PREROLL;
    let idx = timestamps.partition_point(|ts| ts.1 <= effective_time);
    if idx == 0 {
        return None;
    }
    let (line_id, _, _) = timestamps[idx - 1];
    line_id_to_index.get(&line_id).copied()
}
```

`partition_point(|ts| ts.1 <= effective_time)` returns the count of timestamps whose `start_time` is `<= effective_time`. So `timestamps[idx - 1]` is the current/active line A, and `timestamps[idx]` (if it exists) is the next line B, which has not yet started.

The tuple fields are `(line_id, start_time, end_time)` = `(ts.0, ts.1, ts.2)`.

---

## File Structure

- **Modify:** `src/input/navigation.rs` — add `SYNC_GAP_THRESHOLD` and `SYNC_GAP_PREROLL` constants.
- **Modify:** `src/mpv/client.rs` — gap-aware logic in `find_line_for_time`.
- **Test:** `src/mpv/client.rs` — new `test_find_line_for_time_gap_aware` test in the existing `#[cfg(test)] mod tests`.

No new files. The existing `test_find_line_for_time` test stays valid (its lines are 1 s apart, below the 2 s threshold, so the new logic does not change its outcomes).

---

## Task 1: Add the tuning constants

**Files:**
- Modify: `src/input/navigation.rs:60` (after the existing `SYNC_PREROLL`)

- [ ] **Step 1: Add the two constants**

Open `src/input/navigation.rs`. Immediately after the existing `SYNC_PREROLL` constant (the line `pub const SYNC_PREROLL: f64 = 0.0;`, around line 60), insert:

```rust
/// Minimum silent gap (seconds) between a line's end_time and the next
/// line's start_time required to trigger an early jump to the next line
/// during MPV playback sync. Gaps at or below this keep normal timing.
pub const SYNC_GAP_THRESHOLD: f64 = 2.0;

/// Seconds before the next line's start_time to jump the highlight when
/// the preceding gap exceeds SYNC_GAP_THRESHOLD.
pub const SYNC_GAP_PREROLL: f64 = 1.0;
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: builds successfully. (Two unused constants will NOT warn, because `pub` items in a binary crate don't trigger dead-code warnings here; even if a warning appears, the build succeeds. They are used in Task 2.)

- [ ] **Step 3: Commit**

```bash
git add src/input/navigation.rs
git commit -m "sync: add SYNC_GAP_THRESHOLD and SYNC_GAP_PREROLL constants

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Gap-aware early jump in find_line_for_time

**Files:**
- Modify: `src/mpv/client.rs:284-296` (the `find_line_for_time` function)
- Test: `src/mpv/client.rs` (new test in `mod tests`, after `test_find_line_for_time`)

- [ ] **Step 1: Write the failing test**

In `src/mpv/client.rs`, inside the `#[cfg(test)] mod tests { ... }` block, add this new test immediately after the existing `test_find_line_for_time` function (after its closing `}`, before the `mod tests` closing `}`):

```rust
    #[test]
    fn test_find_line_for_time_gap_aware() {
        // A: id 10, start 1.0, end 2.0. B: id 20, start 6.0, end 7.0.
        // Gap = 6.0 - 2.0 = 4.0 > 2.0 threshold -> early jump applies.
        let gap = vec![(10, 1.0, 2.0), (20, 6.0, 7.0)];
        let map: HashMap<i64, usize> = [(10, 0), (20, 1)].into();

        // Before the preroll window (B.start - 1.0 = 5.0): still on A.
        assert_eq!(find_line_for_time(4.9, &gap, &map), Some(0));
        // At the preroll boundary: jump to B early.
        assert_eq!(find_line_for_time(5.0, &gap, &map), Some(1));
        // After B actually starts: still B (normal rule).
        assert_eq!(find_line_for_time(6.5, &gap, &map), Some(1));

        // No-gap case: A ends 2.0, B starts 3.0 -> gap 1.0 <= 2.0, no early jump.
        let nogap = vec![(10, 1.0, 2.0), (20, 3.0, 4.0)];
        assert_eq!(find_line_for_time(2.5, &nogap, &map), Some(0));
        assert_eq!(find_line_for_time(3.0, &nogap, &map), Some(1));

        // Invalid A.end (end == start): gap unknown -> no early jump.
        let badend = vec![(10, 1.0, 1.0), (20, 6.0, 7.0)];
        assert_eq!(find_line_for_time(5.0, &badend, &map), Some(0));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib test_find_line_for_time_gap_aware`
Expected: FAIL. The assertion `find_line_for_time(5.0, &gap, &map) == Some(1)` fails — the current function returns `Some(0)` because it has no early-jump logic.

- [ ] **Step 3: Implement the gap-aware logic**

Replace the entire `find_line_for_time` function (around lines 284-296) with:

```rust
fn find_line_for_time(
    time_pos: f64,
    timestamps: &[(i64, f64, f64)],
    line_id_to_index: &HashMap<i64, usize>,
) -> Option<usize> {
    use crate::input::navigation::{SYNC_GAP_PREROLL, SYNC_GAP_THRESHOLD, SYNC_PREROLL};

    let effective_time = time_pos + SYNC_PREROLL;
    let idx = timestamps.partition_point(|ts| ts.1 <= effective_time);
    if idx == 0 {
        return None;
    }

    // Gap-aware early jump: if the current line A (timestamps[idx - 1]) and
    // the next line B (timestamps[idx]) are separated by a gap longer than
    // SYNC_GAP_THRESHOLD, advance to B SYNC_GAP_PREROLL seconds before B's
    // start. Requires a valid A.end (end > start); otherwise the gap is
    // unknown and we keep normal timing. Promotes by exactly one line, so a
    // line is never skipped.
    let mut active = idx - 1;
    if let Some(&(_, b_start, _)) = timestamps.get(idx) {
        let (_, a_start, a_end) = timestamps[idx - 1];
        if a_end > a_start {
            let gap = b_start - a_end;
            if gap > SYNC_GAP_THRESHOLD && time_pos >= b_start - SYNC_GAP_PREROLL {
                active = idx;
            }
        }
    }

    let (line_id, _, _) = timestamps[active];
    line_id_to_index.get(&line_id).copied()
}
```

- [ ] **Step 4: Run the new test to verify it passes**

Run: `cargo test --lib test_find_line_for_time_gap_aware`
Expected: PASS.

- [ ] **Step 5: Run the existing test to verify no regression**

Run: `cargo test --lib test_find_line_for_time`
Expected: PASS for both `test_find_line_for_time` and `test_find_line_for_time_gap_aware` (the name filter matches both). The original test's timestamps are 1 s apart (gaps of 1.0 ≤ 2.0), so the new logic does not change its outcomes.

- [ ] **Step 6: Run the full test suite and build**

Run: `cargo test`
Expected: all tests pass.

Run: `cargo build`
Expected: builds successfully, no warnings about unused `SYNC_GAP_THRESHOLD` / `SYNC_GAP_PREROLL` (they are now used).

- [ ] **Step 7: Commit**

```bash
git add src/mpv/client.rs
git commit -m "sync: early-jump cursor across long silent gaps

When the gap between a dialogue line's end and the next line's start
exceeds SYNC_GAP_THRESHOLD (2s), advance the highlight to the next line
SYNC_GAP_PREROLL (1s) before it begins. Back-to-back dialogue is
unaffected. Lives in find_line_for_time; no event-handler changes.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Manual verification

**Files:** none (user-run).

- [ ] **Step 1: Hand off to the user for runtime verification**

`cargo run` is dev-only and the user runs it themselves (never run it from the agent). Tell the user:

1. Open a play with timestamped dialogue that has a known silent gap between two consecutive spoken lines (e.g. across a stage-business beat or a pause).
2. Press Tab to start MPV playback and let it approach that gap.
3. Watch for: the highlight stays on the current line through the gap, then jumps to the upcoming line about 1 second before it is spoken — but only across gaps longer than ~2 seconds. Rapid back-to-back exchanges should look unchanged (transitions still land as the line begins).

If the early jump fires on short back-to-back lines, or never fires across a long gap, capture `~/utono/linux-lit/linux-lit-dev.log` and re-open the `debug-playback-sync` skill.

---

## Self-Review Notes

- **Spec coverage:** The rule (gap > 2s → jump at next.start − 1s), the two named constants, the single-function location, the "stay on current line through gap" behavior (achieved by only promoting `active` when within the preroll window), the untouched `pending_advance`/`main.rs` paths, and all three edge cases (invalid `A.end`, no next line via `timestamps.get(idx)`, gap ≤ threshold) are each implemented and tested. The spec's three test scenarios map to the three blocks of `test_find_line_for_time_gap_aware`.
- **Placeholders:** none — every code and command step is concrete.
- **Type consistency:** `find_line_for_time` signature unchanged; constants typed `f64` matching `SYNC_PREROLL`; tuple field access `(_, a_start, a_end)` / `(_, b_start, _)` matches the `(i64, f64, f64)` shape used throughout the file and in the existing test.
