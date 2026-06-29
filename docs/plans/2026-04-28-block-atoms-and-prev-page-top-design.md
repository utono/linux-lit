# F7 + F9 Design: Exact Backward Boundary + Block-Atom Trim

**Status:** Approved (brainstorm 2026-04-28).
**Source review:** `docs/reviews/2026-04-28-pagination-vs-references.md` F7 and F9.
**Out of scope:** F10 (`OverlayMode` trait — keymap refactor, ranked last by review; deferred to a separate keymap-focused brainstorm).

---

## Problem

Two pagination correctness issues remain after F1–F8 landed:

**F7 — Approximate backward boundary.** When `page_history` is empty (resume mid-book, or the user paged back through all of history), `page_backward` falls back to `current_top - lines_per_page`. `lpp` is computed from the *current* page's metrics, so the resulting top can land mid-paragraph or split a speaker from dialogue. The forward path always calls `back_up_for_speaker`; the backward fallback skips it.

**F9 — Per-line "fully visible" rule.** `last_fully_visible_line` judges per buffer line. The trailing-speaker trim (added in F2) catches single dangling speakers but not multi-line group continuity. Verse stanzas and multi-line stage directions can split mid-block at the bottom of a page.

Both findings are explicit in the pagination review. F7 is S-effort; F9 is M-effort. Together they finish the visibility/boundary correctness work the F1–F8 series started.

---

## Reference shape

**F7:** Foliate has no equivalent — uses CFI for resume, so backward navigation always lands on a real previous viewport boundary (`paginator.js:1050-1054` `atStart`/`atEnd` use page indices, not approximation). bk has no fallback because chapter-relative line offsets are exact (`bk/src/view.rs:200-207`).

**F9:** `foliate-js/paginator.js:104-106` — "elements must be completely in view to be considered visible." Visibility is judged per *element*, so a stanza or stage-direction block is atomic. linux-lit can't use the per-element CSS query foliate uses, but the per-block *rule* translates directly: detect block runs at trim time and back up `last_fit` to the block's start.

---

## F7: `prev_page_top` — exact backward page boundary

### Algorithm

```
fn prev_page_top(state: &AppState, current_top: usize) -> NextPage:
    if current_top == 0:
        return NextPage { new_top: 0, next_dialogue: 0 }

    // Fast path — F8 page_tops cache populated.
    if let Some(tops) = state.page_tops.borrow().as_ref():
        if let Ok(idx) = tops.binary_search(&current_top):
            if idx > 0:
                let prev_top = tops[idx - 1]
                let next_dialogue = next_dialogue_from(buffer, prev_top, line_count)
                let new_top = back_up_for_speaker(buffer, next_dialogue)
                return NextPage { new_top, next_dialogue }

    // Cold-start fallback: linear walk from 0 looking for the page whose
    // next_page_top equals current_top. O(page_count) one-time cost.
    let mut top = 0
    while top < current_top:
        let next = next_page_top(state, top).new_top
        if next == current_top:
            let next_dialogue = next_dialogue_from(buffer, top, line_count)
            let new_top = back_up_for_speaker(buffer, next_dialogue)
            return NextPage { new_top, next_dialogue }
        if next <= top:
            break  // safety: no progress
        top = next

    // Pathological case: current_top is not on any forward-walkable
    // boundary (e.g., user resumed at an arbitrary line via concordance).
    // Preserve existing approximate fallback rather than refusing to move.
    let lpp = lines_per_page(state).max(1)
    let approx = current_top.saturating_sub(lpp)
    let next_dialogue = next_dialogue_from(buffer, approx, line_count)
    let new_top = back_up_for_speaker(buffer, next_dialogue)
    NextPage { new_top, next_dialogue }
```

### Wire-in

`page_backward` and `page_backward_bottom` replace their `lpp`-based fallback block (currently 6+ lines) with a single `prev_page_top(state, state.page_top_line)` call. The existing `next_dialogue_from + back_up_for_speaker` call inside each function moves *into* `prev_page_top` (it already does both internally), so call-site bodies shrink, not just relocate.

### Tests

Pure-Rust unit tests in a new `prev_page_top_tests` mod. Index-only tests are pure on synthetic `Vec<usize>`; tests requiring `next_page_top` are GTK-bound and may be deferred to manual verification.

