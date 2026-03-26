use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, CssProvider, EventControllerKey, ScrolledWindow, TextBuffer, TextView,
    WrapMode,
};

use crate::config::Config;
use crate::db::models::{Work, WorkSummary};
use crate::ui::library_picker::LibraryPicker;

#[allow(dead_code)]
pub struct AppState {
    pub text_view: TextView,
    pub buffer: TextBuffer,
    pub picker: LibraryPicker,
    pub current_work: Option<Work>,
    pub current_line: usize,
    pub highlight_tag: gtk4::TextTag,
    pub scrolled_window: ScrolledWindow,
    pub window: ApplicationWindow,
    pub config: Config,
}

pub fn build_window(
    app: &gtk4::Application,
    works: Vec<WorkSummary>,
    tokio_handle: tokio::runtime::Handle,
    config: Config,
) -> Rc<RefCell<AppState>> {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("linux-lit")
        .default_width(1000)
        .default_height(800)
        .build();

    let buffer = TextBuffer::new(None);
    let highlight_tag = gtk4::TextTag::builder()
        .name("current-line")
        .background("rgba(100, 140, 200, 0.3)")
        .build();
    buffer.tag_table().add(&highlight_tag);

    let text_view = TextView::builder()
        .buffer(&buffer)
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(WrapMode::Word)
        .build();

    // Apply serif font and picker CSS
    let css_provider = CssProvider::new();
    css_provider.load_from_string(&format!(
        "textview {{ font-family: Georgia, 'Noto Serif', 'Liberation Serif', 'DejaVu Serif'; font-size: {}pt; }} \
         .library-picker {{ background-color: rgba(40, 40, 40, 0.95); color: white; padding: 16px; border-radius: 8px; }} \
         .library-picker entry {{ margin-bottom: 8px; }} \
         .library-picker row:selected {{ background-color: rgba(100, 140, 200, 0.8); }}",
        18
    ));
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css_provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Line spacing
    text_view.set_pixels_above_lines(14);
    text_view.set_pixels_below_lines(14);

    // Initial margins
    text_view.set_left_margin(150);
    text_view.set_right_margin(150);

    // Scrolled window — hide scrollbar
    let scrolled = ScrolledWindow::builder()
        .child(&text_view)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::External)
        .vexpand(true)
        .hexpand(true)
        .build();

    // Recalculate margins on resize
    let text_view_for_resize = text_view.clone();
    scrolled.connect_notify_local(Some("width"), move |scrolled, _| {
        let width = scrolled.width();
        let margin = ((width - 700) / 2).max(20);
        text_view_for_resize.set_left_margin(margin);
        text_view_for_resize.set_right_margin(margin);
    });

    // Library picker overlay
    let mut picker = LibraryPicker::new();
    picker.set_works(works);
    picker.attach(&scrolled);

    window.set_child(Some(&picker.overlay));

    let last_work = config.last_work.clone();
    let last_line = config.last_line;

    let state = Rc::new(RefCell::new(AppState {
        text_view,
        buffer,
        picker,
        current_work: None,
        current_line: 0,
        highlight_tag,
        scrolled_window: scrolled,
        window: window.clone(),
        config,
    }));

    // Connect picker search entry filter
    let state_for_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.picker.search_entry().connect_changed(move |entry| {
            let text = entry.text();
            state_for_filter.borrow().picker.populate_list(&text);
        });
    }

    // Key event controller — capture phase so we intercept before Entry consumes keys
    let tokio_handle_for_mru = tokio_handle.clone();
    let state_for_keys = Rc::clone(&state);
    let key_state = Rc::new(RefCell::new(crate::input::keymap::KeyState::default()));
    let key_controller = EventControllerKey::new();
    key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
    key_controller.connect_key_pressed(move |_controller, keyval, _keycode, modifier| {
        let key_name = keyval.name().unwrap_or_default();
        let is_ctrl = modifier.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
        let is_shift = modifier.contains(gtk4::gdk::ModifierType::SHIFT_MASK);
        let consumed = crate::input::keymap::handle_key(
            &state_for_keys,
            &key_state,
            &key_name,
            is_ctrl,
            is_shift,
            &tokio_handle,
        );
        if consumed {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(key_controller);

    window.present();

    // Startup: load MRU work or show picker
    if let Some(abbrev) = last_work {
        let state_clone = Rc::clone(&state);
        let handle = tokio_handle_for_mru;
        glib::spawn_future_local(async move {
            let work = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::queries::load_work(&conn, &abbrev)
                })
                .await;
            match work {
                Ok(Ok(work)) => {
                    let mut s = state_clone.borrow_mut();
                    display_work(&mut s, work);
                    // Jump to last line
                    if last_line > 0 {
                        s.current_line = last_line.min(
                            s.current_work.as_ref().map_or(0, |w| w.lines.len().saturating_sub(1)),
                        );
                        crate::input::navigation::restore_cursor(&mut s);
                    }
                }
                Ok(Err(_)) | Err(_) => {
                    // Work not found — fall back to picker
                    state_clone.borrow().picker.show();
                }
            }
        });
    } else {
        state.borrow().picker.show();
    }

    state
}

pub fn display_work(state: &mut AppState, work: Work) {
    let text: String = work
        .lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    state.buffer.set_text(&text);
    state
        .window
        .set_title(Some(&format!("{} — linux-lit", work.title)));

    // Save MRU to config
    state.config.last_work = Some(work.abbrev.clone());
    state.config.last_line = 0;
    crate::config::save(&state.config);

    state.current_line = 0;
    state.current_work = Some(work);

    // Apply initial highlight to first line
    if let Some(iter) = state.buffer.iter_at_line(0) {
        let mut line_end = iter;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        state
            .buffer
            .apply_tag(&state.highlight_tag, &iter, &line_end);
    }
}

/// Save current position to config (call on quit).
pub fn save_position(state: &mut AppState) {
    if let Some(work) = &state.current_work {
        state.config.last_work = Some(work.abbrev.clone());
        state.config.last_line = state.current_line;
        crate::config::save(&state.config);
    }
}
