use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, ListBox, Orientation, ScrolledWindow, SizeGroup, SizeGroupMode};

/// Top margin of a top-anchored (`valign: Start`) picker card.
pub(crate) const PICKER_TOP_MARGIN: i32 = 40;
/// Width of the wide top-anchored pickers (line / occurrence lists).
pub(crate) const PICKER_WIDE_W: i32 = 900;
/// Width of the narrow top-anchored pickers (word / voice lists).
pub(crate) const PICKER_NARROW_W: i32 = 675;
/// Scroll cap for a top-anchored picker's list.
pub(crate) const PICKER_LIST_MAX_H: i32 = 750;
/// Fixed width for the JOURNAL pickers, which carry five columns. One
/// constant so the Q&A picker and the journal-move picker cannot drift apart.
/// Other pickers keep `build_picker_card`'s 900.
pub(crate) const JOURNAL_PICKER_WIDTH: i32 = 1180;

/// Build the hidden full-bleed scrim box (`library-picker-scrim`) that sits
/// behind the scrim-style pickers/overlays. Byte-identical (modulo source
/// formatting) at every scrim site.
pub(crate) fn build_picker_scrim() -> GtkBox {
    let scrim = GtkBox::builder().hexpand(true).vexpand(true).build();
    scrim.add_css_class("library-picker-scrim");
    scrim.set_visible(false);
    scrim
}

/// Build the picker header bar (`library-picker-header`) holding a left-aligned
/// hexpanding title (`library-picker-title`) with the given `title` text, and
/// append the title. Returns `(header_box, header_title)` — the caller appends
/// the box into its layout and keeps the title label if it needs to mutate it.
/// EXCLUDED: pickers that add a SECOND header label (settings "N items",
/// library crumb) — a structurally different header, left inline.
pub(crate) fn build_picker_header(title: &str) -> (GtkBox, Label) {
    let header_box = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .hexpand(true)
        .build();
    header_box.add_css_class("library-picker-header");

    let header_title = Label::builder()
        .label(title)
        .halign(Align::Start)
        .hexpand(true)
        .build();
    header_title.add_css_class("library-picker-title");
    header_box.append(&header_title);

    (header_box, header_title)
}

/// Build the standard single-selection `ListBox` wrapped in a vexpanding
/// `ScrolledWindow` — the byte-identical pair every card-style picker's `new()`
/// constructs before appending the scrolled view to its `picker_box`. Returns
/// both (the caller keeps the `list_box` for row population/selection and
/// appends the `scrolled`).
pub(crate) fn new_picker_list() -> (ListBox, ScrolledWindow) {
    let list_box = ListBox::builder()
        .selection_mode(gtk4::SelectionMode::Single)
        .build();

    let scrolled = ScrolledWindow::builder()
        .child(&list_box)
        .vexpand(true)
        .build();

    (list_box, scrolled)
}

/// The vertical outer box for a top-anchored picker card: centered, anchored to
/// the top with `PICKER_TOP_MARGIN`, `width` wide, styled with `css_class`
/// (`"picker-box"` for the dark pickers, `"library-picker"` for the cream-themed
/// voice picker). The caller appends its own header/entry/list and controls
/// visibility.
pub(crate) fn new_top_anchored_picker_box(width: i32, css_class: &str) -> GtkBox {
    let picker_box = GtkBox::new(Orientation::Vertical, 0);
    picker_box.set_halign(Align::Center);
    picker_box.set_valign(Align::Start);
    picker_box.set_margin_top(PICKER_TOP_MARGIN);
    picker_box.set_width_request(width);
    picker_box.add_css_class(css_class);
    picker_box
}

/// Remove every row of `list_box` (the "clear the list before repopulating"
/// loop that every ListBox picker repeats). Scoped to `ListBox` because
/// `remove` lives on `ListBoxExt` with a `ListBox` receiver — a `&impl
/// IsA<Widget>` generic can't call it. The few `Box`-based overlays (vocab
/// popup `content_box`, translation overlay `content_vbox`) repeat the same
/// loop but call `Box::remove`; they are excluded here (a `Box`-typed helper
/// would be a separate, lower-value cut).
pub(crate) fn clear_list(list_box: &ListBox) {
    while let Some(row) = list_box.first_child() {
        list_box.remove(&row);
    }
}

/// Select the row at `index` in `list_box` if it exists; no-op otherwise.
/// The shared tail of every ListBox picker's `move_selection`: callers compute
/// their own target index (preserving each picker's empty-start and clamp rules)
/// and pass it here. `index < 0` or past the end selects nothing (GTK's
/// `row_at_index` returns None) — the existing behavior at every call site.
///
/// Also scrolls the enclosing `ScrolledWindow` so the newly-selected row is
/// visible: a programmatic `select_row` does NOT move the viewport (only
/// keyboard focus-navigation auto-scrolls, which these pickers bypass by driving
/// selection themselves), so Ctrl+n/Ctrl+p past the last/first visible row would
/// otherwise select an off-screen row.
pub(crate) fn select_row_at(list_box: &ListBox, index: i32) {
    if let Some(row) = list_box.row_at_index(index) {
        list_box.select_row(Some(&row));
        scroll_row_into_view(list_box, &row);
    }
}

