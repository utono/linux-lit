# Phase 6: Search — Design Spec

## Overview

Text search with vim-style `/` activation, live results, match highlighting, and `n`/`N` cycling through matches.

## Search Bar UI

- Thin `GtkBox` anchored to the bottom of the window
- Contains a `GtkEntry` (search input) and `GtkLabel` (match counter, e.g. `[3/17]`)
- Hidden by default; `/` shows it and grabs focus
- Styled to match current theme (same bg/fg as reader)

## Search Execution

- Live search on every keystroke (same pattern as library picker's `connect_changed`)
- Smart-case: if query contains any uppercase character, match case-sensitively against `Line.text`; otherwise match case-insensitively against `Line.normalized` with a lowercased query
- Matches stored as `Vec<SearchMatch>` where `SearchMatch = { line_index: usize, byte_start: usize, byte_end: usize }`
- `search_tag` applied to all match spans in the buffer (highlight background)
- `search_current_tag` applied to the active match (stronger/distinct highlight)

## Navigation

- `n` — next match (wraps from last to first)
- `N` — previous match (wraps from first to last)
- Sets `current_line` to the match's line index
- Calls `ensure_cursor_on_page` + `update_highlight` (same pattern as dialogue jump)
- Counter label updates to `[idx/total]`

## Dismissal & Highlight Lifecycle

- `Escape` — hides search bar; highlights and `n`/`N` remain active (`search_active = true`)
- Any key other than `n`/`N` while `search_active` — clears all highlights and match state
- Loading a new work clears search state
- New `/` press clears previous search and starts fresh

## AppState Additions

```rust
pub search_bar: SearchBar,           // UI struct
pub search_matches: Vec<SearchMatch>, // current results
pub search_match_idx: usize,         // index into search_matches
pub search_tag: TextTag,             // all-matches highlight
pub search_current_tag: TextTag,     // current-match highlight
pub search_active: bool,             // true after dismiss, until cleared
```

## File Structure

- **New:** `src/ui/search_bar.rs` — SearchBar struct (build, show, hide, update_counter, is_visible, entry)
- **New:** `src/input/search.rs` — execute_search, next_match, prev_match, clear_search
- **Modified:** `src/app.rs` — add search fields to AppState, create tags at build time
- **Modified:** `src/input/keymap.rs` — `/` keybind, search-visible guard, `n`/`N` routing, clear-on-other-key logic
- **Modified:** `src/input/mod.rs` — add `pub mod search;`
- **Modified:** `src/ui/mod.rs` — add `pub mod search_bar;`

## Keymap Integration

In `handle_key`, add guards in this order:

1. **Search bar visible:** `Return` executes search and hides bar, `Escape` hides bar, all other keys pass through to the `Entry` widget (return `false`)
2. **Search active (bar hidden, highlights shown):** `n` calls `next_match`, `N` calls `prev_match`, any other key calls `clear_search` first then processes normally

## Tab Keybind Behavior

When `n`/`N` lands on a match line and the user presses `Tab`: resume playback (if paused) at `start_time - 0.2s` of the current line's timestamp. This follows the same seek pattern used by dialogue jump (comma/q).

## Theme Colors

- `search_tag`: use theme's selection/highlight color at reduced opacity
- `search_current_tag`: use theme's accent/cursor color for stronger contrast
