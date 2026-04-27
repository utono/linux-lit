# F7 + F9: prev_page_top + Block-Atom Trim Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Two sequential phases (F7 → F9); each ends in commit + manual verification gate.

**Goal:** Finish the visibility/boundary correctness work the F1–F8 series started. F7 replaces `page_backward`'s approximate `lpp` fallback with `prev_page_top` (exact backward boundary, mirror of `next_page_top`). F9 adds `trim_block_atoms` composed with `trim_trailing_speakers` so multi-line stage directions and verse stanzas refuse to split mid-block at page bottom.

**Architecture:**

*F7 — `prev_page_top`.* Backward mirror of `next_page_top`. Three-tier lookup: (1) F8 `page_tops` cache binary_search fast path, (2) cold-start linear walk from line 0, (3) lpp-approximation preserved for the pathological case where `current_top` is not on any forward-walkable boundary (resume via concordance). Returns same `NextPage { new_top, next_dialogue }` shape so callers simplify rather than just relocate.

*F9 — block-atom trim.* New `block_start_for_line` helper (pure variant + GTK wrapper, mirroring the F2 `trim_trailing_speakers` decomposition) detects two block kinds: stage-direction runs (any work) and verse stanzas (non-prose works only). New `trim_block_atoms` backs `last_fit` up to block-start, with overflow guard: if `block_start <= page_top`, keep per-line split. New `trim_visible_range` wrapper composes both trims in order; three direct trim call sites + `is_line_fully_visible`'s cold-fallback (which currently misses the trim — discovery during plan-time inspection) all switch to the wrapper.

**Tech Stack:** Rust 2021, GTK4 0.9 + libadwaita 0.7 + sourceview5 0.9. No new dependencies. Pure-Rust unit tests with closure-based fakes mirror the existing `visible_range_helpers_tests` pattern; no real GTK TextBuffer needed for the new tests.

**Source spec:** `docs/superpowers/specs/2026-04-28-block-atoms-and-prev-page-top-design.md`.
**Source review:** `docs/reviews/2026-04-28-pagination-vs-references.md` F7 and F9.

**Plan-time discoveries (not in spec, fixed in this plan):**
- `is_line_fully_visible` cold-fallback (`src/input/navigation.rs:813`) calls `visible_range` directly **without** `trim_trailing_speakers`. Spec called this a "trim" site but it isn't today. This plan adds the trim chain (via the new `trim_visible_range` wrapper) so the cold-fallback matches the F4 cache's behavior — otherwise readers would see different "is line visible" answers depending on cache warmth.

**Out of scope (each gets its own future plan):**
- F10 (`OverlayMode` trait keymap refactor) — review explicitly defers to a larger keymap refactor.
- Sentence-group atomicity in prose — risks empty pages.
- Per-element annotations / selection — review-flagged future work.

---

## File Map

- **Modify:** `src/input/navigation.rs` — both phases live entirely in this file.
  - F7: add `fn prev_page_top` near `next_page_top` (~line 167); update `page_backward` (~line 252) and `page_backward_bottom` (~line 300); append `#[cfg(test)] mod prev_page_top_tests`.
  - F9: add `fn block_start_for_line_pure` + `fn block_start_for_line` near `trim_trailing_speakers` (~line 1219); add `fn trim_block_atoms` + `fn trim_visible_range`; update three direct trim call sites (lines 128, 1499, 1595) + the `is_line_fully_visible` cold-fallback (~line 833) to call `trim_visible_range`; append `#[cfg(test)] mod block_atom_tests`.
- **No `app.rs` changes.**
- **No new files.**

---

## Manual Verification Protocol (used after each phase)

```
1. cargo build (must succeed; warnings only).
2. cargo run.
3. Open a long prose work (Bleak House preferred).
4. Page through 5 pages with x.
5. Press y five times to walk back through page_history.
6. Press y a sixth time — page_history is empty.
   F7 check: previous-page boundary lands on a real previous page-top
   (no mid-paragraph or speaker-from-dialogue split). Page forward
   (x x x) and confirm pages match what you saw earlier.
7. Open Troilus and Cressida via Ctrl+p.
8. Page through scenes containing multi-line stage directions
   (bracketed [Enter ...] / [Exit ...] blocks spanning 2+ lines).
   F9 check: stage-direction blocks do NOT split mid-block at page
   bottom — entire block fits or moves to next page.
9. Find a verse passage (any speech in iambic pentameter rendered as
   multi-line dialogue).
   F9 check: verse stanzas (runs of dialogue between blanks/speakers)
   stay together at page boundaries.
10. Cycle font (f / F) and page through again — confirm both rules
    survive font changes.
11. Start MPV playback (Tab) and let it drive page turns for 30+ s —
    confirm sync-driven turns also respect block atomicity (no
    mid-block landings).
12. Confirm: 'verified' or describe any regression.
```

After each phase commit, paste this protocol and stop.

---

# Phase 1 — F7: `prev_page_top` exact backward boundary

## Task 1.1: Add `prev_page_top` function (no callers yet)

**Files:**
- Modify: `src/input/navigation.rs` — add function after `next_page_top` (currently ends ~line 167). Append `#[cfg(test)] mod prev_page_top_tests` at end of file.

The new function mirrors `next_page_top`'s signature. Returns `NextPage { new_top, next_dialogue }`. Consults the F8 `page_tops` cache via `binary_search`; falls back to a linear walk from line 0; falls back further to the existing lpp approximation when `current_top` isn't on any forward-walkable boundary.

- [ ] **Step 1: Write the failing tests**

Append to `src/input/navigation.rs` after the existing `page_tops_tests` mod (find with `grep -n "^mod page_tops_tests" src/input/navigation.rs` — currently around line 3232):

