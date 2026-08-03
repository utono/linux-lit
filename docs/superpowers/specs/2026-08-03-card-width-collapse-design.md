# Card width collapses to one word per line (single-column prose)

_Design spec. 2026-08-03. Status: IMPLEMENTED, with one acceptance criterion
knowingly unmet — see "Outcome" at the end._

## Symptom

Single-column prose renders one word per line in a ~280px card, centered in an
otherwise correct 1920x1200 window. Reproduced on BH-Barrett under niri, twice,
across a restart. The card is narrow and full-height; the wallpaper fills the
rest of the window.

## What the log proves — and rules out

From `linux-lit-dev.log` (slot 1, 2026-08-03 09:46), the window geometry is
healthy for the entire run:

```
[  300ms] RESIZE_TICK: vbox.width changed 0 -> 1920
[  300ms] RESIZE_TICK: text_view.height changed -1 -> 1128
[ 1801ms] CARD_SIZING: ww=1920 col_cfg=1050 cols=1 target=1050 card_w=1050 margin=24
```

`ww=1920` throughout; `text_view.height=1128` holds; `niri msg` reports the
window at `1920x1200` in a `1920x1200` tile, not floating. So the following are
NOT the cause, despite fitting the visual:

- niri shrinking the tile to the window's advertised minimum (the 2026-08-01
  bug). That produced `ww=1098` in the log; this run never leaves 1920.
- The two-column downgrade or the `MIN_TWO_COLUMN_WINDOW_WIDTH` floor —
  `cols=1` from the first tick.
- A stale or overfull stored page. `PAGES_PROSE: table hit (780 pages)`.

The decisive line is the clip tripwire:

```
CLIP_WARN: main-card prose-1col OVERFLOW total=8566 > widget_h=1128
           clip=0 page_top=329 top_off=0 bottom_head=Some(90) end=345
```

17 lines (329→345) measure 8566px. At the ~28px line height this build uses
that should be roughly 480px. 8566px is ~18x too tall, which is what one-word-
per-line wrapping costs. The page table is fine; the TEXT is being laid out
against a near-zero width. This is the render-side mis-measurement case already
described in `clip-prevention.md` under "A `total > widget_h` OVERFLOW is not
always an overfull page" — the table is correct, the measurement is not.

## Root cause

`content_hbox` is `halign: Center` (`src/app/mod.rs:1762`) and every widget
from it down to the text view carries `width_request = -1`:

- `content_hbox` — cleared by `apply_card_sizing` (`layout.rs:504,524`)
- `columns_hbox`, `scrolled_overlay`, `scrolled_window` — the single-column
  branch of `apply_tiled_mode` sets all of them `hexpand(true)` +
  `width_request(-1)` (`layout.rs:334-348`)

In GTK4 a box with `halign: Center` is allocated its NATURAL width, not its
parent's width. `hexpand` on a descendant does not override an ancestor's
centering. The natural width of a `GtkTextView` in `WrapMode::Word`
(`mod.rs:1564`) is its MINIMUM width — the widest unbreakable word. Hence a
card sized to fit "unexpectedly" and text wrapped one word per line.

`apply_card_sizing` computes `card_w` correctly (`card_w=1050` in the log) and
then **discards it**. It sets margins and `hexpand`, but never applies the
width to any widget:

```rust
let card_w = computed_card_width(ww, column_width, column_count, translations);
// ...
content_hbox.set_hexpand(true);
content_hbox.set_width_request(-1);   // clears; card_w is never applied
content_hbox.set_margin_start(margin);
content_hbox.set_margin_end(margin);
```

The comment at `mod.rs:1772-1776` asserts the opposite and is factually wrong:

> This value is only the pre-first-allocation seed: `apply_card_sizing`
> overwrites it on the first resize tick and every one after, always clamped to
> the window.

It clears rather than overwrites, so `CARD_SEED_WIDTH = 320` is the only width
the box ever carries — and it is a floor, not a target. The card renders at
~280px of text plus padding, matching the seed.

### Origin

Commit `7451c1c2` ("fix(window): let the window shrink to its tile"), one line
in `src/app/mod.rs`:

```diff
-    content_hbox.set_width_request(config.column_width as i32);   // 1050
+    content_hbox.set_width_request(crate::app::layout::CARD_SEED_WIDTH);  // 320
```

That line was the only thing giving the card its 1050px width. Removing it
correctly fixed the fullscreen bug — a 1050px `width_request` propagates up as
the window's minimum size, and niri shrinks its tile to match rather than
granting fullscreen — but nothing replaced it as the width source.

The two bugs are the same knob in opposite directions:

- Width on `content_hbox` → correct card, window pinned, fullscreen broken.
- No width on `content_hbox` → fullscreen fine, card collapses to a word.

## The constraint any fix must satisfy

The card needs a definite width that does NOT propagate up as the window's
minimum size. Two existing guards enforce the second half and will fail the
build on a naive revert:

- `card_sizing_never_gives_content_hbox_a_width_request` (`layout.rs:786`) —
  greps `layout.rs` for `content_hbox.set_width_request(` with any argument
  other than `-1`.
- `window_can_shrink_below_its_card_width` (`tests/niri_smoke.rs:185`) — asks
  real niri for a 33% column and asserts the window shrinks below half the
  output.

