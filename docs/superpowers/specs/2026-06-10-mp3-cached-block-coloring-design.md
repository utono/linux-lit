# Color mp3-cached blocks with the theme accent color

**Date:** 2026-06-10
**Status:** Approved (design), pending implementation plan

## Problem

In the gloss and synopsis overlays, the reader cannot tell at a glance which
blocks already have synthesized TTS audio. The user wants a visual cue: any
block whose TTS mp3 is cached should render in the theme's accent color (the
`root_color`, the same color already used for speaker headings and the accent
bar). This makes "already synthesized" obvious and turns the overlay into a
progress view as blocks are batch-synthesized.

## Requirements

- **Scope: all block kinds.** Source verse blocks, explication paragraphs, and
  synopsis paragraphs are each colored when their own mp3 is cached. (Initial
  phrasing said "explication but not source"; the final decision is all blocks.)
- **Color: match the speaker accent.** Use the exact color the speaker headings
  and accent bar use — the stored `root_color`, held in `GlossOverlay.bar_color`
  as parsed RGB. Do not re-parse the hex; do not dim or blend.
- **Trigger 1 — on open.** When a gloss or synopsis overlay opens, scan every
  block for a cached mp3 and color the cached ones.
- **Trigger 2 — after synthesis.** The instant a block's mp3 is written and its
  DB row saved, re-color that block in the still-open overlay. Applies to:
  - Gloss single-block synth (Space, synth-on-miss)
  - Gloss batch synth (Shift+Space, per block as each completes)
  - Synopsis single-block synth (Space, synth-on-miss)
  - Synopsis batch synth (Shift+Space, per block)
- Uncached blocks keep the default foreground.
- Re-applying to an already-colored block is idempotent (no flicker, no error).

## Non-goals (YAGNI)

- No new persistent state on `GlossOverlay` beyond reusing `blocks` and
  `bar_color`.
- No change to accent-bar, speaker, or any existing coloring.
- No coloring in the "Glossing…" loading card (it has no real blocks yet).
- No config toggle, no second color mode.

## Existing structures this builds on

- **`GlossOverlay.blocks: Rc<RefCell<Vec<BlockRange>>>`** — one `BlockRange` per
  cursor-stop block in document order, each with `kind: BlockKind`
  (`Source` | `Explication`), `index: i32` (0-based, counted within kind for
  gloss; sequential for synopsis), `start_line: i32`, `end_line: i32` (buffer
  line span). Populated by `rebuild_block_ranges` (gloss) /
  `rebuild_block_ranges_from(synopsis_blocks(..))` (synopsis). Same vec is reused
  by both overlays — they are mutually exclusive.
  (`src/ui/gloss_overlay.rs:17`, `:70`, `:1076`, `:1640`, `:1687`)
- **`GlossOverlay.bar_color: Rc<RefCell<(f64,f64,f64)>>`** — the parsed
  `root_color`, set in `show_gloss_with_color` / `show_synopsis`. Reuse as the
  foreground for colored blocks. (`src/ui/gloss_overlay.rs:45`, `:590`, `:884`)
- **DB existence check pattern** (authoritative cache index):
  - Gloss: `find_gloss_audio(conn, gloss_id, kind_str, index, voice_id)` then
    `Path::exists()`. `kind_str` is `"source"` | `"explication"`.
  - Synopsis: `find_synopsis_audio(conn, work_abbrev, div1, div2, index, voice_id)`
    then `Path::exists()`.
  - Both try the active voice first, then `ALICE_VOICE_ID` fallback (matching the
    existing playback cache-hit logic at `gloss.rs:1057`, `:1355`).

## Design

### Component 1 — `GlossOverlay::color_audio_blocks` (UI-only)

A new public method on `GlossOverlay`:

```rust
/// Color each block for which `is_cached(kind, index)` returns true with the
/// stored accent color (`bar_color` = root_color). Idempotent. Call AFTER
/// `apply_font`, with `blocks` already populated.
pub fn color_audio_blocks(&self, is_cached: impl Fn(&BlockKind, i32) -> bool)
```

Behavior:
1. Look up (or add once) a `TextTag` named `gloss-audio-cached` in the buffer's
   tag table, with `foreground` set from `bar_color` (converted to a hex/RGBA
   string GTK accepts). Update its foreground each call in case the theme
   changed.
2. Bump the tag's priority to the top (the `synopsis-label` pattern at
   `gloss_overlay.rs:508`) so it reliably wins over the buffer-wide `gloss-font`
   tag. (Foreground is not set by `gloss-font`, so this is belt-and-suspenders.)
