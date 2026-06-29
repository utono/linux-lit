# Synopsis overlay visual mode (Shift+V) — design

**Date:** 2026-06-19
**Status:** Approved, ready for implementation plan

## Summary

Add a vim-style **visual mode** to the synopsis overlay, mirroring the reader's
existing visual mode but operating on **paragraph blocks** instead of buffer
lines. In the synopsis overlay, `Shift+V` enters visual mode anchored at the
current paragraph; `j`/`k` (and `gg`/`G`) extend the selection block-by-block;
`y` yanks the selected paragraphs' text to the system clipboard (Wayland
`wl-copy`) and exits; `Escape` or `Shift+V` exits without copying. The selection
is shown by extending the overlay's existing left-edge cursor bar to span all
selected blocks.

## Motivation

The reader has a full visual mode (`InputMode::Visual`, `handle_visual_key`,
`src/input/visual.rs`) but it operates on the main reading `TextBuffer` by line
index — a different widget from the synopsis overlay, which is a separate
`gtk4::TextView` (`gloss_view`) navigated by a **block (paragraph) cursor**
(`cursor_block`, `mark_cursor_block`, `bar_ranges`). The user wants the same
select-and-yank capability inside the synopsis overlay. Because the overlay
already moves a block cursor with `j`/`k`, the natural unit of selection is the
paragraph block, and the existing bar is the natural selection indicator.

## Decisions (from brainstorming)

- **Granularity:** per-paragraph block (the unit `j`/`k` already move).
- **Highlight:** extend the existing cursor bar to span `anchor..=cursor`. No
  new TextTag, no text-background tint (avoids conflict with the overlay's
  speaker/verse/label tags).
- **Yank text:** plain paragraphs with `<p>` tags stripped, joined by a blank
  line (matches on-screen reading).
- **After yank:** copy, show a brief "Copied" toast, exit to the synopsis
  overlay with the cursor left on the last-selected block.

## Non-goals

- No reuse of the reader's `AppState.visual_selection` / `SelectionState` (those
  are line-based over `state.buffer`; the overlay is a different widget).
