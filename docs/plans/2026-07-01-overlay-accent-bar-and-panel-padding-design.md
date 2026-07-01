# Design — overlay accent-bar standardization + wider panel padding

_Date: 2026-07-01 (US Central)._

## Context

The inset tinted panel (branch `feat/overlay-inset-panel-framing`) now frames the
prose column in the journal, gloss, and synopsis overlays. On screen the user
noticed two polish issues:

1. **The accent bar sits in different places across the three overlays.** In the
   gloss (and synopsis, which renders through the gloss widget) the selection
   accent bar draws AT the text's left margin — just inside the panel, beside the
   text, which reads well. The journal overlay draws its bar 12px OUT into the
   gutter (`left_margin() - 12.0`), left of the panel's text column — the outlier.
2. **The text runs too close to the panel's inner right border.** The last
   character of a wrapped line stops only ~`PANEL_PAD` (10px) from the panel edge,
   so the column feels cramped against the frame.

Both are small geometry tweaks in the overlay UI layer. No new concepts.

## Decisions (from brainstorming)

- **Standardize the journal accent bar to the gloss position:** draw it at the
  text's `left_margin()` (exactly like gloss's `bar_x = left`), not
  `left_margin() - 12.0`. This also matches the journal's own blank-line block
  cursor, which already draws at `left_margin()`.
- **Widen the panel gap symmetrically:** increase `PANEL_PAD` in
  `draw_overlay_panel` from `10.0` to **`24.0`**, so the panel's inner edges sit
  further from the text on BOTH sides. Text margins are unchanged (still
  symmetric), so only the panel rectangle grows outward — an even, comfortable
  gap all around the text. The accent bar still hugs the text, so it gains a
  little breathing room from the panel's left edge (reads as intentional).

Explicitly NOT changing: text margins, fonts, line spacing, paragraph
separation, or the panel color. The user confirmed none of those is the concern.

## Scope

- **Journal accent bar** (`src/ui/journal_overlay.rs`): the selection-bar x in
  the `bar_drawing` draw func, currently `(view.left_margin() as f64 - 12.0).max(2.0)`
  (~line 258), becomes the text's left margin `(view.left_margin() as f64).max(2.0)`
  — matching the blank-line block cursor at ~line 240 and the gloss bar. One line.
- **Panel pad** (`src/ui/mod.rs` / the two `draw_overlay_panel` call sites): the
  `pad` argument passed at the gloss (`gloss_overlay.rs`) and journal
  (`journal_overlay.rs`) panel draw funcs changes `10.0` → `24.0`. (Synopsis
  shares the gloss widget's panel, so it is covered by the gloss call site.)

## Data flow

Unchanged. `draw_overlay_panel` still reads the view's live `left_margin()` /
`right_margin()` each paint; only the constant `pad` it insets by grows. The
journal bar's x is still computed from the live `left_margin()` each paint; only
the `- 12.0` offset is removed.

## Testing

- `cargo build` + `cargo test --bins` — pure geometry, no logic change, no new
  unit test warranted (there are deliberately no pixel-level unit tests for these
  draw funcs; the clipping invariants still apply).
- **On-screen (user):** open all three overlays on a 2-col play (Cymbeline):
  - the journal accent bar sits INSIDE the panel beside the text (like gloss),
    not out in the gutter;
  - the gap between the last character of a line and the panel's inner border is
    visibly larger, and even on left and right, across all three overlays;
  - nothing clips; the accent bar and page marker still paint on top of the panel.

## Risks / notes

- `PANEL_PAD = 24` is a tuning constant; if 24 reads too wide/narrow it is a
  one-number follow-up, not a redesign. The user chose 24 as the starting value.
- The panel grows outward by (24 − 10) = 14px per side. It is drawn clamped to
  `[0, area_w]` in `draw_overlay_panel`, so a wider pad cannot push the panel off
  the DrawingArea; at worst it reaches the card edge (still inside the cream
  gutter, since the card is much wider than the text column).
