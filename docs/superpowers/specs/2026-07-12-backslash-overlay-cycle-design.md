# Backslash Segment-Overlay Cycle — Design

Date: 2026-07-12
Branch: `feat/backslash-overlay-cycle` (worktree off master)
Status: approved

## Summary

Plain `\` cycles through the three per-segment overlays for the segment the
reader is on: journal Q&A → gloss → synopsis → journal Q&A (wraps forever).
Pressing `\` in reader mode starts the lap at journal Q&A; pressing `\`
inside any of the three overlays advances to the next stop. The segment is
anchored at the reader position where the lap started.

Plain `\` is currently unbound in reader mode (`keymap_config.rs` even
asserts this). `Alt+\` (ToggleVocabHighlight) and `Ctrl+\`
(OpenLibraryPicker) are untouched.

## Behavior

### Ring and entry

- Reader mode `\` → new `Action::CycleSegmentOverlays`, always opening the
  journal Q&A stop first.
- Inside JournalOverlay / GlossOverlay / SynopsisOverlay, plain `\`
  advances: journal → gloss → synopsis → journal. View mode only — while a
  vim editor is active on an overlay, `\` goes to the editor as text.
- The ring wraps; a successful stop never closes to the reader on its own
  (only an empty stop can end a lap — see below). Escape and each overlay's
  existing close/flip keys behave exactly as today.

### Segment anchor (fixed at cycle entry)

- The first `\` of a lap saves the current reader position as the cycle
  anchor (new `AppState` field, e.g. `cycle_anchor: Option<...>` matching
  the saved-position shape used by `restore_saved_position_resnap`).
- Each advance closes the current overlay via return-to-reader + restore to
  the ANCHOR, deliberately skipping the jump-to-source logic
  (`jump_to_journal_source_start` / `jump_to_gloss_source_start`). All
  stops in a lap therefore show the same segment, even if the user paged
  to another scene inside the journal overlay with Ctrl+n/p.
- The anchor is taken/cleared on ANY other exit: Escape, Ctrl+g/Ctrl+j,
  Ctrl+Tab, picker confirm/jump, work switch. Those paths keep today's
  behavior (source jump or saved-position restore) unchanged.

### Empty stops — existing fallbacks fire

- Journal stop, scene band has no Q&A: the existing fallback opens the
  work-wide Q&A picker. The lap ends there (anchor cleared); picker keys
  are untouched.
- Gloss stop, no gloss on the anchor line: existing toast
  ("No gloss on this line"); the previous overlay has already closed, so
  the user is back in the reader at the anchor. Next `\` starts a fresh
  lap at journal.
- Synopsis stop, no synopsis for the section: existing toast, same
  land-in-reader outcome.

## Implementation shape (approach A — thin dispatcher)

Compose the existing, proven handlers rather than building new overlay
machinery:

- New small module `src/input/actions/overlay_cycle.rs` with
  `advance(state, from: Stop)` where `Stop` ∈ {Reader, Journal, Gloss,
  Synopsis}:
  1. On `Reader`, save the anchor.
  2. Close the current overlay with its existing return-to-reader path
     (TTS stop, tint recolor, `entry_page_id`/`return_pos` hygiene stay in
     the overlay's own close code), but restore to the anchor instead of
     jumping to source.
  3. Call the next stop's existing open function: `journal::
     open_journal_scene`, the gloss open half of `gloss::toggle_overlay`
     (factor into a callable fn if not already), `scene_synopsis::
     show_synopsis_overlay`.
- `keymap.rs`: dispatch arm for `CycleSegmentOverlays`; a plain-`\` case in
  each of the three overlay modal handlers (view mode only).
- `src/input/actions/mod.rs`: `CycleSegmentOverlays` variant + name string.

Rejected alternatives: extending the Ctrl+Tab / ToggleLastOverlay flip
machinery (touches subtle return-position code for all three overlays at
once), and a full ring state machine with cycle-aware open/close variants
per overlay (duplicates close logic that already works).

## Keybind mirrors (same change, per house rules)

- `keymap_config.rs` compiled default: plain `\` → `CycleSegmentOverlays`
  (and update the test asserting `\` is unbound).
- Stowed `~/tty-dotfiles/linux-lit` `keymap.json`: add the binding
  (otherwise the JSON silently shadows the compiled change).
- Ctrl+/ reader overlay (`src/ui/keybinds_overlay.rs`): keycap strip AND
  describe() detail arm, via the `update-cairo-keybinds-overlay` skill's
  three-pass cross-reference.
- Overlay legends: the `GROUPS` consts in
  `src/ui/{journal,gloss,synopsis}_keybinds_overlay.rs` each gain the `\`
  entry (advance-cycle), updated in the same change as the handlers.

## Testing

- `cargo test --bins` for keymap/action plumbing (including the updated
  unbound-`\` assertion).
- Headless cage acceptance: from a Shakespeare scene, drive `\` four times,
  screenshot each stop; verify journal → gloss → synopsis → journal wrap
  and that every stop shows the entry segment. Drive one lap with a
  Ctrl+n/p traverse inside the journal to verify the anchor holds. Verify
  Escape from mid-lap restores the reader position as today.
- Empty-stop checks: a line with no gloss (toast, reader at anchor), a
  scene with no Q&A (work-wide picker opens).

## Out of scope

- No changes to Escape/Ctrl+Tab/flip behavior, the overlays' internal
  navigation, vim editors, or the pickers.
- No persistence of cycle state across work switches or sessions.
