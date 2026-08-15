# Handoff: warn when `b` leaves phrase timestamps stale

Date: 2026-08-15
From: `~/utono/ro`
To: `~/utono/linux-lit`

> **Status: IMPLEMENTED, with corrections — read this box before acting on
> anything below.**
>
> Landed in linux-lit `40317014` on 2026-08-15. The original text is kept
> intact as a record of what was believed; corrections are marked inline.
> Four things changed:
>
> 1. **The threshold question was the wrong question.** Triage of the 67
>    rows shows 88% are dramatic recordings where divergence is normal, so
>    no threshold separates signal from noise. The answer was a change of
>    WORDING, not a number — see "Correction: the threshold".
> 2. **Do NOT align the constant with litdb's gate.** The handoff asks for
>    this below; it is now explicitly rejected. They answer different
>    questions at different costs — see the same section.
> 3. **The toast must not say "stale".** litdb had to correct gate 6.7e for
>    asserting which side was wrong (`b4ecd85`). The shipped wording
>    observes only, and a unit test enforces it.
> 4. **litdb's side has moved a long way.** Gates 6.7e/6.7f landed, three
>    media were repaired, and the five-row diagnosis this handoff's sibling
>    made turned out to be four different defects. See "Correction: what
>    litdb landed".
>
> Diagnosis lessons from both sides are now consolidated in
> `docs/troubleshooting/timestamp-diagnosis.md`. Read that before
> diagnosing any wrong-start-time complaint.

## The ask, in one sentence

When `b` (or `p`/`P`) writes a line's start time and that line has
`phrase_timestamps` rows that now disagree with it, **say so in a toast** —
change no data, cascade nothing, just tell the user the phrase rows are stale.

## Why

`b` writes `line_timestamps` (and `line_spoken_status`) and never touches
`phrase_timestamps`. Nothing else does either — verified two ways:

- No code in this repo re-derives, shifts, or invalidates phrase rows on a
  line edit; `phrase_timestamps` appears in the write path only inside test
  fixtures.
- **`phrase_timestamps` has no triggers.** `line_timestamps` has three
  (`_mirror_ad/_ai/_au`), but they only mirror a row between the two
  `media_id`s of a priority 20 ↔ 10 twin.

So after a correction the line and its phrases disagree by exactly the amount
corrected, silently. The karaoke sweep keeps driving off the stale phrases,
and the only way to notice is to watch the highlight drift.

This surfaced from the other direction. `ro` played the Audible intro on LoJ
because it took a phrase start (0.594 s) over the line start (16.264 s); both
readers now floor the start at the line, so **a stale phrase can no longer
cause a wrong start time**. What remains is sweep drift — a display problem,
on a small population. A toast is proportionate to that; a cascade is not.

## Why NOT to make `b` fix it

Considered and rejected, recorded here so it is not re-proposed:

- **Shifting the phrase rows** is wrong whenever the line was misaligned and
  the phrases were not — which is the common case for a manual correction.
- **Deleting them** throws away karaoke data to fix a start time.
- **Invoking `build_phrase_timestamps.py`** from a GTK keypress makes this app
  a writer to a pipeline `litdb` owns.

The invariant that matters is already enforced where it belongs: this app
clamps its one phrase-derived seek with `.max(line_start)`
(`prose_cross_time`), and `ro` floors its window start at
`line_timestamps.start_time`. Neither reader will start a row's audio before
the row does.

## Scale — this is a small, real population

Measured against the live corpus 2026-08-15:

- **1,133** rows carry `source = 'manual'`.
- Of those, the number whose phrases disagree with the corrected line:

| threshold | rows |
|-----------|------|
| > 0.5 s | 67 |
| > 1.0 s | 44 |
| > 2.0 s | 20 |
| > 5.0 s | 10 |

So under 4% of corrections at a 1 s threshold. The toast will fire rarely,
which is what makes it worth reading when it does.

Worst offenders give a sense of the shape (`line_mapping_id`, `media_id`,
line start, phrase count, earliest phrase):

```
1613795  58   667.29   31 phrases   min 580.67
1368552  15  3987.17    3 phrases   min 3962.13
1386687  21    92.36    1 phrase    min 112.15
1330726   4  7848.90    2 phrases   min 7831.29
1760896  56    17.33    1 phrase    min 1.35
```

Note 1386687: the phrase starts *after* the line. Drift runs both ways, so the
check must be on absolute difference, not a one-sided comparison.

## Suggested implementation

A query and a toast, in `set_start_time` after `upsert_start_time` succeeds
(`src/input/timestamps.rs:132`, write at ~line 189).

The count to display — phrase rows for this (line, media) whose span is now
inconsistent with the written start:

```sql
SELECT COUNT(*),
       MIN(start_time)
FROM phrase_timestamps
WHERE line_mapping_id = ?1 AND media_id = ?2;
```

