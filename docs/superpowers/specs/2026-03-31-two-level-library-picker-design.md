# Two-Level Library Picker

## Overview

Replace the flat work list in Ctrl+p picker with a two-level author-then-works browser. Improve visual styling to use theme colors, add a scrim behind the picker, and thin the borders.

## Interaction

### Author List (Level 1)

- Opens on Ctrl+p
- Shows authors with work counts: `Shakespeare (43)`
- Sort order: Shakespeare pinned first, Dickens second, remaining alphabetical
- Enter on an author drills into their works
- Escape closes the picker entirely
- Ctrl+n / Ctrl+p or j/k navigate the list (existing behavior)

### Work List (Level 2)

- Shows works for the selected author, sorted alphabetically by title
- Each row: `Hamlet (Ham)` — title with abbreviation
- Enter loads the selected work and closes picker
- Escape goes back to author list
- Backspace when filter is empty goes back to author list
- Escape on author list closes the picker

### Global Fuzzy Filter

The search entry filters across both levels simultaneously:

- Typing on the author list filters authors by name
- If the filter matches works but not the current author list, auto-drill into the matching author's works
- Example: typing "hamlet" on the author screen finds it under Shakespeare, switches to Shakespeare's work list filtered to Hamlet
- If matches span multiple authors, stay on author list filtered to those authors
- Clearing the filter returns to whichever level you're on (unfiltered)

### Filter Matching

Reuse existing subsequence matching. On the author level, match against author name. When checking for cross-level matches, match against `"{title} {author} {abbrev}"` (existing logic).

## State

Add a `PickerLevel` enum to `LibraryPicker`:

```rust
enum PickerLevel {
    Authors,
    Works { author: String },
}
```

Store current level alongside the grouped data:

```rust
struct AuthorGroup {
    author: String,
    works: Vec<WorkSummary>,
}
```

At `set_works()` time, group works by author and sort groups per the pinning rules. Store as `Vec<AuthorGroup>`.

## Visual Styling

### Theme Integration

Replace hardcoded `rgba(40,40,40,0.95)` with theme colors:

```css
.library-picker {
    background-color: {bg};
    color: {fg};
    padding: 16px;
    border-radius: 12px;
    border: 1px solid {dim};
}
.library-picker entry {
    margin-bottom: 8px;
}
.library-picker row:selected {
    background-color: {cursor_bg};
    color: {cursor_fg};
}
```

This matches the existing vocab-popup and action-popup styling.

### Scrim

Add a scrim widget (full-size semi-transparent overlay) behind the picker box, visible only when the picker is open:

```css
.library-picker-scrim {
    background-color: rgba(0, 0, 0, 0.3);
}
```

The scrim is a `gtk4::Box` added as an overlay between the base content and the picker box. Show/hide it alongside the picker.

### Row Styling

Author rows show: `{author name}` left-aligned, `{count}` right-aligned in dim color.

Work rows show: `{title}` left-aligned, `({abbrev})` right-aligned in dim color.

Use a horizontal box per row with the count/abbrev label having `halign: End` and `hexpand: true`.

### Placeholder Text

- Author level: `"Filter authors..."`
- Work level: `"Filter works..."`

Update placeholder when switching levels.

## Files to Modify

- `src/ui/library_picker.rs` — main changes: state machine, grouped data, scrim, row rendering
- `src/theme.rs` — update `.library-picker` CSS to use theme variables
- `src/input/keymap.rs` — handle Escape/Backspace level navigation

## Scope Exclusions

- No changes to database queries (grouping done client-side)
- No changes to how works are loaded after selection
- No changes to other picker overlays
