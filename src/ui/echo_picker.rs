use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, ListBox, ListBoxRow, Orientation, Overlay};

use crate::db::echoes::EchoCandidate;

/// Picker for selecting a semantic-search echo candidate before
/// generating an inner-monologue gloss. Matches the library-picker
/// look-and-feel (cream card, header, footer hint, scrim).
pub struct EchoPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    scrim: GtkBox,
    list_box: ListBox,
    pub items: Vec<EchoCandidate>,
    titles: std::collections::HashMap<String, String>,
}

impl EchoPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let scrim = crate::ui::picker_nav::build_picker_scrim();

        let picker_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(Align::Center)
            .valign(Align::Center)
            .width_request(960)
            .height_request(975)
            .build();
        picker_box.add_css_class("library-picker");

        let (header_box, _header_title) = crate::ui::picker_nav::build_picker_header("SUGGESTED ECHOES");

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();

        // NO footer: the list is the LAST child so it runs to the card's
        // bottom edge, leaving no strip for a partial row (clip-prevention
        // #16c). Binds live in the keybinds overlay.

        picker_box.append(&header_box);
        picker_box.append(&scrolled);

        Self {
            overlay,
            picker_box,
            scrim,
            list_box,
            items: Vec::new(),
            titles: std::collections::HashMap::new(),
        }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_panel(&self.overlay, base, Some(&self.scrim), &self.picker_box);
    }

    pub fn set_titles(&mut self, titles: std::collections::HashMap<String, String>) {
        self.titles = titles;
    }

    pub fn set_items(&mut self, items: Vec<EchoCandidate>) {
        self.items = items;
    }

    pub fn show(&self) {
        self.populate_list();
        self.scrim.set_visible(true);
        self.picker_box.set_visible(true);
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
        self.scrim.set_visible(false);
    }

    fn populate_list(&self) {
        crate::ui::picker_nav::clear_list(&self.list_box);

        for (idx, item) in self.items.iter().enumerate() {
            let row_box = GtkBox::builder()
                .orientation(Orientation::Vertical)
                .spacing(2)
                .build();

            let title = self.titles.get(&item.work_abbrev)
                .cloned()
                .unwrap_or_else(|| item.work_abbrev.clone());
            let meta = format!("{}  ·  {} {}.{}", item.speaker, title, item.div1, item.div2);
            let meta_label = Label::builder()
                .label(&meta)
                .halign(Align::Start)
                .build();
            meta_label.add_css_class("picker-item-detail");
            row_box.append(&meta_label);

            let first_line = item.passage_text.lines().next().unwrap_or("").trim();
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

        crate::ui::picker_nav::select_first_row(&self.list_box);
    }

    pub fn selected_index(&self) -> Option<usize> {
        crate::ui::picker_nav::selected_index(&self.list_box)
    }

    pub fn move_selection(&self, delta: i32) {
        crate::ui::picker_nav::move_selection_from(&self.list_box, delta);
    }
}
