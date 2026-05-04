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
use crate::ui::bookmark_picker::BookmarkPicker;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Reader,
    LibraryPicker,
    BookmarkPicker,
    MediaPicker,
    Settings,
    Search,
    GlossOverlay,
    GlossPrompt,
    GamepadOverlay,
    KeybindsOverlay,
    ConcordancePicker,
    ConcordanceWordPicker,
    ConcordanceListPicker,
    ActionPopup,
    Visual,
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
    pub card_vbox: gtk4::Box,
    pub scrolled_window: ScrolledWindow,
    pub content_hbox: gtk4::Box,
    pub vbox: gtk4::Box,
    pub window: ApplicationWindow,
    pub config: Config,
    pub css_provider: CssProvider,
    pub theme: crate::theme::Theme,
    /// Active page-turn animation (crossfade or slide). Stored so it can be
    /// cancelled via .skip() if a new page turn fires mid-flight.
    pub page_turn_anim: Option<adw::TimedAnimation>,
    /// Re-entrancy lock for animated page turns. `set_page` consults this to
    /// drop racing second turns (e.g. MPV CursorSync arriving mid-animation)
    /// instead of letting them compose with the in-flight turn. Cleared by the
    /// animation's connect_done callback. Wrapped in Rc so connect_done
    /// closures can clone-and-release without a &mut AppState borrow.
    /// Mirrors foliate-js Paginator.#locked.
    pub page_turn_lock: std::rc::Rc<crate::input::navigation::PageTurnLock>,
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
    pub is_manual: Rc<RefCell<Vec<bool>>>,
    pub is_chapter_line: Rc<RefCell<Vec<bool>>>,
    pub is_bookmarked: Rc<RefCell<Vec<bool>>>,
    pub gutter_renderer: Option<sourceview5::GutterRendererText>,
    /// Logical left_margin at the moment the gutter was installed, used to
    /// detect when monocle↔tiled transitions require rebuilding the gutter.
    pub gutter_logical_left: Cell<i32>,
    pub chunk_renderer: Option<sourceview5::GutterRendererText>,
    pub line_number_renderer: Option<sourceview5::GutterRendererText>,
    pub line_numbers: Rc<RefCell<Vec<Option<i64>>>>,
    pub ab_repeat: crate::ab_repeat::AbRepeatState,
    pub ab_a_line: Rc<Cell<Option<usize>>>,
    pub ab_b_line: Rc<Cell<Option<usize>>>,
    pub line_map: Option<crate::text_file_map::LineMap>,
    pub settings_overlay: crate::ui::settings_overlay::SettingsOverlay,
    pub media_picker: MediaPicker,
    pub bookmark_picker: BookmarkPicker,
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
    pub gamepad_overlay: crate::ui::gamepad_overlay::GamepadOverlay,
    pub gloss_overlay: crate::ui::gloss_overlay::GlossOverlay,
    pub gloss_original_text: Option<String>,
    pub gloss_list: Vec<crate::db::queries::SavedGloss>,
    pub gloss_index: usize,
    pub gloss_context: Option<crate::gloss::GlossContext>,
    pub gloss_passages: Vec<crate::db::queries::GlossedPassage>,
    pub gloss_passage_index: usize,
    pub gloss_prompt_container: Option<glib::WeakRef<gtk4::Box>>,
    pub gloss_prompt_overlay: Option<glib::WeakRef<gtk4::Overlay>>,
    pub gloss_prompt_textview: Option<glib::WeakRef<gtk4::TextView>>,
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
    pub vocab_popup_line: Option<usize>,
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
    pub debug_icon: gtk4::Label,
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
    /// Set when loading_work clears so the resize tick can run a deferred
    /// layout refresh (apply_tiled_mode + snap) with correct line metrics.
    pub needs_layout_refresh: Rc<Cell<bool>>,
    pub timestamp_undo: Option<crate::input::timestamps::TimestampUndoState>,
    /// Cached last visible range from the most recent snap_scroll_to_line or
    /// update_bottom_clip. None during cold start, after work load, or after
    /// any after_page_change for a reason that shifts the viewport. Read by
    /// is_line_fully_visible to avoid recomputing through the height-summing
    /// loop on every MPV time-pos tick.
    /// Mirrors foliate-js Paginator.#lastVisibleRange.
    pub last_visible_range: std::cell::Cell<Option<crate::input::navigation::VisibleRange>>,
    /// Cached vec of viewport-page top line indices for the current work at
    /// the current font/size. Built lazily on first need by ensure_page_tops;
    /// invalidated to None when font/size changes or a new work loads. The
    /// cache eliminates the O(line_count²) replay-from-line-0 walk that
    /// viewport_page_for_line used to do on every overlay-label refresh.
    pub page_tops: std::cell::RefCell<Option<Vec<usize>>>,
    /// Loaded keybinds. Compiled-in defaults overridden by
    /// ~/.config/linux-lit/keymap.json if present.
    pub keymap: crate::input::keymap_config::Keymap,
    pub input_mode: InputMode,
}

impl AppState {
    /// Whether the current work is prose (true) or play/poetry (false).
    /// Returns true when no work is loaded — equivalent to pre-F9 behavior
    /// (trim_visible_range becomes a no-op on an empty buffer regardless).
    pub fn is_prose(&self) -> bool {
        self.current_work.as_ref()
            .map(|w| crate::db::line_types::is_prose_work(&w.work_type))
            .unwrap_or(true)
    }

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

