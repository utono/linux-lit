# Sonnet single-column, one-per-page layout

## Problem

`The Sonnets` (`work_type = "sonnet_sequence"`, `abbrev = "Son"`) rendered badly
in the default two-column "book foliation" layout: each sonnet is its own
`(div1, div2)` section (`div1` = sonnet number, `div2` empty → `0`), so
`column_split`'s plays-oriented "stop at scene break" rule pushed every sonnet to
the right column and left the left column empty — one sonnet per spread with a
blank left half.

Two follow-on requirements:

1. Render sonnets in a single centered column.
2. Show exactly **one sonnet per page** — a page-turn advances one sonnet; the
   next sonnet starts fresh on the next page.
3. With translations visible, show **one sonnet + its translation** per page.

## Root causes

- **Column count:** `default_column_count_for_parts` (`src/app.rs`) returned `2`
  for every work. Nothing made `sonnet_sequence` single-column by default.
- **Page packing:** the single-column display clip (`update_bottom_clip` in
  `src/input/scroll.rs`) already clamps the page at the next section start via
  `trim_visible_range` → `clamp_at_section_break`, but a **fill-guard** reverts to
  the unclamped viewport range when the clamped page is <85% full. A 14-line
  sonnet is well under 85%, so the guard packed the next sonnets onto the page.
- **Translations:** `show_translations` inflates the buffer with inserted
  translation lines, but `section_starts` stays indexed to the original buffer.
  The one-section clamp would land on the wrong physical line.
- **Boundary attribution (the real off-by-a-line bug):** the authoritative
  boundary is correct — `lit.db` (imported from `folger-xml/Son.xml`) has 154
  distinct `(div1, div2)` pairs, one per sonnet, so `build_section_starts`
  detects each sonnet via the `(div1,div2)` change. But it pinned the boundary
  bit to the wrong buffer line: the bare-number heading ("1", "2") is unmapped
  chrome, and `is_act_scene_marker`/`is_separator` don't match a bare number, so
  the bit landed on the blank above the body (sonnet 1) or the body's first line
  (later sonnets). The page then broke right after the heading — heading alone on
  one page, body + next heading on the next. Per the authoritative-boundary
  principle this is *not* re-inferring a boundary from text: the boundary still
  comes from `(div1,div2)`; only the chrome-line *attribution* (which sanctioned
  build-time text classification already does) needed to recognize a bare stanza
  number.

## Design

Data source is unchanged — sonnets already come from `line_mapping` like every
work. The levers are column count and the section-break clamp, not the text
pipeline.

### 1. Default `sonnet_sequence` to one column

`default_column_count_for_parts` returns `1` for `sonnet_sequence`, `2`
otherwise. The existing per-work override (`config.column_overrides`, `Alt+[`)
still takes precedence.

### 1b. Pin the boundary to the sonnet-number heading

`build_section_starts` (`src/text_file_map.rs`) — both `first_chrome_at_or_before`
(opening section) and `first_chrome_after` (each transition) — now also treats
`line_types::is_stanza_number` (a bare all-digits line) as a chrome/marker line,
so the boundary bit pins to the "1"/"2" heading instead of the blank or body.
The walk-back in `first_chrome_at_or_before` was also extended to step across a
stanza-number line so the opening boundary reaches line 0. `SNAPSHOT_VERSION`
bumped 4→5 so stale cached bitmaps for `Son` are rebuilt.

### 1c. Don't land page_top on the blank below a heading

After 1b fixed attribution, a blank page still appeared *between* sonnets: the
forward turn from sonnet 1 set `page_top` to the trailing blank above sonnet 2's
heading. `next_page_top` calls `back_up_for_speaker(next)`; the sonnet number
heading is classified as dialogue (a bare number is not blank/speaker/marker/
separator/stage-direction), so `next` lands on the heading line — which is itself
the section start. `back_up_for_speaker` only checked `is_section(top-1)`, so it
stepped down over the blank below the heading instead of stopping on it. Fix:
when the target `line` itself is a section start (authoritative bitmap), return
it immediately. No effect on plays — there the page-turn target is the next
*dialogue* line, never a section-start chrome line, so the guard never fires
(verified by the all-shakespeare nav suite).

### 1d. Cursor on first verse line, not the number heading

The sonnet number is a section heading, not spoken verse. `is_dialogue` (load
time, `db/line_types.rs`) and `is_dialogue_line` (runtime, `viewport.rs`) now
both exclude `is_stanza_number` in verse mode (prose returns early, so a numeric
line stays content there). No `lit.db` line is affected — the numbers are
unmapped `.txt` chrome (a DB scan for bare-number `canonical_text` returns
nothing), so only the buffer-text runtime check matters. `next_dialogue` after a
sonnet now lands on the next sonnet's first verse line; `back_up_for_speaker`
still pulls page_top up to the heading (1c).

### 1e. No line numbers for one-section-per-page works

`rebuild_line_number_gutter` (`app.rs`) gates `show_numbers` on
`!one_section_per_page()` — each page is one short numbered poem, so the
every-5th foliation gutter is noise.

### 1f. G / jump-to-end lands on the last section, one sonnet

