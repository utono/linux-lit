# Reserve-independent final-spread anchor — design

**Date:** 2026-07-16
**Status:** approved, implementing
**Depends on:** 2026-07-15-two-column-fill-reserve-design.md (this fixes a
regression that change exposed).

## Problem

The two-column fill-reserve change (fitting ~1 more line per column) made the
final page of a two-column play orphan the work's last 1–2 lines. Confirmed at
**production 1920×1200** (`widget_h=1052`) on `LLL-Arkangel`:

```
last_page_top chosen=4178 page_end=4247 last_line=4249 covers=false
```

The final anchor page ends at line 4247; the work ends at 4249. Pressing **G**
(jump-to-end), searching near the end, or paging forward lands on a degenerate
2-line tail page instead of a full final page that includes the last line. The
nav-fuzz catches it as `SearchJump viewport fill < 10%`.

## Root cause (code-confirmed)

`last_page_top` (`src/input/navigation.rs`) walks the forward page chain and
keeps the last page whose **right column is non-empty** (`last_full`). Its
final-region pull-forward (`navigation.rs` ~543) only accepts a candidate whose
right column is non-empty (`!would_empty_right_column(t)`). When the fuller
columns leave only a 1–2 line tail, no such candidate reaches the last line, so
`chosen` stops one page short and the tail is orphaned.

Before the reserve change, the shorter columns left a **bigger** tail (e.g.
master's `4239…4249`, 11 lines) that filled a left column, so the anchor logic
accepted it. The bug is that the anchor prefers "fills both columns" so strongly
it will stop short of the last line rather than accept a short empty-right final
page — a behavior the old reserve masked.

`record_spreads` (`src/input/page_table.rs` ~313) copies its final table page
from `last_page_top`, so the stored table inherits the same orphan; fixing
`last_page_top` fixes both the live engine and the generated table.

## Fix: coverage fallback in `last_page_top`

Keep the existing "prefer a page that fills both columns" pull as-is. After it
selects `chosen`, add a **coverage fallback**:

> If `chosen`'s spread does not render through the work's last line
> (`column_split(chosen).page_end < line_count - 1`), replace `chosen` with the
> **earliest** page top whose spread's `page_end` reaches the last line —
> accepting an empty right column (a short tail in a lone left column, exactly
> the pre-reserve-change final page shape).

"Earliest such top" is found by walking the forward chain from the current
`chosen` (or from a bounded back-up) and taking the first page whose
`column_split(top).page_end >= line_count - 1`. Choosing the EARLIEST covering
page (not the last) maximizes how full that final left column is — it packs the
tail with as much preceding content as fits, rather than landing on the 2-line
sliver.

### Idempotency (the `G` invariant)

`last_page_top` must be idempotent: recomputing from its own result returns the
same top (the nav-fuzz `JUMP-TO-END not idempotent` guard, `nav_test.rs` ~978).
The fallback preserves this: the chosen covering page's left column already
reaches the last line, so on a second call the same page is selected again (the
fallback either re-selects it or is a no-op because the primary logic now lands
there). The implementation MUST verify this by construction — the fallback's
"earliest covering top" is a pure function of layout + `line_count`, never of
the start position or a target line.

### No regression to the common case

When the tail is large enough to fill a right column (the normal case for most
spreads and for plays with a long final scene), the primary pull-forward already
returns a covering, non-empty-right page, so `column_split(chosen).page_end >=
last_line` holds and the fallback never fires. The change is inert except on the
short-tail case the reserve change created.

## Alternatives considered (rejected)

- **Back the tail up into the previous spread** (merge, making the penultimate
  boundary's left column span through the last line): heavier — shifts more
  spread boundaries and risks the fit-validator (a taller merged left column
  could exceed `usable_height`). The coverage fallback is more local and matches
  the known-good master shape.
- **Larger `TWO_COLUMN_BOTTOM_MARGIN`** to avoid the tiny tail: a tuning
  band-aid that partly defeats the line-gain feature and could recur on other
  plays/geometries. Rejected in the parent spec's Q&A.

## Testing & verification (thorough — several plays)

Design target is production 1920×1200. Verify across `LLL-Arkangel` + at least
two other two-column plays (e.g. a play with a trailing EPILOGUE and one with a
plain dialogue tail — the two final-region shapes `last_page_top` distinguishes):

1. **`cargo test --bins`** — all green (clip + pagination invariants).
2. **Anchor coverage probe** (temporary log, removed after): at 1920×1200,
   `last_page_top` returns a `chosen` with `page_end >= line_count - 1`
   (`covers=true`) for every tested play.
3. **G / search / page-forward convergence:** drive each — `G`, `/`-search on a
   word near the end, and paging forward to the end — and confirm all three land
   on the SAME final page, and that page shows the work's last line.
4. **Idempotency:** the nav-fuzz `JUMP-TO-END not idempotent` guard stays green;
   spot-check that `last_page_top` recomputed from its own result is unchanged.
5. **nav-fuzz** at the harness geometry across the tested plays — no
   `SearchJump viewport fill < 10%` (or other tail) FAILs, and no new failures
   vs master with the same seed.
6. **Real-display hand-off:** give the user the exact launch command to eyeball
   the final page of each tested play on the real GL renderer.

## Files

- `src/input/navigation.rs` — `last_page_top`: add the coverage fallback after
  `chosen` is selected, before `clamp_page_top_to_scroll_ceiling`.
- (No change to `record_spreads` — it inherits via `last_page_top`.)
- `docs/troubleshooting/page-turning-mechanics.md` — document the coverage
  fallback in the "final spread is special" section (the anchor must cover the
  last line even for a short empty-right tail).
