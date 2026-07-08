# Shakespeare Timestamp Fixes — Playback-Sync Worklist

Actionable data corrections in `~/utono/litdb/data/lit.db`, surfaced by the
`test-playback-sync` sweeps (2026-07-05: all 38 Arkangel editions, then the
remaining ~30 non-Arkangel editions) plus a full-database timestamp
monotonicity scan. Check items off as they are completed. Manual timestamps
are set in-app with the `u` bind (see the `debug-timestamp-bind` skill);
large gaps go through wizard-ambrose in litdb.

## 1. Corrupt timestamps — fix these values (wrong-line-jump class)

The only monotonicity violation in the whole Arkangel set, in **Ant-Arkangel
1.2** — two adjacent lines have tangled times:

- `line_mapping` **1334130** (1.2.1 "Lord Alexas, sweet Alexas, most
  anything…") — stored `348.66 – 0.00` (broken end)
- `line_mapping` **1334131** (1.2.2 "Alexas, almost most absolute Alexas…") —
  stored `344.24 – 357.10` (starts *before* the line above it)

While audio plays 344–348s the highlight jumps to 1.2.2 early, and 1.2.1's
window is unusable. The starts look swapped; the correction that restores
monotonicity:

```sql
UPDATE line_timestamps SET start_time=344.24, end_time=348.66 WHERE line_mapping_id=1334130;
UPDATE line_timestamps SET start_time=348.66 WHERE line_mapping_id=1334131;
```

- [x] Ant-Arkangel 1.2.1 / 1.2.2 (ids 1334130, 1334131) corrected and
      verified by listening across the 344–357s window
      — DONE: now monotonic (1.2.1 @347.96, 1.2.2 @353.16; 1.2.2 starts after 1.2.1)

## 2. Missing timestamps at the failing boundaries (suppression class)

These untimestamped dialogue lines are exactly where the Arkangel sweep's six
works stalled. The shipped suppression-clear fix makes sync *survive* them;
giving them timestamps makes page turns precise there.

- [x] **Tit-Arkangel** — essentially the whole opening: **1.1.1–1.1.47**
      (Saturninus / Bassianus / Marcus speeches), plus 1.1.64, 1.1.239.
      Gap is big enough that a wizard-ambrose re-alignment pass in litdb
      beats manual entry.
      — DONE 2026-07-07: targeted `--interval 0-700 --keep-manual` opening
      re-align recovered 1.1.1–1.1.47 + 1.1.64 (coverage 97.5%→99.4%, gate 0,
      nav 426/0). Only the lone singleton 1.1.239 stays untimestamped
      (u-fixable; not worth a re-align).
- [ ] **TN-Arkangel** — 1.1.1–1.1.5 (Orsino's "If music be the food of
      love…" opening) + 1.1.43, 1.5.208
- [ ] **Err-Arkangel** — 1.1.18–1.1.27 (ten lines mid-Duke speech) + 1.1.158
- [ ] **Shr-Arkangel** — Induction: −1.1.5, −1.1.16, −1.1.18, −1.2.18,
      −1.2.147; plus 1.1.164, 1.1.258, 1.1.264
- [ ] **TNK-Arkangel** — Prologue 0.0.1 and 0.0.33; the 1.1 song lines
      (1.1.4, 1.1.17, 1.1.20, 1.1.24); 1.1.229, 1.2.135, 1.4.1, 1.4.56
- [ ] **WT-Arkangel** — 1.1.18, 1.1.48, 1.2.27, 1.2.124, 1.2.173, 1.2.555 —
      all short wrap-tail fragments ("freely.", "world,"), the known
      split-line import class: check the folger-cleaned .txt source before
      hand-timing

## 3. Bulk outlier — not manual work

- [x] **Cor-Arkangel: 1,452 untimestamped content lines** — an order of
      magnitude beyond every other edition (next worst: Cym-Arkangel at 72).
      Incomplete alignment; re-run the wizard-ambrose alignment in litdb for
      Cor rather than hand-correcting.
      — DONE 2026-07-06: full wizard-ambrose aberrant re-align, now 89.9%
      coverage (393 untimestamped, down from 1,452), gate 0, nav 424/0.

## 4. Non-Arkangel editions — monotonicity violations (wrong-line-jump class)

The full-DB scan found ~200 backwards timestamps outside the Arkangel set
(which had only the single Ant case above). Each one makes the highlight
jump to the wrong line while audio plays inside the overlapping window.
Counts per edition (`start_time` going back by >0.5s in citation order).

> **⚠️ Bundle counts below are INFLATED — re-scan with the corrected query.**
> The original counts came from the `ORDER BY line_mapping_id` scan, which
> straddles work boundaries on any m4b that bundles multiple works, reporting
> a false backwards "jump" at each boundary (same artifact as the PL
> false-positive in §4 tail). Re-scanned 2026-07-07 with the corrected
> book-order query (scoped per `work_abbrev`) — real >0.5s counts are far
> lower, and most residuals are sub-5s jitter (the leave-alone threshold):
>
> - Rom-BBCClassic 20→**4** (worst 7.1s) · LLL-Argo 16→**4** (all <5s) ·
>   Cor-Argo 14→**4** (all <5s) · Shr-BBCClassic 11→**4** (worst 11.3s) ·
>   MND-Argo 11→**1** (1.7s jitter) · 2H4-Argo 9→**2** (<5s) ·
>   3H6-Argo 8→**1** (2.8s jitter) · MND-BBCClassic **3** (10s) ·
>   TN-BBCClassic **4** (7.2s) · Mac-BBCClassic **1** (jitter) ·
>   Shr-Argo **2** (<5s) · Ham-BBCClassic **15** (worst 28.6s — but the 28.6s
>   is the deliberately-retained 2.2.57–60 Polonius block, see its
>   wizard-ambrose bullet; not a defect).
>
> Net: only Ham/Shr/Rom-BBCClassic/TN/MND-BBCClassic have any >5s stray worth
> a gate pass; the rest are jitter. Single-media editions below (Rom-BBC 18,
> Per 16, Tmp 13, etc.) were NOT affected by the artifact — their counts stand.

- [x] **1H4-Amb** — 31 (worst; video-rip alignment)
      — DONE 2026-07-07: gate cleanup (3 stage-row + 28 strays deleted, incl.
      2 near-duplicate-phrase mispins), gate 0 backwards>5s, parity 2404/2404,
      nav 426/0. No re-align needed.
- [ ] **Rom-BBCClassic** — 20 (bundle m4b — rows include other plays in the
      bundle; verify against the bundle audio, not Rom alone)
- [x] **Rom-BBC** — 18
      — DONE 2026-07-07: deleted 7 duplicate-timestamp mispins; kept the documented Chorus-after-brawl reorder + scene-boundary overlaps. 0 gross isolated strays remain (residual >5s flags are legit scene-overlaps). ts=1983.
- [x] **TN** — 17 (Feste's-song refrain mispins)
      — DONE 2026-07-07: deleted 5 "wind and the rain"/"What is your parentage?" refrain-dup mispins + 1 grossly-misplaced manual mark (1.5.167 @1034s, 260s out of place). 0 gross isolated strays remain. ts=1587.
- [x] **Per** — 16
      — DONE 2026-07-07: deleted 3 repeated-phrase duplicate-timestamp mispins. 0 gross isolated strays remain. ts=1595.
- [ ] **LLL-Argo** — 16 (bundle m4b)
- [ ] **Cor-Argo** — 14 (bundle m4b)
- [x] **Tmp** — 13
      — DONE 2026-07-07: 0 real defects — all duplicate-timestamp pairs were legit quick-succession short lines (correctly bracketed); residual >5s flags are scene-boundary overlaps. Nothing safe to delete. ts=2050.
- [x] **2H6-Amb** — 13 (video rip)
      — DONE 2026-07-07: gate cleanup (3 stage-row + 10 strays deleted, incl.
      York-genealogy + Cardinal-reading duplicate-phrase mispins), gate 0
      backwards>5s, parity 2764/2764, nav 424/0. No re-align needed.
- [ ] **Shr** — 12, **Shr-BBCClassic** — 11 (bundle)
- [ ] **MND-Argo** — 11, **MND-KPR** — 9, **2H4-Argo** — 9, **3H6-Argo** — 8
- [ ] **Rom-BBCTrystanGravelle** — 10, **Rom-Naxos** — 7
- [ ] **Cym-BBC** — 9 (the edition with the historical corrupt-timestamp
      sync-jump; see wizard-ambrose Step 6.6 monotonicity gate)
- [ ] **Ado** — 8, **R2** — 7, **WT** — 6, **Mac** — 6, **H5** — 6, **JC** — 1

Non-Shakespeare rows the same scan caught (fix in their own passes):
**PL** (Paradise Lost, 2 catastrophic: `39818 -> 37` and `58107 -> 7795` on
the same line id), **BenCrystalOP** — 12 (incl. willow-song refrains),
**ChurchillWC1** — 13, **Ven** — 3, **TC / Ref / DamClub6** — 1 each.

- [x] **PL — the 2 "catastrophic" rows are a FALSE POSITIVE, no fix needed.**
      Investigated 2026-07-07. Both are on line 312778 (PL 1.26 "Of Man's first
      disobedience…") on the COMBINED "Paradise Lost & Paradise Regained"
      recordings — media 183 (bbc-radio) and 212 (griffin-charlton). The scan
      query's `ORDER BY line_mapping_id` straddles the PL↔PR work boundary: PR's
      line ids (…250635 = PR 4.639, the last line) are LOWER than PL's (312778)
      but play at the END of the ~11–16h audio (58107s / 39818s), so LAG wrongly
      pairs "end of PR" as the predecessor of "start of PL" and reports a huge
      drop. In true book order **PL and PR are each 0-backwards on both media**
      (and on all 5 PL media). Lesson: the section-4 scan over-reports on any
      media that bundles two works — verify a flagged row in book order
      (`ORDER BY div1,div2,line_in_div,sub_line` scoped to ONE work_abbrev)
      before treating it as corrupt. Also noted: media 68 (quayle, **priority 1,
      NOT the reader's choice** — PL plays media 225 lesser-anton @10, which is
      clean) has a genuinely degenerate PL alignment (2103/3558 lines in <0.3s
      collapsed spans, incl. the benign 0.6s 2.886→2.887 dip); that needs a full
      re-align, not a row edit, and is low-priority since it's an unused media.
- [x] **PL/PR subsequently split into per-reading variant works (2026-07-07).**
      Each Milton reading is now its own work (like the Shakespeare -Arkangel/-Argo
      variants): `PL-Lesser` (media 225, the reader's pick), `PL-Quayle` (68),
      `PL-Penguin` (211), `PL-BBC` (183), `PL-Griffin` (212), plus `PR-BBC` (183)
      and `PR-Griffin` (212). Built with `scripts/split_reading.py` (base-row-copy
      + tuple-relink, bundle-aware for the shared 183/212 media). All 65 manual
      marks preserved, Step 6.7 gate 0-backwards on all 7, base PL/PR now own no
      media. **So future timestamp scans should target the per-reading abbrev
      (`PL-Lesser`, `PR-BBC`, …), not base `PL`/`PR`.**
- [x] **PL-Quayle degenerate alignment RE-ALIGNED (2026-07-07).** Root cause: the
      quayle m4b is only 92 min — a SELECTIONS reading of **Book 1 + Book 4 only**
      (Satan's speeches), not the full epic; the old aligner had force-mapped all
      10,780 lines onto it (2103/3558 = 59% <0.3s collapsed spans). Re-transcribed
      (local medium.en) + re-aligned aberrant (~35 min — PL is 3× a play), then
      deleted the un-recorded books' scatter wholesale by `div1` (Books 2,3,5–12)
      and the ~6 residual within-Book-1/4 strays. Final **1559 ts** (Book 1: 779,
      Book 4: 780), **0 collapsed spans**, Step 6.7 gate **0 backwards>0.5s**, 9
      manual marks preserved, spoken-status synced (1531 spoken; the un-recorded
      lines correctly not-spoken). See the wizard-ambrose "Re-aligning a degenerate
      SELECTIONS recording" lesson.

List any edition's offending rows. **Order by BOOK POSITION, scoped to ONE
`work_abbrev` — NOT by `line_mapping_id`.** The original scan ordered by
`line_mapping_id`, which straddles work boundaries on media that bundle two
works (PL+PR, the ClassicBBC/Argo multi-play m4bs) and reports false backwards
"jumps" at the boundary (see the PL false-positive above). This form is correct:

```sql
WITH t AS (
  SELECT lm.id lid, lt.start_time s, lt.media_id,
         LAG(lt.start_time) OVER (
           PARTITION BY lt.media_id
           ORDER BY lm.div1, lm.div2, lm.line_in_div, lm.sub_line) prev
  FROM line_mapping lm JOIN line_timestamps lt ON lt.line_mapping_id = lm.id
  WHERE lm.work_abbrev = '<ABBREV>')
SELECT t.media_id, t.lid, printf('%.2f -> %.2f', t.prev, t.s),
       substr(lm.canonical_text, 1, 50)
FROM t JOIN line_mapping lm ON lm.id = t.lid
WHERE t.s < t.prev - 0.5
ORDER BY t.media_id, (t.prev - t.s) DESC;
```

(To pick the media the reader actually plays, filter to the highest-priority
`work_media_associations.media_id` for the work — a defect on a priority-1
media the reader never selects is low-priority.)

## 5. Non-Arkangel sweep results (2026-07-05)

~30 editions, 2 boundaries each, plus a corrected rerun of H5/Ham/Mac.
Outcome: **no app bugs** — every genuine check passed (typical delta
+0.1–0.25s; several −1.3s by-design gap-preroll turns). Data findings:

- [x] **Ham (Naxos, media 82) is effectively unaligned — 5 timestamps in the
      whole play** (its other 6 media rows have 0). Needs a full
      wizard-ambrose alignment pass before sync is usable on it.
      — DONE 2026-07-07: split off base Ham as Ham-Naxos, full aberrant align,
      now 3,762 timestamps (85.2% coverage), gate 0, nav 423/0. The 5 manual
      marks were preserved across the re-import.
- [ ] **Rom-BBCTrystanGravelle** — boundary 1 turned 32s early
      (`turn 247.06s vs expected 279.50s`): one of its 10 backwards
      timestamps (section 4) sits inside that window and pulls sync forward.
      Fixing the section-4 rows fixes this.
- [ ] **2H4-Argo** — boundary 1 delta −2.04s, just past the gap-preroll
      allowance: the stored `start_time` (40722.449) looks ~0.5s late
      relative to the spoken line in the bundle audio. Minor; verify while
      fixing its section-4 rows.
- 14 WARNs (Ado, JC, MND-KPR, MV, Oth, R3, Shr, TN, Ham, Mac…) are
  untimestamped landing lines with dead-air audio — the sparse-coverage
  editions; they resolve as coverage improves (sections 2/4), not app fixes.

Test-harness notes baked into these results: the test picks the media row
with the MOST timestamps (H5's media 2 has two rows vs 2,934 on media 63),
and a timeout only counts as the suppression bug when the log shows no
"cleared indefinite suppression" line.

## Verifying a fix

After correcting a work, re-run its timed boundary check:

```bash
.claude/skills/test-playback-sync/run-sync-test.sh --boundaries 2 <ABBR>
```
