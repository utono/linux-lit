use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Overlay, Widget};

/// Attach an overlay-panel picker: set `base` as the overlay's child, add the
/// optional `scrim` then `panel` as overlays, and hide `panel`. The shared body
/// of every picker's `attach`; callers that need extra setup (e.g. responsive
/// resize) call this first, then do their own work.
pub(crate) fn attach_panel(
    overlay: &Overlay,
    base: &impl IsA<Widget>,
    scrim: Option<&GtkBox>,
    panel: &GtkBox,
) {
    overlay.set_child(Some(base));
    if let Some(scrim) = scrim {
        overlay.add_overlay(scrim);
    }
    overlay.add_overlay(panel);
    panel.set_visible(false);
}

/// Attach a full-screen overlay panel (scrim + container): set `child` as the
/// overlay's child, add `scrim` then `container` as overlays, and mark both
/// non-measuring + clipping. The shared body of the gloss/journal/translation
/// `attach` methods. Distinct from `attach_panel` (which omits the
/// measure/clip calls and hides its panel) — those are different contracts.
pub(crate) fn attach_overlay_panel(
    overlay: &Overlay,
    child: &impl IsA<Widget>,
    scrim: &GtkBox,
    container: &GtkBox,
) {
    overlay.set_child(Some(child));
    overlay.add_overlay(scrim);
    overlay.add_overlay(container);
    overlay.set_measure_overlay(scrim, false);
    overlay.set_measure_overlay(container, false);
    overlay.set_clip_overlay(scrim, true);
    overlay.set_clip_overlay(container, true);
}
