# Vocab popup suspend across overlays + journal ask pending question

Date: 2026-07-29

Two independent reader-surface defects, specified together because both were
reported from the same session and both concern what the user sees while an
overlay is opening.

## Item 1 — Suspend the vocab popup while a gloss/journal overlay is up

### Observed

The vocab popup (definition card, right strip / column float) stays painted
over the scrim when a reader bind opens a gloss overlay or a journal entry.
The screenshot shows `remonstrate` and its definition bleeding through beside
an open ask card.

### Wanted

Whenever a reader bind opens a gloss overlay or a journal entry, the vocab
popup — if it was visible in the reader — hides for the duration, and comes
back when the user returns to the reader.

### Why the obvious fix is wrong

There is no shared "enter overlay" helper: ~50 sites assign
`state.input_mode = InputMode::{Gloss,Journal,Synopsis}Overlay` directly
(gloss.rs, journal.rs, synopsis.rs, corpus_search.rs, visual.rs, keymap.rs,
settings.rs). Patching each is unmaintainable and will silently rot as new
open paths land.

There is also no existing suspend/restore concept for the popup. Every site
that wants it gone calls `close_vocab_popup()` (a permanent close that also
clears the chat-panel carve), and one site (`pickers.rs:909`) calls raw
`.popup.hide()`, bypassing that cleanup.

### Approach — transition detection at the single key funnel

`crate::input::keymap::handle_key` is the sole entry point for all key input
(`src/app/mod.rs:3151` is its only non-test caller). Sample `input_mode`
before dispatch and compare after:

- Reader → overlay mode, popup currently visible: hide the widget and set
  `vocab_popup.suspended = true`. Do NOT go through `close_vocab_popup` —
  the popup's `data`/`index`/`view` must survive so the restore is exact,
  and the chat-carve teardown must not run.
- overlay mode → Reader with `suspended` set: re-show and re-place (the
  cursor may have crossed the column split while the overlay was up, so
  restore runs `position_vocab_popup` before `show_vocab_popup`), clear the
  flag.

The flag lives on `VocabPopupState` beside the existing `auto`/`chat_inline`
flags.

### Scope of "overlay mode"

Only the surfaces the request names: `GlossOverlay`, `JournalOverlay`, and
their visual/edit satellites (`GlossVisual`, `GlossEdit`, `JournalVisual`,
`JournalEdit`), plus `SynopsisOverlay`/`SynopsisVisual` (it renders through
the gloss overlay widget and reads as the same surface to the user).

Explicitly NOT suspended:

- Overlay-scoped popups (`VocabAnchor::Corner` / `ChatPanel`). Those are
  opened BY an overlay and are the point of the interaction. Suspend applies
  only to a popup that was anchored in the reader — track this by only
  arming the flag on a Reader→overlay transition, which by construction can
  only catch a reader-anchored popup.
- Pickers and the chat layout — out of scope for this request.

### Interaction with existing permanent closes

`vocab_journal.rs:185,284` already call `close_vocab_popup` before opening a
journal entry for a vocab Q&A. That is a deliberate permanent close (the
answer supersedes the definition card) and stays as-is: the popup is already
hidden and `data` is untouched by close, but the suspend flag must not arm
for an already-hidden popup, so the transition check reads visibility, not
intent.

### Acceptance

- Reader with popup up → `g` (gloss) → popup gone; Escape → popup back with
  the same word and view.
- Popup up → journal entry opens → gone; return to reader → back.
- Popup NOT up before the overlay → nothing appears on return.
- A corner/chat-anchored popup opened from inside an overlay is unaffected.

## Item 2 — Journal ask must show the question while waiting

### Observed

Submitting a vocab Q&A (`R` on the vocab popup) leaves a collapsed journal
card on screen — running head only ("BH-Barrett · Chapter 4"), empty body —
for the whole round trip. The debug log for the reported session shows the
wait is not brief: improve-question returns at `483317ms`, the answer at
`507610ms`, with the overlay only populated at `507653ms`. Roughly 24
seconds of blank card.

### Root cause

The passage-ask path already does this correctly:
`submit_passage_question` (`journal.rs:2543`) calls
`journal_overlay.show_loading(text, "Refining question…")` and `ask_claude`
(`journal.rs:2892`) re-shows it as `show_loading(question, "Answering…")`.
`show_loading` (`journal_overlay.rs:1107`) renders the held question via
`prefix_question` plus an animated indicator.

The VOCAB Q&A path (`vocab_journal.rs:vocab_journal_ask`) never calls
`show_loading`. It only raises a held bottom-strip toast
(`show_chapter_toast_hold`, "Journal Q&A - {word}") and calls
`improve_question` → `run_claude_request` directly. The blank card is a
journal overlay left showing its running head with no body render.

### Wanted

The vocab Q&A path shows the submitted question during the wait, matching
the passage-ask path: question text in the body, an indicator, no navigable
blocks, footer hidden.

### Approach

Have `vocab_journal_ask` follow the same two-stage loading render the
passage path uses:

1. Before `improve_question`: set the running head and
   `show_loading(&question, "Refining question…")` with the seed question
   (`vocab_question(&word)`), so the body is never empty.
2. In `improve_question`'s `on_done`, before the answer request:
   re-`show_loading(&improved, "Answering…")` so the user sees the sharpened
   phrasing that is actually being answered.

The existing held toast stays (it is the cross-surface progress signal and
already covers the case where the reply lands while the user has navigated
elsewhere).

The overlay must be visible for the loading card to be seen — the vocab path
currently only enters `JournalOverlay` mode when the answer arrives
(`open_overlay_at_entry`). The loading render therefore also needs the
overlay shown and the mode set at submit time, with the existing
answer-arrival branch (`s.input_mode == Reader` guard, `vocab_journal.rs:283`)
adjusted so it does not skip the reveal now that the mode is already
`JournalOverlay`. Failure paths (`run_claude_request`'s error callback and
improve-question failure) must leave the overlay in a sane state rather than
a spinner that never stops.

### Acceptance

- `R` on a vocab popup word → journal card immediately shows the seed
  question and an indicator, not an empty body.
- After improve-question returns, the body updates to the improved question,
  still with an indicator.
- Answer arrives → normal Q&A page renders, footer restored, indicator
  stopped.
- API error → no permanent spinner; the user is returned to a sane surface
  with the failure surfaced.
- The stored-answer fast path (`find_vocab_page` hit) is unchanged — it
  opens the entry directly with no loading card.

## Out of scope

- The `pickers.rs:909` raw `.popup.hide()` inconsistency (pre-existing; note
  only).
- Any keybind changes. Neither item moves or adds a bind, so no overlay
  legend or `keymap.json` mirror is touched.
