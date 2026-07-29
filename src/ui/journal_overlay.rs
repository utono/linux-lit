use crate::ui::ask_card::{AskCard, AskCardHost};
use crate::ui::gloss_block::{visual_block_range, visual_selection_count};
use crate::ui::journal_block::{journal_blocks, JournalBlock};
use gtk4::prelude::*;
use gtk4::{Label, Overlay};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub struct JournalOverlay {
    pub overlay: Overlay,
    scrim: gtk4::Box,
    container: gtk4::Box,
    /// Running-head labels (work abbrev left, position right) above the scroll,
    /// matching the main card strip and the synopsis/gloss overlays. Set via
    /// `set_running_head` before each show; measured by `size_card`.
    head_work: Label,
    head_division: Label,
    scrolled: gtk4::ScrolledWindow,
    view: gtk4::TextView,
    /// Underline tag for the overlay `-` family (`overlay_word_copy`).
    word_underline_tag: gtk4::TextTag,
    clip_guard: crate::ui::bottom_clip_guard::BottomClipGuard,
    footer_container: gtk4::Box,
    footer_left: Label,
    position_label: Label,
    hint: Label,
    bar_drawing: gtk4::DrawingArea,
    /// Whether the doc card's accent bar is drawn (see the `bar_drawing` draw
    /// func). Toggled by `set_doc_accent_active` — hidden while the ask card is
    /// the active surface in the 2-col float.
    bar_active: Rc<Cell<bool>>,
    panel_drawing: gtk4::DrawingArea,
    bar_ranges: Rc<RefCell<Vec<(i32, i32)>>>,
    /// When the vim editor's NORMAL/VISUAL cursor sits on a BLANK line, that
    /// line's `\n` has no glyph cell, so the char-background block tag paints
    /// nothing. We draw a thin left-edge block here instead. `Some((buffer_line,
    /// r, g, b))` while on a blank line; `None` otherwise. Painted by
    /// `bar_drawing`'s draw func.
    vim_block_line: crate::ui::VimBlankCursor,
    /// The blocks RENDERED on the current page (buffer-line spans). Visual mode
    /// and the accent bar work on these. Re-derived by `render_page` from the
    /// current page's slice of `all_paragraphs`.
    blocks: RefCell<Vec<JournalBlock>>,
    visual_anchor: Cell<Option<usize>>,
    /// Cursor index within the CURRENT PAGE's `blocks` (for the bar + visual).
    cursor_block: Cell<usize>,
    /// The FULL paragraph list for the open Q&A — the pagination unit. The page
    /// renders only a contiguous slice of these so no partial paragraph is ever
    /// shown at either edge (the main-card pagination strategy; see
    /// docs/troubleshooting/clip-prevention.md). Empty for the loading/empty card.
    all_paragraphs: RefCell<Vec<String>>,
    /// Number of leading `all_paragraphs` entries that are the prepended passage
    /// source (speaker/verse/citation), styled on page 0 by
    /// `apply_source_style`. 0 when the entry has no source block.
    source_para_count: Cell<usize>,
    /// Whether the prepended source block has a speaker line / a citation line
    /// (set with `source_para_count`, from the `JournalSource` flags), so
    /// `apply_source_style` maps roles.
    source_has_speaker: Cell<bool>,
    source_has_citation: Cell<bool>,
    /// Page ranges over `all_paragraphs` from `paginate`.
    pages: RefCell<Vec<crate::ui::pagination::Page>>,
    /// Current page index into `pages`.
    page_idx: Cell<usize>,
    /// Cursor index within `all_paragraphs` (the whole Q&A). `cursor_block` is its
    /// page-local projection.
    cursor_full: Cell<usize>,
    /// Footer position state, rebuilt by `update_footer_position`: the Q&A-ENTRY
    /// position `(entry_index, entry_count)` (the Ctrl+n/p count). The far-left
    /// footer shows only "Q&A n of m" — the work-citation band was dropped since
    /// the center pill already names the work/scene.
    entry_pos: Cell<(usize, usize)>,
    text_margins: i32,
    column_width: i32,
    /// True when the CURRENT work is prose (set by the journal action layer
    /// before each show). NOT a margin input any more — every work type uses
    /// card/8 (`size_card`). Its only remaining job is the source quote's verse
    /// hang-indent: a prose quote is ONE long wrapped line, so the hang indent
    /// would ragged-left every wrapped row (`apply_source_style`).
    prose_reading: Cell<bool>,
    font_family: RefCell<String>,
    font_size: Cell<i32>,
    /// Reading font family stashed on edit-enter and restored on exit, so the
    /// monospace edit font does not leak into the rendered display. `None` when
    /// not editing. Save-and-restore (not hardcode-Charter) so a non-default
    /// overlay font would survive an edit.
    pre_edit_family: RefCell<Option<String>>,
    last_card_size: Cell<(i32, i32)>,
    /// Drives the Braille-spinner tick for the loading state; started by
    /// `show_loading`, stopped by every result-render path (`show_page`,
    /// `show_message`, `show_passage_source`) and by `hide()`.
    loading_animator: crate::ui::loading_animator::LoadingAnimator,
    /// Owns the ask-card lifecycle + the fixed-scroll-height viewport-shrink (the
    /// occlusion fix) + the footer hide/show + the clip recompute. Shared with the
    /// gloss overlay so the mechanism can't drift. See `AskCardHost`.
    ask_host: AskCardHost,
    /// The in-place vim editor's engine, `Some` while the `e` editor is open.
    /// The page `view` mirrors its buffer/cursor; `enter_edit_buffer` seeds it,
    /// `feed_edit_key` drives it, `exit_edit_buffer` drops it. See
    /// docs/superpowers/specs/2026-06-30-journal-vim-edit-design.md.
    vim_engine: RefCell<Option<crate::input::vim::VimEngine>>,
    /// The buffer the editor was seeded with, for dirty-check on cancel.
    vim_seed: RefCell<String>,
    /// The kind of the page being edited (`"note"` or `"qa"`). Set on enter,
    /// used by the save path to choose the right parse function.
    edit_kind: RefCell<String>,
    /// (block-fill, glyph-fg) for the NORMAL-mode block cursor, set on enter from
    /// the theme's cursor colors.
    vim_cursor_colors: RefCell<(String, String)>,
    /// `<hi>` highlight background (theme `cursor_line_bg`), threaded by the app
    /// via `set_highlight_color`; defaults to `DEFAULT_HIGHLIGHT_BG`.
    highlight_bg: RefCell<String>,
    /// Char ranges of `<hi>` highlights in the CURRENT page body, re-applied on
    /// the `journal-hi` tag after each `set_text`. Empty when none.
    hi_ranges: RefCell<Vec<(usize, usize)>>,
    /// Markdown emphasis spans (`*italic*` / `**bold**`) in the CURRENT Q&A page
    /// body, re-applied on the `md_tags` italic/bold tags after each `set_text`
    /// — exactly how `hi_ranges` works, and for the same reason: the Q&A path
    /// must keep its single flat `set_text` so `<hi>`, the rewrite diff, overlay
    /// search and the page char-spans all stay offset-aligned. Always empty for
    /// note pages, which go through the block Markdown renderer instead.
    emphasis_ranges: RefCell<Vec<crate::ui::gloss_block::EmphasisSpan>>,
    /// Pre-registered Markdown TextTags for the journal buffer. Built once in
    /// `new` so tag-table lookup is O(1) on every `render_page` call.
    md_tags: crate::ui::markdown::MarkdownTags,
    /// True when the current page holds a `kind == "note"` entry (imported
    /// Markdown). Set by `show_page`; read by `render_page` to route note
    /// bodies through the styled Markdown renderer rather than plain `set_text`.
    page_is_note: Cell<bool>,
    /// For notes: the ONE planned-block list shared by pagination, rendering,
    /// and cursor navigation (index-aligned with `all_paragraphs` and `pages`).
    /// Empty for Q&A entries (whose unit is the plain paragraph). This single
    /// unit is what keeps page fill, the accent bar, and j/k stepping aligned —
    /// the old raw-paragraph pagination measured different blocks than the
    /// styled render drew, so pages underfilled and j hit phantom stops.
    note_blocks: RefCell<Vec<crate::ui::markdown::PlannedBlock>>,
    /// Page-marker glyph (`⌄`/`•`/None) drawn on `bar_drawing` — no Label, so no
    /// overlay-child allocation lag. Set by `update_page_marker`, read by the draw
    /// func. Its dim color is `marker_color`.
    marker_glyph: Rc<RefCell<Option<&'static str>>>,
    marker_color: Rc<RefCell<(f64, f64, f64)>>,
    panel_color: Rc<RefCell<(f64, f64, f64)>>,
    /// Selection accent-bar color = theme `root_color` (the crisp accent), threaded
    /// by the app via `set_bar_color` — matching the gloss overlay's theme-wired
    /// bar. (Was a hardcoded pale grey-blue default, the odd one out.)
    bar_color: Rc<RefCell<(f64, f64, f64)>>,
    /// Overlay-search highlight tags, registered once on `view.buffer()`'s tag
    /// table in `new` (the buffer is never replaced — `set_text` writes into it
    /// in place — so registering once here is safe for the view's lifetime).
    /// Placeholder colors; `set_search_colors` wires them to the theme (Task 5).
    search_tag: gtk4::TextTag,
    search_current_tag: gtk4::TextTag,
    /// Vocab-word foreground tint (mirrors the main reading card's `vocab_tag`
    /// and the gloss overlay's). Registered once here like the search tags;
    /// placeholder color until `set_vocab_color` wires it to the theme. Applied
    /// over the current buffer by `apply_vocab_tags` after every populate/recolor
    /// (gated on `vocab_highlight_visible` at the action-layer call sites).
    vocab_tag: gtk4::TextTag,
    /// Ephemeral rewrite diff-highlight tag (Task 4 of the rewrite-revision-
    /// history feature). Marks the char ranges a custom-prompt rewrite changed;
    /// applied by `apply_rewrite_diff` (Tasks 5/6), cleared by `clear_rewrite_diff`
    /// (Task 7). Placeholder color; `set_rewrite_diff_color` wires it to the theme.
    rewrite_diff_tag: gtk4::TextTag,
    /// True while a rewrite diff highlight is currently applied (empty ranges
    /// count as inactive). Read by Task 7 to decide whether to clear on next edit.
    rewrite_diff_active: std::cell::Cell<bool>,
    /// Rewrite diff-highlight ranges as char offsets into the WHOLE-entry body
    /// (`prefix_question(q) + "\n\n" + answer`), so the highlight survives page
    /// turns: `render_page` clips these to the current page's char span and
    /// re-tags them (mirrors how `hi_ranges` is re-derived per page). Empty when
    /// no rewrite highlight is active.
    rewrite_diff_full: RefCell<Vec<(usize, usize)>>,
    /// True while the diff ranges are TINTED. Distinct from
    /// `rewrite_diff_active`, which stays true as long as ranges are
    /// remembered: after the 3-second auto-fade the tint is gone but the
    /// ranges live on so `w` can flash them again. Mirrors the gloss overlay.
    rewrite_diff_shown: std::rc::Rc<std::cell::Cell<bool>>,
    /// Generation counter for the auto-fade timer — a timer whose captured
    /// generation no longer matches is a no-op, so a re-flash or an early
    /// clear cannot be undone by a stale timer. Mirrors the gloss overlay.
    rewrite_diff_fade_gen: std::rc::Rc<std::cell::Cell<u64>>,
    /// Overlay-search match spans as char offsets into the WHOLE-entry body (the
    /// `whole_entry_text` basis) + the current-match index, so `/` search survives
    /// page turns WITHIN a paginated entry: `render_page` clips these to the
    /// current page's char span and re-tags them (mirrors `rewrite_diff_full`).
    /// Empty when no search is active. Set by `set_search_matches`, cleared by
    /// `clear_search_tags`.
    search_full: RefCell<Vec<(i32, i32)>>,
    search_full_current: Cell<usize>,
}

/// Split the full Q&A text into paragraph blocks (the pagination unit): maximal
/// runs of non-blank lines, blank-line separated. Returns each paragraph's text.
/// Reuses `journal_blocks` so the split matches what `render_page` re-derives for
/// the accent bar.
fn paragraph_texts(full: &str) -> Vec<String> {
    let lines: Vec<&str> = full.split('\n').collect();
    journal_blocks(&lines).into_iter().map(|b| b.text).collect()
}

/// The prepended quoted-source block for a passage Q&A: its display paragraphs
/// plus the role flags `apply_source_style` needs. Built by
/// `source_paragraphs` (input/actions/journal.rs), which knows exactly whether
/// it pushed a speaker/citation paragraph — replacing the old em-dash sniffing
/// of `paras[0]`, which mistook a speakerless PROSE quote for a speaker line
/// and painted the whole quote with the reduced-scale speaker tag.
#[derive(Default)]
pub struct JournalSource {
    pub paras: Vec<String>,
    pub has_speaker: bool,
    pub has_citation: bool,
}

/// Which buffer lines the prepended source paragraphs occupy, by role.
/// Paragraphs render joined by "\n\n", so paragraph `i` starts one blank line
/// after paragraph `i-1` ends. The verse paragraph is a single `\n`-joined
/// block, so it spans MULTIPLE buffer lines — `verse_lines` lists each of them
/// (the hang-indent tag is applied per line). Order of source paragraphs is:
/// `[speaker?] verse [citation?]`.
struct SourceLineRoles {
    speaker_line: Option<i32>,
    verse_lines: Vec<i32>,
    citation_line: Option<i32>,
    /// The BLANK separator lines inside the quotation: the one under the speaker
    /// label and the one above the citation. They are collapsed to a hairline so
    /// the parts of one quotation don't drift apart (see `SOURCE_GAP_SCALE`);
    /// the blank line between the source and the QUESTION is deliberately not
    /// included — that break should stay full-size.
    inner_gap_lines: Vec<i32>,
}

/// Map the leading source paragraphs to their buffer lines by role. `paras` are
/// the source paragraph TEXTS (the first `source_para_count` of
/// `all_paragraphs`), each possibly multi-line (`\n`-joined). Paragraphs render
/// joined by "\n\n"; the start line of paragraph `i` is the sum of every prior
/// paragraph's line count plus one blank separator line each. `has_speaker`/
/// `has_citation` say whether the first paragraph is a speaker label and
/// whether the LAST paragraph is a citation (both flags come from
/// `JournalSource`, set by the builder — never sniffed from the text).
fn source_line_roles(paras: &[String], has_speaker: bool, has_citation: bool) -> SourceLineRoles {
    let count = paras.len();
    // Buffer start line of each source paragraph: paragraph i starts after all
    // prior paragraphs' lines plus one blank line separating each from the next.
    let mut starts: Vec<i32> = Vec::with_capacity(count);
    let mut line = 0i32;
    for p in paras {
        starts.push(line);
        let para_lines = p.split('\n').count() as i32;
        line += para_lines + 1; // +1 for the blank separator line
    }
    let last = count - 1;
    let citation_para = if has_citation { Some(last) } else { None };
    let speaker_para = if has_speaker { Some(0usize) } else { None };
    let first_verse = if has_speaker { 1 } else { 0 };
    // The verse block is the single paragraph before the citation (else the
    // last paragraph). Expand it to every buffer line it spans so the
    // hang-indent tag lands on each quoted line.
    let verse_para = match citation_para {
        Some(c) => c.checked_sub(1),
        None => Some(last),
    };
    let verse_lines = match verse_para {
        Some(vp) if vp >= first_verse => {
            let n = paras[vp].split('\n').count() as i32;
            (starts[vp]..starts[vp] + n).collect()
        }
        _ => Vec::new(),
    };
    // The blank separator sits on the line directly ABOVE a paragraph's start.
    // Collect the two INSIDE the quotation (under the speaker, above the
    // citation); never the one below the whole source block, which separates the
    // quotation from the question and stays full-size.
    let mut inner_gap_lines = Vec::new();
    if speaker_para.is_some() {
        if let Some(&first_verse_line) = verse_lines.first() {
            if first_verse_line > 0 {
                inner_gap_lines.push(first_verse_line - 1);
            }
        }
    }
    if let Some(c) = citation_para {
        if starts[c] > 0 {
            inner_gap_lines.push(starts[c] - 1);
        }
    }
    SourceLineRoles {
        speaker_line: speaker_para.map(|p| starts[p]),
        verse_lines,
        citation_line: citation_para.map(|p| starts[p]),
        inner_gap_lines,
    }
}

/// Prefix a journal Q&A question with `Q: ` for display (the answer follows
/// below). Idempotent: a question already starting with `Q:` is returned as-is,
/// so a stored/re-rendered question isn't double-prefixed.
pub(crate) fn prefix_question(question: &str) -> String {
    if question.trim_start().starts_with("Q:") {
        question.to_string()
    } else {
        format!("Q: {}", question)
    }
}

/// Next cursor index from `cur` stepping by `delta` (±1), skipping indices
/// where `is_stop` is false (note heading/rule chrome). Returns `None` when no
/// stoppable index exists in that direction — the caller no-ops (the press is
/// consumed at the edge, exactly like the old clamp). Pure, unit-tested.
fn step_skipping_chrome(
    cur: usize,
    delta: i32,
    total: usize,
    is_stop: impl Fn(usize) -> bool,
) -> Option<usize> {
    let mut i = cur as i64;
    loop {
        i += delta as i64;
        if i < 0 || i >= total as i64 {
            return None;
        }
        if is_stop(i as usize) {
            return Some(i as usize);
        }
    }
}

