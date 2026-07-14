# `E` Journal Edit Card Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the journal overlay's `E` action (which re-asks Claude and overwrites the answer) with a dedicated 3-field edit card: hand-edit the stored Question + Answer and save straight to `lit.db` with Ctrl+Enter, or type a rewrite instruction and have Claude revise the answer with Alt+Enter.

**Architecture:** A new `JournalEditCard` widget (three pre-fillable TextView fields) is appended to the journal overlay's container and opened/closed through a new generic shrink/restore pair on `AskCardHost` (reusing the occlusion-prevention scroll mechanism). `begin_edit` is rewritten to pre-fill the card from the current page; two new handlers (`submit_edit_save`, `submit_edit_rewrite`) write via the existing `update_journal_page`. The old `JournalPromptMode::Edit` path is removed and `ask_claude` is simplified to its always-save-new behavior.

**Tech Stack:** Rust, GTK4 (gtk4-rs), SQLite (rusqlite), sourceview5.

**Spec:** `docs/superpowers/specs/2026-06-28-journal-edit-card-design.md`

## Global Constraints

- Do NOT run `cargo run` — the user runs the app. Verify with `cargo build` and `cargo test --bins`; GUI behavior is user-verified.
- The DB write uses the existing `update_journal_page(conn, id, question, answer, claude_model)` — it must keep touching ONLY question/answer/claude_model/timestamp, never scope/div1/div2.
- On a pure hand-edit (Ctrl+Enter) the page's EXISTING `claude_model` is preserved (a hand-edit does not relabel provenance). The rewrite path stores the model that produced the revision.
- Empty rewrite instruction on Alt+Enter → behave like save-as-is, with toast `"No rewrite instruction — saved as-is"`. No Claude call.
- The `A` "ask a new question" flow and the gloss/synopsis ask cards stay untouched. `Alt+Return` is journal-edit-only (NOT added to the shared `ask_card_intercept`).
- RPD note: confirm key names against `~/utono/rpd` when touching keybinds. Enter = GTK `Return`; the edit submit keys are `Ctrl+Return` (save) and `Alt+Return` (rewrite).
- Keymap changes are mirrored in the Ctrl+/ overlay (`keybinds_overlay.rs`).
- End commit messages with the standard Co-Authored-By / Claude-Session trailer used in this repo.

---

## File Structure

- `src/ui/ask_card.rs` — add a generic `open_for_natural_height` / `close_to_closed_height` pair to `AskCardHost` (Task 1).
- `src/ui/journal_edit_card.rs` — NEW: the `JournalEditCard` widget + `EditField` enum (Task 2).
- `src/ui/mod.rs` — register the new module (Task 2).
- `src/ui/journal_overlay.rs` — construct/append the edit card; `open_edit_card`/`close_edit_card`/`edit_is_open`/`edit_focus`/`toggle_edit_focus`/`take_edit_fields`; font it (Task 3).
- `src/input/actions/journal.rs` — rewrite `begin_edit`; add `submit_edit_save`, `submit_edit_rewrite`, `rewrite_user_message`; remove the `JournalPromptMode::Edit` path from `ask_claude`/`submit_prompt` (Tasks 4, 5).
- `src/app/mod.rs` — drop the `Edit` variant from `JournalPromptMode` (Task 5).
- `src/input/keymap.rs` — branch `handle_journal_key` on the edit card; wire Tab/Ctrl+Return/Alt+Return/Escape (Task 6).
- `src/ui/keybinds_overlay.rs` — update the `E` description (Task 7).

---

## Task 1: `AskCardHost` generic shrink/restore for an external card

**Files:**
- Modify: `src/ui/ask_card.rs` (add two public methods to `impl AskCardHost`, after `close` ~line 313)

**Interfaces:**
- Produces: `pub fn open_for_natural_height(&self, natural_h: i32)` and `pub fn close_to_closed_height(&self)` on `AskCardHost`.

The journal edit card is a separate widget (not the hosted `AskCard`), but it must shrink the same scroll viewport so it doesn't occlude the page — exactly what `AskCardHost::open` does for the ask card. `open` is hard-wired to the internal `ask` card's natural height and hides the footer; these two methods generalize the scroll shrink to an arbitrary card height and let the caller manage footer visibility.

