# Concordance Cross-Work Spawn

Date: 2026-04-05

## Problem

Loading a different work's 30k+ line buffer in the same GTK instance causes blank screens due to GTK's lazy text layout. The deferred scroll workarounds are fragile and unreliable.

## Design

When a user selects a word from the concordance picker, spawn a separate linux-lit instance for each work (other than the current one) that contains the word. The current instance handles its own occurrences in-process.

## Behavior

1. User selects a word from the concordance picker (Ctrl+\ or Ctrl+Shift+P, then Return)
2. Query all occurrences across all works (filtered by `push_to_device = 1`) — existing `find_word_occurrences` query
3. Group hits by `work_abbrev`
4. **Current work has hits**: create `ConcordanceState` with only this work's hits, jump to first occurrence. `r`/`R` cycles within these hits only.
5. **Current work has no hits**: stay where you are, no concordance state change
6. **Other works with hits**: spawn one new linux-lit instance per work, passing `LINUX_LIT_WORK=<abbrev>` and `LINUX_LIT_LINE_ID=<first hit's line_mapping_id>` as environment variables
7. Spawned instances open with playback paused
8. Concordance bar shows work-scoped count (e.g., "abject 1/2")

## Changes Required

- **src/input/keymap.rs**: Both concordance picker Return handlers (line ~580 and ~640) — after querying hits, partition by work_abbrev. Build ConcordanceState from current-work hits only. Spawn new instances for other-work hits.
- **src/input/navigation.rs**: `concordance_jump_to_current` cross-work branch — already spawns a process, but this code path won't be reached anymore since cross-work hits are handled at the picker level. Can be simplified or removed.
- **src/concordance.rs**: No changes needed — `advance_within_work`/`retreat_within_work` already exist.

## Existing Infrastructure

- `LINUX_LIT_WORK` / `LINUX_LIT_LINE_ID` env vars for spawn — already implemented
- Unique app_id per concordance spawn (`com.utono.linux-lit.dev.conc.<PID>`) — already implemented
- `display_work_at` resolves `target_line_id` to buffer index — already implemented
- `find_word_occurrences` filters by `push_to_device = 1` — already implemented
- `advance_within_work` / `retreat_within_work` for r/R cycling — already implemented
