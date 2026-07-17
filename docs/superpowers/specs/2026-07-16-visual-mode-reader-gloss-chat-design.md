# Visual-mode `-` — auto reader gloss in the chat panel

_Design spec. 2026-07-16 (US Central)._

## Summary

Add `-` in visual mode: after selecting lines with `V`, pressing `-` opens the
existing chat panel pinned to that selection, immediately fires the
reader-gloss prompt without showing an ask input, and writes the resulting
gloss to `passages` + `glosses` on arrival.

Inside the panel, three keys act on the pinned passage: `a` reopens the ask
input for follow-up questions and `s` saves those to the journal (both exactly
as they do today), `r`/`R` reglosses — a fresh Claude call saved as a new
lit.db row — and `Ctrl+n`/`Ctrl+p` cycles through the passage's stored
glosses, wrapping.

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
- **New (panel keys, `handle_chat_transcript_key` arms — not reader binds):**
  `r`/`R` → regloss; `Ctrl+n`/`Ctrl+p` → cycle stored glosses. These live in
  the panel's own handler, so they do not touch `keymap_config.rs` and cannot
  collide with the reader-level meanings of those keys.
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
   journal; `r`/`R` reglosses the passage into a new lit.db row; `Ctrl+Tab`
   or Escape closes.

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

### Placement ordering defect this exposes

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

## Regloss — `r` / `R` in the panel

`r` or `R` in the chat transcript reglosses the pinned passage: it calls
Claude again with `READER_GLOSS_PROMPT` and saves a **new** reader-gloss row
to lit.db for the same span, appending the result to the transcript as a new
exchange.

This is a panel key, so it is an arm in `handle_chat_transcript_key`
(`keymap.rs:1326`) beside the existing `s` (`:1351`) and `a` (`:1355`) —
not a reader-level bind. Reader-level `r` stays `VocabPopupTap`
(`keymap_config.rs:302`) and plain `R` stays unbound (asserted at `:514`);
neither test changes. Both `r` and `R` map to the same action; `R` already
means regenerate/edit in the gloss overlay (`READER_GLOSS_EDIT_PROMPT`,
`gloss.rs:1490`), so the meaning carries over.

**It bypasses the cache.** The `-` cache check exists to avoid re-spending an
API call on a span that already has a gloss. Regloss wants the opposite, so
`r`/`R` skips `find_glosses_by_start` and always calls Claude. It requires a
pinned passage (`chat.pinned_passage`) and honors the `chat.pending`
in-flight guard.