- [ ] **Step 1: Add the two methods**

In `src/ui/ask_card.rs`, after the `close` method (~line 313) in `impl AskCardHost`, add:

```rust
    /// Shrink the scroll viewport to make room for an EXTERNAL card of natural
    /// height `natural_h` (the journal edit card), mirroring `open` but without
    /// touching the internal ask card or the footer (the caller manages those).
    /// Open height = card_height − fixed_chrome − natural_h.
    pub fn open_for_natural_height(&self, natural_h: i32) {
        let scroll_h =
            (self.card_height.get() - self.fixed_chrome_h.get() - natural_h).max(80);
        self.pin_scroll_height(scroll_h);
        self.recompute_now_and_idle();
    }

    /// Restore the scroll's stored CLOSED height after an external card closes
    /// (mirrors `close`'s restore, without touching the ask card or footer).
    pub fn close_to_closed_height(&self) {
        self.pin_scroll_height(self.closed_scroll_h.get().max(80));
        self.recompute_now_and_idle();
    }
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: builds (the new methods are unused until Task 3 — `pub` methods don't warn).

- [ ] **Step 3: Commit**

```bash
git add src/ui/ask_card.rs
git commit -m "feat(ask-card): AskCardHost shrink/restore for an external card

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 2: The `JournalEditCard` widget

**Files:**
- Create: `src/ui/journal_edit_card.rs`
- Modify: `src/ui/mod.rs` (add `pub mod journal_edit_card;` next to `pub mod journal_overlay;`)

**Interfaces:**
- Produces:
  - `pub enum EditField { Question, Answer, Instruction }` (derives `Clone, Copy, PartialEq, Eq`)
  - `pub struct JournalEditCard` with: `pub fn new(text_margins: i32, return_focus: &impl IsA<gtk4::Widget>) -> Self`, `pub fn container(&self) -> &gtk4::Box`, `pub fn open(&self, question: &str, answer: &str, card_width: i32)`, `pub fn close(&self)`, `pub fn is_open(&self) -> bool`, `pub fn cycle_focus(&self)`, `pub fn focused_field(&self) -> EditField`, `pub fn take(&self) -> (String, String, String)`, `pub fn views(&self) -> [&gtk4::TextView; 3]`
- Consumes: `crate::ui::card_side_margin`.

Modeled on `AskCard` (`src/ui/ask_card.rs`): same CSS classes (`ask-card`, `gloss-header`, `gloss-text`, `ask-input`, `ask-hint`, `card-focused`, `card-dimmed`), same `card_side_margin` inset on open, but three labeled fields and three-way focus cycling.

- [ ] **Step 1: Create the file**

Create `src/ui/journal_edit_card.rs`:

