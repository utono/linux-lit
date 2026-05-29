# Page-Turn Mechanics Enhancements

Three fixes to page-turning behavior for plays: animation lock guard,
structural jump return, and unconditional section-break clamping.

## 1. Animation lock guard (implemented)

`page_forward`, `page_backward`, and `page_backward_bottom` check
`page_turn_lock.is_locked()` at the top and return early if held.

### Problem

These functions modify `page_back_stack` and `current_line` before calling
`set_page`. During a crossfade animation (700ms), `set_page` silently drops
the turn via `page_turn_lock`. The stack entry is consumed but the page never
turns — subsequent `y` presses skip pages.

### Fix

Guard all three functions with `is_locked()` before any state mutation. Key
press is simply ignored while the animation runs.

### Files changed

- `src/input/navigation.rs` — early return in `page_forward`,
  `page_backward`, `page_backward_bottom`
- `docs/troubleshooting/page-turning-mechanics.md` — new "Page-turn
  animation lock" section

## 2. Structural jumps preserve return point

### Problem

Scene jumps (`2`/`3`), chapter jumps (`[`/`{`), `gg`, `G`, bookmark jumps,
vocab jumps, and `zt` all clear `page_back_stack` before calling
`set_page_instant`. After a scene jump, pressing `y` falls through to
`prev_page_top` Tier 3 (lpp approximation), which overshoots badly —
especially near scene boundaries. The typical use case is peek-and-return:
press `3` to glance at the next scene, press `y` to come back.

### Fix

Push `state.page_top_line` onto the stack before clearing it. The clear
still runs so stale history doesn't accumulate, but the one entry that
matters — where the user just was — survives for a single `y`.

Change every call site that currently does:

```rust
state.page_back_stack.clear();
set_page_instant(state, target);
```

To:

```rust
state.page_back_stack.clear();
state.page_back_stack.push(state.page_top_line);
set_page_instant(state, target);
```

### Call sites

All in `src/input/navigation.rs`:

- `jump_to_start` (gg)
- `jump_to_end` (G)
- `jump_to_prev_paragraph`
- `jump_to_next_paragraph`
- `jump_to_prev_chapter` ([)
- `jump_to_next_chapter` ({)
- `jump_to_prev_scene` (2)
- `jump_to_next_scene` (3)
- `scroll_cursor_top` (zt)
- `jump_to_line` (bookmark picker)
- `jump_to_prev_vocab` / `jump_to_next_vocab`

### Behavior after the fix

- `x x x 3 y` — returns to the page the user was on when they pressed `3`
- `x x x 3 y y` — second `y` has an empty stack, falls through to
  `prev_page_top` (normal behavior for "no more history")
- `3 3 y` — returns to where the user was before the second `3`, not the
  first (each jump replaces the single return entry)

### page_back_stack invariant update

The current rule is "structural jumps clear the stack." The new rule is
"structural jumps replace the stack with a single return entry." Update
`docs/troubleshooting/page-turning-mechanics.md` to reflect this.

## 3. Unconditional section-break clamping

### Problem

`clamp_at_section_break` in `viewport.rs` has a half-full guard:

```rust
if total * 2 < range.total_height {
    return range;
}
```

When a scene break appears in the top half of a page (content before the
break fills less than 50% of the viewport), the clamp is skipped. The scene
header appears mid-page, and the next `x` skips past it — the reader never
sees a clean scene-header-at-top page.

### Fix

Remove the half-full guard entirely. The clamp always fires when a scene
marker or separator is found in the visible range, ending the page at the
line before the break.

This means scene headers always appear at the top of the next page, even if
the resulting page has only a few lines of old-scene content.

### Files changed

- `src/input/viewport.rs` — remove the `total * 2 < range.total_height`
  guard and the associated height-recomputation loop in
  `clamp_at_section_break`

### Test impact

Existing headless page-turn tests (`test_page_forward_all_shakespeare_no_stuck`,
`test_page_forward_backward_roundtrip_all_shakespeare`) must still pass. Short
pages near scene breaks are valid — the stuck-state detector allows any
forward progress.

The `test_x_page_forward_covers_every_line_errors` test verifies that every
non-blank line appears in at least one visited viewport. Short pages don't
break this — they just produce more pages.

## Files changed (summary)

- `src/input/navigation.rs` — animation lock guards (done), push-before-clear
  at all structural jump sites
- `src/input/viewport.rs` — remove half-full guard in `clamp_at_section_break`
- `docs/troubleshooting/page-turning-mechanics.md` — update page_back_stack
  rules section and animation lock section (partially done)