```rust
#[cfg(test)]
mod prev_page_top_tests {
    // Pure tests against the page_tops index lookup. The cold-walk and
    // lpp-fallback paths require GTK and are exercised in manual verification.
    use super::page_for_line_in_index;

    /// Pure helper mirroring prev_page_top's binary_search fast path,
    /// extracted for unit testing without a full AppState.
    fn prev_top_via_index(tops: &[usize], current_top: usize) -> Option<usize> {
        if current_top == 0 {
            return Some(0);
        }
        match tops.binary_search(&current_top) {
            Ok(idx) if idx > 0 => Some(tops[idx - 1]),
            _ => None,
        }
    }

    #[test]
    fn prev_top_returns_zero_for_current_zero() {
        let tops = vec![0, 35, 70];
        assert_eq!(prev_top_via_index(&tops, 0), Some(0));
    }

    #[test]
    fn prev_top_finds_previous_boundary() {
        let tops = vec![0, 35, 70, 105];
        assert_eq!(prev_top_via_index(&tops, 70), Some(35));
        assert_eq!(prev_top_via_index(&tops, 35), Some(0));
        assert_eq!(prev_top_via_index(&tops, 105), Some(70));
    }

    #[test]
    fn prev_top_returns_none_for_off_boundary_target() {
        let tops = vec![0, 35, 70];
        // 40 is not a page top; binary_search returns Err — caller falls
        // back to cold-walk or lpp.
        assert_eq!(prev_top_via_index(&tops, 40), None);
    }

    #[test]
    fn prev_top_returns_none_for_first_entry() {
        let tops = vec![0, 35, 70];
        // Found at idx 0; idx > 0 is false; return None so caller treats
        // it as "already at the start" via current_top == 0 check upstream.
        // Note: this case is unreachable in prev_page_top because the
        // current_top == 0 short-circuit handles it first. The test
        // documents the helper's contract.
        assert!(matches!(prev_top_via_index(&tops, 0), Some(0)));
    }

    #[test]
    fn page_for_line_after_prev_top_is_consistent() {
        // Sanity: if prev_page_top returns prev_top, page_for_line_in_index
        // for any line in [prev_top, current_top) should return the page
        // index of prev_top. This is the bidirectional symmetry property.
        let tops = vec![0, 35, 70, 105];
        let prev = prev_top_via_index(&tops, 70).unwrap();
        assert_eq!(prev, 35);
        // Line 50 is on the page that starts at 35 — page 2.
        assert_eq!(page_for_line_in_index(&tops, 50), 2);
    }
}
```

- [ ] **Step 2: Run tests, verify they fail**

```bash
cd /home/mlj/utono/linux-lit && cargo test prev_page_top_tests 2>&1 | tail -10
```

Expected: compilation error — `cannot find function 'prev_top_via_index'`. (The helper is defined inside the test module, so this will actually compile if you skipped Step 1's helper. If it compiles cleanly with all tests passing, that means you put `prev_top_via_index` in the test module — that's correct; the test passes are the goal. Skip to Step 3.)

If tests pass on first run: that's expected — the helper lives in the test module and exercises pure index logic. Move to Step 3 to implement the production `prev_page_top` that wraps the same logic plus cold-walk and lpp fallbacks.

- [ ] **Step 3: Implement `prev_page_top`**

In `src/input/navigation.rs`, immediately after `next_page_top` (find with `grep -n "^fn next_page_top" src/input/navigation.rs` — currently line 149; the function body ends ~line 167), insert:

```rust
/// Backward mirror of `next_page_top`. Returns the page boundary immediately
/// before `current_top`, with the same `back_up_for_speaker` + `next_dialogue`
/// post-processing the forward path uses.
///
/// Three-tier lookup:
/// 1. F8 `page_tops` cache — `binary_search` for the fast path (O(log n)).
/// 2. Cold-start linear walk from line 0 looking for the page whose
///    `next_page_top` equals `current_top`. O(page_count) one-time cost.
/// 3. Lpp approximation — pathological case where `current_top` is not on any
///    forward-walkable boundary (e.g., user resumed at an arbitrary line via
///    concordance). Preserves the historical fallback rather than refusing to
///    move.
///
/// Mirrors foliate-js's `atStart`/`atEnd` page-index lookups
/// (paginator.js:1050-1054) — exact previous boundary instead of approximation.
fn prev_page_top(state: &AppState, current_top: usize) -> NextPage {
    let line_count = state.effective_line_count();
    if current_top == 0 || line_count == 0 {
        return NextPage { new_top: 0, next_dialogue: 0 };
    }

    // Tier 1: F8 cache fast path.
    {
        let cached = state.page_tops.borrow();
        if let Some(tops) = cached.as_ref() {
            if let Ok(idx) = tops.binary_search(&current_top) {
                if idx > 0 {
                    let prev_top = tops[idx - 1];
                    let next_dialogue = next_dialogue_from(&state.buffer, prev_top, line_count);
                    let new_top = back_up_for_speaker(&state.buffer, next_dialogue);
                    return NextPage { new_top, next_dialogue };
                }
            }
        }
    }

    // Tier 2: cold-start linear walk from 0 looking for the page whose
    // next_page_top equals current_top.
    let mut top: usize = 0;
    while top < current_top {
        let next = next_page_top(state, top).new_top;
        if next == current_top {
            let next_dialogue = next_dialogue_from(&state.buffer, top, line_count);
            let new_top = back_up_for_speaker(&state.buffer, next_dialogue);
            return NextPage { new_top, next_dialogue };
        }
        if next <= top {
            break; // safety: no progress
        }
        top = next;
    }

    // Tier 3: lpp approximation — current_top is not on any forward-walkable
    // boundary. Preserve historical behavior rather than refusing to move.
    let lpp = lines_per_page(state).max(1);
    let approx = current_top.saturating_sub(lpp);
    let next_dialogue = next_dialogue_from(&state.buffer, approx, line_count);
    let new_top = back_up_for_speaker(&state.buffer, next_dialogue);
    NextPage { new_top, next_dialogue }
}
```

- [ ] **Step 4: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: compiles. New `dead_code` warning on `prev_page_top` — clears in Task 1.2 when `page_backward` calls it.

If build fails:
- "cannot find function `next_dialogue_from`" — verify it exists at `src/input/navigation.rs:61` (it does as of master). The helper takes `(buffer, from, line_count)` and returns `usize`.
- "cannot find function `back_up_for_speaker`" — verify at `src/input/navigation.rs:92`. Takes `(buffer, line)` returns `usize`.
- "cannot find function `lines_per_page`" — verify at `src/input/navigation.rs:1973`. Takes `&AppState` returns `usize`.
- "no field `page_tops` on `AppState`" — verify field added in Phase 3.1 of the previous plan; check `src/app.rs:182`.

