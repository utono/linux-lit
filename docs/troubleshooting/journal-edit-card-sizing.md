# Journal edit card (`e` bind) sizing — covering the whole Q&A card without overflowing the parent

This documents a multi-round effort to make the journal overlay's **edit card**
(opened with `e`, the `JournalEditCard` rendered by `open_edit_card`) **cover the
entire Q&A page text** so the Answer field is a large editing area, while keeping
its line-wrapping matched to the page and — critically — **never growing the
parent overlay past the reading-card size**.

Read this before touching `src/ui/journal_edit_card.rs::build_field` /
`JournalEditCard::open` / `pin_answer_height` / `fixed_chrome_height`,
`src/ui/journal_overlay.rs::open_edit_card` / `new` (the `scroll_overlay.add_overlay`
site).

## What the user wanted

1. The `e` edit card should **cover ALL of the Q&A page text — top AND bottom**,
   not just the area below the question. "Just like it overlaps the entire WIDTH
   of the Q&A card's text, so also should it overlap the text at the TOP." So the
   edit card is a full-region overlay, not a panel stacked below a visible
   question line.
2. **Wide** — the edit card's line wrapping should match the journal page's
   wrapping (it was wrapping much narrower). (Worked first try; see below.)
3. **Do NOT change the parent overlay's dimensions.** The parent reading card
   must stay the normal size; the edit card floats *over* the page region.

## The layout (why this is fiddly)

The journal overlay's outer `container` (a vertical `gtk4::Box`) is:

- `set_size_request(card_width, card_height)` — a **minimum** size, not a maximum
- `valign: Center` — so the parent overlay (full screen) centers it at its
  *natural* height, which is `card_height` **only if no child demands more**.
  This is the crux of every overflow below: `set_size_request` does NOT cap the
  height; a tall child inflates the box past `card_height` and the whole overlay
  spills off-screen. (The gloss overlay's `size_scroll` has the identical
  comment — it is settled GTK behavior, not a guess.)

Inside that box, appended in order:

1. `scroll_overlay` — the page viewport (an `Overlay`; height pinned by
   `pin_scroll_height`). Carries its own `margin_top(24)` + `margin_bottom(20)`.
2. `footer.container` — hidden while editing
3. `ask.container()` — the AskCard input (hidden while editing)

