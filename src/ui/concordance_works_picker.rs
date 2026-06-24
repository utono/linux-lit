use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, ListBox, ListBoxRow, Orientation};

pub struct ConcordanceWorksPicker {
    pub container: GtkBox,
    pub scrim: GtkBox,
    list_box: ListBox,
    header_title: Label,
}

impl ConcordanceWorksPicker {
    pub fn new() -> Self {
        let scrim = crate::ui::picker_nav::build_picker_scrim();

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(Align::Center)
            .valign(Align::Center)
            .width_request(360)
            .height_request(280)
            .build();
        container.add_css_class("library-picker");
        container.set_visible(false);

        let (header_box, header_title) = crate::ui::picker_nav::build_picker_header("JUMP TO WORK");

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();

        let footer_label = Label::builder()
            .label("ENTER: jump  ESC: cancel")
            .halign(Align::Start)
            .hexpand(true)
            .build();
        footer_label.add_css_class("library-picker-footer");

        container.append(&header_box);
        container.append(&scrolled);
        container.append(&footer_label);

        Self { container, scrim, list_box, header_title }
    }

    pub fn show(&self, works: &[(String, String, usize)]) {
        crate::ui::picker_nav::clear_list(&self.list_box);

        self.header_title.set_text(&format!("JUMP TO WORK  ({})", works.len()));

        for (abbrev, title, count) in works {
            let hbox = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(8)
                .hexpand(true)
                .build();

            let title_label = Label::builder()
                .label(title.as_str())
                .halign(Align::Start)
                .hexpand(true)
                .ellipsize(gtk4::pango::EllipsizeMode::End)
                .xalign(0.0)
                .build();

            let count_label = Label::builder()
                .label(&count.to_string())
                .halign(Align::End)
                .build();
            count_label.add_css_class("picker-item-detail");

            hbox.append(&title_label);
            hbox.append(&count_label);

            let row = ListBoxRow::builder().child(&hbox).build();
            row.set_widget_name(abbrev);
            self.list_box.append(&row);
        }

        crate::ui::picker_nav::select_first_row(&self.list_box);

        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.list_box.grab_focus();
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
        self.scrim.set_visible(false);
    }

    pub fn selected_abbrev(&self) -> Option<String> {
        self.list_box
            .selected_row()
            .map(|row| row.widget_name().to_string())
    }

    pub fn move_selection(&self, delta: i32) {
        let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
        let next = current + delta;
        crate::ui::picker_nav::select_row_at(&self.list_box, next);
    }
}
