use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, CssProvider, EventControllerKey, ScrolledWindow, WrapMode,
};
use libadwaita as adw;
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

#[derive(Debug, Clone)]
pub struct VocabMatch {
    pub word: String,
    pub line_index: usize,
    pub char_start: usize,
    pub char_end: usize,
}

#[allow(dead_code)]
pub struct AppState {
    pub text_view: View,
    pub buffer: sourceview5::Buffer,
    pub picker: LibraryPicker,
    pub current_work: Option<Work>,
    pub current_line: usize,
    pub prev_highlight_line: std::cell::Cell<Option<usize>>,
    pub page_top_line: usize,
    pub dim_tag: gtk4::TextTag,
    pub cursor_line_tag: gtk4::TextTag,
    pub cursor_fade_tag: gtk4::TextTag,
    pub ab_dim_tag: gtk4::TextTag,
    pub page_turn_overlay: gtk4::Overlay,
    pub bottom_clip: gtk4::Box,
    pub top_spacer: gtk4::Box,
    pub bottom_spacer: gtk4::Box,
    pub card_vbox: gtk4::Box,
    pub scrolled_window: ScrolledWindow,
    pub content_hbox: gtk4::Box,
    pub window: ApplicationWindow,
    pub config: Config,
    pub css_provider: CssProvider,
    pub theme: crate::theme::Theme,
    /// Active page-turn animation (crossfade or slide). Stored so it can be
    /// cancelled via .skip() if a new page turn fires mid-flight.
    pub page_turn_anim: Option<adw::TimedAnimation>,
    /// Active cursor highlight fade-out animation.
    pub cursor_fade_anim: Option<adw::TimedAnimation>,
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
    pub has_timestamp: Rc<RefCell<Vec<bool>>>,
    pub is_chapter_line: Rc<RefCell<Vec<bool>>>,
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
    /// When set, advance cursor to the given buffer line once time_pos exceeds
    /// the end time. Fields: (end_time, next_buffer_line, source_work_line_idx).
    pub pending_advance: Option<(f64, usize, usize)>,
    /// After pending_advance fires, ignore CursorSync that would pull the cursor
    /// back to this buffer line (the timestamped source line). Cleared when
    /// CursorSync targets any other line.
    pub pending_advance_ignore_bl: Option<usize>,
    pub visual_selection: Option<crate::input::visual::SelectionState>,
    pub selection_tag: gtk4::TextTag,
    pub action_popup: Option<crate::input::visual::ActionPopupState>,
    pub action_popup_widget: crate::ui::action_popup::ActionPopup,
    pub keybinds_overlay: crate::ui::keybinds_overlay::KeybindsOverlay,
    pub correction_overlay: crate::ui::correction_overlay::CorrectionOverlay,
    pub gloss_original_text: Option<String>,
    pub vocab_words: std::collections::HashSet<String>,
    pub vocab_matches: Vec<VocabMatch>,
    pub vocab_match_idx: Option<usize>,
    pub vocab_tag: gtk4::TextTag,
    pub dim_enabled: bool,
    pub vocab_highlight_visible: bool,
    pub vocab_popup: crate::ui::vocab_popup::VocabPopup,
    pub vocab_popup_data: Vec<crate::ui::vocab_popup::VocabWordData>,
    pub vocab_popup_index: usize,
    pub vocab_popup_view: crate::ui::vocab_popup::VocabView,
    pub vocab_popup_auto: bool,
    /// Generation counter for the vocab popup auto-hide timer. Incremented
    /// on each backslash/numbersign press; when the timer fires it only hides
    /// if the generation hasn't changed.
    pub vocab_popup_fade_gen: Rc<Cell<u64>>,
    pub concordance_picker: crate::ui::concordance_picker::ConcordancePicker,
    pub concordance_state: Option<crate::concordance::ConcordanceState>,
    pub concordance_word_picker: crate::ui::concordance_word_picker::ConcordanceWordPicker,
    pub concordance_list_picker: crate::ui::concordance_list_picker::ConcordanceListPicker,
    pub concordance_bar: crate::ui::concordance_bar::ConcordanceBar,
    /// Index of the current sentence group (for prose with text_file).
    pub current_sentence_group: Option<usize>,
    /// Tracks the start line of the current paragraph to detect transitions.
    pub current_paragraph_start: Option<usize>,
    pub sync_enabled: bool,
    pub sync_icon: gtk4::Label,
    pub word_status_label: gtk4::Label,
    pub word_cycle_line: Option<usize>,
    pub word_cycle_index: usize,
    pub word_status_timer: Rc<Cell<u64>>,
    pub word_bold_tag: gtk4::TextTag,
    pub word_bold_gen: Rc<Cell<u64>>,
    pub word_collect_words: Vec<String>,
    pub word_collect_ranges: Vec<(usize, usize)>,
    /// True while display_work is replacing the buffer. CursorSync and other
    /// layout-dependent callbacks must skip when this is set because GTK
    /// hasn't laid out the new content yet. Cleared in an idle callback after
    /// the layout has settled.
    pub loading_work: Rc<Cell<bool>>,
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

    /// Check if a buffer line is within the currently highlighted sentence group.
    #[allow(dead_code)]
    pub fn is_in_current_sentence(&self, line_index: usize) -> bool {
        if line_index == self.current_line {
            return true;
        }
        if let Some(ref lm) = self.line_map {
            if let Some(group) = crate::text_file_map::sentence_group_for(
                &lm.sentence_groups,
                self.current_line,
            ) {
                return group.line_range.contains(&line_index);
            }
        }
        false
    }

