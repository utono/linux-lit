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
    ///
    /// Keep this VISIBLE even when blank — it is the row's only hexpand child,
    /// so hiding it collapses the right-aligned counter to the left edge. See
    /// `gloss_overlay::hide_citation`.
    pub left: Label,
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

/// Right inset for the footer row — the gap between the page counter's last
/// glyph and the card's right edge.
///
/// 40 (`text_margins`, matching the left label and the running head) read too
/// tight on the counter: measured 41px with `1 / 5` in the journal Q&A card,
/// nothing clipped, but the token crowds the corner. 56 gives it the same
/// visual weight of surrounding space the 24px bottom margin gives it
/// vertically, since a short right-aligned token has no neighbouring text to
/// its right to establish the margin the way a line of prose does on the left.
///
/// Deliberately NOT `text_margins`: the left and right ends of this row hold
/// different kinds of content (a prose-aligned label vs a lone counter) and
/// want different insets. Keep the LEFT at `text_margins` so the footer label
/// stays flush with the body text above it.
const FOOTER_END_INSET: i32 = 56;

/// Build the gloss/journal footer row: a horizontal box (`text_margins` start /
/// [`FOOTER_END_INSET`] end, [`FOOTER_MARGIN_TOP`]/[`FOOTER_MARGIN_BOTTOM`]
/// top/bottom, `gloss-hint` class) holding one left-anchored hexpand label.
/// Left-label visibility and any extra widgets (e.g. the page counter, appended
/// after `left`) are the caller's responsibility.
///
/// There is NO hint label. The row used to carry a right-anchored one for the
/// visual-mode legend ("⇧V/Esc exit · j/k extend · …") and the echoes view's
/// keybind list; both were removed 2026-07-29 along with their setters, so the
/// footer now shows only the caller's left label and counter. Ctrl+/ documents
/// the binds those legends advertised.
pub(crate) fn build_footer_row(text_margins: i32) -> FooterRow {
    let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    container.set_margin_start(text_margins);
    // The counter both callers append after `left` is the RIGHTMOST thing in the
    // row, so this end margin is all that holds it off the card edge. It gets
    // FOOTER_END_INSET rather than the symmetric `text_margins`: the left label
    // starts a line of prose (40px reads correct against the body's left edge),
    // while the counter is a lone 2-4 glyph token that crowds the corner at the
    // same value. Reported 2026-07-29 with `1 / 5` at a measured 41px inset —
    // not clipped (glyphs whole at 6x), just tight.
    container.set_margin_end(FOOTER_END_INSET);
    container.set_margin_top(FOOTER_MARGIN_TOP);
    container.set_margin_bottom(FOOTER_MARGIN_BOTTOM);
    container.add_css_class("gloss-hint");

    let left = Label::new(None);
    left.set_halign(Align::Start);
    left.set_hexpand(true);
    left.add_css_class(FOOTER_LABEL_CLASS);
    container.append(&left);

    FooterRow { container, left }
}

/// CSS class for the footer's own labels (`.gloss-position`: 14px + the theme's
/// dim colour). Callers MUST add it to any label they append to the row —
/// notably the page counter — so the whole row shares one size and colour.
///
/// Both callers `remove_css_class("gloss-hint")` to drop its border-top rule,
/// and that was the ONLY rule styling the row, so every footer label was
/// rendering at GTK's default size in the body-text colour. Two visible
/// consequences, both reported 2026-07-29:
///
/// 1. The counter was near-black (measured ink `87,82,121`, same as body prose)
///    where the header chrome beside it is dim — the footer did not read as
///    chrome.
/// 2. At the inherited default size the digit `5`'s flat top bar landed on a
///    pixel boundary and rasterised at ~13% coverage (`217` against a `250`
///    card) while its stem stayed solid (`109`) — so the bar washed out and the
///    glyph read as missing its top. `3` and `/` were unaffected: their strokes
///    fall differently on the grid. The same `5` in the running head, at a
///    defined size, rasterises its bar solid (`133`). A hinting artifact of the
///    unstyled size, NOT a clip and NOT occlusion — the band above the glyph is
///    pure card colour.
///
/// `.gloss-position` already existed in `theme.rs` for exactly this and was
/// applied to NOTHING (dead CSS since the footer was extracted). Wiring it up
/// fixes both.
pub(crate) const FOOTER_LABEL_CLASS: &str = "gloss-position";
