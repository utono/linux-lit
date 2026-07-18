pub mod vocab_popup;
pub mod font;
use self::font::reapply_font;
pub mod text_prep;
use self::text_prep::{PreparedText, SnapshotOrPrep, prepare_text_only, prepare_text_for_display};
pub mod formatting;
use self::formatting::{apply_dialogue_formatting, apply_authorship_formatting, apply_scansion_marks};
pub mod scene_synopsis;
use self::scene_synopsis::{is_first_line_of_scene, scene_heading_start};
pub mod translations;
pub mod layout;
use self::layout::{apply_tiled_mode, apply_card_sizing, line_number_gutter_geometry, overlay_card_size};

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
use crate::ui::journal_move_picker::JournalMovePicker;
use crate::ui::journal_picker::JournalQaPicker;
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

/// Grouped state for the page-scan image view + calibration mode. Was five flat
/// fields on AppState (`page_images`/`image_dir`/`image_mode`/`current_page_order`/
/// `calibration_index`); grouped per the AppState god-struct decomposition
/// (pure-tier cluster). All accesses are mod.rs-internal (the image/calibration
/// free functions).
#[derive(Default)]
pub struct PageImageState {
    pub images: Vec<crate::db::models::PageImage>,
    pub dir: Option<String>,
    pub mode: bool,
    pub page_order: Option<i64>,
    pub calibration_index: usize,
}

/// Grouped state for the scansion-marks feature (the per-line scansion data,
/// the current display level, and the buffer-line→label-start map). Was three
/// flat `scansion_*` fields on AppState; grouped per the AppState god-struct
/// decomposition (render-tier cluster).
pub struct ScansionState {
    pub label_starts: std::collections::HashMap<usize, usize>,
    pub level: crate::scansion::ScanLevel,
    pub data: std::collections::HashMap<i64, crate::scansion::LineScansion>,
}

/// Where the voice picker was opened from, so confirm/cancel route back
/// correctly and write the right target.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VoicePickerOrigin {
    Settings,
    GlossOverlay,
    /// Opened from the gloss overlay's `R` source-verse TTS key: confirming
    /// picks the active voice and plays the source verse (pausing MPV first).
    /// Wired in a later task; currently routes like `GlossOverlay`.
    GlossPlay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Reader,
    LibraryPicker,
    BookmarkPicker,
    MediaPicker,
    Settings,
    Search,
    /// Typing a regex into the reader `search_bar` to search the CURRENT
    /// journal/gloss overlay entry (the overlay `/` bind). Return sets the
    /// pattern on the overlay buffer; Escape cancels back to the overlay. Keys
    /// otherwise flow to the focused search-bar Entry.
    OverlaySearchInput,
    GlossOverlay,
    GlossVisual,
    JournalOverlay,
    JournalVisual,
    /// In-place modal vim editing of the journal Q&A page (the `e` bind). All
    /// keys route to the vim engine via `handle_journal_edit_key`; the page
    /// TextView mirrors the engine's buffer/cursor. `:w` saves, `:q`/Esc cancels,
    /// `R` opens the LLM-rewrite prompt. Replaces the old JournalEditCard.
    JournalEdit,
    /// In-place modal vim editor for the gloss/synopsis overlay (the same
    /// `GlossOverlay` widget). Early-dispatched in `handle_key` beside
    /// `JournalEdit` so Insert-mode space and printable keys reach the engine.
    /// `:w`/`:wq` save the raw text; `:q`/double-Esc exit; `R` opens the ask-Claude
    /// rewrite. The save path branches on whether the overlay shows a gloss or a
    /// synopsis.
    GlossEdit,
    /// Copy-only vim view of the cursor's segment (the reader's `v` bind),
    /// hosted on the same `GlossOverlay` edit buffer as `GlossEdit` and
    /// early-dispatched beside it. Seeded in VISUAL mode; visual `y` copies to
    /// the system clipboard; `:q`/double-Esc exit. Save verbs are refused —
    /// nothing is written back to the reading buffer or lit.db.
    SegmentVim,
    /// Fully modal vocab-sentence drill loop (Ctrl+-; requires phrase data
    /// for the playing media, else the entry toasts the reason): the sentence
    /// under review repeats via MPV ab-loop; n/p step between vocab
    /// sentences, a/Space toggles pause, Escape (or Ctrl+-, legacy Ctrl+r)
    /// exits. All other keys are swallowed.
    VocabLoop,
    SynopsisOverlay,
    SynopsisVisual,
    TranslationOverlay,
    GlossPicker,
    JournalPicker,
    JournalMovePicker,
    JournalTermInput,
    EchoPicker,
    EchoTurnsPicker,
    EchoesOverlay,
    GamepadOverlay,
    KeybindsOverlay,
    ConcordancePicker,
    ConcordanceWordPicker,
    VoicePicker,
    EchoLinePicker,
    EchoKeybindsOverlay,
    GlossKeybindsOverlay,
    SynopsisKeybindsOverlay,
    JournalKeybindsOverlay,
    ConcordanceListPicker,
    ConcordanceWorksPicker,
    AuthorshipPicker,
    /// Ctrl+f cross-corpus regex search popup: live-filters journal or gloss
    /// rows as you type; Tab toggles corpus, Up/Down moves selection, Return
    /// selects, Escape restores `corpus_search_return_mode`.
    CorpusSearch,
    ActionPopup,
    Visual,
    DeleteConfirm,
    /// Confirm reverting the last `e` edit in the gloss/synopsis/journal overlay:
    /// the card shows "Undo last edit? y / Esc". `y` restores the pre-edit text
    /// and returns to the originating overlay; `Esc`/`n` cancels.
    UndoConfirm,
    /// Choose the target of a journal `R` rewrite: the card shows
    /// "Rewrite: q question · a answer · b both · Esc cancel". A single key
    /// routes to the answer-only flow (`a`), the question-improve + regenerate
    /// flow (`q`), or improve-question + instruction-driven answer rewrite
    /// (`b`); `Esc`/any other key returns to the journal overlay.
    RewriteTargetChoice,
    /// Manual page-image calibration: the card shows a page PNG and a readout of
    /// the cursor line; Enter marks the cursor line as that page's start and
    /// advances to the next page.
    PageCalibration,
    /// Chat layout: the panel's vim prompt owns keys (Tab cycles to the
    /// transcript; Ctrl+Tab closes the panel; Ctrl+Enter submits).
    ChatPrompt,
    /// Chat layout: the transcript owns keys (j/k exchange cursor, s saves,
    /// Tab cycles to the reader, Ctrl+Tab closes).
    ChatTranscript,
}

/// Which of the two toggleable reader overlays (gloss / journal) was most
/// recently open. Recorded at the single close chokepoint
/// (`return_to_reader_mode`) so `ToggleLastOverlay` (Ctrl+Tab) can flip the
/// reader back to whichever overlay you last had up, regardless of how it was
/// closed (toggle, Escape, or undo-confirm return).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LastOverlay {
    Gloss,
    Journal,
}

#[derive(Clone, Copy, PartialEq)]
pub enum GlossPromptMode {
    Add,
    Edit,
    /// Gloss-overlay `i`: correct one word's /IPA/ in the cursor's source verse.
    FixIpa,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalPromptMode {
    Ask,
}

/// Which "band" of the journal is currently shown. The Work band holds
/// whole-work pages (scope='work'); a Scene band holds one (div1,div2)'s pages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JournalBand {
    Work,
    Scene(i64, i64),
    Passage { div1: i64, div2: i64, start: String, end: String },
    /// Author/corpus band: holds scope='author' pages keyed by the author name.
    Author(String),
}

#[derive(Clone, Copy, PartialEq)]
pub enum SidebarMode {
    Vocab,
    Synopsis,
}

/// Which Claude system prompt the open synopsis input card will use on submit.
/// `A` opens it as `Ask` (augment/explain); `E` opens it as `Edit` (structural
/// edit). Read by `submit_amend_prompt` to dispatch to the right revision path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SynopsisPromptKind {
    Ask,
    Edit,
}

