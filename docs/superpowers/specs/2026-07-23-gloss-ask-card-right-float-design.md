# Gloss/synopsis ask card → right-floated 2-column layout

**Date:** 2026-07-23 (US Central)
**Status:** Approved design (brainstormed with user; decisions inline)
**Scope:** The gloss/synopsis overlay's ask card only
(`src/ui/gloss_overlay.rs` + `AskCardHost` in `src/ui/ask_card.rs`). The
journal overlay's ask card is out of scope and unchanged.

## Problem

When the ask card opens from within the **gloss overlay** — any of its five
prompts: reader-gloss *Ask a question about the passage* (`gloss.rs:753`),
*Edit gloss* rewrite instruction (`GlossPromptMode::Edit`), *Fix IPA*
(`GlossPromptMode::FixIpa`), *Inner monologue* paste, and the synopsis *Edit
this scene* ask (`synopsis.rs:43`) — the current layout is single-column:
`AskCardHost::open` shrinks the gloss scroll to the top of the centered card
and reveals the ask input **stacked below it** (`gloss_overlay.rs:747-748`;
the occlusion-avoidance shrink in `ask_card.rs:559-583`).

The user wants the gloss commentary to stay **full-height on the left** and
the ask input to appear as a **right column**, with the two centered on
screen as a pair — the same visual relationship the Tab chat panel already
has with the reading card.

## Decision summary (user-approved)

- **Layout:** the ask card **floats as a separate right-side overlay**; the
  gloss card reserves matching room and the pair re-centers together. NOT a
  split of the gloss container.
- **Scope:** **all five** gloss/synopsis prompts float right. The
  stacked-below scroll-shrink path is **retired for this overlay entirely** —
  one code path, no per-prompt mode flag.
- **Positioning:** the gloss card shifts left so gloss+ask are centered
  together (like `apply_card_sizing(chat_open=true)` centers the chat panel +
  reading card as a pair).
- **Trigger/binds:** unchanged. The prompts open via their existing keys;
  only the ask card's *layout* changes. No new binds, no `keymap.json` edit,
  no Ctrl+/ overlay change.
- **State:** float positioning is derived from the current card geometry,
  session-only. No config persistence.

## Why float, not split (the approach rationale)

The gloss overlay's `container` is a **fixed-width, centered, size-bearing**
vertical box: every `show_*` path calls `container.set_width_request(card_width)`
(`gloss_overlay.rs:1527, 1649, 1733, 1849, 3085`), and the `AskCardHost`
scroll-shrink, `BottomClipGuard`, and paginated-height measurement all assume
a single fixed-width column. Splitting that container horizontally on ask-open
would fight the sizing pass across ~7 `show_*` sites and risk the gloss
overlay's clip/pagination math.

Floating instead:

- **Obeys the house rule** `feedback_picker_overlay_not_chain`: new panels are
  `add_overlay` layers, never links in the size-bearing chain.
- **Reuses proven machinery.** The Tab chat panel already floats an
  `add_overlay` panel `halign=Start valign=Center` with a `margin_end`
  reservation on the reading card (`apply_card_sizing` `chat_open` branch,
  `layout.rs:404-414`; panel added at `app/mod.rs:1976`). The
  `2026-07-10-chat-panel-float-2col-design.md` spec already worked out the
  opaque-background float pattern.
- **Leaves the gloss card untouched.** The gloss scroll keeps full width and
  full height, so `BottomClipGuard` and the paginated-height math on the gloss
  side never see a changed viewport — the whole class of clip regressions the
  shrink path guards against simply does not arise.

The only cost is that the ask card, no longer physically inside the gloss
container, needs its own opaque bordered background — mirroring
`.chat-panel-float` (`theme.rs:1429`), which floats over a transparent-blend
surface for exactly the same reason.

## Architecture

### Lift the ask card out of the gloss container

Today the `AskCard` is `container.append(ask.container())` inside the gloss
overlay's vertical box (`gloss_overlay.rs:556`), and its lifecycle runs through
`AskCardHost` (which shrinks `gloss_scrolled`). In the new model:

