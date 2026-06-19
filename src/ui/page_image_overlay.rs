//! Page-scan image overlay for the main reading card.
//!
//! When a work has `page_images` (e.g. BCP1549's leaf scans), the reader can
//! toggle the card from rendered text to the page PNG for the cursor's current
//! position. This overlay is the image surface: a `gtk4::Picture` constrained to
//! the card's width AND height with `ContentFit::Contain`, so the whole leaf is
//! scaled to fit inside the card (letterboxed, never overflowing the rounded
//! frame), preserving aspect ratio.
//!
//! Like the other reader overlays it attaches as an `add_overlay` panel on the
//! outer overlay (NEVER spliced into the size-bearing card chain — that
//! collapses the reader layout; see the gloss/synopsis overlays).

use gtk4::prelude::*;
use gtk4::{Align, Label, Overlay, Picture};

pub struct PageImageOverlay {
    /// The panel root: a centered, fixed-size column holding the picture, sized
    /// to the card so `Contain` fits the leaf within it. Added to the outer
    /// overlay via `add_overlay`.
    pub container: gtk4::Box,
    picture: Picture,
    /// Caption banner shown above the image during calibration (page N/M + the
    /// cursor line text). Hidden in normal image-view mode.
    caption: Label,
    /// Path currently loaded into the picture, so repeated shows of the same
    /// page don't reload the file.
    current_path: std::cell::RefCell<Option<String>>,
}

impl PageImageOverlay {
    pub fn new() -> Self {
        // Fill the card exactly. The overlay is added onto `page_turn_overlay`
        // (which wraps `card_vbox` = the visible cream card), so filling our
        // parent means filling the card — not the whole window. Inset a little so
        // the leaf sits inside the rounded frame rather than flush to its edges.
        // The container fills the whole card (no outer margins) and carries an
        // OPAQUE card-colored background (.page-image-overlay CSS), so it fully
        // masks the text/divider underneath — the letterbox gutters around a
        // portrait leaf show the card background, not the text behind.
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.set_halign(Align::Fill);
        container.set_valign(Align::Fill);
        container.set_hexpand(true);
        container.set_vexpand(true);
        container.add_css_class("page-image-overlay");
        container.set_visible(false);

        let picture = Picture::new();
        // Fit the WHOLE leaf inside the card: Contain scales to fit both the
        // width and height of the picture's allocation, preserving aspect —
        // letterboxed, never overflowing. can_shrink lets a large scan shrink to
        // the card; without it the natural (full-res) height forces it taller.
        // The Picture (not the container) carries the inset so the opaque
        // background still covers the card edges.
        picture.set_can_shrink(true);
        picture.set_content_fit(gtk4::ContentFit::Contain);
        picture.set_hexpand(true);
        picture.set_vexpand(true);
        picture.set_halign(Align::Fill);
        picture.set_valign(Align::Fill);
        picture.set_margin_top(24);
        picture.set_margin_bottom(24);
        picture.set_margin_start(24);
        picture.set_margin_end(24);

        // Calibration caption FLOATS over the top of the image (an overlay panel,
        // not a sibling in the layout). This keeps the image's position/size
        // IDENTICAL whether the caption is shown (calibration) or hidden (plain
        // Ctrl+i toggle) — so toggling between the two modes doesn't reposition
        // the leaf. The caption sits at the top with its own opaque pill bg.
        let caption = Label::new(None);
        caption.set_halign(Align::Center);
        caption.set_valign(Align::Start);
        caption.set_margin_top(6);
        caption.set_wrap(true);
        caption.add_css_class("page-image-caption");
        caption.set_visible(false);

        // Inner overlay: Picture fills the card; caption floats on top.
        let inner = Overlay::new();
        inner.set_hexpand(true);
        inner.set_vexpand(true);
        inner.set_child(Some(&picture));
        inner.add_overlay(&caption);
        inner.set_measure_overlay(&caption, false);
        inner.set_clip_overlay(&caption, true);

        container.append(&inner);

        PageImageOverlay {
            container,
            picture,
            caption,
            current_path: std::cell::RefCell::new(None),
        }
    }

    /// Show/update the calibration caption banner above the image. Pass None to
    /// hide it (normal image-view mode).
    pub fn set_caption(&self, text: Option<&str>) {
        match text {
            Some(t) => {
                self.caption.set_text(t);
                self.caption.set_visible(true);
            }
            None => self.caption.set_visible(false),
        }
    }

    /// Add this overlay as a panel on the card's own overlay (`page_turn_overlay`)
    /// so it spans exactly the card geometry, not the full window. `measure=false`
    /// keeps it from forcing the card larger; it just fills the card's allocation.
    pub fn attach_to(&self, overlay: &Overlay) {
        overlay.add_overlay(&self.container);
        overlay.set_measure_overlay(&self.container, false);
        overlay.set_clip_overlay(&self.container, true);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    /// Show `path` (an absolute file path). The container fills the card via
    /// expand, so the leaf is fit-to-card by `Contain`; no explicit sizing is
    /// needed. Reloads the file only when the path changes. `_card_width` /
    /// `_card_height` are accepted for call-site symmetry but unused now.
    pub fn show(&self, path: &str, _card_width: i32, _card_height: i32) {
        let changed = self.current_path.borrow().as_deref() != Some(path);
        if changed {
            self.picture.set_filename(Some(path));
            *self.current_path.borrow_mut() = Some(path.to_string());
        }
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }
}
