# Flag spoken lines for targeted re-alignment

**Date:** 2026-06-01
**Status:** Approved design

## Problem

The `/wizard-ambrose` forced-alignment pipeline (whisperX → `bin/align-forced`)
sometimes fails to assign a timestamp to a line of dialogue that is genuinely
spoken in a production recording. The matcher finds no match and the line is
left untimestamped (and, after Step 6.5, marked `is_spoken=0`). There is
currently no way for a human reading the work in linux-lit to say "this line
*is* spoken — find its timestamp on the next wizard run."

## Goal

While reading a production work in linux-lit, the user marks an
untimestamped-but-spoken line. A later wizard re-run then aligns just those
flagged lines against the transcript, using the human's manual mark both as a
search anchor and as a validity check.

This spans two repositories:

- **linux-lit** — the `u` keybind, extended.
- **whisper-transcript** — `bin/align-forced`, a new targeted-pass flag.

## Half 1 — linux-lit: `u` also marks the line spoken

### Behavior

The existing `u` / `Right` bind (`Action::SetStartTime` →
`crate::input::timestamps::set_start_time`) already:

- captures the current MPV position (minus a 0.30s lead) as a manual
  `line_timestamps` row (`source='manual'`), via
  `queries::upsert_start_time`,
- updates in-memory `line.timestamp`,
- updates the sign column and advances the cursor.

**Change:** after the timestamp upsert succeeds, also upsert
`line_spoken_status` to `is_spoken=1, confidence=1.0` for the same
`(line_mapping_id, media_id)`, and set in-memory `line.is_spoken = Some(true)`.

Each press sets `is_spoken=1` (never toggles to 0). There is no separate
flag-only keybind — `u` does both jobs.

### Dual-media (Ambrose `.mkv` + `.m4b`)

No extra code. The `line_spoken_status_mirror_ai` / `_au` triggers already
mirror any priority-20 (`.mkv`) write to the priority-10 (`.m4b`) twin, exactly
as the `line_timestamps` triggers do. linux-lit writes against the active
`media_id` (the priority-20 `.mkv` per wizard convention), so mirroring is
automatic.

### Pieces

- **`src/db/queries.rs`** — new function `upsert_spoken_status`:

  ```rust
  pub fn upsert_spoken_status(
      conn: &Connection,
      line_mapping_id: i64,
      media_id: i64,
      is_spoken: bool,
  ) -> Result<(), rusqlite::Error> {
      conn.execute(
          "INSERT INTO line_spoken_status \
           (line_mapping_id, media_id, is_spoken, confidence) \
           VALUES (?1, ?2, ?3, 1.0) \
           ON CONFLICT(line_mapping_id, media_id) \
           DO UPDATE SET is_spoken = ?3, confidence = 1.0",
          rusqlite::params![line_mapping_id, media_id, is_spoken as i64],
      )?;
      Ok(())
  }
  ```

- **`src/input/timestamps.rs`** — in `set_start_time`, after the existing
  `upsert_start_time` call returns Ok, call `upsert_spoken_status(&conn,
  line.id, media_id, true)` (log on error, do not abort the timestamp write).
  Set `line.is_spoken = Some(true)` in the in-memory update block.

### Out of scope (Half 1)

- No new keybind; no `keymap.json` change.
- No sign-column glyph for spoken status (the timestamp glyph already shows the
  `u` press).
- No toggle-to-not-spoken in linux-lit.

## Half 2 — whisper-transcript: `--spoken-no-ts` targeted pass

### Behavior

A new flag on `bin/align-forced`:

```bash
.venv/bin/python bin/align-forced \
    --work <WORK_ABBREV> --media-id <MEDIA_ID> \
    --media-path <full-media-path> \
    --strategy aberrant --spoken-no-ts
```

When `--spoken-no-ts` is set, the run is **non-destructive and scoped**:

1. **Line set filter.** Restrict `sentences` to lines where
   `line_spoken_status.is_spoken = 1` AND the line currently has no
   *whisper* timestamp — i.e. either no `line_timestamps` row, or a row whose
   `source = 'manual'`. Lines already aligned by a whisper source
   (`whisper-align*`) are excluded so good timestamps are never disturbed.

