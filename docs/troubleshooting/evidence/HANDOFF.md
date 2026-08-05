# Hand-off: prose page height truth (branch `fix/prose-height-truth`)

What to verify on the real GL renderer. Cage (the headless test compositor)
is software rendering and has disagreed with the real renderer before, so
none of the automated runs below substitute for a look on-screen.

## 1. The exact page the user screenshotted

Open BH-Barrett, go to **chapter 37**, and confirm the bottom line sits
clear of the card rule — no glyph sliced at the bottom edge.

Before this fix, a fresh regeneration at production geometry produced 56
`CLIP_WARN … OVERFLOW` lines across 14 pages in this region. After the fix,
a fresh regeneration produced zero. That gap was confirmed in cage; it still
needs a look on the real renderer.

## 2. A few other late chapters, not only 37

The defect was pervasive across the deep-page region, not unique to chapter
37 — spot-check two or three more late chapters in BH-Barrett (or another
long prose work) rather than treating chapter 37 as the only affected page.

## 3. Expect a one-time regeneration pause on first open

The fingerprint bump (`pv7` -> `pv8`) evicts every prose table generated
under the old, racy measurement. The first time you open a given prose work
after updating, expect a brief pause while it regenerates — generation is
about **1.6x** slower than before (roughly 972ms -> 1593ms measured on
BH-Barrett). This is cached per work+fingerprint, so it happens once per
work+geometry, not on every load. If you resize the window or change fonts,
expect it again for that new geometry.

## 4. Honest caveat: a separate, smaller issue remains

The whole-table census still measures `over_usable=48` across 817 pages
(worst case 7px over) at production geometry, after this fix. All 48 fall
within the existing 14px fit-slack budget, so nothing in item 4 above is
masking a regression — but it does mean
`prose_pages_keep_bottom_breathing_room` (the test asserting `over_usable ==
0`) still fails on this branch, exactly as it failed on master before this
work started. That test failure is a pre-existing, separate fit-slack/
rounding issue this branch did not set out to fix and did not fix. It is
not the whole-row (multiple-of-28px) overflow this branch addresses.

## Also verified, no action needed

- Verse/play pagination (Cymbeline fuzz): 268 steps, 0 failures — untouched
  by this change.
- Determinism: the same fingerprint previously produced 801, 806, and 808
  pages across separate runs (a race, not a bug in the content or
  geometry); the determinism test went red (`[781, 782, 781]`) before the
  fix and green after, reconfirmed twice.

See `docs/troubleshooting/evidence/ROOT-CAUSE.md` for the full measurement
trail and `docs/troubleshooting/clip-prevention.md` item 23 for the
ledger entry (including how to distinguish this bug from item 22, an
unrelated but symptomatically similar overflow fixed earlier).
