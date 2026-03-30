# Prose Sentence Highlighting Design

**Date:** 2026-03-29
**Status:** Approved

## Problem

For prose works with a `text_file` (e.g., PP-gutenberg / The Pickwick Papers), the text file is line-wrapped at ~72 characters. Each file line is one DB row with its own timestamp. The app currently highlights one buffer line at a time during audio playback, but a sentence typically spans multiple consecutive lines. The user wants to see the entire sentence highlighted (undimmed) as it's being spoken.

## Scope

**Applies to:** Prose works (`is_prose_work(work_type)` returns true) that have a `text_file`. This excludes Shakespeare, Milton, Pope, and other verse/play works.

**Does not apply to:** Works without a text_file (rendered directly from DB lines), plays, poetry, or any non-prose work type.

## Design

### Sentence Group Computation

During `build_line_map()` in `text_file_map.rs`, after constructing the line map, compute sentence groups:

- A **sentence group** is a contiguous range of buffer lines (`Range<usize>`) that form one sentence
- A sentence ends when a buffer line's text ends with sentence-terminating punctuation: `.`, `!`, `?`, optionally followed by closing quotes (`'`, `"`, `\u{2019}`, `\u{201D}`)
- Blank lines and lines not matched to any DB row act as sentence boundaries
- The last group in the buffer is closed at buffer end even without terminal punctuation
- Store as `Vec<Range<usize>>` in `LineMap` (new field: `sentence_groups`)

### Highlight Behavior

In `update_highlight()` (`navigation.rs`):

- When `AppState.line_map` is present and `sentence_groups` is non-empty, find the sentence group containing `current_line`
- Undim all lines in that sentence group instead of just `current_line`
- Fall back to single-line highlighting if no sentence group contains the current line

### MPV Sync

No changes. `CursorSync` continues to set `current_line` to a single buffer line. The highlight logic expands that to the enclosing sentence group.

### Chunk (A/B Repeat)

Chunk undimming continues to work at the line level as before — no interaction with sentence groups.

## Edge Cases

- **Dialogue in quotes spanning lines:** Sentences ending with `."` or `?'` are correctly detected by allowing closing quotes after terminal punctuation
- **Abbreviations (Mr., Dr., etc.):** May cause false sentence breaks. Acceptable for v1 — these are rare in line-wrapped text where the period rarely falls at line end
- **Ellipsis (...):** Three dots at line end would trigger a break. Acceptable for v1
- **Headers/chapter titles:** Typically on their own line without terminal punctuation; they form their own group or join the next sentence. Either behavior is acceptable

## Files Modified

- `src/text_file_map.rs` — Add `sentence_groups: Vec<Range<usize>>` to `LineMap`, compute during `build_line_map()`
- `src/input/navigation.rs` — Modify `update_highlight()` to undim sentence group when available