#[allow(dead_code)]
pub struct AppState {
    pub text_view: View,
    pub buffer: sourceview5::Buffer,
    pub library_picker: LibraryPicker,
    pub current_work: Option<Work>,
    pub current_line: usize,
    pub prev_highlight_line: std::cell::Cell<Option<usize>>,
    /// Pinned play page table (buffer-line space), loaded from lit.db when the
    /// layout fingerprint matches, or generated+stored after first settled
    /// layout. None = live engine. See input::page_table.
    pub page_table: std::cell::RefCell<Option<std::rc::Rc<Vec<crate::input::page_table::Spread>>>>,
    /// The layout fingerprint the active `page_table` was loaded/generated at
    /// (empty when no table is active). Compared against the CURRENT
    /// fingerprint on resize so a plain window resize (dwl retiling) can't
    /// leave a table with stale-geometry boundaries active. See
    /// `input::page_table::revalidate_on_resize`.
    pub page_table_fp: std::cell::RefCell<String>,
    /// One generation attempt per work load (reset in display_work).
    pub page_table_gen_attempted: std::cell::Cell<bool>,
    /// Pinned prose page table (visual-row pages) for the current work, when
    /// one was loaded/generated for the CURRENT layout fingerprint. None = live
    /// prose engine. See input::prose_pages.
    pub prose_page_table: std::cell::RefCell<Option<std::rc::Rc<Vec<crate::input::prose_pages::ProsePage>>>>,
    /// The layout fingerprint the active `prose_page_table` was loaded/generated
    /// at (empty when none active). Compared against the CURRENT fingerprint on
    /// resize the same way `page_table_fp` is.
    pub prose_page_table_fp: std::cell::RefCell<String>,
    /// One prose-table generation attempt per work load (reset in display_work).
    pub prose_page_table_gen_attempted: std::cell::Cell<bool>,
    /// The first buffer line hidden below each column's paged bottom clip
    /// (left/main view: the two-column `exact_end` = `cs.split`, `None` in
    /// single-column mode; right view: `cs.page_end + 1`). Stored so
    /// `update_highlight` can re-schedule that column's clip when the cursor
    /// crosses the boundary — the descender allowance must collapse to 0 while
    /// the boundary line carries the cursor-highlight band, or the reveal shows
    /// the band's top edge as a colored sliver (see `descender_allowance`).
    pub left_clip_boundary: std::cell::Cell<Option<usize>>,
    pub right_clip_boundary: std::cell::Cell<Option<usize>>,
    pub page_top_line: usize,
    /// Pixels scrolled PAST `page_top_line`'s pixel top. 0 in the normal
    /// (line-aligned) case; non-zero only while paging WITHIN an over-tall prose
    /// paragraph (one buffer line taller than the viewport) — the viewport top is
    /// `line_yrange(page_top_line).y + page_top_offset`. See
    /// `docs/troubleshooting/page-turning-mechanics.md` → "Prose over-tall paragraph".
    pub page_top_offset: i32,
    /// History of `(page_top_line, page_top_offset)` so `y` round-trips a
    /// mid-paragraph forward turn exactly. Pushed by `page_forward`, popped by
    /// `page_backward`.
    pub page_back_stack: Vec<(usize, i32)>,
    pub dim_tag: gtk4::TextTag,
    pub cursor_line_tag: gtk4::TextTag,
    pub cursor_fade_tag: gtk4::TextTag,
    pub ab_dim_tag: gtk4::TextTag,
    /// Karaoke spoken-phrase tint during narration sync (phrase_highlight.rs).
    pub phrase_tag: gtk4::TextTag,
    /// Sentence-extent tint for the vocab-sentence loop mode: marks the whole
    /// looping sentence while phrase_tag's sweep moves inside it.
    pub vocab_sentence_tag: gtk4::TextTag,
    /// Transient prose nav-flash tint: the cursor paragraph's background flashes
    /// the phrase-highlight color on a nav keybind, then fades out. Color is set
    /// per-frame by the flash animation, so no static color lives on the tag.
    /// See highlight::flash_prose_cursor_line.
    pub prose_flash_tag: gtk4::TextTag,
    /// Cached phrase spans for the (line, media) being narrated. Empty spans
    /// vec = cached negative result; see phrase_highlight.rs.
    pub phrase_cache: Option<crate::input::phrase_highlight::PhraseCache>,
    /// Last applied phrase (buffer_line, span_idx) — skips redundant re-tags.
    pub active_phrase: Option<(usize, usize)>,
    /// Keep a pending-phrase paint (seek keybind target) through sync
    /// suppression: TimePos ticks inside this window must not clear the tint.
    pub phrase_paint_hold: Option<std::time::Instant>,
    pub page_turn_overlay: gtk4::Overlay,
    pub bottom_clip: gtk4::Box,
    pub top_spacer: gtk4::Box,
    /// Running-head strip labels living inside `top_spacer` (the card's top
    /// band). `running_head_work` is the work abbrev (left); `running_head_scene`
    /// is the position label (right) — act/scene for plays, chapter for prose.
    /// Both are refreshed on every cursor move and on work load via
    /// `scene_synopsis::update_running_heads`, on ALL works. Blank only when no
    /// work is loaded. Replaces the persistent bottom-center position toast.
    pub running_head_work: gtk4::Label,
    pub running_head_scene: gtk4::Label,
    pub card_vbox: gtk4::Box,
    pub scrolled_window: ScrolledWindow,
    /// Left-column container. Carries the divider-hug left margin in two-column
    /// mode so the left column's text shifts toward the center divider.
    pub scrolled_overlay: gtk4::Overlay,
    pub right_view: View,
    pub right_scrolled_window: ScrolledWindow,
    pub right_scrolled_overlay: gtk4::Overlay,
    pub right_bottom_clip: gtk4::Box,
    /// Dim "Next: Act N, Scene M" label shown centered in an empty right
    /// column (scene ended in the left column). Overlay child of
    /// `right_scrolled_overlay`; hidden in every other case.
    pub next_scene_watermark: gtk4::Label,
    pub columns_hbox: gtk4::Box,
    /// Thin vertical rule between the two columns; visible only in two-column mode.
    pub column_divider: gtk4::Separator,
    pub right_line_number_renderer: Option<sourceview5::GutterRendererText>,
    /// Sign-column (timestamp/bookmark glyph) renderer for the right column in
    /// two-column mode. Mirrors `gutter_renderer` on the left `text_view`.
    pub right_gutter_renderer: Option<sourceview5::GutterRendererText>,
    pub content_hbox: gtk4::Box,
    /// Chat layout (Tab): card pinned right, left chat panel visible.
    pub chat_layout_open: bool,
    /// Set by `chat::on_work_switched` when a work switch happened with the
    /// panel open: the panel's width hold was released immediately (so it
    /// can't inflate the window), and the real re-gate/resize is deferred
    /// until the resize tick observes settled (non-changing) geometry —
    /// reading `s.window.width()` at the work-switch hook point can observe
    /// a transient in-flight size (e.g. mid-reflow into a two-column work)
    /// rather than the compositor-settled width.
    pub chat_regate_pending: bool,
    /// Where an open chat panel sits: pinned beside the card (single-column)
    /// or floating over one reading column (two-column). Session-only.
    pub chat_placement: crate::input::actions::chat::ChatPlacement,
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
    /// Active prose nav-flash fade-out animation.
    pub prose_flash_anim: Option<adw::TimedAnimation>,
    /// Set by dispatch_action when a nav keybind fires in a prose work;
    /// consumed by update_highlight to flash the cursor paragraph. Sync-driven
    /// highlight updates never set it, so playback stays flash-free.
    pub pending_prose_flash: std::cell::Cell<bool>,
    /// Set by the instant page-set paths, consumed with the flag above: the
    /// nav-flash follows a page turn, so it holds full strength briefly before
    /// fading — an immediate fade burns its brightest phase before the new
    /// page's first paint and the q/, turn showed no visible blink.
    pub prose_flash_hold: std::cell::Cell<bool>,
    pub cmd_tx: tokio::sync::mpsc::Sender<crate::mpv::MpvCommand>,
    pub tokio_handle: tokio::runtime::Handle,
    pub playback_speed: f64,
    pub search_bar: SearchBar,
    pub search_matches: Vec<SearchMatch>,
    pub search_match_idx: usize,
    /// Most-recently-used non-empty search pattern. Persists for the session
    /// (survives Escape and work switches) so n/N can reactivate search. NOT
    /// cleared by clear_search.
    pub last_search_query: Option<String>,
    /// Direction of the active search: false = forward (`/`, seek first match at
    /// or after the cursor), true = backward (`?`, seek last match at or before
    /// the cursor). Set when the search bar opens.
    pub search_backward: bool,
    /// Reader position (current_line, page_top_line, page_top_offset) saved when
    /// search opens, so Escape can cancel the live-search jump and restore the
    /// original page. The offset matters for prose row-fill pages whose top sits
    /// mid-paragraph — dropping it re-anchored the restored page at the
    /// paragraph's row 0 (the "Esc out of an overlay repaginates" bug).
    pub search_return_pos: Option<(usize, usize, i32)>,
    /// Reader position (current_line, page_top_line, page_top_offset) saved when
    /// a gloss overlay opens (picker, MRU toggle, or from synopsis), so Escape
    /// restores the page the user was on instead of jumping to the glossed
    /// passage.
    pub gloss_return_pos: Option<(usize, usize, i32)>,
    /// Which reader overlay (gloss/journal) was most recently open, so
    /// `ToggleLastOverlay` (Ctrl+Tab) can reopen it from the reader. Set at the
    /// close chokepoint `return_to_reader_mode`.
    pub last_overlay: Option<LastOverlay>,
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
    pub corpus_search_popup: crate::ui::corpus_search_popup::CorpusSearchPopup,
    pub last_corpus: crate::input::corpus_search::Corpus,
    /// Where to return `input_mode` when the corpus-search popup closes
    /// (Escape) — the reader or whichever gloss/journal overlay opened it.
    pub corpus_search_return_mode: InputMode,
    pub translations: HashMap<i64, String>,
    pub translations_visible: bool,
    /// Column count to assume BEFORE `current_work` is loaded — seeded at build
    /// time from `config.last_column_count`. `column_count()` falls back to this
    /// instead of `1` when no work is set yet, so the first card-sizing pass
    /// matches the target layout and there's no visible 1→2-column reflow on
    /// startup. `None` (no saved value) → fall back to `1` as before.
    pub pending_column_count: Option<u8>,
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
    /// Section-start bitmap remapped to the inflated (translation-inserted)
    /// buffer. Built in `show_translations` from the line_map's original-buffer
    /// `section_starts` via the same insert remap used for `current_line`;
    /// inserted translation lines are `false`. `section_starts()` returns this
    /// (instead of the line_map's original-indexed bitmap) while translations
    /// are visible so the one-section-per-page clamp lands on the right physical
    /// line. Empty when translations are hidden.
    pub translation_section_starts: Vec<bool>,
    pub translation_dim_tag: gtk4::TextTag,
    pub translation_text_tag: gtk4::TextTag,
    pub scansion_label_tag: gtk4::TextTag,
    pub scansion: ScansionState,
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
    /// Prose straddling-paragraph page crossing (Task 9). When the spoken
    /// paragraph continues past the current page's stored boundary, this holds
    /// `(fire_time, target_page_index)`: once MPV time_pos reaches `fire_time`,
    /// advance the prose page grid to `target_page_index` (the paragraph's
    /// continuation page) while the cursor stays on the same paragraph. Cleared
    /// by any non-MpvSync page change (manual x/y/j/k/G/gg, seek), on work
    /// switch, and when the prose table drops (resize) — a stale scheduled cross
    /// firing after the user navigated away is the classic bug here.
    pub pending_prose_cross: Option<(f64, usize)>,
    pub visual_selection: Option<crate::input::visual::SelectionState>,
    pub selection_tag: gtk4::TextTag,
    pub action_popup: Option<crate::input::visual::ActionPopupState>,
    pub action_popup_widget: crate::ui::action_popup::ActionPopup,
    pub keybinds_overlay: crate::ui::keybinds_overlay::KeybindsOverlay,
    pub gamepad_overlay: crate::ui::gamepad_overlay::GamepadOverlay,
    pub gloss_overlay: crate::ui::gloss_overlay::GlossOverlay,
    pub journal_overlay: crate::ui::journal_overlay::JournalOverlay,
    pub journal_picker: JournalQaPicker,
    pub journal_move_picker: JournalMovePicker,
    pub journal_term_input: crate::ui::journal_term_input::JournalTermInput,
    pub journal_band: JournalBand,
    pub journal: crate::input::actions::journal::JournalState,
    /// Page-scan image surface for the main card (BCP1549 etc.). Hidden unless
    /// `image_mode` is on.
    pub page_image_overlay: crate::ui::page_image_overlay::PageImageOverlay,
    /// Grouped page-scan image view + calibration state (images, dir, mode,
    /// page_order, calibration_index). See `PageImageState`.
    pub page_image: PageImageState,
    pub tts: crate::tts::TtsPlayer,
    /// True while a Shift+Space batch synthesis is running, so a second press is
    /// a no-op rather than launching a concurrent batch.
    pub tts_batch_running: std::cell::Cell<bool>,
    pub translation_overlay: crate::ui::translation_overlay::TranslationOverlay,
    pub gloss_original_text: Option<String>,
    pub gloss_list: Vec<crate::db::queries::SavedGloss>,
    pub gloss_index: usize,
    pub gloss_context: Option<crate::gloss::GlossContext>,
    pub gloss_passages: Vec<crate::db::queries::GlossedPassage>,
    pub gloss_passage_index: usize,
    pub gloss_opened_from_picker: bool,
    /// True when the gloss picker (Alt+g) was opened while a gloss overlay was
    /// already open. The overlay stays visible behind the picker, and cancelling
    /// the picker (Escape) returns to the overlay instead of the reader.
    pub gloss_picker_from_overlay: bool,
    /// Index into the current gloss's associated voice set (gloss_voices,
    /// position order) — which voice plays next. Session-only; reset to 0 on
    /// gloss change. With no associated voices, the gender default is used.
    pub gloss_active_voice: usize,
    /// Live regex search over the CURRENT gloss overlay buffer (the `/` bind,
    /// n/N stepping). Its match spans are re-collected on every gloss render so
    /// the pattern keeps highlighting as glosses/passages are stepped. `None`
    /// when no search is active. The gloss analog of `journal.search` (gloss has
    /// no grouped state struct, so this is a flat field).
    pub gloss_search: Option<crate::input::overlay_search::OverlaySearch>,
    /// MRU search pattern for post-Escape n/N revival in the gloss overlay:
    /// clearing the search drops `gloss_search` but keeps this, so the next n/N
    /// rebuilds the search from it. Gloss analog of `journal.last_pattern`.
    pub gloss_last_pattern: Option<String>,
    /// Which overlay opened the shared `OverlaySearchInput` search bar (the `/`
    /// bind). Set by `journal`/`gloss` `open_overlay_search`; read by the search
    /// bar's Return/Escape handler to route confirm/cancel back to the right
    /// overlay and write the right overlay's search state. Defaults to
    /// `JournalOverlay`.
    pub overlay_search_origin: InputMode,
    /// Where the voice picker was opened from, so confirm/cancel route back
    /// correctly and write the right target.
    pub voice_picker_origin: VoicePickerOrigin,
    /// Where to return when the settings overlay closes. Settings can be opened
    /// from the reader (→ `Reader`) or from the gloss / synopsis overlay (→
    /// `GlossOverlay` / `SynopsisOverlay`), in which case that overlay stays
    /// visible behind the settings scrim and is restored on close. Reset to
    /// `Reader` each time settings opens from the reader.
    pub settings_return_mode: InputMode,
    /// When the Ctrl+/ keybinds overlay is opened from another overlay (gloss,
    /// synopsis, a picker, …), this records which mode to restore when it
    /// closes, so Escape returns to that overlay instead of the reader. Reset to
    /// `Reader` each time the keybinds overlay opens from the reader.
    pub keybinds_return_mode: InputMode,
    /// Gloss-picker type filter, cycled by Ctrl+t through teacher-generic ->
    /// inner-monologue -> reader-gloss while the picker is open; reset to the
    /// default (teacher-generic) each time the picker is opened.
    pub gloss_picker_filter: crate::input::actions::pickers::GlossPickerFilter,
    /// Which add/edit prompt the stacked gloss input card will submit as.
    pub gloss_prompt_mode: GlossPromptMode,
    pub delete_confirm_container: Option<glib::WeakRef<gtk4::Box>>,
    pub delete_confirm_overlay: Option<glib::WeakRef<gtk4::Overlay>>,
    /// Which overlay the delete confirmation (`D`) was opened from, so `y` runs
    /// the right delete (gloss vs journal page) and returns to the right mode.
    /// Set when `D` opens the confirm; cleared when it closes. Mirrors
    /// `undo_confirm_origin`.
    pub delete_confirm_origin: Option<InputMode>,
    /// The journal `R` target chooser box (q/a/b/Esc) and its parent overlay,
    /// so the chooser handler can tear it down on any exit key. Mirrors
    /// `delete_confirm_container`/`delete_confirm_overlay`; the chooser always
    /// returns to the journal overlay, so no origin marker is needed.
    pub rewrite_target_container: Option<glib::WeakRef<gtk4::Box>>,
    pub rewrite_target_overlay: Option<glib::WeakRef<gtk4::Overlay>>,
    pub gloss_picker: GlossPicker,
    pub echo_picker: crate::ui::echo_picker::EchoPicker,
    pub echo_turns_picker: crate::ui::echo_turns_picker::EchoTurnsPicker,
    pub pending_echo_context: Option<crate::gloss::GlossContext>,
    pub pending_echo_scene_lines: Vec<crate::db::models::Line>,
    /// `<speaker>`/`<verse>` markup for the passage being glossed, stashed with
    /// the pending inner-monologue call so the post-picker "Glossing…" loading
    /// card can render the passage (like reader-gloss) instead of a bare label.
    pub pending_echo_passage_doc: String,
    pub echo_overlay: crate::input::actions::echoes::EchoOverlayState,
    pub echo_session: Option<crate::input::actions::echoes::EchoSession>,
    pub vocab_words: std::collections::HashSet<String>,
    pub vocab_matches: Vec<VocabMatch>,
    pub vocab_loop: Option<crate::input::vocab_loop::VocabLoopState>,
    pub vocab_tag: gtk4::TextTag,
    /// Foreground tint applied to source lines covered by a `reader-gloss`
    /// passage. Color comes from `theme.reader_gloss` — the contrast-guarded
    /// gloss tint derived from the dwl focuscolor. See `reader_gloss_lines` and
    /// `apply_reader_gloss_highlighting`.
    pub reader_gloss_tag: gtk4::TextTag,
    /// Foreground tag for a glossed line that is ALSO the cursor block — a
    /// distinct, contrast-guarded color (`theme.reader_gloss_cursor`) so it reads
    /// differently from both body text and the off-cursor gloss tint. Applied by
    /// `repaint_reader_gloss_visible` on the cursor line.
    pub reader_gloss_cursor_tag: gtk4::TextTag,
    /// Buffer line indices that fall inside a `reader-gloss` passage for the
    /// current work. Recomputed by `display_work`; used to repaint the
    /// `theme.reader_gloss` tint after the cursor leaves a glossed line
    /// (cursor-line wins while on it).
    pub reader_gloss_lines: std::collections::HashSet<usize>,
    pub dim_enabled: bool,
    pub vocab_highlight_visible: bool,
    pub vocab_popup: crate::app::vocab_popup::VocabPopupState,
    pub sidebar_mode: SidebarMode,
    pub synopsis_cache: HashMap<(i64, i64), String>,
    pub synopsis_visible: bool,
    /// The (div1, div2) scene currently displayed in the synopsis overlay. n/p
    /// step this through the work's scenes; the `A` amend targets it too.
    pub synopsis_overlay_scene: (i64, i64),
    /// The (div1, div2) scene whose synopsis the open `A` amend prompt targets.
    pub synopsis_amend_scene: (i64, i64),
    /// Single-level undo for the `A` amend flow: the scene and its synopsis text
    /// from immediately before the last amendment. `u` in the synopsis overlay
    /// restores it. Cleared once consumed.
    pub synopsis_undo: Option<((i64, i64), String)>,
    /// Single-level undo snapshot for the gloss overlay's `e` edit:
    /// `(gloss_id, pre_edit_gloss_text)`. Set by `update_and_render_gloss_in_place`
    /// before it overwrites the row; `u` restores it via `update_gloss`. Cleared
    /// once consumed.
    pub gloss_undo: Option<(i64, String)>,
    /// Single-level undo snapshot for the journal overlay's `e` edit:
    /// `(page_id, pre_edit_question, pre_edit_answer, claude_model)`. Set by the
    /// journal edit-save / edit-rewrite paths before they update the row; `u`
    /// restores it via `update_journal_page`. Cleared once consumed.
    pub journal_undo: Option<(i64, String, String, String)>,
    /// Active read-only revision browse (Ctrl+Shift+n/p), if the user is
    /// stepping a journal Q&A or gloss entry's stored `rewrite_revisions`.
    /// View-only: browsing never writes the DB or mutates the live row; only
    /// `browse_restore` (Ctrl+Shift+r) writes. Dropped on Escape/nav/close.
    pub rewrite_browse: Option<crate::input::actions::rewrite_history::RewriteBrowse>,
    /// Which overlay a pending `UndoConfirm` belongs to, so `y` runs the right
    /// overlay's undo and returns to the right mode. Set when `u` opens the
    /// confirm; cleared when it closes.
    pub undo_confirm_origin: Option<InputMode>,
    pub undo_confirm_container: Option<glib::WeakRef<gtk4::Box>>,
    pub undo_confirm_overlay: Option<glib::WeakRef<gtk4::Overlay>>,
    /// Which prompt the currently-open synopsis input card will run on submit
    /// (set by `A` -> Ask / `E` -> Edit). Meaningful only while the card is open.
    pub synopsis_prompt_kind: SynopsisPromptKind,
    pub concordance_picker: crate::ui::concordance_picker::ConcordancePicker,
    pub concordance_state: Option<crate::concordance::ConcordanceState>,
    pub concordance_origin: Option<crate::concordance::ConcordanceOrigin>,
    pub concordance_word_cache: Option<(String, Vec<(String, usize)>)>,
    pub concordance_word_picker: crate::ui::concordance_word_picker::ConcordanceWordPicker,
    pub voice_picker: crate::ui::voice_picker::VoicePicker,
    pub echo_line_picker: crate::ui::echo_line_picker::EchoLinePicker,
    pub echo_keybinds_overlay: crate::ui::keybinds_legend::KeybindsLegend,
    pub gloss_keybinds_overlay: crate::ui::keybinds_legend::KeybindsLegend,
    pub synopsis_keybinds_overlay: crate::ui::keybinds_legend::KeybindsLegend,
    pub journal_keybinds_overlay: crate::ui::keybinds_legend::KeybindsLegend,
    /// turn_id the add-echo picker will attach the chosen line to.
    pub echo_add_turn_id: Option<i64>,
    pub concordance_list_picker: crate::ui::concordance_list_picker::ConcordanceListPicker,
    pub concordance_works_picker: crate::ui::concordance_works_picker::ConcordanceWorksPicker,
    pub concordance_bar: crate::ui::concordance_bar::ConcordanceBar,
    pub title_bar: gtk4::Box,
    pub title_bar_label: gtk4::Label,
    pub title_bar_scene_label: gtk4::Label,
    pub chat_panel: crate::ui::chat_panel::ChatPanel,
    pub chat: crate::input::actions::chat::ChatState,
    /// Index of the current sentence group (for prose with text_file).
    pub current_sentence_group: Option<usize>,
    /// Tracks the start line of the current paragraph to detect transitions.
    pub current_paragraph_start: Option<usize>,
    /// Tracks (div1, div2) of the last synced dialogue line to detect scene transitions.
    pub current_sync_scene: Option<(i64, i64)>,
    pub nav_test: crate::input::nav_test::NavTestState,
    pub sync_enabled: bool,
    pub mpv_connected: bool,
    pub mpv_playing: bool,
    pub concordance_resume_playback: bool,
    pub sync_enabled_before_concordance: Option<bool>,
    pub skip_mpv_discovery: bool,
    pub debug_icon: gtk4::Label,
    pub word_status_label: gtk4::Label,
    pub chapter_toast: gtk4::Label,
    /// Generation counter for the chapter toast's hide timer. Each
    /// `show_chapter_toast` bumps this and the scheduled hide-timeout captures
    /// the value; a stale timeout (one whose generation has since been bumped
    /// by a newer toast) becomes a no-op, so rapid `;` presses can never have an
    /// earlier press's timer cut a later toast short. See show_chapter_toast.
    pub chapter_toast_gen: Rc<Cell<u64>>,
    /// Legacy flag for the retired persistent bottom act/scene toast. The
    /// always-visible running-head strip replaced that toast, so this now stays
    /// `false` for the app's lifetime; it is only read by the transient-toast
    /// borrow logic (`begin_chapter_toast_borrow`), where `false` means "no
    /// pill to restore — hide the strip when the transient clears". Kept rather
    /// than deleted to avoid churning the shared borrow-state struct.
    pub chapter_toast_persistent: Rc<Cell<bool>>,
    /// True while a transient bottom-center toast is borrowing the act/scene
    /// strip (a "Sync: on" / search / "Copied" / etc. message). While set,
    /// `refresh_persistent_chapter_toast` is a no-op so sync-driven cursor
    /// moves can't resurrect the act/scene pill underneath the transient. The
    /// transient's gen-guarded expiry clears it and restores the pill. See
    /// `show_transient_over_chapter_toast` / `refresh_persistent_chapter_toast`.
    pub chapter_toast_borrowed: Rc<Cell<bool>>,
    /// The act/scene pill text saved when a transient first borrows the strip
    /// (false→true edge of `chapter_toast_borrowed`), so the pill is restored
    /// verbatim when the LAST transient in a chain (spinner → "Saved") clears —
    /// snapshotting at each transient would capture the previous transient's
    /// text, not the pill's. `Some` only while borrowed; `None` when free or the
    /// pill is not persistent.
    pub chapter_toast_saved: Rc<RefCell<Option<String>>>,
    pub speed_toast: gtk4::Label,
    /// Centered bottom toast for search boundaries ("no earlier/later
    /// occurrence"). Placed like chapter_toast (centered, 32px from the bottom)
    /// so it stays fully visible below the card; separate widget so search
    /// messages never clobber the chapter/speed toast text.
    pub search_toast: gtk4::Label,
    pub word_cycle: crate::input::actions::word_copy::WordCycleState,
    pub word_status_timer: Rc<Cell<u64>>,
    pub word_bold_tag: gtk4::TextTag,
    /// True while display_work is replacing the buffer. CursorSync and other
    /// layout-dependent callbacks must skip when this is set because GTK
    /// hasn't laid out the new content yet. Cleared in an idle callback after
    /// the layout has settled.
    pub loading_work: Rc<Cell<bool>>,
    /// Set when loading_work clears so the resize tick can run a deferred
    /// layout refresh (apply_tiled_mode + snap) with correct line metrics.
    pub needs_layout_refresh: Rc<Cell<bool>>,
    /// One-shot: set by the two-column `hide_translations` branch after it
    /// restores the faithful pre-toggle `(current_line, page_top_line)`. The
    /// RESIZE_TICK layout-refresh path consumes it (`replace(false)`) to skip
    /// `snap_near_end_to_canonical`, so the saved canonical spread is painted
    /// verbatim instead of being re-derived from the cursor (which lands on the
    /// previous boundary when the cursor is the last line of the spread).
    pub trust_restored_page: Rc<Cell<bool>>,
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

/// Core of `AppState::is_play`, split out so it is unit-testable without an
/// `AppState`. A "play" is `work_type == "play"` exactly — NOT the whole
/// `!is_prose()` set (poem / sonnet_sequence / anthology are excluded).
pub(crate) fn work_type_is_play(work_type: &str) -> bool {
    work_type == "play"
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

    /// True when the work paginates one `(div1,div2)` section per page rather
    /// than filling each page to the viewport height. A `sonnet_sequence`
    /// renders one sonnet per page: each sonnet is its own section, and the
    /// single-column display clip honors the section break verbatim instead of
    /// reverting to a full viewport when the sonnet leaves the page underfilled
    /// (the fill-guard in `update_bottom_clip`).
    pub fn one_section_per_page(&self) -> bool {
        self.current_work.as_ref()
            .map(|w| w.work_type == "sonnet_sequence")
            .unwrap_or(false)
    }

    /// True for an anthology work (`work_type='anthology'`, e.g. DavidCrystalOP):
    /// one media performing excerpts from many works, where each excerpt is its
    /// own `(div1,div2)` section (div1 = excerpt index, not an act number).
    /// Anthology excerpts flow continuously to fill both columns rather than
    /// claiming a spread each (the play "stop at scene break" model is disabled),
    /// and the "next: Act N, Scene N" watermark is suppressed (its div1 is an
    /// excerpt index, so the label would be meaningless).
    pub fn is_anthology(&self) -> bool {
        self.current_work.as_ref()
            .map(|w| w.work_type == "anthology")
            .unwrap_or(false)
    }

    /// True only for a play (`work_type == "play"`). Distinct from `!is_prose()`,
    /// which also matches poem / sonnet_sequence / anthology. Used to decide
    /// whether the `+` chapter toast persists.
    pub fn is_play(&self) -> bool {
        self.current_work.as_ref()
            .map(|w| work_type_is_play(&w.work_type))
            .unwrap_or(false)
    }

    /// True when the `+` chapter toast should PERSIST (live "you are here"
    /// indicator) instead of auto-dismissing: a play, or prose that actually
    /// has chapter markers. Front-matter-only prose (no markers), non-play
    /// verse, and anthology are false — they keep the transient toast. The
    /// "has chapters" test mirrors `show_current_chapter`: prefer the line
    /// map's `chapter_breaks`, else scan `is_chapter` on the work lines.
    pub fn chapter_toast_persists(&self) -> bool {
        if self.is_play() {
            return true;
        }
        if !self.is_prose() {
            return false;
        }
        let has_chapters = if let Some(ref lm) = self.line_map {
            !lm.chapter_breaks.is_empty()
        } else {
            self.current_work
                .as_ref()
                .map(|w| w.lines.iter().any(|l| l.is_chapter))
                .unwrap_or(false)
        };
        has_chapters
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
    /// True only when the chat layout is open in its PINNED (single-column)
    /// form — the only placement where the card yields space to the panel.
    /// Float placements overlay a column and must NOT pin the card, so every
    /// apply_card_sizing site reads this, not chat_layout_open.
    pub fn chat_pinned(&self) -> bool {
        self.chat_layout_open
            && self.chat_placement == crate::input::actions::chat::ChatPlacement::Pinned
    }

    pub fn column_count(&self) -> u8 {
        if !matches!(self.config.navigation_mode, crate::config::NavigationMode::EReader) {
            return 1;
        }
        if self.translations_visible {
            return 1;
        }
        let Some(work) = self.current_work.as_ref() else {
            // No work loaded yet (early startup): use the count the last session
            // resolved, so the first card-sizing/formatting pass already matches
            // the target layout and there's no visible 1→2-column reflow. Falls
            // back to 1 when there's no saved value.
            return self.pending_column_count.unwrap_or(1).clamp(1, 2);
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

    /// Inverse of `work_line_for_buffer`: the buffer line rendering work line
    /// `work_idx`. Identity when no line map is loaded (DB-rendered works).
    pub fn buffer_line_for_work(&self, work_idx: usize) -> Option<usize> {
        if let Some(ref lm) = self.line_map {
            lm.work_to_buffer.get(work_idx).copied()
        } else {
            let count = self.current_work.as_ref().map_or(0, |w| w.lines.len());
            if work_idx < count { Some(work_idx) } else { None }
        }
    }

    /// Authoritative scene/section-boundary check for a buffer line, derived
    /// from the DB `(div1,div2)` columns at load (`LineMap.section_starts`).
    /// Returns `false` when the bitmap is absent (mid-load) or out of range —
    /// callers needing a mid-load fallback consult the buffer text directly.
    pub fn is_section_start(&self, buffer_line: usize) -> bool {
        if self.translations_visible && !self.translation_section_starts.is_empty() {
            return self.translation_section_starts.get(buffer_line).copied().unwrap_or(false);
        }
        self.line_map
            .as_ref()
            .and_then(|lm| lm.section_starts.get(buffer_line).copied())
            .unwrap_or(false)
    }

    /// Borrow the section-boundary bitmap, if a line map is loaded. Pagination
    /// helpers thread this slice down so they consult the authoritative DB
    /// boundary instead of re-inferring it from buffer text.
    ///
    /// While translations are visible the buffer is inflated with inserted
    /// translation lines, so the line_map's original-indexed bitmap no longer
    /// aligns. Return the translation-remapped bitmap built in
    /// `show_translations` instead, so the section-break clamp (and the
    /// one-section-per-page clip) lands on the correct physical line.
    pub fn section_starts(&self) -> Option<&[bool]> {
        if self.translations_visible && !self.translation_section_starts.is_empty() {
            return Some(self.translation_section_starts.as_slice());
        }
        self.line_map.as_ref().map(|lm| lm.section_starts.as_slice())
    }

    /// Get line_mapping.id for a buffer line, if available.
    pub fn line_mapping_id_for_buffer(&self, buffer_line: usize) -> Option<i64> {
        let work_idx = self.work_line_for_buffer(buffer_line)?;
        self.current_work.as_ref()?.lines.get(work_idx).map(|l| l.id)
    }

    /// The page image whose calibrated line range contains `line_id`. Pages with
    /// an uncalibrated (NULL) start are skipped; `end_line_id` is open-ended when
    /// NULL (the last calibrated page covers everything after its start). Returns
    /// None when no page matches (e.g. before calibration, or `line_id` precedes
    /// the first marked page). line_mapping ids are assigned in reading order, so
    /// a numeric id comparison is the page order.
    pub fn page_image_for_line_id(&self, line_id: i64) -> Option<&crate::db::models::PageImage> {
        self.page_image.images.iter().rev().find(|p| match p.start_line_id {
            Some(start) => {
                line_id >= start && p.end_line_id.map(|end| line_id <= end).unwrap_or(true)
            }
            None => false,
        })
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

    /// Record the currently-open gloss (from `self.gloss_context`) as the
    /// most-recently-viewed gloss for its work, and persist config. Called at
    /// every site that displays a gloss, so a freshly created gloss becomes
    /// "most recent" the instant it is shown. No-op if no gloss_context is set.
    pub fn record_last_gloss(&mut self, gloss_type: &str) {
        if let Some(ctx) = &self.gloss_context {
            let work = ctx.work_abbrev.clone();
            let entry = crate::config::LastGloss {
                start_citation: ctx.start_citation.clone(),
                gloss_type: gloss_type.to_string(),
            };
            crate::config::mark_work_dirty(&work);
            self.config.last_gloss.insert(work, entry);
            crate::config::save(&self.config);
        }
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

/// Vertical gap (pixels above) on each BCP body sentence line. BCP body prayers
/// are split one sentence per buffer line; this gap gives the airy, separated
/// "blank line between sentences" layout. Tunable.
pub const BCP_SENTENCE_GAP: i32 = 12;

/// Fixed height for the top spacer above the first text line. Doubles as the
/// running-head strip: tall enough to hold the head labels AND leave clear
/// padding between them and the first line of text below. Increasing this
/// shrinks the columns' usable height, so the live pagination engine fits one
/// fewer row per page — the pinned play_pages/prose_pages tables must be
/// regenerated at the new geometry to match (LIT_GEN_PAGE_TABLE / re-import).
pub const TOP_SPACER_HEIGHT: i32 = 74;

/// Pure default-column rule: works default to two columns, except a
/// `sonnet_sequence` and every prose work type, which default to one. A sonnet
/// sequence has each sonnet as its own `(div1, div2)` section, so the
/// two-column "stop at scene break" rule would push every sonnet to the right
/// column and leave the left empty; a single column lets the sonnets flow
/// top-to-bottom instead. Prose works (`novel`/`essay_collection`/`prose_book`/
/// `prose` — see `line_types::PROSE_TYPES`) render through the single-column
/// prose visual-row pagination engine (`src/input/prose_pages.rs`), which is
/// gated on `column_count()==1`; defaulting prose to two columns would route it
/// through the play engine and disable that engine entirely. Note anthologies
/// (`anthology`) are NOT prose — they deliberately pack two columns, so they
/// keep the `_ => 2` default. Split out from `default_column_count_for` so it is
/// unit-testable without constructing a `Work`. Per-work overrides in
/// `config.column_overrides` still take precedence (e.g. `Alt+[`), and
/// `column_count()` forces a single column when not in EReader mode or when
/// translations are visible.
pub(crate) fn default_column_count_for_parts(_author: &str, work_type: &str) -> u8 {
    if work_type == "sonnet_sequence" || crate::db::line_types::is_prose_work(work_type) {
        return 1;
    }
    2
}

/// Default column count for a work: 2 columns by default, 1 for a
/// `sonnet_sequence` or any prose work type.
pub(crate) fn default_column_count_for(work: &crate::db::models::Work) -> u8 {
    default_column_count_for_parts(&work.author, &work.work_type)
}


/// Fraction of the window width the two-column card aims to fill (minus the
/// outer margins). One-column works keep their fixed `column_width`.
pub const TWO_COLUMN_WIDTH_FRACTION: f32 = 0.68;

/// Whether to show verse line numbers in two-column mode. When false, the
/// left-column outer-foliation numbers and the right-column numbers are both
/// skipped, reclaiming ~40px per column for the text. (Experimental: flip to
/// `true` to restore the book-style foliation.)
pub const SHOW_LINE_NUMBERS_TWO_COL: bool = false;

/// Wrap-safe floor for a single column's width in two-column mode. Sized for the
/// widest DIALOGUE PROSE line, not just verse: the ~63-char Folger verse worst
/// case carries only `text_margins` + `TWO_COLUMN_LEFT_OFFSET`, but dialogue
/// prose also pays `TWO_COLUMN_DIALOGUE_INDENT`, so a wide prose line (e.g. H8
/// 5.3 Porter "fry of fornication is at door! On my Christian conscience,", 58
/// chars) overflowed at the old 700px floor. The text budget per column is
/// `floor − text_margins(40) − TWO_COLUMN_LEFT_OFFSET(30) − right margin`; a
/// 58-char Charter-19 line needs ~630–660px of that, so the floor must clear
/// ~760px. With line numbers hidden in two-column mode there's no gutter eating
/// the column. The card is never narrowed below `2 ×` this (plus the divider),
/// so shrinking `TWO_COLUMN_WIDTH_FRACTION` can never push a column into wrapping.
pub const MIN_TWO_COLUMN_COLUMN_WIDTH: i32 = 760;

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

    // Load theme from the app's own config (independent of the system-wide
    // theme; default kindle-sepia).
    let theme = crate::theme::load_theme_with_fallback(
        config.theme_name(),
        config.root_variant_for(config.theme_name()),
    );
    crate::logging::log("BUILD: loading_work guard active");
    crate::logging::log(&format!("Theme: {} ({})", theme.display_name, theme.name));
    crate::logging::log(&format!("Highlight color: {}", theme.cursor_line_bg));
    // MPV windows opened by linux-lit take the reader's root color as their
    // backdrop (letterbox/border matte + idle background). Record it now so the
    // first launch_mpv (which runs in a spawn_blocking closure that can't borrow
    // AppState) uses it; apply_theme_to_state refreshes it on later theme changes.
    crate::mpv::discovery::set_mpv_background(&theme.root_color);

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

    // Sentence-extent tint for the vocab-sentence loop mode: marks the whole
    // looping sentence while the phrase sweep (phrase_tag, added after this,
    // so it wins the overlap) moves inside it.
    let vocab_sentence_tag = gtk4::TextTag::builder()
        .name("vocab-sentence")
        .background(&theme.vocab_sentence_bg())
        .build();
    buffer.tag_table().add(&vocab_sentence_tag);

    // Span background (NOT paragraph_background): the karaoke tint covers only
    // the spoken phrase's chars inside the full-strength prose paragraph.
    let phrase_tag = gtk4::TextTag::builder()
        .name("phrase-highlight")
        .background(&theme.phrase_highlight_bg)
        .build();
    buffer.tag_table().add(&phrase_tag);

    // Prose nav-flash tint. No static color: flash_prose_cursor_line drives
    // paragraph_background_rgba per animation frame from theme.phrase_highlight_bg.
    let prose_flash_tag = gtk4::TextTag::builder()
        .name("prose-flash")
        .build();
    buffer.tag_table().add(&prose_flash_tag);

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

    let scansion_label_tag = gtk4::TextTag::builder()
        .name("scansion-label")
        .foreground(&theme.dim_fg)
        .build();
    buffer.tag_table().add(&scansion_label_tag);

    let translation_text_tag = gtk4::TextTag::builder()
        .name("translation-text")
        .pixels_above_lines(0)
        .pixels_below_lines(0)
        .build();
    buffer.tag_table().add(&translation_text_tag);

    let selection_tag = gtk4::TextTag::builder()
        .name("visual-selection")
        .background(crate::theme::selection_bg(&theme))
        .build();
    buffer.tag_table().add(&selection_tag);

    let vocab_tag = gtk4::TextTag::builder()
        .name("vocab-word")
        .foreground(&theme.vocab_fg)
        .build();
    buffer.tag_table().add(&vocab_tag);

    // Source lines covered by a reader-gloss passage are tinted with the
    // contrast-guarded off-cursor gloss color (theme.reader_gloss). Added after
    // the dim/cursor tags so this foreground wins over the dim foreground on a
    // glossed line; the cursor-line tag paints a paragraph background (not a
    // foreground). On the cursor's own line the on-cursor variant
    // (reader-gloss-cursor-line) is applied instead by
    // `repaint_reader_gloss_visible`. Both colors are refreshed on theme change
    // in `input::actions::settings`.
    let reader_gloss_tag = gtk4::TextTag::builder()
        .name("reader-gloss-line")
        .foreground(&theme.reader_gloss)
        .build();
    buffer.tag_table().add(&reader_gloss_tag);

    // The on-cursor glossed tint: same role as reader-gloss-line but a distinct
    // color, applied while a glossed line is the cursor block. Added after
    // reader-gloss-line so it outranks it on the cursor's own line.
    let reader_gloss_cursor_tag = gtk4::TextTag::builder()
        .name("reader-gloss-cursor-line")
        .foreground(&theme.reader_gloss_cursor)
        .build();
    buffer.tag_table().add(&reader_gloss_cursor_tag);

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
        crate::input::scroll::two_column_divider_bottom_px(&text_view),
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

    // Dim "Next: Act N, Scene M" watermark for an empty right column. Overlay
    // child (NOT buffer text — buffer text is measured by pagination and would
    // corrupt the right-column clip). Centered; hidden until snap_scroll_to_line
    // detects an empty right column with a following scene.
    let next_scene_watermark = gtk4::Label::new(None);
    next_scene_watermark.set_halign(gtk4::Align::Center);
    next_scene_watermark.set_valign(gtk4::Align::Center);
    next_scene_watermark.set_visible(false);
    next_scene_watermark.add_css_class("next-scene-watermark");
    right_scrolled_overlay.add_overlay(&next_scene_watermark);

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

    // Top spacer — one line height, rounded top corners only. Doubles as the
    // running-head strip: work abbrev at the start, position (act/scene or
    // chapter) at the end, with a hairline rule (CSS border-bottom) separating
    // it from the reading text.
    let top_spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    top_spacer.set_hexpand(true);
    top_spacer.set_height_request(TOP_SPACER_HEIGHT);
    top_spacer.add_css_class("card-top");
    top_spacer.add_css_class("running-head");

    let running_head_work = gtk4::Label::new(None);
    running_head_work.set_halign(gtk4::Align::Start);
    running_head_work.set_valign(gtk4::Align::Center);
    running_head_work.set_hexpand(true);
    running_head_work.add_css_class("running-head-work");

    let running_head_scene = gtk4::Label::new(None);
    running_head_scene.set_halign(gtk4::Align::End);
    running_head_scene.set_valign(gtk4::Align::Center);
    running_head_scene.set_hexpand(true);
    running_head_scene.add_css_class("running-head-scene");

    top_spacer.append(&running_head_work);
    top_spacer.append(&running_head_scene);

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
    // Vertical outer margins (window edge → card) are intentionally SMALLER than
    // the horizontal CARD_OUTER_MARGIN(24): a slightly taller card, whose gained
    // height funds more breathing room inside (header top-inset + bottom reserve)
    // WITHOUT costing a text row. See layout.rs `main_card_rect` (mirrors this in
    // its fallback height) and CARD_VERTICAL_OUTER_MARGIN. Horizontal start/end
    // are overwritten by apply_card_sizing; top/bottom are set once here.
    content_hbox.set_margin_top(crate::app::layout::CARD_VERTICAL_OUTER_MARGIN);
    content_hbox.set_margin_bottom(crate::app::layout::CARD_VERTICAL_OUTER_MARGIN);
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

    // Journal overlay wraps the gloss overlay
    let journal_overlay = crate::ui::journal_overlay::JournalOverlay::new(config.column_width, config.text_margins);
    journal_overlay.attach(&gloss_overlay.overlay);
    journal_overlay.overlay.set_vexpand(true);

    // Thread the `<hi>` highlight background (theme current-line color) into the
    // overlays so visual-mode highlights render the theme marker color.
    let highlight_bg = crate::theme::selection_bg(&theme);
    gloss_overlay.set_highlight_color(highlight_bg);
    journal_overlay.set_highlight_color(highlight_bg);
    // Page-marker glyph color = theme dim foreground (matches the old CSS).
    gloss_overlay.set_marker_color(&theme.dim_fg);
    journal_overlay.set_marker_color(&theme.dim_fg);
    gloss_overlay.set_panel_color(&theme.overlay_panel_bg);
    journal_overlay.set_panel_color(&theme.overlay_panel_bg);
    journal_overlay.set_bar_color(&theme.root_color);
    // Overlay search-highlight tints (variants of the cursor-segment color).
    // MUST be set here at startup too — apply_theme_to_state (the only other
    // caller of set_search_colors) does NOT run on a fresh launch, only on
    // Ctrl+t / SIGUSR1 / snapshot-revert; without this the overlays kept their
    // hardcoded placeholder yellow/orange until the user cycled the theme.
    let (search_all, search_current) = theme.search_highlight_colors();
    gloss_overlay.set_search_colors(&search_all, &search_current);
    journal_overlay.set_search_colors(&search_all, &search_current);
    // Ephemeral rewrite diff-highlight tint follows the same "all matches"
    // search color (Task 4 of the rewrite-revision-history feature).
    gloss_overlay.set_rewrite_diff_color(&search_all);
    journal_overlay.set_rewrite_diff_color(&search_all);

    // Journal picker overlays the journal overlay (above journal, below translation)
    let journal_picker = JournalQaPicker::new();
    journal_picker.attach(&journal_overlay.overlay);
    journal_picker.overlay.set_vexpand(true);

    // Journal move-to-band picker overlays the journal Q&A picker
    let journal_move_picker = JournalMovePicker::new();
    journal_move_picker.attach(&journal_picker.overlay);
    journal_move_picker.overlay.set_vexpand(true);

    // Journal term-browse input overlays the journal move picker
    let journal_term_input = crate::ui::journal_term_input::JournalTermInput::new();
    journal_term_input.attach(&journal_move_picker.overlay);
    journal_term_input.overlay.set_vexpand(true);

    // Translation overlay wraps the journal term input overlay
    let translation_overlay = crate::ui::translation_overlay::TranslationOverlay::new();
    translation_overlay.attach(&journal_term_input.overlay);
    translation_overlay.overlay.set_vexpand(true);

    // Gloss picker wraps the translation overlay
    let gloss_picker = GlossPicker::new();
    gloss_picker.attach(&translation_overlay.overlay);
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

    // Corpus search popup (Ctrl+f: cross-corpus journal/gloss regex search)
    // wraps the authorship picker. Downstream `add_overlay`/`attach_to` panels
    // below chain off THIS overlay (not authorship_picker.overlay) so the new
    // layer sits in the overlay stack like the others.
    let corpus_search_popup = crate::ui::corpus_search_popup::CorpusSearchPopup::new();
    corpus_search_popup.attach(&authorship_picker.overlay);
    corpus_search_popup.overlay.set_vexpand(true);

    // Echo turns picker (Ctrl+Shift+G: list all turns in this work that have
    // echoes). add_overlay panel onto the outer overlay, NOT wrapped into the
    // reader's size-bearing chain (wrapping collapses the reader layout).
    let echo_turns_picker = crate::ui::echo_turns_picker::EchoTurnsPicker::new();
    corpus_search_popup.overlay.add_overlay(echo_turns_picker.picker_box());

    // Echo line picker (add-echo: choose a line to attach an echo to).
    // Added as an overlay panel onto the outer overlay (like concordance_works
    // below), NOT wrapped into the reader's size-bearing chain — wrapping it
    // orphaned the reader content and collapsed the layout (sw_h stuck at 0).
    let echo_line_picker = crate::ui::echo_line_picker::EchoLinePicker::new();
    corpus_search_popup.overlay.add_overlay(&echo_line_picker.picker_box);

    // Settings overlay panels (scrim + card). Added here as add_overlay panels
    // on the OUTERMOST overlay — NOT via the chain link at settings_overlay.attach
    // above — so settings renders ABOVE the gloss/synopsis overlay (which is a
    // chain link lower in the z-stack). This lets Ctrl+, from those overlays show
    // settings on top while the overlay stays visible behind it. Added before the
    // voice picker so the voice picker (opened from the settings Voice row) layers
    // above settings.
    {
        let (settings_scrim, settings_card) = settings_overlay.panels();
        corpus_search_popup.overlay.add_overlay(settings_scrim);
        corpus_search_popup.overlay.add_overlay(settings_card);
    }

    // Voice picker (settings overlay → Voice row). add_overlay panel, NOT a
    // chain link (chain insertion collapses the reader layout).
    let voice_picker = crate::ui::voice_picker::VoicePicker::new();
    corpus_search_popup.overlay.add_overlay(&voice_picker.picker_box);

    // Echo keybinds legend (Ctrl+/ in the echoes overlay). add_overlay panel,
    // NOT a chain link (chain insertion collapses the reader layout).
    let echo_keybinds_overlay = crate::ui::keybinds_legend::KeybindsLegend::new(
        crate::ui::echo_keybinds_overlay::TITLE,
        crate::ui::echo_keybinds_overlay::GROUPS,
    );
    echo_keybinds_overlay.attach_to(&corpus_search_popup.overlay);

    // Per-overlay Ctrl+/ keybind legends (gloss, synopsis, journal). add_overlay
    // panels on a persistent outer overlay, NOT chain links.
    use crate::ui::keybinds_legend::KeybindsLegend;
    let gloss_keybinds_overlay = KeybindsLegend::new(
        crate::ui::gloss_keybinds_overlay::TITLE,
        crate::ui::gloss_keybinds_overlay::GROUPS,
    );
    gloss_keybinds_overlay.attach_to(&corpus_search_popup.overlay);
    let synopsis_keybinds_overlay = KeybindsLegend::new(
        crate::ui::synopsis_keybinds_overlay::TITLE,
        crate::ui::synopsis_keybinds_overlay::GROUPS,
    );
    synopsis_keybinds_overlay.attach_to(&corpus_search_popup.overlay);
    let journal_keybinds_overlay = KeybindsLegend::new(
        crate::ui::journal_keybinds_overlay::TITLE,
        crate::ui::journal_keybinds_overlay::GROUPS,
    );
    journal_keybinds_overlay.attach_to(&corpus_search_popup.overlay);

    // Concordance works picker (Alt+R: jump to a specific work)
    let concordance_works_picker = crate::ui::concordance_works_picker::ConcordanceWorksPicker::new();
    corpus_search_popup.overlay.add_overlay(&concordance_works_picker.scrim);
    corpus_search_popup.overlay.add_overlay(&concordance_works_picker.container);

    // Page-scan image overlay (toggle the BCP card to its leaf PNG). Added onto
    // `page_turn_overlay`, which wraps `card_vbox` (the visible cream card), so it
    // spans EXACTLY the card geometry — not the whole window (which would include
    // the media footer below the card). Fills the card; `Contain` fits the leaf.
    let page_image_overlay = crate::ui::page_image_overlay::PageImageOverlay::new();
    page_image_overlay.attach_to(&page_turn_overlay);

    // Action popup overlay for visual mode
    let action_popup_widget = crate::ui::action_popup::ActionPopup::new();
    corpus_search_popup.overlay.add_overlay(&action_popup_widget.container);

    // Add vocab popup to full-width overlay so it appears to the right of the text card
    vocab_popup.attach_to(&corpus_search_popup.overlay);

    // Debug-mode indicator (lower-left corner, next to sync icon, hidden by default)
    let debug_icon = gtk4::Label::new(Some("⚙"));
    debug_icon.set_valign(gtk4::Align::End);
    debug_icon.set_halign(gtk4::Align::Start);
    debug_icon.set_margin_start(44);
    debug_icon.set_margin_bottom(12);
    debug_icon.add_css_class("debug-icon");
    debug_icon.set_visible(false);
    corpus_search_popup.overlay.add_overlay(&debug_icon);
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
    corpus_search_popup.overlay.add_overlay(&word_status_label);

    // Transient status toasts (Copied / Sync: on / No timestamp / Saved /
    // spinners) float at the TOP CENTER, inside the header band (the 64px
    // `top_spacer`, which sits `content_hbox` margin_top=24 below the window
    // top). They overlay the running-head strip's center gap — the work abbrev
    // (left) and act/scene (right) labels stay visible on the sides. margin_top
    // ≈ card_top(24) + ~header-center so the toast lines up with those labels.
    let chapter_toast = gtk4::Label::new(None);
    chapter_toast.set_valign(gtk4::Align::Start);
    chapter_toast.set_halign(gtk4::Align::Center);
    chapter_toast.set_margin_top(40);
    chapter_toast.add_css_class("chapter-toast");
    chapter_toast.set_visible(false);

    // Shared status toast (Sync: on/off, Speed: Nx, Copied …). Despite the name
    // it is the common bottom-center status widget; every caller re-centers it at
    // runtime (set_halign(Center) + reset horizontal margins) and relies on this
    // valign/margin_top for vertical placement. Now TOP-center, in the header
    // band, next to chapter_toast/search_toast — the callers set only halign +
    // horizontal margins, so this default is the single source of vertical
    // placement for all of them.
    let speed_toast = gtk4::Label::new(None);
    speed_toast.set_valign(gtk4::Align::Start);
    speed_toast.set_halign(gtk4::Align::Center);
    speed_toast.set_margin_top(40);
    speed_toast.add_css_class("center-toast");
    speed_toast.set_visible(false);

    // Search boundary toast ("No later/earlier occurrence"): top center, in the
    // header band alongside the status toast (chapter_toast). Small font
    // (.search-toast). Sits at the same margin_top so the two never appear at
    // different heights.
    let search_toast = gtk4::Label::new(None);
    search_toast.set_valign(gtk4::Align::Start);
    search_toast.set_halign(gtk4::Align::Center);
    search_toast.set_margin_top(42);
    search_toast.add_css_class("search-toast");
    search_toast.set_visible(false);

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
    corpus_search_popup.overlay.add_overlay(&search_bar.container);

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    vbox.append(&corpus_search_popup.overlay);

    // Left chat panel (Tab chat layout): bare-on-root overlay, not in the
    // size-bearing widget chain (see feedback_picker_overlay_not_chain).
    // Hidden until toggle_chat_layout shows it and sizes it via size_panel.
    let chat_panel = crate::ui::chat_panel::ChatPanel::new();
    chat_panel.container.set_halign(gtk4::Align::Start);
    chat_panel.container.set_valign(gtk4::Align::Center);

    concordance_bar.container.set_valign(gtk4::Align::End);
    title_bar.set_valign(gtk4::Align::End);
    let outer_overlay = gtk4::Overlay::new();
    outer_overlay.set_child(Some(&vbox));
    outer_overlay.add_overlay(&concordance_bar.container);
    outer_overlay.add_overlay(&title_bar);
    outer_overlay.add_overlay(&chat_panel.container);
    // Transient toasts sit on the OUTER overlay, added AFTER the chat panel, so
    // they render on top of the floating chat panel rather than behind it (the
    // chat panel is left-aligned + center-valigned and would otherwise obscure
    // the bottom-left speed_toast and clip the bottom-center chapter/search
    // toasts). Pinning (valign/halign/margins) is set at their creation above.
    outer_overlay.add_overlay(&chapter_toast);
    outer_overlay.add_overlay(&speed_toast);
    outer_overlay.add_overlay(&search_toast);

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
    // Captured before `config` is moved into AppState; seeds the early-startup
    // column-count guess so the first card pass matches the target layout.
    // `LIT_START_COLUMNS` overrides it for hermetic test runs (config writeback is
    // suppressed under LIT_HEADLESS_TEST, so the persisted value can't be relied
    // on there) — mirrors LIT_START_WORK / LIT_START_POS.
    let pending_column_count = std::env::var("LIT_START_COLUMNS")
        .ok()
        .and_then(|v| v.trim().parse::<u8>().ok())
        .map(|n| n.clamp(1, 2))
        .or(config.last_column_count);

    let state = Rc::new(RefCell::new(AppState {
        text_view,
        buffer,
        library_picker: picker,
        current_work: None,
        current_line: 0,
        prev_highlight_line: std::cell::Cell::new(None),
        page_table: std::cell::RefCell::new(None),
        page_table_fp: std::cell::RefCell::new(String::new()),
        page_table_gen_attempted: std::cell::Cell::new(false),
        prose_page_table: std::cell::RefCell::new(None),
        prose_page_table_fp: std::cell::RefCell::new(String::new()),
        prose_page_table_gen_attempted: std::cell::Cell::new(false),
        left_clip_boundary: std::cell::Cell::new(None),
        right_clip_boundary: std::cell::Cell::new(None),
        page_top_line: 0,
        page_top_offset: 0,
        page_back_stack: Vec::new(),
        dim_tag,
        cursor_line_tag,
        cursor_fade_tag,
        ab_dim_tag,
        phrase_tag,
        vocab_sentence_tag,
        prose_flash_tag,
        phrase_cache: None,
        active_phrase: None,
        phrase_paint_hold: None,
        page_turn_overlay: page_turn_overlay.clone(),
        bottom_clip,
        top_spacer,
        running_head_work,
        running_head_scene,
        card_vbox,
        scrolled_window: scrolled,
        scrolled_overlay,
        right_view,
        right_scrolled_window: right_scrolled,
        right_scrolled_overlay,
        right_bottom_clip,
        next_scene_watermark,
        columns_hbox,
        column_divider,
        right_line_number_renderer: None,
        right_gutter_renderer: None,
        content_hbox: content_hbox.clone(),
        chat_layout_open: false,
        chat_regate_pending: false,
        chat_placement: crate::input::actions::chat::ChatPlacement::Pinned,
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
        prose_flash_anim: None,
        pending_prose_flash: std::cell::Cell::new(false),
        prose_flash_hold: std::cell::Cell::new(false),
        cmd_tx,
        tokio_handle: tokio_handle.clone(),
        playback_speed: 1.0,
        search_bar,
        search_matches: Vec::new(),
        search_match_idx: 0,
        last_search_query: None,
        search_backward: false,
        search_return_pos: None,
        gloss_return_pos: None,
        last_overlay: None,
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
        pending_column_count,
        sign_visible_before_translations: None,
        pre_translation_page: None,
        translation_lines: Vec::new(),
        translation_section_starts: Vec::new(),
        translation_dim_tag,
        translation_text_tag,
        scansion_label_tag,
        scansion: ScansionState {
            label_starts: std::collections::HashMap::new(),
            level: crate::scansion::ScanLevel::Off,
            data: std::collections::HashMap::new(),
        },
        suppress_sync_until: None,
        pending_advance: None,
        pending_advance_ignore_bl: None,
        pending_prose_cross: None,
        visual_selection: None,
        selection_tag,
        action_popup: None,
        action_popup_widget,
        keybinds_overlay,
        gamepad_overlay,
        gloss_overlay,
        journal_overlay,
        journal_picker,
        journal_move_picker,
        journal_term_input,
        journal_band: JournalBand::Scene(0, 0),
        journal: crate::input::actions::journal::JournalState {
            pages: Vec::new(),
            page_index: 0,
            return_pos: None,
            prompt_mode: JournalPromptMode::Ask,
            pending_passage: None,
            picker_from_reader: false,
            vim_rewrite: None,
            entry_page_id: None,
            filter: None,
            search: None,
            last_pattern: None,
        },
        page_image_overlay,
        page_image: PageImageState::default(),
        tts: crate::tts::TtsPlayer::new(),
        translation_overlay,
        gloss_original_text: None,
        gloss_list: Vec::new(),
        gloss_index: 0,
        gloss_context: None,
        gloss_passages: Vec::new(),
        gloss_passage_index: 0,
        gloss_opened_from_picker: false,
        gloss_picker_from_overlay: false,
        gloss_active_voice: 0,
        gloss_search: None,
        gloss_last_pattern: None,
        overlay_search_origin: InputMode::JournalOverlay,
        voice_picker_origin: VoicePickerOrigin::Settings,
        settings_return_mode: InputMode::Reader,
        keybinds_return_mode: InputMode::Reader,
        gloss_picker_filter: crate::input::actions::pickers::GlossPickerFilter::default(),
        gloss_prompt_mode: GlossPromptMode::Add,
        delete_confirm_container: None,
        delete_confirm_overlay: None,
        delete_confirm_origin: None,
        rewrite_target_container: None,
        rewrite_target_overlay: None,
        gloss_undo: None,
        journal_undo: None,
        rewrite_browse: None,
        undo_confirm_origin: None,
        undo_confirm_container: None,
        undo_confirm_overlay: None,
        gloss_picker,
        echo_picker,
        echo_turns_picker,
        pending_echo_context: None,
        pending_echo_scene_lines: Vec::new(),
        pending_echo_passage_doc: String::new(),
        echo_overlay: crate::input::actions::echoes::EchoOverlayState::default(),
        echo_session: None,
        vocab_words: std::collections::HashSet::new(),
        vocab_matches: Vec::new(),
        vocab_loop: None,
        vocab_tag,
        reader_gloss_tag,
        reader_gloss_cursor_tag,
        reader_gloss_lines: std::collections::HashSet::new(),
        dim_enabled,
        vocab_highlight_visible: false,
        vocab_popup: crate::app::vocab_popup::VocabPopupState {
            popup: vocab_popup,
            data: Vec::new(),
            index: 0,
            view: crate::ui::vocab_popup::VocabView::Definition,
            auto: false,
            line: None,
            fade_gen: Rc::new(Cell::new(0)),
            journal: None,
        },
        sidebar_mode: SidebarMode::Vocab,
        synopsis_cache: HashMap::new(),
        synopsis_visible: false,
        synopsis_overlay_scene: (0, 0),
        synopsis_amend_scene: (0, 0),
        synopsis_undo: None,
        synopsis_prompt_kind: SynopsisPromptKind::Ask,
        concordance_picker,
        concordance_state: None,
        concordance_origin: None,
        concordance_word_cache: None,
        concordance_word_picker,
        voice_picker,
        echo_line_picker,
        echo_keybinds_overlay,
        gloss_keybinds_overlay,
        synopsis_keybinds_overlay,
        journal_keybinds_overlay,
        echo_add_turn_id: None,
        concordance_list_picker,
        concordance_works_picker,
        concordance_bar,
        title_bar,
        title_bar_label,
        title_bar_scene_label,
        chat_panel,
        chat: Default::default(),
        current_sentence_group: None,
        current_paragraph_start: None,
        current_sync_scene: None,
        nav_test: crate::input::nav_test::NavTestState::default(),
        sync_enabled: true,
        mpv_connected: false,
        mpv_playing: false,
        concordance_resume_playback: false,
        sync_enabled_before_concordance: None,
        skip_mpv_discovery: false,
        debug_icon,
        word_status_label,
        chapter_toast,
        chapter_toast_gen: Rc::new(Cell::new(0)),
        chapter_toast_persistent: Rc::new(Cell::new(false)),
        chapter_toast_borrowed: Rc::new(Cell::new(false)),
        chapter_toast_saved: Rc::new(RefCell::new(None)),
        speed_toast,
        search_toast,
        word_cycle: crate::input::actions::word_copy::WordCycleState::default(),
        word_status_timer: Rc::new(Cell::new(0)),
        word_bold_tag,
        // If we have an MRU work to load, mark loading_work=true now so the
        // 500ms reveal grace doesn't fire before display_work runs and
        // expose an empty vbox. Cleared by update_highlight_and_show.
        loading_work: Rc::new(Cell::new(last_work.is_some())),
        needs_layout_refresh: Rc::new(Cell::new(false)),
        trust_restored_page: Rc::new(Cell::new(false)),
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
        corpus_search_popup,
        last_corpus: crate::input::corpus_search::Corpus::Journal,
        corpus_search_return_mode: InputMode::Reader,
        input_mode: InputMode::Reader,
        tts_batch_running: std::cell::Cell::new(false),
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
            let (top, off) = (s.page_top_line, s.page_top_offset);
            crate::input::scroll::snap_scroll_to_line_offset(&mut s, top, off);
            // Startup orientation cue: karaoke-tint the phrase that will play
            // (the resume line's start time) instead of flashing the cursor
            // paragraph. Repaint here because snap/highlight work since
            // display_work's paint may have cleared the tint. No-op while
            // playing / for works without phrase data / karaoke off.
            crate::input::phrase_highlight::show_startup_phrase(&mut s);
        }
    }
    {
        let vbox_for_reveal = vbox.clone();
        let state_for_reveal = Rc::clone(&state);
        glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
            if vbox_for_reveal.opacity() < 1.0 {
                // Reveal here ONLY in the true picker case: no work loading AND no
                // post-load layout refresh pending. A resumed work clears
                // `loading_work` quickly but leaves `needs_layout_refresh` set
                // until the resize tick snaps + settles + reveals; revealing here
                // in that window shows the pre-layout spread and then visibly
                // re-flows. Defer to the resize-tick reveal in that case.
                let (loading, refresh_pending) = state_for_reveal
                    .try_borrow()
                    .map(|s| (s.loading_work.get(), s.needs_layout_refresh.get()))
                    .unwrap_or((true, true));
                if !loading && !refresh_pending {
                    crate::logging::log("STARTUP: revealing vbox (500ms grace, no work loading)");
                    reveal_snap(&state_for_reveal);
                    vbox_for_reveal.set_opacity(1.0);
                } else {
                    crate::logging::log("STARTUP: 500ms grace skipped — work load / layout refresh pending; waiting for resize-tick reveal");
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
        // Bounds the two-column width-settle wait so the tick can't spin forever
        // when the columns can never reach the balance band (e.g. a viewport too
        // narrow to hold two MIN_TWO_COLUMN_COLUMN_WIDTH columns: GTK shrinks them
        // below the band floor, near_target stays false, and layout/reveal would
        // otherwise block indefinitely — observed wedging the headless fuzz).
        let settle_attempts: Rc<Cell<u32>> = Rc::new(Cell::new(0));
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
                        let cw = crate::app::layout::effective_column_width(&s);
                        let cc = s.column_count();
                        let tr = s.translations_visible;
                        apply_card_sizing(&content_hbox_tick, ww, cw, cc, tr, s.chat_pinned());
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
                    let cw = crate::app::layout::effective_column_width(&s);
                    let cc = s.column_count();
                    let tr = s.translations_visible;
                    apply_card_sizing(&content_hbox_tick, ww, cw, cc, tr, s.chat_pinned());
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
                        if near_target && balanced {
                            settle_attempts.set(0);
                        } else {
                            // Cap the wait: ~60 ticks ≈ 1s at 60fps. If the columns
                            // still haven't reached the band, they likely never will
                            // (viewport too narrow to fit two full columns) — proceed
                            // with the current geometry rather than blocking forever.
                            const MAX_SETTLE_TICKS: u32 = 60;
                            let n = settle_attempts.get() + 1;
                            settle_attempts.set(n);
                            if n <= MAX_SETTLE_TICKS {
                                crate::log_fmt!(
                                    "RESIZE_TICK: two-col width not settled (left_w={} right_w={} band={}..={} balanced={}), waiting ({}/{})",
                                    lw, rw, lo, hi, balanced, n, MAX_SETTLE_TICKS
                                );
                                return glib::ControlFlow::Continue;
                            }
                            crate::log_fmt!(
                                "RESIZE_TICK: two-col width never settled (left_w={} right_w={} band={}..={}) after {} ticks — proceeding with current geometry",
                                lw, rw, lo, hi, MAX_SETTLE_TICKS
                            );
                            settle_attempts.set(0);
                        }
                    }
                    crate::log_fmt!("RESIZE_TICK: deferred layout refresh, sw_h={}", sw_h);
                    s.needs_layout_refresh.set(false);
                    // Re-apply the chat panel geometry at the now-settled
                    // column layout: a floating panel positioned earlier (or
                    // about to be positioned by the deferred regate below)
                    // reads the column overlays' compute_bounds, which are
                    // only trustworthy once the two-column block has finished
                    // centering.
                    if s.chat_layout_open {
                        crate::input::actions::chat::size_panel(&s);
                    }
                    do_reveal = vbox_for_tick.opacity() < 1.0;
                    // Pinned play pagination: once layout is settled, generate+store
                    // the page table if this work/layout doesn't have one (no-op when
                    // one was already loaded from lit.db, or on any fallback mode).
                    // Gated to the deferred-layout-refresh branch only — this must
                    // NOT fire on every qualifying resize tick (width/height
                    // changes), since column geometry (and thus the page table)
                    // is only settled here, not mid-resize.
                    {
                        let st = state_for_tick.clone();
                        glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
                            {
                                let s = st.borrow();
                                crate::input::page_table::load_for_work(&s);
                                crate::input::page_table::generate_and_store(&s);
                                crate::input::prose_pages::load_for_prose_work(&s);
                            }
                            // Prose generation needs &mut (drives page_top during
                            // the walk); the play gate no-ops it for a 2-col work
                            // and vice versa, so the two are mutually exclusive.
                            if let Ok(mut s) = st.try_borrow_mut() {
                                crate::input::prose_pages::generate_and_store_prose(&mut s);
                            }
                            // The load/gen above may have swapped the active grid
                            // (e.g. startup snapped to a stored table for another
                            // fingerprint that was just dropped) — re-anchor an
                            // off-grid page top before the user sees a mid-page
                            // cursor jump on the first sync page turn.
                            if let Ok(mut s) = st.try_borrow_mut() {
                                crate::input::page_table::resnap_to_table(&mut s);
                                crate::input::prose_pages::resnap_prose_to_table(&mut s);
                                // Orientation cue AFTER the page has settled at
                                // final geometry: re-assert the startup karaoke
                                // tint (the resnap above may have repainted the
                                // buffer and dropped the reveal-time paint).
                                crate::input::phrase_highlight::show_startup_phrase(&mut s);
                            }
                        });
                    }
                } else {
                    if width_changed {
                        let cw = crate::app::layout::effective_column_width(&s);
                        let cc = s.column_count();
                        let tr = s.translations_visible;
                        apply_card_sizing(&content_hbox_tick, ww, cw, cc, tr, s.chat_pinned());
                        if s.chat_layout_open {
                            crate::input::actions::chat::size_panel(&s);
                        }
                        apply_tiled_mode(&mut s, &vbox_for_tick, ww);
                    }
                    if width_changed || height_changed {
                        // Pinned play pagination: a plain window resize (dwl
                        // retiling — routine, not a work load) leaves any loaded
                        // page table active with boundaries for the OLD geometry
                        // unless we check the fingerprint here too. This must also
                        // fire on a HEIGHT-ONLY change (e.g. dwl stack-retiling
                        // that changes height without width) — the fingerprint
                        // includes height, so a height-only resize can go stale
                        // just as easily as a width-only one. Same settle delay
                        // as the deferred-layout-refresh branch above so column
                        // geometry has actually finished reflowing before we
                        // fingerprint it; does not regenerate — see
                        // revalidate_on_resize's doc comment.
                        let st = state_for_tick.clone();
                        glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
                            {
                                let s = st.borrow();
                                crate::input::page_table::revalidate_on_resize(&s);
                                crate::input::prose_pages::revalidate_prose_on_resize(&s);
                            }
                            // Same grid-swap re-anchor as the settled-layout hook.
                            if let Ok(mut s) = st.try_borrow_mut() {
                                // Task 9: a resize may have dropped/regenerated the
                                // prose grid; a cross scheduled against the old grid
                                // is now meaningless. Cancel it (it re-schedules on
                                // the next CursorSync if still warranted).
                                s.pending_prose_cross = None;
                                crate::input::page_table::resnap_to_table(&mut s);
                                crate::input::prose_pages::resnap_prose_to_table(&mut s);
                            }
                        });
                    }
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
                // A translation hide restores the faithful pre-toggle canonical
                // spread; consume the one-shot flag and skip the canonical
                // re-derivation (which would resolve the saved cursor — the last
                // line of the spread — to the previous boundary, painting the
                // wrong spread). Work-load / resize still snap normally.
                if !s.trust_restored_page.replace(false) {
                    snap_near_end_to_canonical(&mut s);
                }
                // Offset-aware: a prose page top can sit MID-paragraph
                // ((line, px_offset) from the row-fill grid). Snapping to the
                // line's row 0 here silently scrolled 171px above the stored
                // boundary after the startup resnap, rendering rows the page
                // table (and the bottom clip) doesn't account for — the
                // "clipped partial row at the bottom on startup" bug.
                let (top, off) = (s.page_top_line, s.page_top_offset);
                crate::input::scroll::snap_scroll_to_line_offset(&mut s, top, off);
                // The synchronous snap above can race ahead of GTK's layout pass:
                // when a buffer swap (a scansion toggle) changes the content
                // height, `adjustment.upper` is momentarily stale, so column_split
                // measures against the wrong height and the spread blanks (observed
                // upper 103321 at snap time, settling to 71654 one tick later).
                // Re-snap once on idle, after layout settles, so the final spread
                // is computed against the real geometry.
                let state_idle = Rc::clone(&state_for_tick);
                glib::idle_add_local_once(move || {
                    if let Ok(mut s) = state_idle.try_borrow_mut() {
                        crate::input::navigation::invalidate_page_tops(&s);
                        crate::input::scroll::ensure_scroll_range(&s);
                        let (top, off) = (s.page_top_line, s.page_top_offset);
                        crate::input::scroll::snap_scroll_to_line_offset(&mut s, top, off);
                    }
                });
                // Reveal LAST: apply_tiled_mode, snap_scroll, and the label
                // update inside snap can all shift visible geometry. Doing
                // them before opacity=1 keeps everything stable when the
                // user first sees the window.
                if do_reveal {
                    crate::log_fmt!("STARTUP: revealing vbox (sw_h={})", s.scrolled_window.height());
                    vbox_for_tick.set_opacity(1.0);
                    let (top, off) = (s.page_top_line, s.page_top_offset);
                    crate::input::scroll::snap_scroll_to_line_offset(&mut s, top, off);
                    // Headless UI test harness: emit the reading viewport's
                    // rectangle in window (== screenshot) coordinates so the
                    // line-clipping detector can target it via --region.
                    // sourceview5::View exposes no AT-SPI Text interface, so this
                    // log line is how the harness locates the pane.
                    crate::input::scroll::emit_test_viewport_rect(&s);
                }
                // Chat layout: a work switch with the panel open deferred its
                // re-gate to here (see `chat::on_work_switched`) because
                // `s.window.width()` at the switch hook point can be a
                // transient, not-yet-settled size. Only consume the flag on a
                // frame where the width did NOT change — that's the settled
                // case. The regate itself runs on a short timeout, NOT
                // synchronously: the float placement positions the panel from
                // the column overlays' compute_bounds, and those rects only
                // update after GTK's next layout pass — regating inside this
                // very frame read mid-centering rects and put the panel ~90px
                // off its column.
                if s.chat_regate_pending && !width_changed {
                    s.chat_regate_pending = false;
                    let st = state_for_tick.clone();
                    glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                        if let Ok(mut s) = st.try_borrow_mut() {
                            crate::input::actions::chat::regate_panel(&mut s);
                        }
                    });
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // Connect picker search entry filter
    let state_for_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.library_picker.search_entry().connect_changed(move |entry| {
            let text = entry.text();
            state_for_filter.borrow().library_picker.populate_list(&text);
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

    // Connect journal Q&A picker search entry filter.
    //
    // BORROW SAFETY (applies to all four picker filters below): a picker's
    // `show()`/reset calls `search_entry.set_text("")`, which SYNCHRONOUSLY
    // emits `changed`. The open path that calls `show()` holds `borrow_mut()`,
    // so a plain `state.borrow()` here would be a RefCell double-borrow → a
    // non-unwinding panic inside the GTK callback (the `f` term-box crash).
    // Use `try_borrow()` and skip on contention: a programmatic clear under the
    // open path's borrow needs no re-filter (the open path already populated the
    // list); only genuine user typing (no borrow held) should re-filter.
    let state_for_journal_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.journal_picker.search_entry().connect_changed(move |entry| {
            if let Ok(st) = state_for_journal_filter.try_borrow() {
                st.journal_picker.populate_list(&entry.text());
            }
        });
    }

    // Connect journal move-picker search entry filter
    let state_for_journal_move_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.journal_move_picker.search_entry().connect_changed(move |entry| {
            if let Ok(st) = state_for_journal_move_filter.try_borrow() {
                st.journal_move_picker.populate_list(&entry.text());
            }
        });
    }

    // Connect journal term-input search entry filter (live tag suggestions)
    let state_for_journal_term_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.journal_term_input.search_entry().connect_changed(move |entry| {
            if let Ok(st) = state_for_journal_term_filter.try_borrow() {
                st.journal_term_input.populate_list(&entry.text());
            }
        });
    }

    // Connect corpus-search popup search entry filter (Ctrl+f). GUARDED:
    // open() calls search_entry().set_text("") which fires this synchronously
    // under its own borrow_mut; a plain borrow() would double-borrow and abort.
    let state_for_corpus_search = Rc::clone(&state);
    {
        let s = state.borrow();
        s.corpus_search_popup.search_entry().connect_changed(move |entry| {
            if let Ok(st) = state_for_corpus_search.try_borrow() {
                st.corpus_search_popup.populate_list(&entry.text());
            }
        });
    }

    // Connect concordance picker search entry filter
    let state_for_concordance_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.concordance_picker.search_entry().connect_changed(move |entry| {
            if let Ok(st) = state_for_concordance_filter.try_borrow() {
                st.concordance_picker.populate_list(&entry.text());
            }
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

    // Connect voice picker search entry filter
    let state_for_voice_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.voice_picker.entry().connect_changed(move |_| {
            state_for_voice_filter.borrow().voice_picker.filter_changed();
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
    // Cloned up front for the key-RELEASE handler below (the pressed closure
    // moves the originals).
    let state_for_release = Rc::clone(&state);
    let key_state_release = Rc::clone(&key_state);
    let key_controller = EventControllerKey::new();
    key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
    key_controller.connect_key_pressed(move |_controller, keyval, _keycode, modifier| {
        let key_name = keyval.name().unwrap_or_default();
        // The printable character this keyval produces (None for non-printables
        // like arrows/Esc). The journal vim-edit mode needs it to insert typed
        // text; other modes ignore it and route on `key_name`.
        let key_char = keyval.to_unicode();
        let is_ctrl = modifier.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
        let is_shift = modifier.contains(gtk4::gdk::ModifierType::SHIFT_MASK);
        let is_alt = modifier.contains(gtk4::gdk::ModifierType::ALT_MASK);
        let consumed = crate::input::keymap::handle_key(
            &state_for_keys,
            &key_state,
            &key_name,
            key_char,
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
    // Key RELEASE: drives the lone-Shift-tap timestamp delete/undo (Reader mode
    // only; a plain modifier everywhere else). See keymap::handle_key_released.
    key_controller.connect_key_released(move |_controller, keyval, _keycode, _modifier| {
        let key_name = keyval.name().unwrap_or_default();
        crate::input::keymap::handle_key_released(&state_for_release, &key_state_release, &key_name);
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
                    let conn = crate::db::queries::open_db().expect(crate::db::queries::OPEN_DB_PANIC_MSG);
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
                    let mode = crate::text_file_map::match_mode_for_work(
                        &work.abbrev,
                        work.text_file.is_some(),
                    );
                    let line_map = handle
                        .spawn_blocking(move || {
                            let t_map = std::time::Instant::now();
                            let lm = crate::text_file_map::build_line_map_mode(&cleaned, &work_lines, is_prose, mode);
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

            // Warm the concordance word-list cache for this author in the
            // background so the first `\` opens the picker instantly instead of
            // waiting ~10s to tokenize the whole corpus. No-op if already cached.
            crate::input::actions::concordance::warm_word_cache(&state_clone, &handle);

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
        state.borrow_mut().library_picker.show_prepare();
        state.borrow().library_picker.show_finish();
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
    state.prose_flash_anim = None;
    state.pending_prose_flash.set(false);

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

/// After layout settles (`text_view.height() > 0`), correct a `page_top` that
/// `display_work` could only GUESS before layout (it uses `current_line - 1`,
/// forcing the cursor to the top-left and rendering a different, often near-empty
/// spread than the one the user actually quit on). Snap to the canonical spread
/// that DISPLAYS the cursor:
///   - near the end: the canonical final spread (`last_page_top`, EPILOGUE in the
///     right column), re-anchoring the cursor to its last visible dialogue line;
///   - otherwise: the page boundary that CONTAINS the cursor
///     (`page_top_containing`), leaving the cursor where it is.
/// No-op when the cursor is already fully visible on the restored spread,
/// single-column, or layout isn't ready.
fn snap_near_end_to_canonical(s: &mut AppState) {
    let line_count = s.effective_line_count();
    if line_count == 0 || s.text_view.height() <= 0 {
        return;
    }
    // One section per page (sonnet_sequence, single column): the saved page_top
    // may sit mid-sequence (not on a sonnet boundary), so the restored page packs
    // the surrounding sonnets. Snap page_top to the section that contains the
    // cursor and put the cursor on that sonnet's first verse line — matching the
    // gg/x landing. (The two-column path below never runs for these.)
    if s.one_section_per_page() {
        // Snap page_top to the SECTION BOUNDARY (the sonnet heading) containing
        // the cursor — read the authoritative bitmap directly. A forward-walk
        // (canonical_page_top_for) can return a non-boundary top mid-sonnet,
        // which is exactly the resumed-mid-sonnet case we're correcting.
        let anchor = s.current_line.min(line_count.saturating_sub(1));
        let mut top = anchor;
        while top > 0 && !s.is_section_start(top) {
            top -= 1;
        }
        let first = {
            let stage_lookup = |bi: usize| -> Option<i64> {
                s.work_line_for_buffer(bi)
                    .and_then(|wi| s.current_work.as_ref()?.lines.get(wi))
                    .map(|l| l.sub_line)
            };
            crate::input::viewport::next_dialogue_from(&s.buffer, top, line_count, s.is_prose(), &stage_lookup)
                .min(line_count.saturating_sub(1))
        };
        if top != s.page_top_line || first != s.current_line {
            crate::logging::log(&format!(
                "STARTUP: snap one-section page_top {} -> {} (cursor {} -> {})",
                s.page_top_line, top, s.current_line, first
            ));
            s.page_top_line = top;
            s.current_line = first;
        }
        return;
    }
    if s.column_count() != 2 {
        return;
    }
    // If the restored `page_top` is ALREADY the canonical spread that contains the
    // cursor, the saved position is faithful — nothing to do. (Cursor merely being
    // *visible* is NOT enough: the pre-layout guess `current_line - 1` can leave
    // the cursor visible on a near-empty, non-canonical spread — e.g. H8 4192 on
    // top of the sparse 4191 spread — that differs from the canonically-tiled
    // spread the user actually quit on, where the cursor sat in the right column.)
    if crate::input::viewport::page_top_containing(s, s.current_line) == s.page_top_line {
        return;
    }
    // The canonical spread that DISPLAYS the cursor — the page boundary you'd
    // reach by paging forward to `current_line`. This is correct for ALL
    // positions (mid-book or near-end): `page_top_containing` walks the same
    // `next_page_top` tiling that defines every spread.
    let containing = crate::input::viewport::page_top_containing(s, s.current_line);

    // NEAR-END special case: when the cursor's containing page IS the work's final
    // spread, prefer `last_page_top` over the raw containing page. `last_page_top`
    // has the EPILOGUE-fill semantics G/forward-paging use (it pulls the top so a
    // short trailing section — the EPILOGUE — fills the right column rather than
    // being stranded). It also re-anchors the cursor onto that spread. Detect "the
    // cursor's page is the final one" by: paging forward from `containing` reaches
    // the work's end (no further full spread).
    let containing_reaches_end = {
        let nxt = crate::input::viewport::next_page_top(s, containing).new_top;
        nxt >= line_count || nxt <= containing
    };
    if containing_reaches_end {
        let canonical = crate::input::navigation::last_page_top(s);
        if canonical == s.page_top_line {
            return;
        }
        let cs = crate::input::viewport::column_split(s, canonical);
        let cursor = {
            let stage_lookup = |bi: usize| -> Option<i64> {
                s.work_line_for_buffer(bi)
                    .and_then(|wi| s.current_work.as_ref()?.lines.get(wi))
                    .map(|l| l.sub_line)
            };
            crate::input::viewport::prev_dialogue_line(
                &s.buffer, &s.translation_lines, cs.page_end + 1, s.is_prose(), &stage_lookup,
            )
            .filter(|&d| d >= canonical && d <= cs.page_end)
            .unwrap_or(s.current_line.min(cs.page_end))
        };
        crate::logging::log(&format!(
            "STARTUP: snap near-end page_top {} -> canonical {} (cursor {})",
            s.page_top_line, canonical, cursor
        ));
        s.page_top_line = canonical;
        s.current_line = cursor;
        return;
    }

    // GENERAL case: snap to the canonical page that contains the cursor — the
    // spread that displays it where the user left it (e.g. the right column of a
    // two-column play spread), instead of the pre-layout `current_line - 1` guess
    // that forces the cursor to the top-left and renders a sparse, off spread. Do
    // NOT move the cursor; only the page.
    if containing != s.page_top_line {
        crate::logging::log(&format!(
            "STARTUP: snap to containing page_top {} -> {} (cursor {})",
            s.page_top_line, containing, s.current_line
        ));
        s.page_top_line = containing;
    }
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
            let _ = crate::db::echoes::ensure_echo_tables(&conn);
            let _ = crate::db::queries::ensure_gloss_audio_table(&conn);
            let _ = crate::db::queries::ensure_characters_table(&conn);
            let _ = crate::db::queries::ensure_gloss_voices_table(&conn);
            let _ = crate::db::queries::ensure_voice_catalog_table(&conn);
            let _ = crate::db::queries::ensure_claude_model_columns(&conn);
            let _ = crate::db::queries::ensure_vocab_highlight_column(&conn);
            let _ = crate::db::journal::ensure_journal_table(&conn);
            let _ = crate::db::journal::ensure_rewrite_revisions_table(&conn);
            let _ = crate::db::queries::ensure_canonical_artifact_abbrevs(&conn);
        }
    });

    state.loading_work.set(true);
    // Route the post-load reveal through the resize-tick `layout_refresh` branch,
    // which snaps to the canonical spread and waits for two-column widths to
    // settle BEFORE revealing. Without this the startup reveal fell to the 500ms
    // grace timer, which fired as soon as `loading_work` cleared — BEFORE the
    // deferred snap (`snap to containing`) and re-format ran — so the window
    // appeared on the pre-layout guess spread and then visibly jumped/re-flowed.
    state.needs_layout_refresh.set(true);

    // Hide the scrolled window to prevent any flash of content at the wrong
    // scroll position while we rebuild the buffer.
    state.scrolled_window.set_visible(false);

    // Save position of the outgoing work before switching
    if let Some(ref old_work) = state.current_work {
        state.config.work_positions.insert(old_work.abbrev.clone(), state.current_line);
        if let Some(id) = state.work_line_for_buffer(state.current_line)
            .and_then(|wi| old_work.lines.get(wi)).map(|l| l.id)
        {
            state.config.work_position_ids.insert(old_work.abbrev.clone(), id);
        }
        crate::config::mark_work_dirty(&old_work.abbrev);
    }

    crate::input::search::clear_search(state);
    state.search_bar.hide();
    state.current_time_pos = 0.0;
    state.current_sync_scene = None;
    // Task 9: a scheduled prose page crossing is tied to the OLD work's grid and
    // media; drop it so it can't fire against the freshly loaded work.
    state.pending_prose_cross = None;
    // The sub-line pixel offset belongs to the OLD work's prose row-fill grid.
    // Every position derived below sets page_top_line only, so a stale nonzero
    // offset makes the resize-tick snap scroll the (left) view `offset` px past
    // the new work's page top — a half-cut first line, and in two-column mode a
    // left column that duplicates the right column's opening lines. The prose
    // resnap re-derives the offset for a prose target from its own table.
    state.page_top_offset = 0;
    // A vocab-sentence loop never survives a work switch (its buffer lines,
    // media id, and ab-loop all belong to the old work).
    crate::input::vocab_loop::exit_vocab_loop(state);
    state.media_id = work.media_id;
    // Phrase highlight is keyed to the OLD work's lines/media; reset so the
    // first TimePos in the new work refills against the new (line, media).
    state.phrase_cache = None;
    state.active_phrase = None;
    state.phrase_paint_hold = None;
    let slot = crate::instance::slot();
    let window_title = if slot > 1 {
        format!("{} — linux-lit [{}]", work.title, slot)
    } else {
        format!("{} — linux-lit", work.title)
    };
    state.window.set_title(Some(&window_title));
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
    // Persist the column count this work will resolve to, so the NEXT startup's
    // first card pass matches and there's no 1→2-column reflow. Computed from the
    // same inputs `column_count()` uses (override map → work-type default); we
    // can't call `column_count()` yet because `current_work` isn't set.
    if matches!(state.config.navigation_mode, crate::config::NavigationMode::EReader)
        && !state.translations_visible
    {
        let cc = state.config.column_overrides
            .get(&work.abbrev)
            .copied()
            .unwrap_or_else(|| default_column_count_for(&work))
            .clamp(1, 2);
        state.config.last_column_count = Some(cc);
    }
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
    // Per-work vocab coloring: the loaded work's column is the source of truth.
    // Capture before `work` is moved into current_work; the gate further down
    // (`if state.vocab_highlight_visible { apply_vocab_highlighting }`) reads it.
    state.vocab_highlight_visible = work.vocab_highlight;
    state.current_work = Some(work);
    crate::input::actions::chat::on_work_switched(state);

    // The persistent bottom toast is retired (the running head replaced it),
    // so this flag stays false for the app's lifetime. Reset it here alongside
    // the borrow state so a work switch never leaves stale borrow bookkeeping.
    state.chapter_toast_persistent.set(false);
    state.chapter_toast_borrowed.set(false);
    *state.chapter_toast_saved.borrow_mut() = None;
    state.chapter_toast.set_visible(false);

    // Build buffer text (with or without sign column)
    state.line_map = None;
    state.dialogue_formatting_active = false;
    state.authorship_line_ids.clear();
    state.authorship_sets.clear();
    state.active_attribution_set_id = None;
    // Translations are always OFF for a newly loaded work. Reset the flag
    // HERE — before the card sizing, `apply_tiled_mode`, and the `two_col`
    // visibility below — because all three read it (directly, and through
    // `column_count()`, which forces 1 while translations are visible). A
    // switch made with translations on would otherwise size and lay out the
    // NEW work against the OLD work's stale flag: a two-column play would get
    // a single-column width_request (children wider than parent → window
    // grows), and a single-column work would get the two-column card width.
    state.translations_visible = false;
    state.translation_lines = Vec::new();
    state.translation_section_starts = Vec::new();
    // Left margin + tiled-mode visuals. apply_tiled_mode handles the verse
    // offset for wide windows, the page-label padding, and the root-color
    // masking CSS class for narrow/tiled windows.
    let work_type = state.current_work.as_ref().map(|w| w.work_type.clone()).unwrap_or_default();
    let work_is_prose = crate::db::line_types::is_prose_work(&work_type);
    let vbox = state.vbox.clone();
    let ww = state.window.width();
    // Update the card's width_request for THIS work's column count/layout
    // BEFORE apply_tiled_mode below sets the two-column children's widths.
    // Without this, content_hbox keeps the PREVIOUS work's (possibly
    // single-column, narrower) width_request while apply_tiled_mode already
    // requests the new work's two-column widths on its children — GTK's
    // natural-size measurement then sees children wider than their parent's
    // request and grows the toplevel window past its true settled width
    // (observed: BH (1050, 1-col) -> Hamlet (2-col, columns request 1520+)
    // grew the window from 1920 to 2407 before the deferred resize-tick
    // refresh caught up). The resize tick's own `apply_card_sizing` call
    // (layout_refresh branch) still runs afterward and is a no-op once the
    // width already matches.
    {
        // translations_visible was reset above, so cw/cc/tr all describe the
        // NEW work's real (translations-off) layout here.
        let cw = crate::app::layout::effective_column_width(state);
        let cc = state.column_count();
        let tr = state.translations_visible;
        apply_card_sizing(&state.content_hbox.clone(), ww, cw, cc, tr, state.chat_pinned());
    }
    apply_tiled_mode(state, &vbox, ww);
    // Non-prose works (plays, poems, epics) use tight 0px global spacing.
    // Prose uses the configured line_spacing. Reset on every load so the
    // previous work's spacing never leaks through.
    let ls = if work_is_prose {
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
    // (translations_visible / translation_lines / translation_section_starts
    // are reset earlier, before the card sizing — see the comment there.)
    state.page_table_gen_attempted.set(false);
    *state.page_table.borrow_mut() = None;
    state.page_table_fp.borrow_mut().clear();
    state.prose_page_table_gen_attempted.set(false);
    *state.prose_page_table.borrow_mut() = None;
    state.prose_page_table_fp.borrow_mut().clear();
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
            let base_abbrev = work.canonical_abbrev.as_str();
            state.synopsis_cache = crate::db::queries::load_synopses(&conn, base_abbrev);
            crate::logging::log(&format!(
                "SYNOPSIS: loaded {} scene synopses for {}",
                state.synopsis_cache.len(),
                base_abbrev,
            ));
        }
    }
    // Load page-scan images for this work (e.g. BCP1549 leaf PNGs). Reset the
    // image-view toggle on every work switch so a new work starts in text mode.
    state.page_image.mode = false;
    state.page_image.page_order = None;
    state.page_image_overlay.hide();
    state.page_image.images = Vec::new();
    state.page_image.dir = None;
    if let Some(ref work) = state.current_work {
        if let Ok(conn) = crate::db::queries::open_db() {
            state.page_image.images = crate::db::queries::load_page_images(&conn, &work.abbrev);
            state.page_image.dir = crate::db::queries::load_image_dir(&conn, &work.abbrev);
            if !state.page_image.images.is_empty() {
                crate::logging::log(&format!(
                    "PAGE_IMAGES: loaded {} page images for {} (dir={:?})",
                    state.page_image.images.len(),
                    work.abbrev,
                    state.page_image.dir,
                ));
            }
        }
    }
    state.sidebar_mode = SidebarMode::Vocab;
    state.synopsis_visible = false;
    // Restore the persisted scansion level for this work. When it is on,
    // load the cache now so the marks paint on open (true restore), not on
    // the first keypress. The off-thread set_text path below does NOT bake
    // marks, so when the overlay is on we must route through
    // rebuild_buffer_text; we do that by short-circuiting the prepared
    // fast-path for this case.
    state.scansion.level =
        crate::scansion::ScanLevel::from_config_str(&state.config.scansion_level);
    state.scansion.data.clear(); // force reload for the new work
    if state.scansion.level != crate::scansion::ScanLevel::Off {
        if let Some(work) = state.current_work.as_ref() {
            let abbrev = work.abbrev.clone();
            if let Ok(conn) = crate::db::queries::open_db() {
                match crate::db::queries::load_scansion_for_work(&conn, &abbrev) {
                    Ok(map) => state.scansion.data = map,
                    Err(e) => crate::logging::log(&format!("SCANSION: load failed: {}", e)),
                }
            }
        }
    }
    let t0 = std::time::Instant::now();
    // The off-thread set_text path renders PLAIN (no combining marks). When the
    // overlay is on and we have scansion for this work, skip that fast-path and
    // render through rebuild_buffer_text, which bakes the marks in.
    let overlay_on = state.scansion.level != crate::scansion::ScanLevel::Off
        && !state.scansion.data.is_empty();
    if let (Some(prep), false) = (prepared, overlay_on) {
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

    // Tint source lines covered by a reader-gloss passage (theme.reader_gloss,
    // the contrast-guarded color derived from the dwl focuscolor).
    let t_rg = std::time::Instant::now();
    apply_reader_gloss_highlighting(state);
    crate::logging::log(&format!(
        "TIMING: apply_reader_gloss_highlighting {:.0}ms ({} lines)",
        t_rg.elapsed().as_millis(),
        state.reader_gloss_lines.len()
    ));

    // Remove old gutter renderers — they'll be recreated lazily on first
    // sign column toggle (`l` key) via setup_gutter().
    if let Some(old_renderer) = state.gutter_renderer.take() {
        crate::gutter::remove_gutter_renderer(&state.text_view, old_renderer);
        // No gutter → no restore point. A stale value here would override the
        // next work's freshly computed left margin inside setup_gutter().
        state.gutter_logical_left.set(0);
    }
    if let Some(old_renderer) = state.chunk_renderer.take() {
        crate::gutter::remove_gutter_renderer(&state.text_view, old_renderer);
    }
    if let Some(old_renderer) = state.line_number_renderer.take() {
        if state.line_number_renderer_on_left {
            crate::gutter::remove_line_number_renderer_left(&state.text_view, old_renderer);
        } else {
            crate::gutter::remove_line_number_renderer(&state.text_view, old_renderer);
        }
        let right_margin = state.config.text_margins as i32 + crate::config::EXTRA_RIGHT_MARGIN;
        state.text_view.set_right_margin(right_margin);
    }
    state.line_number_renderer_on_left = false;
    if let Some(old_renderer) = state.right_line_number_renderer.take() {
        crate::gutter::remove_line_number_renderer(&state.right_view, old_renderer);
    }
    if let Some(old_renderer) = state.right_gutter_renderer.take() {
        crate::gutter::remove_gutter_renderer(&state.right_view, old_renderer);
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
        // enabled, reclaiming the gutter space for text. Also skipped for a
        // one-section-per-page work (sonnet_sequence): each page is one short
        // numbered poem, so the every-5th foliation is noise.
        let show_numbers = (state.column_count() != 2 || SHOW_LINE_NUMBERS_TWO_COL)
            && !state.one_section_per_page();
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

    // Part F: when resuming (no explicit concordance target), prefer the
    // citation-stable line_mapping_id over the legacy raw buffer index, so a
    // lit.db re-import / repagination doesn't land on the wrong speech.
    if target_line_id.is_none() && std::env::var("LIT_START_POS").is_err() {
        if let Some(work) = &state.current_work {
            if let Some(&saved_id) = state.config.work_position_ids.get(&work.abbrev) {
                if let Some(work_idx) = work.lines.iter().position(|l| l.id == saved_id) {
                    let buf_idx = if let Some(ref lm) = state.line_map {
                        let bi = *lm.work_to_buffer.get(work_idx).unwrap_or(&state.current_line);
                        if lm.buffer_to_work.get(bi) == Some(&Some(work_idx)) { bi } else { state.current_line }
                    } else {
                        // No line_map: the buffer is 1:1 with work lines (same
                        // assumption as work_line_for_buffer), so work_idx is the
                        // buffer index.
                        work_idx
                    };
                    state.current_line = buf_idx.min(state.effective_line_count().saturating_sub(1));
                }
            }
        }
    }

    // If no saved position and no concordance target, start at first
    // dialogue line with viewport showing the line above (usually a
    // speaker name). When current_line > 0 here it came from the resume
    // remap above (work_position_ids, or the legacy work_positions index) —
    // honor the user's saved place.
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
        let snapped = {
            let stage_lookup = |bi: usize| -> Option<i64> {
                state.work_line_for_buffer(bi)
                    .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
                    .map(|l| l.sub_line)
            };
            if state.current_line < line_count
                && !crate::input::viewport::is_dialogue_line(&state.buffer, state.current_line, state.is_prose(), &stage_lookup)
            {
                let forward = crate::input::viewport::next_dialogue_line(
                    &state.buffer, &state.translation_lines,
                    state.current_line, line_count, state.is_prose(), &stage_lookup,
                );
                let backward = if state.current_line > 0 {
                    (0..state.current_line).rev().find(|&i| {
                        crate::input::viewport::is_dialogue_line(&state.buffer, i, state.is_prose(), &stage_lookup)
                    })
                } else {
                    None
                };
                forward.or(backward)
            } else {
                None
            }
        };
        if let Some(snapped_line) = snapped {
            state.current_line = snapped_line;
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
            crate::input::navigation::last_page_top(state)
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
                    let bi = *lm.work_to_buffer.get(work_idx).unwrap_or(&state.current_line);
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

    // Pinned play pagination: attempt to load a stored table for this work now
    // that current_work, the buffer, and line_map are all set. The layout
    // fingerprint depends on window size, which may still be 0 pre-first-layout
    // (fresh startup) — in that case this misses and the resize-tick generation
    // hook's own load_for_work call (before generate_and_store) picks it up
    // once geometry settles.
    crate::input::page_table::load_for_work(state);
    crate::input::prose_pages::load_for_prose_work(state);

    // Persistent bottom toast retired: the running-head strip is now the
    // always-visible position indicator. Keep the flag false so the transient-
    // toast borrow/restore never treats an (unpainted, empty) pill as
    // restorable — otherwise a Sync:/copy toast clearing would restore an empty
    // visible strip.
    state.chapter_toast_persistent.set(false);

    // Apply highlight, snap scroll, show the scrolled window.
    let t7 = std::time::Instant::now();
    crate::input::navigation::update_highlight_and_show(state);
    crate::logging::log(&format!("TIMING: update_highlight {:.0}ms", t7.elapsed().as_millis()));

    // Populate the running-head strip for the freshly-loaded work (both labels).
    // Cursor-move updates keep it fresh afterward; this covers the first paint.
    crate::app::scene_synopsis::update_running_heads(state);

    // Karaoke: tint the phrase that will begin to play (the resume line's
    // start time) so it's visible before playback starts.
    crate::input::phrase_highlight::show_startup_phrase(state);

    crate::logging::log(&format!("TIMING: display_work total {:.0}ms", t0.elapsed().as_millis()));
}

pub(crate) fn rebuild_buffer_text(state: &mut AppState) {
    // The scansion line-type label is appended to each verse line and would
    // overflow the tight per-column width budget in two-column mode (the column
    // is sized for the ~63-char verse worst case, with no room for a trailing
    // label word — see apply_scansion_marks). Suppress the label when two
    // columns are showing; the marks themselves still render in both columns.
    let two_col = state.column_count() == 2;
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    if let Some(prep) = prepare_text_for_display(work) {
        let mapped = prep.line_map.buffer_to_work.iter().filter(|o| o.is_some()).count();
        let first_mapped = prep.line_map.buffer_to_work.iter().position(|o| o.is_some());

        let (display_text, label_starts) = if state.scansion.level == crate::scansion::ScanLevel::Off
            || state.scansion.data.is_empty()
        {
            (prep.filtered_contents.clone(), std::collections::HashMap::new())
        } else {
            apply_scansion_marks(
                &prep.filtered_contents,
                &prep.line_map,
                &work.lines,
                &state.scansion.data,
                state.scansion.level,
                two_col,
            )
        };
        state.buffer.set_text(&display_text);
        // set_text replaces the whole buffer, discarding every applied TextTag —
        // including the buffer-wide "font-size" tag that carries the configured
        // reader font size. Without re-applying it the text falls back to the
        // smaller CSS-default size (the scansion-toggle "small font" bug). Restore
        // it so scansion mode inherits the main card's font size.
        reapply_font(state);
        state.scansion.label_starts = label_starts;
        // Dim-tag each scansion line-type label span (from its start char to the
        // line end). Clone the small map so iterating it doesn't hold an immutable
        // borrow of `state` across the `state.buffer.apply_tag` call.
        let label_starts = state.scansion.label_starts.clone();
        for (&buf_idx, &label_start) in &label_starts {
            if let Some(mut start_iter) = state.buffer.iter_at_line(buf_idx as i32) {
                start_iter.forward_chars(label_start as i32);
                let mut end_iter = start_iter;
                if !end_iter.ends_line() {
                    end_iter.forward_to_line_end();
                }
                state.buffer.apply_tag(&state.scansion_label_tag, &start_iter, &end_iter);
            }
        }
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

    // Fallback for BCP works that have NO text_file yet (1549/1559/1559M/…):
    // load straight from the DB. (A BCP work WITH a text_file already returned
    // above through the generic prose path — prepare_text_for_display keys on
    // work.text_file and text-matches the TEI-rendered .txt back to the DB rows,
    // so the matins layout authored in the .txt is shown verbatim.) Split body
    // prayers one sentence per buffer line for the airy, separated layout, and
    // build a LineMap so every sentence sub-line still maps to its one DB row
    // (timestamps / sync / u-. / concordance key off work_line_for_buffer, which
    // is the buffer==work identity when line_map is None — splitting breaks that
    // identity, so the map is mandatory).
    if crate::db::line_types::is_bcp_work(&work.abbrev) {
        let mut buf_lines: Vec<String> = Vec::with_capacity(work.lines.len());
        let mut source_index: Vec<usize> = Vec::with_capacity(work.lines.len());
        for (wi, l) in work.lines.iter().enumerate() {
            if crate::db::line_types::is_bcp_body(&l.text) {
                for sentence in crate::db::line_types::split_bcp_sentences(&l.text) {
                    buf_lines.push(sentence);
                    source_index.push(wi);
                }
            } else if let Some(stripped) = l.text.strip_prefix("## ") {
                // Strip the `## ` heading marker from the DISPLAYED buffer text.
                // apply_bcp_formatting re-derives heading-ness from the mapped
                // work-line's original text (which keeps the marker), so it still
                // styles the line as a centered heading.
                buf_lines.push(stripped.to_string());
                source_index.push(wi);
            } else if crate::db::line_types::is_bcp_speaker(&l.text) {
                // Strip the `@ ` speaker-cue marker for display (same pattern as
                // `## `): apply_bcp_formatting re-derives cue-ness from the mapped
                // work-line (which keeps the marker) and styles the bare cue
                // centered-italic. The cue is its own row, so no buffer split.
                buf_lines.push(
                    crate::db::line_types::strip_bcp_speaker_marker(&l.text).to_string(),
                );
                source_index.push(wi);
            } else {
                buf_lines.push(l.text.clone());
                source_index.push(wi);
            }
        }
        let line_map = crate::text_file_map::build_line_map_bcp(
            &buf_lines, &source_index, &work.lines,
        );
        state.buffer.set_text(&buf_lines.join("\n"));
        state.line_map = Some(line_map);
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
pub(super) fn setup_gutter(state: &mut AppState) {
    if let Some(old_renderer) = state.gutter_renderer.take() {
        crate::gutter::remove_gutter_renderer(&state.text_view, old_renderer);
    }
    // Make this idempotent. The body reads the view's CURRENT left_margin and
    // reduces it by gutter_width to make room for the signs. On a fresh work
    // load the margin is the full logical value, so that's correct — but on a
    // REBUILD (e.g. a live theme switch calls setup_gutter again) the margin was
    // already reduced by the prior run, so re-reading it here would double-reduce
    // and push the text (and the signs) hard against the card's left edge on each
    // switch. Restore the full logical margin first, captured at the end of the
    // last run in `gutter_logical_left` (0 = never run yet, nothing to restore).
    // CONTRACT: a caller that intends a NEW logical margin must store it in
    // `gutter_logical_left` (and the view margin) before calling — otherwise
    // this restore clobbers the new value with the stale one and the gutter
    // stays pinned at its creation-time geometry (the lost-left-padding bug).
    let logical_left = state.gutter_logical_left.get();
    if logical_left > 0 {
        state.text_view.set_left_margin(logical_left);
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
        // Signs use the text hue gently dimmed (sign_fg = 65% fg); dim_fg's
        // heavier blend washes the hue out to a neutral grey.
        &state.theme.sign_fg,
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
        crate::gutter::remove_gutter_renderer(&state.text_view, old_renderer);
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
        crate::gutter::remove_gutter_renderer(&state.right_view, old_renderer);
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
            // Signs use the text hue gently dimmed (sign_fg = 65% fg); dim_fg's
            // heavier blend washes the hue out to a neutral grey.
            &state.theme.sign_fg,
            0,
        );
        state.right_view.set_left_margin(right_left_margin - gutter_width);
        state.right_gutter_renderer = Some(right_renderer);
    }

    crate::logging::log("GUTTER: set up on demand");
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
        // Record the resolved column count so the NEXT launch can size the first
        // card pass correctly and avoid the startup 1→2-column reflow.
        let cc = state.column_count();
        // Resolve the cursor's line_mapping_id (citation-stable).
        let id = state.work_line_for_buffer(state.current_line)
            .and_then(|wi| work.lines.get(wi))
            .map(|l| l.id);
        state.config.last_work = Some(abbrev.clone());
        state.config.work_positions.insert(abbrev.clone(), state.current_line); // legacy fallback
        crate::config::mark_work_dirty(&abbrev);
        if let Some(id) = id {
            state.config.work_position_ids.insert(abbrev, id);
        }
        state.config.last_column_count = Some(cc);
        crate::config::save(&state.config);
    }
}

/// Tokenize buffer lines and find vocab word matches.
fn build_vocab_matches(state: &mut AppState) {
    state.vocab_matches.clear();

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
        // If this line carries a scansion label, only scan the text BEFORE it so
        // the line-type label isn't vocab-highlighted.
        let scan_text: &str = match state.scansion.label_starts.get(&line_idx) {
            Some(&label_start) => {
                // label_start is a CHAR offset; convert to a byte index for slicing.
                match line_text.char_indices().nth(label_start) {
                    Some((byte_idx, _)) => &line_text[..byte_idx],
                    None => line_text,
                }
            }
            None => line_text,
        };
        let mut char_offset = 0usize;
        let mut in_word = false;
        let mut word_start = 0usize;
        let mut word_buf = String::new();

        for ch in scan_text.chars() {
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

/// Parse a citation `"ABBR.div1.div2.line_in_div"` into `(div1, div2, line)`.
/// Only the trailing three dot-separated numbers matter, so an `-Amb`-suffixed
/// abbrev (or any abbrev with dots) parses the same. Returns `None` if the tail
/// is not three integers.
pub(crate) fn parse_citation(cite: &str) -> Option<(i64, i64, i64)> {
    let mut parts = cite.rsplitn(4, '.');
    let line = parts.next()?.parse().ok()?;
    let div2 = parts.next()?.parse().ok()?;
    let div1 = parts.next()?.parse().ok()?;
    Some((div1, div2, line))
}

/// True if the line `(div1, div2, line_in_div)` falls within any glossed
/// passage's `[start_citation, end_citation]` range. Passages never cross a
/// scene (verified: start and end share `(div1, div2)`), so a match requires the
/// SAME `(div1, div2)` and `line_in_div` within the inclusive line range.
///
/// This is identity-based, NOT text-based: two different lines that happen to
/// share text are distinguished by their citation, fixing the over-coloring bug
/// where any line matching a glossed line's TEXT was tinted.
pub(crate) fn line_in_any_passage(
    div1: i64,
    div2: i64,
    line_in_div: i64,
    passages: &[(String, String)],
) -> bool {
    for (start, end) in passages {
        let (Some((sd1, sd2, sl)), Some((ed1, ed2, el))) =
            (parse_citation(start), parse_citation(end))
        else {
            continue;
        };
        // A passage stays within one scene; guard anyway by requiring the line's
        // scene to equal the passage's start scene.
        if div1 == sd1 && div2 == sd2 && ed1 == sd1 && ed2 == sd2 {
            let (lo, hi) = if sl <= el { (sl, el) } else { (el, sl) };
            if (lo..=hi).contains(&line_in_div) {
                return true;
            }
        }
    }
    false
}

/// Recompute which buffer lines fall inside a `reader-gloss` passage OR a
/// journal passage Q&A for the current work and tint them with
/// `theme.reader_gloss` (contrast-guarded, derived from the dwl focuscolor).
/// Both artifact kinds mark "this line has an annotation" and share the one
/// tint. Stores the set in `reader_gloss_lines` so `update_highlight` can
/// restore the tint on a line the cursor leaves. The cursor's own line is left
/// untinted (`update_highlight` strips it) so the active line wins.
///
/// Buffer→work mapping is read through `work_line_for_buffer` so the split
/// (line_map) and identity (no map) cases are handled uniformly. Glossed lines
/// are matched by CITATION/LINE IDENTITY (`line_in_any_passage`), not by source
/// text — so two lines sharing text are distinguished, fixing the over-coloring
/// bug. Clears all tint first, so this is also the recompute path after a gloss
/// is created or deleted.
pub fn apply_reader_gloss_highlighting(state: &mut AppState) {
    state.reader_gloss_lines.clear();
    state.buffer.remove_tag(
        &state.reader_gloss_tag,
        &state.buffer.start_iter(),
        &state.buffer.end_iter(),
    );

    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return,
    };
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };
    // Look up passages under the SAME abbrev the gloss-save + gloss-overlay
    // paths use — `Work.canonical_abbrev`, the variant-base abbrev all shared
    // artifacts are keyed by. Every gloss path (save, overlay, picker, tint)
    // MUST normalize identically or a variant edition misses its own glosses
    // (the recurring `-BBC`/`-Amb` lookup-mismatch bug class).
    let abbrev = work.canonical_abbrev.clone();
    let passages = crate::db::queries::find_glossed_passages(&conn, &abbrev, &["reader-gloss"])
        .unwrap_or_default();

    // Match glossed lines by CITATION/LINE IDENTITY, not by text. Each passage
    // covers an inclusive `[start_citation, end_citation]` line range within one
    // scene; a buffer line is glossed iff its `(div1, div2, line_in_div)` falls
    // in some passage's range. Text-matching (the old approach) tinted ANY line
    // whose text matched a glossed line's text — so unglossed lines that merely
    // shared text got colored (the over-coloring bug). Base and the production
    // editions (-Amb/-BBC/-DC) are now byte-identical in line_mapping numbering
    // (litdb folger-stage-directions), so the citation tuple resolves correctly
    // on every edition — the reason text-matching was introduced is gone.
    let mut ranges: Vec<(String, String)> = passages
        .iter()
        .map(|p| (p.start_citation.clone(), p.end_citation.clone()))
        .collect();
    // Journal passage Q&As carry the same [start_citation, end_citation] shape
    // and mark their lines the same way.
    ranges.extend(
        crate::db::journal::find_passage_citation_ranges(&conn, &abbrev).unwrap_or_default(),
    );
    if ranges.is_empty() {
        return;
    }

    let line_count = state.buffer.line_count() as usize;
    let mut lines: Vec<usize> = Vec::new();
    for buf_idx in 0..line_count {
        let (d1, d2, lid) = match state
            .work_line_for_buffer(buf_idx)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
        {
            Some(l) => (l.div1, l.div2, l.line_in_div),
            None => continue,
        };
        if line_in_any_passage(d1, d2, lid, &ranges) {
            lines.push(buf_idx);
        }
    }

    for &buf_idx in &lines {
        apply_reader_gloss_tag_to_line(state, buf_idx);
        state.reader_gloss_lines.insert(buf_idx);
    }
}

/// Return to reader mode from a gloss/journal overlay AND refresh the main-card
/// reader-gloss tint. Single source of truth so the tint can't go stale when a
/// reader-gloss was just created/edited/deleted in the overlay: every
/// overlay-close-to-reader site routes through this rather than each remembering
/// to recompute. Callers still own their own position-restore / re-snap; this
/// owns only the two invariants (mode + tint).
pub(crate) fn return_to_reader_mode(state: &mut AppState) {
    // Record which toggleable overlay we're leaving so Ctrl+Tab
    // (ToggleLastOverlay) can reopen it later. Synopsis renders through the
    // gloss overlay widget but carries its own InputMode::SynopsisOverlay, so
    // the GlossOverlay arm never mis-records a synopsis close.
    let was_overlay = state.input_mode != InputMode::Reader;
    match state.input_mode {
        InputMode::GlossOverlay => state.last_overlay = Some(LastOverlay::Gloss),
        InputMode::JournalOverlay => state.last_overlay = Some(LastOverlay::Journal),
        _ => {}
    }
    state.input_mode = InputMode::Reader;
    apply_reader_gloss_highlighting(state);
    // Re-orient the eye: flash the main-card cursor line when an overlay
    // actually closed (never on a redundant call from reader mode).
    if was_overlay {
        crate::input::highlight::flash_reader_cursor(state);
    }
}

/// Position restore + resnap + highlight: restores `current_line`/`page_top_line`
/// from `pos` then calls `resnap_page` and `update_highlight` to re-tile the
/// canonical spread. Used by the "final close" paths that return the user to the
/// reader without immediately opening another overlay.
pub(crate) fn restore_saved_position_resnap(s: &mut AppState, pos: Option<(usize, usize, i32)>) {
    if let Some((line, top, off)) = pos {
        s.current_line = line;
        s.page_top_line = top;
        s.page_top_offset = off;
        crate::input::scroll::resnap_page(s);
        crate::input::highlight::update_highlight(s);
    }
}

/// Apply the slate reader-gloss tint to a single buffer line.
pub(crate) fn apply_reader_gloss_tag_to_line(state: &AppState, buf_idx: usize) {
    if let Some(start) = state.buffer.iter_at_line(buf_idx as i32) {
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        state.buffer.apply_tag(&state.reader_gloss_tag, &start, &end);
    }
}

/// Remove the slate reader-gloss tint from a single buffer line (used so the
/// cursor line reads in the normal foreground while the cursor is on it).
pub(crate) fn remove_reader_gloss_tag_from_line(state: &AppState, buf_idx: usize) {
    if let Some(start) = state.buffer.iter_at_line(buf_idx as i32) {
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        state.buffer.remove_tag(&state.reader_gloss_tag, &start, &end);
    }
}

/// Apply the on-cursor glossed tint to a single buffer line.
pub(crate) fn apply_reader_gloss_cursor_tag_to_line(state: &AppState, buf_idx: usize) {
    if let Some(start) = state.buffer.iter_at_line(buf_idx as i32) {
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        state.buffer.apply_tag(&state.reader_gloss_cursor_tag, &start, &end);
    }
}

/// Remove the on-cursor glossed tint from a single buffer line.
pub(crate) fn remove_reader_gloss_cursor_tag_from_line(state: &AppState, buf_idx: usize) {
    if let Some(start) = state.buffer.iter_at_line(buf_idx as i32) {
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        state.buffer.remove_tag(&state.reader_gloss_cursor_tag, &start, &end);
    }
}

/// Remove all vocab-word tags from the buffer.
pub fn remove_vocab_highlighting(state: &AppState) {
    let start = state.buffer.start_iter();
    let end = state.buffer.end_iter();
    state.buffer.remove_tag(&state.vocab_tag, &start, &end);
}

/// (div1, div2) stored for a journal page scoped to the whole work (vs a scene).
/// The journal_entries table ALSO carries a `scope` TEXT column ('work'/'scene'),
/// so this pair is not unique on its own — it is always paired with scope='work'.
pub(crate) const JOURNAL_WORK_DIV: (i64, i64) = (-1, -1);
/// Author/corpus-scope sentinel. Single source of truth is `db::journal::AUTHOR_DIV`
/// (declared beside `save_author_page`); re-exported here so the app-side band
/// classification reads the same `(-2, -2)` (audit #69).
pub(crate) use crate::db::journal::AUTHOR_DIV as JOURNAL_AUTHOR_DIV;

/// Toggle the main card between text and the page-scan image for the current
/// work. No-op (with a toast) for works that have no page images. When turning
/// the image on, immediately render the page for the cursor's current line.
pub fn toggle_image_view(state: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.page_image.images.is_empty() {
        crate::input::navigation::show_chapter_toast_secs(&s, "No page images for this work", 3);
        return;
    }
    s.page_image.mode = !s.page_image.mode;
    if s.page_image.mode {
        // Hide the two-column chrome (divider + right column) so nothing peeks
        // around the opaque image overlay. Restored on toggle-off.
        s.column_divider.set_visible(false);
        s.right_scrolled_overlay.set_visible(false);
        s.page_image.page_order = None; // force a load
        drop(s);
        refresh_page_image(state);
    } else {
        s.page_image_overlay.hide();
        s.page_image.page_order = None;
        // Restore the two-column chrome if this work uses two columns.
        let two_col = s.column_count() == 2;
        s.column_divider.set_visible(two_col);
        s.right_scrolled_overlay.set_visible(two_col);
    }
}

/// While `image_mode` is on, show the page image for the cursor's current line.
/// Reloads the PNG only when the page changes. Hides the overlay if no page
/// matches the cursor (e.g. uncalibrated region). Safe to call on every cursor
/// move; cheap no-op when `image_mode` is off.
pub fn refresh_page_image(state: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if !s.page_image.mode || s.page_image.images.is_empty() {
        return;
    }
    // Find the calibrated page for the cursor's line. FALLBACK (until calibration
    // exists): if the line has no mapping or no page covers it, show the FIRST
    // page so the image surface is verifiable. Once pages are calibrated this
    // fallback only fires for lines genuinely before the first marked page.
    let calibrated = s
        .line_mapping_id_for_buffer(s.current_line)
        .and_then(|id| s.page_image_for_line_id(id));
    let (order, filename) = match calibrated {
        Some(p) => (p.page_order, p.image_path.clone()),
        None => match s.page_image.images.first() {
            Some(p) => (p.page_order, p.image_path.clone()),
            None => return,
        },
    };
    if s.page_image.page_order == Some(order) {
        return; // same page already shown
    }
    let dir = match &s.page_image.dir {
        Some(d) => d.clone(),
        None => return,
    };
    let path = format!("{}/{}", dir.trim_end_matches('/'), filename);
    let (cw, ch) = overlay_card_size(&s);
    s.page_image_overlay.show(&path, cw, ch);
    s.page_image.page_order = Some(order);
}

// ---------------------------------------------------------------------------
// Page-image calibration: manually mark which canonical line begins each page
// scan. Card shows the page PNG + a caption (page N/M + cursor line text); the
// user moves the cursor (j/k) to the page's first line and presses Enter to
// record it and advance. Esc closes ranges + persists.
// ---------------------------------------------------------------------------

/// Enter calibration mode. Shows page 1 and the caption. No-op (toast) for works
/// without page images.
pub fn enter_page_calibration(state: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    {
        let mut s = state.borrow_mut();
        if s.page_image.images.is_empty() {
            crate::input::navigation::show_chapter_toast_secs(&s, "No page images to calibrate", 3);
            return;
        }
        s.page_image.mode = true;
        s.page_image.calibration_index = 0;
        s.page_image.page_order = None;
        s.column_divider.set_visible(false);
        s.right_scrolled_overlay.set_visible(false);
        // If the cursor is parked on an unmapped buffer line (chrome/blank), snap
        // it forward to the first mapped line so the caption shows real text and
        // Enter can record a line_mapping.id from the start.
        if s.work_line_for_buffer(s.current_line).is_none() {
            let n_lines = s.buffer.line_count().max(1) as usize;
            let start = s.current_line;
            for bl in start..n_lines {
                if s.work_line_for_buffer(bl).is_some() {
                    s.current_line = bl;
                    crate::input::highlight::update_highlight_and_center(&mut s);
                    break;
                }
            }
        }
        s.input_mode = InputMode::PageCalibration;
    }
    calibration_show_page(state);
}

/// Load the page at `calibration_index` into the overlay and update the caption.
pub fn calibration_show_page(state: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    let idx = s.page_image.calibration_index.min(s.page_image.images.len().saturating_sub(1));
    let total = s.page_image.images.len();
    let (order, filename) = match s.page_image.images.get(idx) {
        Some(p) => (p.page_order, p.image_path.clone()),
        None => return,
    };
    let dir = match &s.page_image.dir {
        Some(d) => d.clone(),
        None => return,
    };
    let path = format!("{}/{}", dir.trim_end_matches('/'), filename);
    // Caption: page position + the cursor line's text (what Enter will record).
    let cursor_text = s
        .line_mapping_id_for_buffer(s.current_line)
        .and_then(|id| {
            s.current_work
                .as_ref()?
                .lines
                .iter()
                .find(|l| l.id == id)
                .map(|l| l.text.clone())
        })
        .unwrap_or_else(|| "(cursor on an unmapped line)".to_string());
    let caption = format!(
        "Calibrate {} ({}/{})  ·  Enter marks start  ·  start line: {}",
        filename, idx + 1, total, cursor_text
    );
    let (cw, ch) = overlay_card_size(&s);
    s.page_image_overlay.show(&path, cw, ch);
    s.page_image_overlay.set_caption(Some(&caption));
    s.page_image.page_order = Some(order);
}

/// Enter: record the cursor's line as the current page's start, then advance to
/// the next page (or finish on the last page).
pub fn calibration_mark(state: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let (abbrev, page_order, line_id, last_page) = {
        let s = state.borrow();
        let idx = s.page_image.calibration_index;
        let line_id = match s.line_mapping_id_for_buffer(s.current_line) {
            Some(id) => id,
            None => {
                drop(s);
                let s = state.borrow();
                crate::input::navigation::show_chapter_toast_secs(&s, "Cursor not on a mapped line — move it first", 2);
                return;
            }
        };
        let (abbrev, page_order) = match (s.current_work.as_ref(), s.page_image.images.get(idx)) {
            (Some(w), Some(p)) => (w.abbrev.clone(), p.page_order),
            _ => return,
        };
        (abbrev, page_order, line_id, idx + 1 >= s.page_image.images.len())
    };

    // Persist the start + update the in-memory copy.
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::queries::save_page_image_start(&conn, &abbrev, page_order, line_id);
    }
    {
        let mut s = state.borrow_mut();
        let idx = s.page_image.calibration_index;
        if let Some(p) = s.page_image.images.get_mut(idx) {
            p.start_line_id = Some(line_id);
        }
        if last_page {
            drop(s);
            exit_page_calibration(state, true);
            return;
        }
        s.page_image.calibration_index += 1;
    }
    calibration_show_page(state);
}

/// gg / G: jump calibration to the first / last page without marking.
pub fn calibration_jump_page(state: &std::rc::Rc<std::cell::RefCell<AppState>>, last: bool) {
    {
        let mut s = state.borrow_mut();
        let n = s.page_image.images.len();
        if n == 0 {
            return;
        }
        s.page_image.calibration_index = if last { n - 1 } else { 0 };
    }
    calibration_show_page(state);
}

/// n/p: step to the next/previous page without marking (delta = +1 / -1).
pub fn calibration_step_page(state: &std::rc::Rc<std::cell::RefCell<AppState>>, delta: i32) {
    {
        let mut s = state.borrow_mut();
        let n = s.page_image.images.len() as i32;
        if n == 0 {
            return;
        }
        let cur = s.page_image.calibration_index as i32;
        s.page_image.calibration_index = (cur + delta).clamp(0, n - 1) as usize;
    }
    calibration_show_page(state);
}

/// Esc: finish calibration. Recompute every page's end_line_id from the marked
/// starts, persist, hide the overlay, and return to the reader.
pub fn exit_page_calibration(state: &std::rc::Rc<std::cell::RefCell<AppState>>, save: bool) {
    let (abbrev, ordered_ids) = {
        let s = state.borrow();
        let abbrev = s.current_work.as_ref().map(|w| w.abbrev.clone());
        let ids: Vec<i64> = s
            .current_work
            .as_ref()
            .map(|w| w.lines.iter().map(|l| l.id).collect())
            .unwrap_or_default();
        (abbrev, ids)
    };
    if save {
        if let (Some(abbrev), Ok(mut conn)) = (abbrev.clone(), crate::db::queries::open_db_rw()) {
            let _ = crate::db::queries::recompute_page_image_ends(&mut conn, &abbrev, &ordered_ids);
        }
        // Reload ranges so the live view reflects the calibration immediately.
        if let (Some(abbrev), Ok(conn)) = (abbrev, crate::db::queries::open_db()) {
            let mut s = state.borrow_mut();
            s.page_image.images = crate::db::queries::load_page_images(&conn, &abbrev);
        }
    }
    let mut s = state.borrow_mut();
    s.page_image.mode = false;
    s.page_image.page_order = None;
    s.page_image_overlay.set_caption(None);
    s.page_image_overlay.hide();
    let two_col = s.column_count() == 2;
    s.column_divider.set_visible(two_col);
    s.right_scrolled_overlay.set_visible(two_col);
    s.input_mode = InputMode::Reader;
    crate::input::navigation::show_chapter_toast_secs(&s, "Calibration saved", 2);
}

/// Count word-character runs in a line, treating combining marks (which attach to
/// the preceding letter) as part of the word. Used to verify scansion marks don't
/// split vocab words. Mirrors the word-character rule used by the vocab highlight pass.
#[cfg(test)]
fn word_run_count(line: &str) -> usize {
    let mut runs = 0;
    let mut in_word = false;
    for ch in line.chars() {
        let is_word = ch.is_alphanumeric() || ch == '\'' || ch == '\u{2019}'
            || ch == '\u{0301}' || ch == '\u{0306}';
        if is_word && !in_word { runs += 1; }
        in_word = is_word;
    }
    runs
}

#[cfg(test)]
mod reader_gloss_range_tests {
    use super::{parse_citation, line_in_any_passage};

    #[test]
    fn parse_citation_extracts_div_and_line() {
        assert_eq!(parse_citation("2H6.1.4.43"), Some((1, 4, 43)));
        assert_eq!(parse_citation("Ham.3.1.56"), Some((3, 1, 56)));
        // -Amb suffix on the abbrev is irrelevant — only the trailing 3 numbers matter.
        assert_eq!(parse_citation("2H6-Amb.1.4.43"), Some((1, 4, 43)));
        assert_eq!(parse_citation("garbage"), None);
    }

    #[test]
    fn line_matches_only_inside_a_passage_range_in_same_scene() {
        // One glossed passage: 2H6 1.4.43–50.
        let passages = [("2H6.1.4.43".to_string(), "2H6.1.4.50".to_string())];

        // In range (same scene, line within [43,50]) -> glossed.
        assert!(line_in_any_passage(1, 4, 43, &passages), "start of range");
        assert!(line_in_any_passage(1, 4, 50, &passages), "end of range (inclusive)");
        assert!(line_in_any_passage(1, 4, 47, &passages), "mid range");

        // Out of range by line number -> NOT glossed (the over-coloring bug:
        // a line with line 52 must not tint just because it shares text).
        assert!(!line_in_any_passage(1, 4, 42, &passages), "before range");
        assert!(!line_in_any_passage(1, 4, 51, &passages), "after range");

        // Same line number, DIFFERENT scene -> NOT glossed (identity, not text).
        assert!(!line_in_any_passage(1, 3, 47, &passages), "different div2");
        assert!(!line_in_any_passage(2, 4, 47, &passages), "different div1");
    }

    #[test]
    fn stage_sub_lines_within_range_are_covered() {
        // A passage range 1.4.43–50 covers line_in_div 43..=50; stage directions
        // share their host line's line_in_div (e.g. 43), so they fall in range
        // and get tinted as part of the glossed passage. (sub_line is not part of
        // the citation; the host line number is what the range checks.)
        let passages = [("2H6.1.4.43".to_string(), "2H6.1.4.50".to_string())];
        assert!(line_in_any_passage(1, 4, 43, &passages));
    }
}

#[cfg(test)]
mod scansion_vocab_tests {
    use super::word_run_count;
    #[test]
    fn combining_marks_dont_split_words() {
        // "músic" (acute after u) is still one word run.
        let marked = "If m\u{0075}\u{0301}sic be";
        assert_eq!(word_run_count(marked), 3); // If, músic, be
    }
}

#[cfg(test)]
mod is_play_tests {
    use super::work_type_is_play;

    #[test]
    fn play_is_play() {
        assert!(work_type_is_play("play"));
    }

    #[test]
    fn non_play_types_are_not_play() {
        for t in ["poem", "sonnet_sequence", "novel", "prose", "prose_book", "essay_collection", "anthology"] {
            assert!(!work_type_is_play(t), "{t} must not be a play");
        }
    }
}

