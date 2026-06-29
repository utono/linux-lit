# Source-block cursor + media playback in the gloss overlay

**Date:** 2026-06-08
**Status:** Design approved, ready for implementation plan
**Builds on:** `2026-06-08-gloss-tts-read-aloud-design.md` (the paragraph cursor + TTS feature, already merged)

## Goal

Make the gloss-overlay accent-bar cursor stop on **every block**, not just
explication paragraphs. A **source block** (the quoted `<speaker>`/`<verse>`
passage) becomes a cursor stop alongside each explication paragraph. When the
cursor is on a source block and the reader presses **Space**:

- If the media file is available and the block's first quoted line has a
  timestamp → **seek the media to that start time and play** (`ResumeAndSeek`),
  with no end enforcement.
- Otherwise (prose work with no per-line timing, MPV not connected, or a
  cross-work gloss whose lines aren't in the current work) → **fall back to
  TTS** of the block's verse-line text, cached and played like an explication
  paragraph.

Explication-paragraph behavior (Space → TTS) is unchanged.

## Why both branches

- **Verse works (plays):** almost every quoted `<verse>` line has a media
  timestamp, so Space plays the recorded audio of the passage. This is the
  common case.
- **Prose works (novels):** the quoted source comes through as the same
  `<speaker>`/`<verse>` tags, but the underlying prose lines often lack per-line
  timestamps. Those blocks fall back to TTS automatically — the timing-presence
  check routes verse→media and untimed→TTS with no structural difference.

## Non-goals (YAGNI)

- No end-time enforcement, no pause-at-end, no loop. Space on a source block
  just seeks + plays (verse) or plays a TTS clip (prose). The earlier
  "play to end / start+5s fallback" idea is dropped per the approved design.
- No speaker name in the TTS — verse lines only.
- No new keys; the existing scroll keys (`j/k/gg/G`) move the cursor as today.
- No change to echo glosses (they use a different overlay path,
  `show_echoes`, where the block cursor is inert / cleared).

## User-facing behavior

1. Open a gloss overlay. The accent bar marks the block nearest the viewport
   center. As the reader scrolls, the bar moves onto the source passage (the
   CRANMER verse), then onto explication paragraph 1, then 2, etc. — each block
   is a stop, in document order.
2. **Space on an explication block** → reads that paragraph via TTS (unchanged).
3. **Space on a source block**:
   - media available + first line timestamped → seek + play the recording.
   - else → synthesize the verse text via ElevenLabs (cached), play it.

## Component design

### 1. Generalize the block cursor (`src/ui/gloss_overlay.rs`)

A gloss is a flat `Vec<GlossElement>` of `Speaker | Verse | Gloss`. Define a
**block** as:

- **Source block** — a maximal contiguous run of `Speaker`/`Verse` elements.
- **Explication block** — a non-echo `Gloss` element (`split_echo` returns
  `None`).

(Echo `Gloss` elements are excluded, matching the current
`explication_paragraphs`.)

**Replace** the explication-only machinery with block-level equivalents:

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BlockKind { Source, Explication }

/// One cursor stop in the gloss, in document order.
pub struct GlossBlock {
    pub kind: BlockKind,
    /// 0-based index WITHIN its kind (source blocks numbered separately from
    /// explication paragraphs).
    pub index: i32,
    /// For Source: the joined verse-line text (speaker labels excluded).
    /// For Explication: the paragraph prose.
    pub text: String,
}

/// Parse a gloss into ordered cursor-stop blocks.
pub fn gloss_blocks(gloss: &str) -> Vec<GlossBlock>;
```

- `gloss_blocks` walks `parse_gloss_tags`: accumulates consecutive
  `Speaker`/`Verse` into a pending source block (verse texts joined by `\n`,
  speaker names dropped), flushing it as a `Source` block when a non-echo
  `Gloss` (Explication) or end-of-stream is reached; each non-echo `Gloss`
  becomes an `Explication` block. Source and explication indices increment
  independently.

Replace `struct ParaRange` with:

```rust
struct BlockRange {
    kind: BlockKind,
    index: i32,
    start_line: i32,
    end_line: i32,
}
```

- `explication_paras` field → `blocks: Rc<RefCell<Vec<BlockRange>>>`.
- `rebuild_explication_ranges` → `rebuild_block_ranges(&self, gloss: &str)`:
  for each `GlossBlock`, locate its buffer line span. An explication block is
  one logical buffer line (as today). A **source block** spans from its first
  speaker/verse buffer line to its last verse buffer line — found by scanning
  for the block's first verse line text, then extending to its last verse line
  text (reuse the existing forward-scan-from-`search_from` approach; for source
  blocks, `end_line` is the last verse line's buffer line, not equal to
  `start_line`).
- `current_explication_para()` → `current_block(&self) -> Option<(BlockKind, i32)>`:
  the block whose mid-y is nearest the viewport center (same center math as
  today), returning its kind + index.
- `mark_cursor_paragraph()` → `mark_cursor_block()`: set `bar_ranges` to the
  current block's `[start_line, end_line]` and `queue_draw`. Called from the
  same sites (`scroll_gloss`, `scroll_gloss_to_top`, `scroll_gloss_to_bottom`,
  end of `show_gloss_with_color`).
- Cleared in `show_echoes`/`show_synopsis`/`show_glossing`/`show_loading_message`
  (rename the existing `explication_paras.clear()` calls to `blocks`).

`gloss_blocks` keeps a `pub` helper `explication_paragraphs`-equivalent only if
still needed by the action layer; otherwise the action layer calls
`gloss_blocks` directly (see §3).

### 2. Storage: add a `kind` to the audio cache (`src/db/queries.rs`)

The source-block TTS fallback caches one MP3 per source block, keyed distinctly
from explication paragraphs. The existing table is:

```sql
gloss_audio(id, gloss_id, paragraph_index, audio_path, voice_id, model_id,
            timestamp, UNIQUE(gloss_id, paragraph_index))
```

The original `UNIQUE(gloss_id, paragraph_index)` is column-level and **would
reject a source row whose index collides with an existing explication row's
index** (both can be `0`). SQLite cannot drop a column-level constraint with
`ALTER`, so the correct migration is a **one-time guarded table rebuild** to the
new shape with `UNIQUE(gloss_id, kind, paragraph_index)`.

`ensure_gloss_audio_table` does:

1. `CREATE TABLE IF NOT EXISTS gloss_audio (...)` with the **new** shape (so
   fresh installs get it directly):

```sql
CREATE TABLE IF NOT EXISTS gloss_audio (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    gloss_id        INTEGER NOT NULL REFERENCES glosses(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL DEFAULT 'explication',
    paragraph_index INTEGER NOT NULL,
    audio_path      TEXT NOT NULL,
    voice_id        TEXT NOT NULL,
    model_id        TEXT NOT NULL,
    timestamp       DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(gloss_id, kind, paragraph_index)
);
CREATE INDEX IF NOT EXISTS idx_gloss_audio_gloss_id ON gloss_audio(gloss_id);
```

2. **Detect the legacy shape and rebuild if needed.** Check whether the `kind`
   column exists:

```rust
let has_kind: bool = conn
    .prepare("SELECT 1 FROM pragma_table_info('gloss_audio') WHERE name = 'kind'")?
    .exists([])?;
if !has_kind {
    conn.execute_batch(
        "BEGIN;
         ALTER TABLE gloss_audio RENAME TO gloss_audio_old;
         CREATE TABLE gloss_audio (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            gloss_id INTEGER NOT NULL REFERENCES glosses(id) ON DELETE CASCADE,
            kind TEXT NOT NULL DEFAULT 'explication',
            paragraph_index INTEGER NOT NULL,
            audio_path TEXT NOT NULL,
            voice_id TEXT NOT NULL,
            model_id TEXT NOT NULL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(gloss_id, kind, paragraph_index)
         );
         INSERT INTO gloss_audio (id, gloss_id, kind, paragraph_index, audio_path, voice_id, model_id, timestamp)
            SELECT id, gloss_id, 'explication', paragraph_index, audio_path, voice_id, model_id, timestamp
            FROM gloss_audio_old;
         DROP TABLE gloss_audio_old;
         CREATE INDEX IF NOT EXISTS idx_gloss_audio_gloss_id ON gloss_audio(gloss_id);
         COMMIT;",
    )?;
}
```

The rebuild runs once: after it, `has_kind` is true and the block is skipped.
Existing rows are preserved and labeled `'explication'`. The `CREATE TABLE IF
NOT EXISTS` in step 1 handles the brand-new-DB case; the step-2 rebuild handles
the upgrade-from-legacy case. Both are idempotent across launches.

**Update the queries to take `kind`:**

```rust
pub fn find_gloss_audio(conn, gloss_id: i64, kind: &str, index: i64)
    -> Result<Option<String>, rusqlite::Error>;   // WHERE gloss_id=? AND kind=? AND paragraph_index=?

pub fn save_gloss_audio(conn, gloss_id: i64, kind: &str, index: i64,
    audio_path: &str, voice_id: &str, model_id: &str)
    -> Result<(), rusqlite::Error>;
    // INSERT (gloss_id, kind, paragraph_index, audio_path, voice_id, model_id)
    // ... ON CONFLICT(gloss_id, kind, paragraph_index) DO UPDATE ...
```

`delete_gloss_audio(conn, gloss_id)` is unchanged (deletes all rows for the
gloss regardless of kind).

> Implementation note: because `find`/`save` signatures change, update the
> existing explication call sites in `read_current_paragraph` to pass
> `kind = "explication"`. The roundtrip test is updated to pass `kind`.

### 3. Space behavior (`src/input/actions/gloss.rs`)

`read_current_paragraph` becomes `read_current_block` (or keep the name and
branch inside). Flow:

1. Toggle-stop: if `s.tts.is_playing()` → `s.tts.stop()`, return. (Unchanged —
   note this only stops TTS; media playback is MPV's own toggle, see step 4.)
2. Resolve the current block: `s.gloss_overlay.current_block()` →
   `Some((kind, index))` or return.
3. **Explication block** → exactly today's path: find the explication text via
   `gloss_blocks(&gloss.gloss_text)` (filter `kind==Explication`, match
   `index`), cache key `("explication", index)`, synth/cache/play.
4. **Source block**:
   a. Get the block's verse lines from `gloss_blocks` (the `Source` block at
      `index`; its `text` is the joined verse lines). Also need the per-line
      `(div1,div2,line_in_div)` of the block's first quoted line to look up
      timing — derive these the way `jump_to_gloss_source_start` does: the
      gloss's `source_line_pairs()` / `source_line_numbers` map quoted lines to
      `line_in_div`, and `current_work.lines` carries `timestamp`.
   b. **Resolve the first timestamped line:** for the block's quoted lines in
      order, find the first matching `current_work` line with a
      `Some(timestamp)`; take its `timestamp.start`.
   c. **If MPV is connected (`s.mpv_connected`) AND a start time was found** →
      `s.cmd_tx.try_send(MpvCommand::ResumeAndSeek(start))`. Done. (No end
      enforcement.)
   d. **Else (TTS fallback)** → synthesize `block.text` (verse lines only) via
      ElevenLabs, cache key `("source", index)`, file path
      `~/Music/glosses/<abbrev>/<gloss-id>/source-<index>.mp3`, play. Reuse the
      exact async cache-or-synth path from the explication case, parameterized
      by `kind` and the filename stem.

**Filename scheme:**
- Explication: `<index>.mp3` (unchanged).
- Source: `source-<index>.mp3`.

`gloss_audio_dir` is unchanged; the filename stem differs by kind.

### 4. MPV interaction details

- Source-block media play uses `MpvCommand::ResumeAndSeek(start)` — the same
  command `play_current_line` uses. It seeks the already-loaded media and
  resumes; it does not load a different file.
- This assumes the loaded media is the gloss's work. For a cross-work gloss
  (different `work_abbrev` than the loaded media), step 4b will find no matching
  `current_work` line (or the wrong one), so `start` is `None` and it falls
  through to TTS — the safe behavior. (Loading the gloss work's media for a
  cross-work source-block play is out of scope.)
- The Space toggle-stop (step 1) stops TTS only. If media is playing from a
  prior source-block Space, pressing Space again on a source block just issues
  another `ResumeAndSeek` (re-seek to start) — acceptable; pausing media is the
  reader's existing global Space-in-Reader / playback controls, not this path.

## Files touched

- `src/ui/gloss_overlay.rs` — `BlockKind`, `GlossBlock`, `gloss_blocks`,
  `BlockRange`, `blocks` field, `rebuild_block_ranges`, `current_block`,
  `mark_cursor_block`; rename clears; source-block multi-line range spanning.
- `src/db/queries.rs` — `kind` column migration + unique index in
  `ensure_gloss_audio_table`; `find_gloss_audio`/`save_gloss_audio` take `kind`;
  updated roundtrip test.
- `src/input/actions/gloss.rs` — `read_current_block` source/explication
  branch; source-block timing lookup → `ResumeAndSeek` or TTS fallback;
  `kind`-parameterized cache path + `source-<n>.mp3` filename.
- `src/input/keymap.rs` — if the Space arm calls `read_current_paragraph`,
  rename to the new entry point (no behavioral change to routing).

## Testing

Pure-logic units (no GTK/audio/MPV):

- `gloss_blocks`: given a gloss XML with interleaved source passages and
  explications (and an echo gloss mixed in), assert the ordered blocks, kinds,
  per-kind indices, and that a source block's `text` is the joined verse lines
  (speaker dropped, echoes excluded).
- `find_gloss_audio`/`save_gloss_audio` with `kind`: roundtrip + that
  `("source", 0)` and `("explication", 0)` are distinct rows for the same gloss.
- The first-timestamped-line resolution helper, if factored pure (given block
  line numbers + a slice of `(line_in_div, Option<start>)`, returns the first
  start) — assert first-with-timing wins and `None` when none have timing.

Not unit-tested (manual, per the project's headless limits): the rendered
accent bar landing on the source block, MPV seek+play, and rodio TTS playback.
The user verifies these by launching the app (verse work: bar on source →
Space plays audio; prose work without timing: bar on source → Space speaks the
quote).

## Migration / compatibility notes

- The `kind` column defaults existing rows to `'explication'`, so prior cached
  explication audio keeps resolving (the explication queries now pass
  `kind="explication"`).
- `SNAPSHOT_VERSION` is unaffected — no `LineMap` serialization change.
