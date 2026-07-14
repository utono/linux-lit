# Design — journal Q&A matches gloss look-and-feel

_Date: 2026-07-01 (US Central)._

## Context

After the inset-panel work, the journal, gloss, and synopsis overlays share the
panel frame, colors, and inner padding. The user wants the journal Q&A to adopt
two remaining gloss traits so the two surfaces read the same:

1. **Answer body indented past the accent bar** — in gloss the explication body
   is indented ~60px right of the accent bar (`bar_left + 60`), so the bar sits
   in a clear gutter beside the text. The journal Q&A currently renders flush-left
   at the view margin, with the bar pushed 12px into the pad gutter.
2. **`Q:` line styled as a header** — gloss shows a small-caps-style speaker label
   (`.gloss-header`: small, bold, dim, `letter-spacing: 2px`) above the body. The
   journal's `Q: …` is inline plain body text.

## Decisions (from brainstorming)

- **Header content:** keep the literal `Q: ` prefix, but render that whole line in
  the header style (user's choice — not "question-as-title", not a fixed
  QUESTION/ANSWER label).
- **Indent mechanism:** shift the WHOLE body (header + answer) right via the
  journal `view`'s `left_margin` — NOT a per-paragraph `TextTag`. The journal
  already computes wrap width and pagination height from `left_margin`
  (`card_width - 2 * left_margin`), so a larger left_margin flows through
  automatically with NO pagination/measurement change. This is the safe option;
  the per-paragraph-tag alternative would require teaching `repaginate`/`measure`
  to subtract the indent or wrapping/clipping breaks. Tradeoff accepted: the `Q:`
  header shifts right with the body (not flush-left like gloss's speaker label) —
  acceptable, and simpler.
- **Header CSS:** reuse the existing `.gloss-header` class (identical look, one
  fewer class) rather than a dedicated `.journal-qa-header`.
- **Indent amount:** 60px (matching gloss's `bar_left + 60` gap), a tuning
  constant.

## Scope

`src/ui/journal_overlay.rs` only (plus possibly a comment in `theme.rs` if a class
is added — but we reuse `.gloss-header`, so no CSS change).

### Part 1 — body indent via view left_margin
- In `size_card` (~line 462), where the journal sets
  `self.view.set_left_margin(side)`, add the indent to the LEFT only:
  `self.view.set_left_margin(side + JOURNAL_BODY_INDENT)` with
  `JOURNAL_BODY_INDENT = 60`. The right margin stays `side` (the indent is a
  left-only shift into the bar gutter; the column narrows by 60px on the left,
  which is the intended "body pushed right of the bar" look).
- No change to `repaginate`/`measure`/`render_page` pagination math — it reads
  `self.view.left_margin()` live (confirmed: wrap width at ~line 1040 is
  `card_width - 2 * self.view.left_margin()`), so the new margin is picked up
  automatically. (Note: because wrap subtracts `2 * left_margin`, the right side
  loses the same 60px of usable width as the left even though the right MARGIN is
  unchanged — the column simply narrows by ~60px, centered slightly left. This is
  fine and matches the "indented body" intent; verify on screen that it doesn't
  over-narrow.)

### Part 2 — header-styled `Q:` line
- In `render_page`, after inserting the page text, apply a header `TextTag` to the
  `Q:` line's character range (only when the page contains the question line —
  i.e. the first page / the block that starts with `Q:`). The tag mirrors
  `.gloss-header`: reduced size, bold, dim foreground, letter-spacing, with
  `pixels_below_lines` for breathing room. Reuse the existing `tag_table` /
  `apply_tag` infra already used for `<hi>` (journal_overlay.rs ~747-762).
- The `Q:` detection reuses `prefix_question` / the existing `Q:`-prefix
  convention (a line starting with `Q:`); only that line gets the header tag, the
  answer paragraphs stay body-styled.

## Data flow

Unchanged pagination. `size_card` sets a larger left_margin → wrap width and
per-paragraph measured height (both derived from `left_margin`) shrink
accordingly → pagination already correct. `render_page` additionally tags the
`Q:` line range with the header tag after inserting text.

## Testing

- `cargo build` + `cargo test --bins` — the paragraph-splitting + prefix logic is
  unit-tested; the margin + tag change is geometry/styling with no new pure logic.
- **On-screen (user):** open the journal (Ctrl+j) on a 2-col play:
  - the `Q:` line reads as a styled header (small, bold, dim, spaced) distinct
    from the answer body;
  - the answer body is indented right of the accent bar, the bar sitting in a
    clear gutter beside it — matching gloss;
  - nothing clips; pagination still turns cleanly (the narrower column may change
    where pages break — that's expected and fine as long as no partial line shows).

## Risks / notes

- **Column narrows by ~60px.** Because wrap subtracts `2 * left_margin`, adding 60
  to the left reduces usable width by 60. If the column reads too narrow, reduce
  `JOURNAL_BODY_INDENT` (one-number tune) — not a redesign.
- **Header tag range.** The `Q:` line is the first paragraph; on later pages (the
  answer continues) there is no `Q:` line, so the header tag simply isn't applied
  — correct. Apply the tag only when the rendered page text starts with `Q:`.
- Gloss is untouched; this is journal-only. The two now share the `.gloss-header`
  look and the bar-in-gutter geometry, which is the standardization goal.
