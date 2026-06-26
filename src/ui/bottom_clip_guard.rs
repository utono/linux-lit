use gtk4::prelude::*;
use gtk4::{Align, Overlay};

/// Which shared clip-math fn a guard drives: TextView surfaces mask the partial
/// wrapped row; Box-child surfaces (the translation column stack) only cover
/// trailing slack.
#[derive(Clone)]
enum ClipKind {
    TextView(gtk4::TextView),
    #[allow(dead_code)] // Task 2 wires attach_box
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
        // path (c): recompute on EVERY value change (scroll OR layout-driven
        // page_size change, e.g. an ask card resizing the viewport).
        {
            let kind = guard.kind.clone();
            let clip = clip.clone();
            let scrolled = scrolled.clone();
            scrolled.vadjustment().connect_value_changed(move |_| {
                recompute(&kind, &clip, &scrolled);
            });
        }
        guard
    }

    /// The clip Box, e.g. so the caller can stack a selection-bar overlay after it.
    pub(crate) fn clip(&self) -> &gtk4::Box {
        &self.clip
    }

    /// (b) Recompute now — call from the named scroll methods.
    pub(crate) fn recompute(&self) {
        recompute(&self.kind, &self.clip, &self.scrolled);
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
