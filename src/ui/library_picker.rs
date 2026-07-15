use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Orientation, Overlay,
};

use crate::db::models::WorkSummary;

// ─── Data Structures ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PickerMode {
    Library,
    Recent,
}

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

/// Filter all_groups to only include works in `recent_abbrevs`, ordered by
/// most-recent author access. Within each author group, works are ordered by
/// their position in the recent list.
fn group_works_recent(all_groups: &[AuthorGroup], recent_abbrevs: &[String]) -> Vec<AuthorGroup> {
    let recency: std::collections::HashMap<&str, usize> = recent_abbrevs
        .iter()
        .enumerate()
        .map(|(i, a)| (a.as_str(), i))
        .collect();

    let mut result: Vec<AuthorGroup> = Vec::new();
    for group in all_groups {
        let mut matching: Vec<(usize, WorkSummary)> = group
            .works
            .iter()
            .filter_map(|w| recency.get(w.abbrev.as_str()).map(|&idx| (idx, w.clone())))
            .collect();
        if matching.is_empty() {
            continue;
        }
        matching.sort_by_key(|(idx, _)| *idx);
        result.push(AuthorGroup {
            author: group.author.clone(),
            works: matching.into_iter().map(|(_, w)| w).collect(),
        });
    }

    result.sort_by_key(|g| {
        g.works
            .iter()
            .filter_map(|w| recency.get(w.abbrev.as_str()).copied())
            .min()
            .unwrap_or(usize::MAX)
    });

    result
}

// ─── Struct ──────────────────────────────────────────────────────────────────

pub struct LibraryPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    header_title: Label,
    header_crumb: Label,
    search_entry: Entry,
    list_box: ListBox,
    footer_label: Label,
    scrim: GtkBox,
    all_groups: Vec<AuthorGroup>,
    groups: Vec<AuthorGroup>,
    level: PickerLevel,
    mode: PickerMode,
    coauthored_works: std::collections::HashMap<String, String>,
}

// ─── impl LibraryPicker ──────────────────────────────────────────────────────

