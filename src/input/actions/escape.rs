use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::WidgetExt;

use crate::app::AppState;

pub(crate) fn escape_reader_mode(state: &Rc<RefCell<AppState>>) {
    // Toasts take top priority: if any transient toast pill is on screen,
    // Escape just dismisses it and does nothing else (no tearing down
    // concordance / AB-loop / search state). `chapter_toast` is shared by the
    // chapter/scene, scansion, and TTS (incl. persistent "Synthesizing…")
    // toasts; `speed_toast` and `search_toast` are the other two. Bumping
    // `chapter_toast_gen` makes any pending auto-hide timer a stale no-op.
    {
        let s = state.borrow();
        let any_visible = s.chapter_toast.is_visible()
            || s.speed_toast.is_visible()
            || s.search_toast.is_visible();
        if any_visible {
            s.chapter_toast_gen.set(s.chapter_toast_gen.get().wrapping_add(1));
            s.chapter_toast.set_visible(false);
            s.speed_toast.set_visible(false);
            s.search_toast.set_visible(false);
            crate::logging::log("ESCAPE: cleared visible toast(s)");
            return;
        }
    }
    // Translations: Escape toggles them off, same as `i`, restoring the
    // pre-translation two-column page.
    {
        let visible = state.borrow().translations_visible;
        if visible {
            crate::app::translations::toggle_translations(&mut state.borrow_mut());
            return;
        }
    }
    // Concordance state takes priority
    {
        let has_conc = state.borrow().concordance_state.is_some();
        if has_conc {
            let mut s = state.borrow_mut();
            s.concordance_state = None;
            s.concordance_origin = None;
            s.concordance_bar.hide();
            crate::input::actions::concordance::restore_sync_after_concordance(&mut s);
            if s.config.title_bar_visible {
                s.title_bar.set_visible(true);
            }
            return;
        }
    }
    // AB loop
    {
        let is_ab_active = state.borrow().ab_repeat.loop_active;
        if is_ab_active {
            let mut s = state.borrow_mut();
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::ClearAbLoop);
            s.ab_repeat.clear();
            s.ab_repeat.chunk_index = None;
            s.ab_a_line.set(None);
            s.ab_b_line.set(None);
            s.suppress_sync_until = None;
            if let Some(ref renderer) = s.gutter_renderer {
                renderer.queue_draw();
            }
            crate::app::remove_ab_dim(&s);
            crate::logging::log("CHUNK: AB loop cleared");
            drop(s);
            crate::input::navigation::update_highlight_and_center(&mut state.borrow_mut());
            return;
        }
    }
    // Search matches
    {
        let has_search = !state.borrow().search_matches.is_empty();
        if has_search {
            crate::input::search::clear_search(&mut state.borrow_mut());
        }
    }
}