**Storage: insert, newest wins.** Each regloss is a new `glosses` row on the
same `passage_id` via the same `persist_render_install_gloss` path — history
is kept, nothing is overwritten. `save_gloss`'s `INSERT OR IGNORE INTO
passages` (`queries.rs:2250`) means the passage row is reused, not
duplicated.

Lookups resolve to the newest row with no change: `find_glosses_by_start`
already orders `(g.gloss_type = 'reader-gloss') DESC, g.timestamp DESC`
(`queries.rs:2169`), so the `-` cache check and the glossed-line tint pick up
the most recent gloss.

### One-second timestamp tie

`glosses.timestamp` is written by `CURRENT_TIMESTAMP`, which SQLite stores at
one-second granularity. Two glosses on the same span within the same second
tie on `timestamp DESC`, and SQLite may return either — reglossing twice in
quick succession is precisely that case.

Fix: add `g.id DESC` as a final tiebreak to `find_glosses_by_start`'s ORDER
BY. `id` is `last_insert_rowid()` (`queries.rs:2266`) and so is monotonic per
insert, making "newest wins" deterministic. This is a one-line change to an
existing query; it strictly refines an ordering that was previously arbitrary
within a tie, so no existing caller's behavior regresses.

## Gloss cycling — `Ctrl+n` / `Ctrl+p` in the panel

`Ctrl+n` and `Ctrl+p` cycle forward and backward through every stored gloss
for the pinned passage, wrapping at both ends. Since regloss keeps history,
this is how the history is read back — including glosses written in earlier
sessions.

Also arms in `handle_chat_transcript_key`, beside `Ctrl+l`
(`keymap.rs:1360`). No conflict with reader-level `Ctrl+n`/`Ctrl+p`
(`VocabJournalPageNext`/`PagePrev`, `keymap_config.rs:308-309`) — the panel
handler is a separate context and those binds are untouched.

**Two different lists.** `j`/`k` move `transcript_cursor_move`
(`keymap.rs:1343`), a cursor over this session's in-memory `chat.exchanges`.
`Ctrl+n`/`Ctrl+p` moves over stored `glosses` rows from lit.db. These are
distinct axes and need distinct state — cycling must not reuse `chat.cursor`
as its index.

Cycling does, however, leave the transcript cursor ON the gloss: the shared
`push_gloss_exchange` sets `chat.cursor = 0` when it rewrites slot #1, so a
`Ctrl+n` pressed while the cursor sits on a follow-up snaps it back up to the
gloss. That is intended, and consistent with `-` and `r`/`R`, which reset the
cursor through the same helper. Slot #1's content has changed under the user;
selecting it beats leaving the cursor on a follow-up while the gloss silently
swaps above. The invariant is about the INDEX (`cycle_gloss` writes only
`gloss_index`), not about the cursor never moving.

**Cycling swaps exchange #1 in place.** The auto-gloss occupies the first
transcript slot; `Ctrl+n`/`Ctrl+p` replaces the gloss text shown there with
the next stored gloss. Follow-up exchanges below it are untouched and `j`/`k`
still moves over them. The slot indicates which gloss is showing (e.g.
"2 of 5").

**New state on `ChatState`:** the ordered list of stored glosses for the
pinned passage and an index into it. Populated when `-` opens the panel
(from the `find_glosses_by_start` call the cache check already makes — no
extra query) and re-populated after a regloss, which appends a row and leaves
the index on the new gloss. With one stored gloss, cycling is a no-op.

**Scope:** cycling reads `gloss_type = "reader-gloss"` rows for the pinned
span only. It does not cycle journal entries, and it does not apply when no
passage is pinned.

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
- `r`/`R` bypasses the cache and always issues a request, where `-` on the
  same span does not.
- `find_glosses_by_start` returns the newest reader-gloss first when two rows
  share a `timestamp` (the `id DESC` tiebreak). This test fails against the
  current query — write it first.
- Cycling wraps at both ends and is a no-op with a single stored gloss;
  reader-level `Ctrl+n`/`Ctrl+p` still resolve to `VocabJournalPageNext`/
  `PagePrev`.
- Placement: a selection wholly in the left column floats right; wholly in
  the right column floats left; spanning both floats left.
  `line_in_right_column` takes explicit lines, so these are table-driven cases
  needing no GUI.

**Needs a real render** — the headless cage e2e or the user's `crll` session:

- The panel floats over the non-cursor column on a two-column work.
- A both-column selection floats left and the panel does not cover the
  selection's left half.
- `r`/`R` appends a regloss and `Ctrl+n`/`Ctrl+p` cycles between the stored
  glosses, swapping exchange #1 in place while follow-ups stay put.
- The auto-fired answer lands in the transcript with no input shown.
- `a` reopens the ask input; `s` saves a follow-up.

Per the project testing rule, a green build is not "done" for a change with
visible behavior; the on-screen criteria above get either a headless drive or
a manual hand-off with exact steps.

## Out of scope

- A separate floating gloss overlay widget with its own `InputMode` — rejected
  in favor of reusing the chat panel.
- Rebinding the vocab loop, or changing reader-level `r`, `R`, `Ctrl+n`, or
  `Ctrl+p`.
- Changing what `s`, `Ctrl+Enter` (revision), `S` (consolidate), or `j`/`k`
  (transcript cursor) mean.
- Deleting or pruning stored glosses. Regloss only ever appends; nothing in
  this design removes a gloss row.
- Changing placement for the plain (unpinned) `Tab` open, or for
  `regate_panel`'s work-switch re-check (`chat.rs:171`) — both keep using
  `float_side_for_cursor`. Only the selection-pinned entry point gains the
  spanning rule.
