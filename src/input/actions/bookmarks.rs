use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;
use crate::input::navigation;

/// Toggle a bookmark on the current cursor line. Updates DB, AppState's
/// is_bookmarked vec, and gutter renderer. Called from `m` in reader.
pub(crate) fn toggle_bookmark(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let (abbrev, line_mapping_id, buffer_line) = {
        let s = state.borrow();
        let abbrev = s.current_work.as_ref().map(|w| w.abbrev.clone());
        let lm_id = s.current_work.as_ref().and_then(|w| {
            let work_idx = if let Some(ref lm) = s.line_map {
                lm.buffer_to_work.get(s.current_line)?.as_ref().copied()
            } else {
                Some(s.current_line)
            };
            work_idx.and_then(|wi| w.lines.get(wi).map(|l| l.id))
        });
        (abbrev, lm_id, s.current_line)
    };
    if let (Some(abbrev), Some(lm_id)) = (abbrev, line_mapping_id) {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let result = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db_rw()
                        .expect("Failed to open lit.db rw");
                    crate::db::queries::toggle_bookmark(&conn, &abbrev, lm_id)
                })
                .await;
            if let Ok(Ok(added)) = result {
                let s = state_clone.borrow();
                {
                    let mut bm = s.is_bookmarked.borrow_mut();
                    if buffer_line < bm.len() {
                        bm[buffer_line] = added;
                    }
                }
                // Redraw both column gutters: a bookmarked line in the right
                // column would otherwise show its star only after a page turn.
                if let Some(ref renderer) = s.gutter_renderer {
                    renderer.queue_draw();
                }
                if let Some(ref renderer) = s.right_gutter_renderer {
                    renderer.queue_draw();
                }
            }
        });
    }
}

/// Jump to the most recently created bookmark in the current work.
/// Called from `g;` chord.
pub(crate) fn jump_to_recent_bookmark(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());
    if let Some(abbrev) = abbrev {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let result = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db()
                        .expect(crate::db::queries::OPEN_DB_PANIC_MSG);
                    crate::db::queries::most_recent_bookmark(&conn, &abbrev)
                })
                .await;
            if let Ok(Ok(Some(lm_id))) = result {
                let mut s = state_clone.borrow_mut();
                let buffer_line = if let Some(ref lm) = s.line_map {
                    s.current_work.as_ref().and_then(|w| {
                        let work_idx = w.lines.iter().position(|l| l.id == lm_id)?;
                        Some(lm.work_to_buffer[work_idx])
                    })
                } else {
                    s.current_work.as_ref().and_then(|w| {
                        w.lines.iter().position(|l| l.id == lm_id)
                    })
                };
                if let Some(bl) = buffer_line {
                    navigation::jump_to_line(&mut s, bl);
                }
            }
        });
    }
}
