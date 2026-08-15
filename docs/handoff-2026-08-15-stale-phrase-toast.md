# Handoff: warn when `b` leaves phrase timestamps stale

Date: 2026-08-15
From: `~/utono/ro`
To: `~/utono/linux-lit`

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

## What this does not do

- It does not repair anything. The fix is
  `python scripts/build_phrase_timestamps.py` in `~/utono/litdb` for the
  affected work/media.
- It does not detect drift that arose any other way — the aligner producing
  bad phrases in the first place is a `litdb` problem, and is the subject of
  `~/utono/litdb/docs/handoff-2026-08-15-phrase-line-timestamp-misalignment.md`.
  That handoff proposes a corpus-wide gate; a `litdb` session reviewing it has
  since found the population is broader than first diagnosed (10,169 orphan
  phrase rows with no line timestamp at all). Read it before assuming this
  toast is the whole story.

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
