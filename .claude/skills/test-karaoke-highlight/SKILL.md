---
name: test-karaoke-highlight
description: Use when verifying or tuning linux-lit's karaoke phrase highlight on any work — the tint not advancing/disappearing during narration, or phrase spans cutting mid-clause for a long-clause reading style. Runs a headless real-MPV sweep with LIT_DEBUG_KARAOKE tracing and reports the clause-width profile.
argument-hint: <ABBR> [<ABBR>...] | all-prose [--secs N] [--short N] [--keep-log]
---

# Test Karaoke Highlight (real MPV, headless)

Drives the real chain — private mpv → TimePos → the phrase sweep in
`src/input/phrase_highlight.rs` — with `LIT_DEBUG_KARAOKE=1` so every decision
the sweep makes is traced. Answers two different questions:

- **Is the sweep live?** (asserted) — does the tint actually advance?
- **How wide are the phrases?** (reported) — the tuning number for a reader
  who takes a long period, with two or more relative clauses, as ONE unit.

## Run

```bash
.claude/skills/test-karaoke-highlight/run-karaoke-test.sh --secs 30 --at 0.5 --keep-log TT
```

**Use `--at 0.5` for any clause-width question.** It starts playback halfway
through the timestamped text, past front matter and any narrated table of
contents. Without it a long novel measures its TOC and reports nonsense (PP:
median 0.74s / 73%-under in the TOC vs 1.80s / 17% mid-book — same data, same
build). Omit `--at` only when testing liveness from a cold start.

```bash
.claude/skills/test-karaoke-highlight/run-karaoke-test.sh all-prose
```

`all-prose` = every work having `phrase_timestamps` rows (the sweep needs
spans; only backfilled editions have them). Run from the repo root. No
`e2e-env.sh` wrapper — the script runs its own `dbus-run-session` per work and
is hermetic: own `XDG_RUNTIME_DIR`, DB copy, mpv, socket, and log.

Options: `--secs N` playback observed per work (default 25); `--short N` the
"too short for a long-clause reader" threshold in seconds (default 1.5);
`--at F` start F (0.0-1.0) of the way into the timestamped text — the reliable
way past a TOC; `--skip N` step N dialogue lines in first (keyboard walk, only
useful for a handful of lines); `--keep-log` keeps traces on PASS too;
`--start-line N` a specific `line_mapping_id`.

## Reading the result

```
PASS  TT  advances=9  median=2.08s  mean=2.65s  max=5.84s  under1.5s=33%  median_words=8  misses=13
```

- `advances` — distinct `KARAOKE: paint` events. Under 3 is a FAIL: the tint
  is not moving. The trace tag counts in the FAIL detail name the cause.
- `median` / `mean` / `max` — phrase duration. **This is the clause-width
  dial.** A median near 1s means the grouper is chopping inside clauses; ~2-3s
  with a `max` of 5-6s means long periods survive whole.
- `under1.5s=%` — share of spans too short to carry a clause. High = chopped.
- `misses` — ticks that resolved nothing and held the previous tint. A small
  number is normal (inter-phrase gaps, page straddles). **Hundreds means the
  sweep is mostly frozen even though it "passed"** — read the trace.

## Sweeping a whole work

`run-karaoke-test.sh` samples ONE position. To cover every passage, use the
sweep driver — it walks the work in `--at` windows and prints a whole-work
roll-up including a mid-clause-cut count:

```bash
.claude/skills/test-karaoke-highlight/sweep-work.sh TT --windows 12 --secs 30 --speed 3.0
```

`--speed` is the accelerant: mpv advances through `time-pos` faster, so each
wall-clock second covers proportionally more text. Span timestamps are
absolute, so speed does not distort the widths being measured — only how fast
the sweep walks them. 3x is comfortable; the last window (`--at 0.98`) often
FAILs on too little audio left, which is expected.

TT whole-work, 12 windows at 3x: 96 distinct phrases, median 2.90s, p10/p90
1.20s/4.86s, 0 misses, 11/12 windows PASS.

**Interpreting the mid-clause count:** it flags spans not ending in terminal
punctuation, which over-reports in two ways. (1) A long span that IS a
complete unit but exceeds the 6.0s budget has no closing mark — TT's worst
"offenders" are 6.7-7.2s complete clauses, held too whole rather than cut.
(2) **Italic markup shifts the count.** On lines whose `canonical_text`
contains `_italics_`, the reader strips the underscores for display and
translates DB char offsets through `italic_offset_map`; where that
under-compensates, the paint is shifted and the span text comes out mangled
(`d how can it be otherwise, w` instead of `and how can it be otherwise,` —
shifted by exactly the 2 underscores earlier in the line). 70 of TT's 279
phrase-bearing lines carry italics, which accounts for most of its 22%
raw mid-clause rate. Measured on non-italic lines only, TT is 2913 spans,
mean 3.04s, 8.9% mid-clause. **This offset bug is pre-existing** (identical
in a pre-merge DB backup) and is a linux-lit rendering issue, not a grouping
one — verify against the DB before blaming the grouper.

## Baselines (2026-07-25, `--secs 30 --at 0.5`)

