use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Orientation, Overlay, ScrolledWindow,
};

use crate::db::models::WorkSummary;

// ─── Data Structures ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum PickerLevel {
    Authors,
    Works(String), // holds the selected author name
}

#[derive(Debug, Clone)]
pub struct AuthorGroup {
    pub author: String,
    pub works: Vec<WorkSummary>,
}

const PINNED_AUTHORS: &[&str] = &["Shakespeare", "Dickens, Charles"];

/// Group a flat list of works into `AuthorGroup`s.
/// Pinned authors appear first (in PINNED_AUTHORS order), then the rest alphabetically.
pub fn group_works(works: &[WorkSummary]) -> Vec<AuthorGroup> {
    // Collect unique authors in the order they first appear so we can stable-sort later.
    let mut author_order: Vec<&str> = Vec::new();
    for w in works {
        if !author_order.contains(&w.author.as_str()) {
            author_order.push(&w.author);
        }
    }

    // Build a map: author → works (preserving per-author order).
    let mut map: std::collections::HashMap<&str, Vec<WorkSummary>> =
        std::collections::HashMap::new();
    for w in works {
        map.entry(&w.author).or_default().push(w.clone());
    }

    // Determine final order: pinned first, then remaining sorted alphabetically.
    let mut ordered: Vec<&str> = Vec::new();
    for &pinned in PINNED_AUTHORS {
        if map.contains_key(pinned) {
            ordered.push(pinned);
        }
    }
    let mut rest: Vec<&str> = author_order
        .iter()
        .filter(|&&a| !PINNED_AUTHORS.contains(&a))
        .copied()
        .collect();
    rest.sort_unstable();
    ordered.extend(rest);

    ordered
        .into_iter()
        .filter_map(|a| {
            map.remove(a).map(|ws| AuthorGroup {
                author: a.to_string(),
                works: ws,
            })
        })
        .collect()
}

// ─── Struct ──────────────────────────────────────────────────────────────────

pub struct LibraryPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    scrim: GtkBox,
    groups: Vec<AuthorGroup>,
    level: PickerLevel,
}

// ─── impl LibraryPicker ──────────────────────────────────────────────────────