- The ask card is reparented to the gloss overlay's own `Overlay` as a
  **separate `add_overlay` child**, `halign=End`, height matched to the gloss
  card (`valign=Fill` within the card's vertical span, or an explicit
  `height_request` equal to the gloss card height — the plan picks the exact
  call, matching the chat panel's own choice). It is a sibling of the gloss
  card, never in the `container` chain.
- The `AskCardHost` scroll-shrink (`set_input_fill_fraction(0.75)` at
  `gloss_overlay.rs:575`; the `open`-shrinks-`gloss_scrolled` behavior in
  `ask_card.rs:559-583`) is **not used** for this overlay. `open`/`close`
  become: reveal/hide the floated ask overlay + toggle the gloss card's
  `margin_end` reservation + recompute the gloss clip against the (unchanged)
  full-height scroll.

The plan decides whether to (a) keep `AskCardHost` but add a "float" mode that
skips the shrink and drives the margin reservation instead, or (b) give the
gloss overlay a thin float-host of its own and leave `AskCardHost` to the
journal overlay. Either keeps the journal overlay's `AskCardHost` behavior
byte-for-byte unchanged. **Recommendation: (a)** — one host type, a
`float: bool` (or placement) that branches `open`/`close`/`size`; the journal
overlay never sets it, so its path is untouched. Confirm in the plan.

### Reserve room + re-center the pair

On ask-open, the gloss `container` gets a `margin_end` = `ask_width + seam`,
and the gloss+ask pair centers together — the same shape as
`apply_card_sizing(chat_open=true)`. On close, the `margin_end` is removed and
the gloss card returns to full width and center. This is the exact inverse of
open (mirrors the chat panel's `regate` / `chat_open=false` restore).

Because the gloss overlay owns its own centered card (it is a full-screen
overlay with a centered `container`), the reservation + re-center is local to
the gloss overlay's layout, not `apply_card_sizing` (which sizes the main
reading card). The plan implements the reservation at the gloss overlay's card
level, reusing the same "reserve width, center the pair" arithmetic.

### Geometry

- **Ask card width:** the width the stacked ask card uses today (~the gloss
  column width the `AskCard` was built with, `text_margins`-inset). Keeps the
  input roomy for typing.
- **Ask card height:** full gloss-card height — the same vertical span as the
  gloss commentary, so the two columns are top- and bottom-aligned.
- **Gloss `margin_end`:** `ask_width + seam` (seam matches the chat panel's
  card↔panel seam so the two designs read consistently).
- **Centering:** gloss+ask centered together on screen.

### Visuals — new `.gloss-ask-float` CSS class

The current gloss ask card is transparent-on-`container`; once floated it sits
over the `.gloss-scrim` / whatever is behind and would blend. Add a
`.gloss-ask-float` class in `generate_css` (`theme.rs`), modeled on
`.chat-panel-float` (`theme.rs:1429`): **opaque root/card-color background +
hairline border**, so the floated ask card reads as a distinct panel. The
class is applied to the ask card's container in float mode only.

### Focus & lifecycle

- On open, focus lands in the ask input in INSERT — unchanged (the existing
  `feed_ask_vim_key(Char('i'))` auto-enter at `gloss.rs`/`journal.rs` open
  sites still applies to the gloss/synopsis prompts that use it).
- `ask_is_open()`, `take_ask_text()`, `feed_ask_vim_key()`,
  `paste_ask_text()`, and `close_ask_card()` on `GlossOverlay` keep their
  signatures — callers in `keymap.rs:2305,2866`, `gloss.rs`, and
  `synopsis.rs` are unchanged. Only the `open`/`close` layout internals move
  from shrink to float.

## Files

- `src/ui/gloss_overlay.rs` — reparent the ask card to the overlay's own
  `Overlay` as an `add_overlay` sibling; drive the `margin_end` reservation +
  pair re-center on open/close; drop `set_input_fill_fraction` for this
  overlay; apply `.gloss-ask-float`.
- `src/ui/ask_card.rs` — `AskCardHost` gains a float mode (recommendation (a))
  that skips the scroll-shrink and reveals/hides without touching
  `gloss_scrolled`'s height; the gloss clip recompute still fires against the
  full-height scroll. Journal path unchanged.
- `src/theme.rs` — new `.gloss-ask-float` rule in `generate_css`, modeled on
  `.chat-panel-float`.
- No `keymap_config.rs`, no `keymap.json`, no Ctrl+/ overlay changes (no bind
  moved).

## Testing

- **Headless cage e2e** (`test-headless-navigation` / `verify-overlay-ui`
  harness): open a reader-glossed passage, open the gloss overlay, open the
  ask card; screenshot must show the gloss commentary at **full height on the
  left** and the ask input as a **distinct bordered panel on the right**, the
  pair centered — NOT the gloss shrunk to the top with the ask stacked below.
  Repeat for the synopsis *Edit this scene* ask (`h` synopsis → edit) to
  confirm the shared path floats too. Escape → gloss card returns to full
  width, centered.
- **UI review protocol:** open every `target/ui/` PNG and report on-screen
  what is seen (gloss left / ask right / centered pair; no clipping at the
  gloss card's bottom edge, which the untouched full-height scroll should
  preserve).
- **`cargo test --bins`:** any pure geometry helper added for the reservation
  width / re-center offset (float vs stacked branch selection) is unit-tested.
- **`cargo test --test line_clipping -- --ignored`:** the gloss card's clip
  invariant must still pass — the full-height gloss scroll should make this a
  no-regression check.
- Cage is software rendering; hand the user the exact e2e command for a final
  eyeball on the real GL renderer (per the house testing rule).

## Out of scope

- The **journal overlay** ask card (`begin_passage_ask` / `begin_ask`,
  `journal.rs`) — keeps its stacked-below layout. Only the gloss/synopsis
  overlay changes.
- Persisting the float to config (it is session-derived, always float).
- Any change to what the ask card **sends** or the Q&A / gloss / rewrite / IPA
  pipelines — purely the ask card's on-screen layout within the gloss overlay.
- A left/right side toggle (the ask card is always on the right; no `Ctrl+l`
  flip like the chat panel — the gloss overlay has no cursor-column notion to
  choose a side from).
- Reader-level `Ctrl+a` and the visual-mode ask paths — the trigger is
  unchanged; this spec is layout only.
