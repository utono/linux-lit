# Word Cycle Copy (`w` keybind)

**Date:** 2026-04-03

## Purpose

The `w` key in normal mode cycles through words on the cursor line, copying each to the system clipboard via `wl-copy`. This supports vocab lookup workflows where the user copies a word and then adds it to lit.db from another tool.

## Behavior

- Each `w` press advances to the next word on the current line and copies it to the clipboard
- Words are whitespace-split, then leading/trailing punctuation is stripped
- After the last word, wraps back to the first
- If the cursor moves to a different line, the word index resets to 0
- A status label at bottom-left shows the copied word for 2 seconds after the last press
- Each `w` press resets the 2-second auto-hide timer

## State

Two new fields on `AppState`:

- `word_cycle_line: Option<usize>` — the line the cycle is tracking (reset when cursor moves to a different line)
- `word_cycle_index: usize` — current position in the word list for that line

## UI

A new `gtk::Label` appended to the bottom of the main vbox, below the search bar. Styled with a CSS class `word-status` for consistent theming. Hidden by default, shown on `w` press, auto-hidden 2 seconds after the last press via `glib::timeout_add_local_once`.

## Word Extraction

1. Get the text of the current cursor line from `work.lines[current_line].text`
2. Split on whitespace
3. For each token, strip leading and trailing characters that are not alphanumeric (Unicode-aware)
4. Filter out empty strings after stripping
5. Collect into a `Vec<String>`

## Clipboard

Reuse the existing `wl-copy` pattern from `src/input/visual.rs`: spawn `wl-copy`, pipe the word via stdin.

## Key Routing

Add `"w"` match arm in the normal-mode single-key dispatch section of `src/input/keymap.rs`, calling a new handler function.

## Files Modified

- `src/app.rs` — add `word_cycle_line`, `word_cycle_index` fields to AppState; create and append the status label; add auto-hide timer logic
- `src/input/keymap.rs` — add `"w"` match arm routing to handler
- `src/input/navigation.rs` — implement `word_cycle_copy()` handler (word extraction, clipboard copy, status update, timer management)
