use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};

pub struct ActionPopup {
    pub container: GtkBox,
    rows: Vec<GtkBox>,
    selected: usize,
}

impl ActionPopup {
    pub fn new() -> Self {
        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Start)
            .margin_top(80)
            .width_request(350)
            .build();
        container.add_css_class("action-popup");
        container.set_visible(false);

        // Title
        let title = Label::builder()
            .label("Action")
            .css_classes(vec!["settings-title"])
            .build();
        container.append(&title);

        ActionPopup {
            container,
            rows: Vec::new(),
            selected: 0,
        }
    }

    /// Populate the popup with action names. Built-in actions come first,
    /// then a separator, then external commands.
    pub fn show_actions(&mut self, builtin: &[&str], external: &[(String, String)]) {
        // Clear existing rows (keep the title which is first child)
        while let Some(child) = self.container.last_child() {
            if self.container.first_child().as_ref() == Some(&child) {
                break;
            }
            self.container.remove(&child);
        }
        self.rows.clear();

        for name in builtin {
            let row = self.make_row(name);
            self.container.append(&row);
            self.rows.push(row);
        }

        if !external.is_empty() {
            let sep = Label::builder()
                .label("───────────────")
                .css_classes(vec!["action-separator"])
                .build();
            self.container.append(&sep);

            for (name, _cmd) in external {
                let row = self.make_row(name);
                self.container.append(&row);
                self.rows.push(row);
            }
        }

        // Footer
        let footer = Label::builder()
            .label("Ctrl+n/p navigate · Enter confirm · Esc cancel")
            .css_classes(vec!["settings-footer"])
            .build();
        self.container.append(&footer);

        self.selected = 0;
        self.update_row_highlight();
        self.container.set_visible(true);
    }

    pub fn hide(&mut self) {
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    pub fn move_selection(&mut self, delta: i32) {
        if self.rows.is_empty() {
            return;
        }
        let new = (self.selected as i32 + delta).rem_euclid(self.rows.len() as i32) as usize;
        self.selected = new;
        self.update_row_highlight();
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    fn make_row(&self, label: &str) -> GtkBox {
        let row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(0)
            .css_classes(vec!["settings-row"])
            .build();
        let name_label = Label::builder()
            .label(label)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();
        row.append(&name_label);
        row
    }

    fn update_row_highlight(&self) {
        for (i, row) in self.rows.iter().enumerate() {
            if i == self.selected {
                row.add_css_class("settings-row-selected");
            } else {
                row.remove_css_class("settings-row-selected");
            }
        }
    }
}
