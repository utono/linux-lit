use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, Overlay};

use crate::theme::Theme;

const NUM_SETTINGS: usize = 4;

#[derive(Clone)]
struct SettingsSnapshot {
    line_spacing: u32,
    column_width: u32,
    text_margins: u32,
    theme_index: usize,
}

pub struct SettingsOverlay {
    pub overlay: Overlay,
    container: GtkBox,
    rows: Vec<GtkBox>,
    value_labels: Vec<Label>,
    selected: usize,
    snapshot: SettingsSnapshot,
    themes: Vec<Theme>,
    theme_index: usize,
}

impl SettingsOverlay {
    pub fn new(themes: Vec<Theme>, current_theme_name: &str) -> Self {
        let overlay = Overlay::new();

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(500)
            .build();
        container.add_css_class("settings-overlay");

        // Title
        let title = Label::builder()
            .label("Settings")
            .css_classes(vec!["settings-title"])
            .build();
        container.append(&title);

        // Setting names
        let names = ["Line Spacing", "Column Width", "Text Margins", "Theme"];

        let mut rows = Vec::new();
        let mut value_labels = Vec::new();

        for name in &names {
            let row = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(0)
                .css_classes(vec!["settings-row"])
                .build();

            let name_label = Label::builder()
                .label(*name)
                .halign(gtk4::Align::Start)
                .hexpand(true)
                .build();

            let value_label = Label::builder()
                .label("")
                .halign(gtk4::Align::End)
                .build();

            row.append(&name_label);
            row.append(&value_label);
            container.append(&row);

            rows.push(row);
            value_labels.push(value_label);
        }

        // Footer
        let footer = Label::builder()
            .label("j/k navigate · h/l adjust · r reset · Enter confirm · Esc revert")
            .css_classes(vec!["settings-footer"])
            .build();
        container.append(&footer);

        // Find current theme index
        let theme_index = themes
            .iter()
            .position(|t| t.name == current_theme_name)
            .unwrap_or(0);

        SettingsOverlay {
            overlay,
            container,
            rows,
            value_labels,
            selected: 0,
            snapshot: SettingsSnapshot {
                line_spacing: 4,
                column_width: 950,
                text_margins: 48,
                theme_index,
            },
            themes,
            theme_index,
        }
    }

    pub fn show(&mut self, line_spacing: u32, column_width: u32, text_margins: u32) {
        self.snapshot = SettingsSnapshot {
            line_spacing,
            column_width,
            text_margins,
            theme_index: self.theme_index,
        };
        self.selected = 0;
        self.update_displayed_values(line_spacing, column_width, text_margins);
        self.update_row_highlight();
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.container);
        self.container.set_visible(false);
    }

    pub fn move_selection(&mut self, delta: i32) {
        let new = (self.selected as i32 + delta).rem_euclid(NUM_SETTINGS as i32) as usize;
        self.selected = new;
        self.update_row_highlight();
    }

    /// Adjust the currently selected setting. Returns the new values to apply live.
    /// For theme changes, returns the new Theme to apply.
    pub fn adjust_value(
        &mut self,
        delta: i32,
        line_spacing: u32,
        column_width: u32,
        text_margins: u32,
    ) -> SettingsChange {
        match self.selected {
            0 => {
                // Line Spacing: 0-20, step 1
                let new_val = (line_spacing as i32 + delta).clamp(0, 20) as u32;
                self.value_labels[0].set_label(&format!("\u{25C0} {}px \u{25B6}", new_val));
                SettingsChange::LineSpacing(new_val)
            }
            1 => {
                // Column Width: 400-1200, step 50
                let new_val = (column_width as i32 + delta * 50).clamp(400, 1200) as u32;
                self.value_labels[1].set_label(&format!("\u{25C0} {}px \u{25B6}", new_val));
                SettingsChange::ColumnWidth(new_val)
            }
            2 => {
                // Text Margins: 8-96, step 4
                let new_val = (text_margins as i32 + delta * 4).clamp(8, 96) as u32;
                self.value_labels[2].set_label(&format!("\u{25C0} {}px \u{25B6}", new_val));
                SettingsChange::TextMargins(new_val)
            }
            3 => {
                // Theme: cycle through loaded themes
                let len = self.themes.len();
                if len == 0 {
                    return SettingsChange::None;
                }
                let new_idx = (self.theme_index as i32 + delta).rem_euclid(len as i32) as usize;
                self.theme_index = new_idx;
                let theme = &self.themes[new_idx];
                self.value_labels[3].set_label(&format!("\u{25C0} {} \u{25B6}", theme.display_name));
                SettingsChange::Theme(Box::new(theme.clone()))
            }
            _ => SettingsChange::None,
        }
    }

    pub fn snapshot(&self) -> (u32, u32, u32, usize) {
        (
            self.snapshot.line_spacing,
            self.snapshot.column_width,
            self.snapshot.text_margins,
            self.snapshot.theme_index,
        )
    }

    pub fn set_theme_index(&mut self, idx: usize) {
        self.theme_index = idx;
    }

    pub fn themes(&self) -> &[Theme] {
        &self.themes
    }

    pub fn update_displayed_values(&self, line_spacing: u32, column_width: u32, text_margins: u32) {
        self.value_labels[0].set_label(&format!("\u{25C0} {}px \u{25B6}", line_spacing));
        self.value_labels[1].set_label(&format!("\u{25C0} {}px \u{25B6}", column_width));
        self.value_labels[2].set_label(&format!("\u{25C0} {}px \u{25B6}", text_margins));
        if let Some(theme) = self.themes.get(self.theme_index) {
            self.value_labels[3].set_label(&format!("\u{25C0} {} \u{25B6}", theme.display_name));
        }
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

pub enum SettingsChange {
    LineSpacing(u32),
    ColumnWidth(u32),
    TextMargins(u32),
    Theme(Box<Theme>),
    None,
}
