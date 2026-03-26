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
    pub page_top_line: usize,
    pub highlight_tag: gtk4::TextTag,
    pub scrolled_window: ScrolledWindow,
    pub window: ApplicationWindow,
    pub config: Config,
    /// Generation counter for crossfade animations. Incremented on each page turn
    /// so stale animation callbacks don't stomp on opacity.
    pub animation_gen: std::rc::Rc<std::cell::Cell<u64>>,
    pub cmd_tx: tokio::sync::mpsc::Sender<crate::mpv::MpvCommand>,
    pub tokio_handle: tokio::runtime::Handle,
}

pub fn build_window(
    app: &gtk4::Application,
    works: Vec<WorkSummary>,
    tokio_handle: tokio::runtime::Handle,
    config: Config,
    cmd_tx: tokio::sync::mpsc::Sender<crate::mpv::MpvCommand>,
) -> Rc<RefCell<AppState>> {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("linux-lit")
        .default_width(1000)
        .default_height(800)
        .build();

    // Load theme
    let theme_name = crate::theme::current_theme_name();
    let theme = if theme_name.is_empty() {
        crate::theme::load_theme("gruvbox-material")
    } else {
        crate::theme::load_theme(&theme_name)
    };
    crate::logging::log(&format!("Theme: {} ({})", theme.display_name, theme.name));

    let buffer = TextBuffer::new(None);
    let highlight_tag = gtk4::TextTag::builder()
        .name("current-line")
        .background(&theme.cursor_line_bg)
        .build();
    buffer.tag_table().add(&highlight_tag);

    let text_view = TextView::builder()
        .buffer(&buffer)
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(WrapMode::Word)
        .build();

    // Apply theme CSS
    let css_provider = CssProvider::new();
    let css = crate::theme::generate_css(
        &theme,
        &config.font_family,
        config.font_size,
    );
    css_provider.load_from_string(&css);
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css_provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Line spacing
    text_view.set_pixels_above_lines(14);
    text_view.set_pixels_below_lines(14);

    // Text area padding (inside the text background)
    text_view.set_left_margin(24);
    text_view.set_right_margin(24);
    text_view.set_top_margin(24);

    // Scrolled window — centered card with wallpaper visible on all sides
    // ~72 chars at 18pt serif ≈ 768px wide, with top/bottom margin for the card effect
    let scrolled = ScrolledWindow::builder()
        .child(&text_view)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::External)
        .vexpand(true)
        .halign(gtk4::Align::Center)
        .valign(gtk4::Align::Fill)
        .width_request(1000)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

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
        page_top_line: 0,
        highlight_tag,
        scrolled_window: scrolled,
        window: window.clone(),
        config,
        animation_gen: std::rc::Rc::new(std::cell::Cell::new(0)),
        cmd_tx,
        tokio_handle: tokio_handle.clone(),
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
                    {
                        let mut s = state_clone.borrow_mut();
                        display_work(&mut s, work);
                        // Set cursor to MRU line (or 0 for first canonical line)
                        s.current_line = last_line.min(
                            s.current_work
                                .as_ref()
                                .map_or(0, |w| w.lines.len().saturating_sub(1)),
                        );
                    }
                    // Defer highlight + scroll until after GTK lays out the text
                    glib::idle_add_local_once(move || {
                        crate::input::navigation::restore_cursor(
                            &mut state_clone.borrow_mut(),
                        );
                    });
                }
                Ok(Err(_)) | Err(_) => {
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

    // Send timestamp data to MPV client
    {
        let mut ts_data: Vec<(i64, f64, f64)> = work
            .timestamps
            .iter()
            .map(|t| (t.line_id, t.start, t.end))
            .collect();
        ts_data.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut id_to_idx: std::collections::HashMap<i64, usize> =
            std::collections::HashMap::new();
        for (i, line) in work.lines.iter().enumerate() {
            id_to_idx.insert(line.id, i);
        }
        let _ = state
            .cmd_tx
            .try_send(crate::mpv::MpvCommand::SetTimestamps {
                timestamps: ts_data,
                line_id_to_index: id_to_idx,
            });
    }

    // Find or launch MPV socket
    if !work.media_paths.is_empty() {
        let media_paths = work.media_paths.clone();
        let cmd_tx = state.cmd_tx.clone();
        let handle = state.tokio_handle.clone();
        glib::spawn_future_local(async move {
            let socket_path = handle
                .spawn_blocking(move || {
                    if let Some(path) =
                        crate::mpv::discovery::find_socket_for_work(&media_paths)
                    {
                        return path.to_string_lossy().to_string();
                    }
                    let launched = crate::mpv::discovery::launch_mpv(&media_paths[0]);
                    for _ in 0..60 {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        if std::path::Path::new(&launched).exists() {
                            return launched;
                        }
                    }
                    launched
                })
                .await
                .unwrap_or_default();

            if !socket_path.is_empty() {
                let _ = cmd_tx
                    .send(crate::mpv::MpvCommand::Connect(socket_path))
                    .await;
            }
        });
    }

    state.current_line = 0;
    state.page_top_line = 0;
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
