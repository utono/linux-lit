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

## Real-renderer verification — CONFIRMED (2026-08-05)

The user ran the merged build (`crll`, master `1f5125e0`) and screenshotted
BH-Barrett chapter 37 — the exact page that showed the defect.

Pixel-measured from the two captures, at the card's cream/text boundary:

- capture 16-43-03: card bottom y=1185, last ink y=1102 — **83px clear**
- capture 16-43-08: card bottom y=1185, last ink y=1114 — **71px clear**

Compare the pre-fix report: the final row rendered **8px of a ~28px line box**,
sliced through the glyphs, with no clip band.

Both pages end on complete lines ("…which she always did in the" /
"…for I was going to ask him by whom he had"), so the text is intact rather
than masked.

This is the acceptance the headless suite could not give: cage is software
rendering and has disagreed with the real GL renderer on layout before.
