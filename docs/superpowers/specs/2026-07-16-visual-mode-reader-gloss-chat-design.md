# Visual-mode `-` — auto reader gloss in the chat panel

_Design spec. 2026-07-16 (US Central)._

## Summary

Add one keybind: `-` in visual mode. After selecting lines with `V`, pressing
`-` opens the existing chat panel pinned to that selection, immediately fires
the reader-gloss prompt without showing an ask input, and writes the resulting
gloss to `passages` + `glosses` on arrival. Inside the panel, `a` reopens the
ask input for follow-up questions and `s` saves those to the journal, exactly
as they do today.

The bind joins visual-mode `Ctrl+a` (Journal Q&A ask card) and visual-mode
`Tab` (chat pinned to selection) as a third sibling on the same
select-then-act flow.

## Corrections to the original premise

Three assumptions in the original request do not match the code, and the
design follows the code:

- **`Ctrl+a` is not a reader-level ask-passage bind.** It was deliberately
  unbound (`keymap_config.rs:400`, with a test asserting it stays `None`).
  The ask-passage path lives inside visual mode (`visual.rs:512`).
- **`Tab` does not toggle the chat panel.** When the panel is open, `Tab`
  cycles focus (prompt → transcript → reader); only `Ctrl+Tab` closes
  (`chat.rs:238`). So `-` is not mirroring a toggle.
- **The chat panel already floats over the non-cursor column** on two-column
  works (`float_side_for_cursor`, `chat.rs:143`). The floating behavior is
  existing machinery to reuse, not new work.

## Binds

- **New:** `-` (`minus`) in visual mode → new action, auto reader gloss.
- **Unchanged:** plain `-` in the reader stays unbound. The existing test at
  `keymap_config.rs:511` asserting `plain("minus") == None` keeps passing.
- **Unchanged:** `Ctrl+-` → `JumpToNextVocab`, `Ctrl+Shift+-` /
  `Ctrl+Shift+_` → `JumpToPrevVocab`. No rebinding, no `keymap.json` edit.
- **Closing:** `Ctrl+Tab` and Escape, which already close this panel from both
  prompt and transcript focus.

### Why no `Ctrl+-` close bind

`Ctrl+-` was considered as a gloss-close bind mirroring `Ctrl+Tab`. It was
rejected: the panel `-` opens *is* the chat panel, which `Ctrl+Tab` and Escape
already close, so the bind would be a third way to close one widget. Its cost
would be evicting the vocab loop from the one RPD cap where forward and
backward sit naturally together — `minus` is level 1 and `underscore` level 2
on `<AC11>`, so `Ctrl+-` and `Ctrl+Shift+-` are distinct chords on one key.

Every alternative home for the vocab pair fails:

- `Ctrl+a` / `Ctrl+Shift+a` — `Ctrl+Shift+A` is taken by `PickAttributionSet`
  (`keymap_config.rs:379`), so the pair cannot form.
- `Ctrl+equal` / `Ctrl+plus` — both free, but `equal` is on `<AE06>` and
  `plus` on `<AE01>`: different physical caps under RPD, not a shift-pair.

## Flow

1. `V` enters visual mode; the selection lives in
   `AppState.visual_selection: Option<SelectionState>` (`visual.rs:6`).
2. `-` dispatches the new action from `handle_visual_key` (`keymap.rs`, beside
   the existing `Ctrl+a` and `Tab` arms).
3. The panel opens pinned to the selection; the reader-gloss prompt fires with
   no input shown.
4. The answer lands as exchange #1 in the transcript and is written to the DB.
5. `a` reopens the ask input for follow-ups; `s` saves a follow-up to the
   journal; `Ctrl+Tab` or Escape closes.

## Handler

New function `action_reader_gloss_chat` in `src/input/visual.rs`, beside
`action_journal_qa` (`:512`). It composes three existing pieces:

1. **`chat::open_chat_pinned_to_selection`** (`chat.rs:213`) — reads
   `visual_selection.range()`, builds the `SegmentContext` via
   `segments::selection_context`, exits visual mode, opens and floats the
   panel, sets `chat.pinned_passage`. Reused, with the placement change in
   "Panel placement" below.
2. **`gloss::build_context_for_type(work, &selected_lines, "reader-gloss")`**
   — the same call `action_journal_qa` makes at `visual.rs:533`. Yields the
   citation-based context (`start_citation`, `end_citation`, `div1`, `div2`,
   `character`, `source_text`) the `passages` row requires.
3. **A direct submit** — builds the user message with
   `gloss::build_user_message`, sends `gloss::READER_GLOSS_PROMPT`
   (`gloss.rs:1081`), and dispatches through `claude_bridge`.

### Why not reuse `submit_chat_prompt`

`submit_chat_prompt` (`chat.rs:347`) assumes it is draining a typed draft from
the ask card, and it intercepts the literal strings `"s"` and `"S"` as
save/consolidate aliases (`:354-365`). Rather than thread a bypass flag
through it, `-` calls the bridge directly and reuses only the success-callback
shape: push an `Exchange` onto `s.chat.exchanges`, set `chat.cursor`, focus
the transcript.

### Prompt selection

`READER_GLOSS_PROMPT` (`gloss.rs:1081`) — the auto-gloss prompt. Not
`READER_GLOSS_QUESTION_PROMPT` (`gloss.rs:1409`), which is the Add/question
variant used when the user supplies a question.

## Panel placement

On a two-column work the panel floats over the column the passage is *not*
in, so the passage stays visible. A selection spanning both columns has no
such column: either side covers half of it.

