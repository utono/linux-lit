# Timestamp Diagnosis

Frequency-ordered ledger for "the audio starts in the wrong place" and its
relatives. Read this BEFORE diagnosing a timestamp complaint.

The domain rule that generalizes across every entry below: **the two
timestamp tables are separate sources of truth, and nothing keeps them in
sync.** Diagnosing one from the other — or from the artefact that produced
one of them — is how sessions reach confident wrong answers.

## The two tables (read this first)

| table | granularity | who reads it |
|---------------------|--------------------|-------------------|
| `line_timestamps` | one row per line | seek, sync, `b` |
| `phrase_timestamps` | many rows per line | karaoke sweep |

`phrase_timestamps` columns are `id, line_mapping_id, media_id, start_time,
end_time, start_char, end_char` — **no `source`, no `updated_at`.** Any plan
that wants to know "were these phrases rebuilt after that line was
corrected" needs a schema migration first. litdb hit this and abandoned the
approach (2026-08-15).

`phrase_timestamps` has **no triggers**. `line_timestamps` has three
(`_mirror_ad/_ai/_au`), but they only mirror a row between the two
`media_id`s of a priority 20 <-> 10 twin.

---

### FAILURE MODE 1 — diagnosing from the cached whisperX JSON (2026-08-15)

**The single most expensive trap in this domain. It produces a confident,
fully-evidenced, exactly-backwards answer.**

**Tell:** you are checking whether a line's start time is right, the cached
whisperX transcript is to hand, and its first segment agrees with the
*phrase* rows rather than the line.

**What happened.** LoJ (`media_id = 233`, line 1790843) had
`line_timestamps.start_time = 16.264` and `MIN(phrase_timestamps.start_time)
= 0.594`. A litdb session read the cached transcript
`TheLifeofSamuelJohnson_ep6.whisperX-transcript-large-v3.en.json`, whose
first segment is:

```
0.594 -> 16.538   "To write the life of him who excelled all mankind..."
```

No mention of Audible in the first 60 segments; word timings ran
continuously from `To` (0.594) to `age` (16.458). On that evidence the
session concluded there was no intro, the phrases were correct, and the
manual line start was the defect — **the exact opposite of the truth.**

Re-transcribing the first 40 s of the **audio** settles it:

```
0.537 -> 13.143   "This is Audible."
16.265 -> ...     "To write the life of him who excelled all mankind..."
```

The narration starts at 16.265, matching the manual `16.264` to within a
millisecond.

**Root cause of the trap:** the cached JSON *is the artefact the bad
alignment was produced from*. Its transcript of the intro region was
mis-assigned to the book's opening words — which is precisely how the phrase
rows ended up there. Diagnosing a bad alignment from the transcript that
caused it is circular.

**Fix / procedure:** always re-transcribe the audio.

```bash
ffmpeg -v error -i <media> -t 40 -ac 1 -ar 16000 head.wav
~/utono/whisper-transcript/.venv/bin/python -m whisperx head.wav \
    --model medium.en --output_format json --device cpu
```

What caught it in practice was the user saying "there is an audible intro to
LoJ but maybe just not in the json." Absent that, the wrong conclusion would
have shipped as a line-timestamp "repair."

---

### FAILURE MODE 2 — reading a coverage GAP as a time shift (2026-08-15)

**Tell:** karaoke goes dark for a stretch of a line and resumes
**mid-sentence**, and a time-based check reports a large disagreement.

A time comparison (`MIN(phrase_timestamps.start_time)` vs the line start)
cannot tell these apart:

- **A shift** — coverage is complete, but the times are wrong.
- **A gap** — the opening characters have NO phrase row at all, so
  `MIN(start_time)` is the time of the first *surviving* row. The rows that
  exist are often perfectly correct.

They need different repairs, so naming the wrong one sends the next session
at rows that are fine.

**The case.** TT/56 line 1760897 reported "13 phrase rows disagree with this
line by 12.8s". All 13 rows were monotonic in char and time, non-overlapping,
and closed the line exactly at `end_char 965`. The defect was that coverage
began at `start_char = 192`: chars 0–191 (`Though the author has written a
large Dedication ... as far as I can observe,`) had no row at all. On screen,
that span was dark and the highlight resumed at char 192 — `not at all
regarded...` — exactly as the data predicted.

**Diagnose by character coverage, not by time:**

```sql
SELECT MIN(start_char) AS first_char, MAX(end_char) AS last_char, COUNT(*)
FROM phrase_timestamps
WHERE line_mapping_id = ? AND media_id = ?;
```

`first_char > 0` is a leading gap. Compare `last_char` against
`length(canonical_text)` for a trailing shortfall, and use `LEAD(start_char)
- end_char` for interior holes.

**Handled since `d339ad9f`:** the toast checks `MIN(start_char)` first and
reports "phrase coverage starts N chars into this line", logged as `TS: phrase
gap` (distinct from `TS: phrase drift`). Interior gaps are deliberately not
detected — that is a gate's job, not a toast's.

