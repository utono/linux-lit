use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};

/// Fixed width of the two-column float card (`place_float`), in px.
///
/// Pinned 2026-07-27. The previous `(w - 48).clamp(200, 420)` happened to
/// saturate at 420 on the usual 1920px two-column layout — measured at 418px
/// on screen (420 minus the 1px borders) — so this preserves the look the user
/// asked to keep while making it independent of column width.
const FLOAT_WIDTH: i32 = 420;

/// Floor for `place_float` when the column is too narrow for `FLOAT_WIDTH`,
/// so a small window degrades gracefully instead of overhanging the column.
const MIN_FLOAT_WIDTH: i32 = 200;

/// Wrap width for the popup's body labels, in CHARACTERS.
///
/// A GTK4 `Label` with `wrap(true)` still reports its FULL unwrapped text as
/// its natural width unless `max_width_chars` is set — so a long definition
/// pushed the card far past its `width_request` (measured 624px against a
/// requested 420px) instead of wrapping. The `width_request` is a MINIMUM, not
/// a cap, so it cannot hold the card on its own.
///
/// Calibrated by MEASURING the rendered card, not estimated — the per-char
/// width at the popup's 16px body font is ~8.4px, well off a naive estimate.
/// Successive headless measurements on the long `solemn` definition:
/// 56 chars ⇒ 507px, 46 ⇒ 437px, **44 ⇒ 425px** (the ~5px over `FLOAT_WIDTH`
/// is the card's border/padding, matching the 418px the user measured on the
/// real renderer).
///
/// Undershooting is safe — the card holds at `FLOAT_WIDTH` and simply wraps
/// sooner. Overshooting is what stretched the card past its width_request.
/// Re-measure this if the popup's font size changes.
const BODY_WRAP_CHARS: i32 = 44;

/// Bound a wrapping label's natural width so it wraps INSIDE the card instead
/// of stretching it. Apply to every `wrap(true)` body label.
fn constrain_wrap(label: &Label) {
    label.set_max_width_chars(BODY_WRAP_CHARS);
}

/// Data for a single vocab word: definition + etymology + gloss.
pub struct VocabWordData {
    pub word: String,
    pub definition: Option<String>,
    pub etymology_markup: Option<String>,
    pub gloss: Option<String>,
}

/// Which view is currently shown in the popup.
#[derive(Clone, Copy, PartialEq)]
pub enum VocabView {
    Definition,
    Gloss,
}

pub struct VocabPopup {
    container: GtkBox,
    content_box: GtkBox,
    header_label: Label,
    counter_label: Label,
    footer_label: Label,
}

impl VocabPopup {
    pub fn new() -> Self {
        let content_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            // In float mode the container is column-height; expanding the
            // content region pins the hint footer to the panel bottom. At
            // natural height (strip mode) this has no effect.
            .vexpand(true)
            .build();

        let header_label = Label::builder()
            .halign(gtk4::Align::Start)
            .margin_bottom(8)
            .build();
        header_label.add_css_class("definition-header");

        let counter_label = Label::builder()
            .halign(gtk4::Align::End)
            .margin_bottom(8)
            .build();
        counter_label.add_css_class("definition-header");

        let header_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(0)
            .build();
        header_row.append(&header_label);
        let spacer = GtkBox::builder().hexpand(true).build();
        header_row.append(&spacer);
        header_row.append(&counter_label);

        let footer_label = Label::builder()
            .halign(gtk4::Align::Center)
            .build();
        footer_label.add_css_class("definition-hint");

        // Anchored upper-right: the popup hugs the window top beside the
        // card (margin_start is set live to clear the card's right edge).
        // These are only the initial values — every `place_*` call overwrites
        // the alignment and all four margins.
        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .valign(gtk4::Align::Start)
            .margin_end(5)
            .margin_top(24)
            .build();
        container.add_css_class("vocab-popup");
        container.append(&header_row);
        container.append(&content_box);
        container.append(&footer_label);
        container.set_visible(false);

        VocabPopup {
            container,
            content_box,
            header_label,
            counter_label,
            footer_label,
        }
    }

    /// Add the popup as an overlay child of a full-width Overlay widget.
    pub fn attach_to(&self, overlay: &gtk4::Overlay) {
        overlay.add_overlay(&self.container);
        self.container.set_visible(false);
    }

