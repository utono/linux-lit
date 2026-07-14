# Paginated journal Q&A overlay — design

_2026-06-28 (US Central)_

## Problem

The journal Q&A overlay scrolls a single TextView. Block nav (`j`/`k`/`q`/`,`)
moves the cursor block and scrolls it into view, but the scroll lands at an
arbitrary offset, so a partial paragraph clips at the **top** edge (which has no
clip box — only the bottom does). Screenshot: the top paragraph is sliced
mid-line ("…notoriously slow procedure. It handled" cut in half) while the cursor
block sits at the bottom.

The robust fix (per `docs/troubleshooting/clip-prevention.md` → "Pagination
instead of a mask") is the main card's strategy: **paginate** — render only the
whole paragraph blocks that fit, so no partial block is ever rendered at either
edge. The 2-col translation overlay already does this
(`src/ui/translation_overlay.rs::paginate`).

## Design

Convert the journal overlay from **scroll** to **pagination** over its paragraph
blocks, reusing the translation overlay's pure pagination helpers.

### Shared helpers (move to `src/ui/pagination.rs`)

`paginate(block_heights, page_height) -> Vec<Page>`, `page_containing_block`, and
`struct Page { start, end }` currently live in `translation_overlay.rs`. Move
them verbatim to a new `src/ui/pagination.rs` (pure, already unit-tested) and
re-export so both overlays share one copy. Also move `measure_text_height`
(standalone `pango::Layout` height — no widget allocation, no settle race).

### Journal overlay changes (`src/ui/journal_overlay.rs`)

State additions:
- `all_blocks: RefCell<Vec<JournalBlock>>` — the FULL paragraph list for the
  current Q&A (today's `blocks` becomes the per-PAGE rendered list).
- `pages: RefCell<Vec<Page>>` — block-index ranges per page.
- `page_idx: Cell<usize>` — current page.
- The block cursor (`cursor_block`) indexes the FULL list.

On `show_page`:
1. Build the full block list from `question\n\nanswer` (the existing
   `journal_blocks` split, but on the full text — measured, not yet rendered).
2. Measure each block's height via `measure_text_height` at the view's font
   family + size and wrap width (`card_width − 2*side_margin`), adding the
   per-paragraph line spacing the rendered view adds (mirror `block_height`).
3. `pages = paginate(heights, page_height)` where `page_height` = the usable
   viewport height (the closed scroll budget the AskCardHost already computes:
   `card_height − title − UNACCOUNTED_CHROME_MARGINS − footer`).
4. `page_idx = page_containing_block(pages, cursor_block)` (cursor starts at 0).
5. `render_page()` — set the TextView buffer to ONLY the current page's blocks
   (joined by blank lines); re-derive the per-page `blocks` (their buffer-line
   spans) for the accent bar; mark the cursor block's bar (page-local).

Block nav:
- `cursor_next_block`/`cursor_prev_block` (and the `q`/`,` aliases) step
  `cursor_block` in the FULL list (clamped). If the new cursor leaves the current
  page's `[start,end)`, recompute `page_idx = page_containing_block(...)` and
  `render_page()`. Mark the bar at the cursor's page-local block.
- `gg`/`G` jump the cursor to the first/last block of the WHOLE Q&A (page 0 /
  last page), render, mark.

No scrolling: the buffer holds exactly the page's whole blocks, so it fits the
viewport with no partial row. The vadjustment stays at 0. The bottom-clip box is
no longer needed for the journal surface (like the translation overlay deleted
its clip machinery) — but keeping `BottomClipGuard` attached is harmless (it
clips 0 when nothing overflows). To minimize risk, KEEP the guard; it becomes a
no-op once pages never overflow. (If a single block is taller than a page,
`paginate` gives it its own page and the guard masks its trailing partial row —
the same over-tall fallback the main card uses.)

Visual mode (Shift+V): operates within the current page's rendered blocks (the
existing per-page `blocks`/anchor logic is unchanged — it already works on the
rendered buffer). Out of scope to make visual selection span pages.

### Footer position label

The footer's "page N of M in this scene" today means Q&A-entry N of M in the
band (Ctrl+n/p pages). That meaning is unchanged — do NOT repurpose it for the
new render-pages. The new render-pagination is *within* one Q&A entry and is not
surfaced in the footer (the page simply turns as the cursor moves). Optionally
append "· ¶ p/q" later; out of scope now.

## Why not keep scrolling + snap-to-top

Block-aligned scrolling would also remove the top clip, but the user chose full
pagination (main-card consistency, no partial block at EITHER edge, no scroll
position to manage). Pagination also matches the translation overlay, so the two
free-prose overlays share one proven model.

## Testing

- `paginate`/`page_containing_block` already unit-tested (moved verbatim).
- New: a pure test for the full→page→cursor mapping if any non-trivial logic is
  added beyond the shared helpers (e.g. "stepping the cursor past a page end
  selects the next page's first block").
- Build + `cargo test --bins`.
- Visual (user): j/k/q/, never clip a partial paragraph at the top or bottom;
  the cursor block is always fully visible; gg/G land on the first/last block.

## Files

- `src/ui/pagination.rs` — NEW: moved `Page`, `paginate`, `page_containing_block`,
  `measure_text_height` (+ their tests).
- `src/ui/translation_overlay.rs` — use the shared helpers (delete the moved copies).
- `src/ui/journal_overlay.rs` — paginate instead of scroll; render per page.
- `src/ui/mod.rs` — `pub mod pagination;`.
- `src/input/keymap.rs` — block-nav arms unchanged in shape (still call
  cursor_next_block etc.); no new binds.
