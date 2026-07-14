# Prose works: NYTimes-style centered text column

**Date:** 2026-06-27
**Branch:** `feat/prose-nyt-column`
**Status:** Design — awaiting user review

## Goal

Reposition the text of **prose works** so it reads like the body column of a
nytimes.com article: a fixed-width text column, **centered** in its card, with
generous and **symmetric** left/right whitespace (~⅓ of the card on each side).

This applies to **four surfaces**, all for prose works only:

1. The **main reading card** (monocle, single column).
2. The **synopsis overlay** (`h`).
3. The **gloss overlay** (`Ctrl+g`, etc.).
4. The **journal overlay** (`Alt+w`).

Verse / play / sonnet / two-column / translation / BCP layouts are **unchanged**.

## Target geometry

The reference screenshot (≈1920px window) shows a centered body column ~650px
wide with ~⅓ whitespace each side. The overlay/reading card is `column_width`
(default 1050px) wide, centered in the window.

**Decision (user-approved): a symmetric inset of `card_width / 5` on both
sides.**

- `1050 / 5 = 210px` left, `210px` right → column ≈ `1050 − 420 = 630px`.
- Matches the screenshot's centered ~⅓-margin look.
- The column is **centered** in the card (equal margins), unlike today's
  left-aligned prose column.

A single shared helper expresses this so all four surfaces stay in lockstep:

```rust
// src/ui/mod.rs — beside card_side_margin
/// Symmetric inset (both sides) for the NYTimes-style centered prose column.
/// Wider whitespace than `card_side_margin` (card_width/4): a ~⅓-margin
/// reading measure. Used by the prose reading card and the prose overlays.
pub(crate) fn prose_column_margin(card_width: i32) -> i32 {
    card_width / 5
}
```

## Scope decisions (defaults chosen; stated for review)

- **Prose only.** The branch points that change are guarded by the existing
  `is_prose_work(work_type)` check. Verse, two-column, sonnet-sequence,
  translation, and BCP paths are not touched.
- **Main card padding constant is overridden.** The original request said
  "don't change prose main-card padding," but the follow-up clarified that the
  main card should also be repositioned to match the screenshot. Per the
  user's selection, the prose main reading card moves to the **same**
  `prose_column_margin` symmetric inset as the overlays, so the reading card
  and its overlays line up.
- **Tiled / narrow window degrades to today's behavior.** When the card nearly
  fills the window (`is_tiled_layout`), the prose reading card keeps its
  current tiled rule (offset → 0, text at `text_margins`). The NYTimes inset is
  a monocle (wide-window) aesthetic, exactly like the existing
  `VERSE_LEFT_OFFSET` degradation. Overlays always size from the card width, so
  on a narrow window their inset shrinks proportionally too.
- **Symmetric.** Both left and right use `prose_column_margin`. The prose main
  card today is asymmetric (160 left / 88 right); it becomes symmetric.

## Changes by surface

### 1. Main reading card — `src/app/layout.rs::apply_tiled_mode`

Currently (prose monocle):
- `left_bump = PROSE_LEFT_OFFSET (120)` → `logical_left = text_margins + 120 = 160`.
- right margin = `text_margins + EXTRA_RIGHT_MARGIN = 88`.

Change (prose monocle, untiled, single column only):
- Compute the centered inset from the **actual card width**
  (`target_card_width(...).min(window_width)`), then derive
  `left_bump = (prose_column_margin(card_w) − text_margins).max(0)` so
  `logical_left = prose_column_margin(card_w)`.
- Set the **right** margin to the same `prose_column_margin(card_w)` (symmetric),
  replacing the `text_margins + EXTRA_RIGHT_MARGIN` value for this case only.

This mirrors the existing `translations_visible` branch, which already computes
a card-relative inset via `card_side_margin(card_w) − text_margins`. The prose
branch is the same shape with `prose_column_margin` instead.

Guard order in the existing `if/else` chain is preserved: `translations` →
`one_section_per_page` → `tiled` (→0) → `two_col` → `is_verse` → **prose
(changed)**. Only the final prose `else` and the corresponding right-margin
`else` change. The sign-column gutter is `logical_left − 20` wide and
right-aligned, so a wider `logical_left` just widens the (empty) gutter
harmlessly — no wrap risk for prose (prose has no verse line-length constraint).

### 2. Synopsis overlay — `src/app/scene_synopsis.rs::prose_synopsis_card`

`SynopsisProseCard` already carries caller-supplied `left_margin` / `right_margin`
(pixels) and the prose path in `gloss_overlay::show_synopsis` consumes them. Only
the values change:

- `left_margin = prose_column_margin(card_width)` (was `text_margins + PROSE_LEFT_OFFSET`).
- `right_margin = prose_column_margin(card_width)` (was `text_margins + EXTRA_RIGHT_MARGIN`).

