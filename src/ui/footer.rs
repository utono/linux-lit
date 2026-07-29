use gtk4::prelude::*;
use gtk4::{Align, Label};

/// Handles to the shared "gloss-hint" footer row. The caller sets the left
/// label's text/visibility and may append further widgets (e.g. the gloss
/// position counter) into `container` before/after appending it to the column.
pub(crate) struct FooterRow {
    /// The footer box (`gloss-hint` class). Append to your card column.
    pub container: gtk4::Box,
    /// Left-anchored, hexpand label (citation / work-scene). Caller sets text
    /// and visibility, and typically stores it in its own struct for updates.
    pub left: Label,
    /// Right-anchored fixed keybind hint (text set from `hint_text`).
    pub hint: Label,
}

/// Gap ABOVE the footer text — the breathing room between the body's last line
/// and the footer labels. Matches the head labels' `margin_bottom(12)` in the
/// journal/gloss running-head rows, so body-to-chrome spacing reads the same at
/// both ends of the card.
const FOOTER_MARGIN_TOP: i32 = 12;

/// Gap BELOW the footer text, against the card's bottom edge. Mirrors the head
/// labels' `margin_top(24)` (journal_overlay.rs `head_row`, gloss_overlay.rs
/// `title`) so the foot strip is as deep as the head strip.
///
/// Was 12 — symmetric with [`FOOTER_MARGIN_TOP`] — which left the counters only
/// ~14px above the card edge against the head's ~29px, and read as clipped
/// against the bottom rule. The main reading card moved 30px from its running
/// head to its foot over 2026-07-28 (`TOP_SPACER_HEIGHT` 74 -> 44 against
/// `SINGLE_COLUMN_BOTTOM_MARGIN`/`TWO_COLUMN_BOTTOM_MARGIN` 22 -> 52); these
/// overlays already mirrored the head side at 24 but never gained the matching
/// foot, so they kept the pre-shift proportions.
///
/// Costs no text: both callers' scroll budgets read the footer's
/// `preferred_size()` LIVE (`journal_overlay::size_card`,
/// `gloss_overlay::size_scroll`), so the extra height comes out of the scroll
/// viewport automatically and the row grid stays consistent.
const FOOTER_MARGIN_BOTTOM: i32 = 24;

/// Build the gloss/journal footer row: a horizontal box (text_margins sides,
/// [`FOOTER_MARGIN_TOP`]/[`FOOTER_MARGIN_BOTTOM`] top/bottom, `gloss-hint`
/// class) with a left-anchored hexpand label and a right-anchored hint label
/// (`halign End`, `margin_end 12`). Left then hint are appended into the box.
/// Left-label visibility and any extra widgets are the caller's responsibility.
pub(crate) fn build_footer_row(text_margins: i32, hint_text: &str) -> FooterRow {
    let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    container.set_margin_start(text_margins);
    container.set_margin_end(text_margins);
    container.set_margin_top(FOOTER_MARGIN_TOP);
    container.set_margin_bottom(FOOTER_MARGIN_BOTTOM);
    container.add_css_class("gloss-hint");

    let left = Label::new(None);
    left.set_halign(Align::Start);
    left.set_hexpand(true);
    container.append(&left);

    let hint = Label::new(Some(hint_text));
    hint.set_halign(Align::End);
    hint.set_margin_end(12);
    container.append(&hint);

    FooterRow { container, left, hint }
}
