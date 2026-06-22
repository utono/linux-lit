use gtk4::prelude::*;
use gtk4::{Align, Label, ScrolledWindow, TextView};
use std::cell::Cell;

/// Which side of a "<document> + ask" overlay holds keyboard focus.
/// `Doc` = the synopsis/gloss card or journal page; `Ask` = the input field.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AskFocus {
    Doc,
    Ask,
}

/// The shared multi-line "ask" input card, stacked below a synopsis/gloss or
/// journal card. Built with the canonical synopsis values; both overlays embed
/// one and delegate their `*_ask_*` methods to it so the two cards can't drift.
pub struct AskCard {
    container: gtk4::Box,
    title: Label,
    input: TextView,
    hint: Label,
    focus: Cell<AskFocus>,
    return_focus: gtk4::Widget,
}

impl AskCard {
    /// Build the card with the canonical synopsis values. `return_focus` is the
    /// document-side widget that GTK focus returns to when leaving the input
    /// (gloss: its scroller; journal: its page view).
    pub fn new(text_margins: i32, return_focus: &impl IsA<gtk4::Widget>) -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("ask-card");
        container.set_margin_top(14);
        container.set_margin_start(text_margins);
        container.set_margin_end(text_margins);
        container.set_margin_bottom(14);

        let title = Label::new(Some(""));
        title.add_css_class("gloss-header");
        title.set_halign(Align::Start);
        title.set_margin_start(16);
        title.set_margin_top(12);
        container.append(&title);

        let scrolled = ScrolledWindow::new();
        scrolled.set_min_content_height(160);
        scrolled.set_max_content_height(320);
        scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
        scrolled.set_margin_start(16);
        scrolled.set_margin_end(16);
        scrolled.set_margin_top(6);
        scrolled.set_margin_bottom(6);

        let input = TextView::new();
        input.set_editable(true);
        input.set_cursor_visible(true);
        input.set_wrap_mode(gtk4::WrapMode::Word);
        input.set_top_margin(6);
        input.set_bottom_margin(6);
        input.set_left_margin(6);
        input.set_right_margin(6);
        input.add_css_class("gloss-text");
        input.add_css_class("ask-input");
        scrolled.set_child(Some(&input));
        container.append(&scrolled);

        let hint = Label::new(Some(""));
        hint.add_css_class("ask-hint");
        hint.set_halign(Align::Center);
        hint.set_margin_bottom(10);
        container.append(&hint);

        container.set_visible(false);

        Self {
            container,
            title,
            input,
            hint,
            focus: Cell::new(AskFocus::Doc),
            return_focus: return_focus.clone().upcast(),
        }
    }

    /// The card box; the embedding overlay appends this into its own card column.
    pub fn container(&self) -> &gtk4::Box {
        &self.container
    }

    /// The input TextView — exposed so each overlay applies its own font.
    pub fn input(&self) -> &TextView {
        &self.input
    }

    /// Reveal with heading + hint, clear the field, re-align margins to
    /// card_width/4, focus the input (AskFocus::Ask + card-focused highlight).
    pub fn open(&self, title: &str, hint: &str, card_width: i32) {
        self.title.set_text(title);
        self.hint.set_text(hint);
        self.input.buffer().set_text("");
        if card_width > 0 {
            let margin = card_width / 4;
            self.container.set_margin_start(margin);
            self.container.set_margin_end(margin);
        }
        self.container.set_visible(true);
        self.set_focus(AskFocus::Ask);
    }

    /// Hide, set AskFocus::Doc, drop the highlight, return focus to return_focus.
    pub fn close(&self) {
        self.container.set_visible(false);
        self.focus.set(AskFocus::Doc);
        self.container.remove_css_class("card-focused");
        self.container.remove_css_class("card-dimmed");
        if self.input.has_focus() {
            let _ = self.return_focus.grab_focus();
        }
    }

    pub fn is_open(&self) -> bool {
        self.container.is_visible()
    }

    pub fn focus(&self) -> AskFocus {
        self.focus.get()
    }

    /// Flip Doc<->Ask (no-op if closed). Owns the card-focused/card-dimmed
    /// highlight swap and the input grab / return-focus grab.
    pub fn toggle_focus(&self) {
        if !self.is_open() {
            return;
        }
        let next = match self.focus.get() {
            AskFocus::Doc => AskFocus::Ask,
            AskFocus::Ask => AskFocus::Doc,
        };
        self.set_focus(next);
    }

    fn set_focus(&self, focus: AskFocus) {
        self.focus.set(focus);
        match focus {
            AskFocus::Ask => {
                self.container.remove_css_class("card-dimmed");
                self.container.add_css_class("card-focused");
                self.input.grab_focus();
            }
            AskFocus::Doc => {
                self.container.remove_css_class("card-focused");
                self.container.add_css_class("card-dimmed");
                if self.input.has_focus() {
                    let _ = self.return_focus.grab_focus();
                }
            }
        }
    }

    /// Read and clear the input's text.
    pub fn take_text(&self) -> String {
        let buffer = self.input.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        buffer.set_text("");
        text
    }
}