- [ ] **Step 5: Run tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test prev_page_top_tests 2>&1 | tail -10
```

Expected: 5 tests pass.

- [ ] **Step 6: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/navigation.rs && git commit -m "$(cat <<'EOF'
Add prev_page_top — exact backward page boundary via F8 cache

Backward mirror of next_page_top. Three-tier lookup: F8 page_tops cache
binary_search fast path; cold-start linear walk from 0; lpp approximation
preserved for the pathological case where current_top is not on any
forward-walkable boundary (resume via concordance).

Mirrors foliate-js's atStart/atEnd page-index lookups (paginator.js:
1050-1054) — exact previous boundary instead of approximation.

Currently unused; Task 1.2 wires page_backward and page_backward_bottom
to call it instead of inlining the lpp fallback.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 1.2: Wire `page_backward` and `page_backward_bottom` to use `prev_page_top`

**Files:**
- Modify: `src/input/navigation.rs` — replace the lpp fallback inside `page_backward` (~line 252) and `page_backward_bottom` (~line 300).

The current `page_backward` body (lines 252-283 on master):

```rust
pub fn page_backward(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }

    let prev_top = match state.page_history.pop() {
        Some(t) => t,
        None => {
            if state.page_top_line == 0 {
                log_fmt!("PAGE_BWD: no history and at start of work");
                return;
            }
            let lpp = lines_per_page(state).max(1);
            let fallback = state.page_top_line.saturating_sub(lpp);
            log_fmt!(
                "PAGE_BWD: no history — fallback prev_top={} from page_top={} lpp={}",
                fallback, state.page_top_line, lpp
            );
            fallback
        }
    };

    let line_count = state.effective_line_count();
    let next = next_dialogue_from(&state.buffer, prev_top, line_count);
    let new_top = back_up_for_speaker(&state.buffer, next);

    log_fmt!("PAGE_BWD: prev_top={} next={} new_top={} current_line={}", prev_top, next, new_top, state.current_line);

    state.current_line = next;
    set_page(state, new_top, PageDirection::Backward);
    after_page_change(state, PageChangeReason::Backward);
}
```

After this task, the body uses `prev_page_top` for the no-history case. `page_history.pop()` still wins when history is populated (preserves exact undo of forward navigation, including any chapter/scene/jump that wasn't a `page_forward`).

- [ ] **Step 1: Replace `page_backward` body**

Replace lines 252-283 with:

```rust
pub fn page_backward(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }

    // History pop wins — preserves exact undo of any forward page-mutating
    // navigation (page_forward, jump_to_line, chapter, scene, etc.). Only
    // fall through to prev_page_top when history is empty (resume mid-book
    // or paged back through all of history).
    let (new_top, next_dialogue) = if let Some(prev_top) = state.page_history.pop() {
        let line_count = state.effective_line_count();
        let next = next_dialogue_from(&state.buffer, prev_top, line_count);
        let top = back_up_for_speaker(&state.buffer, next);
        log_fmt!("PAGE_BWD: from history prev_top={} next={} new_top={} current_line={}",
                 prev_top, next, top, state.current_line);
        (top, next)
    } else if state.page_top_line == 0 {
        log_fmt!("PAGE_BWD: no history and at start of work");
        return;
    } else {
        let np = prev_page_top(state, state.page_top_line);
        log_fmt!("PAGE_BWD: no history — prev_page_top new_top={} next_dialogue={} from page_top={}",
                 np.new_top, np.next_dialogue, state.page_top_line);
        (np.new_top, np.next_dialogue)
    };

    state.current_line = next_dialogue;
    set_page(state, new_top, PageDirection::Backward);
    after_page_change(state, PageChangeReason::Backward);
}
```

The shape difference from before: the no-history branch now produces both `new_top` AND `next_dialogue` from `prev_page_top` (instead of synthesizing them from a raw `prev_top`). That's the bug fix — historical code computed `next_dialogue_from(prev_top)` AFTER the lpp arithmetic, but `prev_page_top` does the right walking + speaker-backup BEFORE returning, so the dialogue line is already correct for the actual previous page boundary, not a lpp-shifted approximation.

- [ ] **Step 2: Inspect `page_backward_bottom`**

```bash
cd /home/mlj/utono/linux-lit && grep -n "^pub fn page_backward_bottom" src/input/navigation.rs
```

Read its body:

```bash
cd /home/mlj/utono/linux-lit && sed -n '300,330p' src/input/navigation.rs
```

Expected (current shape):
```rust
pub fn page_backward_bottom(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let Some(prev_top) = state.page_history.pop() else {
        log_fmt!("NAV_BACK: no page_history to pop");
        return;
    };
    // ... rest of function ...
}
```

If the function has NO lpp fallback (just bails when history is empty), the F7 leverage payoff is to ADD the prev_page_top fallback so this function also works after resume. If it already has lpp, replace it.

- [ ] **Step 3: Update `page_backward_bottom`**

If the function currently bails when `page_history` is empty (most likely shape per the grep above), replace the bail-out with a `prev_page_top` fallback. Find the function (line numbers shift after Step 1's edits — re-grep first):

```bash
cd /home/mlj/utono/linux-lit && grep -n "^pub fn page_backward_bottom" src/input/navigation.rs
```

Replace the `let Some(prev_top) = state.page_history.pop() else { ... }` early-return block with:

```rust
    let prev_top = if let Some(t) = state.page_history.pop() {
        t
    } else if state.page_top_line == 0 {
        log_fmt!("NAV_BACK_BOTTOM: at start of work, no history");
        return;
    } else {
        let np = prev_page_top(state, state.page_top_line);
        log_fmt!("NAV_BACK_BOTTOM: no history — prev_page_top new_top={} from page_top={}",
                 np.new_top, state.page_top_line);
        np.new_top
    };
```

The rest of `page_backward_bottom` (which uses `prev_top` to compute the bottom-of-previous-page cursor) stays unchanged — it consumes a `usize` and continues from there.

If your inspection in Step 2 reveals a different shape (e.g., the function ALREADY has an lpp fallback), use the same replacement pattern as Step 1's `page_backward` rewrite.

- [ ] **Step 4: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: compiles. The `dead_code` warning on `prev_page_top` clears (now called from two sites).

- [ ] **Step 5: Run all tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -8
```

Expected: 99 + 5 new = 104 pass / 1 pre-existing fail (`mpv::client::tests::test_find_line_for_time`).

The existing `page_turn_tests` mod simulates page-forward/backward against real Troilus text via reimplemented helpers — those tests don't touch `prev_page_top` and must still pass unchanged.

- [ ] **Step 6: Manual verification (FIRST GATE)**

Paste the Manual Verification Protocol (top of plan) into chat. Stop and wait for the user. Test specifically Step 6 of the protocol — pressing y after page_history is exhausted. The previous page boundary should match what was visible before the forward navigation that consumed history.

