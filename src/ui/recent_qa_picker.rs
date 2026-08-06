use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Entry, ListBox, ListBoxRow, Overlay};

/// One row in the recent-Q&A jump-back picker (Ctrl+a). Cross-work: each row
/// carries the entry id AND its work so confirm can load the right edition.
#[derive(Clone)]
pub struct RecentQaRow {
    pub id: i64,
    pub work_abbrev: String,
    pub work_label: String,
    pub question_prefix: String,
}

/// Cross-work "recent Q&A" picker, modelled on `JournalQaPicker` but sorted
/// newest-first across every work (the query already orders it — never re-sort)
/// and work-labeled per row. An `add_overlay` layer, never in the size-bearing
/// chain.
pub struct RecentQaPicker {
    pub overlay: Overlay,
    scrim: GtkBox,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    pub items: Vec<RecentQaRow>,
}

impl RecentQaPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = crate::ui::picker_nav::build_picker_card();

        let search_entry = Entry::builder()
            .placeholder_text("Filter recent Q&A...")
            .build();

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();

        let (header_box, _header_title) =
            crate::ui::picker_nav::build_picker_header("RECENT Q&A");
        picker_box.append(&header_box);
        picker_box.append(&search_entry);
        picker_box.append(&scrolled);

        RecentQaPicker {
            overlay,
            scrim: crate::ui::picker_nav::build_picker_scrim(),
            picker_box,
            search_entry,
            list_box,
            items: Vec::new(),
        }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_panel(
            &self.overlay,
            base,
            Some(&self.scrim),
            &self.picker_box,
        );
    }

    pub fn set_items(&mut self, items: Vec<RecentQaRow>) {
        self.items = items;
        self.populate_list("");
    }

    pub fn show(&self) {
        self.scrim.set_visible(true);
        self.picker_box.set_visible(true);
        self.search_entry.set_text("");
        self.search_entry.grab_focus();
        self.populate_list("");
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
        self.scrim.set_visible(false);
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    /// Rebuild the visible rows for `filter`. Preserves the query's newest-first
    /// order (no re-sort). Empty item list -> one non-selectable empty-state row.
    pub fn populate_list(&self, filter: &str) {
        crate::ui::picker_nav::clear_list(&self.list_box);

        if self.items.is_empty() {
            let hbox = crate::ui::picker_nav::two_label_row(
                "No Q&A yet — press Ctrl+a after asking.",
                "",
            );
            let row = ListBoxRow::builder().child(&hbox).build();
            row.set_selectable(false);
            row.set_activatable(false);
            self.list_box.append(&row);
            return;
        }

        let filter_lower = filter.to_lowercase();

        for (idx, item) in self.items.iter().enumerate() {
            if !filter.is_empty() {
                // Same length-scaled rule as the journal Q&A picker: the short
                // work label stays fuzzy, the 80-char question label is
                // contiguous-substring only. This picker carries no body
                // haystack, so it passes "".
                let short_target = item.work_label.to_lowercase();
                let long_target = item.question_prefix.to_lowercase();
                if !crate::ui::picker_filter::row_matches(
                    &filter_lower,
                    &short_target,
                    &long_target,
                    "",
                ) {
                    continue;
                }
            }

            let hbox = crate::ui::picker_nav::two_label_row(
                &item.work_label,
                &item.question_prefix,
            );

            let row = ListBoxRow::builder().child(&hbox).build();
            row.set_widget_name(&idx.to_string());
            self.list_box.append(&row);
        }

        crate::ui::picker_nav::select_first_row(&self.list_box);
    }

    pub fn move_selection(&self, delta: i32) {
        crate::ui::picker_nav::move_selection_clamped(&self.list_box, delta);
    }

    /// Index into `items` of the selected row (the row's widget_name).
    pub fn selected_index(&self) -> Option<usize> {
        crate::ui::picker_nav::selected_index(&self.list_box)
    }
}
