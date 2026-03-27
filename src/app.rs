use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, CssProvider, EventControllerKey, ScrolledWindow, WrapMode,
};
use sourceview5::prelude::*;
use sourceview5::View;

use crate::config::Config;
use crate::db::models::{Work, WorkSummary};
use crate::ui::library_picker::LibraryPicker;
use crate::ui::media_picker::MediaPicker;
use crate::ui::search_bar::SearchBar;

#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub line_index: usize,
    pub byte_start: usize,
    pub byte_end: usize,
}

#[allow(dead_code)]
pub struct AppState {
    pub text_view: View,
    pub buffer: sourceview5::Buffer,
    pub picker: LibraryPicker,
    pub current_work: Option<Work>,
    pub current_line: usize,
    pub page_top_line: usize,
    pub dim_tag: gtk4::TextTag,
    pub ab_dim_tag: gtk4::TextTag,
    pub scrolled_window: ScrolledWindow,
    pub window: ApplicationWindow,
    pub config: Config,
    pub css_provider: CssProvider,
    pub theme: crate::theme::Theme,
    /// Generation counter for crossfade animations. Incremented on each page turn
    /// so stale animation callbacks don't stomp on opacity.
    pub animation_gen: std::rc::Rc<std::cell::Cell<u64>>,
    pub cmd_tx: tokio::sync::mpsc::Sender<crate::mpv::MpvCommand>,
    pub tokio_handle: tokio::runtime::Handle,
    pub playback_speed: f64,
    pub search_bar: SearchBar,
    pub search_matches: Vec<SearchMatch>,
    pub search_match_idx: usize,
    pub search_tag: gtk4::TextTag,
    pub search_current_tag: gtk4::TextTag,
    pub current_time_pos: f64,
    pub media_id: Option<i64>,
    pub sign_column_visible: Rc<Cell<bool>>,
    pub gutter_renderer: Option<sourceview5::GutterRendererText>,
    pub chunk_renderer: Option<sourceview5::GutterRendererText>,
    pub ab_repeat: crate::ab_repeat::AbRepeatState,
    pub ab_a_line: Rc<Cell<Option<usize>>>,
    pub ab_b_line: Rc<Cell<Option<usize>>>,
    pub line_map: Option<crate::text_file_map::LineMap>,
    pub settings_overlay: crate::ui::settings_overlay::SettingsOverlay,
    pub media_picker: MediaPicker,
    pub dialogue_formatting_active: bool,
    pub translations: HashMap<i64, String>,
    pub translations_visible: bool,
    /// Tracks which buffer lines are inserted translation lines.
    pub translation_lines: Vec<bool>,
    pub translation_dim_tag: gtk4::TextTag,
    pub translation_text_tag: gtk4::TextTag,
    /// When set, CursorSync events are suppressed until this instant passes.
    /// Prevents playback sync from overriding manual navigation.
    pub suppress_sync_until: Option<std::time::Instant>,
}

impl AppState {
    pub fn effective_line_count(&self) -> usize {
        if let Some(ref lm) = self.line_map {
            lm.buffer_to_work.len()
        } else {
            self.current_work.as_ref().map_or(0, |w| w.lines.len())
        }
    }

    pub fn work_line_for_buffer(&self, buffer_line: usize) -> Option<usize> {
        if let Some(ref lm) = self.line_map {
            lm.buffer_to_work.get(buffer_line).copied().flatten()
        } else {
            let count = self.current_work.as_ref().map_or(0, |w| w.lines.len());
            if buffer_line < count { Some(buffer_line) } else { None }
        }
    }
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
    crate::logging::log(&format!("Highlight color: {}", theme.cursor_line_bg));

    let buffer = sourceview5::Buffer::new(None);
    // Disable sourceview5's default style scheme so our CSS controls all colors
    buffer.set_style_scheme(None::<&sourceview5::StyleScheme>);
    let dim_tag = gtk4::TextTag::builder()
        .name("dim")
        .foreground(&theme.dim_fg)
        .build();
    buffer.tag_table().add(&dim_tag);

