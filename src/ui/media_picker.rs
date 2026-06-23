use std::path::Path;

use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Orientation, Overlay,
};

use crate::db::models::MediaItem;

pub struct MediaPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    title_label: Label,
    search_entry: Entry,
    list_box: ListBox,
    items: Vec<MediaItem>,
}

impl MediaPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(600)
            .height_request(400)
            .build();
        picker_box.add_css_class("library-picker");

        let title_label = Label::builder()
            .halign(gtk4::Align::Center)
            .margin_top(8)
            .margin_bottom(4)
            .build();
        title_label.add_css_class("picker-title");

        let search_entry = Entry::builder()
            .placeholder_text("Filter media...")
            .build();

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();

        picker_box.append(&title_label);
        picker_box.append(&search_entry);
        picker_box.append(&scrolled);

        MediaPicker {
            overlay,
            picker_box,
            title_label,
            search_entry,
            list_box,
            items: Vec::new(),
        }
    }

    pub fn set_title(&self, abbrev: &str, title: &str) {
        self.title_label.set_text(&format!("{} — {}", abbrev, title));
    }

    pub fn set_items(&mut self, items: Vec<MediaItem>) {
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

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_panel(&self.overlay, base, None, &self.picker_box);
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    pub fn populate_list(&self, filter: &str) {
        crate::ui::picker_nav::clear_list(&self.list_box);

        let filter_lower = filter.to_lowercase();
        let max_priority = self.items.iter().map(|i| i.priority).max().unwrap_or(0);

        for item in &self.items {
            let is_default = item.priority == max_priority && max_priority > 0;
            let display = format_media_label(item, is_default);
            if !filter.is_empty() {
                let target = display.to_lowercase();
                if !crate::ui::picker_filter::subsequence_match(&filter_lower, &target) {
                    continue;
                }
            }

            let label = Label::builder()
                .label(&display)
                .halign(gtk4::Align::Start)
                .build();

            let row = ListBoxRow::builder().child(&label).build();
            row.set_widget_name(&item.media_id.to_string());
            self.list_box.append(&row);
        }

        if let Some(first) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&first));
        }
    }

    pub fn selected_media_id(&self) -> Option<i64> {
        self.list_box
            .selected_row()
            .and_then(|row| row.widget_name().to_string().parse().ok())
    }

    pub fn selected_media_path(&self) -> Option<String> {
        let id = self.selected_media_id()?;
        self.items
            .iter()
            .find(|item| item.media_id == id)
            .map(|item| item.path.clone())
    }

    pub fn set_default(&mut self, media_id: i64, new_priority: i64) {
        for item in &mut self.items {
            if item.media_id == media_id {
                item.priority = new_priority;
            } else {
                item.priority = 10;
            }
        }
        // Re-sort by priority descending
        self.items.sort_by(|a, b| b.priority.cmp(&a.priority));
        self.populate_list("");
    }

    pub fn move_selection(&self, delta: i32) {
        if let Some(current) = self.list_box.selected_row() {
            let idx = current.index();
            let new_idx = (idx + delta).max(0);
            crate::ui::picker_nav::select_row_at(&self.list_box, new_idx);
        }
    }
}

fn format_media_label(item: &MediaItem, is_default: bool) -> String {
    let base = if let Some(ref name) = item.display_name {
        if !name.is_empty() {
            name.clone()
        } else {
            format_path(&item.path)
        }
    } else {
        format_path(&item.path)
    };
    if is_default {
        format!("* {}", base)
    } else {
        format!("  {}", base)
    }
}

fn format_path(path: &str) -> String {
    let p = Path::new(path);
    let filename = p.file_name().unwrap_or_default().to_string_lossy();
    let parent = p
        .parent()
        .and_then(|pp| pp.file_name())
        .map(|d| d.to_string_lossy())
        .unwrap_or_default();
    if parent.is_empty() {
        filename.to_string()
    } else {
        format!("{}/{}", parent, filename)
    }
}