    /// Return the line range (start..end exclusive) of the current paragraph
    /// (contiguous non-blank lines around current_line).
    pub fn current_paragraph_range(&self) -> std::ops::Range<usize> {
        let line_count = self.buffer.line_count() as usize;
        if self.current_line >= line_count {
            return 0..0;
        }

        let is_blank = |line: usize| -> bool {
            let Some(start_it) = self.buffer.iter_at_line(line as i32) else {
                return true;
            };
            let mut end_it = start_it;
            if !end_it.ends_line() {
                end_it.forward_to_line_end();
            }
            self.buffer.text(&start_it, &end_it, false).trim().is_empty()
        };

        // Find paragraph start: walk backwards from current_line
        let mut start = self.current_line;
        while start > 0 && !is_blank(start - 1) {
            start -= 1;
        }

        // Find paragraph end (exclusive): walk forwards from current_line
        let mut end = self.current_line + 1;
        while end < line_count && !is_blank(end) {
            end += 1;
        }

        start..end
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
    crate::logging::log("BUILD: loading_work guard active");
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

    let cursor_line_tag = gtk4::TextTag::builder()
        .name("cursor-line")
        .paragraph_background(&theme.cursor_line_bg)
        .build();
    buffer.tag_table().add(&cursor_line_tag);

    let cursor_fade_tag = gtk4::TextTag::builder()
        .name("cursor-fade")
        .paragraph_background(&theme.cursor_line_bg)
        .build();
    buffer.tag_table().add(&cursor_fade_tag);

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
        .build();
    buffer.tag_table().add(&translation_text_tag);

    let selection_tag = gtk4::TextTag::builder()
        .name("visual-selection")
        .background(if theme.is_light {
            "rgba(38, 109, 211, 0.15)"
        } else {
            "rgba(68, 138, 255, 0.25)"
        })
        .build();
    buffer.tag_table().add(&selection_tag);

    let vocab_tag = gtk4::TextTag::builder()
        .name("vocab-word")
        .foreground(&theme.vocab_fg)
        .build();
    buffer.tag_table().add(&vocab_tag);

    let word_bold_tag = gtk4::TextTag::builder()
        .name("word-bold")
        .underline(pango::Underline::Single)
        .build();
    buffer.tag_table().add(&word_bold_tag);

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
    text_view.set_right_margin(config.text_margins as i32 + crate::config::EXTRA_RIGHT_MARGIN);
    text_view.set_top_margin(0);
    text_view.set_bottom_margin(10);

    // Scrolled window — centered card with wallpaper visible on all sides
    let scrolled = ScrolledWindow::builder()
        .child(&text_view)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::External)
        .vexpand(true)
        .hexpand(true)
        .valign(gtk4::Align::Fill)
        .overflow(gtk4::Overflow::Hidden)
        .build();

    scrolled.add_css_class("card-middle");

    // Scrolled overlay — holds the bottom clip bar over the scrolled text area
    let scrolled_overlay = gtk4::Overlay::new();
    scrolled_overlay.set_child(Some(&scrolled));
    scrolled_overlay.set_vexpand(true);
    scrolled_overlay.set_hexpand(true);

    // Bottom clip bar — covers partially-visible lines at the bottom of a page.
    // Height is set dynamically by snap_scroll_to_line.
    let bottom_clip = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    bottom_clip.set_valign(gtk4::Align::End);
    bottom_clip.set_hexpand(true);
    bottom_clip.set_height_request(0);
    bottom_clip.add_css_class("card-middle");
    scrolled_overlay.add_overlay(&bottom_clip);

    // Top spacer — one line height, rounded top corners only
    let top_spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    top_spacer.set_hexpand(true);
    top_spacer.set_height_request(24); // updated dynamically after font metrics known
    top_spacer.add_css_class("card-top");

    // Bottom spacer — one line height, rounded bottom corners only
    let bottom_spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    bottom_spacer.set_hexpand(true);
    bottom_spacer.set_height_request(24); // updated dynamically after font metrics known
    bottom_spacer.add_css_class("card-bottom");

    // Vertical card assembly: top spacer + text card + bottom spacer
    let card_vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    card_vbox.set_vexpand(true);
    card_vbox.append(&top_spacer);
    card_vbox.append(&scrolled_overlay);
    card_vbox.append(&bottom_spacer);

    // Page turn overlay — wraps the entire card for crossfade snapshots.
    // Snapshot is placed here as a sibling of card_vbox so fading card_vbox
    // opacity doesn't also hide the snapshot.
    let page_turn_overlay = gtk4::Overlay::new();
    page_turn_overlay.set_child(Some(&card_vbox));
    page_turn_overlay.set_vexpand(true);
    page_turn_overlay.set_hexpand(true);
    page_turn_overlay.add_css_class("page-turn-overlay");

    // Centered text card container — width_request controls the card width
    let content_hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    content_hbox.set_halign(gtk4::Align::Center);
    content_hbox.set_valign(gtk4::Align::Fill);
    content_hbox.set_vexpand(true);
    content_hbox.set_width_request(config.column_width as i32);
    content_hbox.set_margin_top(24);
    content_hbox.set_margin_bottom(24);
    content_hbox.set_margin_start(24);
    content_hbox.set_margin_end(24);
    content_hbox.append(&page_turn_overlay);

    // Vocab popup (bottom-right, full window width)
    let vocab_popup = crate::ui::vocab_popup::VocabPopup::new();

    // Library picker overlay
    let mut picker = LibraryPicker::new();
    picker.set_works(works);
    picker.attach(&content_hbox);
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

    // Keybinds overlay wraps the settings overlay
    let keybinds_overlay = crate::ui::keybinds_overlay::KeybindsOverlay::new();
    keybinds_overlay.attach(&settings_overlay.overlay);
    keybinds_overlay.overlay.set_vexpand(true);

    // Correction overlay wraps the keybinds overlay
    let correction_overlay = crate::ui::correction_overlay::CorrectionOverlay::new(config.column_width);
    correction_overlay.attach(&keybinds_overlay.overlay);
    correction_overlay.overlay.set_vexpand(true);

    // Concordance picker wraps the correction overlay
    let concordance_picker = crate::ui::concordance_picker::ConcordancePicker::new();
    concordance_picker.attach(&correction_overlay.overlay);
    concordance_picker.overlay.set_vexpand(true);

    // Concordance word picker wraps the concordance picker
    let concordance_word_picker = crate::ui::concordance_word_picker::ConcordanceWordPicker::new();
    concordance_word_picker.attach(&concordance_picker.overlay);
    concordance_word_picker.overlay.set_vexpand(true);

    // Concordance list picker wraps the word picker
    let concordance_list_picker = crate::ui::concordance_list_picker::ConcordanceListPicker::new();
    concordance_list_picker.attach(&concordance_word_picker.overlay);
    concordance_list_picker.overlay.set_vexpand(true);

    // Action popup overlay for visual mode
    let action_popup_widget = crate::ui::action_popup::ActionPopup::new();
    concordance_list_picker.overlay.add_overlay(&action_popup_widget.container);

    // Add vocab popup to full-width overlay so it appears to the right of the text card
    vocab_popup.attach_to(&concordance_list_picker.overlay);

    // Sync-off indicator (lower-left corner of window, hidden by default)
    let sync_icon = gtk4::Label::new(Some("⇄\u{0338}"));
    sync_icon.set_valign(gtk4::Align::End);
    sync_icon.set_halign(gtk4::Align::Start);
    sync_icon.set_margin_start(12);
    sync_icon.set_margin_bottom(12);
    sync_icon.add_css_class("sync-off-icon");
    sync_icon.set_visible(false);
    concordance_list_picker.overlay.add_overlay(&sync_icon);