- No action popup (the reader's `Return` → gloss/inner-monologue menu). Synopsis
  visual mode is select + yank only.
- No per-line or per-sentence selection.
- No background-tint highlight.

## Architecture

### State (in `GlossOverlay`, `src/ui/gloss_overlay.rs`)

Add alongside the existing `cursor_block: Cell<usize>`:

```rust
/// `Some(block_index)` while synopsis visual mode is active (the anchor end of
/// the selection); `None` in normal synopsis navigation. The cursor end is the
/// existing `cursor_block`. The selected range is
/// `anchor.min(cursor)..=anchor.max(cursor)`.
synopsis_visual_anchor: Cell<Option<usize>>,
```

### Input mode (in `src/app.rs`)

Add `SynopsisVisual` to the `InputMode` enum (parallel to `SynopsisOverlay`).

### Routing (in `src/input/keymap.rs`)

- Dispatch `InputMode::SynopsisVisual` to a new `handle_synopsis_visual_key`
  (parallel to the `SynopsisOverlay => handle_synopsis_overlay_key` arm).
- In `handle_synopsis_overlay_key`, add a `"V"` arm → enter visual mode.

### Selection-range helper (pure, unit-tested)

A free function (so it is testable without GTK):

```rust
/// Inclusive block range for a visual selection given anchor and cursor block
/// indices. Direction-independent.
pub fn visual_block_range(anchor: usize, cursor: usize) -> (usize, usize) {
    (anchor.min(cursor), anchor.max(cursor))
}
```

## Keybindings & control flow

**In `InputMode::SynopsisOverlay`:**
- `Shift+V` (GTK key_name `"V"`) → `enter_synopsis_visual`: if there are blocks,
  set `synopsis_visual_anchor = Some(cursor_block)`, set
  `input_mode = SynopsisVisual`, re-span the bar (single block initially), set
  the visual-mode footer hint. No-op if no blocks.

**In `InputMode::SynopsisVisual` (`handle_synopsis_visual_key`):**
- `j` → move `cursor_block` +1 (clamped), re-span bar, scroll into view.
- `k` → move `cursor_block` -1 (clamped), re-span bar, scroll into view.
- `G` → cursor to last block, re-span, scroll.
- `g` → start `ChordState::PendingG`; a following `g` → cursor to first block,
  re-span, scroll. (Same chord mechanism the synopsis handler already uses.)
- `y` → `selected_synopsis_text()` → `wl-copy` → log + "Copied" toast →
  `exit_synopsis_visual` (back to `SynopsisOverlay`).
- `Escape` or `V` → `exit_synopsis_visual` with no copy.
- All other keys → consumed (no-op), like `handle_visual_key`.

Entering visual mode does NOT move the cursor (anchor = current block), matching
vim and the reader's `enter_visual_mode`.

**Exit** (`exit_synopsis_visual`): set `synopsis_visual_anchor = None`, set
`input_mode = SynopsisOverlay`, redraw the bar as the single cursor block,
restore the synopsis-overlay footer hint.

## Rendering (bar span)

Today `mark_cursor_block` sets `bar_ranges` to one `BarRange { start_line,
end_line }` for the cursor block. Add a span path used while visual mode is
active: compute `(start, end) = visual_block_range(anchor, cursor)`, then set one
`BarRange` from `blocks[start].start_line` to `blocks[end].end_line`. When
`synopsis_visual_anchor` is `None`, behavior is unchanged (single block). No new
tags or draw code — only the computed `bar_ranges` span changes.

## Yank

`GlossOverlay::selected_synopsis_text() -> String`:
- `(start, end) = visual_block_range(anchor, cursor)`,
- for each block in `start..=end`, take `GlossBlock.display` (already the `<p>`-
  stripped paragraph text) from `synopsis_blocks(synopsis)` — the SAME block list
  the cursor indexes, so the range math and the text extraction agree,
- join with a blank line (`"\n\n"`).

Note: `synopsis_blocks` skips label paragraphs (e.g. "Shakespearean parallels:")
as non-cursor-stops, so its indices are the cursor-stop indices that
`cursor_block`/`anchor` already use. Both the bar span (which reads
`blocks[i].start_line`/`end_line` from the same cursor-stop list via
`rebuild_block_ranges_from`) and the yank text are over this one consistent
block list — do not introduce a second, differently-indexed paragraph list.

The handler copies via `std::process::Command::new("wl-copy").arg(text).spawn()`
(the pattern from `copy_gloss_id`), logs `SYNOPSIS: copied N blocks`, and shows
the existing `chapter_toast` with "Copied" for ~2s.

## Data flow

1. `SynopsisOverlay` + `Shift+V` → `enter_synopsis_visual` (anchor set, mode
   `SynopsisVisual`, bar spans one block, visual footer).
2. `j`/`k`/`G`/`gg` → move cursor, re-span bar, scroll cursor block into view.
3. `y` → build text → `wl-copy` → toast → `exit_synopsis_visual`.
4. `Esc`/`Shift+V` → `exit_synopsis_visual` (no copy).

## Error handling / edge cases

- No blocks → `Shift+V` is a no-op (mode stays `SynopsisOverlay`).
- Selection can never be empty (anchor is always a valid block).
- `wl-copy` failure → logged, non-fatal (matches `copy_gloss_id`).
- Single-block selection (no j/k) → `y` copies that one paragraph.
- `cursor_block` clamping reuses the existing `step_cursor`/`cursor_to_end`
  bounds logic.

## Mandatory project touch points

- **`keymap.json`:** no change — synopsis handlers match `key_name` directly,
  not via the reader's `keymap.json`/`Action` dispatch.
- **Ctrl+/ keybinds overlay** (`src/ui/keybinds_overlay.rs`): document the
  synopsis-context `Shift+V` (enter visual) and the visual-mode `y` (yank
  paragraphs) via the `update-cairo-keybinds-overlay` skill (three-pass
  cross-check).
- **Footer hints** (`src/ui/gloss_overlay.rs`):
  - synopsis-overlay footer: append `· ⇧V select`.
  - new visual-mode footer: e.g.
    `⇧V/Esc exit · j/k extend · gg/G ends · y yank`.

## Testing

- **Pure unit tests** (`cargo test --bins`): `visual_block_range` (anchor/cursor
  → inclusive range, both directions, single block) and `selected_synopsis_text`
  paragraph extraction (`<p>`-strip + blank-line join over a known
  `synopsis_blocks` input). These are the overlay's first pure tests.
- **Visual / side-effecting** (user verifies on screen): bar span growth on
  `j`/`k`, `gg`/`G`; `y` copies the right paragraphs (paste to confirm); toast;
  exit restores single-block bar and normal navigation. The agent cannot
  reliably launch cage in the live dwl session, so the user runs `cargo run`,
  opens the synopsis overlay (`h`), and exercises Shift+V / j / k / y.

## Files touched

- `src/app.rs` — add `InputMode::SynopsisVisual`.
- `src/ui/gloss_overlay.rs` — `synopsis_visual_anchor` field; `visual_block_range`
  (pure); bar-span path; `enter_synopsis_visual` / `exit_synopsis_visual`;
  `selected_synopsis_text`; footer-hint strings; pure tests.
- `src/input/keymap.rs` — `SynopsisVisual` dispatch arm; `handle_synopsis_visual_key`;
  `"V"` enter arm in `handle_synopsis_overlay_key`.
- `src/ui/keybinds_overlay.rs` — Ctrl+/ documentation for Shift+V / y.