`last_page_top`'s single-column branch packed `widget_height` of content
backward, pulling several trailing sonnets onto the final page. New
`one_section_per_page` branch sets the final page top to the last
`is_section_start` boundary at/before the end. `jump_to_end` then places the
cursor on that sonnet's first verse line (not the two-column `column_split`
path).

### 1g. Startup resume snaps to the sonnet boundary

`snap_near_end_to_canonical` (`app.rs`) early-returned for single column, so a
resumed `page_top` that sat mid-sequence (saved by an older build, or just
mid-sonnet) was used verbatim and the page packed surrounding sonnets (with line
numbers, because the gutter was rebuilt before the snap). It now has a
`one_section_per_page` branch that snaps `page_top` to the section boundary
containing the cursor — read DIRECTLY from the `is_section_start` bitmap (scan
backward from the cursor), not via `canonical_page_top_for`'s forward walk, which
can return a non-boundary top mid-sonnet (the exact resumed-mid-sonnet case) —
and the cursor to that sonnet's first verse line, then returns. Prose single-column behavior is unchanged (still
returns at the `column_count() != 2` guard).

### 1h. y (page-back) highlights the sonnet's first verse line

`page_backward` lands the cursor on the page's LAST dialogue line (reader
expectation for plays/prose). For one-section-per-page works it now lands on the
sonnet's FIRST verse line instead — same landing as gg/x — so y is symmetric with
forward paging for sonnets.

### 1i. Center the sonnet block and its number heading

`apply_tiled_mode` (`app.rs`) — for `one_section_per_page` single column — sets a
centered left margin so the verse block sits in the middle of the card (verse
lines stay left-aligned to that common edge) and a symmetric right margin so the
text region is centered. The block width is measured via Pango against the LONGEST
line across all 154 sonnets (`SONNET_BLOCK_SAMPLE`, the 60-char sonnet-14 line),
plus 16px slack, so the centered left edge is STABLE across sonnets AND no line
ever wraps (an under-sized sample wrapped sonnet 17). In Charter 19 the block is
~646px in the 1050px card, leaving ~200px margins each side. `apply_stanza_number_centering` then center-justifies the
bare number heading lines (`is_stanza_number`) over that centered region via a
`stanza-number-center` tag, run from `apply_dialogue_formatting` before its
speaker scan (sonnets have no speakers, so the rest early-returns).

### 1j. Last sonnet scroll-clamp dragged page_top into the previous sonnet

The resume correctly snapped `page_top` to sonnet 154's heading (the last
section), but 154 sits ~16 lines from the buffer end — too close to scroll to the
viewport top. `snap_scroll_to_line`'s clamp-correction then walked `page_top`
BACKWARD to the nearest line that fits the clamped scroll, landing mid-sonnet-152
(the "For all my vows…" tail the screenshots showed). Fix: for
`one_section_per_page`, on a clamped scroll, EXTEND the text_view bottom margin so
the section heading can reach the top (needed_upper = line_y + page_size), scroll
to it, and add an idle re-scroll backstop for stale `upper` — never walk
`page_top` back into the previous section. (`ensure_scroll_range` sets
margin=page_size up front but GTK's `upper` is stale at snap time, leaving the
scroll ~600px short.)

### 2. One section per page

- `AppState::one_section_per_page()` → `true` for `sonnet_sequence`.
- Thread the flag into `update_bottom_clip` (and `schedule_bottom_clip_update`,
  `refresh_bottom_clip`, `update_bottom_clip_public`, the highlight idle clip).
- When set on a single-column page, **skip the fill-guard revert** and keep the
  section-clamped range — the page ends at this sonnet's last line; the next
  sonnet is hidden until the page turns.
- The page-turn boundary (`last_fully_visible_line` → `next_page_top`) already
  honors the section clamp (its internal revert falls back to a state that still
  has the clamp), so forward/backward turns already advance one sonnet.

### 3. One sonnet + translation per page

- New field `AppState.translation_section_starts: Vec<bool>`, built in
  `show_translations` by remapping the line_map's original-indexed
  `section_starts` through the same `map_line_after_insert` used for
  `current_line` / `page_top_line`. Inserted translation lines are `false`.
- `section_starts()` and `is_section_start()` return this remapped bitmap while
  `translations_visible` (else the line_map bitmap). Every existing clip /
  pagination caller goes through `section_starts()`, so they become correct with
  no further changes.
- Cleared on hide (`hide_translations`) and on work switch (`clear_display`).

## Scope / non-goals

- No DB or `.txt` change; no `LineMap` serialized-shape change (no
  `SNAPSHOT_VERSION` bump).
- Does not touch the deferred translation lockstep-scroll work. The remapped
  bitmap only changes which physical line the section clamp targets — a latent
  correctness fix for any single-column-with-translations work, not just sonnets.

## Verification

- `cargo build`, `cargo test --bins` (224 pass, incl.
  `sonnet_sequence_defaults_to_one`).
- Visual ("renders on screen") criterion → headless launch of `Son`: confirm
  single centered column, one sonnet per page forward and backward, and one
  sonnet + translation per page with translations toggled on.
