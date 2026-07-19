# Chat panel pagination

**Date:** 2026-07-19
**Status:** Design — pending user review
**Scope:** The left chat panel (`src/ui/chat_panel.rs` + `src/input/actions/chat.rs`), all three views (Gloss, Journal, Question).

## Problem

The chat panel free-scrolls a `ScrolledWindow` over a `gtk4::Box` of row widgets. This has produced a string of clipping bugs that cannot all be fixed within the free-scroll model:

- **Top edge:** a partial row shows clipped under the panel's top gap when scrolled.
- **Bottom edge:** a partial row shows clipped at the bottom when content overflows. `BottomClipGuard::attach_box` masks only *trailing slack* (short content); it clips 0 on overflow, so the partial bottom row stays exposed (pixel-verified).
- **Journal view:** a saved Q&A answer is ONE `Answer` row (never split into paragraphs), so `journal_cursor` has only the single `Q:` line to land on — `j`/`k` cannot move the accent bar into or through the answer.

`docs/troubleshooting/clip-prevention.md` already establishes the durable answer for a scrolling Box of wrapping content: **stop scrolling and paginate** — render only the whole rows that fit per page, so no partial row can exist at either edge. The translation overlay took exactly this path.

## Goal

Paginate the chat panel like the reading card / translation overlay:

- Render only the whole row-units that fit in the transcript viewport — **no partial row at either edge, ever.**
- `j`/`k` (and `h`/`t`) move a row cursor (the accent bar); when the cursor would leave the current page, **turn the whole page** and land the cursor on the **first row of the next page** (and symmetrically for `k`/`t` at the top → previous page, cursor on last row). `gg`/`G` jump to the first/last page + first/last landable row.
- All three views (Gloss, Journal, Question) paginate with a uniform row cursor.
- **Journal answers split into paragraph rows** so the cursor traverses them (the accent-bar-stuck fix).

## Reuse (verified infrastructure)

- **`src/ui/pagination.rs`** (pure, GTK-free, already used by the translation + journal overlays):
  - `Page { start, end }`, `paginate(block_heights, page_height) -> Vec<Page>`.
  - **`paginate_grouped(block_heights, group_start: &[bool], page_height)`** — packs indivisible multi-block units, never splitting one across a page. This is the mechanism for `GlossAnswer`'s exploded widgets and for a split journal answer's paragraph run.
  - `measure_text_height` / `measure_text_height_leaded` — standalone `pango::Layout` measurement, no widget allocation.
- The pattern is proven at this exact panel scale in `journal_overlay.rs` (its repaginate + measure).

## Verified architecture facts (the design rests on these)

- `TranscriptRow` variants: `Question`, `Answer`, `GlossAnswer`, `Chip`, `Error`, `Thinking`, `SavedMark` (`chat_panel.rs:18`).
- `rebuild_rows` emits ONE `Label` per row EXCEPT `GlossAnswer`, which `append_gloss_answer` explodes into N labels (speaker/verse/stage/gloss) via `gloss_render::chat_gloss_rows` (`chat_panel.rs:397`, `477`).
- **`row_cursor` is a WIDGET index**, not a row index (`chat_panel.rs:226`, `chat.rs:109`). `row_owner: Vec<usize>` maps each widget → its exchange (`chat.rs:978`). `row_widget_texts` / `row_widget_landable` stay widget-count-aligned.
- Views: `PanelView::{Gloss, Journal, Question}` (`chat.rs:72`). `t` flips Gloss↔Journal (`chat.rs:1461`, `1224`); Question is entered only by a live follow-up submit.
- Cursor nav: `transcript_cursor_move` branches by view (`chat.rs:1803`): Gloss steps `row_cursor` over `landable_mask` (skips speaker widgets, `chat.rs:1871`); Journal steps `journal_cursor` (entry granularity, `chat.rs:1291`); Question plain-scrolls.
- Journal rows: `journal_view_rows` = 2 rows/entry (`Question`, `Answer`), never subdivided (`chat.rs:1240`); `journal_entry_qrow(e) = e*2` only ever targets `Q:` (`chat.rs:1270`).
- Viewport height: `size_panel` sets the whole container `panel_h` (`chat.rs:2446`); the transcript-only budget = `panel_h − input-card height − chrome`, derived the way `journal_overlay.rs:1650` subtracts chrome margins.

## Architecture

Four pieces. The pagination engine is pure and unit-testable; the GTK render slices one page.

### 1. Per-widget height model (new, pure-ish helper)

