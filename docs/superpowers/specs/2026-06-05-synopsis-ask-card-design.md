# Synopsis "Ask" card + loading-overlay fix — design

Date: 2026-06-05

## Goal

Two related changes to the synopsis overlay (`gloss_overlay.rs`):

1. **Stacked ask card.** Replace the current floating "ASK ABOUT THIS SCENE"
   prompt (a centered `add_overlay` panel that covers the synopsis text) with a
   second card stacked *below* a shrunken synopsis card. Pressing `A` shrinks the
   synopsis card and reveals the ask card beneath it; both visible at once. `Tab`
   toggles focus between the two cards (synopsis = `j/k` scroll; ask = type).
   `Ctrl+Enter` submits, `Esc` is two-stage (ask → full synopsis → close overlay).

2. **Loading overlay fix.** The "Glossing…" loading state renders as bare
   centered text with no card and no scrim (the synopsis-amend `show_loading`
   path never sizes the container and hides the scrim). Fix it to a full card +
   scrim, consistent with the synopsis/gloss cards.

## Existing structure (as found)

- `GlossOverlay` (`src/ui/gloss_overlay.rs`) is a `gtk4::Overlay` wrapping a
  centered card `container` (`.gloss-overlay`) + a `scrim`. The synopsis text is
  a `TextView` (`gloss_view`) inside a `ScrolledWindow`.
- `show_synopsis(title, text, card_width, card_height)` sizes the card to the
  full reading area and renders the synopsis.
- `A` in `SynopsisOverlay` mode → `synopsis::show_amend_prompt`, which builds a
  floating `add_overlay` box (centered, width 600) over the synopsis and switches
  to `SynopsisPrompt` mode; `handle_synopsis_prompt_key` reads `Ctrl+Return` and
  calls `amend_synopsis` (the async Claude call — unchanged backend).
- `show_loading()` → `show_loading_message("Glossing...")`: sets the title label
  centered+vexpand, hides the scroll overlay, **hides the scrim**, and never
  sizes the container → the card collapses to the label's size = bare text.

## Part A — stacked ask card

### New widgets (fields on `GlossOverlay`)

- `ask_container: gtk4::Box` — second card (`.gloss-overlay` + `.ask-card`),
  halign/valign handled by stacking (see layout). Holds:
  - `ask_title: Label` ("ASK ABOUT THIS SCENE", `.gloss-header`-ish).
  - `ask_input: gtk4::TextView` — **editable**, wrap word, `.gloss-text`.
  - `ask_hint: Label` — "Tab switch · Ctrl+Enter submit · Esc cancel".
- `ask_focus: Cell<AskFocus>` where `enum AskFocus { Synopsis, Ask }`.

### Layout — stacking both cards as a column

The synopsis `container` is centered in the overlay. To stack a second card
below it without disturbing the existing centered single-card layout, wrap the
synopsis `container` and `ask_container` in a vertical `stack_box`
(halign/valign Center) that is the thing added as the overlay child layer.

Simpler, lower-risk alternative chosen: keep `container` as-is and append
`ask_container` **inside** `container` as its last child (after the footer),
hidden by default. When `A` is pressed:

- Reduce the scroll viewport: the synopsis `gloss_scroll_overlay` already
  `vexpand`s; appending a visible `ask_container` below the footer naturally
  shrinks the scroll area because the card height is fixed (`card_height`). The
  synopsis text becomes scrollable in the smaller remaining space. This is
  exactly "synopsis shrinks to make room", with no second top-level widget.

Decision: **append `ask_container` inside `container`**, below the footer box,
separated by a thin rule. `card_height` stays fixed, so revealing the ask card
shrinks the scroll viewport. This avoids a second centered overlay and keeps the
bottom-clip math working on the same scrolled window.

### Focus + active highlight

- `ask_focus` starts `Ask` when the card opens (cursor in the input).
- `Tab` (handled in `handle_synopsis_overlay_key`, mode stays `SynopsisOverlay`)
  flips `ask_focus`; a method `set_ask_focus(focus)` toggles a `.card-focused`
  CSS class on whichever sub-region is active and calls `ask_input.grab_focus()`
  or blurs it. Visible treatment: focused region gets a left accent bar / bg
  lift (new `.card-focused` class).
- `j/k` scroll the synopsis only when `ask_focus == Synopsis`.

### Mode handling

- Merge `SynopsisPrompt` into `SynopsisOverlay`. When the ask card is open and
  focused, typed characters must reach `ask_input`: the global key controller
  returns `false` for printable keys so GTK routes them to the focused editable
  TextView, mirroring how `handle_search_key` returns `false` to feed the Entry.
- `handle_synopsis_overlay_key` gains:
  - `Tab`/`ISO_Left_Tab`: toggle focus (consume).
  - `Ctrl+Return`: if ask open, read `ask_input`, close ask card, call
    `amend_synopsis`. (consume)
  - `Escape`: if ask open → close ask card (back to full synopsis); else close
    overlay. (consume)
  - `A`: open ask card (no-op if already open).
  - `j`/`k`: only scroll when `ask_focus == Synopsis`; when `Ask`, return `false`
    so the letters type into the input.
  - When ask focused, printable keys not handled above → return `false`.
- `SynopsisPrompt` mode + `handle_synopsis_prompt_key` + the floating
  `show_amend_prompt`/`close_amend_prompt` are removed; the methods on
  `GlossOverlay` (`open_ask_card`/`close_ask_card`/`take_ask_text`) replace them.

### Backend reuse

`amend_synopsis(state, question)` is unchanged. It calls `show_loading()` then,
on result, `show_synopsis(...)` which re-renders the full synopsis card (ask card
hidden during loading). `synopsis_amend_scene` is still set when the ask card
opens (from `synopsis_overlay_scene`).

## Part B — loading overlay fix

`show_loading()` / `show_loading_message(msg)` gain `card_width, card_height`
params. Body additionally:

- `container.set_width_request(card_width); set_height_request(card_height);`
- `scrim.set_visible(true);`
- ensure `ask_container.set_visible(false)` so a stale ask card can't show.

Callers (`amend_synopsis` and any gloss/echo loading callers) pass
`content_hbox.width()/height()` (already in scope where loading is shown). If a
caller lacks geometry, fall back to the last card size already on the container.

## Files touched

- `src/ui/gloss_overlay.rs` — new ask widgets + methods, loading fix.
- `src/input/keymap.rs` — `handle_synopsis_overlay_key` gains Tab/Ctrl+Return/
  Esc/typing routing; remove `SynopsisPrompt` arm + `handle_synopsis_prompt_key`.
- `src/input/actions/synopsis.rs` — replace `show_amend_prompt`/`close_amend_prompt`
  with calls into the new overlay methods; keep `amend_synopsis`/`undo_amend`.
- `src/app.rs` — `show_loading` call sites pass geometry; possibly drop
  `SynopsisPrompt` InputMode variant + the now-unused `gloss_prompt_*` weakrefs
  if nothing else uses them (gloss add/edit still use them — keep).
- `src/theme.rs` — `.ask-card`, `.card-focused` CSS; reuse gloss palette vars.

## Non-goals

- No change to the Claude amend prompt/backend.
- No change to the gloss/echo overlays beyond the shared loading fix.
- Gloss add/edit prompts (`GlossPrompt`) keep their existing floating dialog.