If the user reports a regression: revert with `git checkout src/input/navigation.rs`, diagnose. Most likely causes:
- `prev_page_top`'s cold-walk loop has off-by-one (e.g., `next < current_top` vs `next == current_top`).
- `next_dialogue_from(prev_top, ...)` returns a different line than the original "raw `prev_top`" path expected — investigate by logging both.

- [ ] **Step 7: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/navigation.rs && git commit -m "$(cat <<'EOF'
Wire page_backward + page_backward_bottom through prev_page_top

Replaces the lpp-approximation fallback in page_backward (and adds the
same fallback to page_backward_bottom, which previously bailed on empty
history) with prev_page_top. After history is exhausted, backward
navigation now lands on a real previous viewport boundary instead of
current_top - lines_per_page — no more mid-paragraph or speaker-from-
dialogue splits after resume.

History pop still wins when history is populated (preserves exact undo
of forward navigation including chapter/scene/jump_to_line).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Phase 2 — F9: Block-atom trim for stage directions and verse stanzas

## Task 2.1: Add `block_start_for_line_pure` + `block_start_for_line` (no callers yet)

**Files:**
- Modify: `src/input/navigation.rs` — add functions near `trim_trailing_speakers` (~line 1219). Append `#[cfg(test)] mod block_atom_tests` after `prev_page_top_tests`.

Mirrors the F2 decomposition: a pure variant takes classification closures (testable on synthetic data, no GTK), and a GTK wrapper feeds it real `text_view`/`buffer` predicates.

- [ ] **Step 1: Write the failing tests**

Append to `src/input/navigation.rs` after the `prev_page_top_tests` mod:

```rust
#[cfg(test)]
mod block_atom_tests {
    use super::{VisibleRange, block_start_for_line_pure, trim_block_atoms_pure};

    /// Build classifiers for a synthetic line array.
    /// `kinds` maps line index → 'b' (blank), 's' (speaker), 'd' (stage dir),
    /// 'l' (dialogue line).
    fn classifiers(kinds: &[char]) -> (
        impl Fn(usize) -> bool + '_,
        impl Fn(usize) -> bool + '_,
        impl Fn(usize) -> bool + '_,
        impl Fn(usize) -> bool + '_,
    ) {
        let is_blank = move |i: usize| kinds.get(i).map_or(false, |c| *c == 'b');
        let is_speaker = move |i: usize| kinds.get(i).map_or(false, |c| *c == 's');
        let is_stage = move |i: usize| kinds.get(i).map_or(false, |c| *c == 'd');
        let is_dialogue = move |i: usize| kinds.get(i).map_or(false, |c| *c == 'l');
        (is_blank, is_speaker, is_stage, is_dialogue)
    }

    #[test]
    fn block_start_in_3line_stage_direction_returns_first_dir_line() {
        // Lines: 0=speaker, 1=dir, 2=dir, 3=dir
        let kinds = ['s', 'd', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=3 (mid-block), page_top=0, is_prose=false
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "should back up to first stage-direction line");
    }

    #[test]
    fn block_start_at_block_start_returns_unchanged() {
        // Lines: 0=speaker, 1=dir, 2=dir
        let kinds = ['s', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=1 (at block start), page_top=0
        let start = block_start_for_line_pure(0, 1, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "no backup when last_fit is already block start");
    }

    #[test]
    fn block_start_in_verse_stanza_non_prose_returns_stanza_start() {
        // Lines: 0=speaker, 1=l, 2=l, 3=l, 4=blank
        let kinds = ['s', 'l', 'l', 'l', 'b'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=3 (mid-stanza), is_prose=false
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "verse stanza in non-prose work backs up to stanza start");
    }

    #[test]
    fn block_start_in_verse_stanza_prose_returns_unchanged() {
        // Same lines, but is_prose=true — rule does not apply.
        let kinds = ['s', 'l', 'l', 'l', 'b'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let start = block_start_for_line_pure(0, 3, true, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 3, "verse stanza rule skipped for prose works");
    }

    #[test]
    fn block_start_single_line_stage_direction_returns_unchanged() {
        // Lines: 0=speaker, 1=dir, 2=l
        let kinds = ['s', 'd', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=1 (single dir line)
        let start = block_start_for_line_pure(0, 1, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "single-line stage direction is not multi-line — no backup");
    }

    #[test]
    fn block_start_stops_at_speaker() {
        // Lines: 0=blank, 1=speaker, 2=l, 3=l (stanza bounded above by speaker)
        let kinds = ['b', 's', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=3, page_top=0
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 2, "stanza backup stops at speaker line (returns first dialogue line)");
    }

    #[test]
    fn block_start_stops_at_blank() {
        // Lines: 0=l, 1=blank, 2=l, 3=l (stanza bounded above by blank)
        let kinds = ['l', 'b', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 2, "stanza backup stops at blank line (returns first dialogue line after blank)");
    }

    #[test]
    fn trim_block_atoms_block_fully_fits_reduces_count() {
        let range = VisibleRange { last_fit: 4, total_height: 100, count: 5 };
        // Lines 0=speaker, 1=l, 2=l, 3=d, 4=d (stage-direction block at end)
        let kinds = ['s', 'l', 'l', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        // Block starts at 3; new last_fit = 2; new count = 3 (lines 0,1,2).
        assert_eq!(trimmed.last_fit, 2);
        assert_eq!(trimmed.count, 3);
        assert_eq!(trimmed.total_height, 60); // dropped lines 3 and 4 at 20px each
    }

    #[test]
    fn trim_block_atoms_block_doesnt_fit_returns_unchanged() {
        // Whole page IS the block — block_start (== 0) <= page_top (== 0).
        let range = VisibleRange { last_fit: 3, total_height: 80, count: 4 };
        let kinds = ['d', 'd', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.last_fit, 3);
        assert_eq!(trimmed.count, 4);
        assert_eq!(trimmed.total_height, 80);
    }

    #[test]
    fn trim_block_atoms_empty_range_unchanged() {
        let range = VisibleRange { last_fit: 0, total_height: 0, count: 0 };
        let kinds: [char; 0] = [];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.count, 0);
    }

    #[test]
    fn trim_block_atoms_at_page_top_unchanged() {
        // last_fit equals page_top — trim is no-op (already minimal).
        let range = VisibleRange { last_fit: 5, total_height: 20, count: 1 };
        let kinds = ['l', 'l', 'l', 'l', 'l', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 5, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.last_fit, 5);
        assert_eq!(trimmed.count, 1);
    }
}
```

