# Echoes Overlay (press `I` on a line)

**Date:** 2026-05-30

## Problem

The semantic echo search (Phase 2) is wired into inner-monologue gloss creation: it suggests cross-work echoes before a Claude call. But a reader may simply want to *see* which lines elsewhere in Shakespeare echo the meaning of the line under the cursor — without creating a gloss or calling Claude.

## Solution

Press `I` (Shift+i) on any line in reader mode. The app embeds the cursor line's speaker turn, finds the most semantically similar passages from other works, and displays them in the gloss overlay card (the cream text card) — formatted like inner-monologue echoes: the source turn as a header, then a scrollable list of echoes, each an italic quote with its citation indented below. Pure reference for the cursor line; no gloss is created.

## Trigger

New `Action::ShowEchoes` bound to `I` in reader mode. Added to compiled defaults (`keymap_config.rs`) and the stow keymap (`keymap.json`).

## Data Flow

1. Resolve the cursor line to its `Line`, then gather its **speaker turn** — the contiguous block of lines spoken by the same speaker that contains the cursor line.
2. Build the enriched query string `"{SPEAKER} to {ADDRESSEE}: {turn text}"`, matching the format used in `scripts/build_embeddings.py` and `build_echo_query` in `visual.rs`.
3. `crate::voyage::embed_query(query)` → embedding vector.
4. `crate::db::queries::find_similar_passages(conn, &embedding, current_work, 15)` → top 15 `EchoCandidate`s (excludes the current work).
5. Load work titles via `load_work_titles`.
6. Format the echoes into a gloss-style document string:
   - `<speaker>NAME</speaker>` then `<verse>line</verse>` for each line of the source turn (the header)
   - then `<gloss>["echo text" — Work Title act.scene]</gloss>` for each echo
7. Render via `gloss_overlay.show_gloss_with_color`, reusing the existing echo quote/citation rendering (italic quote on its own line, citation indented below).

The embed + search runs on the tokio runtime (like the gloss path); the overlay shows a loading state first, then the result.

## New Input Mode

`InputMode::EchoesOverlay` with handler `handle_echoes_overlay_key`:

- **Ctrl+n / Ctrl+p** — move the selected-echo highlight down / up
- **Enter** — copy the selected echo line + citation to the clipboard via `wl-copy`, then keep the overlay open
- **j / k** — scroll the card
- **g / G** — scroll to top / bottom
- **Esc** — close, return to reader

## Selection Highlight

The selected echo (the one Enter copies) gets a subtle background highlight on its quote line. Ctrl+n/p moves the highlight; the card auto-scrolls to keep it visible. The highlight is a GTK text tag applied to the selected echo's quote line, re-applied on each Ctrl+n/p.

Implementation: the echo render path takes a `selected_echo: usize` parameter. The buffer-population code records each echo's quote buffer-line as it inserts it, and applies a `gloss-echo-selected` highlight tag (subtle background) to the selected echo's quote line. On Ctrl+n/p, update `echo_overlay_index` and re-render the card with the new selection (re-rendering is cheap for ~15 items and already happens for gloss navigation). After re-render, scroll the card so the selected quote line is visible.

## State

New `AppState` fields:

- `echo_overlay_candidates: Vec<EchoCandidate>` — the echoes currently shown
- `echo_overlay_index: usize` — the selected echo index
- `echo_overlay_titles: HashMap<String, String>` — work-abbrev → title, for citations and copy text

## Reuse

- `crate::voyage::embed_query` — query embedding
- `crate::db::queries::find_similar_passages`, `load_work_titles`, `EchoCandidate`
- `gloss_overlay` echo rendering (quote/citation tags, `split_echo`)
- Speaker-turn gathering logic (adapt from `action_inner_monologue` in `visual.rs`)

## New Code

- `Action::ShowEchoes` variant + `I` binding
- `show_echoes_for_cursor_line(state, tokio_handle)` — gather turn → embed → search → format → render → set `InputMode::EchoesOverlay`
- `handle_echoes_overlay_key` in `keymap.rs`
- `InputMode::EchoesOverlay` enum variant
- AppState fields above
- A `selected_echo` highlight in `populate_gloss_buffer` (or a sibling render path)

## Out of Scope

- No gloss creation, no Claude call
- No jump-to-work navigation (Enter copies only)
- No filtering/search within the popup

## Risks

- **Empty results:** if no candidates (embed fails or VOYAGE_API_KEY unset), show a toast/message and stay in reader mode rather than opening an empty card.
- **Cursor line has no speaker:** if the cursor is on a stage direction or blank line, fall back to embedding just the line's text, or show "no echoes for this line."
- **Highlight re-render cost:** re-rendering the whole buffer on each Ctrl+n/p is acceptable (lists are ~15 items); if it flickers, switch to moving a single highlight tag instead.