    // Word-copy status indicator (lower-left corner, hidden by default)
    let word_status_label = gtk4::Label::new(None);
    word_status_label.set_valign(gtk4::Align::End);
    word_status_label.set_halign(gtk4::Align::Start);
    word_status_label.set_margin_start(12);
    word_status_label.set_margin_bottom(40);
    word_status_label.add_css_class("word-status");
    word_status_label.set_visible(false);
    concordance_list_picker.overlay.add_overlay(&word_status_label);

    // Concordance status bar
    let concordance_bar = crate::ui::concordance_bar::ConcordanceBar::new();

    // Search bar at bottom
    let search_bar = SearchBar::new();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    vbox.append(&concordance_list_picker.overlay);
    vbox.append(&concordance_bar.container);
    vbox.append(&search_bar.container);

    window.set_child(Some(&vbox));

    // Override startup work/line with env vars (used by concordance cross-work spawn)
    let (last_work, last_line) = if let Ok(work_abbrev) = std::env::var("LINUX_LIT_WORK") {
        let line_id: Option<i64> = std::env::var("LINUX_LIT_LINE_ID").ok()
            .and_then(|s| s.parse().ok());
        crate::logging::log(&format!(
            "STARTUP: concordance spawn work='{}' line_id={:?}", work_abbrev, line_id
        ));
        // Store line_id for display_work_at to resolve after buffer is built.
        // Use 0 as placeholder — display_work_at will override with the resolved buf_idx.
        (Some(work_abbrev), 0)
    } else {
        (config.last_work.clone(), config.last_line)
    };
    let dim_enabled = config.dim_enabled;
    let vocab_highlight_visible = config.vocab_highlight_visible;

    let state = Rc::new(RefCell::new(AppState {
        text_view,
        buffer,
        picker,
        current_work: None,
        current_line: 0,
        prev_highlight_line: std::cell::Cell::new(None),
        page_top_line: 0,
        dim_tag,
        cursor_line_tag,
        cursor_fade_tag,
        ab_dim_tag,
        page_turn_overlay: page_turn_overlay.clone(),
        bottom_clip,
        top_spacer,
        bottom_spacer,
        card_vbox,
        scrolled_window: scrolled,
        content_hbox: content_hbox.clone(),
        window: window.clone(),
        config,
        css_provider,
        theme,
        page_turn_anim: None,
        cursor_fade_anim: None,
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
        has_timestamp: Rc::new(RefCell::new(Vec::new())),
        is_chapter_line: Rc::new(RefCell::new(Vec::new())),
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
        pending_advance: None,
        pending_advance_ignore_bl: None,
        visual_selection: None,
        selection_tag,
        action_popup: None,
        action_popup_widget,
        keybinds_overlay,
        correction_overlay,
        gloss_original_text: None,
        vocab_words: std::collections::HashSet::new(),
        vocab_matches: Vec::new(),
        vocab_match_idx: None,
        vocab_tag,
        dim_enabled,
        vocab_highlight_visible,
        vocab_popup,
        vocab_popup_data: Vec::new(),
        vocab_popup_index: 0,
        vocab_popup_view: crate::ui::vocab_popup::VocabView::Definition,
        vocab_popup_auto: false,
        vocab_popup_fade_gen: Rc::new(Cell::new(0)),
        concordance_picker,
        concordance_state: None,
        concordance_word_picker,
        concordance_list_picker,
        concordance_bar,
        current_sentence_group: None,
        current_paragraph_start: None,
        sync_enabled: true,
        sync_icon,
        word_status_label,
        word_cycle_line: None,
        word_cycle_index: 0,
        word_status_timer: Rc::new(Cell::new(0)),
        word_bold_tag,
        word_bold_gen: Rc::new(Cell::new(0)),
        word_collect_words: Vec::new(),
        word_collect_ranges: Vec::new(),
        loading_work: Rc::new(Cell::new(false)),
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

    // Connect concordance picker search entry filter
    let state_for_concordance_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.concordance_picker.search_entry().connect_changed(move |entry| {
            let text = entry.text();
            state_for_concordance_filter.borrow().concordance_picker.populate_list(&text);
        });
    }

    // Connect concordance word picker search entry filter
    let state_for_conc_word_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.concordance_word_picker.entry().connect_changed(move |_| {
            state_for_conc_word_filter.borrow().concordance_word_picker.filter_changed();
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
                    // Check if this is a concordance spawn with a target line
                    let target_line_id: Option<i64> = std::env::var("LINUX_LIT_LINE_ID").ok()
                        .and_then(|s| s.parse().ok());
                    {
                        let mut s = state_clone.borrow_mut();
                        // Set cursor to MRU line before display_work so the
                        // single deferred scroll/clip pass uses the right position.
                        if target_line_id.is_none() {
                            s.config.work_positions.insert(
                                work.abbrev.clone(),
                                last_line,
                            );
                        }
                        if target_line_id.is_some() {
                            display_work_at(&mut s, work, target_line_id);
                        } else {
                            display_work(&mut s, work);
                        }
                    }
                    // Set up concordance state if this is a concordance spawn
                    if let Ok(conc_word) = std::env::var("LINUX_LIT_CONC_WORD") {
                        let s = state_clone.borrow();
                        let work_abbrev = s.current_work.as_ref().map(|w| w.abbrev.clone());
                        drop(s);
                        if let Some(abbrev) = work_abbrev {
                            let sc = Rc::clone(&state_clone);
                            let handle2 = handle.clone();
                            let word = conc_word.clone();
                            glib::spawn_future_local(async move {
                                let word_q = word.clone();
                                let abbrev_q = abbrev.clone();
                                let hits = handle2
                                    .spawn_blocking(move || {
                                        let conn = crate::db::queries::open_db()
                                            .expect("Failed to open lit.db");
                                        crate::db::concordance::find_word_occurrences(&conn, &word_q)
                                            .unwrap_or_default()
                                    })
                                    .await
                                    .unwrap_or_default();
                                // Filter to only this work's hits
                                let conc_hits: Vec<crate::concordance::ConcordanceHit> = hits
                                    .into_iter()
                                    .filter(|h| h.work_abbrev == abbrev_q)
                                    .map(|h| crate::concordance::ConcordanceHit {
                                        work_abbrev: h.work_abbrev,
                                        work_title: h.title,
                                        author: h.author,
                                        line_mapping_id: h.line_mapping_id,
                                        div1: h.div1,
                                        div2: h.div2,
                                        line_in_div: h.line_in_div,
                                        canonical_text: h.canonical_text,
                                        has_audio: h.has_audio,
                                    })
                                    .collect();
                                if !conc_hits.is_empty() {
                                    let conc_state = crate::concordance::ConcordanceState::new(
                                        word.clone(),
                                        conc_hits,
                                    );
                                    {
                                        let mut s = sc.borrow_mut();
                                        s.concordance_bar.update(
                                            &conc_state.status_label(),
                                            &conc_state.status_work(),
                                        );
                                        s.concordance_state = Some(conc_state);
                                    }
                                    // Jump to first hit — positions cursor, highlights, seeks MPV
                                    crate::input::navigation::concordance_jump_to_current(
                                        &sc, &handle2,
                                    );
                                    // Defer a centered scroll after layout settles
                                    let sc2 = Rc::clone(&sc);
                                    glib::timeout_add_local_once(
                                        std::time::Duration::from_millis(200),
                                        move || {
                                            let s = sc2.borrow();
                                            let adj = s.scrolled_window.vadjustment();
                                            let max_scroll = adj.upper() - adj.page_size();
                                            if max_scroll > 0.0 {
                                                let line_y = if let Some(iter) = s.buffer.iter_at_line(s.current_line as i32) {
                                                    let (y, _) = s.text_view.line_yrange(&iter);
                                                    y as f64
                                                } else {
                                                    0.0
                                                };
                                                let centered = (line_y - adj.page_size() * 0.5).max(0.0).min(max_scroll);
                                                adj.set_value(centered);
                                            }
                                        },
                                    );
                                }
                            });
                        }
                    }
                }
                Ok(Err(_)) | Err(_) => {
                    state_clone.borrow_mut().picker.show_prepare();
                    state_clone.borrow().picker.show_finish();
                }
            }
        });
    } else {
        state.borrow_mut().picker.show_prepare();
        state.borrow().picker.show_finish();
    }

    state
}