```rust
use gtk4::prelude::*;
use gtk4::{Align, Label, ScrolledWindow, TextView};
use std::cell::Cell;

/// Which of the three edit fields holds focus.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditField {
    Question,
    Answer,
    Instruction,
}

/// A dedicated edit card for the journal overlay's `E` action: three stacked,
/// pre-fillable fields (Question, Answer, Rewrite-instruction) with three-way
/// Tab cycling. Modeled on `AskCard` but multi-field and pre-fillable.
pub struct JournalEditCard {
    container: gtk4::Box,
    question: TextView,
    answer: TextView,
    instruction: TextView,
    focus: Cell<EditField>,
    return_focus: gtk4::Widget,
}

/// Build one labeled field: a header label + a scrolled, editable TextView.
/// `min_h`/`max_h` bound the scroller. Returns (the field's vbox, the view).
fn build_field(label_text: &str, min_h: i32, max_h: i32) -> (gtk4::Box, TextView) {
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    let label = Label::new(Some(label_text));
    label.add_css_class("gloss-header");
    label.set_halign(Align::Start);
    label.set_margin_start(16);
    label.set_margin_top(8);
    vbox.append(&label);

    let scrolled = ScrolledWindow::new();
    scrolled.set_min_content_height(min_h);
    scrolled.set_max_content_height(max_h);
    scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scrolled.set_margin_start(16);
    scrolled.set_margin_end(16);
    scrolled.set_margin_top(4);
    scrolled.set_margin_bottom(4);

    let view = TextView::new();
    view.set_editable(true);
    view.set_cursor_visible(true);
    view.set_wrap_mode(gtk4::WrapMode::Word);
    view.set_top_margin(6);
    view.set_bottom_margin(6);
    view.set_left_margin(6);
    view.set_right_margin(6);
    view.add_css_class("gloss-text");
    view.add_css_class("ask-input");
    scrolled.set_child(Some(&view));
    vbox.append(&scrolled);

    (vbox, view)
}

impl JournalEditCard {
    pub fn new(text_margins: i32, return_focus: &impl IsA<gtk4::Widget>) -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("ask-card");
        container.set_margin_top(14);
        container.set_margin_start(text_margins);
        container.set_margin_end(text_margins);
        container.set_margin_bottom(14);

        let title = Label::new(Some("Edit Q&A"));
        title.add_css_class("gloss-header");
        title.set_halign(Align::Start);
        title.set_margin_start(16);
        title.set_margin_top(12);
        container.append(&title);

        let (q_box, question) = build_field("Question", 60, 120);
        let (a_box, answer) = build_field("Answer", 140, 280);
        let (i_box, instruction) = build_field("Rewrite instruction (Alt+Enter)", 50, 100);
        container.append(&q_box);
        container.append(&a_box);
        container.append(&i_box);

        let hint = Label::new(Some(
            "Tab cycle  \u{00b7}  Ctrl+Enter save  \u{00b7}  Alt+Enter rewrite  \u{00b7}  Esc cancel",
        ));
        hint.add_css_class("ask-hint");
        hint.set_halign(Align::Center);
        hint.set_margin_bottom(10);
        container.append(&hint);

        container.set_visible(false);

        Self {
            container,
            question,
            answer,
            instruction,
            focus: Cell::new(EditField::Question),
            return_focus: return_focus.clone().upcast(),
        }
    }

    pub fn container(&self) -> &gtk4::Box {
        &self.container
    }

    /// The three views, for font application by the host overlay.
    pub fn views(&self) -> [&TextView; 3] {
        [&self.question, &self.answer, &self.instruction]
    }

    /// Reveal the card, pre-fill Question + Answer, clear the instruction,
    /// re-inset to card_width/4, and focus the Question field.
    pub fn open(&self, question: &str, answer: &str, card_width: i32) {
        self.question.buffer().set_text(question);
        self.answer.buffer().set_text(answer);
        self.instruction.buffer().set_text("");
        if card_width > 0 {
            let margin = crate::ui::card_side_margin(card_width);
            self.container.set_margin_start(margin);
            self.container.set_margin_end(margin);
        }
        self.container.set_visible(true);
        self.set_focus(EditField::Question);
    }

    pub fn close(&self) {
        self.container.set_visible(false);
        self.container.remove_css_class("card-focused");
        self.container.remove_css_class("card-dimmed");
        if self.question.has_focus() || self.answer.has_focus() || self.instruction.has_focus() {
            let _ = self.return_focus.grab_focus();
        }
        self.focus.set(EditField::Question);
    }

    pub fn is_open(&self) -> bool {
        self.container.is_visible()
    }

    pub fn focused_field(&self) -> EditField {
        self.focus.get()
    }

    /// Question -> Answer -> Instruction -> Question.
    pub fn cycle_focus(&self) {
        if !self.is_open() {
            return;
        }
        let next = match self.focus.get() {
            EditField::Question => EditField::Answer,
            EditField::Answer => EditField::Instruction,
            EditField::Instruction => EditField::Question,
        };
        self.set_focus(next);
    }

    fn set_focus(&self, field: EditField) {
        self.focus.set(field);
        self.container.add_css_class("card-focused");
        self.container.remove_css_class("card-dimmed");
        let view = match field {
            EditField::Question => &self.question,
            EditField::Answer => &self.answer,
            EditField::Instruction => &self.instruction,
        };
        view.grab_focus();
    }

    /// Read (question, answer, instruction); does NOT clear (the caller decides
    /// whether the edit committed). Trims trailing newline GTK may leave.
    pub fn take(&self) -> (String, String, String) {
        let read = |v: &TextView| {
            let b = v.buffer();
            b.text(&b.start_iter(), &b.end_iter(), false).to_string()
        };
        (read(&self.question), read(&self.answer), read(&self.instruction))
    }
}
```

