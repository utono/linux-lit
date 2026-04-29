use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Label, ListBox, ListBoxRow, Orientation, Overlay, ScrolledWindow,
};

use crate::theme::Theme;

const NUM_SETTINGS: usize = 7;

#[derive(Clone)]
struct SettingsSnapshot {
    line_spacing: u32,
    column_width: u32,
    text_margins: u32,
    theme_index: usize,
    navigation_mode: crate::config::NavigationMode,
    transition_style: crate::config::TransitionStyle,
    show_cursor_line: bool,
}

pub struct SettingsOverlay {
    pub overlay: Overlay,
    picker_box: GtkBox,
    scrim: GtkBox,
    list_box: ListBox,
    rows: Vec<ListBoxRow>,
    value_labels: Vec<Label>,
    snapshot: SettingsSnapshot,
    themes: Vec<Theme>,
    theme_index: usize,
    navigation_mode: crate::config::NavigationMode,
    transition_style: crate::config::TransitionStyle,
}

impl SettingsOverlay {
    pub fn new(themes: Vec<Theme>, current_theme_name: &str) -> Self {
        let overlay = Overlay::new();

        let scrim = GtkBox::builder()
            .hexpand(true)
            .vexpand(true)
            .build();
        scrim.add_css_class("library-picker-scrim");
        scrim.set_visible(false);

        let picker_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(500)
            .build();
        picker_box.add_css_class("library-picker");

        let header_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .build();
        header_box.add_css_class("library-picker-header");

        let header_title = Label::builder()
            .label("SETTINGS")
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();
        header_title.add_css_class("library-picker-title");

        let header_count = Label::builder()
            .label("7 items")
            .halign(gtk4::Align::End)
            .build();
        header_count.add_css_class("library-picker-crumb");

        header_box.append(&header_title);
        header_box.append(&header_count);

        let list_box = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .build();

        let scrolled = ScrolledWindow::builder()
            .child(&list_box)
            .vexpand(true)
            .build();

        let footer = Label::builder()
            .label("↑↓ MOVE · ←→ ADJUST · r RESET · ↵ CONFIRM · ESC REVERT")
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();
        footer.add_css_class("library-picker-footer");

        let names = [
            "Theme",
            "Line Spacing",
            "Column Width",
            "Text Margins",
            "Navigation",
            "Transition",
            "Cursor Line",
        ];

        let mut rows = Vec::new();
        let mut value_labels = Vec::new();

        for name in &names {
            let row = ListBoxRow::new();
            let hbox = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(0)
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
            value_label.add_css_class("picker-item-detail");

            hbox.append(&name_label);
            hbox.append(&value_label);
            row.set_child(Some(&hbox));
            list_box.append(&row);

            rows.push(row);
            value_labels.push(value_label);
        }

        picker_box.append(&header_box);
        picker_box.append(&scrolled);
        picker_box.append(&footer);

        let theme_index = themes
            .iter()
            .position(|t| t.name == current_theme_name)
            .unwrap_or(0);

        SettingsOverlay {
            overlay,
            picker_box,
            scrim,
            list_box,
            rows,
            value_labels,
            snapshot: SettingsSnapshot {
                line_spacing: 5,
                column_width: 750,
                text_margins: 48,
                theme_index,
                navigation_mode: crate::config::NavigationMode::default(),
                transition_style: crate::config::TransitionStyle::default(),
                show_cursor_line: false,
            },
            themes,
            theme_index,
            navigation_mode: crate::config::NavigationMode::default(),
            transition_style: crate::config::TransitionStyle::default(),
        }
    }

