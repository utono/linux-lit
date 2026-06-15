use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, ListBox, ListBoxRow, Orientation, ScrolledWindow};

use crate::db::queries::EchoTurnSummary;

/// Picker listing every turn in the current work that has echoes
/// (Ctrl+Shift+G). Selecting a turn jumps the cursor there and opens the
/// echoes overlay. The `picker_box` is added directly as an overlay onto
/// the app's outer overlay (like `EchoLinePicker`) — NOT wrapped into the
/// reader's size-bearing widget chain, which would collapse the layout.
pub struct EchoTurnsPicker {
    picker_box: GtkBox,
    list_box: ListBox,
    pub items: Vec<EchoTurnSummary>,
    titles: std::collections::HashMap<String, String>,
    work_abbrev: String,
    pub channel: crate::db::echo_channel::EchoChannel,
}

impl EchoTurnsPicker {
    pub fn new() -> Self {
        let picker_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(Align::Center)
            .valign(Align::Center)
            .width_request(640)
            .height_request(520)
            .build();
        picker_box.add_css_class("library-picker");

        let header_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .build();
        header_box.add_css_class("library-picker-header");

        let header_title = Label::builder()
            .label("ECHOES IN THIS WORK")
            .halign(Align::Start)
            .hexpand(true)
            .build();
        header_title.add_css_class("library-picker-title");
        header_box.append(&header_title);

        let list_box = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .build();

        let scrolled = ScrolledWindow::builder()
            .child(&list_box)
            .vexpand(true)
            .build();

        let footer_label = Label::builder()
            .label("j/k navigate  ·  Enter select  ·  Esc cancel")
            .halign(Align::Start)
            .hexpand(true)
            .build();
        footer_label.add_css_class("library-picker-footer");

        picker_box.append(&header_box);
        picker_box.append(&scrolled);
        picker_box.append(&footer_label);

        picker_box.set_visible(false);

        Self {
            picker_box,
            list_box,
            items: Vec::new(),
            titles: std::collections::HashMap::new(),
            work_abbrev: String::new(),
            channel: crate::db::echo_channel::EchoChannel::Shakespeare,
        }
    }

    pub fn picker_box(&self) -> &GtkBox {
        &self.picker_box
    }

    pub fn set_titles(&mut self, titles: std::collections::HashMap<String, String>) {
        self.titles = titles;
    }

    pub fn set_items(&mut self, items: Vec<EchoTurnSummary>, work_abbrev: String) {
        self.items = items;
        self.work_abbrev = work_abbrev;
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

        let title = self
            .titles
            .get(&self.work_abbrev)
            .cloned()
            .unwrap_or_else(|| self.work_abbrev.clone());

        for (idx, item) in self.items.iter().enumerate() {
            let row_box = GtkBox::builder()
                .orientation(Orientation::Vertical)
                .spacing(2)
                .build();

            let meta = format!("{}  ·  {} {}.{}", item.speaker, title, item.div1, item.div2);
            let meta_label = Label::builder()
                .label(&meta)
                .halign(Align::Start)
                .build();
            meta_label.add_css_class("picker-item-detail");
            row_box.append(&meta_label);

            let first_line = item.turn_text.lines().next().unwrap_or("").trim();
            let text_label = Label::builder()
                .label(first_line)
                .halign(Align::Start)
                .hexpand(true)
                .ellipsize(gtk4::pango::EllipsizeMode::End)
                .build();
            row_box.append(&text_label);

            let row = ListBoxRow::builder().child(&row_box).build();
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
        let next = (current + delta).max(0);
        if let Some(row) = self.list_box.row_at_index(next) {
            self.list_box.select_row(Some(&row));
        }
    }
}
