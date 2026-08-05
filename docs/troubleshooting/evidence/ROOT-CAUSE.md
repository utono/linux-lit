# Root cause: BH-Barrett prose pages overflow the card (post-pv7)

Controller-verified. Supersedes CANDIDATES.md (my font-size candidate was WRONG).

## The evidence

Three vectors for the same lines, same run:

```
GEN_HEIGHTS      [4220..=4240] = [68,96,68,124,40,40,293,349,153,68,40,124,40,...]
POSTWALK_HEIGHTS [4220..=4240] = [68,96,68,124,40,40,293,349,153,68,40,124,40,...]   IDENTICAL
RENDER_HEIGHTS   page_top=4258 heights=[96,96,40,40,96,40,96,40,68,209,293,406]
```

Per-line deltas (lines 4258-4269): 0,+28,0,0,0,0,+28,0,+28,+28,+56,+57 = **+225px**.
Every delta is an exact multiple of the ~28px row pitch. Zero fractional deltas.

`wrap_w=899` at BOTH phases. Geometry, margins, font identical.

## What this rules OUT (each by measurement, not argument)

- **Tags / fonts.** 5 of 6 divergent lines carry byte-identical tags at GEN and
  RENDER; tags were already applied BEFORE generation. My `font-size` reapply
  candidate is dead. Vocab tag is foreground-only (cannot change wrap). Reader
  gloss tints likewise.
- **Geometry / wrap width.** `wrap_w=899` both phases.
- **The boundary walk.** `GEN_HEIGHTS == POSTWALK_HEIGHTS` byte-identical, so
  heights do not move during the walk.
- **Arithmetic.** `page_px` and `exact_page_content_height` are algebraically
  identical at `end_off == 0`. The heights differ, never the maths.

## The cause

The buffer-wide `line_yrange` sweep in `record_prose_pages`
(`src/input/prose_pages.rs:435`) reads GTK's **lazily-validated** line layout.
GTK gates full validation on proximity to the viewport. At generation the
viewport sat at `page_top=4223`; lines 4258-4269 were 35-46 lines below and had
NEVER been displayed, so `line_yrange` returned a PROVISIONAL estimate. The
estimate is systematically SHORT — always by whole rows — because unvalidated
lines under-report wrapped row count. When those lines are finally drawn, the
real layout is one or two rows taller.

**The convergence guard cannot see this.** The run logged
`changed_between_sweeps=0` and `delta_sum=0` — two back-to-back sweeps agreed
exactly — while the render measured +225px on those very lines. Both sweeps read
the SAME stale cache, so they agree with each other and with nothing real. The
guard validates self-consistency, not correctness.

Corroboration: `LIT_TRACE_PANGO=1` (the codebase's own ground-truth probe) shows
`line_yrange` disagreeing with true Pango metrics on **6047 of 7300 lines**, in
BOTH directions.

## The smoking gun: generation is NOT deterministic

Same fingerprint, same content, same geometry, three different page counts:

```
808 pages  (stored this morning, validated=1 — the table the user was reading)
806 pages  (traced regeneration)
801 pages  (currently stored at 1920x1200 pv7)
```

A pure function of content+layout cannot return 801, 806, and 808. Page
generation depends on how much of the buffer GTK happened to have validated at
that moment — i.e. on scroll history and timing. This is a RACE, and the
fingerprint cannot encode it, so the pv-bump strategy cannot fix it: every
regeneration is a fresh roll of the dice.

## Why the earlier fix looked complete

The font-tag fix (pv5->pv7) was REAL and did help — it removed one genuine
source of short heights. But it addressed a different mechanism, and its
acceptance test lands on `LIT_START_SCENE=26.0`. Chapter 26 passes. Chapter 37
was never re-verified, and 22 pages there overflow by 13-114px.

## Implication for any fix

A fix must make generation read TRUE metrics rather than GTK's viewport-gated
cache — e.g. measure via Pango directly (the `LIT_TRACE_PANGO` path already
computes ground truth), or force validation of every line before sweeping.
Adding another sweep will NOT work: two sweeps already agree with each other
while both are wrong.
