//! Shared builder for the per-overlay Ctrl+/ keybind legends (gloss, synopsis,
//! journal, chat, echo). A legend is a centered card whose binds are organized
//! into named groups, laid out in AT MOST TWO side-by-side columns so the card
//! reads narrow-and-tall — a short measure scanned downward rather than a
//! near-full-screen band (2026-08-01; it was three columns and ~1400px wide).
//! Modeled on `echo_keybinds_overlay`; factored here so the legends share one
//! layout, font, and color treatment.
//!
//! Two things bound the card, and both were learned by pixel-measuring rather
//! than from the widget API: the WIDTH comes from `ACTION_MAX_CHARS` (a column
//! `width_request` is only a minimum, so `wrap(true)` alone let labels run to
//! their natural width), and the HEIGHT is kept on-screen by spilling the tail
//! groups under the MRU column once the left column exceeds `MAX_LEFT_ROWS`.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, Orientation, PolicyType, ScrolledWindow, Separator};

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

/// Build a legend `(container, scrim)` with `title` and the given `groups`.
///
/// The card is deliberately NARROW AND TALL (2026-08-01): at most two columns,
/// each `COLUMN_W` wide, so the eye scans down a short measure instead of across
/// a near-full-screen band. Layout depends on whether an `mru` group is given:
///
/// - **With `mru`** — the `groups` stack down the left column and the MRU
///   quick-reference owns the TOP of the right one. Roughly doubles the height
///   versus the old 3-column card, which is the point. Once the groups exceed
///   `MAX_LEFT_ROWS` the tail ones spill UNDER the MRU, into what would
///   otherwise be dead space, so a long legend still fits the viewport.
/// - **Without `mru`** (echo, chat) — the groups split across the two columns by
///   row weight, balanced as evenly as possible without splitting a group, so a
///   legend with no MRU block doesn't become one absurdly tall column.
///
/// Both widgets start hidden; the caller attaches them to an outer overlay and
/// toggles `show`/`hide`.
pub fn build_legend(title: &str, groups: &[Group], mru: Option<Group>) -> (GtkBox, GtkBox) {
    // TRANSPARENT (2026-08-01). This used to be a 30% black wash, back when the
    // parent overlay stayed visible behind the legend and wanted dimming. The
    // parent card is now hidden outright (`suspend_for_legend`), leaving only
    // the parent's opaque root-colored scrim behind us — so a black wash has
    // nothing left to dim and instead visibly DARKENED the root, making the
    // legend's backdrop a different color from the reader's. The layer is kept
    // (rather than deleted) as the legend's full-bleed input/hit-test area.
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

    // At most two columns side by side.
    let columns = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(COLUMN_GAP)
        .build();
    let left = column_box();

    if let Some((name, binds)) = mru {
        // MRU present: it owns the TOP of the right column, and the groups
        // stack down the left one. If the groups alone would overrun the
        // viewport, the tail groups spill UNDER the MRU — the right column's
        // lower half is otherwise dead space, and the alternative is the
        // scroller silently hiding rows (journal lost its "Esc" and "Ctrl+/
        // close this legend" rows that way).
        let mru_col = column_box();
        append_group(&mru_col, name, binds);

        let weights: Vec<usize> = groups.iter().map(|(_, b)| b.len() + 1).collect();
        let group_rows: usize = weights.iter().sum();
        let spill_at = if group_rows > MAX_LEFT_ROWS {
            // Fill the left column up to the row budget, then spill the rest.
            let mut acc = 0usize;
            groups
                .iter()
                .position(|(_, b)| {
                    acc += b.len() + 1;
                    acc > MAX_LEFT_ROWS
                })
                .unwrap_or(groups.len())
        } else {
            groups.len()
        };

        for (i, (gname, gbinds)) in groups.iter().enumerate() {
            let target = if i < spill_at { &left } else { &mru_col };
            append_group(target, gname, gbinds);
        }
        columns.append(&left);
        columns.append(&mru_col);
    } else {
        // No MRU: balance the groups across the two columns so the card doesn't
        // run off the bottom of the screen. Break where the two halves are
        // closest in row weight (a heading counts as ~1 row), never splitting a
        // group and never leaving the right column empty.
        let right = column_box();
        let weights: Vec<usize> = groups.iter().map(|(_, b)| b.len() + 1).collect();
        let total: usize = weights.iter().sum();
        let mut break_at = 1usize;
        let mut best_diff = usize::MAX;
        let mut left_sum = 0usize;
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
    }

    // Backstop for the card's height. The spill rule above is what normally
    // keeps a legend on screen; this catches anything that still overruns.
    // `propagate_natural_height` keeps legends that already fit (all five as of
    // 2026-08-01) sized to their content, so this only engages for a future
    // legend long enough to need it. NOTE a scrolled row is an INVISIBLE row —
    // there is no always-on scrollbar to hint at it — so prefer tuning
    // MAX_LEFT_ROWS over relying on this.
    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .propagate_natural_height(true)
        .propagate_natural_width(true)
        .max_content_height(MAX_CARD_H)
        .child(&columns)
        .build();
    container.append(&scroller);

    (container, scrim)
}