/// Tear down the current display state: cancel animations, remove snapshot
/// overlays, clear the buffer, and reset card opacity. Called before showing
/// the library picker so that display_work starts from a clean slate.
pub fn clear_display(state: &mut AppState) {
    state.loading_work.set(true);

    // Cancel in-flight animations (drop without skip to avoid stale callbacks)
    state.page_turn_anim = None;
    state.cursor_fade_anim = None;

    // Remove snapshot overlays left by page turn animations
    {
        let overlay = &state.page_turn_overlay;
        let card = &state.card_vbox;
        let mut to_remove = Vec::new();
        let mut child = overlay.first_child();
        while let Some(c) = child {
            let next = c.next_sibling();
            if &c != card.upcast_ref::<gtk4::Widget>() {
                to_remove.push(c);
            }
            child = next;
        }
        for c in to_remove {
            overlay.remove_overlay(&c);
        }
    }

    // Restore card visibility and clear the buffer so GTK drops all layout
    // state. display_work will rebuild from scratch.
    state.card_vbox.set_opacity(1.0);
    state.buffer.set_text("");
}

pub fn display_work(state: &mut AppState, work: Work) {
    display_work_at(state, work, None);
}

/// Load and display a work, optionally overriding the saved cursor position.
/// `target_line_id` is a line_mapping_id to position the cursor on after load.
pub fn display_work_at(state: &mut AppState, work: Work, target_line_id: Option<i64>) {
    state.loading_work.set(true);

    // Place a solid-color mask over the card to hide layout/scroll churn.
    // The mask will be crossfaded out after the scroll and clip settle.
    let mask = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    mask.set_vexpand(true);
    mask.set_hexpand(true);
    mask.add_css_class("load-mask");
    let mask_css = gtk4::CssProvider::new();
    mask_css.load_from_string(&format!(
        ".load-mask {{ background-color: {}; border-radius: 12px; }}",
        state.theme.text_bg
    ));
    gtk4::style_context_add_provider_for_display(
        &gtk4::prelude::WidgetExt::display(&state.window),
        &mask_css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    state.page_turn_overlay.add_overlay(&mask);

    // Save position of the outgoing work before switching
    if let Some(ref old_work) = state.current_work {
        state.config.work_positions.insert(old_work.abbrev.clone(), state.current_line);
    }

    crate::input::search::clear_search(state);
    state.search_bar.hide();
    state.current_time_pos = 0.0;
    state.media_id = work.media_id;
    state
        .window
        .set_title(Some(&format!("{} — linux-lit", work.title)));

    // Save MRU to config
    let saved_line = state.config.work_positions.get(&work.abbrev).copied().unwrap_or(0);
    state.config.last_work = Some(work.abbrev.clone());
    state.config.last_line = saved_line;
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
        // Build path→media_id lookup for matching discovered socket to correct timestamps
        let path_to_mid: std::collections::HashMap<String, i64> = work
            .media_paths
            .iter()
            .zip(work.media_ids.iter())
            .map(|(p, id)| (p.clone(), *id))
            .collect();
        let timestamps = work.timestamps.clone();
        let lines = work.lines.clone();
        let default_media_id = state.media_id;
        let cmd_tx = state.cmd_tx.clone();
        let handle = state.tokio_handle.clone();
        glib::spawn_future_local(async move {
            let (socket_path, matched_media_path) = handle
                .spawn_blocking(move || {
                    if let Some((sock, matched)) =
                        crate::mpv::discovery::find_socket_for_work(&media_paths)
                    {
                        return (sock.to_string_lossy().to_string(), Some(matched));
                    }
                    let launched = crate::mpv::discovery::launch_mpv(&media_paths[0]);
                    for _ in 0..60 {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        if std::path::Path::new(&launched).exists() {
                            return (launched, Some(media_paths[0].clone()));
                        }
                    }
                    (launched, Some(media_paths[0].clone()))
                })
                .await
                .unwrap_or_default();

            // If the discovered socket matches a different media file, re-send timestamps
            if let Some(ref matched_path) = matched_media_path {
                let matched_mid = path_to_mid.get(matched_path).copied();
                if matched_mid.is_some() && matched_mid != default_media_id {
                    let mid = matched_mid.unwrap();
                    crate::logging::log(&format!(
                        "MPV discovery: switching active media_id from {:?} to {} for {}",
                        default_media_id, mid, matched_path
                    ));
                    let mut ts_data: Vec<(i64, f64, f64)> = timestamps
                        .iter()
                        .filter(|t| t.media_id == mid)
                        .map(|t| (t.line_id, t.start, t.end))
                        .collect();
                    ts_data.sort_by(|a, b| {
                        a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                    });
                    let mut id_to_idx: std::collections::HashMap<i64, usize> =
                        std::collections::HashMap::new();
                    for (i, line) in lines.iter().enumerate() {
                        id_to_idx.insert(line.id, i);
                    }
                    let _ = cmd_tx
                        .send(crate::mpv::MpvCommand::SetTimestamps {
                            timestamps: ts_data,
                            line_id_to_index: id_to_idx,
                        })
                        .await;
                }
            }

            if !socket_path.is_empty() {
                let _ = cmd_tx
                    .send(crate::mpv::MpvCommand::Connect(socket_path))
                    .await;
            }
        });
    }

    state.current_line = saved_line;
    state.page_top_line = 0;
    state.visual_selection = None;
    state.current_work = Some(work);

    // Build buffer text (with or without sign column)
    state.line_map = None;
    state.dialogue_formatting_active = false;
    // Poetry/plays get a larger left margin for visual indentation
    let work_type = state.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or("");
    let left_margin = if crate::db::line_types::is_prose_work(work_type) {
        state.config.text_margins as i32
    } else {
        state.config.text_margins as i32 + 120
    };
    state.text_view.set_left_margin(left_margin);
    state.translations_visible = false;
    state.translation_lines = Vec::new();
    // Load translations for this work
    if let Some(ref work) = state.current_work {
        if let Ok(conn) = crate::db::queries::open_db() {
            state.translations = crate::db::queries::load_translations(&conn, &work.abbrev)
                .unwrap_or_default();
            crate::logging::log(&format!(
                "TRANSLATIONS: loaded {} translations for {}",
                state.translations.len(),
                work.abbrev,
            ));
        }
    }
    let t0 = std::time::Instant::now();
    rebuild_buffer_text(state);
    crate::logging::log(&format!("TIMING: rebuild_buffer_text {:.0}ms", t0.elapsed().as_millis()));
    let t1 = std::time::Instant::now();
    apply_dialogue_formatting(state);
    crate::logging::log(&format!("TIMING: apply_dialogue_formatting {:.0}ms", t1.elapsed().as_millis()));

    // Load vocab words and apply highlighting
    let t2 = std::time::Instant::now();
    if let Some(ref work) = state.current_work {
        if let Ok(conn) = crate::db::queries::open_db() {
            state.vocab_words = crate::db::queries::load_vocab_words(&conn, &work.abbrev)
                .unwrap_or_default();
            crate::logging::log(&format!(
                "VOCAB: loaded {} vocab words",
                state.vocab_words.len(),
            ));
        }
    }
    crate::logging::log(&format!("TIMING: load_vocab_words {:.0}ms", t2.elapsed().as_millis()));
    let t3 = std::time::Instant::now();
    build_vocab_matches(state);
    crate::logging::log(&format!("TIMING: build_vocab_matches {:.0}ms", t3.elapsed().as_millis()));
    if state.vocab_highlight_visible {
        let t4 = std::time::Instant::now();
        apply_vocab_highlighting(state);
        crate::logging::log(&format!("TIMING: apply_vocab_highlighting {:.0}ms", t4.elapsed().as_millis()));
    }

    // Remove old gutter renderers — they'll be recreated lazily on first
    // sign column toggle (`l` key) via setup_gutter().
    if let Some(old_renderer) = state.gutter_renderer.take() {
        crate::gutter::remove_gutter_renderer(&state.text_view, &old_renderer);
    }
    if let Some(old_renderer) = state.chunk_renderer.take() {
        crate::gutter::remove_gutter_renderer(&state.text_view, &old_renderer);
    }

    // Load chunk data (needed for AB repeat, not just gutter display)
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

    // Set font size based on work type: 18pt for plays/poetry, 19pt for prose
    let is_prose = state.current_work.as_ref()
        .map(|w| crate::db::line_types::is_prose_work(&w.work_type))
        .unwrap_or(true);
    state.config.font_size = if is_prose {
        crate::config::default_font_size()
    } else {
        18
    };

    // Apply font tag to new buffer content
    let t6 = std::time::Instant::now();
    reapply_font(state);
    crate::logging::log(&format!("TIMING: reapply_font {:.0}ms", t6.elapsed().as_millis()));

    // Clamp saved line to buffer bounds and restore cursor position
    state.current_line = state.current_line.min(
        state.effective_line_count().saturating_sub(1),
    );

    // If a concordance target was specified, resolve it to a buffer line
    if let Some(target_id) = target_line_id {
        if let Some(work) = &state.current_work {
            if let Some(work_idx) = work.lines.iter().position(|l| l.id == target_id) {
                let buf_idx = if let Some(ref lm) = state.line_map {
                    lm.work_to_buffer[work_idx]
                } else {
                    work_idx
                };
                state.current_line = buf_idx;
                state.page_top_line = buf_idx;
            }
        }
    }

    // Dim all lines except the current one; defer scroll to idle callback
    // so GTK has time to lay out the new buffer before we query geometry.
    let t7 = std::time::Instant::now();
    crate::input::navigation::update_highlight_deferred_scroll(state, mask);
    crate::logging::log(&format!("TIMING: update_highlight {:.0}ms", t7.elapsed().as_millis()));
    crate::logging::log(&format!("TIMING: display_work total {:.0}ms", t0.elapsed().as_millis()));
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
                // Strip blank lines that immediately precede speaker lines —
                // the speaker-gap tag provides the visual spacing instead.
                let filtered_lines: Vec<&str> = {
                    let mut result: Vec<&str> = Vec::with_capacity(file_lines.len());
                    for (i, line) in file_lines.iter().enumerate() {
                        if crate::db::line_types::is_blank(line) {
                            // Look ahead: skip this blank if the next non-blank line is a speaker
                            let next_non_blank = file_lines[i + 1..]
                                .iter()
                                .find(|l| !crate::db::line_types::is_blank(l));
                            if let Some(next) = next_non_blank {
                                if crate::db::line_types::is_speaker(next) {
                                    continue;
                                }
                            }
                        }
                        result.push(line);
                    }
                    result
                };
                let filtered_contents = filtered_lines.join("\n");
                let filtered_file_lines: Vec<String> = filtered_lines.iter().map(|s| s.to_string()).collect();
                let is_prose = crate::db::line_types::is_prose_work(&work.work_type);
                let line_map = crate::text_file_map::build_line_map(&filtered_file_lines, &work.lines, is_prose);
                state.buffer.set_text(&filtered_contents);
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

    // Set global spacing to 0 for dialogue formatting
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

    // Widen left margin for stage plays so speaker names aren't flush with card edge
    let play_left_margin = state.text_view.left_margin() + 80;
    state.text_view.set_left_margin(play_left_margin);

    // Create tags
    let base_margin = play_left_margin;
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
    // Lazily set up gutter renderers on first toggle
    if state.gutter_renderer.is_none() {
        setup_gutter(state);
    }

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

/// Set up gutter renderers (timestamp signs and chunk bars).
/// Called lazily on first sign column toggle rather than at work load time.
fn setup_gutter(state: &mut AppState) {
    if let Some(old_renderer) = state.gutter_renderer.take() {
        crate::gutter::remove_gutter_renderer(&state.text_view, &old_renderer);
    }
    {
        let new_has_ts: Vec<bool> = if let Some(ref lm) = state.line_map {
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
        *state.has_timestamp.borrow_mut() = new_has_ts;

        let new_is_ch: Vec<bool> = if let Some(ref lm) = state.line_map {
            lm.buffer_to_work
                .iter()
                .map(|opt_idx| {
                    opt_idx
                        .and_then(|idx| Some(state.current_work.as_ref()?.lines.get(idx)?.is_chapter))
                        .unwrap_or(false)
                })
                .collect()
        } else {
            state
                .current_work
                .as_ref()
                .map(|w| w.lines.iter().map(|l| l.is_chapter).collect())
                .unwrap_or_default()
        };
        *state.is_chapter_line.borrow_mut() = new_is_ch;
    }
    let left_margin = state.text_view.left_margin();
    let gutter_width = (left_margin - 20).max(10);
    let renderer = crate::gutter::setup_timestamp_gutter(
        &state.text_view,
        state.sign_column_visible.clone(),
        state.has_timestamp.clone(),
        state.is_chapter_line.clone(),
        state.ab_a_line.clone(),
        state.ab_b_line.clone(),
        left_margin,
        &state.theme.root_color,
    );
    // Reduce left margin so the gutter absorbs the space instead of pushing text
    state.text_view.set_left_margin(left_margin - gutter_width);
    // Also adjust dialogue-indent tag so dialogue lines don't shift right
    if let Some(buffer) = state.text_view.buffer().downcast_ref::<gtk4::TextBuffer>() {
        if let Some(tag) = buffer.tag_table().lookup("dialogue-indent") {
            let old_margin = tag.left_margin();
            tag.set_left_margin(old_margin - gutter_width);
        }
    }
    state.gutter_renderer = Some(renderer);

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
    crate::logging::log("GUTTER: set up on demand");
}

/// Toggle translation lines below original text.
/// When showing: dims all lines, inserts translation text below matched lines.
/// When hiding: removes inserted lines and dim tag.
pub fn toggle_translations(state: &mut AppState) {
    if state.translations.is_empty() {
        crate::logging::log("TRANSLATIONS: no translations for this work");
        return;
    }

    if state.translations_visible {
        hide_translations(state);
    } else {
        show_translations(state);
    }
}

fn show_translations(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    // Build a list of (buffer_line, translation_text) pairs
    let mut inserts: Vec<(usize, String)> = Vec::new();
    let line_count = state.buffer.line_count() as usize;

    for buf_line in 0..line_count {
        let work_idx = state.work_line_for_buffer(buf_line);
        if let Some(wi) = work_idx {
            if let Some(line) = work.lines.get(wi) {
                if let Some(translation) = state.translations.get(&line.id) {
                    inserts.push((buf_line, translation.to_string()));
                }
            }
        }
    }

    // Insert bottom-to-top to avoid index shifting
    for (buf_line, text) in inserts.iter().rev() {
        let line_end = if let Some(mut iter) = state.buffer.iter_at_line(*buf_line as i32) {
            if !iter.ends_line() {
                iter.forward_to_line_end();
            }
            iter
        } else {
            continue;
        };
        state.buffer.insert(&mut line_end.clone(), &format!("\n    {}", text));
    }

    // Build translation_lines tracking vector
    let new_line_count = state.buffer.line_count() as usize;
    let mut tl = vec![false; new_line_count];

    let mut orig_idx = 0;
    let orig_line_count = line_count;
    let mut buf_idx = 0;
    let work_lines = &work.lines;
    while orig_idx < orig_line_count && buf_idx < new_line_count {
        tl[buf_idx] = false;
        let work_idx = if let Some(ref lm) = state.line_map {
            lm.buffer_to_work.get(orig_idx).copied().flatten()
        } else if orig_idx < work_lines.len() {
            Some(orig_idx)
        } else {
            None
        };
        let has_translation = work_idx
            .and_then(|wi| work_lines.get(wi))
            .and_then(|line| state.translations.get(&line.id))
            .is_some();
        buf_idx += 1;
        if has_translation && buf_idx < new_line_count {
            tl[buf_idx] = true;
            buf_idx += 1;
        }
        orig_idx += 1;
    }
    state.translation_lines = tl;

    // Configure the translation tag with current font (italic, 2pt smaller)
    let trans_size = state.config.font_size.saturating_sub(2);
    let desc = pango::FontDescription::from_string(
        &format!("{} Italic {}", state.config.font_family, trans_size),
    );
    state.translation_text_tag.set_font_desc(Some(&desc));

    // Apply translation-text tag to translation lines
    for (i, is_trans) in state.translation_lines.iter().enumerate() {
        if *is_trans {
            if let Some(line_start) = state.buffer.iter_at_line(i as i32) {
                let mut line_end = line_start;
                if !line_end.ends_line() {
                    line_end.forward_to_line_end();
                }
                state.buffer.apply_tag(&state.translation_text_tag, &line_start, &line_end);
            }
        }
    }

    // Ensure translation tag overrides the font-size tag
    let highest = state.buffer.tag_table().size() - 1;
    state.translation_text_tag.set_priority(highest);

    // Adjust current_line and page_top_line to account for inserted lines
    state.current_line = map_line_after_insert(state.current_line, &inserts);
    state.page_top_line = map_line_after_insert(state.page_top_line, &inserts);

    state.translations_visible = true;

    reapply_font(state);
    crate::input::navigation::update_highlight_and_ensure_visible(state);

    crate::logging::log(&format!(
        "TRANSLATIONS: shown ({} translation lines inserted)",
        inserts.len(),
    ));
}

/// Map an original buffer line index to its new position after translation inserts.
fn map_line_after_insert(orig_line: usize, inserts: &[(usize, String)]) -> usize {
    let mut offset = 0;
    for (buf_line, _) in inserts {
        if *buf_line < orig_line {
            offset += 1;
        } else {
            break;
        }
    }
    orig_line + offset
}

fn hide_translations(state: &mut AppState) {
    // Remove translation lines from buffer bottom-to-top
    let line_count = state.buffer.line_count() as usize;
    for i in (0..line_count).rev() {
        if i < state.translation_lines.len() && state.translation_lines[i] {
            let line_start = if i > 0 {
                if let Some(mut iter) = state.buffer.iter_at_line((i - 1) as i32) {
                    if !iter.ends_line() {
                        iter.forward_to_line_end();
                    }
                    iter
                } else {
                    continue;
                }
            } else {
                state.buffer.start_iter()
            };
            let line_end = if let Some(mut iter) = state.buffer.iter_at_line(i as i32) {
                if !iter.ends_line() {
                    iter.forward_to_line_end();
                }
                iter
            } else {
                continue;
            };
            state.buffer.delete(&mut line_start.clone(), &mut line_end.clone());
        }
    }

    // Remove translation tag from entire buffer
    let (buf_start, buf_end) = state.buffer.bounds();
    state.buffer.remove_tag(&state.translation_text_tag, &buf_start, &buf_end);

    // Reverse-map current_line and page_top_line
    let old_current = state.current_line;
    let old_top = state.page_top_line;
    state.current_line = map_line_before_insert(old_current, &state.translation_lines);
    state.page_top_line = map_line_before_insert(old_top, &state.translation_lines);

    state.translation_lines.clear();
    state.translations_visible = false;

    reapply_font(state);
    crate::input::navigation::update_highlight_and_ensure_visible(state);

    crate::logging::log("TRANSLATIONS: hidden");
}

/// Map a buffer line index (with translations) back to the original line index.
fn map_line_before_insert(buf_line: usize, translation_lines: &[bool]) -> usize {
    let mut orig = 0;
    for i in 0..=buf_line.min(translation_lines.len().saturating_sub(1)) {
        if i < translation_lines.len() && translation_lines[i] {
            // Skip translation lines
        } else if i == buf_line {
            return orig;
        } else {
            orig += 1;
        }
    }
    orig
}

/// Measure the pixel height of a representative buffer line.
fn measure_line_height(text_view: &sourceview5::View) -> i32 {
    let buf = text_view.buffer();
    let iter = buf.iter_at_line(0).unwrap_or_else(|| buf.start_iter());
    let (_y, h) = text_view.line_yrange(&iter);
    h.max(1)
}

/// Reapply font size using a TextTag spanning the entire buffer.

/// Update top/bottom spacer heights to match one line of text.
fn update_spacer_heights(state: &AppState) {
    let line_height = measure_line_height(&state.text_view);
    state.top_spacer.set_height_request(line_height);
    state.bottom_spacer.set_height_request(line_height);
}

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
    // Keep translation tag in sync (italic, 2pt smaller) and ensure it
    // overrides the freshly re-added font-size tag.
    let trans_size = state.config.font_size.saturating_sub(2);
    let trans_desc = pango::FontDescription::from_string(
        &format!("{} Italic {}", state.config.font_family, trans_size),
    );
    state.translation_text_tag.set_font_desc(Some(&trans_desc));
    let highest = state.buffer.tag_table().size() - 1;
    state.translation_text_tag.set_priority(highest);
    crate::logging::log(&format!("FONT: reapply_font size={}pt via TextTag", state.config.font_size));
    update_spacer_heights(state);
}

/// Adjust font size by delta, clamp to 8..=72, reapply CSS and repaginate.
pub fn adjust_font_size(state: &mut AppState, delta: i32) {
    let new_size = (state.config.font_size as i32 + delta).clamp(8, 72) as u32;
    if new_size == state.config.font_size {
        return;
    }
    state.config.font_size = new_size;
    reapply_font(state);
    crate::input::navigation::resnap_page(state);
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
    crate::input::navigation::resnap_page(state);
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
    crate::input::navigation::resnap_page(state);
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

    // Don't dim lines when navigating chunks — the chunk gutter bar
    // already indicates the active range.
    if state.ab_repeat.chunk_index.is_some() {
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
        let abbrev = work.abbrev.clone();
        state.config.last_work = Some(abbrev.clone());
        state.config.last_line = state.current_line;
        state.config.work_positions.insert(abbrev, state.current_line);
        crate::config::save(&state.config);
    }
}

/// Tokenize buffer lines and find vocab word matches.
fn build_vocab_matches(state: &mut AppState) {
    state.vocab_matches.clear();
    state.vocab_match_idx = None;

    if state.vocab_words.is_empty() {
        return;
    }

    let line_count = state.effective_line_count();
    let buffer_text = state.buffer.text(
        &state.buffer.start_iter(),
        &state.buffer.end_iter(),
        false,
    );

    for (line_idx, line_text) in buffer_text.lines().enumerate() {
        if line_idx >= line_count {
            break;
        }
        let mut char_offset = 0usize;
        let mut in_word = false;
        let mut word_start = 0usize;
        let mut word_buf = String::new();

        for ch in line_text.chars() {
            let is_word_char = ch.is_alphanumeric() || ch == '\'' || ch == '\u{2019}';
            if is_word_char {
                if !in_word {
                    word_start = char_offset;
                    word_buf.clear();
                    in_word = true;
                }
                word_buf.push(ch);
            } else if in_word {
                let lower = word_buf.to_lowercase();
                if state.vocab_words.contains(&lower) {
                    state.vocab_matches.push(VocabMatch {
                        word: lower,
                        line_index: line_idx,
                        char_start: word_start,
                        char_end: char_offset,
                    });
                }
                in_word = false;
            }
            char_offset += 1;
        }
        if in_word {
            let lower = word_buf.to_lowercase();
            if state.vocab_words.contains(&lower) {
                state.vocab_matches.push(VocabMatch {
                    word: lower,
                    line_index: line_idx,
                    char_start: word_start,
                    char_end: char_offset,
                });
            }
        }
    }
}

/// Apply the vocab-word TextTag to all matches in the buffer.
pub fn apply_vocab_highlighting(state: &AppState) {
    for m in &state.vocab_matches {
        let mut line_iter = state.buffer.iter_at_line(m.line_index as i32);
        if let Some(ref mut iter) = line_iter {
            let mut start = iter.clone();
            start.forward_chars(m.char_start as i32);
            let mut end = iter.clone();
            end.forward_chars(m.char_end as i32);
            state.buffer.apply_tag(&state.vocab_tag, &start, &end);
        }
    }
}

/// Remove all vocab-word tags from the buffer.
pub fn remove_vocab_highlighting(state: &AppState) {
    let start = state.buffer.start_iter();
    let end = state.buffer.end_iter();
    state.buffer.remove_tag(&state.vocab_tag, &start, &end);
}

/// Load vocab data for all words on the current line into state, show popup with first word.
pub fn open_vocab_popup(state: &mut AppState) {
    use crate::ui::vocab_popup::{VocabWordData, VocabView};

    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };

    let work_abbrev = state.current_work.as_ref().map(|w| w.abbrev.clone());
    let citation = state.current_work.as_ref().and_then(|work| {
        let work_line = state.work_line_for_buffer(state.current_line)?;
        let line = work.lines.get(work_line)?;
        Some(line.citation.clone())
    });

    // Collect unique vocab words in the current paragraph
    let para_range = state.current_paragraph_range();
    crate::logging::log(&format!(
        "VOCAB POPUP: current_line={} paragraph={}..{}", state.current_line, para_range.start, para_range.end
    ));
    let mut seen = std::collections::HashSet::new();
    let words: Vec<String> = state
        .vocab_matches
        .iter()
        .filter(|m| para_range.contains(&m.line_index))
        .filter(|m| seen.insert(m.word.clone()))
        .map(|m| m.word.clone())
        .collect();

    if words.is_empty() {
        crate::logging::log("VOCAB POPUP: no vocab words in current paragraph");
        return;
    }
    crate::logging::log(&format!("VOCAB POPUP: {} words: {:?}", words.len(), words));

    state.vocab_popup_data = words
        .into_iter()
        .map(|w| {
            let definition = crate::db::queries::load_vocab_definition(&conn, &w)
                .map(|(d, _)| d);
            let etymology_markup = crate::db::queries::load_vocab_etymology(&conn, &w)
                .map(|e| format_etymology(&e, &state.theme.vocab_fg));
            let gloss = match (&work_abbrev, &citation) {
                (Some(abbrev), Some(cit)) => {
                    crate::db::queries::load_vocab_gloss(&conn, &w, abbrev, cit)
                }
                _ => None,
            };
            VocabWordData { word: w, definition, etymology_markup, gloss }
        })
        .collect();

    state.vocab_popup_index = 0;
    state.vocab_popup_view = VocabView::Definition;

    update_vocab_popup_margin(state);
    show_vocab_popup(state);
}

