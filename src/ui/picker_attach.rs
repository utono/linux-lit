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