**Repair is upstream and NOT a backfill on TT/56** — that media carries
shipped smear repairs a rebuild would discard. See
`docs/handoff-2026-08-15-tt56-phrase-coverage-gaps.md`.

---

### FAILURE MODE 3 — assuming a shared signature means a shared defect (2026-08-15)

**Tell:** several media match one query, and a fix is planned for "the N
rows" as a batch.

Five media had the same first-row signature — phrases reaching back to ~0.6 s
while the line starts many seconds later. Inspecting each individually showed
**four different defects**:

- **233 LoJ** — genuine intro contamination; 5 of 8 phrases span
  0.594->16.4 s, the rest sit at 30-35 s.
- **325 DC** — one stray phrase on a heading row; rows 2+ agree to 0.00 s.
  Repaired by DELETE (there was no correct phrasing to recover).
- **4 MND-Arkangel** — phrases 2-3 *also* wrong (23.4-26.6 s against a line
  at 83.45 s). Needed a full rebuild, not a delete.
- **33 Tmp-Arkangel** — media-wide disorder; row 2's phrases at 135-142 s
  against a line at 63 s. NOT repaired; needs its own spec.
- **56 TT** — collapsed duplicate spans, entangled with the orphan class.
  NOT repaired; a rebuild would silently discard shipped smear repairs.

**Consequence:** a row-scoped batch fix would have left 33 and 56 broken —
moving a reader from "wrong at second zero" to "wrong somewhere less
obvious", which is **worse**, because it stops looking like a bug.

---

### FAILURE MODE 4 — a gate anchored on the wrong table (2026-08-15)

**Tell:** a detection query inner-JOINs from `line_timestamps` and reports a
clean corpus.

Any check anchored on `line_timestamps` is structurally blind to phrase rows
on lines that have **no line timestamp at all**. Corpus-wide that is
**10,169 orphan rows across 73 media**. Three further media (22 MM-Arkangel,
17 JC-Arkangel, 320 BH-Barrett) carried the intro signature and were missed
by the original query.

BH-Barrett is the instructive one: it **passes** the first-row check because
its first *timestamped* row is clean, yet still carries 19 intro-region
phrases. A reader can land in the announcement on a media the gate calls
healthy.

litdb's gate 6.7f now reports these, advisory by default — 10,169 rows would
turn every wizard run red before the class is specified.

---

### FAILURE MODE 5 — treating all phrase/line divergence as broken (2026-08-15)

**Tell:** a query returns thousands of rows and the instinct is to fix them.

Corpus-wide, **17,880 rows across 73 media** have phrases starting >2 s
before their line. **This population is mostly legitimate.** It is dominated
by dramatic recordings — Arkangel and Naxos Shakespeare — where overlapping
dialogue, music and sound effects make phrase/line divergence normal:

| media file | rows | max gap |
|---------------------|------|---------|
| HenryVArkangel | 630 | 19.4 |
| CoriolanusArkangel | 461 | 26.8 |
| HamletArkangel | 417 | 27.1 |
| HenryV (Naxos) | 414 | 17.4 |

Only the **first-row** pattern reliably indicates an intro, which is why
gate 6.7e gates on the phrase start rather than on the size of the gap.

---

### FAILURE MODE 6 — expecting `b` to cascade (2026-08-15)

**Tell:** a start time was corrected in the reader and the karaoke sweep
still drifts.

`b` writes `line_timestamps` (and `line_spoken_status`) with
`source = 'manual'`. `p`/`P` nudge +/-0.2 s through the same path. **Nothing
propagates any of that into `phrase_timestamps`** — no triggers, and no code
in this repo writes that table outside test fixtures.

So after a correction the line and its phrases disagree by exactly the amount
corrected, silently.

**Mitigation shipped here (`40317014`, extended in `d339ad9f`):** `b` and
`p`/`P` now toast when the line's phrase rows disagree with the written start
by more than `STALE_PHRASE_SECS` (1.0 s), or when coverage does not start at
char 0 (FAILURE MODE 2). It **observes only** — see FAILURE MODE 7. Repair
remains litdb's `build_phrase_timestamps.py`.

**Do NOT make `b` fix it.** Considered and rejected, recorded so it is not
re-proposed:

- **Shifting the phrase rows** is wrong whenever the line was misaligned and
  the phrases were not — the common case for a manual correction.
- **Deleting them** throws away karaoke data to fix a start time.
- **Invoking `build_phrase_timestamps.py`** from a GTK keypress makes this
  app a writer to a pipeline litdb owns.

A stale phrase can no longer cause a wrong *start*: this app clamps its one
phrase-derived seek with `.max(line_start)` (`prose_cross_time`), and `ro`
floors its window start likewise (`1dd2fe8`). What remains is sweep drift, a
display problem. The floor is a defence, not a fix.

---

### FAILURE MODE 7 — asserting which side of a disagreement is wrong (2026-08-15)

**Tell:** a warning message, comment, or commit says the phrases are stale /
the line is right (or the reverse).