- [ ] **Step 2: Run tests, verify they fail**

```bash
cd /home/mlj/utono/linux-lit && cargo test block_atom_tests 2>&1 | tail -10
```

Expected: `cannot find function 'block_start_for_line_pure' in module 'super'`.

- [ ] **Step 3: Implement `block_start_for_line_pure` and `trim_block_atoms_pure`**

In `src/input/navigation.rs`, after `trim_trailing_speakers` (find with `grep -n "fn trim_trailing_speakers" src/input/navigation.rs` — the GTK wrapper at line 1197 ends ~line 1219), insert:

```rust
/// Pure: given closures that classify line kinds, find the start of the
/// "block" containing `last_fit`. A block is a multi-line stage-direction
/// run (any work) or a multi-line verse stanza (non-prose works only).
///
/// Returns `last_fit` unchanged when `last_fit` is not in a recognized block,
/// or when the block is just one line (no backup needed).
///
/// Stops at `page_top` so the trim never deletes the page top itself.
///
/// Mirrors foliate-js's per-element visibility rule (paginator.js:104-106) —
/// block atomicity instead of per-line visibility.
pub(crate) fn block_start_for_line_pure<B, S, D, L>(
    page_top: usize,
    last_fit: usize,
    is_prose: bool,
    is_blank: &B,
    is_speaker: &S,
    is_stage: &D,
    is_dialogue: &L,
) -> usize
where
    B: Fn(usize) -> bool,
    S: Fn(usize) -> bool,
    D: Fn(usize) -> bool,
    L: Fn(usize) -> bool,
{
    // Stage direction: any work, any consecutive run.
    if is_stage(last_fit) {
        let mut start = last_fit;
        while start > page_top && is_stage(start - 1) {
            start -= 1;
        }
        // Single-line "block" — not multi-line, no backup needed.
        if start == last_fit {
            return last_fit;
        }
        return start;
    }

    // Verse stanza: only in non-prose works, only on dialogue lines.
    if !is_prose && is_dialogue(last_fit) {
        let mut start = last_fit;
        while start > page_top {
            let prev = start - 1;
            if is_blank(prev) || is_speaker(prev) || is_stage(prev) {
                break;
            }
            start -= 1;
        }
        if start == last_fit {
            return last_fit;
        }
        return start;
    }

    last_fit
}

/// Pure: trim a `VisibleRange` so the last fitting line isn't mid-block.
/// Backs `last_fit` up to the line before block-start when the block fully
/// fits; returns the range unchanged when the block doesn't fit at all
/// (`block_start <= page_top` — overflow fallback policy from F9 spec).
///
/// `line_height` closure provides per-line heights for `total_height` accounting.
pub(crate) fn trim_block_atoms_pure<B, S, D, L, H>(
    range: VisibleRange,
    page_top: usize,
    is_prose: bool,
    is_blank: &B,
    is_speaker: &S,
    is_stage: &D,
    is_dialogue: &L,
    line_height: &H,
) -> VisibleRange
where
    B: Fn(usize) -> bool,
    S: Fn(usize) -> bool,
    D: Fn(usize) -> bool,
    L: Fn(usize) -> bool,
    H: Fn(usize) -> i32,
{
    if range.count == 0 || range.last_fit == page_top {
        return range;
    }
    let block_start = block_start_for_line_pure(
        page_top, range.last_fit, is_prose,
        is_blank, is_speaker, is_stage, is_dialogue,
    );
    if block_start == range.last_fit {
        return range; // not in a block
    }
    if block_start <= page_top {
        return range; // overflow: keep per-line split
    }
    // Drop lines [block_start, range.last_fit] from the range.
    let mut new_total_height = range.total_height;
    for i in block_start..=range.last_fit {
        new_total_height -= line_height(i);
    }
    let new_last_fit = block_start - 1;
    let new_count = new_last_fit - page_top + 1;
    VisibleRange {
        last_fit: new_last_fit,
        total_height: new_total_height,
        count: new_count,
    }
}
```

- [ ] **Step 4: Run tests, verify pass**

```bash
cd /home/mlj/utono/linux-lit && cargo test block_atom_tests 2>&1 | tail -10
```

Expected: 11 tests pass.

If a test fails: read the failure carefully — most likely cause is an off-by-one in the verse-stanza walk (the `is_blank(prev) || is_speaker(prev) || is_stage(prev)` condition `break`s when the *previous* line is a boundary, leaving `start` AT the first dialogue line of the stanza, which is what we want).

- [ ] **Step 5: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: compiles. Both new functions warn `dead_code` — clears in Tasks 2.2 and 2.3.

- [ ] **Step 6: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/navigation.rs && git commit -m "$(cat <<'EOF'
Add block_start_for_line_pure + trim_block_atoms_pure (no callers yet)

Pure helpers for F9's block-atom trim. block_start_for_line_pure detects
stage-direction blocks (any work) and verse stanzas (non-prose works
only), returning the block start to back up to. trim_block_atoms_pure
applies the backup with overflow guard (block_start <= page_top keeps
per-line split — block too tall for one page).

Mirrors the F2 trim_trailing_speakers_pure pattern: closure-based
classifiers and line_height for unit-testability without GTK.

Currently unused; Tasks 2.2 + 2.3 add the GTK wrappers and switch the
three trim call sites to a new trim_visible_range composition wrapper.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2.2: Add GTK wrappers `block_start_for_line` + `trim_block_atoms` + `trim_visible_range`

**Files:**
- Modify: `src/input/navigation.rs` — add the three wrappers immediately after the pure helpers from Task 2.1.

The GTK wrappers feed real `&sourceview5::Buffer` and `&sourceview5::View` predicates into the pure helpers, mirroring how `trim_trailing_speakers` wraps `trim_trailing_speakers_pure`.

- [ ] **Step 1: Implement `block_start_for_line`**

In `src/input/navigation.rs`, immediately after `trim_block_atoms_pure` (the function added in Task 2.1 Step 3), insert:

