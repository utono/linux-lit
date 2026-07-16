use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, ListBox, ListBoxRow, Orientation, Overlay, ScrolledWindow};

use crate::concordance::ConcordanceHit;

/// Picker for jumping to a specific occurrence in cross-work concordance.
pub struct ConcordanceListPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    list_box: ListBox,
}

impl ConcordanceListPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = GtkBox::new(Orientation::Vertical, 0);
        picker_box.set_halign(Align::Center);
        picker_box.set_valign(Align::Start);
        picker_box.set_margin_top(40);
        picker_box.set_width_request(900);
        picker_box.add_css_class("picker-box");

        let header = Label::new(Some("Concordance occurrences"));
        header.add_css_class("picker-header");
        header.set_margin_top(8);
        header.set_margin_bottom(4);
        picker_box.append(&header);

        let scrolled = ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_max_content_height(940);
        scrolled.set_propagate_natural_height(true);

        let list_box = ListBox::new();
        list_box.add_css_class("picker-list");
        scrolled.set_child(Some(&list_box));
        picker_box.append(&scrolled);

        Self {
            overlay,
            picker_box,
            list_box,
        }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_panel(&self.overlay, base, None, &self.picker_box);
    }

    pub fn show(&self, hits: &[ConcordanceHit], current_index: usize) {
        // Remove existing rows
        crate::ui::picker_nav::clear_list(&self.list_box);

        for (i, hit) in hits.iter().enumerate() {
            let row_box = GtkBox::new(Orientation::Vertical, 2);
            row_box.set_margin_start(8);
            row_box.set_margin_end(8);
            row_box.set_margin_top(4);
            row_box.set_margin_bottom(4);

            // Top line: author, title
            let author = crate::concordance::shorten_author(&hit.author);
            let title = shorten_title(&hit.work_title);
            let header = Label::new(Some(&format!(
                "{}, {} [{}.{}]",
                author, title, hit.div1, hit.line_in_div
            )));
            header.set_halign(Align::Start);
            header.add_css_class("picker-item-title");

            // Bottom line: snippet
            let snippet = truncate_around_center(&hit.canonical_text, 80);
            let detail = Label::new(Some(&snippet));
            detail.set_halign(Align::Start);
            detail.add_css_class("picker-item-detail");
            detail.set_ellipsize(pango::EllipsizeMode::End);

            row_box.append(&header);
            row_box.append(&detail);

            let row = ListBoxRow::new();
            row.set_child(Some(&row_box));
            // Store index as widget name for retrieval
            row.set_widget_name(&i.to_string());
            self.list_box.append(&row);
        }

        // Select current occurrence
        if let Some(row) = self.list_box.row_at_index(current_index as i32) {
            self.list_box.select_row(Some(&row));
        }

        self.picker_box.set_visible(true);
        self.list_box.grab_focus();
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    pub fn selected_index(&self) -> Option<usize> {
        crate::ui::picker_nav::selected_index(&self.list_box)
    }

    pub fn move_selection(&self, delta: i32) {
        crate::ui::picker_nav::move_selection_from(&self.list_box, delta);
    }
}

fn shorten_title(title: &str) -> &str {
    let t = title.split(':').next().unwrap_or(title).trim();
    t.strip_prefix("The ").unwrap_or(t)
}

fn truncate_around_center(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}...", &text[..max_len])
    }
}
