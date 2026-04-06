use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Entry, Label, Orientation};

pub struct SearchBar {
    pub container: GtkBox,
    entry: Entry,
    counter: Label,
}

impl SearchBar {
    pub fn new() -> Self {
        let container = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .halign(gtk4::Align::Fill)
            .valign(gtk4::Align::End)
            .build();
        container.add_css_class("search-bar");

        let slash_label = Label::builder().label("/").build();
        slash_label.add_css_class("search-slash");

        let entry = Entry::builder()
            .hexpand(true)
            .build();
        entry.add_css_class("search-entry");

        let counter = Label::builder()
            .label("")
            .build();
        counter.add_css_class("search-counter");

        container.append(&slash_label);
        container.append(&entry);
        container.append(&counter);
        container.set_visible(false);

        SearchBar {
            container,
            entry,
            counter,
        }
    }

    pub fn show(&self) {
        self.entry.set_text("");
        self.counter.set_label("");
        self.container.set_visible(true);
        self.entry.grab_focus();
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    pub fn update_counter(&self, current: usize, total: usize) {
        if total == 0 {
            self.counter.set_label("[0/0]");
        } else {
            self.counter.set_label(&format!("[{}/{}]", current + 1, total));
        }
    }

    pub fn query(&self) -> String {
        self.entry.text().to_string()
    }
}