`prose_synopsis_card` currently has no `card_width` in scope. It is called from
**six** sites — three in `scene_synopsis.rs` (`show_synopsis_overlay`,
`cycle_synopsis`, and `show_synopsis`) and three in `input/actions/synopsis.rs`
(the open / amend / re-render paths) — each of which already computes
`overlay_card_size(&s)` (or has the card width to hand). Add a `card_width: i32`
parameter: `prose_synopsis_card(state, card_width)`, and update all six call
sites to pass the card width they already hold. Font family/size fields are
unchanged (still the reading card's font).

The accent-bar offset in `show_synopsis` (`bar_left = (left_margin − 60).max(0)`)
still works: `210 − 60 = 150`, bar visible to the left of the body.

### 3. Gloss overlay — `src/ui/gloss_overlay.rs::show_gloss_with_color`

`show_gloss_with_color` is shared by verse and prose glosses and currently always
uses `card_side_margin(card_width)` (card_width/4). To switch **prose** glosses to
the NYTimes column without disturbing verse glosses, the renderer needs to know the
work is prose. There are **10** `show_gloss_with_color` call sites and **4**
`show_glossing` call sites across `gloss.rs`, `journal.rs`, `visual.rs`,
`synopsis.rs`, plus a test in `gloss_overlay.rs` — too many to thread a bool
through cleanly.

**Approach: a stateful `set_prose(bool)` setter on `GlossOverlay`** (mirroring the
existing per-overlay setters `set_position` / `set_citation` / hint setters). The
overlay stores `is_prose: Cell<bool>`. `display_work` (or `display_work_at`) calls
`gloss_overlay.set_prose(is_prose_work(&work.work_type))` once per work load. Then
`show_gloss_with_color` and `show_glossing` read that cell: prose →
`prose_column_margin(card_width)`, else `card_side_margin(card_width)`. The accent
bar `bar_left` follows the same value as today (it equals the left inset). No call
site changes — only the renderer body and the one `display_work` setter call.

The standalone unit test (`gloss_overlay.rs:1793`) constructs an overlay directly;
its default `is_prose = false` keeps the test's verse geometry, so it needs no
change.

Rationale for a stateful setter over a per-call bool: the overlay intentionally
stays a dumb renderer with no `AppState`, and the work type is constant for the
life of an open work — set it once, not on every render call.

### 4. Journal overlay — `src/ui/journal_overlay.rs::size_card`

`size_card` always uses `card_side_margin(card_width)` and has no prose branch.
Mirror the gloss approach: add `is_prose: Cell<bool>` + a `set_prose(bool)` setter,
called from `display_work` alongside the gloss one. When prose, `size_card` uses
`prose_column_margin(card_width)` for the view's left/right margins and the title's
`margin_start`. `show_passage_page` renders **source verse** (a passage excerpt)
and keeps `card_side_margin` for that verse block regardless — passages are verse,
not prose body. `show_passage_page` already calls `size_card` first; to keep the
verse inset it explicitly re-sets `self.view.set_left_margin/right_margin` to
`card_side_margin(card_width)` (and `title.margin_start`) **after** `size_card`,
overriding whatever the prose flag produced. (`bar_left`/`populate_verse_buffer`
already use `card_side_margin` there.)

## Shared helper

Add `prose_column_margin(card_width) -> card_width / 5` in `src/ui/mod.rs`
next to `card_side_margin`. Every surface above calls it, so the ratio lives in
one place and the four surfaces can never drift.

## What is explicitly NOT changed

- `card_side_margin` (card_width/4) — still used by verse/play overlays and the
  verse synopsis/gloss/journal paths.
- `PROSE_LEFT_OFFSET`, `VERSE_LEFT_OFFSET`, `EXTRA_RIGHT_MARGIN`,
  `text_margins` — constants stay; only the prose monocle reading-card branch
  stops using `PROSE_LEFT_OFFSET` / `EXTRA_RIGHT_MARGIN`.
- Two-column, sonnet-sequence, translation, BCP, and tiled paths.
- Vertical layout (scroll, clip, top/bottom margins), fonts, colors, the accent
  bar's existence, and all audio/TTS.

## Testing & verification

This is a pure horizontal-geometry change whose acceptance criterion is "it
renders centered like the screenshot." Per `CLAUDE.md`, that requires a rendered
spread, not just `cargo test --bins`.

- `cargo build` and `cargo test --bins` must stay green (the helper is trivially
  unit-testable: `prose_column_margin(1050) == 210`).
- Runtime verification is **visual** and must be done by launching the app on a
  prose work (e.g. Bleak House) and:
  - eyeballing the main reading card (centered ~630px column),
  - opening `h` (synopsis), `Ctrl+g` (gloss), `Alt+w` (journal) and confirming
    each overlay's body is the same centered column,
  - confirming verse works (e.g. Rom) are **unchanged**.
- Because the agent generally cannot drive cage on the live dwl seat, the user
  will run the visual check (headless `e2e-env.sh` or a direct `cargo run` on a
  prose work) and confirm the spread, or paste a screenshot.

## Open questions

None. Ratio (`/5`), symmetry, prose-only scope, and main-card inclusion are all
resolved by the user's answers.
