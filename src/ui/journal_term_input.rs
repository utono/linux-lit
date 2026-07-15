use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Overlay};

/// Typed-term box + tag-suggestion list for the journal's cross-work "term
/// browse" (Task 4 wires this into `AppState`/dispatch/the `f` key). Modeled
/// on `JournalMovePicker`: an `Entry` primary input over a `ListBox` of
/// distinct existing journal tags, filtered as the user types.
pub struct JournalTermInput {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    /// Live one-line hint stating exactly what Return will do — search the
    /// highlighted TAG, or the raw typed TERM when nothing matches. Kept in
    /// sync with `query_term`'s precedence by `update_hint` (called on every
    /// keystroke via `populate_list`, and after each `move_selection`).
    hint: Label,
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

        // Left margin aligns the hint text with the suggestion-row text above
        // it. Rows sit inside the scrolledwindow (.library-picker scrolledwindow
        // padding-left 8px) then the row itself (.library-picker row padding-left
        // 14px) = 22px from the card edge; the hint is a direct card child, so
        // it needs the full 22px to line up. Flush-left looked misaligned.
        let hint = Label::builder().xalign(0.0).margin_start(22).build();
        hint.add_css_class("picker-item-detail");

        let (header_box, _header_title) =
            crate::ui::picker_nav::build_picker_header("JOURNAL TERMS");
        picker_box.append(&header_box);
        picker_box.append(&search_entry);
        picker_box.append(&scrolled);
        picker_box.append(&hint);

        JournalTermInput {
            overlay,
            picker_box,
            search_entry,
            list_box,
            hint,
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
        self.update_hint();
    }

    pub fn move_selection(&self, delta: i32) {
        crate::ui::picker_nav::move_selection_clamped(&self.list_box, delta);
        self.update_hint();
    }

    /// Refresh the hint line to state exactly what Return will do — must mirror
    /// `query_term`'s precedence (selected tag wins, else the typed term).
    pub fn update_hint(&self) {
        let text = match self.query_term() {
            Some(term) => {
                let is_tag = self
                    .selected_index()
                    .and_then(|i| self.suggestions.get(i))
                    .map(|t| *t == term)
                    .unwrap_or(false);
                if is_tag {
                    format!("Return: browse tag \u{201c}{term}\u{201d}")
                } else {
                    format!("Return: search \u{201c}{term}\u{201d} (no tag)")
                }
            }
            None => "Type a term or pick a tag".to_string(),
        };
        self.hint.set_text(&text);
    }

    /// Index into `suggestions` of the selected row (the row's widget_name).
    pub fn selected_index(&self) -> Option<usize> {
        crate::ui::picker_nav::selected_index(&self.list_box)
    }

    /// The term to search. A HIGHLIGHTED SUGGESTION WINS: when the user types a
    /// prefix the list filters and auto-selects the first match, so typing
    /// "fee" and pressing Return searches the whole selected tag "fee simple",
    /// not the prefix "fee" (what the user sees highlighted is what they get).
    /// Only when the typed text matches NO tag (empty list → no selection) does
    /// it fall through to the raw typed text — so a freely-typed term still
    /// reaches the FTS fallback with zero tags. `None` when both are empty.
    pub fn query_term(&self) -> Option<String> {
        if let Some(term) = self
            .selected_index()
            .and_then(|i| self.suggestions.get(i).cloned())
        {
            return Some(term);
        }
        let trimmed = self.search_entry.text().trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }
}
