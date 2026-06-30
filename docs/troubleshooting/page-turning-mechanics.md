# Page Turning Mechanics

Reference for debugging page-forward (`x`), page-backward (`y`), and related
navigation in e-reader mode.

## The authoritative-boundary principle (read this BEFORE touching pagination)

Every line in `lit.db` carries `(div1, div2)` (act, scene). **A scene/section
boundary is exactly where `(div1, div2)` changes — full stop.** The `ACT N` /
`=====` / `Scene N` lines you see in the buffer are display chrome that linux-lit
synthesizes; they are NOT the source of truth. The source of truth is the loaded
metadata.

linux-lit therefore precomputes a boundary bitmap at load
(`LineMap.section_starts`, built in `build_line_map`) and all pagination consults
it through one predicate (`AppState::is_section_start` / the `section_break_fn`
closure threaded into the pure helpers). **Never re-infer a boundary from buffer
text in pagination code.** `line_types::is_act_scene_marker` / `is_separator`
survive only as a mid-load fallback (before the line map exists) and for *display*
styling (title bar, synopsis) — never for deciding where a page ends.

Why this matters (the expensive lesson): for a long time the pagination paths
re-inferred "is this a section break?" from the raw `.txt` text. That inference
is fragile exactly at scene transitions — a scene-ending column reads `dialogue →
blank → [They exit.] → blank → ACT 2 → ===== → Scene 1`, and the text-based
"header-block skip" bridged across the exit/blanks straight into the `ACT 2`
marker and skipped it, so the column ran into the next act (the AWW 25-line `y
GAP`). Two attempts to patch the text heuristic caused catastrophic regressions
(169 test fails; `JumpEnd` → a 1-line page). The whole class dissolved the moment
the boundary was read from `(div1,div2)` instead of guessed from text. **If you
find yourself reasoning about which buffer lines "look like" a marker to decide a
page boundary, stop and read the bitmap instead.**

Pattern when adding/keeping a per-line structural fact (boundary, chapter,
dialogue, spoken-status): if the DB already encodes it, surface it through
`LineMap` / `Line` and read it — do not reconstruct it by classifying buffer
text. Reconstruction drifts from the data and the drift surfaces as a pagination
bug three transformations downstream.

## The pagination model (read this first)

linux-lit paginates a **flat buffer of lines** into pages on the fly — there is
no precomputed page list (a font/size/width change would invalidate it). A play
renders as a **two-column spread**: the reader fills the LEFT column top-to-
bottom, then the RIGHT column, then turns to the next spread. Prose and
translation mode use one column.

**One function defines a spread: `column_split(top)`** (`viewport.rs`). Given a
page-top line it returns a `ColumnSplit { split, page_end, next_page_top }`:

- `split` — first line of the RIGHT column (left column is `[top, split)`).
- `page_end` — last visible line of the spread (bottom of the right column).
- `next_page_top` — first line of the FOLLOWING spread (`page_end + 1`).

Everything else is built on this. The renderer scrolls the left view to `top`
and the right view to `split`; `page_forward` advances `page_top` to
`next_page_top`; `prev_page_top` walks *backward* to find the spread before the
current one. **The cardinal rule is TILING:** consecutive spreads must abut with
no gap and no overlap — `column_split(top).next_page_top` is exactly the next
spread's `top`. Most pagination bugs are a tiling violation (a line shown twice,
or skipped).

**How a spread's extent is decided** (inside `column_split`):

1. Fill the left column by pixel height (`visible_range` + a descender guard),
   then trim what would look broken at the split (a dangling speaker name, a
   half stage-direction). `split = left.last_fit + 1`.
2. **If the right column would BEGIN a new (non-final) ACT/SCENE** — skipping
   leading blanks/exits from `split` lands on a section marker — the page **ends
   in the left column**: `page_end` is before the marker, `next_page_top` is the
   marker. The new scene starts the next spread. (This is the "stop at a scene
   break" reading model — a scene-ending page may have a short/empty right
   column, and `y` from the next scene tiles into it exactly.)
3. Otherwise fill the right column, **clamped at a section break** so a new
   act/scene never appears mid-column — EXCEPT the work's final trailing section
   (an EPILOGUE), which has nowhere to be pushed and fills the right column.

**Two symmetric exceptions to "fill left first".** A short section at either END
of the work fills the RIGHT column rather than sitting alone in the left:
- **EPILOGUE (final spread):** a short tail fills the right column; `last_page_top`
  forward-pulls the top so it lands there (see *the asymmetry* below).
- **PROLOGUE / opening section (first spread):** a short COMPLETE opening section
  (Prologue, Induction, Chorus, or a brief opening scene) that ends at the first
  section boundary moves to the right column with the LEFT column EMPTY. The
  start-of-document mirror of the EPILOGUE rule. `column_split(state, 0)` returns
  `split = 0` (empty left), `page_end = N-1`, `next_page_top = N` — `next_page_top`
  is UNCHANGED from the empty-right behavior it replaces, so tiling is untouched;
  only the visual placement within the first spread changes. Requires the whole
  opening section to fit the right view's height (otherwise normal left-fill).

**The asymmetry that bites every backward/jump fix:** forward paging uses
`column_split`'s boundary; but the work's **final spread** is special. When the
tail is short, `last_page_top` (`navigation.rs`) FORWARD-PULLS the final top a
few lines so the tail fills the right column (a full spread, not a lonely left
column). That pulled top is NOT on the natural `column_split` chain, so:
- it must be reached the same way from EVERY entry point (startup, `G`, `x`,
  `j`, `y`) — see *Diagnosing § "FIVE paths"* in headless-testing.md;
- `y` from it cannot tile exactly (a small benign seam) — the fuzz exempts it.

**`column_split` is the source of truth.** Render, the page-tiling fuzz
invariants, `prev_page_top`, and `last_page_top` all consult it. If you change
how a spread is measured, change `column_split` and everything follows; do NOT
add a parallel boundary calc (the historical `next_page_top()` single-column
helper diverged from `column_split` by a speaker block and caused a persistent
`y GAP` — backward nav now tiles against `column_split` in two-column mode).

## Architecture

Page state lives in `AppState`:

- `page_top_line: usize` — buffer line at the top of the current viewport
- `page_top_offset: i32` — pixels scrolled PAST `page_top_line`'s pixel top. 0 in
  the normal line-aligned case; non-zero ONLY while paging within an over-tall
  prose paragraph (see *Prose over-tall paragraph* below). Viewport top y =
  `line_yrange(page_top_line).y + page_top_offset`.
- `page_back_stack: Vec<(usize, i32)>` — history of previous
  `(page_top_line, page_top_offset)` pairs, pushed by `page_forward`, popped by
  `page_backward`. The offset is in the entry so `y` round-trips a mid-paragraph
  forward turn exactly.

## Prose over-tall paragraph (sub-line paging)

**The trap:** prose stores ONE buffer line per paragraph, and a long paragraph
wraps TALLER than the viewport (Bleak House "On such an afternoon…" = 2529 chars,
1170px vs ~1067px usable). Pagination counts whole buffer lines via
`line_yrange`, so `visible_range` fits ZERO lines for an over-tall paragraph at
`page_top` (`last_fully_visible_line == page_top`). Without special handling
`next_page_top` then advances `new_top` to `page_top + 1` = the NEXT paragraph,
**dropping every wrapped row of the current paragraph below the fold** (the
classic "x skips a chunk of a long paragraph" bug). The render/clip side already
handled this (`update_bottom_clip`'s `range.count==0` branch reads the live scroll
and clips at a visual-row boundary), but page-forward did not continue by row.

**The fix — sub-line scroll within the paragraph.** When the paragraph at
`page_top` is taller than the viewport, `x` advances the SCROLL by one viewport
height WITHIN the same buffer line (a `page_top_offset`, snapped to a real
visual-row top), and only advances `page_top_line` to the next paragraph once the
paragraph is exhausted. `y` reverses it.

