# Paginated 2-col translation overlay

**Date:** 2026-06-27
**Branch:** `fix/inline-translation-clip`
**Status:** Design — approved (end-to-end, no review gates)

## Problem

The 2-col translation overlay (`i` → `ShowTranslationOverlay`) renders the whole
scene as a **scrolled** `Box` of paired original/translation `TextView` columns.
Because the columns are wrapping TextViews inside a Box, the bottom row at the
viewport edge is a **partial wrapped row** that the box-slack clip guard can't
mask, and a per-row mask across two independently-wrapping columns proved fragile
(coordinate-mapping bugs, a top row clipped on an un-snapped scroll, the cursor
highlight off-screen on open). Multiple scroll-model fixes did not fully resolve
the clipping.

## Decision

**Paginate the overlay like the main reading card.** Render **one page at a
time**, holding only the whole speaker blocks that fit in the card height, with
the last block ending above the bottom edge. A block is never split across a
page, and the page never overflows — so **no partial row is ever rendered, and
the entire bottom-clip mechanism is deleted.** The clip bug class disappears by
construction.

This is a rewrite of the overlay's layout + navigation, not a clip patch.

## Approved decisions (from brainstorming)

1. **Page unit = whole speaker block.** A block is `group_scene_into_blocks`'s
   unit: a speaker label + its paired `(orig, trans)` lines (or a non-spoken
   interlude with a single full-width view). A block is never split across pages;
   a page holds as many consecutive blocks as fit. Page height budget is driven
   by `max(orig_height, trans_height)` per block.
2. **Navigation = page-turn keys + cursor follows (mirrors the main card).** The
   overlay reuses the READER's cursor (`current_line`) — it has no separate
   cursor model. The existing `comma`/`q`/`j`/`k` binds already drive the real
   reader cursor via the main-card nav fns; on cursor move, the overlay shows the
   PAGE containing the cursor's block and highlights that block. Opening the
   overlay and playback-sync both land on the cursor's page. (A dedicated
   page-only key is not added; paging happens by moving the cursor, exactly as
   the existing binds already do — see "Navigation" below.)

## Architecture

### Components

- **`TranslationOverlay` (rewritten, `src/ui/translation_overlay.rs`)**
  - Holds the full scene's `blocks: Vec<TranslationBlock>` (unchanged grouping),
    plus `pages: Vec<Page>` and `current_page: Cell<usize>`.
  - A `Page` is a contiguous `start_block..end_block` (exclusive) range of block
    indices that fit one card height.
  - Renders a plain **non-scrolling** `content_vbox` (a `gtk4::Box`, no
    `ScrolledWindow`) holding only the current page's block widgets.
  - **Deleted:** the `ScrolledWindow`, `scroll_overlay`, `clip_guard`
    (`attach_custom`/`Custom`/`recompute_translation_bottom_clip`/`clip_views`),
    `scroll_to_highlight`. The bottom-clip machinery added in commit `0b5fdc9`
    and the `Custom`/`attach_custom`/`ClipFn` additions to
    `bottom_clip_guard.rs` are removed (the `attach_box` + box-slack guard stay
    for other potential Box surfaces; if `attach_box`/`Box`/`recompute_overlay_bottom_clip_box`
    become fully unused, leave them with the existing `#[allow(dead_code)]`).

- **Page-fitting (pure, in `translation_overlay.rs`)**
  - `paginate(block_heights: &[i32], page_height: i32) -> Vec<Page>` — pure
    function: greedily packs consecutive blocks until the next would exceed
    `page_height`; a single block taller than `page_height` gets its own page
    (never silently dropped). **Unit-tested.**
  - `block_height(block, col_width, font_family, font_size, header_pt) -> i32`
    — measures a block's rendered height = header height (if any) + the
    `max(orig_lines_height, trans_lines_height)` + the block's top margin, using
    a **`pango::Layout`** at `col_width` (synchronous, deterministic — NO GTK
    widget measurement, avoiding the settle-timing races that plagued the scroll
    version). The interlude (no-speaker) block measures its single full-width
    view at the full text width.

### Data flow