/// Vertical chrome margins the column needs that `preferred_size()` does NOT
/// report (GTK's preferred-size excludes a widget's own margins). The journal
/// scroll_overlay carries a 24px top + 20px bottom margin (the breathing gap
/// below the title / above the footer — mirrors the gloss overlay). `size_card`
/// folds these into the host's fixed-chrome arg so the scroll budget matches the
/// gloss overlay's `size_scroll` exactly (which reserves the same 44 via its
/// `SCROLL_OVERLAY_MARGINS`). Keep in sync with the two scroll_overlay margin
/// sites in `new`.
// Match the gloss overlay's `size_scroll`, which reserves ONLY the scroll_overlay
// top+bottom margins (24+20=44) — NOT the title's top margin or the footer's
// top/bottom. Reserving those extra 48px (the old value 92) made the journal
// column 48px shorter than the gloss's for the same card, so its footer sat
// flush at the bottom while the gloss footer floated higher. With 44 the journal
// sizes its scroll exactly like the gloss, so the footer lands in the same place.
const UNACCOUNTED_CHROME_MARGINS: i32 = 24 + 20 /* scroll_overlay top+bottom */;

/// Extra LEFT indent on the Q&A body so it sits ~12px right of the accent bar,
/// with the bar in the gutter beside the text — MATCHING the gloss explication's
/// left position (`quote_body = bar_left + QUOTE_BODY_INDENT`) so the two
/// overlays have the same text-column width. Added to the left margin only;
/// pagination reads left_margin live so wrap/height follow automatically (no
/// measure change).
const JOURNAL_BODY_INDENT: i32 = crate::ui::gloss_render::QUOTE_BODY_INDENT;

/// Font scale applied to the two BLANK separator lines inside the quoted source
/// (under the speaker label, above the citation). The source's paragraphs are
/// `\n\n`-joined like every other block, so each internal boundary renders a
/// full blank line — too airy for parts of one quotation.
///
/// Shrinking the blank line's own font is the only way to close it: GTK's
/// `pixels-above-lines`/`pixels-below-lines` are `min=0`, so a NEGATIVE pull is
/// not merely ignored — setting one aborts the process with "invalid or out of
/// range" inside a non-unwinding GTK callback. Do not reach for negative
/// spacing here.
///
/// Scaling the blank leaves the TEXT untouched, so every char offset (search,
/// diff spans, page char spans) and every buffer line index (blocks,
/// pagination, the accent bar) is unchanged.
const SOURCE_GAP_SCALE: f64 = 0.3;

/// The journal's family when the reader is on `JOURNAL_FONT_ALT_FAMILY`
/// (Charis, the reader's default). The two form ONE pair that the journal and
/// the reading card trade between, so the Q&A is never set in the same face as
/// the card behind it — see `journal_family_for_reader` for the full table.
///
/// The reader card, the gloss and the synopsis all follow whichever
/// `config::FONT_CYCLE` family is active (`reapply_font` → `sync_reader_font`);
/// only the journal diverges.
///
/// Only the FAMILY is chosen here — `sync_reader_font` still follows the
/// reader's SIZE, so `+`/`-` scales the journal with everything else.
const JOURNAL_FONT_FAMILY: &str = "Charter";

/// The journal's family when the reader is on `JOURNAL_FONT_FAMILY` — and its
/// default whenever the reader is on anything else. Charis is the reader's own
/// default (`config::default_font_family`), so the journal and the card behind
/// it simply trade the two faces of one pair.
const JOURNAL_FONT_ALT_FAMILY: &str = "Charis";

/// The journal's family for a given reader family. The Q&A must never render in
/// the same family as the card behind it, so the two swap within one pair:
///
/// | reader          | journal |
/// |-----------------|---------|
/// | Charis          | Charter |
/// | Charter         | Charis  |
/// | anything else   | Charis  |
///
/// The "anything else" arm covers the rest of `config::FONT_CYCLE` (Gentium
/// Book today) and any hand-edited config value: Charis is the reader's default
/// and the journal's fallback, and it cannot collide there because that arm is
/// only reached when the reader is on neither of the pair.
///
/// Comparison is case-insensitive and trimmed because the reader family comes
/// from config (hand-editable) while ours are compile-time constants.
fn journal_family_for_reader(reader_family: &str) -> &'static str {
    if reader_family.trim().eq_ignore_ascii_case(JOURNAL_FONT_ALT_FAMILY) {
        JOURNAL_FONT_FAMILY
    } else {
        JOURNAL_FONT_ALT_FAMILY
    }
}

