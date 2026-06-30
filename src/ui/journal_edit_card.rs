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
    /// Per-field "already focused since this open" flags ([Question, Answer,
    /// Instruction]). The FIRST time Tab lands on a field its cursor is moved to
    /// the start; later Tabs back to it leave the cursor where the user left it.
    /// Reset to all-false on `open`.
    visited: Cell<[bool; 3]>,
    return_focus: gtk4::Widget,
}

impl EditField {
    /// Index into the `visited` array.
    fn index(self) -> usize {
        match self {
            EditField::Question => 0,
            EditField::Answer => 1,
            EditField::Instruction => 2,
        }
    }
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

        let title = Label::new(Some("Edit this Q&A"));
        title.add_css_class("gloss-header");
        title.set_halign(Align::Start);
        title.set_margin_start(16);
        title.set_margin_top(12);
        container.append(&title);

        let (q_box, question) = build_field("Question", 60, 120);
        let (a_box, answer) = build_field("Answer", 140, 280);
        let (i_box, instruction) = build_field("Rewrite instruction (optional)", 50, 100);
        container.append(&q_box);
        container.append(&a_box);
        container.append(&i_box);

        let hint = Label::new(Some(
            "Ctrl+Enter submit",
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
            visited: Cell::new([false; 3]),
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
        // Fresh open: no field has been focused yet, so the first Tab landing on
        // each field will move its cursor to the start.
        self.visited.set([false; 3]);
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
        // On the FIRST focus of this field since `open`, move the cursor to the
        // start so the user reads/edits from the beginning of the pre-filled
        // text. On later Tabs back to the field, leave the cursor where it was.
        let mut visited = self.visited.get();
        if !visited[field.index()] {
            let buffer = view.buffer();
            buffer.place_cursor(&buffer.start_iter());
            visited[field.index()] = true;
            self.visited.set(visited);
        }
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