1. `current_top == 0` → returns `{ 0, 0 }`.
2. Index `[0, 35, 70]`, `current_top == 70` → returns `{ 35, ... }` (binary_search hit).
3. Index `[0, 35, 70]`, `current_top == 35` → returns `{ 0, ... }`.
4. (GTK) cold-start no cache, `current_top` on a real boundary → linear walk finds previous boundary.
5. (GTK) `current_top` not on any forward-walkable boundary → falls back to lpp approximation; smoke test only.

### Effort

S (one new function ~40 LOC, two call site updates ~6 LOC, 3 pure unit tests + 2 manual-verification cases).

---

## F9: Block-atom trim for stage directions and verse stanzas

### Scope

Two block kinds are atomic (refuse to split mid-block at page bottom):
1. **Stage directions** — runs of consecutive `is_stage_direction(text)` lines (any work).
2. **Verse stanzas** — runs of consecutive non-blank, non-speaker, non-stage-direction dialogue lines bounded above by a blank/speaker/direction line. **Only in non-prose works** (plays, poetry); prose paragraphs are not block-atomic — that path would risk empty pages on long prose paragraphs.

Sentence groups (already in `line_map`) are NOT made atomic — would risk empty pages on long sentences and exceeds the review's intent.

### Backstop policy when block doesn't fit

Best-effort. If the entire block is taller than `usable_height` (block-start ≤ page_top), keep the per-line split (current behavior). The descender guard (F5) owns the visual cleanup. Foliate handles this via CSS overflow; linux-lit can't, so the fallback is "split mid-block on overflow" rather than "render zero lines."

### Block detection

Live, no schema change. Block boundaries are detected at trim time using existing `line_types` predicates (`is_blank`, `is_speaker`, `is_stage_direction`, `is_dialogue`).

```
fn block_start_for_line(buffer, page_top, last_fit, is_prose) -> usize:
    let text = line_text(buffer, last_fit)

    // Verse stanza: only in non-prose works, only on dialogue lines.
    if !is_prose && is_dialogue(text, false):
        let mut start = last_fit
        while start > page_top:
            let prev_text = line_text(buffer, start - 1)
            if is_blank(prev_text) || is_speaker(prev_text) || is_stage_direction(prev_text):
                break
            start -= 1
        if start == last_fit:
            return last_fit  // single-line "block" — not multi-line
        return start

    // Stage direction: any work, any consecutive run.
    if is_stage_direction(text):
        let mut start = last_fit
        while start > page_top:
            let prev_text = line_text(buffer, start - 1)
            if !is_stage_direction(prev_text):
                break
            start -= 1
        if start == last_fit:
            return last_fit
        return start

    last_fit  // not in a recognized block
```

### Trim

```
fn trim_block_atoms(range, page_top, text_view, buffer, is_prose) -> VisibleRange:
    if range.count == 0 || range.last_fit == page_top:
        return range

    let block_start = block_start_for_line(buffer, page_top, range.last_fit, is_prose)
    if block_start == range.last_fit:
        return range  // not in a block

    // Overflow guard: if backing up would leave zero rendered lines
    // (block starts at or before page_top), keep per-line split.
    if block_start <= page_top:
        return range

    let new_last_fit = block_start - 1
    let new_count = new_last_fit - page_top + 1
    VisibleRange { last_fit: new_last_fit, count: new_count }
```

### Composition

A canonical wrapper that composes both trims in order:

```
fn trim_visible_range(range, page_top, text_view, buffer, is_prose) -> VisibleRange:
    let r = trim_trailing_speakers(range, page_top, text_view, buffer)
    trim_block_atoms(r, page_top, text_view, buffer, is_prose)
```

The three direct callers of `visible_range + trim_trailing_speakers` collapse to `trim_visible_range`: `snap_scroll_to_line`'s F4 cache populate, `last_fully_visible_line`, and `is_line_fully_visible`'s cold-fallback path. `next_page_top` inherits the new behavior transitively via its `last_fully_visible_line` call. Each caller already has access to `state.current_work` and can compute `is_prose` via `is_prose_work`.

### Tests

Pure-Rust unit tests in a new `block_atom_tests` mod. `block_start_for_line` takes `&TextBuffer` so unit tests construct a `gtk::TextBuffer` in-test (same pattern as existing `visible_range_helpers_tests`).