Measured after litdb `2674463` (SEAMLESS_MAX_SECONDS 6.0, SILENCE_GAP 0.45).
Compare a suspect work against these before concluding the grouper regressed:

- `TT` — median 3.18s, max 3.40s, 0% under 1.5s, 0 misses.
- `DC` — median 2.30s, max 3.08s, 0% under.
- `PP` — median 1.80s, max 4.27s, 17% under.
- `MobyDick` — median 1.40s, max 3.78s, **60% under**. NOT a defect: this
  passage is dialogue-dense, and speech tags ("said Stubb.", "said the old man
  sullenly.") are genuinely short. Its long narrative clauses still hold whole.

A median at or above ~1.8s with the max reaching 3-6s is the shape that suits a
long-clause reader. Before calling a low median a regression, **read the
`paint` lines**: short spans that are speech tags are correct, short spans that
cut inside a clause are not. A median near 0.7s usually means the measurement
landed in front matter or a table of contents — re-run with `--at`.

Long clauses confirmed held whole after the litdb fix (TT):
*"and I being wholly free from that slavery which booksellers usually lie under
to the caprices of authors,"* — one span. The residual known split is
*"not at all regarded or thought on by any of"* / *"our present writers;"*,
the un-mergeable splice boundary noted in litdb `2674463`.

## Trace tags (`/tmp/karaoke-logs/<ABBR>-karaoke.log`)

Every line is prefixed `KARAOKE:`. When the tint misbehaves, the tag says why —
do not guess from a screenshot:

- `paint` — the advance. Logs the span's window AND the exact tinted text, so
  a mid-clause cut is visible in the log without watching the screen.
- `profile` — per-line span-width summary, emitted on each cache fill.
- `gate-off` — deliberately off. Reports `cursor_line_mode`, `vocab_loop`,
  translations, and class mode. Verse launches in cursor-line mode (sweep off)
  and needs Alt+p; prose launches in karaoke.
- `gate-hold` — paused or mid-load; the tint is kept on purpose.
- `gate-suppressed` — a manual seek/nav window. `held=true` means a pending
  paint is holding the tint; `held=false` clears it.
- `resolve-miss` — narration is outside the ±8-line walk window around the
  cursor. A long run means cursor and audio are far apart.
- `span-miss` — the line resolved but no span is active at that time; a
  repeating run on a line with `spans>0` means the line's phrase times and its
  line timestamp disagree (they are backfilled separately).
- `tick` — heartbeat every 50 ticks, proving the driver still fires.

## Gotchas

- **Front matter suppresses sync indefinitely.** Stepping `q` onto an
  untimestamped line logs `NO_TIMESTAMP suppress=86400s`, which stops the
  sweep. The script steps `q` until a SEEK carries a real `start=`; if you
  drive this by hand, do the same.
- **Cursor/audio alignment is required.** `resolve_spoken_idx` only walks ±8
  work lines around the CURSOR, so audio playing far from the reader's line
  yields a flood of `resolve-miss`. That is a harness artifact, not a bug.
- **`LIT_START_POS` is not honored on every load path** (e.g. DB-join works
  with no `text_file`, like TT) — align by stepping, not the env var.
- **Measure prose, not the table of contents.** The first timestamped lines of
  a Gutenberg text are often the TOC ("DETAILED CONTENTS", "The Pickwickians
  2."). Title fragments are short by nature, so a run that starts there
  reports a chopped-looking median that says nothing about the grouper. PP
  scored median 0.74s / 73%-under-1.5s purely from TOC entries. Use `--skip N`
  and confirm from the `paint` lines that the tinted text is real prose.
- **`Q` (JumpToNextDialogue) is the stepper, not `q`** (JumpToNextSpeaker):
  in prose with no speaker changes `q` is a no-op after the first line.
- **"Play toggle did not start playback" is almost always key DELIVERY, not
  playback.** Check the failing work's `/tmp/karaoke-logs/<ABBR>-playfail.log`
  for `KEY:` lines: zero means wtype went to the wrong Wayland socket (the app
  renders and logs everything else normally). The script now resets `WLSOCK`
  per work, waits for a real socket file, and handshakes with an Escape before
  measuring. Two traps behind this: `basename ""` returns `"."`, so an
  empty-glob check on the basename yields a bogus-but-truthy socket name; and
  `WLSOCK` is loop-scoped, so a stale value from the previous work points into
  an already-removed runtime dir. Symptom: the FIRST work passes and every
  later one fails.
- **Editions matter.** A work can carry several media, each with its OWN
  `line_timestamps`; the script picks by `wma.priority DESC` to match the app.
  Selecting by phrase count instead made the reader seek with one edition's
  times while another played (PP line 61: 20.79s on m241 vs 58.61s on m245).
  The tell is `MPV discovery: switching active media_id from Some(X) to Y`;
  the script now SKIPs that work rather than report bogus timings.
- Grouping is **upstream data, not reader code**. If phrases are too narrow,
  the fix belongs in litdb's `scripts/build_phrase_timestamps.py`
  (`SEAMLESS_MAX_SECONDS`, `SILENCE_GAP`, `CUT_WINDOW`) and the repaired rows
  are re-backfilled — never patched around in linux-lit.
