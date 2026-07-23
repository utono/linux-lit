# Recent Q&A jump-back — design

**Backlog item #10.** A quick way to reopen the last few Q&A answers the
reader generated *across works*, without hunting through the current-work
journal picker.

## Problem

The Q&A you just made is often the one you want again a minute later. Today the
only list is `Alt+j` → `OpenJournalPicker`, which is **current-work only** and
sorted band-wise (reading order, oldest-first within a band) — not by recency,
and blind to answers made in other works. Recency across works is unreachable
without switching works and browsing.

## Key facts (from codebase survey)

- `journal_entries` already has `timestamp TEXT NOT NULL DEFAULT (datetime('now'))`,
  bumped on edit/rewrite. **No schema change needed.** `id` (rowid alias) is a
  monotonic tiebreaker.
- No cross-work recency query exists yet. Closest primitive: `find_page_by_id`
  (`src/db/journal.rs:373`) — cross-work single fetch, includes `work_abbrev`.
- The cross-work *open* path already exists and is reused verbatim:
  `load_arkangel_edition_then` (`src/input/actions/pickers.rs:241`, switches
  `current_work` if different, discovers MPV media, skips reload when same-work)
  → `find_page_by_id` → `open_journal_hit` / `render_filtered_match`
  (`src/input/actions/corpus_search.rs:206`).
- `Ctrl+a` is explicitly unbound (`AskPassage` removed; asserted `None`).

## Decisions

- **Surface:** a new cross-work picker, `RecentQaPicker`, modelled on
  `JournalQaPicker` (`src/ui/journal_picker.rs`) but each row is **work-labeled**
  (`TT · tone shift in the…`) and the list is sorted **newest-first across all
  works**. Subsequence-filterable like the existing picker.
- **Keybind:** `Ctrl+a` (natural "ask/answer recall" mnemonic, adjacent to the
  ask flow). Main-card bind → update the `Ctrl+/` overlay + keymap.json in the
  same change.
- **Size:** 15 entries, filterable — enough for "the one I made a minute ago"
  plus a session's worth, without becoming a full browser.

## Components

1. **Query** — new `find_recent_pages(conn, limit) -> Vec<TermMatch>` in
   `src/db/journal.rs`: `... ORDER BY timestamp DESC, id DESC LIMIT ?`, selecting
   the same columns as `find_page_by_id` (so it carries `work_abbrev`). No
   `timestamp`-only index today; a `LIMIT 15` scan over `journal_entries` is
   cheap at current table sizes — no index added (note it, revisit only if the
   table grows large).
2. **Picker widget** — `RecentQaPicker` in `src/ui/recent_qa_picker.rs`
   (new file). Row = `{ id, work_abbrev, work_label, question_prefix }`. Renders
   `two_label_row(work_label, question_prefix)`; subsequence filter over the
   combined text. Same overlay-layer construction as `journal_picker.rs` (an
   `add_overlay` layer, never in the size-bearing chain).
3. **Action + open path** — new `Action::OpenRecentQaPicker`. Handler queries
   `find_recent_pages`, populates the picker, and on confirm resolves the entry
   by id via the existing cross-work open sequence
   (`load_arkangel_edition_then` → `find_page_by_id` → `render_filtered_match`),
   so a different-work entry loads its work + MPV media and opens the overlay to
   that entry. Same-work entries skip the reload (the shared loader already
   short-circuits).
4. **Keybind wiring** — `Ctrl+a` → `OpenRecentQaPicker` in `keymap_config.rs`;
   mirror in the stowed `keymap.json`; add to the `Ctrl+/` Cairo overlay
   (`src/ui/keybinds_overlay.rs`, keycap strip + describe() arm) via the
   `update-cairo-keybinds-overlay` three-pass cross-reference.

## Data flow

`Ctrl+a` → `OpenRecentQaPicker` → `find_recent_pages(conn, 15)` → populate
`RecentQaPicker` (work-labeled, newest-first) → filter/select → confirm → resolve
id → `load_arkangel_edition_then` (load work if different) → `find_page_by_id` →
`render_filtered_match` (open overlay at entry, seek MPV). Escape closes the
picker back to the reader.

## Error handling

- Empty result (no Q&A anywhere): picker opens with an empty-state row
  ("No Q&A yet — press Ctrl+a after asking.") or a toast; do not crash on an
  empty list. (Match the existing picker's empty handling.)
- `find_page_by_id` returns `None` (entry deleted between list and confirm):
  toast "Entry no longer exists" and stay in the reader.
- Cross-work load failure: `load_arkangel_edition_then` already toasts on media
  discovery failure — reuse, don't reinvent.

## Testing

- `cargo test --bins` for `find_recent_pages` ordering (insert rows across two
  works with known timestamps; assert newest-first, `LIMIT` honored, tiebreak by
  id).
- Headless (cage): `Ctrl+a` opens the picker; confirm it lists entries from more
  than one work newest-first; select a different-work entry and confirm the work
  switches and the overlay opens to that entry. Key names verified against
  `keymap_config.rs` before scripting.

## Out of scope

- No "recent notes" (kind='note') filtering toggle — recency over all Q&A rows;
  revisit if noise is a problem.
- No pinning/favorites. No preview pane. No deletion from this picker.
