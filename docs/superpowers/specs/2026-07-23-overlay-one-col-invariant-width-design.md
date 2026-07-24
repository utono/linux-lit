# Overlay 1-column-invariant width — design

**Supersedes backlog #13.** The journal Q&A, gloss, and synopsis overlays must
present at the **same dimensions regardless of whether the reader behind them is
1-column or 2-column**, inheriting the **1-column reading width** (the
comfortable prose measure). Reverts the #13 per-work-type fill-fraction change,
which addressed the wrong dimension.

## Problem

The three answer overlays are reading surfaces (glosses, Q&A answers, synopses —
flowing prose you read). Today their outer card **width tracks the reader's
column count**: over a 2-column play spread the overlay is wide
(`window_width × TWO_COLUMN_WIDTH_FRACTION`), over a 1-column work it is the
configured column width. Same gloss, two different widths depending on the work
behind it — an inconsistency the eye must re-adjust to every time, and the 2-col
width is a poor measure for flowing answer prose (it was sized for two verse
columns, not one prose column).

The earlier #13 work tuned the *input-box height* inside a *pinned single-column*
ask card — a surface these overlays don't even use (they use the float ask
column, whose `open` early-returns before the fraction is read). So #13 changed
nothing visible and targeted the wrong axis.

## Key facts (traced)

- All three overlays size from `overlay_card_size(&s)` (`src/app/layout.rs:503`),
  a thin alias over `main_card_rect` (`layout.rs:480`).
- `main_card_rect` returns the **settled `content_hbox` allocation**
  (`layout.rs:481-485`) — which was set by `apply_card_sizing` from
  `target_card_width` and therefore carries the reader's column-count-dependent
  width. Its fallback path (`layout.rs:490-495`) also passes `s.column_count()`.
- `target_card_width` (`layout.rs:371-388`): `column_count >= 2 || translations`
  → `window_width × TWO_COLUMN_WIDTH_FRACTION` (wide); else `column_width`
  (the 1-col measure, prose-adaptive via `effective_column_width`).
- **Height is already invariant** (`card_h`, `layout.rs:497`, no column term).
- **Ask-float width is already invariant** (`config.column_width × 5/8`, floored
  360 — `journal_overlay.rs:565`, `gloss_overlay.rs:600`).
- Overlay call sites (all consume `overlay_card_size`): journal
  `journal.rs:575,732`; gloss `gloss.rs:993,1097,1191,1267,1455,1518,1551,1637,3107`;
  synopsis `scene_synopsis.rs:431,599`.

## Decision

Compute the overlay width as if the reader were **1-column**, independent of the
reader's actual `column_count()`. Keep the height exactly as today (already
invariant). Scope: **the three answer overlays only** — pickers, ask cards,
settings, echo, vocab-add are out of scope this change.

## Component

Change `overlay_card_size` (`src/app/layout.rs:503`) so it no longer aliases the
live `main_card_rect` width. New behavior:

- **Height:** unchanged — take `main_card_rect(s).1` (the settled/allocated
  height, or the window-minus-margins fallback).
- **Width:** compute `target_card_width(window_width, effective_column_width(s),
  1 /* force 1-col */, false /* ignore translations for overlay measure */)`,
  clamped to the window. Forcing `column_count = 1` and `translations = false`
  makes `target_card_width` return the 1-column measure (`effective_column_width`,
  which is prose-adaptive) regardless of the reader's real state.

`effective_column_width` already short-circuits to `config.column_width` unless
the work is 1-col prose (`layout.rs:346`), so calling it here yields the
comfortable prose measure for prose works and the configured width otherwise —
exactly the "1-col reading width" the user chose.

`overlay_card_height` (`layout.rs:508`) already returns `main_card_rect(s).1` —
leave it; it's the invariant height.

### Why not change `main_card_rect`

`main_card_rect` is the SINGLE SOURCE OF TRUTH for the *reader* card and must keep
mirroring the reader's real 2-col width. Only the *overlay* accessor should
diverge. Decoupling `overlay_card_size` from it (rather than editing
`main_card_rect`) is the minimal, correct seam.

## Revert #13

Remove the per-work-type fill-fraction change merged as #13+#14 (commit
`7c78c623`), since it targeted the wrong axis and surface:

- `src/config.rs`: `ask_fill_fraction_by_type`, `default_ask_fill_fraction`,
  `ASK_FILL_FRACTION_RANGE`, `ask_fill_fraction_for`, and their tests.
- `src/app/mod.rs`: `AppState::ask_fill_fraction`.
- `src/db/line_types.rs`: `work_class` + its test — **only if** nothing else
  now uses `work_class`; if it reads naturally as a general helper, keep it but
  confirm no dead-code warning.
- `src/ui/ask_card.rs`: restore `set_input_fill_fraction` +
  `input_fill_fraction` field + the read block to their pre-#13 state. This
  re-opens the #14 question (the method becomes dead again) — so **fold #14 back
  in as an actual deletion this time**: remove `set_input_fill_fraction`, the
  `input_fill_fraction` field, and the now-unreachable read block
  (`ask_card.rs:661-667`). The float path never used it; deleting is safe.
- `src/ui/journal_overlay.rs`, `gloss_overlay.rs`, `src/input/actions/{gloss,
  journal,synopsis}.rs`: revert the `fill_fraction` parameter threading on the
  ask-open helpers back to their pre-#13 signatures.

Net after revert + this change: #13's mechanism is gone, #14 is done as a real
deletion, and the overlays are 1-col-invariant.

## Error handling / edge cases

- Window width 0 (pre-first-allocation): `target_card_width` clamps to
  `window.max(1)`; keep the existing `.min(ww.max(1))` guard.
- Translations visible: the reader forces 1-col already, so overlay width is
  unaffected; passing `translations=false` to the overlay width calc is correct
  (we always want the 1-col measure for the overlay).
- Chat pinned: orthogonal (narrows the reader card, not the overlay measure) —
  no interaction; the overlay still wants the 1-col reading width.

## Testing

- `cargo test --bins`: a unit test on the new `overlay_card_size` width — given a
  window width and a work configured 2-col, assert the returned width equals the
  1-col `target_card_width`, NOT the 2-col proportional width. (Construct or mock
  the minimal AppState the function needs, mirroring existing layout tests.)
- Headless (cage) + **real GL renderer** (the acceptance path, per CLAUDE.md
  cage-caveat): open the gloss/journal/synopsis overlay over a **2-column play**
  and over a **1-column prose** work; pixel-measure the overlay card width in
  both and confirm it is identical (the 1-col measure), not wider over the play.
  Do NOT eyeball — sample the cream/teal boundary.

## Out of scope

- Pickers and non-answer overlays (user chose the 3 answer overlays only).
- Any per-work-type variation (explicitly rejected — the goal is invariance).
- Overlay height changes (already invariant).