/// Set the vocab popup's left margin so it starts just right of the text card.
fn update_vocab_popup_margin(state: &AppState) {
    let window = state.text_view.root()
        .and_then(|r| r.downcast::<gtk4::Window>().ok());
    let window = match window {
        Some(w) => w,
        None => return,
    };
    let sw_right = gtk4::graphene::Point::new(
        state.scrolled_window.width() as f32,
        0.0,
    );
    if let Some(pt) = state.scrolled_window.compute_point(&window, &sw_right) {
        let margin = (pt.x() as i32 + 12).max(0);
        state.vocab_popup.set_margin_start(margin);
    }
}

/// Hide the vocab popup.
pub fn close_vocab_popup(state: &mut AppState) {
    state.vocab_popup.hide();
}

/// Render the current vocab popup entry.
pub fn show_vocab_popup(state: &AppState) {
    if state.vocab_popup_data.is_empty() {
        state.vocab_popup.hide();
        return;
    }
    let idx = state.vocab_popup_index;
    let total = state.vocab_popup_data.len();
    let work_abbrev = state.current_work.as_ref()
        .map(|w| w.abbrev.as_str())
        .unwrap_or("");
    state.vocab_popup.update(
        &state.vocab_popup_data[idx],
        idx,
        total,
        state.vocab_popup_view,
        work_abbrev,
    );
    state.vocab_popup.show();
}

