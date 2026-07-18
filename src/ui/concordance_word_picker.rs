use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Orientation, Overlay, ScrolledWindow};

/// Picker for selecting a vocab word for cross-work concordance.
pub struct ConcordanceWordPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    words: Vec<(String, usize)>,
}

impl ConcordanceWordPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = super::picker_nav::new_top_anchored_picker_box(
            super::picker_nav::PICKER_NARROW_W,
            "picker-box",
        );

        let search_entry = Entry::new();
        search_entry.set_placeholder_text(Some("Search vocab words..."));
        search_entry.add_css_class("picker-entry");
        picker_box.append(&search_entry);

        let scrolled = ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_max_content_height(super::picker_nav::PICKER_LIST_MAX_H);
        scrolled.set_propagate_natural_height(true);

        let list_box = ListBox::new();
        list_box.add_css_class("picker-list");
        scrolled.set_child(Some(&list_box));
        picker_box.append(&scrolled);

        Self {
            overlay,
            picker_box,
            search_entry,
            list_box,
            words: Vec::new(),
        }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_panel(&self.overlay, base, None, &self.picker_box);
    }

    pub fn show(&self) {
        self.search_entry.set_text("");
        self.populate_list("");
        self.picker_box.set_visible(true);
        self.search_entry.grab_focus();
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    pub fn set_words(&mut self, words: Vec<(String, usize)>) {
        self.words = words;
    }

    pub fn filter_changed(&self) {
        let filter = self.search_entry.text().to_string();
        self.populate_list(&filter);
    }

    fn populate_list(&self, filter: &str) {
        // Remove existing rows
        crate::ui::picker_nav::clear_list(&self.list_box);

        let filter_lower = filter.to_lowercase();
        for (word, _count) in &self.words {
            if !filter_lower.is_empty() && !word.contains(&filter_lower) {
                continue;
            }

            let row_box = GtkBox::new(Orientation::Horizontal, 8);
            row_box.set_margin_start(8);
            row_box.set_margin_end(8);
            row_box.set_margin_top(2);
            row_box.set_margin_bottom(2);

            let word_label = Label::new(Some(word));
            word_label.set_halign(Align::Start);
            word_label.set_hexpand(true);
            word_label.add_css_class("picker-item-title");

            row_box.append(&word_label);

            let row = ListBoxRow::new();
            row.set_child(Some(&row_box));
            row.set_widget_name(word);
            self.list_box.append(&row);
        }

        // Select first row
        crate::ui::picker_nav::select_first_row(&self.list_box);
    }

    pub fn selected_word(&self) -> Option<String> {
        self.list_box
            .selected_row()
            .map(|row| row.widget_name().to_string())
    }

    pub fn move_selection(&self, delta: i32) {
        crate::ui::picker_nav::move_selection_from(&self.list_box, delta);
    }

    pub fn entry(&self) -> &Entry {
        &self.search_entry
    }
}
