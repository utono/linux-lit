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
use crate::ui::gloss_picker::GlossPicker;
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
    SynopsisOverlay,
    GlossPrompt,
    GlossPicker,
    EchoPicker,
    EchoTurnsPicker,
    EchoesOverlay,
    GamepadOverlay,
    KeybindsOverlay,
    ConcordancePicker,
    ConcordanceWordPicker,
    EchoLinePicker,
    EchoKeybindsOverlay,
    ConcordanceListPicker,
    ConcordanceWorksPicker,
    AuthorshipPicker,
    ActionPopup,
    Visual,
    DeleteConfirm,
}

#[derive(Clone, Copy, PartialEq)]
pub enum GlossPromptMode {
    Add,
    Edit,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SidebarMode {
    Vocab,
    Synopsis,
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
    pub page_back_stack: Vec<usize>,
    pub dim_tag: gtk4::TextTag,
    pub cursor_line_tag: gtk4::TextTag,
    pub cursor_fade_tag: gtk4::TextTag,
    pub ab_dim_tag: gtk4::TextTag,
    pub page_turn_overlay: gtk4::Overlay,
    pub bottom_clip: gtk4::Box,
    pub top_spacer: gtk4::Box,
    pub card_vbox: gtk4::Box,
    pub scrolled_window: ScrolledWindow,
    /// Left-column container. Carries the divider-hug left margin in two-column
    /// mode so the left column's text shifts toward the center divider.
    pub scrolled_overlay: gtk4::Overlay,
    pub right_view: View,
    pub right_scrolled_window: ScrolledWindow,
    pub right_scrolled_overlay: gtk4::Overlay,
    pub right_bottom_clip: gtk4::Box,
    pub columns_hbox: gtk4::Box,
    /// Thin vertical rule between the two columns; visible only in two-column mode.
    pub column_divider: gtk4::Separator,
    pub right_line_number_renderer: Option<sourceview5::GutterRendererText>,
    /// Sign-column (timestamp/bookmark glyph) renderer for the right column in
    /// two-column mode. Mirrors `gutter_renderer` on the left `text_view`.
    pub right_gutter_renderer: Option<sourceview5::GutterRendererText>,
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
    /// Reader position (current_line, page_top_line) saved when search opens, so
    /// Escape can cancel the live-search jump and restore the original page.
    pub search_return_pos: Option<(usize, usize)>,
    /// Reader position (current_line, page_top_line) saved when a gloss overlay
    /// opens (picker, MRU toggle, or from synopsis), so Escape restores the page
    /// the user was on instead of jumping to the glossed passage.
    pub gloss_return_pos: Option<(usize, usize)>,
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
    /// True when the left column's line numbers are in its LEFT gutter (the
    /// two-column "book foliation" layout) rather than the default right gutter.
    /// Tells the teardown path which gutter to remove `line_number_renderer` from.
    pub line_number_renderer_on_left: bool,
    pub line_numbers: Rc<RefCell<Vec<Option<i64>>>>,
    pub ab_repeat: crate::ab_repeat::AbRepeatState,
    pub ab_a_line: Rc<Cell<Option<usize>>>,
    pub ab_b_line: Rc<Cell<Option<usize>>>,
    pub line_map: Option<crate::text_file_map::LineMap>,
    pub settings_overlay: crate::ui::settings_overlay::SettingsOverlay,
    pub media_picker: MediaPicker,
    pub bookmark_picker: BookmarkPicker,
    pub dialogue_formatting_active: bool,
    pub authorship_tag: gtk4::TextTag,
    pub authorship_line_ids: std::collections::HashSet<i64>,
    pub authorship_enabled: bool,
    pub authorship_sets: Vec<crate::db::authorship::AttributionSet>,
    pub active_attribution_set_id: Option<i64>,
    pub authorship_picker: crate::ui::authorship_picker::AuthorshipPicker,
    pub translations: HashMap<i64, String>,
    pub translations_visible: bool,
    /// Sign-column visibility saved when translations are shown, so it can be
    /// restored when translations are hidden. `None` when not in translation
    /// mode. Signs are hidden while translations are visible.
    pub sign_visible_before_translations: Option<bool>,
    /// `(current_line, page_top_line)` captured before `show_translations`
    /// mutates them, so toggling translations off restores the exact
    /// pre-toggle page. `None` when not in translation mode.
    pub pre_translation_page: Option<(usize, usize)>,
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
    pub gloss_opened_from_picker: bool,
    pub gloss_prompt_container: Option<glib::WeakRef<gtk4::Box>>,
    pub gloss_prompt_overlay: Option<glib::WeakRef<gtk4::Overlay>>,
    pub gloss_prompt_textview: Option<glib::WeakRef<gtk4::TextView>>,
    pub gloss_prompt_mode: GlossPromptMode,
    pub delete_confirm_container: Option<glib::WeakRef<gtk4::Box>>,
    pub delete_confirm_overlay: Option<glib::WeakRef<gtk4::Overlay>>,
    pub gloss_picker: GlossPicker,
    pub echo_picker: crate::ui::echo_picker::EchoPicker,
    pub echo_turns_picker: crate::ui::echo_turns_picker::EchoTurnsPicker,
    pub pending_echo_context: Option<crate::gloss::GlossContext>,
    pub pending_echo_scene_lines: Vec<crate::db::models::Line>,
    pub echo_overlay_links: Vec<crate::db::queries::StoredEchoLink>,
    pub echo_overlay_index: usize,
    pub echo_overlay_titles: std::collections::HashMap<String, String>,
    pub echo_overlay_source: String,
    pub echo_overlay_turn_id: Option<i64>,
    pub echo_overlay_turn_key: Option<crate::db::queries::EchoTurnKey>,
    pub echo_session: Option<crate::input::actions::echoes::EchoSession>,
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
    pub sidebar_mode: SidebarMode,
    pub synopsis_cache: HashMap<(i64, i64), String>,
    pub synopsis_visible: bool,
    /// The (div1, div2) scene currently displayed in the synopsis overlay. n/p
    /// step this through the work's scenes; the `A` amend targets it too.
    pub synopsis_overlay_scene: (i64, i64),
    /// The (div1, div2) scene whose synopsis the open `A` amend prompt targets.
    pub synopsis_amend_scene: (i64, i64),
    /// Single-level undo for the `A` amend flow: the scene and its synopsis text
    /// from immediately before the last amendment. `U` in the synopsis overlay
    /// restores it. Cleared once consumed.
    pub synopsis_undo: Option<((i64, i64), String)>,
    pub concordance_picker: crate::ui::concordance_picker::ConcordancePicker,
    pub concordance_state: Option<crate::concordance::ConcordanceState>,
    pub concordance_origin: Option<crate::concordance::ConcordanceOrigin>,
    pub concordance_word_cache: Option<(String, Vec<(String, usize)>)>,
    pub concordance_word_picker: crate::ui::concordance_word_picker::ConcordanceWordPicker,
    pub echo_line_picker: crate::ui::echo_line_picker::EchoLinePicker,
    pub echo_keybinds_overlay: crate::ui::echo_keybinds_overlay::EchoKeybindsOverlay,
    /// turn_id the add-echo picker will attach the chosen line to.
    pub echo_add_turn_id: Option<i64>,
    pub concordance_list_picker: crate::ui::concordance_list_picker::ConcordanceListPicker,
    pub concordance_works_picker: crate::ui::concordance_works_picker::ConcordanceWorksPicker,
    pub concordance_bar: crate::ui::concordance_bar::ConcordanceBar,
    pub title_bar: gtk4::Box,
    pub title_bar_label: gtk4::Label,
    pub title_bar_scene_label: gtk4::Label,
    /// Index of the current sentence group (for prose with text_file).
    pub current_sentence_group: Option<usize>,
    /// Tracks the start line of the current paragraph to detect transitions.
    pub current_paragraph_start: Option<usize>,
    /// Tracks (div1, div2) of the last synced dialogue line to detect scene transitions.
    pub current_sync_scene: Option<(i64, i64)>,
    pub nav_test_active: bool,
    pub nav_test_step: usize,
    pub nav_test_failures: usize,
    pub nav_test_prev_top: usize,
    pub nav_test_expect_return: Option<usize>,
    /// When true the nav-test harness runs the long random `fuzz` script
    /// (verifying jump landings) instead of the fixed `jumps-only` script.
    pub nav_test_fuzz: bool,
    pub sync_enabled: bool,
    pub mpv_connected: bool,
    pub mpv_playing: bool,
    pub concordance_resume_playback: bool,
    pub sync_enabled_before_concordance: Option<bool>,
    pub skip_mpv_discovery: bool,
    pub debug_icon: gtk4::Label,
    pub word_status_label: gtk4::Label,
    pub chapter_toast: gtk4::Label,
    pub speed_toast: gtk4::Label,
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
    /// Deferred synopsis show: set in display_work when the cursor lands on a
    /// scene boundary, cleared by the resize tick once layout is valid.
    pub pending_synopsis: Rc<Cell<bool>>,
    /// First-open page anchor: set in display_work when a work is opened with no
    /// saved position, so `update_highlight_and_show` keeps `page_top_line == 0`
    /// (the opening Act/Prologue header at the top of the page) instead of
    /// scrolling the page down to the first dialogue line. Cleared on read.
    pub pending_top_anchor: Rc<Cell<bool>>,
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

    /// Number of e-reader columns for the CURRENT work: scroll mode → 1;
    /// translations visible → 1; else a per-work override (if `Alt+[` set one)
    /// wins, otherwise the work-type default (2 for a Shakespeare play, else 1).
    /// Clamped to 1..=2.
    ///
    /// Translations force a single column: they roughly double the buffer line
    /// count, but the two-column pagination math (`column_split`/`visible_range`)
    /// is bounded by `effective_line_count`, which excludes the inserted
    /// translation lines. Paginating the inflated buffer with that bound yields
    /// degenerate (one-line) or underfilled spreads. The single-column scroll
    /// path walks the real buffer and handles translations correctly.
    pub fn column_count(&self) -> u8 {
        if !matches!(self.config.navigation_mode, crate::config::NavigationMode::EReader) {
            return 1;
        }
        if self.translations_visible {
            return 1;
        }
        let Some(work) = self.current_work.as_ref() else {
            return 1;
        };
        let n = self.config.column_overrides
            .get(&work.abbrev)
            .copied()
            .unwrap_or_else(|| default_column_count_for(work));
        n.clamp(1, 2)
    }

    pub fn work_line_for_buffer(&self, buffer_line: usize) -> Option<usize> {
        if let Some(ref lm) = self.line_map {
            lm.buffer_to_work.get(buffer_line).copied().flatten()
        } else {
            let count = self.current_work.as_ref().map_or(0, |w| w.lines.len());
            if buffer_line < count { Some(buffer_line) } else { None }
        }
    }

    /// Authoritative scene/section-boundary check for a buffer line, derived
    /// from the DB `(div1,div2)` columns at load (`LineMap.section_starts`).
    /// Returns `false` when the bitmap is absent (mid-load) or out of range —
    /// callers needing a mid-load fallback consult the buffer text directly.
    pub fn is_section_start(&self, buffer_line: usize) -> bool {
        self.line_map
            .as_ref()
            .and_then(|lm| lm.section_starts.get(buffer_line).copied())
            .unwrap_or(false)
    }

    /// Borrow the section-boundary bitmap, if a line map is loaded. Pagination
    /// helpers thread this slice down so they consult the authoritative DB
    /// boundary instead of re-inferring it from buffer text.
    pub fn section_starts(&self) -> Option<&[bool]> {
        self.line_map.as_ref().map(|lm| lm.section_starts.as_slice())
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
pub const VERSE_LEFT_OFFSET: i32 = 120;
pub const PROSE_LEFT_OFFSET: i32 = 120;

/// Left offset used in two-column mode. Small on purpose: it only needs to
/// give the sign-column gutter enough room to put padding to the LEFT of the
/// sign glyphs (the gutter is `logical_left - 20` wide, right-aligned), without
/// stealing the column width the full verse/monocle offset would and causing
/// verse lines to wrap. logical_left = text_margins + this; gutter width then
/// = (logical_left - 20).
pub const TWO_COLUMN_LEFT_OFFSET: i32 = 30;

/// Extra left margin applied to dialogue lines (hanging indent beneath the
/// flush-left speaker name). Single-column uses the full monocle indent;
/// two-column halves it because each column is too narrow to spend 60px on
/// indentation without wrapping verse lines.
pub const DIALOGUE_INDENT: i32 = 60;
pub const TWO_COLUMN_DIALOGUE_INDENT: i32 = 20;

/// Fixed height for the top spacer above the first text line.
pub const TOP_SPACER_HEIGHT: i32 = 40;

/// Pure default-column rule: every work defaults to two columns. Split out from
/// `default_column_count_for` so it is unit-testable without constructing a
/// `Work`. Per-work overrides in `config.column_overrides` still take
/// precedence, and `column_count()` forces a single column when not in EReader
/// mode or when translations are visible.
pub(crate) fn default_column_count_for_parts(_author: &str, _work_type: &str) -> u8 {
    2
}

/// Default column count for a work: 2 columns for all works by default.
pub(crate) fn default_column_count_for(work: &crate::db::models::Work) -> u8 {
    default_column_count_for_parts(&work.author, &work.work_type)
}

/// (renderer width, trailing margin past the number, gap between text and
/// number) for the right-side line-number gutter. Two-column mode uses a
/// tighter text↔number gap (more room for the verse line) but keeps real
/// padding past the number so it doesn't crowd the column/card edge.
pub(crate) fn line_number_gutter_geometry(column_count: u8) -> (i32, i32, i32) {
    if column_count >= 2 {
        (
            crate::gutter::LINE_NUMBER_WIDTH_TWO_COL,
            crate::gutter::LINE_NUMBER_MARGIN_END_TWO_COL,
            crate::gutter::LINE_NUMBER_TEXT_GAP_TWO_COL,
        )
    } else {
        (
            crate::gutter::LINE_NUMBER_WIDTH,
            crate::gutter::LINE_NUMBER_MARGIN_END,
            crate::gutter::LINE_NUMBER_MARGIN_END,
        )
    }
}

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
    // The full verse/prose left offset is a monocle (single wide column)
    // aesthetic. In two-column mode each column is narrow, so we use a small
    // offset instead: enough to give the sign-column gutter padding to the left
    // of its glyphs, but not so much that verse lines wrap.
    let two_col = state.column_count() == 2;
    // In two-column mode the left column's verse line numbers sit in its LEFT
    // gutter (book foliation), outside the sign column, so the left margin must
    // reserve room for them on top of the normal offset. Prose has no numbers.
    let left_number_allowance = if two_col && !tiled && is_verse && SHOW_LINE_NUMBERS_TWO_COL {
        crate::gutter::LINE_NUMBER_WIDTH_TWO_COL + crate::gutter::LINE_NUMBER_LEFT_GAP_TWO_COL
    } else {
        0
    };
    let left_bump = if state.translations_visible {
        // Translation view: the card is now sized like the two-column layout
        // (wide), so inset the text like the gloss/synopsis cards (~card_width/4
        // from the card edge) instead of hugging the left edge. Use the ACTUAL
        // on-screen card width (clamped to the window) so the inset degrades to
        // 0 when the card fills a narrow window — this runs even when `tiled`
        // (which is computed against column_width, not the wide translation
        // card) would otherwise be true. Subtract the base text_margins so
        // logical_left lands at ~card_width/4 overall.
        let target = target_card_width(
            window_width, state.config.column_width, state.column_count(), true,
        );
        let card_w = target.min(window_width.max(1));
        (card_w / 4 - state.config.text_margins as i32).max(0)
    } else if tiled {
        0
    } else if two_col {
        TWO_COLUMN_LEFT_OFFSET + left_number_allowance
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
    } else if state.sign_column_visible.get() {
        // Sign column is shown by default — create the gutter on the first
        // layout pass after a work loads. Margin is at logical_left here, so
        // setup_gutter() computes its width correctly.
        if state.text_view.left_margin() != logical_left {
            state.text_view.set_left_margin(logical_left);
            if state.dialogue_formatting_active {
                apply_dialogue_formatting(state);
            }
        }
        setup_gutter(state);
    } else if state.text_view.left_margin() != logical_left {
        state.text_view.set_left_margin(logical_left);
        if state.dialogue_formatting_active {
            apply_dialogue_formatting(state);
        }
    }

    // Right margin = gap between the text and its line number. In two-column
    // mode this must stay the tight line-number gap (set by the gutter setup),
    // NOT the wide single-column EXTRA_RIGHT_MARGIN — otherwise the left
    // column loses ~90px of text width and verse lines wrap. The right view's
    // margin is set in the gutter setup and never touched here, so without
    // this guard the two columns would also be asymmetric (left narrower).
    if two_col {
        state.text_view.set_right_margin(crate::gutter::LINE_NUMBER_TEXT_GAP_TWO_COL);
    } else if state.translations_visible {
        // Translation view: inset the right edge like the gloss/synopsis cards
        // (~card_width/4) so the reading block is symmetric within the wide card.
        let target = target_card_width(
            window_width, state.config.column_width, state.column_count(), true,
        );
        let card_w = target.min(window_width.max(1));
        state.text_view.set_right_margin((card_w / 4).max(crate::gutter::LINE_NUMBER_TEXT_GAP_TWO_COL));
    } else {
        let logical_right = state.config.text_margins as i32
            + crate::config::EXTRA_RIGHT_MARGIN;
        state.text_view.set_right_margin(logical_right);
    }

    // Book-spine symmetry. In two-column mode each half of the card is wider
    // than the verse needs, so left-aligned text in both columns drifts toward
    // the left of the card. To make the layout hug the center divider
    // symmetrically, size each column's scrolled window to the text width and
    // align it toward the divider: left column → right-aligned, right column →
    // left-aligned. Equal leftover then falls on the two outer edges.
    if two_col && !tiled {
        // Center the two-column BLOCK in the card rather than letting each
        // column fill half of it. Each column is fixed to its natural width
        // (the verse-safe column width); the block [col | divider | col] then
        // sizes to content and is centered, so the card's slack becomes equal
        // outer margins on both sides and the divider stays centered.
        let col_w = MIN_TWO_COLUMN_COLUMN_WIDTH;
        state.columns_hbox.set_hexpand(false);
        state.columns_hbox.set_halign(gtk4::Align::Center);
        state.scrolled_overlay.set_margin_start(0);
        state.scrolled_overlay.set_hexpand(false);
        state.scrolled_overlay.set_width_request(col_w);
        state.right_scrolled_overlay.set_hexpand(false);
        state.right_scrolled_overlay.set_width_request(col_w);
        // Each scrolled window fills its fixed-width column overlay; text is
        // left-aligned inside as usual.
        state.scrolled_window.set_hexpand(true);
        state.scrolled_window.set_halign(gtk4::Align::Fill);
        state.scrolled_window.set_width_request(-1);
        state.right_scrolled_window.set_hexpand(true);
        state.right_scrolled_window.set_halign(gtk4::Align::Fill);
        state.right_scrolled_window.set_width_request(-1);
    } else {
        // Restore single-column fill behavior.
        state.columns_hbox.set_hexpand(true);
        state.columns_hbox.set_halign(gtk4::Align::Fill);
        state.scrolled_overlay.set_margin_start(0);
        state.scrolled_overlay.set_hexpand(true);
        state.scrolled_overlay.set_width_request(-1);
        state.right_scrolled_overlay.set_hexpand(true);
        state.right_scrolled_overlay.set_width_request(-1);
        state.scrolled_window.set_hexpand(true);
        state.scrolled_window.set_halign(gtk4::Align::Fill);
        state.scrolled_window.set_width_request(-1);
        state.right_scrolled_window.set_hexpand(true);
        state.right_scrolled_window.set_halign(gtk4::Align::Fill);
        state.right_scrolled_window.set_width_request(-1);
    }

    state.top_spacer.set_height_request(TOP_SPACER_HEIGHT);
}

/// Reconfigure the column layout to match the current `column_count()`:
/// re-run `apply_tiled_mode` (margins/widths/gutter) and show or hide the
/// right column + divider. Use after anything that changes `column_count()`
/// at runtime — e.g. toggling translations, which forces a single column.
pub fn apply_column_layout(state: &mut AppState) {
    let vbox = state.vbox.clone();
    let ww = state.window.width();
    // Resize the card to match the current layout: the narrow centered
    // translation card, the configured single-column width, or the wide
    // two-column card. apply_tiled_mode only sets margins/gutters, not the
    // card's width_request, so this must run too or the card keeps its old
    // (wrong) width.
    let cw = state.config.column_width;
    let cc = state.column_count();
    let tr = state.translations_visible;
    apply_card_sizing(&state.content_hbox, ww, cw, cc, tr);
    apply_tiled_mode(state, &vbox, ww);
    let two_col = state.column_count() == 2;
    state.right_scrolled_overlay.set_visible(two_col);
    state.column_divider.set_visible(two_col);
    if !two_col {
        state.right_bottom_clip.set_height_request(0);
    }
}

/// Fraction of the window width the two-column card aims to fill (minus the
/// outer margins). One-column works keep their fixed `column_width`.
pub const TWO_COLUMN_WIDTH_FRACTION: f32 = 0.68;

/// Whether to show verse line numbers in two-column mode. When false, the
/// left-column outer-foliation numbers and the right-column numbers are both
/// skipped, reclaiming ~40px per column for the text. (Experimental: flip to
/// `true` to restore the book-style foliation.)
pub const SHOW_LINE_NUMBERS_TWO_COL: bool = false;

/// Verse-safe floor for a single column's width in two-column mode. The longest
/// Folger verse line (~63 chars in Charter 19) needs roughly this much text
/// width; below it, verse starts wrapping. With line numbers hidden in
/// two-column mode there's no number gutter eating into the column, so this can
/// sit closer to the bare text width. The card is never narrowed below `2 ×`
/// this (plus the divider), so shrinking `TWO_COLUMN_WIDTH_FRACTION` can never
/// push a column into wrapping.
pub const MIN_TWO_COLUMN_COLUMN_WIDTH: i32 = 700;

/// Target card width before clamping to the window.
///
/// - One column: the configured `column_width` (unchanged).
/// - Two columns: the larger of `column_width` and 85% of the window, so the
///   card grows on wide screens instead of squeezing two columns into one
///   column's worth of space. Never narrower than the single-column floor.
/// Tighter card width for the single-column translation view: a comfortable
/// reading measure (the verse-safe column width) plus room for the line-number
/// gutter, so the block centers in a wide window and the numbers hug the text
/// ends rather than sitting at the far card edge.
pub(crate) fn target_card_width(
    window_width: i32,
    column_width: u32,
    column_count: u8,
    translations: bool,
) -> i32 {
    let cw_cfg = column_width as i32;
    // Translation mode renders two logical columns (original + translation), so
    // size its card identically to the two-column layout — same width whether or
    // not translations are visible.
    if column_count >= 2 || translations {
        let proportional = (window_width as f32 * TWO_COLUMN_WIDTH_FRACTION) as i32;
        // Never narrow a column below the verse-safe floor: two columns plus a
        // few px for the divider. Also never below the single-column floor.
        let two_col_floor = 2 * MIN_TWO_COLUMN_COLUMN_WIDTH + 8;
        proportional.max(cw_cfg).max(two_col_floor)
    } else {
        cw_cfg
    }
}

pub fn apply_card_sizing(
    content_hbox: &gtk4::Box,
    window_width: i32,
    column_width: u32,
    column_count: u8,
    translations: bool,
) {
    const MAX_OUTER_MARGIN: i32 = 24;
    let ww = window_width.max(0);
    let target = target_card_width(ww, column_width, column_count, translations);
    // Reserve room for margins first; if that overflows, the card itself shrinks.
    let card_w = target.min(ww.max(1));
    let slack = ww - card_w;
    let margin = (slack / 2).clamp(0, MAX_OUTER_MARGIN);
    content_hbox.set_width_request(card_w);
    content_hbox.set_margin_start(margin);
    content_hbox.set_margin_end(margin);
    crate::log_fmt!(
        "CARD_SIZING: ww={} col_cfg={} cols={} target={} card_w={} margin={}",
        ww, column_width as i32, column_count, target, card_w, margin
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

    let authorship_tag = gtk4::TextTag::builder()
        .name("authorship-italic")
        .style(pango::Style::Italic)
        .build();
    buffer.tag_table().add(&authorship_tag);

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

    // RIGHT column view — shares the same buffer as the left view. Hidden
    // until column_count == 2 (set in a later task's toggle).
    let right_view = View::builder()
        .buffer(&buffer)
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(WrapMode::Word)
        .build();
    right_view.set_show_line_numbers(false);
    right_view.set_highlight_current_line(false);
    right_view.set_pixels_above_lines(config.line_spacing as i32);
    right_view.set_pixels_below_lines(config.line_spacing as i32);
    right_view.set_left_margin(config.text_margins as i32);
    right_view.set_right_margin(config.text_margins as i32 + crate::config::EXTRA_RIGHT_MARGIN);
    right_view.set_top_margin(0);
    right_view.set_bottom_margin(40);

    let right_scrolled = ScrolledWindow::builder()
        .child(&right_view)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::External)
        .vexpand(true)
        .hexpand(true)
        .valign(gtk4::Align::Fill)
        .overflow(gtk4::Overflow::Hidden)
        .build();
    right_scrolled.add_css_class("card-bottom");

    let right_scrolled_overlay = gtk4::Overlay::new();
    right_scrolled_overlay.set_child(Some(&right_scrolled));
    right_scrolled_overlay.set_vexpand(true);
    right_scrolled_overlay.set_hexpand(true);

    let right_bottom_clip = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    right_bottom_clip.set_valign(gtk4::Align::End);
    right_bottom_clip.set_hexpand(true);
    right_bottom_clip.set_height_request(0);
    right_bottom_clip.add_css_class("card-bottom");
    right_scrolled_overlay.add_overlay(&right_bottom_clip);

    // Columns row: left | divider | right. Right starts hidden (1-column
    // default); the divider is a thin vertical rule shown only in two-column
    // mode to separate the columns like a book's gutter.
    let column_divider = gtk4::Separator::new(gtk4::Orientation::Vertical);
    column_divider.add_css_class("column-divider");
    column_divider.set_visible(false);
    let columns_hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    columns_hbox.set_vexpand(true);
    columns_hbox.set_hexpand(true);
    columns_hbox.append(&scrolled_overlay);
    columns_hbox.append(&column_divider);
    columns_hbox.append(&right_scrolled_overlay);
    right_scrolled_overlay.set_visible(false);

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
    card_vbox.append(&columns_hbox);

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
    if let Ok(conn) = crate::db::queries::open_db() {
        if let Ok(coauthored) = crate::db::authorship::load_coauthored_works(&conn) {
            picker.set_coauthored_works(coauthored);
        }
    }
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
    // Prose-gloss commentary uses the normal foreground (no dimming) — it is
    // set off from the verse only by a slightly smaller scale and looser spacing.
    gloss_overlay.attach(&gamepad_overlay.overlay);
    gloss_overlay.overlay.set_vexpand(true);

    // Gloss picker wraps the gloss overlay
    let gloss_picker = GlossPicker::new();
    gloss_picker.attach(&gloss_overlay.overlay);
    gloss_picker.overlay.set_vexpand(true);

    // Echo picker wraps the gloss picker
    let echo_picker = crate::ui::echo_picker::EchoPicker::new();
    echo_picker.attach(&gloss_picker.overlay);
    echo_picker.overlay.set_vexpand(true);

    // Concordance picker wraps the echo picker
    let concordance_picker = crate::ui::concordance_picker::ConcordancePicker::new();
    concordance_picker.attach(&echo_picker.overlay);
    concordance_picker.overlay.set_vexpand(true);

    // Concordance word picker wraps the concordance picker
    let concordance_word_picker = crate::ui::concordance_word_picker::ConcordanceWordPicker::new();
    concordance_word_picker.attach(&concordance_picker.overlay);
    concordance_word_picker.overlay.set_vexpand(true);

    // Concordance list picker wraps the word picker
    let concordance_list_picker = crate::ui::concordance_list_picker::ConcordanceListPicker::new();
    concordance_list_picker.attach(&concordance_word_picker.overlay);
    concordance_list_picker.overlay.set_vexpand(true);

    // Authorship picker wraps the concordance list picker
    let authorship_picker = crate::ui::authorship_picker::AuthorshipPicker::new();
    authorship_picker.attach(&concordance_list_picker.overlay);
    authorship_picker.overlay.set_vexpand(true);

    // Echo turns picker (Ctrl+Shift+G: list all turns in this work that have
    // echoes). add_overlay panel onto the outer overlay, NOT wrapped into the
    // reader's size-bearing chain (wrapping collapses the reader layout).
    let echo_turns_picker = crate::ui::echo_turns_picker::EchoTurnsPicker::new();
    authorship_picker.overlay.add_overlay(echo_turns_picker.picker_box());

    // Echo line picker (add-echo: choose a line to attach an echo to).
    // Added as an overlay panel onto the outer overlay (like concordance_works
    // below), NOT wrapped into the reader's size-bearing chain — wrapping it
    // orphaned the reader content and collapsed the layout (sw_h stuck at 0).
    let echo_line_picker = crate::ui::echo_line_picker::EchoLinePicker::new();
    authorship_picker.overlay.add_overlay(&echo_line_picker.picker_box);

    // Echo keybinds legend (Ctrl+/ in the echoes overlay). add_overlay panel,
    // NOT a chain link (chain insertion collapses the reader layout).
    let echo_keybinds_overlay = crate::ui::echo_keybinds_overlay::EchoKeybindsOverlay::new();
    echo_keybinds_overlay.attach_to(&authorship_picker.overlay);

    // Concordance works picker (Alt+R: jump to a specific work)
    let concordance_works_picker = crate::ui::concordance_works_picker::ConcordanceWorksPicker::new();
    authorship_picker.overlay.add_overlay(&concordance_works_picker.scrim);
    authorship_picker.overlay.add_overlay(&concordance_works_picker.container);

    // Action popup overlay for visual mode
    let action_popup_widget = crate::ui::action_popup::ActionPopup::new();
    authorship_picker.overlay.add_overlay(&action_popup_widget.container);

    // Add vocab popup to full-width overlay so it appears to the right of the text card
    vocab_popup.attach_to(&authorship_picker.overlay);

    // Debug-mode indicator (lower-left corner, next to sync icon, hidden by default)
    let debug_icon = gtk4::Label::new(Some("⚙"));
    debug_icon.set_valign(gtk4::Align::End);
    debug_icon.set_halign(gtk4::Align::Start);
    debug_icon.set_margin_start(44);
    debug_icon.set_margin_bottom(12);
    debug_icon.add_css_class("debug-icon");
    debug_icon.set_visible(false);
    authorship_picker.overlay.add_overlay(&debug_icon);
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
    authorship_picker.overlay.add_overlay(&word_status_label);

    let chapter_toast = gtk4::Label::new(None);
    chapter_toast.set_valign(gtk4::Align::End);
    chapter_toast.set_halign(gtk4::Align::Center);
    chapter_toast.set_margin_bottom(32);
    chapter_toast.add_css_class("chapter-toast");
    chapter_toast.set_visible(false);
    authorship_picker.overlay.add_overlay(&chapter_toast);

    let speed_toast = gtk4::Label::new(None);
    speed_toast.set_valign(gtk4::Align::End);
    speed_toast.set_halign(gtk4::Align::Start);
    speed_toast.set_margin_bottom(32);
    speed_toast.set_margin_start(24);
    speed_toast.add_css_class("chapter-toast");
    speed_toast.set_visible(false);
    authorship_picker.overlay.add_overlay(&speed_toast);

    // Concordance status bar
    let concordance_bar = crate::ui::concordance_bar::ConcordanceBar::new();

    // Work title bar (persistent footer showing author + title, scene info)
    let title_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    title_bar.set_hexpand(true);
    title_bar.add_css_class("title-bar");

    let title_bar_left_spacer = gtk4::Label::new(None);
    title_bar_left_spacer.set_halign(gtk4::Align::Start);
    title_bar_left_spacer.set_hexpand(true);

    let title_bar_label = gtk4::Label::new(None);
    title_bar_label.set_halign(gtk4::Align::Center);
    title_bar_label.set_hexpand(true);
    title_bar_label.add_css_class("title-bar-label");

    let title_bar_scene_label = gtk4::Label::new(None);
    title_bar_scene_label.set_halign(gtk4::Align::End);
    title_bar_scene_label.set_hexpand(true);
    title_bar_scene_label.add_css_class("title-bar-hint");

    title_bar.append(&title_bar_left_spacer);
    title_bar.append(&title_bar_label);
    title_bar.append(&title_bar_scene_label);
    title_bar.set_visible(config.title_bar_visible);

    // Search bar floats over the TOP of the card — overlay panel, not in the
    // size-bearing widget chain, so it does not displace content. Width is 3/4
    // of the card; top margin clears the card's top edge (24px card margin + a
    // small inset into the top spacer).
    let search_bar = SearchBar::new();
    search_bar.container.set_width_request(config.column_width as i32 * 3 / 4);
    search_bar.container.set_margin_top(120);
    authorship_picker.overlay.add_overlay(&search_bar.container);

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    vbox.append(&authorship_picker.overlay);

    concordance_bar.container.set_valign(gtk4::Align::End);
    title_bar.set_valign(gtk4::Align::End);
    let outer_overlay = gtk4::Overlay::new();
    outer_overlay.set_child(Some(&vbox));
    outer_overlay.add_overlay(&concordance_bar.container);
    outer_overlay.add_overlay(&title_bar);

    // Suppress startup flicker: hide content until the deferred layout
    // refresh fires (after dwl has tiled the window AND display_work
    // finishes loading). The tick callback below reveals it on the first
    // stable layout. Without this, users see content drawn at GTK's default
    // 1000×800 size, then jump to dwl's tiled size, then jump again as
    // display_work / bottom_clip recompute. Opacity-only so the window
    // chrome (including dwl's tile decoration) appears immediately.
    vbox.set_opacity(0.0);

    window.set_child(Some(&outer_overlay));

    // Work override for hermetic test runs: LIT_START_WORK (preferred) or the
    // legacy LINUX_LIT_WORK both override the saved work from config, so a run is
    // reproducible from env alone without editing config-dev.json.
    let last_work = if let Ok(work_abbrev) = std::env::var("LIT_START_WORK")
        .or_else(|_| std::env::var("LINUX_LIT_WORK"))
    {
        crate::logging::log(&format!(
            "STARTUP: env override work='{}'", work_abbrev
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
        page_back_stack: Vec::new(),
        dim_tag,
        cursor_line_tag,
        cursor_fade_tag,
        ab_dim_tag,
        page_turn_overlay: page_turn_overlay.clone(),
        bottom_clip,
        top_spacer,
        card_vbox,
        scrolled_window: scrolled,
        scrolled_overlay,
        right_view,
        right_scrolled_window: right_scrolled,
        right_scrolled_overlay,
        right_bottom_clip,
        columns_hbox,
        column_divider,
        right_line_number_renderer: None,
        right_gutter_renderer: None,
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
        search_return_pos: None,
        gloss_return_pos: None,
        search_tag,
        search_current_tag,
        current_time_pos: 0.0,
        media_id: None,
        sign_column_visible: Rc::new(Cell::new(true)),
        has_timestamp: Rc::new(RefCell::new(Vec::new())),
        is_manual: Rc::new(RefCell::new(Vec::new())),
        is_chapter_line: Rc::new(RefCell::new(Vec::new())),
        is_bookmarked: Rc::new(RefCell::new(Vec::new())),
        gutter_renderer: None,
        gutter_logical_left: Cell::new(0),
        chunk_renderer: None,
        line_number_renderer: None,
        line_number_renderer_on_left: false,
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
        sign_visible_before_translations: None,
        pre_translation_page: None,
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
        gloss_opened_from_picker: false,
        gloss_prompt_container: None,
        gloss_prompt_overlay: None,
        gloss_prompt_textview: None,
        gloss_prompt_mode: GlossPromptMode::Add,
        delete_confirm_container: None,
        delete_confirm_overlay: None,
        gloss_picker,
        echo_picker,
        echo_turns_picker,
        pending_echo_context: None,
        pending_echo_scene_lines: Vec::new(),
        echo_overlay_links: Vec::new(),
        echo_overlay_index: 0,
        echo_overlay_titles: std::collections::HashMap::new(),
        echo_overlay_source: String::new(),
        echo_overlay_turn_id: None,
        echo_overlay_turn_key: None,
        echo_session: None,
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
        sidebar_mode: SidebarMode::Vocab,
        synopsis_cache: HashMap::new(),
        synopsis_visible: false,
        synopsis_overlay_scene: (0, 0),
        synopsis_amend_scene: (0, 0),
        synopsis_undo: None,
        concordance_picker,
        concordance_state: None,
        concordance_origin: None,
        concordance_word_cache: None,
        concordance_word_picker,
        echo_line_picker,
        echo_keybinds_overlay,
        echo_add_turn_id: None,
        concordance_list_picker,
        concordance_works_picker,
        concordance_bar,
        title_bar,
        title_bar_label,
        title_bar_scene_label,
        current_sentence_group: None,
        current_paragraph_start: None,
        current_sync_scene: None,
        nav_test_active: false,
        nav_test_step: 0,
        nav_test_failures: 0,
        nav_test_prev_top: 0,
        nav_test_expect_return: None,
        nav_test_fuzz: false,
        sync_enabled: true,
        mpv_connected: false,
        mpv_playing: false,
        concordance_resume_playback: false,
        sync_enabled_before_concordance: None,
        skip_mpv_discovery: false,
        debug_icon,
        word_status_label,
        chapter_toast,
        speed_toast,
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
        pending_synopsis: Rc::new(Cell::new(false)),
        pending_top_anchor: Rc::new(Cell::new(false)),
        timestamp_undo: None,
        last_visible_range: std::cell::Cell::new(None),
        page_tops: std::cell::RefCell::new(None),
        keymap: crate::input::keymap_config::Keymap::load(),
        authorship_tag,
        authorship_line_ids: std::collections::HashSet::new(),
        authorship_enabled: true,
        authorship_sets: Vec::new(),
        active_attribution_set_id: None,
        authorship_picker,
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
    // Snap both columns to the current page top before a non-resize-tick reveal
    // so the right column is scrolled to cs.split (not the buffer start). No-op
    // when layout isn't ready yet (snap clamps; the deferred re-scroll inside
    // snap_scroll_to_line corrects post-layout).
    fn reveal_snap(state: &Rc<RefCell<AppState>>) {
        if let Ok(mut s) = state.try_borrow_mut() {
            crate::input::scroll::ensure_scroll_range(&s);
            snap_near_end_to_canonical(&mut s);
            let top = s.page_top_line;
            crate::input::navigation::snap_scroll_to_line(&mut s, top);
        }
    }
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
                    reveal_snap(&state_for_reveal);
                    vbox_for_reveal.set_opacity(1.0);
                } else {
                    crate::logging::log("STARTUP: 500ms grace skipped — work loading; waiting for deferred refresh");
                }
            }
        });
    }
    {
        let vbox_for_fallback = vbox.clone();
        let state_for_fallback = Rc::clone(&state);
        glib::timeout_add_local_once(std::time::Duration::from_secs(5), move || {
            if vbox_for_fallback.opacity() < 1.0 {
                crate::logging::log("STARTUP: revealing vbox (5s fallback — load may be stuck)");
                // Snap both columns to the current page before showing — the
                // resize-tick reveal (which normally does this) never fired, so
                // without it the right column shows the buffer start instead of
                // cs.split.
                reveal_snap(&state_for_fallback);
                vbox_for_fallback.set_opacity(1.0);
            }
        });
    }