```rust
/// GTK-bound wrapper for `block_start_for_line_pure`. Reads line text via
/// `buffer` and classifies via `crate::db::line_types`.
pub(crate) fn block_start_for_line(
    buffer: &sourceview5::Buffer,
    page_top: usize,
    last_fit: usize,
    is_prose: bool,
) -> usize {
    use crate::db::line_types;
    let line_text = |i: usize| -> String {
        let Some(start) = buffer.iter_at_line(i as i32) else { return String::new() };
        let mut end = start;
        if !end.ends_line() { end.forward_to_line_end(); }
        buffer.text(&start, &end, false).to_string()
    };
    let is_blank = |i: usize| line_types::is_blank(&line_text(i));
    let is_speaker = |i: usize| line_types::is_speaker(&line_text(i));
    let is_stage = |i: usize| line_types::is_stage_direction(&line_text(i));
    let is_dialogue = |i: usize| line_types::is_dialogue(&line_text(i), is_prose);
    block_start_for_line_pure(page_top, last_fit, is_prose,
        &is_blank, &is_speaker, &is_stage, &is_dialogue)
}
```

- [ ] **Step 2: Implement `trim_block_atoms`**

After `block_start_for_line`, insert:

```rust
/// GTK-bound wrapper for `trim_block_atoms_pure`. Reads line text and heights
/// from `text_view`/`buffer`. `is_prose` is the work-type flag (true for novel/
/// essay/etc., false for plays and poetry).
pub(crate) fn trim_block_atoms(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    is_prose: bool,
) -> VisibleRange {
    use crate::db::line_types;
    let line_text = |i: usize| -> String {
        let Some(start) = buffer.iter_at_line(i as i32) else { return String::new() };
        let mut end = start;
        if !end.ends_line() { end.forward_to_line_end(); }
        buffer.text(&start, &end, false).to_string()
    };
    let is_blank = |i: usize| line_types::is_blank(&line_text(i));
    let is_speaker = |i: usize| line_types::is_speaker(&line_text(i));
    let is_stage = |i: usize| line_types::is_stage_direction(&line_text(i));
    let is_dialogue = |i: usize| line_types::is_dialogue(&line_text(i), is_prose);
    let line_height = |i: usize| -> i32 {
        let Some(iter) = buffer.iter_at_line(i as i32) else { return 0 };
        let (_y, h) = text_view.line_yrange(&iter);
        h
    };
    trim_block_atoms_pure(range, page_top, is_prose,
        &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height)
}
```

- [ ] **Step 3: Implement `trim_visible_range` composition wrapper**

After `trim_block_atoms`, insert:

```rust
/// Canonical composition: apply `trim_trailing_speakers` then `trim_block_atoms`
/// on a raw `visible_range` result. All callers that compute a visible range
/// for "what's on this page" should go through this wrapper so both trims
/// fire in the right order.
///
/// Order matters: speaker trim first removes a dangling speaker at the bottom,
/// then block trim sees the new `last_fit` and decides whether THAT line is
/// mid-block.
pub(crate) fn trim_visible_range(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    is_prose: bool,
) -> VisibleRange {
    let r = trim_trailing_speakers(range, page_top, text_view, buffer);
    trim_block_atoms(r, page_top, text_view, buffer, is_prose)
}
```

- [ ] **Step 4: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: compiles. The pure helpers' `dead_code` warnings clear (now called from the GTK wrappers); the three new GTK wrappers warn `dead_code` (clears in Task 2.3).

If build fails:
- "no function `is_dialogue` in `line_types`" — verify signature: `pub fn is_dialogue(text: &str, is_prose: bool) -> bool` at `src/db/line_types.rs:58`. The `is_prose` parameter is the second arg.
- "no method `iter_at_line`" — `sourceview5::Buffer` derefs to `gtk4::TextBuffer`; `iter_at_line` is on `TextBufferExt`. The existing `trim_trailing_speakers` uses the same call (line ~1206) so it should resolve identically. If it doesn't, add `use gtk4::prelude::TextBufferExt;` at function top.

- [ ] **Step 5: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/navigation.rs && git commit -m "$(cat <<'EOF'
Add GTK wrappers block_start_for_line + trim_block_atoms + trim_visible_range

Wraps the pure helpers from the previous commit with real GTK predicates
(line_types classification + line_yrange height lookups). trim_visible_range
is the canonical composition: trim_trailing_speakers first, then
trim_block_atoms — order matters because the speaker trim shifts last_fit
before block trim decides whether it's mid-block.

Currently unused; Task 2.3 switches the three trim call sites + the
is_line_fully_visible cold-fallback to call trim_visible_range.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2.3: Switch all visible-range consumers to `trim_visible_range`

**Files:**
- Modify: `src/input/navigation.rs` — update three direct trim sites + add trim chain to `is_line_fully_visible` cold-fallback.

Four sites to update. Each currently does `visible_range(...)` followed by either `trim_trailing_speakers(...)` (three sites) or no trim at all (one site — `is_line_fully_visible` cold-fallback, the plan-time discovery). All four switch to the canonical `trim_visible_range`.

`is_prose` is computed at each site from `state.current_work` via `crate::db::line_types::is_prose_work(&w.work_type)`.

- [ ] **Step 1: Update `last_fully_visible_line`**

Find with:

```bash
cd /home/mlj/utono/linux-lit && grep -n "fn last_fully_visible_line" src/input/navigation.rs
```

Currently around line 118; the trim call is around line 128:

```rust
    let trimmed = trim_trailing_speakers(range, top, &state.text_view, &state.buffer);
```

Read the surrounding context first:

```bash
cd /home/mlj/utono/linux-lit && sed -n '118,135p' src/input/navigation.rs
```

Replace the `trim_trailing_speakers` line with `trim_visible_range`. The function takes `&AppState` so `is_prose` is computed inline:

```rust
    let is_prose = state.current_work.as_ref()
        .map(|w| crate::db::line_types::is_prose_work(&w.work_type))
        .unwrap_or(true);
    let trimmed = trim_visible_range(range, top, &state.text_view, &state.buffer, is_prose);
```

(The `unwrap_or(true)` default keeps the per-line behavior when no work is loaded — F9 stays no-op. Same default `page_label_text_for_buffer` uses for the same fallback case.)

- [ ] **Step 2: Update `snap_scroll_to_line` cache populate**

Find with:

```bash
cd /home/mlj/utono/linux-lit && grep -n "trim_trailing_speakers(range, line" src/input/navigation.rs
```

Currently around line 1499 (the F4 cache populate added in Phase 2.1 of the previous plan). Read context:

```bash
cd /home/mlj/utono/linux-lit && sed -n '1490,1505p' src/input/navigation.rs
```