/// Refresh the vocab popup for the current line during playback sync.
/// If the new line has vocab words, update the popup content and position.
/// If it has none, close the popup.
pub fn refresh_vocab_popup(state: &mut AppState) {
    if !state.vocab_popup.is_visible() {
        return;
    }

    use crate::ui::vocab_popup::{VocabWordData, VocabView};

    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };

    let work_abbrev = state.current_work.as_ref().map(|w| w.abbrev.clone());
    let citation = state.current_work.as_ref().and_then(|work| {
        let work_line = state.work_line_for_buffer(state.current_line)?;
        let line = work.lines.get(work_line)?;
        Some(line.citation.clone())
    });

    let para_range = state.current_paragraph_range();
    let mut seen = std::collections::HashSet::new();
    let words: Vec<String> = state
        .vocab_matches
        .iter()
        .filter(|m| para_range.contains(&m.line_index))
        .filter(|m| seen.insert(m.word.clone()))
        .map(|m| m.word.clone())
        .collect();

    if words.is_empty() {
        return;
    }

    state.vocab_popup_data = words
        .into_iter()
        .map(|w| {
            let definition = crate::db::queries::load_vocab_definition(&conn, &w)
                .map(|(d, _)| d);
            let etymology_markup = crate::db::queries::load_vocab_etymology(&conn, &w)
                .map(|e| format_etymology(&e, &state.theme.vocab_fg));
            let gloss = match (&work_abbrev, &citation) {
                (Some(abbrev), Some(cit)) => {
                    crate::db::queries::load_vocab_gloss(&conn, &w, abbrev, cit)
                }
                _ => None,
            };
            VocabWordData { word: w, definition, etymology_markup, gloss }
        })
        .collect();

    state.vocab_popup_index = 0;
    state.vocab_popup_view = VocabView::Definition;
    show_vocab_popup(state);
}