impl JournalOverlay {
    pub fn new(column_width: u32, text_margins: u32) -> Self {
        let overlay = Overlay::new();

        let scrim = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        scrim.add_css_class("gloss-scrim");
        scrim.set_hexpand(true);
        scrim.set_vexpand(true);
        scrim.set_visible(false);

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("gloss-overlay");
        container.set_halign(gtk4::Align::Center);
        container.set_valign(gtk4::Align::Center);
        container.set_width_request(column_width as i32);
        container.set_visible(false);

        // Running head matching the main card and the synopsis/gloss overlays:
        // work abbrev (left) + position (right) in the small-caps
        // `running-head-*` styles. Text is set by the action layer
        // (`set_running_head`) BEFORE each show, so `size_card`'s measurement
        // of the row (folded into the fixed chrome) is real.
        let head_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        let head_work = Label::new(None);
        head_work.add_css_class("running-head-work");
        head_work.set_halign(gtk4::Align::Start);
        head_work.set_valign(gtk4::Align::Center);
        head_work.set_hexpand(true);
        // Mirror the card strip's `.running-head` 40px side padding.
        head_work.set_margin_start(40);
        head_work.set_margin_top(24);
        head_work.set_margin_bottom(12);
        head_row.append(&head_work);
        let head_division = Label::new(None);
        head_division.add_css_class("running-head-division");
        head_division.set_halign(gtk4::Align::End);
        head_division.set_valign(gtk4::Align::Center);
        head_division.set_margin_end(40);
        head_division.set_margin_top(24);
        head_division.set_margin_bottom(12);
        head_row.append(&head_division);

        let scrolled = gtk4::ScrolledWindow::new();
        scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
        scrolled.set_vscrollbar_policy(gtk4::PolicyType::External);
        scrolled.set_propagate_natural_height(false);
        // SPIKE (fixed-scroll-height architecture): vexpand is OFF. The earlier
        // races came from the vexpand scroll fighting the container's unbounded
        // height non-deterministically when the ask card appeared/vanished. With
        // vexpand off, the scroll's height is EXACTLY what size_card sets it to —
        // deterministic, no fight, no resize-on-open race. size_card sets it to
        // the pane height minus the title+footer; open/close adjust it by the ask
        // card's reserved height.
        scrolled.set_vexpand(false);

        let view = gtk4::TextView::new();
        crate::ui::set_view_readonly(&view);
        view.set_wrap_mode(gtk4::WrapMode::Word);
        // ~one line of breathing room above the first line and below the last
        // line INSIDE the panel, so the text isn't flush against the panel's
        // inner top/bottom edge (matches the gloss view's inner margins; the
        // scroll_overlay's own margins sit OUTSIDE the panel, so they can't
        // provide this gap).
        view.set_top_margin(28);
        view.set_bottom_margin(28);
        // Reading leading between wrapped lines; pagination charges the same
        // (measure_planned_block / measure_text_height_leaded) — keep in sync.
        view.set_pixels_inside_wrap(crate::ui::OVERLAY_LINE_LEADING);
        view.add_css_class("gloss-text");
        view.add_css_class("overlay-prose");

        // The ScrolledWindow's child MUST be the TextView DIRECTLY so GTK uses
        // the view's native scroll adjustments (a TextView is `Scrollable`).
        // Wrapping it in an Overlay made GTK insert a GtkViewport, which gave the
        // vadjustment no real scroll range — j/k/G/gg did nothing and overflow
        // content stayed clipped. The bottom_clip therefore overlays an OUTER
        // Overlay that wraps the scrolled window, exactly like the gloss overlay
        // (Overlay(ScrolledWindow(TextView) + bottom_clip)).
        scrolled.set_child(Some(&view));

        let scroll_overlay = Overlay::new();
        // The Overlay's MAIN CHILD is set to `panel_drawing` below (so the inset
        // tint paints BELOW the transparent prose view); the scroll becomes a
        // measured overlay on top. `panel_drawing` isn't built until after the
        // clip guard, so the main child is assigned in the panel-wiring block
        // (search "set_child(Some(&panel_drawing))"), not here. The clip guard
        // only ADDS an overlay, so it does not need the main child set first.
        let clip_guard = crate::ui::bottom_clip_guard::BottomClipGuard::attach(
            &scroll_overlay,
            &view,
            &scrolled,
        );

        // Selection bar: a DrawingArea overlay over the same scroll_overlay that
        // hosts bottom_clip, drawing a 2px vertical accent line over selected
        // buffer-line spans. Fixed color — NOT theme-wired.
        let bar_ranges: Rc<RefCell<Vec<(i32, i32)>>> = Rc::new(RefCell::new(Vec::new()));
        let vim_block_line: crate::ui::VimBlankCursor = Rc::new(RefCell::new(None));
        // Page-marker glyph (⌄ more / • last page) drawn on the bar (no Label —
        // see draw_page_marker_glyph) + its dim color, threaded by set_marker_color.
        let marker_glyph: Rc<RefCell<Option<&'static str>>> = Rc::new(RefCell::new(None));
        let marker_color: Rc<RefCell<(f64, f64, f64)>> = Rc::new(RefCell::new((0.5, 0.5, 0.5)));
        // Inset-panel DrawingArea + its color cell (shared helper, audit #52). The
        // draw_func is wired inside; the caller sets it as the Overlay main child
        // and adds panel_drawing.queue_draw() to its scroll-repaint closure below.
        // The journal folds JOURNAL_BODY_INDENT into the view's left_margin
        // (size_card), so the panel must exclude it to anchor at the COLUMN edge
        // — otherwise the journal panel renders 12px narrower than the gloss
        // panel on the identical card (left edge inboard, right edges aligned).
        let (panel_drawing, panel_color) =
            crate::ui::attach_overlay_panel(&view, JOURNAL_BODY_INDENT);
        // Accent-bar color = theme root_color, set by set_bar_color at startup.
        let bar_color: Rc<RefCell<(f64, f64, f64)>> =
            Rc::new(RefCell::new((0.53, 0.62, 0.71))); // placeholder; set at startup
        // Whether the doc card's accent bar (the left selection/cursor line) is
        // shown. False while the ask card holds focus in the 2-col float: the
        // hidden bar is the doc card's inactive cue (replacing the old dim). The
        // page marker stays drawn regardless — only the vim block cursor and the
        // selection spans are gated. Set by `set_doc_accent_active`.
        let bar_active: Rc<Cell<bool>> = Rc::new(Cell::new(true));
        let bar_drawing = gtk4::DrawingArea::new();
        bar_drawing.set_can_target(false);
        {
            let ranges_clone = bar_ranges.clone();
            let view_clone = view.clone();
            let vim_block_clone = vim_block_line.clone();
            let marker_glyph_clone = marker_glyph.clone();
            let marker_color_clone = marker_color.clone();
            let bar_color_clone = bar_color.clone();
            let bar_active_clone = bar_active.clone();
            bar_drawing.set_draw_func(move |_area, cr, area_w, _h| {
                // Page marker first (independent of the selection bar's early-return
                // AND of the accent-bar active flag — the ⌄/• cue stays visible even
                // when the doc card is the inactive surface).
                crate::ui::draw_page_marker_glyph(
                    cr,
                    &view_clone,
                    area_w,
                    *marker_glyph_clone.borrow(),
                    *marker_color_clone.borrow(),
                    0.55,
                    8,
                );
                // Accent bar (vim block cursor + selection spans) is the doc card's
                // focus cue: skip it entirely while the ask card is active.
                if !bar_active_clone.get() {
                    return;
                }
                // Vim block cursor on a BLANK line (shared draw). Drawn BEFORE
                // the selection-bar early-return so it shows while editing (no
                // selection ranges then). x reads left_margin() live.
                let bx = (view_clone.left_margin() as f64).max(2.0);
                crate::ui::draw_vim_block_cursor(
                    cr,
                    &view_clone,
                    *vim_block_clone.borrow(),
                    bx,
                );
                let ranges = ranges_clone.borrow();
                if ranges.is_empty() {
                    return;
                }
                // Theme accent (root_color), matching the gloss overlay's bar.
                let (r, g, b) = *bar_color_clone.borrow();
                cr.set_source_rgb(r, g, b);
                cr.set_line_width(2.0);
                // Draw the bar 12px LEFT of the text — at the COLUMN edge
                // (left_margin - JOURNAL_BODY_INDENT), exactly where the gloss
                // draws its bar (`bar_x = left`). The panel's inner edge sits a
                // further PANEL_PAD left of that (the panel excludes the body
                // indent — see attach_overlay_panel), so bar-to-panel and
                // bar-to-glyph gaps match the gloss. (Drawing at exactly
                // left_margin() made the bar collide with the first glyph.)
                let x = ((view_clone.left_margin() - JOURNAL_BODY_INDENT) as f64).max(2.0);
                crate::ui::draw_bar_spans(cr, &view_clone, &ranges, x);
            });
        }
        // Repaint the bar when the view scrolls (buffer->window y is scroll-dependent).
        {
            let bar_for_scroll = bar_drawing.clone();
            let panel_for_scroll = panel_drawing.clone();
            scrolled.vadjustment().connect_value_changed(move |_| {
                bar_for_scroll.queue_draw();
                panel_for_scroll.queue_draw();
            });
        }
        // Panel is the Overlay's MAIN CHILD so its inset tint paints BELOW the
        // (transparent) prose view; the scroll becomes a measured overlay on top.
        // A GTK Overlay paints its main child first, then overlays in add-order —
        // so a panel added as an *overlay* would paint ON TOP of the text (an
        // opaque tint rect hiding the prose). The scroll MUST be measured
        // (`set_measure_overlay(.., true)`) — a bare DrawingArea main child
        // reports 0×0 natural size and would collapse the Overlay.
        scroll_overlay.set_child(Some(&panel_drawing));
        scroll_overlay.add_overlay(&scrolled);
        scroll_overlay.set_measure_overlay(&scrolled, true);
        scroll_overlay.add_overlay(&bar_drawing);
        scroll_overlay.set_measure_overlay(&bar_drawing, false);
        scroll_overlay.set_clip_overlay(&bar_drawing, true);

        // The page marker (⌄ more / • last page) is drawn ON `bar_drawing` via
        // `draw_page_marker_glyph` (see above) — NOT a Label. An Overlay child's
        // allocation lagged `set_margin_top` by several frames, so a Label glyph
        // painted off a short last page until an unrelated relayout. The bar's
        // draw func reads live `buffer_to_window_coords` and repaints on every
        // render/scroll, so the glyph is always at the right y.

        // Breathing gap above the footer and below the title, mirroring the gloss
        // overlay (gloss_overlay.rs). Without the bottom margin the viewport's
        // bottom edge sits flush against the footer, so a block scrolled to the
        // bottom edge by j/k has its last line bisected by the footer rule and
        // reads as clipped — the journal block-nav clipping the user saw. The
        // bottom-clip box masks only a PARTIAL row at the viewport edge; this gap
        // keeps the last whole line clear at any scroll position. The top margin
        // gives the symmetric gap below the title rule (the view's internal
        // top_margin scrolls away with the content, so it can't keep this gap).
        scroll_overlay.set_margin_bottom(20);
        scroll_overlay.set_margin_top(24);

        container.append(&head_row);
        container.append(&scroll_overlay);

        // Footer mirroring the gloss overlay (gloss_overlay.rs footer_box):
        // "Q&A n of m" on the left, the bare page counter on the right. Drop the
        // `gloss-hint` class so there is NO border-top divider (the act/scene
        // pill already separates the footer visually), matching the gloss footer.
        let footer = crate::ui::footer::build_footer_row(
            text_margins as i32,
            "",
        );
        footer.container.remove_css_class("gloss-hint");
        let footer_left = footer.left;
        let hint = footer.hint;
        // Right-aligned bare "X / Y" render-page counter, mirroring the gloss
        // overlay's position_label (gloss_overlay.rs). The journal footer's left
        // label keeps "band · Q&A N of M"; the page count moves here so the two
        // overlays' footers read the same way (no "page" word inline).
        let position_label = Label::new(None);
        position_label.set_halign(gtk4::Align::End);
        position_label.set_visible(false);
        footer.container.append(&position_label);
        let footer_container = footer.container.clone();
        container.append(&footer.container);

        // Shared "ask" input card. Floats to the RIGHT of the journal card in a
        // 2-column layout (mirrors the gloss overlay), so Ctrl+Tab can toggle
        // focus between the two. Added as an add_overlay sibling in `attach()`,
        // NOT appended to the column.
        let ask = AskCard::new(text_margins as i32, &view);
        // Center the fixed-width float panel (mirrors the gloss ask container).
        // valign=Center MATCHES the journal card (also valign=Center); with the
        // reserve closure capping the ask panel to the card's height, both
        // centered same-height cards share a top edge, so the "Ask a question…"
        // title aligns with the running head. (Was valign=Fill, which pinned the
        // ask card to the overlay top while the journal card sat centered.)
        ask.container().set_halign(gtk4::Align::Center);
        ask.container().set_valign(gtk4::Align::Center);
        let ask_container_for_reserve = ask.container().clone();

        // The host owns the ask-card lifecycle: the fixed-scroll-height
        // viewport-shrink, the footer hide/show, and the clip recompute. The
        // recompute closure drives this overlay's BottomClipGuard's clip box.
        let recompute = {
            let clip = clip_guard.clip().clone();
            let view = view.clone();
            let scrolled = scrolled.clone();
            Rc::new(move || {
                crate::ui::recompute_overlay_bottom_clip(&view, &clip, &scrolled);
            }) as Rc<dyn Fn()>
        };
        let ask_host =
            AskCardHost::new(ask, &scrolled, Some(footer_container.clone()), recompute);
        // Journal ask floats to the RIGHT of the journal card (2-column layout,
        // mirroring the gloss overlay). Fixed float width. On open the journal
        // card reserves margin_end = ask_width + seam (shifts it left by half);
        // the ask panel reserves margin_start = journal_width + seam (shifts it
        // right by half). Both children are halign=Center, so the two mirrored
        // allocation boxes yield equal L/R gutters (the centered pair).
        let float_w = (column_width as i32 * 5 / 8).max(360);
        {
            let container_for_reserve = container.clone();
            let reserve = Rc::new(move |px: i32| {
                // `px` is the reservation (ask_width + seam) on open, 0 on close.
                container_for_reserve.set_margin_end(px);
                if px > 0 {
                    let journal_w = container_for_reserve
                        .width()
                        .max(container_for_reserve.width_request());
                    let seam = px - float_w; // reservation minus ask width
                    ask_container_for_reserve.set_margin_start(journal_w + seam);
                    // Cap the ask panel height to the journal card's height so the
                    // two columns are top/bottom aligned.
                    let h = container_for_reserve
                        .height()
                        .max(container_for_reserve.height_request());
                    ask_container_for_reserve.set_height_request(h.max(200));
                } else {
                    ask_container_for_reserve.set_margin_start(0);
                    ask_container_for_reserve.set_height_request(-1);
                }
            }) as Rc<dyn Fn(i32)>;
            ask_host.enable_float(float_w, reserve);
        }
        // Build markdown tags once against the view's tag table so every
        // render_page call reuses the same registered tags (O(1) apply).
        let md_tags = crate::ui::markdown::MarkdownTags::register(&view.buffer());
        // `register` defaults the serif tags to GLOSS_DEFAULT_FONT_FAMILY; point
        // them at the journal's own family up front. `sync_reader_font` only
        // re-applies this when the family CHANGES, and the journal's family is
        // pinned — so without this the note body/headings would keep rendering
        // in the gloss's Charter while the rest of the journal used Quattro.
        // Pre-first-work baseline only — `sync_reader_font` re-points these at
        // the family chosen against the ACTUAL reader font on the first work
        // load, including the swap when the reader is on Gentium itself.
        md_tags.set_serif_family(JOURNAL_FONT_FAMILY);
        // Overlay-search highlight tags (Task 2 of the overlay-search feature).
        // Registered once here — same pattern as md_tags — so later search/step
        // logic (Task 3) can apply them without re-registering. Placeholder
        // colors; Task 5 wires these to the theme via `set_search_colors`.
        let search_tag = gtk4::TextTag::builder()
            .name("overlay_search")
            .background("#ffe000")
            .build();
        let search_current_tag = gtk4::TextTag::builder()
            .name("overlay_search_current")
            .background("#ff9000")
            .build();
        view.buffer().tag_table().add(&search_tag);
        view.buffer().tag_table().add(&search_current_tag);
        // Ephemeral rewrite diff-highlight tag (Task 4 of the rewrite-revision-
        // history feature). Same registration pattern as the search tags above;
        // Tasks 5/6 call apply_rewrite_diff, Task 7 calls clear_rewrite_diff.
        let rewrite_diff_tag = gtk4::TextTag::builder()
            .name("rewrite_diff")
            .background("#ffe000") // placeholder; set via set_rewrite_diff_color
            .build();
        view.buffer().tag_table().add(&rewrite_diff_tag);
        // Vocab-word tint tag: same one-time registration as the search tags.
        // Placeholder color; `set_vocab_color` wires it to the theme.
        let vocab_tag = gtk4::TextTag::builder()
            .name("journal_vocab_word")
            .foreground("#3b5bdb") // placeholder; set via set_vocab_color
            .build();
        view.buffer().tag_table().add(&vocab_tag);
        // Word-underline tag for the overlay `-` family
        // (`input::actions::overlay_word_copy`), mirroring the reader's
        // "word-bold" tag. Underline only — no color — so it reads the same on
        // every theme.
        let word_underline_tag = gtk4::TextTag::builder()
            .name("journal_word_underline")
            .underline(gtk4::pango::Underline::Single)
            .build();
        view.buffer().tag_table().add(&word_underline_tag);

        Self {
            overlay,
            scrim,
            container,
            head_work,
            head_division,
            scrolled,
            view,
            word_underline_tag,
            clip_guard,
            footer_container,
            footer_left,
            position_label,
            hint,
            bar_drawing,
            bar_active,
            panel_drawing,
            bar_ranges,
            vim_block_line,
            blocks: RefCell::new(Vec::new()),
            visual_anchor: Cell::new(None),
            cursor_block: Cell::new(0),
            all_paragraphs: RefCell::new(Vec::new()),
            source_para_count: Cell::new(0),
            source_has_speaker: Cell::new(false),
            source_has_citation: Cell::new(false),
            pages: RefCell::new(Vec::new()),
            page_idx: Cell::new(0),
            cursor_full: Cell::new(0),
            entry_pos: Cell::new((0, 0)),
            text_margins: text_margins as i32,
            column_width: column_width as i32,
            prose_reading: Cell::new(false),
            // The journal's OWN family (distinct from every other surface — see
            // JOURNAL_FONT_FAMILY), at the shared overlay size less the journal's
            // delta. Pre-first-work baseline only: `sync_reader_font` re-applies
            // the reader's SIZE (keeping this family) on the first work load.
            // Must be NON-EMPTY — an empty family makes apply_font early-return,
            // which drops the journal to the `.gloss-text` CSS at the reader's
            // size instead of its own tag.
            font_family: RefCell::new(JOURNAL_FONT_FAMILY.to_string()),
            font_size: Cell::new(crate::ui::gloss_overlay::GLOSS_DEFAULT_FONT_SIZE),
            pre_edit_family: RefCell::new(None),
            last_card_size: Cell::new((0, 0)),
            loading_animator: crate::ui::loading_animator::LoadingAnimator::new(),
            ask_host,
            vim_engine: RefCell::new(None),
            vim_seed: RefCell::new(String::new()),
            edit_kind: RefCell::new(String::new()),
            note_blocks: RefCell::new(Vec::new()),
            vim_cursor_colors: RefCell::new((String::new(), String::new())),
            highlight_bg: RefCell::new(crate::ui::DEFAULT_HIGHLIGHT_BG.to_string()),
            hi_ranges: RefCell::new(Vec::new()),
            emphasis_ranges: RefCell::new(Vec::new()),
            md_tags,
            page_is_note: Cell::new(false),
            marker_glyph,
            marker_color,
            bar_color,
            panel_color,
            search_tag,
            search_current_tag,
            vocab_tag,
            rewrite_diff_tag,
            rewrite_diff_active: std::cell::Cell::new(false),
            rewrite_diff_full: RefCell::new(Vec::new()),
            rewrite_diff_shown: std::rc::Rc::new(std::cell::Cell::new(false)),
            rewrite_diff_fade_gen: std::rc::Rc::new(std::cell::Cell::new(0)),
            search_full: RefCell::new(Vec::new()),
            search_full_current: Cell::new(0),
        }
    }

    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_overlay_panel(
            &self.overlay, child, &self.scrim, &self.container,
        );
        // The ask card floats as a right-anchored sibling of the journal card
        // (2-column ask layout). Added last so it paints above the card; hidden
        // until an ask flow opens it. Not measured (its size is fixed by the
        // float width + height reservation) and clipped like the container.
        let ask_container = self.ask_host.card().container();
        self.overlay.add_overlay(ask_container);
        self.overlay.set_measure_overlay(ask_container, false);
        self.overlay.set_clip_overlay(ask_container, true);
    }

    /// Show/hide the doc card's accent bar (its inactive cue). `active=false`
    /// hides the left selection/cursor bar (page marker stays); `true` restores
    /// it. Repaints the bar layer.
    pub fn set_doc_accent_active(&self, active: bool) {
        self.bar_active.set(active);
        self.bar_drawing.queue_draw();
    }

    /// Update the two 2-col cards' focus cues. No dimming: the ask card freezes
    /// its INSERT caret when inactive, and the journal (doc) card hides its
    /// accent bar when inactive. `ask_focused` true → doc bar hidden, ask caret
    /// blinking; false → doc bar shown, ask caret frozen.
    pub fn set_ask_focus_dim(&self, ask_focused: bool) {
        self.set_doc_accent_active(!ask_focused);
        self.ask_host.set_active(ask_focused);
    }

    /// Restore both cards' cues on ask close/submit: doc accent bar back, ask
    /// caret handled by the ask card's own close.
    pub fn clear_focus_dim(&self) {
        self.set_doc_accent_active(true);
    }

    /// Set by the journal action layer before each show: `true` when the
    /// current work is prose, switching `size_card` to the main reading
    /// card's tighter prose margin (see `size_card`).
    /// Set the running-head pair (work abbrev left, position right). MUST be
    /// called before `show_page`/`show_loading` so `size_card` measures the
    /// row at its real height.
    pub fn set_running_head(&self, work: &str, position: &str) {
        self.head_work.set_text(work);
        self.head_division.set_text(position);
    }

    /// Record whether the current work is prose. Now used ONLY to decide the
    /// source quote's verse hang-indent (`apply_source_style`) — the column
    /// margin no longer branches on it (every work type uses card/8), and the
    /// ask card derives its own inset from that same single rule, so there is
    /// nothing to forward.
    pub fn set_prose_reading(&self, prose: bool) {
        self.prose_reading.set(prose);
    }

    fn size_card(&self, card_width: i32, card_height: i32) {
        self.container.set_size_request(card_width, card_height);
        self.last_card_size.set((card_width, card_height));
        // Fixed-scroll-height (the host owns it): the scroll (vexpand off) gets an
        // EXPLICIT height = pane minus the fixed chrome — its height while the ask
        // card is CLOSED. `ask_host.open` subtracts the ask slot, `close` restores
        // this stored closed height. Fixed chrome = the running-head row above the
        // scroll (preferred size includes its 24/12 margins), the scroll_overlay
        // margins (UNACCOUNTED_CHROME_MARGINS = 44, which `preferred_size()`
        // omits), and the footer. Without the head's height the `valign=Center`
        // container grows PAST `card_height` (the gloss overlay's title_division bug).
        let (_, head_h) = self.head_work.preferred_size();
        let (_, footer_h) = self.footer_container.preferred_size();
        self.ask_host.size(
            card_width,
            card_height,
            head_h.height() + UNACCOUNTED_CHROME_MARGINS,
            footer_h.height(),
        );
        // Pin the display scroll's CLOSED height INDEPENDENTLY of the host call
        // above. When the ask card is a float (`enable_float`), `AskCardHost::size`
        // early-returns BEFORE `pin_scroll_height()`, so it never sizes our scroll —
        // leaving `self.scrolled` at height_request=-1 → alloc_h=0 and a blank body.
        // Mirror the gloss overlay's `size_scroll` (gloss_overlay.rs:2082-2087),
        // which survives the same float early-return by pinning its own scroll.
        // The closed height is exactly what `AskCardHost::size` computes in stacked
        // mode: card minus the fixed chrome (head row + scroll_overlay margins,
        // already folded into UNACCOUNTED_CHROME_MARGINS = 44) minus the footer.
        // `.max(80)` floors it like gloss / the scroll_budget_tests helper.
        let scroll_h = (card_height
            - (head_h.height() + UNACCOUNTED_CHROME_MARGINS)
            - footer_h.height())
        .max(80);
        // Order matters: height_request → max → min, so max_content_height is never
        // transiently below min_content_height (avoids the Gtk-CRITICAL assertion
        // `height >= min_content_height` — same order as gloss/pin_scroll_height).
        self.scrolled.set_height_request(scroll_h);
        self.scrolled.set_max_content_height(scroll_h);
        self.scrolled.set_min_content_height(scroll_h);
        self.scrolled.queue_resize();
        // Anchor the text to a card-relative margin rather than the small fixed
        // `text_margins` — otherwise the Q&A prose runs nearly edge to edge on
        // a wide card. Card SIZE is unchanged; only the inner padding grows.
        //
        // ONE margin for every work type: the MAIN reading card's
        // `prose_reading_card_margin` (card/8), so the overlay column matches
        // the 1-col prose layout AND a play's Q&A sits at the same measure as a
        // novel's. Verse/plays previously took the narrower `prose_column_margin`
        // (card/5), which made the same overlay render two different column
        // widths depending on the work — the inconsistency this removes.
        let side = crate::ui::prose_reading_card_margin(card_width);
        // Indent the body right of the accent bar (bar sits in the gutter),
        // matching gloss. Left-only; the right margin stays `side`.
        self.view.set_left_margin(side + JOURNAL_BODY_INDENT);
        self.view.set_right_margin(side);
        let _ = (self.text_margins, self.column_width);
    }

    pub fn show_page(
        &self,
        // Retained for call-site symmetry; the work-citation band is no longer
        // displayed (footer shows only "Q&A n of m").
        _footer_left: &str,
        page_index: usize,
        page_count: usize,
        question: &str,
        answer: &str,
        kind: &str,
        source_para: Option<JournalSource>,
        card_width: i32,
        card_height: i32,
    ) {
        // Stop the loading spinner: this render replaces the loading buffer with
        // the real answer, so no queued tick may repaint over it.
        self.loading_animator.stop();
        // Restore the navigation footer BEFORE sizing: `size_card` reads
        // `footer_container.preferred_size()` to reserve the footer's slot in the
        // fixed-scroll-height budget, but `show_loading` hid the footer during the
        // "Asking…" state. A hidden widget reports height 0, so sizing here (in the
        // post-generation callback) reserved no footer slot — then showing the
        // footer below pushed the `valign=Center` container PAST `card_height`, and
        // the card rendered taller than the reading card (the bug the user saw
        // after a Claude Q&A generated). Show it first so the measurement is real.
        self.footer_container.set_visible(true);
        self.size_card(card_width, card_height);
        // Store the Q&A-entry position; the footer text is (re)built by
        // update_footer_position, which shows "Q&A n of m" (plus the render-page
        // count once pagination has run / on every page turn).
        self.entry_pos.set((page_index, page_count));
        if page_count == 0 {
            // Empty band: a bare message, no navigable paragraphs.
            self.page_is_note.set(false);
            self.note_blocks.borrow_mut().clear();
            self.view.buffer().set_text("No pages yet \u{2014} press Ctrl+a to ask.");
            self.apply_font();
            self.clear_blocks();
            *self.all_paragraphs.borrow_mut() = Vec::new();
            self.pages.borrow_mut().clear();
            self.page_idx.set(0);
            self.cursor_full.set(0);
            self.source_para_count.set(0);
            self.source_has_speaker.set(false);
            self.source_has_citation.set(false);
        } else {
            // Split the full entry into blocks (the shared pagination/render/
            // nav unit), paginate by measured height, and render the first
            // page. j/k step the cursor across the FULL list, turning the page
            // at boundaries — so no partial block is ever rendered at either
            // edge.
            let is_note = kind == "note";
            self.page_is_note.set(is_note);
            if is_note {
                // Notes: plan the Markdown ONCE. The planned blocks are the
                // unit everywhere; all_paragraphs mirrors their plain text
                // (TTS / has_nav_blocks). Cursor starts on the first
                // STOPPABLE block — never the title heading.
                let planned = crate::ui::markdown::plan_markdown_blocks(answer);
                *self.all_paragraphs.borrow_mut() =
                    planned.iter().map(|b| b.plain_text()).collect();
                let first_stop = planned
                    .iter()
                    .position(|b| b.stoppable)
                    .unwrap_or(0);
                *self.note_blocks.borrow_mut() = planned;
                self.cursor_full.set(first_stop);
                self.source_para_count.set(0);
                self.source_has_speaker.set(false);
                self.source_has_citation.set(false);
            } else {
                self.note_blocks.borrow_mut().clear();
                let full = format!("{}\n\n{}", prefix_question(question), answer);
                let mut paras = paragraph_texts(&full);
                // Role flags come straight from the builder (`source_paragraphs`),
                // which knows whether it pushed a speaker/citation paragraph. The
                // old "first paragraph isn't an em-dash line" sniff mistook a
                // speakerless PROSE quote for a speaker line, and the whole quote
                // rendered at the speaker tag's reduced scale.
                let src = source_para.unwrap_or_default();
                self.source_para_count.set(src.paras.len());
                self.source_has_citation.set(src.has_citation);
                self.source_has_speaker.set(src.has_speaker);
                if !src.paras.is_empty() {
                    let mut combined = src.paras;
                    combined.extend(paras);
                    paras = combined;
                }
                *self.all_paragraphs.borrow_mut() = paras;
                // Start on the QUESTION, not the quoted source: the source
                // paragraphs are non-stoppable (see `is_stoppable`), so landing
                // the cursor at 0 would paint the accent bar on the quote and
                // leave `k` unable to return to it once the reader stepped off.
                self.cursor_full.set(self.source_para_count.get());
            }
            self.repaginate();
            self.page_idx.set(0);
            self.render_page();
        }
        // Now the render-page count is known — build the footer with it.
        self.update_footer_position();
        self.ask_host.card().close();
        // Footer already re-shown at the top of show_page (before size_card, so
        // its slot is measured). Left visible here for clarity.
        self.footer_container.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.clip_guard.on_open();
        // The accent bar DRAW reads per-line geometry (line_yrange), which is
        // 0/stale until GTK lays out the buffer just made visible — so the
        // synchronous mark in render_page paints nothing on a fresh open. Repaint
        // once after layout settles (same fix the gloss overlay uses).
        let bar = self.bar_drawing.clone();
        glib::idle_add_local_once(move || bar.queue_draw());

        // Headless test: emit the journal overlay viewport rect once layout
        // settles, so tests/journal_clipping.rs can target the card's region.
        // Connect to the vadjustment's `changed` signal, which fires when GTK
        // first assigns a scroll range (i.e. after the first layout pass) — the
        // same event BottomClipGuard uses to detect settled geometry. Disconnect
        // after the first emission with a non-zero rect.
        if std::env::var_os("LIT_HEADLESS_TEST").is_some() {
            let sc = self.scrolled.clone();
            let view = self.view.clone();
            let adj = sc.vadjustment();
            let id_cell: Rc<std::cell::Cell<Option<glib::SignalHandlerId>>> =
                Rc::new(std::cell::Cell::new(None));
            let id_cell_clone = id_cell.clone();
            let id = adj.connect_changed(move |adj| {
                if let Some(r) = sc.root().and_then(|root| sc.compute_bounds(&root)) {
                    if r.width() > 0.0 && r.height() > 0.0 {
                        crate::logging::log_viewport_rect("TEST_JOURNAL_VIEWPORT_RECT", &r);
                        // The horizontal band ALL text ink must stay inside:
                        // the inset panel span (accent bar at left_margin −
                        // JOURNAL_BODY_INDENT, panel pad beyond it) in window
                        // coords. tests/journal_markdown.rs asserts no ink
                        // outside this band — the guard for the "tag
                        // left-margin replaces the view margin and text
                        // escapes the column" bug class.
                        let pad = crate::ui::PANEL_PAD as i32;
                        let x0 = r.x().round() as i32 + view.left_margin()
                            - JOURNAL_BODY_INDENT
                            - pad;
                        let x1 = r.x().round() as i32 + r.width().round() as i32
                            - view.right_margin()
                            + pad;
                        crate::logging::log(&format!(
                            "TEST_JOURNAL_CONTENT_BAND {} {}",
                            x0, x1
                        ));
                        if let Some(hid) = id_cell_clone.take() {
                            // Disconnect so we only emit once per show_page open.
                            // The adjustment fires again on every scroll, so without
                            // this guard we would spam the log with updates.
                            adj.disconnect(hid);
                        }
                    }
                }
            });
            id_cell.set(Some(id));
        }
    }

    pub fn show_loading(&self, question: &str, label: &str) {
        let (w, h) = self.last_card_size.get();
        if w > 0 {
            self.container.set_size_request(w, h);
        }
        self.apply_font();
        self.ask_host.card().close();
        // Drop the prior page's blocks + bar: during the transient loading
        // state there is no real Q&A page, so Space/a must not read the prior
        // page's cursor paragraph. With no blocks, current_block_text() is None
        // and play_journal_block is a no-op.
        self.clear_blocks();
        // Keep the navigation footer hidden during the loading state. The result
        // render (show_page/show_message) restores it.
        self.footer_container.set_visible(false);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        // Animate the indicator: the sink writes the view buffer each frame.
        // Body = the held question (empty → indicator only). The animator paints
        // frame 0 immediately, so the first paint is correct before any tick.
        let body = if question.trim().is_empty() {
            String::new()
        } else {
            prefix_question(question)
        };
        let view = self.view.clone();
        let sink: std::rc::Rc<dyn Fn(String)> =
            std::rc::Rc::new(move |text: String| view.buffer().set_text(&text));
        self.loading_animator.start(sink, body, label.to_string());
    }

    /// Render a PENDING passage ask: the visually selected source text
    /// (`<speaker>/<segment>` markup) shown through the shared gloss source
    /// renderer (speaker small-caps + verse hang-indent, full ink), in place of
    /// the empty band's "No pages yet — press Ctrl+a to ask." placeholder — so the
    /// reader sees the passage they are asking about while the ask card is open
    /// (mirrors the gloss overlay's "Glossing…" card). No navigable blocks and
    /// no accent bar: the render is transient until submit/cancel.
    pub fn show_passage_source(
        &self,
        // Retained for call-site symmetry; the work-citation band is no longer
        // displayed (footer shows only "Q&A n of m").
        _footer_left: &str,
        source_doc: &str,
        card_width: i32,
        card_height: i32,
    ) {
        // Stop the loading spinner: this render replaces the loading buffer, so
        // no queued tick may repaint over it.
        self.loading_animator.stop();
        self.size_card(card_width, card_height);
        self.entry_pos.set((0, 0));
        // Anchor the source tags at the COLUMN edge (left_margin minus the body
        // indent) — the same anchor the gloss passes as `bar_left`, so the
        // speaker/verse indents land exactly where the gloss card puts them.
        let bar_left = self.view.left_margin() - JOURNAL_BODY_INDENT;
        let _ = crate::ui::gloss_render::populate_gloss_buffer(
            &self.view, source_doc, self.text_margins, bar_left, &[], None,
        );
        self.apply_font();
        self.clear_blocks();
        self.update_footer_position();
        self.footer_container.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.clip_guard.on_open();
    }

    pub fn show_message(&self, text: &str) {
        // Stop the loading spinner: this render replaces the loading buffer, so
        // no queued tick may repaint over it.
        self.loading_animator.stop();
        let (w, h) = self.last_card_size.get();
        if w > 0 {
            self.container.set_size_request(w, h);
        }
        self.view.buffer().set_text(text);
        self.apply_font();
        self.ask_host.card().close();
        // A bare message (toast/empty state) has no navigable Q&A paragraphs.
        self.clear_blocks();
        // Restore the navigation footer (show_loading may have hidden it).
        self.footer_container.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    /// Drop the current page's paragraph blocks + accent bar (used by the
    /// transient loading / message states where there is no real Q&A page to
    /// navigate or read aloud).
    fn clear_blocks(&self) {
        self.blocks.borrow_mut().clear();
        self.cursor_block.set(0);
        self.visual_anchor.set(None);
        self.all_paragraphs.borrow_mut().clear();
        self.note_blocks.borrow_mut().clear();
        self.pages.borrow_mut().clear();
        self.page_idx.set(0);
        self.cursor_full.set(0);
        self.clear_bar();
        *self.marker_glyph.borrow_mut() = None;
        // Stale <hi> char ranges from the last Q&A page must not survive into a
        // block-less buffer (loading / message / pending-passage): a later theme
        // change calls apply_hi_color, which would paint the OLD page's ranges
        // over arbitrary spans of the new text. Emphasis ranges are cleared for
        // the same reason.
        self.hi_ranges.borrow_mut().clear();
        self.emphasis_ranges.borrow_mut().clear();
    }

    pub fn hide(&self) {
        // Universal close funnel: stop any running spinner so leaving the
        // overlay mid-load cannot leave a tick running.
        self.loading_animator.stop();
        self.container.set_visible(false);
        self.scrim.set_visible(false);
        self.ask_host.card().close();
        // Universal close funnel: restore the doc card's accent bar (hidden while
        // the ask card held focus) so the overlay never reopens with a missing bar.
        self.clear_focus_dim();
        // Drop any `-`-family underline: the next open re-renders the buffer,
        // so the stored char offsets would point at unrelated text.
        self.clear_word_underline();
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    /// Set the footer-left label to the band identity (`<abbrev> <act>.<scene>`)
    /// Rebuild the footer from the stored band + Q&A-entry position + the current
    /// render-page count. LEFT label: `<abbrev> <act>.<scene> · Q&A 2 of 5` (the
    /// entry's position in the band, Ctrl+n/p). RIGHT label: a bare `X / Y` render
    /// page within this Q&A (j/k), shown ONLY when the Q&A spans >1 render page —
    /// consistent with the gloss overlay's right-aligned position_label (no "page"
    /// word inline). Call after pagination (page count known) and on every page
    /// turn.
    fn update_footer_position(&self) {
        let (entry_idx, entry_count) = self.entry_pos.get();
        // The far-left footer shows ONLY the Q&A counter; the work-citation band
        // ("Cym 5.3") is dropped — the center pill already names the work/scene.
        let s = if entry_count == 0 {
            "no Q&A yet".to_string()
        } else {
            format!("Q&A {} of {}", entry_idx + 1, entry_count)
        };
        self.footer_left.set_text(&s);

        // Right-aligned bare "X / Y" page counter (gloss-consistent), via the
        // shared helper. Hidden on a single page.
        let n_pages = self.pages.borrow().len();
        match crate::ui::pagination::page_token(self.page_idx.get(), n_pages) {
            Some(token) => {
                self.position_label.set_text(&token);
                self.position_label.set_visible(true);
            }
            None => self.position_label.set_visible(false),
        }
    }

    fn update_bottom_clip(&self) {
        self.clip_guard.recompute();
    }

    pub fn set_font(&self, family: &str, size: i32) {
        *self.font_family.borrow_mut() = family.to_string();
        self.font_size.set(size);
        self.apply_font();
    }

    /// Follow the reader card's font SIZE, and pick a family DISTINCT from the
    /// reader's — what makes the journal legible as its own surface, unlike the
    /// gloss and synopsis (which follow the reader's family outright via
    /// `GlossOverlay::sync_reader_font`).
    ///
    /// The family is the other half of the Charis/Charter pair the journal and
    /// the reading card trade between — see `journal_family_for_reader` for the
    /// table. The SIZE is taken from the reader unchanged: the two faces are
    /// within 6px on the reference line, so no compensation is needed.
    ///
    /// Does NOT run while `begin_edit_font` has stashed the reading family for
    /// the mono edit swap — clobbering `font_family` mid-edit would corrupt the
    /// stash `end_edit_font` restores from (see the gloss overlay's sync for the
    /// full rationale).
    pub fn sync_reader_font(&self, reader_family: &str, size: i32) {
        if self.pre_edit_family.borrow().is_some() {
            return;
        }
        let family = journal_family_for_reader(reader_family);
        // No size correction: Charis and Charter sit within 6px of each other on
        // the reference line, so the journal takes the reader's size as-is.
        let size = size.max(8);
        let family_changed = self.font_family.borrow().as_str() != family;
        let size_changed = self.font_size.get() != size;
        if family_changed || size_changed {
            *self.font_family.borrow_mut() = family.to_string();
            self.font_size.set(size);
            // Keep rendered-markdown serif tags (note body/headings) on the
            // same reading family.
            self.md_tags.set_serif_family(family);
            self.apply_font();
        }
    }

    /// Swap to the monospace edit font, stashing the current reading family so
    /// `end_edit_font` can restore it. Size is unchanged. Idempotent: a second
    /// call without an intervening `end_edit_font` no-ops (the reading family is
    /// already stashed; re-stashing would overwrite it with the mono font and lose
    /// the reading baseline).
    pub fn begin_edit_font(&self) {
        // Already editing: the reading family is already stashed. Do NOT re-stash
        // (the current family is the mono edit font now) or the reading baseline
        // would be lost and never restored on exit. This makes the call idempotent.
        if self.pre_edit_family.borrow().is_some() {
            return;
        }
        let current = self.font_family.borrow().clone();
        *self.pre_edit_family.borrow_mut() = Some(current);
        let size = self.font_size.get();
        self.set_font(crate::ui::EDIT_FONT_FAMILY, size);
    }

    /// Restore the reading font stashed by `begin_edit_font`. No-op when nothing
    /// is stashed, so redundant exit paths (e.g. `:q` after a font-less state)
    /// are safe.
    pub fn end_edit_font(&self) {
        let stashed = self.pre_edit_family.borrow_mut().take();
        if let Some(family) = stashed {
            let size = self.font_size.get();
            self.set_font(&family, size);
        }
    }

    /// Apply the overlay's font (family + size) to the page text and the ask
    /// input via a buffer-wide font TextTag — the same technique the gloss
    /// overlay uses (`GlossOverlay::apply_font`), since this gtk4 version's
    /// per-widget CSS provider path is the deprecated `style_context()` API.
    fn apply_font(&self) {
        let font_str =
            crate::ui::font_string(&self.font_family.borrow(), self.font_size.get());
        crate::ui::apply_font_to_views(
            &[&self.view, self.ask_host.input()],
            &font_str,
            "journal-font",
        );
        // NOTE: the buffer-wide journal-font tag (re-added above, top
        // priority) flattens md-mono code/table/inline-code runs to the
        // reading family. That is CONSISTENT with pagination, which measures
        // every planned block — mono included — in the reading family
        // (measure_planned_block). Do not re-raise md-mono here without also
        // making the measurement mono-aware, or measured != rendered.
    }

    /// Set the `<hi>` highlight background and re-assert it (so a live theme
    /// change repaints the current read view). `apply_hi_color` re-applies over
    /// `hi_ranges`, which are cleared while editing, so this is safe in any mode.
    pub fn set_highlight_color(&self, color: &str) {
        *self.highlight_bg.borrow_mut() = color.to_string();
        self.apply_hi_color();
    }

    /// Set the search-highlight tag colors (theme-wired; see Task 5).
    pub fn set_search_colors(&self, all: &str, current: &str) {
        self.search_tag.set_background(Some(all));
        self.search_current_tag.set_background(Some(current));
    }

    /// Theme wiring for the vocab-word tint (mirrors the main card's
    /// `vocab_tag` color). Called from `build_window` AND the theme-apply path.
    pub fn set_vocab_color(&self, color: &str) {
        self.vocab_tag.set_foreground(Some(color));
    }

    /// Re-scan the CURRENT buffer text and tint vocab words. Idempotent per
    /// populate: remove-then-apply so page turns never stack stale tags.
    /// Mirrors `GlossOverlay::apply_vocab_tags`.
    pub fn apply_vocab_tags(&self, words: &std::collections::HashSet<String>) {
        let buffer = self.view.buffer();
        let (start, end) = (buffer.start_iter(), buffer.end_iter());
        buffer.remove_tag(&self.vocab_tag, &start, &end);
        if words.is_empty() {
            return;
        }
        let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        let spans = crate::vocab_scan::scan_lines(
            text.lines().enumerate(),
            words,
            true, // skip all-caps speaker header lines
        );
        for s in spans {
            if let Some(mut line_iter) = buffer.iter_at_line(s.line_index as i32) {
                let mut a = line_iter.clone();
                a.forward_chars(s.char_start as i32);
                line_iter.forward_chars(s.char_end as i32);
                buffer.apply_tag(&self.vocab_tag, &a, &line_iter);
            }
        }
    }

    /// True while a rewrite diff highlight is currently applied.
    pub fn rewrite_diff_active(&self) -> bool {
        self.rewrite_diff_active.get()
    }

    /// Set the diff-highlight background (theme-wired via set_search_colors' path).
    pub fn set_rewrite_diff_color(&self, color: &str) {
        self.rewrite_diff_tag.set_background(Some(color));
    }

    /// Record the rewrite diff-highlight as char ranges into the WHOLE-entry
    /// body and paint whatever falls on the current page. Stored so `render_page`
    /// can re-tint it after a page turn (long answers paginate — a change on
    /// page 2+ would otherwise be lost, since the buffer holds only one page).
    pub fn apply_rewrite_diff(&self, ranges: &[(i32, i32)]) {
        *self.rewrite_diff_full.borrow_mut() = ranges
            .iter()
            .filter(|(a, b)| b > a)
            .map(|(a, b)| (*a as usize, *b as usize))
            .collect();
        self.rewrite_diff_active.set(!ranges.is_empty());
        self.rewrite_diff_shown.set(!ranges.is_empty());
        self.reapply_rewrite_diff();
        self.arm_rewrite_diff_fade();
    }

    /// Hide the diff tint after `REWRITE_DIFF_FADE_SECS`, KEEPING the ranges so
    /// `flash_rewrite_diff` can bring them back. Mirrors the gloss overlay;
    /// bumping the generation first cancels any timer already in flight, so a
    /// re-flash always gets a full window.
    fn arm_rewrite_diff_fade(&self) {
        let generation = self.rewrite_diff_fade_gen.get() + 1;
        self.rewrite_diff_fade_gen.set(generation);
        if !self.rewrite_diff_shown.get() {
            return;
        }
        let gen_rc = self.rewrite_diff_fade_gen.clone();
        let buffer = self.view.buffer();
        let tag = self.rewrite_diff_tag.clone();
        let shown = self.rewrite_diff_shown.clone();
        glib::timeout_add_local_once(
            std::time::Duration::from_secs(crate::input::rewrite_diff::REWRITE_DIFF_FADE_SECS),
            move || {
                if gen_rc.get() != generation {
                    return;
                }
                let (s, e) = buffer.bounds();
                buffer.remove_tag(&tag, &s, &e);
                shown.set(false);
            },
        );
    }

    /// Re-tint the remembered diff ranges for another fade window (the `w`
    /// bind). Returns false when there is no diff to flash.
    pub fn flash_rewrite_diff(&self) -> bool {
        if self.rewrite_diff_full.borrow().is_empty() {
            return false;
        }
        self.rewrite_diff_shown.set(true);
        self.reapply_rewrite_diff();
        self.arm_rewrite_diff_fade();
        true
    }

    /// Re-tag the stored full-body diff ranges onto the CURRENT page's buffer,
    /// clipping each to the page's char span and shifting to page-local offsets.
    /// Called after each `set_text` in `render_page`, so the highlight survives
    /// page turns (the `<hi>` model). No-op when no ranges are stored.
    ///
    /// Also a no-op once the tint has FADED: the ranges are still remembered
    /// (for `w`), so without the `rewrite_diff_shown` guard a page turn after
    /// the fade would repaint a diff the reader already watched disappear.
    fn reapply_rewrite_diff(&self) {
        let buffer = self.view.buffer();
        let (bs, be) = buffer.bounds();
        buffer.remove_tag(&self.rewrite_diff_tag, &bs, &be);
        if !self.rewrite_diff_shown.get() {
            return;
        }
        let full = self.rewrite_diff_full.borrow();
        if full.is_empty() {
            return;
        }
        let (page_start, page_len) = self.current_page_char_span();
        let page_end = page_start + page_len;
        for (a, b) in full.iter() {
            let s = (*a).max(page_start);
            let e = (*b).min(page_end);
            if s < e {
                let si = buffer.iter_at_offset((s - page_start) as i32);
                let ei = buffer.iter_at_offset((e - page_start) as i32);
                buffer.apply_tag(&self.rewrite_diff_tag, &si, &ei);
            }
        }
    }

    /// Char offset (into the whole-entry body) where the current page begins,
    /// and the page's char length — computed from the cleaned (`<hi>`-stripped)
    /// paragraph slice so it matches the char basis of `render_page`'s `body`
    /// and of the whole-body diff ranges. The page body is
    /// `paras[start..end].join("\n\n")` (2 join chars between blocks).
    fn current_page_char_span(&self) -> (usize, usize) {
        self.page_char_span(self.page_idx.get())
    }

    /// Char span (start offset into the whole-entry body, char length) of the
    /// page at `page_idx`, in the same cleaned (`<hi>`-stripped) basis as
    /// `whole_entry_text` / `render_page`'s `body`. Generalizes
    /// `current_page_char_span` to an ARBITRARY page so `page_for_whole_offset`
    /// can locate which page holds a whole-body offset. Returns `(0, 0)` when the
    /// page index is out of range.
    fn page_char_span(&self, page_idx: usize) -> (usize, usize) {
        let paras = self.all_paragraphs.borrow();
        let pages = self.pages.borrow();
        let Some(page) = pages.get(page_idx) else {
            return (0, 0);
        };
        // Must mirror `render_page`'s cleaning EXACTLY (both strips, in the same
        // order): a search/diff offset computed on a longer basis than the
        // rendered body lands short of its match.
        let clean_chars = |raw: &str| -> usize {
            let hi_clean = crate::ui::gloss_block::strip_hi_spans(raw).0;
            crate::ui::gloss_block::strip_emphasis_spans(&hi_clean)
                .0
                .chars()
                .count()
        };
        // Chars before this page: every earlier paragraph's cleaned length plus
        // the 2-char "\n\n" join that follows each of them.
        let mut start = 0usize;
        for p in paras.iter().take(page.start) {
            start += clean_chars(p) + 2;
        }
        // This page's body length: cleaned paragraphs joined by "\n\n".
        let mut len = 0usize;
        for (i, p) in paras[page.start..page.end].iter().enumerate() {
            if i > 0 {
                len += 2;
            }
            len += clean_chars(p);
        }
        (start, len)
    }

    /// The WHOLE current entry's search text: for a Q&A, every paragraph
    /// `<hi>`-stripped and joined by `"\n\n"` — the SAME cleaned basis
    /// `page_char_span` measures, so a match's char offset maps cleanly onto a
    /// page via `page_for_whole_offset`. For a note page (Markdown-planned, not
    /// the paragraph basis) this falls back to the CURRENT buffer text, so search
    /// stays page-local for notes (the pre-pagination-search behavior) instead of
    /// finding nothing; `jump_to_whole_offset` treats note offsets as buffer-local.
    pub fn whole_entry_text(&self) -> String {
        if self.page_is_note.get() {
            let b = self.view.buffer();
            let (s, e) = b.bounds();
            return b.text(&s, &e, false).to_string();
        }
        let paras = self.all_paragraphs.borrow();
        paras
            .iter()
            .map(|p| {
                // Both strips, in `render_page`'s order — search matches on this
                // basis, and a `/` hit must land where the text actually is.
                let hi_clean = crate::ui::gloss_block::strip_hi_spans(p).0;
                crate::ui::gloss_block::strip_emphasis_spans(&hi_clean).0
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// The page index whose char span (in the whole-entry basis) contains the
    /// whole-body char offset `off`. Clamps to the last page if `off` is past the
    /// end; returns 0 when there are no pages. Mirrors
    /// `pagination::page_containing_block` but over char spans, so overlay search
    /// can turn to the page holding a match. No-op basis for note pages (returns
    /// current page) — callers gate on `whole_entry_text` being non-empty first.
    pub fn page_for_whole_offset(&self, off: usize) -> usize {
        let n_pages = self.pages.borrow().len();
        if n_pages == 0 {
            return 0;
        }
        for i in 0..n_pages {
            let (start, len) = self.page_char_span(i);
            if off >= start && off < start + len {
                return i;
            }
        }
        n_pages - 1
    }

    /// Turn to the page containing whole-body char offset `off` (search jump),
    /// re-mark the accent bar on that page, and scroll the match on-screen. Turns
    /// the page via `page_idx` + `render_page` when `off` is on a different page.
    /// Then converts `off` to a page-local offset and scrolls to it. No-op for
    /// note pages / empty entries (the buffer already holds the whole thing).
    pub fn jump_to_whole_offset(&self, off: usize) {
        if self.page_is_note.get() || self.all_paragraphs.borrow().is_empty() {
            // Note / empty: the buffer is the whole entry, so `off` is already a
            // buffer offset — just scroll to it.
            self.scroll_to_char_offset(off as i32);
            return;
        }
        let target_page = self.page_for_whole_offset(off);
        if target_page != self.page_idx.get() {
            self.page_idx.set(target_page);
            self.render_page();
            self.update_footer_position();
        }
        let (page_start, _) = self.page_char_span(target_page);
        let local = off.saturating_sub(page_start) as i32;
        self.cursor_to_char_offset(local);
        self.scroll_to_char_offset(local);
    }

    /// Store `search`'s WHOLE-body match spans + current index so `render_page`
    /// can re-tag whatever falls on the shown page (survives page turns within a
    /// paginated entry). Paints the current page immediately. Mirrors
    /// `apply_rewrite_diff` — the spans are page-clipped, not buffer offsets.
    pub fn set_search_matches(&self, search: &crate::input::overlay_search::OverlaySearch) {
        *self.search_full.borrow_mut() = search.matches.clone();
        self.search_full_current.set(search.current);
        self.reapply_search();
    }

    /// Drop the stored search spans and remove both search tags over the buffer
    /// (Escape clears the search). After this a page turn no longer re-paints it.
    pub fn clear_search_tags(&self) {
        self.search_full.borrow_mut().clear();
        let buffer = self.view.buffer();
        let (bs, be) = buffer.bounds();
        buffer.remove_tag(&self.search_tag, &bs, &be);
        buffer.remove_tag(&self.search_current_tag, &bs, &be);
    }

    /// Re-tag the stored WHOLE-body search match spans onto the CURRENT page's
    /// buffer, clipping each to the page's char span and shifting to page-local
    /// offsets (mirrors `reapply_rewrite_diff`). The current match gets the
    /// brighter `search_current_tag` when it falls on this page. Called from
    /// `render_page` so search highlights survive page turns within an entry.
    fn reapply_search(&self) {
        let buffer = self.view.buffer();
        let (bs, be) = buffer.bounds();
        buffer.remove_tag(&self.search_tag, &bs, &be);
        buffer.remove_tag(&self.search_current_tag, &bs, &be);
        let matches = self.search_full.borrow();
        if matches.is_empty() {
            return;
        }
        // Note pages search the buffer directly (whole_entry_text == buffer text),
        // so the stored spans are already buffer-local: use the full buffer as the
        // "page" span. Q&A pages clip whole-body spans to the paragraph-basis page.
        let (page_start, page_len) = if self.page_is_note.get() {
            (0usize, buffer.char_count() as usize)
        } else {
            self.current_page_char_span()
        };
        let page_end = page_start + page_len;
        let paint = |a: i32, b: i32, tag: &gtk4::TextTag| {
            let s = (a as usize).max(page_start);
            let e = (b as usize).min(page_end);
            if s < e {
                let si = buffer.iter_at_offset((s - page_start) as i32);
                let ei = buffer.iter_at_offset((e - page_start) as i32);
                buffer.apply_tag(tag, &si, &ei);
            }
        };
        for (a, b) in matches.iter() {
            paint(*a, *b, &self.search_tag);
        }
        if let Some((a, b)) = matches.get(self.search_full_current.get()) {
            paint(*a, *b, &self.search_current_tag);
        }
    }

    /// Remove the diff-highlight tag over the whole buffer and forget the stored
    /// full-body ranges (so a page turn no longer re-paints it).
    pub fn clear_rewrite_diff(&self) {
        let buffer = self.view.buffer();
        let (s, e) = buffer.bounds();
        buffer.remove_tag(&self.rewrite_diff_tag, &s, &e);
        self.rewrite_diff_full.borrow_mut().clear();
        self.rewrite_diff_active.set(false);
        // Forget the tint AND the ranges: unlike the fade, a clear leaves
        // nothing for `w` to flash. The generation bump cancels any fade timer
        // still in flight so it cannot fire against a later diff.
        self.rewrite_diff_shown.set(false);
        self.rewrite_diff_fade_gen
            .set(self.rewrite_diff_fade_gen.get() + 1);
    }

    /// Scroll the view so the given char offset is on-screen. Creates a
    /// throwaway mark at the offset, scrolls it into view, then deletes it
    /// (matches the `get_insert`/`scroll_mark_onscreen` idiom used for the vim
    /// cursor above).
    pub fn scroll_to_char_offset(&self, off: i32) {
        let buffer = self.view.buffer();
        let iter = buffer.iter_at_offset(off);
        let mark = buffer.create_mark(None, &iter, false);
        self.view.scroll_mark_onscreen(&mark);
        buffer.delete_mark(&mark);
    }

    /// Set the page-marker glyph's dim color (theme `dim_fg`) and repaint the bar.
    pub fn set_marker_color(&self, hex: &str) {
        crate::ui::set_rc_color(hex, &self.marker_color, &self.bar_drawing);
    }

    /// Set the inset panel tint color (theme `panel_bg`) and repaint the panel.
    pub fn set_panel_color(&self, hex: &str) {
        crate::ui::set_rc_color(hex, &self.panel_color, &self.panel_drawing);
    }

    /// Set the selection accent-bar color (theme `root_color`) and repaint the
    /// bar — matches the gloss overlay's theme-wired bar.
    pub fn set_bar_color(&self, hex: &str) {
        crate::ui::set_rc_color(hex, &self.bar_color, &self.bar_drawing);
    }

    fn apply_hi_color(&self) {
        let buffer = self.view.buffer();
        let table = buffer.tag_table();
        let ranges = self.hi_ranges.borrow();
        if table.lookup("journal-hi").is_none() && !ranges.is_empty() {
            table.add(
                &gtk4::TextTag::builder()
                    .name("journal-hi")
                    .background(&*self.highlight_bg.borrow())
                    .build(),
            );
        }
        if let Some(tag) = table.lookup("journal-hi") {
            tag.set_background(Some(&self.highlight_bg.borrow()));
            for &(s, e) in ranges.iter() {
                let si = buffer.iter_at_offset(s as i32);
                let ei = buffer.iter_at_offset(e as i32);
                buffer.apply_tag(&tag, &si, &ei);
            }
        }
    }

    /// Paint `*italic*` / `**bold**` spans stripped by `strip_emphasis_spans`,
    /// reusing the buffer's already-registered Markdown tags. Runs after
    /// `set_text` (like `apply_hi_color`), and is a no-op for note pages and for
    /// bodies with no emphasis.
    fn apply_emphasis(&self) {
        let spans = self.emphasis_ranges.borrow();
        if spans.is_empty() {
            return;
        }
        let buffer = self.view.buffer();
        // `apply_font` re-adds the buffer-wide `journal-font` tag at TOP
        // priority over the whole body, which flattens any style a lower tag
        // asks for — that is why merely applying md-italic here rendered
        // upright text with the markers correctly stripped. Re-raise the two
        // emphasis tags above it, the same trick `reassert_italic_tags` does
        // for the gloss stage/bracket italics. Must run AFTER apply_font, which
        // it does: render_page calls apply_font, then apply_hi_color, then this.
        // Distinct priorities: GTK keeps tag priorities unique and setting one
        // shuffles the rest, so raise bold to the top and italic just under it
        // (they never need to out-rank each other — a run is one or the other).
        let table = buffer.tag_table();
        let top = table.size();
        if top > 1 {
            self.md_tags.bold.set_priority(top - 1);
            self.md_tags.italic.set_priority(top - 2);
        }
        for span in spans.iter() {
            let tag = if span.bold {
                &self.md_tags.bold
            } else {
                &self.md_tags.italic
            };
            let si = buffer.iter_at_offset(span.start as i32);
            let ei = buffer.iter_at_offset(span.end as i32);
            buffer.apply_tag(tag, &si, &ei);
        }
    }

    /// Style the prepended passage source on page 0: small-caps-ish speaker
    /// (smaller + bold), hang-indented verse, dim italic right-aligned
    /// citation. No-op off page 0 or when there is no source block.
    /// Runs after `set_text` (like `apply_hi_color`), applying tags by buffer
    /// line. The source paragraphs are the first `source_para_count` entries of
    /// `all_paragraphs`, joined by "\n\n"; `source_line_roles` maps each to its
    /// buffer line(s) from the paragraph texts (the verse block is one
    /// `\n`-joined paragraph spanning several lines).
    fn apply_source_style(&self) {
        if self.page_idx.get() != 0 {
            return;
        }
        let count = self.source_para_count.get();
        if count == 0 {
            return;
        }
        let buffer = self.view.buffer();
        let table = buffer.tag_table();
        // The quote indents are ABSOLUTE pixel margins derived from the current
        // card's column edge, so they MUST be recomputed per render — a tag
        // cached by `lookup().is_none()` keeps the margin of whatever card built
        // it first and outdents the source on every later card of a different
        // width. Remove and rebuild each time.
        //
        // Anchor at the COLUMN edge (`left_margin` minus the body indent, the
        // same value `show_passage_source` passes the gloss renderer as
        // `bar_left`) and reuse the gloss's own indent constants, so a journal
        // source block sits exactly where the gloss card puts it.
        let bar_left = self.view.left_margin() - JOURNAL_BODY_INDENT;
        for name in ["journal-src-speaker", "journal-src-verse", "journal-src-gap"] {
            if let Some(old) = table.lookup(name) {
                table.remove(&old);
            }
        }
        // Collapse the blank separator lines INSIDE the quotation by shrinking
        // their font (see `SOURCE_GAP_SCALE` — negative pixel spacing is not an
        // option; GTK aborts on it).
        table.add(
            &gtk4::TextTag::builder()
                .name("journal-src-gap")
                .scale(SOURCE_GAP_SCALE)
                .build(),
        );
        table.add(
            &gtk4::TextTag::builder()
                .name("journal-src-speaker")
                // Match the main reading card's speaker-name scale
                // (formatting.rs `speaker-name`, 0.75) — the only overlay
                // text allowed below the card's body size.
                .scale(0.75)
                .weight(600)
                .left_margin(bar_left + crate::ui::gloss_render::QUOTE_SPEAKER_INDENT)
                .build(),
        );
        // Verse hangs one dialogue step past the speaker label, matching the
        // main card's speaker→dialogue hang-indent (the gloss's
        // `QUOTE_VERSE_INDENT` when a speaker is present; without a label the
        // verse sits at the speaker's own indent, as `populate_verse_buffer`
        // does).
        let verse_indent = if self.source_has_speaker.get() {
            crate::ui::gloss_render::QUOTE_VERSE_INDENT
        } else {
            crate::ui::gloss_render::QUOTE_SPEAKER_INDENT
        };
        table.add(
            &gtk4::TextTag::builder()
                .name("journal-src-verse")
                .left_margin(bar_left + verse_indent)
                // Turnovers of a long verse line tuck under the hang indent.
                .indent(-28)
                .build(),
        );
        if table.lookup("journal-src-citation").is_none() {
            table.add(
                &gtk4::TextTag::builder()
                    .name("journal-src-citation")
                    .justification(gtk4::Justification::Right)
                    .style(gtk4::pango::Style::Italic)
                    .build(),
            );
        }
        let paras = self.all_paragraphs.borrow();
        let roles = source_line_roles(
            &paras[..count.min(paras.len())],
            self.source_has_speaker.get(),
            self.source_has_citation.get(),
        );
        drop(paras);
        let apply_line = |name: &str, line: i32| {
            if let Some(tag) = table.lookup(name) {
                let Some(start) = buffer.iter_at_line(line) else {
                    return;
                };
                let mut end = start.clone();
                if !end.ends_line() {
                    end.forward_to_line_end();
                }
                buffer.apply_tag(&tag, &start, &end);
            }
        };
        if let Some(l) = roles.speaker_line {
            apply_line("journal-src-speaker", l);
        }
        // The hang-indent verse tag is for VERSE quotes: each quoted line is its
        // own buffer line, and a long line's turnover tucks under the -28
        // indent. A PROSE quote is ONE long wrapped buffer line — the hang
        // indent would push every wrapped row 28px right of the first (ragged
        // left edge), so prose quotes stay untagged at the body margin.
        if !self.prose_reading.get() {
            for l in &roles.verse_lines {
                apply_line("journal-src-verse", *l);
            }
        }
        if let Some(l) = roles.citation_line {
            apply_line("journal-src-citation", l);
        }
        // The gap lines are EMPTY, so `apply_line`'s start..line-end range would
        // be zero-length and tag nothing. Span the line's trailing newline
        // instead — that is the character carrying the blank line's height.
        if let Some(tag) = table.lookup("journal-src-gap") {
            for l in &roles.inner_gap_lines {
                let Some(start) = buffer.iter_at_line(*l) else {
                    continue;
                };
                let mut end = start.clone();
                if !end.forward_line() {
                    continue;
                }
                buffer.apply_tag(&tag, &start, &end);
            }
        }
    }

    /// Set the floating page marker for the current page: `⌄` when another page
    /// follows, `•` on the last page, hidden on single-page content. The marker
    /// is an overlay child floating just BELOW the page's last block (NOT in the
    /// text flow), so it shows even when the page is full. Glyph chosen by the
    /// shared `pagination::page_marker`. Mirrors `GlossOverlay::update_page_marker`.
    ///
    /// The glyph is stored for the bar's draw func and the bar is repainted; the
    /// draw func reads live line geometry each paint, so there is no allocation
    /// race and the marker lands correctly the moment the page reflows.
    fn update_page_marker(&self, page_idx: usize, n_pages: usize) {
        *self.marker_glyph.borrow_mut() = crate::ui::pagination::page_marker(page_idx, n_pages);
        self.bar_drawing.queue_draw();
        // The draw reads live line geometry; a page turn's reflow may not have run
        // yet, so also repaint on the next idle (after layout) so the glyph lands
        // at the new page's last line even when the scroll range didn't change.
        let bar = self.bar_drawing.clone();
        glib::idle_add_local_once(move || bar.queue_draw());
    }

    pub fn ask_is_open(&self) -> bool {
        self.ask_host.is_open()
    }

    /// True when the ask card is open in 2-col float layout.
    pub fn is_ask_float(&self) -> bool {
        self.ask_host.is_ask_float()
    }

    pub fn open_ask_card(
        &self,
        title: &str,
        hint: &str,
        legend: &str,
        block_fill: &str,
        block_fg: &str,
    ) {
        // The host reveals the ask card (a vim editor, NORMAL by default), hides
        // the nav footer, shrinks the scroll viewport (occlusion fix), recomputes
        // the clip. apply_font re-fonts the now-visible input. block_fill/fg are
        // the NORMAL-mode block-cursor colors. `legend` is centered how-to text
        // over the empty box, cleared on INSERT.
        self.ask_host.open(title, hint, legend, block_fill, block_fg);
        self.apply_font();
        // A fresh ask card always starts focused, so dim the doc card from the
        // moment it opens (mirrors the gloss overlay). Without this the journal
        // page stayed at full brightness until a Ctrl+Tab toggled the dim.
        self.set_ask_focus_dim(true);

        // Headless test: emit the scrolled viewport rect WITH the ask card open
        // (the exact regression from Tasks 1-5). The card open shrinks the
        // scrolled window's height; this idle fires after that layout pass, so
        // the rect reflects the reduced viewport. Tests/journal_clipping.rs reads
        // TEST_JOURNAL_ASK_VIEWPORT_RECT for the ask-open assertion.
        if std::env::var_os("LIT_HEADLESS_TEST").is_some() {
            let sc = self.scrolled.clone();
            glib::idle_add_local_once(move || {
                if let Some(r) = sc.root().and_then(|root| sc.compute_bounds(&root)) {
                    crate::logging::log_viewport_rect("TEST_JOURNAL_ASK_VIEWPORT_RECT", &r);
                } else {
                    crate::logging::log(
                        "TEST_JOURNAL_ASK_VIEWPORT_RECT unavailable (root/compute_bounds returned None)",
                    );
                }
            });
        }
    }

    pub fn close_ask_card(&self) {
        // The host hides the ask card, re-shows the footer, restores the scroll's
        // stored CLOSED height, and recomputes the clip.
        self.ask_host.close();
    }


    pub fn take_ask_text(&self) -> String {
        self.ask_host.take_text()
    }

    /// Feed a key to the ask card's vim engine (the prompt is a modal editor).
    pub fn feed_ask_vim_key(
        &self,
        key: crate::input::vim::VimKey,
    ) -> crate::input::vim::EditorAction {
        self.ask_host.feed_vim_key(key)
    }

    /// Paste system-clipboard text into the ask card's vim engine.
    pub fn paste_ask_text(&self, text: &str) {
        self.ask_host.paste_text(text);
    }

    // ---- in-place vim editor (the `e` bind) ----

    /// Enter the in-place vim editor: build the `Q: …\n\n<answer>` buffer, seed
    /// the engine, make the page view show the whole buffer (pagination
    /// suspended), place the cursor, and show the mode indicator in the footer.
    pub fn enter_edit_buffer(&self, question: &str, answer: &str, block_fill: &str, block_fg: &str, kind: &str) {
        self.begin_edit_font();
        // The editor shows RAW text (with `<hi>` and `*` literals); the
        // read-mode hi/emphasis ranges are stale here and must not be
        // re-applied to the raw buffer — their offsets are for the STRIPPED
        // body, so on raw text they would land mid-word.
        self.hi_ranges.borrow_mut().clear();
        self.emphasis_ranges.borrow_mut().clear();
        *self.vim_cursor_colors.borrow_mut() = (block_fill.to_string(), block_fg.to_string());
        *self.edit_kind.borrow_mut() = kind.to_string();
        let buf = if kind == "note" {
            crate::input::vim::journal_doc::build_note_buffer(answer)
        } else {
            crate::input::vim::journal_doc::build_buffer(question, answer)
        };
        *self.vim_seed.borrow_mut() = buf.clone();
        let engine = crate::input::vim::VimEngine::new(buf);
        // Render the whole buffer (no pagination while editing).
        self.view.buffer().set_text(engine.buffer());
        self.apply_font();
        *self.vim_engine.borrow_mut() = Some(engine);
        // Show the text caret while editing. GTK only PAINTS the caret when the
        // TextView holds keyboard focus, so the read view's `focusable(false)`
        // must be lifted and focus grabbed — otherwise there is no visible
        // insertion point (the "no insertion point" bug). Key routing is on the
        // window's capture-phase controller, so giving the view focus does not
        // change which handler sees keys.
        self.view.set_cursor_visible(true);
        self.view.set_focusable(true);
        let _ = self.view.grab_focus();
        // Hide the floating page marker + accent bar while editing.
        *self.marker_glyph.borrow_mut() = None;
        self.clear_bar();
        self.mirror_engine();
        // Scroll to top so the start of the Q&A is visible.
        let adj = self.scrolled.vadjustment();
        adj.set_value(adj.lower());
    }

    /// Feed one key to the engine, mirror the result to the view, and return the
    /// `EditorAction` the engine asks the host to perform.
    pub fn feed_edit_key(&self, key: crate::input::vim::VimKey) -> crate::input::vim::EditorAction {
        let action = {
            let mut guard = self.vim_engine.borrow_mut();
            let Some(engine) = guard.as_mut() else {
                return crate::input::vim::EditorAction::Nop;
            };
            let outcome = engine.handle_key(key);
            outcome.action
        };
        self.mirror_engine();
        action
    }

    /// Paste system-clipboard text into the in-place vim editor and mirror.
    pub fn paste_edit_text(&self, text: &str) {
        {
            let mut guard = self.vim_engine.borrow_mut();
            let Some(engine) = guard.as_mut() else { return };
            let _ = engine.paste_text(text);
        }
        self.mirror_engine();
    }

    /// The current edited Q&A, parsed back from the engine buffer.
    /// For `note` entries, returns `("", raw_answer)` (question is always empty).
    /// For `qa` entries, returns the normal `(question, answer)` parse.
    pub fn edit_buffer_qa(&self) -> (String, String) {
        let guard = self.vim_engine.borrow();
        match guard.as_ref() {
            Some(e) => {
                if self.edit_kind.borrow().as_str() == "note" {
                    let answer = crate::input::vim::journal_doc::parse_note_back(e.buffer());
                    (String::new(), answer)
                } else {
                    crate::input::vim::journal_doc::parse_back(e.buffer())
                }
            }
            None => (String::new(), String::new()),
        }
    }

    /// Reset the dirty baseline to the engine's CURRENT buffer (called after a
    /// non-quit `:w` so the just-saved text becomes "clean"). The `q`/`a` args are
    /// unused — the seed tracks the raw buffer — but kept for caller clarity.
    pub fn reseed_edit_buffer(&self, _q: &str, _a: &str) {
        let cur = self
            .vim_engine
            .borrow()
            .as_ref()
            .map(|e| e.buffer().to_string());
        if let Some(buf) = cur {
            *self.vim_seed.borrow_mut() = buf;
        }
    }

    /// Whether the edit buffer differs from what it was seeded with.
    pub fn edit_is_dirty(&self) -> bool {
        let guard = self.vim_engine.borrow();
        match guard.as_ref() {
            Some(e) => e.buffer() != self.vim_seed.borrow().as_str(),
            None => false,
        }
    }

    /// Leave the vim editor: drop the engine and restore the read view's
    /// non-editable, non-focusable state. Does NOT clear the buffer text — the
    /// caller re-renders the read page (clearing here left a blank card when the
    /// caller opened the rewrite prompt without an immediate re-render).
    pub fn exit_edit_buffer(&self) {
        *self.vim_engine.borrow_mut() = None;
        self.vim_seed.borrow_mut().clear();
        crate::ui::clear_block_cursor(&self.view.buffer(), "journal-vim-block");
        *self.vim_block_line.borrow_mut() = None;
        self.bar_drawing.queue_draw();
        self.view.set_cursor_visible(false);
        self.view.set_focusable(false);
        self.end_edit_font();
    }

    /// Write the engine's buffer + cursor + selection + mode indicator into the
    /// page view. The view itself stays non-editable — the engine is the source
    /// of truth and we paint it here.
    fn mirror_engine(&self) {
        let guard = self.vim_engine.borrow();
        let Some(engine) = guard.as_ref() else { return };
        let buffer = self.view.buffer();
        // Only rewrite the text when it actually changed (cheap guard; avoids
        // resetting marks on pure cursor moves).
        let current = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        if current != engine.buffer() {
            buffer.set_text(engine.buffer());
            self.apply_font();
        }
        // Char index -> byte offset for GtkTextIter.
        let char_to_iter = |ci: usize| -> gtk4::TextIter {
            let n_chars = engine.buffer().chars().count();
            let ci = ci.min(n_chars);
            buffer.iter_at_offset(ci as i32)
        };
        // Selection (Visual) or plain cursor.
        if let Some(sel) = engine.selection() {
            let start = char_to_iter(sel.start);
            let end = char_to_iter(sel.end);
            buffer.select_range(&start, &end);
        } else {
            let cur = char_to_iter(engine.cursor());
            buffer.place_cursor(&cur);
        }
        // Cursor style: a solid BLOCK over the char in NORMAL/VISUAL, the thin
        // native caret in INSERT (vim convention).
        let insert_mode = engine.mode() == crate::input::vim::Mode::Insert;
        if insert_mode {
            crate::ui::clear_block_cursor(&buffer, "journal-vim-block");
            *self.vim_block_line.borrow_mut() = None;
            self.view.set_cursor_visible(true);
        } else {
            let (fill, fg) = self.vim_cursor_colors.borrow().clone();
            crate::ui::paint_block_cursor(&buffer, "journal-vim-block", &fill, &fg, engine.cursor());
            // On a BLANK line the cursor char is the line's `\n` (no glyph cell),
            // so the char-background paints nothing. Draw a left-edge block via
            // `bar_drawing` instead (cleared otherwise). A line is blank when its
            // cursor iter both starts and ends the line.
            let cur_iter = char_to_iter(engine.cursor());
            let on_blank = cur_iter.starts_line() && cur_iter.ends_line();
            if on_blank {
                let rgb = crate::ui::gloss_util::parse_hex_color(&fill)
                    .unwrap_or((0.53, 0.62, 0.71));
                *self.vim_block_line.borrow_mut() =
                    Some((cur_iter.line(), rgb.0, rgb.1, rgb.2));
            } else {
                *self.vim_block_line.borrow_mut() = None;
            }
            self.bar_drawing.queue_draw();
            // Hide the native caret so it doesn't sit inside the block; but at true
            // end-of-buffer (and not a blank line) there is no block, so keep it.
            let at_end = engine.cursor() >= engine.buffer().chars().count() && !on_blank;
            self.view.set_cursor_visible(at_end);
        }
        // Keep the cursor on screen.
        let mark = buffer.get_insert();
        self.view.scroll_mark_onscreen(&mark);
        // Mode indicator in the footer-left label.
        let indicator = match engine.cmdline() {
            Some(cmd) => format!(":{cmd}"),
            None => match engine.mode() {
                crate::input::vim::Mode::Normal => "-- NORMAL --  (e edit · :w save · R rewrite · :q quit)".to_string(),
                crate::input::vim::Mode::Insert => "-- INSERT --".to_string(),
                crate::input::vim::Mode::Visual => "-- VISUAL --".to_string(),
                crate::input::vim::Mode::VisualLine => "-- VISUAL LINE --".to_string(),
            },
        };
        self.footer_left.set_text(&indicator);
        self.position_label.set_visible(false);
    }

    /// The usable viewport height one rendered page may fill — the closed scroll
    /// budget the AskCardHost pins (card minus the scroll_overlay margins + footer;
    /// there is no title header). Used as the `paginate` page_height.
    fn page_height(&self) -> i32 {
        let (_, card_h) = self.last_card_size.get();
        let (_, footer_h) = self.footer_container.preferred_size();
        // Subtract the VIEW's own top/bottom padding (28+28) too: it lives
        // INSIDE the scrolled viewport, so content taller than
        // `viewport − padding` clips at the card bottom. The old Q&A-only
        // estimator hid this shortfall under its per-block line_h slack; the
        // exact note measurement exposed it (page-1 tail line clipped).
        (card_h - UNACCOUNTED_CHROME_MARGINS - footer_h.height()
            - self.view.top_margin()
            - self.view.bottom_margin())
        .max(80)
    }

    /// Measure each full paragraph and pack them into `pages` (whole blocks per
    /// page). Heights come from a standalone `pango::Layout` at the view's font +
    /// wrap width, plus the real blank-line gap between paragraphs, so
    /// pagination doesn't over-pack. No widget allocation — no settle race.
    fn repaginate(&self) {
        let paras = self.all_paragraphs.borrow();
        if paras.is_empty() {
            self.pages.borrow_mut().clear();
            return;
        }
        let family = self.font_family.borrow().clone();
        let size = self.font_size.get();
        // Wrap width = the view's content width (card minus the LEFT and RIGHT
        // margins). These are asymmetric: the left carries JOURNAL_BODY_INDENT
        // (body pushed right of the accent bar) while the right does not, so
        // subtract each separately — `2 * left_margin` would over-narrow the
        // measured width by JOURNAL_BODY_INDENT and over-paginate (a short/extra
        // page). Must match the buffer's actual left+right margins.
        let wrap_w = (self.last_card_size.get().0
            - self.view.left_margin()
            - self.view.right_margin())
        .max(1);
        let pctx = self.view.pango_context();
        // A rendered page is `slice.join("\n\n")` — each paragraph plus one
        // blank line per gap — and the view has NO per-line spacing
        // (apply_font_to_views sets only a font tag), so a standalone
        // `pango::Layout` at the same font + wrap width measures the render
        // exactly: page height = Σ text_h + (k-1)·line_h. Charging every block
        // text_h + line_h therefore over-counts by exactly ONE line_h per page
        // (the last block's gap never renders) — deliberate headroom, so
        // packing can never under-count and clip a paragraph tail (the old
        // paragraph-split / dropped-text bug). A ×1.15 slack used to sit on
        // top of this from when the view added per-line leading; with that
        // spacing gone it was pure over-count (~15% of every paragraph) and
        // UNDERFILLED pages — a fitting paragraph got pushed to the next page
        // (Cym 1.4 Q&A id 14; JOURNAL-PAGINATE log confirmed the estimates
        // summed well under the budget while a block still moved at tighter
        // geometries).
        let line_h = crate::ui::pagination::measure_text_height(&pctx, "Mg", size, &family, wrap_w);
        let heights: Vec<i32> = if self.page_is_note.get() {
            // Notes: measure each PLANNED Markdown block at its real style —
            // heading scale/weight, list/quote extra indent — plus its tag
            // pixels_above/below. The render inserts no blank lines, so the
            // sum of these heights IS the rendered page height (same unit,
            // no over- or under-count).
            let blocks = self.note_blocks.borrow();
            blocks
                .iter()
                .map(|b| {
                    crate::ui::markdown::measure_planned_block(&pctx, b, size, &family, wrap_w)
                })
                .collect()
        } else {
            paras
                .iter()
                .map(|p| {
                    // Measured on the RAW paragraph, so `<hi>` tags and `*`
                    // emphasis markers are counted though the render strips
                    // them. That is deliberate: the estimate errs LONG, which
                    // packs a page slightly loose rather than clipping its last
                    // line — the safe direction. (Same pre-existing behavior
                    // `<hi>` has always had.)
                    // Leaded: the view renders with pixels_inside_wrap.
                    let text_h = crate::ui::pagination::measure_text_height_leaded(
                        &pctx, p, size, &family, wrap_w,
                    );
                    text_h + line_h
                })
                .collect()
        };
        drop(paras);
        // Budget + per-block estimates for diagnosing pack decisions from a run.
        crate::log_fmt!(
            // Log the RESOLVED font string (comma form), not a bare
            // "family size" — the bare form is exactly what mis-parses, so
            // printing it would mask the fallback it is meant to reveal.
            "JOURNAL-PAGINATE: page_h={} wrap_w={} line_h={} font='{}' heights={:?}",
            self.page_height(), wrap_w, line_h,
            crate::ui::font_string(&family, size), heights
        );
        *self.pages.borrow_mut() = if self.page_is_note.get() {
            // Notes: no-orphaned-heading packing — chrome blocks glue forward
            // onto their content block so a page never ends on a heading.
            let stoppable: Vec<bool> = self
                .note_blocks
                .borrow()
                .iter()
                .map(|b| b.stoppable)
                .collect();
            crate::ui::pagination::paginate_note_blocks(
                &heights,
                &stoppable,
                self.page_height(),
            )
        } else {
            // Q&A: a HARD break after the quoted source passage, so the question
            // always starts a page instead of trailing the passage partway down
            // one. The source is the first `source_para_count` paragraphs
            // (0 when the entry quotes nothing, which makes this a plain
            // `paginate`). A multi-page source still ends its last page early —
            // that is the point: the question stays at the top of the next one.
            let src = self.source_para_count.get();
            crate::ui::pagination::paginate_with_breaks(
                &heights,
                &[src],
                self.page_height(),
            )
        };
    }

    /// Render ONLY the current page's paragraphs into the buffer (joined by blank
    /// lines), re-derive the per-page `blocks` (their buffer-line spans for the
    /// accent bar + visual mode), project the full cursor to its page-local block,
    /// and mark the bar. No scrolling: the buffer holds exactly whole blocks that
    /// fit, so no partial paragraph is shown at either edge.
    fn render_page(&self) {
        let paras = self.all_paragraphs.borrow();
        let pages = self.pages.borrow();
        let n_pages = pages.len();
        let pidx = self.page_idx.get().min(n_pages.saturating_sub(1));
        let Some(page) = pages.get(pidx) else {
            drop(paras);
            drop(pages);
            self.clear_blocks();
            return;
        };
        let page_start = page.start;
        let page_end = page.end.min(paras.len());
        let is_note = self.page_is_note.get();
        let body = if is_note {
            String::new()
        } else {
            let slice = &paras[page_start..page_end];
            let raw_body = slice.join("\n\n");
            // Strip inline `<hi>` for display, recording the highlight ranges
            // so the `journal-hi` background is re-applied after set_text.
            // Blocks are derived from the CLEAN body so line indices line up
            // with what's shown. (Notes have no <hi> spans.)
            let (body, hi_ranges) = crate::ui::gloss_block::strip_hi_spans(&raw_body);
            *self.hi_ranges.borrow_mut() = hi_ranges;
            body
        };
        // Emphasis is stripped from the ALREADY `<hi>`-cleaned body, so both
        // range sets index the same final string — the one handed to set_text.
        // Notes skip this: their markers are consumed by the block renderer.
        let body = if is_note {
            self.emphasis_ranges.borrow_mut().clear();
            body
        } else {
            let (clean, spans) = crate::ui::gloss_block::strip_emphasis_spans(&body);
            *self.emphasis_ranges.borrow_mut() = spans;
            clean
        };
        drop(paras);
        drop(pages);

        // Notes (kind == "note") render the page's PLANNED Markdown blocks
        // directly — the same blocks pagination measured — so the rendered
        // block list is index-aligned 1:1 with `all_paragraphs[page range]`
        // (the invariant the cursor projection relies on). Q&A answers keep
        // plain set_text so that <hi> highlight ranges (char offsets into
        // `body`) and per-page block offsets (derived from body.split('\n')
        // below) stay byte-aligned with the buffer content.
        let note_md_blocks = if is_note {
            self.hi_ranges.borrow_mut().clear();
            // Re-anchor list/quote tag margins to the view's CURRENT left
            // margin (a tag left-margin replaces the view's — an absolute
            // value would render lists outside the centered column).
            self.md_tags.set_base_left_margin(self.view.left_margin());
            let buffer = self.view.buffer();
            buffer.set_text("");
            let planned = self.note_blocks.borrow();
            Some(crate::ui::markdown::render_markdown_blocks(
                &buffer,
                &planned[page_start..page_end.min(planned.len())],
                &self.md_tags,
            ))
        } else {
            self.view.buffer().set_text(&body);
            None
        };
        self.apply_font();
        // Paint the `<hi>` highlight AFTER set_text + font (read-mode only; the
        // editor sets raw text and must not re-apply these read-mode ranges).
        // Notes have no <hi> spans so apply_hi_color is a no-op for them.
        self.apply_hi_color();
        // Same contract as apply_hi_color: char ranges into the just-set body.
        self.apply_emphasis();
        // Style the prepended passage source (page 0 only): small-caps speaker,
        // hang-indented verse, dim right-aligned citation.
        self.apply_source_style();
        // Re-paint the rewrite diff-highlight for THIS page (clipped to the
        // page's char span), so a change on page 2+ survives page turns.
        self.reapply_rewrite_diff();
        // Re-paint the overlay-search highlights for THIS page (whole-body match
        // spans clipped to the page's char span), so `/` matches on page 2+
        // survive page turns within the entry (mirrors reapply_rewrite_diff).
        self.reapply_search();
        // The leading `Q:` line renders as PLAIN body text — no header tag. It
        // used to get a bold/0.9-scale/dim header treatment, but the tag only
        // landed on the first render (page turns skipped it), and the user
        // prefers the plain look: same weight and size as the answer.
        // Floating page marker (⌄ more / • end), bottom-center of the viewport.
        self.update_page_marker(pidx, n_pages);
        // The vadjustment stays at top — the page fits, nothing scrolls.
        let adj = self.scrolled.vadjustment();
        adj.set_value(adj.lower());

        // Re-derive per-page blocks for the bar / visual mode.
        // - Notes: the rendered blocks, ALL of them (chrome included) so their
        //   indices stay 1:1 with the planned/page indices — the cursor itself
        //   never lands on chrome because `step_full_cursor` skips
        //   non-stoppable indices.
        // - Q&A: split `body` (byte-aligned with the plain set_text buffer).
        *self.blocks.borrow_mut() = if let Some(md_blocks) = note_md_blocks {
            md_blocks
                .into_iter()
                .map(|b| JournalBlock {
                    start_line: b.start_line,
                    end_line: b.end_line,
                    text: String::new(),
                })
                .collect()
        } else {
            let lines: Vec<&str> = body.split('\n').collect();
            journal_blocks(&lines)
        };
        self.visual_anchor.set(None);
        // Project the full cursor onto this page. The cursor can legitimately
        // sit on ANOTHER page — a chrome-only opening note page at small sizes,
        // or a Q&A page 0 holding only the quoted source. Clamping it here
        // painted the bar on a heading (or on the quote); clear the bar instead
        // and let the first j/k turn to the cursor's own page.
        let cursor_on_page = {
            let c = self.cursor_full.get();
            c >= page_start && c < page_end
        };
        if !cursor_on_page {
            self.cursor_block.set(0);
            self.clear_bar();
        } else {
            let page_local = self
                .cursor_full
                .get()
                .saturating_sub(page_start)
                .min(self.blocks.borrow().len().saturating_sub(1));
            self.cursor_block.set(page_local);
            self.mark_cursor_block();
        }
        self.update_bottom_clip();
    }

    /// The FULL-paragraph index of the current page's first block — the offset to
    /// map a page-local block index to its `all_paragraphs`/journal_audio index.
    pub fn current_page_start(&self) -> usize {
        let pages = self.pages.borrow();
        pages
            .get(self.page_idx.get().min(pages.len().saturating_sub(1)))
            .map(|p| p.start)
            .unwrap_or(0)
    }

    /// Color every block ON THE CURRENT PAGE whose audio is cached with `accent`
    /// (the same cached-block accent the gloss/synopsis overlays use). `is_cached`
    /// is called with each block's FULL paragraph index (page_start + local), so
    /// the caller can look it up in `journal_audio` by entry id + paragraph index.
    /// Mirrors the gloss overlay's `color_audio_blocks`.
    pub fn color_cached_blocks(&self, accent: &str, is_cached: impl Fn(usize) -> bool) {
        let buffer = self.view.buffer();
        let page_start = self.current_page_start();
        let spans: Vec<(i32, i32)> = self
            .blocks
            .borrow()
            .iter()
            .enumerate()
            .filter(|(local, _)| is_cached(page_start + local))
            .map(|(_, blk)| (blk.start_line, blk.end_line))
            .collect();
        crate::ui::apply_cached_coloring(&buffer, "journal-audio-cached", accent, &spans);
    }

    /// The text of the cursor's current block (for TTS). None when no blocks.
    pub fn current_block_text(&self) -> Option<String> {
        let blocks = self.blocks.borrow();
        let len = blocks.len();
        if len == 0 {
            return None;
        }
        let i = self.cursor_block.get().min(len - 1);
        blocks.get(i).map(|b| b.text.clone())
    }

    /// The cursor block as a `-`-family target (buffer + underline tag + the
    /// block's buffer-line span). `None` when the overlay has no blocks.
    pub fn word_copy_target(
        &self,
    ) -> Option<crate::input::actions::overlay_word_copy::BlockTarget> {
        let blocks = self.blocks.borrow();
        let len = blocks.len();
        if len == 0 {
            return None;
        }
        let i = self.cursor_block.get().min(len - 1);
        let b = blocks.get(i)?;
        Some(crate::input::actions::overlay_word_copy::BlockTarget {
            buffer: self.view.buffer(),
            tag: self.word_underline_tag.clone(),
            start_line: b.start_line,
            end_line: b.end_line,
            block_index: i,
        })
    }

    /// Drop any `-`-family underline (Escape / overlay close).
    pub fn clear_word_underline(&self) {
        let buffer = self.view.buffer();
        let (s, e) = (buffer.start_iter(), buffer.end_iter());
        buffer.remove_tag(&self.word_underline_tag, &s, &e);
    }

    /// Every paragraph of the displayed entry (source + question + answer),
    /// index-aligned with the FULL block indices the TTS cache keys on
    /// (`journal_audio.paragraph_index`). For the Shift+Space batch synth.
    pub fn all_paragraph_texts(&self) -> Vec<String> {
        self.all_paragraphs.borrow().clone()
    }

    /// How many leading `all_paragraphs` entries are the prepended passage
    /// source (speaker/verse/citation). The batch synth skips these — the
    /// source is the work's own text, not Q&A prose.
    pub fn source_paragraph_count(&self) -> usize {
        self.source_para_count.get()
    }

    /// The block cursor's WHOLE-ENTRY index (`cursor_full`) — the index the
    /// TTS cache keys on (`current_block_index` is the page-local projection
    /// and must never key the cache: it repeats across page turns). None when
    /// the page has no blocks.
    pub fn current_full_block_index(&self) -> Option<usize> {
        if self.blocks.borrow().is_empty() {
            None
        } else {
            let last = self.all_paragraphs.borrow().len().saturating_sub(1);
            Some(self.cursor_full.get().min(last))
        }
    }

    /// `j`/`q`: move the cursor down one paragraph across the WHOLE Q&A, turning
    /// the page when it crosses the current page's range. `k`/`,`: up. No-op at
    /// the first/last paragraph of the Q&A.
    pub fn cursor_next_block(&self) {
        self.step_full_cursor(1);
    }
    pub fn cursor_prev_block(&self) {
        self.step_full_cursor(-1);
    }

    /// True when the current render has navigable paragraph blocks (a Q&A
    /// page). False for the block-less renders (loading / message / pending
    /// passage source), where j/k fall back to `scroll_view`.
    pub fn has_nav_blocks(&self) -> bool {
        !self.all_paragraphs.borrow().is_empty()
    }

    /// Raw viewport scroll for BLOCK-LESS renders — the pending-passage source
    /// card renders the whole selection unpaginated, so without this a
    /// selection taller than the card was keyboard-unreachable (j/k no-op with
    /// no blocks). Steps ~3 line-heights; the BottomClipGuard's persistent
    /// value_changed recompute masks the partial row at the viewport bottom.
    /// Mirrors the gloss overlay's loading-card scroll fallback.
    pub fn scroll_view(&self, delta: i32) {
        let adj = self.scrolled.vadjustment();
        let ctx = self.view.pango_context();
        let metrics = ctx.metrics(None, None);
        let line = ((metrics.ascent() + metrics.descent()) as f64
            / gtk4::pango::SCALE as f64)
            .max(12.0);
        let target = (adj.value() + line * 3.0 * delta as f64)
            .clamp(adj.lower(), (adj.upper() - adj.page_size()).max(adj.lower()));
        adj.set_value(target);
    }
    /// `gg`/`G`: jump the cursor to the first/last paragraph of the whole Q&A
    /// (turning to its page).
    pub fn cursor_first_block(&self) {
        self.full_cursor_to_end(false);
    }
    pub fn cursor_last_block(&self) {
        self.full_cursor_to_end(true);
    }

    /// Step the full-list cursor by `delta`, SKIPPING non-stoppable chrome
    /// blocks (note headings / rules), turning the page if the new cursor
    /// leaves the current page; otherwise just re-mark the bar. One keypress =
    /// one visible bar move — never a phantom press swallowed by a heading.
    /// Whether the whole-entry paragraph `i` is a valid cursor stop.
    ///
    /// Two kinds of non-stoppable paragraph, both rendered but never landed on:
    ///
    /// - the prepended passage SOURCE (speaker, quoted verse/prose, citation) —
    ///   the first `source_para_count` entries. It is the work's own text quoted
    ///   above the question, not Q&A prose, so it takes no accent bar and no
    ///   cursor stop (it is likewise skipped by the batch TTS synth).
    /// - note chrome (headings, rules), flagged `stoppable == false` on the
    ///   planned Markdown blocks. Q&A pages have no `note_blocks`, hence the
    ///   `unwrap_or(true)` default.
    fn is_stoppable(&self, i: usize) -> bool {
        if i < self.source_para_count.get() {
            return false;
        }
        self.note_blocks
            .borrow()
            .get(i)
            .map(|b| b.stoppable)
            .unwrap_or(true)
    }

    fn step_full_cursor(&self, delta: i32) {
        let total = self.all_paragraphs.borrow().len();
        if total == 0 {
            return;
        }
        let cur = self.cursor_full.get().min(total - 1);
        let next = step_skipping_chrome(cur, delta, total, |i| self.is_stoppable(i));
        let Some(next) = next else { return };
        self.cursor_full.set(next);
        self.sync_cursor_page();
    }

    /// Move the block cursor (the accent bar) to the paragraph that CONTAINS the
    /// buffer char offset `off`, turning to its page. Used by overlay search so
    /// n/N move the accent bar to the block holding the current match, not just
    /// the highlight. No-op for note pages (their block model is Markdown-planned,
    /// not paragraph-line based) and when there are no nav blocks.
    pub fn cursor_to_char_offset(&self, off: i32) {
        if self.page_is_note.get() || self.all_paragraphs.borrow().is_empty() {
            return;
        }
        // Rebuild paragraph line-spans from the live buffer (same basis as
        // all_paragraphs = paragraph_texts(full)); map `off` (a CHAR offset)
        // to the paragraph whose [start_line, end_line] char span contains it.
        let buffer = self.view.buffer();
        let (start, end) = buffer.bounds();
        let full = buffer.text(&start, &end, false).to_string();
        let lines: Vec<&str> = full.split('\n').collect();
        // Char offset of each line's first char (line i starts after i newlines
        // + all prior line chars).
        let mut line_start = Vec::with_capacity(lines.len() + 1);
        let mut acc: i32 = 0;
        for l in &lines {
            line_start.push(acc);
            acc += l.chars().count() as i32 + 1; // +1 for the '\n'
        }
        let blocks = crate::ui::journal_block::journal_blocks(&lines);
        // Blocks are index-aligned with all_paragraphs (both from the same
        // journal_blocks split). Find the block whose char span holds `off`.
        let target = blocks.iter().position(|b| {
            let s = *line_start.get(b.start_line as usize).unwrap_or(&0);
            let e = line_start
                .get(b.end_line as usize)
                .map(|ls| ls + lines.get(b.end_line as usize).map(|l| l.chars().count() as i32).unwrap_or(0))
                .unwrap_or(i32::MAX);
            off >= s && off <= e
        });
        if let Some(idx) = target {
            let total = self.all_paragraphs.borrow().len();
            // A match inside the quoted source must NOT drag the accent bar onto
            // it — those paragraphs are non-stoppable (`is_stoppable`). The
            // search HIGHLIGHT still paints there (reapply_search is independent
            // of the bar); only the block cursor is held back.
            if idx < total && self.is_stoppable(idx) {
                self.cursor_full.set(idx);
                self.sync_cursor_page();
            }
        }
    }

    fn full_cursor_to_end(&self, last: bool) {
        let total = self.all_paragraphs.borrow().len();
        if total == 0 {
            return;
        }
        let target = if last {
            (0..total).rev().find(|&i| self.is_stoppable(i)).unwrap_or(total - 1)
        } else {
            (0..total).find(|&i| self.is_stoppable(i)).unwrap_or(0)
        };
        self.cursor_full.set(target);
        self.sync_cursor_page();
    }

    /// `x`/`y`: turn to the next/prev RENDER page of the current Q&A (the same
    /// entry), landing the cursor on the first stoppable block of that page —
    /// unlike `j`/`k` which step one block at a time. No-op at the first/last
    /// page (or when the entry has no blocks / is a single page).
    pub fn page_turn(&self, delta: i32) {
        let n_pages = self.pages.borrow().len();
        if n_pages < 2 {
            return;
        }
        let cur_page = self.page_idx.get().min(n_pages - 1);
        let target_page = cur_page as i64 + delta as i64;
        if target_page < 0 || target_page >= n_pages as i64 {
            return;
        }
        let (page_start, page_end) = {
            let pages = self.pages.borrow();
            let p = &pages[target_page as usize];
            (p.start, p.end)
        };
        // Land on the first STOPPABLE block of the target page (skip note
        // chrome and the quoted passage source), falling back to the page start.
        let target = (page_start..page_end)
            .find(|&i| self.is_stoppable(i))
            .unwrap_or(page_start);
        self.cursor_full.set(target);
        self.sync_cursor_page();
    }

    /// After `cursor_full` moves: if it now falls on a different page, turn the
    /// page (re-render, which re-projects + marks); otherwise just re-mark the bar
    /// at the new page-local block — no re-render.
    fn sync_cursor_page(&self) {
        let target_page = crate::ui::pagination::page_containing_block(
            &self.pages.borrow(),
            self.cursor_full.get(),
        );
        if target_page != self.page_idx.get() {
            self.page_idx.set(target_page);
            self.render_page();
            // The render page changed — refresh the footer's "page X / Y".
            self.update_footer_position();
        } else {
            let page_start = self
                .pages
                .borrow()
                .get(target_page)
                .map(|p| p.start)
                .unwrap_or(0);
            let page_local = self
                .cursor_full
                .get()
                .saturating_sub(page_start)
                .min(self.blocks.borrow().len().saturating_sub(1));
            self.cursor_block.set(page_local);
            self.mark_cursor_block();
        }
    }

    /// Move the left accent bar to the single cursor block and repaint. No-op
    /// when there are no blocks. Logs the landing block so j/k/gg/G navigation
    /// stays verifiable from the dev log (mirrors the gloss overlay).
    fn mark_cursor_block(&self) {
        let blocks = self.blocks.borrow();
        if blocks.is_empty() {
            drop(blocks);
            self.clear_bar();
            return;
        }
        let i = self.cursor_block.get().min(blocks.len() - 1);
        let span = (blocks[i].start_line, blocks[i].end_line);
        drop(blocks);
        // `cursor#` is the PAGE-LOCAL index (repeats across page turns);
        // `full#` is the whole-entry block index — the one tests must compare
        // to detect a swallowed/phantom keypress.
        crate::logging::log(&format!(
            "JOURNAL-CURSOR: cursor#{} full#{} bar lines [{}, {}]",
            i,
            self.cursor_full.get(),
            span.0,
            span.1
        ));
        *self.bar_ranges.borrow_mut() = vec![span];
        self.bar_drawing.queue_draw();
    }

    /// Clear the selection bar (no ranges) and repaint.
    fn clear_bar(&self) {
        self.bar_ranges.borrow_mut().clear();
        self.bar_drawing.queue_draw();
    }

    /// Redraw the bar over the current selection span (anchor..=cursor). No-op
    /// (clears) when no anchor is set or there are no blocks.
    fn refresh_bar(&self) {
        let blocks = self.blocks.borrow();
        let anchor = match self.visual_anchor.get() {
            Some(a) if !blocks.is_empty() => a.min(blocks.len() - 1),
            _ => {
                drop(blocks);
                self.clear_bar();
                return;
            }
        };
        let cursor = self.cursor_block.get().min(blocks.len() - 1);
        let (s, e) = visual_block_range(anchor, cursor);
        let span = (blocks[s].start_line, blocks[e].end_line);
        drop(blocks);
        *self.bar_ranges.borrow_mut() = vec![span];
        self.bar_drawing.queue_draw();
    }

    /// Enter visual mode: anchor at the CURRENT cursor block (the segment the
    /// reader is on), so `V` selects that segment — extend from there with j/k.
    /// Returns false (no-op) when there are no blocks.
    pub fn enter_visual(&self) -> bool {
        let n = self.blocks.borrow().len();
        if n == 0 {
            crate::logging::log("JOURNAL-VISUAL: enter_visual no-op (0 blocks)");
            return false;
        }
        let seed = self.cursor_block.get().min(n - 1);
        self.visual_anchor.set(Some(seed));
        self.cursor_block.set(seed);
        self.refresh_bar();
        crate::logging::log(&format!(
            "JOURNAL-VISUAL: entered, {} blocks, anchor {}",
            n, seed
        ));
        true
    }

    /// Move the cursor end of the selection by `delta` blocks (clamped), redraw
    /// the bar, and scroll the cursor block into view.
    pub fn visual_step(&self, delta: i32) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        let cur = self.cursor_block.get().min(len - 1) as i64;
        let next = (cur + delta as i64).clamp(0, len as i64 - 1) as usize;
        self.cursor_block.set(next);
        self.refresh_bar();
        // No scroll: visual selection stays within the rendered page, which
        // already fits (pagination). Spanning pages is out of scope.
    }

    /// Move the cursor end to the first (`false`) or last (`true`) block.
    pub fn visual_to_end(&self, last: bool) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        self.cursor_block.set(if last { len - 1 } else { 0 });
        self.refresh_bar();
    }

    /// The selected paragraphs' text (anchor..=cursor), blank-line joined.
    pub fn visual_selection_text(&self) -> String {
        let blocks = self.blocks.borrow();
        let anchor = match self.visual_anchor.get() {
            Some(a) if !blocks.is_empty() => a.min(blocks.len() - 1),
            _ => return String::new(),
        };
        let cursor = self.cursor_block.get().min(blocks.len() - 1);
        let (s, e) = visual_block_range(anchor, cursor);
        blocks[s..=e]
            .iter()
            .map(|b| b.text.clone())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Number of blocks currently selected.
    pub fn visual_selection_len(&self) -> usize {
        visual_selection_count(self.visual_anchor.get(), self.cursor_block.get())
    }

    /// Exit visual mode: clear the anchor and return the bar to the single block
    /// cursor (the journal now has a persistent normal-mode block cursor that
    /// j/k drive and Space/a read).
    pub fn exit_visual(&self) {
        self.visual_anchor.set(None);
        self.mark_cursor_block();
    }

    /// Exit visual mode returning the cursor to the anchor block, then re-mark
    /// the single cursor bar.
    pub fn exit_visual_to_anchor(&self) {
        if let Some(anchor) = self.visual_anchor.get() {
            self.cursor_block.set(anchor);
        }
        self.visual_anchor.set(None);
        self.mark_cursor_block();
    }


    /// Normal-navigation footer hint (advertises Shift+V). Re-set on visual exit.
    pub fn set_journal_hint(&self) {
        self.hint.set_text(
            "",
        );
    }

    /// Footer hint shown while journal visual mode is active.
    pub fn set_journal_visual_hint(&self) {
        self.hint
            .set_text("\u{21e7}V/Esc exit \u{00b7} j/k extend \u{00b7} gg/G ends \u{00b7} y yank");
    }
}

#[cfg(test)]
mod journal_font_swap_tests {
    use super::{journal_family_for_reader, JOURNAL_FONT_ALT_FAMILY, JOURNAL_FONT_FAMILY};

    #[test]
    fn the_pair_swaps_in_both_directions() {
        // The rule: reader on Charis -> journal Charter; reader on Charter ->
        // journal Charis. Neither side ever matches the card behind it.
        assert_eq!(journal_family_for_reader("Charis"), "Charter");
        assert_eq!(journal_family_for_reader("Charter"), "Charis");
    }

    #[test]
    fn any_other_reader_family_gets_the_default() {
        // Everything outside the pair — the rest of FONT_CYCLE, or a
        // hand-edited config value — falls back to Charis. Safe because this
        // arm is only reached when the reader is on neither of the pair.
        assert_eq!(journal_family_for_reader("Gentium Book"), JOURNAL_FONT_ALT_FAMILY);
        assert_eq!(journal_family_for_reader("Junicode SemiExp"), JOURNAL_FONT_ALT_FAMILY);
        assert_eq!(journal_family_for_reader("Some Unknown Face"), JOURNAL_FONT_ALT_FAMILY);
    }

    #[test]
    fn the_journal_never_matches_the_reader() {
        // The invariant the whole mechanism exists for, asserted directly over
        // every family the reader can actually be on.
        for reader in crate::config::FONT_CYCLE {
            assert_ne!(
                journal_family_for_reader(reader).to_lowercase(),
                reader.to_lowercase(),
                "journal would match the reader on {reader}",
            );
        }
    }

    #[test]
    fn reader_family_is_matched_loosely() {
        // config.font_family is hand-editable, so tolerate case and padding.
        assert_eq!(journal_family_for_reader("  charis "), JOURNAL_FONT_FAMILY);
        assert_eq!(journal_family_for_reader("CHARIS"), JOURNAL_FONT_FAMILY);
    }

    #[test]
    fn the_two_families_actually_differ() {
        // A copy-paste slip here would silently defeat the whole mechanism.
        assert_ne!(JOURNAL_FONT_FAMILY, JOURNAL_FONT_ALT_FAMILY);
    }
}

#[cfg(test)]
mod prefix_question_tests {
    use super::prefix_question;

    #[test]
    fn adds_q_prefix() {
        assert_eq!(prefix_question("What customs governed correspondence?"),
            "Q: What customs governed correspondence?");
    }

    #[test]
    fn is_idempotent() {
        // A question already prefixed (e.g. re-rendered from a stored page) is
        // not double-prefixed.
        assert_eq!(prefix_question("Q: already asked"), "Q: already asked");
        assert_eq!(prefix_question("  Q: leading space"), "  Q: leading space");
    }
}

#[cfg(test)]
mod source_line_roles_tests {
    use super::source_line_roles;

    fn p(s: &str) -> Vec<String> {
        s.split('|').map(|x| x.to_string()).collect()
    }

    #[test]
    fn maps_paragraphs_to_buffer_lines() {
        // 3 source paras: speaker, verse(3-line block), citation.
        // Rendered joined by "\n\n":
        //   0 speaker
        //   1 blank
        //   2 v1   3 v2   4 v3   (the one verse paragraph, 3 lines)
        //   5 blank
        //   6 citation
        let paras = p("SPEAKER|v1\nv2\nv3|— cite");
        let roles = source_line_roles(&paras, true, true);
        assert_eq!(roles.speaker_line, Some(0));
        assert_eq!(roles.verse_lines, vec![2, 3, 4]);
        assert_eq!(roles.citation_line, Some(6));
        // The two blanks INSIDE the quotation get collapsed: line 1 (under the
        // speaker) and line 5 (above the citation). The blank BELOW the whole
        // source block separates it from the question and is not listed.
        assert_eq!(roles.inner_gap_lines, vec![1, 5]);
    }

    #[test]
    fn no_speaker_no_citation() {
        // 1 source para: verse(1-line block).
        let paras = p("v1");
        let roles = source_line_roles(&paras, false, false);
        assert_eq!(roles.speaker_line, None);
        assert_eq!(roles.verse_lines, vec![0]);
        assert_eq!(roles.citation_line, None);
        // Nothing to collapse: no speaker label above, no citation below.
        assert!(roles.inner_gap_lines.is_empty());
    }

    #[test]
    fn multi_line_verse_no_speaker_with_citation() {
        // 2 source paras: verse(2-line block), citation.
        //   0 v1  1 v2  2 blank  3 citation
        let paras = p("v1\nv2|— cite");
        let roles = source_line_roles(&paras, false, true);
        assert_eq!(roles.speaker_line, None);
        assert_eq!(roles.verse_lines, vec![0, 1]);
        assert_eq!(roles.citation_line, Some(3));
        // Only the citation's own gap (line 2) — there is no speaker label, so
        // the quote starts at line 0 with no blank above it to collapse.
        assert_eq!(roles.inner_gap_lines, vec![2]);
    }
}

#[cfg(test)]
mod step_skipping_chrome_tests {
    use super::step_skipping_chrome;

    // Block layout mirroring an imported note:
    // 0 H1(chrome) 1 body 2 rule(chrome) 3 H2(chrome) 4 body 5 body
    const STOP: [bool; 6] = [false, true, false, false, true, true];

    fn is_stop(i: usize) -> bool {
        STOP[i]
    }

    #[test]
    fn j_skips_chrome_in_one_press() {
        // From the first paragraph (1), one j lands on the next paragraph (4),
        // hopping the rule + section heading — the "4 presses of j" bug.
        assert_eq!(step_skipping_chrome(1, 1, STOP.len(), is_stop), Some(4));
        assert_eq!(step_skipping_chrome(4, -1, STOP.len(), is_stop), Some(1));
    }

    #[test]
    fn edge_press_is_a_noop() {
        // No stoppable block past the last paragraph → None (caller no-ops).
        assert_eq!(step_skipping_chrome(5, 1, STOP.len(), is_stop), None);
        // Backwards from the first paragraph: only the H1 above → None.
        assert_eq!(step_skipping_chrome(1, -1, STOP.len(), is_stop), None);
    }

    #[test]
    fn plain_qa_all_stoppable_steps_one() {
        // A Q&A with no quoted source and no chrome — plain ±1 stepping.
        assert_eq!(step_skipping_chrome(2, 1, 5, |_| true), Some(3));
        assert_eq!(step_skipping_chrome(2, -1, 5, |_| true), Some(1));
    }

    // A PASSAGE Q&A: paragraphs 0..3 are the prepended source (speaker, quoted
    // verse, citation), then the question (3) and the answer paragraphs. The
    // source is non-stoppable, exactly like note chrome — `is_stoppable` returns
    // false below `source_para_count`.
    const SOURCE_COUNT: usize = 3;
    const PASSAGE_TOTAL: usize = 6;
    fn passage_is_stop(i: usize) -> bool {
        i >= SOURCE_COUNT
    }

    #[test]
    fn k_never_enters_the_quoted_source() {
        // From the question (3), k has nothing stoppable above → no-op, so the
        // accent bar can never land on the quote.
        assert_eq!(step_skipping_chrome(3, -1, PASSAGE_TOTAL, passage_is_stop), None);
    }

    #[test]
    fn j_steps_through_the_answer_normally() {
        assert_eq!(step_skipping_chrome(3, 1, PASSAGE_TOTAL, passage_is_stop), Some(4));
        assert_eq!(step_skipping_chrome(5, -1, PASSAGE_TOTAL, passage_is_stop), Some(4));
    }

    #[test]
    fn gg_lands_on_the_question_not_the_source() {
        // `full_cursor_to_end(false)` scans for the FIRST stoppable index.
        let first = (0..PASSAGE_TOTAL).find(|&i| passage_is_stop(i));
        assert_eq!(first, Some(SOURCE_COUNT));
    }
}

#[cfg(test)]
mod scroll_budget_tests {
    use super::UNACCOUNTED_CHROME_MARGINS;

    /// `size_card` passes `title_h + UNACCOUNTED_CHROME_MARGINS` as the fixed
    /// chrome, so the host's closed scroll height is
    /// `card_height − title_h − margins − footer_h`. This is the formula that the
    /// too-tall bug got wrong (it omitted `margins`). Mirror the production
    /// arithmetic here so a change to either is caught.
    fn closed_scroll_budget(card_height: i32, title_h: i32, footer_h: i32) -> i32 {
        (card_height - (title_h + UNACCOUNTED_CHROME_MARGINS) - footer_h).max(80)
    }

    #[test]
    fn reserves_unaccounted_chrome_margins() {
        // window 1200 → card_height 1152; title 40, footer 30.
        let (card_h, title_h, footer_h) = (1152, 40, 30);
        let budget = closed_scroll_budget(card_h, title_h, footer_h);
        // Exactly the old (buggy) budget minus the reserved margins.
        let old_buggy = card_h - title_h - footer_h;
        assert_eq!(old_buggy - budget, UNACCOUNTED_CHROME_MARGINS);
    }

    #[test]
    fn floors_at_80_for_tiny_cards() {
        assert_eq!(closed_scroll_budget(50, 40, 30), 80);
    }

    #[test]
    fn margins_match_the_scroll_overlay_sites() {
        // Mirrors the gloss overlay's SCROLL_OVERLAY_MARGINS: 24 + 20 = 44 (the
        // scroll_overlay's top+bottom margins only — not the title/footer margins).
        assert_eq!(UNACCOUNTED_CHROME_MARGINS, 44);
    }
}

#[cfg(test)]
mod scroll_structure_tests {
    use super::*;

    /// The ScrolledWindow's child MUST be the TextView directly. If it is an
    /// Overlay (or anything else), GTK can't use the TextView's native scroll
    /// adjustments, so the vadjustment has no scroll range and j/k/G/gg do
    /// nothing (and overflowing content stays clipped). The gloss overlay nests
    /// it correctly; this guards the journal overlay against re-introducing the
    /// ScrolledWindow→Overlay→TextView inversion.
    ///
    /// #[ignore]: needs gtk4::init(), which panics if a second GTK-init test runs
    /// in the same process. Run serially:
    /// `cargo test --bins -- --ignored scrolled_window_child`.
    #[test]
    #[ignore]
    fn scrolled_window_child_is_the_text_view() {
        if gtk4::init().is_err() {
            eprintln!("skip: no GTK display");
            return;
        }
        let overlay = JournalOverlay::new(1050, 80);
        let child = overlay
            .scrolled
            .child()
            .expect("ScrolledWindow should have a child");
        assert!(
            child.downcast_ref::<gtk4::TextView>().is_some(),
            "ScrolledWindow child must be the TextView directly (for native scroll \
             adjustments), not a {:?}",
            child.type_(),
        );
    }
}