    /// Single-column placement: the strip right of the text card, anchored to
    /// the TOP of that strip (2026-07-29 — was bottom-anchored). Restores
    /// every container property `place_float` changes, so the two calls can
    /// alternate as the work's layout changes.
    pub fn place_strip(&self, margin_start: i32) {
        self.container.remove_css_class("vocab-popup-float");
        self.container.remove_css_class("vocab-popup-overlay");
        self.container.set_halign(gtk4::Align::Fill);
        self.container.set_valign(gtk4::Align::Start);
        self.container.set_margin_start(margin_start);
        self.container.set_margin_end(5);
        self.container.set_margin_top(24);
        self.container.set_margin_bottom(0);
        self.container.set_width_request(-1);
        self.container.set_height_request(-1);
    }

    /// Two-column placement: a COMPACT card centered in the reading column
    /// the cursor is NOT in (x/w = that column's window-coord rect from
    /// layout::column_float_rect).
    ///
    /// Width is FIXED at `FLOAT_WIDTH` (2026-07-27) rather than derived from
    /// the column. It used to be `(w - 48).clamp(200, 420)`, which in practice
    /// saturated at the 420 ceiling on a 1920px two-column layout — so the card
    /// was already ~420px wide, but only incidentally, and it would have
    /// silently narrowed on a different column width. Pinning it keeps the
    /// popup identical across works and layouts. It still shrinks if the
    /// column genuinely cannot fit `FLOAT_WIDTH`, so a narrow window degrades
    /// instead of overhanging.
    ///
    /// Height stays NATURAL (`-1`): a long definition makes the card taller
    /// rather than being squeezed or clipped. `h` is accepted for signature
    /// compatibility and unused.
    pub fn place_float(&self, x: i32, w: i32, h: i32) {
        self.container.add_css_class("vocab-popup-float");
        self.container.remove_css_class("vocab-popup-overlay");
        // Fixed width, but never wider than the column can hold.
        let width = FLOAT_WIDTH.min((w - 16).max(MIN_FLOAT_WIDTH));
        let centered_x = x + (w - width) / 2;
        self.container.set_halign(gtk4::Align::Start);
        self.container.set_valign(gtk4::Align::Center);
        self.container.set_margin_start(centered_x.max(0));
        self.container.set_margin_end(0);
        self.container.set_margin_top(0);
        self.container.set_margin_bottom(0);
        self.container.set_width_request(width);
        self.container.set_height_request(-1);
        // Never taller than the card: content itself is short (definition +
        // etymology); the Journal view already caps its body height.
        let _ = h;
    }

    /// Overlay placement, primary form: the SAME frameless strip as the
    /// reader (`place_strip` geometry), with a transparent background so the
    /// overlay scrim shows through — the popup reads as loose ink beside the
    /// card, exactly like the main-card presentation, instead of a boxed
    /// corner card.
    pub fn place_overlay_strip(&self, margin_start: i32) {
        self.place_strip(margin_start);
        self.container.add_css_class("vocab-popup-overlay");
    }

    /// Overlay fallback placement (strip too narrow to wrap text): compact
    /// card anchored to the window's lower right, natural size (the popup
    /// floats above the overlay chain).
    pub fn place_corner(&self) {
        self.container.remove_css_class("vocab-popup-float");
        self.container.remove_css_class("vocab-popup-overlay");
        self.container.set_halign(gtk4::Align::End);
        self.container.set_valign(gtk4::Align::End);
        self.container.set_margin_start(0);
        self.container.set_margin_end(24);
        self.container.set_margin_top(0);
        self.container.set_margin_bottom(24);
        self.container.set_width_request(-1);
        self.container.set_height_request(-1);
    }

    /// Chat placement: the compact card in the strip `chat::size_panel` frees
    /// BELOW the raised chat-panel bottom. `x`/`y` are window coords (the
    /// popup lives in the window-filling outer overlay, like the panel);
    /// `w` pins the card to the panel's transcript text width so its left and
    /// right borders land on the text margins.
    pub fn place_chat(&self, x: i32, y: i32, w: i32) {
        // Keep the float-card skin (border + radius): the chat slot sits on
        // the bare root, where the plain .vocab-popup (bg == root, no border)
        // would render as loose text with no card edge.
        self.container.add_css_class("vocab-popup-float");
        self.container.set_halign(gtk4::Align::Start);
        self.container.set_valign(gtk4::Align::Start);
        self.container.set_margin_start(x.max(0));
        self.container.set_margin_end(0);
        self.container.set_margin_top(y.max(0));
        self.container.set_margin_bottom(0);
        self.container.set_width_request(w.max(0));
        self.container.set_height_request(-1);
    }