- `page_forward` (single column only): `overtall_forward_step` measures the
  paragraph height + usable height, asks the PURE
  `viewport::overtall_next_offset(offset, para_h, usable)` whether rows remain
  below the fold, snaps `y + raw` DOWN to a real visual-row top via
  `scroll::snap_value_to_display_row` (the main-card per-`display_rows` snap —
  sanctioned in clip-prevention.md), and on a within-paragraph step pushes
  `(page_top_line, page_top_offset)` and calls `set_page_instant_offset(state,
  top, new_off)` (page_top_line UNCHANGED). Falls through to the normal line turn
  when the paragraph is exhausted.
- `set_page` resets `page_top_offset = 0` on every whole-line turn; jumps/search/
  scene all go through it (offset 0). `set_page_instant_offset` /
  `snap_scroll_to_line_offset` carry a non-zero offset only on the over-tall
  forward step and the `page_backward` mid-paragraph restore.
- `page_backward` mirror: when the popped entry is the SAME buffer line behind the
  current scroll, restore via `set_page_instant_offset` (no line turn); else
  normal `set_page`. The stale-drop loop compares `(line, offset)`.
- Playback sync / dimming / two-column plays: unchanged. The over-tall guard
  `current_line > last_vis` is already false when the cursor is the same buffer
  line as `page_top` (over-tall → `last_vis == page_top`), so sync never spuriously
  turns; sub-line offset is manual-paging only.

Guarded by `viewport::overtall_offset_tests` (pure coverage + multi-step + safety)
— the old `test_page_forward_prose_bleak_house` models a fixed 30-line page and is
BLIND to an over-tall single buffer line, which is why this bug shipped. Visual
acceptance is pixel-level (the dropped tail must reappear; `x`/`y` must round-trip
the mid-paragraph stops) — verify on the real display.

Page boundaries are computed by `next_page_top()` in `viewport.rs`, which:

1. Calls `last_fully_visible_line(state, top)` to find where the current page
   ends (pixel-height walk with descender guard, trimmed by
   `trim_visible_range`)
2. Finds the last dialogue line on the visible page via `last_dialogue_in_page`
3. Finds the next dialogue after that via `next_dialogue_from`
4. Backs up over speakers/stage-directions/scene-headers via
   `back_up_for_speaker` to get the new page top

Key files:

- `src/input/navigation.rs` — `page_forward`, `page_backward`,
  `page_backward_bottom`, all jump functions
- `src/input/viewport.rs` — `next_page_top`, `prev_page_top`,
  `last_fully_visible_line`, `visible_range`, `trim_visible_range`,
  `clamp_at_section_break`, `section_break_fn`, `back_up_for_speaker`
  (+ `_state` wrappers), `is_dialogue_line`, `is_inside_stage_direction`
- `src/text_file_map.rs` — `build_line_map`, `build_section_starts`
  (the `(div1,div2)` boundary bitmap), `LineMap.section_starts`
- `src/app.rs` — `AppState::is_section_start` / `section_starts` (read the bitmap)
- `src/input/scroll.rs` — `set_page`, `set_page_instant`, `snap_scroll_to_line`
- `src/db/line_types.rs` — `is_dialogue`, `is_stage_direction`, `is_speaker`,
  `is_act_scene_marker`, `is_separator` (text classifiers — for the line-map
  build and the mid-load pagination FALLBACK only; not the boundary source of
  truth, see *The authoritative-boundary principle*)
- `src/db/models.rs` — `Line.div1` / `div2` / `line_in_div` (the authoritative
  per-line act/scene metadata)

## page_back_stack rules

Every function that changes `page_top_line` must interact with the stack:

- **page_forward (`x`)** — pushes old `page_top_line` before turning
- **page_backward (`y`)** — pops; falls back to `prev_page_top()` when empty
- **page_backward_bottom (Shift+comma)** — pops (same as `page_backward`)
- **Structural jumps (gg, G, `[`, `{`, bookmarks, vocab, zt)** — clear the stack
  then push current `page_top_line` as a single return entry. `y` after such a
  jump returns to the page the user was on when they jumped; a second `y` has an
  empty stack and falls through to `prev_page_top()`
- **Scene jumps (2, 3)** — clear the stack but do NOT push. A scene jump can skip
  many pages, so `y` should page back one viewport into the skipped content via
  `prev_page_top()`, not teleport to the jump origin (see `jump_to_next_scene` /
  `jump_to_prev_scene`)
- **Line-by-line dialogue navigation (comma, q, j, k)** — no stack interaction;
  incidental page turns from `scroll_after_jump_forward/backward` don't touch the
  stack. These follow a plain reading-order model (see *Dialogue navigation
  reading model* below) and do NOT scene-snap — scene snapping is the 2/3 jumps'
  job
- **Search jumps (`/`, `n`, `N`)** — push current `page_top_line` (with dedup)
  before `update_highlight_and_center`. This means `y` after dismissing a
  search with Escape returns to the pre-search page. The dedup avoids
  polluting the stack when `execute_search` fires on every keystroke during
  live-search (only pushes if the top of stack differs from current
  `page_top_line`)
- **MPV sync (scroll_paragraph_to_top, highlight auto-advance)** — no stack
  interaction; system-driven, not user navigation

If a new navigation function is added that calls `set_page` or
`set_page_instant`, it must either push/pop/clear `page_back_stack` or
document why it doesn't.

## Debugging page-forward stuck states

### Symptom

Pressing `x` doesn't advance, or advances by only a few lines then gets
stuck oscillating between two nearby page tops.

### Debug log entries

`page_forward` already logs at the `PAGE_FWD:` prefix:

```
PAGE_FWD: page_top=177 new_top=177 next_dialogue=185 line_count=4548
PAGE_FWD: candidate_top=185 effective_top=185 (from new_top=177)
```

Check:

- **`new_top <= page_top`** — means `back_up_for_speaker(next_dialogue)` pulled
  the top behind a section break. The fallback sets `candidate_top =
  next_dialogue`. If this happens repeatedly from the same page_top, the
  section-break clamping is too aggressive.
- **`next_dialogue` never advancing** — the dialogue classifier is
  misidentifying a non-dialogue line as dialogue. Check multi-line stage
  directions (see below).
- **`effective_top <= page_top`** — `clamp_page_top_to_scroll_ceiling` hit the
  GTK scroll ceiling; falls through to `jump_to_end`.

### Adding detailed diagnostics

To trace the full page-boundary computation, add temporary logging inside
`next_page_top` in `viewport.rs`:

```rust
pub(crate) fn next_page_top(state: &AppState, top: usize) -> NextPage {
    let line_count = state.effective_line_count();
    // ... existing early returns ...
    let last_visible = last_fully_visible_line(state, top);
    let last = last_dialogue_in_page(&state.buffer, top, last_visible.saturating_sub(top) + 1, line_count);
    let next_dialogue = next_dialogue_from(&state.buffer, last + 1, line_count);
    crate::log_fmt!("NEXT_PAGE_TOP: top={} last_visible={} last_dialogue={} next_dialogue={}",
                    top, last_visible, last, next_dialogue);
    // ... rest of function ...
}
```

To trace section-break clamping, add inside `clamp_at_section_break`:

```rust
crate::log_fmt!("SECTION_CLAMP: page_top={} break_line={} clamped_last={} orig_last_fit={}",
                page_top, break_line, clamped_last, range.last_fit);
```

Remove after diagnosing — these fire on every page turn and duplicate work.

## Page-turn animation lock

`set_page` in `scroll.rs` acquires `page_turn_lock` for the duration of the
crossfade/slide animation (700ms for crossfade). While the lock is held,
subsequent `set_page` calls return early without updating `page_top_line`.

`page_forward`, `page_backward`, and `page_backward_bottom` all check
`page_turn_lock.is_locked()` at the top and return early if held. This
prevents stack/cursor mutations from running when the page turn would be
silently dropped by `set_page`.