    let ab_dim_tag = gtk4::TextTag::builder()
        .name("ab-dim")
        .foreground(&theme.dim_fg)
        .build();
    buffer.tag_table().add(&ab_dim_tag);

    let search_tag = gtk4::TextTag::builder()
        .name("search-match")
        .background(if theme.is_light {
            "rgba(255, 200, 0, 0.35)"
        } else {
            "rgba(255, 200, 0, 0.25)"
        })
        .build();
    buffer.tag_table().add(&search_tag);

    let search_current_tag = gtk4::TextTag::builder()
        .name("search-current")
        .background(if theme.is_light {
            "rgba(255, 140, 0, 0.55)"
        } else {
            "rgba(255, 140, 0, 0.45)"
        })
        .build();
    buffer.tag_table().add(&search_current_tag);

    let translation_dim_tag = gtk4::TextTag::builder()
        .name("translation-dim")
        .foreground(&theme.dim_fg)
        .build();
    buffer.tag_table().add(&translation_dim_tag);

    let translation_text_tag = gtk4::TextTag::builder()
        .name("translation-text")
        .style(pango::Style::Italic)
        .left_margin(60)
        .build();
    buffer.tag_table().add(&translation_text_tag);

    let text_view = View::builder()
        .buffer(&buffer)
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(WrapMode::Word)
        .build();

    text_view.set_show_line_numbers(false);
    text_view.set_highlight_current_line(false);

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
    text_view.set_pixels_above_lines(config.line_spacing as i32);
    text_view.set_pixels_below_lines(config.line_spacing as i32);

    // Text area padding (inside the text background)
    text_view.set_left_margin(config.text_margins as i32);
    text_view.set_right_margin(config.text_margins as i32);
    text_view.set_top_margin(24);

