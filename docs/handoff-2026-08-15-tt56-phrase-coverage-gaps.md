# Handoff: TT/56 phrase coverage gaps (karaoke goes dark mid-line)

Date: 2026-08-15
From: `~/utono/linux-lit`
To: `~/utono/litdb`

## The ask, in one sentence

TT / media 56 has six lines whose `phrase_timestamps` do not cover the whole
line, so karaoke goes dark across the uncovered span — **and this is the media
your own handoff marks "do not rebuild"**, so it needs the targeted treatment
you gave LoJ, not a backfill.

## How it surfaced

`linux-lit` shipped a toast that fires when a line's phrase rows disagree with
its start time (`40317014`, from your sibling handoff). It fired on TT.2.0.34
with "13 phrase rows disagree with this line by 12.8s". Watching playback then
showed the actual symptom: **no karaoke at all** from `Though the author has
written a large Dedication...` through `...alderman than a patent`, resuming
mid-sentence at `not at all regarded or thought on by any of our present
writers;`.

The toast's number was measuring the wrong thing. The 13 rows are fine.

## The defect: a gap, not a shift

Line 1760897 (`TT.2.0.34`), media 56. All 13 phrase rows, in char order:

```
  id     sc   ec     st      et     dur
2847255  192  256  30.846  34.047  3.201
2847256  257  362  35.268  41.110  5.842
2847257  363  401  42.051  43.991  1.940
2847258  402  444  44.391  46.652  2.261
2847259  445  495  47.553  50.414  2.861
2847260  496  535  52.055  54.476  2.421
2847261  536  553  54.836  55.556  0.720
2847262  554  582  56.796  58.157  1.361
2847263  583  624  58.677  60.458  1.781
2847264  625  678  61.162  63.303  2.141
2847265  679  747  63.903  67.424  3.521
2847266  748  838  68.424  73.045  4.621
2847267  839  965  74.606  81.428  6.822
```

**These rows are internally clean** — monotonic in both char and time, no
overlaps, no duplicate spans, and they close the line exactly (`end_char 965`
= line length). The defect is that coverage *starts* at `start_char = 192`.

Chars 0–191 have no phrase row at all:

```
Though the author has written a large Dedication, yet that being addressed
to a Prince whom I am never likely to have the honour of being known to; a
person, besides, as far as I can observe,
```

Char 192 onward is `not at all regarded or thought on by any of our present
writers;` — exactly where the highlight resumes on screen. The rendered
symptom and the data agree precisely.

`MIN(start_time)` is 30.846 only *because* the opening rows are missing. That
is what made it look like a 12.8 s shift.

## Scope on this media: six lines

Two kinds. `leading` = coverage starts past char 0; `interior` = a hole
between consecutive spans (>20 chars).

| line | citation | kind | from | to | size | len |
|---------|-------------|----------|------|------|------|------|
| 1760897 | TT.2.0.34 | leading | 0 | 192 | 192 | 965 |
| 1761156 | TT.17.0.293 | leading | 0 | 128 | 128 | 162 |
| 1761101 | TT.14.0.238 | interior | 3730 | 3812 | 82 | 4096 |
| 1761087 | TT.13.0.224 | interior | 384 | 432 | 48 | 1235 |
| 1760884 | TT.1.0.2 | leading | 0 | 45 | 45 | 58 |
| 1761110 | TT.14.0.247 | interior | 724 | 758 | 34 | 3942 |

Six of the 279 lines that have any phrase rows on this media. Isolated, not
systemic.

Two are worth individual note:

- **1761156** (`TT.17.0.293`) is the worst in proportion: a 162-char editorial
  bracket carrying **one** 10-char row (chars 128–138, 1.6 s). It also has a
  24-char trailing shortfall — the only one on the media. Effectively
  uncovered.
- **1760884** (`TT.1.0.2`) has **no `line_timestamps` row at all** — it is in
  your 6.7f orphan class as well. Its text is long-s:
  `A _Character of the prefent Set of_ Wits _in this Ifland_.` That is a
  plausible reason the aligner dropped it, and it may generalize to other
  long-s lines corpus-wide.

Reproduce (read-only):