1. `rebuild_translation_overlay` (in `src/app/translations.rs`) groups the scene
   into `blocks` (unchanged), then calls `overlay.show(...)` with the same args
   it passes today **plus** the cursor's work index so the overlay can open on
   the right page.
2. `show()`:
   - stores `blocks`,
   - computes each block's height via `block_height`,
   - `paginate(...)` → `pages`,
   - picks `current_page` = the page whose block range contains the cursor's
     block (fallback: page 0),
   - renders that page's blocks into `content_vbox`,
   - highlights the cursor block (if on this page).
3. On cursor move, `sync_translation_overlay` calls `overlay.show_for_cursor(w)`:
   - find the page containing the cursor's block,
   - if it differs from `current_page`, re-render that page,
   - highlight the cursor block.

### Navigation

The existing `handle_translation_overlay_key` binds are **unchanged** in what
they call (`jump_to_prev_dialogue`/`jump_to_next_dialogue`/`cursor_next_dialogue`/
`cursor_prev_line` via `overlay_nav` → `sync_translation_overlay`). Only the
overlay's *response* to a cursor move changes: instead of `scroll_to_highlight`
(scroll), `sync_translation_overlay` calls the new `show_for_cursor`, which turns
to the cursor's page and highlights the block. So `j`/`k`/`q`/`,` move the reader
cursor and the overlay follows by paging — exactly the main-card model, with no
new keybinds and no Ctrl+/ overlay changes.

### Highlighting

`highlight_work_line(work_idx)` is kept but simplified: it locates the block
containing `work_idx`, and if that block is on the **current page**, applies the
`cursor-line` paragraph-background tag to the matching line in its `orig` (and
`trans`) view. If the block is off-page, `show_for_cursor` will have already
turned to its page before highlighting, so the highlight is always on a rendered
block (this fixes the "highlight only after a nav key" symptom — there is no
scroll-settle timing because the page is rendered synchronously around the
cursor).

## What is explicitly NOT changed

- `group_scene_into_blocks` and `TranslationBlock` (the grouping is reused).
- The key binds themselves (`handle_translation_overlay_key`), playback,
  playback-sync, `s`/`a`/`space`/`Tab`/`Escape`/`i`.
- The speaker-label font fix (`95d2969`), the inline-translation (`Ctrl+Alt+i`)
  fixes, and the synopsis/gloss/journal overlays.
- The main reading card and its pagination.

## Error handling / edge cases

- **A block taller than a full page** (a very long speech with many wrapped
  lines): it gets its own page; the bottom may still exceed the card, but since
  it's the only block on the page and the overlay is reference material, this is
  acceptable and rare. (The main card has the analogous over-tall-paragraph case;
  here we simply don't split a speaker turn.) `paginate` must place such a block
  alone on a page, never drop it.
- **Empty scene** → `rebuild` already returns false; unchanged.
- **Cursor not in any block** (e.g. on a chrome line) → fall back to page 0 / no
  highlight, same as today's `locate_line` miss.
- **Card resize** → on the next `show`/rebuild the heights + pages recompute from
  the current `card_height`. (No live re-pagination on resize while open; the
  overlay is modal and the window rarely resizes under it.)

## Testing

- **Pure unit tests** (`#[cfg(test)]` in `translation_overlay.rs`):
  - `paginate`: packs consecutive blocks until full; an over-tall single block
    gets its own page; exact-fit boundary; empty input → empty pages.
  - `page_containing_block`: returns the page index whose range contains a given
    block; clamps for out-of-range.
- **No new GTK measurement tests** (consistent with the rest of the overlay).
- **Runtime verification is visual** (CLAUDE.md): the user opens `i` on a scene
  longer than one page and confirms (a) no top/bottom clipping ever, (b) the
  cursor-line highlight is visible immediately on open, (c) `j`/`k`/`q`/`,` turn
  pages as the cursor crosses page boundaries, (d) playback-sync turns pages.
- `cargo build` + `cargo test --bins` stay green.

## Migration / cleanup

This supersedes the scroll-based bottom-mask (commit `0b5fdc9`). The
implementation removes that machinery. `clip-prevention.md`'s
checklist #8 + the `recompute_translation_bottom_clip` references are updated to
note the translation overlay now paginates (no mask needed); the box-of-TextViews
caveat stays as a general warning.
