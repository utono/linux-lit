# Synopsis overlay as a gloss overlay + Shift+Space batch-synthesize

**Date:** 2026-06-09
**Status:** Approved design, pending implementation plan

## Goals

Two related changes to the linux-lit gloss/synopsis overlays
(`src/ui/gloss_overlay.rs`):

1. **Make the synopsis overlay function like the gloss overlay.** Render the
   scene synopsis as **cursor blocks** with an **accent bar to the left of the
   current block**, and move the cursor with `j`/`k` (`gg`/`G` for first/last) —
   exactly like the gloss overlay. The synopsis text already uses `<p>`
   paragraph tags in `lit.db`; each `<p>` becomes one block.

2. **Add `Shift+Space`** (active in both the gloss and synopsis overlays) that
   **synthesizes all prose blocks** of the open card to ElevenLabs MP3s and
   **caches them** (no playback), showing a persistent `"Synthesizing…"` toast
   while it runs.

## Decisions (locked)

### Synopsis-as-gloss

- **No DB re-tag, no prompt change.** Synopses stay `<p>`-tagged in
  `scene_synopses.synopsis`. A new `synopsis_blocks()` wraps each `<p>`
  paragraph as a `BlockKind::Explication` block; the synopsis overlay then runs
  through the existing block/cursor/accent-bar machinery
  (`rebuild_block_ranges` → `cursor_block` → `mark_cursor_block` → `bar_ranges`
  → Cairo draw). All 863 stored rows already use `<p>`, so nothing in the DB
  changes.
- **`j`/`k` move the cursor block** (with the accent bar), `gg`/`G` jump
  first/last — matching the gloss overlay. Scrolling follows the cursor.

### Shift+Space batch synth

- **Cache-only.** Synthesize each prose block, write + record the MP3, no
  playback.
- **Fixed plain prose voice:**
  `elevenlabs::voice_for(Gender::Unknown, /*is_verse=*/ false)`.
