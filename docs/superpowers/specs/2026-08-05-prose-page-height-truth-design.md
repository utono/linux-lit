# Prose page generation must measure true wrapped heights

_Design spec — 2026-08-05. Target: `master` (`b0ccf5c9`). Diagnosis worktree:
`~/utono/linux-lit-wt/diag/ch37-clip` (`ROOT-CAUSE.md`, `TRACE-FINDINGS.md`,
`DIAGNOSIS.md`, raw logs)._

## Problem

Prose pages in `BH-Barrett` still overflow the card after the pv5→pv7 fix. In
chapter 37 at 1920x1200 (`text_view` 1128, `uh1071`), **22 distinct pages
overflow by 13–114px**. `paged_bottom_clip` floors to 0 and the last line
renders sliced mid-glyph with no masking band. The user reported it from the
live build; it reproduces headlessly.

The engine serving those pages is the **stored pv7 table** (`PAGES_PROSE: table
hit`, `validated=1`, generated after both fix commits). So this is a residual
defect, not a stale table.

## Root cause

`record_prose_pages` (`src/input/prose_pages.rs:435`) builds the page grid from
a buffer-wide `line_yrange` sweep. The sweep's premise — documented in the code
— is that `line_yrange` validates a line synchronously and GTK caches the
result. **That premise does not hold for lines far from the viewport.**

Measured, on one run, for page top 4258:

```
GEN_HEIGHTS      [4220..=4240] = [68,96,68,124,40,40,293,349,153,68,40,124,40,...]
POSTWALK_HEIGHTS [4220..=4240] = [68,96,68,124,40,40,293,349,153,68,40,124,40,...]
RENDER_HEIGHTS   page_top=4258  = [96,96,40,40,96,40,96,40,68,209,293,406]
```

Per-line deltas across 4258–4269: `0,+28,0,0,0,0,+28,0,+28,+28,+56,+57` =
**+225px**, every delta an exact multiple of the ~28px row pitch. At generation
the viewport sat at line 4223; the divergent lines were 35–46 rows below and had
never been displayed.

Eliminated by measurement, each of which killed a hypothesis:

- **Tags and fonts.** 5 of 6 divergent lines carry byte-identical tags at
  generation and render, applied *before* generation. The `font-size` reapply
  path (the pv5→pv7 mechanism) is not this bug. `vocab_tag` is
  foreground-only and cannot change wrap.
- **Geometry.** `wrap_w=899` at both phases; margins and font identical.
- **The boundary walk.** `GEN_HEIGHTS == POSTWALK_HEIGHTS` byte-identical.
- **Arithmetic.** `page_px` and `exact_page_content_height` are algebraically
  identical at `end_off == 0`. The heights differ, never the maths.

### The convergence guard is self-referential

The same run logged `changed_between_sweeps=0` and `delta_sum=0` — two
back-to-back sweeps agreeing exactly — while the render measured +225px on those
lines. Both sweeps read the same unvalidated cache, so they agree with each other
and with nothing real. **The guard proves self-consistency, not correctness**, and
adding a third sweep cannot help.

### Generation is not deterministic

Same fingerprint, same content, same geometry, three page counts:

- **808** — stored this morning, `validated=1`, the table the user was reading
- **806** — the traced regeneration
- **801** — currently stored at 1920x1200 pv7

A pure function of content and layout cannot return all three. Generation
depends on how much of the buffer GTK had validated at that moment, which
depends on scroll history and timing. **This is why the pv-bump strategy cannot
fix it:** a fingerprint cannot encode a race, so each regeneration is a fresh
roll of the dice, and a bad table can be stored `validated=1` at any time.

## Approach

Make generation measure heights from a source that does not depend on what GTK
has validated. The codebase already contains such a measurement.

`LIT_TRACE_PANGO` (`prose_pages.rs:490`) builds an independent Pango layout per
line at the view's real wrap width, with the body font taken from the
buffer-wide `font-size` tag rather than the view context, and computes:

```
let pango_h = layout.pixel_size().1 + above + below;
```

That is a complete line-box height in the same terms the page budget uses. It
is presently computed only to log a comparison. **Promote it from a probe to the
generation-time measurement**, behind the staged rollout below.

### The honest caveat, and why the rollout is staged

Pango is not a proven drop-in oracle. On the traced run it disagreed with
`line_yrange` on **6,047 of 7,300 lines**, `delta_sum=-1561`, **in both
directions** — e.g. `L1: yr=265 / pango=237` (yrange over-measures by 28px)
against `line 4333: pango=322 / yrange=293` (yrange under-measures by 29px).

