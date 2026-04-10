use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Orientation, Overlay, ScrolledWindow,
};

use crate::db::models::BookmarkItem;

pub struct BookmarkPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    items: Vec<BookmarkItem>,
}

impl BookmarkPicker {
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

        let search_entry = Entry::builder()
            .placeholder_text("Filter bookmarks...")
            .build();

        let list_box = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .build();

        let scrolled = ScrolledWindow::builder()
            .child(&list_box)
            .vexpand(true)
            .build();

        picker_box.append(&search_entry);
        picker_box.append(&scrolled);

        BookmarkPicker {
            overlay,
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
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.picker_box);
        self.picker_box.set_visible(false);
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    pub fn populate_list(&self, filter: &str) {
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        let filter_lower = filter.to_lowercase();

        for item in &self.items {
            if !filter.is_empty() {
                let target = item.line_text.to_lowercase();
                if !subsequence_match(&filter_lower, &target) {
                    continue;
                }
            }

            let text = truncate_text(&item.line_text, 80);
            let time_label = format_relative_time(&item.created_at);

            let text_label = Label::builder()
                .label(&text)
                .halign(gtk4::Align::Start)
                .hexpand(true)
                .ellipsize(gtk4::pango::EllipsizeMode::End)
                .build();

            let time_lbl = Label::builder()
                .label(&time_label)
                .halign(gtk4::Align::End)
                .build();
            time_lbl.add_css_class("picker-item-detail");

            let hbox = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(8)
                .build();
            hbox.append(&text_label);
            hbox.append(&time_lbl);

            let row = ListBoxRow::builder().child(&hbox).build();
            row.set_widget_name(&item.line_mapping_id.to_string());
            self.list_box.append(&row);
        }

        if let Some(first) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&first));
        }
    }

    pub fn selected_line_mapping_id(&self) -> Option<i64> {
        self.list_box
            .selected_row()
            .and_then(|row| row.widget_name().to_string().parse().ok())
    }

    pub fn move_selection(&self, delta: i32) {
        if let Some(current) = self.list_box.selected_row() {
            let idx = current.index();
            let new_idx = (idx + delta).max(0);
            if let Some(row) = self.list_box.row_at_index(new_idx) {
                self.list_box.select_row(Some(&row));
            }
        }
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

fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let end = text.char_indices().nth(max_chars).map(|(i, _)| i).unwrap_or(text.len());
        format!("{}...", &text[..end])
    }
}

fn format_relative_time(iso: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let created = parse_iso_to_unix(iso).unwrap_or(now);
    if created >= now {
        return "just now".to_string();
    }
    let diff = now - created;

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 86400 * 30 {
        format!("{}d ago", diff / 86400)
    } else {
        format!("{}mo ago", diff / (86400 * 30))
    }
}

fn parse_iso_to_unix(iso: &str) -> Option<u64> {
    let s = iso.trim_end_matches('Z');
    let (date_part, time_part) = s.split_once('T')?;
    let mut date_iter = date_part.split('-');
    let year: i64 = date_iter.next()?.parse().ok()?;
    let month: i64 = date_iter.next()?.parse().ok()?;
    let day: i64 = date_iter.next()?.parse().ok()?;

    let time_no_frac = time_part.split('.').next()?;
    let mut time_iter = time_no_frac.split(':');
    let hour: i64 = time_iter.next()?.parse().ok()?;
    let minute: i64 = time_iter.next()?.parse().ok()?;
    let second: i64 = time_iter.next().unwrap_or("0").parse().ok()?;

    let mut days: i64 = 0;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    let month_days = [31, if is_leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for m in 0..(month - 1) as usize {
        days += month_days.get(m).copied().unwrap_or(30) as i64;
    }
    days += day - 1;

    let secs = days * 86400 + hour * 3600 + minute * 60 + second;
    Some(secs as u64)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn subsequence_match(filter: &str, target: &str) -> bool {
    let mut target_chars = target.chars();
    for fc in filter.chars() {
        if !target_chars.any(|tc| tc == fc) {
            return false;
        }
    }
    true
}