- **Both overlays.**
- **Synopsis cache:** a new `synopsis_audio` DB table (synopses have no
  `gloss_id`, so they can't use `gloss_audio`).
- **On error:** stop on the first failure; replace the `"Synthesizing…"` toast
  with an error toast naming the cause.

## Part 1 — Synopsis overlay as cursor blocks + accent bar

### `synopsis_blocks(synopsis: &str) -> Vec<GlossBlock>`

New pure function in `gloss_overlay.rs`. Parse `<p>...</p>` content (reuse the
same extraction `render_synopsis_with_labels` uses) and emit one
`GlossBlock { kind: Explication, index, text, display }` per paragraph, `index`
counting from 0. **Skip label paragraphs** (the existing `is_label_paragraph`
heuristic — short lines ending in `:`) so a heading like "Shakespearean
parallels:" is shown but is not a cursor stop. Fallback: if the string has no
`<p>` tags (legacy plain text), treat the whole string as one block.

### `show_synopsis` changes

Convert `show_synopsis` (currently plain scrolling text) to the block path:

1. **Stop clearing the block machinery.** Remove the `self.blocks.clear()` /
   `bar_ranges = empty` resets that currently disable the bar in synopsis mode.
2. **Set `bar_x`** to `card_width / 4` (same as gloss) and set the accent bar
   color to the **same color the gloss overlay uses** (consistency; a
   synopsis-specific color is a later tweak if it ever reads wrong).
3. **Populate the buffer** paragraph-by-paragraph from `synopsis_blocks` so the
   buffer line layout matches the block list (keep the existing label-bolding via
   `apply_synopsis_label_bold` for label paragraphs that are rendered but not
   cursor stops).
4. **`rebuild_block_ranges`** against the synopsis blocks to map each block to
   its buffer-line span, then **`mark_cursor_block()`** (+ a queued
   `bar_drawing.queue_draw()`), `cursor_block` reset to 0.
5. Keep the title (scene label), `reset_scroll_top`, `apply_font`. Update the
   `hint` to the gloss-style block hint (e.g.
   `"Esc close · j/k block · n/p scene · Shift+Space synth · A ask · U undo"`).

The clipping fix already in place (bottom-clip recompute, top row-snap) is
unaffected — block-based rendering still scrolls the same `gloss_view`.

### Key routing — `handle_synopsis_overlay_key` (keymap.rs)

- `j` → `cursor_next_block()`, `k` → `cursor_prev_block()` (replacing
  `scroll_gloss(±1)`), each calling `mark_cursor_block` +
  `scroll_cursor_into_view` (already done inside the cursor steppers).
- `gg` chord → `cursor_first_block()`, `G` → `cursor_last_block()`.
- `n`/`p` (next/prev scene) and `A` (ask) and `U` (undo) unchanged.
- `Escape` unchanged.

### Caveat — bar row mapping uses `display` matching

`rebuild_block_ranges` locates a block's buffer span by matching the block's
first display line against buffer lines. For synopsis paragraphs that is the
paragraph's first line — fine, but confirm the buffer is populated with the same
per-paragraph text the matcher searches for (don't pre-join all paragraphs into
one `set_text` blob if the matcher needs line granularity). Mirror how the gloss
path populates + matches.

## Part 2 — `Shift+Space` batch-synthesize prose blocks

### Key routing

`handle_key` (keymap.rs:42) already has `is_shift`; the spacebar guard
(keymap.rs:73) lets `Shift+Space` (`key_name == "space"`, `is_shift == true`)
fall through to mode dispatch. Forward `is_shift` into `handle_gloss_key` and
`handle_synopsis_overlay_key` (both currently omit it), and add before each
`match key_name`:

```rust
if key_name == "space" && is_shift {
    // dispatch batch synth for this overlay
    return true;
}
```

Plain `space` keeps its behavior (`read_current_block` in gloss; in synopsis,
plain space can play the current block once synopsis_audio exists — optional,
out of scope here). No new `Action` enum variant.

### Shared async + toast skeleton (in `src/input/actions/gloss.rs`)

One `glib::spawn_future_local` task per invocation; iterate blocks
**sequentially**, awaiting `tokio_handle.spawn(synthesize…)` each (avoids the
ElevenLabs rate limit; matches `play_block_tts`). Reuse:

- `show_persistent_tts_toast(state_rc, "Synthesizing…")` before the loop.
- `hide_tts_toast(state_rc)` after the last block.
- First `Err(e)` → `show_tts_toast(state_rc, &format!("Synthesis failed: {e}"))`
  and **return** (stop the batch).

Re-entrancy guard: `pub tts_batch_running: Cell<bool>` on `AppState`; set true at
task start, clear in every exit path; if already true the press is a no-op.

Text sent to ElevenLabs is `crate::ui::gloss_overlay::ipa_for_tts(&text)` per
block (same as `play_block_tts`).

### Gloss overlay batch — `synth_all_prose_blocks(state_rc)`

Reuses the existing cache infra. Prose = `gloss_blocks(&gloss.gloss_text)`
filtered to `kind == Explication`. Per block: `find_gloss_audio(conn, gloss_id,
"explication", index, voice_id)` (+ Alice fallback) → skip if present; else
`synthesize(...)` → write
`~/Music/glosses/<abbrev>/<gloss_id>/<index>-<voice_tag>.mp3` →
`save_gloss_audio(...)`. No schema change.

### Synopsis overlay batch — `synth_all_synopsis_blocks(state_rc)`

Prose = `synopsis_blocks(&synopsis)` (the same blocks the cursor navigates), so
the cache index matches the on-screen block. Key the cache on
`(work_abbrev, div1, div2, block_index, voice_id)`.

**New table `synopsis_audio`** (mirrors `gloss_audio`, lazy
`ensure_synopsis_audio_table` with `CREATE TABLE IF NOT EXISTS` + index, called
at synth time — NOT a `user_version` migration, NOT a `SNAPSHOT_VERSION` bump):

```
id              INTEGER PRIMARY KEY AUTOINCREMENT,
work_abbrev     TEXT NOT NULL,
div1            INTEGER NOT NULL,
div2            INTEGER NOT NULL,
paragraph_index INTEGER NOT NULL,
audio_path      TEXT NOT NULL,
voice_id        TEXT NOT NULL,
model_id        TEXT NOT NULL,
timestamp       DATETIME DEFAULT CURRENT_TIMESTAMP,
UNIQUE(work_abbrev, div1, div2, paragraph_index, voice_id)
```

Add `find_synopsis_audio(...)` / `save_synopsis_audio(...)` mirroring the gloss
equivalents. Per block: cache check → miss → `synthesize(ipa_for_tts(&text),
voice_id, model_id)` → write
`~/Music/synopses/<abbrev>/<div1>-<div2>/<index>-<voice_tag>.mp3` →
`save_synopsis_audio(...)`.

## Files touched

- `src/ui/gloss_overlay.rs` — `synopsis_blocks()`; rework `show_synopsis` onto
  the block/cursor/accent-bar path; synopsis bar color/x.
- `src/input/keymap.rs` — `handle_synopsis_overlay_key`: `j`/`k` → cursor
  blocks, `gg`/`G` first/last; forward `is_shift` into both overlay handlers;
  `Shift+Space` dispatch in each.
- `src/input/actions/gloss.rs` — `synth_all_prose_blocks`,
  `synth_all_synopsis_blocks`, `tts_batch_running` guard, shared toast/async
  skeleton.
- `src/db/queries.rs` — `SYNOPSIS_AUDIO_COLUMNS`,
  `ensure_synopsis_audio_table`, `find_synopsis_audio`, `save_synopsis_audio`.
- `src/app.rs` — `tts_batch_running: Cell<bool>` on `AppState` + initializer.
- `src/ui/keybinds_overlay.rs` — add `Shift+Space` (and the synopsis-overlay
  `j`/`k` block-nav note) to the Ctrl+/ overlay (cap + `describe()` arm) via the
  `update-cairo-keybinds-overlay` skill.
- `keymap.json` / `keymap_config.rs` — **N/A**: overlay keys bypass the `Action`
  enum / `keymap.json` (confirm during implementation).

## Testing

- `cargo build`, `cargo test --bins`, `cargo clippy`.
- Pure-logic units: `synopsis_blocks` (each `<p>` → one Explication block, label
  paragraphs skipped, indices contiguous, legacy no-`<p>` fallback); the
  `synopsis_audio` cache round-trip (`ensure` → `save` → `find`) on a temp DB.
- **Runtime-only (ask the user, per CLAUDE.md "do not run the app"):**
  - Synopsis overlay renders the accent bar on the current block and `j`/`k`
    step it (screenshot via `h` then `j`). This is a rendered-pixel change —
    the criterion is "the bar draws on the right block," so it needs the
    headless `e2e-env.sh` flow or the user's eyes.
  - `Shift+Space` in both overlays with a valid `ELEVENLABS_API_KEY`: toast
    appears/dismisses, MP3s land under `~/Music/glosses/…` and
    `~/Music/synopses/…`, error path shows the failure toast.

## Out of scope (YAGNI)

- Playback of the batch (cache-only by decision).
- Verse/source-block synthesis (prose only).
- Per-gloss voice override for the batch (fixed voice).
- Re-tagging `lit.db` synopses to `<gloss>` tags (rejected in favor of the
  `<p>`-as-Explication approach).
- Plain-`Space` single-block play in the synopsis overlay (possible follow-on
  once `synopsis_audio` exists, but not required here).
- Per-block progress counter in the toast.