impl LibraryPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        // Scrim — sits between base content and the picker box.
        let scrim = crate::ui::picker_nav::build_picker_scrim();

        let picker_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(360)
            .height_request(280)
            .build();
        picker_box.add_css_class("library-picker");

        // Header: title (left) + crumb (right)
        let header_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .build();
        header_box.add_css_class("library-picker-header");

        let header_title = Label::builder()
            .label("")
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();
        header_title.add_css_class("library-picker-title");

        let header_crumb = Label::builder()
            .label("")
            .halign(gtk4::Align::End)
            .build();
        header_crumb.add_css_class("library-picker-crumb");

        header_box.append(&header_title);
        header_box.append(&header_crumb);

        let search_entry = Entry::builder()
            .placeholder_text("Filter authors...")
            .build();

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();

        // Footer: single label, hint text rebuilt on level change.
        let footer_label = Label::builder()
            .label("")
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();
        footer_label.add_css_class("library-picker-footer");

        picker_box.append(&header_box);
        picker_box.append(&search_entry);
        picker_box.append(&scrolled);
        picker_box.append(&footer_label);

        LibraryPicker {
            overlay,
            picker_box,
            header_title,
            header_crumb,
            search_entry,
            list_box,
            footer_label,
            scrim,
            all_groups: Vec::new(),
            groups: Vec::new(),
            level: PickerLevel::Authors,
            mode: PickerMode::Library,
            coauthored_works: std::collections::HashMap::new(),
        }
    }

    pub fn set_coauthored_works(&mut self, map: std::collections::HashMap<String, String>) {
        self.coauthored_works = map;
    }

    pub fn set_works(&mut self, works: Vec<WorkSummary>) {
        self.all_groups = group_works(&works);
        self.groups = self.all_groups.clone();
        self.mode = PickerMode::Library;
        self.level = PickerLevel::Authors;
        self.update_header();
        self.update_footer();
        self.populate_list("");
    }

    /// Prepare picker for showing — mutates level and mode.
    /// Call `show_finish()` after dropping the mutable borrow.
    pub fn show_prepare(&mut self) {
        self.mode = PickerMode::Library;
        self.groups = self.all_groups.clone();
        self.level = PickerLevel::Authors;
    }

    /// Prepare picker for showing in recent mode — filters to recent works only.
    /// Call `show_finish()` after dropping the mutable borrow.
    pub fn show_prepare_recent(&mut self, recent_abbrevs: &[String]) {
        self.mode = PickerMode::Recent;
        self.groups = group_works_recent(&self.all_groups, recent_abbrevs);
        self.level = PickerLevel::Authors;
    }

    /// Complete showing — does GTK widget updates that may trigger signals.
    pub fn show_finish(&self) {
        self.picker_box.set_visible(true);
        self.scrim.set_visible(true);
        self.search_entry.set_placeholder_text(Some("Filter authors..."));
        self.search_entry.set_text("");
        self.search_entry.grab_focus();
        self.update_header();
        self.update_footer();
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
        crate::ui::picker_attach::attach_panel(&self.overlay, base, Some(&self.scrim), &self.picker_box);

        // Responsive sizing: track the toplevel window's allocated size and
        // update the picker_box size_request on every resize. We use a tick
        // callback because the property-notify signals on default-width /
        // default-height only fire when the property is explicitly set via
        // `set_default_size`, not when the user resizes the window.
        // The ~60Hz wakeup is acceptable here: the early-return when (w, h)
        // hasn't changed reduces the steady-state work to a Cell read and
        // two integer comparisons.
        // attach() is one-shot during app construction so we don't worry
        // about re-attachment leaking handlers.
        let picker_box = self.picker_box.clone();
        self.picker_box.connect_realize(move |pb| {
            if let Some(window) = pb.root().and_then(|r| r.downcast::<gtk4::Window>().ok()) {
                let pb = picker_box.clone();
                let last: std::cell::Cell<(i32, i32)> = std::cell::Cell::new((0, 0));
                window.add_tick_callback(move |win, _frame_clock| {
                    let w = win.width().max(1);
                    let h = win.height().max(1);
                    if (w, h) != last.get() {
                        last.set((w, h));
                        let (rw, rh) = responsive_size(w, h);
                        pb.set_size_request(rw, rh);
                    }
                    glib::ControlFlow::Continue
                });
            }
        });
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    pub fn list_box(&self) -> &ListBox {
        &self.list_box
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
        self.update_header();
        self.update_footer();
        self.populate_list("");
        self.search_entry.grab_focus();
    }

    fn update_header(&self) {
        let (title, crumb) = header_text(&self.level, &self.groups, self.mode);
        self.header_title.set_text(&title);
        self.header_crumb.set_text(&crumb);
    }

    fn update_footer(&self) {
        let text = footer_hints(&self.level).join(" · ");
        self.footer_label.set_text(&text);
    }

    pub fn populate_list(&self, filter: &str) {
        crate::ui::picker_nav::clear_list(&self.list_box);

        match &self.level {
            PickerLevel::Authors => {
                if filter.is_empty() {
                    // Show all authors with work counts.
                    for group in &self.groups {
                        self.add_author_row(&group.author, group.works.len());
                    }
                } else {
                    // Show authors whose name matches, plus individual works
                    // whose title/author/abbrev matches — all ranked by score so
                    // the best match highlights first (fixes the loose-subsequence
                    // ordering where an unrelated author outranked a real hit).
                    let filter_lower = filter.to_lowercase();

                    enum Scored<'a> {
                        Author(&'a str, usize),
                        Work(&'a WorkSummary),
                    }
                    let mut rows: Vec<(i32, Scored)> = Vec::new();

                    for group in &self.groups {
                        if let Some(score) = author_name_score(&filter_lower, &group.author) {
                            // Author rows sort above work rows at equal relevance.
                            rows.push((
                                score + 100,
                                Scored::Author(&group.author, group.works.len()),
                            ));
                            continue; // don't also list this author's works individually
                        }
                        for work in &group.works {
                            if let Some(score) = work_score(&filter_lower, work) {
                                rows.push((score, Scored::Work(work)));
                            }
                        }
                    }

                    // Stable sort by score descending; ties keep group order.
                    rows.sort_by(|a, b| b.0.cmp(&a.0));
                    for (_, item) in rows {
                        match item {
                            Scored::Author(author, count) => self.add_author_row(author, count),
                            Scored::Work(work) => self.add_work_row(work),
                        }
                    }
                }
            }
            PickerLevel::Works(author) => {
                if let Some(group) = self.groups.iter().find(|g| &g.author == author) {
                    if filter.is_empty() {
                        for work in &group.works {
                            self.add_work_row(work);
                        }
                    } else {
                        let filter_lower = filter.to_lowercase();
                        let mut scored: Vec<(i32, &WorkSummary)> = group
                            .works
                            .iter()
                            .filter_map(|w| work_score(&filter_lower, w).map(|s| (s, w)))
                            .collect();
                        scored.sort_by(|a, b| b.0.cmp(&a.0));
                        for (_, work) in scored {
                            self.add_work_row(work);
                        }
                    }
                }
            }
        }

        crate::ui::picker_nav::select_first_row(&self.list_box);
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
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .xalign(0.0)
            .build();

        let abbrev_label = Label::builder()
            .label(&work.abbrev)
            .halign(gtk4::Align::End)
            .build();
        abbrev_label.add_css_class("picker-item-detail");

        hbox.append(&title_label);

        if let Some(collaborator) = self.coauthored_works.get(&work.abbrev) {
            let mut cap = collaborator.clone();
            if let Some(first) = cap.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            let coauthor_label = Label::builder()
                .label(&format!("+{}", cap))
                .halign(gtk4::Align::End)
                .build();
            coauthor_label.add_css_class("picker-item-detail");
            hbox.append(&coauthor_label);
        }

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
            let mut count = 0i32;
            while self.list_box.row_at_index(count).is_some() {
                count += 1;
            }
            if count == 0 {
                return;
            }
            let new_idx = ((idx + delta) % count + count) % count;
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

/// Case-insensitive match score of `filter` against a work, taking the BEST of
/// its title / author / abbrev scored SEPARATELY (not a concatenated haystack).
/// Scoring each field on its own prevents a query's chars from scattering across
/// field boundaries (title→author→abbrev) and beating a genuine per-field match —
/// which is what let "romeo" surface unrelated works via `title author abbrev`.
/// `None` when the filter matches none of the three fields.
fn work_score(filter: &str, work: &WorkSummary) -> Option<i32> {
    use crate::ui::picker_filter::match_score;
    let title = work.title.to_lowercase();
    let author = work.author.to_lowercase();
    let abbrev = work.abbrev.to_lowercase();
    // Title is the primary field; author/abbrev matches are demoted so a title
    // hit always outranks an author/abbrev-only hit for the same query.
    [
        match_score(filter, &title),
        match_score(filter, &author).map(|s| s - 400),
        match_score(filter, &abbrev).map(|s| s - 400),
    ]
    .into_iter()
    .flatten()
    .max()
}

/// Case-insensitive subsequence match against title, author, and abbrev.
/// Boolean form retained for tests and any yes/no caller; the picker itself now
/// ranks via `work_score`.
#[cfg_attr(not(test), allow(dead_code))]
fn subsequence_match_work(filter: &str, work: &WorkSummary) -> bool {
    work_score(filter, work).is_some()
}

/// Case-insensitive match score against an author name (`None` = no match).
pub fn author_name_score(filter: &str, author: &str) -> Option<i32> {
    crate::ui::picker_filter::match_score(&filter.to_lowercase(), &author.to_lowercase())
}

/// Case-insensitive subsequence match against an author name.
/// Boolean form retained for tests; the picker ranks via `author_name_score`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn author_name_matches(filter: &str, author: &str) -> bool {
    author_name_score(filter, author).is_some()
}

/// Compute the (title, crumb) header text pair for the given picker level.
/// Title format: "LIBRARY — AUTHORS" or "LIBRARY — <AUTHOR NAME>".
/// Crumb format: "<n> AUTHORS" or "<n> WORKS".
/// Both strings are uppercase because GTK 4 CSS does not support text-transform.
pub(crate) fn header_text(level: &PickerLevel, groups: &[AuthorGroup], mode: PickerMode) -> (String, String) {
    let label = match mode {
        PickerMode::Library => "LIBRARY",
        PickerMode::Recent => "RECENT",
    };
    match level {
        PickerLevel::Authors => (
            format!("{} — AUTHORS", label),
            format!("{} AUTHORS", groups.len()),
        ),
        PickerLevel::Works(author) => {
            let count = groups
                .iter()
                .find(|g| &g.author == author)
                .map(|g| g.works.len())
                .unwrap_or(0);
            (
                format!("{} — {}", label, author.to_uppercase()),
                format!("{} WORKS", count),
            )
        }
    }
}

/// Footer hint strings for the given picker level, in display order.
/// Each entry is one segment; the renderer joins them with " · ".
pub(crate) fn footer_hints(level: &PickerLevel) -> Vec<&'static str> {
    match level {
        PickerLevel::Authors => vec!["↑↓ MOVE", "↵ OPEN", "ESC CLOSE"],
        PickerLevel::Works(_) => vec![
            "↑↓ MOVE",
            "↵ OPEN",
            "BACKSPACE BACK",
            "ESC CLOSE",
        ],
    }
}

/// Compute the picker box size request from the toplevel window's allocated
/// dimensions. Width is 60% of the window clamped to [450, 800]; height is
/// 70% clamped to [350, 700]. Returns (width, height) in pixels.
pub(crate) fn responsive_size(window_w: i32, window_h: i32) -> (i32, i32) {
    let w = (window_w as f32 * 0.6).round() as i32;
    let h = (window_h as f32 * 0.7).round() as i32;
    let w = w.clamp(450, 800);
    let h = h.clamp(350, 700);
    (w, h)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Test-only alias so existing tests can call subsequence_match(filter, work).
    fn subsequence_match(filter: &str, work: &WorkSummary) -> bool {
        subsequence_match_work(filter, work)
    }

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

    #[test]
    fn test_romeo_ranks_real_works_above_scatter() {
        // Regression for the Ctrl+P bug: typing "romeo" must rank the real
        // Romeo works above a loose scatter through "Ralph Waldo Emerson", and
        // above unrelated works that only matched via the old concatenated
        // title+author+abbrev haystack.
        let rom = make_work("Rom", "Romeo and Juliet", "Shakespeare");
        let rom_amb = make_work("Rom-Amb", "Romeo and Juliet (Ambrose)", "Shakespeare");
        let cor = make_work("Cor-Amb", "Coriolanus (Ambrose)", "Shakespeare");

        let romeo = "romeo";
        let rom_s = work_score(romeo, &rom).expect("Rom should match");
        let rom_amb_s = work_score(romeo, &rom_amb).expect("Rom-Amb should match");
        let emerson = author_name_score(romeo, "Ralph Waldo Emerson");

        // Real title matches score positively (Tier-1 substring).
        assert!(rom_s > 500 && rom_amb_s > 500);
        // Coriolanus must NOT match "romeo" per-field (no cross-field scatter).
        assert_eq!(work_score(romeo, &cor), None);
        // If Emerson matches at all it's only a low scatter, well below the works.
        if let Some(e) = emerson {
            assert!(rom_s > e && rom_amb_s > e);
        }
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

    // ── Task 3 tests ──────────────────────────────────────────────────────

    #[test]
    fn test_header_text_for_authors_level() {
        let groups = vec![
            AuthorGroup { author: "Shakespeare".into(), works: vec![] },
            AuthorGroup { author: "Austen".into(), works: vec![] },
        ];
        let (title, crumb) = header_text(&PickerLevel::Authors, &groups, PickerMode::Library);
        assert_eq!(title, "LIBRARY — AUTHORS");
        assert_eq!(crumb, "2 AUTHORS");
    }

    #[test]
    fn test_header_text_for_works_level() {
        let groups = vec![
            AuthorGroup {
                author: "Shakespeare".into(),
                works: vec![
                    make_work("Ham", "Hamlet", "Shakespeare"),
                    make_work("Mac", "Macbeth", "Shakespeare"),
                ],
            },
        ];
        let level = PickerLevel::Works("Shakespeare".into());
        let (title, crumb) = header_text(&level, &groups, PickerMode::Library);
        assert_eq!(title, "LIBRARY — SHAKESPEARE");
        assert_eq!(crumb, "2 WORKS");
    }

    #[test]
    fn test_header_text_for_works_level_unknown_author() {
        let groups: Vec<AuthorGroup> = vec![];
        let level = PickerLevel::Works("Nobody".into());
        let (title, crumb) = header_text(&level, &groups, PickerMode::Library);
        assert_eq!(title, "LIBRARY — NOBODY");
        assert_eq!(crumb, "0 WORKS");
    }

    // ── Task 4 tests ──────────────────────────────────────────────────────

    #[test]
    fn test_footer_hints_for_authors_level() {
        let hints = footer_hints(&PickerLevel::Authors);
        assert_eq!(hints, vec!["↑↓ MOVE", "↵ OPEN", "ESC CLOSE"]);
    }

    #[test]
    fn test_footer_hints_for_works_level() {
        let level = PickerLevel::Works("Shakespeare".into());
        let hints = footer_hints(&level);
        assert_eq!(hints, vec!["↑↓ MOVE", "↵ OPEN", "BACKSPACE BACK", "ESC CLOSE"]);
    }

    // ── Task 5 tests ──────────────────────────────────────────────────────

    #[test]
    fn test_responsive_size_clamps_to_max() {
        // Big window — clamp at the 800x700 ceiling.
        let (w, h) = responsive_size(2400, 1600);
        assert_eq!(w, 800);
        assert_eq!(h, 700);
    }

    #[test]
    fn test_responsive_size_uses_floor() {
        // Tiny window — never go below 450x350.
        let (w, h) = responsive_size(200, 200);
        assert_eq!(w, 450);
        assert_eq!(h, 350);
    }

    #[test]
    fn test_responsive_size_scales_in_middle() {
        // 800x600 -> 60% width = 480, 70% height = 420.
        let (w, h) = responsive_size(800, 600);
        assert_eq!(w, 480);
        assert_eq!(h, 420);
    }

    // ── Recent picker tests ──────────────────────────────────────────────

    #[test]
    fn test_header_text_recent_mode() {
        let groups = vec![
            AuthorGroup { author: "Shakespeare".into(), works: vec![] },
        ];
        let (title, crumb) = header_text(&PickerLevel::Authors, &groups, PickerMode::Recent);
        assert_eq!(title, "RECENT — AUTHORS");
        assert_eq!(crumb, "1 AUTHORS");
    }

    #[test]
    fn test_group_works_recent_filters_to_recent_only() {
        let all_groups = vec![
            AuthorGroup {
                author: "Shakespeare".into(),
                works: vec![
                    make_work("Ham", "Hamlet", "Shakespeare"),
                    make_work("Mac", "Macbeth", "Shakespeare"),
                    make_work("Lear", "King Lear", "Shakespeare"),
                ],
            },
            AuthorGroup {
                author: "Dickens, Charles".into(),
                works: vec![
                    make_work("OT", "Oliver Twist", "Dickens, Charles"),
                    make_work("BH", "Bleak House", "Dickens, Charles"),
                ],
            },
        ];
        let recent = vec!["OT".to_string(), "Ham".to_string()];
        let result = group_works_recent(&all_groups, &recent);

        assert_eq!(result.len(), 2);
        // Dickens first (OT is most recent at index 0)
        assert_eq!(result[0].author, "Dickens, Charles");
        assert_eq!(result[0].works.len(), 1);
        assert_eq!(result[0].works[0].abbrev, "OT");
        // Shakespeare second (Ham is at index 1)
        assert_eq!(result[1].author, "Shakespeare");
        assert_eq!(result[1].works.len(), 1);
        assert_eq!(result[1].works[0].abbrev, "Ham");
    }

    #[test]
    fn test_group_works_recent_empty_list() {
        let all_groups = vec![
            AuthorGroup {
                author: "Shakespeare".into(),
                works: vec![make_work("Ham", "Hamlet", "Shakespeare")],
            },
        ];
        let recent: Vec<String> = vec![];
        let result = group_works_recent(&all_groups, &recent);
        assert!(result.is_empty());
    }
}