    /// Get line_mapping.id for a buffer line, if available.
    pub fn line_mapping_id_for_buffer(&self, buffer_line: usize) -> Option<i64> {
        let work_idx = self.work_line_for_buffer(buffer_line)?;
        self.current_work.as_ref()?.lines.get(work_idx).map(|l| l.id)
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

/// Fit the centered text card to the current window width.
///
/// - Wide windows: full `column_width` with 24px outer margins (unchanged).
/// - Narrow windows: shrink outer margins first (24 → 0), then shrink the
///   card width itself. Font size is never changed — text reflows instead.
/// Additional left-margin offset for verse works when the window is wide
/// enough to absorb it visually (i.e. monocle / untiled layouts).
///
/// When the card has ≥240px of total slack around it, the +120 offset produces
/// the classic indented-verse look. When the card nearly fills the window
/// (tiled layouts), the offset is dropped so the text stays symmetric inside
/// the card and isn't pushed off-center.
pub const VERSE_LEFT_OFFSET: i32 = 200;
pub const PROSE_LEFT_OFFSET: i32 = 120;

/// Fixed height for the top spacer above the first text line.
pub const TOP_SPACER_HEIGHT: i32 = 40;
pub fn verse_left_offset(window_width: i32, column_width: u32) -> i32 {
    let card_w = (column_width as i32).min(window_width.max(1));
    let slack = window_width - card_w;
    if slack >= 2 * VERSE_LEFT_OFFSET { VERSE_LEFT_OFFSET } else { 0 }
}

/// True when the window is narrow enough that the text card nearly fills
/// it — used to trigger tiled-mode visual adjustments.
pub fn is_tiled_layout(window_width: i32, column_width: u32) -> bool {
    let card_w = (column_width as i32).min(window_width.max(1));
    (window_width - card_w) < 2 * VERSE_LEFT_OFFSET
}

/// Apply tiled-vs-monocle visual state: verse left offset and root-color
/// wallpaper masking via the `tiled` CSS class.
/// Called from both the resize tick and load_work so the initial render
/// picks up the right state before the first resize notification.
pub fn apply_tiled_mode(state: &mut AppState, root_box: &gtk4::Box, window_width: i32) {
    let cw = state.config.column_width;
    let tiled = is_tiled_layout(window_width, cw);

    // Root-color masking: paint the vbox with the card bg so no wallpaper
    // shows through when the card fills the tile.
    if tiled {
        root_box.add_css_class("tiled");
    } else {
        root_box.remove_css_class("tiled");
    }

    // Compute and apply the text_view left margin first. Verse works get
    // the +120 offset only when untiled — that means in tile mode the text
    // column starts at text_margins (e.g. 48) while in monocle it sits at
    // text_margins + 120 (e.g. 168). Page-label positioning depends on this
    // value, so derive it up-front.
    let work_type = state.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or("").to_string();
    let is_verse = !crate::db::line_types::is_prose_work(&work_type);
    let left_bump = if tiled {
        0
    } else if is_verse {
        VERSE_LEFT_OFFSET
    } else {
        PROSE_LEFT_OFFSET
    };
    let logical_left = state.config.text_margins as i32 + left_bump;
    let gutter_active = state.gutter_renderer.is_some();
    if gutter_active {
        // Gutter's baked-in width only matches its creation-time logical left.
        // If the layout changed, tear down and rebuild so the gutter fits the
        // new column geometry.
        if state.gutter_logical_left.get() != logical_left {
            if let Some(old) = state.gutter_renderer.take() {
                crate::gutter::remove_gutter_renderer(&state.text_view, &old);
            }
            state.text_view.set_left_margin(logical_left);
            if state.dialogue_formatting_active {
                apply_dialogue_formatting(state);
            }
            setup_gutter(state);
        }
    } else if state.text_view.left_margin() != logical_left {
        state.text_view.set_left_margin(logical_left);
        if state.dialogue_formatting_active {
            apply_dialogue_formatting(state);
        }
    }

    state.top_spacer.set_height_request(TOP_SPACER_HEIGHT);
}

pub fn apply_card_sizing(content_hbox: &gtk4::Box, window_width: i32, column_width: u32) {
    const MAX_OUTER_MARGIN: i32 = 24;
    let ww = window_width.max(0);
    let cw_cfg = column_width as i32;
    // Reserve room for margins first; if that overflows, the card itself shrinks.
    let card_w = cw_cfg.min(ww.max(1));
    let slack = ww - card_w;
    let margin = (slack / 2).clamp(0, MAX_OUTER_MARGIN);
    content_hbox.set_width_request(card_w);
    content_hbox.set_margin_start(margin);
    content_hbox.set_margin_end(margin);
    crate::log_fmt!(
        "CARD_SIZING: ww={} col_cfg={} card_w={} margin={}",
        ww, cw_cfg, card_w, margin
    );
}

pub fn build_window(
    app: &gtk4::Application,
    works: Vec<WorkSummary>,
    tokio_handle: tokio::runtime::Handle,
    config: Config,
    cmd_tx: tokio::sync::mpsc::Sender<crate::mpv::MpvCommand>,
) -> Rc<RefCell<AppState>> {
    let t_build = std::time::Instant::now();
    crate::logging::log("STARTUP: build_window enter");
    let window = ApplicationWindow::builder()
        .application(app)
        .title("linux-lit")
        .default_width(1000)
        .default_height(800)
        .build();
    window.connect_show(|_| crate::logging::log("STARTUP: window connect_show fired"));
    window.connect_map(|_| crate::logging::log("STARTUP: window connect_map fired"));

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
        .pixels_above_lines(0)
        .pixels_below_lines(0)
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
    text_view.set_bottom_margin(40);

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

    // scrolled carries the rounded bottom corners of the card. The top corners
    // are rounded by top_spacer; the middle (when the card is shown mid-stack)
    // renders as the same bg as scrolled.
    scrolled.add_css_class("card-bottom");

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
    bottom_clip.add_css_class("card-bottom");
    scrolled_overlay.add_overlay(&bottom_clip);

    // Top spacer — one line height, rounded top corners only
    let top_spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    top_spacer.set_hexpand(true);
    top_spacer.set_height_request(TOP_SPACER_HEIGHT);
    top_spacer.add_css_class("card-top");

    // Vertical card assembly: top spacer + scrolled area. No bottom spacer —
    // the scrolled area's card-bottom CSS provides the rounded bottom.
    let card_vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    card_vbox.set_vexpand(true);
    card_vbox.append(&top_spacer);
    card_vbox.append(&scrolled_overlay);

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

    // Bookmark picker overlay wraps the media picker overlay
    let bookmark_picker = BookmarkPicker::new();
    bookmark_picker.attach(&media_picker.overlay);
    bookmark_picker.overlay.set_vexpand(true);

    // Settings overlay wraps the bookmark picker overlay
    let all_themes = crate::theme::load_all_themes();
    let settings_overlay = crate::ui::settings_overlay::SettingsOverlay::new(
        all_themes,
        &theme.name,
    );

    settings_overlay.attach(&bookmark_picker.overlay);
    settings_overlay.overlay.set_vexpand(true);

    // Keybinds overlay wraps the settings overlay
    let keybinds_overlay = crate::ui::keybinds_overlay::KeybindsOverlay::new();
    keybinds_overlay.attach(&settings_overlay.overlay);
    keybinds_overlay.overlay.set_vexpand(true);

    // Gamepad overlay wraps the keybinds overlay
    let gamepad_overlay = crate::ui::gamepad_overlay::GamepadOverlay::new();
    gamepad_overlay.attach(&keybinds_overlay.overlay);
    gamepad_overlay.overlay.set_vexpand(true);

    // Correction overlay wraps the gamepad overlay
    let gloss_overlay = crate::ui::gloss_overlay::GlossOverlay::new(config.column_width, config.text_margins);
    gloss_overlay.attach(&gamepad_overlay.overlay);
    gloss_overlay.overlay.set_vexpand(true);

    // Concordance picker wraps the gloss overlay
    let concordance_picker = crate::ui::concordance_picker::ConcordancePicker::new();
    concordance_picker.attach(&gloss_overlay.overlay);
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

    // Debug-mode indicator (lower-left corner, next to sync icon, hidden by default)
    let debug_icon = gtk4::Label::new(Some("⚙"));
    debug_icon.set_valign(gtk4::Align::End);
    debug_icon.set_halign(gtk4::Align::Start);
    debug_icon.set_margin_start(44);
    debug_icon.set_margin_bottom(12);
    debug_icon.add_css_class("debug-icon");
    debug_icon.set_visible(false);
    concordance_list_picker.overlay.add_overlay(&debug_icon);
    // Flash the gear on launch if debug mode is already on.
    if crate::logging::debug_mode() {
        debug_icon.set_visible(true);
        let icon = debug_icon.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
            icon.set_visible(false);
        });
    }

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

    // Suppress startup flicker: hide content until the deferred layout
    // refresh fires (after dwl has tiled the window AND display_work
    // finishes loading). The tick callback below reveals it on the first
    // stable layout. Without this, users see content drawn at GTK's default
    // 1000×800 size, then jump to dwl's tiled size, then jump again as
    // display_work / bottom_clip recompute. Opacity-only so the window
    // chrome (including dwl's tile decoration) appears immediately.
    vbox.set_opacity(0.0);

    window.set_child(Some(&vbox));