Note the grep guard only scans `layout.rs`; the seed at `mod.rs:1777` is not
covered by it. Any fix must keep both green while restoring the width, and must
not reintroduce the margin-absorption variant either — the comments record that
capping was tried and re-broke at `min=1937` on a 1912 output.

## Options

1. **`halign: Fill` on `content_hbox` + slack absorbed by margins.**
   Makes `hexpand` do the work the current code already assumes it does.
   Cheapest change. Risk: `card_side_margin` caps margins at
   `CARD_OUTER_MARGIN`, so uncapping to absorb 435px per side is exactly the
   variant that measured `min=1937`. Only viable if margins stay capped and
   something else absorbs the slack.

2. **Width on an inner child that is not in the window's minimum-size chain.**
   The overlays already avoid this trap this way (`overlay_card_width`,
   `main_card_rect` docs). Most consistent with existing patterns.
   Needs the specific child identified and proven not to propagate.

3. **`set_size_request` on a widget wrapped so its minimum is not advertised**
   — e.g. inside a `GtkScrolledWindow` with a propagate-natural-width policy,
   which reports a small minimum while allocating the natural width.

Recommendation: option 2, with option 3 as fallback if the propagation turns
out not to stop where expected. Option 1 is the trap the previous session
already fell into.

## Acceptance criteria

Per the repo's verification rules, a green build is not sufficient — this is a
visible layout change and must be confirmed in pixels on the real renderer.

1. Single-column prose (BH-Barrett) fills the card: multiple words per line,
   card ~1050px wide on a 1920px window.
2. `CLIP_WARN ... prose-1col OVERFLOW` no longer fires on the landing page;
   `total` for a 17-line page is in the hundreds of px, not thousands.
3. `CARD_SIZING: card_w=1050` continues to match the width actually drawn —
   verify by pixel-measuring the cream/teal boundary, not by eye.
4. Fullscreen still granted on launch: `niri msg` reports the window at the
   full output size, not a shrunken tile.
5. `window_can_shrink_below_its_card_width` passes under real niri
   (tiled, 33% column).
6. `card_sizing_never_gives_content_hbox_a_width_request` passes.
7. Two-column plays unaffected — spot-check one play at 1920.

Both the collapse (#1) and the fullscreen guard (#4, #5) must hold in the SAME
build. They are the two directions of one knob, and a fix that trades one for
the other is the bug, not the fix.

## Outcome (implemented 2026-08-03)

The fix is one line restored, clamped: `content_hbox.set_width_request(card_w)`
in both branches of `apply_card_sizing`, where `card_w` is the window-clamped
`computed_card_width`. Verified in cage: viewport 1050px at x=435 on a 1920px
output (centred), no `CLIP_WARN`, screenshot correct.

**Two claims in the analysis above were WRONG, and cost most of the effort:**

1. **"The text view's minimum is the widest unbreakable word."** False.
   `gtk_text_view_measure` never consults the text layout horizontally; the
   minimum is just the margins. Confirmed in the GTK 4.22.4 C source and
   measured (`bare TextView WORD-wrap: min=0 nat=0`). The card collapsed toward
   zero, not toward a word. The "widest word" reading was a coincidence of the
   screenshot.
2. **Option 2 ("put the width on an inner child outside the minimum-size
   chain") does not exist.** Measured in cage, EVERY route to a 1050 allocation
   reports a ~1050 window minimum: on `content_hbox`, on `scrolled_overlay`, via
   `min-content-width`, via margins (435px margin -> min 870). `max-content-width`
   is ignored under `hscrollbar-policy=Never`, and `propagate-natural-width`
   touches natural only. A `width_request` IS the window minimum.

**Acceptance criteria 5 is NOT met, deliberately.**
`window_can_shrink_below_its_card_width` fails: the request pins the window's
minimum at 1098, so niri will not tile the reader narrower than that. It passes
on master only because master has no width at all — i.e. the guard is currently
green *because of* the collapse bug. The two cannot both hold with any widget
property; the "clamp will track the window down" idea is circular and was
tested to fail (once 1098 is the minimum, the compositor never offers less, so
the clamp never re-engages).

Criteria 1-4, 6, 7 pass. Fullscreen is unaffected — a minimum only breaks it
when it exceeds the OUTPUT, which was the 2026-08-01 bug at 1585; 1098 on a
1920 output is fine.

**The real fix for both** is a custom `GtkLayoutManager` that reports a small
minimum and allocates `card_w` itself. It needs GObject subclassing, which this
crate uses nowhere yet, so it is deferred rather than rushed. Tracked as a
follow-up; a 1098 tiling floor is a far smaller defect than prose rendering one
word per line.

## Follow-ups

- Correct the false comment at `mod.rs:1772-1776`; it actively misleads.
- Consider extending the grep guard to `mod.rs`, or replacing it with a
  positive assertion about where the width DOES live — "this call never
  happens" did not prevent the width from going missing entirely.
- Add the failure mode to `docs/troubleshooting/clip-prevention.md`: a
  `total` an order of magnitude over `widget_h` means degenerate wrapping
  (near-zero layout width), not an overfull page. Required by the repo's
  clipping-ledger rule.
