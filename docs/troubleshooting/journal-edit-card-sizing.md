# (superseded) Journal edit card sizing

The journal `e` **edit card** (`JournalEditCard`) was **removed** in favor of
in-place modal **vim editing** on the journal page. `e` now enters a vim editor
(`InputMode::JournalEdit`) that shows the whole Q&A as one editable buffer; `:w`
saves, `:q`/Esc cancels, `R` opens the LLM-rewrite prompt.

There is no longer an edit card to size, so the multi-round card-overflow saga
this file documented no longer applies. See:

- `docs/plans/2026-06-30-journal-vim-edit-design.md` — the vim-editor design.
- `docs/plans/2026-06-30-journal-vim-edit-plan.md` — the implementation plan.
- `src/input/vim/` — the pure engine; `src/ui/journal_overlay.rs`
  (`enter_edit_buffer` / `mirror_engine` / `feed_edit_key`) — the GTK adapter.

This file is kept as a tombstone so existing links don't 404.
