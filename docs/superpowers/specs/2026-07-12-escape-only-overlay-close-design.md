# Escape-Only Overlay Close + TTS-Stop on Cycle Advance — Design

Date: 2026-07-12
Branch: `feat/escape-only-overlay-close` (worktree off master @ 648029a)
Status: approved (user-directed follow-up to the `\` segment-overlay cycle)

## Summary

Two simplifications to the reader's overlay keys:

1. **Every `\` cycle advance stops TTS.** `cycle_from_journal` and
   `cycle_from_synopsis` gain the `s.tts.stop()` the gloss advance already
   has.
2. **Escape is the only key that closes a reader overlay, and overlay-to-
   overlay cross-jump/create keys are dropped — the `\` cycle is the only
   overlay-to-overlay navigation.** One exception, user-retained: the
   translation overlay keeps its `i` same-key toggle-close.

## Behavior changes (per overlay handler)

All dropped keys become EXPLICIT consumed no-op arms (`true` / `return
true` with an "Escape-only close policy" comment) — NOT deleted arms.
Rationale: deleted arms fall through (e.g. Ctrl+g would start a `gg`
chord; Ctrl+Tab would hit the Tab-plays-TTS arm; unmatched keys may leak
past the handler).

### Gloss overlay (`handle_gloss_key`)
- `Escape` keeps today's close (`close_gloss_to_reader`, jump-to-source).
- Dropped → no-op: `n` (was Escape's alias), `Ctrl+g` (close),
  `Ctrl+Tab` (flip), `Ctrl+j` (cross-jump to journal), `r` (cross-create:
  journal ask card for the gloss passage).
- Kept: `R` (rewrite this gloss — native edit), `Alt+g` (glosses picker
  over the overlay), `Ctrl+/`, `Ctrl+,`, `\` cycle, all nav/TTS keys.

### Journal overlay (`handle_journal_key`)
- `Escape` keeps today's close (`journal::close_overlay`).
- Dropped → no-op: `Ctrl+j` (close), `Ctrl+Tab` (flip), `Ctrl+g`
  (cross-jump to gloss view), `Alt+g` (cross-create: reader-gloss from
  the journal passage).
- Kept: `r`/`R`/`e` (the journal's own ask/rewrite/edit — native),
  `Ctrl+\` (work-wide Q&A picker), `Ctrl+Shift+J` (move picker),
  `Ctrl+/`, `\` cycle, all band/block nav.

### Synopsis overlay (`handle_synopsis_overlay_key`)
- `Escape` keeps today's close.
- Dropped → no-op: `h` (close), `Ctrl+g` (close), `Ctrl+j` (cross-jump to
  journal), `r` (cross-create: scene ask card).
- Kept: `R` (rewrite synopsis), `e` (edit), `Alt+g` (work glosses picker),
  `Ctrl+/`, `Ctrl+,`, `\` cycle.

### Translation overlay (`handle_translation_overlay_key`)
- `Escape` AND `i` both keep closing (user-retained same-key toggle).
- Dropped → no-op: `Ctrl+j` (cross-jump to journal).

### Echoes overlay (`handle_echoes_overlay_key`)
- `Escape` keeps today's close (`close_echoes_to_reader`).
- Dropped → no-op: `Ctrl+g` (close), `Ctrl+j` (cross-jump to journal).

### Untouched
- The `\` segment-overlay cycle (all four arms).
- Per-overlay Ctrl+/ legends and their Escape/Ctrl+/ close-to-PARENT keys
  (they don't close to the reader).
- Settings (`Return` = confirm-save; Escape = cancel), keybinds/gamepad
  overlays (already Escape-only), delete/undo confirms (return to origin).
- Vim editors, ask cards, pickers, visual modes, vocab loop.
- Reader-side OPEN keys (`Ctrl+g`, `Ctrl+j`, `h`, `u`, echoes keys, …).

## Knock-on simplifications (dead code from the drops)

- `gloss::toggle_last_overlay` (Ctrl+Tab): its GlossOverlay/JournalOverlay
  close arms become unreachable (reader dispatch only fires in Reader
  mode) → slim to the Reader-mode reopen + update doc comment and the
  `Action::ToggleLastOverlay` doc + Ctrl+/ describe text ("last overlay"),
  which currently say it closes from inside an overlay.
- `gloss::toggle_overlay`'s close half loses its last caller
  (toggle_last_overlay's arm) → slim to delegate to `open_gloss_at_cursor`
  (keep the fn name; `Action::ToggleGlossOverlay` stays bound).
- `journal::view_gloss_from_journal` and gloss's journal-view helper
  (`view_journal_from_gloss`) lose their only callers → delete IF no other
  caller exists (verify with rg before deleting; if another caller exists,
  leave the fn and only no-op the arm).
- Shared ask helpers (`journal::begin_passage_ask`, `begin_scene_ask`) are
  NOT deleted — the reader's Ctrl+a ask-passage path still uses them.

## Keybind mirrors (same change)

- `src/ui/gloss_keybinds_overlay.rs`: close row → `("Esc", "close (jump
  to source)")`; drop the `Ctrl+j` view-journal row and the `r` ask row.
- `src/ui/journal_keybinds_overlay.rs`: close row → `("Esc", "close →
  reader")`; drop `Ctrl+g` view-gloss and `Alt+g` gloss-passage rows from
  "Cross-reference" (keep `Ctrl+\` picker).
- `src/ui/synopsis_keybinds_overlay.rs`: close row → `("Esc", "close")`;
  drop the `r` and `Ctrl+j` journal rows.
- `src/ui/echo_keybinds_overlay.rs`: close row → `("Esc", "close echoes →
  reader")`; drop the `Ctrl+j` journal row.
- `src/ui/keybinds_overlay.rs` describe(): update "last overlay" text
  (reader-side reopen only). No keycap strip changes (reader binds are
  unchanged). No keymap.json changes (all dropped keys are overlay-handler
  internal, not reader-map entries).

## Testing

- `cargo test --bins` (existing suites; keymap unchanged).
- Headless cage e2e: for each of the five overlays — open it, press each
  dropped key, screenshot to confirm the overlay is STILL open and
  unchanged; press Escape, confirm return to reader. Run one `\` lap to
  confirm the cycle still works. Translation: confirm both `i` and Escape
  close. (TTS stop on advance is not headlessly assertable — no audio in
  cage; verified by code review only.)

## Out of scope

- Reader-mode binds, keymap.json, pickers, vim editors, chat layout.
- Any renaming of Action variants (Toggle* names stay).
