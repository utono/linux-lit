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
    /// The Answer field's scroller, kept so `open` can PIN its height to the
    /// space freed when the card fills the parent (the expanding field).
    answer_scrolled: ScrolledWindow,
    /// The card's fixed (Answer-independent) chrome widgets, measured to size the
    /// Answer field: the title, the Question field vbox, the Instruction field
    /// vbox, and the hint. Their heights do NOT depend on the (variable-length)
    /// Answer text, so summing their `preferred_size()` is race-free — unlike
    /// measuring the whole container after the long Answer was set (the approach
    /// that raced; see docs/troubleshooting/journal-edit-card-sizing.md).
    title: Label,
    question_box: gtk4::Box,
    instruction_box: gtk4::Box,
    hint: Label,
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
fn build_field(label_text: &str, min_h: i32, max_h: i32, expand: bool) -> (gtk4::Box, TextView, ScrolledWindow) {
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    // An expanding field (the Answer) takes all the spare height the card has,
    // so a card grown to nearly its parent's height gives that room to editing.
    if expand {
        vbox.set_vexpand(true);
    }

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
    if expand {
        // Fill the vexpanded vbox's spare height (max_content_height alone caps
        // growth; vexpand lets the scroller stretch past it).
        scrolled.set_vexpand(true);
    }

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

    (vbox, view, scrolled)
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

        let (q_box, question, _) = build_field("Question", 60, 120, false);
        let (a_box, answer, answer_scrolled) = build_field("Answer", 140, 4000, true);
        let (i_box, instruction, _) = build_field("Rewrite instruction (optional)", 50, 100, false);
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
            answer_scrolled,
            title,
            question_box: q_box,
            instruction_box: i_box,
            hint,
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

    /// Inner padding each field adds INSIDE the card container, between the
    /// container edge and the editable text: the scroller margin (16) + the
    /// TextView's left/right margin (6). `open` subtracts this from the requested
    /// text inset so the field text lines up with the page text at the same x.
    const FIELD_INNER_PAD: i32 = 16 + 6;

    /// Reveal the card, pre-fill Question + Answer, clear the instruction, inset
    /// it so the field text wraps at the SAME width as the journal page text
    /// (`text_inset` is the page's left/right margin), and focus Question.
    pub fn open(&self, question: &str, answer: &str, text_inset: i32) {
        self.question.buffer().set_text(question);
        self.answer.buffer().set_text(answer);
        self.instruction.buffer().set_text("");
        // The field text sits FIELD_INNER_PAD inside the container, so set the
        // container margin to text_inset − that pad: the editable text then starts
        // at exactly text_inset (matching the page's left margin → same wrap).
        let margin = (text_inset - Self::FIELD_INNER_PAD).max(0);
        self.container.set_margin_start(margin);
        self.container.set_margin_end(margin);
        self.container.set_visible(true);
        // Fresh open: no field has been focused yet, so the first Tab landing on
        // each field will move its cursor to the start.
        self.visited.set([false; 3]);
        self.set_focus(EditField::Question);
    }

    /// Summed height of the card's FIXED chrome — everything except the Answer
    /// scroller: the title, the Question field, the Instruction field, the hint,
    /// and the container's own top+bottom margins. These are all
    /// Answer-independent, so this is safe to read synchronously right after the
    /// Answer text is set (the whole-container measurement was NOT — it
    /// over-counted the just-set tall Answer and raced). The caller subtracts this
    /// from the height freed below the page strip to get the Answer's exact size.
    pub fn fixed_chrome_height(&self) -> i32 {
        let h = |w: &gtk4::Widget| w.preferred_size().1.height();
        h(self.title.upcast_ref())
            + h(self.question_box.upcast_ref())
            + h(self.instruction_box.upcast_ref())
            + h(self.hint.upcast_ref())
            + self.container.margin_top()
            + self.container.margin_bottom()
    }

    /// PIN the Answer scroller to EXACTLY `h` px (min == max content height), so it
    /// fills the height freed below the collapsed page viewport AND scrolls any
    /// overflow. A `max_content_height` cap is the real bound (`vexpand` /
    /// `min_content_height` alone let the scroller's natural height grow with the
    /// Answer text, which inflated the `valign:Center` journal container PAST the
    /// parent card height — the overflow bug). Mirrors `gloss_overlay::size_scroll`
    /// pinning its visible scroll. Pass a value derived from the parent card
    /// height minus `fixed_chrome_height()`, never from the Answer's own height.
    pub fn pin_answer_height(&self, h: i32) {
        let h = h.max(120);
        self.answer_scrolled.set_min_content_height(h);
        self.answer_scrolled.set_max_content_height(h);
        self.answer_scrolled.queue_resize();
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
