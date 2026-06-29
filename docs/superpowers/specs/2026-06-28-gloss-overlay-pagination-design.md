# Paginate the gloss overlay (all block modes) — design

_2026-06-28 (US Central)_

## Problem

The journal overlay now paginates (renders only whole paragraph blocks that fit,
no partial block at either edge — `src/ui/pagination.rs`). The gloss overlay
still scrolls, so block nav can leave a partial paragraph/verse clipped at the
top edge (no top clip box). Make the gloss overlay paginate the same way, across
its block-bearing render modes.

## Modes (from the render-pipeline map)

- **gloss result** (`show_gloss_with_color`) — has blocks (Source verse +
  Explication prose). **Paginate.** Medium risk.
- **synopsis** (`show_synopsis`) — has blocks (all Explication prose, uniform).
  **Paginate.** Low risk — a near 1:1 port of the journal.
- **echoes** (`show_echoes`) — NO blocks (echo-index nav, own
  `scroll_echo_into_view`). **Unchanged** (the journal doesn't paginate echoes
  either).
- **glossing-loading** (`show_glossing`) / loading messages — transient, snapped
  to top, no cursor. **Unchanged.**

## Design

Mirror the journal's model on the gloss overlay for the two block-bearing modes.

### State (new, on GlossOverlay)
- `all_blocks: Rc<RefCell<Vec<GlossBlock>>>` — the FULL block list for the open
  gloss/synopsis (the pagination unit; `GlossBlock` is the existing
  `gloss_block`/`synopsis_blocks` output, carrying kind+index+text).
- `pages: RefCell<Vec<pagination::Page>>`, `page_idx: Cell<usize>`.
- `cursor_full: Cell<usize>` — cursor index across ALL blocks; `cursor_block`
  stays page-local (drives the existing bar + visual mode).

### Render flow (both modes)
1. Build the full block list (as today: `gloss_blocks` / `synopsis_blocks`).
2. Measure each block → `heights`; `pages = paginate(heights, page_height)`
   where `page_height` = `size_scroll`'s `scroll_h` (the pinned viewport).
3. `page_idx = page_containing_block(pages, cursor_full)` (cursor starts 0).
4. `render_page()`: render ONLY the current page's blocks via the EXISTING
   `populate_gloss_buffer`/`populate_verse_buffer` (gloss) or
   `render_synopsis_with_labels` + `set_text` (synopsis) over the page's block
   slice; then `rebuild_block_ranges_from(page_slice)` re-derives `self.blocks`
   (page-local buffer lines); pin vadjustment at 0; `mark_cursor_block()`.

Because the buffer holds only the page and the vadjustment is pinned at 0:
- the accent bar (`buffer_to_window_coords` at scroll 0) is correct with the
  page-local line spans — **automatic**, no change to the draw func;
- the line-number gutter (built by `populate_verse_buffer` per render, 0-relative
  to that render) is correct — **automatic**.

### Block height measurement (THE risk — Source blocks)
`pagination::measure_text_height` (plain `pango::Layout`) underestimates a Source
(verse) block: the speaker heading carries `pixels_above_lines(36)` + `scale(0.75)`
and verse lines have their own gaps — none modeled by a plain layout.

**Rule: over-estimate, never under.** Better to give a Source block its own page
than clip its speaker label at the top. Concretely:
- **Explication / synopsis prose blocks**: `measure_text_height(text)` + the
  journal's `pad_per_para` (`size*0.4*2` per paragraph).
- **Source verse blocks**: `measure_text_height(verse_text)` + a per-block
  overhead = `SPEAKER_BLOCK_OVERHEAD` (the 36px gap + the ~0.75-scaled speaker
  line height) + a conservative `*1.15` factor on the verse height (verse lines
  carry per-line gaps). Tune so a typical multi-line speech never clips its
  speaker. Put the overhead in named consts with a comment tying them to the
  `gloss-speaker`/`gloss-verse` tag geometry in `gloss_render.rs`.

A too-conservative estimate only means a block paginates onto its own page early
— acceptable; a too-small estimate clips — unacceptable.

### Navigation
- `cursor_next_block`/`prev` step `cursor_full` (clamped); if it leaves the page,
  `page_idx = page_containing_block(...)` + `render_page()`; else re-mark the bar.
  `gg`/`G` jump to first/last block. (Exactly the journal's `step_full_cursor` /
  `full_cursor_to_end` / `sync_cursor_page`.)
- `read_current_block` (Space/Tab TTS) and `color_audio_blocks` operate on the
  CURRENT PAGE's blocks (per-page application, like the journal). Cached-block
  color re-applies on each page turn.

### What is removed / kept
- **Removed for gloss+synopsis:** `scroll_cursor_into_view`, `scroll_gloss`,
  `scroll_gloss_to_top/bottom` calls from these modes (the page turns instead).
- **Kept:** `scroll_echo_into_view` + `scroll_gloss*` for ECHOES mode (still
  scroll-based). `snap_value_to_line`/`display_rows` stay (echoes uses them).
- The bar draw func, line-number draw func, `populate_*_buffer`,
  `rebuild_block_ranges_from`, `size_scroll`, AskCardHost — all unchanged.

### Ask-card interaction
Do NOT re-paginate when the ask card opens (matches the journal). The viewport
shrinks; the retained `BottomClipGuard` masks any block that no longer fits. Same
accepted tradeoff as the journal.

### Visual mode
Stays within the current page's rendered blocks (as the journal does).
Cross-page visual selection is OUT OF SCOPE (would need to read from `all_blocks`,
not the buffer) — note it as a follow-on.

## Staging (implement + verify in this order)
1. **Shared:** extend `pagination.rs` only if needed (paginate/page_containing
   already exist). Add a `gloss_block_height` helper (verse vs prose overhead).
2. **Synopsis mode** (low risk) — paginate `show_synopsis` + nav. Build, test,
   user visual check.
3. **Gloss-result mode** (medium) — paginate `show_gloss_with_color` + nav +
   per-page cached-coloring/TTS. Build, test, user visual check (verse: speaker
   labels never clipped; line numbers correct; bar tracks cursor).
4. Echoes + glossing-loading: confirm untouched.

## Testing
- `paginate`/`page_containing_block` already unit-tested.
- New pure test for `gloss_block_height` ordering (Source ≥ prose for equal text)
  if the helper has real logic.
- Build + `cargo test --bins`.
- Visual (user, no-cargo-run): synopsis + gloss page with j/k/q/, — no partial
  block at either edge; verse speaker labels whole; line-number gutter aligned;
  accent bar on the cursor block; gg/G to ends; Space reads the cursor block.

## Files
- `src/ui/pagination.rs` — maybe a `gloss_block_height` (or keep measurement in
  gloss_overlay).
- `src/ui/gloss_overlay.rs` — pagination state + render_page for synopsis & gloss
  modes; remove their scroll calls.
- `src/input/keymap.rs` — gloss nav already calls cursor_next_block etc.; add a
  per-page cached-recolor after page-turning nav (like the journal).

## Risk register
- **HIGH: Source-block height under-measure → clipped speaker label.** Mitigate
  with conservative over-estimate; verify on a real multi-speaker gloss.
- MED: cursor_full/page mapping across modes — proven by the journal.
- LOW: bar + line-numbers (automatic at scroll 0); echoes/glossing untouched.