impl LibraryPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        // Scrim — sits between base content and the picker box.
        let scrim = GtkBox::builder()
            .hexpand(true)
            .vexpand(true)
            .build();
        scrim.add_css_class("library-picker-scrim");
        scrim.set_visible(false);

        let picker_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(400)
            .height_request(400)
            .build();
        picker_box.add_css_class("library-picker");

        let search_entry = Entry::builder()
            .placeholder_text("Filter authors...")
            .build();

        let list_box = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .build();

        let scrolled = ScrolledWindow::builder()
            .child(&list_box)
            .vexpand(true)
            .build();

        picker_box.append(&search_entry);
        picker_box.append(&scrolled);

        LibraryPicker {
            overlay,
            picker_box,
            search_entry,
            list_box,
            scrim,
            groups: Vec::new(),
            level: PickerLevel::Authors,
        }
    }

    pub fn set_works(&mut self, works: Vec<WorkSummary>) {
        self.groups = group_works(&works);
        self.level = PickerLevel::Authors;
        self.populate_list("");
    }

    /// Prepare picker for showing — mutates level only.
    /// Call `show_finish()` after dropping the mutable borrow.
    pub fn show_prepare(&mut self) {
        self.level = PickerLevel::Authors;
    }

    /// Complete showing — does GTK widget updates that may trigger signals.
    pub fn show_finish(&self) {
        self.picker_box.set_visible(true);
        self.scrim.set_visible(true);
        self.search_entry.set_placeholder_text(Some("Filter authors..."));
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

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.scrim);
        self.overlay.add_overlay(&self.picker_box);
        self.picker_box.set_visible(false);
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    pub fn list_box(&self) -> &ListBox {
        &self.list_box
    }

    #[allow(dead_code)]
    pub fn picker_box(&self) -> &GtkBox {
        &self.picker_box
    }

    pub fn level(&self) -> &PickerLevel {
        &self.level
    }

    /// Set level to a specific author's works — mutates level only.
    /// Call `refresh_after_level_change()` after dropping the mutable borrow.
    pub fn enter_author(&mut self, author: &str) {
        self.level = PickerLevel::Works(author.to_string());
    }

    /// Set level back to authors — mutates level only.
    /// Call `refresh_after_level_change()` after dropping the mutable borrow.
    pub fn go_back_to_authors(&mut self) {
        self.level = PickerLevel::Authors;
    }

    /// Update widgets after a level change. Safe to call under `&self` borrow.
    pub fn refresh_after_level_change(&self) {
        let placeholder = match &self.level {
            PickerLevel::Authors => "Filter authors...",
            PickerLevel::Works(_) => "Filter works...",
        };
        self.search_entry.set_placeholder_text(Some(placeholder));
        self.search_entry.set_text("");
        self.populate_list("");
        self.search_entry.grab_focus();
    }

    pub fn populate_list(&self, filter: &str) {
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        match &self.level {
            PickerLevel::Authors => {
                if filter.is_empty() {
                    // Show all authors with work counts.
                    for group in &self.groups {
                        self.add_author_row(&group.author, group.works.len());
                    }
                } else {
                    // Show authors whose name matches, plus individual works
                    // whose title/abbrev matches (with author context).
                    let filter_lower = filter.to_lowercase();
                    let mut has_rows = false;

                    // First: authors whose name matches the filter
                    for group in &self.groups {
                        if author_name_matches(&filter_lower, &group.author) {
                            self.add_author_row(&group.author, group.works.len());
                            has_rows = true;
                        }
                    }

                    // Second: individual works that match (skip if author already shown)
                    for group in &self.groups {
                        if author_name_matches(&filter_lower, &group.author) {
                            continue; // author already listed above
                        }
                        for work in &group.works {
                            if subsequence_match_work(&filter_lower, work) {
                                self.add_work_row(work);
                                has_rows = true;
                            }
                        }
                    }

                    let _ = has_rows;
                }
            }
            PickerLevel::Works(author) => {
                if let Some(group) = self.groups.iter().find(|g| &g.author == author) {
                    let filter_lower = filter.to_lowercase();
                    for work in &group.works {
                        if filter.is_empty() || subsequence_match_work(&filter_lower, work) {
                            self.add_work_row(work);
                        }
                    }
                }
            }
        }

        if let Some(first) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&first));
        }
    }

    fn add_author_row(&self, author: &str, count: usize) {
        let hbox = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .hexpand(true)
            .build();

        let name_label = Label::builder()
            .label(author)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();

        let count_label = Label::builder()
            .label(count.to_string())
            .halign(gtk4::Align::End)
            .build();
        count_label.add_css_class("picker-item-detail");

        hbox.append(&name_label);
        hbox.append(&count_label);

        let row = ListBoxRow::builder().child(&hbox).build();
        row.set_widget_name(&format!("author:{}", author));
        self.list_box.append(&row);
    }

    fn add_work_row(&self, work: &WorkSummary) {
        let show_author = matches!(&self.level, PickerLevel::Authors);
        let hbox = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .hexpand(true)
            .build();

        let display = if show_author {
            format!("{} — {}", work.title, work.author)
        } else {
            work.title.clone()
        };
        let title_label = Label::builder()
            .label(&display)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();

        let abbrev_label = Label::builder()
            .label(&work.abbrev)
            .halign(gtk4::Align::End)
            .build();
        abbrev_label.add_css_class("picker-item-detail");

        hbox.append(&title_label);
        hbox.append(&abbrev_label);

        let row = ListBoxRow::builder().child(&hbox).build();
        row.set_widget_name(&work.abbrev);
        self.list_box.append(&row);
    }

    pub fn selected_abbrev(&self) -> Option<String> {
        self.list_box.selected_row().and_then(|row| {
            let name = row.widget_name().to_string();
            if name.starts_with("author:") {
                None
            } else {
                Some(name)
            }
        })
    }

    pub fn move_selection(&self, delta: i32) {
        if let Some(current) = self.list_box.selected_row() {
            let idx = current.index();
            let new_idx = (idx + delta).max(0);
            if let Some(row) = self.list_box.row_at_index(new_idx) {
                self.list_box.select_row(Some(&row));
                // Scroll the selected row into view within the ScrolledWindow
                if let Some(adj) = self.list_box.adjustment() {
                    if let Some(bounds) = row.compute_bounds(&self.list_box) {
                        let y = bounds.y() as f64;
                        let row_height = bounds.height() as f64;
                        let page_size = adj.page_size();
                        let current_val = adj.value();

                        if y < current_val {
                            adj.set_value(y);
                        } else if y + row_height > current_val + page_size {
                            adj.set_value(y + row_height - page_size);
                        }
                    }
                }
            }
        }
    }
}

// ─── Helper functions ────────────────────────────────────────────────────────

/// Case-insensitive subsequence match against title, author, and abbrev.
fn subsequence_match_work(filter: &str, work: &WorkSummary) -> bool {
    let target = format!("{} {} {}", work.title, work.author, work.abbrev).to_lowercase();
    subsequence_chars(filter, &target)
}

/// Case-insensitive subsequence match against an author name.
pub fn author_name_matches(filter: &str, author: &str) -> bool {
    let filter_lower = filter.to_lowercase();
    let author_lower = author.to_lowercase();
    subsequence_chars(&filter_lower, &author_lower)
}