**Rule: a selection spanning both columns floats the panel LEFT.** A
selection within one column keeps today's behavior — float over the other
column.

`line_in_right_column(line, split, end)` (the free function
`cursor_in_right_column` calls at `chat.rs:132,139`) takes an explicit line,
so the selection's span is classified with the same helper and no new column
logic:

- `line_in_right_column(start, ..) != line_in_right_column(end, ..)` →
  spans both → `ChatPlacement::FloatLeft`.
- Otherwise → float over the column the selection is not in.

The `split`/`end` bounds come from the same two sources
`cursor_in_right_column` uses, in the same order: the active page table's
spread for `page_top_line` when in table mode, else the live
`viewport::column_split` with its `split > page_end` "no right column"
normalization.

### Ordering defect this exposes

`open_chat_pinned_to_selection` calls `exit_visual_mode` (`chat.rs:228`)
*before* `toggle_chat_layout` (`:231`), and `toggle_chat_layout` picks the
side via `float_side_for_cursor(s)`, which reads `s.current_line` — not the
selection. By the time placement is decided the selection is already cleared,
so today the side is chosen from wherever the cursor sits (one end of the
selection). That is invisible for a within-column selection, since both ends
agree, but it makes the spanning case unimplementable as written.

The fix: capture `(start, end)` before `exit_visual_mode` (the function
already does, at `:217`) and thread the resulting placement into the open
path, rather than letting `toggle_chat_layout` re-derive it from the cursor.
Scope this to the selection-pinned entry point; the plain `Tab` open with no
selection keeps using `float_side_for_cursor`.

`Ctrl+l` (`flip_panel_side`, `chat.rs:153`) still flips the panel afterwards,
so a left placement the user dislikes is one keypress from the other side.

### Guards

- **In-flight:** honor `chat.pending` (`chat.rs:377`) so a second `-` cannot
  double-fire.
- **No room:** if `open_chat_pinned_to_selection` fails (single-column work
  with under `CHAT_MIN_PANEL_W` = 500px free, toasting "No room for chat panel
  at this layout", `chat.rs:273`), `-` aborts there and does nothing further.
- **Cache:** before calling Claude, check `find_glosses_by_start` for an
  existing gloss on that span, as `action_reader_gloss` does (`visual.rs:575`).
  On a hit, populate the transcript from the stored gloss and issue no
  request — pressing `-` twice on a passage is cheap and idempotent.

## Persistence

Two stores, each keeping its existing meaning.

**The `-` gloss → `passages` + `glosses`, on arrival.** The success callback
calls `persist_render_install_gloss(..., "reader-gloss", ...)`
(`gloss.rs:1328`) — the same function the Action-popup "Reader Gloss" uses —
which routes to `db::queries::save_gloss` (`queries.rs:2235`):
`INSERT OR IGNORE INTO passages(hash, work_abbrev, start_citation,
end_citation, div1, div2, character, source_text)`, resolve `passage_id`, then
`INSERT INTO glosses(passage_id, gloss_type, gloss_text, claude_model)` with
`gloss_type = "reader-gloss"`. Reusing this path yields the glossed-line tint
and gloss-overlay pickup with no new DB code.

Note that gloss spans are **citation-based** on the `passages` row, not line
numbers. Any line-number-keyed code touching them needs `work_line_for_buffer`
translation.

**Follow-ups → `journal_entries`, unchanged.** A question asked with `a`
behaves exactly as chat does today: held in `s.chat.exchanges` in memory until
`s` saves it via `save_passage_page` (`db/journal.rs:420`).

### Accepted consequence

The auto-gloss exchange arrives with `saved_id: None` — that field tracks
*journal* saves. So pressing `s` on exchange #1 writes a second copy into
`journal_entries`. This is deliberate, not a defect: `s` means "save this to
the journal" everywhere in the panel, and on the gloss exchange it produces a
second artifact in a different store. Revisit only if it proves annoying in
use.

## Error handling

Follows the existing chat paths. A Claude failure surfaces the same toast chat
shows today, `chat.pending` clears, and the panel stays open. The DB write
happens only on a successful response, so a failure leaves no gloss row.

## Testing

**Unit (`cargo test --bins`), no GUI:**

- `-` in visual mode resolves to the new action.
- Plain `-` in the reader remains unbound (existing test, must still pass).
- `Ctrl+-` still resolves to `JumpToNextVocab`.
- `build_context_for_type` yields the expected citations for a selection.
- The cache-hit path issues no API request.
- Placement: a selection wholly in the left column floats right; wholly in
  the right column floats left; spanning both floats left.
  `line_in_right_column` takes explicit lines, so these are table-driven cases
  needing no GUI.

**Needs a real render** — the headless cage e2e or the user's `crll` session:

- The panel floats over the non-cursor column on a two-column work.
- A both-column selection floats left and the panel does not cover the
  selection's left half.
- The auto-fired answer lands in the transcript with no input shown.
- `a` reopens the ask input; `s` saves a follow-up.

Per the project testing rule, a green build is not "done" for a change with
visible behavior; the on-screen criteria above get either a headless drive or
a manual hand-off with exact steps.

## Out of scope

- A separate floating gloss overlay widget with its own `InputMode` — rejected
  in favor of reusing the chat panel.
- Rebinding the vocab loop.
- Changing what `s`, `Ctrl+Enter` (revision), or `S` (consolidate) mean.
- Changing placement for the plain (unpinned) `Tab` open, or for
  `regate_panel`'s work-switch re-check (`chat.rs:171`) — both keep using
  `float_side_for_cursor`. Only the selection-pinned entry point gains the
  spanning rule.