/// Scroll `list_box`'s vadjustment so `row` is fully visible. `ListBox`
/// implements `Scrollable`, so `adjustment()` is its vertical adjustment.
/// Mirrors the library picker's inline scroll-into-view (the one card picker
/// that had it); every other card picker reaches it through `select_row_at`.
fn scroll_row_into_view(list_box: &ListBox, row: &gtk4::ListBoxRow) {
    if let Some(adj) = list_box.adjustment() {
        if let Some(bounds) = row.compute_bounds(list_box) {
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

/// Select the first row of `list_box` if any (the "select row 0 after populate"
/// block every picker repeats). Thin wrapper over `select_row_at(list_box, 0)`;
/// no-op on an empty list. EXCLUDED at call sites: any `row_at_index(0)` followed
/// by extra per-row logic — only the bare select extracts here.
pub(crate) fn select_first_row(list_box: &ListBox) {
    select_row_at(list_box, 0);
}

/// The `items`-index encoded in the selected row's widget name, or None if no
/// row is selected (or its name doesn't parse). Every picker that stamps each
/// row's `widget_name` with its `items` index reads it back through this exact
/// body. NOTE: this is the widget-name-encoded index, NOT `selected_row().index()`
/// (the row's visual position) — those pickers compute a different value and are
/// excluded.
pub(crate) fn selected_index(list_box: &ListBox) -> Option<usize> {
    list_box
        .selected_row()
        .and_then(|row| row.widget_name().parse::<usize>().ok())
}

/// Build the centered 600×400 `library-picker` card box (Vertical, spacing 4)
/// that the card pickers use as their root `picker_box`. Byte-identical at the
/// gloss/journal/media/bookmark pickers. EXCLUDED: concordance_picker (400-wide,
/// `concordance-picker` css), echo_picker (640×520, spacing 0), and
/// concordance_word_picker (built via `GtkBox::new` + setters, not the builder).
pub(crate) fn build_picker_card() -> GtkBox {
    let picker_box = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .halign(Align::Center)
        .valign(Align::Center)
        .width_request(900)
        .height_request(775)
        .build();
    picker_box.add_css_class("library-picker");
    picker_box
}

/// `build_picker_card` at an explicit width. Used by the journal pickers
/// to apply `JOURNAL_PICKER_WIDTH` without affecting the other eight callers
/// that rely on the 900px default.
pub(crate) fn build_picker_card_wide(width: i32) -> GtkBox {
    let picker_box = build_picker_card();
    picker_box.set_width_request(width);
    picker_box
}

/// Build the two-label picker row: a start-aligned, hexpanding, end-ellipsizing
/// `primary` label + an end-aligned `detail` label with the `picker-item-detail`
/// css class, packed into a Horizontal spacing-8 `GtkBox`. The byte-identical row
/// body of the card pickers (gloss/bookmark/journal). The caller wraps the
/// returned box in a `ListBoxRow` and stamps the (varying) `widget_name`, which
/// stays out of this helper. EXCLUDED: echo pickers (Vertical meta-over-text row)
/// and concordance works/list pickers (explicit per-label margins).
pub(crate) fn two_label_row(primary: &str, detail: &str) -> GtkBox {
    let text_label = Label::builder()
        .label(primary)
        .halign(Align::Start)
        .hexpand(true)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .build();

    let detail_label = Label::builder()
        .label(detail)
        .halign(Align::End)
        .build();
    detail_label.add_css_class("picker-item-detail");

    let hbox = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .build();
    hbox.append(&text_label);
    hbox.append(&detail_label);
    hbox
}

/// Column width groups for the five-column picker rows. One set per picker
/// instance, shared across all its rows — that is what makes the columns line
/// up: every label in a group takes the width of the widest member.
pub(crate) struct PickerColumnGroups {
    author: SizeGroup,
    work: SizeGroup,
    div: SizeGroup,
    kind: SizeGroup,
}

impl PickerColumnGroups {
    pub(crate) fn new() -> Self {
        let mk = || SizeGroup::new(SizeGroupMode::Horizontal);
        Self { author: mk(), work: mk(), div: mk(), kind: mk() }
    }
}

/// `author · work · tag … div kind` with the four fixed columns width-matched
/// across every row via `groups`. The TAG column is the only elastic one: it
/// hexpands and ellipsizes, so a long identifying line shortens rather than
/// pushing the division and type off the right edge.
pub(crate) fn five_column_row(
    groups: &PickerColumnGroups,
    author: &str,
    work: &str,
    tag: &str,
    div: &str,
    kind: &str,
) -> GtkBox {
    let fixed = |text: &str, group: &SizeGroup, css: &str| {
        let l = Label::builder()
            .label(text)
            .halign(Align::Start)
            .xalign(0.0)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        l.add_css_class(css);
        group.add_widget(&l);
        l
    };

    let author_l = fixed(author, &groups.author, "picker-item-detail");
    let work_l = fixed(work, &groups.work, "picker-item-detail");

    let tag_l = Label::builder()
        .label(tag)
        .halign(Align::Start)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .build();

    let div_l = fixed(div, &groups.div, "picker-item-detail");
    let kind_l = fixed(kind, &groups.kind, "picker-item-detail");

    let hbox = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .build();
    hbox.append(&author_l);
    hbox.append(&work_l);
    hbox.append(&tag_l);
    hbox.append(&div_l);
    hbox.append(&kind_l);
    hbox
}

/// `"SPEAKER: first line"` (or just the first line when `speaker` is empty) — the
/// byte-identical display-text computation the gloss and bookmark pickers share
/// for their primary label. EXCLUDED: journal_picker uses a precomputed
/// `question_prefix`, not this speaker form.
pub(crate) fn speaker_prefixed_first_line(speaker: &str, source_text: &str) -> String {
    let first_line = source_text.lines().next().unwrap_or("");
    if speaker.is_empty() {
        first_line.to_string()
    } else {
        format!("{}: {}", speaker, first_line)
    }
}

/// How many rows `list_box` holds. O(1): GTK keeps the child list, and the last
/// child's own index is `len - 1`. 0 on an empty list.
pub(crate) fn row_count(list_box: &ListBox) -> i32 {
    list_box
        .last_child()
        .and_then(|c| c.downcast::<gtk4::ListBoxRow>().ok())
        .map(|r| r.index() + 1)
        .unwrap_or(0)
}

/// Wrap `idx` into `0..count` (count > 0). `-1` becomes the LAST row and `count`
/// becomes the first, so a single step off either end lands on the other end.
/// Pure — unit-tested below.
pub(crate) fn wrap_index(idx: i32, count: i32) -> i32 {
    if count <= 0 {
        return 0;
    }
    idx.rem_euclid(count)
}

/// Move the selection by `delta` rows, FAMILY A: requires a current selection.
/// WRAPS at both ends — Ctrl+p on the first row lands on the last, Ctrl+n on the
/// last lands on the first. No-op if nothing is selected or the list is empty.
/// The byte-identical body of the card pickers
/// (bookmark/gloss/journal/media/concordance). The sibling FAMILY B
/// (`move_selection_from`) differs only in treating "no selection" as a start
/// point; both wrap identically.
pub(crate) fn move_selection_clamped(list_box: &ListBox, delta: i32) {
    if let Some(current) = list_box.selected_row() {
        let count = row_count(list_box);
        if count == 0 {
            return;
        }
        select_row_at(list_box, wrap_index(current.index() + delta, count));
    }
}

/// Move the selection by `delta` rows, FAMILY B: treats "no selection" as index
/// −1, so a first Ctrl+n selects row 0 and a first Ctrl+p selects the LAST row.
/// WRAPS at both ends like FAMILY A. The byte-identical body of the
/// concordance-word/list/works and echo-line pickers. Kept separate from
/// `move_selection_clamped` because the −1 start is load-bearing (those pickers
/// can be shown with nothing selected).
pub(crate) fn move_selection_from(list_box: &ListBox, delta: i32) {
    let count = row_count(list_box);
    if count == 0 {
        return;
    }
    let current = list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
    select_row_at(list_box, wrap_index(current + delta, count));
}

#[cfg(test)]
mod tests {
    use super::wrap_index;

    /// The wrap contract Ctrl+n/Ctrl+p rely on: one step past either end lands
    /// on the other end, and an existing in-range index is untouched.
    #[test]
    fn wrap_index_wraps_both_ends_and_passes_through() {
        // In range: unchanged.
        assert_eq!(wrap_index(0, 5), 0);
        assert_eq!(wrap_index(3, 5), 3);
        assert_eq!(wrap_index(4, 5), 4);
        // Ctrl+n off the end -> first.
        assert_eq!(wrap_index(5, 5), 0);
        // Ctrl+p off the front -> last. Also the FAMILY B "no selection" start:
        // current = -1, delta = -1 -> -2 must still land on a real row.
        assert_eq!(wrap_index(-1, 5), 4);
        assert_eq!(wrap_index(-2, 5), 3);
        // Single-row list: every step stays put.
        assert_eq!(wrap_index(1, 1), 0);
        assert_eq!(wrap_index(-1, 1), 0);
        // Empty list: defined, never panics or divides by zero.
        assert_eq!(wrap_index(0, 0), 0);
        assert_eq!(wrap_index(-1, 0), 0);
    }
}
