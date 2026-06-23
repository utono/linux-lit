use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Orientation, Overlay,
};

pub struct ConcordancePicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    words: Vec<(String, usize)>,
}

impl ConcordancePicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(400)
            .height_request(400)
            .build();
        picker_box.add_css_class("concordance-picker");

        let title = Label::builder()
            .label("Vocab Words")
            .halign(gtk4::Align::Start)
            .build();
        title.add_css_class("settings-title");
        picker_box.append(&title);

        let search_entry = Entry::builder()
            .placeholder_text("Filter...")
            .build();
        picker_box.append(&search_entry);

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();
        picker_box.append(&scrolled);

        let footer = Label::builder()
            .label("Type to filter \u{00B7} \u{2191}/\u{2193} navigate \u{00B7} Enter jump \u{00B7} Esc close")
            .build();
        footer.add_css_class("settings-footer");
        picker_box.append(&footer);

        ConcordancePicker {
            overlay,
            picker_box,
            search_entry,
            list_box,
            words: Vec::new(),
        }
    }

    pub fn set_words(&mut self, words: Vec<(String, usize)>) {
        self.words = words;
        self.populate_list("");
    }

    pub fn show(&self) {
        self.picker_box.set_visible(true);
        self.search_entry.grab_focus();
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_panel(&self.overlay, base, None, &self.picker_box);
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    pub fn populate_list(&self, filter: &str) {
        const MAX_VISIBLE: usize = 200;

        crate::ui::picker_nav::clear_list(&self.list_box);

        if filter.is_empty() {
            if let Some(first) = self.list_box.row_at_index(0) {
                self.list_box.select_row(Some(&first));
            }
            return;
        }

        let filter_lower = filter.to_lowercase();
        let mut shown = 0;

        for (word, count) in &self.words {
            if !word.contains(&filter_lower) {
                continue;
            }

            let row_box = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(0)
                .build();
            row_box.add_css_class("settings-row");

            let word_label = Label::builder()
                .label(word)
                .halign(gtk4::Align::Start)
                .hexpand(true)
                .build();

            let count_label = Label::builder()
                .label(&format!("{} occurrence{}", count, if *count == 1 { "" } else { "s" }))
                .halign(gtk4::Align::End)
                .opacity(0.5)
                .build();

            row_box.append(&word_label);
            row_box.append(&count_label);

            let row = ListBoxRow::builder().child(&row_box).build();
            row.set_widget_name(word);
            self.list_box.append(&row);

            shown += 1;
            if shown >= MAX_VISIBLE {
                break;
            }
        }

        if let Some(first) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&first));
        }
    }

    pub fn selected_word(&self) -> Option<String> {
        self.list_box
            .selected_row()
            .map(|row| row.widget_name().to_string())
    }

    pub fn move_selection(&self, delta: i32) {
        if let Some(current) = self.list_box.selected_row() {
            let idx = current.index();
            let new_idx = (idx + delta).max(0);
            crate::ui::picker_nav::select_row_at(&self.list_box, new_idx);
        }
    }
}
