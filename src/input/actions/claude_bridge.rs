use crate::app::AppState;
use gtk4::glib;
use std::cell::RefCell;
use std::rc::Rc;

/// Spawn a Claude `call_claude_with_prompt` request off the GTK thread, then
/// dispatch back on the main loop: `on_success(state, reply)` on success, or
/// `on_error(state, msg)` on API error / tokio join panic (so the overlay is
/// never left stuck on the loading card). Callers must call the overlay's
/// `show_loading()` BEFORE invoking this.
///
/// `model` is moved into the spawned future; if the success body needs the
/// model id (e.g. to stamp a DB row) the caller captures its own clone in the
/// `on_success` closure.
pub(crate) fn run_claude_request(
    state_rc: &Rc<RefCell<AppState>>,
    system_prompt: String,
    user_msg: String,
    model: String,
    on_success: impl Fn(&Rc<RefCell<AppState>>, String) + 'static,
    on_error: impl Fn(&Rc<RefCell<AppState>>, &str) + 'static,
) {
    let tokio_handle = state_rc.borrow().tokio_handle.clone();
    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(&system_prompt, &user_msg, &model).await
            })
            .await;
        match result {
            Ok(Ok(reply)) => on_success(&state_for_result, reply),
            Ok(Err(e)) => {
                crate::logging::log(&format!("CLAUDE: API error: {}", e));
                on_error(&state_for_result, &format!("Error: {}", e));
            }
            Err(e) => {
                crate::logging::log(&format!("CLAUDE: tokio join error: {}", e));
                on_error(&state_for_result, "Internal error \u{2014} try again.");
            }
        }
    });
}