    // Scrolled window — centered card with wallpaper visible on all sides
    let scrolled = ScrolledWindow::builder()
        .child(&text_view)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::External)
        .vexpand(true)
        .halign(gtk4::Align::Center)
        .valign(gtk4::Align::Fill)
        .width_request(config.column_width as i32)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .css_classes(vec!["text-card"])
        .overflow(gtk4::Overflow::Hidden)
        .build();

    // Library picker overlay
    let mut picker = LibraryPicker::new();
    picker.set_works(works);
    picker.attach(&scrolled);
    picker.overlay.set_vexpand(true);

    // Media picker overlay wraps the library picker overlay
    let media_picker = MediaPicker::new();
    media_picker.attach(&picker.overlay);
    media_picker.overlay.set_vexpand(true);

    // Settings overlay wraps the media picker overlay
    let all_themes = crate::theme::load_all_themes();
    let settings_overlay = crate::ui::settings_overlay::SettingsOverlay::new(
        all_themes,
        &theme.name,
    );

    settings_overlay.attach(&media_picker.overlay);
    settings_overlay.overlay.set_vexpand(true);

    // Search bar at bottom
    let search_bar = SearchBar::new();
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    vbox.append(&settings_overlay.overlay);
    vbox.append(&search_bar.container);

    window.set_child(Some(&vbox));

    let last_work = config.last_work.clone();
    let last_line = config.last_line;

    let state = Rc::new(RefCell::new(AppState {
        text_view,
        buffer,
        picker,
        current_work: None,
        current_line: 0,
        page_top_line: 0,
        dim_tag,
        ab_dim_tag,
        scrolled_window: scrolled,
        window: window.clone(),
        config,
        css_provider,
        theme,
        animation_gen: std::rc::Rc::new(std::cell::Cell::new(0)),
        cmd_tx,
        tokio_handle: tokio_handle.clone(),
        playback_speed: 1.0,
        search_bar,
        search_matches: Vec::new(),
        search_match_idx: 0,
        search_tag,
        search_current_tag,
        current_time_pos: 0.0,
        media_id: None,
        sign_column_visible: Rc::new(Cell::new(false)),
        gutter_renderer: None,
        chunk_renderer: None,
        ab_repeat: crate::ab_repeat::AbRepeatState::default(),
        ab_a_line: Rc::new(Cell::new(None)),
        ab_b_line: Rc::new(Cell::new(None)),
        line_map: None,
        settings_overlay,
        media_picker,
        dialogue_formatting_active: false,
        translations: HashMap::new(),
        translations_visible: false,
        translation_lines: Vec::new(),
        translation_dim_tag,
        translation_text_tag,
        suppress_sync_until: None,
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

    // Connect media picker search entry filter
    let state_for_media_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.media_picker.search_entry().connect_changed(move |entry| {
            let text = entry.text();
            state_for_media_filter
                .borrow()
                .media_picker
                .populate_list(&text);
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
        let is_alt = modifier.contains(gtk4::gdk::ModifierType::ALT_MASK);
        let consumed = crate::input::keymap::handle_key(
            &state_for_keys,
            &key_state,
            &key_name,
            is_ctrl,
            is_shift,
            is_alt,
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
                            s.effective_line_count().saturating_sub(1),
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
    crate::input::search::clear_search(state);
    state.search_bar.hide();
    state.current_time_pos = 0.0;
    state.media_id = work.media_id;
    state
        .window
        .set_title(Some(&format!("{} — linux-lit", work.title)));

    // Save MRU to config
    state.config.last_work = Some(work.abbrev.clone());
    state.config.last_line = 0;
    crate::config::save(&state.config);

    // Send timestamp data to MPV client (filtered by active media_id)
    {
        let active_media_id = state.media_id;
        let mut ts_data: Vec<(i64, f64, f64)> = work
            .timestamps
            .iter()
            .filter(|t| active_media_id.map_or(true, |mid| t.media_id == mid))
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

    // Build buffer text (with or without sign column)
    state.line_map = None;
    state.dialogue_formatting_active = false;
    rebuild_buffer_text(state);
    apply_dialogue_formatting(state);

    // Set up gutter: remove old renderer, place marks, create new renderer
    if let Some(old_renderer) = state.gutter_renderer.take() {
        crate::gutter::remove_gutter_renderer(&state.text_view, &old_renderer);
    }
    let has_timestamp: Vec<bool> = if let Some(ref lm) = state.line_map {
        lm.buffer_to_work
            .iter()
            .map(|opt_idx| {
                opt_idx
                    .and_then(|idx| state.current_work.as_ref()?.lines.get(idx)?.timestamp.as_ref())
                    .is_some()
            })
            .collect()
    } else {
        state
            .current_work
            .as_ref()
            .map(|w| w.lines.iter().map(|l| l.timestamp.is_some()).collect())
            .unwrap_or_default()
    };
    crate::gutter::place_timestamp_marks(&state.buffer, &has_timestamp);
    let renderer = crate::gutter::setup_timestamp_gutter(
        &state.text_view,
        state.sign_column_visible.clone(),
        has_timestamp,
        state.ab_a_line.clone(),
        state.ab_b_line.clone(),
    );
    state.gutter_renderer = Some(renderer);

    // Load chunks for the current work
    if let Some(media_id) = state.current_work.as_ref().and_then(|w| w.media_id) {
        if let Ok(conn) = crate::db::queries::open_db() {
            let abbrev = &state.current_work.as_ref().unwrap().abbrev;
            if let Ok(chunks) = crate::db::chunks::load_chunks(&conn, abbrev, media_id) {
                crate::logging::log(&format!("CHUNKS: loaded {} chunks", chunks.len()));
                state.ab_repeat.chunks = chunks;
                state.ab_repeat.chunk_index = None;
            }
        }
    }

    // Set up chunk bar gutter
    if let Some(old_renderer) = state.chunk_renderer.take() {
        crate::gutter::remove_gutter_renderer(&state.text_view, &old_renderer);
    }
    if !state.ab_repeat.chunks.is_empty() {
        if let Some(ref work) = state.current_work {
            let renderer = crate::gutter::setup_chunk_gutter(
                &state.text_view,
                state.sign_column_visible.clone(),
                &state.ab_repeat.chunks,
                &work.lines,
                state.line_map.as_ref(),
            );
            state.chunk_renderer = Some(renderer);
        }
    }

    // Apply font tag to new buffer content
    reapply_font(state);

    // Dim all lines except the current one
    crate::input::navigation::update_highlight_and_ensure_visible(state);
}

/// Rebuild the buffer text from current_work.
/// If the work has a text_file and it exists, load from file and build a line map.
/// Otherwise, join work.lines as before.
fn rebuild_buffer_text(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    if let Some(ref path) = work.text_file {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                let file_lines: Vec<String> = contents.lines().map(String::from).collect();
                let is_prose = crate::db::line_types::is_prose_work(&work.work_type);
                let line_map = crate::text_file_map::build_line_map(&file_lines, &work.lines, is_prose);
                state.buffer.set_text(&contents);
                state.line_map = Some(line_map);
                crate::logging::log(&format!(
                    "TEXT_FILE: loaded {} lines from {}",
                    file_lines.len(),
                    path
                ));
                return;
            }
            Err(e) => {
                crate::logging::log(&format!(
                    "TEXT_FILE: WARNING — failed to read {}: {}",
                    path, e
                ));
            }
        }
    }

    // Default: join work.lines
    state.line_map = None;
    let text: String = work
        .lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    state.buffer.set_text(&text);
}

/// Apply dialogue indentation and tight spacing for text-file mode.
/// Scans buffer lines for speaker patterns. If speakers found:
/// - Sets global line spacing to 0
/// - Applies "dialogue-indent" tag (extra left margin) to dialogue lines
/// - Applies "speaker-gap" tag (extra pixels above) to speaker lines
/// - Applies "stage-direction-gap" tag to stage directions
fn apply_dialogue_formatting(state: &mut AppState) {
    use crate::db::line_types;

    // Only in text-file mode
    if state.line_map.is_none() {
        state.dialogue_formatting_active = false;
        return;
    }

    // Scan first 200 lines for any speaker
    let line_count = state.buffer.line_count() as usize;
    let scan_limit = line_count.min(200);
    let mut has_speakers = false;
    for i in 0..scan_limit {
        let iter = match state.buffer.iter_at_line(i as i32) {
            Some(it) => it,
            None => continue,
        };
        let end = if i + 1 < line_count {
            state.buffer.iter_at_line((i + 1) as i32).unwrap_or_else(|| state.buffer.end_iter())
        } else {
            state.buffer.end_iter()
        };
        let text = state.buffer.text(&iter, &end, false);
        let text = text.trim_end_matches('\n');
        if line_types::is_speaker(text) {
            has_speakers = true;
            break;
        }
    }

    if !has_speakers {
        state.dialogue_formatting_active = false;
        return;
    }

    state.dialogue_formatting_active = true;

    // Set global spacing to 0
    state.text_view.set_pixels_above_lines(0);
    state.text_view.set_pixels_below_lines(0);

    // Remove old formatting tags if they exist
    let tag_table = state.buffer.tag_table();
    for name in &["dialogue-indent", "speaker-gap", "stage-direction-gap",
                   "speaker-name", "stage-direction-style", "act-scene-header"] {
        if let Some(old) = tag_table.lookup(name) {
            tag_table.remove(&old);
        }
    }

    // Create tags
    let base_margin = state.text_view.left_margin();
    let speaker_gap = state.config.line_spacing.max(1) as i32;

    let indent_tag = gtk4::TextTag::builder()
        .name("dialogue-indent")
        .left_margin(base_margin + 60)
        .build();

    let speaker_gap_tag = gtk4::TextTag::builder()
        .name("speaker-gap")
        .pixels_above_lines(speaker_gap * 5)
        .build();

    let stage_gap_tag = gtk4::TextTag::builder()
        .name("stage-direction-gap")
        .pixels_above_lines(10)
        .build();

    let speaker_name_tag = gtk4::TextTag::builder()
        .name("speaker-name")
        .variant(pango::Variant::SmallCaps)
        .scale(0.85)
        .build();

    let stage_italic_tag = gtk4::TextTag::builder()
        .name("stage-direction-style")
        .style(pango::Style::Italic)
        .build();

    let act_scene_tag = gtk4::TextTag::builder()
        .name("act-scene-header")
        .weight(700)
        .pixels_above_lines(20)
        .build();

    tag_table.add(&indent_tag);
    tag_table.add(&speaker_gap_tag);
    tag_table.add(&stage_gap_tag);
    tag_table.add(&speaker_name_tag);
    tag_table.add(&stage_italic_tag);
    tag_table.add(&act_scene_tag);

    // Apply tags per line
    for i in 0..line_count {
        let line_start = match state.buffer.iter_at_line(i as i32) {
            Some(iter) => iter,
            None => continue,
        };
        let line_end = if i + 1 < line_count {
            match state.buffer.iter_at_line((i + 1) as i32) {
                Some(iter) => iter,
                None => state.buffer.end_iter(),
            }
        } else {
            state.buffer.end_iter()
        };

        let text = state.buffer.text(&line_start, &line_end, false);
        let text = text.trim_end_matches('\n');

        if line_types::is_blank(text) {
            continue;
        } else if line_types::is_speaker(text) {
            state.buffer.apply_tag(&speaker_gap_tag, &line_start, &line_end);
            state.buffer.apply_tag(&speaker_name_tag, &line_start, &line_end);
        } else if line_types::is_stage_direction(text) {
            state.buffer.apply_tag(&stage_gap_tag, &line_start, &line_end);
            state.buffer.apply_tag(&indent_tag, &line_start, &line_end);
            state.buffer.apply_tag(&stage_italic_tag, &line_start, &line_end);
        } else if line_types::is_act_scene_marker(text) || line_types::is_separator(text) {
            state.buffer.apply_tag(&act_scene_tag, &line_start, &line_end);
        } else {
            // Dialogue line — indent
            state.buffer.apply_tag(&indent_tag, &line_start, &line_end);
        }
    }

    crate::logging::log(&format!(
        "FORMATTING: applied dialogue formatting ({} lines)",
        line_count
    ));
}

/// Toggle sign column visibility.
pub fn toggle_sign_column(state: &mut AppState) {
    let new_val = !state.sign_column_visible.get();
    state.sign_column_visible.set(new_val);
    // Queue redraw on gutter renderers so query_data re-evaluates visibility
    if let Some(ref renderer) = state.gutter_renderer {
        renderer.queue_draw();
    }
    if let Some(ref renderer) = state.chunk_renderer {
        renderer.queue_draw();
    }
    crate::logging::log(&format!(
        "SIGN: signs {}",
        if new_val { "shown" } else { "hidden" },
    ));
}

/// Reapply font size using a TextTag spanning the entire buffer.
fn reapply_font(state: &AppState) {
    let tag_table = state.buffer.tag_table();
    // Remove old font tag if it exists
    if let Some(old) = tag_table.lookup("font-size") {
        tag_table.remove(&old);
    }
    let font_str = format!("{} {}",  state.config.font_family, state.config.font_size);
    let tag = gtk4::TextTag::builder()
        .name("font-size")
        .font(&font_str)
        .build();
    tag_table.add(&tag);
    let start = state.buffer.start_iter();
    let end = state.buffer.end_iter();
    state.buffer.apply_tag(&tag, &start, &end);
    // Also update CSS for consistency
    let css = crate::theme::generate_css(&state.theme, &state.config.font_family, state.config.font_size);
    state.css_provider.load_from_string(&css);
    crate::logging::log(&format!("FONT: reapply_font size={}pt via TextTag", state.config.font_size));
}

/// Adjust font size by delta, clamp to 8..=72, reapply CSS and repaginate.
pub fn adjust_font_size(state: &mut AppState, delta: i32) {
    let new_size = (state.config.font_size as i32 + delta).clamp(8, 72) as u32;
    if new_size == state.config.font_size {
        return;
    }
    state.config.font_size = new_size;
    reapply_font(state);
    crate::config::save(&state.config);
}

/// Reset font size to default (18pt).
pub fn reset_font_size(state: &mut AppState) {
    let default = 16u32;
    if state.config.font_size == default {
        return;
    }
    state.config.font_size = default;
    reapply_font(state);
    crate::config::save(&state.config);
}

/// Cycle font family forward (f) or backward (F).
pub fn cycle_font(state: &mut AppState, forward: bool) {
    let cycle = crate::config::FONT_CYCLE;
    let current = &state.config.font_family;
    let idx = cycle.iter().position(|f| *f == current).unwrap_or(0);
    let next = if forward {
        (idx + 1) % cycle.len()
    } else {
        (idx + cycle.len() - 1) % cycle.len()
    };
    state.config.font_family = cycle[next].to_string();
    reapply_font(state);
    crate::config::save(&state.config);
    let position = format!("{}/{}", next + 1, cycle.len());
    let body = format!("{} {}pt", state.config.font_family, state.config.font_size);
    crate::logging::log(&format!("FONT: cycled to {}", state.config.font_family));
    let _ = std::process::Command::new("notify-send")
        .args(["-t", "1500", "-h", "string:x-canonical-private-synchronous:linux-lit-font",
               &format!("Font [{}]", position), &body])
        .spawn();
}

/// Show current font info via desktop notification.
pub fn show_font_info(state: &AppState) {
    let cycle = crate::config::FONT_CYCLE;
    let idx = cycle.iter().position(|f| *f == state.config.font_family).unwrap_or(0);
    let position = format!("{}/{}", idx + 1, cycle.len());
    let body = format!("{} {}pt", state.config.font_family, state.config.font_size);
    let _ = std::process::Command::new("notify-send")
        .args(["-t", "1500", "-h", "string:x-canonical-private-synchronous:linux-lit-font",
               &format!("Font [{}]", position), &body])
        .spawn();
}

/// Apply AB loop dimming: dim everything outside the A-B line range.
pub fn apply_ab_dim(state: &AppState) {
    let buffer = &state.buffer;
    let tag = &state.ab_dim_tag;

    // First remove any existing AB dim
    let (buf_start, buf_end) = buffer.bounds();
    buffer.remove_tag(tag, &buf_start, &buf_end);

    let (a_line, b_line) = match (state.ab_repeat.a_line, state.ab_repeat.b_line) {
        (Some(a), Some(b)) => (a, b),
        _ => return,
    };

    if !state.ab_repeat.loop_active {
        return;
    }

    // Dim lines before A
    if a_line > 0 {
        if let Some(dim_end) = buffer.iter_at_line(a_line as i32) {
            buffer.apply_tag(tag, &buf_start, &dim_end);
        }
    }

    // Dim lines after B
    if let Some(dim_start_iter) = buffer.iter_at_line((b_line + 1) as i32) {
        buffer.apply_tag(tag, &dim_start_iter, &buf_end);
    }
}

/// Remove AB loop dimming.
pub fn remove_ab_dim(state: &AppState) {
    let buffer = &state.buffer;
    let tag = &state.ab_dim_tag;
    let (buf_start, buf_end) = buffer.bounds();
    buffer.remove_tag(tag, &buf_start, &buf_end);
}

/// Save current position to config (call on quit).
pub fn save_position(state: &mut AppState) {
    if let Some(work) = &state.current_work {
        state.config.last_work = Some(work.abbrev.clone());
        state.config.last_line = state.current_line;
        crate::config::save(&state.config);
    }
}
