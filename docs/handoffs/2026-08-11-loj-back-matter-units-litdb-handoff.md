# Handoff → litdb: LoJ back matter is numbered as reading units

**Date:** 2026-08-11 (US Central)
**Raised from:** linux-lit (reader-side investigation; no reader code changed)
**Target repo:** `~/utono/litdb`
**Work:** `LoJ` — Boswell, *Life of Johnson*, `work_type='prose_book'`
**Routing rule:** linux-lit `CLAUDE.md` → "Upstream root-cause routing". The
reader must NOT filter around this; `(div1, div2)` is authoritative metadata,
so the fix lands in litdb.

## The reported symptom

Pressing `{` (`JumpToNextDivision`) in the reader jumped from body text in
Chapter 3 into a block of footnotes (`[33]`–`[36]`), with no cursor-segment
tint on arrival.

## What is NOT broken (verified — do not re-investigate)

`[` / `{` behave correctly. Verified headlessly on LoJ via
`scripts/land-on.sh LoJ 1.6` plus a `wtype -k braceleft` drive:

```
[341356ms] ACTION: JumpToNextDivision
[341357ms] CURSOR_LINE: applied tag to line 371
[341358ms] SEEK: line=371 work_idx=371 start=396.75 base=396.75 seek=396.55
[341423ms] PHRASE_HL: cache fill line_id=1790850 media=233 spans=19
```

Buffer line 371 is exactly `MIN(line_in_div)` for LoJ `div1=1, div2=8`, and a
screenshot confirmed the segment tint landed on that division's first segment.
The stepper finds the boundary, lands the cursor on the first segment, and
paints the tint. No reader defect.

## Root cause

LoJ's **scholarly back matter is numbered as ordinary reading units**. The
import flattened the source file's explicit `APPENDIX X` and `FOOTNOTES:`
markers into the same `div2` sequence as the biography's body text.

`work_has_units()` in the reader gates `[`/`{` unit stepping on "prose work
with any non-zero `div2`" — LoJ qualifies with 482 units — so `{` faithfully
steps into appendices, footnote runs, and index blocks. Two visible effects:

1. The landing looks wrong (a footnote or a table is not a reading unit).
2. The tint usually does not appear, because most back-matter lines are
   untimestamped — only 6,847 of 21,520 lines have timestamps — which sends
   `seek_to_current_line` down its `NO_TIMESTAMP` branch: no seek, no paint.

Effect 2 is a *consequence* of effect 1, not a separate bug. The narration
genuinely does not read the back matter, which the existing
`wizard-loj-block-reimport` skill already documents:

> the audio narrates the main text of the biography … it does NOT read the
> scholarly back-matter — appendices, "Various Readings", the biographical
> index, or the "Wit and Wisdom" quotation index.

So the data is right about *what is narrated* and wrong about *what counts as
a unit*.

## Measured scope

Total LoJ divisions: **482**. Of those, **202** contain at least one
bracket-prefixed line and **156** are majority bracket-prefixed lines.

Per volume, units at or after the first `APPENDIX` / `FOOTNOTES:` marker:

- vol 1 — 117 units total; 48 at/after appendix; 45 at/after footnotes
- vol 2 — 93 total; 38 / 37
- vol 3 — 83 total; 33 / 30
- vol 4 — 92 total; 38 / 37
- vol 5 — 69 total; 29 / 26
- vol 6 — 28 total; 0 / 0 (the index; no appendix or footnote marker)

Roughly **30–48 units per volume in vols 1–5** are back matter. Volume 6 is
the biographical/quotation index end to end.

The user's reported jump: cursor started in `div1=3, div2=51` (line_in_div
10943, "'What I like least in your letter…"); `{` advanced toward `3.54`,
which is the `[33]`–`[36]` footnote block.

## The markers are still present and unambiguous

The source file retains the structural markers the import flattened:

```bash
rg -n "^FOOTNOTES:|^APPENDIX " ~/utono/litdb/data/imports/LoJ-import/loj-all-blocks.txt
```

Their current division placement (this is the authoritative work-list):

```
div1 div2  line_in_div  marker
1    70    2499         APPENDIX A
1    71    2587         APPENDIX B.
1    72    2690         APPENDIX D.
1    72    2704         APPENDIX E.
1    72    2711         APPENDIX F.
1    73    2718         FOOTNOTES:
2    56    7226         APPENDIX A.
2    56    7268         APPENDIX B. (_Page_ 312.)
2    57    7270         FOOTNOTES:
3    51    10992        APPENDIX A.
3    52    11006        APPENDIX B.
3    53    11152        APPENDIX C.
3    53    11158        APPENDIX D.
3    54    11164        APPENDIX E.
3    54    11215        FOOTNOTES:
4    55    14662        APPENDIX A.
4    55    14666        APPENDIX B.
4    55    14679        APPENDIX C.
4    55    14699        APPENDIX D.
4    55    14723        APPENDIX E.
4    55    14731        APPENDIX F.
4    56    14752        APPENDIX G.
4    56    14768        APPENDIX H.
4    56    14774        APPENDIX I.
4    56    14786        FOOTNOTES:
5    41    17390        APPENDIX A.
5    42    17396        APPENDIX B.
5    42    17400        APPENDIX C.
5    44    17597        FOOTNOTES:
```

