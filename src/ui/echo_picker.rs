use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, ListBox, ListBoxRow, Orientation, Overlay, ScrolledWindow};

use crate::db::queries::EchoCandidate;

/// Picker for selecting a semantic-search echo candidate before
/// generating an inner-monologue gloss.
pub struct EchoPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    list_box: ListBox,
    pub items: Vec<EchoCandidate>,
    titles: std::collections::HashMap<String, String>,
}

impl EchoPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = GtkBox::new(Orientation::Vertical, 0);
        picker_box.set_halign(Align::Center);
        picker_box.set_valign(Align::Start);
        picker_box.set_margin_top(40);
        picker_box.set_width_request(640);
        picker_box.add_css_class("picker-box");

        let header = Label::new(Some("Suggested echoes  ·  j/k navigate  ·  Enter select  ·  Esc skip"));
        header.add_css_class("picker-entry");
        header.set_halign(Align::Center);
        picker_box.append(&header);

        let scrolled = ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_max_content_height(500);
        scrolled.set_propagate_natural_height(true);

        let list_box = ListBox::new();
        list_box.add_css_class("picker-list");
        scrolled.set_child(Some(&list_box));
        picker_box.append(&scrolled);

        Self {
            overlay,
            picker_box,
            list_box,
            items: Vec::new(),
            titles: std::collections::HashMap::new(),
        }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.picker_box);
        self.picker_box.set_visible(false);
    }

    pub fn set_titles(&mut self, titles: std::collections::HashMap<String, String>) {
        self.titles = titles;
    }

    pub fn set_items(&mut self, items: Vec<EchoCandidate>) {
        self.items = items;
    }

    pub fn show(&self) {
        self.populate_list();
        self.picker_box.set_visible(true);
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    fn populate_list(&self) {
        while let Some(row) = self.list_box.first_child() {
            self.list_box.remove(&row);
        }

        for (idx, item) in self.items.iter().enumerate() {
            let row_box = GtkBox::new(Orientation::Vertical, 2);
            row_box.set_margin_start(10);
            row_box.set_margin_end(10);
            row_box.set_margin_top(6);
            row_box.set_margin_bottom(6);

            let title = self.titles.get(&item.work_abbrev)
                .cloned()
                .unwrap_or_else(|| item.work_abbrev.clone());
            let meta = format!("{}  ·  {} {}.{}", item.speaker, title, item.div1, item.div2);
            let meta_label = Label::new(Some(&meta));
            meta_label.set_halign(Align::Start);
            meta_label.add_css_class("picker-item-detail");
            row_box.append(&meta_label);

            let first_line = item.passage_text.lines().next().unwrap_or("").trim();
            let text_label = Label::new(Some(first_line));
            text_label.set_halign(Align::Start);
            text_label.set_wrap(true);
            text_label.set_max_width_chars(70);
            text_label.add_css_class("picker-item-title");
            row_box.append(&text_label);

            let row = ListBoxRow::new();
            row.set_child(Some(&row_box));
            row.set_widget_name(&idx.to_string());
            self.list_box.append(&row);
        }

        if let Some(row) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&row));
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.list_box
            .selected_row()
            .and_then(|row| row.widget_name().parse::<usize>().ok())
    }

    pub fn move_selection(&self, delta: i32) {
        let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
        let next = current + delta;
        if let Some(row) = self.list_box.row_at_index(next) {
            self.list_box.select_row(Some(&row));
        }
    }
}
