# Synopsis paragraph spacing — design

Date: 2026-08-06
Surface: gloss overlay, `PaginatedMode::Synopsis` only.

## Problem

The first page of a synopsis reads as scattered rather than grouped. Each
one-line metadata entry ("Length: ~5,832 words", "Gist:") sits marooned in its
own field of whitespace.

Measured from a production screenshot (BH-Barrett, Chapter 11, 1920px):

- Gap between wrapped lines *inside* a paragraph: **9px**
- Gap between paragraphs (sections): **47–53px**

A ratio of ~5.5x. Typographic convention puts a paragraph break at roughly 2x
the line gap; past that the break stops grouping text and starts scattering it.

## Root cause

`render_synopsis_with_labels` joins paragraphs with a literal `"\n\n"`
(`gloss_block.rs:71`), and the paginated branch of `render_synopsis_page` does
the same (`gloss_overlay.rs:2835,2844`). The second newline renders as a full
EMPTY TEXT ROW — a whole line height (~30px at this font) that no spacing tag
governs.

The blank row is the entire excess. `set_pixels_below_lines(6)` and
`OVERLAY_LINE_LEADING` (2px) are minor contributors: 30 + 9 + 9 ≈ 48, matching
the measurement.

## Approach

Replace the blank row with tag-driven paragraph spacing. This is the pattern
the gloss body already uses — `gloss_render.rs:572` spaces prose explications
with `pixels_above_lines`/`pixels_below_lines` (20/20), never with blank lines.

1. **Join with `"\n"`.** One buffer line per paragraph. Applies to both join
   sites (`gloss_block.rs` and the paginated branch in `gloss_overlay.rs`).
2. **Add a `synopsis-para` tag** carrying `pixels_above_lines(SYNOPSIS_PARA_GAP
   = 18)`, applied to non-label paragraphs. 18px is 2x the measured 9px
   intra-paragraph gap.
3. **Bind labels to their bodies.** The existing `synopsis-label` tag gains
   `pixels_below_lines(SYNOPSIS_LABEL_GAP = 6)` and keeps the full 18px above,
   so "Gist:" attaches to the prose it heads instead of floating equidistant
   between two sections.

## Pagination coupling (the load-bearing part)

`repaginate` charges `text_h + line_h` per Explication block
(`block_height_overhead`, `gloss_overlay.rs:3758`). The comment at 2709–2723
states that this per-block `line_h` headroom exists to cover "per-buffer-line
`pixels_below_lines` + the blank-line paragraph separator, neither modeled by a
plain `pango::Layout`", and that it was tuned against the 2026-07 "Gist:"
clipping bug on THIS card (8 blocks: 7 one-line metadata paragraphs + Gist),
which still clipped after an earlier page-level fix (commit 4df9352).

Removing the blank line therefore invalidates the charge. Leaving the charge as
`line_h` would be safe against clipping but would over-estimate every block by
nearly a full line, under-filling pages and partly defeating the fix.

Decision: replace the separator component with the real new constant.

- Synopsis Explication blocks charge `text_h + SYNOPSIS_PARA_GAP`.
- The lead-label charge (`gloss_overlay.rs:3790`, currently `+ line`) charges
  the real label gap instead.
- The GLOSS path keeps `line_h` untouched. Only `PaginatedMode::Synopsis`
  behavior changes.

## Offset tracking

Label ranges are located by char offset while assembling the joined string.
Every join site advancing `char_off += 2` for `"\n\n"` becomes `+= 1`. All join
sites must change together or `apply_synopsis_label_bold` bolds the wrong
characters.

Sites: `gloss_block.rs:71-72`, `gloss_overlay.rs:2835-2836`, `2844-2845`.

## Non-goals

- No change to the gloss-result path, the journal overlay, or the reading card.
- No change to `OVERLAY_LINE_LEADING` (2px), which is shared across overlays.
- No change to the title/rule spacing above the first paragraph.

## Verification

`cargo build`, `cargo clippy`, `cargo test`, then user review on the real GL
renderer.

Risk area is pagination height, not the visual gap. The failure mode to watch
for is a clipped last line at a page bottom — exactly the shape of the 2026-07
"Gist:" bug. Page count for a given synopsis is expected to DROP or hold, never
rise; a rise means the charge is now over-conservative.
