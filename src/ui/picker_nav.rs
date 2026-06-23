use gtk4::prelude::*;
use gtk4::ListBox;

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
