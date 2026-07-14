# Translation overlay — match per-line spacing to the main card

## Problem

The translation overlay puts visibly more vertical space between verse lines
than the main reading card does, even though both are configured with the same
`line_spacing` (dev config: `6`). The overlay reads looser and less like the
reader.

## Root cause

Both surfaces call `pixels_above_lines(line_spacing)` **and**
`pixels_below_lines(line_spacing)`. The overlay renders each source line as its
own paragraph in short per-column `TextView`s, so between two adjacent lines the
gap is `line_spacing` (below line N) + `line_spacing` (above line N+1) =
`2 × line_spacing` = 12px. The perceived rendered gap in the overlay is double
what the reader shows for the same value.

The pagination height estimator already encodes this doubling on purpose: the
`spacing()` helper in `block_height` adds `2 * ctx.line_spacing * paras`, and
`line_height` adds `2 * ctx.line_spacing` per line, to keep page packing honest
against the doubled rendered spacing.

## Fix (overlay only — main card untouched)

Move the whole per-line gap to one side so the inter-line gap is a single
`line_spacing` (6px), matching the reader:

- In `make_column` and the full-width interlude view
  (`src/ui/translation_overlay.rs`):
  - `set_pixels_above_lines(0)`
  - `set_pixels_below_lines(line_spacing)`

The first line then sits flush to the top like the main card, and each
subsequent line is exactly `line_spacing` below its predecessor.

### Cascade — pagination math must follow

Because the rendered per-line spacing drops from `2×` to `1×`, the height
estimator must match or pages will under-pack:

1. `block_height` — the `spacing()` closure returns
   `2 * ctx.line_spacing * paras`; change to `ctx.line_spacing * paras`.
2. `line_height` — returns `oh.max(th) + 2 * ctx.line_spacing`; change to
   `oh.max(th) + ctx.line_spacing`.
3. `split_oversize_blocks` and `paginate` consume the corrected heights
   unchanged — no edit.

Update the doc comments that reference "above AND below every paragraph" /
`2 * line_spacing` so they describe the new single-sided spacing.

### Guardrail preserved

The per-line spacing was added originally so the cursor-line highlight band
(`paragraph_background`) clears the highlighted line's descenders. Keeping the
full `line_spacing` **below** each line preserves that room, so the band still
contains descenders. Verify headlessly against the line-clipping invariant.

## Out of scope

- No CSS change (the `translation-col` styling stays as-is).
- No change to `src/app/translations.rs` — it keeps passing the same
  `line_spacing`.
- The main reading card (`src/app/mod.rs`, `formatting.rs`) is not touched.

## Verification

- `cargo build` clean.
- Headless cage capture of the translation overlay (`i` key) — inter-line gap
  visibly matches the main card; cursor-line band clears descenders.
- Existing `translation_overlay` unit tests still pass (they don't assert on
  spacing; grouping/splitting logic is unchanged).
