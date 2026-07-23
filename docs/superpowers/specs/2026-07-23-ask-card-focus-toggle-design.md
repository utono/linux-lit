# Ctrl+Tab focus-toggle between the ask card and the gloss/journal card (2-col)

## Problem

In 2-column float layout, the shared ask card (used for a gloss
rewrite/passage-ask and for a journal Q&A ask) floats to the right of the
gloss/journal card. While the ask card is open it is a modal vim editor:
`ask_vim_intercept` (`src/input/keymap.rs`) consumes **every** key, so the
left card receives nothing. A reader composing a question cannot scroll or
navigate the gloss/journal content on the left to reference it — they must
close the ask card (losing draft context) to look, then reopen and retype.

## Goal

`Ctrl+Tab` toggles input focus between the ask card and the left
(gloss/journal) card while the ask card is open in 2-col float layout.
Focus on the left card gives full read-navigation; `Ctrl+Tab` returns to
typing with the ask draft preserved. A dim cue shows which card is live.

## Focus model — two states

One new field on the shared `AppState`:

```rust
pub ask_card_focus: bool,   // true = ask card has input; false = left card
```

A single field is sufficient because at most one overlay (gloss OR journal)
is open at a time. It is only meaningful while the ask card is **open in
float mode** (`AskCardHost::float_width > 0`; expose as `is_ask_float()`).

- **Ask-card-focused (`true`, the default on open):** unchanged from today.
  `ask_vim_intercept` runs and the vim editor drives; all keys type/command
  the ask card.
- **Left-card-focused (`false`):** the overlay's own key handler
  (`handle_gloss_key` / `handle_journal_key`) runs for full read-nav — `j`/`k`
  block-nav, `gg`/`G`, scrolling, `Ctrl+n`/`p` page-through — and it is
  **fully modal**: every key goes to the left card. The ask card stays
  visible with its typed text intact but ignores input.

On open (both gloss and journal ask paths), `ask_card_focus` initializes to
`true`. On close/submit it is reset to `true` so the next open starts on the
ask card.

## The toggle key — `Ctrl+Tab`

`Ctrl+Tab` and `Ctrl+ISO_Left_Tab` flip `ask_card_focus`, but **only when
the ask card is open AND in float mode**. Placement in each of
`handle_gloss_key` and `handle_journal_key`:

1. **Before** the `ask_vim_intercept` call, add a guard:
   `if ask_open && is_ask_float() && (Ctrl+Tab | Ctrl+ISO_Left_Tab)` → flip
   `ask_card_focus`, update the dim cue, `return true` (consumed). This must
   precede the intercept because `ask_vim_intercept` currently maps Ctrl+Tab
   to a vim `Tab` and would swallow it.
2. When `ask_card_focus == false`, **skip `ask_vim_intercept` entirely** so
   keys fall through to the overlay read-nav handler below.

Outside float mode (1-col stacked), `Ctrl+Tab` remains the current consumed
no-op — there is no separate card to focus.

## Escape semantics

- **Left-card-focused + `Escape`** → set `ask_card_focus = true` (return to
  the ask card), update the dim cue, consume. Does NOT close the overlay.
  One Escape means "back to typing."
- **Ask-card-focused + `Escape`** → unchanged (single = vim Normal mode;
  quick double = close the prompt).

## Focus cue — dim the unfocused card (0.55)

Add a CSS class `.card-unfocused` in `src/theme.rs` (`generate_css`) that
sets `opacity: 0.55` on the card container. On every `ask_card_focus` flip,
apply the class to the now-unfocused card and remove it from the focused
one; on ask-card close/submit, clear it from both. Overlays expose a small
helper (e.g. `set_ask_focus_dim(ask_focused: bool)`) that toggles the class
on the gloss/journal card container vs the ask float container.

## Scope

- **Both** the gloss overlay and the journal overlay.
- **Only** when the ask card is open in 2-col float (`is_ask_float()` true).
- 1-col stacked ask layout is unchanged (Ctrl+Tab no-op).

## Files touched

- `src/app/mod.rs` — add `ask_card_focus: bool` to `AppState` + its init.
- `src/input/keymap.rs` — in `handle_gloss_key` and `handle_journal_key`:
  the Ctrl+Tab flip guard (before the intercept), the conditional skip of
  `ask_vim_intercept` when unfocused, and the Escape-returns-to-ask branch
  in the left-card-focused path.
- `src/ui/gloss_overlay.rs`, `src/ui/journal_overlay.rs` — `is_ask_float()`
  passthrough and `set_ask_focus_dim(bool)`; reset focus + clear dim on
  close/submit.
- `src/ui/ask_card.rs` — expose float-mode query (`float_width > 0`) if not
  already public.
- `src/theme.rs` — `.card-unfocused { opacity: 0.55 }`.
- **Keybind legends** (lockstep, required): the gloss and journal overlay
  Ctrl+/ legends (`src/ui/gloss_keybinds_overlay.rs`,
  `src/ui/journal_keybinds_overlay.rs`) get a `Ctrl+Tab → toggle ask/card
  focus` entry, replacing the current "Ctrl+Tab consumed no-op" note.
  (Reader-side keymap_config and the Ctrl+/ main-card overlay are NOT
  touched — this bind lives only in the overlay modal handlers.)

## Non-goals

- No copy-from-left-card-into-question flow (paste already exists via
  Ctrl+v). Left-card focus is read-nav only.
- No change to 1-col stacked ask layout.
- No new focus behavior in the chat panel (its Tab/Ctrl+Tab model is
  separate and unchanged).

## Testing

- **Headless (cage) drive:** open a gloss rewrite in 2-col; type a partial
  question; `Ctrl+Tab`; press `j`/`G` and confirm the LEFT card scrolls/block-
  navigates while `-- INSERT --`/draft text is preserved on the right; the
  unfocused card is visibly dimmed; `Ctrl+Tab` back and confirm typing
  resumes; `Escape` from the left card returns focus to the ask card without
  closing the overlay. Repeat for the journal Q&A ask.
- **Unit test:** if the flip/Escape logic is factored into a pure state
  helper, assert the `ask_card_focus` transitions (open→true, Ctrl+Tab→false,
  Ctrl+Tab→true, Escape-when-false→true, close→true).

## Acceptance

- In 2-col gloss and journal ask layouts, `Ctrl+Tab` toggles focus; the
  unfocused card dims to 0.55; ask draft survives the round trip.
- Left-card focus supports full overlay read-nav and is fully modal.
- Escape from the left card returns to the ask card (does not close).
- 1-col stacked and the chat panel are unaffected.
- Both overlay Ctrl+/ legends document the new bind.
