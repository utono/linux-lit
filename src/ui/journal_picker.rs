use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Overlay,
};

use crate::app::JournalBand;

#[derive(Clone)]
pub struct JournalRow {
    pub id: i64,
    pub band: JournalBand,
    pub question_prefix: String,
    pub synopsis_division_label: String,
    /// `Some(work title)` in AUTHOR scope only — it is the one cross-work
    /// list, where two identically-worded questions from different works are
    /// otherwise indistinguishable. `None` in scene/work scope leaves the
    /// row rendering exactly as before.
    pub work_label: Option<String>,
    /// The entry's owning work ABBREV — distinct from `work_label`, which is
    /// a display title and not usable for loading. `Some(abbrev)` only when
    /// the row is a genuinely different work than the one currently loaded
    /// (possible in AUTHOR scope, the one cross-work list); `None` for
    /// scene/work-scope rows and same-work rows in author scope, which
    /// `confirm_picker` keeps on today's exact `land_on_page` path.
    pub work_abbrev: Option<String>,
    /// `Some(surname)` in AUTHOR scope only — the five-column form. `None`
    /// in scene/work scope, which keep the two-column rendering because
    /// author and work are constant there and would be noise.
    pub author_label: Option<String>,
    /// The entry's OWN scope (`passage`/`scene`/`work`/`author`). Shown as
    /// the type column so the header's BROWSING scope is never mistaken for
    /// each row's own scope — the confusion that prompted this change.
    pub type_label: String,
    /// The division column in the WORK'S OWN noun — "Ch. 2" for prose,
    /// "1.4" for a play, "Preface" for prose front matter. Distinct from
    /// `synopsis_division_label`, which already embeds the TYPE ("1.4 passage") and would
    /// print it twice beside `type_label`. AUTHOR scope only.
    pub div_label: String,
    /// The entry's FULL question + answer, lowercased, for the filter to search
    /// beyond what the row displays. Never rendered.
    ///
    /// Without this the filter could only match `question_prefix` — the
    /// 80-char row label — which misses an entry two ways: a term past char 80
    /// of the question, and (for `scope='passage'` rows) any term at all,
    /// because those rows label themselves with the first line of the SOURCE
    /// PASSAGE rather than the question. Searching "remonstrate" on a passage
    /// entry whose question and answer are both about the word returned zero
    /// rows for exactly that reason.
    ///
    /// Lowercased once at build time: `populate_list` runs on every keystroke
    /// and would otherwise re-lowercase the whole corpus per row per keypress.
    pub search_haystack: String,
}

pub struct JournalQaPicker {
    pub overlay: Overlay,
    scrim: GtkBox,
    picker_box: GtkBox,
    header_title: Label,
    search_entry: Entry,
    list_box: ListBox,
    pub items: Vec<JournalRow>,
    column_groups: crate::ui::picker_nav::PickerColumnGroups,
}

impl JournalQaPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = crate::ui::picker_nav::build_picker_card_wide(crate::ui::picker_nav::JOURNAL_PICKER_WIDTH);

        let search_entry = Entry::builder()
            // Scope names track the header, whose tightest label is the work's
            // own division noun (chapter/scene/book) — so name the scopes by
            // what they WIDEN to rather than restating a noun that changes per
            // work.
            .placeholder_text("Filter Q&A pages…   (Alt+t cycles scope: division · work · all)")
            .build();

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();

        let (header_box, header_title) =
            crate::ui::picker_nav::build_picker_header("Q&A PAGES");
        picker_box.append(&header_box);
        picker_box.append(&search_entry);
        picker_box.append(&scrolled);

        JournalQaPicker {
            overlay,
            scrim: crate::ui::picker_nav::build_picker_scrim(),
            picker_box,
            header_title,
            search_entry,
            list_box,
            items: Vec::new(),
            column_groups: crate::ui::picker_nav::PickerColumnGroups::new(),
        }
    }

    /// Retitle the header so the active scope is always visible. Three
    /// different list contents behind one unlabeled title is unreadable.
    pub fn set_header_scope(&self, scope_label: &str) {
        self.header_title.set_label(&format!("Q&A PAGES — {scope_label}"));
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_panel(&self.overlay, base, Some(&self.scrim), &self.picker_box);
    }

    pub fn set_items(&mut self, items: Vec<JournalRow>) {
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

    pub fn is_visible(&self) -> bool {
        self.picker_box.is_visible()
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    pub fn has_items(&self) -> bool {
        !self.items.is_empty()
    }

    pub fn populate_list(&self, filter: &str) {
        crate::ui::picker_nav::clear_list(&self.list_box);

        if self.items.is_empty() {
            let hbox = crate::ui::picker_nav::two_label_row(
                "No Q&A in this scope — Alt+t to widen.",
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
            let primary = match &item.work_label {
                Some(w) => format!("{} · {}", w, item.question_prefix),
                None => item.question_prefix.clone(),
            };
            if !filter.is_empty() {
                // Match rule scales to field LENGTH (see picker_filter::row_matches).
                //
                // SHORT fields stay fuzzy so a surname ("dickens") or a division
                // ("ch. 2") still narrows — the natural gesture on a global
                // cross-work list.
                //
                // The 80-char row label and the multi-thousand-character body
                // are CONTIGUOUS-substring only. A scattered subsequence over
                // prose that long matches almost any short filter, so every row
                // survives and the filter stops filtering.
                //
                // `type_label` belongs with the SHORT fields, deliberately away
                // from the passage prose: while it shared one concatenated
                // target with the label, the "s" in "passage" supplied the
                // leading letter for every "simile" false positive.
                let short_target = format!(
                    "{} {} {} {}",
                    item.author_label.as_deref().unwrap_or(""),
                    item.synopsis_division_label,
                    item.div_label,
                    item.type_label,
                )
                .to_lowercase();
                // `primary` already embeds `work_label`, so the work title still
                // matches literally here (fuzzy work-finding lives in the
                // library picker).
                let long_target = primary.to_lowercase();
                let hit = crate::ui::picker_filter::row_matches(
                    &filter_lower,
                    &short_target,
                    &long_target,
                    &item.search_haystack,
                );
                if !hit {
                    continue;
                }
            }

            let hbox = match (&item.author_label, &item.work_label) {
                (Some(author), Some(work)) => crate::ui::picker_nav::five_column_row(
                    &self.column_groups,
                    author,
                    work,
                    &item.question_prefix,
                    // `div_label`, NOT `synopsis_division_label` — the latter already
                    // embeds the type ("1.4 passage") and would print it
                    // twice beside the type column.
                    &item.div_label,
                    &item.type_label,
                ),
                // Scene/work scope: byte-identical to before this change.
                _ => crate::ui::picker_nav::two_label_row(&primary, &item.synopsis_division_label),
            };

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
