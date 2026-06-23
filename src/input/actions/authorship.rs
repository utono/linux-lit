use std::cell::RefCell;
use std::rc::Rc;
use crate::app::AppState;

pub fn open_attribution_picker(state: &Rc<RefCell<AppState>>) {
    let sets = state.borrow().authorship_sets.clone();
    {
        let mut s = state.borrow_mut();
        s.authorship_picker.set_items(sets);
        s.authorship_picker.show();
        s.input_mode = crate::app::InputMode::AuthorshipPicker;
    }
}

pub fn confirm_attribution_selection(state: &Rc<RefCell<AppState>>) {
    let selected = state.borrow().authorship_picker.selected_set().cloned();
    if let Some(set) = selected {
        {
            let s = state.borrow();
            s.authorship_picker.hide();
        }
        let mut s = state.borrow_mut();
        s.input_mode = crate::app::InputMode::Reader;
        s.active_attribution_set_id = Some(set.id);
        if let Ok(conn) = crate::db::queries::open_db() {
            s.authorship_line_ids = crate::db::authorship::load_secondary_line_ids(
                &conn, set.id, &set.work_abbrev,
            ).unwrap_or_default();
        }
        s.authorship_enabled = true;
        crate::app::formatting::apply_authorship_formatting(&mut s);

        let msg = format!("Authorship: {}", set.display_name);
        crate::ui::toast::show_transient(&s.chapter_toast, &msg, 3);
    }
}