For `block_start_for_line`:
1. Stage-direction block of 3 lines, `last_fit` mid-block → returns block start.
2. Stage-direction block of 3 lines, `last_fit == block_start` → returns `last_fit` (no backup).
3. Verse stanza of 4 lines in non-prose work, `last_fit` mid-stanza → returns stanza start.
4. Verse stanza in prose work → returns `last_fit` (rule doesn't apply).
5. Single-line stage direction → returns `last_fit`.
6. Block bounded above by speaker → backup stops at speaker line (returns first stanza line).
7. Block bounded above by blank → backup stops at blank line.

For `trim_block_atoms`:
8. Block fully fits (`block_start > page_top`) → trimmed range with reduced count.
9. Block doesn't fit (`block_start <= page_top`) → original range (overflow fallback).
10. Empty range → returned unchanged.

For `trim_visible_range`:
11. Range with trailing speaker AND mid-block last line → both trims apply in order; speaker trim runs first, block trim runs on the new `last_fit`.

### Effort

M (one helper ~30 LOC, one trim ~25 LOC, one wrapper ~10 LOC, four caller updates ~12 LOC, ~11 unit tests).

---

## Integration

### Phase order

Two sequential phases. F7 first (smaller, lower-risk; shipping it doesn't depend on F9 and a regression doesn't block the larger F9 change). Then F9.

### File map

**F7:**
- Modify `src/input/navigation.rs`:
  - Add `fn prev_page_top` near `next_page_top` (~line 149).
  - Update `page_backward` (~lines 252–283) — replace `page_history.pop().unwrap_or_else(...lpp...)` block with `prev_page_top` call.
  - Update `page_backward_bottom` (~lines 300+) — same pattern.
  - Append `#[cfg(test)] mod prev_page_top_tests` after existing `page_tops_tests`.
- No `app.rs` changes.

**F9:**
- Modify `src/input/navigation.rs`:
  - Add `fn block_start_for_line` near `trim_trailing_speakers`.
  - Add `fn trim_block_atoms` after it.
  - Add `fn trim_visible_range` as the canonical composition wrapper.
  - Update the three direct call sites that currently do `visible_range + trim_trailing_speakers` to call `trim_visible_range` instead. Sites: `snap_scroll_to_line` (~line 1490), `last_fully_visible_line` (~line 127), `is_line_fully_visible` cold-fallback (~line 830). `next_page_top` inherits the new behavior transitively via `last_fully_visible_line`.
  - Append `#[cfg(test)] mod block_atom_tests` after `prev_page_top_tests`.
- No `app.rs` changes.

### Manual verification protocol (used after each phase)

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

### Test counts

- After F7: 99 → ~104 (5 new tests in `prev_page_top_tests`; 2-3 may be GTK-deferred).
- After F9: ~104 → ~115 (11 new tests in `block_atom_tests`).
- Pre-existing `mpv::client::tests::test_find_line_for_time` failure stays.

### Commit message shape

- F7: `Add prev_page_top for exact backward page boundary` — explains lpp-fallback shortcoming, references foliate's `atStart`/`atEnd`, notes binary-search fast path off the F8 cache.
- F9: `Trim visible range for stage direction and verse stanza atomicity` — explains per-line vs per-block, references foliate's `getVisibleRange` per-element rule, notes overflow-fallback policy.

### Rollback

F7 is one new function + two replacement bodies — `git revert` cleanly restores. F9 changes the visible_range trim contract used by 4 call sites, but all four already use the F2-style composition pattern; revert drops `trim_block_atoms` and renames `trim_visible_range` back to direct `trim_trailing_speakers` calls.

### Risks

- **MPV sync edge (F9):** When MPV drives the cursor to a line that "fits per-line but block-trim removes it," `is_line_fully_visible` returns false → MPV path may force a page-turn one line earlier than before. This is the intended payoff (no mid-stanza strands). Verified manually during F9 verification.
- **Stanza definition borderline (F9):** Two consecutive dialogue lines with no blank between them count as a "block" (couplets in verse). Algorithm treats this correctly; if real-world borderline cases prove otherwise, predicate tunes.
- **F7 pathological case:** When `current_top` is not on any forward-walkable boundary (resumed via concordance to an arbitrary line), the existing lpp approximation is preserved. Same behavior as today for that specific case.

---

## Out of scope

- F10 (`OverlayMode` trait keymap refactor) — review explicitly says "do as part of a larger keymap refactor, not standalone for pagination." Deferred to a separate keymap-focused brainstorm.
- Sentence-group atomicity in prose — risks empty pages on long sentences; exceeds the review's intent.
- Schema changes to `line_map` — F9 uses live predicates, no precomputed block boundaries.
- Per-element annotations / selection — review-flagged future work.
- Touch / scroll-velocity / RTL / vertical writing-mode — not applicable to linux-lit (review out-of-scope items).
