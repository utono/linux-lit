use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Orientation, ScrolledWindow};
use std::collections::HashMap;

/// A single search hit: (work_abbrev, div1, div2, line_in_div, text).
pub type LineHit = (String, i64, i64, i64, String);

/// Picker for adding an echo: fuzzy-search Shakespeare lines, pick one.
pub struct EchoLinePicker {
    /// The picker panel, added directly as an overlay onto the app's outer
    /// overlay (like concordance_works_picker.container) — NOT wrapped into the
    /// reader's size-bearing widget chain.
    pub picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    results: Vec<LineHit>,
}

impl EchoLinePicker {
    pub fn new() -> Self {
        let picker_box = GtkBox::new(Orientation::Vertical, 0);
        picker_box.set_halign(Align::Center);
        picker_box.set_valign(Align::Start);
        picker_box.set_margin_top(40);
        picker_box.set_width_request(600);
        picker_box.add_css_class("picker-box");

        let search_entry = Entry::new();
        search_entry.set_placeholder_text(Some("Search Shakespeare lines…"));
        search_entry.add_css_class("picker-entry");
        picker_box.append(&search_entry);

        let scrolled = ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_max_content_height(400);
        scrolled.set_propagate_natural_height(true);

        let list_box = ListBox::new();
        list_box.add_css_class("picker-list");
        scrolled.set_child(Some(&list_box));
        picker_box.append(&scrolled);

        picker_box.set_visible(false);
        Self { picker_box, search_entry, list_box, results: Vec::new() }
    }

    pub fn show(&self) {
        self.search_entry.set_text("");
        self.picker_box.set_visible(true);
        self.search_entry.grab_focus();
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    pub fn entry(&self) -> &Entry {
        &self.search_entry
    }

    /// Replace the result rows. `titles` maps work_abbrev -> display title for
    /// the "text — Title div1.div2" row label.
    pub fn set_results(&mut self, results: Vec<LineHit>, titles: &HashMap<String, String>) {
        self.results = results;
        while let Some(row) = self.list_box.first_child() {
            self.list_box.remove(&row);
        }
        for (work, div1, div2, _line, text) in &self.results {
            let title = titles.get(work).cloned().unwrap_or_else(|| work.clone());
            let label = Label::new(Some(&format!("{} — {} {}.{}", text, title, div1, div2)));
            label.set_halign(Align::Start);
            label.set_hexpand(true);
            label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            label.add_css_class("picker-item-title");
            let row = ListBoxRow::new();
            row.set_child(Some(&label));
            self.list_box.append(&row);
        }
        if let Some(row) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&row));
        }
    }

    pub fn move_selection(&self, delta: i32) {
        let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
        let next = current + delta;
        crate::ui::picker_nav::select_row_at(&self.list_box, next);
    }

    /// The currently selected line hit, if any.
    pub fn selected_hit(&self) -> Option<LineHit> {
        let idx = self.list_box.selected_row()?.index();
        if idx < 0 { return None; }
        self.results.get(idx as usize).cloned()
    }
}