- [ ] **Step 2: Register the module**

In `src/ui/mod.rs`, next to `pub mod journal_overlay;`, add:

```rust
pub mod journal_edit_card;
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: builds (unused-until-Task-3 warnings on `JournalEditCard` methods are fine).

- [ ] **Step 4: Commit**

```bash
git add src/ui/journal_edit_card.rs src/ui/mod.rs
git commit -m "feat(journal): JournalEditCard widget (3 pre-fillable fields)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 3: Host the edit card in the journal overlay

**Files:**
- Modify: `src/ui/journal_overlay.rs` (import; struct field; construct/append in `new`; new public methods; font the edit views in `apply_font`)

**Interfaces:**
- Consumes: `JournalEditCard`, `EditField` (Task 2); `AskCardHost::open_for_natural_height`/`close_to_closed_height` (Task 1).
- Produces (on `JournalOverlay`): `open_edit_card(&self, question: &str, answer: &str)`, `close_edit_card(&self)`, `edit_is_open(&self) -> bool`, `edit_focus(&self) -> EditField`, `toggle_edit_focus(&self)`, `take_edit_fields(&self) -> (String, String, String)`.

- [ ] **Step 1: Import + struct field**

In `src/ui/journal_overlay.rs`, near the top imports (after the `journal_block` use, ~line 4), add:

```rust
use crate::ui::journal_edit_card::{EditField, JournalEditCard};
```

In the `JournalOverlay` struct (after `ask_host: AskCardHost,` ~line 38), add:

```rust
    edit_card: JournalEditCard,
```

- [ ] **Step 2: Construct + append in `new`**

In `new`, after the `ask_host` is built (~line 196) and before the `Self { ... }` literal, add:

```rust
        let edit_card = JournalEditCard::new(text_margins as i32, &view);
        container.append(edit_card.container());
```

Add `edit_card,` to the `Self { ... }` literal (after `ask_host,`).

- [ ] **Step 3: Add the public methods**

In `impl JournalOverlay`, after `take_ask_text` (~line 622), add:

```rust
    pub fn edit_is_open(&self) -> bool {
        self.edit_card.is_open()
    }

    pub fn edit_focus(&self) -> EditField {
        self.edit_card.focused_field()
    }

    pub fn toggle_edit_focus(&self) {
        self.edit_card.cycle_focus();
    }

    pub fn take_edit_fields(&self) -> (String, String, String) {
        self.edit_card.take()
    }

    /// Open the edit card pre-filled with the current page's Q & A. Hides the
    /// nav footer (the edit card carries its own hint) and shrinks the scroll so
    /// the card doesn't occlude the page (mirrors open_ask_card).
    pub fn open_edit_card(&self, question: &str, answer: &str) {
        let (card_width, _) = self.last_card_size.get();
        self.edit_card.open(question, answer, card_width);
        self.footer_container.set_visible(false);
        self.apply_font();
        let (_, edit_h) = self.edit_card.container().preferred_size();
        self.ask_host.open_for_natural_height(edit_h.height());
    }

    pub fn close_edit_card(&self) {
        self.edit_card.close();
        self.footer_container.set_visible(true);
        self.ask_host.close_to_closed_height();
    }
```

- [ ] **Step 4: Font the edit views**

In `apply_font` (~line 551), change the iterated view list from:

```rust
        for view in [&self.view, self.ask_host.input()] {
```

to include the three edit views:

```rust
        let edit_views = self.edit_card.views();
        for view in [&self.view, self.ask_host.input(), edit_views[0], edit_views[1], edit_views[2]] {
```

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: builds clean (only pre-existing dead-code warnings; the new methods are used in Tasks 4-6).

- [ ] **Step 6: Commit**

