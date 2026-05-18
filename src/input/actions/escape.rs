use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::WidgetExt;

use crate::app::AppState;

pub(crate) fn escape_reader_mode(state: &Rc<RefCell<AppState>>) {
    // Concordance state takes priority
    {
        let has_conc = state.borrow().concordance_state.is_some();
        if has_conc {
            let mut s = state.borrow_mut();
            s.concordance_state = None;
            s.concordance_bar.hide();
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
