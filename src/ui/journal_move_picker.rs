use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Entry, ListBox, ListBoxRow, Overlay};

use crate::app::JournalBand;

/// One selectable target band in the "move Q&A to band" picker.
/// `band` is the destination (Work or Scene(d1,d2)); `label` is its display
/// text ("whole work" / "Act 3, Scene 2" / "Chapter 5").
#[derive(Clone)]
pub struct MoveTargetRow {
    pub band: JournalBand,
    pub label: String,
}

pub struct JournalMovePicker {
    pub overlay: Overlay,
    scrim: GtkBox,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    pub items: Vec<MoveTargetRow>,
}

impl JournalMovePicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();
        let picker_box = crate::ui::picker_nav::build_picker_card_wide(crate::ui::picker_nav::JOURNAL_PICKER_WIDTH);

        let search_entry = Entry::builder()
            .placeholder_text("Move Q&A to...")
            .build();

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();

        let (header_box, _header_title) =
            crate::ui::picker_nav::build_picker_header("MOVE Q&A TO");
        picker_box.append(&header_box);
        picker_box.append(&search_entry);
        picker_box.append(&scrolled);

        JournalMovePicker {
            overlay,
            scrim: crate::ui::picker_nav::build_picker_scrim(),
            picker_box,
            search_entry,
            list_box,
            items: Vec::new(),
        }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_panel(&self.overlay, base, Some(&self.scrim), &self.picker_box);
    }

    pub fn set_items(&mut self, items: Vec<MoveTargetRow>) {
        self.items = items;
        self.populate_list("");
    }

    pub fn show(&self) {
        self.scrim.set_visible(true);
        self.picker_box.set_visible(true);
        self.search_entry.set_text("");
        self.search_entry.grab_focus();
        self.populate_list("");
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
        self.scrim.set_visible(false);
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
                let target = item.label.to_lowercase();
                if !crate::ui::picker_filter::subsequence_match(&filter_lower, &target) {
                    continue;
                }
            }

            let hbox = crate::ui::picker_nav::two_label_row(&item.label, "");
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
