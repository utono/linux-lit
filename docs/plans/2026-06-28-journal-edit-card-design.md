# `E` — direct-edit a journal Q&A, with optional Claude rewrite

_2026-06-28 (US Central)._

## Problem

In the journal overlay, `E` is meant to edit the current Q&A page. Today it
opens the single-field ask card titled "Edit: ask a new question for this page"
and **re-asks Claude with a brand-new question, replacing the answer** with
Claude's fresh response (`begin_edit` → `submit_prompt` → `ask_claude` with
`JournalPromptMode::Edit`). The user never sees or directly edits the stored
answer. This is the reported "didn't work properly": there is no way to revise
the stored answer text; the only edit is "throw it away and re-ask".

## Goal

`E` opens a dedicated **edit card** where the user can:

1. **Hand-edit** the stored Question and Answer and save them straight to
   `lit.db` (no Claude call), and
2. Optionally type a **rewrite instruction** and have Claude revise the answer.

## Decisions (from brainstorming)

- **Three stacked, Tab-cycled fields:** Question (pre-filled), Answer
  (pre-filled, larger), Rewrite-instruction (empty).
- **Two submit keys** (journal edit card only):
  - **Ctrl+Enter → save as-is:** write edited Question + edited Answer verbatim
    to the current page. No Claude. Instruction field ignored.
  - **Alt+Enter → ask Claude to rewrite:** send Question + Answer + instruction
    to Claude; save Claude's revised answer (with the edited question).
- **Empty instruction on Alt+Enter** → fall back to save-as-is, with a toast
  `"No rewrite instruction — saved as-is"` (no surprise Claude call).
- **A dedicated edit card**, NOT a modification of the shared single-field
  `AskCard`. The `A` "ask a new question" flow and the gloss/synopsis ask cards
  stay untouched.

## Components

### 1. DB layer — `src/db/journal.rs` (no new code)

`update_journal_page(conn, id, question, answer, claude_model)` already updates
both fields by id (touches only question/answer/claude_model + timestamp; never
scope/div1/div2). Both paths use it: save-as-is writes the user's edited text;
rewrite writes Claude's revised answer. The page's existing `claude_model` is
preserved on a pure hand-edit (a hand-edit does not relabel provenance); the
rewrite path stores the model that produced the revision.

### 2. New widget — `src/ui/journal_edit_card.rs`

A self-contained three-field edit card, modeled on `AskCard` but pre-fillable
and multi-field:

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditField { Question, Answer, Instruction }

pub struct JournalEditCard {
    container: gtk4::Box,         // title, Q label+view, A label+view, instr label+view, hint
    question: gtk4::TextView,
    answer: gtk4::TextView,
    instruction: gtk4::TextView,
    focus: std::cell::Cell<EditField>,
    return_focus: gtk4::Widget,   // the page view, like AskCard
}

