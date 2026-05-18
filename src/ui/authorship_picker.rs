use gtk4::prelude::*;
use crate::db::authorship::AttributionSet;

pub struct AuthorshipPicker {
    pub overlay: gtk4::Overlay,
    container: gtk4::Box,
    list_box: gtk4::ListBox,
    items: Vec<AttributionSet>,
}

impl AuthorshipPicker {
    pub fn new() -> Self {
        let overlay = gtk4::Overlay::new();

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        container.set_halign(gtk4::Align::Center);
        container.set_valign(gtk4::Align::Center);
        container.set_width_request(400);
        container.add_css_class("library-picker");

        let title = gtk4::Label::new(Some("Attribution Sets"));
        title.add_css_class("picker-title");
        title.set_margin_top(8);
        title.set_margin_bottom(4);
        container.append(&title);

        let list_box = gtk4::ListBox::new();
        list_box.set_selection_mode(gtk4::SelectionMode::Single);

        let scroll = gtk4::ScrolledWindow::builder()
            .child(&list_box)
            .vexpand(true)
            .build();
        container.append(&scroll);

        container.set_visible(false);

        Self { overlay, container, list_box, items: Vec::new() }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.container);
        self.container.set_visible(false);
    }

    pub fn set_items(&mut self, items: Vec<AttributionSet>) {
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }
        for set in &items {
            let label = gtk4::Label::new(Some(&set.display_name));
            label.set_halign(gtk4::Align::Start);
            label.set_margin_start(12);
            label.set_margin_end(12);
            label.set_margin_top(4);
            label.set_margin_bottom(4);
            self.list_box.append(&label);
        }
        self.items = items;
        if let Some(row) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&row));
        }
    }

    pub fn show(&self) {
        self.container.set_visible(true);
        if let Some(row) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&row));
        }
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn move_selection(&self, delta: i32) {
        let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(0);
        let next = (current + delta).max(0);
        if let Some(row) = self.list_box.row_at_index(next) {
            self.list_box.select_row(Some(&row));
        }
    }

    pub fn selected_set(&self) -> Option<&AttributionSet> {
        let idx = self.list_box.selected_row()?.index();
        self.items.get(idx as usize)
    }
}
