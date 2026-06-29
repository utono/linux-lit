# Paginated Translation Overlay — Implementation Plan

**Goal:** Replace the scrolled 2-col translation overlay with whole-block pagination (like the main card), eliminating the partial-row clipping by construction.

**Spec:** `docs/superpowers/specs/2026-06-27-paginated-translation-overlay-design.md`

**Branch:** `fix/inline-translation-clip`

## Global constraints

- Page unit = whole speaker block; never split a block; a block taller than a page gets its own page (never dropped).
- Reuse the reader cursor (`current_line`); no separate cursor model; no new keybinds.
- Heights measured with `pango::Layout` (synchronous) — NOT GTK widget measurement (avoid settle races).
- `cargo build` + `cargo test --bins` green.
- Verse/main-card/inline-translation/font fixes untouched.

## Tasks

### Task 1 — Pure pagination core + tests

**File:** `src/ui/translation_overlay.rs`

- Add `Page { start: usize, end: usize }` (block index range, `end` exclusive).
- `paginate(block_heights: &[i32], page_height: i32) -> Vec<Page>`: greedy pack; a block taller than `page_height` alone on its page; empty input → empty.
- `page_containing_block(pages: &[Page], block_idx: usize) -> usize`: first page whose range contains `block_idx`; clamp to last page if out of range; 0 if empty.
- `#[cfg(test)]`: pack-until-full; over-tall-block-alone; exact-fit; empty; page_containing across ranges + clamp.
- Verify: `cargo test --bins paginate` / `page_containing`.

### Task 2 — Pango block-height measurement

**File:** `src/ui/translation_overlay.rs`

- `block_height(block: &TranslationBlock, col_width, full_width, font_family, font_size, header_pt, block_margin_top) -> i32`:
  - speaker block: `block_margin_top + header_height + max(measure(orig_text, col_width), measure(trans_text, col_width)) + header_margin_bottom`.
  - interlude (no speaker): `block_margin_top + measure(joined_text, full_width)`.
  - `measure(text, width)` uses a `pango::Layout` with the body font at `width` (px → `width * pango::SCALE`), `set_wrap(WordChar)`, returns `pixel_size().1`.
  - header height measured with a `pango::Layout` at `header_pt` (small-caps doesn't change height materially; use the font at header_pt).
- These constants come from the existing `show`: `block_margin_top = 14`, `header_margin_bottom = 4`, `side_margin = card_width/12`, `col_width = ((card_width - 2*side_margin)/2 - 12).max(120)`, `full_width = card_width - 2*side_margin`, `header_pt = (font_size*0.75).round().max(8)`.
- No dedicated unit test (Pango needs a context); covered by the visual check. Keep the fn small and obviously correct.

### Task 3 — Rewrite `TranslationOverlay` struct + `new()` (drop scroll/clip)

**File:** `src/ui/translation_overlay.rs`

- Struct: remove `scrolled`, `clip_guard`, `clip_views`. Add:
  - `content_vbox: gtk4::Box` (plain, non-scrolling, appended directly to `container` with a bottom margin).
  - `blocks: RefCell<Vec<TranslationBlock>>`, `pages: RefCell<Vec<Page>>`, `current_page: Cell<usize>`.
  - keep `block_widgets: RefCell<Vec<BlockEntry>>` (now only the current page's rendered blocks).
  - store render context needed to re-render a page on `show_for_cursor`: `card_width/height`, `text_fg`, `dim_fg`, `body_font_size`, `font_family`, `cursor_line_bg`, `header_pt`, `side_margin`, `col_width`, `full_width` — bundle into a `RenderCtx` struct in a `RefCell`.
- `new()`: build `content_vbox`, append to `container` (no `ScrolledWindow`, no `scroll_overlay`, no guard).

### Task 4 — `show()` paginates and renders the cursor's page

**File:** `src/ui/translation_overlay.rs` + caller in `src/app/translations.rs`

- `show(...)` signature gains `cursor_work_idx: Option<usize>` (caller already computes `cursor_idx`).
- Body:
  - store `blocks`, compute `RenderCtx`, measure `block_heights`, `paginate` → store `pages`.
  - `page_height = card_height - title_height - bottom_margin` (title is `gloss-title`; measure its preferred height or reuse the known chrome: title ~ `24 top + line + 8 bottom`; use `self.title.preferred_size().1.height()` — it's realized by now since title text is set; fall back to a constant if 0).
  - `current_page` = page containing the cursor block (via `block_for_work_idx` + `page_containing_block`), else 0.
  - call `render_page(current_page)`.
  - highlight the cursor block (Task 6).
- Extract the per-block widget construction (the existing `for block in blocks` body) into `render_page(page_idx)`: clears `content_vbox` + `block_widgets`, renders only `pages[page_idx]`'s blocks, sets `current_page`.
- Caller (`rebuild_translation_overlay`): pass `cursor_idx` to `show`.

### Task 5 — `show_for_cursor` (page-follow on cursor move)

**File:** `src/ui/translation_overlay.rs` + `sync_translation_overlay` in `src/app/translations.rs`

- `show_for_cursor(&self, work_idx: usize)`: find the block for `work_idx`; the page containing it; if `!= current_page`, `render_page(that)`; then `highlight_work_line(work_idx)`.
- Replace the `highlight_work_line` + `scroll_to_highlight` pair in `sync_translation_overlay` with a single `show_for_cursor(w)`. Delete `scroll_to_highlight`.
- `block_for_work_idx(work_idx) -> Option<usize>`: scan `blocks` for the one whose `start_idx..=end_idx` contains `work_idx` (mirrors `locate_line`'s range logic but over the stored blocks).

### Task 6 — Highlight on the rendered page

**File:** `src/ui/translation_overlay.rs`

- `highlight_work_line(work_idx)`: clear cursor tag on all current `block_widgets`; find the block for `work_idx`; if it's in the current page's range, find its `BlockEntry` and the line offset, `apply_cursor_tag` to `orig` (and `trans`). Off-page → no-op (the caller pages first).
- This removes the scroll-settle timing — the page is rendered synchronously, so the tag paints immediately.

### Task 7 — Delete the clip machinery + docs

**Files:** `src/ui/translation_overlay.rs`, `src/ui/mod.rs`, `src/ui/bottom_clip_guard.rs`, `docs/troubleshooting/clip-prevention.md`

- Remove `recompute_translation_bottom_clip` + `view_rows_in_container` from `mod.rs` if now unused (grep first).
- Remove `ClipKind::Custom`, `ClipFn`, `attach_custom` from `bottom_clip_guard.rs` if now unused (the translation overlay was their only user). Keep `attach_box`/`Box`/`recompute_overlay_bottom_clip_box` (general; mark `#[allow(dead_code)]` if unused).
- `clip-prevention.md`: update checklist #8 + the box-of-TextViews bullet to note the translation overlay now **paginates** (no mask needed); keep the general caveat.
- Verify nothing else references the removed symbols (`rg`).

### Task 8 — Build, test, verify

- `cargo build` clean; `cargo test --bins` green; `cargo clippy` no new warnings.
- `rg` confirms no dangling references to deleted symbols, no `TRANS_CLIP`/`TRANS_POS`/`TRANS_HL` diagnostics remain.
- Visual (user): open `i` on a multi-page scene → no clipping, highlight visible on open, `j`/`k`/`q`/`,` page as the cursor crosses boundaries, playback-sync pages.
