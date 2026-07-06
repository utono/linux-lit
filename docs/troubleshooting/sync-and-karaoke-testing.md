# Sync & Karaoke Testing (real MPV, headless)

How to verify playback-sync seeks, cursor-sync suppression, and the karaoke
phrase highlight without touching the live session — and how to read the log
lines those paths emit. Complements *headless-testing.md* (nav/pagination) and
the `test-playback-sync` skill (which owns the harness script).

## The harness in one paragraph

```bash
.claude/skills/test-playback-sync/run-sync-test.sh --boundaries 1 BH
```

launches a **private paused mpv** (`--ao=null`, private socket; the DB COPY's
media path is rewritten so `derive_socket_path` can only reach the test mpv)
and the reader in an isolated cage with `LIT_SYNC_TEST=1` (the only flag that
re-enables MPV discovery under `LIT_HEADLESS_TEST`). It presses `x`, `y`,
`Tab` (play) and waits for a sync page turn. Plays PASS/FAIL on turn timing;
see the skill for selectors and failure readings.

## Capture the per-work log BEFORE the trap cleans up

The harness deletes its temp base on exit, and its app log
(`$BASE/<WORK>.log`) is the only place the SEEK/PHRASE_HL evidence lives.
Copy it while the run is going:

```bash
d=$(ls -d /tmp/synctest.* | head -1)        # note: appears a few seconds in
until [ ! -d "$d" ]; do
  [ -f "$d/BH.log" ] && \cp -f "$d/BH.log" /tmp/synctest-bh.log
  sleep 2
done
```

Gotchas hit in practice: a *stale* `/tmp/synctest.*` from a previous run can
match before the new one appears (re-check the mtime); zsh aborts a compound
command on a failed glob, so guard with `[ -f ... ]` instead of globbing.

## BH "sync stall" FAILs are (usually) false

The harness waits 60s for a page turn. BH's opening page holds paragraphs
that are each ~40s of narration — the first boundary is minutes away, so
`no page turn within 60s` fires with sync perfectly healthy. Confirm health
from the captured log instead: `PHRASE_HL: cache fill` lines advancing
through consecutive `line_id`s = the karaoke engine is tracking narration.
A play edition (e.g. `R2-Arkangel`) in the same sweep gives a real PASS/FAIL
on turn timing.

## Reading the SEEK line

```
SEEK: line=4 work_idx=4 start=78.98 base=94.69 seek=94.49 suppress=until-resume
```

- `start` — the line's own `line_timestamps.start_time`.
- `base` — the chosen seek anchor. `base > start` means the mid-paragraph
  page-top branch fired: the cursor sits on a straddling paragraph
  (`current_line == page_top_line`, `page_top_offset > 0`) and the seek
  targeted the phrase at the page-top offset via `prose_cross_time`
  (navigation.rs) — the "first NEW segment of the page", not the paragraph
  start. `base == start` on an offset-0 landing is correct whole-line
  behavior. If x/y ever replays previous-page audio again, this is the first
  thing to check.
- `seek` — `base` minus `SEEK_PREROLL` (0.2s).
- `suppress` — `500ms` while playing; `until-resume` while paused. The
  paused hold exists because a paused mpv freezes at the preroll position and
  the post-seek TimePos echo resolves to the PREVIOUS line — before this
  hold, every nav bind visibly lost a step and `{` re-found the chapter it
  just left. `PlaybackState(true)` clears the hold
  (`SYNC: cleared indefinite suppression on playback start`); `o`/`e` scrubs
  overwrite it with their own 500ms hold, so paused scrub-following works.

## Karaoke evidence in the log

`update_phrase_highlight` runs on **every** TimePos, unconditioned by sync
suppression — so karaoke latency after a nav press is mpv seek processing +
one TimePos event (~100–250ms playing; the single seek echo when paused).
The observable is `PHRASE_HL: cache fill line_id=… media=… spans=N` on each
line change. `spans=0` = cached negative (no phrase rows for that
line/media). There is no per-frame karaoke log; for "did the tint appear",
trust these lines plus a settled screenshot — a transient tint cannot be
reliably screenshotted in cage (see headless-testing.md → grim staleness).

## Karaoke goes dark mid-paragraph: a DATA gap, not reader code

Signature: tint stops at a phrase boundary mid-paragraph and resumes at the
next paragraph. Cause: whisperX (VAD) dropped an audio region — typically
quoted speech after a pause — so the alignment truncated that line's phrase
coverage AND its `line_timestamps.end_time`. Diagnose against the PLAYING
media id:

```bash
sqlite3 ~/utono/litdb/data/lit.db "
WITH cov AS (
  SELECT lm.id, length(lm.canonical_text) len, MAX(pt.end_char) covered
  FROM line_mapping lm
  JOIN phrase_timestamps pt ON pt.line_mapping_id=lm.id AND pt.media_id=<MID>
  WHERE lm.work_abbrev='<ABBR>' GROUP BY lm.id)
SELECT id, len, covered FROM cov WHERE covered < len - 15;"
```

Fix in **litdb**, not linux-lit: the `fix-karaoke-gaps` skill
(`~/utono/litdb/.claude/skills/fix-karaoke-gaps/`) re-transcribes just the
gap clips (fresh VAD context recovers the dropped speech), splices phrase
rows, and extends the truncated end_times. `backfill-phrase-timestamps`
cannot fix this class — the hole is in the whisperX JSON itself. Restart the
reader afterwards (phrase_cache is per-session).

## Perceived nav latency: split handler time from paint time

Handler time is the KEY→ACTION→SEEK cluster (normally 1–5ms). Paint time is
the one-shot probe in scroll.rs:

```
PAINT: first frame for page_top=40 after 12ms
```

logged by both instant page-set paths. If a bind "feels slow" but PAINT is a
frame or two, look at semantics instead — e.g. a **no-op nav press**
(`;` at the first bookmark) logs `ACTION:` + `PROSE_FLASH:` with **no**
`CURSOR_LINE:` between them: that's the flush flashing the current line to
signal "nothing to jump to", not a missed keypress. Sync-driven moves must
never flash; a `PROSE_FLASH` right after a `CURSOR_SYNC` line is a
lingering-flag bug (fixed by the dispatch flush — see
`flush_pending_prose_flash` in highlight.rs).
