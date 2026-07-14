# Ollama Sentence Boundary Detection Design

**Date:** 2026-03-29
**Status:** Approved

## Problem

Prose works have hard line breaks at ~72 characters. Sentences span multiple lines, and the current text-heuristic approach in `build_sentence_groups` (`text_file_map.rs`) uses punctuation patterns to detect boundaries. This fails for mid-line sentence breaks (e.g., "...fog. On such an afternoon") and edge cases like abbreviations. The heuristic is fragile and produces incorrect groupings for complex prose like Dickens.

## Solution

Replace the text heuristic with Ollama-powered sentence boundary detection. A batch preprocessing script sends paragraphs to Ollama (`qwen2.5:7b`), which identifies where sentences begin. Results are stored in the existing `sentence_start_time` / `sentence_end_time` columns of `line_timestamps`. The Rust app reads these DB values instead of running text heuristics.

## Scope

- New Python script `detect_sentences.py` in `~/utono/litdb/scripts/`
- New Step 8 in wizard-gutenberg workflow
- Rust app changes: load sentence times from DB, build `sentence_groups` from them
- Text heuristic remains as fallback when DB has no sentence data

## Script: detect_sentences.py

### Input

- `work_abbrev` (required)
- `--media-id` (optional, defaults to active media)
- `--endpoint` (optional, defaults to `http://localhost:11434`)
- `--model` (optional, defaults to `qwen2.5:7b`)
- `--dry-run` (preview boundaries without writing to DB)

### Algorithm

1. Load all `line_mapping` rows for the work, ordered by `div1`, `div2`, `line_in_div`
2. Group into paragraphs (contiguous non-blank lines, split on blank `canonical_text`)
3. For each paragraph, send numbered lines to Ollama:

```
System: Given numbered lines from a paragraph of literary text, identify
where each sentence begins. Output ONLY the line numbers where a new
sentence starts, one per line. Line 1 always starts a sentence.

User:
1: lantern in the roof, where he can see nothing but fog. On such an
2: afternoon some score of members of the High Court of Chancery bar
3: ought to be—as here they are—mistily engaged in one of the ten
...
7: making a pretence of equity with serious faces, as players might. On
8: such an afternoon the various solicitors in the cause, some two or
```

Expected response:
```
1
1
7
```

4. Parse response: extract integers, filter to valid range (1..=paragraph line count)
5. Duplicate line numbers mean the line ends the previous sentence AND starts a new one (shared-line boundary)
6. Map sentence groups back to `line_mapping` IDs
7. For each sentence group, look up `start_time` of the first line and `end_time` of the last line from `line_timestamps`
8. Write `sentence_start_time` and `sentence_end_time` to all `line_timestamps` rows in the group

### Error Handling

- If Ollama returns non-numeric lines or out-of-range numbers, skip that paragraph and log a warning
- If Ollama is not running, exit with a clear error message
- Idempotent: re-running overwrites existing `sentence_start_time` / `sentence_end_time` values
- Single-line paragraphs skip Ollama (trivially one sentence)
- Progress output: print paragraph count and current progress

### Dry-Run Mode

With `--dry-run`, print each paragraph's detected sentence groups without writing to the DB. Format:

```
Paragraph 42 (lines 1205-1230):
  Sentence 1: lines 1205-1212
  Sentence 2: lines 1212-1218
  Sentence 3: lines 1218-1230
```

## Database Changes

No schema changes. Uses existing columns on `line_timestamps`:

- `sentence_start_time REAL` — set to `start_time` of the first line in the sentence group
- `sentence_end_time REAL` — set to `end_time` of the last line in the sentence group

All lines in a sentence group share the same `sentence_start_time` / `sentence_end_time` values.

## Rust App Changes

### queries.rs

Add `sentence_start_time` and `sentence_end_time` to the `line_timestamps` query in `load_work`. Expose on the `Line` model or as a separate lookup.

### text_file_map.rs

In `build_line_map`, after constructing the buffer-to-work mapping:

1. Check if `sentence_start_time` data exists for this work's lines
2. If yes: build `sentence_groups` by grouping consecutive buffer lines that share the same `sentence_start_time` value
3. If no: fall back to `build_sentence_groups` (text heuristic)

This means the text heuristic is only used for works that haven't been processed by `detect_sentences.py`.

## Wizard-Gutenberg Integration

New **Step 8: Detect sentence boundaries** after Step 7 (populate spoken status):

```
python3 ~/utono/litdb/scripts/detect_sentences.py WORK_ABBREV
```

Optional — skippable if Ollama isn't running. The wizard should check Ollama availability and offer to skip.

## Files Modified

- `~/utono/litdb/scripts/detect_sentences.py` — New script
- `~/utono/litdb/.claude/skills/wizard-gutenberg/SKILL.md` — Add Step 8
- `~/utono/linux-lit/src/db/queries.rs` — Load sentence times
- `~/utono/linux-lit/src/db/models.rs` — Add sentence time fields to Line or TimeRange
- `~/utono/linux-lit/src/text_file_map.rs` — Build sentence_groups from DB data when available
