# Line Translations Toggle — Design Spec

**Date:** 2026-03-26
**Status:** Approved

## Overview

Add an `Alt+i` keybind to toggle inline translations below original text lines. When visible, all original lines are dimmed and translation text appears below each matched source line, styled italic and indented.

## Data Layer

- New query in `queries.rs`:
  ```sql
  SELECT lm.id, lm.canonical_text, lt.translation
  FROM line_translations lt
  JOIN line_mapping lm ON lt.line_mapping_id = lm.id
  WHERE lm.work_abbrev = ?1
  ```
- Returns a `HashMap<i64, String>` keyed by `line_mapping.id` (which matches `Line.id`)
- Called once during `load_work()`, stored on AppState
- No changes to the `Line` struct

## State

Two new fields on `AppState`:
- `translations: HashMap<i64, String>` — populated during `load_work()`, empty if no translations exist
- `translations_visible: bool` — default `false`

## Toggle Behavior

**Toggle on (`translations_visible` becomes `true`):**
1. If `translations` map is empty, do nothing
2. Apply `translation-dim` TextTag to all original lines (reduced opacity/blended color)
3. Iterate buffer lines bottom-to-top (avoids index shifting)
4. For each buffer line mapping to a work line with a translation, insert a new line after it with the translation text (8-space indent prefix)
5. Apply `translation-text` TextTag to inserted lines (italic, normal brightness)
6. Track which buffer lines are translation inserts (e.g., `Vec<bool>` or `HashSet<usize>`)
7. Recalculate `current_line` and `page_top_line` to account for added lines

**Toggle off (`translations_visible` becomes `false`):**
1. Remove inserted translation lines bottom-to-top
2. Remove `translation-dim` tag from all lines
3. Clear translation-line tracking
4. Recalculate `current_line` and `page_top_line` to account for removed lines

## Cursor Behavior

Navigation commands (`j`/`k`, page up/down, `G`, `gg`) skip over translation lines. The cursor only lands on original lines. Translation lines are display-only.

## Keybinding

- `Alt+i` in `keymap.rs`, placed near existing `Alt+f` (font info)
- Calls `toggle_translations()` in `app.rs`

## TextTags

- `translation-dim` — applied to all original lines when translations are visible; similar to existing `dim_tag` pattern with reduced opacity
- `translation-text` — applied to inserted translation lines; italic, indented, normal brightness

## Integration

- Translations apply to all lines regardless of AB chunk state
- Dialogue formatting and translations are independent; both can be active simultaneously
- Translation lines use their own TextTags, not reusing dialogue tags
- On `display_work()` (new work loaded): reset `translations_visible = false`, load new work's translations into HashMap

## Reference

Mirrors behavior of `~/utono/lit` plugin `lit_translations` which uses `Alt+i`, dims originals, and shows virtual lines below matched source lines. Key difference: `lit` scopes to active chunk; `linux-lit` shows all lines.
