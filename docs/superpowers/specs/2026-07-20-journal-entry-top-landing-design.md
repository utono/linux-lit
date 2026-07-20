# Journal-entry creation lands cursor at top of the new entry

Date: 2026-07-20
Status: approved

## Problem

After creating a Journal Q&A from the chat panel, the panel ends up
scrolled to the bottom and the question is not reachable at all:

- `render_current_question` (`src/input/actions/chat.rs:1041`) builds
  `[Q, Answer]` but renders the **last** page; the answer is a single
  oversized widget, so the `Q:` row lives alone on page 0 and is not in
  the widget tree. Scrolling up never reveals it.
- `render_page` (`src/ui/chat_panel.rs:258`) never resets the
  vadjustment, so an oversized page inherits the previous scroll
  position ("scrolled to bottom").
- The Question view has no row cursor; j/k/gg/G degrade to plain
  scrolling (`chat.rs:1793`, `1841`, `~1880`) — no accent bar, no block
  navigation.

The journal overlay's ask flow already lands correctly (newest entry,
block cursor 0, scroll parked at top — `journal.rs:2519-2523`,
`journal_overlay.rs:698-703`, `1826-1828`). The gloss overlay's ask-card
Add path (`persist_and_render_gloss`, `gloss.rs:992-1048`) creates a
gloss Q&A (separate `glosses` table) and its landing has not been
audited.

## Desired behavior

After creating an entry, the rendering surface shows the entry from its
top with the block cursor on the first block:

1. **Chat panel** (answer-completion render): show `[Q, answer
   paragraphs...]`, page 0, scrolled to top, accent-bar cursor on the
   `Q:` block. No source-passage text in any chat-panel render of the
   entry (source stays journal-overlay-only).
2. **Journal overlay**: unchanged (already correct); verify headlessly.
3. **Gloss overlay**: after ask-card Add creates a gloss entry, land
   cursor/scroll at the top of the new entry.

## Design

- **Per-paragraph answer rows**: `render_current_question` splits the
  answer with the existing `split_answer_paragraphs`
  (`chat_rows.rs:186-193`) instead of one `Answer` widget, so the entry
  paginates correctly and each paragraph is a navigable block.
- **Top landing + cursor**: `render_current_question` sets
  `s.chat.row_cursor = 0` and renders via the paginated path
  (`render_paginated(s, &rows, Some(0), None)`) so page 0 shows with the
  accent bar on `Q:` and `s.chat.pages`/`page_idx` become authoritative
  for later navigation.
- **Scroll reset**: `render_page` resets the transcript scroll
  vadjustment to 0.0 on every rebuild. This also fixes the inherited
  bottom-scroll for `render_saved_entry` (the `s`-key save path).
- **Paragraph-level j/k stepping**: replace the Question-view
  scroll-degrade guards in the j/k/gg/G handlers with the same paged
  cursor stepping the Gloss/Journal arms use, so j/k walk Q → paragraph
  → paragraph across pages.
- **Gloss overlay landing**: audit `persist_and_render_gloss` →
  `show_gloss_with_color`; if the new entry does not land top-scrolled
  with the cursor on its first block, set the overlay's entry
  index/cursor/scroll the same way the journal overlay's ask path does.

## Out of scope

- Any change to journal-overlay rendering or its source-text display.
- Streaming/incremental answer rendering (answers arrive atomically).
- The chat panel's Journal and Gloss views (only the answer-completion
  Question view and the shared `render_page` scroll reset change).

## Testing

- `cargo test --bins` for row-model changes (paragraph split, page 0
  landing index).
- Headless cage e2e (`verify-overlay-ui` flow): create a Q&A in the
  chat panel, screenshot, confirm `Q:` at top with the accent bar and
  no source text; j/k step through paragraphs. Repeat for the journal
  overlay (regression check) and the gloss overlay Add path.
