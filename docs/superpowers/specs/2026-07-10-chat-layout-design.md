# Chat Layout — design

Date: 2026-07-10
Status: approved (brainstormed with visual mockups; treatment C selected)
Mockups: `.superpowers/brainstorm/979386-1783729919/content/layout*.html`
Updated 2026-07-10 after the `ask-passage` branch landed (Ctrl+a block-select →
ask card, `docs/superpowers/specs/2026-07-10-ask-passage-keybind-design.md`) — notes marked
**[ask-passage]** reflect what that branch built or discovered.

## Overview

An alternative reader layout for creating journal Q&A entries in conversation
with the Claude API. Toggled by **Tab**: the main reading card keeps its exact
dimensions and pins to the far right of the window; the freed left space
becomes a chat panel of the same height. The panel sends passage-anchored
prompts to Claude, holds a multi-turn conversation, and — once the user is
satisfied — saves a curated Q&A to `journal_entries` and pivots into a
revision loop on that saved entry.

## Decisions (from brainstorming)

- **Panel treatment: "bare on root" (C).** No card chrome. Chat text renders
  directly on the themed root background in light ink at graded opacities
  (light sepia on the teal root for kindle-sepia), with a thin quote rule for
  context chips and a translucent-bordered input box. Per-theme colors.
- **Toggle: Tab.** Displaces `TogglePause`, which moves to **`-`** (minus).
  `TogglePreviousWork` (the old `-` bind) is retired; Ctrl+`-` (recent-works
  picker) covers that need.
- **Close: Ctrl+Tab** while the panel is open (shadows `ToggleLastOverlay`
  only in that state; the bind keeps its current meaning when the panel is
  closed).
- **Focus: split, three-way Tab cycle.** With the panel open, Tab cycles
  chat prompt → chat/journal content (transcript) → main card → back to
  prompt. The reader stays fully live when it has focus.
- **Prompt context: cursor segment ±2 neighbors** plus title, author, and
  chapter/scene (`div1`/`div2`). Nothing more (no synopsis, no prior journal
  entries). System prompt is the existing `journal_qa_prompt(work_type)`
  from lit.db `api_prompts` (`journal.qa`, active version).
- **Multi-turn conversation**, session-scoped (cleared on panel close and on
  work switch).
- **Curated save with revision loop** (see below), not auto-save.

## Layout & architecture

**Approach: overlay panel + asymmetric card margin.** The card's position is
already owned by `apply_card_sizing` (`src/app/layout.rs`), which centers
`content_hbox` by splitting slack margin equally. A new layout-mode flag
(`chat_layout_open`) makes the split asymmetric: right margin stays
`CARD_OUTER_MARGIN` (24px), all remaining slack goes to `margin_start`. The
card is flush right with no change to its size or to the widget chain.

