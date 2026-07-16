# Fuller two-column play columns — design

**Date:** 2026-07-15
**Status:** implemented (N=16); verified.

> **Implementation note (2026-07-16).** `TWO_COLUMN_BOTTOM_MARGIN = 16`, chosen
> empirically at 1920×1200 (recovers +1 line per column — LLL-Arkangel Act 3
> Scene 1 went left 34→35, right 32→33 — with the `LIT_DEBUG_CLIP_COLOR` band
> clearing the last line's descenders). Fingerprint bumped `v2`→`v3`. One
> follow-on surfaced during verification: the shifted spread breaks moved the
> nav-fuzz cursor so a SearchJump could target the work's trailing stage
> direction (`[They all exit.]`) and trip the `viewport fill < 10%` guard. That
> is NOT a product bug — the anchor covers the last DIALOGUE line — and was
> resolved by exempting pure non-dialogue tail landings in `nav_test.rs`. The
> speculatively-drafted anchor-coverage spec/plan (2026-07-16) are SUPERSEDED.

## Problem

On two-column plays (e.g. `LLL-Arkangel`), every column stops roughly one line
short of what would fit, leaving a persistent ~40px blank band at the bottom of
each column. Measured on the Act 3 Scene 1 spread (`top=1154`) at production
1920×1200 geometry, line height ≈ 30px:

- Left column: filled 1000px of 1007px usable → 7px slack
- Right column: filled 981px of 1007px usable → 26px slack

Both columns are packed tight against `usable_height`, but `usable_height` is
itself 45px smaller than the card (1052px), so ~1.5 lines are held blank at the
bottom of each column on every spread.

## Root cause (code-confirmed)

The two-column **fill** decision and the two-column **clip** use *different*
bottom reserves:

- **Fill** (`column_split`, `src/input/viewport.rs:1239` left, `:1376` right):
  `usable = height − descender_guard(5) − BASE_BOTTOM_MARGIN(40)` — reserves
  **45px**.
- **Clip** (`update_bottom_clip` `exact_end` branch, `src/input/scroll.rs`
  ~907–960): clips at `widget_height − Σ line_heights − descender_allowance`,
  where `descender_allowance` is the font descent (~5px) capped by the boundary
  line's blank budget — so it consumes only **~5–10px**.

`BASE_BOTTOM_MARGIN` (40px, `src/input/scroll.rs:733`) is a scroll-reachability /
single-column constant: its documented job is letting the last buffer line
scroll to the viewport top, and the **single-column** clip genuinely covers the
whole 45px band (`scroll.rs` ~1021: `reserve = widget_height − usable_height`).
The **two-column** `exact_end` clip does not — it sums actual line heights and
reserves only the descender allowance. So subtracting the full 40px in the
two-column *fill* over-reserves by ~40px, discarding roughly one line per column.

This is not a data bug and not a clip-math bug. The generated `play_pages` table
is a byte-for-byte cache of the live `column_split` output (`record_spreads`
walks the live chain), so table and live engine underfill identically — the fix
is in `column_split`'s reserve, and it propagates to the table via regeneration.

## Fix

Introduce a two-column-specific bottom reserve for the **fill** path, sized to
what the two-column clip actually consumes.

### New constant

In `src/input/scroll.rs`, beside `BASE_BOTTOM_MARGIN`:

```rust
/// Bottom reserve for a TWO-COLUMN paged column's FILL decision. Unlike the
/// single-column path (whose clip covers the full descender_guard +
/// BASE_BOTTOM_MARGIN band, scroll.rs ~1021), the two-column `exact_end` clip
/// (scroll.rs ~907) consumes only a descender allowance below the last line, so
/// the fill only needs to reserve that much. Reserving the full 40px margin
/// wastes ~1 line per column. Value chosen empirically — smallest reserve whose
/// last-column line keeps a clean descender at production geometry.
pub(crate) const TWO_COLUMN_BOTTOM_MARGIN: i32 = <N>;
```

`N` is decided empirically (see "Choosing N"). It is `≪ 40` but `> 0` (the fill
still needs the descender guard plus a small breathing pad below the last line so
the `exact_end` clip's descender allowance has room to drop into).

### Call sites (three, two-column path only)

Swap `BASE_BOTTOM_MARGIN → TWO_COLUMN_BOTTOM_MARGIN`:

1. `src/input/viewport.rs:1239` — left column `usable`.
2. `src/input/viewport.rs:1376` — right column `usable`.
3. `src/input/page_table.rs` ~379 — `validate_spreads`' `usable`
   (`ValidateCtx.usable_height`). MUST match sites 1–2 exactly, or validation
   rejects the fuller spreads `column_split` now produces (`fit:` errors) and
   generation silently falls back to no-table.

The first-spread short-opening probe (`viewport.rs:1327`, a right-column `usable`
on the two-column path) also gets the new constant so the "does the opening
section fit the right column" test stays consistent with the fill.

### Untouched

- `update_bottom_clip` — the two-column clip already reserves only the descender
  allowance and sums actual line heights, so a fuller column just yields a
  smaller (correct) clip box. No change.
- Single-column paged paths (`scroll.rs:595`, `:897`; `viewport.rs:1138`, `:1174`)
  keep `BASE_BOTTOM_MARGIN` — their clip covers the full band (#10 risk if
  reduced).
- Prose row-fill — untouched.

### Why a new constant, not a smaller `BASE_BOTTOM_MARGIN`

`BASE_BOTTOM_MARGIN` has a legitimate single-column/scroll job. Lowering it
globally would risk slicing the last line's descenders on ordinary single-column
pages (clip-prevention.md #10). A separate constant confines the change to the
two-column fill path.

## Choosing N (empirical spike)

The clip-prevention.md #10 failure — a two-column verse column's last line
(`pixels_above/below_lines = 0`, ink flush to the logical bottom) sliced by the
card-colored clip — is the risk. N must be the *smallest* reserve that still lets
the `exact_end` clip drop its top edge below the last line's descender ink
without slicing.

Procedure, at production 1920×1200 on `LLL-Arkangel`:

1. Build with a candidate `N` (start at the descender guard's clamp ceiling plus
   a few px, e.g. try `N ∈ {8, 12, 16}`).
2. Force-regenerate the table at current geometry (`LIT_GEN_PAGE_TABLE=1`) or run
   with `LIT_NO_PAGE_TABLE=1` (live engine) so the new reserve takes effect.
3. Drive headless to a representative two-column spread and screenshot with
   `grim`; also paint the clip box (`LIT_DEBUG_CLIP_COLOR='#ff0000'`) on one run
   to see the band.
4. Open every PNG and inspect the LAST line of each column for sliced
   descenders (`g/y/p`, trailing comma).
5. Pick the smallest `N` with a clean descender on the last line across the
   sampled spreads. Prefer the value that adds a line while the `_clip.png`
   overlay shows the clip's top edge clearing the descender ink.

Headless cairo rendering is not pixel-exact (clip-prevention.md "Verifying"), so
the final N is confirmed on the real display via the user OR via the
`tests/line_clipping.rs` pixel e2e — not from cairo screenshots alone.

## Stored-table regeneration

Reducing the fill reserve changes where every two-column spread breaks, so all
stored `play_pages` tables become stale (they cache the old, underfilled breaks).
A stale table would render an old split that no longer matches the live fill.

Fix: bump the **layout fingerprint** so every stored table auto-regenerates at
the new fill on next load. The fingerprint (`fingerprint_string`,
`page_table.rs`) already carries a version tag (`v2|…`). Bump it to `v3` so:

- `load_pages` finds no matching `layout_fingerprint` → returns `None` →
  `generate_and_store` runs `record_spreads` at the new reserve → stores a fresh
  `v3` table.
- Old `v2` rows are simply never matched again (harmless; a future vacuum could
  drop them, out of scope here).

No manual DB surgery, no per-work step. Prose `prose_pages` tables are unaffected
(different reserve, unchanged) but share the fingerprint version tag; they
regenerate too, producing identical output (their reserve did not change), which
is correct and cheap.

## Testing & verification

1. **`cargo build`** — clean.
2. **`cargo test --bins`** — the clip-invariant unit tests
   (clip-prevention.md "Verifying") must stay green; they guard the arithmetic
   the clip depends on.
3. **`validate-play-pages`** on a regenerated play — structural PASS (coverage,
   ordering, fit against the NEW usable).
4. **nav-fuzz** (`test-headless-navigation` `run-fuzz.sh --start-work LLL-Arkangel`)
   — no UNBALANCED / short-column / G-idempotency regressions from the fuller
   columns.
5. **Pixel e2e** (`tests/line_clipping.rs` via `scripts/e2e-env.sh`) — the
   fail-closed line-clipping assertion on the main card, to catch a #10 slice.
6. **Before/after screenshots** at 1920×1200 on `LLL-Arkangel` — visually
   confirm each column gains a line and the last line's descenders are clean.
7. **Real-display confirmation** — hand the user the exact e2e/launch command for
   the final eyeball on the real GL renderer (cage is software rendering).

## Files

- `src/input/scroll.rs` — add `TWO_COLUMN_BOTTOM_MARGIN`.
- `src/input/viewport.rs` — two fill sites + the first-spread probe.
- `src/input/page_table.rs` — `validate_spreads` usable + fingerprint version bump.
- `docs/troubleshooting/clip-prevention.md` — add the new consideration (fill vs
  clip reserve asymmetry on the two-column path) to the checklist, per the
  project's post-clipping-change rule.