**The edit card is NOT a child of this box.** It is added as an *overlay* over
`scroll_overlay` (`scroll_overlay.add_overlay(edit_card.container())`, non-measuring
+ clipping, `halign/valign = Fill`). So when shown it **fills the page-text region
and floats on top of it — covering the whole Q&A text, top and bottom** — which is
requirement #1. A vbox sibling could only ever stack BELOW the page viewport,
leaving the question line + chevron visible above it (the old strip approach; see
failed approach #6). The `.ask-card` background is opaque (`{gloss_bg}`), so the
page text behind is fully obscured.

`open_edit_card` does **not** resize the page viewport at all — the overlay covers
it. It only hides the nav footer and **pins the Answer field's height** (below).

The width comes from insets: the page text uses
`prose_column_margin(card_width)` (prose) or `card_side_margin(card_width)`
(verse) as its left/right margin. The edit card's fields add their own inner pad
(scroller margin 16 + TextView margin 6 = `FIELD_INNER_PAD = 22`). So to match
the page wrap, `JournalEditCard::open(text_inset)` sets the container margin to
`text_inset − FIELD_INNER_PAD`, and `open_edit_card` passes the SAME prose-aware
`side` the page uses. (This part worked first try and stayed.)

## What was tried for the HEIGHT, and why each failed

The height was the hard part. `set_size_request` on the journal container is a
**minimum**, so any child that demands more height inflates the container and
therefore the parent overlay (which spills off the top/bottom of the screen).

1. **Scale the target (`card_height * 0.92`, then `* 0.97`).** No visible change.
   Cause: `open_for_natural_height` floored the scroll viewport at a hardcoded
   `.max(80)`; both 0.92 and 0.97 drove the requested scroll height negative, so
   it clamped to the same 80px floor. The target wasn't the limiter — the floor
   was. Fix: gave `open_for_natural_height` a `min_scroll` parameter (set to a
   thin 44px strip).

2. **`set_height_request` on the edit-card container = `card_height − strip`.**
   This GREW THE PARENT. `set_size_request` (min) on the journal container does
   not cap it; a child `height_request` larger than the box's allocation makes
   the box (and the parent overlay) grow past `card_height`. Reverted.

3. **`set_min_content_height` on the Answer scroller, sized by a CONSTANT
   `EDIT_CARD_FIXED_CHROME` (tried 340 → gappy, 250 → overflow, 300 → still
   subtly overflowed).** A constant is fundamentally wrong: `min_content_height`
   is a hard minimum, so any value small enough to fill eventually forces the
   container taller than `card_height` and overflows. Too high leaves a gap; too
   low overflows. There is no safe constant across fonts/card sizes.

4. **`set_min_content_height` sized by MEASURING the fixed chrome**
   (`container.preferred_size().height() − answer_base`). **Non-deterministic** —
   sometimes the card fit, sometimes it overflowed on the SAME answer text. Cause:
   `preferred_size()` is read SYNCHRONOUSLY in `open_edit_card`, right after the
   widgets were made visible / `set_text` / min-height applied on the same frame,
   so GTK had not re-laid-out yet. The measurement was stale on some opens
   (over-counting an already-tall Answer → `answer_h` too big → overflow) and
   settled on others. This is the "sometimes pressing e does X, sometimes Y" bug.

5. **Pure `vexpand`, no forced/measured height** (the Answer vbox + scroller +
   the edit-card container all `vexpand(true)` + `valign(Fill)`; `open_edit_card`
   just collapsed the scroll to a strip and did nothing else). **OVERFLOWED on a
   long Answer.** The reasoning "the container is size-requested to `card_height`,
   so the box has exactly that to distribute" is WRONG: `set_size_request` is a
   **minimum**, and the Answer scroller's `max_content_height` was `4000`
   (effectively unbounded), so for a long Answer the scroller's *natural* height
   was huge → the `valign:Center` container grew PAST `card_height` and the Answer
   rendered all its content with NO scroll, spilling over the Instruction field +
   hint. `vexpand` does fill an *existing* allocation, but here the allocation
   itself ballooned because nothing capped the scroller's natural height. This is
   the exact bug the gloss overlay's `size_scroll` already documents and fixes by
   pinning `max_content_height`.

6. **Stacked sibling + collapse-the-viewport-to-a-strip** (the edit card appended
   as the LAST child of the journal vbox; `open_edit_card` shrank the page
   viewport to a 44px strip — just the question line + chevron — via
   `ask_host.open_for_natural_height`). This worked geometrically once the Answer
   was capped, BUT it could not satisfy requirement #1: a vbox sibling stacks
   BELOW the page viewport, so the 44px strip of question text always showed ABOVE
   the edit card — the card never covered the TOP of the Q&A. The user explicitly
   wanted full top-to-bottom overlap, so this was replaced by the full-region
   overlay below (and `open_for_natural_height`/`close_to_closed_height` were
   removed from `AskCardHost` — they had no other caller).

## The approach that holds: full-region overlay + a PINNED (capped) Answer

Two pieces, both required:

**1. The edit card is an overlay over the page region (covers all the text).**
In `JournalOverlay::new`, `scroll_overlay.add_overlay(edit_card.container())` with
`set_measure_overlay(false)` + `set_clip_overlay(true)` and the container's
`halign/valign = Fill`. When shown it fills the page-text region and paints over
it — top to bottom — satisfying requirement #1. Hidden during normal reading. It
is NOT in the size-bearing vbox, so it can never push the page viewport or inflate
the parent (this also follows the house "pickers/panels are `add_overlay`, never
in the widget chain" rule).

**2. The Answer field is PINNED to an exact height (a cap, not a min).**
`open_edit_card` computes
`answer_h = page_height() − edit_card.fixed_chrome_height()` and calls
`edit_card.pin_answer_height(answer_h)`, which sets the Answer scroller's
`min_content_height` AND `max_content_height` to that value (mirroring
`gloss_overlay::size_scroll`). Because `max_content_height` is a real CAP, the
scroller's natural height can never exceed it, so the edit card's contents fit the
region and a long Answer **scrolls** instead of spilling. `page_height()` is the
page viewport budget (`card − scroll_overlay margins − footer`) — the region the
overlay fills — so the card's contents exactly fit it.

Why this can't overflow OR race:

- The Answer scroller is **capped** at a `card_height`-derived value, so its
  natural height is bounded → the overlay's content can't inflate. (A cap NEVER
  forces growth, unlike a `min_content_height`.)
- The overlay is non-measuring + clipping, so it cannot change `scroll_overlay`'s
  size or paint outside the page region regardless of its content.
- `fixed_chrome_height()` measures ONLY the Answer-independent widgets (title,
  Question box, Instruction box, hint, container margins). Their heights do not
  depend on the just-set Answer text, so the synchronous `preferred_size()` read
  is **race-free** — unlike failed approach #4, which measured the whole container
  *including* the just-set tall Answer and so over-counted non-deterministically.

`build_field("Answer", 140, 4000, …)` still passes `max_content_height = 4000` at
construction, but `pin_answer_height` OVERRIDES it with the real cap on every
open. Do not rely on the 4000 to bound anything — it doesn't; the pin does.

## If the Answer overflows again (spills over Instruction/hint)

The cap is broken. Check, in order:

1. `open_edit_card` still calls `edit_card.pin_answer_height(answer_h)` with
   `answer_h = page_height() − fixed_chrome_height()` (NOT a constant, NOT the
   Answer's own measured height).
2. `pin_answer_height` still sets BOTH `min_content_height` AND
   `max_content_height` (the `max` is the cap that bounds the natural height; the
   `min` alone does not).
3. The edit card is still an `add_overlay` on `scroll_overlay` (non-measuring),
   NOT appended to the journal vbox. If it is in the vbox, its natural height
   feeds the `valign:Center` container and inflates it.
4. `fixed_chrome_height()` still sums ONLY the fixed widgets — if it ever folds in
   the Answer box, it self-references and the pin is wrong.

## If a gap appears below the Answer (region not filled)

The pin is too SHORT — `fixed_chrome_height()` is over-counting (e.g. a field's
`min_content_height` was raised) or `page_height()` shrank. The fix is to correct
those inputs, NOT to add `vexpand`/`min_content_height` back (that reintroduces the
overflow in failed approach #5). A small gap is harmless; an overflow is not, so
err toward a slightly-short pin.

## Verification

This is rendered geometry — `cargo test --bins` only proves it compiles. The
parent-overlay-overflow and the gap are BOTH visible only on screen, so verify by
opening the `e` card on a Q&A with a **long** Answer (the overflow only shows when
the Answer exceeds the region — e.g. King Lear's "art itself is nature" entry):

- The edit card must cover the WHOLE Q&A text — **including the top**: there is no
  question line / chevron peeking out above the card (requirement #1).
- The parent reading card must keep a clear teal margin on all four sides (its
  rounded top corners visible — overflow hides the top corners off-screen).
- The Answer field should reach near the bottom (minimal gap above "Ctrl+Enter
  submit"), and a long Answer must SCROLL inside its box — never spill over the
  "Rewrite instruction" field or the "Ctrl+Enter submit" hint.
- The Answer's line breaks should match the page text behind it.

## Related

- `gloss_overlay::size_scroll` — the SAME `valign:Center` overflow and the SAME
  `max_content_height`-pin fix, on the gloss overlay's visible scroll. The
  authoritative precedent for "a cap bounds natural height; a min does not".
- `clip-prevention.md` (the general "verify geometry on the real display, not
  from logs" rule).
- The chevron-after-last-block change attempted in the same session was a
  SEPARATE effort and was reverted (page marker stayed bottom-pinned) — see git
  history if revisiting.