Compare `MIN(start_time)` against the `time_pos` just written; if
`abs(min_start - time_pos) > STALE_PHRASE_SECS`, toast the count.

Wording, matching the existing register (`NOT_SPOKEN_TOAST` at
`timestamps.rs:23` is `"Not a spoken line — no timestamp set"`):

```rust
const STALE_PHRASE_SECS: f64 = 1.0;

// e.g. "start set — 6 phrase rows now stale (rebuild to fix karaoke)"
```

> **Correction: that wording shipped changed.** It asserts the phrase rows
> are the wrong side ("stale", "rebuild to fix"), which the toast cannot
> know. litdb shipped exactly that claim in gate 6.7e and had to correct it
> (`b4ecd85`): on LoJ the line was right, but that is a fact about LoJ, not
> an invariant. The shipped wording observes instead:
>
> ```
> 13 phrase rows disagree with this line by 11.3s
> ```
>
> A unit test (`wording_observes_rather_than_diagnoses`) asserts "stale",
> "wrong", "rebuild" and "fix" stay out of the string, so a future change
> that helpfully reintroduces a diagnosis fails on purpose.

The toast helpers are `show_chapter_toast(state, text)` and
`show_chapter_toast_secs(state, text, secs)`
(`src/input/navigation.rs:2849`, `:2922`). `timestamps.rs` already uses the
former for `NOT_SPOKEN_TOAST`, so either fits the house style.

### Details worth getting right

- **Absolute difference, not signed.** Phrases can end up either side of the
  corrected line (see 1386687 above).
- **Say nothing when the line has no phrase rows.** Silence is correct there —
  many lines legitimately have none, and a toast on every one would train the
  user to ignore it.
- **Non-fatal.** If the query fails, log and move on. The timestamp write has
  already succeeded and must not be reported as failed — the same treatment
  `upsert_spoken_status` already gets at `timestamps.rs:193`.
- **`p` / `P` too.** `nudge_start_time` (`timestamps.rs:542`) goes through the
  same `upsert_start_time`. A ±0.2 s nudge will rarely cross a 1 s threshold on
  its own, but repeated nudges accumulate and should eventually warn. Putting
  the check in one helper called by both paths is cleaner than duplicating it.
- **`Alt+b` (end time) is out of scope** unless it is free — the reported
  problem is start drift.

### The threshold

`1.0 s` is proposed, not measured-optimal. It gives 44 rows corpus-wide today.
`0.5 s` gives 67 and would catch more genuine drift at the cost of firing on
ordinary clause jitter; `2.0 s` gives 20 and is quieter. Pick whatever you
will actually read — and if `litdb` lands the consistency gate discussed
below, align this constant with that gate's tolerance so the two never
disagree about what "stale" means.

### Correction: the threshold, and why not to align it (2026-08-15)

**Shipped at `1.0 s`, and deliberately NOT aligned with litdb's gate.** Both
halves of the advice above are superseded.

litdb's spec `636dddc` does not pin a tolerance — it calls tolerance "the open
question" and says the 67 rows need triage first. Blocking on it would mean
waiting for work nobody has scheduled.

That triage, run 2026-08-15, changes what the number means. Of the 67 manual
rows drifting >0.5 s:

| group | rows | >1.0s | >2.0s | worst |
|---------------------|------|-------|-------|--------|
| dramatic recordings | 59 | 37 | 13 | 25.04s |
| prose / other | 7 | 6 | 6 | 86.62s |

88% are dramatic recordings (Arkangel/Naxos/Amb/BBC/RSC) — the population
litdb established diverges legitimately, where overlapping dialogue, music
and effects make phrase/line divergence normal. That is why gate 6.7e gates
on the phrase START rather than on the size of the gap.

The per-media comparison is the useful part:

| work | media | manual drifting | media drifting |
|--------------|-------|-----------------|----------------|
| 2H6-Arkangel | 15 | 12 | 960 |
| Mac-Arkangel | 21 | 8 | 1001 |
| TT | 56 | 3 | 4 |
| BH-Barrett | 320 | 2 | 51 |

On 2H6-Arkangel a manual edit is indistinguishable from the recording's own
baseline. On TT/56, three drifting manual rows against a baseline of four is
real signal.

At a flat `1.0 s`, 37 of 44 firings land on media where drift is already
normal — roughly a 6:1 false-positive ratio, enough to train someone to
dismiss the toast. **No threshold fixes that**, because the discriminator is
per-media baseline, not gap size.

So the answer was not a number. It was the wording: the toast reports what it
measured and leaves the judgement to the reader. A per-media baseline
suppression is a known, cheap follow-up (one query) — deliberately not
shipped, because tuning against 44 rows is premature.

**Why not align with litdb's gate.** They answer different questions at
different costs:

- The toast asks *"did the edit I just made create a disagreement?"* It fires
  on one row the user just touched, where a false positive costs a glance.
