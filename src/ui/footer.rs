use gtk4::prelude::*;
use gtk4::{Align, Label};

/// Handles to the shared "gloss-hint" footer row. The caller sets the left
/// label's text/visibility and may append further widgets (e.g. the gloss
/// position counter) into `container` before/after appending it to the column.
pub(crate) struct FooterRow {
    /// The footer box (`gloss-hint` class). Append to your card column.
    pub container: gtk4::Box,
    /// Left-anchored, hexpand label (citation / work-scene). Caller sets text
    /// and visibility, and typically stores it in its own struct for updates.
    pub left: Label,
    /// Right-anchored fixed keybind hint (text set from `hint_text`).
    pub hint: Label,
}

/// Build the gloss/journal footer row: a horizontal box (text_margins sides,
/// 12px top/bottom, `gloss-hint` class) with a left-anchored hexpand label and a
/// right-anchored hint label (`halign End`, `margin_end 12`). Left then hint are
/// appended into the box. Left-label visibility and any extra widgets are the
/// caller's responsibility.
pub(crate) fn build_footer_row(text_margins: i32, hint_text: &str) -> FooterRow {
    let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    container.set_margin_start(text_margins);
    container.set_margin_end(text_margins);
    container.set_margin_top(12);
    container.set_margin_bottom(12);
    container.add_css_class("gloss-hint");

    let left = Label::new(None);
    left.set_halign(Align::Start);
    left.set_hexpand(true);
    container.append(&left);

    let hint = Label::new(Some(hint_text));
    hint.set_halign(Align::End);
    hint.set_margin_end(12);
    container.append(&hint);

    FooterRow { container, left, hint }
}
