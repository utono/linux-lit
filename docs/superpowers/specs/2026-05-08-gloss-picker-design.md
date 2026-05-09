# Gloss Picker Design

**Date:** 2026-05-08

## Summary

Alt+g opens a fuzzy-filterable picker listing all glossed passages in the
currently loaded work. Each row shows `SPEAKER: first line of source_text`
with the citation right-aligned. Selecting a passage navigates to it and
opens the gloss overlay.

## Scope

- Current work only (no cross-work browsing)
- Reuses existing `find_glossed_passages(conn, work_abbrev)` query — no new SQL
- Follows the `BookmarkPicker` flat-list pattern exactly

## Widget: `GlossPicker`

**File:** `src/ui/gloss_picker.rs`

**Struct fields:**

- `overlay: Overlay`
- `picker_box: GtkBox`
- `search_entry: Entry`
- `list_box: ListBox`
- `items: Vec<GlossedPassage>`

**Dimensions:** 600x400, CSS class `library-picker` (reuses existing styling).

**Search entry placeholder:** "Filter glosses..."

**Row layout:** HBox with two labels:

- Left label: `SPEAKER: <first line of source_text>` — ellipsized, expands
  horizontally
- Right label: `start_citation` (e.g. `Ham.1.2.1`) — dimmed style, right-aligned

**Row identity:** `widget_name` stores the index into `self.items` as a string.

**Fuzzy filtering:** `subsequence_match` on `"{speaker} {source_text}"`,
matching the library picker's character-by-character subsequence algorithm.

**Methods:**

- `new()` — build GTK widgets
- `set_items(&mut self, items: Vec<GlossedPassage>)` — store items, call
  `populate_list("")`
- `show(&self)` — show picker_box, clear entry, grab focus, repopulate
- `hide(&self)` / `is_visible(&self)`
- `attach(&self, base)` — overlay nesting
- `populate_list(&self, filter: &str)` — clear and rebuild list_box rows
- `selected_index(&self) -> Option<usize>` — parse widget_name of selected row
- `move_selection(&self, delta: i32)` — move selection up/down

## Action & Keybinding

- Add `OpenGlossPicker` variant to `Action` enum in `src/input/actions/mod.rs`
- Category: `Vocab`
- Default binding: `Alt+g` in `src/input/keymap_config.rs`

## InputMode

- Add `GlossPicker` variant to `InputMode` enum in `src/app.rs`

## Keymap Routing

Add `GlossPicker` to `handle_picker_key()` in `src/input/keymap.rs`:

- **Hide (Escape):** hide picker, set `input_mode = Reader`
- **Confirm (Enter):** read `selected_index()`, clone
  `gloss_picker.items[idx]` as `passage`, then replicate
  `navigate_gloss_passage` steps 5-10:
  1. `find_all_glosses(&conn, &passage.work_abbrev, &passage.start_citation, &passage.end_citation)`
  2. Build `GlossContext` from passage fields
  3. `gloss_overlay.show_gloss_with_color(...)` with first gloss text
  4. `gloss_overlay.set_position(0, all_glosses.len())`
  5. Update `gloss_list`, `gloss_index = 0`, `gloss_context`,
     `gloss_passage_index = idx`, `gloss_passages` (copy from picker items)
  6. Set `input_mode = GlossOverlay`
- **MoveDown/MoveUp:** `move_selection(1)` / `move_selection(-1)`

## Opener Function

**File:** `src/input/actions/pickers.rs`

`open_gloss_picker(state, tokio_handle)`:

1. Get `abbrev` from `current_work` (return early if none)
2. Spawn async: `find_glossed_passages(&conn, &abbrev)`
3. On result: `gloss_picker.set_items(passages)`, `gloss_picker.show()`
4. Set `input_mode = GlossPicker`

## AppState Changes

**File:** `src/app.rs`

- Add `gloss_picker: GlossPicker` field to `AppState`

## Overlay Nesting

Insert `GlossPicker` in the chain after `gloss_overlay` and before
`concordance_picker`:

```
gloss_overlay.overlay → GlossPicker.attach(&gloss_overlay.overlay)
gloss_picker.overlay  → ConcordancePicker.attach(&gloss_picker.overlay)
```

## Signal Wiring

In `app.rs`, connect `search_entry`'s `changed` signal to call
`gloss_picker.populate_list()` with the current entry text.

## Module Registration

- Add `pub mod gloss_picker;` to `src/ui/mod.rs`

## Files Changed

1. `src/ui/gloss_picker.rs` — new file, ~170 lines
2. `src/ui/mod.rs` — add module declaration
3. `src/app.rs` — add `GlossPicker` to `InputMode`, add `gloss_picker` field
   to `AppState`, overlay nesting, signal wiring
4. `src/input/actions/mod.rs` — add `OpenGlossPicker` action variant
5. `src/input/keymap.rs` — add `GlossPicker` arms to `handle_picker_key()`
6. `src/input/keymap_config.rs` — default `Alt+g` binding
7. `src/input/actions/pickers.rs` — add `open_gloss_picker()`, add dispatch
   in `dispatch_action` for `OpenGlossPicker`
