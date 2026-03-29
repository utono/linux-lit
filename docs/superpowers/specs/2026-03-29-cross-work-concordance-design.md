# Cross-Work Concordance Mode

**Created**: 2026-03-29T09:48:36Z
**Status**: Design approved

## Goal

Add a concordance mode to linux-lit that lets the user pick a vocabulary word and navigate through all its occurrences across every work in `lit.db`. Each jump loads the full work, positions the cursor on the matching line, and syncs audio if available. A background pre-load makes cross-work jumps feel instant.

## Entry Points

- **Ctrl+Shift+p** -- opens concordance word picker (fuzzy-filtered vocab word list)
- **Ctrl+Alt+p** -- opens occurrence list picker (all hits for the active concordance word)

## Data Model

### ConcordanceState

New struct added to `AppState`. `None` when concordance mode is inactive.

```rust
struct ConcordanceState {
    word: String,
    occurrences: Vec<ConcordanceHit>,
    current_index: usize,
    preloaded_work: Option<PreloadedWork>,
}

struct ConcordanceHit {
    work_abbrev: String,
    work_title: String,
    author: String,
    line_mapping_id: i64,
    div1: i64,
    div2: i64,
    line_in_div: i64,
    canonical_text: String,
    has_audio: bool,
}

struct PreloadedWork {
    work_abbrev: String,
    // Same data as a loaded work: lines, timestamps, media_paths, media_id
}
```

### Database Query

Single query to populate the occurrence list, run once on word selection:

```sql
SELECT lm.id, lm.work_abbrev, w.title, w.author,
       lm.div1, lm.div2, lm.line_in_div, lm.canonical_text,
       EXISTS(
           SELECT 1 FROM line_timestamps lt WHERE lt.line_mapping_id = lm.id
       ) AS has_audio
FROM line_mapping lm
JOIN works w ON w.abbrev = lm.work_abbrev
WHERE lm.normalized_text LIKE ?
ORDER BY w.author, lm.work_abbrev, lm.div1, COALESCE(lm.div2, 0), lm.line_in_div
```

Parameter: `%{word_lower}%`

## Key Bindings

### Entering concordance mode

- **Ctrl+Shift+p** -- opens concordance word picker
  - Fuzzy-filtered list of all words from `vocab_words` table
  - Reuses the same overlay pattern as `library_picker.rs` (text entry + scrollable list)
  - On selection: run cross-work query, populate `ConcordanceState`, load first occurrence's work, position cursor on the line, show status bar

### Navigating within concordance mode

- **r** -- next occurrence. If same work, moves cursor. If different work, swaps in preloaded work (or loads synchronously if preload missed), positions cursor, kicks off preload of the next-next work.
- **R** -- previous occurrence. Same logic, preloads in the reverse direction.
- **Ctrl+Alt+p** -- opens occurrence list picker
  - Each row: `"Author, Title -- '...snippet with word...'"` with line reference
  - Arrow keys + Enter to select
  - On selection: load work if needed, position cursor, update index

### Exiting concordance mode

- **Escape** -- clears `ConcordanceState`, r/R revert to within-work vocab navigation, status bar disappears
- **Ctrl+p** (library picker) -- also clears concordance mode
- **Ctrl+Shift+p** -- starting a new concordance replaces the old one

### Existing r/R behavior

When concordance mode is **inactive**, r/R retain their current behavior: navigate to next/previous vocab word occurrence within the current work.

When concordance mode is **active**, r/R navigate the cross-work occurrence list instead.

## Status Bar

Bottom-center bar, persistent while concordance mode is active. Vim-style with three segments:

```
concordance: disapprobation          [3/13] Boswell, Life of Johnson          r/R: next/prev | Esc: exit
```

- **Left**: `concordance: {word}` (word highlighted in accent color)
- **Center**: `[{index}/{total}] {author}, {title}` (position highlighted in secondary color)
- **Right**: `r/R: next/prev | Esc: exit` (dimmed)

The bar uses `#3c3836` background (gruvbox dark) or equivalent from the active theme. Updates on every r/R jump. Disappears when concordance mode is exited.

**Theme integration**: The bar colors should derive from the active theme's palette, same as other UI elements. Use the existing theme color accessors.

## Pre-loading

### Trigger

After every `r/R` jump, check if the next occurrence (in the direction of travel) is in a different work. If so, dispatch a background task on the Tokio thread to load that work's full data (line_mapping, line_timestamps, media_files).

### Storage

Store the result in `ConcordanceState.preloaded_work`. Only one preload at a time.

### On cross-work jump

1. If `preloaded_work` matches the target `work_abbrev`, swap it in instantly
2. If preload hasn't finished or doesn't match (user changed direction), fall back to synchronous load
3. After the swap, kick off a new preload for the next-next work

### Direction changes

If the user presses `R` (reverse) after `r` (forward), the existing forward preload is stale. Ignore it and start a reverse preload for the previous-previous occurrence's work.

## Concordance Word Picker (Ctrl+Shift+p)

GTK overlay matching the existing `library_picker.rs` pattern:
- Text entry at top for fuzzy filtering
- Scrollable list below showing matching vocab words
- Source: `SELECT word FROM vocab_words ORDER BY word`
- Arrow keys to navigate, Enter to select, Escape to dismiss
- On selection: populate `ConcordanceState` and jump to first occurrence

## Occurrence List Picker (Ctrl+Alt+p)

GTK overlay matching the same pattern:
- Scrollable list of all occurrences for the active word
- Each row formatted as: `"{Author}, {Title} -- '{canonical_text_snippet}'"` with `[{div1}.{line_in_div}]` reference
- Current occurrence highlighted
- Arrow keys to navigate, Enter to jump, Escape to dismiss without changing position
- No fuzzy filter (list is typically 5-50 items)

## Session Behavior

- **Ephemeral**: concordance mode is not persisted to config. Quitting linux-lit clears the state.
- **Work switch**: loading a work via Ctrl+p clears concordance mode.
- **Audio**: when jumping to a work with audio (`has_audio=true`), the media file loads and audio sync activates as normal. Pressing `a` plays the current line.

## Implementation Scope

### New files
- `src/ui/concordance_word_picker.rs` -- word picker overlay
- `src/ui/concordance_list_picker.rs` -- occurrence list overlay
- `src/ui/concordance_bar.rs` -- status bar widget
- `src/db/concordance.rs` -- cross-work occurrence query

### Modified files
- `src/app.rs` -- add `ConcordanceState` to `AppState`, status bar to window
- `src/input/keymap.rs` -- Ctrl+Shift+p, Ctrl+Alt+p bindings, r/R concordance branching
- `src/input/navigation.rs` -- concordance jump logic with preload
- `src/db/mod.rs` -- add concordance module
- `src/ui/mod.rs` -- add new UI modules
- `src/main.rs` -- preload task on Tokio thread

### Not in scope
- Playing compilation `.m4b` files (the compilations from `extract-word-audio` are independent)
- Persisting concordance state across sessions
- Filtering occurrences by work or author
- Editing or creating vocab entries from concordance mode
