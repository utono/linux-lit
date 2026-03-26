use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Overlay, Orientation, ScrolledWindow,
};

use crate::db::models::WorkSummary;

pub struct LibraryPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    works: Vec<WorkSummary>,
}

impl LibraryPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(500)
            .height_request(400)
            .build();
        picker_box.add_css_class("library-picker");

        let search_entry = Entry::builder()
            .placeholder_text("Filter works...")
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

        LibraryPicker {
            overlay,
            picker_box,
            search_entry,
            list_box,
            works: Vec::new(),
        }
    }

    pub fn set_works(&mut self, works: Vec<WorkSummary>) {
        self.works = works;
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

    pub fn list_box(&self) -> &ListBox {
        &self.list_box
    }

    pub fn picker_box(&self) -> &GtkBox {
        &self.picker_box
    }

    pub fn populate_list(&self, filter: &str) {
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        let filter_lower = filter.to_lowercase();

        for work in &self.works {
            if !filter.is_empty() && !subsequence_match(&filter_lower, work) {
                continue;
            }

            let label = Label::builder()
                .label(&format!("{} — {} ({})", work.title, work.author, work.abbrev))
                .halign(gtk4::Align::Start)
                .build();

            let row = ListBoxRow::builder().child(&label).build();
            row.set_widget_name(&work.abbrev);
            self.list_box.append(&row);
        }

        if let Some(first) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&first));
        }
    }

    pub fn selected_abbrev(&self) -> Option<String> {
        self.list_box
            .selected_row()
            .map(|row| row.widget_name().to_string())
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
}

/// Case-insensitive subsequence match against title, author, and abbrev.
fn subsequence_match(filter: &str, work: &WorkSummary) -> bool {
    let target = format!("{} {} {}", work.title, work.author, work.abbrev).to_lowercase();
    let mut target_chars = target.chars();
    for fc in filter.chars() {
        if !target_chars.any(|tc| tc == fc) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_work(abbrev: &str, title: &str, author: &str) -> WorkSummary {
        WorkSummary {
            abbrev: abbrev.to_string(),
            title: title.to_string(),
            author: author.to_string(),
            work_type: "play".to_string(),
        }
    }

    #[test]
    fn test_subsequence_match_exact() {
        let w = make_work("Ham", "Hamlet", "Shakespeare");
        assert!(subsequence_match("hamlet", &w));
    }

    #[test]
    fn test_subsequence_match_partial() {
        let w = make_work("Ham", "Hamlet", "Shakespeare");
        assert!(subsequence_match("hml", &w));
        assert!(subsequence_match("ham", &w));
    }

    #[test]
    fn test_subsequence_match_no_match() {
        let w = make_work("Ham", "Hamlet", "Shakespeare");
        assert!(!subsequence_match("xyz", &w));
    }

    #[test]
    fn test_subsequence_match_author() {
        let w = make_work("Ham", "Hamlet", "Shakespeare");
        assert!(subsequence_match("shk", &w));
    }
}
