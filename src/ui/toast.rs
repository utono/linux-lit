use gtk4::prelude::*;
use gtk4::Label;

/// Show `label` with `text`, then auto-hide it after `secs` seconds.
///
/// The shared tail of every transient toast: callers pass their own label,
/// message, and duration (each preserved exactly at the call site). Takes the
/// `Label` by reference so it works identically from both `&AppState` and
/// `&Rc<RefCell<AppState>>` sites.
///
/// Does NOT cover generation-guarded toasts (`navigation::show_chapter_toast`)
/// or persistent toasts (`show_persistent_tts_toast`) — those keep their own
/// closures by design.
pub(crate) fn show_transient(label: &Label, text: &str, secs: u64) {
    label.set_text(text);
    label.set_visible(true);
    let label = label.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(secs), move || {
        label.set_visible(false);
    });
}