3. For each `BlockRange` in `self.blocks` where `is_cached(&kind, index)`:
   apply the tag over `iter_at_line(start_line) .. iter_at_line(end_line + 1)`
   (clamped to buffer end).

This keeps `gloss_overlay.rs` free of DB code — the existence decision is
injected by the caller, consistent with how `root_color` is passed in rather
than looked up.

### Component 2 — caller helpers in the action modules

Two helpers that build the closure (owning DB/state) and call the overlay:

- `recolor_cached_gloss_blocks(state: &Rc<RefCell<AppState>>)` in
  `src/input/actions/gloss.rs`: reads `gloss_id`, `work_abbrev`, active voice;
  opens the DB once; passes a closure that maps `BlockKind` → `"source"` /
  `"explication"` and calls `find_gloss_audio` + `Path::exists()` (active voice
  then Alice). Then calls `state.borrow().gloss_overlay.color_audio_blocks(..)`.
- `recolor_cached_synopsis_blocks(state: &Rc<RefCell<AppState>>)` in
  `src/input/actions/synopsis.rs` (or gloss.rs alongside the synopsis synth
  fns): reads `work_abbrev`, `synopsis_overlay_scene` (`div1`, `div2`), the fixed
  prose voice; opens the DB once; closure calls `find_synopsis_audio` +
  `Path::exists()`. Then calls `color_audio_blocks`.

Open the DB connection **once** per helper call and move it into the closure (or
borrow it), so a multi-block overlay does one connection, not one per block.

### Component 3 — call sites

**On open:**
- After `apply_font()` in `show_gloss_with_color` returns to its caller, the
  gloss-display path calls `recolor_cached_gloss_blocks(state)`. (The overlay
  method needs `blocks` populated — `rebuild_block_ranges` runs at
  `gloss_overlay.rs:613`, before `apply_font` at `:618`, so calling the helper
  after the `show_gloss_with_color` call returns is safe.)
- The synopsis-display path calls `recolor_cached_synopsis_blocks(state)` after
  `show_synopsis` returns.

**After synthesis** (re-color the just-cached block; simplest correct form is to
re-run the whole-overlay helper, which re-tags every cached block including the
new one — cheap, idempotent):
- Gloss single: after `save_gloss_audio` at `gloss.rs:1133`.
- Gloss batch: after each `save_gloss_audio` at `gloss.rs:1225` (inside the loop,
  so colors appear progressively).
- Synopsis single: after the synopsis-single `save_synopsis_audio` (the
  completion arm of `play_synopsis_block`, mirroring `gloss.rs:1133`).
- Synopsis batch: after each `save_synopsis_audio` at `gloss.rs:1307`.

In each async block, `state_for_result` is in scope and reaches the overlay via
`state_for_result.borrow().gloss_overlay`. The borrow must be released before
re-borrowing inside the helper — call the helper after the `save_*` block, not
while holding an existing borrow.

## Error handling

- DB open failure / query error → treat the block as uncached (no color). Never
  panic; never block the open or the synth completion.
- A block range that exceeds the current buffer (shouldn't happen, but guard) →
  clamp the end iter to `buffer.end_iter()`.
- Theme with an unparseable `root_color` → `bar_color` retains its prior value;
  coloring still applies with whatever RGB is stored (same behavior as the
  accent bar today).

## Testing / verification

- `cargo build` and `cargo clippy` clean.
- `cargo test --bins` (pure-logic suite) stays green — no pure unit test covers
  this (it's a rendered-pixel/tag concern).
- **Visual acceptance (must be eyeballed):** the criterion is "the right blocks
  render in the accent color," so per the project's e2e rule this requires a
  rendered overlay. Ask the user to either:
  - run `cargo run`, open a gloss/synopsis where some blocks are cached, and
    confirm cached blocks show in `root_color` while uncached ones do not; press
    Space on an uncached block and confirm it recolors when synthesis finishes;
    or
  - run the headless e2e:
    `./scripts/e2e-env.sh cargo test --test smoke -- --ignored --nocapture`
    and inspect `target/ui/` screenshots (open `h`-opened overlay).

## Files touched

- `src/ui/gloss_overlay.rs` — add `color_audio_blocks`; expose `BlockKind`/block
  iteration as needed.
- `src/input/actions/gloss.rs` — add `recolor_cached_gloss_blocks` +
  `recolor_cached_synopsis_blocks` (or split the latter into synopsis.rs); call
  at the 4 synth-completion points; call on gloss open.
- `src/input/actions/synopsis.rs` — call `recolor_cached_synopsis_blocks` on
  synopsis open.