```sql
WITH s AS (
  SELECT line_mapping_id lid, start_char sc, end_char ec,
         LEAD(start_char) OVER (PARTITION BY line_mapping_id
                                ORDER BY start_char) nxt
  FROM phrase_timestamps WHERE media_id = 56)
SELECT lid, 'interior' kind, ec, nxt, (nxt - ec) size
FROM s WHERE nxt IS NOT NULL AND (nxt - ec) > 20
UNION ALL
SELECT line_mapping_id, 'leading', 0, MIN(start_char), MIN(start_char)
FROM phrase_timestamps WHERE media_id = 56
GROUP BY line_mapping_id HAVING MIN(start_char) > 0
ORDER BY size DESC;
```

## Why this needs a spec, not a backfill

**Your own handoff rules out the obvious fix.** From
`docs/handoffs/handoff-2026-08-15-phrase-line-timestamp-misalignment.md`:

> **TT / media 56 — do not rebuild.** ... TT/56 is the media whose smear
> repairs shipped on 2026-07-25, and it still carries that repaired state
> (**13 phrases on line 1760897**). A backfill re-aligns from the whisperX
> JSON, which still contains the dropout, so a rebuild would silently discard
> shipped repair work.

Note the coincidence: the line cited there as carrying shipped repair state is
**this same line**. The 13 clean rows are the *result* of that repair. So:

- A `build_phrase_timestamps.py` run would re-align from a JSON that still has
  the dropout — discarding the repair and likely reintroducing the smears.
- The gap is almost certainly *in the JSON*, not in the DB write. The aligner
  never produced rows for those words.

The precedent that fits is your **LoJ/233** repair: re-transcribe the affected
audio span, group with the project's own `group_into_phrases`, and insert only
the missing rows in one transaction — leaving the existing 13 untouched.
Applied here that means generating rows for chars 0–191 spanning roughly the
line start to 30.846, and leaving everything from char 192 alone.

We have NOT run anything. No linux-lit session writes `lit.db` phrase rows.

Media path, since `media_files.work_abbrev` is empty for 56 (your gate gotcha
— the real abbrev is `TT`):

```
/home/mlj/Music/swift-jonathan/ATaleofaTub_ep6.m4b
```

## A gate suggestion

None of your current gates catch this. 6.7e gates on the phrase *start time*
of a media's first row; 6.7f finds orphans by missing `line_timestamps`. A
line with clean, monotonic, non-orphan rows that simply begin at char 192
passes both.

The check is cheap and needs no new columns — coverage, not time:

- `MIN(start_char) > 0` on a line that has phrase rows → leading gap.
- `LEAD(start_char) - end_char > N` → interior gap.
- `length(canonical_text) - MAX(end_char) > N` → trailing shortfall.

Worth running corpus-wide before setting a threshold: we only measured media
56, and the dramatic-recording caveat from your other handoff may well apply
here too (overlapping dialogue could legitimately leave words unaligned). We
would guess a leading gap is higher-signal than an interior one, on the same
reasoning that made 6.7e first-row-only. **Advisory first** — you have been
right about that twice now.

## What linux-lit changed on its side

`d339ad9f` — the toast now distinguishes the two defects, because they need
different repairs:

- Leading gap (`MIN(start_char) > 0`) → "phrase coverage starts 192 chars into
  this line (13 rows)", logged as `TS: phrase gap`. Checked first; it outranks
  the time comparison, since the times present may be correct.
- Otherwise → the existing drift message, logged as `TS: phrase drift`.

Both stay observational — neither claims which side is wrong (your `b4ecd85`
lesson, which we took to heart and unit-test for).

Interior gaps are deliberately NOT detected: they need per-span analysis, and
the toast fires on one row at edit time. That is a gate's job, not a toast's.

This changes no data. `linux-lit` writes `line_timestamps` only, and only via
`b`/`p`/`P`.

## Related

- `~/utono/litdb/docs/handoffs/handoff-2026-08-15-phrase-line-timestamp-misalignment.md`
  — the do-not-rebuild ruling, gates 6.7e/6.7f, the LoJ repair precedent.
- `~/utono/linux-lit/docs/handoff-2026-08-15-stale-phrase-toast.md` — the
  toast this came out of.
- `~/utono/linux-lit/docs/troubleshooting/timestamp-diagnosis.md` — the
  reader-side ledger; FAILURE MODE 1 is the cached-JSON trap, which applies
  directly if anyone diagnoses this gap from the whisperX transcript.