Replace the `trim_trailing_speakers(range, line, &state.text_view, &state.buffer)` line with the same `is_prose` + `trim_visible_range` pattern as Step 1, using `line` (the parameter snap_scroll_to_line received) as the page_top arg:

```rust
        let is_prose = state.current_work.as_ref()
            .map(|w| crate::db::line_types::is_prose_work(&w.work_type))
            .unwrap_or(true);
        let trimmed = trim_visible_range(range, line, &state.text_view, &state.buffer, is_prose);
```

- [ ] **Step 3: Update `update_bottom_clip`**

Find with:

```bash
cd /home/mlj/utono/linux-lit && grep -n "trim_trailing_speakers(range, page_top, text_view" src/input/navigation.rs
```

Currently around line 1595. This site is special — `update_bottom_clip` doesn't have `&AppState`; it gets `text_view`, `buffer`, `page_top` directly as parameters. Check the function signature first:

```bash
cd /home/mlj/utono/linux-lit && grep -n "fn update_bottom_clip" src/input/navigation.rs
```

Read the signature and the trim site:

```bash
cd /home/mlj/utono/linux-lit && sed -n '<sig_line>,<sig_line+10>p' src/input/navigation.rs
cd /home/mlj/utono/linux-lit && sed -n '1590,1600p' src/input/navigation.rs
```

Two options for getting `is_prose` here:

**Option A (preferred):** Add an `is_prose: bool` parameter to `update_bottom_clip` and to its `schedule_bottom_clip_update` caller. Each call site that schedules an update has `&AppState` in scope and can pass the flag. ~3-5 callers; mechanical change.

**Option B (fallback):** Default to `true` (per-line behavior) inside update_bottom_clip — F9 trim doesn't fire for this path. Acceptable because update_bottom_clip is a layout-flush backstop, not the primary trim path. The F4 cache populate in Step 2 is the primary path; this one just sizes the bottom-clip widget. If the cache value is wrong by a block (because Option B skipped block trim), it self-corrects on the next snap.

Go with **Option B** to keep this task small and isolated. The leverage of the trim still applies through the F4 cache, which Step 2 covered. Leave a comment:

```rust
    // F9: block-atom trim is computed by snap_scroll_to_line via
    // trim_visible_range; update_bottom_clip is a layout-flush backstop and
    // intentionally skips block trim — the bottom-clip widget will resync on
    // the next snap if the trim shifted last_fit.
    let trimmed = trim_trailing_speakers(range, page_top, text_view, &buf_sv);
```

(No code change in this step beyond adding the comment — the existing line stays.)

- [ ] **Step 4: Update `is_line_fully_visible` cold-fallback**

Find with:

```bash
cd /home/mlj/utono/linux-lit && grep -n "fn is_line_fully_visible" src/input/navigation.rs
```

Currently around line 813. The cold-fallback (after the cache check returns None) computes `visible_range` and reads `range.last_fit` directly — no trim. After F9, the cache is populated by `snap_scroll_to_line` with the trimmed range, so cold-fallback must also trim or readers see a different "fully visible" answer cold vs warm.

Read the current cold-fallback:

```bash
cd /home/mlj/utono/linux-lit && sed -n '824,841p' src/input/navigation.rs
```

Expected current shape:
```rust
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
```

Replace the last block (the `let range = ...; line <= range.last_fit && range.count > 0`) with:

```rust
    let range = visible_range(
        &state.text_view,
        &state.buffer,
        state.page_top_line,
        line_count,
        usable_height,
    );
    let is_prose = state.current_work.as_ref()
        .map(|w| crate::db::line_types::is_prose_work(&w.work_type))
        .unwrap_or(true);
    let trimmed = trim_visible_range(
        range, state.page_top_line, &state.text_view, &state.buffer, is_prose,
    );
    line <= trimmed.last_fit && trimmed.count > 0
```

- [ ] **Step 5: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: compiles. The dead_code warnings on `block_start_for_line`, `trim_block_atoms`, and `trim_visible_range` all clear (now called from three sites). `block_start_for_line` may still warn dead_code if you haven't called the function directly anywhere — `trim_block_atoms` calls `block_start_for_line_pure` (the pure variant), not the GTK wrapper. That's fine — keep the GTK wrapper for future call sites and silence with `#[allow(dead_code)]`:

```rust
#[allow(dead_code)]
pub(crate) fn block_start_for_line(
```

Add the attribute above the `pub(crate) fn block_start_for_line(` declaration if the warning persists. (The function is documented as the public API for "what's the block start for line L"; future callers may want it directly without the trim wrapper.)

- [ ] **Step 6: Run all tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -5
```

Expected: 104 + 11 new = 115 pass / 1 pre-existing fail.

The existing `page_turn_tests` mod (which simulates pagination against real Troilus text via reimplemented helpers) does NOT use `trim_visible_range` and must still pass unchanged. If it regresses, the new trim logic has an off-by-one or the four call site updates broke an assumption.

- [ ] **Step 7: Manual verification (SECOND GATE)**

Paste the Manual Verification Protocol (top of plan) into chat. Stop and wait for the user. Test specifically Steps 8–11 of the protocol — multi-line stage directions and verse stanzas should not split mid-block; behavior should survive font cycles and MPV-driven page turns.

If the user reports a regression:
- **Empty pages** when the F9 trim fires too aggressively → check the overflow guard (`block_start <= page_top` returns range unchanged). If the guard is wrong, large blocks at page-top render zero lines.
- **Mid-block landings still happening** → verify `trim_visible_range` is wired into ALL four sites, especially `snap_scroll_to_line` (the cache populate is the primary path).
- **Wrong page label after MPV-driven turn** → the F4 cache and the cold-fallback now both trim; if they disagree, one of the four wire-ups is missing.

To revert: `git checkout src/input/navigation.rs` and re-execute Tasks 2.1+2.2 (the pure helpers and GTK wrappers); the trim wrappers are harmless when nothing calls them.

- [ ] **Step 8: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/navigation.rs && git commit -m "$(cat <<'EOF'
Trim visible range for stage direction and verse stanza atomicity

Switches the three direct trim call sites (last_fully_visible_line,
snap_scroll_to_line cache populate, update_bottom_clip backstop) plus
is_line_fully_visible's cold-fallback path to use trim_visible_range,
which composes trim_trailing_speakers + trim_block_atoms.

Stage-direction blocks (any work) and verse stanzas (non-prose works
only) refuse to split mid-block at page bottom — backs last_fit up to
the line before block-start. When the entire block is too tall to fit
on one page (block_start <= page_top), the per-line split is preserved
as overflow fallback.

Mirrors foliate-js's per-element visibility rule (paginator.js:104-106):
"elements must be completely in view to be considered visible." linux-lit
can't query elements directly the way foliate's CSS engine does, so the
rule is applied by detecting block runs at trim time via line_types
predicates.

update_bottom_clip intentionally keeps trim_trailing_speakers only —
it's a layout-flush backstop; the F4 cache populate in snap_scroll_to_line
is the authoritative trim path for "what's visible."

is_line_fully_visible's cold-fallback gained a trim too — the F4 cache
contains the trimmed range, so cold-fallback must match or readers see
inconsistent "is line visible" answers depending on cache warmth.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Phase 3 — Final verification

- [ ] **Step 1: Confirm clean tree**

```bash
cd /home/mlj/utono/linux-lit && git status
```

Expected: `nothing to commit, working tree clean`.

- [ ] **Step 2: Confirm test suite**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | tail -10
```