- The gate asks *"does this corpus have drift worth fixing?"* It fires over a
  whole wizard run, where false positives turn every run red and train
  operators to ignore gate output.

Different costs justify different numbers. Coupling them would force one of
the two to accept the wrong error budget. This is recorded in the
`STALE_PHRASE_SECS` doc comment so the coupling is not re-proposed.

## Correction: what litdb landed (2026-08-15)

The sibling handoff has been revised substantially since this one was
written. Its current text lives at
`~/utono/litdb/docs/handoffs/handoff-2026-08-15-phrase-line-timestamp-misalignment.md`
(note: moved into `docs/handoffs/` in `76ec8fc`). What changed:

- **The five intro-contamination rows were four different defects**, not one.
  Only LoJ was straightforward. DC was a clean one-row DELETE; MND-Arkangel
  needed a full REBUILD (all three of its first row's phrases were wrong);
  Tmp-Arkangel/33 and TT/56 were deliberately NOT repaired and need their own
  specs — a row-scoped fix there would move a reader from "wrong at second
  zero" to "wrong somewhere less obvious", which is worse.
- **Gates 6.7e and 6.7f landed** (`93781e1`, fixed in `4d76a48`, message
  corrected in `b4ecd85`). 6.7e catches intro contamination; 6.7f reports
  orphan phrase rows, advisory by default.
- **The population is larger than reported here.** Any check anchored on
  `line_timestamps` is blind to phrase rows on lines with no line timestamp:
  **10,169 orphan rows across 73 media**. BH-Barrett passes the first-row
  check while carrying 19 intro-region phrases.
- **Three repairs shipped**: DC/325, MND-Arkangel/4, LoJ/233.
- **The `updated_at`/`source` gate proposed in that handoff's §3 is
  unwritable.** `phrase_timestamps` has no such columns. litdb's accepted
  alternative is to gate on the invariant directly — which is the same check
  this toast performs, one level up.

**The most valuable thing in that revision is a diagnosis trap**, not a
finding: a session read the cached whisperX JSON, found no intro, and
concluded `line_timestamps` was the wrong side — the exact opposite of the
truth. The JSON is the artefact the bad alignment was produced from, so
diagnosing from it is circular. Re-transcribing the audio showed "This is
Audible." at 0.537–13.143 s. Recorded as FAILURE MODE 1 in
`docs/troubleshooting/timestamp-diagnosis.md`.

## What this does not do

- It does not repair anything. The fix is
  `python scripts/build_phrase_timestamps.py` in `~/utono/litdb` for the
  affected work/media.
- It does not detect drift that arose any other way — the aligner producing
  bad phrases in the first place is a `litdb` problem, and is the subject of
  `~/utono/litdb/docs/handoff-2026-08-15-phrase-line-timestamp-misalignment.md`
  (**moved**: now `docs/handoffs/…`). That handoff proposes a corpus-wide
  gate; a `litdb` session reviewing it has
  since found the population is broader than first diagnosed (10,169 orphan
  phrase rows with no line timestamp at all). Read it before assuming this
  toast is the whole story.

## What shipped (linux-lit `40317014`, 2026-08-15)

One file, `src/input/timestamps.rs`, +156 lines.

- `stale_phrase_note(conn, line_mapping_id, media_id, written_start)` runs
  after `upsert_start_time` succeeds in **both** paths — `set_start_time`
  (`b`) and `nudge_start_time` (`p`/`P`), as this handoff asked.
- The pure decision is split into `phrase_drift_note(count, min_start,
  written_start)` so it is testable without a database.
- Absolute difference, per this handoff. Verified against a real row that
  drifts the other way: TT/56 line 1760897's phrases start **11.3 s after**
  its line.
- Silent when the line has no phrase rows; non-fatal on query failure (logs
  and says nothing, since the write already succeeded).
- Log line for diagnosis: `TS: phrase drift line=… media=… written=…
  min_phrase=… diff=… n=…`.

Seven unit tests use real corpus rows as fixtures — LoJ 1790843 pre-repair
(phrases before the line), TT/56 1760897 (phrases after), TT/56 1760896
(singular wording), plus threshold-boundary and no-phrase-rows silence.

Not shipped, deliberately: per-media baseline suppression (premature at 44
rows) and any handling of `Alt+b` end times (out of scope, as stated).

## Precedent in this repo

`docs/shakespeare-timestamp-fixes.md` already records fixing a recording whose
reading starts after a "We present…" intro and applause, by setting the first
line's start by hand with `b`. That is exactly the workflow this toast
supports: the correction is right, and the phrase rows underneath it are now
wrong until rebuilt.

## Related

- `~/utono/ro/docs/guides/audio-timestamps.md` — the two tables, which app
  reads which, the correction workflow and its caveat, and the diagnostic
  order for a suspected wrong start time.
- `~/utono/ro` commit `1dd2fe8` — the floor that makes stale phrases a display
  problem rather than a playback one.
