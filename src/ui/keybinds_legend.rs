//! Shared builder for the per-overlay Ctrl+/ keybind legends (gloss, synopsis,
//! journal). A legend is a centered card whose binds are organized into named
//! groups, laid out in two side-by-side columns so even the longest legend fits
//! on one screen without scrolling. Modeled on `echo_keybinds_overlay`; factored
//! here so the three legends share one layout, font, and color treatment.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, Orientation, Separator};

/// One titled group of `(key, action)` rows. Group titles come from the shared
/// category vocabulary — MRU, Focus, Navigation, TTS, Playback, Editing,
/// "Vim edit mode (…)", Misc — so equivalent binds sit under the same heading
/// in every legend (volume nudges, Esc, and "Ctrl+/ close" always in Misc).
pub type Group<'a> = (&'a str, &'a [(&'a str, &'a str)]);

/// The vim-EDIT-mode group shared verbatim by the gloss and synopsis legends —
/// both front the ONE shared vim engine, so a new vim bind is one edit here.
/// The journal legend keeps its own copy: its Ctrl+v hint deliberately reads
/// "(also in the r ask prompt)" (per-overlay wording, not drift).
// Standard vim motions/ops (i a o, x dd cw, y p, u/redo, :w, :q, …) are
// deliberately unlisted (2026-07-22) — they stay live in the editor; the
// legend keeps only the non-obvious rows.
pub const VIM_EDIT_GROUP: Group<'static> = ("Vim edit mode (after e)", &[
    ("H", "highlight selection — wraps it in <hi>..</hi> (visual; toggles)"),
    ("Ctrl+v", "paste clipboard (also in ask_card prompts)"),
]);

/// A per-overlay Ctrl+/ keybind legend: the centered card built by `build_legend`
/// plus its translucent scrim, with the show/hide/attach lifecycle. One concrete
/// type shared by the gloss, synopsis, and journal legends (audit #50) — each of
/// those overlays now contributes only its own `GROUPS` data + title.
pub struct KeybindsLegend {
    pub container: GtkBox,
    pub scrim: GtkBox,
}

impl KeybindsLegend {
    pub fn new(title: &str, groups: &[Group], mru: Option<Group>) -> Self {
        let (container, scrim) = build_legend(title, groups, mru);
        Self { container, scrim }
    }

    pub fn attach_to(&self, overlay: &gtk4::Overlay) {
        overlay.add_overlay(&self.scrim);
        overlay.add_overlay(&self.container);
    }

    pub fn show(&self) {
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.scrim.set_visible(false);
        self.container.set_visible(false);
    }
}

/// Build a legend `(container, scrim)` with `title` and the given `groups`, laid
/// out across two columns. Groups are distributed left-to-right by row weight so
/// the two columns end up roughly balanced; a group is never split across the
/// gap. When `mru` is given, it renders as a THIRD, top-aligned column at the
/// far right — the upper-right quick-reference block — widening the card to
/// fit. Both widgets start hidden; the caller attaches them to an outer overlay
/// and toggles `show`/`hide`.
pub fn build_legend(title: &str, groups: &[Group], mru: Option<Group>) -> (GtkBox, GtkBox) {
    // Translucent dim (NOT the opaque gloss-scrim) so the parent overlay shows
    // through, dimmed, behind the legend rather than being fully hidden.
    let scrim = GtkBox::builder().hexpand(true).vexpand(true).build();
    scrim.add_css_class("legend-scrim");
    scrim.set_visible(false);

    let container = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(14)
        .halign(Align::Center)
        .valign(Align::Center)
        .build();
    // Match the library picker's parchment card (theme {bg}/{fg}), not the dark
    // picker-box — see .legend-box in theme.rs.
    container.add_css_class("legend-box");
    container.set_visible(false);

    let title_label = Label::builder().label(title).halign(Align::Start).build();
    title_label.add_css_class("legend-title");
    container.append(&title_label);

    // Two columns side by side. Split the groups so the left column holds about
    // half the total rows (counting a heading as ~1 row), keeping groups intact.
    let columns = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(48)
        .build();
    let left = column_box();
    let right = column_box();

    // Choose the group index at which to break into the right column so the two
    // columns are as balanced as possible (minimize |left_weight - right_weight|,
    // counting each heading as ~1 row). Always keep at least one group per side.
    let weights: Vec<usize> = groups.iter().map(|(_, b)| b.len() + 1).collect();
    let total: usize = weights.iter().sum();
    let mut break_at = 1usize;
    let mut best_diff = usize::MAX;
    let mut left_sum = 0usize;
    // Consider every break point except after the last group (so the right
    // column is never empty).
    let last = weights.len().saturating_sub(1);
    for (i, w) in weights.iter().take(last).enumerate() {
        left_sum += w;
        let diff = left_sum.abs_diff(total - left_sum);
        if diff < best_diff {
            best_diff = diff;
            break_at = i + 1;
        }
    }
    for (i, (name, binds)) in groups.iter().enumerate() {
        let target = if i < break_at { &left } else { &right };
        append_group(target, name, binds);
    }

    columns.append(&left);
    columns.append(&right);
    // MRU quick-reference: its own top-aligned column at the far right, so the
    // section reads as the card's upper-right block regardless of how tall the
    // two main columns run.
    if let Some((name, binds)) = mru {
        let mru_col = column_box();
        append_group(&mru_col, name, binds);
        columns.append(&mru_col);
    }
    container.append(&columns);

    (container, scrim)
}

/// A single legend column: vertical box of groups.
fn column_box() -> GtkBox {
    GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(16)
        .valign(Align::Start)
        .width_request(380)
        .build()
}

/// Append one titled group (dim heading + its rows) to a column, with a hairline
/// rule above every group except the column's first.
fn append_group(column: &GtkBox, name: &str, binds: &[(&str, &str)]) {
    if column.first_child().is_some() {
        let rule = Separator::new(Orientation::Horizontal);
        rule.add_css_class("legend-rule");
        column.append(&rule);
    }

    let group = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .build();

    let heading = Label::builder().label(name).halign(Align::Start).build();
    heading.add_css_class("legend-group");
    group.append(&heading);

    for (key, action) in binds {
        let row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .build();
        let key_label = Label::builder()
            .label(*key)
            .halign(Align::Start)
            .width_chars(15)
            .xalign(0.0)
            .build();
        key_label.add_css_class("legend-key");
        let action_label = Label::builder()
            .label(*action)
            .halign(Align::Start)
            .hexpand(true)
            .xalign(0.0)
            .wrap(true)
            .build();
        action_label.add_css_class("legend-action");
        row.append(&key_label);
        row.append(&action_label);
        group.append(&row);
    }

    column.append(&group);
}
