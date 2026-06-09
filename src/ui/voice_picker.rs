use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Orientation, ScrolledWindow,
};

use crate::elevenlabs::VoiceInfo;

/// Fuzzy picker for choosing the preferred ElevenLabs TTS voice. Lists the
/// account's voices (fetched live), each badged free (premade) or paid. Opened
/// from the settings overlay.
///
/// This is an `add_overlay` panel (`picker_box`), added onto the existing
/// outer overlay in `app.rs` — NOT wrapped into the reader's size-bearing
/// widget chain (wrapping collapses the reader layout; see the memory-bank
/// "pickers: overlay, not chain link" rule). Mirrors `EchoLinePicker`.
pub struct VoicePicker {
    pub picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    voices: Vec<VoiceInfo>,
    associated: Vec<String>,
}

impl VoicePicker {
    pub fn new() -> Self {
        let picker_box = GtkBox::new(Orientation::Vertical, 0);
        picker_box.set_halign(Align::Center);
        picker_box.set_valign(Align::Start);
        picker_box.set_margin_top(40);
        picker_box.set_width_request(450);
        // Use the shared themed picker style (cream card, themed entry/rows),
        // matching the library/media/gloss pickers — not the old dark picker-box.
        picker_box.add_css_class("library-picker");
        picker_box.set_visible(false);

        let search_entry = Entry::new();
        search_entry.set_placeholder_text(Some("Search voices..."));
        picker_box.append(&search_entry);

        let scrolled = ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_max_content_height(400);
        scrolled.set_propagate_natural_height(true);

        let list_box = ListBox::new();
        scrolled.set_child(Some(&list_box));
        picker_box.append(&scrolled);

        Self {
            picker_box,
            search_entry,
            list_box,
            voices: Vec::new(),
            associated: Vec::new(),
        }
    }

    /// Mark which voice ids are already in the current gloss's set (✓ badge).
    pub fn set_associated(&mut self, ids: Vec<String>) {
        self.associated = ids;
        if self.is_visible() {
            let filter = self.search_entry.text().to_string();
            self.populate_list(&filter);
        }
    }

    pub fn show(&self) {
        self.search_entry.set_text("");
        self.populate_list("");
        self.picker_box.set_visible(true);
        self.search_entry.grab_focus();
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.picker_box.is_visible()
    }

    pub fn set_voices(&mut self, voices: Vec<VoiceInfo>) {
        self.voices = voices;
        // Refresh if currently shown so a deferred fetch result appears.
        if self.is_visible() {
            let filter = self.search_entry.text().to_string();
            self.populate_list(&filter);
        }
    }

    /// Show a transient one-row message (e.g. "Loading voices…", "No voices").
    pub fn set_status(&self, msg: &str) {
        while let Some(row) = self.list_box.first_child() {
            self.list_box.remove(&row);
        }
        let label = Label::new(Some(msg));
        label.set_halign(Align::Start);
        label.add_css_class("picker-item-title");
        let row = ListBoxRow::new();
        row.set_child(Some(&label));
        row.set_selectable(false);
        self.list_box.append(&row);
    }

    pub fn filter_changed(&self) {
        let filter = self.search_entry.text().to_string();
        self.populate_list(&filter);
    }

    fn populate_list(&self, filter: &str) {
        while let Some(row) = self.list_box.first_child() {
            self.list_box.remove(&row);
        }

        let filter_lower = filter.to_lowercase();
        for voice in &self.voices {
            if !filter_lower.is_empty() && !voice.name.to_lowercase().contains(&filter_lower) {
                continue;
            }

            let row_box = GtkBox::new(Orientation::Horizontal, 8);
            row_box.set_margin_start(8);
            row_box.set_margin_end(8);
            row_box.set_margin_top(2);
            row_box.set_margin_bottom(2);

            let name_label = Label::new(Some(&voice.name));
            name_label.set_halign(Align::Start);
            name_label.set_hexpand(true);
            name_label.add_css_class("picker-item-title");
            row_box.append(&name_label);

            // Free (premade) vs paid badge so the user can avoid a voice that
            // will always 402-fall-back to Alice on a free plan.
            let badge = Label::new(Some(if voice.is_free_tier_safe() {
                "free"
            } else {
                "paid"
            }));
            badge.set_halign(Align::End);
            badge.add_css_class("picker-item-detail");
            row_box.append(&badge);

            if self.associated.iter().any(|id| id == &voice.voice_id) {
                let assoc = Label::new(Some("\u{2713}")); // ✓ associated
                assoc.set_halign(Align::End);
                assoc.add_css_class("picker-item-detail");
                row_box.append(&assoc);
            }

            let row = ListBoxRow::new();
            row.set_child(Some(&row_box));
            // Stash the voice id in the widget name so confirm can read it.
            row.set_widget_name(&voice.voice_id);
            self.list_box.append(&row);
        }

        if let Some(row) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&row));
        }
    }

    /// The selected voice: (id, display name, is_free_tier_safe).
    pub fn selected_voice(&self) -> Option<(String, String, bool)> {
        let id = self
            .list_box
            .selected_row()
            .map(|row| row.widget_name().to_string())?;
        let (name, safe) = self
            .voices
            .iter()
            .find(|v| v.voice_id == id)
            .map(|v| (v.name.clone(), v.is_free_tier_safe()))
            .unwrap_or_else(|| (id.clone(), true));
        Some((id, name, safe))
    }

    pub fn move_selection(&self, delta: i32) {
        let current = self
            .list_box
            .selected_row()
            .map(|r| r.index())
            .unwrap_or(-1);
        let next = current + delta;
        if let Some(row) = self.list_box.row_at_index(next) {
            self.list_box.select_row(Some(&row));
        }
    }

    pub fn entry(&self) -> &Entry {
        &self.search_entry
    }
}
