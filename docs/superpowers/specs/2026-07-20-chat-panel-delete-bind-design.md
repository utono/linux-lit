# Chat panel `D` deletes the current gloss / journal entry

Date: 2026-07-20
Status: approved
Branch: stacked on feat/journal-entry-top-landing (user's choice — depends on
its Journal-view cursor mechanics; both merge together after testing)

## Problem

The chat panel can display glosses (Gloss view, `Ctrl+n`/`Ctrl+p` cycling)
and journal entries (Journal view, `j`/`k` cursor), but deleting either
requires opening the corresponding overlay. Both overlays already bind `D`
with a "Delete …? y / Esc" confirmation modal.

## Desired behavior

`D` in the chat panel transcript (`handle_chat_transcript_key`,
`src/input/keymap.rs:1500-1678` — `D` confirmed unbound today):

- **Gloss view**: delete the currently shown gloss
  (`s.chat.gloss_list[s.chat.gloss_index]`, id in `SavedGloss.gloss_id`).
- **Journal view**: delete the entry under the cursor
  (`s.chat.journal_list[s.chat.journal_cursor]`, id in `JournalPage.id`).
- **Question view**: `D` stays unbound (falls through to the catch-all).
- Both routes go through the existing confirmation modal
  (`show_delete_confirmation` → `InputMode::DeleteConfirm` →
  `handle_delete_confirm_key`) with a new chat-panel dispatch origin, so
  `y` confirms and `Escape`/`n` cancels — identical UX to the overlays.

## Design

### Gloss delete (mirrors `delete_current_gloss`, gloss.rs:348-412)

- Extract the overlay's DB-and-audio purge block (`delete_gloss` +
  `delete_gloss_audio` + mp3-dir removal) into ONE shared helper used by
  both the overlay and the new panel path, so the two cannot drift.
- Panel bookkeeping after the purge: remove from `s.chat.gloss_list`,
  clamp `s.chat.gloss_index`; if a gloss remains, show it in the
  transcript's gloss slot (the same replace-in-place path `Ctrl+n`/`p`
  uses); if the list is now EMPTY, show a "No glosses for this passage"
  placeholder row and stay in Gloss view (user's choice) — follow-up
  Q&A exchanges below remain.
- Refresh the reader's glossed-line tint (`apply_reader_gloss_highlighting`).
- Reconcile the gloss OVERLAY's separate cache (`AppState.gloss_list` /
  `gloss_index` — a distinct Vec from the panel's): remove the deleted
  id if present and clamp its index, so a stale entry cannot resurface.
- Toast `Deleted gloss {id}` (same style as the overlay).

### Journal delete (mirrors journal.rs `delete_current`, 2760-2796)

- `delete_journal_page` + `purge_journal_audio` (both existing).
- Remove from `s.chat.journal_list`, clamp `s.chat.journal_cursor`,
  re-render snapped to the neighboring entry
  (`render_journal_view_inner(s, true)`); the empty case already renders
  the "No journal entries for this passage" placeholder.
- **Dangling-reference cleanup** (panel-specific, overlays never needed
  it): any exchange with `saved_id == deleted id` gets `saved_id = None`
  (SavedMark disappears; `s` can re-save), and `s.chat.revision_of` is
  cleared if it pointed at the deleted id.
- Reconcile the journal OVERLAY's cache (`s.journal.pages`): remove the
  deleted id if present, clamp `page_index`.
- Toast `Deleted journal {id}`.

### Legend (standing rule)

Every chat-panel bind change updates the panel's own Ctrl+/ legend in the
same change: add a `D` entry to the "Transcript actions" group in
`src/ui/chat_keybinds_overlay.rs` (`GROUPS`, lines 10-45), styled like the
`("c", "copy id: …")` entry. No keymap_config/keymap.json/main-overlay
changes — chat-panel keys are hardcoded in the handler.

## Out of scope

- `D` in the Question view.
- Deleting from the overlays (unchanged; gloss purge refactor is
  behavior-preserving for the overlay).
- Any revision-history purge beyond what `delete_journal_page` /
  `delete_gloss` already do.

## Testing

- Unit tests for the pure parts: list/index clamping after removal (both
  views), dangling `saved_id`/`revision_of` cleanup, empty-list placeholder
  row shape.
- `cargo build` + full suite + clippy green.
- Manual/headless gate shared with the stacked branch: delete a gloss
  (next gloss appears; reader tint updates; overlay shows no stale entry),
  delete the last gloss (placeholder, still Gloss view), delete a journal
  entry (cursor lands on neighbor; SavedMark cleared if it was the saved
  exchange), `Escape` cancels, legend shows `D`.
