# Word-Level Sentence Highlighting

**Date:** 2026-03-30
**Status:** Approved
**Scope:** Text-heuristic path only (plain text files, not DB-driven)

## Problem

Sentence highlighting currently operates at the line level: the entire buffer is dimmed, then whole lines belonging to the current sentence group are undimmed. On lines where one sentence ends and another begins mid-line (e.g., `"...end of the fog. On such an afternoon"`), both sentences are fully visible even though only one is current.

## Goal

Highlight only the words belonging to the current sentence. On boundary lines, dim the portion that belongs to the adjacent sentence and undim only the current sentence's words.

## Data Structure

Replace `Vec<Range<usize>>` with `Vec<SentenceGroup>`:

```rust
pub struct SentenceGroup {
    pub line_range: Range<usize>,  // buffer line indices
    pub start_col: usize,          // char offset on first line where sentence begins
    pub end_col: Option<usize>,    // char offset on last line where sentence ends (None = end of line)
}
```

- Most groups: `start_col: 0`, `end_col: None` (whole lines, no boundary).
- Mid-line boundary: the ending group gets `end_col: Some(split_point)`, the starting group gets `start_col: split_point`.
- DB-driven path produces whole-line groups (`start_col: 0`, `end_col: None`) since each DB line maps to one sentence. No changes to DB-driven behavior.

## Sentence Boundary Detection

`has_mid_line_sentence_boundary()` becomes `find_mid_line_sentence_boundary()`:

- **Input:** a line of text
- **Output:** `Option<usize>` — character index where the new sentence starts
- Detection logic: sentence-ending punctuation (`.!?`), optional closing quote (`"'`), space, then uppercase letter. The returned index is the position of the uppercase letter (word-level split).

Example: `"...end of the fog. On such an afternoon"` returns `Some(19)` (the `O` in `On`).

`build_sentence_groups()` uses this to populate `start_col`/`end_col` on the appropriate groups.

`ends_sentence_at_eol()` is unchanged — groups ending at line boundaries get `end_col: None`.

## Highlight Application

`update_highlight()` in `navigation.rs` changes from line-level to character-aware tag removal:

1. Dim entire buffer (unchanged).
2. For the current sentence group:
   - **First line:** undim from `start_col` to end of line.
   - **Last line:** undim from start of line to `end_col` (or end of line if `None`).
   - **Single-line group:** undim from `start_col` to `end_col`.
   - **Middle lines:** undim the whole line (unchanged).
3. Fallback: if no sentence group found, undim the current line (unchanged).

Character offsets are converted to `TextIter` positions using `buffer.iter_at_line_index(line, col)`.

## AB-Repeat and Visual Selection

Both continue to operate at line level for now. Word-level precision for these modes is deferred to a future change.

## Files Changed

- **`src/text_file_map.rs`** — `SentenceGroup` struct, `find_mid_line_sentence_boundary()`, updated `build_sentence_groups()` and `build_sentence_groups_from_db()`, updated `sentence_group_for()` and `sentence_group_index()`, updated tests.
- **`src/input/navigation.rs`** — `update_highlight()` uses `start_col`/`end_col` for character-aware tag removal.
- **`src/app.rs`** — `LineMap.sentence_groups` field type updated to `Vec<SentenceGroup>`.
- **`src/input/visual.rs`** — no changes.
