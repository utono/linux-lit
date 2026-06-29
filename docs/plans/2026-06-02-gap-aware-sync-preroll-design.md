# Gap-Aware Sync Preroll

## Problem

During MPV audio playback, the reading cursor follows the audio: for the
current `time_pos`, `find_line_for_time` (`src/mpv/client.rs:284`) selects the
most recent dialogue line whose `start_time <= time_pos + SYNC_PREROLL`, then
emits `CursorSync(idx)`. `SYNC_PREROLL` is currently `0.0`, so the highlight
moves to a line exactly when its `start_time` is reached and rests there until
the next line's `start_time`.

When two consecutive dialogue lines are separated by a long silent gap (e.g. a
pause, a scene beat, or stage business), the highlight sits on the just-finished
line through the entire gap and only advances at the next line's `start_time` —
the moment the next line is already being spoken. The reader has no visual lead
into the upcoming line.

## Goal

When the gap between the current line's `end_time` and the next line's
`start_time` is long enough (> 2 s), advance the highlight to the next line
**1 s before** it begins (`next.start - 1.0`) rather than waiting for
`next.start`. Back-to-back dialogue (gap ≤ 2 s) is unaffected — those
transitions still happen at `next.start`, so rapid exchanges are not disrupted.

## Behavior Summary

For a transition from line A (current) to line B (immediately next):

- Compute `gap = B.start - A.end`.
- If `gap > SYNC_GAP_THRESHOLD` (2.0 s) **and** `time_pos >= B.start - SYNC_GAP_PREROLL` (B.start − 1.0 s):
  treat B as the active line (early jump).
- Otherwise: keep current behavior — B becomes active at `B.start`.

The highlight **stays on line A** through the gap until the early jump fires.
There is no empty-highlight / no-line state.

Only the immediately-next line gets the early treatment. Lines are never
skipped.

## Where the change lives

All logic is in `find_line_for_time` in `src/mpv/client.rs`. This function is
the single place that maps audio time → active line index; it already owns the
`(line_id, start_time, end_time)` timestamp triples and the `partition_point`
selection. The gap rule belongs here.

The `MpvEvent::CursorSync` handler in `src/main.rs` needs **no change** — it
simply receives the `CursorSync(idx)` event one second earlier for qualifying
gapped transitions, and its existing highlight/page-advance/scene-scroll logic
runs as normal.

The `pending_advance` path (untimestamped next lines, handled in the `TimePos`
branch of `src/main.rs`) is **untouched**. This feature only affects
timestamped → timestamped transitions, where both `A.end` and `B.start` are
known.

## Implementation

### Constants

Add to `src/input/navigation.rs`, beside the existing `SYNC_PREROLL`:

```rust
/// Minimum silent gap (seconds) between a line's end and the next line's
/// start required to trigger an early jump to the next line during sync.
pub const SYNC_GAP_THRESHOLD: f64 = 2.0;

/// Seconds before the next line's start_time to jump the highlight when the
/// preceding gap exceeds SYNC_GAP_THRESHOLD.
pub const SYNC_GAP_PREROLL: f64 = 1.0;
```

### find_line_for_time

Current implementation:

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

New implementation. After computing the normal active index `idx` (so
`timestamps[idx - 1]` is the current line A and `timestamps[idx]`, if present,
is the next line B), check whether B qualifies for an early jump:

```rust
fn find_line_for_time(
    time_pos: f64,
    timestamps: &[(i64, f64, f64)],
    line_id_to_index: &HashMap<i64, usize>,
) -> Option<usize> {
    use crate::input::navigation::{SYNC_PREROLL, SYNC_GAP_THRESHOLD, SYNC_GAP_PREROLL};

    let effective_time = time_pos + SYNC_PREROLL;
    let idx = timestamps.partition_point(|ts| ts.1 <= effective_time);
    if idx == 0 {
        return None;
    }

    // Gap-aware early jump: if the current line (A) and the next line (B)
    // are separated by a gap longer than SYNC_GAP_THRESHOLD, advance to B
    // SYNC_GAP_PREROLL seconds before B.start. Requires a valid A.end
    // (end > start); otherwise the gap is unknown and we fall back to normal.
    let mut active = idx - 1;
    if let Some(&(_, b_start, _)) = timestamps.get(idx) {
        let (_, a_start, a_end) = timestamps[idx - 1];
        let a_end_valid = a_end > a_start;
        if a_end_valid {
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

Notes:
- `timestamps` is start-sorted, so `timestamps[idx]` is the next line by start
  time. `partition_point` guarantees `timestamps[idx].start > effective_time`,
  i.e. B has not yet started under the normal rule.
- The early jump promotes `active` from `idx - 1` to `idx` (exactly one line),
  so it can never skip a line.
- Because the function is called on every `TimePos` tick, once `time_pos`
  crosses `b_start - SYNC_GAP_PREROLL` the function keeps returning B, and
  continues to return B after `b_start` under the normal rule — the transition
  is stable, no flicker.

## Edge Cases

- **Missing / zero `end_time` on A** (`a_end <= a_start`): gap is unknown; fall
  back to normal behavior (no early jump). This matches the `ts.end > ts.start`
  guard already used in `src/main.rs:277`.
- **No next line** (`idx == timestamps.len()`): `timestamps.get(idx)` is `None`;
  normal behavior.
- **Gap ≤ 2 s**: normal behavior — back-to-back dialogue unaffected.
- **Untimestamped next dialogue line**: not represented in `timestamps`, so this
  path doesn't apply; the existing `pending_advance` mechanism handles it.

## Testing

Extend `test_find_line_for_time` (or add a sibling test) in
`src/mpv/client.rs`:

- **Gap case**: `timestamps = [(10, 1.0, 2.0), (20, 6.0, 7.0)]` (A ends 2.0, B
  starts 6.0 → gap 4.0 s > 2.0). Assert:
  - At `time_pos = 4.9` → still A (`Some(0)`), since `6.0 - 1.0 = 5.0` not yet
    reached.
  - At `time_pos = 5.0` → B (`Some(1)`), early jump fires.
  - At `time_pos = 6.5` → B (`Some(1)`), normal rule.
- **No-gap case**: `timestamps = [(10, 1.0, 2.0), (20, 3.0, 4.0)]` (gap 1.0 s ≤
  2.0). Assert at `time_pos = 2.5` → still A (`Some(0)`); at `time_pos = 3.0` →
  B (`Some(1)`). Confirms back-to-back transitions are unchanged.
- **Invalid A.end case**: `timestamps = [(10, 1.0, 1.0), (20, 6.0, 7.0)]`
  (`a_end == a_start`). Assert at `time_pos = 5.0` → still A (`Some(0)`) — no
  early jump because the gap is unknown.

Run:

```bash
cargo test
cargo build
```

The user runs `cargo run` to verify end-to-end against a play with a known
silent gap between two timestamped dialogue lines (the highlight should move to
the upcoming line ~1 s before it is spoken, only across long gaps).