```bash
git add src/ui/journal_overlay.rs
git commit -m "feat(journal): host the edit card in JournalOverlay

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 4: Rewrite `begin_edit` + add the two submit handlers

**Files:**
- Modify: `src/input/actions/journal.rs` (rewrite `begin_edit`; add `rewrite_user_message`, `submit_edit_save`, `submit_edit_rewrite`)

**Interfaces:**
- Consumes: `JournalOverlay::open_edit_card`/`close_edit_card`/`take_edit_fields` (Task 3); `crate::db::journal::update_journal_page`; `crate::db::queries::open_db_rw`; `crate::input::actions::claude_bridge::run_claude_request`; `crate::gloss::JOURNAL_QA_PROMPT`.
- Produces: `pub(crate) fn submit_edit_save(state)`, `pub(crate) fn submit_edit_rewrite(state)`, `fn rewrite_user_message(question, answer, instruction) -> String`.

- [ ] **Step 1: Write the failing test (pure helper)**

In the `#[cfg(test)] mod tests` block at the bottom of `src/input/actions/journal.rs`, add:

```rust
#[test]
fn rewrite_user_message_includes_all_three_parts() {
    let msg = rewrite_user_message("Who is Esther?", "She narrates half the book.", "Add her surname.");
    assert!(msg.contains("Who is Esther?"));
    assert!(msg.contains("She narrates half the book."));
    assert!(msg.contains("Add her surname."));
    // The instruction must come after the current answer (revise-this shape).
    let a_pos = msg.find("She narrates half the book.").unwrap();
    let i_pos = msg.find("Add her surname.").unwrap();
    assert!(i_pos > a_pos, "instruction should follow the current answer");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins rewrite_user_message_includes_all_three -- --nocapture`
Expected: FAIL — `cannot find function rewrite_user_message`.

- [ ] **Step 3: Add `rewrite_user_message` and rewrite `begin_edit`**

In `src/input/actions/journal.rs`, replace the existing `begin_edit` (the current body opens the ask card in Edit mode, ~lines 333-339) with:

```rust
/// Build the user message for an Alt+Enter rewrite: the question, the current
/// answer, and the user's revision instruction, in "revise this answer" shape.
fn rewrite_user_message(question: &str, answer: &str, instruction: &str) -> String {
    format!(
        "Original question:\n{}\n\nCurrent answer:\n{}\n\nRevise the answer per this instruction (return only the revised answer):\n{}",
        question, answer, instruction,
    )
}

/// `E` in the journal overlay: open the dedicated edit card pre-filled with the
/// current page's stored Question and Answer. No-op if the band is empty.
pub(crate) fn begin_edit(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    let Some(page) = s.journal.pages.get(s.journal.page_index) else {
        return;
    };
    let (q, a) = (page.question.clone(), page.answer.clone());
    s.journal_overlay.open_edit_card(&q, &a);
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins rewrite_user_message_includes_all_three -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Add the two submit handlers**

Append to `src/input/actions/journal.rs` (after `begin_edit`):

```rust
/// Ctrl+Enter in the edit card: save the hand-edited Question + Answer straight
/// to lit.db (no Claude). Preserves the page's existing claude_model. Closes the
/// card and re-renders.
pub(crate) fn submit_edit_save(state: &Rc<RefCell<AppState>>) {
    let (question, answer, _instr) = state.borrow().journal_overlay.take_edit_fields();
    let mut s = state.borrow_mut();
    let Some(page) = s.journal.pages.get(s.journal.page_index) else {
        s.journal_overlay.close_edit_card();
        return;
    };
    let (id, model) = (page.id, page.claude_model.clone());
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        if let Err(e) =
            crate::db::journal::update_journal_page(&conn, id, question.trim(), answer.trim(), &model)
        {
            crate::logging::log(&format!("JOURNAL: edit-save failed: {}", e));
        }
    }
    s.journal_overlay.close_edit_card();
    render_current(&mut s);
    crate::ui::toast::show_transient(&s.chapter_toast, "Saved", 2);
}