Without this guard, pressing `y` during a crossfade would pop an entry from
`page_back_stack` and update `current_line`, but `set_page` would discard the
turn — the stack entry is consumed and lost, causing the next `y` to skip a
page.

The same applies to `page_forward`: pressing `x` during a crossfade would
push a stale `page_top_line` onto the stack and update `current_line` without
the page actually turning.

Rule: any function that modifies `page_back_stack` or `current_line` before
calling `set_page` must guard against `page_turn_lock` first.

## Debugging page-backward wrong destination

### Symptom

Pressing `y` after `x` doesn't return to the previous page — it jumps much
further back or to an unrelated position.

### Diagnosis

1. Check the log for `PAGE_BWD: stack pop` vs `PAGE_BWD: prev_page_top`.
   Stack pop means the back-stack had an entry; `prev_page_top` means it
   was empty and had to recompute.
2. **How `prev_page_top` works now:** it walks the forward chain from line 0
   using the SAME boundary the renderer uses — `column_split(probe).next_page_top`
   in two-column mode (not the single-column `next_page_top()` helper, which
   diverges by a speaker block and caused a persistent 3-line `y GAP`). It
   returns the page `probe` whose forward boundary hits `current_top` exactly
   (`next == current_top` → perfect tile), or the last boundary that does not
   overshoot. It returns that boundary VERBATIM — never a
   `back_up_for_speaker(next_dialogue_from(...))` re-derivation, which shifts off
   the boundary and re-creates a gap.
3. **If it still gaps/overlaps, suspect `current_top` itself.** It may not be on
   the `column_split` chain at all: a scene jump (`2`/`3`) lands at a scene
   heading, and the forward-pulled final spread (`last_page_top`) sits off the
   chain. Then no boundary tiles exactly. For the final spread that seam is
   benign (exempt). For a scene start it should tile — check that `column_split`
   ends the previous page at the scene boundary (the "right column begins a new
   scene" rule under *Section-break clamping*).
4. Check whether the navigation that preceded `y` pushed to or cleared the
   stack. If a jump function forgot to clear, the stack has stale entries.

### Common causes

- **New jump function doesn't clear the stack** — add
  `state.page_back_stack.clear()` before its `set_page`/`set_page_instant`
  call.
- **`current_top` not a `column_split` boundary** — a scene jump or the
  forward-pulled final spread produced a `page_top` the forward chain skips. The
  fix is to make `column_split` produce a boundary there (scene-ends-in-left
  rule), or to exempt the genuinely un-tileable final spread, NOT to fudge
  `prev_page_top`.

## Multi-line stage directions

Folger-cleaned Shakespeare texts have multi-line stage directions:

```
[Enter the King of England, Humphrey Duke of
Gloucester, Bedford, Clarence, Warwick, Westmoreland,
and Exeter, with other Attendants.]
```

`is_stage_direction` in `line_types.rs` recognizes:

- Single-line: `^\[.*\]$`
- Multi-line opener: starts with `[`, no closing `]`
- Multi-line closer: ends with `]`, no opening `[`

Continuation lines ("Gloucester, Bedford...") are detected by
`is_inside_stage_direction` in `viewport.rs`, which scans backward up to
20 lines looking for an unclosed `[` opener. This function is used by
`is_dialogue_line`, `next_dialogue_from`, `last_dialogue_in_page`, and
`back_up_for_speaker` to ensure multi-line stage directions are never
treated as dialogue.

If a new multi-line pattern appears that isn't caught, `next_dialogue_from`
will return one of its lines as "the next dialogue", and
`back_up_for_speaker` may pull the page top behind a section break,
creating a stuck loop.

## Section-break clamping

> **Boundaries are AUTHORITATIVE, not inferred.** A scene/section boundary is
> exactly where a line's `(div1, div2)` changes — that is unambiguous in the DB.
> At load, `build_line_map` precomputes a `LineMap.section_starts: Vec<bool>`
> bitmap (one bit per buffer line, `true` on the FIRST line of each new
> `(div1,div2)` run) and every pagination decision below reads it via the
> `is_section_start(line)` predicate / `section_break_fn` closure. Do **not**
> re-derive a boundary from buffer text (`is_act_scene_marker` / `is_separator`)
> in pagination code — see *The authoritative-boundary principle* at the top of
> this file. (Those text checks survive only as a mid-load FALLBACK inside the
> helpers, used before the line map exists.)

A new ACT/SCENE must start a fresh spread, never appear mid-column. `column_split`
enforces this in three places, all driven by the `section_starts` bitmap:

- **Left column:** `clamp_at_section_break` scans `(page_top, left.last_fit]` for
  the first line where `is_section_start` is true and clamps `last_fit` to the
  line before it. A page that STARTS at a boundary (`is_section_start(page_top)`)
  never self-clamps because the scan begins at `page_top + 1` — the boundary line
  is the page's own opening heading. (No more text-based "header-block skip":
  with an authoritative single-line boundary there is nothing to bridge across,
  which is what eliminated the AWW `y GAP` where the old header-skip ran straight
  through an `ACT 2` marker hidden behind a `[They exit.]`.)
- **Right column would BEGIN a new scene:** if skipping leading blanks/exits from
  `split` lands on a boundary line (`is_section_start(hi)`), the previous scene
  ended in the left column. `column_split` ends the page there: `page_end` before
  the boundary, `next_page_top` = the boundary, right column empty. The new scene
  starts the next spread, so `y` from it tiles into this page exactly
  (`column_split(prev).next_page_top == scene_top`).
  - **First-spread exception (short opening section → right column).** When this
    case fires on the VERY FIRST spread (`page_top == 0`) and the whole opening
    section `[0, hi)` fits the right view's height, `column_split` instead returns
    `split = 0` so the section renders in the RIGHT column with an EMPTY left —
    the start-of-document mirror of the EPILOGUE final-spread rule. `next_page_top`
    stays `hi` (identical to the empty-right branch), so tiling is unchanged: `y`
    from Act 1's spread still tiles back here exactly. `update_bottom_clip` treats
    `exact_end == 0` (`end <= page_top`) as an empty column and clips the left
    view's full height. Verify visually (H8) — a rendered-spread criterion; the
    `FIRST_SPREAD_SPLIT split=0 …` log line (under `LIT_HEADLESS_TEST`, asserted by
    `tests/startup_column_layout.rs`) confirms the rule fired.
- **Right column interior:** `clamp_at_section_break` again, so a boundary partway
  down the right column starts the next spread.

**The one exemption: the work's FINAL trailing section** (an EPILOGUE, e.g. AWW's
`div1=6, div2=0`). It has nowhere to be pushed and `last_page_top`/`G` expect it
to fill the right column, so when clamping would empty the right column AND the
unclamped range already reaches the work's end, `column_split` keeps it
unclamped. Detected by "no further `is_section_start` after this one".

**Non-dialogue tail skip (the AWW Scene-1→2 underfill).** A scene's last spread
ends on its last *dialogue* line; the trailing `[They exit.]` / blank lines the
trim drops are NOT a page of their own. `column_split` therefore advances
`next_page_top` past a pure non-dialogue tail to the next real page top
(`back_up_for_speaker` of the next dialogue). Without this, `prev_page_top` would
tile a tiny dialogue-less spread on the way back (a 2-line UNBALANCED spread).

Edge case: when the boundary is very close to `page_top` (1-2 lines), the clamped
page is trivially small. `next_page_top` then computes a `next_dialogue` whose
`back_up_for_speaker` pulls back behind the break, producing `new_top <=
page_top` (no progress). `page_forward` handles this with the fallback
`candidate_top = next_dialogue`.

## Two-column right-column positioning

In two-column mode the right column is a separate `right_view` sharing the
buffer; `snap_scroll_to_line` (`scroll.rs`) scrolls it so `cs.split` (from
`column_split`) sits at its top, and `update_bottom_clip`'s `exact_end` path
clips it at `cs.page_end`. Two failure modes, both worst near the document end:

**Right column duplicates the left / shows the buffer start.** The right
view's `set_value` to `cs.split` clamped low because the right view's `upper`
was too small — either layout wasn't settled yet (stale `upper`) or the right
view had no bottom-margin headroom for a near-the-end split. Fixes:
`ensure_scroll_range` now extends the **right** view's bottom margin too (it
used to extend only the left `text_view`), `snap_scroll_to_line` calls
`ensure_scroll_range` before scrolling the right view, and the right-view scroll
runs synchronously **and** on an idle + 100ms backstop (`scroll_right_view_to_split`)
so a stale-`upper` first pass is corrected post-layout. If the right column
still shows line 0, check the right view's `upper` vs `cs.split`'s y.

**Right column unscrolled on first paint.** The startup resize-tick reveal calls
`snap_scroll_to_line` (which positions both columns), but the 500ms-grace and
5s-stuck-fallback reveals only set opacity. If the resize tick never fires (e.g.
its two-column width-settling guard never passes — common in a headless cage),
the fallback reveals the window with the right column at line 0. Both fallbacks
now call `reveal_snap` (`ensure_scroll_range` + `snap_scroll_to_line`) before
`set_opacity(1.0)`.

**Page-forward stuck on the final spread.** `scene_snap_top` may return a scene
start (`cs.split`) that sits past the scroll ceiling; `set_page` then clamps it
back below `page_top_line`, so the view never advances and `x` oscillates
(`scene-snap page_top=N -> new_top=M` repeating with the same N). `page_forward`
now clamps the snap target with `clamp_page_top_to_scroll_ceiling` and only
takes the snap when it yields real forward progress (`clamped > page_top_line`);
otherwise it falls through to the normal path, which recognizes end-of-document
(`next_dialogue >= line_count`) and stops cleanly.

Key files: `src/input/scroll.rs` (`snap_scroll_to_line`,
`scroll_right_view_to_split`, `ensure_scroll_range`), `src/app.rs`
(`reveal_snap` and the 500ms/5s reveal timeouts, resize-tick `do_reveal`),
`src/input/navigation.rs` (`page_forward` scene-snap guard, `scene_snap_top`,
`jump_to_end`), `src/input/viewport.rs` (`column_split`).

### "Empty right column" is NOT just `page_end < split`

When a spread ENDS at a scene break, `column_split` takes the scene-break branch
(`viewport.rs`, the `at_break && !is_final_section` return). It leaves the
scene's trailing exit/blank lines in the right range `[split, page_end]` — which
the bottom-clip then hides — and sets `next_page_top` to the next scene's marker.
The key subtlety: when a SINGLE trailing line sits at the split, that branch
returns `page_end == split` (not `< split`). The right column is then VISUALLY
empty (its one line is a clipped `[They exit.]` / blank), yet the common
`page_end < split` test reports it as NON-empty.

Observed on H8 1.3 (`split=804 page_end=804 next_page_top=805`): the right
column shows nothing, but `page_end < split` is false. The robust test for
"right column is visually empty" is the one the next-scene watermark uses
(`update_next_scene_watermark` in `scroll.rs`): authoritatively, **`next_page_top`
is a DB section start (`is_section_start`) AND the right range carries no
dialogue (`is_dialogue_line`)** — with `page_end < split` kept as the sufficient
condition for the strict lone-tail geometry, and `next_page_top < line_count`
excluding end-of-work and the empty-LEFT first-spread mirror.

**`would_empty_right_column` (`viewport.rs`) carries the same too-strict test**
(`cs.split >= line_count || cs.page_end < cs.split`). It works for its current
callers (the lone-EPILOGUE geometry it guards has `split >= line_count`), but if
`G` / `jump_to_end` / `page_forward`'s final-spread guard ever mis-tiles at a
scene boundary whose new scene opens after exactly one trailing exit line, this
is the root cause: `would_empty_right_column` returns `false` for a spread whose
right column is in fact empty. The fix would mirror the watermark's predicate
(section-start + no-dialogue), not loosen `page_end < split` to `<=` (which would
mis-flag genuine one-line right columns).

## Testing

Two layers: headless tests verify the page-turn algorithm across many works
cheaply (no display server needed); the in-app test harness verifies
integration with real GTK pixel layout on the current work.

**Clip invariant (pixel-level, e2e under cage):** `tests/line_clipping.rs` asserts
the MAIN reading card never clips its first/last line (top/mid/end), driven by
the `TEST_VIEWPORT_RECT` the app logs on reveal. `tests/overlay_clipping.rs`
extends the same invariant to the synopsis OVERLAY (opens it with `h`, scrolls to
the bottom with `j`), driven by `TEST_OVERLAY_VIEWPORT_RECT`. Both are `#[ignore]`d;
run via `./scripts/e2e-env.sh cargo test --test line_clipping --test overlay_clipping
-- --ignored --nocapture`.

### Headless tests

`src/input/navigation.rs` has headless tests that simulate page turning
using text-only line counts (no GTK). They approximate
`last_fully_visible_line` with a fixed `page_size = 30` lines and simulate
`clamp_at_section_break`, `back_up_for_speaker`, and the page_back_stack.

Algorithm tests (all Shakespeare plays, ~38 works):

- `test_page_forward_all_shakespeare_no_stuck` — every page turn advances
  (no stuck states)
- `test_page_forward_backward_roundtrip_all_shakespeare` — forward tops
  strictly increasing; backward via history round-trips exactly
- `test_x_y_roundtrip_with_clamping_all_shakespeare` — same as above but
  with section-break clamping simulation
- `test_x_page_forward_no_mid_page_scene_breaks_all_shakespeare` — no
  scene marker or separator in the interior of any page (after the opening
  header block)
- `test_y_after_scene_jump_returns_to_origin_all_shakespeare` — y after a
  scene jump (3) returns to the exact jump origin
- `test_scene_synopsis_identification_all_shakespeare` — scene markers
  resolve to correct synopsis keys via the database

Single-work tests:

- `test_page_forward_no_gaps_or_repeats` — Troilus: every highlighted line
  is dialogue, strictly increasing, gaps bounded by page_size
- `test_x_page_forward_covers_every_line_errors` — Comedy of Errors: every
  non-blank line appears in at least one visited viewport
- `test_j_cursor_next_dialogue_covers_every_line_errors` — same coverage
  via j/q cursor navigation