impl JournalEditCard {
    pub fn new(text_margins: i32, return_focus: &impl IsA<gtk4::Widget>) -> Self;
    pub fn container(&self) -> &gtk4::Box;
    pub fn open(&self, question: &str, answer: &str, card_width: i32); // pre-fill Q+A, clear instr, focus Question
    pub fn close(&self);
    pub fn is_open(&self) -> bool;
    pub fn cycle_focus(&self);    // Question -> Answer -> Instruction -> Question
    pub fn focused_field(&self) -> EditField;
    pub fn take(&self) -> (String, String, String); // (question, answer, instruction); clears boxes
    pub fn set_font(&self, family: &str, size: i32);
}
```

Reuses the existing card CSS (`card-focused` / `card-dimmed`) and
`card_side_margin` insetting. Each field is a label + a `TextView`; the answer
view is the tallest. Focus styling mirrors `AskCard::set_focus`.

### 3. Host integration — `src/ui/journal_overlay.rs`

`JournalOverlay` gains the edit card alongside the existing ask card, appended
to its `container`. Because the edit card is taller than the single-field ask
card, opening it must shrink the fixed-height scroll by the edit card's natural
height (the occlusion-prevention mechanism `AskCardHost` already implements for
the ask card). The overlay opens/closes the edit card through that same
shrink/restore path.

New `JournalOverlay` methods:
`open_edit_card(question, answer)`, `close_edit_card()`, `edit_is_open()`,
`edit_focus() -> EditField`, `toggle_edit_focus()`,
`take_edit_fields() -> (String, String, String)`.

### 4. Handlers — `src/input/actions/journal.rs`

`begin_edit` is **rewritten**:
- Guard: empty band (`journal.pages` empty) → return.
- Read the current page's stored `question` + `answer` from
  `journal.pages[page_index]`.
- `s.journal_overlay.open_edit_card(&q, &a)`.

Two new submit handlers:

- `submit_edit_save(state)` — **Ctrl+Enter.** `take_edit_fields()` → (q, a, _);
  if both q and a are non-empty(/unchanged-ok), `update_journal_page(conn, id,
  &q, &a, &existing_model)`; close card; `render_current`; toast `"Saved"`.
- `submit_edit_rewrite(state)` — **Alt+Enter.** `take_edit_fields()` → (q, a,
  instr). If `instr.trim()` is empty → behave like `submit_edit_save` and toast
  `"No rewrite instruction — saved as-is"`. Otherwise build a rewrite
  user-message (`rewrite_user_message(q, a, instr)`) and call Claude via the
  existing `run_claude_request` bridge (system prompt: `JOURNAL_QA_PROMPT`);
  in the callback `update_journal_page(conn, id, &q, &claude_answer,
  &model_used)`, `render_current`, toast.

The old `JournalPromptMode::Edit` path through `ask_claude` is removed.
`JournalPromptMode` keeps only `Ask` (drop the `Edit` variant; update the one
match site that referenced it).

A pure helper to unit-test:
```rust
fn rewrite_user_message(question: &str, answer: &str, instruction: &str) -> String
```
e.g. "Original question:\n{q}\n\nCurrent answer:\n{a}\n\nRevise the answer per
this instruction:\n{instruction}".

### 5. Keymap — `src/input/keymap.rs`

`handle_journal_key` branches on which card is open (checked before the existing
ask-card intercept):
- **Edit card open:** `Tab`/`ISO_Left_Tab` → `toggle_edit_focus`; `Ctrl+Return`
  → `submit_edit_save`; **`Alt+Return` → `submit_edit_rewrite`**; `Escape` →
  `close_edit_card`; when the focused field is an input, typed keys fall through
  to it.
- **Ask card open:** unchanged (existing `ask_card_intercept`).
- `E` → `begin_edit` (now the new path).

`Alt+Return` is added in the journal edit-card branch only, NOT in the shared
`ask_card_intercept` (gloss/synopsis ask cards stay untouched). RPD: `Return`
with the alt modifier; Enter is not a remapped key, but the GTK key name
(`Return`) and `is_alt` will be confirmed against `~/utono/rpd`.

### 6. Ctrl+/ overlay — `src/ui/keybinds_overlay.rs`

Update the `E` description (the `journal tog` describe arm mentions "E edits the
current page's question") to reflect the new direct-edit + Alt+Enter rewrite
behavior. Run the `update-cairo-keybinds-overlay` three-pass cross-reference.

## Testing

- **Unit (`cargo test --bins`):**
  - `update_journal_page` round-trip (exists); add one asserting a Q+A update by
    id leaves scope/div1/div2 unchanged.
  - `rewrite_user_message(q, a, instr)` produces the expected structured prompt.
- **Runtime (user-verified):** pre-fill correctness, three-way Tab, Ctrl+Enter
  saves hand-edits to `lit.db` without a Claude call, Alt+Enter with an
  instruction rewrites the answer, empty-instruction Alt+Enter saves as-is with
  the toast, and the edit card does not occlude/clip the page. GUI criteria, per
  the no-`cargo run` rule.

## Out of scope (YAGNI)

- Editing passage-page source text.
- Changing the `A` "ask a new question" flow or the gloss/synopsis ask cards.
- Multi-page batch edit.
- (Separately tracked, not part of this feature: the journal overlay
  "too-tall after creating a Q&A" sizing bug.)