Note the flattening is visible here too: several distinct appendices share one
`div2` (e.g. `4.55` holds APPENDIX A–F), and a single `div2` straddles the
body/back-matter boundary (`3.54` contains both APPENDIX E and FOOTNOTES:).
Any fix must handle a marker appearing **mid-division**, not only at a
division start.

`block_type` cannot be used as a shortcut — it does not distinguish back
matter today:

```
block_type  n      bracket-prefixed
heading     1385   3
prose       18068  6835
verse       2067   0
```

## What to decide (this is the actual design question)

The desired end state is: `[` / `{` step through the **biography's reading
units only**, and back matter is reachable but not part of the unit sequence.
Pick the representation:

1. **Re-number back matter out of the `div2` unit sequence** — e.g. move it to
   a distinct `div1`, or a reserved `div2` band — so `work_has_units()` stepping
   never enters it.
2. **Type it and let the reader skip by type** — introduce a back-matter
   `block_type` (or a column) and have the stepper skip those. Note this needs
   a matching reader change, so it is *not* purely upstream; if you choose it,
   say so explicitly in the reply so linux-lit can spec its half.
3. **Split back matter into its own work(s)** — heaviest; changes abbrevs and
   therefore touches ~15 tables plus snapshots and config.

My recommendation is (1): it keeps the fix inside litdb, needs no reader
change, and matches the authoritative-metadata principle. But the choice is
the litdb owner's — please confirm before implementing.

## Constraints and hazards

- **Never rename `LoJ`'s abbrev with raw SQL.** `works.abbrev` is the de-facto
  FK for ~15 tables plus the snapshot cache and config. Option 3 in particular
  must go through litdb's `rename-work-abbrev` skill, with linux-lit closed.
- **Do not hand-copy `lit.db`.** The systemd timer
  `lit-db-backup-local.timer` (every 2h) snapshots it; each ad-hoc copy is
  ~1.5 GB. Verify timer health instead:
  `systemctl --user list-timers lit-db-backup-local.timer --all --no-pager`.
- **Close linux-lit before writing**, and avoid concurrent lit.db writers.
- **Preserve the timestamps.** 6,847 LoJ lines carry `line_timestamps` against
  `media_id=233`. Re-numbering divisions must not orphan them — they key off
  `line_mapping.id`, so keep row identity stable if at all possible.
- **`wizard-loj-block-reimport` is resumable and the DB is its state.** If you
  reach for a reimport, run its status check FIRST. Expected done-state:
  ~21,520 rows / 6 volumes; per-volume 5067/4117/4010/3111/2667/2548;
  volume-start `line_in_div` 1, 5068, 9185, 13195, 16306, 18973. Prefer a
  targeted re-numbering over a full reimport if it achieves the same end state.

## Downstream coupling (low — measured today)

- `bookmarks` — 4 rows
- `passages` — 16 rows
- `prose_pages` — 2,856 rows (regenerable by the reader)
- `journal_entries`, `division_synopses` — 0 rows
- `glosses` — no work column; not keyed to LoJ directly

Only `bookmarks` (4) and `passages` (16) are hand-made and worth preserving
deliberately. `prose_pages` is a cache: after any division change, it is
expected to be stale.

## Acceptance criteria

1. Stepping `{` repeatedly from the start of any volume 1–5 reaches the end of
   that volume's **body text** and never lands the cursor on an `APPENDIX`
   block, a `FOOTNOTES:` run, or an index entry.
2. The reader's `work_has_units('prose_book', lines)` still returns true for
   LoJ (unit stepping stays enabled) — unless option 2/3 is chosen, in which
   case state the intended new behavior explicitly.
3. The 6,847 existing `line_timestamps` rows for `media_id=233` still resolve
   to the same text after the change.
4. Back matter remains *readable* by ordinary scrolling / page turns — this is
   about the unit sequence, not about hiding content.

## Verification to run back in linux-lit (after the data change)

```bash
cd ~/utono/linux-lit && cargo build
./scripts/land-on.sh LoJ 3.50
# then drive: wtype -k braceleft   (repeat), and confirm in /tmp/land-on.log:
#   ACTION: JumpToNextDivision → CURSOR_LINE → SEEK (with a start=, not NO_TIMESTAMP)
```

Confirm the landing lines are body text, not `[NN]` footnotes. Note that LoJ
builds a prose page table on first load (~100 s headless); queued keystrokes
land only after `PAGES_PROSE_*` settles, so wait for it before believing a
drive did nothing.

## Reference

- Reader-side entry points (context only, not to be changed):
  `src/input/navigation.rs` — `jump_to_next_division`, `jump_to_prev_division`,
  `work_has_units`, `seek_to_current_line`.
- litdb skill: `.claude/skills/wizard-loj-block-reimport/SKILL.md`
