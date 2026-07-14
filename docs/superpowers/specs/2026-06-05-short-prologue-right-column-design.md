# Short opening section fills the first spread's right column

**Date:** 2026-06-05
**Status:** Implemented (runtime visual verification pending — user runs e2e)

## Problem

For a work whose first section is short front matter — a Prologue, Induction,
Chorus, or a brief opening scene that fits within a single column — the current
two-column pagination renders it in the LEFT column and leaves the RIGHT column
empty.

Concretely, Henry VIII (H8) opens:

```
Prologue            │
[Enter Prologue.]   │
PROLOGUE            │   (empty)
 I come no more...  │
 ...                │
[He exits.]         │
```

The desired result, the start-of-document mirror of the existing end-of-document
EPILOGUE rule (a short tail fills the RIGHT column rather than sitting alone in
the left):

```
                    │ Prologue
                    │ [Enter Prologue.]
   (empty)          │ PROLOGUE
                    │  I come no more...
                    │  ...
                    │ [He exits.]
```

## Why it currently lands left + empty right

`column_split(state, 0)` (`src/input/viewport.rs`) for the first spread:

1. The left column fills from line 0; `clamp_at_section_break` truncates it just
   before the Act 1 boundary `N` (the first `section_starts` line after the
   Prologue). The whole short Prologue `[0, N)` is the left column. `split = N`.
2. The "right column would BEGIN a new (non-final) section" early return fires:
   skipping leading blanks/exits from `split = N` lands on the Act 1 chrome line,
   `is_section_start(N)` is true, not the final section, so it returns
   `ColumnSplit { split: N, page_end: N-1, next_page_top: N }`.
3. The right view is scrolled to `split = N` and clipped to `page_end + 1 = N`,
   rendering zero lines — the empty right column.

## Design

### Scope

Fires on the **first spread only** (`page_top` is the work's first content line),
for **any** opening section that fits in one column. Not just literal Prologues —
also Induction, Chorus, or a short opening scene. Single-column (prose,
translation) modes are unaffected because the two-column `column_split` path does
not run there.

### Placement: empty left, section in the right

When the trigger holds, the opening section moves to the right column and the
left column is empty. This is the direct mirror of the EPILOGUE short-tail rule
(`last_page_top` / `would_empty_right_column`), which fills the right column and
leaves the left padded.

### The change — `column_split` only

`column_split(state, page_top)` is the single source of truth (render, the
page-tiling fuzz invariants, `prev_page_top`, `last_page_top` all consult it).
The change lives there and nowhere else; a parallel boundary calc is exactly the
anti-pattern the troubleshooting doc warns against.

Trigger (all must hold):

1. `page_top == 0` — the first spread (after the existing first-chrome
   resolution; page_top is the work's first buffer line).
2. The opening section is complete and short: the left column's
   `clamp_at_section_break` truncated at a `section_starts` boundary `N`
   (`N < line_count` and `is_section_start(N)`), i.e. the section `[0, N)` ends
   inside the spread.
3. The whole opening section `[0, N)` fits within the RIGHT view's height — so
   moving it right does not itself overflow. Verified with the same
   `visible_range` / pixel-height walk used for the right column, measured from
   line 0 against `state.right_view.height()`. If it does NOT fit, fall back to
   the current behavior (the section is not "short" for the right column).

Transform (replaces the empty-right early return for the first spread):

```
ColumnSplit { split: 0, page_end: N - 1, next_page_top: N }
```

- `split = 0` → the right view starts at line 0; the left column range `[0, 0)`
  is empty.
- `page_end = N - 1` → last line of the opening section (right column bottom
  clip).
- `next_page_top = N` → **unchanged** from today; Act 1 starts the next spread.

### Why tiling is preserved

`next_page_top` stays `N`, identical to the current behavior. `prev_page_top`
(walks `column_split(probe).next_page_top`) and `last_page_top` are unaffected;
`y` from Act 1's spread still tiles back to this first spread exactly. Only the
visual placement *within* the first spread changes — never a spread boundary.

### Renderer

The renderer (`snap_scroll_to_line` in `src/input/scroll.rs`) already scrolls
`right_view` to `cs.split` and `text_view` to `page_top`, clipping the left at
`cs.split` and the right at `cs.page_end + 1`. With `split = 0`:

- left clip = 0 → left column renders zero lines (empty), and
- right view scrolled to 0, clipped to `N` → shows `[0, N)`.

No renderer change is anticipated. If the left view with a 0-height clip does not
visually clear (e.g. a stale paint), that is a follow-up in `update_bottom_clip`,
not a `column_split` change.

## Edge cases

- **No front matter** (Act 1 Scene 1 is the very first section and is long):
  trigger 2 fails — the first section does not end inside the spread — so the
  normal left-to-right fill runs. No change.
- **Opening section longer than one column:** trigger 3 fails (does not fit the
  right view height) — fall back to current behavior.
- **Single-section / one-spread works:** if the only section is short and there
  is no `N` boundary (`is_section_start(N)` false / `N >= line_count`), trigger 2
  fails — unchanged.
- **Prose / translation mode:** two-column `column_split` path does not run.

## Testing

- **Headless unit/fuzz assertion:** for H8, assert the first spread
  (`column_split(state, 0)`) returns `split == 0`, `page_end == N - 1`,
  `next_page_top == N`, with `N` the first `section_starts` line — i.e. left
  empty, right holds the Prologue, tiling boundary unchanged. Confirm the
  existing all-Shakespeare tiling fuzz still passes (no new gaps/overlaps).
- **Visual e2e (user-run):** rendered-spread criterion — the user runs
  `./scripts/e2e-env.sh` (or the manual `cage` + `grim` single-work launch on
  H8) to confirm the Prologue renders in the right column with an empty left.

## Documentation

Update `docs/troubleshooting/page-turning-mechanics.md`:

- *The pagination model* → the spread-extent list: note the first-spread
  exception alongside the final-spread EPILOGUE exception.
- *Section-break clamping* → the "Right column would BEGIN a new scene" bullet:
  document that on the FIRST spread a short complete opening section moves to the
  right column (the start-of-document mirror of the EPILOGUE rule).
