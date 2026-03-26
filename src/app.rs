use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, CssProvider, EventControllerKey, ScrolledWindow, TextBuffer, TextView,
    WrapMode,
};

use crate::db::models::{Work, WorkSummary};
use crate::ui::library_picker::LibraryPicker;

pub struct AppState {
    pub text_view: TextView,
    pub buffer: TextBuffer,
    pub picker: LibraryPicker,
    pub current_work: Option<Work>,
    pub window: ApplicationWindow,
}

pub fn build_window(
    app: &gtk4::Application,
    works: Vec<WorkSummary>,
    tokio_handle: tokio::runtime::Handle,
) -> Rc<RefCell<AppState>> {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("linux-lit")
        .default_width(1000)
        .default_height(800)
        .build();

    let buffer = TextBuffer::new(None);
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

    let state = Rc::new(RefCell::new(AppState {
        text_view,
        buffer,
        picker,
        current_work: None,
        window: window.clone(),
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

    // Key event controller — single controller handles all keys
    let state_for_keys = Rc::clone(&state);
    let key_controller = EventControllerKey::new();
    key_controller.connect_key_pressed(move |_controller, keyval, _keycode, modifier| {
        let key_name = keyval.name().unwrap_or_default();

        // Ctrl+p: toggle library picker
        if modifier.contains(gtk4::gdk::ModifierType::CONTROL_MASK) && key_name == "p" {
            let state = state_for_keys.borrow();
            if state.picker.is_visible() {
                state.picker.hide();
            } else {
                state.picker.show();
            }
            return glib::Propagation::Stop;
        }

        // When picker is visible, handle picker keys
        let picker_visible = state_for_keys.borrow().picker.is_visible();
        if picker_visible {
            match key_name.as_str() {
                "Escape" => {
                    state_for_keys.borrow().picker.hide();
                    return glib::Propagation::Stop;
                }
                "Return" => {
                    let abbrev = state_for_keys.borrow().picker.selected_abbrev();
                    if let Some(abbrev) = abbrev {
                        let state_clone = Rc::clone(&state_for_keys);
                        let handle = tokio_handle.clone();
                        glib::spawn_future_local(async move {
                            let work = handle
                                .spawn_blocking(move || {
                                    let conn = crate::db::queries::open_db()
                                        .expect("Failed to open lit.db");
                                    crate::db::queries::load_work(&conn, &abbrev)
                                })
                                .await;
                            match work {
                                Ok(Ok(work)) => {
                                    let mut s = state_clone.borrow_mut();
                                    s.picker.hide();
                                    display_work(&mut s, work);
                                }
                                Ok(Err(e)) => eprintln!("Failed to load work: {}", e),
                                Err(e) => eprintln!("Task join error: {}", e),
                            }
                        });
                    }
                    return glib::Propagation::Stop;
                }
                "Down" => {
                    state_for_keys.borrow().picker.move_selection(1);
                    return glib::Propagation::Stop;
                }
                "Up" => {
                    state_for_keys.borrow().picker.move_selection(-1);
                    return glib::Propagation::Stop;
                }
                "j" => {
                    if !state_for_keys.borrow().picker.search_entry().has_focus() {
                        state_for_keys.borrow().picker.move_selection(1);
                        return glib::Propagation::Stop;
                    }
                }
                "k" => {
                    if !state_for_keys.borrow().picker.search_entry().has_focus() {
                        state_for_keys.borrow().picker.move_selection(-1);
                        return glib::Propagation::Stop;
                    }
                }
                _ => {}
            }
        }

        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    window.present();

    // Show picker on startup
    state.borrow().picker.show();

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
    state.current_work = Some(work);
}
