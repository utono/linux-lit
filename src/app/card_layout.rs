//! `CardLayout` — the reading card's layout manager.
//!
//! Exists to break a deadlock that no combination of GTK widget properties can:
//! **the card needs a definite width, and the window needs a small minimum.**
//!
//! In GTK a `width_request` IS the widget's minimum size, and a child's minimum
//! propagates up to the window, which GTK advertises to the compositor. Measured
//! in cage across every alternative — `width_request` on `content_hbox`, on
//! `scrolled_overlay`, `min-content-width` on the ScrolledWindow, margins
//! absorbing the slack (435px of margin → min 870) — each one that got GTK to
//! *allocate* 1050 also advertised ~1050+ as the minimum. `max-content-width` is
//! ignored entirely under `hscrollbar-policy = Never`, and
//! `propagate-natural-width` touches natural only.
//!
//! Both halves of that deadlock have shipped as bugs, in opposite directions:
//!
//! - **2026-08-01** — an unclamped 1050 `width_request` pinned the window's
//!   minimum, so niri shrank its TILE to match instead of granting fullscreen.
//! - **2026-08-03** — removing the request left NOTHING setting the width. A
//!   `WrapMode::Word` text view's minimum width is only its margins
//!   (`gtk_text_view_measure` never consults the text layout horizontally), so
//!   the card collapsed to 281px and prose rendered one word per line.
//!
//! A layout manager separates the two numbers that a `width_request` conflates.
//! [`LayoutManagerImpl::measure`] decides what the widget ADVERTISES; `allocate`
//! decides what the child actually GETS. So this reports a deliberately tiny
//! minimum — the window can always shrink — while allocating the card at
//! `card_width`, centred in whatever it is given.
//!
//! The manager owns the width; `apply_card_sizing` sets it via
//! [`CardLayout::set_card_width`] and no longer calls `set_width_request` on
//! `content_hbox` at all.

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{glib, graphene, gsk, Orientation, SizeRequestMode, Widget};
use std::cell::Cell;