The chat panel is an `add_overlay` layer on the window's outer overlay (like
the title bar and concordance bar), aligned left, sized to the card's height
via `main_card_rect`. This respects the established rule: new panels go in as
overlay layers, never into the reader's size-bearing widget chain
(`feedback_picker_overlay_not_chain`). Every existing overlay sizes itself
off `main_card_rect` (the card's live allocation), so the whole overlay stack
follows the card to its new position with no changes.

The existing resize tick callback re-applies both the margin split and the
panel geometry. Because the card's dimensions never change, the pinned
`play_pages` / `prose_pages` fingerprints (keyed on card width) stay valid —
no pagination regeneration.

**Rejected alternative:** a true sibling `Box` wrapping `[chat_panel, card]`.
Restructures the chain every picker/overlay attaches to — the exact class of
change that caused past sizing bugs — for no user-visible benefit.

## Panel anatomy (top to bottom)

- **Header** — work title, author, current chapter/scene in small caps,
  faint ink; thin rule below.
- **Transcript** — scrolled view of the session's exchanges: user question
  (brightest ink), Claude answer (slightly dimmer), faint role labels. An
  exchange asked from a different cursor position than the previous one shows
  its context chip (quote rule + italic excerpt of the cursor segment) above
  the question. Saved entries show a `✓ saved` mark.
- **Input box** — translucent-bordered vim editor (same vim engine as the
  ask cards; `;` maps to `:`, visual `y` copies to clipboard). `Ctrl+Enter`
  sends; Escape is vim normal mode and closes nothing.

## Focus model

- **Tab (panel closed):** open the panel; card slides right; focus lands in
  the chat prompt.
- **Tab (panel open):** cycle focus — chat prompt → transcript → reader →
  prompt.
  - *Prompt focused:* keys go to the vim editor.
  - *Transcript focused:* `j`/`k` scroll / move the exchange cursor; `s`
    saves (see below).
  - *Reader focused:* all reader keys work — navigation, page turns,
    playback, sync. The context chip live-follows the cursor segment.
- **Ctrl+Tab (panel open):** close the panel; card re-centers.

MPV/sync interactions: none by design. The panel is display-only with respect
to playback; sync page-turns and karaoke continue on the card while typing.

## Prompt construction

New helper `segment_context(state, n)` built on the pure `block_bounds`
(`src/input/visual.rs`): find the cursor's paragraph/speech, then walk
outward for `n = 2` neighbors on each side (truncating at chapter/buffer
edges). The user message contains:

**[ask-passage] Block discovery is buffer-structure-dependent — reuse the
shipped rule, don't re-derive it.** The ask-passage branch found (via headless
e2e on Bleak House) that the blank-line rule only works for `.txt`-built
buffers; works with NO `text_file` (BH and all default DB-join prose) render
one work row per buffer line with no blank lines at all, so a naive
blank-line walk selects the entire 7306-line buffer.
`visual::enter_visual_block_mode` already implements the correct dual rule:
blank-line/separator boundaries when `current_work.text_file` is set,
same-`work_line_for_buffer`-row boundaries otherwise. Factor that bounds
computation out of `enter_visual_block_mode` into a shared
`cursor_block_bounds(state) -> Option<(usize, usize)>` and build
`segment_context` on it — the ±2 neighbor walk must use the SAME boundary
semantics (next blank-delimited block on `.txt` buffers; next work row on
DB-join buffers).

- title, author, chapter/scene (`div1`/`div2`)
- the five segments in order, the cursor's segment explicitly marked
- the user's question

System prompt: `journal_qa_prompt(work_type)` — genre/unit substitution as
today. Nothing else.

## Multi-turn conversation

New `send_chat(system, messages, model)` in `src/claude.rs` alongside the
single-shot `send_message` — same endpoint, but `messages` carries the
session's prior user/assistant turns. Context blocks ride inside the user
turns they belonged to, so history stays coherent when the cursor moves
between questions. No streaming in v1; the panel shows a "thinking…" line
until the reply lands. Async bridge unchanged (`tokio_handle` +
`glib::spawn_future_local`).

History resets on panel close and on work switch.

## Curated save & revision loop

1. Explore multi-turn until an exchange is worth keeping.
2. With the transcript focused, `j`/`k` select an exchange; **`s`** saves it
   via the existing `db::journal::save_passage_page` (the table is the
   journal *pages* store — there is no `journal_entries` table): citations
   and `source_text` from the segment context attached to that exchange,
   `claude_model` provenance, keyed by `Work.canonical_abbrev`.
   **[ask-passage]** For the saved page to render correctly in the journal
   overlay's passage band, derive the citation pair the way
   `action_journal_qa` does — `gloss::build_context_for_type(work, lines,
   "reader-gloss")` for `start_citation`/`end_citation`/`div1`/`div2` — and
   store `source_text` as the same `<speaker>/<verse>` markup produced by
   `echoes::build_source_header`. Matching both keeps chat-saved pages
   indistinguishable from Ctrl+a ask-card pages in the band.
3. The transcript then **clears and is replaced by the saved entry itself**
   (the stored Q and A rendered as the panel content).
