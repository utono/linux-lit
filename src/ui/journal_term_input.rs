use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Entry, ListBox, ListBoxRow, Overlay};

/// Typed-term box + tag-suggestion list for the journal's cross-work "term
/// browse" (Task 4 wires this into `AppState`/dispatch/the `f` key). Modeled
/// on `JournalMovePicker`: an `Entry` primary input over a `ListBox` of
/// distinct existing journal tags, filtered as the user types.
pub struct JournalTermInput {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    pub suggestions: Vec<String>,
}

impl JournalTermInput {
    pub fn new() -> Self {
        let overlay = Overlay::new();
        let picker_box = crate::ui::picker_nav::build_picker_card();

        let search_entry = Entry::builder()
            .placeholder_text("Browse journal by term (type; existing tags suggested)…")
            .build();

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();

        picker_box.append(&search_entry);
        picker_box.append(&scrolled);

        JournalTermInput {
            overlay,
            picker_box,
            search_entry,
            list_box,
            suggestions: Vec::new(),
        }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_panel(&self.overlay, base, None, &self.picker_box);
    }

    /// Replace the suggestion list and repopulate the (unfiltered) list.
    ///
    /// Does NOT clear the entry text: `set_text` synchronously emits `changed`,
    /// whose handler re-borrows `AppState`, so clearing here (under the caller's
    /// borrow) caused a RefCell panic. Clearing the entry is `show()`'s job, and
    /// `open_term_input` calls `show()` only after dropping its borrow. Callers
    /// that need a cleared entry must call `show()` (all current callers do).
    pub fn set_suggestions(&mut self, suggestions: Vec<String>) {
        self.suggestions = suggestions;
        self.populate_list("");
    }

    pub fn show(&self) {
        self.picker_box.set_visible(true);
        self.search_entry.set_text("");
        self.search_entry.grab_focus();
        self.populate_list("");
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    pub fn populate_list(&self, filter: &str) {
        crate::ui::picker_nav::clear_list(&self.list_box);
        let filter_lower = filter.to_lowercase();

        for (idx, term) in self.suggestions.iter().enumerate() {
            if !filter.is_empty()
                && !crate::ui::picker_filter::subsequence_match(&filter_lower, &term.to_lowercase())
            {
                continue;
            }

            let hbox = crate::ui::picker_nav::two_label_row(term, "");
            let row = ListBoxRow::builder().child(&hbox).build();
            row.set_widget_name(&idx.to_string());
            self.list_box.append(&row);
        }

        crate::ui::picker_nav::select_first_row(&self.list_box);
    }

    pub fn move_selection(&self, delta: i32) {
        crate::ui::picker_nav::move_selection_clamped(&self.list_box, delta);
    }

    /// Index into `suggestions` of the selected row (the row's widget_name).
    pub fn selected_index(&self) -> Option<usize> {
        crate::ui::picker_nav::selected_index(&self.list_box)
    }

    /// The term to search: the typed entry text (trimmed) if non-empty, else
    /// the highlighted suggestion. This ordering means a freely-typed term
    /// always wins over a selected row — so the FTS fallback is reachable
    /// even with zero tags. `None` only when both are empty.
    pub fn query_term(&self) -> Option<String> {
        let typed = self.search_entry.text();
        let trimmed = typed.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
        self.selected_index()
            .and_then(|i| self.suggestions.get(i).cloned())
    }
}
