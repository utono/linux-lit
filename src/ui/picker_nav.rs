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
