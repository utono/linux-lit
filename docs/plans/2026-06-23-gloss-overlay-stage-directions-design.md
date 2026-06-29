# Gloss overlay: show stage directions in the source verse

**Date:** 2026-06-23
**Status:** Approved, ready for implementation plan

## Problem

When a glossed passage includes stage directions, the gloss overlay's source
verse omits them. In the main reading card (image #6) the passage interleaves
verse with italic stage directions:

```
YORK
    Lay hands upon these traitors and their trash.
    [The Guard arrest Margery Jourdain and her accomplices and seize
    their papers.]
    [To Jourdain.]
    Beldam, I think we watched you at an
    inch.
    [To the Duchess, aloft.]
    What, madam, are you
    ...
```

In the gloss overlay (image #7) the source block is stripped down to bare verse:

```
YORK
    Lay hands upon these traitors and their trash.
    Beldam, I think we watched you at an
    inch.
    What, madam, are you
    ...
```

The user wants the gloss overlay's source verse to closely resemble the main
card: stage directions present, in their true interleaved positions, rendered
italic.

## Root cause

The source block is built two different ways, and neither shows stage
directions correctly:

- **Result card** (`GlossOverlay::show_gloss_with_color`, `gloss_overlay.rs`):
  the source verse is re-derived from the **stored gloss text's `<verse>`
  tags**. The gloss prompt instructs the model to quote *verbatim verse lines
  only* — stage directions are absent from the stored data, so they cannot
  appear no matter how the renderer styles them.

- **Loading card** (`GlossOverlay::show_glossing`, `gloss_overlay.rs`): builds
  the passage from the real selected lines via
  `echoes::build_source_header(&selected_lines, …)`. Stage-direction lines ARE
  present here, but each line is wrapped in `<verse>`, so a whole-line stage
  direction renders upright like verse (only its brackets are italicized by
  `apply_bracket_styling`), and the `<speaker>` carry-forward logic can attach a
  speaker label to it.

## Key insight

The actual selected lines — including stage directions, in true document order —
are available at **both** display points:

- The loading card already passes them through `build_source_header`.
- The result card has them in `ctx.source_text` (all selected lines joined by
  `\n`, verbatim from `line.text`) and `ctx.source_line_numbers`
  (`GlossContext`, `gloss.rs:496`), 1:1 aligned via `ctx.source_line_pairs()`.

So both cards can build their source block from the real lines. No stored gloss
data is touched and nothing is re-glossed; only the **display** of the source
portion changes.

Note: `apply_bracket_styling` (`gloss_overlay.rs:2053`) already italicizes
bracketed spans inside verse. So inline brackets are already styled; the new
work is (a) getting whole stage-direction *lines* into the data the result card
renders, and (b) marking them so block/speaker logic treats them correctly.

## Design

### 1. New `<stage>` element in the gloss markup vocabulary

In `src/ui/gloss_block.rs`:

- Add `GlossElement::Stage(String)` to the `GlossElement` enum.
- `parse_gloss_tags` extracts `<stage>…</stage>` to `GlossElement::Stage`
  (same `try_extract` pattern as `verse`/`pron`).
- `carry_forward_block_speakers`: a `Stage` element does NOT reset or trigger
  the synthetic-speaker carry-forward — it is transparent to speaker tracking.
- `gloss_blocks`: a `Stage` line is part of the current Source block's pending
  run (it does not start a new block and is not its own cursor stop). It should
  be appended to the source-block display text so the block's line span covers
  it, but contributes no independent cursor stop. Simplest: treat `Stage` like
  `Verse` for the purpose of `pending_verses` accumulation (so the Source block
  text and line span include the stage line), but it carries no speaker.

### 2. Render `<stage>` italic in `populate_gloss_buffer_ex`

In `src/ui/gloss_overlay.rs`:

- Add a `gloss-stage` `TextTag`: `style(pango::Style::Italic)`, indented to
  `quote_verse` (same left margin as verse), so the stage line sits in the verse
  column but reads italic — matching the main card. Add it to the tag cleanup
  list and the `tag_table.add(...)` block.
- In the element loop, add a `GlossElement::Stage(text)` arm: set
  `only_speakers_so_far = false`, insert the text, apply `gloss-stage`. Do NOT
  push a line-number gutter entry for a stage line. (Bracket styling is implicit
  in the full-line italic; no separate `apply_bracket_styling` call needed.)

### 3. Both cards build the source block from the real lines

Extract source-block construction into one place and route both cards through
it, emitting `<stage>` for stage-direction lines.

- **New module** `src/ui/source_block.rs` (or `src/input/actions/source_doc.rs`):
  move `build_source_header` here with the single responsibility of turning
  `&[Line]` into `<speaker>` / `<verse>` / `<stage>` markup. For each line:
  - if `crate::db::line_types::is_stage_direction(&line.text)` → emit
    `<stage>{text}</stage>`, and do NOT emit a `<speaker>` for it (do not change
    the `current` speaker);
  - else emit `<speaker>` (when the speaker changes) + `<verse>{text}</verse>`
    as today.

  Update the existing call sites in `echoes.rs` (the echo source header, 4 call
  sites) to use the moved function. This immediately fixes the **loading card**
  and the **echoes** source header.

- **Result card**: rebuild its source block from the real lines instead of the
  stored gloss's `<verse>` tags. The gloss's `<gloss>` explication blocks are
  kept verbatim; only the source portion is replaced by `build_source_header`
  output built from the selected lines. Concretely, the `show_gloss_with_color`
  callers (`visual.rs`, `gloss.rs`, `synopsis.rs`) already have `ctx`; build the
  source header from the lines `ctx` was built from and splice it in front of
  the explication, then pass the spliced document to `show_gloss_with_color`.

  The mechanism for splicing source-header-over-stored-`<verse>` must preserve
  the existing explication blocks, cursor stops, audio-cached coloring, and
  per-verse line numbers. The implementation plan will determine the least
  invasive splice point (most likely a helper that replaces the leading
  `<speaker>/<verse>` run of the stored gloss with the freshly built one, since
  the model already emits the source turn first).

### Out of scope

- No change to stored gloss data, the gloss prompt, or re-glossing.
- No change to TTS: stage directions are display-only, never spoken (the
  `Stage` element is not a cursor stop and not collected for TTS, like `Pron`).
- No unrelated splitting of the 2075-line `gloss_overlay.rs`; the refactor is
  scoped to source-block construction only.

## Testing

Pure unit tests (no GTK), runnable under `cargo test --bins`:

- `build_source_header` (moved): a turn with interleaved stage directions emits
  `<stage>` in the correct positions; no `<speaker>` precedes a stage line;
  verse lines still wrapped in `<verse>`; speaker carries correctly across a
  stage line (a stage direction between two verses by the same speaker does not
  re-emit the speaker).
- `parse_gloss_tags` round-trips `<stage>` to `GlossElement::Stage`.
- `gloss_blocks`: a `Stage` line stays inside the current Source block and does
  not create an extra cursor stop; the source block's line span includes it.
- `carry_forward_block_speakers`: a `Stage` line does not trigger a synthetic
  speaker.

Visual verification (requires the user to launch; visual criterion per
CLAUDE.md): build with `cargo build`, run `cargo test --bins`, then ask the user
to open 2H6 1.4.43–50 and gloss it (or open the stored gloss), confirming the
source block in the overlay interleaves italic stage directions exactly as the
main card does (image #6).

## Acceptance criteria

- Glossing a passage with stage directions shows them, interleaved in true
  position, italic, in **both** the loading card and the result card.
- A stage-direction line is not a cursor stop (j/k skip over it within the
  source block) and never carries a speaker label.
- Verse line numbers, audio-cached coloring, and explication cursor stops are
  unchanged.
- `cargo build`, `cargo test --bins`, and `cargo clippy` pass.
