# Bookmarks Feature Design

**Date:** 2026-04-09
**Status:** Approved

## Overview

Add per-work bookmarks to linux-lit. A bookmark pins a line (by `line_mapping.id`) so the user can cycle through marked lines and jump to the most recently created one.

## Database Schema

New `bookmarks` table in `lit.db`:

```sql
CREATE TABLE IF NOT EXISTS bookmarks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    work_abbrev TEXT NOT NULL,
    line_mapping_id INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (work_abbrev) REFERENCES works(abbrev),
    FOREIGN KEY (line_mapping_id) REFERENCES line_mapping(id),
    UNIQUE(work_abbrev, line_mapping_id)
);
```

- `work_abbrev` + `line_mapping_id` is unique — one bookmark per line per work.
- `created_at` stores ISO-8601 with milliseconds for reliable recency ordering.
- Toggle semantics: INSERT on add, DELETE on remove. The UNIQUE constraint prevents duplicates.

## AppState Changes

Add `is_bookmarked: Rc<RefCell<Vec<bool>>>` to `AppState`, following the same pattern as `has_timestamp`, `is_manual`, and `is_chapter_line`.

- Populated during `display_work` (work load) by calling `load_bookmarks()`.
- Updated in-place on toggle without full reload.

## Gutter Rendering

Add bookmark glyph `★` (U+2605) to the existing timestamp gutter renderer in `src/gutter.rs`.

Priority order (highest wins when a line has multiple markers):

1. `★` — bookmark
2. `▸` — chapter
3. `─` — manual timestamp
4. `◐` / `◑` — AB loop points
5. `•` — auto timestamp

## Keybinds

| Key | Action | Notes |
|-----|--------|-------|
| `m` | Toggle bookmark on current line | Was media picker |
| `'` (apostrophe) | Jump to next bookmark by line position | Wraps around |
| `"` (quotedbl) | Jump to previous bookmark by line position | Wraps around |
| `g'` | Jump to most recently created bookmark | By `created_at DESC` |
| `Ctrl+m` | Open media picker | Moved from `m` |

### Navigation behavior

- Next/previous cycle through bookmarks sorted by **buffer line position** within the current work.
- Wrap: next from last bookmark goes to first; previous from first goes to last.
- No bookmarks: all navigation keys are no-ops.
- `g'` queries `created_at DESC LIMIT 1` for the current work and jumps to that line.

## Database Query Functions

Add to `src/db/queries.rs`:

- `load_bookmarks(conn, work_abbrev) -> Vec<i64>` — returns all bookmarked `line_mapping_id`s for a work.
- `toggle_bookmark(conn, work_abbrev, line_mapping_id) -> bool` — tries INSERT; on UNIQUE conflict, DELETEs instead. Returns `true` if bookmark was added, `false` if removed.
- `most_recent_bookmark(conn, work_abbrev) -> Option<i64>` — returns the `line_mapping_id` of the most recently created bookmark for the work.

## Files to Modify

- `src/db/queries.rs` — table creation in `ensure_tables()`, new query functions
- `src/app.rs` — add `is_bookmarked` field to `AppState`, populate on work load
- `src/input/keymap.rs` — move media picker from `m` to `Ctrl+m`, add `m`/`'`/`"`/`g'` keybinds
- `src/input/navigation.rs` — add `toggle_bookmark()`, `next_bookmark()`, `prev_bookmark()`, `most_recent_bookmark()` navigation functions
- `src/gutter.rs` — add `★` glyph with highest priority in timestamp gutter renderer

## Out of Scope

- Bookmark labels or annotations
- Cross-work bookmark lists
- Bookmark export/import
- Bookmark picker UI (Ctrl+p style)