    pub fn show(
        &mut self,
        line_spacing: u32,
        column_width: u32,
        text_margins: u32,
        navigation_mode: crate::config::NavigationMode,
        transition_style: crate::config::TransitionStyle,
        show_cursor_line: bool,
    ) {
        self.snapshot = SettingsSnapshot {
            line_spacing,
            column_width,
            text_margins,
            theme_index: self.theme_index,
            navigation_mode,
            transition_style,
            show_cursor_line,
        };
        self.navigation_mode = navigation_mode;
        self.transition_style = transition_style;
        self.update_displayed_values(
            line_spacing,
            column_width,
            text_margins,
            navigation_mode,
            transition_style,
            show_cursor_line,
        );
        self.update_row_opacity();
        self.list_box.select_row(Some(&self.rows[0]));
        self.picker_box.set_visible(true);
        self.scrim.set_visible(true);
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
        self.scrim.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.picker_box.is_visible()
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.scrim);
        self.overlay.add_overlay(&self.picker_box);
        self.picker_box.set_visible(false);
    }

    pub fn move_selection(&mut self, delta: i32) {
        let current = self
            .list_box
            .selected_row()
            .and_then(|r| r.index().try_into().ok())
            .unwrap_or(0usize);
        let mut new =
            (current as i32 + delta).rem_euclid(NUM_SETTINGS as i32) as usize;
        if new == 5 && self.is_transition_disabled() {
            new = (new as i32 + delta).rem_euclid(NUM_SETTINGS as i32) as usize;
        }
        self.list_box.select_row(Some(&self.rows[new]));
    }

    fn is_transition_disabled(&self) -> bool {
        self.navigation_mode == crate::config::NavigationMode::Scroll
    }

    pub fn adjust_value(
        &mut self,
        delta: i32,
        line_spacing: u32,
        column_width: u32,
        text_margins: u32,
        navigation_mode: crate::config::NavigationMode,
        transition_style: crate::config::TransitionStyle,
        show_cursor_line: bool,
    ) -> SettingsChange {
        let selected = self
            .list_box
            .selected_row()
            .and_then(|r| r.index().try_into().ok())
            .unwrap_or(0usize);
        match selected {
            0 => {
                let len = self.themes.len();
                if len == 0 {
                    return SettingsChange::None;
                }
                let new_idx =
                    (self.theme_index as i32 + delta).rem_euclid(len as i32) as usize;
                self.theme_index = new_idx;
                let theme = &self.themes[new_idx];
                self.value_labels[0]
                    .set_label(&format!("\u{25C0} {} \u{25B6}", theme.display_name));
                SettingsChange::Theme(Box::new(theme.clone()))
            }
            1 => {
                let new_val = (line_spacing as i32 + delta).clamp(0, 20) as u32;
                self.value_labels[1]
                    .set_label(&format!("\u{25C0} {}px \u{25B6}", new_val));
                SettingsChange::LineSpacing(new_val)
            }
            2 => {
                let new_val = (column_width as i32 + delta * 25).clamp(400, 1200) as u32;
                self.value_labels[2]
                    .set_label(&format!("\u{25C0} {}px \u{25B6}", new_val));
                SettingsChange::ColumnWidth(new_val)
            }
            3 => {
                let new_val = (text_margins as i32 + delta * 4).clamp(4, 96) as u32;
                self.value_labels[3]
                    .set_label(&format!("\u{25C0} {}px \u{25B6}", new_val));
                SettingsChange::TextMargins(new_val)
            }
            4 => {
                let new_mode = match navigation_mode {
                    crate::config::NavigationMode::Scroll => {
                        crate::config::NavigationMode::EReader
                    }
                    crate::config::NavigationMode::EReader => {
                        crate::config::NavigationMode::Scroll
                    }
                };
                self.navigation_mode = new_mode;
                let label = match new_mode {
                    crate::config::NavigationMode::Scroll => "Scroll",
                    crate::config::NavigationMode::EReader => "E-Reader",
                };
                self.value_labels[4]
                    .set_label(&format!("\u{25C0} {} \u{25B6}", label));
                self.update_row_opacity();
                SettingsChange::Navigation(new_mode)
            }
            5 => {
                if self.is_transition_disabled() {
                    return SettingsChange::None;
                }
                use crate::config::TransitionStyle;
                let variants = [
                    TransitionStyle::Crossfade,
                    TransitionStyle::Slide,
                    TransitionStyle::Instant,
                ];
                let current_idx = variants
                    .iter()
                    .position(|v| *v == transition_style)
                    .unwrap_or(0);
                let new_idx = (current_idx as i32 + delta)
                    .rem_euclid(variants.len() as i32)
                    as usize;
                let new_style = variants[new_idx];
                self.transition_style = new_style;
                let label = match new_style {
                    TransitionStyle::Crossfade => "Crossfade",
                    TransitionStyle::Slide => "Slide",
                    TransitionStyle::Instant => "Instant",
                };
                self.value_labels[5]
                    .set_label(&format!("\u{25C0} {} \u{25B6}", label));
                SettingsChange::Transition(new_style)
            }
            6 => {
                let new_val = !show_cursor_line;
                let label = if new_val { "On" } else { "Off" };
                self.value_labels[6]
                    .set_label(&format!("\u{25C0} {} \u{25B6}", label));
                SettingsChange::CursorLine(new_val)
            }
            _ => SettingsChange::None,
        }
    }

    pub fn snapshot(
        &self,
    ) -> (
        u32,
        u32,
        u32,
        usize,
        crate::config::NavigationMode,
        crate::config::TransitionStyle,
        bool,
    ) {
        (
            self.snapshot.line_spacing,
            self.snapshot.column_width,
            self.snapshot.text_margins,
            self.snapshot.theme_index,
            self.snapshot.navigation_mode,
            self.snapshot.transition_style,
            self.snapshot.show_cursor_line,
        )
    }

    pub fn set_theme_index(&mut self, idx: usize) {
        self.theme_index = idx;
    }

    pub fn themes(&self) -> &[Theme] {
        &self.themes
    }

    pub fn update_displayed_values(
        &self,
        line_spacing: u32,
        column_width: u32,
        text_margins: u32,
        navigation_mode: crate::config::NavigationMode,
        transition_style: crate::config::TransitionStyle,
        show_cursor_line: bool,
    ) {
        if let Some(theme) = self.themes.get(self.theme_index) {
            self.value_labels[0]
                .set_label(&format!("\u{25C0} {} \u{25B6}", theme.display_name));
        }
        self.value_labels[1].set_label(&format!("\u{25C0} {}px \u{25B6}", line_spacing));
        self.value_labels[2].set_label(&format!("\u{25C0} {}px \u{25B6}", column_width));
        self.value_labels[3].set_label(&format!("\u{25C0} {}px \u{25B6}", text_margins));
        let nav_label = match navigation_mode {
            crate::config::NavigationMode::Scroll => "Scroll",
            crate::config::NavigationMode::EReader => "E-Reader",
        };
        self.value_labels[4].set_label(&format!("\u{25C0} {} \u{25B6}", nav_label));
        let transition_label = match transition_style {
            crate::config::TransitionStyle::Crossfade => "Crossfade",
            crate::config::TransitionStyle::Slide => "Slide",
            crate::config::TransitionStyle::Instant => "Instant",
        };
        self.value_labels[5]
            .set_label(&format!("\u{25C0} {} \u{25B6}", transition_label));
        let cursor_label = if show_cursor_line { "On" } else { "Off" };
        self.value_labels[6]
            .set_label(&format!("\u{25C0} {} \u{25B6}", cursor_label));
    }

    fn update_row_opacity(&self) {
        let disabled = self.is_transition_disabled();
        self.rows[5].set_opacity(if disabled { 0.35 } else { 1.0 });
    }
}

pub enum SettingsChange {
    LineSpacing(u32),
    ColumnWidth(u32),
    TextMargins(u32),
    Theme(Box<Theme>),
    Navigation(crate::config::NavigationMode),
    Transition(crate::config::TransitionStyle),
    CursorLine(bool),
    None,
}
