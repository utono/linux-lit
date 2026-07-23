# Float the journal Q&A ask card (2-col) — enable Ctrl+Tab in the journal overlay

## Problem

The `2026-07-23-ask-card-focus-toggle` feature added a `Ctrl+Tab` toggle
that swaps input focus between the ask card and the card underneath it, but
only when the ask card is open in **2-column float** layout
(`AskCard::is_float()` = `float_width > 0`). The plumbing was wired into
BOTH `handle_gloss_key` and `handle_journal_key`.

It works in the gloss overlay because the gloss overlay floats its ask card
(`gloss_overlay.rs` calls `ask_host.enable_float(...)` unconditionally). It
is a **dead no-op in the journal overlay** because the journal overlay never
calls `enable_float` — its ask card opens STACKED (fills 3/4 of the overlay
height via `set_input_fill_fraction(0.75)`, shrinking the reading page into
the top quarter). So `is_ask_float()` is always false there and Ctrl+Tab
does nothing.

The original user request named "asking for a journal q&a" in 2-col, so this
follow-up makes the journal ask card float 2-col, mirroring the gloss
overlay exactly, which activates the already-shipped Ctrl+Tab toggle in the
journal overlay with no key-handler changes.

## Goal

Float the journal overlay's ask card to the right as a 2-column panel,
identical in wiring to the gloss overlay's float, so:
- `is_ask_float()` returns true for journal asks → `Ctrl+Tab` in
  `handle_journal_key` becomes active.
- The dim cue, focus resets, and `hide()` dim-clear (all already present on
  `JournalOverlay`) apply to the journal 2-col pair automatically.

## Scope

