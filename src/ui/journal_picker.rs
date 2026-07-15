use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Entry, ListBox, ListBoxRow, Overlay,
};

use crate::app::JournalBand;

#[derive(Clone)]
pub struct JournalRow {
    pub id: i64,
    pub band: JournalBand,
    pub question_prefix: String,
    pub scene_label: String,
}

pub struct JournalQaPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    pub items: Vec<JournalRow>,
}

impl JournalQaPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = crate::ui::picker_nav::build_picker_card();

        let search_entry = Entry::builder()
            .placeholder_text("Filter Q&A pages...")
            .build();

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();

        let (header_box, _header_title) =
            crate::ui::picker_nav::build_picker_header("Q&A PAGES");
        picker_box.append(&header_box);
        picker_box.append(&search_entry);
        picker_box.append(&scrolled);

        JournalQaPicker {
            overlay,
            picker_box,
            search_entry,
            list_box,
            items: Vec::new(),
        }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_panel(&self.overlay, base, None, &self.picker_box);
    }

    pub fn set_items(&mut self, items: Vec<JournalRow>) {
        self.items = items;
        self.populate_list("");
    }

    pub fn show(&self) {
        self.picker_box.set_visible(true);
        self.search_entry.set_text("");
        self.search_entry.grab_focus();
        self.populate_list("");
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.picker_box.is_visible()
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    pub fn has_items(&self) -> bool {
        !self.items.is_empty()
    }

    pub fn populate_list(&self, filter: &str) {
        crate::ui::picker_nav::clear_list(&self.list_box);
        let filter_lower = filter.to_lowercase();

        for (idx, item) in self.items.iter().enumerate() {
            if !filter.is_empty() {
                let target = format!("{} {}", item.scene_label, item.question_prefix).to_lowercase();
                if !crate::ui::picker_filter::subsequence_match(&filter_lower, &target) {
                    continue;
                }
            }

            let hbox = crate::ui::picker_nav::two_label_row(
                &item.question_prefix,
                &item.scene_label,
            );

            let row = ListBoxRow::builder().child(&hbox).build();
            row.set_widget_name(&idx.to_string());
            self.list_box.append(&row);
        }

        crate::ui::picker_nav::select_first_row(&self.list_box);
    }

    pub fn move_selection(&self, delta: i32) {
        crate::ui::picker_nav::move_selection_clamped(&self.list_box, delta);
    }

    /// Index into `items` of the selected row (the row's widget_name).
    pub fn selected_index(&self) -> Option<usize> {
        crate::ui::picker_nav::selected_index(&self.list_box)
    }
}
