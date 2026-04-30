# Multi-Gloss Navigation and Management

**Date**: 2026-04-29

## Summary

Support multiple glosses per passage with navigation, add-from-prompt, delete, copy ID, and undo-via-navigation. Replaces the single-gloss amend/regenerate model.

## Data Model

No schema changes. The `glosses` table already supports multiple rows per passage (no UNIQUE on `passage_id + gloss_type`). `save_gloss` already inserts new rows.

### New query: `find_all_glosses`

Returns all glosses for a passage matching `(work_abbrev, start_citation, end_citation, gloss_type='teacher-generic')`, ordered by `timestamp DESC`. Returns `Vec<SavedGloss>`.

### Removed: `update_gloss`

No longer called. "Add" always creates a new row. `update_gloss` function can remain in queries.rs but has no callers.

## AppState Changes

Replace:
- `gloss_saved: Option<SavedGloss>` -> `gloss_list: Vec<SavedGloss>`
- Add: `gloss_index: usize` (current position in `gloss_list`)

`gloss_context: Option<GlossContext>` stays unchanged.

## Gloss Overlay Keybinds

- **j / k** — scroll gloss text
- **Ctrl+n** — next gloss (newer, index - 1 since sorted DESC)
- **Ctrl+p** — previous gloss (older, index + 1)
- **u** — alias for Ctrl+p (previous/undo)
- **a** — add new gloss: opens prompt dialog, sends prompt + current gloss text to Claude, result saved as new row, appended to list, displayed
- **c** — copy current gloss ID to clipboard via `wl-copy`
- **d** — delete current gloss: shows confirmation overlay, on confirm deletes row from DB, removes from list, shows adjacent gloss or closes if last
- **Esc / n** — close overlay

**Removed**: `r` (regenerate) — redundant with add

## Hint Bar

```
Esc close · a add · d delete · c copy id · Ctrl+n/p navigate
```

## Position Indicator

When `gloss_list.len() > 1`, show `"1 / 3"` (1-indexed) near the hint bar. Hidden when only one gloss exists.

## Flow: Visual Select -> Gloss

1. User selects lines, picks "Gloss with Claude"
2. `find_all_glosses` called
3. If any exist: show newest (index 0), populate `gloss_list`
4. If none: call Claude API, `save_gloss`, set `gloss_list = vec![new]`, `gloss_index = 0`

## Flow: Ctrl+g / Shift+Tab Toggle

1. Check `gloss_list` is non-empty
2. Show gloss at `gloss_index` (preserves last-viewed position)

## Flow: Add (a key)

1. Open prompt dialog (in-app overlay)
2. On Ctrl+Enter submit:
   - Send to Claude: existing gloss text as context + user prompt
   - `save_gloss` with result (new row)
   - Re-query `find_all_glosses` to refresh list
   - Set `gloss_index = 0` (newest)
   - Display the new gloss

## Flow: Delete (d key)

1. Show confirmation overlay: "Delete this gloss? (y/n)"
2. On `y`:
   - `delete_gloss(current_gloss_id)`
   - Remove from `gloss_list`
   - If list empty: close overlay, clear state
   - Else: clamp `gloss_index`, show adjacent gloss
3. On `n` or Esc: dismiss confirmation, stay in overlay

## Flow: Navigate (Ctrl+n / Ctrl+p / u)

- Ctrl+n: `gloss_index = (gloss_index - 1).max(0)` (toward newest)
- Ctrl+p / u: `gloss_index = (gloss_index + 1).min(len - 1)` (toward oldest)
- Re-render overlay with gloss at new index
- Update position indicator

## Flow: Copy ID (c key)

- Run `echo -n '{gloss_id}' | wl-copy` via `std::process::Command`
- Brief visual feedback not required (wl-copy is silent)

## Delete Confirmation

Minimal overlay, same style as amend dialog:
- Centered Box with `amend-dialog` CSS class
- Text: "Delete gloss {id}? y = confirm, Esc = cancel"
- Key handler: `y` confirms and closes, `Esc`/`n` cancels

## Files Changed

- `src/db/queries.rs` — add `find_all_glosses`
- `src/app.rs` — change `gloss_saved` to `gloss_list: Vec<SavedGloss>` + `gloss_index: usize`
- `src/input/keymap.rs` — rewrite `handle_gloss_key` with new keybinds, remove `regenerate_gloss`, rename `amend_gloss` to `add_gloss`, add delete confirmation, add copy ID, add navigation
- `src/input/visual.rs` — update `action_gloss_with_claude` to use `find_all_glosses` and populate `gloss_list`
- `src/ui/gloss_overlay.rs` — add position indicator label, update hint text