**Journal overlay only.** The synopsis ask card also floats (it shares the
gloss overlay's `ask_host`) but its handler `handle_synopsis_overlay_key`
lacks the toggle; that is explicitly OUT of scope here — a possible sibling
follow-up, not part of this change.

## Accepted visual change

The journal Q&A ask card changes from a tall stacked box (reading page
shrinks above it) to a right-side floated panel beside a full-height journal
card — for ALL journal asks, not only when Ctrl+Tab is used. This is
inherent to floating it and matches "mirror the gloss float exactly." The
user has accepted this general visual change.

## Design — mirror the gloss float wiring

The `AskCardHost` already supports both modes: `enable_float(width,
reserve)` sets `float_width` and stores a reservation closure that
`open`/`close` fire; `set_input_fill_fraction` is the stacked-mode height
behavior. The journal overlay currently opts into stacked; this change opts
it into float instead.

### 1. `src/ui/journal_overlay.rs` — `new()`

Reference implementation: `gloss_overlay.rs` lines ~583–621.

- **Remove** `ask_host.set_input_fill_fraction(0.75)` (the stacked-height
  behavior; incompatible with float).
- **Before** `AskCardHost::new(ask, ...)` consumes `ask`, capture the ask
  container for the reservation closure:
  `let ask_container_for_reserve = ask.container().clone();`
- After building `ask_host`, add the float wiring, copied from gloss with
  `container` = the journal overlay's main card box (field `container`):

  ```rust
  let float_w = (column_width as i32 * 5 / 8).max(360);
  {
      let container_for_reserve = container.clone();
      let reserve = Rc::new(move |px: i32| {
          // px = ask_width + seam on open, 0 on close.
          container_for_reserve.set_margin_end(px);
          if px > 0 {
              let journal_w = container_for_reserve
                  .width()
                  .max(container_for_reserve.width_request());
              let seam = px - float_w;
              ask_container_for_reserve.set_margin_start(journal_w + seam);
              let h = container_for_reserve
                  .height()
                  .max(container_for_reserve.height_request());
              ask_container_for_reserve.set_height_request(h.max(200));
          } else {
              ask_container_for_reserve.set_margin_start(0);
              ask_container_for_reserve.set_height_request(-1);
          }
      }) as Rc<dyn Fn(i32)>;
      ask_host.enable_float(float_w, reserve);
  }
  ```

- **Centering prerequisite:** the reservation math centers the pair only if
  the journal `container` has `halign = Center` and a fixed
  `width_request(column_width)` (the gloss container sets both). VERIFY the
  journal container's current alignment/width setup in `new()`; if it lacks
  `halign(Center)` and/or `width_request(column_width as i32)`, add them so
  the two mirrored allocation boxes yield equal left/right gutters. Do not
  otherwise restyle the journal card.

### 2. `src/ui/journal_overlay.rs` — `attach()`

Reference: `gloss_overlay.rs` `attach()` (~1466). The journal `attach()`
currently only calls `attach_overlay_panel(...)`. Add the ask container as a
right-anchored `add_overlay` sibling so it paints above the journal card:

  ```rust
  let ask_container = self.ask_host.card().container();
  self.overlay.add_overlay(ask_container);
  self.overlay.set_measure_overlay(ask_container, false);
  self.overlay.set_clip_overlay(ask_container, true);
  ```

### 3. `src/ui/journal_keybinds_overlay.rs` — legend

Add one row documenting the now-active bind (the last feature deliberately
withheld it because the journal toggle was dead; it now works). Match the
file's `(key, description)` tuple + group style. Suggested row, placed near
the passage-ask / most-used rows:

  `("Ctrl+Tab", "toggle focus: ask card ↔ journal card (2-col)")`

### What needs NO change (already present from the prior feature)

- `handle_journal_key` Ctrl+Tab flip + Escape-return + conditional
  `ask_vim_intercept` — already handles the journal overlay; it was dormant
  only because `is_ask_float()` returned false. It activates automatically.
- `JournalOverlay::is_ask_float`, `set_ask_focus_dim`, `clear_focus_dim`,
  and the `AppState.ask_card_focus` open/close resets — already exist and
  apply to the journal path.
- `AppState.ask_card_focus` open resets: `begin_ask` and `begin_passage_ask`
  already set it `true` on open, so the journal ask starts ask-focused.
- The ask-close path `close_prompt` already resets focus AND calls
  `clear_focus_dim`. NOTE: the OTHER journal close paths do NOT (see Risk 2)
  — closing this gap is a required part of this change, not a no-op.

## Risks / must-verify in the plan

1. **Post-submit answer renders full-width.** After a journal ask submits,
   the ask card closes and the ANSWER renders in the same journal overlay
   (the reading card). Closing must restore the journal card to full width:
   the `reserve(0)` path resets `margin_end` to 0. Headless-verify the
   answer renders full-width and un-clipped after submit — this is the
   primary regression risk of dropping the stacked layout.
2. **Stale-dim leak parity with gloss (CONFIRMED — a required fix, not just
   a check).** The journal overlay has the SAME leak the gloss overlay had:
   `JournalOverlay::hide()` (`journal_overlay.rs:986`) does NOT call
   `clear_focus_dim`; only the ask-close path `close_prompt`
   (`journal.rs:2224`) does. Two close paths hide the journal overlay WITHOUT
   clearing the dim — `cycle_from_journal` (`overlay_cycle.rs:56`) and
   `toggle_overlay` / Ctrl+j close (`journal.rs:1152`). Today this is latent
   because the dim is never applied (journal never floats); this change makes
   it live. **The plan MUST add `self.clear_focus_dim();` to
   `JournalOverlay::hide()`** (mirroring the gloss `hide()` fix), so a
   Ctrl+Tab-dimmed journal card cannot survive a `\`-cycle or Ctrl+j close and
   reopen dimmed. For the `ask_card_focus` bool, also reset it to `true` in
   `cycle_from_journal` (inside its existing `state.borrow_mut()` scope) —
   the ask-open paths already reset it, but this keeps the state coherent on
   the non-ask close funnel, matching the gloss branch's `cycle_from_gloss`
   reset.
3. **Height/measurement.** The journal ask previously fed the layout via
   `set_input_fill_fraction`; as a non-measured float overlay
   (`set_measure_overlay(false)`) its height comes from the reservation
   closure's `set_height_request`. Verify the ask input is tall enough to
   type in and does not clip (pixel-check the float panel).

## Testing

- **Headless (cage) drive:** open a journal Q&A ask (`Ctrl+a` in the journal
  overlay); confirm it now floats 2-col (right panel beside a full-height
  journal card, centered pair with equal gutters), NOT stacked. Type a
  partial question; `Ctrl+Tab` → confirm focus moves to the left journal
  card (nav keys route there, draft preserved) and the unfocused card dims
  to 0.55; `Ctrl+Tab` back → dim flips; `Escape` from the left card →
  returns to the ask card without closing. Submit a question and confirm the
  answer renders full-width in the journal overlay (Risk 1).
- `cargo test --bins` stays green (the shared `gloss_reservation_width`
  helper's unit test is unaffected; no new pure logic is introduced).
- `cargo clippy --bin linux-lit` — no new errors.

## Acceptance

- The journal Q&A ask card floats 2-col identically to the gloss ask.
- `Ctrl+Tab` toggles focus in the journal overlay; the unfocused card dims
  to 0.55 (correct direction each way); the draft is preserved; Escape from
  the left card returns to the ask without closing.
- After submit, the answer renders full-width and un-clipped in the journal
  overlay.
- No stale dim survives any journal overlay close path.
- The journal Ctrl+/ legend documents the bind.
- Gloss/synopsis behavior is unchanged.