2. **Manual ts as per-line anchor.** For each filtered line that has a manual
   `line_timestamps` row, use its `start_time` as an anchor fed into the
   existing windowed matcher (`map_words_to_lines_windowed`) — the same
   mechanism `is_chapter` / `is_scene_start` anchors already drive in
   `cmd_aberrant`. This scopes the search to the audio neighborhood of the
   human's mark. Lines flagged spoken but with no manual ts (none expected from
   the `u` workflow, but handled) fall back to global matching.

3. **±1.0s overwrite guard.** When writing a result for a line that has a
   manual timestamp, overwrite it only if the aligned `start_time` is within
   1.0s of the manual mark. Otherwise keep the manual row untouched and count
   it as "kept (manual, no close match)". Lines with no prior timestamp are
   written normally.

4. **No global delete.** Unlike the standard Step 6 flow, `--spoken-no-ts` never
   runs `DELETE FROM line_timestamps`. Existing whisper timestamps elsewhere are
   preserved.

### Implementation notes

- The filter reuses `load_sentences` then narrows by querying
  `line_spoken_status` (`is_spoken=1`) and `line_timestamps` (absent or
  `source='manual'`) for `media_id`.
- The anchor list is built the same way `cmd_aberrant` builds `anchors` today,
  but from the per-line manual timestamps of the filtered set rather than only
  `is_chapter`/`is_scene_start` rows.
- The ±1.0s guard lives in the write path. Prefer threading a
  `manual_anchor_by_lm_id: dict[int, float]` + `tolerance=1.0` into
  `write_results` (extend its signature) rather than forking a second writer;
  when a result's `lm_id` is in that dict and the existing row is `manual`,
  apply the tolerance test before overwriting.
- `--spoken-no-ts` implies preserving non-flagged manual rows (it already
  excludes whisper-aligned lines from the set). It composes with
  `--strategy aberrant`.

### Dual-media

Writes go to the priority-20 `media_id` (the `.mkv`, per wizard convention) and
the SQLite mirror triggers propagate to the `.m4b` twin — same as the standard
alignment pass. No mirror code in the script.

### Out of scope (Half 2)

- No separate standalone script — the logic lives in `align-forced`.
- No change to the default (non-`--spoken-no-ts`) alignment behavior.

## Wizard documentation

Add a step to `~/utono/litdb/.claude/skills/wizard-ambrose/SKILL.md` — a
"Step 6.6: Fill manually-flagged lines" after Step 6.5 — documenting:

- When to use it (after reading a work and marking missed lines with `u` in
  linux-lit).
- The `--spoken-no-ts` re-run command (against `<MKV_MEDIA_ID>` for Ambrose).
- That it is non-destructive and applies the ±1.0s anchor rule.
- Re-run the spoken-status update (Step 5) is **not** required afterward, since
  the flagged lines are already `is_spoken=1`; but note that any line that gets
  a fresh whisper timestamp should not be re-marked not-spoken by a later Step 6
  step-5 sweep (the sweep only marks lines with *no* timestamp as not-spoken, so
  newly-filled lines are safe).

## Testing

- **linux-lit:** `cargo build` + `cargo clippy`. Manual: press `u` on a line,
  confirm a `line_spoken_status` row with `is_spoken=1` appears for the active
  `media_id` (and its dual-media twin via trigger). `cargo test` for any query
  unit coverage added.
- **whisper-transcript:** dry-run `--spoken-no-ts --dry-run` on a work with a
  known missed line that has been flagged in linux-lit; confirm only the
  flagged line is in the candidate set, and that the ±1.0s guard keeps vs.
  overwrites as expected. Then a live run and verify the new timestamp.

## Files touched

- `src/db/queries.rs` — `upsert_spoken_status`
- `src/input/timestamps.rs` — call it from `set_start_time`
- `whisper-transcript/bin/align-forced` — `--spoken-no-ts` flag, line-set
  filter, anchor build
- `whisper-transcript/whisper_transcriber/alignment_io.py` — `write_results`
  tolerance/anchor params
- `~/utono/litdb/.claude/skills/wizard-ambrose/SKILL.md` — Step 6.6
