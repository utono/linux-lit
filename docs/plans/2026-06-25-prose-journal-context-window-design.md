# Window prose journal-Q&A context to ±10 paragraphs

**Date:** 2026-06-25
**Status:** Design approved

## Problem

The journal Q&A `Scene` and `Passage` bands send `scene_text_for(div1, div2)`
— the FULL text of the current `(div1, div2)` division — to the Claude API as
context (`src/input/actions/journal.rs:314,316`).

For a **prose** work this is a disaster: prose imports as a single division. The
Cromwell biography is one division `(1, 0)` of **4,133 paragraphs / ~1.44 M
characters (~370 k tokens)**. So every journal Q&A on a prose work ships the
entire book — which may exceed the model's context window outright, and is
extremely slow/expensive per question. (Plays are fine: a `(div1, div2)` is one
short scene, a few KB.)

## Goal

For a prose work, send only the reader's surrounding context: the cursor's
paragraph ±10 paragraphs (21 total), clamped to the division. Plays are
unchanged (the whole scene is the right context). Bounded token cost regardless
of prose-division/chapter length.

## Design

### New windowed text builder (`src/app/scene_synopsis.rs`)

Add alongside `scene_text_for`:

```rust
/// Like `scene_text_for`, but for PROSE works returns only the paragraphs
/// around `anchor_work_line` (±`radius`, clamped to the division). Plays and
/// other non-prose works return the full `scene_text_for` (a real scene is
/// small and the whole scene is the intended context). `radius` paragraphs
/// each side of the anchor, so up to `2*radius + 1` paragraphs total.
pub fn scene_text_windowed(
    state: &AppState,
    div1: i64,
    div2: i64,
    anchor_work_line: usize,
    radius: usize,
) -> String
```

Behavior:
- **Non-prose** (`!is_prose_work(work.work_type)`): return
  `scene_text_for(state, div1, div2)` verbatim. No behavior change for plays.
- **Prose:** collect the work-line INDICES (into `work.lines`) of the division
  `(div1, div2)` in order. Find the position of `anchor_work_line` in that list
  (the anchor paragraph). Take the sub-range `[pos - radius, pos + radius]`
  saturating-clamped to the division's bounds. Render those paragraphs with the
  SAME speaker-interleave logic `scene_text_for` uses (speaker label printed when
  it changes, then the paragraph text). Return that string.
- **Anchor not in the division / not resolvable:** fall back to anchor = the
  first paragraph of the division (so the window is the division's opening ±
  radius — never empty, never out of range).

`radius` is passed by the caller; the journal path uses a module const
`PROSE_CONTEXT_RADIUS: usize = 10`.

### Anchor = the reader's position

In `ask_claude` (`journal.rs`), the anchor is the reader's line when the journal
overlay was opened — `s.journal.return_pos.0` (the saved `current_line`), mapped
to a work-line via `state.work_line_for_buffer`. That is "where the reader was
looking," the natural focus for a question. If `return_pos` is None (shouldn't
happen on the ask path) or the map fails, fall back as above (division start).

Resolve the anchor inside the same initial `state.borrow()` block that already
computes `scene_text` (journal.rs ~304-327), so no extra borrow.

### Wire it into the journal bands

`journal.rs:312-318` builds `scene_text`. Change the `Scene` and `Passage` arms
to call `scene_text_windowed(&s, d1, d2, anchor, PROSE_CONTEXT_RADIUS)` instead
of `scene_text_for`. The `Work` band is unchanged (it already sends no text).
The Passage band still appends `passage_source_text` after the (now windowed)
scene_text — the passage itself is small and stays.

`scene_text_for` itself is UNCHANGED and stays (it has only these two callers
today, but keep it as the play/full-scene primitive the windowed fn delegates
to).

## Out of scope (separate, complementary)

- **Cromwell chapter re-import (litdb).** Re-dividing Cromwell into per-chapter
  `(div1, div2)` (detecting the inconsistent "CHAPTER N" / "Chapter N." headings)
  improves navigation + synopsis granularity. It is a litdb DATA migration, NOT
  part of this code change, and the two are independent: **prose always windows
  to ±10 regardless of chapter length** (decided), so the journal context stays
  bounded whether or not Cromwell is re-divided. Spec'd separately.

## Testing

### Unit (pure-ish, `cargo test --bins`)

`scene_text_windowed`'s paragraph-selection math is the testable core. Extract
the index-window arithmetic into a tiny pure helper so it can be unit-tested
without GTK:

```rust
/// The inclusive index range of paragraphs to include: anchor ± radius, clamped
/// to [0, n). Returns (lo, hi) with lo <= hi < n. n is the division's paragraph count.
fn window_range(anchor_pos: usize, radius: usize, n: usize) -> (usize, usize)
```

Tests:
- anchor in the middle → exactly `2*radius+1` paragraphs.
- anchor near 0 → clamped low (e.g. anchor=2, radius=10 → (0, 12)).
- anchor near end → clamped high.
- division smaller than the window → whole division.
- n == 0 edge (empty division) → handled (no panic).

### Integration / data-gated (lit.db present)

A `#[ignore]`-or-skip-gated test: load Cromwell (prose, single division), call
`scene_text_windowed` with an anchor in the middle, assert the result is far
smaller than `scene_text_for` (e.g. < 5% of the full-division length) and
contains the anchor paragraph's text. Load a play (e.g. 2H6), assert
`scene_text_windowed == scene_text_for` for a scene (plays unchanged).

### Visual / user-run (per CLAUDE.md)

The agent does not run the app. The user verifies: open Cromwell, position the
reader mid-chapter, press A (journal Scene band), submit a question — confirm it
returns promptly (no whole-book upload) and the answer is about the surrounding
passage. The dev log's user_msg length / the Claude request size is the
observable signal that context shrank.

## Files

- `src/app/scene_synopsis.rs` — `scene_text_windowed`, the pure `window_range`
  helper + its tests.
- `src/input/actions/journal.rs` — resolve the anchor; route the Scene/Passage
  bands through `scene_text_windowed`; `PROSE_CONTEXT_RADIUS` const.