`chat_row_heights(rows) -> (Vec<i32>, Vec<bool>)` — returns each rendered WIDGET's height and a `group_start` flag (true at the widget that begins a new indivisible unit). Heights come from `pagination::measure_text_height` at the transcript's wrap width, plus a per-CSS-class padding correction (each `chat-a-*` class's `padding-top`/`padding-bottom`, `chat-a-src-lead`'s 30px, etc. — the labels carry CSS padding `measure_text_height` doesn't see). The class→padding map is a small const table mirroring `theme.rs`. Unit-tested against known class/padding combos.

Rationale for static measurement over live `compute_bounds`: `gg`/`G` and page math must know boundaries WITHOUT rendering the whole transcript first (the translation/journal overlays do this; live-measure-then-discard is the rejected path).

### 2. Pagination pass (reuse `paginate_grouped`)

Given `(heights, group_start, transcript_budget)`, call `pagination::paginate_grouped` → `Vec<Page>` over WIDGET indices. A `GlossAnswer`'s N widgets share one group (all but the first have `group_start=false`) so a block never splits across a page. Store the pages + current page index on `ChatState`.

### 3. Page-slice render (replace the scroll-snap)

`render_rows_focused_cursor` stops snap-scrolling. Instead it renders ONLY `rows[page.start..page.end]` into `transcript_box` (a page slice, like the overlays' `render_page`), applies the accent bar at the page-local cursor widget, and does NOT scroll (the page fits by construction). The `BottomClipGuard`/`attach_box`, the row-snap math, and `render_rows_to_top`'s settle logic are removed — pagination makes them unnecessary. Bottom and top are clean because only whole rows are rendered.

### 4. Cursor + page-turn nav

`transcript_cursor_move(delta)`:
- Move the row cursor within the page over landable widgets (`step_row_cursor_landable`, reused).
- If the cursor would step past the page's last landable widget (`delta>0`): if a next page exists, turn to it and land on its FIRST landable widget; else no-op (clamp at end).
- Symmetric for `delta<0` at the page top → previous page, LAST landable widget.
- `transcript_cursor_first`/`last` → first page/first landable, last page/last landable.

`size_panel` change / resize: on a viewport-height change, re-paginate and clamp the page index + cursor (mirrors the overlays' repaginate-on-resize).

### 5. Journal answer split (the accent-bar-stuck fix)

`journal_view_rows` splits each saved entry's `Answer(String)` into paragraph-level `TranscriptRow`s (blank-line-separated paragraphs → one row each), mirroring how `GlossAnswer` subdivides. Build a `group_start`/owner map so an entry's `[Q, A-para1, A-para2, …]` widgets pack as one group and the cursor steps through them. Replace `journal_entry_qrow`'s single-`Q:` target with real widget stepping (reuse `step_row_cursor_landable` WITHOUT the speaker-skip; the journal has no speaker widgets). Journal and Gloss then paginate uniformly.

### Data flow

```
open / render / resize:
  rows          = build rows (Gloss: transcript_rows; Journal: journal_view_rows split;
                  Question: single exchange)
  (heights, gs) = chat_row_heights(rows)                 // pure + measured
  pages         = paginate_grouped(heights, gs, transcript_budget)   // pure
  page_idx      = clamp(page_idx)
  render page slice rows[pages[page_idx]] + accent bar at page-local cursor  // no scroll

j/k:
  step row cursor over landable widgets in the page
  edge → turn page (whole page), cursor to first/last landable of the new page
```

### What is removed

- `render_rows_focused_cursor`'s scroll-into-view + row-top snap (`chat_panel.rs:290`).
- The `BottomClipGuard`/`attach_box` wiring + `on_open` in `ChatPanel` (pagination replaces it).
- `render_rows_to_top`'s settle-on-`changed` scroll reassert.
- The `.chat-panel .gloss-bottom-clip` CSS override.
- The `transcript_scroll` can keep hscroll `Never` + `propagate_natural_width(false)` (the width fix, still needed); vertical scrolling becomes a no-op since each page fits — the `ScrolledWindow` stays (simpler than swapping the widget) but never scrolls.

### What stays

- The width fix (`propagate_natural_width(false)`), panel height = card height, breathing-room `margin-top`, and the gloss→source `chat-a-src-lead` gap — all orthogonal to pagination.
- `row_owner` / `landable_mask` / `row_widget_texts` (extended to the split journal rows).

## Decisions (locked with user)

- **Page turn:** whole-page — cursor lands on the first row of the next page (last row of the previous). Not row-by-row scroll.
- **Views:** all three (Gloss, Journal, Question) paginate with a uniform row cursor.
- **Journal answers** split into paragraph rows so the cursor traverses them.

## Edge cases

- **A single group taller than the viewport** (a very long gloss/answer block that can't fit one page): `paginate_grouped` must place it on its own page even though it overflows — then that ONE page scrolls (the only scrolling case), OR the block is split at paragraph boundaries. Decision: split answer/gloss blocks at paragraph granularity in step 5/1 so a group is a run of paragraph rows, each viewport-fittable; a single paragraph taller than the viewport is the residual rare case → allow that one page to scroll (documented, matches the reading card's over-tall-paragraph branch).
- **Empty view** (no exchanges / empty journal): one page, placeholder row, no cursor (existing behavior).
- **Resize while open:** re-paginate, clamp page + cursor.
- **View switch (`t`)**: re-paginate for the new view, reset to page 0 + first landable.
- **Live streaming answer** (`render_rows` scroll-to-end today): while a Thinking/streaming answer grows, render the LAST page (so the newest text shows) — the streaming analog of scroll-to-bottom.

## Testing

- **Unit (pure):** `chat_row_heights` class-padding correction; `paginate_grouped` grouping of a GlossAnswer's widgets and a split journal answer (no group split across a page); cursor page-turn arithmetic (edge → next page first landable, symmetric).
- **Headless + pixel (real renderer):** the clip lesson — pixel-measure top AND bottom bands are whole lines (~15-20px, never a ~2px sliver) across `j`/`k` through a long gloss and a long journal answer; the accent bar moves on every `j` and turns the page at the edge; the journal-view accent bar traverses the answer paragraphs. **Cage cannot confirm; verify on the user's real renderer.**
- **Regression:** Question view (single follow-up) still renders; `t` view toggle; save (`s`) still shows the entry.

## Reference

- `docs/troubleshooting/clip-prevention.md` — "Pagination instead of a mask" + the box-of-wrapping-content rule.
- `src/ui/pagination.rs` — `paginate_grouped`, `measure_text_height`.
- `src/ui/translation_overlay.rs` / `journal_overlay.rs` — the paginated-surface precedent.
- `docs/superpowers/specs/2026-06-27-paginated-translation-overlay-design.md` — prior pagination design.
