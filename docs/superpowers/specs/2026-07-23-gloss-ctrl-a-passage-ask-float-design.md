# Gloss-overlay Ctrl+a — float the passage-ask (keep gloss visible)

**Date:** 2026-07-23 (US Central)
**Status:** Approved design (brainstormed with user; decisions inline)
**Scope:** Ctrl+a pressed INSIDE the gloss overlay only
(`handle_gloss_key` in `src/input/keymap.rs`, plus `actions/gloss.rs`). The
journal overlay's layout and the reader's visual-mode Ctrl+a are out of scope.

## Problem

Ctrl+a in the gloss overlay currently **closes** the gloss overlay and opens
the **journal** overlay's stacked "Ask a question about this passage" card
(`gloss.rs::ask_journal_for_passage` → `close_gloss_to_reader` +
`journal::begin_passage_ask`, keymap.rs:2456). The user wants the gloss
commentary to stay full-height on the left with the passage-ask floated on the
right — reusing the Ctrl+r float shipped in
`2026-07-23-gloss-ask-card-right-float-design.md` — while typing the question.
The answer still lands in the journal overlay on submit.

## Decision summary (user-approved)

- **Ctrl+a in the gloss overlay does NOT close the overlay.** It opens the
  gloss overlay's **own floated ask card** (the same right-float the Ctrl+r
  gloss/synopsis asks now use), titled "Ask a question about this passage".
  Gloss commentary stays left; ask floats right; pair centered; auto-INSERT.
- **On submit (Ctrl+Enter):** close the gloss overlay and hand off to the
  existing journal passage-ask Claude flow; the loading card + answer render in
  the **journal overlay** exactly as today. The float is only for *typing* the
  question.
- **On cancel (Escape):** close the floated ask card and STAY in the gloss
  overlay (the gloss card re-centers). Nothing is persisted.
- **New prompt mode:** add `GlossPromptMode::PassageQa` rather than overloading
  an existing gloss mode (Add/Edit/FixIpa route to gloss persistence; PassageQa
  routes to the journal passage flow).
- **Unchanged:** the reader's visual-mode Ctrl+a (no gloss overlay open there —
  nothing to float beside); the journal overlay's layout; all keybinds (Ctrl+a
  stays bound — only its behavior in the gloss context changes).

## Why this approach

The gloss overlay already hosts a floated ask card (from the Ctrl+r feature):
`AskCardHost` in float mode, the `.gloss-ask-float` panel, the centered-pair
reservation. Reusing it means the visual half is already built and proven. The
only new work is **routing**: opening that ask card from Ctrl+a with a new mode,
and on submit dispatching to the journal passage flow instead of
`add_gloss`/`edit_gloss`.