Under-measurement is what overflows a page. Over-measurement is harmless to
correctness but packs pages loose. Before Pango can be trusted as the source of
truth, one question must be answered with evidence:

> On lines that have been **fully displayed** — where `line_yrange` is known
> good — does the Pango layout agree with it?

If yes, Pango is correct everywhere and `line_yrange` is wrong on unvalidated
lines; adopt it wholesale. If no, the two differ systematically (e.g. Pango
misses a tag the view applies) and Pango needs correction before adoption.
**This spec does not assume the answer.** Task 1 of the plan measures it, and
the outcome selects between Option A and Option B below.

### Option A — Pango is the measurement (preferred if Task 1 confirms it)

Replace the `line_yrange` sweep in `record_prose_pages` with the Pango
computation. Independent of viewport position and of GTK's validation state, so
generation becomes deterministic: same content plus same geometry yields the
same table on every run. Keep `line_yrange` in the validator as a
**cross-check**, not as the source.

### Option B — force validation before sweeping (fallback)

If Pango proves not to agree on known-good lines, force GTK to validate every
line before the sweep — scroll the view through the buffer, or call the
validation API directly if one is reachable — and sweep only after. Slower and
inherently more fragile (it re-depends on GTK internals this bug just showed to
be untrustworthy), so it is the fallback, not the default.

### Refuse rather than store a bad table

Independent of A or B: the convergence guard should compare the sweep against
the **independent** measurement, not against a second copy of itself. When they
disagree beyond a small tolerance, log `VALIDATE_FAIL` and fall back to the live
engine rather than persisting a table. A table stored `validated=1` must mean
validated against ground truth.

## Scope

In scope: prose page generation and its validation, `src/input/prose_pages.rs`.

Out of scope, deliberately:

- **Play/verse `play_pages`.** They work and are not implicated. If the same
  lazy-validation flaw affects them, that is a separate, evidence-led change.
- **The rendering and clip path.** `exact_page_content_height` and
  `paged_bottom_clip` are correct; they faithfully report an overfull page.
- **Continuous prose scrolling** (`feat/prose-continuous-scroll`). Unrelated and
  unmerged.

## A fingerprint bump is required, and is not sufficient

Every stored prose table was generated from the flawed sweep, so all are
suspect and must be evicted — a `pv7` → `pv8` bump. But the bump is bookkeeping,
not the fix: the previous bump regenerated the table and the overflow survived,
because each regeneration re-rolls the same race. The bump only matters once
generation is deterministic.

## Testing

The prior fix's acceptance test lands on `LIT_START_SCENE=26.0` — chapter 26,
which passes — and chapter 37 was never re-verified. That is precisely how a
real defect stayed green. Requirements:

- **A failing repro first**, per the repo's TDD default for pagination. Land on
  chapter 37 at 1920x1200 and assert **no `CLIP_WARN ... OVERFLOW`**. It must
  fail on master before any fix.
- **Determinism test.** Generate the table three times at one geometry and
  assert an identical page count and identical boundaries each time. This is the
  test that would have caught the 801/806/808 spread, and it is the one that
  proves the root cause is actually addressed.
- **Whole-table census, not a sampled drive.** A 14-turn drive visits ~14 of
  ~800 pages. Assert on the generation-time census (`PAGES_PROSE_DRIFT: summary
  … over_usable=K`) with `K == 0`, and — because the census historically read
  the same wrong heights generation used and so agreed with itself — also
  re-measure at render for a deep sample.
- **Verse/play regression.** Run the nav-fuzz with `--start-work` to confirm
  play pagination is untouched.
- **Real-renderer confirmation before merge.** Cage is software rendering and has
  disagreed with the real GL renderer on layout before. The acceptance is the
  user opening BH-Barrett chapter 37 and seeing the bottom line clear of the rule.

## Risks

- **Pango may not be the oracle.** Addressed by making Task 1 an
  evidence-gathering step that selects the approach, rather than assuming it.
- **Per-line Pango layout may be slow** across a 7,300-line buffer. Generation
  is already a one-shot cost paid on load or resize, and correctness dominates
  here — but the plan should measure generation time before and after and report
  it rather than discovering a regression on the user's machine.
- **Loose packing.** If Pango over-measures relative to the view on some lines,
  pages pack slightly loose. That is the safe failure direction: a short page is
  a cosmetic loss, an overfull page cuts text.