/// Alt+Enter in the edit card: ask Claude to revise the answer per the
/// instruction, then save the revision (with the edited question). Empty
/// instruction -> fall back to save-as-is with a toast.
pub(crate) fn submit_edit_rewrite(state: &Rc<RefCell<AppState>>) {
    let (question, answer, instruction) = state.borrow().journal_overlay.take_edit_fields();

    // Empty instruction -> behave like save-as-is.
    if instruction.trim().is_empty() {
        {
            let mut s = state.borrow_mut();
            let page = s.journal.pages.get(s.journal.page_index);
            if let Some(page) = page {
                let (id, model) = (page.id, page.claude_model.clone());
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let _ = crate::db::journal::update_journal_page(
                        &conn, id, question.trim(), answer.trim(), &model,
                    );
                }
            }
            s.journal_overlay.close_edit_card();
            render_current(&mut s);
            crate::ui::toast::show_transient(
                &s.chapter_toast, "No rewrite instruction \u{2014} saved as-is", 3,
            );
        }
        return;
    }

    // Capture the page id + model, then call Claude.
    let (edit_id, model) = {
        let s = state.borrow();
        match s.journal.pages.get(s.journal.page_index) {
            Some(p) => (p.id, s.config.claude_model.clone().max(p.claude_model.clone())),
            None => return,
        }
    };
    let question_owned = question.clone();
    let model_for_db = model.clone();
    let user_msg = rewrite_user_message(&question, &answer, &instruction);

    {
        let s = state.borrow();
        s.journal_overlay.close_edit_card();
        crate::ui::toast::show_transient(&s.chapter_toast, "Rewriting\u{2026}", 2);
    }

    crate::input::actions::claude_bridge::run_claude_request(
        state,
        crate::gloss::JOURNAL_QA_PROMPT.to_string(),
        user_msg,
        model,
        move |st, revised| {
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                if let Err(e) = crate::db::journal::update_journal_page(
                    &conn, edit_id, &question_owned, &revised, &model_for_db,
                ) {
                    crate::logging::log(&format!("JOURNAL: edit-rewrite save failed: {}", e));
                }
            }
            let mut s = st.borrow_mut();
            render_current(&mut s);
            crate::ui::toast::show_transient(&s.chapter_toast, "Rewritten", 2);
        },
        move |st, msg| {
            let s = st.borrow();
            crate::ui::toast::show_transient(&s.chapter_toast, msg, 4);
        },
    );
}
```

NOTE: `s.config.claude_model.clone().max(p.claude_model.clone())` is a String `.max` (lexicographic) used only to pick a non-empty model id when the stored one is empty; if the implementer finds this unclear, prefer an explicit `if p.claude_model.is_empty() { config } else { stored }`. Either is acceptable — the goal is "store a real model id for the revision."

- [ ] **Step 6: Build + run the test**

Run: `cargo build && cargo test --bins rewrite_user_message_includes_all_three -- --nocapture`
Expected: build clean; test PASSES. (Other journal handlers may still reference removed Edit-mode pieces — that is fixed in Task 5; if build fails ONLY with `JournalPromptMode::Edit`-related errors in `journal.rs` `submit_prompt`/`ask_claude`, proceed to Task 5; any OTHER error is a real bug to fix now.)

- [ ] **Step 7: Commit**

```bash
git add src/input/actions/journal.rs
git commit -m "feat(journal): edit-card begin_edit + save/rewrite submit handlers

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 5: Remove the old `JournalPromptMode::Edit` path