Because the answer is read in the journal overlay (user's choice), NONE of the
journal Q&A rendering has to move into the gloss overlay — the gloss overlay
only hosts the *input*. The submit reuses the journal's existing `ask_claude`
path, which reads `journal_band` + `journal.pending_passage` from state (both
set at Ctrl+a time), so the answer persists and renders identically to today.

## Architecture

### The Ctrl+a flow (open)

New handler `open_passage_qa_float(state)` in `actions/gloss.rs`, replacing the
`ask_journal_for_passage` call at the gloss-overlay Ctrl+a arm (keymap.rs:2456):

1. Capture the passage args from `gloss_context` — the SAME extraction
   `ask_journal_for_passage` does today (`gloss.rs:3216-3254`): the
   `(div1, div2, start_citation, end_citation, source_text)` tuple, preferring
   the exact start..end citation range, falling back to the whole scene. Factor
   that extraction into a shared helper so both paths use one copy (DRY).
2. Set `journal_band = JournalBand::Passage{div1,div2,start,end}` and
   `journal.pending_passage = Some(PendingPassage{source_text, band})` and
   `journal.prompt_mode = JournalPromptMode::Ask` — exactly what
   `begin_passage_ask` sets (`journal.rs:1589-1599`), MINUS opening the journal
   overlay's ask card and MINUS setting `input_mode = JournalOverlay`. Also set
   `journal.return_pos` / `journal.entry_page_id` as `begin_passage_ask` does so
   the eventual journal render restores correctly.
3. Do NOT `close_gloss_to_reader`. Set `gloss_prompt_mode = PassageQa` and open
   the gloss overlay's floated ask card via `show_prompt_dialog(state,
   PassageQa)` (which calls `open_ask_card_with(...)` → float), then auto-INSERT
   (`feed_ask_vim_key(Char('i'))`), matching the other gloss asks.

`input_mode` stays `GlossOverlay` so the gloss overlay's key handling (including
the ask-card key interception already wired for Ctrl+r/Edit) routes the typed
keys to the floated ask card.

### The title (show_prompt_dialog)

`show_prompt_dialog` (`gloss.rs:703`) gains a `PassageQa` title arm:
"Ask a question about this passage" (matching the journal card's wording,
`journal.rs:1603`). Hint: "Ctrl+Enter submit". Legend: "" (a fresh question has
no answer, drop straight to INSERT).

### The submit (submit_gloss_prompt)

`submit_gloss_prompt` (`gloss.rs:3706`) gains a `PassageQa` arm:

- Read the text (`take_ask_text`), then `close_gloss_prompt` (hides the float).
- **Empty** input → no-op: stay in the gloss overlay (nothing to ask), matching
  Add/FixIpa. Leave band/pending_passage as-is (a later re-ask reuses them) OR
  clear them — pick one in the plan; the simplest is to leave them since the
  overlay is still open and a re-Ctrl+a re-sets them anyway.
- **Non-empty** → `close_gloss_to_reader(state)` (canonical gloss close, back to
  reader), THEN invoke the journal passage submit with the captured text. Expose
  a thin entry in `actions/journal.rs`, e.g.
  `submit_passage_question(state, &text)`, that runs today's post-`submit_prompt`
  body: show the loading card, call `ask_claude(state, &text)`, land in the
  journal overlay. `ask_claude` reads the band + `pending_passage` set at step
  2, so no extra args are needed. Factor `submit_passage_question` out of the
  existing `submit_prompt` (`journal.rs:2218`) so the two share the ask path
  (the `rewrite` branch stays in `submit_prompt`; only the new-Q&A tail is
  shared).

The journal overlay opens itself as part of `ask_claude`'s loading-card render
(that path already sets `input_mode = JournalOverlay` and shows the journal
overlay), so the handoff needs no explicit overlay-open call beyond what
`ask_claude` already does. Confirm this in the plan (if `ask_claude` assumes the
journal overlay's ask card was open, add the minimal overlay-open the journal
path needs — the plan verifies against the code, the spec commits to the
outcome: answer renders in the journal overlay as today).

### The cancel (Escape)

Escape in the gloss overlay with the floated ask card open already routes to the
gloss ask-card cancel (the Ctrl+r Edit float uses the same double-Escape
force-cancel via `handle_gloss_edit_key`). The `PassageQa` cancel path calls
`close_gloss_prompt` (hides the float, gloss re-centers) and stays in the gloss
overlay. Clear the transient `journal.pending_passage` / band on cancel so a
later unrelated journal open isn't polluted — confirm the exact cleanup site in
the plan (mirror how the journal overlay's own cancel treats `pending_passage`).

## Components / files

- `src/app/mod.rs` — `GlossPromptMode` gains `PassageQa`.
- `src/input/keymap.rs` — the gloss-overlay Ctrl+a arm (~2456) calls
  `open_passage_qa_float` instead of `ask_journal_for_passage`. (The
  `handle_gloss_edit_key` submit/cancel arms already dispatch
  `submit_gloss_prompt` / `close_gloss_prompt` for the floated ask card; confirm
  the `PassageQa` mode flows through the same arms.)
- `src/input/actions/gloss.rs` —
  - factor the passage-args extraction shared by `ask_journal_for_passage` and
    the new path;
  - `open_passage_qa_float(state)` (open, no close);
  - `show_prompt_dialog` PassageQa title arm;
  - `submit_gloss_prompt` PassageQa arm (close gloss → journal submit).
  - `ask_journal_for_passage` may be retired if nothing else calls it (grep in
    the plan; keep it only if another caller exists).
- `src/input/actions/journal.rs` — extract `submit_passage_question(state,
  &text)` from `submit_prompt` (the new-Q&A ask tail), called by the gloss
  submit arm. `submit_prompt` keeps its rewrite branch and delegates its own
  new-Q&A tail to the shared helper (no behavior change for the journal
  overlay's own submit).
- No `theme.rs` change (reuses `.gloss-ask-float`). No new keybind, no
  `keymap.json`, no Ctrl+/ overlay change (Ctrl+a already documented; its
  gloss-context behavior changed, not its binding).

## Testing

- **Headless cage** (`test-headless-navigation` harness): open a reader-glossed
  passage, Ctrl+g (gloss overlay), Ctrl+a → screenshot must show the gloss
  commentary full-height LEFT and a bordered ask panel RIGHT titled "Ask a
  question about this passage", the pair centered (pixel-measured equal L/R
  gutters, as the Ctrl+r feature verified). Escape (double, force-cancel) →
  gloss card re-centers, still in the gloss overlay.
- **`cargo test --bins`:** the mode routing (`PassageQa` → journal submit path
  selection) and the shared passage-args extraction, as pure/unit tests where
  possible.
- **Submit → journal answer** needs a live Claude call, so that leg is a manual
  hand-off (exact steps given to the user): Ctrl+Enter a real question, confirm
  the gloss overlay closes and the answer renders + persists in the journal
  overlay as today.
- Cage is software rendering — final on-screen eyeball on the real GL renderer
  handed to the user.

## Out of scope

- The reader's visual-mode Ctrl+a (no gloss overlay to float beside).
- The journal overlay's own layout / its stacked ask card.
- Rendering the answer inside the gloss overlay (user chose journal-overlay
  handoff on submit).
- Any change to the Claude prompt, persistence schema, or Q&A content — purely
  which surface hosts the *input* and the trigger's close-vs-stay behavior.
