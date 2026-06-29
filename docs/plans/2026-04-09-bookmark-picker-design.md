# Bookmark Picker Design

**Date:** 2026-04-09
**Status:** Approved

## Overview

Add a bookmark picker UI (Ctrl+m) that lists all bookmarks for the current work, allows jumping to a bookmark or deleting it. Follows the media picker pattern — flat list, overlay-based, single selection, search entry with subsequence filtering.

## Row Display

Each row shows:
- **Left:** First ~80 characters of the bookmarked line's text, truncated with ellipsis if longer
- **Right:** Relative timestamp from `created_at` — "2m ago", "3h ago", "5d ago" etc.

Widget name on each `ListBoxRow` stores the `line_mapping_id` as a string.

## Data

New query `load_bookmarks_with_details(conn, work_abbrev) -> Vec<BookmarkItem>` joins `bookmarks` with `line_mapping` to return:
- `line_mapping_id: i64`
- `line_text: String` (from `line_mapping.canonical_text`)
- `created_at: String` (ISO-8601)

Results sorted by `created_at DESC` (most recent first).

New query `delete_bookmark(conn, work_abbrev, line_mapping_id)` removes a bookmark and is used by the `d`/`Delete` key in the picker.

New struct `BookmarkItem` in `src/db/models.rs`:
```rust
pub struct BookmarkItem {
    pub line_mapping_id: i64,
    pub line_text: String,
    pub created_at: String,
}
```

## Widget Structure

New `BookmarkPicker` in `src/ui/bookmark_picker.rs` following media picker pattern:

```
BookmarkPicker {
    overlay: Overlay,
    picker_box: GtkBox (vertical),
    search_entry: Entry,
    list_box: ListBox (single selection),
    items: Vec<BookmarkItem>,
}
```

Public API:
- `new()` — constructor, creates widget tree
- `set_items(items: Vec<BookmarkItem>)` — load bookmark list, auto-populates
- `show()` / `hide()` / `is_visible()` — visibility control
- `attach(base: &impl IsA<Widget>)` — attach to overlay hierarchy
- `search_entry()` — accessor for search signal connection
- `populate_list(filter: &str)` — render filtered items with subsequence matching
- `selected_line_mapping_id()` — get selected bookmark's line_mapping_id
- `move_selection(delta: i32)` — keyboard navigation
- `remove_selected()` — remove the selected row from the list and internal items vec

## Relative Timestamp Formatting

Convert `created_at` ISO-8601 string to relative format:
- Under 1 minute: "just now"
- Under 1 hour: "Nm ago" (minutes)
- Under 1 day: "Nh ago" (hours)
- Under 30 days: "Nd ago" (days)
- Otherwise: "NMo ago" (months)

## Keybinds

| Key | Context | Action |
|-----|---------|--------|
| `Ctrl+m` | Normal mode | Open bookmark picker (fetches bookmarks async) |
| `Ctrl+Shift+M` | Normal mode | Open media picker (moved from Ctrl+m) |
| `Return` | Picker visible | Jump to selected bookmark, close picker |
| `d` / `Delete` | Picker visible | Delete selected bookmark from DB, update `is_bookmarked` vec, refresh picker list, trigger gutter redraw |
| `Escape` | Picker visible | Close picker |
| `j` / `Down` / `Ctrl+n` | Picker visible | Move selection down |
| `k` / `Up` / `Ctrl+p` | Picker visible | Move selection up |

## Filtering

Subsequence match (case-insensitive) on the line text, same algorithm used by media picker and library picker.

## Jump Behavior

On `Return`:
1. Get `line_mapping_id` from selected row's widget name
2. Find the corresponding buffer line (check `line_map` for text_file works, else iterate `work.lines`)
3. Call `navigation::jump_to_line()` to navigate
4. Close picker

## Delete Behavior

On `d` or `Delete`:
1. Get `line_mapping_id` from selected row
2. Call `delete_bookmark()` on DB (async via tokio)
3. Update `is_bookmarked` vec for the corresponding buffer line to `false`
4. Trigger gutter redraw
5. Remove the row from the picker list (call `remove_selected()`)
6. If no bookmarks remain, close picker

## AppState Changes

Add `bookmark_picker: BookmarkPicker` field to `AppState`.

## Files

- **Create:** `src/ui/bookmark_picker.rs` — new picker widget
- **Modify:** `src/ui/mod.rs` — add `pub mod bookmark_picker;`
- **Modify:** `src/db/models.rs` — add `BookmarkItem` struct
- **Modify:** `src/db/queries.rs` — add `load_bookmarks_with_details()`, `delete_bookmark()`
- **Modify:** `src/app.rs` — add `bookmark_picker` to AppState, wire search signal, attach overlay
- **Modify:** `src/input/keymap.rs` — move media picker to Ctrl+Shift+M, add Ctrl+m for bookmark picker, add picker-mode key handling

## Out of Scope

- Cross-work bookmark listing
- Bookmark reordering
- Bookmark labels/annotations
- Bookmark export/import