/// Cycle to the next vocab word in the popup.
pub fn vocab_popup_next(state: &mut AppState) {
    if state.vocab_popup_data.is_empty() {
        return;
    }
    state.vocab_popup_index = (state.vocab_popup_index + 1) % state.vocab_popup_data.len();
    show_vocab_popup(state);
}

pub fn vocab_popup_prev(state: &mut AppState) {
    if state.vocab_popup_data.is_empty() {
        return;
    }
    if state.vocab_popup_index == 0 {
        state.vocab_popup_index = state.vocab_popup_data.len() - 1;
    } else {
        state.vocab_popup_index -= 1;
    }
    show_vocab_popup(state);
}

/// Toggle between definition and gloss view.
pub fn vocab_popup_toggle_view(state: &mut AppState) {
    use crate::ui::vocab_popup::VocabView;
    state.vocab_popup_view = match state.vocab_popup_view {
        VocabView::Definition => VocabView::Gloss,
        VocabView::Gloss => VocabView::Definition,
    };
    show_vocab_popup(state);
}

/// Format a VocabEtymology into Pango markup.
fn format_etymology(e: &crate::db::queries::VocabEtymology, vocab_fg: &str) -> String {
    let mut parts = Vec::new();
    if let Some(ref prefix) = e.prefix {
        let gloss = e.prefix_gloss.as_deref().unwrap_or("");
        parts.push(format!(
            "<span foreground=\"{}\">{}</span> \"{}\"",
            vocab_fg,
            glib::markup_escape_text(prefix),
            glib::markup_escape_text(gloss)
        ));
    }
    if let Some(ref root) = e.root {
        let gloss = e.root_gloss.as_deref().unwrap_or("");
        if !parts.is_empty() {
            parts.push(" + ".to_string());
        }
        parts.push(format!(
            "<span foreground=\"{}\">{}</span> \"{}\"",
            vocab_fg,
            glib::markup_escape_text(root),
            glib::markup_escape_text(gloss)
        ));
    }
    if let Some(ref suffix) = e.suffix {
        let gloss = e.suffix_gloss.as_deref().unwrap_or("");
        if !parts.is_empty() {
            parts.push(" + ".to_string());
        }
        parts.push(format!(
            "<span foreground=\"{}\">{}</span> \"{}\"",
            vocab_fg,
            glib::markup_escape_text(suffix),
            glib::markup_escape_text(gloss)
        ));
    }
    parts.join("")
}