litdb shipped gate 6.7e asserting `line_timestamps` was the correct side, and
had to correct the message (`b4ecd85`). On LoJ the line *was* right — but
that is a fact about LoJ, not an invariant. FAILURE MODE 1 is the case where
believing it cost a session.

**Applies to this repo's toast.** Both messages are deliberately
observational — `"N phrase rows disagree with this line by X.Xs"` and
`"phrase coverage starts N chars into this line"` — with a unit test
(`wording_observes_rather_than_diagnoses`) asserting the words "stale",
"wrong", "rebuild" and "fix" stay out of them. If a future change
reintroduces a diagnosis, that test fails on purpose. A second test
(`drift_wording_does_not_describe_a_gap`) keeps the two messages from
converging, since they route to different repairs.

---

### FAILURE MODE 8 — start-only rows read as a silent gap (2026-08-16)

**Tell:** during playback the highlight advances to the next line while the
current line is still being spoken — by a fixed ~1.5 s, on every line of a
run — and those lines were just stamped with `b`. Log signature: after
`TS: set start=… end=0.00 line=N` for consecutive lines, `CURSOR_SYNC:`
lands on line N+1 at `next.start - 1.5`. (R2-Arkangel 3.3.1-4; the user
deleted and re-stamped the four lines twice before reporting.)

**Root cause (two halves).** (1) `b` filled the cursor line's `end_time`
only from the NEXT dialogue line's existing start, so stamping top-to-bottom
left every line but the last with `end_time` NULL (loads as `0.0`).
`p`/`P` touch only `start`, so nudging cannot repair it. (2) The sync
engine's gap early-jump (`find_line_for_time`, `src/mpv/client.rs`)
treated an unusable `a_end` as "gap unmeasurable, assume a gap" and applied
`SYNC_GAP_PREROLL` (1.5 s) unconditionally — most of a ~2.2 s verse line.

**Fix (this repo, `fix/start-only-sync-lead`).** (1) `set_start_time` now
also backfills the PREVIOUS dialogue line's end (`prev_end_backfill`: only
when it is missing and that line was stamped < 10 s ago; a good end is never
overwritten). (2) With `a_end` unusable the early jump fires only when
`b.start - a.start > SYNC_ASSUMED_LINE_SPAN (6 s) + SYNC_GAP_THRESHOLD` — a
start-only line before a scene break keeps its lead; consecutive lines do
not. Unit tests: `start_only_consecutive_lines_do_not_lead`,
`prev_end_backfill_tests`.

Rows stamped before the fix stay start-only in lit.db (harmless now for
sync; `b` on the next line down still backfills them if within 10 s). This
is reader-side data shape, not a litdb defect — no upstream routing.

---

## Diagnostic order for "the audio starts in the wrong place"

1. **Identify the PLAYING edition first** (`Cym` vs `Cym-Amb` vs `Cym-BBC`).
   Each has its own media and timestamps; the wrong abbrev inspects the
   wrong data. Confirm from the title bar or the log's `SEEK:` /
   `CURSOR_LINE:` lines.
2. **Compare the two tables for that row** (read-only, safe):

   ```bash
   sqlite3 "file:/home/mlj/utono/litdb/data/lit.db?mode=ro" "
   SELECT lt.start_time AS line_start,
          MIN(pt.start_time) AS phrase_start
   FROM line_timestamps lt
   JOIN phrase_timestamps pt
     ON pt.line_mapping_id = lt.line_mapping_id
    AND pt.media_id = lt.media_id
   WHERE lt.line_mapping_id = ?  AND lt.media_id = ?
   GROUP BY lt.line_mapping_id;"
   ```

3. **If they disagree, re-transcribe the audio** — never trust the cached
   JSON (FAILURE MODE 1).
4. **Check whether it is even an audio bug.** `ro` commit `a41cc40` was a
   defect presenting as "wrong start time" that came from the *text* side: a
   card took its lead-in from across a `heading` row that had no phrase
   timestamps at all.

## Gotchas when running litdb's gates

- Pass the abbrev from `line_mapping`, **not** from `media_files`. For media
  320 those disagree (`BH` vs `BH-Barrett`) and the gate silently reports
  SKIP under the wrong one.
- Media 56's `media_files.work_abbrev` is empty; its real abbrev is `TT`.

## Upstream routing

When a reader bug root-causes to lit.db data, **the fix and its regression
guard land in litdb**; this repo gets only a ledger entry linking to the
upstream commit. Never patch around an upstream defect in reader code.

## Related

- `docs/handoff-2026-08-15-stale-phrase-toast.md` — the toast's own handoff.
- `~/utono/litdb/docs/handoffs/handoff-2026-08-15-phrase-line-timestamp-misalignment.md`
  — the corpus-side investigation, gates 6.7e/6.7f, and three repairs.
- `~/utono/ro/docs/guides/audio-timestamps.md` — the reader-side guide.
- `docs/shakespeare-timestamp-fixes.md` — the `b`-by-hand precedent.
- `docs/troubleshooting/sync-and-karaoke-testing.md` — the real-MPV harness.
