use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, ListBox, Orientation, ScrolledWindow};

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
pub(crate) fn select_row_at(list_box: &ListBox, index: i32) {
    if let Some(row) = list_box.row_at_index(index) {
        list_box.select_row(Some(&row));
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
        .width_request(600)
        .height_request(400)
        .build();
    picker_box.add_css_class("library-picker");
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

/// Move the selection by `delta` rows, FAMILY A: requires a current selection and
/// clamps the new index at ≥ 0 (so up-arrow at the top stays on row 0). No-op if
/// nothing is selected. The byte-identical body of the card pickers
/// (bookmark/gloss/journal/media/concordance). The sibling FAMILY B
/// (`move_selection_from`) starts from −1 and does NOT clamp — kept separate
/// because that behavior difference (clamp vs no-clamp) is load-bearing.
pub(crate) fn move_selection_clamped(list_box: &ListBox, delta: i32) {
    if let Some(current) = list_box.selected_row() {
        let idx = current.index();
        let new_idx = (idx + delta).max(0);
        select_row_at(list_box, new_idx);
    }
}

/// Move the selection by `delta` rows, FAMILY B: treats "no selection" as index
/// −1 and adds `delta` with NO lower clamp (an out-of-range index is a no-op in
/// `select_row_at`). The byte-identical body of the concordance-word/list/works
/// and echo-line pickers. See `move_selection_clamped` for FAMILY A; the −1-start
/// and absent `.max(0)` are exactly why these are two helpers, not one.
pub(crate) fn move_selection_from(list_box: &ListBox, delta: i32) {
    let current = list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
    select_row_at(list_box, current + delta);
}
