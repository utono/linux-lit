# Page Turning Mechanics

Reference for debugging page-forward (`x`), page-backward (`y`), and related
navigation in e-reader mode.

## Architecture

Page state lives in `AppState`:

- `page_top_line: usize` — buffer line at the top of the current viewport
- `page_back_stack: Vec<usize>` — history of previous `page_top_line` values,
  pushed by `page_forward`, popped by `page_backward`

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
  `clamp_at_section_break`, `back_up_for_speaker`, `is_dialogue_line`,
  `is_inside_stage_direction`
- `src/input/scroll.rs` — `set_page`, `set_page_instant`, `snap_scroll_to_line`
- `src/db/line_types.rs` — `is_dialogue`, `is_stage_direction`, `is_speaker`,
  `is_act_scene_marker`, `is_separator`

## page_back_stack rules

Every function that changes `page_top_line` must interact with the stack:

- **page_forward (`x`)** — pushes old `page_top_line` before turning
- **page_backward (`y`)** — pops; falls back to `prev_page_top()` when empty
- **page_backward_bottom (Shift+comma)** — pops (same as `page_backward`)
- **Jump functions (gg, G, `[`, `{`, 2, 3, bookmarks, vocab)** — clear the stack
- **zt (scroll_cursor_top)** — clears the stack
- **Line-by-line navigation (comma, q, j, k)** — no stack interaction; incidental
  page turns from `scroll_after_jump_forward/backward` don't touch the stack
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

## Debugging page-backward wrong destination

### Symptom

Pressing `y` after `x` doesn't return to the previous page — it jumps much
further back or to an unrelated position.

### Diagnosis

1. Check the log for `PAGE_BWD: stack pop` vs `PAGE_BWD: prev_page_top`.
   Stack pop means the back-stack had an entry; `prev_page_top` means it
   was empty and had to recompute.
2. If the recomputed `prev_page_top` is wrong, `page_forward` likely used
   the fallback path (`candidate_top = next_dialogue`) which produced a
   `page_top_line` not on a natural page boundary. `prev_page_top`'s Tier 2
   linear walk can't find it, and Tier 3's lpp approximation overshoots.
3. Check whether the navigation that preceded `y` pushed to or cleared the
   stack. If a jump function forgot to clear, the stack has stale entries.

### Common causes

- **New jump function doesn't clear the stack** — add
  `state.page_back_stack.clear()` before its `set_page`/`set_page_instant`
  call.
- **`page_forward` fallback path** — when `new_top <= page_top_line`,
  `page_forward` sets `candidate_top = next_dialogue` (skipping the
  section break). The resulting `page_top_line` is non-canonical, but the
  stack push ensures `y` still returns correctly. If the push is missing,
  `prev_page_top` must recompute and may fail.

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
10 lines looking for an unclosed `[` opener. This function is used by
`is_dialogue_line`, `next_dialogue_from`, `last_dialogue_in_page`, and
`back_up_for_speaker` to ensure multi-line stage directions are never
treated as dialogue.

If a new multi-line pattern appears that isn't caught, `next_dialogue_from`
will return one of its lines as "the next dialogue", and
`back_up_for_speaker` may pull the page top behind a section break,
creating a stuck loop.

## Section-break clamping

`clamp_at_section_break` in `trim_visible_range` scans the visible range
for act/scene markers or separators. When found, it clamps `last_fit` to
the line before the break so the new section starts at the top of the next
page.

Edge case: when the section break is very close to `page_top` (1-2 lines),
the clamped page is trivially small. `next_page_top` then computes a
`next_dialogue` whose `back_up_for_speaker` pulls back behind the break,
producing `new_top <= page_top` (no progress). `page_forward` handles this
with the fallback `candidate_top = next_dialogue`.

## Headless tests

`src/input/navigation.rs` has headless tests that verify page turning
across all Shakespeare plays without GTK:

- `test_page_forward_all_shakespeare_no_stuck` — pages forward through all
  38 plays, panics if any stuck state (no progress after 2000 iterations)
- `test_page_forward_backward_roundtrip_all_shakespeare` — forward tops
  must be strictly increasing; backward via history must round-trip exactly
- `test_page_forward_no_gaps_or_repeats` — Troilus-specific: every
  highlighted line is dialogue, strictly increasing, gaps bounded by
  page_size
- `test_x_page_forward_covers_every_line_errors` — Comedy of Errors: every
  non-blank line appears in at least one visited viewport
- `test_scene_synopsis_identification_all_shakespeare` — scene markers
  resolve to correct synopsis keys via the database

Run with:

```bash
cargo test -- all_shakespeare page_turn
```