    // Concordance spawns load the work specified by env var.
    // Normal startup resumes the most recently used work from config.
    let last_work = if let Ok(work_abbrev) = std::env::var("LINUX_LIT_WORK") {
        crate::logging::log(&format!(
            "STARTUP: concordance spawn work='{}'", work_abbrev
        ));
        Some(work_abbrev)
    } else {
        config.last_work.clone()
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
        card_vbox,
        scrolled_window: scrolled,
        content_hbox: content_hbox.clone(),
        vbox: vbox.clone(),
        window: window.clone(),
        config,
        css_provider,
        theme,
        page_turn_anim: None,
        page_turn_lock: std::rc::Rc::new(
            crate::input::navigation::PageTurnLock::new()
        ),
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
        is_manual: Rc::new(RefCell::new(Vec::new())),
        is_chapter_line: Rc::new(RefCell::new(Vec::new())),
        is_bookmarked: Rc::new(RefCell::new(Vec::new())),
        gutter_renderer: None,
        gutter_logical_left: Cell::new(0),
        chunk_renderer: None,
        line_number_renderer: None,
        line_numbers: Rc::new(RefCell::new(Vec::new())),
        ab_repeat: crate::ab_repeat::AbRepeatState::default(),
        ab_a_line: Rc::new(Cell::new(None)),
        ab_b_line: Rc::new(Cell::new(None)),
        line_map: None,
        settings_overlay,
        media_picker,
        bookmark_picker,
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
        gamepad_overlay,
        gloss_overlay,
        gloss_original_text: None,
        gloss_list: Vec::new(),
        gloss_index: 0,
        gloss_context: None,
        gloss_passages: Vec::new(),
        gloss_passage_index: 0,
        gloss_prompt_container: None,
        gloss_prompt_overlay: None,
        gloss_prompt_textview: None,
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
        vocab_popup_line: None,
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
        debug_icon,
        word_status_label,
        word_cycle_line: None,
        word_cycle_index: 0,
        word_status_timer: Rc::new(Cell::new(0)),
        word_bold_tag,
        word_bold_gen: Rc::new(Cell::new(0)),
        word_collect_words: Vec::new(),
        word_collect_ranges: Vec::new(),
        // If we have an MRU work to load, mark loading_work=true now so the
        // 500ms reveal grace doesn't fire before display_work runs and
        // expose an empty vbox. Cleared by update_highlight_and_show.
        loading_work: Rc::new(Cell::new(last_work.is_some())),
        needs_layout_refresh: Rc::new(Cell::new(false)),
        timestamp_undo: None,
        last_visible_range: std::cell::Cell::new(None),
        page_tops: std::cell::RefCell::new(None),
        keymap: crate::input::keymap_config::Keymap::load(),
        input_mode: InputMode::Reader,
    }));

    // Suppress startup flicker: vbox is hidden (opacity 0) until layout has
    // settled. Three reveal paths:
    //   1. Primary (load case): deferred-layout-refresh in the tick
    //      callback below reveals after display_work + sw_h>0 (the
    //      authoritative "page is settled" signal). Keeps the window
    //      invisible during the ~2-3s load.
    //   2. Picker case: after 500ms, if no work is loading, reveal so
    //      the picker shows.
    //   3. Stuck-load fallback: after 5s, reveal regardless. Guards
    //      against a hung work load leaving the window blank forever.
    {
        let vbox_for_reveal = vbox.clone();
        let state_for_reveal = Rc::clone(&state);
        glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
            if vbox_for_reveal.opacity() < 1.0 {
                let loading = state_for_reveal
                    .try_borrow()
                    .map(|s| s.loading_work.get())
                    .unwrap_or(true);
                if !loading {
                    crate::logging::log("STARTUP: revealing vbox (500ms grace, no work loading)");
                    vbox_for_reveal.set_opacity(1.0);
                } else {
                    crate::logging::log("STARTUP: 500ms grace skipped — work loading; waiting for deferred refresh");
                }
            }
        });
    }
    {
        let vbox_for_fallback = vbox.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(5), move || {
            if vbox_for_fallback.opacity() < 1.0 {
                crate::logging::log("STARTUP: revealing vbox (5s fallback — load may be stuck)");
                vbox_for_fallback.set_opacity(1.0);
            }
        });
    }

    // Adapt card width/margins to window size whenever the window resizes
    // (e.g. dwl switching between tiled and monocle layouts).
    //
    // GTK4 has no reliable "widget resized" signal, and on Wayland the window's
    // default-width property doesn't track compositor-driven resizes. Instead we
    // poll vbox.width() once per frame via a tick callback and re-apply card
    // sizing only when it changes meaningfully. A 4px threshold swallows the
    // 1-2px oscillation GTK produces as the layout re-settles after our own
    // width_request update — otherwise we'd resize on every frame forever.
    // After a real resize we also re-snap the current page so the bottom clip
    // overlay recomputes for the new viewport height.
    {
        let content_hbox_tick = content_hbox.clone();
        let state_for_tick = Rc::clone(&state);
        let vbox_for_tick = vbox.clone();
        let last_width: Rc<Cell<i32>> = Rc::new(Cell::new(-1));
        let last_height: Rc<Cell<i32>> = Rc::new(Cell::new(-1));
        window.add_tick_callback(move |_win, _clock| {
            let ww = vbox_for_tick.width();
            let prev_w = last_width.get();
            // 16px threshold swallows the post-startup width oscillation
            // (observed 10px jitter at ~3.5s after dwl settles + GTK
            // re-applies card width-request). Real resizes are ≥100s of px
            // (compositor tile changes), so 16 still catches them.
            // Exception: the very first allocation (prev_w == -1) always
            // counts so we initialize the layout.
            let width_changed = prev_w == -1 || (ww - prev_w).abs() >= 16;

            // Track text_view height so the bottom clip recomputes when the
            // compositor settles to a different window height (e.g. first open
            // before dwl applies the final tile geometry).
            let hh = if let Ok(s) = state_for_tick.try_borrow() {
                s.text_view.height()
            } else {
                return glib::ControlFlow::Continue;
            };
            let prev_h = last_height.get();
            let height_changed = hh > 0 && (prev_h == -1 || (hh - prev_h).abs() >= 16);

            // Check if a deferred layout refresh is needed after work loading.
            let layout_refresh = if let Ok(s) = state_for_tick.try_borrow() {
                s.needs_layout_refresh.get()
            } else {
                false
            };

            if !width_changed && !height_changed && !layout_refresh {
                return glib::ControlFlow::Continue;
            }

            if width_changed {
                crate::log_fmt!("RESIZE_TICK: vbox.width changed {} -> {}", prev_w, ww);
                last_width.set(ww);
            }
            if height_changed {
                crate::log_fmt!("RESIZE_TICK: text_view.height changed {} -> {}", prev_h, hh);
                last_height.set(hh);
            }

            if ww <= 100 {
                return glib::ControlFlow::Continue;
            }
            if let Ok(mut s) = state_for_tick.try_borrow_mut() {
                // Skip layout updates while a work is loading — the scrolled
                // window is hidden, so line_yrange returns inflated heights
                // that would corrupt spacer sizing and bottom clip.
                if s.loading_work.get() {
                    if width_changed {
                        let cw = s.config.column_width;
                        apply_card_sizing(&content_hbox_tick, ww, cw);
                    }
                    return glib::ControlFlow::Continue;
                }
                let mut do_reveal = false;
                if layout_refresh {
                    // After a work load, the scrolled window was just made
                    // visible.  Wait until it has a real allocated height so
                    // line_yrange returns accurate values.
                    let sw_h = s.scrolled_window.height();
                    if sw_h <= 0 {
                        crate::log_fmt!("RESIZE_TICK: layout refresh waiting, sw_h={}", sw_h);
                        return glib::ControlFlow::Continue;
                    }
                    crate::log_fmt!("RESIZE_TICK: deferred layout refresh, sw_h={}", sw_h);
                    s.needs_layout_refresh.set(false);
                    let cw = s.config.column_width;
                    apply_card_sizing(&content_hbox_tick, ww, cw);
                    apply_tiled_mode(&mut s, &vbox_for_tick, ww);
                    do_reveal = vbox_for_tick.opacity() < 1.0;
                } else if width_changed {
                    let cw = s.config.column_width;
                    apply_card_sizing(&content_hbox_tick, ww, cw);
                    apply_tiled_mode(&mut s, &vbox_for_tick, ww);
                }
                // Layout just changed (resize or post-load refresh) — page
                // boundaries shift, so any cached page_tops index is stale.
                // Drop it; snap_scroll_to_line below sets the label and the
                // build_page_tops walk gets amortized into the snap path.
                crate::input::navigation::invalidate_page_tops(&s);
                // Ensure the vadjustment's upper bound is large enough for
                // any line to be scrolled to the viewport top. Without this,
                // pages near the end of the document can't be reached because
                // GTK clamps set_value to upper - page_size.
                crate::input::scroll::ensure_scroll_range(&s);
                let top = s.page_top_line;
                crate::input::navigation::snap_scroll_to_line(&mut s, top);
                // Reveal LAST: apply_tiled_mode, snap_scroll, and the label
                // update inside snap can all shift visible geometry. Doing
                // them before opacity=1 keeps everything stable when the
                // user first sees the window.
                if do_reveal {
                    crate::log_fmt!("STARTUP: revealing vbox (sw_h={})", s.scrolled_window.height());
                    vbox_for_tick.set_opacity(1.0);
                }
            }
            glib::ControlFlow::Continue
        });
    }

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

    // Connect bookmark picker search entry filter
    let state_for_bookmark_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.bookmark_picker.search_entry().connect_changed(move |entry| {
            let text = entry.text();
            state_for_bookmark_filter
                .borrow()
                .bookmark_picker
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
            // Two-phase startup to minimize the empty-card freeze:
            //
            // Phase 1 (fast, ~50ms): load_work + read file + clean lines.
            //                        Show buffer text immediately so the
            //                        user sees content within <1s of
            //                        launch. Window stays loading_work=true
            //                        so input is gated.
            //
            // Phase 2 (slow, ~1000ms): build_line_map. Without line_map,
            //                          navigation and many display features
            //                          don't work, so this still has to
            //                          finish before clearing loading_work.
            //                          But it runs while the user is
            //                          already looking at content.
            // Two-phase startup with snapshot cache:
            //
            // Phase 1 (off-thread): load_work + try snapshot::read. On cache
            // hit, return WorkSnapshot. On miss, fall through to
            // prepare_text_only and the existing two-phase flow.
            //
            // The snapshot path skips phase 2 (build_line_map) entirely
            // because the LineMap was serialized at last save.
            let phase1 = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                    let work = crate::db::queries::load_work(&conn, &abbrev)?;
                    let t_read = std::time::Instant::now();
                    let result = if let Some(snap) = crate::snapshot::read(&work) {
                        let bytes = std::fs::metadata(crate::snapshot::cache_path(&work.abbrev))
                            .map(|m| m.len())
                            .unwrap_or(0);
                        crate::logging::log(&format!(
                            "SNAPSHOT: cache hit {} ({}ms, {} bytes)",
                            work.abbrev,
                            t_read.elapsed().as_millis(),
                            bytes
                        ));
                        SnapshotOrPrep::Snapshot(snap)
                    } else {
                        // read() already logged the miss reason if the file
                        // existed; if it didn't, log file_missing here.
                        if !crate::snapshot::cache_path(&work.abbrev).exists() {
                            crate::logging::log(&format!(
                                "SNAPSHOT: cache miss {} (file_missing)",
                                work.abbrev
                            ));
                        }
                        SnapshotOrPrep::Prep(prepare_text_only(&work))
                    };
                    Ok::<_, rusqlite::Error>((work, result))
                })
                .await;
            let (work, snapshot_or_prep) = match phase1 {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    crate::logging::log(&format!("STARTUP: load_work error: {}", e));
                    return;
                }
                Err(e) => {
                    crate::logging::log(&format!("STARTUP: spawn_blocking phase 1 join error: {}", e));
                    return;
                }
            };

            // Phase 1.5 (main thread): set buffer text + font from whatever
            // source we have (snapshot or prep). Same set_text call shape;
            // text appears at the same point in either case.
            let filtered_contents_for_phase1: Option<&str> = match &snapshot_or_prep {
                SnapshotOrPrep::Snapshot(snap) => Some(snap.filtered_contents.as_str()),
                SnapshotOrPrep::Prep(Some(prep)) => Some(prep.filtered_contents.as_str()),
                SnapshotOrPrep::Prep(None) => None,
            };
            if let Some(text) = filtered_contents_for_phase1 {
                let s = state_clone.borrow();
                s.buffer.set_text(text);
                drop(s);
                let s = state_clone.borrow();
                reapply_font(&s);
                drop(s);
                crate::logging::log("STARTUP: buffer.set_text + font from phase 1 (line_map status TBD)");
            }

            // Phase 2 (off-thread, cache miss only): build line_map from
            // the cleaned_lines we already have. Skipped on cache hit.
            let (prepared, was_cache_miss) = match snapshot_or_prep {
                SnapshotOrPrep::Snapshot(snap) => {
                    // Build a PreparedText directly from the snapshot.
                    let prep = PreparedText {
                        abbrev: snap.abbrev,
                        work_type: work.work_type.clone(),
                        file_lines_count: snap.filtered_contents.lines().count(),
                        cleaned_lines_count: snap.filtered_contents.lines().count(),
                        work_lines_count: work.lines.len(),
                        filtered_contents: snap.filtered_contents,
                        line_map: snap.line_map,
                        path: snap.text_file_path,
                        is_prose: crate::db::line_types::is_prose_work(&work.work_type),
                    };
                    (Some(prep), false)
                }
                SnapshotOrPrep::Prep(Some(text_only)) => {
                    let cleaned = text_only.cleaned_lines.clone();
                    let work_lines = work.lines.clone();
                    let is_prose = text_only.is_prose;
                    let line_map = handle
                        .spawn_blocking(move || {
                            let t_map = std::time::Instant::now();
                            let lm = crate::text_file_map::build_line_map(&cleaned, &work_lines, is_prose);
                            crate::logging::log(&format!(
                                "PREP: build_line_map (phase 2) {}ms",
                                t_map.elapsed().as_millis()
                            ));
                            lm
                        })
                        .await
                        .ok();
                    let prep = line_map.map(|lm| PreparedText {
                        abbrev: text_only.abbrev,
                        work_type: text_only.work_type,
                        file_lines_count: text_only.file_lines_count,
                        cleaned_lines_count: text_only.cleaned_lines_count,
                        work_lines_count: text_only.work_lines_count,
                        filtered_contents: text_only.filtered_contents,
                        line_map: lm,
                        path: text_only.path,
                        is_prose: text_only.is_prose,
                    });
                    (prep, true)
                }
                SnapshotOrPrep::Prep(None) => (None, true),
            };

            // Capture write inputs BEFORE display_work consumes prepared.
            let write_inputs = if was_cache_miss {
                prepared.as_ref().map(|p| (work.clone(), p.filtered_contents.clone(), p.line_map.clone()))
            } else {
                None
            };

            {
                // Check if this is a concordance spawn with a target line
                let target_line_id: Option<i64> = std::env::var("LINUX_LIT_LINE_ID").ok()
                    .and_then(|s| s.parse().ok());
                let mut s = state_clone.borrow_mut();
                display_work_at_with_prepared(&mut s, work, target_line_id, prepared);
            }

            // After display_work, if this was a cache miss AND we have
            // both filtered_contents and line_map (i.e., text_file path
            // was valid), write the snapshot for next launch.
            if let Some((w, filtered, line_map)) = write_inputs {
                handle.spawn_blocking(move || {
                    let _ = crate::snapshot::write(&w, &filtered, &line_map);
                });
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
                            crate::input::actions::concordance::concordance_jump_to_current(
                                &sc, &handle2,
                            );
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
        });
    } else {
        state.borrow_mut().picker.show_prepare();
        state.borrow().picker.show_finish();
    }

    crate::logging::log(&format!(
        "STARTUP: build_window exit ({}ms)",
        t_build.elapsed().as_millis()
    ));
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

    // Remove snapshot overlays left by page turn animations.
    {
        let overlay = &state.page_turn_overlay;
        let card: &gtk4::Widget = state.card_vbox.upcast_ref();
        let mut to_remove = Vec::new();
        let mut child = overlay.first_child();
        while let Some(c) = child {
            let next = c.next_sibling();
            if &c != card {
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
    display_work_at_with_prepared(state, work, None, None);
}

/// Load and display a work, optionally overriding the saved cursor position.
/// `target_line_id` is a line_mapping_id to position the cursor on after load.
pub fn display_work_at(state: &mut AppState, work: Work, target_line_id: Option<i64>) {
    display_work_at_with_prepared(state, work, target_line_id, None);
}

/// Like `display_work_at` but accepts a precomputed `PreparedText` to skip
/// the synchronous file-read + line-map-build step inside
/// `rebuild_buffer_text`. Caller is expected to have produced `prepared`
/// off-thread via `prepare_text_for_display(&work)` inside
/// `tokio::Handle::spawn_blocking`. If `prepared` is None, falls back to
/// the synchronous path.
pub fn display_work_at_with_prepared(
    state: &mut AppState,
    work: Work,
    target_line_id: Option<i64>,
    prepared: Option<PreparedText>,
) {
    static BOOKMARKS_INIT: std::sync::Once = std::sync::Once::new();
    BOOKMARKS_INIT.call_once(|| {
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            let _ = crate::db::queries::ensure_bookmarks_table(&conn);
        }
    });

    state.loading_work.set(true);

    // Hide the scrolled window to prevent any flash of content at the wrong
    // scroll position while we rebuild the buffer.
    state.scrolled_window.set_visible(false);

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
    state.last_visible_range.set(None);
    *state.page_tops.borrow_mut() = None;
    state.visual_selection = None;
    state.current_work = Some(work);

    // Build buffer text (with or without sign column)
    state.line_map = None;
    state.dialogue_formatting_active = false;
    // Left margin + tiled-mode visuals. apply_tiled_mode handles the verse
    // offset for wide windows, the page-label padding, and the root-color
    // masking CSS class for narrow/tiled windows.
    let work_type = state.current_work.as_ref().map(|w| w.work_type.clone()).unwrap_or_default();
    let vbox = state.vbox.clone();
    let ww = state.window.width();
    apply_tiled_mode(state, &vbox, ww);
    // Non-prose works (plays, poems, epics) use tight 0px global spacing.
    // Prose uses the configured line_spacing. Reset on every load so the
    // previous work's spacing never leaks through.
    let ls = if crate::db::line_types::is_prose_work(&work_type) {
        state.config.line_spacing as i32
    } else {
        0
    };
    state.text_view.set_pixels_above_lines(ls);
    state.text_view.set_pixels_below_lines(ls);
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
    if let Some(prep) = prepared {
        // Heavy work was done off-thread; just apply the result.
        let mapped = prep.line_map.buffer_to_work.iter().filter(|o| o.is_some()).count();
        let first_mapped = prep.line_map.buffer_to_work.iter().position(|o| o.is_some());
        state.buffer.set_text(&prep.filtered_contents);
        state.line_map = Some(prep.line_map);
        crate::logging::log(&format!(
            "TEXT_FILE: loaded '{}' work_type='{}' is_prose={} file_lines={} cleaned_lines={} work_lines={} mapped_buffer_lines={} first_mapped={:?} path={} (prepared off-thread)",
            prep.abbrev,
            prep.work_type,
            prep.is_prose,
            prep.file_lines_count,
            prep.cleaned_lines_count,
            prep.work_lines_count,
            mapped,
            first_mapped,
            prep.path
        ));
    } else {
        rebuild_buffer_text(state);
    }
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
    if let Some(old_renderer) = state.line_number_renderer.take() {
        crate::gutter::remove_line_number_renderer(&state.text_view, &old_renderer);
        let right_margin = state.config.text_margins as i32 + crate::config::EXTRA_RIGHT_MARGIN;
        state.text_view.set_right_margin(right_margin);
    }

    // Populate is_bookmarked eagerly so `'` / `"` bookmark navigation works
    // before the sign column has ever been toggled. setup_gutter() will
    // later overwrite this with identical data if the user toggles signs.
    {
        let bookmark_ids: std::collections::HashSet<i64> = {
            if let (Some(ref cw), Ok(conn)) = (state.current_work.as_ref(), crate::db::queries::open_db()) {
                crate::db::queries::load_bookmarks(&conn, &cw.abbrev)
                    .unwrap_or_default()
                    .into_iter()
                    .collect()
            } else {
                std::collections::HashSet::new()
            }
        };
        let new_is_bookmarked: Vec<bool> = if let Some(ref lm) = state.line_map {
            lm.buffer_to_work
                .iter()
                .map(|opt_idx| {
                    opt_idx
                        .and_then(|idx| Some(bookmark_ids.contains(&state.current_work.as_ref()?.lines.get(idx)?.id)))
                        .unwrap_or(false)
                })
                .collect()
        } else {
            state
                .current_work
                .as_ref()
                .map(|w| w.lines.iter().map(|l| bookmark_ids.contains(&l.id)).collect())
                .unwrap_or_default()
        };
        *state.is_bookmarked.borrow_mut() = new_is_bookmarked;
    }

    // Set up right-side line number gutter for plays/verse
    {
        let is_prose = state.current_work.as_ref()
            .map(|w| crate::db::line_types::is_prose_work(&w.work_type))
            .unwrap_or(true);
        if !is_prose {
            let new_line_numbers: Vec<Option<i64>> = if let Some(ref lm) = state.line_map {
                lm.buffer_to_work
                    .iter()
                    .map(|opt_idx| {
                        opt_idx.and_then(|idx| {
                            state.current_work.as_ref()?.lines.get(idx).map(|l| l.line_in_div)
                        })
                    })
                    .collect()
            } else {
                state.current_work.as_ref()
                    .map(|w| w.lines.iter().map(|l| Some(l.line_in_div)).collect())
                    .unwrap_or_default()
            };
            *state.line_numbers.borrow_mut() = new_line_numbers;
            let renderer = crate::gutter::setup_line_number_gutter(
                &state.text_view,
                state.line_numbers.clone(),
                &state.theme.dim_fg,
                &state.config.font_family,
                state.config.font_size,
            );
            state.text_view.set_right_margin(48);
            state.line_number_renderer = Some(renderer);
        }
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

    // Set font size based on work type: 18pt for plays/poetry, 20pt for prose
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

    // If no saved position and no concordance target, start at first
    // dialogue line with viewport showing the line above (usually a
    // speaker name). When current_line > 0 here it came from
    // config.work_positions — honor the user's saved place.
    if target_line_id.is_none() && state.current_line == 0 {
        let first_dialogue = if let Some(ref lm) = state.line_map {
            lm.dialogue_buffer_lines.first().copied()
        } else {
            state.current_work.as_ref().and_then(|w| {
                w.lines.iter().position(|l| l.is_dialogue)
            })
        };
        crate::logging::log(&format!(
            "DISPLAY_WORK: first_dialogue={:?} line_map={} dialogue_buf_lines={}",
            first_dialogue,
            state.line_map.is_some(),
            state.line_map.as_ref().map(|lm| lm.dialogue_buffer_lines.len()).unwrap_or(0)
        ));
        if let Some(target) = first_dialogue {
            state.current_line = target;
            state.page_top_line = target.saturating_sub(1);
        }
    } else if target_line_id.is_none() {
        // Saved position path: anchor page_top one line above cursor so
        // the line preceding the cursor (often a speaker label) is
        // visible. snap_scroll_to_line in update_highlight_and_show will
        // adjust as needed if this falls in the middle of a page.
        state.page_top_line = state.current_line.saturating_sub(1);
        crate::logging::log(&format!(
            "DISPLAY_WORK: resumed saved position current_line={} page_top={}",
            state.current_line, state.page_top_line
        ));
    }

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

    // Suppress CursorSync so MPV events from the previous playback position
    // don't override the initial cursor placement. The window must be long
    // enough to cover async MPV launch + connection + first time_pos event.
    // Don't shorten a longer suppression already set by seek_to_current_line.
    let load_suppress = std::time::Instant::now() + std::time::Duration::from_secs(5);
    if state.suppress_sync_until.map_or(true, |existing| load_suppress > existing) {
        state.suppress_sync_until = Some(load_suppress);
    }

    // Page label is set later by the resize tick once layout is valid.
    // Setting it here would compute a degenerate page=1 because the
    // scrolled_window is still hidden and text_view.height() is 0.

    // Apply highlight, snap scroll, show the scrolled window.
    let t7 = std::time::Instant::now();
    crate::input::navigation::update_highlight_and_show(state);
    crate::logging::log(&format!("TIMING: update_highlight {:.0}ms", t7.elapsed().as_millis()));
    crate::logging::log(&format!("TIMING: display_work total {:.0}ms", t0.elapsed().as_millis()));
}

/// Rebuild the buffer text from current_work.
/// If the work has a text_file and it exists, load from file and build a line map.
/// Otherwise, join work.lines as before.
/// First phase of preparing a work for display: file read + cleanup. Fast
/// (~50ms on Bleak House), produced off the GTK main thread. Lets us call
/// `state.buffer.set_text(filtered_contents)` and reveal the window
/// quickly without waiting for the slower line_map build.
#[derive(Clone)]
pub struct PreparedTextOnly {
    pub abbrev: String,
    pub work_type: String,
    pub file_lines_count: usize,
    pub cleaned_lines_count: usize,
    pub work_lines_count: usize,
    pub filtered_contents: String,
    pub cleaned_lines: Vec<String>,
    pub path: String,
    pub is_prose: bool,
}

/// Full prepared text including the line_map. Used by paths that want a
/// single-shot prep (no two-phase). Produced by
/// `prepare_text_for_display`.
#[derive(Clone)]
pub struct PreparedText {
    pub abbrev: String,
    pub work_type: String,
    pub file_lines_count: usize,
    pub cleaned_lines_count: usize,
    pub work_lines_count: usize,
    pub filtered_contents: String,
    pub line_map: crate::text_file_map::LineMap,
    pub path: String,
    pub is_prose: bool,
}

/// Result of spawn_blocking 1 in build_window's MRU path: either a fresh
/// PreparedTextOnly (cache miss, will require build_line_map in spawn_blocking 2)
/// or a fully-restored WorkSnapshot (cache hit, skip phase 2 entirely).
enum SnapshotOrPrep {
    Snapshot(crate::snapshot::WorkSnapshot),
    Prep(Option<PreparedTextOnly>),
}

/// Heavy precompute: read the work's text file from disk, clean it,
/// build the line map. Pure CPU + I/O — safe to run inside
/// `tokio::Handle::spawn_blocking`. The caller then calls
/// `display_work_with_prepared` on the GTK main thread to apply the
/// result via `state.buffer.set_text(...)`.
///
/// Returns None when the work has no text_file or the file read failed —
/// caller falls back to the default `display_work` path that joins
/// `work.lines` synchronously.
/// Phase 1: read file + clean. Cheap (~50ms on Bleak House). Off-thread
/// safe. Pair with `build_line_map_for_prepared` to get the full
/// `PreparedText`, or use directly via `display_work_text_only` to show
/// content immediately while the line_map builds in the background.
pub fn prepare_text_only(work: &Work) -> Option<PreparedTextOnly> {
    let path = work.text_file.as_ref()?;
    let t_read = std::time::Instant::now();
    let contents = std::fs::read_to_string(path).ok()?;
    let file_lines: Vec<String> = contents.lines().map(String::from).collect();
    crate::logging::log(&format!("PREP: read+split {}ms", t_read.elapsed().as_millis()));

    let t_clean = std::time::Instant::now();
    let cleaned_lines: Vec<String> = {
        let mut result: Vec<String> = Vec::with_capacity(file_lines.len());
        for (i, line) in file_lines.iter().enumerate() {
            if crate::db::line_types::is_blank(line) {
                let next_non_blank = file_lines[i + 1..]
                    .iter()
                    .find(|l| !crate::db::line_types::is_blank(l));
                if let Some(next) = next_non_blank {
                    if crate::db::line_types::is_speaker(next) {
                        continue;
                    }
                }
            }
            if let Some(stripped) = line.strip_prefix("## ") {
                result.push(stripped.to_string());
            } else {
                result.push(line.clone());
            }
        }
        result
    };
    crate::logging::log(&format!("PREP: clean {}ms ({} -> {} lines)", t_clean.elapsed().as_millis(), file_lines.len(), cleaned_lines.len()));

    let is_prose = crate::db::line_types::is_prose_work(&work.work_type);
    let t_join = std::time::Instant::now();
    let filtered_contents = cleaned_lines.join("\n");
    crate::logging::log(&format!("PREP: join {}ms", t_join.elapsed().as_millis()));

    Some(PreparedTextOnly {
        abbrev: work.abbrev.clone(),
        work_type: work.work_type.clone(),
        file_lines_count: file_lines.len(),
        cleaned_lines_count: cleaned_lines.len(),
        work_lines_count: work.lines.len(),
        filtered_contents,
        cleaned_lines,
        path: path.clone(),
        is_prose,
    })
}

/// Phase 2: build_line_map from already-cleaned lines. Slow (~1000ms on
/// Bleak House). Off-thread safe. Used after `prepare_text_only` +
/// `display_work_text_only` to complete navigation setup.
pub fn build_line_map_for_prepared(
    cleaned_lines: &[String],
    work_lines: &[crate::db::models::Line],
    is_prose: bool,
) -> crate::text_file_map::LineMap {
    let t_map = std::time::Instant::now();
    let line_map = crate::text_file_map::build_line_map(cleaned_lines, work_lines, is_prose);
    crate::logging::log(&format!("PREP: build_line_map {}ms", t_map.elapsed().as_millis()));
    line_map
}

pub fn prepare_text_for_display(work: &Work) -> Option<PreparedText> {
    let path = work.text_file.as_ref()?;
    let t_read = std::time::Instant::now();
    let contents = std::fs::read_to_string(path).ok()?;
    let file_lines: Vec<String> = contents.lines().map(String::from).collect();
    crate::logging::log(&format!("PREP: read+split {}ms", t_read.elapsed().as_millis()));

    let t_clean = std::time::Instant::now();
    let cleaned_lines: Vec<String> = {
        let mut result: Vec<String> = Vec::with_capacity(file_lines.len());
        for (i, line) in file_lines.iter().enumerate() {
            if crate::db::line_types::is_blank(line) {
                let next_non_blank = file_lines[i + 1..]
                    .iter()
                    .find(|l| !crate::db::line_types::is_blank(l));
                if let Some(next) = next_non_blank {
                    if crate::db::line_types::is_speaker(next) {
                        continue;
                    }
                }
            }
            if let Some(stripped) = line.strip_prefix("## ") {
                result.push(stripped.to_string());
            } else {
                result.push(line.clone());
            }
        }
        result
    };
    crate::logging::log(&format!("PREP: clean {}ms ({} -> {} lines)", t_clean.elapsed().as_millis(), file_lines.len(), cleaned_lines.len()));

    let is_prose = crate::db::line_types::is_prose_work(&work.work_type);
    let t_map = std::time::Instant::now();
    let line_map = crate::text_file_map::build_line_map(&cleaned_lines, &work.lines, is_prose);
    crate::logging::log(&format!("PREP: build_line_map {}ms", t_map.elapsed().as_millis()));

    let t_join = std::time::Instant::now();
    let filtered_contents = cleaned_lines.join("\n");
    crate::logging::log(&format!("PREP: join {}ms", t_join.elapsed().as_millis()));

    Some(PreparedText {
        abbrev: work.abbrev.clone(),
        work_type: work.work_type.clone(),
        file_lines_count: file_lines.len(),
        cleaned_lines_count: cleaned_lines.len(),
        work_lines_count: work.lines.len(),
        filtered_contents,
        line_map,
        path: path.clone(),
        is_prose,
    })
}

fn rebuild_buffer_text(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    if let Some(prep) = prepare_text_for_display(work) {
        let mapped = prep.line_map.buffer_to_work.iter().filter(|o| o.is_some()).count();
        let first_mapped = prep.line_map.buffer_to_work.iter().position(|o| o.is_some());
        state.buffer.set_text(&prep.filtered_contents);
        state.line_map = Some(prep.line_map);
        crate::logging::log(&format!(
            "TEXT_FILE: loaded '{}' work_type='{}' is_prose={} file_lines={} cleaned_lines={} work_lines={} mapped_buffer_lines={} first_mapped={:?} path={}",
            prep.abbrev,
            prep.work_type,
            prep.is_prose,
            prep.file_lines_count,
            prep.cleaned_lines_count,
            prep.work_lines_count,
            mapped,
            first_mapped,
            prep.path
        ));
        return;
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
pub fn apply_dialogue_formatting(state: &mut AppState) {
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
                   "speaker-name", "stage-direction-style", "act-scene-header",
                   "blank-line"] {
        if let Some(old) = tag_table.lookup(name) {
            tag_table.remove(&old);
        }
    }

    // Text column is already symmetrically inset by state.config.text_margins,
    // so speaker names sit at the same left edge as dialogue. Dialogue lines
    // get an additional indent via the per-tag margin below.
    let base_margin = state.text_view.left_margin();

    let indent_tag = gtk4::TextTag::builder()
        .name("dialogue-indent")
        .left_margin(base_margin + 60)
        .build();

    let speaker_gap_tag = gtk4::TextTag::builder()
        .name("speaker-gap")
        .pixels_above_lines(8)
        .build();

    let stage_gap_tag = gtk4::TextTag::builder()
        .name("stage-direction-gap")
        .pixels_above_lines(8)
        .build();

    let speaker_name_tag = gtk4::TextTag::builder()
        .name("speaker-name")
        .variant(pango::Variant::SmallCaps)
        .weight(400)
        .scale(0.75)
        .build();

    let stage_italic_tag = gtk4::TextTag::builder()
        .name("stage-direction-style")
        .style(pango::Style::Italic)
        .build();

    let act_scene_tag = gtk4::TextTag::builder()
        .name("act-scene-header")
        .weight(700)
        .pixels_above_lines(8)
        .build();

    let blank_line_tag = gtk4::TextTag::builder()
        .name("blank-line")
        .scale(0.25)
        .build();

    tag_table.add(&indent_tag);
    tag_table.add(&speaker_gap_tag);
    tag_table.add(&stage_gap_tag);
    tag_table.add(&speaker_name_tag);
    tag_table.add(&stage_italic_tag);
    tag_table.add(&act_scene_tag);
    tag_table.add(&blank_line_tag);

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
            state.buffer.apply_tag(&blank_line_tag, &line_start, &line_end);
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

        let new_is_manual: Vec<bool> = if let Some(ref lm) = state.line_map {
            lm.buffer_to_work
                .iter()
                .map(|opt_idx| {
                    opt_idx
                        .and_then(|idx| {
                            Some(state.current_work.as_ref()?.lines.get(idx)?.timestamp.as_ref()?.is_manual)
                        })
                        .unwrap_or(false)
                })
                .collect()
        } else {
            state
                .current_work
                .as_ref()
                .map(|w| {
                    w.lines
                        .iter()
                        .map(|l| l.timestamp.as_ref().map_or(false, |t| t.is_manual))
                        .collect()
                })
                .unwrap_or_default()
        };
        *state.is_manual.borrow_mut() = new_is_manual;

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

        // Populate bookmark flags
        let bookmark_ids: std::collections::HashSet<i64> = {
            if let (Some(ref cw), Ok(conn)) = (state.current_work.as_ref(), crate::db::queries::open_db()) {
                crate::db::queries::load_bookmarks(&conn, &cw.abbrev)
                    .unwrap_or_default()
                    .into_iter()
                    .collect()
            } else {
                std::collections::HashSet::new()
            }
        };
        let new_is_bookmarked: Vec<bool> = if let Some(ref lm) = state.line_map {
            lm.buffer_to_work
                .iter()
                .map(|opt_idx| {
                    opt_idx
                        .and_then(|idx| Some(bookmark_ids.contains(&state.current_work.as_ref()?.lines.get(idx)?.id)))
                        .unwrap_or(false)
                })
                .collect()
        } else {
            state
                .current_work
                .as_ref()
                .map(|w| w.lines.iter().map(|l| bookmark_ids.contains(&l.id)).collect())
                .unwrap_or_default()
        };
        *state.is_bookmarked.borrow_mut() = new_is_bookmarked;
    }
    let left_margin = state.text_view.left_margin();
    let gutter_width = (left_margin - 20).max(10);
    let renderer = crate::gutter::setup_timestamp_gutter(
        &state.text_view,
        state.sign_column_visible.clone(),
        state.has_timestamp.clone(),
        state.is_manual.clone(),
        state.is_chapter_line.clone(),
        state.is_bookmarked.clone(),
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
    state.gutter_logical_left.set(left_margin);

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

    crate::logging::log(&format!(
        "TRANSLATIONS: toggle entry visible={} buf_lines={} translations={} current_line={} page_top={} line_map={}",
        state.translations_visible,
        state.buffer.line_count(),
        state.translations.len(),
        state.current_line,
        state.page_top_line,
        state.line_map.is_some(),
    ));

    if state.translations_visible {
        hide_translations(state);
    } else {
        show_translations(state);
    }
}

fn show_translations(state: &mut AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => {
            crate::logging::log("TRANSLATIONS: show aborted — no current_work");
            return;
        }
    };

    // Capture the cursor's on-screen y-position BEFORE mutating the buffer.
    // The cursor is the user's visual anchor — keep it at the same screen
    // position after inserts so the viewport does not appear to scroll.
    let pre_adj_value = state.scrolled_window.vadjustment().value();
    let cursor_screen_y = state
        .buffer
        .iter_at_line(state.current_line as i32)
        .map(|iter| {
            let (y, _h) = state.text_view.line_yrange(&iter);
            y as f64 - pre_adj_value
        });

    // Build a list of (buffer_line, translation_text) pairs
    let mut inserts: Vec<(usize, String)> = Vec::new();
    let line_count = state.buffer.line_count() as usize;
    let lm_len = state
        .line_map
        .as_ref()
        .map(|lm| lm.buffer_to_work.len())
        .unwrap_or(0);

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

    crate::logging::log(&format!(
        "TRANSLATIONS: show scan buf_lines={} line_map_len={} work_lines={} inserts={}",
        line_count,
        lm_len,
        work.lines.len(),
        inserts.len(),
    ));

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
    let trans_size = state.config.font_size.saturating_sub(4);
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
    let old_current = state.current_line;
    let old_top = state.page_top_line;
    state.current_line = map_line_after_insert(state.current_line, &inserts);
    state.page_top_line = map_line_after_insert(state.page_top_line, &inserts);

    state.translations_visible = true;

    reapply_font(state);
    crate::input::navigation::invalidate_page_tops(state);
    // Repaint the cursor highlight but do NOT page-turn. Restore scroll so
    // the cursor stays at the same on-screen y-position the user was looking
    // at before the toggle (anchor on the highlight, not on page_top).
    crate::input::navigation::update_highlight_only(state);

    let cur_y = state
        .buffer
        .iter_at_line(state.current_line as i32)
        .map(|iter| state.text_view.line_yrange(&iter).0 as f64);
    let adj = state.scrolled_window.vadjustment();
    let adj_upper = adj.upper();
    let adj_page = adj.page_size();
    let new_adj = match (cur_y, cursor_screen_y) {
        (Some(y), Some(sy)) => Some((y - sy).max(0.0).min((adj_upper - adj_page).max(0.0))),
        _ => None,
    };
    crate::logging::log(&format!(
        "TRANSLATIONS: anchor cur_y={:?} cursor_screen_y={:?} upper={} page={} new_adj={:?}",
        cur_y, cursor_screen_y, adj_upper as i64, adj_page as i64, new_adj
    ));
    if let Some(val) = new_adj {
        adj.set_value(val);
    }

    // Translation toggle changes line heights via reapply_font; refresh the
    // bottom clip against the new metrics so the last visible line isn't
    // clipped or surrounded by excess gap. resnap_page would clobber the
    // anchored adj.set_value above, so use the lighter refresh helper.
    crate::input::navigation::refresh_bottom_clip(state);

    let new_buf_lines = state.buffer.line_count() as usize;
    let lm_len_after = state
        .line_map
        .as_ref()
        .map(|lm| lm.buffer_to_work.len())
        .unwrap_or(0);
    let line_map_stale = lm_len_after != new_buf_lines;
    let post_adj_value = state.scrolled_window.vadjustment().value();
    crate::logging::log(&format!(
        "TRANSLATIONS: shown inserted={} buf_lines {}->{} current {}->{} page_top {}->{} line_map_len={} stale={} adj {}->{}",
        inserts.len(),
        new_buf_lines.saturating_sub(inserts.len()),
        new_buf_lines,
        old_current,
        state.current_line,
        old_top,
        state.page_top_line,
        lm_len_after,
        line_map_stale,
        pre_adj_value as i64,
        post_adj_value as i64,
    ));

    rebuild_line_number_gutter(state);
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
    // Capture the cursor's on-screen y-position BEFORE removing lines so we
    // can restore it afterwards — the cursor is the user's visual anchor.
    let pre_adj_value = state.scrolled_window.vadjustment().value();
    let cursor_screen_y = state
        .buffer
        .iter_at_line(state.current_line as i32)
        .map(|iter| {
            let (y, _h) = state.text_view.line_yrange(&iter);
            y as f64 - pre_adj_value
        });

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
    let pre_hide_buf_lines = line_count;
    state.current_line = map_line_before_insert(old_current, &state.translation_lines);
    state.page_top_line = map_line_before_insert(old_top, &state.translation_lines);

    state.translation_lines.clear();
    state.translations_visible = false;

    reapply_font(state);
    crate::input::navigation::invalidate_page_tops(state);
    // Repaint highlight but do NOT page-turn. Restore scroll so the cursor
    // sits at the same on-screen y-position the user had before the toggle.
    crate::input::navigation::update_highlight_only(state);

    let cur_y = state
        .buffer
        .iter_at_line(state.current_line as i32)
        .map(|iter| state.text_view.line_yrange(&iter).0 as f64);
    let adj = state.scrolled_window.vadjustment();
    let adj_upper = adj.upper();
    let adj_page = adj.page_size();
    let new_adj = match (cur_y, cursor_screen_y) {
        (Some(y), Some(sy)) => Some((y - sy).max(0.0).min((adj_upper - adj_page).max(0.0))),
        _ => None,
    };
    crate::logging::log(&format!(
        "TRANSLATIONS: anchor cur_y={:?} cursor_screen_y={:?} upper={} page={} new_adj={:?}",
        cur_y, cursor_screen_y, adj_upper as i64, adj_page as i64, new_adj
    ));
    if let Some(val) = new_adj {
        adj.set_value(val);
    }

    // Translation toggle changes line heights via reapply_font; refresh the
    // bottom clip against the new metrics so the last visible line isn't
    // clipped or surrounded by excess gap. resnap_page would clobber the
    // anchored adj.set_value above, so use the lighter refresh helper.
    crate::input::navigation::refresh_bottom_clip(state);

    let new_buf_lines = state.buffer.line_count() as usize;
    let post_adj_value = state.scrolled_window.vadjustment().value();
    crate::logging::log(&format!(
        "TRANSLATIONS: hidden buf_lines {}->{} current {}->{} page_top {}->{} adj {}->{}",
        pre_hide_buf_lines,
        new_buf_lines,
        old_current,
        state.current_line,
        old_top,
        state.page_top_line,
        pre_adj_value as i64,
        post_adj_value as i64,
    ));

    rebuild_line_number_gutter(state);
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

/// Reapply font size using a TextTag spanning the entire buffer.

/// Keep top spacer at fixed TOP_SPACER_HEIGHT.
fn update_spacer_heights(state: &AppState) {
    state.top_spacer.set_height_request(TOP_SPACER_HEIGHT);
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
    let trans_size = state.config.font_size.saturating_sub(4);
    let trans_desc = pango::FontDescription::from_string(
        &format!("{} Italic {}", state.config.font_family, trans_size),
    );
    state.translation_text_tag.set_font_desc(Some(&trans_desc));
    let highest = state.buffer.tag_table().size() - 1;
    state.translation_text_tag.set_priority(highest);
    crate::logging::log(&format!("FONT: reapply_font size={}pt via TextTag", state.config.font_size));
    update_spacer_heights(state);
}

fn rebuild_line_number_gutter(state: &mut AppState) {
    if let Some(old) = state.line_number_renderer.take() {
        crate::gutter::remove_line_number_renderer(&state.text_view, &old);
    }
    let is_prose = state.current_work.as_ref()
        .map(|w| crate::db::line_types::is_prose_work(&w.work_type))
        .unwrap_or(true);
    if !is_prose {
        let base: Vec<Option<i64>> = if let Some(ref lm) = state.line_map {
            lm.buffer_to_work
                .iter()
                .map(|opt_idx| {
                    opt_idx.and_then(|idx| {
                        state.current_work.as_ref()?.lines.get(idx).map(|l| l.line_in_div)
                    })
                })
                .collect()
        } else {
            state.current_work.as_ref()
                .map(|w| w.lines.iter().map(|l| Some(l.line_in_div)).collect())
                .unwrap_or_default()
        };
        let nums = if state.translations_visible && !state.translation_lines.is_empty() {
            let mut expanded = Vec::with_capacity(state.translation_lines.len());
            let mut orig_idx = 0;
            for &is_trans in &state.translation_lines {
                if is_trans {
                    expanded.push(None);
                } else {
                    expanded.push(base.get(orig_idx).copied().flatten());
                    orig_idx += 1;
                }
            }
            expanded
        } else {
            base
        };
        *state.line_numbers.borrow_mut() = nums;
        let renderer = crate::gutter::setup_line_number_gutter(
            &state.text_view,
            state.line_numbers.clone(),
            &state.theme.dim_fg,
            &state.config.font_family,
            state.config.font_size,
        );
        state.text_view.set_right_margin(48);
        state.line_number_renderer = Some(renderer);
    }
}

/// Adjust font size by delta, clamp to 8..=72, reapply CSS and repaginate.
pub fn adjust_font_size(state: &mut AppState, delta: i32) {
    let new_size = (state.config.font_size as i32 + delta).clamp(8, 72) as u32;
    if new_size == state.config.font_size {
        return;
    }
    state.config.font_size = new_size;
    reapply_font(state);
    rebuild_line_number_gutter(state);
    crate::input::navigation::resnap_page(state);
    crate::input::navigation::invalidate_page_tops(state);
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
    rebuild_line_number_gutter(state);
    crate::input::navigation::resnap_page(state);
    crate::input::navigation::invalidate_page_tops(state);
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
    crate::input::navigation::invalidate_page_tops(state);
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

    // Collect unique vocab words on the current line
    let current_line = state.current_line;
    crate::logging::log(&format!(
        "VOCAB POPUP: current_line={}", current_line
    ));
    let mut seen = std::collections::HashSet::new();
    let words: Vec<String> = state
        .vocab_matches
        .iter()
        .filter(|m| m.line_index == current_line)
        .filter(|m| seen.insert(m.word.clone()))
        .map(|m| m.word.clone())
        .collect();

    if words.is_empty() {
        crate::logging::log("VOCAB POPUP: no vocab words on current line");
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
    state.vocab_popup_line = Some(current_line);

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

    let current_line = state.current_line;
    let mut seen = std::collections::HashSet::new();
    let words: Vec<String> = state
        .vocab_matches
        .iter()
        .filter(|m| m.line_index == current_line)
        .filter(|m| seen.insert(m.word.clone()))
        .map(|m| m.word.clone())
        .collect();

    if words.is_empty() {
        state.vocab_popup_data.clear();
        state.vocab_popup.hide();
        state.vocab_popup_line = Some(current_line);
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
    state.vocab_popup_line = Some(current_line);
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