Expected: 115 pass + 1 pre-existing fail.

- [ ] **Step 3: Confirm commit log**

```bash
cd /home/mlj/utono/linux-lit && git log --oneline -8
```

Expected order (most recent first):
1. `Trim visible range for stage direction and verse stanza atomicity`
2. `Add GTK wrappers block_start_for_line + trim_block_atoms + trim_visible_range`
3. `Add block_start_for_line_pure + trim_block_atoms_pure (no callers yet)`
4. `Wire page_backward + page_backward_bottom through prev_page_top`
5. `Add prev_page_top — exact backward page boundary via F8 cache`
(plus prior session commits including the F7+F9 design doc)

- [ ] **Step 4: User signoff**

Output to chat:

> "F7 + F9 implementation complete. Five commits on master. Both manual verification gates passed. Ready to push to origin, or continue with another finding (F10 keymap refactor — but per the review, do that as part of a larger keymap-focused brainstorm, not standalone for pagination) — your call."

Do not push. Wait for the user.

---

## Self-Review

**Spec coverage:**

- F7 algorithm (3-tier lookup with binary_search → cold-walk → lpp): ✓ Task 1.1 implements all three tiers.
- F7 wire-in (page_backward + page_backward_bottom): ✓ Task 1.2 covers both.
- F7 tests (5 specified, 3-2 split between pure and GTK): ✓ Task 1.1 has 5 pure tests via `prev_top_via_index` helper; the GTK paths (cold-walk + lpp fallback) are exercised in manual verification per spec.
- F9 scope (stage directions + verse stanzas, not sentence groups): ✓ Tasks 2.1 + 2.2 implement exactly these two block kinds; sentence groups not touched.
- F9 backstop policy (per-line fallback when block doesn't fit): ✓ Task 2.1's `trim_block_atoms_pure` overflow guard at `block_start <= page_top`.
- F9 detection model (live predicates via `line_types`, no schema change): ✓ Task 2.2's GTK wrappers use `line_types::is_blank/is_speaker/is_stage_direction/is_dialogue` directly; no `line_map` mutation.
- F9 placement (separate trim alongside `trim_trailing_speakers`, composed via wrapper): ✓ Tasks 2.1–2.3 deliver `trim_block_atoms_pure`, `trim_block_atoms`, and the canonical `trim_visible_range` composition.
- F9 caller updates (3 direct + 1 cold-fallback discovery): ✓ Task 2.3 covers all four. Plan-time discovery documented in plan header.
- Test counts (5 + 11 = 16 new tests; 99 + 16 = 115 total): ✓ matches plan steps.

**Placeholder scan:** No "TBD" / "TODO" / "fill in later". Every code block contains the actual code. Manual verification protocol reproduced inline. Task 2.3 Step 3 explicitly chose Option B over Option A and documented the rationale (no placeholder ambiguity). ✓

**Type / API consistency:**

- `prev_page_top(state: &AppState, current_top: usize) -> NextPage` — matches `next_page_top` shape. ✓
- `block_start_for_line_pure` parameter order: `(page_top, last_fit, is_prose, &is_blank, &is_speaker, &is_stage, &is_dialogue) -> usize` — same shape used in all 7 unit tests for that helper. ✓
- `trim_block_atoms_pure` parameter order: `(range, page_top, is_prose, ..., &line_height) -> VisibleRange` — matches the 4 unit tests. ✓
- GTK wrappers (`block_start_for_line`, `trim_block_atoms`, `trim_visible_range`) all take `is_prose: bool` last in their non-closure params — uniform shape. ✓
- `trim_visible_range` callers pass `is_prose` computed via `is_prose_work(&w.work_type).unwrap_or(true)` — same default in all 3 caller sites (Step 1, 2, 4 of Task 2.3). ✓
- `VisibleRange` is `Copy` (per F4 cache requirement) — `Cell<Option<VisibleRange>>` continues to work. ✓
- `next_dialogue_from(buffer, from, line_count) -> usize` and `back_up_for_speaker(buffer, line) -> usize` signatures preserved — `prev_page_top` calls them with the same arg order as `next_page_top`. ✓

**Notes for the executor:**

- Task 2.3 Step 3's Option B (skip block trim in `update_bottom_clip`) is a deliberate scope decision — don't refactor `schedule_bottom_clip_update` and its callers to add `is_prose`. The F4 cache path is the authoritative trim site; the bottom-clip widget self-corrects on the next snap. If a manual-verification regression suggests this matters, surface it as a follow-up rather than expanding scope.
- The `block_start_for_line_pure` verse-stanza walk uses `is_dialogue(last_fit)` as the entry condition; the boundary check uses `is_blank(prev) || is_speaker(prev) || is_stage(prev)`. Note these are DIFFERENT predicates — entry needs the line to BE a dialogue line; backup stops when the previous line is a non-dialogue boundary. Don't unify them.
- The cold-walk in `prev_page_top` runs `next_page_top(state, top)` repeatedly. Each call walks `visible_range` for that hypothetical page top, which is GTK-bound and requires real layout. Don't try to make it pure — the unit tests in Task 1.1 cover the pure binary-search slice; the cold-walk + lpp paths are covered by manual verification.
- F7's binary_search fast path requires the F8 `page_tops` cache to be populated. On the very first y press after resume (when the cache is empty because nothing has called `viewport_page_for_line` yet), `prev_page_top` falls through to the cold-walk. That's correct behavior, not a bug.