- `test_y_after_chapter_jump_returns_to_origin` — Troilus: y after [/{
  returns to jump origin
- `test_x_x_x_scene_jump_y_y_sequence` — Troilus: x x x 3 y returns to
  pre-jump page; second y has empty stack
- `test_chained_scene_jumps_only_last_origin_survives` — Troilus: 3 3 y
  returns to page between the two jumps
- `test_page_forward_prose_bleak_house` — prose forward: every page turn
  advances to next non-blank line
- `test_page_backward_prose_bleak_house` — prose backward via history:
  exact round-trip

Run all page-turn tests:

```bash
cargo test -- page_turn
```

Run only the all-Shakespeare tests:

```bash
cargo test -- all_shakespeare
```

### In-app test harness (Ctrl+Shift+T)

Toggles a deterministic test mode on the currently loaded work with real
GTK layout. Calls the same navigation functions that key dispatch and
playback sync use. Press `gg` first to start from the beginning.

Three modes configured via `/configure-nav-test`:

- **sync-only** — pure playback sync simulation (1s per line advance,
  walks cursor line-by-line triggering page turns via
  `update_highlight_and_advance_page`). Best for catching scene breaks
  mid-page and viewport fill issues during sustained playback
- **jumps-only** — key-press navigation only (x, y, 2, 3, [, {, search
  jump at 300ms each). Tests forward progress, round-trip, structural jump
  return, and search-then-page-back return
- **full** — both interleaved: jump sequences at 300ms with 20-line sync
  runs at 1s each (~20s of simulated playback per run)

Six invariants checked after every step:

- **Forward progress on x** — page_top_line strictly increases
- **y round-trips x** — page_top_line returns to pre-x value
- **y after structural jump returns** — page_top_line returns to pre-jump
  value (also covers search jumps)
- **No scene break mid-page** — no marker/separator in the interior of the
  visible range
- **Viewport fill** — visible content fills at least 10% of viewport height
  (real pixel measurement)
- **current_line is dialogue** — cursor on a dialogue line (plays)

Toast shows "NAV TEST: running…" while active, "NAV TEST: done (N steps,
M fail)" on completion. All steps and failures logged with `NAV_TEST:`
prefix to the debug log. Runs up to 500 steps.

What the in-app harness tests that headless cannot: real GTK pixel heights,
actual line wrapping, real section-break clamping with pixel measurements,
viewport fill percentage, set_page/set_page_instant scroll plumbing,
page_turn_lock interaction with animation timing.

## Playback sync

Playback sync advances the cursor to match MPV audio position. The pipeline:

1. **MPV emits `time-pos`** — the IPC listener in `mpv/client.rs` parses the
   JSON property-change event and extracts the current playback position (seconds)
2. **`find_line_for_time`** — binary search (`partition_point`) over sorted
   `(line_id, start, end)` timestamps to find which line contains
   `time_pos + SYNC_PREROLL` (currently 0.0s). Emits `MpvEvent::CursorSync(work_line_index)`
3. **CursorSync handler** (`main.rs`) — translates work-line index to
   buffer-line index via `line_map` (if present), then:
   - Skips if `sync_enabled` is false, work is loading, search is active,
     chunk mode is active, or `suppress_sync_until` hasn't elapsed
   - Guards against aberrant timestamps (>50 lines from current position)
   - Guards against `pending_advance_ignore_bl` pulling cursor backward
4. **Scene transition** (plays only) — compares the new line's `(div1, div2)`
   against `current_sync_scene`. On scene change, computes the header-block
   top via `back_up_for_speaker` and snaps the viewport with
   `set_page_instant` (unless the page top is already correct). Skips
   paragraph scroll when a scene scroll fired
5. **Paragraph transition** — calls `current_paragraph_range()` to detect
   whether the cursor crossed into a new paragraph (contiguous non-blank
   lines). If so, calls `scroll_paragraph_to_top()` which in e-reader mode
   page-turns so the paragraph start is at the viewport top (only if
   off-screen and `para_start >= page_top_line` — never scrolls backward to
   a paragraph that started on a previous page). Skipped when a scene scroll
   already happened
6. **`update_highlight_and_advance_page`** — applies highlight tags, then
   checks if `current_line > last_raw_visible_line` (the untrimmed last
   visible line — not `last_fully_visible_line`, which trims trailing
   speakers/blanks for pagination and would cause premature turns). If so,
   computes `page_turn_top(current_line)` and calls `set_page` with forward
   direction. This is how playback sync triggers page turns
7. **`after_page_change(MpvSync)`** — runs post-page-turn housekeeping. Does
   not seek MPV (sync-driven, not user-initiated)

### Pending advance (pending_advance)

Scheduled when the current timestamped line ends and the next dialogue line
has no timestamp. Scene boundaries are NOT handled here — CursorSync's
scene-transition detection (step 4 above) picks up scene changes naturally
when `find_line_for_time` lands on a line in the new scene.

- `pending_advance = Some((end_time, next_buffer_line, source_work_index))`
- On each `TimePos` event, if `pos >= end_time`: advance cursor directly,
  set `pending_advance_ignore_bl` to prevent CursorSync from pulling back

When a manually-set timestamp has no valid end time (`end <= start`), the
fallback `end_time` is: the next timestamped line's `start - 0.2s` (clamped
to at least `start`), or `start + 5.0s` if no next timestamp exists.

### Suppression

Manual navigation (comma, q, j, k) sets `suppress_sync_until` to a future
`Instant`, preventing CursorSync from overriding the user's position for a
brief window.

### SetTimestamps dialogue filter

`SetTimestamps` — the timestamp data sent to the MPV client for
`find_line_for_time` — is filtered to include only `is_dialogue` lines.
This prevents `CursorSync` from landing on stage directions, speaker names,
or other non-dialogue lines. The filter is applied at all three build sites:

- `app.rs` `display_work_at_with_prepared` — primary load path
- `app.rs` MPV discovery callback — when switching active `media_id`
- `timestamps.rs` `resync_mpv_timestamps` — after manual timestamp edits

### Always-on logging

These log prefixes are written regardless of debug mode (`Ctrl+d`):

- `CURSOR_SYNC:` — every sync event that changes `current_line`
- `SYNC_ADVANCE:` — the page-turn decision point
- `SYNC_PAGE_TURN:` — confirms a sync-driven page turn
- `SYNC_SCENE_SCROLL:` — scene transition snap
- `PAGE_TURN:` — every `set_page` call (sync and navigation)

Additional detail (`CURSOR_LINE:`, `SEEK:`, `CURSOR_SYNC: SUPPRESSED`)
requires debug mode.

Key files: `src/mpv/client.rs` (TimePos parsing, `find_line_for_time`),
`src/main.rs` (CursorSync + TimePos handlers),
`src/input/highlight.rs` (`update_highlight_and_advance_page`)

## Scenes

Scenes are encoded in the database via `div1` (act) and `div2` (scene)
fields on each line. `line_in_div` gives the line's position within its
scene. These are loaded in `db/queries.rs` and stored on each `Line` struct.

### Scene markers in the text buffer (display chrome, NOT the boundary source)

Act/scene markers are lines like `ACT 1`, `SCENE 2`, `## Act 3, Scene 1`,
`PROLOGUE`, `EPILOGUE`, or `INDUCTION`. `line_types::is_act_scene_marker()`
(strips optional `## ` prefix, uppercases, checks keyword prefixes) and
`is_separator()` (`=====`) detect them. **These are synthesized display chrome.**
The authoritative scene boundary is the `(div1,div2)` change captured in
`LineMap.section_starts` (see *The authoritative-boundary principle*). The text
classifiers are used to BUILD that bitmap (and as a mid-load fallback), not to
make pagination decisions at runtime.

### Scene headers and page boundaries

`back_up_for_speaker` positions page tops. When a dialogue line would be the
first on a new page, it backs up over blanks, speaker names, and entrance stage
directions — and, when the authoritative bitmap is present, it STOPS at the
`is_section_start` boundary line (the chrome line that opens the scene). This puts
the scene header (`ACT 1 / SCENE 2 / =====`) at the page top instead of splitting
it across pages. (Call it via the `back_up_for_speaker_state` wrapper, which
builds the boundary closure from `state.section_starts()`; the bare
`back_up_for_speaker(buffer, line, is_break)` is for the pure test mirror.)

`clamp_at_section_break` clamps `last_fit` to the line before the first
`is_section_start` boundary in the visible range, so new scenes start fresh.

### Title bar scene display

`update_title_bar_scene()` in `app.rs` reads the current line's `div1`/`div2`
and formats a label like "Act 1, Scene 2" (or "Act 1, Chorus" when
`div2==0`). Scene synopses are loaded from the `scene_synopses` table and
cached in `state.synopsis_cache` keyed by `(div1, div2)`.

### Scene-snap on navigation

When FORWARD dialogue navigation (`q`/`j`) or playback sync lands the cursor on
the first dialogue line of a new scene that's off-page, the viewport snaps so the
scene header is at the top of the new spread. Detection uses
`is_first_dialogue_of_scene` in `viewport.rs`, which walks backward from the
cursor — if it hits a scene marker or separator before any dialogue line, the
cursor is the scene's first dialogue; `back_up_for_speaker` then finds the full
header-block top. This applies to plays only (`!is_prose`).

- **Forward (`q`/`j`):** scene-snap fires in `scroll_after_jump_forward`.
- **Sync:** scene-snap fires via the `(div1, div2)` comparison in the CursorSync
  handler (`main.rs`).
- **Backward (`,`/`k`):** does NOT scene-snap. `scroll_after_jump_backward`
  follows the plain reading model below — scene snapping a backward step caused
  cursor oscillation in the final-spread region, and a reader pressing `,`/`k`
  expects to step to the previous dialogue, not jump a scene header to the top.
- **Scene jumps (`2`/`3`):** a separate path (`jump_to_next_scene` /
  `jump_to_prev_scene`), not these handlers — that is where intentional
  scene-to-page-top snapping lives.

### Dialogue navigation reading model (`,` `q` `k` `j`)

`q`/`j` (next dialogue) and `,`/`k` (previous dialogue) move the cursor one
dialogue line and turn the page only when the cursor leaves the visible spread.
In two columns the cursor walks down the left column, down the right column, then
onto the next spread — backward is the mirror. The handlers in `navigation.rs`
set `current_line` to the next/prev dialogue, then call the scroll-after fns in
`scroll.rs`:

- **`scroll_after_jump_forward`** — if the new line is still visible, nothing to
  do. Otherwise turn forward: `page_turn_top(current_line)` makes the cursor the
  FIRST dialogue at the new spread's top-left. If that would leave the right
  column empty (the work's short tail, e.g. a lone EPILOGUE), redirect to
  `navigation::last_page_top` so the tail fills the RIGHT column (cursor in it)
  rather than sitting alone in the left.
- **`scroll_after_jump_backward`** — if still visible, nothing to do. Otherwise
  the cursor stepped above the page top: turn to `prev_page_top`, then set the
  cursor to that spread's LAST visible dialogue line (bottom of the right column)
  — what a reader expects from `,`/`k` at the page top. Backing up off trailing
  non-dialogue is via `prev_dialogue_line(last_fully_visible_line + 1)`.

`navigation::last_page_top(target)` (shared with `jump_to_end`/`G`) walks the
forward page chain (`column_split(top).next_page_top`) from a safe early start.
When the next whole-page turn `would_empty_right_column` (the tail is short), it
does NOT just keep the current spread — the natural page boundary can *skip* a
better final spread. It **pulls the top forward** to the smallest top whose
spread leaves no dialogue below its forward boundary and still has a non-empty
right column, so the short tail (a lone EPILOGUE) fills the RIGHT column of the
canonical last spread. Returning the earlier full spread instead orphans the
EPILOGUE one spread past the end (the 4308-vs-4316 bug: `G` landed showing
"…welcome is the sweet" in the right column with the EPILOGUE unreachable below;
`x`/`G` then did nothing because that spread looked final).
`viewport::would_empty_right_column(top)` is the predicate; both paths and the
sync page-turn (`update_highlight_and_advance_page`) and sync scene-snap use it.

#### `G` / jump-to-end: land on the CANONICAL final spread (EPILOGUE in the right column)

Two coupled requirements, both about the short-tail case (a lone `EPILOGUE` that
opens with a section-break marker):

1. **The page must be the canonical last spread.** `last_page_top` must not stop
   at the last *full* two-column spread when a later spread fits the tail into
   its right column (see `last_page_top` above — it pulls the top forward). The
   wrong spread shows "…welcome is the sweet" in the right column with the
   EPILOGUE orphaned below; the fuzz catches it via invariant 7
   (`JUMP-TO-END not at end: next_page_top < line_count — content still below`),
   and on the real display `x`/`G` do nothing there because the spread looks
   final.
2. **The cursor must be on that page.** `jump_to_end` lands the page first
   (`set_page_instant(last_page_top(target))`), then sets the cursor to the last
   dialogue line actually within that spread
   (`prev_dialogue_line(cs.page_end + 1)` clamped to `[new_top, cs.page_end]`) —
   mirroring the forward final-spread guard in `page_forward`. Without this the
   highlight lands ~10 lines off-page (fuzz `JumpEnd landing off-page`).

With both, the EPILOGUE renders as the right column of the canonical last spread,
the cursor sits on it, and `j`/`q` walking into the tail resolve to the same
spread.

Key files: `src/db/models.rs` (Line struct with div1/div2/line_in_div),
`src/db/queries.rs` (load_work, scene synopses),
`src/input/viewport.rs` (`back_up_for_speaker`, `clamp_at_section_break`,
`is_first_dialogue_of_scene`, `would_empty_right_column`, `prev_page_top`,
`last_fully_visible_line`, `prev_dialogue_line`),
`src/input/navigation.rs` (`last_page_top`, the four nav handlers),
`src/input/scroll.rs` (`scroll_after_jump_forward`, `scroll_after_jump_backward`),
`src/db/line_types.rs` (`is_act_scene_marker`, `is_separator`)

## Dialogue detection

Dialogue classification determines which lines the cursor can land on during
navigation and playback sync. Computed at load time in `db/queries.rs` and
stored as `line.is_dialogue: bool`.

### Play mode (is_prose = false)

A line is dialogue if it is NOT any of:

- Blank (empty or whitespace-only)
- Separator (starts with `=`)
- Act/scene marker (ACT, SCENE, CHAPTER, PROLOGUE, EPILOGUE, INDUCTION)
- Speaker name (all-caps, 2+ characters, optional trailing `.`; may include
  bracketed stage direction like `LUCIANA, [to Adriana]`)
- Stage direction (wrapped in `[...]`, or multi-line opener/closer)

### Prose mode (is_prose = true)

A line is dialogue if it is not blank, not a separator, and not an
act/scene marker. Speaker names and stage directions are treated as content.

### Multi-line stage directions

Folger-cleaned texts have multi-line stage directions spanning 2-19 lines
(the largest is in Henry VIII with 17 continuation lines between opener and
closer). `is_stage_direction` detects single-line (`[...\]`), openers
(`[...` without closing `]`), and closers (`...]` without opening `[`).
Continuation lines in between are caught by `is_inside_stage_direction` in
`viewport.rs` and `is_inside_stage_direction_text` in `text_file_map.rs`,
which scan backward up to 20 lines for an unclosed `[` opener.

### Runtime usage

- **Playback sync** — `pending_advance` finds the next dialogue buffer line
  to advance to when the current timestamp ends
- **Page navigation** — `next_dialogue_from`, `last_dialogue_in_page`,
  `next_dialogue_line`, `prev_dialogue_line` all skip non-dialogue lines
- **Dialogue nav keys** (comma, q, j, k) — move between dialogue lines only
- **Buffer-level check** — `viewport.rs::is_dialogue_line` re-checks the
  buffer text (not the precomputed bool) for viewport math, which also
  catches multi-line stage direction interiors via `is_inside_stage_direction`

Key files: `src/db/line_types.rs` (all classification functions),
`src/input/viewport.rs` (`is_dialogue_line`, `is_inside_stage_direction`),
`src/db/queries.rs` (assignment at load time)

## Synopsis/gloss overlay anti-clipping

The synopsis, gloss, and echoes overlay cards (`src/ui/gloss_overlay.rs`)
scroll their own text in a `gtk4::TextView` inside a `gtk4::ScrolledWindow`
(`gloss_view` in `gloss_scrolled`), separate from the main reading card. They
reuse the **same line-snapping + bottom-clip technique** the main card uses, so
a partial (half) line never sits clipped against the title rule (top) or footer
rule (bottom). A CSS `mask-image` fade was tried first and does **not** work —
GTK4 (4.22) silently ignores `mask-image` on widgets, so do not use it here.

### Open-at-top (`reset_scroll_top`)

Called by `show_synopsis` / `show_gloss_with_color` / `show_echoes` after the
buffer text is set. Snapping to the top inline — or on a single idle tick — is
**timing-dependent and unreliable**: `set_visible` and `apply_font` recompute
the vadjustment range on a later layout pass, which on a slow real display
lands after the idle fires, leaving the card scrolled down with the first lines
clipped. Instead `reset_scroll_top` connects a **one-shot handler on the
vadjustment `changed` signal** (emitted when the range is recomputed, i.e. when
layout settles): it snaps to `lower()`, recomputes the bottom clip, then
disconnects. This reacts to the actual layout event rather than guessing a
delay. An `idle_add_local_once` backstop covers the case where `changed` fired
before the handler connected.

### Top edge — line-snapped scrolling (`scroll_gloss`, `snap_value_to_line`)

`scroll_gloss(delta)` no longer steps by a fixed pixel amount (the old fixed
60px step is what left partial lines). It computes a raw target
`value + 3 * line_height * delta`, then `snap_value_to_line(target_y)` returns
the greatest line-top `y` at or below the target — found by walking lines via
`view.line_yrange(&iter)` — clamped to `[lower, upper - page_size]`. This is the
overlay's local analogue of `snap_scroll_to_line` in `scroll.rs`: the viewport
top always aligns to a whole line.

### Bottom edge — invisible clip box (`recompute_overlay_bottom_clip`)

`bottom_clip` is a `gtk4::Box` overlaid on `gloss_scroll_overlay` (valign=End,
halign=Fill, `can_target=false`, `add_css_class("gloss-bottom-clip")` so it
paints the card background and hides — rather than recolors — whatever is beneath
it).

The clip math walks **real per-visual-row rects** (`display_rows`, which steps
`forward_display_line` and reads each row's `iter_location` rect), **not**
`line_yrange`. This is deliberate: the synopsis/gloss buffers join paragraphs
into single multi-row buffer lines and apply per-tag `pixels_above_lines`/`scale`,
so rows are not uniform and `line_yrange` (logical-line granular) would collapse a
wrapped paragraph to one paragraph-tall "row" and clip the wrong amount. It finds
the bottom of the last visual row that fits **entirely** above the viewport bottom
(`top_y + page_size`), then sets the clip height to
`viewport_bottom − last_full_bottom` so the leftover partial row at the bottom is
covered. Two guards: if the document ends inside the viewport it covers only the
slack below `content_h`; if a single row is taller than the viewport (nothing
fits) the clip stays at 0 so that row is not blanked.

The gloss overlay's `&self` entry point `update_bottom_clip` is a one-line call to
the shared `crate::ui::recompute_overlay_bottom_clip(view, clip, scrolled)` (see
"shared helpers" below); it no longer carries its own copy of the algorithm.

**Recompute on EVERY scroll, not just on the named scroll methods.** The clip is
recomputed from (a) `reset_scroll_top`'s `changed`-signal handler + idle backstop
during an open, (b) the explicit `update_bottom_clip()` calls inside
`scroll_gloss` / `scroll_gloss_to_top` / `scroll_gloss_to_bottom`, **and** (c) a
dedicated handler on the vadjustment's **`value_changed`** signal (connected in
`new()` right after `bottom_clip` is created). Path (c) is the catch-all: the
`changed` handler fires only while the adjustment *range* shifts (during an
open), so once the user scrolls and the range is stable the clip would keep its
stale open-time height. Recomputing on every *value* change keeps the bottom
mask aligned no matter how the scroll position moved.

**The clip box only masks the BOTTOM edge.** There is no top clip box — the top
edge is kept clean entirely by line-snapping the viewport top to a whole row
(`snap_value_to_line`). If a scroll lands the viewport top on a fractional row,
the first line shows clipped under the title rule with no mask to hide it, so
the snap must be correct or the top clips. See the section above on
`scroll_gloss` for the snap.

**Coordinate-space gotcha — `display_rows` must add `top_margin`.** Both the
bottom-clip and the top-snap walk visual rows via `display_rows`, which reads
each row rect with `iter_location`. `iter_location` returns **buffer**
coordinates (y = 0 at the first line of text; the view's `top_margin` is NOT
included), but the vadjustment scrolls over `top_margin + text + bottom_margin`,
so `adj.value()` / `adj.upper()` are `top_margin` larger. Comparing the two
directly shifts every row up by `top_margin`. Symptom (both edges clipped at
once): the bottom-clip under-counts the last partial row so it pokes through
under the footer rule, AND `snap_value_to_line` returns a top `top_margin` px
above the real row top so the first line clips under the title rule after a
scroll. `display_rows` therefore adds `view.top_margin()` to every row so its
output is in vadjustment space. (The main reading card avoids this entirely by
using `line_yrange`, whose y already includes the relevant offsets — but the
overlay can't, because its multi-row paragraphs need per-visual-row rects.)

### The journal overlay shares this clip (and once didn't — descender bug)

The **journal Q&A overlay** (`src/ui/journal_overlay.rs`) renders prose with the
same non-uniform rows as the gloss overlay (paragraph gaps, a larger title row,
descenders), so it needs the same per-row bottom clip. It originally used a
**uniform row-step estimate** instead: `update_bottom_clip` took the first
line's `line_yrange` as a fixed `step` and clipped `page_size − floor(page/step)
× step`. That assumes every row is `step` tall, so on overflowing prose the last
visible line's **descenders were cut by the footer rule** — the exact failure
this section warns `line_yrange` causes. (It was masked until the journal text
padding was widened to `card_side_margin`, which changed the wrap and pushed a
descender-bearing line to the bottom edge.)

The fix made both overlays share one implementation. The descender-correct logic
lives as free helpers in `src/ui/mod.rs`:

- `display_rows(view)` — the per-visual-row walk (`forward_display_line` +
  `iter_location`, `top_margin` added), for TextView-content overlays.
- `bottom_clip_height(rows, top_y, viewport_h, content_h)` — the **pure** clip
  math (last-full-row bottom → viewport bottom, with the empty-viewport,
  document-ends-inside, and single-tall-row guards). Unit-tested in
  `ui::bottom_clip_tests`, including a non-uniform-row case that a uniform-step
  estimate gets wrong. **This is now the single covering algorithm for every
  free-scroll surface** (overlays AND scroll-mode).
- `recompute_overlay_bottom_clip(view, clip, scrolled)` — the GTK wrapper for a
  TextView-content scrolled window (uses `display_rows`).
- `line_yrange_rows(view, top_val, viewport_h)` — the logical-line analog of
  `display_rows`, for scroll-mode (j/k) which clips on whole-line `line_yrange`
  geometry, not wrapped rows. `scroll.rs::scrolloff_bottom_clip_widgets` builds
  these rows and feeds them to `bottom_clip_height` (it was a verbatim copy of
  that algorithm — now it shares it).
- `recompute_overlay_bottom_clip_box(clip, scrolled)` — the variant for an
  overlay whose scrolled child is a widget **Box**, not a TextView (the
  translation overlay's column stack). A Box lays out whole child widgets that
  GTK never splits across the edge, so there is no wrapped partial row to mask —
  it covers only trailing slack below the content when the content ends inside
  the viewport (else clips 0). The translation overlay had **no** bottom clip
  before; this guard is what keeps a short translation's trailing slack from
  reading as a clipped edge.

Both the gloss `update_bottom_clip` and the journal `update_bottom_clip` are now
one-line calls to `recompute_overlay_bottom_clip` — neither carries its own copy.

**Not unified (deliberately, do NOT "dedup"):** the main reading card's
`scroll.rs::update_bottom_clip` is a *paginated* clip — it sums `line_yrange`
heights from a known `page_top` to a column-split/section boundary, with
`descender_guard`/`BASE_BOTTOM_MARGIN`/`exact_end` logic. That is a different
strategy from the free-scroll partial-row mask above; merging them would change
behavior. Likewise the gloss vs journal `snap_value_to_line` are different
algorithms (per-`display_rows`-row snap vs uniform `row_step` rounding), not
duplicates. See `docs/plans/2026-06-25-clip-prevention-design.md`.

**Lesson: any overlay clipping a multi-row prose buffer must use per-row geometry
— never a uniform row-step — or the last line's descenders clip.**

### Margins (cosmetic, separate from clipping)

`gloss_scroll_overlay` carries `set_margin_top(24)` and `set_margin_bottom(20)`
so there is breathing room below the title rule and above the footer; the
line-snap and bottom-clip work on top of these. The `gloss_view` also keeps its
construction-time `set_top_margin`/`set_bottom_margin` (internal padding that
scrolls with the content).

### Verifying

Real GTK pixel layout is what matters here and headless rendering (the
`cage` + `grim` flow in the repo CLAUDE.md) lays out fonts/metrics differently —
it confirms the mechanism runs and roughly looks right but cannot prove
pixel-exact edge alignment. Confirm on the real display: open a long synopsis
(`h`), scroll with `j`/`k`, and check both edges show only whole lines.

Key files: `src/ui/gloss_overlay.rs` (`reset_scroll_top`, `scroll_gloss`,
`snap_value_to_line`, `update_bottom_clip`), `src/ui/mod.rs` (`display_rows`,
`bottom_clip_height`, `recompute_overlay_bottom_clip`, `line_yrange_rows`,
`recompute_overlay_bottom_clip_box` — the shared free-scroll helpers),
`src/input/scroll.rs` (`snap_scroll_to_line`, `update_bottom_clip` — the main
card's *paginated* clip, NOT the same algorithm; `scrolloff_bottom_clip_widgets`
— scroll-mode, now routed through the shared helper), `src/input/viewport.rs`
(`visible_range`)

## Translations overlay (`i`)

Unlike the synopsis/gloss overlay, the translation view is **not** a separate
widget — `show_translations` (`app.rs`) inserts a smaller italic translation
line directly into the main buffer below each original line, so the reader keeps
using `state.text_view` / `state.scrolled_window` and all the normal `page_top_line`
viewport math. `hide_translations` removes the inserted lines and restores the
two-column layout. `translations_visible` forces `column_count()` to 1.

### Card width and margins

Translation mode renders two logical columns (original + translation), so its
card is sized like the **two-column** layout, not a narrow single column:
`target_card_width` (`app.rs`) takes the `column_count >= 2 || translations`
branch (proportional `window_width * TWO_COLUMN_WIDTH_FRACTION`, floored at the
two-column width). The text is then inset like the gloss/synopsis cards —
`apply_tiled_mode` sets `left_bump` and the right margin to ~`card_width/4`
(clamped to the window, so it degrades to 0 on a narrow display). This
translation branch runs **before** the `tiled` short-circuit, because `tiled` is
computed against `column_width` (the single-column config), not the wider
translation card.

### No verse line numbers

The right-gutter every-5th foliation is suppressed in translation mode:
`rebuild_line_number_gutter` (`app.rs`) gates `show_numbers` on
`!state.translations_visible`. The interleaved original/translation rows would
otherwise make the numbers misleading. The sign column (left gutter `u`/`.`
markers) is separately suppressed via `sign_column_visible.set(false)` in
`show_translations`. The gutter teardown at the top of
`rebuild_line_number_gutter` runs unconditionally, so toggling translations off
reinstalls the numbers.

### Anti-clipping — two distinct fixes (toggle + navigation)

**Symptom:** with translations on, the top line is half-clipped at the top edge
and the bottom line is half-clipped at the bottom edge — both on first toggle and
while navigating with `j`/`k`. Without translations the same card snaps cleanly.

The translation view does **not** page-turn like the normal reader. It scrolls
**continuously** with a vim-style scrolloff (`cursor_next_dialogue` /
`cursor_prev_dialogue` take the `translations_visible` branch and call
`scroll_cursor_into_view_scrolloff`, not `scroll_after_jump_*`). That breaks the
two assumptions the paged anti-clipping machinery relies on, in two places:

**(a) Toggle-on anchor — snap top to `page_top_line`, clip bottom immediately.**
Inserting ~3000 translation lines shifts every buffer index; `show_translations`
remaps `page_top_line` via `map_line_after_insert` (correct — it always lands on
an original line). The deferred re-anchor idle used to set `adj.set_value` to the
**cursor's** old screen-y, which leaves the scroll between line tops. It now snaps
to `page_top_line`'s **exact pixel top** via `line_yrange`, clamped to
`[0, upper - page_size]`, **and then calls `scrolloff_bottom_clip_widgets`** to
cover the partial bottom line on the very first reveal — before any j/k. (The
earlier version relied on the paged `refresh_bottom_clip` here, whose scheduled
idles are unreliable right after the big insert, so the bottom clipped on open.)
The anchor must stay in the idle: GTK hasn't re-laid the grown buffer
synchronously, so `line_yrange`/`upper` are stale until the layout pass. Confirm
via the `TRANSLATIONS_SHOW: idle snap to page_top` log line showing `clamped == y`.

**(b) Navigation scroll — line-snap the target + scroll-aware bottom clip.**
`scroll_cursor_into_view_scrolloff` (`scroll.rs`) computed a raw pixel target
(`cursor_top - margin` / `cursor_bottom + margin - page_size`) and set it directly,
landing between line boundaries (top clip). It now:

- snaps that target down to a whole-line top via `snap_value_to_line_top` (which
  uses `TextView::line_at_y` for O(1) y→line mapping), and
- covers the partial bottom line with `update_scrolloff_bottom_clip` — a
  **scroll-position-aware** clip. Its widget-level core
  (`scrolloff_bottom_clip_widgets`) builds whole-line rows from the current
  `adj.value()` via `line_yrange_rows` (`line_at_y` + `forward_line`) and feeds
  them to the shared pure `bottom_clip_height`, so scroll-mode runs the SAME
  covering algorithm as the overlays (it used to inline a verbatim copy). Shared
  with the toggle-on idle in (a), which holds the widgets but not `AppState`.

The paged `update_bottom_clip` (`scroll.rs`) is **page_top-relative** and assumes
the scroll is snapped to `page_top` (offset 0); it is the wrong tool for the
continuously-scrolling translation view, which is why the navigation path uses its
own scroll-aware clip instead. (`update_bottom_clip` does now also add the
`scroll_offset` it had been computing-but-ignoring, which helps any off-boundary
paged case, but the translation nav path does not rely on it.)

**(c) Stale visibility cache.** `is_line_fully_visible` consults the
`last_visible_range` cache of line indices. After the buffer is remapped by a
translation toggle those indices are stale, so the check would mis-report
off-screen lines as visible. `show_translations` / `hide_translations` now clear
`state.last_visible_range` alongside `invalidate_page_tops`.

The failure chain to look for if clipping regresses: in translation mode, j/k →
`cursor_next/prev_dialogue` → `scroll_cursor_into_view_scrolloff`. If that sets a
non-line-aligned `adj.value` (top clip) or skips `update_scrolloff_bottom_clip`
(bottom clip), edges clip. Note `update_bottom_clip`'s scheduled idles may not log
during the toggle in the headless cage — verify on the real display.

### Verifying

Headless `cage` + `grim` confirms the mechanism runs and the gutter is clean,
but (as with the gloss overlay) cannot prove pixel-exact edges. Confirm on the
real display: press `i`, then `x`/`y` to page through, and check both edges show
only whole original+translation pairs and no right-gutter numbers.

Key files: `src/app.rs` (`show_translations`, `hide_translations`,
`map_line_after_insert`, `rebuild_line_number_gutter`, `target_card_width`,
`apply_tiled_mode`), `src/input/scroll.rs`
(`scroll_cursor_into_view_scrolloff`, `snap_value_to_line_top`, `line_at_value`,
`update_scrolloff_bottom_clip`, `scrolloff_bottom_clip_widgets`,
`snap_scroll_to_line`, `update_bottom_clip`, `refresh_bottom_clip`),
`src/input/navigation.rs` (`cursor_next_dialogue` / `cursor_prev_dialogue`
translation branch), `src/input/viewport.rs` (`is_line_fully_visible`,
`visible_range`)