/// Wrap width of the action label, in characters. THIS is what bounds the
/// card's width — a `width_request` on the column is only a minimum, so the
/// labels' natural width is what actually sets the measure. Narrowed
/// (2026-08-01) to make the card read as narrow-and-tall.
const ACTION_MAX_CHARS: i32 = 40;

/// Width of one legend column. Both columns get the SAME request so they
/// divide the card evenly — without it the MRU column ended up visibly
/// narrower than the groups column and wrapped far more aggressively
/// ("corpus_search: all / Q&As / glosses" over two lines).
const COLUMN_W: i32 = 430;

/// Gap between the two legend columns.
const COLUMN_GAP: i32 = 40;

/// Maximum height of the scrolling column area. Sized so the gloss legend (30
/// rows) fits WHOLE — its last row, "Ctrl+/ close this legend", was landing
/// below the fold at 1020 — while still leaving room for the title, the card's
/// 32px padding, and a margin off the edges of the 1236px production viewport.
/// With the spill-into-the-MRU-column rule below, this is a backstop rather
/// than the normal case.
const MAX_CARD_H: i32 = 1090;

/// Row budget for the left column before groups spill under the MRU. Counts a
/// heading as ~1 row, matching the balancing weights. Measured against the real
/// legends at 1920x1236: journal weighs 31 and overran the viewport (its "Esc"
/// and "Ctrl+/ close this legend" rows fell below the fold), gloss weighs 28
/// and fits whole at 1155px. So the budget sits at 28 — gloss still stacks in
/// one column, journal spills its Misc group under the MRU. Note the weight
/// UNDERCOUNTS
/// height, since a long action wraps to two lines; this is why the threshold is
/// well under what a naive rows-per-pixel estimate would suggest.
const MAX_LEFT_ROWS: usize = 28;

/// A single legend column: vertical box of groups.
fn column_box() -> GtkBox {
    GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(16)
        .valign(Align::Start)
        .width_request(COLUMN_W)
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
        // Keys wrap too: the widest rows (e.g. "Ctrl+Shift+n / Ctrl+Shift+p")
        // are ~27 chars and would otherwise blow past the 15-char column and
        // widen the whole card.
        let key_label = Label::builder()
            .label(*key)
            .halign(Align::Start)
            .width_chars(15)
            .max_width_chars(15)
            .xalign(0.0)
            .wrap(true)
            .build();
        key_label.add_css_class("legend-key");
        // `wrap(true)` alone does NOT bound the width: GTK still requests the
        // label's full natural width, so a long action ran the card out to
        // ~1400px even with a 330px column `width_request` (that is a MINIMUM,
        // not a cap). `max_width_chars` is what actually forces the wrap, and
        // dropping `hexpand` keeps the column from stretching to fill.
        let action_label = Label::builder()
            .label(*action)
            .halign(Align::Start)
            .xalign(0.0)
            .wrap(true)
            .max_width_chars(ACTION_MAX_CHARS)
            .build();
        action_label.add_css_class("legend-action");
        row.append(&key_label);
        row.append(&action_label);
        group.append(&row);
    }

    column.append(&group);
}