    // Headless fuzz: when LIT_NAV_FUZZ=1, auto-start the random nav-test harness
    // a few seconds after launch (once the work has loaded). It runs ~2400
    // randomized jumps and logs NAV_TEST: FAIL on any off-page landing,
    // mis-return, mid-page scene break, underfill, or non-dialogue cursor.
    if std::env::var("LIT_NAV_FUZZ").map(|v| v == "1").unwrap_or(false) {
        let state_for_fuzz = Rc::clone(&state);
        glib::timeout_add_local_once(std::time::Duration::from_secs(6), move || {
            crate::logging::log("NAV_FUZZ: auto-starting fuzz harness");
            crate::input::nav_test::toggle(&state_for_fuzz);
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
                        let cc = s.column_count();
                        let tr = s.translations_visible;
                        apply_card_sizing(&content_hbox_tick, ww, cw, cc, tr);
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
                    let cw = s.config.column_width;
                    let cc = s.column_count();
                    let tr = s.translations_visible;
                    apply_card_sizing(&content_hbox_tick, ww, cw, cc, tr);
                    apply_tiled_mode(&mut s, &vbox_for_tick, ww);
                    // In two-column mode the left/right text_view widths must
                    // have reflowed to their FINAL two-column geometry before
                    // column_split measures line heights — otherwise it measures
                    // against a transitional width (observed left=507/right=506
                    // mid-reflow vs settled left=839/right=756), wraps lines
                    // wrong, and computes too-short splits (columns that fill
                    // then "unfill"). The settled state is when the two views'
                    // widths sum to ~the card width; transitional states sum to
                    // much less. Wait until they do.
                    if cc == 2 {
                        let lw = s.text_view.width();
                        let rw = s.right_view.width();
                        // Columns are now fixed-width and centered as a block
                        // (each column overlay is MIN_TWO_COLUMN_COLUMN_WIDTH),
                        // so they no longer fill the card. "Settled" is when both
                        // views have reflowed to ~their final fixed width rather
                        // than a narrower transitional mid-reflow width. Compare
                        // each view against the fixed column width with slack for
                        // the sign gutter / margins inside the column.
                        // "Settled" means each view has reflowed to ~the fixed
                        // Both columns are the SAME fixed width
                        // (MIN_TWO_COLUMN_COLUMN_WIDTH) when settled, so require
                        // the two view widths to be ~equal AND near that target.
                        // Transitional states fail one or both: mid-reflow after
                        // a work load is too narrow; right after toggling
                        // translations off the left view is wider than the right
                        // (e.g. 788 vs 700) until GTK finishes shrinking it. A
                        // looser magnitude band let that 788 through and
                        // column_split measured wrapping at the wrong width,
                        // underfilling the columns.
                        let lo = (MIN_TWO_COLUMN_COLUMN_WIDTH as f32 * 0.85) as i32;
                        let hi = (MIN_TWO_COLUMN_COLUMN_WIDTH as f32 * 1.20) as i32;
                        let near_target = (lo..=hi).contains(&lw) && (lo..=hi).contains(&rw);
                        let balanced = (lw - rw).abs() <= 8;
                        if !(near_target && balanced) {
                            crate::log_fmt!(
                                "RESIZE_TICK: two-col width not settled (left_w={} right_w={} band={}..={} balanced={}), waiting",
                                lw, rw, lo, hi, balanced
                            );
                            return glib::ControlFlow::Continue;
                        }
                    }
                    crate::log_fmt!("RESIZE_TICK: deferred layout refresh, sw_h={}", sw_h);
                    s.needs_layout_refresh.set(false);
                    do_reveal = vbox_for_tick.opacity() < 1.0;
                } else if width_changed {
                    let cw = s.config.column_width;
                    let cc = s.column_count();
                    let tr = s.translations_visible;
                    apply_card_sizing(&content_hbox_tick, ww, cw, cc, tr);
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
                snap_near_end_to_canonical(&mut s);
                let top = s.page_top_line;
                crate::input::navigation::snap_scroll_to_line(&mut s, top);
                // Reveal LAST: apply_tiled_mode, snap_scroll, and the label
                // update inside snap can all shift visible geometry. Doing
                // them before opacity=1 keeps everything stable when the
                // user first sees the window.
                if do_reveal {
                    crate::log_fmt!("STARTUP: revealing vbox (sw_h={})", s.scrolled_window.height());
                    vbox_for_tick.set_opacity(1.0);
                    let top = s.page_top_line;
                    crate::input::navigation::snap_scroll_to_line(&mut s, top);
                    // Headless UI test harness: emit the reading viewport's
                    // rectangle in window (== screenshot) coordinates so the
                    // line-clipping detector can target it via --region.
                    // sourceview5::View exposes no AT-SPI Text interface, so this
                    // log line is how the harness locates the pane.
                    crate::input::scroll::emit_test_viewport_rect(&s);
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

    // Connect gloss picker search entry filter
    let state_for_gloss_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.gloss_picker.search_entry().connect_changed(move |entry| {
            let text = entry.text();
            state_for_gloss_filter
                .borrow()
                .gloss_picker
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

    // Connect echo line picker search entry for live add-echo search
    let state_for_echo_line = Rc::clone(&state);
    {
        let s = state.borrow();
        s.echo_line_picker.entry().connect_changed(move |_| {
            crate::input::actions::echoes::refresh_add_echo_search(&state_for_echo_line);
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

/// Strip variant suffixes (-Amb, -BBC, -Ep-N) to get base work abbreviation
/// for shared data like synopses.
pub fn base_work_abbrev(abbrev: &str) -> &str {
    if let Some(pos) = abbrev.find('-') {
        &abbrev[..pos]
    } else {
        abbrev
    }
}

/// After layout settles (`text_view.height() > 0`), correct a near-end
/// `page_top` that `display_work` could only guess (it runs before layout, so it
/// uses a rough `current_line - lpp` heuristic that lands on a non-canonical,
/// underfilled final spread). Snap to the CANONICAL final spread — the same page
/// `G`/forward-paging use — and put the cursor on its last visible dialogue line.
/// No-op when not near the end, single-column, or layout isn't ready.
fn snap_near_end_to_canonical(s: &mut AppState) {
    let line_count = s.effective_line_count();
    if s.column_count() != 2 || line_count == 0 || s.text_view.height() <= 0 {
        return;
    }
    // Trigger when the current PAGE is in the work's final region — i.e. the
    // page_top is within one spread of the end. (Checking `current_line` is
    // wrong: the saved cursor can sit a column or two before the end yet still be
    // on the final spread, e.g. current_line=4295, page_top=4294 with the canonical
    // final spread at 4297.)
    let lpp = crate::input::viewport::lines_per_page(s);
    if s.page_top_line + lpp * 2 < line_count {
        return;
    }
    // Anchor on the work's last dialogue line (not the saved cursor, which may be
    // stale) so `last_page_top` computes the true final spread.
    let mut target = line_count - 1;
    while target > 0
        && (s.translation_lines.get(target).copied().unwrap_or(false)
            || !crate::input::viewport::is_dialogue_line(&s.buffer, target))
    {
        target -= 1;
    }
    let canonical = crate::input::navigation::last_page_top(s, target);
    if canonical == s.page_top_line {
        return;
    }
    let cs = crate::input::viewport::column_split(s, canonical);
    let cursor = crate::input::viewport::prev_dialogue_line(
        &s.buffer, &s.translation_lines, cs.page_end + 1,
    )
    .filter(|&d| d >= canonical && d <= cs.page_end)
    .unwrap_or(s.current_line.min(cs.page_end));
    crate::logging::log(&format!(
        "STARTUP: snap near-end page_top {} -> canonical {} (cursor {})",
        s.page_top_line, canonical, cursor
    ));
    s.page_top_line = canonical;
    s.current_line = cursor;
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
            let _ = crate::db::queries::ensure_echo_tables(&conn);
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
    state.current_sync_scene = None;
    state.media_id = work.media_id;
    state
        .window
        .set_title(Some(&format!("{} — linux-lit", work.title)));
    state.title_bar_label.set_text(&format!("{}, {}", work.author, work.title));
    state.title_bar_scene_label.set_text("");
    if state.concordance_state.is_none() {
        state.title_bar.set_visible(state.config.title_bar_visible);
    }

    // Save MRU to config; track previous work for toggle.
    // LIT_START_POS overrides the saved start line for hermetic test runs.
    let saved_line = std::env::var("LIT_START_POS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or_else(|| state.config.work_positions.get(&work.abbrev).copied().unwrap_or(0));
    if state.config.last_work.as_deref() != Some(&work.abbrev) {
        state.config.previous_work = state.config.last_work.take();
    }
    state.config.last_work = Some(work.abbrev.clone());
    state.config.push_recent_work(&work.abbrev);
    crate::config::save(&state.config);

    // Send timestamp data to MPV client (filtered by active media_id)
    {
        let active_media_id = state.media_id;
        let dialogue_ids: std::collections::HashSet<i64> = work
            .lines
            .iter()
            .filter(|l| l.is_dialogue)
            .map(|l| l.id)
            .collect();
        let mut ts_data: Vec<(i64, f64, f64)> = work
            .timestamps
            .iter()
            .filter(|t| {
                active_media_id.map_or(true, |mid| t.media_id == mid)
                    && dialogue_ids.contains(&t.line_id)
            })
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

    // Find or launch MPV socket — reuse existing connection via loadfile when possible
    // Skip when caller will open the media picker instead (e.g. concordance cross-work jump).
    if state.skip_mpv_discovery {
        state.skip_mpv_discovery = false;
    } else if !work.media_paths.is_empty() {
        let media_paths = work.media_paths.clone();
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
        let already_connected = state.mpv_connected;
        let primary_media = media_paths[0].clone();
        glib::spawn_future_local(async move {
            if already_connected {
                // Reuse existing MPV — load the new file in-place
                crate::logging::log(&format!(
                    "MPV: reusing connection, loadfile '{}'", primary_media
                ));
                let _ = cmd_tx
                    .send(crate::mpv::MpvCommand::LoadFile(primary_media.clone()))
                    .await;
                let _ = cmd_tx
                    .send(crate::mpv::MpvCommand::Pause)
                    .await;
            } else {
                // No connection — discover or launch
                let media_paths_for_discover = media_paths.clone();
                let (socket_path, matched_media_path) = handle
                    .spawn_blocking(move || {
                        if let Some((sock, matched)) =
                            crate::mpv::discovery::find_socket_for_work(&media_paths_for_discover)
                        {
                            return (sock.to_string_lossy().to_string(), Some(matched));
                        }
                        let launched = crate::mpv::discovery::launch_mpv(&media_paths_for_discover[0]);
                        for _ in 0..60 {
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            if std::path::Path::new(&launched).exists() {
                                return (launched, Some(media_paths_for_discover[0].clone()));
                            }
                        }
                        (launched, Some(media_paths_for_discover[0].clone()))
                    })
                    .await
                    .unwrap_or_default();

                if let Some(ref matched_path) = matched_media_path {
                    let matched_mid = path_to_mid.get(matched_path).copied();
                    if matched_mid.is_some() && matched_mid != default_media_id {
                        let mid = matched_mid.unwrap();
                        crate::logging::log(&format!(
                            "MPV discovery: switching active media_id from {:?} to {} for {}",
                            default_media_id, mid, matched_path
                        ));
                        let dialogue_ids: std::collections::HashSet<i64> = lines
                            .iter()
                            .filter(|l| l.is_dialogue)
                            .map(|l| l.id)
                            .collect();
                        let mut ts_data: Vec<(i64, f64, f64)> = timestamps
                            .iter()
                            .filter(|t| t.media_id == mid && dialogue_ids.contains(&t.line_id))
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
            }
        });
    }

    state.current_line = saved_line;
    state.page_top_line = 0;
    state.page_back_stack.clear();
    state.last_visible_range.set(None);
    *state.page_tops.borrow_mut() = None;
    state.visual_selection = None;
    state.current_work = Some(work);

    // Build buffer text (with or without sign column)
    state.line_map = None;
    state.dialogue_formatting_active = false;
    state.authorship_line_ids.clear();
    state.authorship_sets.clear();
    state.active_attribution_set_id = None;
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
    // Keep the right column's line spacing in sync with the left (both views
    // share the buffer but have independent pixels_above/below settings).
    state.right_view.set_pixels_above_lines(ls);
    state.right_view.set_pixels_below_lines(ls);
    // Show or hide the right column to match this work's resolved column count
    // (Shakespeare plays default to two columns; a per-work Alt+[ override wins).
    let two_col = state.column_count() == 2;
    state.right_scrolled_overlay.set_visible(two_col);
    state.column_divider.set_visible(two_col);
    if !two_col {
        state.right_bottom_clip.set_height_request(0);
    }
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
    // Load scene synopses for this work
    if let Some(ref work) = state.current_work {
        if let Ok(conn) = crate::db::queries::open_db() {
            let base_abbrev = base_work_abbrev(&work.abbrev);
            state.synopsis_cache = crate::db::queries::load_synopses(&conn, base_abbrev);
            crate::logging::log(&format!(
                "SYNOPSIS: loaded {} scene synopses for {}",
                state.synopsis_cache.len(),
                base_abbrev,
            ));
        }
    }
    state.sidebar_mode = SidebarMode::Vocab;
    state.synopsis_visible = false;
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

    let t_auth = std::time::Instant::now();
    if let Some(ref work) = state.current_work {
        if let Ok(conn) = crate::db::queries::open_db() {
            state.authorship_sets = crate::db::authorship::load_attribution_sets(&conn, &work.abbrev)
                .unwrap_or_default();
            if let Some(first) = state.authorship_sets.first() {
                state.active_attribution_set_id = Some(first.id);
                state.authorship_line_ids = crate::db::authorship::load_secondary_line_ids(
                    &conn, first.id, &work.abbrev,
                ).unwrap_or_default();
            } else {
                state.active_attribution_set_id = None;
                state.authorship_line_ids.clear();
            }
        }
    }
    apply_authorship_formatting(state);
    crate::logging::log(&format!("TIMING: apply_authorship_formatting {:.0}ms", t_auth.elapsed().as_millis()));

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
        if state.line_number_renderer_on_left {
            crate::gutter::remove_line_number_renderer_left(&state.text_view, &old_renderer);
        } else {
            crate::gutter::remove_line_number_renderer(&state.text_view, &old_renderer);
        }
        let right_margin = state.config.text_margins as i32 + crate::config::EXTRA_RIGHT_MARGIN;
        state.text_view.set_right_margin(right_margin);
    }
    state.line_number_renderer_on_left = false;
    if let Some(old_renderer) = state.right_line_number_renderer.take() {
        crate::gutter::remove_line_number_renderer(&state.right_view, &old_renderer);
    }
    if let Some(old_renderer) = state.right_gutter_renderer.take() {
        crate::gutter::remove_gutter_renderer(&state.right_view, &old_renderer);
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
        // Line numbers are skipped entirely in two-column mode unless explicitly
        // enabled, reclaiming the gutter space for text.
        let show_numbers = state.column_count() != 2 || SHOW_LINE_NUMBERS_TWO_COL;
        if !is_prose && show_numbers {
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
            let (ln_width, ln_margin, ln_gap) = line_number_gutter_geometry(state.column_count());
            let two_col = state.column_count() == 2;
            // Left column: in two-column mode the numbers sit in the LEFT gutter
            // (outer, book-style), so the text's right side stays tight against
            // the divider. In one-column mode they keep the default right gutter.
            let renderer = if two_col {
                let r = crate::gutter::setup_line_number_gutter_left(
                    &state.text_view,
                    state.line_numbers.clone(),
                    &state.theme.dim_fg,
                    &state.config.font_family,
                    state.config.font_size,
                    ln_width,
                    crate::gutter::LINE_NUMBER_LEFT_GAP_TWO_COL,
                );
                state.text_view.set_right_margin(ln_gap);
                state.line_number_renderer_on_left = true;
                r
            } else {
                let r = crate::gutter::setup_line_number_gutter(
                    &state.text_view,
                    state.line_numbers.clone(),
                    &state.theme.dim_fg,
                    &state.config.font_family,
                    state.config.font_size,
                    ln_width,
                    ln_margin,
                );
                state.text_view.set_right_margin(ln_gap);
                state.line_number_renderer_on_left = false;
                r
            };
            state.line_number_renderer = Some(renderer);
            let right_renderer = crate::gutter::setup_line_number_gutter(
                &state.right_view,
                state.line_numbers.clone(),
                &state.theme.dim_fg,
                &state.config.font_family,
                state.config.font_size,
                ln_width,
                ln_margin,
            );
            state.right_view.set_right_margin(ln_gap);
            state.right_line_number_renderer = Some(right_renderer);
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

    // Apply font tag to new buffer content (uses the configured/saved size —
    // do NOT override it here, or in-app !/| adjustments won't stick and the
    // saved size won't survive a work load).
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
            state.page_top_line = 0;
            // First open with no saved position: keep the opening Act/Prologue
            // header pinned to the top of the page. Without this, the page_top==0
            // guard in update_highlight_and_show would scroll down to the first
            // dialogue line and hide the header.
            state.pending_top_anchor.set(true);
        }
    } else if target_line_id.is_none() {
        // Snap saved cursor to nearest dialogue line if it landed on
        // non-dialogue (speaker, stage direction, blank, marker).
        let line_count = state.effective_line_count();
        if state.current_line < line_count
            && !crate::input::viewport::is_dialogue_line(&state.buffer, state.current_line)
        {
            let forward = crate::input::viewport::next_dialogue_line(
                &state.buffer, &state.translation_lines,
                state.current_line, line_count,
            );
            let backward = if state.current_line > 0 {
                (0..state.current_line).rev().find(|&i| {
                    crate::input::viewport::is_dialogue_line(&state.buffer, i)
                })
            } else {
                None
            };
            state.current_line = forward.or(backward).unwrap_or(state.current_line);
        }

        // Saved position path: anchor page_top so the cursor is visible.
        // If the cursor is near the end of the buffer, back up by ~1 page
        // so the viewport fills instead of showing only the trailing lines.
        let lpp = crate::input::viewport::lines_per_page(state);
        let near_end = state.current_line + lpp >= line_count;
        let page_top = if near_end && state.text_view.height() > 0 {
            // Near the end with layout ready: open on the CANONICAL final spread
            // (the same page G and forward-paging land on — tail in the right
            // column, both columns full) instead of a rough `current_line - lpp`
            // guess that renders a non-canonical mid-page spread.
            crate::input::navigation::last_page_top(state, state.current_line)
        } else if near_end {
            state.current_line.saturating_sub(lpp)
        } else {
            state.current_line.saturating_sub(1)
        };
        state.page_top_line = page_top;

        // If cursor is on a scene boundary, scroll back to show the
        // scene/act heading lines above the first dialogue line.
        if !state.synopsis_cache.is_empty() && is_first_line_of_scene(state) {
            let top = scene_heading_start(state, state.current_line);
            if top < state.page_top_line {
                state.page_top_line = top;
            }
        }

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
                    let bi = lm.work_to_buffer[work_idx];
                    if lm.buffer_to_work.get(bi) == Some(&Some(work_idx)) {
                        bi
                    } else {
                        state.current_line
                    }
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
/// Clean raw source `.txt` lines for display: drop blank lines that precede a
/// speaker name, strip the `## ` markdown act/scene prefix, and fold multi-line
/// stage directions into a single line so they soft-wrap instead of keeping the
/// Folger source's mid-sentence hard breaks. Shared by `prepare_text_only` and
/// `prepare_text_for_display` so both produce identical buffer text.
fn clean_file_lines(file_lines: &[String]) -> Vec<String> {
    let mut result: Vec<String> = Vec::with_capacity(file_lines.len());
    let mut i = 0;
    while i < file_lines.len() {
        let line = &file_lines[i];
        if crate::db::line_types::is_blank(line) {
            let next_non_blank = file_lines[i + 1..]
                .iter()
                .find(|l| !crate::db::line_types::is_blank(l));
            if let Some(next) = next_non_blank {
                if crate::db::line_types::is_speaker(next) {
                    i += 1;
                    continue;
                }
            }
        }

        // Multi-line stage direction: the Folger source hard-wraps a single
        // bracketed direction across several lines (opens with `[`, no closing
        // `]`). Fold those source lines into one buffer line so GTK soft-wraps
        // the direction naturally instead of preserving the mid-sentence breaks.
        // Stage directions normalize to empty in the line map, so folding them
        // doesn't disturb work-line mapping.
        let trimmed = line.trim();
        if trimmed.starts_with('[') && !trimmed.ends_with(']') {
            let mut joined = line.clone();
            let mut j = i + 1;
            let mut closed = false;
            while j < file_lines.len() {
                let cont = file_lines[j].trim();
                if cont.is_empty() {
                    break; // malformed (no closing bracket before a blank) — stop
                }
                joined.push(' ');
                joined.push_str(cont);
                let ends_here = cont.ends_with(']');
                j += 1;
                if ends_here {
                    closed = true;
                    break;
                }
            }
            if closed {
                result.push(joined);
                i = j;
                continue;
            }
        }

        if let Some(stripped) = line.strip_prefix("## ") {
            result.push(stripped.to_string());
        } else {
            result.push(line.clone());
        }
        i += 1;
    }
    result
}

pub fn prepare_text_only(work: &Work) -> Option<PreparedTextOnly> {
    let path = work.text_file.as_ref()?;
    let t_read = std::time::Instant::now();
    let contents = std::fs::read_to_string(path).ok()?;
    let file_lines: Vec<String> = contents.lines().map(String::from).collect();
    crate::logging::log(&format!("PREP: read+split {}ms", t_read.elapsed().as_millis()));

    let t_clean = std::time::Instant::now();
    let cleaned_lines = clean_file_lines(&file_lines);
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
    let cleaned_lines = clean_file_lines(&file_lines);
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
    // get an additional indent via the per-tag margin below. In two-column
    // mode the columns are narrow, so the full +60 monocle indent pushes
    // verse lines past the column edge and they wrap; use a smaller indent
    // there so a 63-char verse line still clears the column width.
    let base_margin = state.text_view.left_margin();
    let dialogue_indent = if state.column_count() == 2 {
        TWO_COLUMN_DIALOGUE_INDENT
    } else {
        DIALOGUE_INDENT
    };

    let indent_tag = gtk4::TextTag::builder()
        .name("dialogue-indent")
        .left_margin(base_margin + dialogue_indent)
        .build();

    let speaker_gap_tag = gtk4::TextTag::builder()
        .name("speaker-gap")
        .pixels_above_lines(8)
        .build();

    let stage_gap_tag = gtk4::TextTag::builder()
        .name("stage-direction-gap")
        .pixels_above_lines(8)
        .build();

    // Speaker names stay flush-left at the text margin, aligned with stage
    // directions, so dialogue hangs-indents beneath them (standard
    // modern-edition look). Dialogue gets the +60 indent via dialogue-indent.
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

    // Apply tags per line. `in_stage_direction` tracks a multi-line stage
    // direction (one that spans several source lines, e.g. "[Enter Lucius,…\n
    // Guards, and an Attendant…]"). Its middle continuation lines start without
    // `[` and end without `]`, so `is_stage_direction` can't recognize them in
    // isolation — without this flag they'd fall through to the dialogue branch
    // and lose the italic styling, leaving the continuation mis-formatted.
    let mut in_stage_direction = false;
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
        let trimmed = text.trim();

        if line_types::is_blank(text) {
            in_stage_direction = false;
            state.buffer.apply_tag(&blank_line_tag, &line_start, &line_end);
        } else if in_stage_direction {
            // Continuation (or closing) line of a multi-line stage direction:
            // same indent + italic as the opening line, but no extra gap above.
            state.buffer.apply_tag(&indent_tag, &line_start, &line_end);
            state.buffer.apply_tag(&stage_italic_tag, &line_start, &line_end);
            // A line that closes the bracket ends the block.
            if trimmed.ends_with(']') {
                in_stage_direction = false;
            }
        } else if line_types::is_act_scene_marker(text) || line_types::is_separator(text) {
            // Check headings BEFORE is_speaker: standalone markers like EPILOGUE,
            // PROLOGUE, CHORUS, INDUCTION are all-caps and would otherwise match
            // is_speaker first and render in the smaller small-caps speaker style
            // instead of the bold ACT/SCENE heading style.
            state.buffer.apply_tag(&act_scene_tag, &line_start, &line_end);
        } else if line_types::is_speaker(text) {
            state.buffer.apply_tag(&speaker_gap_tag, &line_start, &line_end);
            state.buffer.apply_tag(&speaker_name_tag, &line_start, &line_end);
        } else if line_types::is_stage_direction(text) {
            state.buffer.apply_tag(&stage_gap_tag, &line_start, &line_end);
            state.buffer.apply_tag(&indent_tag, &line_start, &line_end);
            state.buffer.apply_tag(&stage_italic_tag, &line_start, &line_end);
            // Opening line of a multi-line stage direction: `[` with no closing
            // `]`. Carry the styling forward until the closing bracket.
            if trimmed.starts_with('[') && !trimmed.ends_with(']') {
                in_stage_direction = true;
            }
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

pub fn apply_authorship_formatting(state: &mut AppState) {
    let tag_table = state.buffer.tag_table();
    if let Some(old) = tag_table.lookup("authorship-italic") {
        let (start, end) = state.buffer.bounds();
        state.buffer.remove_tag(&old, &start, &end);
    }

    if !state.authorship_enabled || state.authorship_line_ids.is_empty() {
        return;
    }

    let line_count = state.buffer.line_count() as usize;
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return,
    };

    for buf_line in 0..line_count {
        let work_idx = if let Some(ref lm) = state.line_map {
            match lm.buffer_to_work.get(buf_line).and_then(|o| *o) {
                Some(wi) => wi,
                None => continue,
            }
        } else {
            buf_line
        };

        let line = match work.lines.get(work_idx) {
            Some(l) => l,
            None => continue,
        };

        if state.authorship_line_ids.contains(&line.id) {
            let line_start = match state.buffer.iter_at_line(buf_line as i32) {
                Some(it) => it,
                None => continue,
            };
            let line_end = if buf_line + 1 < line_count {
                match state.buffer.iter_at_line((buf_line + 1) as i32) {
                    Some(it) => it,
                    None => {
                        let (_, e) = state.buffer.bounds();
                        e
                    }
                }
            } else {
                let (_, e) = state.buffer.bounds();
                e
            };
            state.buffer.apply_tag(&state.authorship_tag, &line_start, &line_end);
        }
    }
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
    // In two-column verse mode the left column's line numbers occupy the
    // outermost slice of the left gutter (book foliation). Reserve that slice so
    // the sign column carves only the remaining margin and doesn't overlap them.
    // Derived from current geometry (not the persisted flag) so it is correct on
    // the first layout pass, before the line-number block runs.
    let two_col_verse = state.column_count() == 2
        && SHOW_LINE_NUMBERS_TWO_COL
        && state.current_work.as_ref()
            .map(|w| !crate::db::line_types::is_prose_work(&w.work_type))
            .unwrap_or(false);
    let left_number_allowance = if two_col_verse {
        crate::gutter::LINE_NUMBER_WIDTH_TWO_COL + crate::gutter::LINE_NUMBER_LEFT_GAP_TWO_COL
    } else {
        0
    };
    let gutter_width = (left_margin - left_number_allowance - 20).max(10);
    let renderer = crate::gutter::setup_timestamp_gutter(
        &state.text_view,
        state.sign_column_visible.clone(),
        state.has_timestamp.clone(),
        state.is_manual.clone(),
        state.is_chapter_line.clone(),
        state.is_bookmarked.clone(),
        state.ab_a_line.clone(),
        state.ab_b_line.clone(),
        left_margin - left_number_allowance,
        &state.theme.dim_fg,
        // Sign column sits at position 1 (just left of text) when the left
        // column also shows outer line numbers at position 0; otherwise 0.
        if left_number_allowance > 0 { 1 } else { 0 },
    );
    // Reduce left margin so the gutter absorbs the space instead of pushing text.
    // The number renderer (when present) lives in the gutter window too and adds
    // its own width, so strip the allowance from the margin to keep text put.
    state.text_view.set_left_margin(left_margin - gutter_width - left_number_allowance);
    // Also adjust dialogue-indent tag so dialogue lines don't shift right.
    // Compute the target absolutely from the captured pre-reduction margin
    // rather than subtracting from the tag's current value: setup_gutter can
    // run repeatedly without an intervening apply_dialogue_formatting rebuild,
    // and a relative `old_margin - gutter_width` would compound on each pass,
    // eventually driving the tag's left-margin negative (GTK rejects a negative
    // left-margin and panics). The full (un-reduced) tag margin is
    // `left_margin + dialogue_indent`; the gutter-reduced target is that minus
    // gutter_width. Clamp at 0 for safety.
    let dialogue_indent = if state.column_count() == 2 {
        TWO_COLUMN_DIALOGUE_INDENT
    } else {
        DIALOGUE_INDENT
    };
    if let Some(buffer) = state.text_view.buffer().downcast_ref::<gtk4::TextBuffer>() {
        if let Some(tag) = buffer.tag_table().lookup("dialogue-indent") {
            let target = (left_margin + dialogue_indent - gutter_width).max(0);
            tag.set_left_margin(target);
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

    // Right-column sign gutter (two-column mode only). The right view shares
    // the same buffer, so the same per-line flags drive its signs. Mirror the
    // left column's geometry: give the right view a logical left margin, let a
    // gutter of `gutter_width` absorb it so the signs sit just left of the
    // text. The shared dialogue-indent tag was already reduced above by the
    // same gutter_width, so the right column's dialogue lines line up too.
    if let Some(old_renderer) = state.right_gutter_renderer.take() {
        crate::gutter::remove_gutter_renderer(&state.right_view, &old_renderer);
    }
    if state.column_count() == 2 {
        // The right column keeps the SAME internal geometry as the left (so the
        // shared dialogue-indent tag stays valid); it is shifted toward the
        // divider via its container's margins instead (see apply_tiled_mode).
        let right_left_margin = left_margin - left_number_allowance;
        state.right_view.set_left_margin(right_left_margin);
        let right_renderer = crate::gutter::setup_timestamp_gutter(
            &state.right_view,
            state.sign_column_visible.clone(),
            state.has_timestamp.clone(),
            state.is_manual.clone(),
            state.is_chapter_line.clone(),
            state.is_bookmarked.clone(),
            state.ab_a_line.clone(),
            state.ab_b_line.clone(),
            right_left_margin,
            &state.theme.dim_fg,
            0,
        );
        state.right_view.set_left_margin(right_left_margin - gutter_width);
        state.right_gutter_renderer = Some(right_renderer);
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

    state.card_vbox.set_opacity(0.0);

    // Capture the cursor's on-screen y-position BEFORE mutating the buffer.
    // The cursor is the user's visual anchor — keep it at the same screen
    // position after inserts so the viewport does not appear to scroll.
    let pre_adj_value = state.scrolled_window.vadjustment().value();
    let pre_adj_upper = state.scrolled_window.vadjustment().upper();
    let pre_adj_page = state.scrolled_window.vadjustment().page_size();
    let cursor_screen_y = state
        .buffer
        .iter_at_line(state.current_line as i32)
        .map(|iter| {
            let (y, h) = state.text_view.line_yrange(&iter);
            crate::logging::log(&format!(
                "TRANSLATIONS_SHOW: pre-insert cursor yrange y={} h={} adj_val={} screen_y={}",
                y, h, pre_adj_value as i64, (y as f64 - pre_adj_value) as i64,
            ));
            y as f64 - pre_adj_value
        });
    crate::logging::log(&format!(
        "TRANSLATIONS_SHOW: pre-insert adj val={} upper={} page={} current_line={} page_top={}",
        pre_adj_value as i64, pre_adj_upper as i64, pre_adj_page as i64,
        state.current_line, state.page_top_line,
    ));

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

    // Configure the translation gloss tag: Charter Italic at 4pt below the
    // independent translation font size (not the two-column reader size).
    let trans_size = state.config.translation_font_size.saturating_sub(4);
    let desc = pango::FontDescription::from_string(
        &format!("Charter Italic {}", trans_size),
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
    // Save the pre-toggle reader position so hide can restore it exactly.
    state.pre_translation_page = Some((old_current, old_top));
    state.current_line = map_line_after_insert(state.current_line, &inserts);
    state.page_top_line = map_line_after_insert(state.page_top_line, &inserts);

    let cursor_on_translation = state.current_line < state.translation_lines.len()
        && state.translation_lines[state.current_line];
    crate::logging::log(&format!(
        "TRANSLATIONS_SHOW: line remap current {}→{} page_top {}→{} (inserts={}) cursor_on_translation={}",
        old_current, state.current_line, old_top, state.page_top_line, inserts.len(),
        cursor_on_translation,
    ));

    state.translations_visible = true;

    // Hide the sign column while translations show — the interleaved
    // translation lines make per-line signs misleading. Remember the prior
    // visibility so hide_translations can restore it.
    if state.sign_visible_before_translations.is_none() {
        state.sign_visible_before_translations = Some(state.sign_column_visible.get());
    }
    state.sign_column_visible.set(false);
    crate::input::timestamps::redraw_sign_gutters(state);

    // Translations force a single column (column_count() now returns 1 because
    // translations_visible is set). Reconfigure the layout to hide the right
    // column and widen the card before anchoring the viewport below.
    apply_column_layout(state);

    reapply_font(state);
    crate::input::navigation::invalidate_page_tops(state);
    // The buffer's translation lines were just inserted/removed, so every cached
    // line index is stale. Drop the last-visible-range cache — otherwise
    // is_line_fully_visible compares the cursor against the old line numbers,
    // never fires a page turn, and the view scrolls off a line boundary
    // (clipping top and bottom).
    state.last_visible_range.set(None);

    let mid_adj = state.scrolled_window.vadjustment();
    crate::logging::log(&format!(
        "TRANSLATIONS_SHOW: post-reapply_font adj val={} upper={} page={}",
        mid_adj.value() as i64, mid_adj.upper() as i64, mid_adj.page_size() as i64,
    ));

    // Repaint the cursor highlight but do NOT page-turn.
    crate::input::navigation::update_highlight_only(state);

    // Defer viewport anchor to an idle callback — GTK hasn't re-laid the
    // buffer yet so line_yrange and adjustment.upper are stale right now.
    //
    // In e-reader mode the page top is a fixed line boundary, so anchor the
    // viewport to page_top_line's EXACT pixel top (a whole-line edge) rather
    // than to the cursor's screen-y. Anchoring to the cursor after a 3000-line
    // insert leaves the scroll between line boundaries, which clips the top
    // line and throws off the bottom-clip computation. Snapping to the line top
    // is the same thing snap_scroll_to_line does for normal page turns; the
    // deferred refresh_bottom_clip below then reads this aligned scroll value
    // and covers the partial bottom line correctly. (See the anti-clipping
    // note in docs/troubleshooting/page-turning-mechanics.md.)
    let top_line = state.page_top_line;
    let _ = cursor_screen_y; // no longer used for anchoring
    let tv = state.text_view.clone();
    let sw = state.scrolled_window.clone();
    let bc = state.bottom_clip.clone();
    let vbox = state.card_vbox.clone();
    gtk4::glib::idle_add_local_once(move || {
        let adj = sw.vadjustment();
        let top_y = tv.buffer().iter_at_line(top_line as i32).map(|iter| {
            let (y, h) = tv.line_yrange(&iter);
            crate::logging::log(&format!(
                "TRANSLATIONS_SHOW: idle snap page_top yrange y={} h={} line={}",
                y, h, top_line,
            ));
            y as f64
        });
        if let Some(y) = top_y {
            let max_val = (adj.upper() - adj.page_size()).max(0.0);
            let val = y.clamp(0.0, max_val);
            crate::logging::log(&format!(
                "TRANSLATIONS_SHOW: idle snap to page_top y={} clamped={} upper={} page={}",
                y as i64, val as i64, adj.upper() as i64, adj.page_size() as i64,
            ));
            adj.set_value(val);
            // Cover the partial line at the bottom edge immediately on reveal —
            // the paged refresh_bottom_clip is page_top-relative and unreliable
            // here, so use the same scroll-aware clip the j/k path uses.
            crate::input::scroll::scrolloff_bottom_clip_widgets(&tv, &sw, &bc, val);
        }
        vbox.set_opacity(1.0);
    });

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
        "TRANSLATIONS_SHOW: FINAL inserted={} buf_lines {}->{} current {}->{} page_top {}->{} line_map_len={} stale={} adj {}->{} effective_line_count={}",
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
        state.effective_line_count(),
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

/// Strip translation lines from the buffer without repositioning the viewport.
/// Caller is responsible for scrolling/page-setting after this returns.
pub fn hide_translations_for_navigation(state: &mut AppState) {
    if !state.translations_visible {
        return;
    }
    strip_translation_lines(state);
}

fn hide_translations(state: &mut AppState) {
    state.card_vbox.set_opacity(0.0);

    // Capture the pre-toggle page BEFORE strip_translation_lines clears it.
    // These are pre-insert line indices, valid again after the strip restores
    // the original buffer numbering.
    let saved_pre_toggle = state.pre_translation_page.take();

    // Capture the cursor's on-screen y-position BEFORE removing lines so we
    // can restore it afterwards — the cursor is the user's visual anchor.
    let pre_adj_value = state.scrolled_window.vadjustment().value();
    let cursor_screen_y = state
        .buffer
        .iter_at_line(state.current_line as i32)
        .map(|iter| {
            let (y, h) = state.text_view.line_yrange(&iter);
            crate::logging::log(&format!(
                "TRANSLATIONS_HIDE: pre-remove cursor yrange y={} h={} adj_val={} screen_y={}",
                y, h, pre_adj_value as i64, (y as f64 - pre_adj_value) as i64,
            ));
            y as f64 - pre_adj_value
        });

    strip_translation_lines(state);

    // Repaint highlight but do NOT page-turn.
    crate::input::navigation::update_highlight_only(state);

    if state.column_count() == 2 {
        // Two-column work: translations were forcing a single column. Restore
        // the layout + page-position state, then defer the ENTIRE re-snap to
        // RESIZE_TICK. Do NOT call set_page_instant here: the left view still
        // has its single-column (over-wide, ~1408px) width, so column_split
        // would scroll the right view to a wrong split; the log showed that
        // pollutes the subsequent settled-width resnap (page_end 4215 vs the
        // correct 4219). The tick waits for the widths to settle (band check)
        // and produces the one correct resnap.
        apply_column_layout(state);
        let (cur, top) = saved_pre_toggle
            .unwrap_or((state.current_line, state.page_top_line));
        state.current_line = cur;
        state.page_top_line = top;
        crate::input::navigation::update_highlight_only(state);
        rebuild_line_number_gutter(state);
        state.needs_layout_refresh.set(true);
        state.card_vbox.set_opacity(1.0);
        return;
    }

    // Defer viewport anchor to an idle callback — GTK hasn't re-laid the
    // buffer yet so line_yrange and adjustment.upper are stale right now.
    let cursor_line = state.current_line;
    let screen_y = cursor_screen_y;
    let tv = state.text_view.clone();
    let sw = state.scrolled_window.clone();
    let vbox = state.card_vbox.clone();
    gtk4::glib::idle_add_local_once(move || {
        let adj = sw.vadjustment();
        let cur_y = tv.buffer().iter_at_line(cursor_line as i32).map(|iter| {
            let (y, h) = tv.line_yrange(&iter);
            crate::logging::log(&format!(
                "TRANSLATIONS_HIDE: idle anchor cursor yrange y={} h={} line={}",
                y, h, cursor_line,
            ));
            y as f64
        });
        let adj_upper = adj.upper();
        let adj_page = adj.page_size();
        let new_adj = match (cur_y, screen_y) {
            (Some(y), Some(sy)) => Some((y - sy).max(0.0).min((adj_upper - adj_page).max(0.0))),
            _ => None,
        };
        crate::logging::log(&format!(
            "TRANSLATIONS_HIDE: idle anchor cur_y={:?} screen_y={:?} upper={} page={} new_adj={:?}",
            cur_y.map(|v| v as i64), screen_y.map(|v| v as i64),
            adj_upper as i64, adj_page as i64, new_adj.map(|v| v as i64),
        ));
        if let Some(val) = new_adj {
            adj.set_value(val);
        }
        vbox.set_opacity(1.0);
    });

    crate::input::navigation::refresh_bottom_clip(state);
    rebuild_line_number_gutter(state);
}

fn strip_translation_lines(state: &mut AppState) {
    let line_count = state.buffer.line_count() as usize;
    let pre_hide_buf_lines = line_count;

    // Remove translation lines from buffer bottom-to-top
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

    crate::logging::log(&format!(
        "TRANSLATIONS_HIDE: line remap current {}→{} page_top {}→{} buf_lines {}→{}",
        old_current, state.current_line, old_top, state.page_top_line,
        pre_hide_buf_lines, state.buffer.line_count(),
    ));

    state.translation_lines.clear();
    state.translations_visible = false;

    // Clear the saved pre-toggle page (covers navigation-driven hide and
    // single-column paths) so it does not leak into a later toggle.
    state.pre_translation_page = None;

    // Restore the sign column to its pre-translation visibility.
    if let Some(prev) = state.sign_visible_before_translations.take() {
        state.sign_column_visible.set(prev);
        crate::input::timestamps::redraw_sign_gutters(state);
    }

    reapply_font(state);
    crate::input::navigation::invalidate_page_tops(state);
    // The buffer's translation lines were just inserted/removed, so every cached
    // line index is stale. Drop the last-visible-range cache — otherwise
    // is_line_fully_visible compares the cursor against the old line numbers,
    // never fires a page turn, and the view scrolls off a line boundary
    // (clipping top and bottom).
    state.last_visible_range.set(None);
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
    // The single-column translation view uses its own Charter font at an
    // independent size, so adjusting it never changes the two-column reader
    // font. Otherwise use the configured reader family/size.
    let (font_family, font_size): (&str, u32) = if state.translations_visible {
        ("Charter", state.config.translation_font_size)
    } else {
        (state.config.font_family.as_str(), state.config.font_size)
    };
    let font_str = format!("{} {}", font_family, font_size);
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
    // Keep translation tag in sync (italic, 4pt smaller) and ensure it
    // overrides the freshly re-added font-size tag.
    let trans_size = font_size.saturating_sub(4);
    let trans_desc = pango::FontDescription::from_string(
        &format!("{} Italic {}", font_family, trans_size),
    );
    state.translation_text_tag.set_font_desc(Some(&trans_desc));
    let highest = state.buffer.tag_table().size() - 1;
    state.translation_text_tag.set_priority(highest);
    state.authorship_tag.set_priority(highest.saturating_sub(1).max(1));
    crate::logging::log(&format!("FONT: reapply_font size={}pt via TextTag", state.config.font_size));
    update_spacer_heights(state);
}

fn rebuild_line_number_gutter(state: &mut AppState) {
    if let Some(old) = state.line_number_renderer.take() {
        crate::gutter::remove_line_number_renderer(&state.text_view, &old);
    }
    if let Some(old) = state.right_line_number_renderer.take() {
        crate::gutter::remove_line_number_renderer(&state.right_view, &old);
    }
    let is_prose = state.current_work.as_ref()
        .map(|w| crate::db::line_types::is_prose_work(&w.work_type))
        .unwrap_or(true);
    // Line numbers are skipped in two-column mode (unless explicitly enabled),
    // matching the work-load gutter setup. Without this guard, rebuilding after
    // a font reapply (e.g. toggling translations off) re-added numbers in
    // two-column mode, which also ate column width and underfilled the right
    // column.
    // No verse line numbers in the translation overlay — the interleaved
    // original/translation lines make the right-gutter foliation noise.
    let show_numbers = (state.column_count() != 2 || SHOW_LINE_NUMBERS_TWO_COL)
        && !state.translations_visible;
    if !is_prose && show_numbers {
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
        let (ln_width, ln_margin, ln_gap) = line_number_gutter_geometry(state.column_count());
        let renderer = crate::gutter::setup_line_number_gutter(
            &state.text_view,
            state.line_numbers.clone(),
            &state.theme.dim_fg,
            &state.config.font_family,
            state.config.font_size,
            ln_width,
            ln_margin,
        );
        state.text_view.set_right_margin(ln_gap);
        state.line_number_renderer = Some(renderer);
        let right_renderer = crate::gutter::setup_line_number_gutter(
            &state.right_view,
            state.line_numbers.clone(),
            &state.theme.dim_fg,
            &state.config.font_family,
            state.config.font_size,
            ln_width,
            ln_margin,
        );
        state.right_view.set_right_margin(ln_gap);
        state.right_line_number_renderer = Some(right_renderer);
    }
}

/// Adjust font size by delta, clamp to 8..=72, reapply CSS and repaginate.
/// While the translation view is visible, this adjusts the INDEPENDENT
/// translation font size (Charter), leaving the two-column reader size
/// (`config.font_size`) untouched.
pub fn adjust_font_size(state: &mut AppState, delta: i32) {
    if state.translations_visible {
        let new_size = (state.config.translation_font_size as i32 + delta).clamp(8, 72) as u32;
        if new_size == state.config.translation_font_size {
            return;
        }
        state.config.translation_font_size = new_size;
        reapply_font(state);
        rebuild_line_number_gutter(state);
        crate::input::navigation::resnap_page(state);
        crate::input::navigation::invalidate_page_tops(state);
        crate::config::save(&state.config);
        return;
    }
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
    // In the translation view, report the independent Charter translation size.
    if state.translations_visible {
        let body = format!("Charter {}pt (translation)", state.config.translation_font_size);
        let _ = std::process::Command::new("notify-send")
            .args(["-t", "1500", "-h", "string:x-canonical-private-synchronous:linux-lit-font",
                   "Font", &body])
            .spawn();
        return;
    }
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
        // Don't gold-highlight heading words. "epilogue"/"prologue"/"chorus"
        // are legitimate vocab words, but on an ACT/SCENE/EPILOGUE marker line
        // they are structural headings (already bolded) and must not also take
        // the vocab color. Separators carry no words but skip them too.
        if crate::db::line_types::is_act_scene_marker(line_text)
            || crate::db::line_types::is_separator(line_text)
        {
            continue;
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

/// Returns true when the active work has chapter markers (is_chapter lines)
/// AND is a prose work type (novel/essay_collection/prose_book/prose).
/// Requiring a prose work type ensures plays with stray is_chapter marks
/// (e.g. Rom, Tro) are never treated as chapter works.
/// Detection reads work.lines directly so it works whether or not a line_map
/// exists (prose works load with line_map = None).
pub fn is_chapter_work(state: &AppState) -> bool {
    state
        .current_work
        .as_ref()
        .map(|w| {
            crate::db::line_types::is_prose_work(&w.work_type)
                && w.lines.iter().any(|l| l.is_chapter)
        })
        .unwrap_or(false)
}

/// Chapter number (1-indexed) for the current line in a chapter work, counting
/// is_chapter work-lines at or before the current line. Returns 0 when before
/// the first chapter (front matter). Works with or without a line_map.
pub fn current_chapter_number(state: &AppState) -> usize {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return 0,
    };
    // Map the current buffer line to a work-line index. If the current buffer
    // line isn't itself mapped (e.g. a blank/heading line), walk forward then
    // backward to the nearest mapped work line, mirroring current_scene_divs.
    let line_count = state.effective_line_count();
    let work_idx = state
        .work_line_for_buffer(state.current_line)
        .or_else(|| (state.current_line + 1..line_count).find_map(|bl| state.work_line_for_buffer(bl)))
        .or_else(|| (0..state.current_line).rev().find_map(|bl| state.work_line_for_buffer(bl)));
    let work_idx = match work_idx {
        Some(i) => i,
        None => return 0,
    };
    let flags: Vec<bool> = work.lines.iter().map(|l| l.is_chapter).collect();
    chapter_number_from_flags(&flags, work_idx)
}

/// Pure core of current_chapter_number: count is_chapter flags up to and
/// including work_idx. 0 = before first chapter.
pub fn chapter_number_from_flags(is_chapter_flags: &[bool], work_idx: usize) -> usize {
    is_chapter_flags.iter().take(work_idx + 1).filter(|&&c| c).count()
}

/// The synopsis-cache key for the current line. For chapter works this is
/// (chapter_number, 0); otherwise the scene's (div1, div2).
pub fn current_synopsis_key(state: &AppState) -> (i64, i64) {
    if is_chapter_work(state) {
        return (current_chapter_number(state) as i64, 0);
    }
    current_scene_divs(state)
}

/// Human-readable overlay label for a synopsis key, branching on work type.
pub fn synopsis_label(state: &AppState, div1: i64, div2: i64) -> String {
    if is_chapter_work(state) {
        format!("Chapter {}", div1)
    } else {
        scene_label(div1, div2)
    }
}

/// Get the (div1, div2) of the scene at the current line.
/// When current_line is on an unmapped buffer line (scene header, separator,
/// stage direction), walks forward then backward to find the nearest mapped line.
pub fn current_scene_divs(state: &AppState) -> (i64, i64) {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return (0, 0),
    };
    let line_count = state.effective_line_count();
    // Try current line first
    if let Some(work_idx) = state.work_line_for_buffer(state.current_line) {
        if let Some(line) = work.lines.get(work_idx) {
            return (line.div1, line.div2);
        }
    }
    // Walk forward to find the nearest mapped line (the first dialogue of the scene)
    for bl in (state.current_line + 1)..line_count {
        if let Some(work_idx) = state.work_line_for_buffer(bl) {
            if let Some(line) = work.lines.get(work_idx) {
                return (line.div1, line.div2);
            }
        }
    }
    // Walk backward as fallback
    for bl in (0..state.current_line).rev() {
        if let Some(work_idx) = state.work_line_for_buffer(bl) {
            if let Some(line) = work.lines.get(work_idx) {
                return (line.div1, line.div2);
            }
        }
    }
    (0, 0)
}

/// Check if the current line is the first line of a new scene.
pub fn is_first_line_of_scene(state: &AppState) -> bool {
    if state.current_line == 0 {
        return true;
    }
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return false,
    };
    let cur_idx = match state.work_line_for_buffer(state.current_line) {
        Some(i) => i,
        None => return false,
    };
    let cur = &work.lines[cur_idx];
    // line_in_div == 1 means this is the first content line of a scene,
    // which is where scene-jump (2/3 keys) lands the cursor.
    if cur.line_in_div == 1 {
        return true;
    }
    let prev_idx = state.work_line_for_buffer(state.current_line - 1);
    match prev_idx {
        Some(pi) => {
            let prev = &work.lines[pi];
            cur.div1 != prev.div1 || cur.div2 != prev.div2
        }
        _ => false,
    }
}

/// Walk backwards from `buf_line` past unmapped buffer lines (headers,
/// separators, blanks, stage directions) to find where the scene heading
/// block begins. Returns the buffer line to use as page_top.
fn scene_heading_start(state: &AppState, buf_line: usize) -> usize {
    let mut top = buf_line;
    while top > 0 {
        let prev = top - 1;
        if state.work_line_for_buffer(prev).is_some() {
            break;
        }
        top = prev;
    }
    top
}

/// Show the synopsis for the current scene in the sidebar popup.
pub fn show_synopsis(state: &mut AppState) {
    let (div1, div2) = current_synopsis_key(state);
    crate::logging::log(&format!(
        "SYNOPSIS: show current_line={} divs=({},{}) cache_hit={}",
        state.current_line, div1, div2, state.synopsis_cache.contains_key(&(div1, div2))
    ));
    if let Some(synopsis) = state.synopsis_cache.get(&(div1, div2)) {
        let scene_label = synopsis_label(state, div1, div2);
        state.vocab_popup.update_synopsis(&scene_label, synopsis);
        state.vocab_popup.show();
        update_vocab_popup_margin(state);
        state.sidebar_mode = SidebarMode::Synopsis;
        state.synopsis_visible = true;
    }
}

/// Toggle between synopsis and vocab sidebar modes.
pub fn toggle_synopsis(state: &mut AppState) {
    if state.synopsis_cache.is_empty() {
        return;
    }
    // Cancel any pending auto-fade timer
    state.vocab_popup_fade_gen.set(state.vocab_popup_fade_gen.get() + 1);
    if state.sidebar_mode == SidebarMode::Synopsis && state.synopsis_visible {
        state.sidebar_mode = SidebarMode::Vocab;
        state.synopsis_visible = false;
        if state.vocab_popup_auto {
            open_vocab_popup(state);
        } else {
            close_vocab_popup(state);
        }
    } else {
        let (div1, div2) = current_synopsis_key(state);
        if state.synopsis_cache.contains_key(&(div1, div2)) {
            show_synopsis(state);
        }
    }
}

pub fn show_synopsis_overlay(state: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let s = state.borrow();
    if s.gloss_overlay.is_visible() {
        drop(s);
        let mut s = state.borrow_mut();
        s.gloss_overlay.hide();
        s.input_mode = InputMode::Reader;
        return;
    }

    if s.synopsis_cache.is_empty() {
        s.chapter_toast.set_text("No synopsis for this section");
        s.chapter_toast.set_visible(true);
        let toast = s.chapter_toast.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            toast.set_visible(false);
        });
        return;
    }

    let (div1, div2) = current_synopsis_key(&s);
    let synopsis = match s.synopsis_cache.get(&(div1, div2)) {
        Some(text) => text.clone(),
        None => {
            s.chapter_toast.set_text("No synopsis for this section");
            s.chapter_toast.set_visible(true);
            let toast = s.chapter_toast.clone();
            glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                toast.set_visible(false);
            });
            return;
        }
    };

    let card_width = s.content_hbox.width();
    let card_height = s.content_hbox.height();
    let label = synopsis_label(&s, div1, div2);
    s.gloss_overlay.show_synopsis(&label, &synopsis, card_width, card_height);
    drop(s);
    let mut s = state.borrow_mut();
    s.synopsis_overlay_scene = (div1, div2);
    s.input_mode = InputMode::SynopsisOverlay;
}

/// Human-readable label for a scene, shared by the synopsis overlay and the
/// gloss overlay. (0,0) = Prologue; (N,0) = Act N, Chorus; else Act N, Scene M.
pub fn scene_label(div1: i64, div2: i64) -> String {
    if div1 == 0 && div2 == 0 {
        "Prologue".to_string()
    } else if div2 == 0 {
        format!("Act {}, Chorus", div1)
    } else {
        format!("Act {}, Scene {}", div1, div2)
    }
}

/// Ordered list of the work's scene keys (div1, div2) that have a synopsis, in
/// reading order. `work.lines` is already sorted by (div1, div2, line_in_div),
/// so collecting unique pairs in encounter order gives reading order.
fn ordered_synopsis_scenes(s: &AppState) -> Vec<(i64, i64)> {
    if is_chapter_work(s) {
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return Vec::new(),
        };
        let chapter_count = work.lines.iter().filter(|l| l.is_chapter).count();
        let mut keys = Vec::new();
        for n in 1..=chapter_count {
            let k = (n as i64, 0);
            if s.synopsis_cache.contains_key(&k) {
                keys.push(k);
            }
        }
        return keys;
    }
    let work = match s.current_work.as_ref() {
        Some(w) => w,
        None => return Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    let mut keys = Vec::new();
    for line in &work.lines {
        let k = (line.div1, line.div2);
        if seen.insert(k) && s.synopsis_cache.contains_key(&k) {
            keys.push(k);
        }
    }
    keys
}

/// Step the synopsis overlay to the next (+1) or previous (-1) scene that has a
/// synopsis, wrapping around. No-op if the overlay isn't showing a known scene.
pub fn cycle_synopsis(state: &std::rc::Rc<std::cell::RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    let scenes = ordered_synopsis_scenes(&s);
    if scenes.is_empty() {
        return;
    }
    let cur = s.synopsis_overlay_scene;
    let idx = scenes.iter().position(|&k| k == cur).unwrap_or(0);
    let new_idx = ((idx as i32 + delta).rem_euclid(scenes.len() as i32)) as usize;
    let (div1, div2) = scenes[new_idx];
    let synopsis = match s.synopsis_cache.get(&(div1, div2)) {
        Some(t) => t.clone(),
        None => return,
    };
    let label = synopsis_label(&s, div1, div2);
    let card_width = s.content_hbox.width();
    let card_height = s.content_hbox.height();
    s.gloss_overlay.show_synopsis(&label, &synopsis, card_width, card_height);
    s.synopsis_overlay_scene = (div1, div2);
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

pub fn update_title_bar_scene(state: &AppState) {
    if !state.title_bar.is_visible() {
        return;
    }
    if !state.synopsis_cache.is_empty() {
        let (div1, div2) = current_synopsis_key(state);
        let label = synopsis_label(state, div1, div2);
        state.title_bar_scene_label.set_text(&label);
    } else {
        state.title_bar_scene_label.set_text("");
    }
}

#[cfg(test)]
mod column_default_tests {
    use super::default_column_count_for_parts;

    #[test]
    fn shakespeare_play_defaults_to_two() {
        assert_eq!(default_column_count_for_parts("Shakespeare", "play"), 2);
    }
    #[test]
    fn shakespeare_poem_defaults_to_two() {
        assert_eq!(default_column_count_for_parts("Shakespeare", "poem"), 2);
        assert_eq!(default_column_count_for_parts("Shakespeare", "sonnet_sequence"), 2);
        assert_eq!(default_column_count_for_parts("Shakespeare", "narrative_poem"), 2);
    }
    #[test]
    fn non_shakespeare_play_defaults_to_two() {
        assert_eq!(default_column_count_for_parts("Marlowe", "play"), 2);
    }
}

#[cfg(test)]
mod card_width_tests {
    use super::target_card_width;

    #[test]
    fn one_column_keeps_configured_width() {
        // Single column always uses column_width regardless of window size.
        assert_eq!(target_card_width(1920, 1050, 1, false), 1050);
        assert_eq!(target_card_width(800, 1050, 1, false), 1050);
    }

    #[test]
    fn two_columns_fill_fraction_of_wide_window() {
        // TWO_COLUMN_WIDTH_FRACTION (0.68) of 1920 = 1305, below the verse-safe
        // two-column floor (2*700+8 = 1408), so clamp up to 1408.
        assert_eq!(target_card_width(1920, 1050, 2, false), 1408);
    }

    #[test]
    fn two_columns_use_proportional_when_above_floor() {
        // On a very wide window the proportional width wins: 0.68 * 2400 = 1632.
        assert_eq!(target_card_width(2400, 1050, 2, false), 1632);
    }

    #[test]
    fn two_columns_never_below_verse_safe_floor() {
        // Narrow window: proportional (0.68*1300=884) and column_width (1050)
        // are both below the 1408 two-column floor → clamp up to 1408.
        assert_eq!(target_card_width(1300, 1050, 2, false), 1408);
    }

    #[test]
    fn translations_match_two_column_width() {
        // Translation mode (column_count forced to 1) sizes like two columns.
        assert_eq!(
            target_card_width(2400, 1050, 1, true),
            target_card_width(2400, 1050, 2, false),
        );
    }
}

#[cfg(test)]
mod chapter_synopsis_tests {
    #[test]
    fn chapter_number_from_flags_counts_inclusive() {
        // lines: ch markers at idx 0 and 3
        let flags = vec![true, false, false, true, false];
        assert_eq!(super::chapter_number_from_flags(&flags, 0), 1); // on first chapter
        assert_eq!(super::chapter_number_from_flags(&flags, 2), 1); // still chapter 1
        assert_eq!(super::chapter_number_from_flags(&flags, 3), 2); // second chapter
        assert_eq!(super::chapter_number_from_flags(&flags, 4), 2);
    }

    #[test]
    fn chapter_number_from_flags_front_matter_is_zero() {
        // first chapter marker at idx 2; idx 0,1 are front matter
        let flags = vec![false, false, true, false];
        assert_eq!(super::chapter_number_from_flags(&flags, 0), 0);
        assert_eq!(super::chapter_number_from_flags(&flags, 1), 0);
        assert_eq!(super::chapter_number_from_flags(&flags, 2), 1);
    }
}
