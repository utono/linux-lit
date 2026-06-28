use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Align, Overlay};

/// Which shared clip-math fn a guard drives: TextView surfaces mask the partial
/// wrapped row; Box-child surfaces only cover trailing slack.
#[derive(Clone)]
enum ClipKind {
    TextView(gtk4::TextView),
    Box,
}

/// Owns a free-scroll surface's bottom clip box AND every recompute path, so a
/// surface attaches it once and cannot drop a path (the historical bug: a
/// surface hand-wired some paths but not the `value_changed` catch-all, so the
/// clip went stale on resize/scroll). See docs/troubleshooting/clip-prevention.md.
pub(crate) struct BottomClipGuard {
    kind: ClipKind,
    clip: gtk4::Box,
    scrolled: gtk4::ScrolledWindow,
}

impl BottomClipGuard {
    /// Build the clip Box (fixed props), add it to `scroll_overlay`
    /// (measure=false, clip=true), and wire the persistent `value_changed`
    /// catch-all (path c). For a TextView-content scrolled window.
    pub(crate) fn attach(
        scroll_overlay: &Overlay,
        view: &gtk4::TextView,
        scrolled: &gtk4::ScrolledWindow,
    ) -> Self {
        let clip = build_clip_box();
        scroll_overlay.add_overlay(&clip);
        scroll_overlay.set_measure_overlay(&clip, false);
        scroll_overlay.set_clip_overlay(&clip, true);

        let guard = Self {
            kind: ClipKind::TextView(view.clone()),
            clip: clip.clone(),
            scrolled: scrolled.clone(),
        };
        guard.wire_recompute_signals();
        guard
    }

    /// Like `attach`, but for a scrolled window whose child is a widget BOX of
    /// whole-widget rows (no wrapped partial row — covers trailing slack only).
    /// Drives `recompute_overlay_bottom_clip_box`. Kept as a general API for a
    /// future Box-only surface (the translation overlay, its former user, now
    /// paginates instead of scrolling — no clip needed).
    #[allow(dead_code)]
    pub(crate) fn attach_box(
        scroll_overlay: &Overlay,
        scrolled: &gtk4::ScrolledWindow,
    ) -> Self {
        let clip = build_clip_box();
        scroll_overlay.add_overlay(&clip);
        scroll_overlay.set_measure_overlay(&clip, false);
        scroll_overlay.set_clip_overlay(&clip, true);

        let guard = Self {
            kind: ClipKind::Box,
            clip: clip.clone(),
            scrolled: scrolled.clone(),
        };
        guard.wire_recompute_signals();
        guard
    }

    /// Path (c): recompute the clip on EVERY change that can move the partial
    /// bottom row relative to the viewport — both a scroll (`value_changed`) AND
    /// a viewport-height change (`page_size` notify). The page_size hook is
    /// essential: when the ask card opens and the scroll's height is re-pinned,
    /// the viewport shrinks but the scroll VALUE often does not change, so
    /// `value_changed` alone never fires and the clip stays stale (computed
    /// against the old, taller viewport) — the half-line then pokes out behind the
    /// ask card. `page_size` notify fires precisely when GTK finishes the
    /// relayout to the new height, so the clip is recomputed against the settled
    /// viewport (no fixed-idle race).
    fn wire_recompute_signals(&self) {
        let adj = self.scrolled.vadjustment();
        {
            let kind = self.kind.clone();
            let clip = self.clip.clone();
            let scrolled = self.scrolled.clone();
            adj.connect_value_changed(move |_| {
                recompute(&kind, &clip, &scrolled);
            });
        }
        {
            let kind = self.kind.clone();
            let clip = self.clip.clone();
            let scrolled = self.scrolled.clone();
            adj.connect_page_size_notify(move |_| {
                recompute(&kind, &clip, &scrolled);
            });
        }
    }

    /// The clip Box, e.g. so the caller can stack a selection-bar overlay after it.
    pub(crate) fn clip(&self) -> &gtk4::Box {
        &self.clip
    }

    /// (b) Recompute now — call from the named scroll methods.
    pub(crate) fn recompute(&self) {
        recompute(&self.kind, &self.clip, &self.scrolled);
    }

    /// (a) Open-time coverage: snap to top, then keep recomputing the clip across
    /// the open's layout passes via a one-shot `connect_changed` (range-change)
    /// handler that self-disconnects after 250ms, plus an idle backstop. Mirrors
    /// the gloss overlay's `reset_scroll_top`. Call from every show/open path.
    pub(crate) fn on_open(&self) {
        let adj = self.scrolled.vadjustment();
        adj.set_value(adj.lower());

        let kind = self.kind.clone();
        let clip = self.clip.clone();
        let scrolled = self.scrolled.clone();

        // Pin the scroll to top across the open's layout passes, then release so
        // we stop fighting later user scrolls.
        let pinning = Rc::new(Cell::new(true));
        let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::new(RefCell::new(None));
        let id = adj.connect_changed({
            let pinning = pinning.clone();
            let kind = kind.clone();
            let clip = clip.clone();
            let scrolled = scrolled.clone();
            move |a| {
                if pinning.get() && a.value() != a.lower() {
                    a.set_value(a.lower());
                }
                recompute(&kind, &clip, &scrolled);
            }
        });
        *handler.borrow_mut() = Some(id);

        let adj_for_stop = adj.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
            pinning.set(false);
            if let Some(hid) = handler.borrow_mut().take() {
                adj_for_stop.disconnect(hid);
            }
        });

        // Backstop: size the clip on first open even if `changed` never fires.
        let kind2 = kind;
        let clip2 = clip;
        let scrolled2 = scrolled;
        glib::idle_add_local_once(move || {
            recompute(&kind2, &clip2, &scrolled2);
        });
    }
}

fn build_clip_box() -> gtk4::Box {
    let clip = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    clip.set_valign(Align::End);
    clip.set_halign(Align::Fill);
    clip.set_vexpand(false);
    clip.set_can_target(false);
    clip.add_css_class("gloss-bottom-clip");
    clip.set_height_request(0);
    clip
}

fn recompute(kind: &ClipKind, clip: &gtk4::Box, scrolled: &gtk4::ScrolledWindow) {
    match kind {
        ClipKind::TextView(view) => {
            crate::ui::recompute_overlay_bottom_clip(view, clip, scrolled)
        }
        ClipKind::Box => crate::ui::recompute_overlay_bottom_clip_box(clip, scrolled),
    }
}
