# Phrase-Level WhisperSync Highlighting

**Date:** 2026-03-30
**Status:** Approved

## Problem

Sentence highlighting requires expensive LLM-based sentence detection that produces errors needing post-processing fixes. The sentence boundaries are also coarse — long sentences spanning many lines don't give the reader a clear sense of where the audio is.

## Goal

Highlight groups of 3-5 words (phrases) as they are spoken during audio playback, using existing whisperX word-level timestamps. All text stays normal brightness; only the active phrase gets a semi-transparent background highlight. When playback is inactive or no phrase data exists, fall back to the existing sentence dim/undim model.

## Database Schema

New table:

```sql
CREATE TABLE phrase_timestamps (
    id INTEGER PRIMARY KEY,
    line_mapping_id INTEGER NOT NULL,
    media_id INTEGER NOT NULL,
    start_time REAL NOT NULL,
    end_time REAL NOT NULL,
    start_char INTEGER NOT NULL,
    end_char INTEGER NOT NULL,
    FOREIGN KEY (line_mapping_id) REFERENCES line_mapping(id),
    FOREIGN KEY (media_id) REFERENCES media_files(id)
);
```

- Each row is one phrase on one line. `start_char`/`end_char` are character offsets into the line's `canonical_text`.
- A phrase that would cross a line break is split into two rows sharing the same `start_time`/`end_time`.
- Rows are queried sorted by `start_time` for binary search during playback.

## Import Script

New script `build_phrase_timestamps.py` in `~/utono/litdb/scripts/`:

1. Loads whisperX JSON (word-level `start`/`end`/`word` data) and Gutenberg text from `line_mapping`.
2. Aligns whisperX words to Gutenberg text at word granularity using difflib, extending the approach from `map_gutenberg_timestamps.py`. Each aligned word gets a character offset within its `line_mapping` row's `canonical_text`.
3. Groups aligned words into phrases:
   - Break at punctuation: commas, semicolons, colons, periods, dashes, open/close quotes.
   - Break on silence: gap between consecutive words exceeds ~0.3s.
   - Cap at ~5 words max even without punctuation or silence.
4. Writes phrase rows to `phrase_timestamps`.

## Rust Data Structures

New struct in `src/db/models.rs`:

```rust
pub struct Phrase {
    pub line_id: i64,
    pub start_time: f64,
    pub end_time: f64,
    pub start_char: usize,
    pub end_char: usize,
}
```

Loaded in `src/db/queries.rs` as `Work.phrases: Vec<Phrase>`, sorted by `start_time`. Only loaded when a `media_id` is active.

## Playback Highlighting

New `phrase_tag` TextTag with a semi-transparent background color (from theme).

On each MPV `time_pos` event:

1. Binary search `phrases` for the current time to find the active phrase.
2. If phrase changed since last update: remove `phrase_tag` from old position, apply to new position using `buffer.iter_at_line(line)` + `set_line_offset(start_char/end_char)`.
3. No dim tag applied during phrase playback — all text stays normal foreground.

When playback stops or pauses: remove `phrase_tag`, revert to sentence highlighting (dim model).

When no phrase data exists for the current work: use the existing sentence highlighting behavior unchanged.

## Files Changed

- **`~/utono/litdb/scripts/build_phrase_timestamps.py`** — New: word alignment, phrase grouping, DB write.
- **`src/db/models.rs`** — Add `Phrase` struct.
- **`src/db/queries.rs`** — Load `phrase_timestamps` into `Work.phrases`.
- **`src/app.rs`** — Add `phrase_tag`, track `current_phrase: Option<usize>` in AppState.
- **`src/input/navigation.rs`** — `update_highlight`: when phrases active, apply `phrase_tag` instead of dim model.
- **`src/main.rs`** — CursorSync handler: binary search phrases, update phrase highlight.
- **Wizard-gutenberg skill** — New step calling `build_phrase_timestamps.py`.