4. The input stays live; prompts are now **revision instructions** against
   the entry ("tighten the answer", "sharpen the question"). Claude receives
   the current entry text + the instruction + the original passage context,
   and returns a revised Q&A that replaces the panel content. Revisions may
   rewrite both the question and the answer.
   **[ask-passage]** This machinery already exists: `journal.rs`'s
   `rewrite_with_claude` (used by the ask card's vim `R` path) assembles
   exactly this shape via `rewrite_context` + `rewrite_user_message`, saves,
   snapshots `journal_undo`, and purges stale TTS audio. Reuse or lightly
   generalize it rather than writing a parallel revision path.
5. **`s` again UPDATEs the same journal row** (never a second insert) —
   `db::journal::update_journal_page` already exists (the vim `:w` path uses
   it); no new DB function is needed. **[ask-passage]** Any update MUST also
   call `purge_journal_audio(conn, id)` as the existing save/rewrite paths
   do, or the journal overlay replays stale cached TTS for the old answer.
   Revise → save iterates freely. The stored artifact is always exactly the
   model's latest output — the gloss `R`-rewrite pattern, no chat noise.
6. Fresh start: Tab to the reader and ask a new question (or reopen the
   panel) — exploration mode with an empty transcript.

Saved entries render in the existing journal overlay's passage band with no
changes there. A toast + `✓ saved` mark confirm each save/update.

## Keybind changes

- `Tab` (reader): `TogglePause` → `ToggleChatLayout`. (Pause stays reachable
  on plain `a` regardless — Tab was a duplicate bind.)
- `-` (reader): `TogglePreviousWork` → `TogglePause`; `TogglePreviousWork`
  retired (Ctrl+`-` recent picker remains)
- `Ctrl+Tab`: unchanged binding; shadowed to "close chat panel" while open
- **[ask-passage] `Ctrl+a` interplay (decide during implementation):**
  reader-mode `Ctrl+a` now block-selects and opens the modal ask card
  (second `Ctrl+a`/Return). With the chat panel open and the reader focused,
  recommend `Ctrl+a` seed the chat prompt with the cursor block as its
  context chip instead of opening the modal ask card — one Q&A entry point
  per layout, no card-over-panel stacking. If the modal card is allowed over
  the open panel instead, define which surface owns focus on its close.
- Update **both** `keymap_config.rs` and the stowed
  `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`, plus the Ctrl+/
  keybinds overlay (keycaps + `describe()` arms — run
  `update-cairo-keybinds-overlay`).

## Edge cases & errors

- **Not enough room:** Tab only opens the layout when the freed left space
  would be ≥ ~500px (single-column prose/verse at 1920×1200 qualifies;
  two-column plays at ~85%-window card width do not). Otherwise a toast:
  "no room for chat panel at this layout."
- **API errors** (`ClaudeError`: missing `ANTHROPIC_API_KEY`, timeout, rate
  limit): a dim error line in the transcript; the failed question is
  restored to the input for retry.
- **Work switch with panel open:** history clears; panel stays open on the
  new work (context chip follows).

## New / changed code

- `src/ui/chat_panel.rs` — new: panel widget, transcript rendering, exchange
  cursor, per-theme light-ink palette
- `src/input/actions/chat.rs` — new: toggle/focus-cycle/send/save/revise
  handlers; session state (`Vec<Exchange>`: question, answer, context,
  saved-entry id)
- `src/claude.rs` — add `send_chat`
- `src/input/segments.rs` (or `visual.rs`) — `segment_context` on the shared
  `cursor_block_bounds` factored out of `enter_visual_block_mode`
  (**[ask-passage]** carries the text_file/DB-join dual boundary rule)
- `src/app/layout.rs` — asymmetric margin branch in `apply_card_sizing`
- `src/db/journal.rs` — no new UPDATE needed: reuse `update_journal_page` +
  `purge_journal_audio` (**[ask-passage]** both exist and are exercised by
  the ask card's vim-save/rewrite paths)
- keymap files + keybinds overlay as above

## Testing

- Unit: `segment_context` (prose paragraph, play speech, buffer edges, ±2
  truncation at chapter start/end); prompt/message assembly; revision-loop
  state transitions. Vim engine already covered.
- Headless (cage + grim): panel opens, card right-pinned at unchanged
  dimensions, three-way Tab focus cycle, Ctrl+Tab restores centering; API
  calls stubbed/skipped headlessly (same spirit as `LIT_HEADLESS_TEST`
  skipping MPV).
- Manual: live check on the real renderer for ink opacities against each
  theme's root color.
