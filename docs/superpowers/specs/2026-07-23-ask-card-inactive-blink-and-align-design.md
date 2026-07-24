# Ask card: pause caret blink when inactive + top-align the float with the doc card

**Date:** 2026-07-23
**Status:** Approved, ready to implement

Two related ask-card polish items.

## Item 1 — freeze the INSERT caret when the ask card is inactive

**Bug.** In the 2-col float layout the reader can Ctrl+Tab focus to the doc card,
leaving the ask card visible but inactive. Its INSERT-mode caret keeps blinking
(`AskCard::start_caret_blink`'s timer is never stopped on focus loss), so a
blinking cursor sits on an unfocused surface.

**Fix.** Add `AskCard::set_active(bool)` (forwarded via `AskCardHost::set_active`):
- `active=false` → stop the blink timer AND leave the caret visible (freeze it
  SOLID, so the reader still sees where they were typing).
- `active=true` → resume the blink, but only if the vim engine is in INSERT
  (NORMAL/VISUAL use the painted block cursor, which never blinks).

Drive it from the single focus chokepoint: each overlay's
`set_ask_focus_dim(ask_focused)` (gloss + journal) already fires at every
focus-change site (Ctrl+Tab toggle, Escape-refocus, initial open), so call
`self.ask_host.set_active(ask_focused)` there. Close already stops the blink.

## Item 2 — top-align the float ask card with the doc card

**Bug.** The floated ask card's "Ask a question…" title sits higher than the doc
card's running head. Root cause: the two cards have different TOP EDGES. The
gloss/journal doc card is `valign=Center` (vertically centered — so it sits low
when its content is short, e.g. the gloss-result view), while the ask float
container is `valign=Fill`, which pins it to the top of the full-height overlay.
Same height (the reserve closure already caps the ask panel to the doc card's
height) but different vertical anchor → tops don't line up → headers don't align.

**Fix.** Change the ask float container's `valign` from `Fill` to `Center`
(matching the doc card). With both cards centered and the ask panel's
`height_request` already capped to the doc card's height, their top edges
coincide, so the "Ask a question…" title lines up with the running head — for
both the short-content gloss-result view and the full-height journal view.

Keep the existing reserve closure that pins the ask panel height to the doc
card's height (`container.height().max(height_request)`). No title-margin nudge
is needed once the card tops align; the small residual title-baseline offset (the
running head and the `gloss-header` ask title use different fonts) is left as-is
unless the render shows it still reads as misaligned.

## Non-goals

- No change to the 1-col stacked (journal) ask-card layout, where the ask card
  is below the reading scroll, not a float.
- No change to the ask-card background, dimming, or border (recent work).

## Testing

- `cargo build` + `cargo clippy` clean; `cargo test --bins` green.
- Headless cage, gloss float (MM 4.2, Ctrl+a) and journal float:
  - Pixel-measure that the ask card's top edge matches the doc card's top edge,
    and the "Ask a question…" title's y is within a few px of the running head's.
  - Ctrl+Tab to the doc card → confirm (by the blink timer log / two screenshots
    ~530ms apart) the ask caret is frozen solid; Ctrl+Tab back → it blinks again.