/// Minimum width the card advertises, whatever it is actually drawn at.
///
/// This is the entire point of the manager: it must stay small enough that the
/// window's minimum never approaches the output, so the compositor can always
/// tile the reader narrow. It is NOT a card width and must never be raised to
/// one — at that point this file would be an expensive `width_request`.
///
/// Not zero, because a genuinely 0-minimum window invites the compositor to
/// allocate a degenerate size; 64px is small enough to be irrelevant to any
/// real tiling decision and large enough to stay a sane window.
pub(crate) const ADVERTISED_MIN_WIDTH: i32 = 64;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct CardLayout {
        /// Width to allocate the child, or <= 0 for "fill whatever we're given".
        pub card_width: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CardLayout {
        const NAME: &'static str = "LitCardLayout";
        type Type = super::CardLayout;
        type ParentType = gtk4::LayoutManager;
    }

    impl ObjectImpl for CardLayout {}

    impl LayoutManagerImpl for CardLayout {
        fn request_mode(&self, _widget: &Widget) -> SizeRequestMode {
            // Height depends on the width the card ends up with (wrapped prose
            // gets taller as it narrows), which is exactly this mode.
            SizeRequestMode::HeightForWidth
        }

        fn measure(
            &self,
            widget: &Widget,
            orientation: Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            let card_w = self.card_width.get();

            if orientation == Orientation::Horizontal {
                // The whole trick, in two numbers that a `width_request` would
                // have forced to be one:
                //   minimum — tiny, so the WINDOW can shrink to any tile.
                //   natural — the card, so GTK gives us the full width when
                //             there is room and the compositor has no opinion.
                let natural = if card_w > 0 {
                    card_w.max(ADVERTISED_MIN_WIDTH)
                } else {
                    ADVERTISED_MIN_WIDTH
                };
                return (ADVERTISED_MIN_WIDTH, natural, -1, -1);
            }

            // Vertical: measure the child against the width it will ACTUALLY be
            // allocated, not the width we were handed. Passing `for_size`
            // straight through would measure wrapped prose at the full window
            // width while `allocate` then renders it at `card_width` -- every
            // row that wraps differently at the narrower width would be missing
            // from the height, clipping the bottom of the card.
            let child_for_size = if for_size >= 0 {
                Some(self.effective_width(for_size))
            } else {
                None
            };

            let mut min = 0;
            let mut nat = 0;
            let mut child = widget.first_child();
            while let Some(c) = child {
                if c.is_visible() {
                    // Never measure a child for a width it has already said it
                    // cannot accept. We advertise ADVERTISED_MIN_WIDTH (64)
                    // horizontally so the window can tile narrow, and GTK takes
                    // that literally: it calls the vertical measure with
                    // for_size = 64, which `effective_width` passes straight
                    // down. But two-column mode hard-sets a 760px width_request
                    // on `scrolled_overlay`, so the subtree's real minimum is
                    // far above 64. Measuring it at 64 makes GTK emit
                    // "Trying to measure GtkOverlay for width of 64, but it
                    // needs at least N" and re-measure the subtree — ~400
                    // warnings per cage run, gone entirely with this clamp.
                    //
                    // Scope, measured rather than assumed: this is wasted
                    // re-measure work and log noise, NOT a hang. The app runs
                    // to completion with or without the clamp (both builds
                    // reached 81s and 223 log lines under the same drive), so
                    // do not cite this as the cause of an unresponsive UI.
                    //
                    // Clamping to the child's OWN reported horizontal minimum
                    // keeps the advertised 64 intact (that number is the whole
                    // point of this manager — raising it re-breaks the niri
                    // tile-shrink guard) while asking the subtree only for
                    // widths it can actually satisfy.
                    let for_child = child_for_size.map(|w| {
                        let (cmin_w, _, _, _) = c.measure(Orientation::Horizontal, -1);
                        w.max(cmin_w)
                    });
                    let (cmin, cnat, _, _) =
                        c.measure(orientation, for_child.unwrap_or(-1));
                    min = min.max(cmin);
                    nat = nat.max(cnat);
                }
                child = c.next_sibling();
            }
            (min, nat, -1, -1)
        }

        fn allocate(&self, widget: &Widget, width: i32, height: i32, baseline: i32) {
            let w = self.effective_width(width);
            // Centre the card in the space we were given. `content_hbox` used
            // to do this with `halign: Center`, which is what made the collapse
            // possible -- a centred box takes its NATURAL width, and with no
            // width anywhere that was near zero. Centring is arithmetic here,
            // so it cannot interact with sizing.
            let x = ((width - w) / 2).max(0);

            let mut child = widget.first_child();
            while let Some(c) = child {
                if c.is_visible() {
                    let transform = if x != 0 {
                        Some(gsk::Transform::new().translate(&graphene::Point::new(x as f32, 0.0)))
                    } else {
                        None
                    };
                    c.allocate(w, height, baseline, transform);
                }
                child = c.next_sibling();
            }
        }
    }

    impl CardLayout {
        /// Width the child is actually allocated inside `available` px.
        ///
        /// Clamped to `available`: the card yields to the window rather than
        /// overflowing it, which is what lets the window keep shrinking past the
        /// configured card width instead of pinning at it.
        pub(super) fn effective_width(&self, available: i32) -> i32 {
            let card_w = self.card_width.get();
            if card_w > 0 {
                card_w.min(available.max(1))
            } else {
                available.max(1)
            }
        }
    }
}

glib::wrapper! {
    pub struct CardLayout(ObjectSubclass<imp::CardLayout>)
        @extends gtk4::LayoutManager;
}

impl Default for CardLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl CardLayout {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    /// Set the width to allocate the card, in px. `<= 0` means "fill the
    /// available space" (the pre-first-allocation state, before
    /// `apply_card_sizing` has a real window width to clamp against).
    ///
    /// Queues a resize only when the value actually changes -- this is called
    /// from the resize tick, which runs on every frame.
    pub fn set_card_width(&self, width: i32) {
        let imp = self.imp();
        if imp.card_width.get() == width {
            return;
        }
        imp.card_width.set(width);
        self.layout_changed();
    }
}
