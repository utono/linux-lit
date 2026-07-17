# Prose gloss readability in the chat panel — design

## Problem

When a prose work is glossed, the panel interleaves QUOTED SOURCE text with the
model's own COMMENTARY. Today both render in the same ink color and upright
weight (`chat-a-verse-flush` for source, `chat-a-gloss` for commentary), so the
two blur together and the reader can't tell at a glance which is Swift and which
is the gloss. (See the 2026-07-17 screenshot of "A Tale of a Tub".)

Plays/poetry don't have this problem as acutely: they carry a speaker heading
and a deep verse indent that already sets the quote apart. Prose has no speaker,
so source rows use the shallower flush indent and lean entirely on a 10px gap to
separate from commentary.

## Scope

Two typographic changes, prose source only. No structural/markup changes, no new
widgets, and NO static left-accent rule — the existing `.chat-cursor-row` inset
accent bar (which follows the j/k row cursor) stays the only accent bar, per the
user's "accent bar should follow cursor" note.

## Change 1 — italicize prose source

`.chat-a-verse-flush` (the speakerless/prose source row) gains
`font-style: italic`. Quoted prose then reads as clearly set-off from the
upright commentary. Play/verse source (`.chat-a-verse`, which has a speaker
heading and deep indent) is left upright — unchanged — so only prose is
affected.

Stage-direction rows (`.chat-a-stage-flush`) are already italic; leaving them
italic is fine (they're rare in prose and already distinguished by their dimmed
color).

## Change 2 — more gap before the gloss

`.chat-a-gloss` `padding-top` goes from `10px` to `18px`, so each commentary
block separates more clearly from the source it explains.

## Files

- `src/theme.rs` — `generate_css`: add `font-style: italic` to
  `.chat-a-verse-flush`; bump `.chat-a-gloss` `padding-top` 10px → 18px. Both
  the pinned rule and (for italic) the `.chat-panel-float` variant inherit the
  color rules already; `font-style`/`padding` are not overridden in the float
  block, so a single edit to the base rule covers both placements.

## Verification

`cargo build`, then headless cage e2e (or the user's live SIGUSR1 reload):
open a prose work ("A Tale of a Tub" / "Bleak House"), `V`-select a passage,
`-` to gloss, and confirm on screen that source text is italic and clearly
separated from the upright commentary.