fn subsequence_chars(filter: &str, target: &str) -> bool {
    let mut target_chars = target.chars();
    for fc in filter.chars() {
        if !target_chars.any(|tc| tc == fc) {
            return false;
        }
    }
    true
}

// Keep the old name as an alias so existing callers (tests) still compile.
#[allow(dead_code)]
fn subsequence_match(filter: &str, work: &WorkSummary) -> bool {
    subsequence_match_work(filter, work)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_work(abbrev: &str, title: &str, author: &str) -> WorkSummary {
        WorkSummary {
            abbrev: abbrev.to_string(),
            title: title.to_string(),
            author: author.to_string(),
            work_type: "play".to_string(),
        }
    }

    // ── Task 1 tests ──────────────────────────────────────────────────────

    #[test]
    fn test_group_works_by_author() {
        let works = vec![
            make_work("Ham", "Hamlet", "Shakespeare"),
            make_work("Mac", "Macbeth", "Shakespeare"),
            make_work("OT", "Oliver Twist", "Dickens, Charles"),
        ];
        let groups = group_works(&works);

        // Shakespeare is pinned first, then Dickens, Charles is pinned second.
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].author, "Shakespeare");
        assert_eq!(groups[0].works.len(), 2);
        assert_eq!(groups[1].author, "Dickens, Charles");
        assert_eq!(groups[1].works.len(), 1);
    }

    #[test]
    fn test_group_works_unpinned_author_sorted() {
        let works = vec![
            make_work("Z1", "Zorro", "Zola"),
            make_work("A1", "Anna", "Austen"),
            make_work("Ham", "Hamlet", "Shakespeare"),
        ];
        let groups = group_works(&works);

        // Shakespeare pinned first; Austen and Zola sorted alphabetically after.
        assert_eq!(groups[0].author, "Shakespeare");
        assert_eq!(groups[1].author, "Austen");
        assert_eq!(groups[2].author, "Zola");
    }

    // ── Task 2 tests ──────────────────────────────────────────────────────

    #[test]
    fn test_subsequence_match_author_name() {
        assert!(author_name_matches("shk", "Shakespeare"));
        assert!(author_name_matches("dick", "Dickens, Charles"));
        assert!(!author_name_matches("xyz", "Shakespeare"));
        // Case-insensitive
        assert!(author_name_matches("SHK", "shakespeare"));
    }

    #[test]
    fn test_filter_finds_works_across_authors() {
        let works = vec![
            make_work("Ham", "Hamlet", "Shakespeare"),
            make_work("OT", "Oliver Twist", "Dickens, Charles"),
            make_work("DC", "David Copperfield", "Dickens, Charles"),
        ];
        let groups = group_works(&works);

        // "oliver" matches Oliver Twist by Dickens (work-level match)
        let filter = "oliver".to_lowercase();
        let matching_works: Vec<&WorkSummary> = groups
            .iter()
            .flat_map(|g| g.works.iter())
            .filter(|w| subsequence_match_work(&filter, w))
            .collect();
        assert_eq!(matching_works.len(), 1);
        assert_eq!(matching_works[0].title, "Oliver Twist");

        // "ham" matches Hamlet
        let filter2 = "ham".to_lowercase();
        let matching2: Vec<&WorkSummary> = groups
            .iter()
            .flat_map(|g| g.works.iter())
            .filter(|w| subsequence_match_work(&filter2, w))
            .collect();
        assert_eq!(matching2.len(), 1);
        assert_eq!(matching2[0].title, "Hamlet");

        // "dick" matches author name "Dickens, Charles"
        assert!(author_name_matches("dick", "Dickens, Charles"));
        assert!(!author_name_matches("dick", "Shakespeare"));
    }

    // ── Pre-existing tests (keep passing) ────────────────────────────────

    #[test]
    fn test_subsequence_match_exact() {
        let w = make_work("Ham", "Hamlet", "Shakespeare");
        assert!(subsequence_match("hamlet", &w));
    }

    #[test]
    fn test_subsequence_match_partial() {
        let w = make_work("Ham", "Hamlet", "Shakespeare");
        assert!(subsequence_match("hml", &w));
        assert!(subsequence_match("ham", &w));
    }

    #[test]
    fn test_subsequence_match_no_match() {
        let w = make_work("Ham", "Hamlet", "Shakespeare");
        assert!(!subsequence_match("xyz", &w));
    }

    #[test]
    fn test_subsequence_match_author() {
        let w = make_work("Ham", "Hamlet", "Shakespeare");
        assert!(subsequence_match("shk", &w));
    }
}
