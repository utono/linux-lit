//! A small Braille-spinner animator for overlay loading states. Each tick
//! rewrites a caller-supplied text sink with the current spinner frame + a
//! label (and an optional held body above it). Used by the journal and gloss
//! overlays; stopped by the result-render paths so a late tick never repaints
//! over the answer.

use std::cell::RefCell;
use std::rc::Rc;

/// The 10 Braille spinner frames, cycled ~every 120 ms.
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// The spinner glyph for frame `i` (wraps every 10).
pub(crate) fn spinner_frame(i: usize) -> char {
    SPINNER[i % SPINNER.len()]
}

/// Owns the active spinner tick. `start` installs a 120 ms `glib` timeout that
/// advances the frame and repaints via `sink`; `stop` removes it. Idempotent.
///
/// The frame counter is NOT a struct field: `glib::timeout_add_local` requires
/// a `'static` closure, which cannot borrow `&self` or a `Cell` field of
/// `self`. Instead `start` creates an `Rc<Cell<usize>>` local to the call,
/// clones it into the timeout closure, and the struct only keeps `source` so
/// `stop` can remove it.
pub(crate) struct LoadingAnimator {
    source: RefCell<Option<gtk4::glib::SourceId>>,
}

impl LoadingAnimator {
    pub fn new() -> Self {
        Self { source: RefCell::new(None) }
    }

    /// Start animating: `sink(text)` receives the full text to display each
    /// frame — `"{body}\n\n{spinner} {label}"`, or `"{spinner} {label}"` when
    /// `body` is empty. Paints frame 0 immediately, then ticks every 120 ms.
    pub fn start(&self, sink: Rc<dyn Fn(String)>, body: String, label: String) {
        self.stop();
        let render = {
            let sink = Rc::clone(&sink);
            let body = body.clone();
            let label = label.clone();
            move |i: usize| {
                let g = spinner_frame(i);
                let text = if body.is_empty() {
                    format!("{g} {label}")
                } else {
                    format!("{body}\n\n{g} {label}")
                };
                sink(text);
            }
        };
        // Immediate first paint (frame 0) so there is no blank gap.
        render(0);
        // Frame counter lives in an Rc<Cell> local to this call, not a struct
        // field — see the struct doc comment for why.
        let frame_cell = Rc::new(std::cell::Cell::new(0usize));
        let id = gtk4::glib::timeout_add_local(
            std::time::Duration::from_millis(120),
            move || {
                let next = frame_cell.get().wrapping_add(1);
                frame_cell.set(next);
                render(next);
                gtk4::glib::ControlFlow::Continue
            },
        );
        *self.source.borrow_mut() = Some(id);
    }

    /// Stop animating and drop the timeout source. Safe to call when not
    /// running (idempotent) — the result-render paths call this before painting
    /// the answer so a queued tick can never repaint over it.
    pub fn stop(&self) {
        if let Some(id) = self.source.borrow_mut().take() {
            id.remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_wraps_every_ten_frames() {
        assert_eq!(spinner_frame(0), '⠋');
        assert_eq!(spinner_frame(9), '⠏');
        assert_eq!(spinner_frame(10), '⠋'); // wraps
        assert_eq!(spinner_frame(23), spinner_frame(3));
    }
}
