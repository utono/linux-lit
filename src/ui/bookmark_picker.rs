use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Entry, ListBox, ListBoxRow, Overlay,
};

use crate::db::models::BookmarkItem;

pub struct BookmarkPicker {
    pub overlay: Overlay,
    scrim: GtkBox,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    items: Vec<BookmarkItem>,
}

impl BookmarkPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = crate::ui::picker_nav::build_picker_card();

        let search_entry = Entry::builder()
            .placeholder_text("Filter bookmarks...")
            .build();

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();

        let (header_box, _header_title) =
            crate::ui::picker_nav::build_picker_header("BOOKMARKS");
        picker_box.append(&header_box);
        picker_box.append(&search_entry);
        picker_box.append(&scrolled);

        BookmarkPicker {
            overlay,
            scrim: crate::ui::picker_nav::build_picker_scrim(),
            picker_box,
            search_entry,
            list_box,
            items: Vec::new(),
        }
    }

    pub fn set_items(&mut self, items: Vec<BookmarkItem>) {
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

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_panel(&self.overlay, base, Some(&self.scrim), &self.picker_box);
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    pub fn populate_list(&self, filter: &str) {
        crate::ui::picker_nav::clear_list(&self.list_box);

        let filter_lower = filter.to_lowercase();

        for item in &self.items {
            if !filter.is_empty() {
                let short_target = item.speaker.to_lowercase();
                let long_target = item.line_text.to_lowercase();
                if !crate::ui::picker_filter::row_matches(
                    &filter_lower,
                    &short_target,
                    &long_target,
                    "",
                ) {
                    continue;
                }
            }

            let display = crate::ui::picker_nav::speaker_prefixed_first_line(
                &item.speaker,
                &item.line_text,
            );
            let hbox = crate::ui::picker_nav::two_label_row(&display, &item.citation);

            let row = ListBoxRow::builder().child(&hbox).build();
            row.set_widget_name(&item.line_mapping_id.to_string());
            self.list_box.append(&row);
        }

        crate::ui::picker_nav::select_first_row(&self.list_box);
    }

    pub fn selected_line_mapping_id(&self) -> Option<i64> {
        self.list_box
            .selected_row()
            .and_then(|row| row.widget_name().to_string().parse().ok())
    }

    pub fn move_selection(&self, delta: i32) {
        crate::ui::picker_nav::move_selection_clamped(&self.list_box, delta);
    }

    /// Remove the selected bookmark from the internal items list and the ListBox.
    /// Returns the line_mapping_id of the removed item, or None if nothing selected.
    pub fn remove_selected(&mut self) -> Option<i64> {
        let row = self.list_box.selected_row()?;
        let lm_id: i64 = row.widget_name().to_string().parse().ok()?;
        let idx = row.index();

        self.items.retain(|i| i.line_mapping_id != lm_id);
        self.list_box.remove(&row);

        let next = self.list_box.row_at_index(idx)
            .or_else(|| self.list_box.row_at_index((idx - 1).max(0)));
        if let Some(r) = next {
            self.list_box.select_row(Some(&r));
        }

        Some(lm_id)
    }

    pub fn has_items(&self) -> bool {
        !self.items.is_empty()
    }
}