**Files:**
- Modify: `src/input/actions/journal.rs` (simplify `submit_prompt` + `ask_claude` to always-save-new; drop the `prompt_mode = Edit` set, already gone after Task 4's `begin_edit` rewrite)
- Modify: `src/app/mod.rs` (drop `Edit` from `JournalPromptMode`)

**Interfaces:**
- Produces: `JournalPromptMode` with only `Ask`.

After Task 4, the only remaining `Edit` references are in `ask_claude` (the edit branch + `edit_id` + the match guard) and `submit_prompt` (which reads `prompt_mode`). `ask_claude` is now only ever called for a NEW page (the `A`/passage ask), so the Edit branch is dead.

- [ ] **Step 1: Simplify `submit_prompt`**

In `src/input/actions/journal.rs`, `submit_prompt` currently reads `(question, mode)` and passes `mode` to `ask_claude`. Change it to drop the mode:

```rust
pub(crate) fn submit_prompt(state: &Rc<RefCell<AppState>>) {
    let question = state.borrow().journal_overlay.take_ask_text();
    close_prompt(state);
    if question.trim().is_empty() {
        return;
    }
    ask_claude(state, &question);
}
```

- [ ] **Step 2: Simplify `ask_claude` to always save a NEW page**

Change `ask_claude`'s signature to drop `mode: JournalPromptMode`:

```rust
fn ask_claude(state_rc: &Rc<RefCell<AppState>>, question: &str) {
```

Remove the `edit_id` block (the `let edit_id: i64 = if mode == ... ` lines) entirely. In the DB-write closure, replace the `match (&band, mode == JournalPromptMode::Edit && edit_id >= 0) { (_, true) => update..., ... }` with a match on `&band` only (the three save arms, dropping the `(_, true)` update arm):

```rust
                let write_result = match &band {
                    JournalBand::Work => {
                        crate::db::journal::save_journal_page(
                            &conn, &work_abbrev,
                            crate::app::JOURNAL_WORK_DIV.0, crate::app::JOURNAL_WORK_DIV.1,
                            &question_owned, &answer, &model_for_db, "work",
                        )
                        .map(|_| ())
                    }
                    JournalBand::Scene(d1, d2) => {
                        crate::db::journal::save_journal_page(
                            &conn, &work_abbrev, *d1, *d2,
                            &question_owned, &answer, &model_for_db, "scene",
                        )
                        .map(|_| ())
                    }
                    JournalBand::Passage { div1, div2, start, end } => {
                        crate::db::journal::save_passage_page(
                            &conn, &work_abbrev, *div1, *div2, start, end,
                            &passage_source_text, &question_owned, &answer, &model_for_db,
                        )
                        .map(|_| ())
                    }
                };
```

And replace the `new_index` computation (which branched on Edit) with the always-new form:

```rust
            let new_index = pages.len().saturating_sub(1);
```

- [ ] **Step 3: Drop the `Edit` variant**

In `src/app/mod.rs`, change the `JournalPromptMode` enum (~line 136) from:

```rust
pub enum JournalPromptMode {
    Ask,
    Edit,
}
```

to:

```rust
pub enum JournalPromptMode {
    Ask,
}
```

- [ ] **Step 4: Build + full bins test**

Run: `cargo build && cargo test --bins`
Expected: build clean (no `JournalPromptMode::Edit` references remain); all bins tests pass (the existing count + the new `rewrite_user_message` test). If the build flags an unused `JournalPromptMode` import or an unreachable match arm, clean it up.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/journal.rs src/app/mod.rs
git commit -m "refactor(journal): drop JournalPromptMode::Edit (replaced by edit card)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 6: Keymap — route the edit card's keys

**Files:**
- Modify: `src/input/keymap.rs` (`handle_journal_key`: add an edit-card intercept before the ask-card intercept)

**Interfaces:**
- Consumes: `JournalOverlay::edit_is_open`/`edit_focus`/`toggle_edit_focus`/`close_edit_card` (Task 3); `submit_edit_save`/`submit_edit_rewrite` (Task 4); `EditField`.

`handle_journal_key` receives `is_alt` already. It must intercept the edit card's keys BEFORE the existing `ask_card_intercept` (the two cards are never open at once, but the edit card is checked first). `Ctrl+Return` saves, `Alt+Return` rewrites, `Tab` cycles, `Esc` closes, and when a field is focused, typed keys fall through to it.

- [ ] **Step 1: Add the edit-card intercept**

In `src/input/keymap.rs`, at the very top of `handle_journal_key` (before the existing `ask_card_intercept` block, ~line 676), add:

```rust
    // ---- Edit card (E) intercepts Tab / Ctrl+Enter / Alt+Enter / Escape ----
    if state.borrow().journal_overlay.edit_is_open() {
        match key_name {
            "Tab" | "ISO_Left_Tab" => {
                state.borrow().journal_overlay.toggle_edit_focus();
                return true;
            }
            "Return" if is_ctrl => {
                crate::input::actions::journal::submit_edit_save(state);
                return true;
            }
            "Return" if is_alt => {
                crate::input::actions::journal::submit_edit_rewrite(state);
                return true;
            }
            "Escape" => {
                state.borrow().journal_overlay.close_edit_card();
                return true;
            }
            // Any other key: let it fall through to the focused editable field.
            _ => return false,
        }
    }
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 3: Full bins test**

Run: `cargo test --bins`
Expected: PASS (unchanged count + the Task 4 test).

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat(journal): route edit-card keys (Tab/Ctrl+Enter/Alt+Enter/Esc)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 7: Update the Ctrl+/ overlay description

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (the `journal tog` describe arm's mention of `E`)

The `journal tog` describe blurb (in `describe()`, ~line 355-370) currently says "E edits the current page's question". Update it to describe the new edit card.

- [ ] **Step 1: Update the `E` mention**

In `src/ui/keybinds_overlay.rs`, in the `"journal tog"` describe arm, find the phrase "E edits the current page's question" and replace it with:

```
E opens an edit card (Question + Answer pre-filled, plus a rewrite-instruction \
field): Ctrl+Enter saves your hand-edits straight to lit.db, Alt+Enter sends \
the Q&A + instruction to Claude and saves the revised answer
```

- [ ] **Step 2: Cross-reference pass**

Invoke the `update-cairo-keybinds-overlay` skill and run its three-pass cross-reference for the `j`/`J` key (the journal binds live there): confirm no blank slot, no wrong label, every label has a `describe()` arm. (No new label/keycap is added — this is a description text change — so the passes should be clean.)

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 4: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(keybinds): describe the journal E edit-card behavior

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 8: Final verification + user runtime check

**Files:** none (verification only)

- [ ] **Step 1: Full build + tests + clippy**

Run:
```bash
cargo build && cargo test --bins && cargo clippy
```
Expected: build clean, all `--bins` tests pass, no new clippy warnings on the changed files.

- [ ] **Step 2: Update `ac`**

Update `CLAUDE-activeContext.md`: record the E edit-card feature (3 fields, Ctrl+Enter save / Alt+Enter rewrite, JournalPromptMode::Edit removed), the new file, that logic tests pass, and that the rendered run is pending user verification. (`ac` is gitignored — do NOT commit it.)

- [ ] **Step 3: Ask the user to verify on a rendered run**

The acceptance criteria are visual/runtime. Per the no-`cargo run` rule, ask the user to: open a work's journal, land on a page with a Q&A, press `E`, confirm the card is pre-filled with the stored Q & A; edit the answer and press `Ctrl+Enter`, confirm it saves to lit.db (reopen to check) with no Claude call; press `E` again, type a rewrite instruction, press `Alt+Enter`, confirm Claude revises the answer; press `E`, leave the instruction empty, `Alt+Enter`, confirm it saves as-is with the toast. Report results; debug from the dev log on failure.

---

## Self-Review

**Spec coverage:**
- Direct-edit Q + A, save to lit.db, no Claude (Ctrl+Enter) → Task 4 `submit_edit_save`. ✓
- Alt+Enter rewrite via Claude → Task 4 `submit_edit_rewrite` + `rewrite_user_message`. ✓
- Empty instruction → save-as-is + toast → Task 4 `submit_edit_rewrite` early branch. ✓
- Three pre-filled, Tab-cycled fields → Task 2 `JournalEditCard`. ✓
- Dedicated card, ask card untouched → Task 2 (new widget); Task 5 keeps `A` flow via `ask_claude`. ✓
- Occlusion-safe (shrink scroll) → Task 1 host methods + Task 3 `open_edit_card`. ✓
- Remove old Edit path / drop `JournalPromptMode::Edit` → Task 5. ✓
- Keymap (Tab/Ctrl+Enter/Alt+Enter/Esc), journal-only Alt+Enter → Task 6. ✓
- Ctrl+/ overlay description → Task 7. ✓
- Tests (rewrite_user_message; existing update_journal_page round-trip) → Task 4 + existing. ✓
- claude_model provenance rule → Task 4 (`submit_edit_save` preserves stored model; rewrite stores `model_for_db`). ✓

**Placeholder scan:** No TBD/TODO; every code step shows full code; commands have expected output. ✓

**Type consistency:**
- `JournalEditCard`/`EditField` + method set — defined Task 2, used Tasks 3, 6. ✓
- `open_for_natural_height`/`close_to_closed_height` — defined Task 1, used Task 3. ✓
- `open_edit_card`/`close_edit_card`/`edit_is_open`/`edit_focus`/`toggle_edit_focus`/`take_edit_fields` — defined Task 3, used Tasks 4, 6. ✓
- `submit_edit_save`/`submit_edit_rewrite`/`rewrite_user_message` — defined Task 4, used Task 6. ✓
- `update_journal_page(conn, id, question, answer, model)` — existing, used Task 4. ✓
- `JournalPromptMode` (Ask only) — Task 5; `submit_prompt`/`ask_claude` no longer take a mode. ✓