    pub fn show(&self) {
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    pub fn widget(&self) -> &GtkBox {
        &self.container
    }


    /// Show "index+1 / total" when stepping through multiple entries; hidden
    /// for a single entry.
    fn set_counter(&self, index: usize, total: usize) {
        if total > 1 {
            self.counter_label.set_text(&format!("{} / {}", index + 1, total));
            self.counter_label.set_visible(true);
        } else {
            self.counter_label.set_visible(false);
        }
    }

    /// Remove every child from the content region (a plain GtkBox — not a
    /// ListBox, so `picker_nav::clear_list` does not apply).
    fn clear_content(&self) {
        while let Some(child) = self.content_box.first_child() {
            self.content_box.remove(&child);
        }
    }

    /// Render the popup content for a given word and view.
    pub fn update(
        &self,
        data: &VocabWordData,
        index: usize,
        total: usize,
        view: VocabView,
        _work_abbrev: &str,
    ) {
        self.clear_content();

        // Header: hide work abbreviation
        self.header_label.set_visible(false);

        self.set_counter(index, total);

        // Word
        let word_label = Label::builder()
            .halign(gtk4::Align::Start)
            .margin_bottom(12)
            .build();
        word_label.add_css_class("definition-word");
        word_label.set_text(&data.word);
        self.content_box.append(&word_label);

        match view {
            VocabView::Definition => {
                if let Some(ref def) = data.definition {
                    let def_label = Label::builder()
                        .halign(gtk4::Align::Start)
                        .wrap(true)
                        .wrap_mode(gtk4::pango::WrapMode::Word)
                        .margin_bottom(8)
                        .build();
                    constrain_wrap(&def_label);
                    def_label.add_css_class("definition-text");
                    def_label.set_text(def);
                    self.content_box.append(&def_label);
                }

                if let Some(ref etym) = data.etymology_markup {
                    let etym_header = Label::builder()
                        .label("ETYMOLOGY")
                        .halign(gtk4::Align::Start)
                        .margin_top(8)
                        .margin_bottom(4)
                        .build();
                    etym_header.add_css_class("definition-header");
                    self.content_box.append(&etym_header);

                    let etym_label = Label::builder()
                        .halign(gtk4::Align::Start)
                        .wrap(true)
                        .wrap_mode(gtk4::pango::WrapMode::Word)
                        .build();
                    constrain_wrap(&etym_label);
                    etym_label.add_css_class("definition-etymology");
                    etym_label.set_markup(etym);
                    self.content_box.append(&etym_label);
                }
            }
            VocabView::Gloss => {
                if let Some(ref gloss) = data.gloss {
                    let gloss_label = Label::builder()
                        .halign(gtk4::Align::Start)
                        .wrap(true)
                        .wrap_mode(gtk4::pango::WrapMode::Word)
                        .build();
                    constrain_wrap(&gloss_label);
                    gloss_label.add_css_class("definition-text");
                    gloss_label.set_text(gloss);
                    self.content_box.append(&gloss_label);
                } else {
                    let no_gloss = Label::builder()
                        .halign(gtk4::Align::Start)
                        .build();
                    no_gloss.add_css_class("definition-text");
                    no_gloss.set_text("(no gloss for this passage)");
                    self.content_box.append(&no_gloss);
                }
            }
        }

        self.footer_label.set_visible(false);
    }

    /// Render a scene synopsis in the popup.
    pub fn update_synopsis(&self, synopsis_division_label: &str, synopsis: &str) {
        self.clear_content();

        self.header_label.set_text("SYNOPSIS");
        self.header_label.set_visible(true);
        self.counter_label.set_visible(false);

        let synopsis_division_label_widget = Label::builder()
            .halign(gtk4::Align::Start)
            .margin_bottom(12)
            .build();
        synopsis_division_label_widget.add_css_class("definition-word");
        synopsis_division_label_widget.set_text(synopsis_division_label);
        self.content_box.append(&synopsis_division_label_widget);

        let synopsis_label = Label::builder()
            .halign(gtk4::Align::Start)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::Word)
            .build();
        constrain_wrap(&synopsis_label);
        synopsis_label.add_css_class("definition-text");
        synopsis_label.set_text(synopsis);
        self.content_box.append(&synopsis_label);

        self.footer_label.set_visible(false);
    }
}

// (The popup's Journal Q&A view was removed: Ctrl+r now holds a toast for the
// API round-trip and opens the JOURNAL OVERLAY on the saved entry — see
// src/input/actions/vocab_journal.rs.)
