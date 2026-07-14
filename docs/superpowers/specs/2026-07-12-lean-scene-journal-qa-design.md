# Lean Scene Journal Q&A — Design

**Date:** 2026-07-12
**Status:** Approved (user picked approach B: budget cap + windowed fallback)

## Problem

The scene-band journal ask (`ask_claude` Scene/Passage bands, plus the band
text builders in `journal.rs`) sends the ENTIRE scene for non-prose works —
`scene_text_windowed` only windows prose (±10 paragraphs ≈ 1.3k tokens).
Measured against lit.db: the median Shakespeare scene is ~250 tokens (fine),
but p90 is ~2.3k and the tail reaches ~10k tokens (Hamlet 2.2 ≈ 6.4k). The
vocab journal ask, by contrast, is ~1k tokens total.

## Decision (approach B)

Cap, don't always window. For non-prose works in `scene_text_windowed`
(`src/app/scene_synopsis.rs`):

- Render the full scene as today; if it is at or under
  `SCENE_TEXT_MAX_CHARS = 12_000` (~3k tokens, ~90% of Shakespeare scenes),
  send it unchanged — whole-scene questions keep whole scenes.
- Over budget, fall back to an anchored excerpt: the scene's lines within
  `VERSE_WINDOW_RADIUS = 80` lines each side of the reader's anchor
  (`anchor_work_line`, the same anchor the prose path already uses;
  fallback = scene opening when the anchor is outside the division), with
  explicit markers on whichever ends were cut —
  `[… scene continues above — this is an excerpt around the reader's
  position …]` / `[… scene continues below …]` — so the model never
  mistakes the excerpt for the whole scene.

Rejected: always-window (degrades "summarize this scene" on short scenes);
prompt-caching the scene text (helps only rapid re-asks, first ask unchanged).

## Shape

- Extract the speaker-interleave render loop shared by `scene_text_for` and
  `prose_window_text` into one private helper; both keep byte-identical
  output.
- New `play_scene_text_lean(work, div1, div2, anchor_work_line) -> String`
  (pure over `&Work`, unit-testable) implements full-or-excerpt;
  `scene_text_windowed`'s non-prose branch calls it. Prose path untouched.

## Unchanged behavior

Prose asks, Work/Author bands, all scenes ≤ 12k chars. Only the long-scene
tail changes: ~6–10k-token sends become ~2.5k with visible excerpt markers.

## Testing

Unit tests on `play_scene_text_lean`: under-budget passthrough (identical to
full render, no markers); over-budget window centered on the anchor with both
markers; anchor at scene start/end (single marker); anchor outside the
division (opening window); speaker interleave preserved. Existing journal +
synopsis tests stay green.

## Cost note

At claude-opus-4-8 the change bounds the worst-case scene ask input; a
per-ask cost estimate is reported alongside the implementation.
