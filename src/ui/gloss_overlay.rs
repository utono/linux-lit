use crate::ui::ask_card::{AskCard, AskCardHost};
use crate::ui::gloss_block::{
    gloss_block_markups, gloss_blocks, render_synopsis_with_labels, selected_blocks_text,
    synopsis_blocks, visual_block_range, visual_selection_count, BlockKind, GlossBlock,
};
use crate::ui::gloss_render::{
    populate_gloss_buffer, populate_verse_buffer, BarRange, LineNumber,
};
use crate::ui::gloss_util::{
    build_diff_markup, cursor_scroll_target, format_citation_range, parse_hex_color,
    snap_up_to_row, CursorScrollGeom,
};
use gtk4::pango;
use gtk4::prelude::*;
use gtk4::{self, Align, Label, Overlay};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Buffer-line span of one cursor-stop block (source or explication).
struct BlockRange {
    kind: BlockKind,
    index: i32,
    start_line: i32,
    end_line: i32,
}

/// Which block-bearing render the pagination state currently holds, so
/// `render_current_page` dispatches to the right page renderer. Synopsis pages
/// render via `render_synopsis_with_labels`/`set_text`; gloss-result pages render
/// via `populate_gloss_buffer` over the page's `<speaker>`/`<verse>`/`<gloss>`
/// markup slice (the speaker tags `gloss_blocks` drops cannot be reconstructed
/// from `GlossBlock.display`). Irrelevant when `paginated` is false.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PaginatedMode {
    Synopsis,
    Gloss,
}

pub struct GlossOverlay {
    pub overlay: Overlay,
    scrim: gtk4::Box,
    container: gtk4::Box,
    title: Label,
    orig_header: Label,
    original_label: Label,
    corr_header: Label,
    corrected_label: Label,
    hint: Label,
    /// The footer/hint row container (holds `hint` + `citation_label` +
    /// `position_label`). Stored so the show paths can measure its height for the
    /// fixed-scroll-height accounting (it stays visible while the ask card is open
    /// — gloss has no toggled footer — so it counts as fixed chrome below the
    /// scroll).
    footer_box: gtk4::Box,
    citation_label: Label,
    position_label: Label,
    gloss_scroll_overlay: Overlay,
    gloss_scrolled: gtk4::ScrolledWindow,
    gloss_view: gtk4::TextView,
    bar_drawing: gtk4::DrawingArea,
    panel_drawing: gtk4::DrawingArea,
    /// Owns the clip Box pinned to the bottom of the gloss viewport and all three
    /// recompute paths (value_changed catch-all, reset_scroll_top range+idle,
    /// update_bottom_clip). Replaces the hand-wired `bottom_clip` + inline
    /// connect_value_changed + reset_scroll_top body.
    clip_guard: crate::ui::bottom_clip_guard::BottomClipGuard,
    bar_ranges: Rc<RefCell<Vec<BarRange>>>,
    bar_color: Rc<RefCell<(f64, f64, f64)>>,
    /// Page-marker glyph (`⌄`/`•`/None) + dim color, drawn on `bar_drawing` (no
    /// Label — no overlay-child allocation lag). See `draw_page_marker_glyph`.
    marker_glyph: Rc<RefCell<Option<&'static str>>>,
    marker_color: Rc<RefCell<(f64, f64, f64)>>,
    panel_color: Rc<RefCell<(f64, f64, f64)>>,
    /// When the vim editor's NORMAL/VISUAL cursor sits on a BLANK line, that
    /// line's `\n` has no glyph cell, so the char-background block tag paints
    /// nothing. We draw a thin left-edge block here instead. `Some((buffer_line,
    /// r, g, b))` while on a blank line; `None` otherwise (real glyph, INSERT
    /// mode, or not editing). Painted by `bar_drawing`'s draw func.
    vim_block_line: crate::ui::VimBlankCursor,
    bar_x: Rc<RefCell<i32>>,
    line_numbers: Rc<RefCell<Vec<LineNumber>>>,
    echo_lines: Rc<RefCell<Vec<i32>>>,
    echo_header_view: gtk4::TextView,
    echo_rule: gtk4::Separator,
    text_margins: i32,
    column_width: i32,
    /// The overlay's own font (independent of the main reader). `!`/`|` adjust
    /// the size while a gloss/synopsis is open without touching the main card.
    /// Applied as a font TextTag over the gloss buffer on every show, overriding
    /// the global `.gloss-text` CSS. Defaults to Charter 19pt.
    font_family: RefCell<String>,
    font_size: std::cell::Cell<i32>,
    /// Char ranges (start, end) of standalone label paragraphs in the current
    /// synopsis buffer (e.g. "Shakespearean parallels:"), bolded on show and
    /// re-asserted after every `apply_font` (which else overrides their weight
    /// with the regular-weight buffer-wide font tag). Empty for glosses/echoes.
    synopsis_label_ranges: RefCell<Vec<(usize, usize)>>,
    /// Last `(card_width, card_height)` a show_* call sized the card to. The
    /// loading state reuses it so "Glossing…" presents as a full card rather
    /// than a label-sized box. `(0,0)` until the first card is shown.
    last_card_size: Cell<(i32, i32)>,
    /// Cursor-stop blocks of the currently shown gloss, with their buffer
    /// line spans, in document order. Empty in echo/synopsis/glossing modes.
    blocks: Rc<RefCell<Vec<BlockRange>>>,
    /// Index into `blocks` of the selected cursor block. `j`/`k` step it, `gg`/
    /// `G` jump to first/last; the accent bar marks it and `Space` acts on it.
    /// Reset to 0 on each gloss render. The cursor is an explicit selection, NOT
    /// derived from scroll position (a tall card's viewport center can leave
    /// top/bottom blocks unreachable — see GLOSS-CURSOR debug logging).
    cursor_block: Cell<usize>,
    /// `Some(block_index)` while synopsis visual mode is active — the anchor end
    /// of the selection. The cursor end is `cursor_block`. `None` in normal
    /// synopsis navigation. Selected range: `visual_block_range(anchor, cursor)`.
    synopsis_visual_anchor: Cell<Option<usize>>,
    /// PAGINATION (synopsis + gloss-result modes, like the journal overlay): the
    /// FULL block list for the open gloss/synopsis (the pagination unit), the page
    /// ranges over it, the current page, and the cursor's GLOBAL index across all
    /// pages (`cursor_block` is its page-local projection). Empty/0 in echo +
    /// glossing-loading modes (those don't paginate). See
    /// docs/plans/2026-06-28-gloss-overlay-pagination-design.md.
    all_blocks: RefCell<Vec<GlossBlock>>,
    pages: RefCell<Vec<crate::ui::pagination::Page>>,
    page_idx: Cell<usize>,
    cursor_full: Cell<usize>,
    /// The cross-gloss INDEX position `(index, total)` over the work's glosses,
    /// recorded by `set_position`. No longer shown in the footer — the right
    /// label now shows only the bare render-page counter (`pages`/`page_idx` via
    /// `update_position_label`). Kept as the authoritative nav index for callers
    /// and any future footer use.
    gloss_pos: Cell<(usize, usize)>,
    /// True while the current render is paginated (synopsis or gloss-result), so
    /// the cursor-nav methods turn pages instead of scrolling. False in echo +
    /// glossing-loading modes (cursor-nav keeps the old scroll behavior).
    paginated: Cell<bool>,
    /// Which paginated render is active (set by `show_synopsis`/`show_gloss_with_color`),
    /// so `render_current_page` dispatches to the right page renderer. Only read
    /// while `paginated` is true.
    paginated_mode: Cell<PaginatedMode>,
    /// PER-BLOCK original markup for the gloss-result render, in the SAME order as
    /// `all_blocks` (`gloss_block_markups`). A page's markup is its blocks' markups
    /// joined, fed back through `populate_gloss_buffer` so speaker headings + verse
    /// indents survive (which `GlossBlock.display` loses). Empty in synopsis/echo
    /// modes. Set by `show_gloss_with_color`.
    gloss_block_markups: RefCell<Vec<String>>,
    /// The gloss string currently shown (raw, tagged), retained for the
    /// single-page full render (echoes/pron intact, exactly as before pagination)
    /// and so a re-show can rebuild blocks. Set by `show_gloss_with_color`.
    current_gloss: RefCell<String>,
    /// Source line-number annotations the open gloss was rendered with. Stored so
    /// each page render can re-pass them to `populate_gloss_buffer`. (Glosses then
    /// clear the produced numbers — verse numbers belong only to the main reading
    /// view — but the argument is preserved for fidelity.)
    gloss_source_line_numbers: RefCell<Vec<(String, i64)>>,
    /// The synopsis string currently shown (raw, `<p>`-tagged), retained so
    /// visual-mode yank can rebuild the selected paragraphs via
    /// `selected_blocks_text`. Set by `show_synopsis`.
    current_synopsis: RefCell<String>,
    /// Hosts the shared "ask" input card (stacked below the synopsis/gloss card)
    /// and owns the fixed-scroll-height viewport-shrink (the occlusion fix), the
    /// clip recompute, and the open/close lifecycle. Shared with the journal
    /// overlay so the mechanism can't drift. Serves both the synopsis "ask" flow
    /// and the gloss add/edit prompts. See `crate::ui::ask_card::AskCardHost`.
    ask_host: AskCardHost,
    /// In-place vim editor engine (None when not editing). The buffer is a single
    /// raw-text blob: the gloss markup OR the synopsis text, depending on which
    /// surface opened the editor.
    vim_engine: RefCell<Option<crate::input::vim::VimEngine>>,
    /// The raw text the editor was seeded with, for the `:q` dirty-check.
    vim_seed: RefCell<String>,
    /// True while the editor hosts the reader's copy-only segment view
    /// (InputMode::SegmentVim): the NORMAL footer then advertises y/`:q`
    /// instead of the save/rewrite verbs, which that mode refuses.
    vim_copy_only: std::cell::Cell<bool>,
    /// Block-cursor (fill, glyph-fg) colors, threaded from the theme on enter.
    vim_cursor_colors: RefCell<(String, String)>,
    /// `<hi>` highlight background (theme `cursor_line_bg`), re-asserted on the
    /// `gloss-hi` tag in `apply_font`. Defaults to `DEFAULT_HIGHLIGHT_BG` until
    /// the app threads the theme color via `set_highlight_color`.
    highlight_bg: RefCell<String>,
    /// Char ranges of `<hi>` highlights in the CURRENT synopsis buffer (the
    /// set_text path doesn't go through `populate_verse_buffer`, so the overlay
    /// re-applies the `gloss-hi` tag here, like `synopsis_label_ranges`). Empty
    /// for glosses (those tag during population).
    hi_ranges: RefCell<Vec<(usize, usize)>>,
    /// Reading font family stashed on edit-enter, restored on exit (mono swap).
    pre_edit_family: RefCell<Option<String>>,
    /// Overlay-search highlight tags, registered once on `gloss_view.buffer()`'s
    /// tag table in `new` (the buffer is never replaced — `set_text`/populate
    /// paths write into it in place — so registering once here is safe for the
    /// view's lifetime). Placeholder colors; `set_search_colors` wires them to
    /// the theme (Task 5).
    search_tag: gtk4::TextTag,
    search_current_tag: gtk4::TextTag,
}

/// Default font for the synopsis/gloss/echoes overlay cards.
/// Default overlay reading-font family. Shared (like `GLOSS_DEFAULT_FONT_SIZE`) so
/// the journal overlay applies the SAME font tag instead of falling back to the
/// `.gloss-text` CSS at the reader's config size. Pre-first-work default only:
/// `reapply_font` syncs the overlay to the reader card's configured family too,
/// on every work load / size change (`sync_reader_font`).
pub(crate) const GLOSS_DEFAULT_FONT_FAMILY: &str = "Charter";
/// Default overlay reading-font size (pt). Shared by the gloss + journal overlays
/// so they always render at the same size (never drift). Pre-first-work default
/// only: `reapply_font` syncs both overlays to the reader card's configured size
/// on every work load / size change (`sync_reader_font`).
pub(crate) const GLOSS_DEFAULT_FONT_SIZE: i32 = 17;

/// Card-matching layout for a PROSE synopsis: render it in the main reading
/// card's font and left padding instead of the play overlay's Charter-19 +
/// `card_width/4` inset. `None` (plays/verse) keeps the inset look. The accent
/// bar is preserved in both cases (it marks the TTS-synthesizable region).
pub struct SynopsisProseCard {
    pub font_family: String,
    pub font_size: i32,
    pub left_margin: i32,
    pub right_margin: i32,
}

impl GlossOverlay {
    pub fn new(column_width: u32, text_margins: u32) -> Self {
        let overlay = Overlay::new();

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.set_halign(Align::Center);
        container.set_valign(Align::Center);
        container.set_width_request(column_width as i32);
        container.add_css_class("gloss-overlay");

        let title = Label::new(Some("Gloss"));
        title.add_css_class("gloss-title");
        title.set_margin_start(text_margins as i32);
        title.set_margin_end(text_margins as i32);
        title.set_margin_top(24);
        container.append(&title);

        // Left margin for these diff/error labels is set per-display in `show()`
        // (card_width/4), so the error/diff card lines up with the loading and
        // result cards rather than hugging the container's left edge.
        let orig_header = Label::new(Some("ORIGINAL"));
        orig_header.add_css_class("gloss-header");
        orig_header.set_halign(Align::Start);
        container.append(&orig_header);

        let original_label = Label::new(None);
        original_label.set_wrap(true);
        original_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        original_label.set_halign(Align::Start);
        original_label.set_hexpand(false);
        original_label.set_selectable(false);
        original_label.set_max_width_chars(1);
        original_label.add_css_class("gloss-text");
        container.append(&original_label);

        let corr_header = Label::new(Some("GLOSS"));
        corr_header.add_css_class("gloss-header");
        corr_header.set_halign(Align::Start);
        container.append(&corr_header);

        let corrected_label = Label::new(None);
        corrected_label.set_wrap(true);
        corrected_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        corrected_label.set_halign(Align::Start);
        corrected_label.set_hexpand(false);
        corrected_label.set_selectable(false);
        corrected_label.set_max_width_chars(1);
        corrected_label.add_css_class("gloss-text");
        container.append(&corrected_label);

        let gloss_scroll_overlay = Overlay::new();

        let gloss_scrolled = gtk4::ScrolledWindow::new();
        // Fixed-scroll-height (AskCardHost precondition): vexpand is OFF. The
        // viewport height is set EXPLICITLY by `ask_host.size` (in each show path)
        // and adjusted on ask open/close. This is what makes the scroll yield room
        // to the ask card deterministically — a vexpand scroll keeps full height
        // and the ask card draws over the bottom rows (the occlusion bug). Mirrors
        // the journal overlay. Every show path that makes the scroll visible MUST
        // call `ask_host.size(...)` or the explicit height stays at its last value.
        gloss_scrolled.set_vexpand(false);
        gloss_scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
        gloss_scrolled.set_vscrollbar_policy(gtk4::PolicyType::External);
        // Report a small natural height (not the full text height) so the
        // fixed-height card honors its `height_request` instead of growing to
        // fit the whole synopsis.
        gloss_scrolled.set_propagate_natural_height(false);

        let gloss_view = gtk4::TextView::new();
        crate::ui::set_view_readonly(&gloss_view);
        gloss_view.set_wrap_mode(gtk4::WrapMode::Word);
        let right_margin = column_width as i32 / 8;
        gloss_view.set_left_margin(text_margins as i32);
        gloss_view.set_right_margin(right_margin);
        gloss_view.set_top_margin(24);
        gloss_view.set_bottom_margin(80);
        // Reading leading between wrapped lines; the paginated height
        // measurement charges the same via measure_text_height_leaded.
        gloss_view.set_pixels_inside_wrap(crate::ui::OVERLAY_LINE_LEADING);
        gloss_view.add_css_class("gloss-text");
        gloss_view.add_css_class("overlay-prose");

        let bar_drawing = gtk4::DrawingArea::new();
        bar_drawing.set_can_target(false);

        let bar_ranges: Rc<RefCell<Vec<BarRange>>> = Rc::new(RefCell::new(Vec::new()));
        let bar_color: Rc<RefCell<(f64, f64, f64)>> = Rc::new(RefCell::new((0.53, 0.62, 0.71)));
        // Page-marker glyph (⌄ more / • last page) drawn on the bar (no Label) +
        // its dim color — see draw_page_marker_glyph.
        let marker_glyph: Rc<RefCell<Option<&'static str>>> = Rc::new(RefCell::new(None));
        let marker_color: Rc<RefCell<(f64, f64, f64)>> = Rc::new(RefCell::new((0.5, 0.5, 0.5)));
        // Inset-panel DrawingArea + its color cell (shared helper, audit #52). The
        // draw_func is wired inside; the caller sets it as the Overlay main child
        // and adds panel_drawing.queue_draw() to its scroll-repaint closure below.
        // body_indent 0: the gloss view's left_margin IS the column edge (the
        // explication's +12 body indent is a per-tag margin, not the view's).
        let (panel_drawing, panel_color) = crate::ui::attach_overlay_panel(&gloss_view, 0);
        let vim_block_line: crate::ui::VimBlankCursor = Rc::new(RefCell::new(None));
        let bar_x: Rc<RefCell<i32>> = Rc::new(RefCell::new((column_width as i32) / 8));
        let line_numbers: Rc<RefCell<Vec<LineNumber>>> = Rc::new(RefCell::new(Vec::new()));
        let echo_lines: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
        let blocks: Rc<RefCell<Vec<BlockRange>>> = Rc::new(RefCell::new(Vec::new()));

        let ranges_clone = bar_ranges.clone();
        let color_clone = bar_color.clone();
        let bar_x_clone = bar_x.clone();
        let line_numbers_clone = line_numbers.clone();
        let view_clone = gloss_view.clone();
        let vim_block_clone = vim_block_line.clone();
        let marker_glyph_clone = marker_glyph.clone();
        let marker_color_clone = marker_color.clone();
        let block_left_margin = text_margins as i32;
        let right_margin_val = right_margin;
        bar_drawing.set_draw_func(move |_area, cr, w, _h| {
            let ranges = ranges_clone.borrow();
            let (r, g, b) = *color_clone.borrow();
            let x = *bar_x_clone.borrow() as f64;

            // Page marker (⌄/•) drawn just below the last line — no Label, so no
            // overlay-child allocation lag.
            crate::ui::draw_page_marker_glyph(
                cr,
                &view_clone,
                w,
                *marker_glyph_clone.borrow(),
                *marker_color_clone.borrow(),
                0.55,
                8,
            );

            // Vim block cursor on a BLANK line (shared draw; same coord path as
            // the accent bar / line numbers below).
            crate::ui::draw_vim_block_cursor(
                cr,
                &view_clone,
                *vim_block_clone.borrow(),
                block_left_margin as f64,
            );

            // Draw bars
            if !ranges.is_empty() {
                cr.set_source_rgb(r, g, b);
                cr.set_line_width(2.0);
                let spans: Vec<(i32, i32)> =
                    ranges.iter().map(|r| (r.start_line, r.end_line)).collect();
                crate::ui::draw_bar_spans(cr, &view_clone, &spans, x);
            }

            // Draw line numbers (every 5th)
            let nums = line_numbers_clone.borrow();
            if !nums.is_empty() {
                cr.set_source_rgb(r, g, b);
                let font_size = {
                    let pango_ctx = view_clone.pango_context();
                    let font_desc = pango_ctx.font_description().unwrap_or_default();
                    (font_desc.size() as f64 / pango::SCALE as f64) * 0.7
                };
                cr.select_font_face("serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
                cr.set_font_size(font_size);

                let buffer = view_clone.buffer();
                let num_x = (w - right_margin_val + 8) as f64;

                for ln in nums.iter() {
                    if ln.number % 5 != 0 {
                        continue;
                    }
                    if let Some(iter) = buffer.iter_at_line(ln.buffer_line) {
                        let loc = view_clone.iter_location(&iter);
                        let (_, by) = view_clone.buffer_to_window_coords(
                            gtk4::TextWindowType::Widget, 0, loc.y());
                        let text = ln.number.to_string();
                        let _ = cr.move_to(num_x, by as f64 + font_size);
                        let _ = cr.show_text(&text);
                    }
                }
            }
        });

        gloss_scrolled.set_child(Some(&gloss_view));

        // The bar overlay (accent bar, source/echo rule, line numbers) maps
        // buffer lines to window y at paint time, so it must repaint whenever
        // the view scrolls — otherwise the rule and bar stick at a stale
        // scroll offset while the text moves beneath them.
        {
            let bar_for_scroll = bar_drawing.clone();
            let panel_for_scroll = panel_drawing.clone();
            gloss_scrolled.vadjustment().connect_value_changed(move |_| {
                bar_for_scroll.queue_draw();
                panel_for_scroll.queue_draw();
            });
        }

        // Panel is the Overlay's MAIN CHILD so it paints BELOW everything: the
        // inset tint sits behind the (transparent) prose view, which sits below
        // the accent bar. A GTK Overlay paints its main child first, then each
        // overlay in add-order — so a panel added as an *overlay* would paint
        // ON TOP of the text (an opaque tint rect hiding the prose). The scroll
        // (with the transparent view) becomes an overlay ON TOP of the panel and
        // MUST be measured (`set_measure_overlay(.., true)`) — a bare DrawingArea
        // main child reports 0×0 natural size and would collapse the Overlay.
        gloss_scroll_overlay.set_child(Some(&panel_drawing));
        gloss_scroll_overlay.add_overlay(&gloss_scrolled);
        gloss_scroll_overlay.set_measure_overlay(&gloss_scrolled, true);
        gloss_scroll_overlay.add_overlay(&bar_drawing);
        gloss_scroll_overlay.set_measure_overlay(&bar_drawing, false);
        gloss_scroll_overlay.set_clip_overlay(&bar_drawing, true);

        // The page marker (⌄ more / • last page) is drawn ON `bar_drawing` via
        // `draw_page_marker_glyph` — NOT a Label (an Overlay child's allocation
        // lagged `set_margin_top`, so the glyph painted off a short last page). The
        // bar reads live geometry each paint and repaints on every render/scroll.

        // Attach the bottom-clip guard: builds the clip Box, adds it as a
        // non-measured, clipped overlay, and wires the persistent value_changed
        // catch-all (path c). All three recompute paths (c / on_open / recompute)
        // are now owned by the guard.
        let clip_guard = crate::ui::bottom_clip_guard::BottomClipGuard::attach(
            &gloss_scroll_overlay,
            &gloss_view,
            &gloss_scrolled,
        );

        gloss_scroll_overlay.set_visible(false);

        // Echoes-only: a fixed source-turn header + a fixed rule, above the
        // scrolling echo list. Hidden in all non-echo overlay modes.
        let echo_header_view = gtk4::TextView::new();
        crate::ui::set_view_readonly(&echo_header_view);
        echo_header_view.set_wrap_mode(gtk4::WrapMode::Word);
        echo_header_view.set_left_margin(text_margins as i32);
        echo_header_view.set_right_margin(right_margin);
        echo_header_view.set_top_margin(24);
        echo_header_view.add_css_class("gloss-text");
        echo_header_view.set_visible(false);
        container.append(&echo_header_view);

        let echo_rule = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        echo_rule.set_margin_start(text_margins as i32);
        echo_rule.set_margin_end(right_margin);
        // Breathing room between the source turn's last line and the rule.
        echo_rule.set_margin_top(16);
        echo_rule.set_visible(false);
        container.append(&echo_rule);

        // Breathing room between the scrolling text viewport and the footer
        // hint bar. Without it the viewport's bottom edge sits flush against the
        // footer's top border, so the last visible text line is bisected by the
        // rule and reads as clipped (the symptom this margin fixes). The 80px
        // bottom margin inside `gloss_view` only helps once scrolled fully to
        // the end; this gap keeps the last line clear at any scroll position.
        gloss_scroll_overlay.set_margin_bottom(20);
        // Symmetric gap below the title rule. The viewport top otherwise sits
        // flush under the title, so when the text is scrolled a partial line at
        // the top edge reads as clipped by the title. (The gloss_view internal
        // top margin scrolls away with the content, so it can't keep this gap.)
        gloss_scroll_overlay.set_margin_top(24);

        container.append(&gloss_scroll_overlay);

        let footer = crate::ui::footer::build_footer_row(
            text_margins as i32,
            "r journal · Ctrl+j/Ctrl+g view jrnl",
        );
        let footer_box = footer.container;
        let citation_label = footer.left;
        citation_label.set_visible(false);
        let hint = footer.hint;

        let position_label = Label::new(None);
        position_label.set_halign(Align::End);
        position_label.set_visible(false);
        footer_box.append(&position_label);

        container.append(&footer_box);

        // ---- Shared "ask" input card, stacked below the synopsis -------------
        // Lives inside `container` so the two cards form one centered column and
        // the synopsis scroll viewport (which vexpands) shrinks to make room when
        // this card is revealed. Hidden until `A` opens it. Built from the
        // canonical values inside `AskCard`; focus returns to `gloss_scrolled`.
        let ask = AskCard::new(text_margins as i32, &gloss_scrolled);
        container.append(ask.container());

        // The host owns the ask-card lifecycle: the fixed-scroll-height
        // viewport-shrink, the clip recompute (driving this overlay's
        // BottomClipGuard clip box), and open/close. The footer (hr + keybind
        // hints) is HIDDEN while the ask card is open, mirroring the journal Q&A —
        // so it is registered as the host's toggled footer.
        let recompute = {
            let clip = clip_guard.clip().clone();
            let view = gloss_view.clone();
            let scrolled = gloss_scrolled.clone();
            Rc::new(move || {
                crate::ui::recompute_overlay_bottom_clip(&view, &clip, &scrolled);
            }) as Rc<dyn Fn()>
        };
        let ask_host =
            AskCardHost::new(ask, &gloss_scrolled, Some(footer_box.clone()), recompute);
        // The gloss/synopsis ask card (add-question, edit gloss, fix-IPA, inner
        // monologue) fills 3/4 of the overlay height, matching the journal Q&A.
        ask_host.set_input_fill_fraction(0.75);

        container.set_visible(false);

        let scrim = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        scrim.set_hexpand(true);
        scrim.set_vexpand(true);
        scrim.add_css_class("gloss-scrim");
        scrim.set_visible(false);

        // Overlay-search highlight tags (Task 2 of the overlay-search feature).
        // Registered once here, mirroring the gloss-hi tag pattern, so later
        // search/step logic (Task 3) can apply them without re-registering.
        // Placeholder colors; Task 5 wires these to the theme via
        // `set_search_colors`.
        let search_tag = gtk4::TextTag::builder()
            .name("overlay_search")
            .background("#ffe000")
            .build();
        let search_current_tag = gtk4::TextTag::builder()
            .name("overlay_search_current")
            .background("#ff9000")
            .build();
        gloss_view.buffer().tag_table().add(&search_tag);
        gloss_view.buffer().tag_table().add(&search_current_tag);

        GlossOverlay {
            overlay,
            scrim,
            container,
            title,
            orig_header,
            original_label,
            corr_header,
            corrected_label,
            hint,
            footer_box,
            citation_label,
            position_label,
            marker_glyph,
            marker_color,
            panel_color,
            gloss_scroll_overlay,
            gloss_scrolled,
            gloss_view,
            bar_drawing,
            panel_drawing,
            clip_guard,
            bar_ranges,
            bar_color,
            vim_block_line,
            bar_x,
            line_numbers,
            echo_lines,
            echo_header_view,
            echo_rule,
            text_margins: text_margins as i32,
            column_width: column_width as i32,
            font_family: RefCell::new(GLOSS_DEFAULT_FONT_FAMILY.to_string()),
            font_size: std::cell::Cell::new(GLOSS_DEFAULT_FONT_SIZE),
            synopsis_label_ranges: RefCell::new(Vec::new()),
            last_card_size: Cell::new((0, 0)),
            blocks,
            cursor_block: Cell::new(0),
            synopsis_visual_anchor: Cell::new(None),
            all_blocks: RefCell::new(Vec::new()),
            pages: RefCell::new(Vec::new()),
            page_idx: Cell::new(0),
            cursor_full: Cell::new(0),
            gloss_pos: Cell::new((0, 0)),
            paginated: Cell::new(false),
            paginated_mode: Cell::new(PaginatedMode::Synopsis),
            gloss_block_markups: RefCell::new(Vec::new()),
            current_gloss: RefCell::new(String::new()),
            gloss_source_line_numbers: RefCell::new(Vec::new()),
            current_synopsis: RefCell::new(String::new()),
            ask_host,
            vim_engine: RefCell::new(None),
            vim_seed: RefCell::new(String::new()),
            vim_copy_only: std::cell::Cell::new(false),
            vim_cursor_colors: RefCell::new((String::new(), String::new())),
            highlight_bg: RefCell::new(crate::ui::DEFAULT_HIGHLIGHT_BG.to_string()),
            hi_ranges: RefCell::new(Vec::new()),
            pre_edit_family: RefCell::new(None),
            search_tag,
            search_current_tag,
        }
    }

    /// Apply the overlay's font (family + size) to the gloss text and header via
    /// a buffer-wide font TextTag, overriding the global `.gloss-text` CSS. Call
    /// after each populate so a rebuilt buffer keeps the chosen size.
    pub fn apply_font(&self) {
        let font_str = format!("{} {}", self.font_family.borrow(), self.font_size.get());
        crate::ui::apply_font_to_views(
            &[&self.gloss_view, &self.echo_header_view, self.ask_host.input()],
            &font_str,
            "gloss-font",
        );
        // The buffer-wide font tag carries the family's regular weight, so it
        // overrides any earlier bold tag. Re-assert the synopsis label bold so
        // it wins (it is added/applied last, hence highest priority).
        self.apply_synopsis_label_bold();
        // Re-assert the `<hi>` highlight background color (the tag was created at
        // population time with a default; this paints it the theme color).
        self.apply_hi_color();
    }

    /// Set the `<hi>` highlight background (theme `cursor_line_bg`) and re-assert
    /// it on the existing `gloss-hi` tag. Idempotent; safe before any render.
    pub fn set_highlight_color(&self, color: &str) {
        *self.highlight_bg.borrow_mut() = color.to_string();
        self.apply_hi_color();
    }

    /// The overlay's TextView buffer (stable for the view's lifetime — never
    /// replaced by `set_text`/populate), for overlay-search (Task 3) to read/scan.
    pub fn buffer(&self) -> gtk4::TextBuffer {
        self.gloss_view.buffer()
    }

    /// The "all matches" search-highlight tag.
    pub fn search_tag(&self) -> &gtk4::TextTag {
        &self.search_tag
    }

    /// The "current match" search-highlight tag (brighter than `search_tag`).
    pub fn search_current_tag(&self) -> &gtk4::TextTag {
        &self.search_current_tag
    }

    /// Set the search-highlight tag colors (theme-wired; see Task 5).
    pub fn set_search_colors(&self, all: &str, current: &str) {
        self.search_tag.set_background(Some(all));
        self.search_current_tag.set_background(Some(current));
    }

    /// Scroll the view so the given char offset is on-screen. Creates a
    /// throwaway mark at the offset, scrolls it into view, then deletes it
    /// (matches the `get_insert`/`scroll_mark_onscreen` idiom used for the vim
    /// cursor elsewhere in this file).
    pub fn scroll_to_char_offset(&self, off: i32) {
        let buffer = self.gloss_view.buffer();
        let iter = buffer.iter_at_offset(off);
        let mark = buffer.create_mark(None, &iter, false);
        self.gloss_view.scroll_mark_onscreen(&mark);
        buffer.delete_mark(&mark);
    }

    /// Set the page-marker glyph's dim color (theme `dim_fg`) and repaint the bar.
    /// (The gloss speaker/verse header used to dim in this color too; it now
    /// renders full ink — see render_gloss_page.)
    pub fn set_marker_color(&self, hex: &str) {
        crate::ui::set_rc_color(hex, &self.marker_color, &self.bar_drawing);
    }

    /// Set the inset tinted panel color (theme `panel_bg`) and repaint the panel.
    pub fn set_panel_color(&self, hex: &str) {
        crate::ui::set_rc_color(hex, &self.panel_color, &self.panel_drawing);
    }

    /// Re-assert the `<hi>` highlight: paint the `gloss-hi` tag the stored theme
    /// background, and (for the set_text synopsis path) re-apply it over the
    /// stored `hi_ranges`. The gloss-result path tags during population, so it has
    /// no ranges here — this only refreshes the color there.
    fn apply_hi_color(&self) {
        let buffer = self.gloss_view.buffer();
        let table = buffer.tag_table();
        let ranges = self.hi_ranges.borrow();
        // Ensure the tag exists if we have ranges to paint (synopsis set_text).
        if table.lookup("gloss-hi").is_none() && !ranges.is_empty() {
            table.add(
                &gtk4::TextTag::builder()
                    .name("gloss-hi")
                    .background(&*self.highlight_bg.borrow())
                    .build(),
            );
        }
        if let Some(tag) = table.lookup("gloss-hi") {
            tag.set_background(Some(&self.highlight_bg.borrow()));
            for &(s, e) in ranges.iter() {
                let si = buffer.iter_at_offset(s as i32);
                let ei = buffer.iter_at_offset(e as i32);
                buffer.apply_tag(&tag, &si, &ei);
            }
        }
    }

    /// Follow the reader card's font FAMILY and SIZE. Called from `reapply_font`
    /// so the overlay's default body font always matches the main card's
    /// currently configured font — on work load, on +/- size changes, and on f/F
    /// family cycling — instead of drifting to the fixed `GLOSS_DEFAULT_FONT_*`.
    /// Pagination is safe: every show path repaginates, and reader font changes
    /// only happen in reader mode (the overlay is closed, or a stale page just
    /// gets repaginated on next show). Does NOT run while `begin_edit_font` has
    /// stashed the reading family for the mono edit swap — clobbering
    /// `font_family` mid-edit would corrupt the stash `end_edit_font` restores
    /// from, and no work-load/font-size change can fire while the vim editor is
    /// open anyway (both routes are reader-mode only), so this is belt-and-braces.
    pub fn sync_reader_font(&self, family: &str, size: i32) {
        if self.pre_edit_family.borrow().is_some() {
            return;
        }
        let family_changed = self.font_family.borrow().as_str() != family;
        let size_changed = self.font_size.get() != size;
        if family_changed || size_changed {
            *self.font_family.borrow_mut() = family.to_string();
            self.font_size.set(size);
            self.apply_font();
        }
    }

    /// Set the overlay's font (family + size) and re-apply it. Thin entry point
    /// mirroring `JournalOverlay::set_font`, so `begin_edit_font`/`end_edit_font`
    /// can swap to the mono edit font and back. (The overlay otherwise drives its
    /// font through `apply_font` + the `font_family`/`font_size` fields directly.)
    pub fn set_font(&self, family: &str, size: i32) {
        *self.font_family.borrow_mut() = family.to_string();
        self.font_size.set(size);
        self.apply_font();
    }

    /// Swap to the monospace edit font, stashing the current reading family so
    /// `end_edit_font` can restore it. Size is unchanged. Mirrors
    /// `JournalOverlay::begin_edit_font`.
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
    /// is stashed, so redundant exit paths are safe.
    pub fn end_edit_font(&self) {
        let stashed = self.pre_edit_family.borrow_mut().take();
        if let Some(family) = stashed {
            let size = self.font_size.get();
            self.set_font(&family, size);
        }
    }

    // ---- in-place vim editor (the `e` bind) ----
    //
    // Mirrors `JournalOverlay`'s editor, but the buffer is a SINGLE raw-text blob
    // (the gloss markup OR the synopsis text) — no `Q:`/answer framing, so the
    // engine is seeded with the raw text directly and `edit_buffer_text` reads it
    // back as-is. A later task wires the `e` keybind + `InputMode::GlossEdit`.

    /// Enter the in-place vim editor on a single raw-text blob (gloss markup or
    /// synopsis text). Seeds a `VimEngine` in NORMAL mode, loads the text as
    /// plain text into `gloss_view`, swaps to the mono edit font, paints the block
    /// cursor + mode footer. The caller sets `InputMode::GlossEdit` afterward.
    pub fn enter_edit_buffer(&self, raw: &str, block_fill: &str, block_fg: &str) {
        self.begin_edit_font();
        // The editor shows RAW markup (with `<hi>` literals); the read-mode hi
        // ranges are stale here and must not be re-applied to the raw buffer.
        self.hi_ranges.borrow_mut().clear();
        *self.vim_cursor_colors.borrow_mut() = (block_fill.to_string(), block_fg.to_string());
        *self.vim_seed.borrow_mut() = raw.to_string();
        *self.vim_engine.borrow_mut() = Some(crate::input::vim::VimEngine::new(raw.to_string()));
        // Drive the buffer/cursor ourselves; the native caret is hidden in NORMAL.
        // GTK only PAINTS the caret while the view holds focus, so lift
        // `focusable(false)` and grab focus — otherwise INSERT has no insertion
        // point. Key routing stays on the window's capture-phase controller.
        self.gloss_view.set_editable(false);
        self.gloss_view.set_cursor_visible(false);
        self.gloss_view.set_focusable(true);
        let _ = self.gloss_view.grab_focus();
        self.mirror_engine();
    }

    /// Mark the active edit buffer as the reader's copy-only segment view: the
    /// NORMAL footer advertises select/copy/quit instead of save/rewrite.
    /// Cleared automatically by `exit_edit_buffer`. Call BEFORE
    /// `enter_edit_buffer` so the first mirror renders the right footer.
    pub fn set_edit_copy_only(&self, on: bool) {
        self.vim_copy_only.set(on);
    }

    /// Feed one key to the engine, re-mirror, and return the resulting action.
    pub fn feed_edit_key(&self, key: crate::input::vim::VimKey) -> crate::input::vim::EditorAction {
        let action = {
            let mut guard = self.vim_engine.borrow_mut();
            match guard.as_mut() {
                Some(engine) => engine.handle_key(key).action,
                None => crate::input::vim::EditorAction::Nop,
            }
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

    /// The current editor buffer text (raw). Empty string when not editing.
    pub fn edit_buffer_text(&self) -> String {
        self.vim_engine
            .borrow()
            .as_ref()
            .map(|e| e.buffer().to_string())
            .unwrap_or_default()
    }

    /// True iff the buffer differs from the seed (for the `:q` dirty refusal).
    pub fn edit_is_dirty(&self) -> bool {
        match self.vim_engine.borrow().as_ref() {
            Some(e) => e.buffer() != self.vim_seed.borrow().as_str(),
            None => false,
        }
    }

    /// Reset the dirty baseline to `raw` (after a non-quit `:w`).
    pub fn reseed_edit_buffer(&self, raw: &str) {
        *self.vim_seed.borrow_mut() = raw.to_string();
    }

    /// Leave the editor: drop the engine, clear the block cursor, restore the
    /// native caret default + the read view's non-focusable state, and restore
    /// the reading font. The caller re-renders the formatted display and resets
    /// the input mode.
    pub fn exit_edit_buffer(&self) {
        self.vim_copy_only.set(false);
        crate::ui::clear_block_cursor(&self.gloss_view.buffer(), "gloss-vim-block");
        *self.vim_block_line.borrow_mut() = None;
        self.bar_drawing.queue_draw();
        *self.vim_engine.borrow_mut() = None;
        self.vim_seed.borrow_mut().clear();
        self.gloss_view.set_cursor_visible(false);
        self.gloss_view.set_focusable(false);
        self.end_edit_font();
    }

    /// Sync the engine state into `gloss_view`: replace the buffer text, place the
    /// cursor/selection, paint the block cursor in NORMAL/VISUAL (hide it + show
    /// the native caret in INSERT), and render the mode/`:` footer. Mirrors
    /// `JournalOverlay::mirror_engine`, adapted to the single raw buffer + the
    /// `"gloss-vim-block"` tag.
    fn mirror_engine(&self) {
        let guard = self.vim_engine.borrow();
        let Some(engine) = guard.as_ref() else {
            return;
        };
        let buffer = self.gloss_view.buffer();
        // 1. Text — only rewrite when it actually changed (avoids resetting marks
        // on pure cursor moves), then re-apply the font over the new text.
        let current = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        if current != engine.buffer() {
            buffer.set_text(engine.buffer());
            self.apply_font();
        }
        // 2. Cursor + selection (char indices → iters, clamped to the buffer).
        let n_chars = engine.buffer().chars().count();
        let char_to_iter = |ci: usize| -> gtk4::TextIter {
            buffer.iter_at_offset(ci.min(n_chars) as i32)
        };
        if let Some(sel) = engine.selection() {
            let start = char_to_iter(sel.start);
            let end = char_to_iter(sel.end);
            buffer.select_range(&start, &end);
        } else {
            let cur = char_to_iter(engine.cursor());
            buffer.place_cursor(&cur);
        }
        // 3. Block cursor (NORMAL/VISUAL) vs native caret (INSERT).
        let mode = engine.mode();
        if mode == crate::input::vim::Mode::Insert {
            crate::ui::clear_block_cursor(&buffer, "gloss-vim-block");
            *self.vim_block_line.borrow_mut() = None;
            self.gloss_view.set_cursor_visible(true);
        } else {
            let (fill, fg) = self.vim_cursor_colors.borrow().clone();
            crate::ui::paint_block_cursor(&buffer, "gloss-vim-block", &fill, &fg, engine.cursor());
            // On a BLANK line the cursor char is the line's `\n` (no glyph cell),
            // so the char-background paints nothing. Draw a left-edge block via
            // `bar_drawing` instead (cleared otherwise). A line is blank when its
            // cursor iter both starts and ends the line.
            let cur_iter = char_to_iter(engine.cursor());
            let on_blank = cur_iter.starts_line() && cur_iter.ends_line();
            if on_blank {
                let rgb = parse_hex_color(&fill).unwrap_or((0.53, 0.62, 0.71));
                *self.vim_block_line.borrow_mut() =
                    Some((cur_iter.line(), rgb.0, rgb.1, rgb.2));
            } else {
                *self.vim_block_line.borrow_mut() = None;
            }
            self.bar_drawing.queue_draw();
            // Hide the native caret except at true end-of-buffer (where neither the
            // char block nor — if that line has glyphs — the drawn block applies).
            let at_end = engine.cursor() >= n_chars && !on_blank;
            self.gloss_view.set_cursor_visible(at_end);
        }
        // Keep the cursor on screen.
        let mark = buffer.get_insert();
        self.gloss_view.scroll_mark_onscreen(&mark);
        // 4. Footer (mode line / `:` command).
        let footer = if let Some(cmd) = engine.cmdline() {
            format!(":{}", cmd)
        } else {
            match mode {
                crate::input::vim::Mode::Normal => {
                    if self.vim_copy_only.get() {
                        "-- NORMAL --  (v select \u{00b7} y copy \u{00b7} :q quit)".to_string()
                    } else {
                        "-- NORMAL --  (:w save \u{00b7} R rewrite \u{00b7} :q quit)".to_string()
                    }
                }
                crate::input::vim::Mode::Insert => "-- INSERT --".to_string(),
                crate::input::vim::Mode::Visual => "-- VISUAL --".to_string(),
                crate::input::vim::Mode::VisualLine => "-- VISUAL LINE --".to_string(),
            }
        };
        self.set_edit_footer(&footer);
    }

    /// Show `text` in the overlay footer during edit. Uses the overlay's
    /// right-aligned `position_label` (the page counter); a non-edit re-render
    /// restores the counter via `update_position_label`.
    fn set_edit_footer(&self, text: &str) {
        self.position_label.set_text(text);
        self.position_label.set_visible(true);
    }

    /// Whether the overlay is currently showing a SYNOPSIS (vs a gloss result),
    /// so the edit caller can seed the engine with the synopsis text rather than
    /// the gloss markup. Reads the paginated render mode.
    pub fn is_showing_synopsis(&self) -> bool {
        self.paginated_mode.get() == PaginatedMode::Synopsis
    }

    /// Bold the stored synopsis label ranges on the gloss view. Adds the
    /// `synopsis-label` weight tag if absent and (re-)applies it last so it
    /// outranks the regular-weight `gloss-font` tag. No-op when no ranges are
    /// stored (glosses, echoes, loading states).
    fn apply_synopsis_label_bold(&self) {
        let ranges = self.synopsis_label_ranges.borrow();
        if ranges.is_empty() {
            return;
        }
        let buffer = self.gloss_view.buffer();
        let table = buffer.tag_table();
        if table.lookup("synopsis-label").is_none() {
            table.add(
                &gtk4::TextTag::builder()
                    .name("synopsis-label")
                    .weight(700)
                    .build(),
            );
        }
        if let Some(tag) = table.lookup("synopsis-label") {
            // Tag conflicts resolve by priority, which defaults to add-order.
            // The `gloss-font` tag (regular weight) is added after this one on
            // the first show, so it would win. Force the label to the highest
            // priority so its bold weight outranks the font tag's weight.
            crate::ui::raise_tag_to_top(&table, &tag);
            for &(start, end) in ranges.iter() {
                let s = buffer.iter_at_offset(start as i32);
                let e = buffer.iter_at_offset(end as i32);
                buffer.apply_tag(&tag, &s, &e);
            }
        }
    }

    /// Color every block whose `is_cached(kind, index)` returns true with the
    /// theme accent (`accent`, = theme `cursor_bg`) — deliberately DISTINCT from
    /// the bar/divider `root_color`, so a synthesized block reads as "active"
    /// rather than blending into the bar. Idempotent; re-tagging an
    /// already-colored block is harmless. Call AFTER `apply_font` with
    /// `self.blocks` already populated (every `show_*` path does both). The
    /// injected `is_cached` predicate must NOT borrow the overlay's own block
    /// state (it runs while `self.blocks` is borrowed), as a re-entrant borrow
    /// would panic.
    pub fn color_audio_blocks(&self, accent: &str, is_cached: impl Fn(&BlockKind, i32) -> bool) {
        let buffer = self.gloss_view.buffer();
        // Normalize the accent to a stable #rrggbb (gloss-only round-trip).
        let rgba = match parse_hex_color(accent) {
            Some((r, g, b)) => format!(
                "#{:02x}{:02x}{:02x}",
                (r * 255.0).round() as u8,
                (g * 255.0).round() as u8,
                (b * 255.0).round() as u8,
            ),
            None => accent.to_string(),
        };
        // Build the cached spans. A Source block's range begins at its first VERSE
        // line; the speaker heading (gloss_blocks drops it from the block text)
        // sits one line above. Recolor it together with the verse so the whole
        // turn — label and body — reads as cached. Only extend when that line
        // truly carries a speaker tag, so we never bleed the accent onto a
        // preceding verse/prose line of another block.
        let blocks = self.blocks.borrow();
        let spans: Vec<(i32, i32)> = blocks
            .iter()
            .filter(|blk| is_cached(&blk.kind, blk.index))
            .map(|blk| {
                let start_line = if blk.kind == BlockKind::Source
                    && blk.start_line > 0
                    && line_is_speaker(&buffer, blk.start_line - 1)
                {
                    blk.start_line - 1
                } else {
                    blk.start_line
                };
                (start_line, blk.end_line)
            })
            .collect();
        crate::log_fmt!("COLOR-AUDIO: {} blocks, {} cached spans, fg={}", blocks.len(), spans.len(), rgba);
        drop(blocks);
        crate::ui::apply_cached_coloring(&buffer, "gloss-audio-cached", &rgba, &spans);
    }

    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_overlay_panel(
            &self.overlay, child, &self.scrim, &self.container,
        );
    }

    /// Restore the body-sized bold `gloss-title` style on the shared title (the
    /// synopsis view swaps it to the quiet `synopsis-header`). Idempotent.
    fn set_gloss_title_style(&self) {
        self.title.remove_css_class("synopsis-header");
        self.title.add_css_class("gloss-title");
    }

    pub fn show(&self, original: &str, corrected: &str) {
        self.hide_citation();
        self.title.set_visible(true);
        self.title.set_text("Gloss");
        self.set_gloss_title_style();
        // Reset the top margin in case `show_glossing` widened it (shared title).
        self.title.set_margin_top(24);
        // Indent the title and the diff/error labels to match the inset used by
        // the loading ("Glossing…") and result cards for the current work type:
        // card_width/5 for prose, card_width/4 for verse. Reuse the last rendered
        // card width (an error/toast always follows a card render); fall back to
        // the container's own width if a card was never shown.
        let card_width = match self.last_card_size.get().0 {
            w if w > 0 => w,
            _ => self.container.width().max(self.container.width_request()),
        };
        let left = crate::ui::prose_column_margin(card_width);
        self.title.set_margin_start(left);
        self.orig_header.set_margin_start(left);
        self.original_label.set_margin_start(left);
        self.corr_header.set_margin_start(left);
        self.corrected_label.set_margin_start(left);
        let orig_markup = build_diff_markup(original, corrected, true);
        let corr_markup = build_diff_markup(original, corrected, false);
        self.original_label.set_markup(&orig_markup);
        self.corrected_label.set_markup(&corr_markup);
        self.orig_header.set_visible(true);
        self.original_label.set_visible(true);
        self.corr_header.set_visible(true);
        self.corrected_label.set_visible(true);
        self.gloss_scroll_overlay.set_visible(false);
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);
        self.hint.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.apply_font();
    }

    pub fn show_gloss_with_color(&self, _original: &str, gloss: &str, card_width: i32, card_height: i32, root_color: Option<&str>, source_line_numbers: &[(String, i64)]) {
        // No synopsis label bolding in gloss view.
        self.synopsis_label_ranges.borrow_mut().clear();        self.hi_ranges.borrow_mut().clear();
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.last_card_size.set((card_width, card_height));
        // A fresh gloss render closes any open add/edit ask card and clears its
        // focus highlight (e.g. after an add/edit completes or n/p navigates).
        self.ask_host.card().close();
        self.title.set_visible(false);
        self.title.set_vexpand(false);
        self.title.set_valign(Align::Start);
        self.title.set_halign(Align::Start);
        // Wide side margins (card/5, uniform for all work types) keep the gloss
        // column at a comfortable reading measure. Anchor to the actual card
        // width (the overlay is full-screen), NOT the fixed column_width (1050)
        // — otherwise on a wide card the margin stays tiny and the text runs
        // nearly edge to edge.
        let left = crate::ui::prose_column_margin(card_width);
        self.set_prose_margins(left);
        self.set_gloss_hint();
        self.hide_diff_labels();
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);

        self.set_bar_color_from_root(root_color);

        let bar_left = left;
        *self.bar_x.borrow_mut() = bar_left;

        // PAGINATE (like the synopsis + journal): each cursor-stop block is one
        // page unit; the page renders only the blocks that fit so no partial
        // verse/paragraph shows at either edge (the top edge has no clip box). The
        // page slice is re-rendered via populate_gloss_buffer over its blocks'
        // ORIGINAL markup (gloss_block_markups), because GlossBlock.display drops
        // the speaker headings + verse tags the gloss render needs. Source (verse)
        // blocks are over-measured (gloss_block_height) so a speaker label never
        // clips at a page top. render_gloss_page (below) does the populate.
        *self.current_gloss.borrow_mut() = gloss.to_string();
        *self.gloss_source_line_numbers.borrow_mut() = source_line_numbers.to_vec();
        *self.all_blocks.borrow_mut() = gloss_blocks(gloss);
        *self.gloss_block_markups.borrow_mut() = gloss_block_markups(gloss);
        self.cursor_full.set(0);
        self.page_idx.set(0);
        self.paginated.set(true);
        self.paginated_mode.set(PaginatedMode::Gloss);

        *self.echo_lines.borrow_mut() = Vec::new();

        self.gloss_scroll_overlay.set_visible(true);
        self.hint.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        // Fixed-scroll-height: the gloss result hides the title; only the footer
        // sits below the scroll (hidden when the ask card opens). Record the closed
        // scroll height so the add/edit ask card shrinks the viewport (no occlusion
        // of the gloss text). Must run BEFORE repaginate (it sets the page budget).
        self.size_scroll(card_height, self.title_pref_h());
        // Paginate against the now-fixed viewport, then render page 0. render_*
        // populates the buffer, re-derives the page-local blocks, applies the
        // font, pins the vadjustment at 0, and marks the bar.
        self.repaginate(self.gloss_page_height());
        self.render_gloss_page();
        self.reset_scroll_top();
        // mark_cursor_block (inside render) sets bar_ranges, but the bar DRAW reads per-line
        // geometry (line_yrange) which is 0/stale until GTK lays out the buffer
        // just made visible above — so the synchronous draw paints nothing and
        // the accent bar only appeared after the first j/k/Alt+n. Repaint once
        // more after layout settles (same fix the echo/synopsis path uses).
        let bar = self.bar_drawing.clone();
        glib::idle_add_local_once(move || bar.queue_draw());
    }

    /// "Glossing…" loading card that shows the passage being glossed, rendered
    /// single-column with the SAME `<speaker>`/`<verse>` formatting the gloss
    /// result uses for the original passage. `passage_doc` is the
    /// `<speaker>`/`<verse>` markup (see `build_source_header`). The "Glossing…"
    /// status sits as a header above the passage; the result simply replaces this
    /// view in place when it arrives, so the passage looks identical before/after.
    /// Shared prose-geometry prefix of the gloss show paths (audit #64 rider):
    /// title indent + body margins for a prose gloss page.
    fn set_prose_margins(&self, left: i32) {
        self.title.set_margin_start(left);
        self.gloss_view.set_left_margin(left);
        self.gloss_view.set_right_margin(left);
        self.gloss_view.set_top_margin(32);
        self.gloss_view.set_pixels_below_lines(4);
    }

    /// Hide the four gloss-diff labels (audit #64). Every show path hides
    /// these; the echo/position/hint extras vary per caller and stay inline.
    fn hide_diff_labels(&self) {
        self.orig_header.set_visible(false);
        self.original_label.set_visible(false);
        self.corr_header.set_visible(false);
        self.corrected_label.set_visible(false);
    }

    /// Update the accent-bar color from the theme `root_color` (audit #63).
    /// Deliberately no `queue_draw` — the show paths repaint anyway (this is
    /// NOT `ui::set_rc_color`, which queues a draw for the standalone setters).
    fn set_bar_color_from_root(&self, root_color: Option<&str>) {
        if let Some(color) = root_color {
            if let Some((r, g, b)) = parse_hex_color(color) {
                *self.bar_color.borrow_mut() = (r, g, b);
            }
        }
    }

    pub fn show_glossing(&self, passage_doc: &str, card_width: i32, card_height: i32, root_color: Option<&str>) {
        self.hide_citation();
        self.synopsis_label_ranges.borrow_mut().clear();        self.hi_ranges.borrow_mut().clear();
        self.blocks.borrow_mut().clear();
        self.paginated.set(false);
        *self.marker_glyph.borrow_mut() = None;
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.last_card_size.set((card_width, card_height));
        self.ask_host.card().close();

        // "Glossing…" as a top header (not the centered label of
        // `show_loading_message`), matching the gloss result's title placement.
        self.title.set_text("Glossing\u{2026}");
        self.set_gloss_title_style();
        self.title.set_visible(true);
        self.title.set_vexpand(false);
        self.title.set_valign(Align::Start);
        self.title.set_halign(Align::Start);
        // Match the gloss result's top spacing (constructor default) — the extra
        // 64px pushed the tinted panel down and shrank it vs the result card.
        self.title.set_margin_top(24);

        // Same passage geometry the gloss result uses (`show_gloss_with_color`):
        // wide side margins anchored to the actual card width, accent bar at
        // card_width/5 (uniform for all work types).
        let left = crate::ui::prose_column_margin(card_width);
        self.set_prose_margins(left);

        // No diff labels or echo views while loading.
        self.hide_diff_labels();
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);
        // Reserve the SAME footer band the gloss result uses (hr + labels), so
        // `size_scroll` sizes the tinted panel to the result's footprint. The
        // labels carry no text on the loading card, so the band shows only the
        // rule — matching dimensions without adding loading-card chrome.
        self.set_gloss_hint();
        self.hint.set_visible(true);
        self.position_label.set_visible(false);

        self.set_bar_color_from_root(root_color);

        let bar_left = left;
        *self.bar_x.borrow_mut() = bar_left;

        // Render the passage through the SAME path as the gloss result's original
        // passage, so speaker small-caps + indented verse look identical.
        // Speaker + verse render FULL ink like the explication (no header color:
        // the user found the dimmed source too recessed) — the hang-indent alone
        // sets the source apart. Matches render_gloss_page.
        let (ranges, _nums) = populate_gloss_buffer(
            &self.gloss_view, passage_doc, self.text_margins, bar_left, &[],
            None,
        );
        *self.bar_ranges.borrow_mut() = ranges;
        self.line_numbers.borrow_mut().clear();
        *self.echo_lines.borrow_mut() = Vec::new();
        self.bar_drawing.queue_draw();

        self.gloss_scroll_overlay.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.apply_font();
        // Fixed-scroll-height: the "Glossing…" loading card shows the title but
        // hides the hint footer, and has no ask card. With vexpand off the scroll
        // still needs an explicit height — title only above it.
        self.size_scroll(card_height, self.title_pref_h());
        self.reset_scroll_top();
    }

    /// Render the echoes overlay: a fixed source-turn header + rule, above the
    /// scrolling echo list. `source_doc` is the <speaker>/<verse> turn; `echo_doc`
    /// is only the <gloss> lines.
    pub fn show_echoes(
        &self,
        source_doc: &str,
        echo_doc: &str,
        card_width: i32,
        card_height: i32,
        root_color: Option<&str>,
        dim_color: Option<&str>,
        selected: usize,
    ) {
        // No synopsis label bolding in echo view.
        self.hide_citation();
        self.synopsis_label_ranges.borrow_mut().clear();        self.hi_ranges.borrow_mut().clear();
        self.blocks.borrow_mut().clear();
        self.paginated.set(false);
        *self.marker_glyph.borrow_mut() = None;
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.last_card_size.set((card_width, card_height));
        self.ask_host.card().close();
        self.title.set_visible(false);
        let left = self.column_width / 8;
        self.title.set_margin_start(left);
        self.gloss_view.set_left_margin(left);
        // Reset margins/spacing the synopsis and gloss views may have widened,
        // so the echo list and its accent bar stay aligned at column_width/8.
        self.gloss_view.set_right_margin(self.column_width / 8);
        self.gloss_view.set_top_margin(24);
        self.gloss_view.set_pixels_below_lines(0);
        self.echo_header_view.set_left_margin(left);
        self.hint.set_text("Esc close · a play · A add · s curate · d/D delete · R refresh");
        self.hide_diff_labels();

        self.set_bar_color_from_root(root_color);

        let bar_left = self.column_width / 8;
        *self.bar_x.borrow_mut() = bar_left;

        // Fixed header: render the source turn into the non-scrolling view.
        // Reuse populate_verse_buffer (it builds the speaker/verse tags and
        // returns empty bar data for a source-only doc).
        let _ = populate_verse_buffer(
            &self.echo_header_view, source_doc, self.text_margins, bar_left, &[], None, dim_color);
        self.echo_header_view.set_visible(true);
        self.echo_rule.set_visible(true);

        // Scrolling list: only the echoes. echo_lines/bar_ranges are now indexed
        // from the first echo (no source lines to offset past).
        let (ranges, nums, echo_lines) = populate_verse_buffer(
            &self.gloss_view, echo_doc, self.text_margins, bar_left, &[], Some(selected), dim_color);
        *self.bar_ranges.borrow_mut() = ranges;
        *self.line_numbers.borrow_mut() = nums;
        *self.echo_lines.borrow_mut() = echo_lines;
        // Repaint the bar overlay after GTK lays out the rebuilt buffer (drawing
        // synchronously reads stale per-line geometry).
        let bar = self.bar_drawing.clone();
        glib::idle_add_local_once(move || bar.queue_draw());

        self.gloss_scroll_overlay.set_visible(true);
        self.hint.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.apply_font();
        // Fixed-scroll-height: echoes mode hides the title but shows the source
        // header + rule ABOVE the scroll (they stay put while the "A add" ask card
        // is open). The footer below is hidden on open (handled by size_scroll).
        let echo_chrome = self.echo_header_view.preferred_size().1.height()
            + self.echo_rule.preferred_size().1.height();
        self.size_scroll(card_height, echo_chrome);
        self.reset_scroll_top();
    }

    /// Scroll the card so the Nth echo's quote line is visible.
    pub fn scroll_echo_into_view(&self, echo_index: usize) {
        let line = match self.echo_lines.borrow().get(echo_index).copied() {
            Some(l) => l,
            None => return,
        };
        // Scroll the ScrolledWindow's adjustment (the actual scroller) rather
        // than the TextView's own — the view is sized to its full content, so
        // gloss_view.scroll_to_mark is a no-op here. Defer to idle so the
        // rebuilt buffer has been laid out before we query line/adjustment
        // geometry (querying synchronously after set_text yields stale bounds).
        let view = self.gloss_view.clone();
        let scrolled = self.gloss_scrolled.clone();
        let bar = self.bar_drawing.clone();
        glib::idle_add_local_once(move || {
            let buffer = view.buffer();
            let iter = match buffer.iter_at_line(line) {
                Some(it) => it,
                None => return,
            };
            let (line_y, line_h) = view.line_yrange(&iter);
            let top_margin = view.top_margin();
            let line_top = (line_y + top_margin) as f64;
            let line_bottom = line_top + line_h as f64;

            let adj = scrolled.vadjustment();
            let view_top = adj.value();
            let view_bottom = view_top + adj.page_size();
            let pad = 24.0;
            let max_val = (adj.upper() - adj.page_size()).max(adj.lower());

            let new_val = if line_top < view_top + pad {
                (line_top - pad).clamp(adj.lower(), max_val)
            } else if line_bottom > view_bottom - pad {
                (line_bottom + pad - adj.page_size()).clamp(adj.lower(), max_val)
            } else {
                return; // Already visible — don't scroll.
            };
            adj.set_value(new_val);
            bar.queue_draw();
        });
    }

    pub fn show_synopsis(
        &self,
        title: &str,
        synopsis: &str,
        root_color: Option<&str>,
        card_width: i32,
        card_height: i32,
        prose_card: Option<SynopsisProseCard>,
    ) {
        self.hide_citation();
        *self.current_synopsis.borrow_mut() = synopsis.to_string();
        // Clear any stale visual-mode anchor: showing a (possibly different)
        // synopsis rebuilds the block list, so an old anchor index is invalid.
        self.synopsis_visual_anchor.set(None);
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.last_card_size.set((card_width, card_height));
        // A fresh synopsis render closes any open ask card and returns focus to
        // the synopsis (e.g. after an amend completes, or n/p moves scenes).
        self.ask_host.card().close();
        // Match the gloss margins: anchor to the actual (full-screen) card
        // width, not the fixed column_width, so the synopsis prose sits at the
        // same ~65-char measure as the gloss instead of running nearly edge to
        // edge.
        let inset = crate::ui::prose_column_margin(card_width);
        // Prose synopses use the main card's fixed pixel left padding; plays/verse
        // use the proportional `card_width/5` inset. The accent bar sits one
        // "breathing room" (60px) to the LEFT of the prose body so text aligns to
        // the card while the bar is still visible.
        let body_left = prose_card.as_ref().map(|p| p.left_margin).unwrap_or(inset + 60);
        let bar_left = prose_card.as_ref().map(|p| (p.left_margin - 60).max(0)).unwrap_or(inset);
        let title_left = prose_card.as_ref().map(|p| p.left_margin).unwrap_or(inset);
        self.title.set_text(title);
        // The synopsis title uses its own quiet `synopsis-header` style (dim,
        // normal weight, underlined) rather than the body-sized bold gloss-title.
        // Scoped separately from `.gloss-header` (the gloss card's ORIGINAL/GLOSS
        // section headers + ask-card title) so bumping this size never touches
        // those. The gloss-result paths restore gloss-title.
        self.title.remove_css_class("gloss-title");
        self.title.add_css_class("synopsis-header");
        self.title.set_visible(true);
        self.title.set_vexpand(false);
        self.title.set_valign(Align::Start);
        self.title.set_halign(Align::Start);
        self.title.set_margin_start(title_left);
        // Reset the top margin in case `show_glossing` widened it (it shares this
        // title widget). The synopsis card gives the "Act N, Scene N" header
        // extra breathing room above it.
        self.title.set_margin_top(56);
        self.hide_diff_labels();
        self.position_label.set_visible(false);
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);

        *self.bar_ranges.borrow_mut() = Vec::new();
        *self.line_numbers.borrow_mut() = Vec::new();
        *self.echo_lines.borrow_mut() = Vec::new();

        // Body sits at `body_left` (prose: the card's pixel padding; plays: the
        // proportional inset + 60 past the bar). The bar stays at `bar_left`. The
        // right margin matches the card too (prose: text_margins+EXTRA_RIGHT;
        // plays keep the proportional inset for the narrower ~65-char measure).
        let body_right = prose_card.as_ref().map(|p| p.right_margin).unwrap_or(inset);
        self.gloss_view.set_left_margin(body_left);
        self.gloss_view.set_right_margin(body_right);
        // Tighten the gap between the title rule and the first synopsis line by
        // ~one line (was 32) — the title's own margin/padding-bottom already
        // supplies separation, so the prose can sit closer under the rule.
        self.gloss_view.set_top_margin(8);
        self.gloss_view.set_pixels_below_lines(6);
        // Match the gloss overlay's accent color (theme root_color) so the bar is
        // the same saturated accent, not the pale constructor default.
        self.set_bar_color_from_root(root_color);
        *self.bar_x.borrow_mut() = bar_left;
        // PAGINATE (like the journal): each non-label <p> is one Explication
        // cursor stop; the page renders only the blocks that fit so no partial
        // paragraph shows at either edge. The first render selects block 0. The
        // actual buffer text + block ranges are produced by render_synopsis_page
        // below (after size_scroll fixes the page budget).
        *self.all_blocks.borrow_mut() = synopsis_blocks(synopsis);
        self.cursor_full.set(0);
        self.page_idx.set(0);
        self.paginated.set(true);
        self.paginated_mode.set(PaginatedMode::Synopsis);

        self.gloss_scroll_overlay.set_visible(true);
        self.set_synopsis_hint();
        self.hint.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        // Fixed-scroll-height: record the closed scroll height (the footer is
        // below; gloss has no toggled footer) so opening the ask card shrinks the
        // viewport. Must run BEFORE repaginate (it sets the page-height budget).
        self.size_scroll(card_height, self.title_pref_h());
        // Paginate against the now-fixed viewport, then render page 0. render_*
        // sets the buffer text, re-derives blocks, applies the font, marks the bar.
        self.repaginate(self.synopsis_page_height());
        self.render_synopsis_page();
        // Prose: override the overlay's Charter-19 font tag on the synopsis body
        // with the main card's font (family + size), so the synopsis reads like
        // its reading card. Applied AFTER render (which applies the default font)
        // so it wins; scoped to gloss_view only.
        if let Some(ref p) = prose_card {
            let buffer = self.gloss_view.buffer();
            let table = buffer.tag_table();
            if let Some(old) = table.lookup("gloss-font") {
                table.remove(&old);
            }
            let font_str = format!("{} {}", p.font_family, p.font_size);
            let tag = gtk4::TextTag::builder().name("gloss-font").font(&font_str).build();
            table.add(&tag);
            let (start, end) = buffer.bounds();
            buffer.apply_tag(&tag, &start, &end);
            crate::ui::reassert_italic_tags(&table);
            self.apply_synopsis_label_bold();
        }
        self.reset_scroll_top();

        // Headless test: emit the overlay viewport rect once layout settles, so
        // tests/overlay_clipping.rs can target the synopsis card's region.
        // GlossOverlay is not Clone, so capture the scrolled window and inline.
        if std::env::var_os("LIT_HEADLESS_TEST").is_some() {
            let sc = self.gloss_scrolled.clone();
            glib::idle_add_local_once(move || {
                if let Some(r) = sc.root().and_then(|root| sc.compute_bounds(&root)) {
                    crate::logging::log(&format!(
                        "TEST_OVERLAY_VIEWPORT_RECT {} {} {} {}",
                        r.x().round() as i32,
                        r.y().round() as i32,
                        r.width().round() as i32,
                        r.height().round() as i32
                    ));
                } else {
                    crate::logging::log(
                        "TEST_OVERLAY_VIEWPORT_RECT unavailable (root/compute_bounds returned None)",
                    );
                }
            });
        }
    }

    /// Snap the overlay's scroll position to the top and cover the open's
    /// multi-pass layout. Delegates to `BottomClipGuard::on_open` — see that
    /// method for the `changed`-handler + `pinning` + one-shot-disconnect + idle
    /// backstop logic (why a single inline/idle `set_value(0.0)` is unreliable).
    fn reset_scroll_top(&self) {
        self.clip_guard.on_open();
    }

    /// `&self` entry point for recomputing the bottom clip after a scroll.
    fn update_bottom_clip(&self) {
        self.clip_guard.recompute();
    }

    /// Record the card geometry on the ask-card host and set the scroll's CLOSED
    /// height (fixed-scroll-height). `above_chrome_h` is the non-scroll chrome
    /// ABOVE the scroll that stays visible while the ask card is open — it varies
    /// by show mode (synopsis/gloss-result: just the title; echoes: the source
    /// header and rule). The footer (hr and hints) is the host's TOGGLED footer,
    /// hidden on open, so the helper passes it separately as `footer_h` (not folded
    /// into the fixed chrome). Call from every show path that makes the scroll
    /// visible, AFTER the chrome visibility is set, so preferred sizes are accurate.
    fn size_scroll(&self, card_height: i32, above_chrome_h: i32) {
        let (card_width, _) = self.last_card_size.get();
        let footer_h = self.footer_pref_h();
        self.ask_host
            .size(card_width, card_height, above_chrome_h, footer_h);
        // Pin the VISIBLE display scroll (`gloss_scrolled` holds the synopsis /
        // gloss / echo text) to exactly the card's content height. Without this it
        // is unbounded (only `propagate_natural_height(false)` + `vexpand(false)`
        // are set), so a long synopsis sizes the scroll to its natural height and
        // the `valign=Center` container grows PAST `card_height` — making the whole
        // overlay taller than the main reading card. `max_content_height` is the
        // real cap (height_request alone is only a minimum); see
        // AskCardHost::pin_scroll_height for the same technique.
        // The scroll's parent `gloss_scroll_overlay` carries its own top+bottom
        // margins (24 + 20 = 44px) which `above_chrome_h`/`footer_h` (label
        // preferred sizes only) do NOT include — without subtracting them the
        // container overran `card_height` by exactly that 44px. Account for the
        // scroll-overlay margins so title + (margins + scroll) + footer == card.
        const SCROLL_OVERLAY_MARGINS: i32 = 24 + 20;
        let scroll_h =
            (card_height - above_chrome_h - footer_h - SCROLL_OVERLAY_MARGINS).max(80);
        self.gloss_scrolled.set_height_request(scroll_h);
        self.gloss_scrolled.set_max_content_height(scroll_h);
        self.gloss_scrolled.set_min_content_height(scroll_h);
        self.gloss_scrolled.queue_resize();
    }

    /// Preferred height of the title row (0 when hidden). Used for the
    /// fixed-scroll-height accounting.
    fn title_pref_h(&self) -> i32 {
        if self.title.is_visible() {
            self.title.preferred_size().1.height()
        } else {
            0
        }
    }

    /// Preferred height of the footer/hint row (hr + keybind hints). It is the
    /// host's TOGGLED footer — hidden while the ask card is open.
    fn footer_pref_h(&self) -> i32 {
        self.footer_box.preferred_size().1.height()
    }

    /// Map a pre-built block list to buffer-line spans (shared by the gloss path,
    /// which builds blocks with `gloss_blocks`, and the synopsis path, which uses
    /// `synopsis_blocks`). Matches each block's first `display` line against
    /// buffer lines, stores `self.blocks`, resets the cursor to block 0.
    fn rebuild_block_ranges_from(&self, blocks: Vec<GlossBlock>) {
        let buffer = self.gloss_view.buffer();
        let line_count = buffer.line_count();
        let mut ranges: Vec<BlockRange> = Vec::new();
        let mut search_from = 0i32;

        let find_line = |needle: &str, from: i32| -> Option<i32> {
            if needle.is_empty() {
                return None;
            }
            for line in from..line_count {
                if let Some(start) = buffer.iter_at_line(line) {
                    let mut end = start.clone();
                    if !end.ends_line() {
                        end.forward_to_line_end();
                    }
                    let line_text = buffer.text(&start, &end, false);
                    if line_text.as_str().trim().starts_with(needle) {
                        return Some(line);
                    }
                }
            }
            None
        };

        for b in blocks {
            let lines: Vec<&str> = b.display.lines().collect();
            let first_needle = lines.first().map(|s| s.trim()).unwrap_or("");
            let start_line = match find_line(first_needle, search_from) {
                Some(l) => l,
                None => continue,
            };
            let end_line = if b.kind == BlockKind::Source && lines.len() > 1 {
                let last_needle = lines.last().map(|s| s.trim()).unwrap_or("");
                find_line(last_needle, start_line + 1).unwrap_or(start_line)
            } else {
                start_line
            };
            ranges.push(BlockRange {
                kind: b.kind,
                index: b.index,
                start_line,
                end_line,
            });
            search_from = end_line + 1;
        }
        *self.blocks.borrow_mut() = ranges;
        // A fresh render selects the first block.
        self.cursor_block.set(0);
    }

    /// The selected cursor block as `(kind, index)`. None when the current card
    /// has no blocks (echoes/synopsis/empty gloss). The selection is the stored
    /// `cursor_block` index (set by j/k/gg/G), clamped to the block list — NOT
    /// derived from scroll position.
    pub fn current_block(&self) -> Option<(BlockKind, i32)> {
        let ranges = self.blocks.borrow();
        if ranges.is_empty() {
            return None;
        }
        let i = self.cursor_block.get().min(ranges.len() - 1);
        ranges.get(i).map(|r| (r.kind, r.index))
    }

    /// `j`/`k`: move the block cursor down/up one block.
    pub fn cursor_next_block(&self) {
        self.step_cursor(1);
    }
    pub fn cursor_prev_block(&self) {
        self.step_cursor(-1);
    }
    /// `gg`/`G`: move the block cursor to the first/last block.
    pub fn cursor_first_block(&self) {
        self.cursor_to_end(false);
    }
    pub fn cursor_last_block(&self) {
        self.cursor_to_end(true);
    }

    /// Step the cursor to the next (`+1`) or previous (`-1`) block, clamped to
    /// the ends; mark it and scroll it into view. No-op with no blocks. When the
    /// render is PAGINATED (synopsis / gloss result), steps the GLOBAL cursor
    /// across all blocks and turns the page at a boundary instead of scrolling.
    fn step_cursor(&self, delta: i32) {
        if self.paginated.get() {
            self.step_full_cursor(delta);
            return;
        }
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        let cur = self.cursor_block.get().min(len - 1) as i64;
        let next = (cur + delta as i64).clamp(0, len as i64 - 1);
        // No movement (already at the first/last block): do nothing. Re-running
        // scroll_cursor_into_view here would re-snap the viewport and recompute
        // the bottom clip every press, which reads as a visible "jiggle" when j
        // is held at the bottom (over-tall last block) — the cursor can't advance
        // but the scroll target keeps nudging.
        if next == cur {
            return;
        }
        self.cursor_block.set(next as usize);
        self.mark_cursor_block();
        self.scroll_cursor_into_view();
    }

    /// Jump the cursor to the first (`false`) or last (`true`) block; mark it and
    /// scroll it into view. Paginated: jumps the global cursor + turns the page.
    fn cursor_to_end(&self, last: bool) {
        if self.paginated.get() {
            self.full_cursor_to_end(last);
            return;
        }
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        self.cursor_block.set(if last { len - 1 } else { 0 });
        self.mark_cursor_block();
        self.scroll_cursor_into_view();
    }

    // ---- Pagination (synopsis + gloss-result) ----------------------------
    // Mirrors the journal overlay: `all_blocks` is the full list, `pages` the
    // ranges, `cursor_full` the global cursor; each page renders only its slice
    // so no partial block is shown at either edge.

    /// Step the global cursor by `delta` (clamped); turn the page if it leaves
    /// the current page, else just re-mark the page-local bar.
    fn step_full_cursor(&self, delta: i32) {
        let total = self.all_blocks.borrow().len();
        if total == 0 {
            return;
        }
        let cur = self.cursor_full.get().min(total - 1) as i64;
        let next = (cur + delta as i64).clamp(0, total as i64 - 1);
        if next == cur {
            return;
        }
        self.cursor_full.set(next as usize);
        self.sync_cursor_page();
    }

    fn full_cursor_to_end(&self, last: bool) {
        let total = self.all_blocks.borrow().len();
        if total == 0 {
            return;
        }
        self.cursor_full.set(if last { total - 1 } else { 0 });
        self.sync_cursor_page();
    }

    /// `x`/`y`: turn to the next/prev RENDER page of the current gloss, landing
    /// the cursor on the first block of that page — unlike `j`/`k` which step one
    /// block at a time. No-op at the first/last page (or when the gloss is a
    /// single page / has no blocks).
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
        let page_start = self.pages.borrow()[target_page as usize].start;
        self.cursor_full.set(page_start);
        self.sync_cursor_page();
    }

    /// After `cursor_full` moves: turn the page (re-render) if it now falls on a
    /// different page; otherwise re-mark the bar at the new page-local block.
    fn sync_cursor_page(&self) {
        let target_page = crate::ui::pagination::page_containing_block(
            &self.pages.borrow(),
            self.cursor_full.get(),
        );
        if target_page != self.page_idx.get() {
            self.page_idx.set(target_page);
            self.render_current_page();
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

    /// Usable viewport height for SYNOPSIS pagination — the `scroll_h`
    /// `size_scroll` pins (card − title chrome − footer − the 44px scroll_overlay
    /// margins) MINUS the gloss view's own top/bottom margins (24 + 80), which
    /// live INSIDE the scrolled viewport: a page packed to the full `scroll_h`
    /// renders `top_margin + content + bottom_margin` in a `scroll_h` viewport
    /// and overflows by up to 104px — the tail block ran flush off the card
    /// bottom (2H6 2.1.1–8 / 2.1.13–18, user-reported). The journal fixed the
    /// same class by subtracting its 28+28 view padding in `page_height`.
    /// Must be called after `size_scroll`.
    fn synopsis_page_height(&self) -> i32 {
        let (_, card_height) = self.last_card_size.get();
        const SCROLL_OVERLAY_MARGINS: i32 = 24 + 20;
        (card_height - self.title_pref_h() - self.footer_pref_h() - SCROLL_OVERLAY_MARGINS
            - self.gloss_view.top_margin()
            - self.gloss_view.bottom_margin())
        .max(80)
    }

    /// Usable viewport height for GLOSS-RESULT pagination. Same accounting as
    /// `synopsis_page_height` (gloss-result hides the title, so `title_pref_h`
    /// is 0; only the footer sits below the scroll). Must be called after
    /// `size_scroll`. Kept separate from `synopsis_page_height` for symmetry with
    /// the two render paths and so the gloss budget can diverge if needed.
    fn gloss_page_height(&self) -> i32 {
        self.synopsis_page_height()
    }

    /// Measure every block in `all_blocks` and pack them into `pages` by the
    /// usable viewport height. Verse Source blocks are over-measured
    /// (`gloss_block_height`) so a speaker label never clips at a page top.
    fn repaginate(&self, page_height: i32) {
        let blocks = self.all_blocks.borrow();
        if blocks.is_empty() {
            self.pages.borrow_mut().clear();
            return;
        }
        let family = self.font_family.borrow().clone();
        let size = self.font_size.get();
        // Measure each block at ITS OWN wrap width. A single mode-wide width is
        // wrong in gloss mode because the two block kinds render at different
        // indents: the quoted source verse hangs at `QUOTE_VERSE_INDENT` past
        // the bar while the explication prose (the bulk of a gloss) sits at
        // `QUOTE_BODY_INDENT`. Measuring explications at the verse's narrower
        // width over-estimated their heights ~11% and pushed units that fit
        // onto the next page (page underfill). Verse blocks keep the deep-indent
        // width — their speaker/stage lines render shallower, so that direction
        // still over-counts (safe; never clips). Synopsis prose sits at the
        // body margin.
        let card_w = self.last_card_size.get().0;
        let left = self.gloss_view.left_margin();
        // Speakerless source (prose): the verse renders at the shallower
        // QUOTE_SPEAKER_INDENT (populate_verse_buffer), so measure at that
        // width — and without the speaker-label reserve (block_height_overhead):
        // no heading renders, so the reserve only underfilled pages. When any
        // block carries a speaker, measure every Source at the deep verse indent
        // with the full reserve — a mixed page renders deep, and the doc-level
        // check only ever over-counts (never clips).
        let doc_has_speaker = self
            .gloss_block_markups
            .borrow()
            .iter()
            .any(|m| crate::ui::gloss_render::markup_has_displayed_speaker(m));
        // bar_left == left in gloss mode; the right margin is `left`.
        let wrap_for = |kind: BlockKind| -> i32 {
            let indent = match (self.paginated_mode.get(), kind) {
                (PaginatedMode::Gloss, BlockKind::Source) if doc_has_speaker => {
                    crate::ui::gloss_render::QUOTE_VERSE_INDENT
                }
                (PaginatedMode::Gloss, BlockKind::Source) => {
                    crate::ui::gloss_render::QUOTE_SPEAKER_INDENT
                }
                (PaginatedMode::Gloss, _) => crate::ui::gloss_render::QUOTE_BODY_INDENT,
                (PaginatedMode::Synopsis, _) => 0,
            };
            (card_w - 2 * left - indent).max(1)
        };
        let pctx = self.gloss_view.pango_context();
        // Real measured line-height at this font, used as the PER-BLOCK safety
        // headroom in `block_height_overhead`'s Explication path (mirrors
        // journal_overlay.rs's `text_h + line_h` per paragraph). Charging this
        // per block — rather than once off the whole page's budget — is
        // required because the real shortfall a flat/page-level estimate
        // misses (per-buffer-line `pixels_below_lines` + the blank-line
        // paragraph separator, neither modeled by a plain `pango::Layout`)
        // accumulates once PER BLOCK BOUNDARY. A single page-level margin
        // (subtract one `line_h` off `page_height`) is insufficient once a
        // page packs enough small blocks that the accumulated shortfall
        // exceeds that one line_h — exactly the TWWLN synopsis card (8 blocks:
        // 7 one-line metadata paragraphs + Gist) at production geometry
        // (2026-07 "Gist:" bug, confirmed to still clip after the page-level
        // fix in commit 4df9352). Per-block charging scales proportionally
        // with block count instead.
        let line_h = crate::ui::pagination::measure_text_height(&pctx, "Mg", size, &family, 200);
        let markups = self.gloss_block_markups.borrow();
        let heights: Vec<i32> = blocks
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let m = match self.paginated_mode.get() {
                    PaginatedMode::Gloss => markups.get(i).map(|s| s.as_str()),
                    PaginatedMode::Synopsis => None,
                };
                gloss_block_height(b, m, &pctx, &family, size, wrap_for(b.kind), doc_has_speaker, line_h)
            })
            .collect();
        drop(markups);
        // No additional page-level margin: the per-block `line_h` headroom
        // above already reserves proportional slack for every block on the
        // page (same as journal_overlay.rs, which packs against its raw
        // `page_height()` with no extra page-level subtraction). Stacking a
        // page-level margin on top of the now-correct per-block charge would
        // only re-introduce the flat-margin under-scaling this fix replaces,
        // for no additional safety.
        let budget = page_height.max(1);
        let pages = match self.paginated_mode.get() {
            // Gloss: keep each gloss together — a Source (speaker+verse) block and
            // the Explication(s) that follow it form one indivisible unit, so a
            // page break never orphans an explication onto the next page (the
            // "don't orphan a gloss" rule). A unit starts at each Source block; an
            // Explication attaches to the preceding unit.
            PaginatedMode::Gloss => {
                let group_start: Vec<bool> = blocks
                    .iter()
                    .map(|b| b.kind == BlockKind::Source)
                    .collect();
                crate::ui::pagination::paginate_grouped(&heights, &group_start, budget)
            }
            // Synopsis: every paragraph is its own unit.
            PaginatedMode::Synopsis => {
                crate::ui::pagination::paginate(&heights, budget)
            }
        };
        drop(blocks);
        *self.pages.borrow_mut() = pages;
    }

    /// Re-render the CURRENT page's block slice, re-derive the page-local block
    /// ranges + bar, pin the vadjustment at 0, and re-apply the font. Dispatches
    /// by `paginated_mode`: synopsis prose via `render_synopsis_page`, gloss-result
    /// verse via `render_gloss_page` (which re-renders the page's markup slice
    /// through `populate_gloss_buffer`).
    fn render_current_page(&self) {
        match self.paginated_mode.get() {
            PaginatedMode::Synopsis => self.render_synopsis_page(),
            PaginatedMode::Gloss => self.render_gloss_page(),
        }
    }

    /// Render the current synopsis page into the buffer. Single-page case renders
    /// the FULL original synopsis (labels included, exactly as before pagination).
    /// Multi-page case renders only the page's cursor-stop blocks' display text
    /// joined by blank lines (inter-page label paragraphs are dropped on
    /// paginated pages — a minor synopsis-only tradeoff for never clipping a
    /// block). Re-derives the page-local block ranges + bar; vadjustment pinned 0.
    fn render_synopsis_page(&self) {
        let buffer = self.gloss_view.buffer();
        let pages = self.pages.borrow();
        let n_pages = pages.len();
        let single_page = n_pages <= 1;
        let pidx = self.page_idx.get().min(n_pages.saturating_sub(1));
        let page = pages.get(pidx).copied();
        drop(pages);

        if single_page {
            // Common case: the whole synopsis fits — render it verbatim (labels
            // intact), then re-derive blocks from synopsis_blocks of the source.
            let synopsis = self.current_synopsis.borrow().clone();
            let (text, label_ranges, hi_ranges) = render_synopsis_with_labels(&synopsis);
            buffer.set_text(&text);
            *self.synopsis_label_ranges.borrow_mut() = label_ranges;
            *self.hi_ranges.borrow_mut() = hi_ranges;
            self.apply_synopsis_label_bold();
            self.apply_hi_color();
            self.rebuild_block_ranges_from(crate::ui::gloss_block::synopsis_blocks(&synopsis));
        } else {
            // Paginated: render this page's blocks, each preceded by its lead
            // label(s) (bolded) so labels survive the page turn. Track label
            // char-offset ranges in the page text so apply_synopsis_label_bold
            // can bold them.
            let Some(page) = page else { return };
            let all = self.all_blocks.borrow();
            let slice: Vec<GlossBlock> = all[page.start..page.end.min(all.len())].to_vec();
            drop(all);
            let mut body = String::new();
            let mut label_ranges: Vec<(usize, usize)> = Vec::new();
            let mut hi_ranges: Vec<(usize, usize)> = Vec::new();
            let mut char_off = 0usize; // char offset into `body`
            for b in &slice {
                for a in &b.attached {
                    // Plain irrefutable `let`: Attachment currently has the single
                    // LeadLabel variant (gloss echoes ride in the markup string, not
                    // here), so `if let`/`let-else` would be flagged irrefutable.
                    let crate::ui::gloss_block::Attachment::LeadLabel(lbl) = a;
                    if !body.is_empty() {
                        body.push_str("\n\n");
                        char_off += 2;
                    }
                    let len = lbl.chars().count();
                    label_ranges.push((char_off, char_off + len));
                    body.push_str(lbl);
                    char_off += len;
                }
                if !body.is_empty() {
                    body.push_str("\n\n");
                    char_off += 2;
                }
                // `b.display` is IPA-stripped but may still carry `<hi>` tags;
                // strip them and shift the highlight ranges into `body`.
                let (clean, hi) = crate::ui::gloss_block::strip_hi_spans(&b.display);
                let len = clean.chars().count();
                for (s, e) in hi {
                    hi_ranges.push((char_off + s, char_off + e));
                }
                body.push_str(&clean);
                char_off += len;
            }
            buffer.set_text(&body);
            *self.synopsis_label_ranges.borrow_mut() = label_ranges;
            *self.hi_ranges.borrow_mut() = hi_ranges;
            self.apply_synopsis_label_bold();
            self.apply_hi_color();
            self.rebuild_block_ranges_from(slice);
        }

        // Floating page marker (⌄ more / • end), bottom-center of the viewport.
        self.update_page_marker(pidx, n_pages);

        // Pin the viewport at the top — the page fits, nothing scrolls.
        let adj = self.gloss_scrolled.vadjustment();
        adj.set_value(adj.lower());

        // Project the global cursor onto this page + mark the bar.
        let page_start = page.map(|p| p.start).unwrap_or(0);
        let page_local = self
            .cursor_full
            .get()
            .saturating_sub(page_start)
            .min(self.blocks.borrow().len().saturating_sub(1));
        self.cursor_block.set(page_local);
        self.apply_font();
        self.mark_cursor_block();
        self.bar_drawing.queue_draw();
        self.update_bottom_clip();
        self.update_position_label();
    }

    /// Render the current GLOSS-RESULT page into the buffer via
    /// `populate_gloss_buffer`. Single-page case renders the FULL original gloss
    /// markup (echo brackets + pron notes intact, exactly as before pagination).
    /// Multi-page case renders only this page's cursor-stop blocks by joining their
    /// ORIGINAL markup (`gloss_block_markups`) — NOT `GlossBlock.display`, which
    /// drops the speaker headings + verse tags the render needs. Re-derives the
    /// page-local block ranges + bar; vadjustment pinned at 0 so the accent bar +
    /// line-number gutter are correct (`populate_gloss_buffer` rebuilds them at
    /// scroll 0). Source blocks were over-measured so a speaker label is never
    /// clipped at a page top.
    fn render_gloss_page(&self) {
        let bar_left = *self.bar_x.borrow();
        let line_numbers = self.gloss_source_line_numbers.borrow().clone();

        let pages = self.pages.borrow();
        let n_pages = pages.len();
        let single_page = n_pages <= 1;
        let pidx = self.page_idx.get().min(n_pages.saturating_sub(1));
        let page = pages.get(pidx).copied();
        drop(pages);

        // Build the markup to render: the whole gloss when it fits one page, else
        // only this page's blocks' markup slice.
        let (markup, page_blocks): (String, Vec<GlossBlock>) = if single_page {
            let gloss = self.current_gloss.borrow().clone();
            (gloss.clone(), gloss_blocks(&gloss))
        } else {
            let Some(page) = page else { return };
            let markups = self.gloss_block_markups.borrow();
            let all = self.all_blocks.borrow();
            let end = page.end.min(markups.len()).min(all.len());
            let start = page.start.min(end);
            let body = markups[start..end].join("\n");
            let slice: Vec<GlossBlock> = all[start..end].to_vec();
            (body, slice)
        };

        // Speaker + verse header render FULL ink like the explication — no
        // dim color (the user found the dimmed source too recessed). The
        // bold/small-caps/0.9-scale header styling and the verse hang-indent
        // alone set the quoted source apart from the prose.
        let (ranges, _nums) = populate_gloss_buffer(
            &self.gloss_view,
            &markup,
            self.text_margins,
            bar_left,
            &line_numbers,
            None,
        );
        *self.bar_ranges.borrow_mut() = ranges;
        // Glosses do not show verse line numbers (those belong only to the main
        // reading view); clear any the buffer produced.
        self.line_numbers.borrow_mut().clear();
        self.synopsis_label_ranges.borrow_mut().clear();        self.hi_ranges.borrow_mut().clear();
        self.rebuild_block_ranges_from(page_blocks);

        // Floating page marker (⌄ more / • end), bottom-center of the viewport.
        self.update_page_marker(pidx, n_pages);

        // Pin the viewport at the top — the page fits, nothing scrolls.
        let adj = self.gloss_scrolled.vadjustment();
        adj.set_value(adj.lower());

        // Project the global cursor onto this page + mark the bar.
        let page_start = page.map(|p| p.start).unwrap_or(0);
        let page_local = self
            .cursor_full
            .get()
            .saturating_sub(page_start)
            .min(self.blocks.borrow().len().saturating_sub(1));
        self.cursor_block.set(page_local);
        self.apply_font();
        self.mark_cursor_block();
        self.bar_drawing.queue_draw();
        self.update_bottom_clip();
        self.update_position_label();
    }

    /// Set the floating page marker for the current gloss/synopsis page: `⌄` when
    /// another page follows, `•` on the last page, hidden on single-page content.
    /// The marker is an overlay child floating just BELOW the page's last block
    /// (NOT in the text flow), so it shows even when the page is full. Glyph
    /// chosen by the shared `pagination::page_marker`. Mirrors
    /// `JournalOverlay::update_page_marker`.
    ///
    /// The glyph is stored for the bar's draw func and the bar repainted; the draw
    /// func reads live line geometry each paint, so there is no allocation race.
    fn update_page_marker(&self, page_idx: usize, n_pages: usize) {
        *self.marker_glyph.borrow_mut() = crate::ui::pagination::page_marker(page_idx, n_pages);
        self.bar_drawing.queue_draw();
        // Also repaint after the next layout pass (the page turn's reflow) so the
        // glyph lands at the new last line even when the scroll range is unchanged.
        let bar = self.bar_drawing.clone();
        glib::idle_add_local_once(move || bar.queue_draw());
    }

    /// Enter synopsis visual mode: anchor at the current block. No-op if there
    /// are no blocks. Returns true if mode was entered.
    pub fn enter_visual(&self) -> bool {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return false;
        }
        let cur = self.cursor_block.get().min(len - 1);
        self.synopsis_visual_anchor.set(Some(cur));
        self.refresh_selection_bar();
        true
    }

    /// Exit synopsis visual mode: clear the anchor and redraw the bar as the
    /// single cursor block.
    pub fn exit_visual(&self) {
        self.synopsis_visual_anchor.set(None);
        self.mark_cursor_block();
    }

    /// Exit visual mode collapsing the cursor to the START of the selection
    /// (the lower of anchor/cursor), then redraw the bar as that single block.
    /// Used by the gloss `y` yank so the cursor lands on the first selected
    /// block rather than wherever the moving end finished.
    pub fn exit_visual_to_start(&self) {
        if let Some(anchor) = self.synopsis_visual_anchor.get() {
            let (start, _) = visual_block_range(anchor, self.cursor_block.get());
            self.cursor_block.set(start);
        }
        self.synopsis_visual_anchor.set(None);
        self.mark_cursor_block();
    }

    /// Exit visual mode returning the cursor to the ANCHOR block — the block the
    /// cursor was on when visual mode was entered (Shift+V). Used by Escape/V so
    /// cancelling a selection lands the cursor back where it started, rather than
    /// at the moving (j/k) end. Unlike `exit_visual_to_start`, the anchor may be
    /// the HIGHER of anchor/cursor (when the selection was extended upward).
    pub fn exit_visual_to_anchor(&self) {
        if let Some(anchor) = self.synopsis_visual_anchor.get() {
            self.cursor_block.set(anchor);
        }
        self.synopsis_visual_anchor.set(None);
        self.mark_cursor_block();
    }

    /// Move the cursor end of the selection by `delta` blocks (clamped) and
    /// re-span the bar. Used by j/k while in visual mode.
    pub fn visual_step(&self, delta: i32) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        let cur = self.cursor_block.get().min(len - 1) as i64;
        let next = (cur + delta as i64).clamp(0, len as i64 - 1) as usize;
        self.cursor_block.set(next);
        self.refresh_selection_bar();
        self.scroll_cursor_into_view();
    }

    /// Move the cursor end of the selection to the first (`false`) or last
    /// (`true`) block and re-span the bar. Used by gg/G while in visual mode.
    pub fn visual_to_end(&self, last: bool) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        self.cursor_block.set(if last { len - 1 } else { 0 });
        self.refresh_selection_bar();
        self.scroll_cursor_into_view();
    }

    /// The currently-selected paragraphs' text (blank-line joined), for yank.
    pub fn visual_selection_text(&self) -> String {
        let anchor = match self.synopsis_visual_anchor.get() {
            Some(a) => a,
            None => return String::new(),
        };
        let cursor = self.cursor_block.get();
        let syn = self.current_synopsis.borrow();
        selected_blocks_text(&syn, anchor, cursor)
    }

    /// Number of blocks currently selected (for the log line).
    pub fn visual_selection_len(&self) -> usize {
        visual_selection_count(self.synopsis_visual_anchor.get(), self.cursor_block.get())
    }

    /// Set the synopsis-overlay footer hint (normal navigation).
    pub fn set_synopsis_hint(&self) {
        self.hint.set_text("");
    }

    /// Set the footer hint shown while synopsis visual mode is active.
    pub fn set_synopsis_visual_hint(&self) {
        self.hint.set_text("\u{21e7}V/Esc exit · j/k extend · gg/G ends · y yank");
    }

    /// Set the gloss-overlay footer hint (normal navigation). Called by the
    /// gloss render path and when exiting gloss visual mode, so both share one
    /// string. `\u{21e7}V select` advertises gloss visual mode.
    pub fn set_gloss_hint(&self) {
        self.hint.set_text("");
    }

    /// Set the footer hint shown while gloss visual mode is active.
    pub fn set_gloss_visual_hint(&self) {
        self.hint.set_text("\u{21e7}V/Esc exit · j/k extend · gg/G ends · y yank");
    }

    /// The currently-selected blocks' text read straight from the gloss buffer
    /// (first selected block's start line through the last block's end line),
    /// for yank in gloss visual mode. Unlike `visual_selection_text` (synopsis),
    /// this does not use `current_synopsis`; it copies the full block text —
    /// source verse plus its gloss — exactly as displayed.
    pub fn visual_selection_buffer_text(&self) -> String {
        let anchor = match self.synopsis_visual_anchor.get() {
            Some(a) => a,
            None => return String::new(),
        };
        let (start_idx, end_idx) = visual_block_range(anchor, self.cursor_block.get());
        let ranges = self.blocks.borrow();
        let buffer = self.gloss_view.buffer();
        // Read each block as its own contiguous span (internal verse-line
        // newlines preserved) and join blocks with a blank line, matching the
        // synopsis yank's `\n\n` paragraph separation.
        let mut blocks: Vec<String> = Vec::new();
        for r in ranges.iter().skip(start_idx).take(end_idx.saturating_sub(start_idx) + 1) {
            let start = match buffer.iter_at_line(r.start_line) {
                Some(it) => it,
                None => continue,
            };
            let mut end = match buffer.iter_at_line(r.end_line) {
                Some(it) => it,
                None => continue,
            };
            if !end.ends_line() {
                end.forward_to_line_end();
            }
            blocks.push(buffer.text(&start, &end, false).to_string());
        }
        blocks.join("\n\n")
    }

    /// Scroll the viewport so the selected cursor block is visible. Only scrolls
    /// when the block falls outside the current viewport: brings its top into
    /// view (with a small pad) if above, or its bottom into view if below. The
    /// scroll target is decided by the pure `cursor_scroll_target` helper.
    fn scroll_cursor_into_view(&self) {
        let (start_line, end_line) = {
            let ranges = self.blocks.borrow();
            let i = self.cursor_block.get().min(ranges.len().saturating_sub(1));
            match ranges.get(i) {
                Some(r) => (r.start_line, r.end_line),
                None => return,
            }
        };
        let buffer = self.gloss_view.buffer();
        let top_margin = self.gloss_view.top_margin() as f64;
        let block_top = match buffer.iter_at_line(start_line) {
            Some(it) => self.gloss_view.line_yrange(&it).0 as f64 + top_margin,
            None => return,
        };
        let block_bottom = match buffer.iter_at_line(end_line) {
            Some(it) => {
                let (y, h) = self.gloss_view.line_yrange(&it);
                (y + h) as f64 + top_margin
            }
            None => block_top,
        };

        let adj = self.gloss_scrolled.vadjustment();
        let view_top = adj.value();
        let view_bottom = view_top + adj.page_size();
        let max_value = (adj.upper() - adj.page_size()).max(adj.lower());
        let pad = 24.0;

        let new_value = match cursor_scroll_target(&CursorScrollGeom {
            block_top,
            block_bottom,
            view_top,
            view_bottom,
            page_size: adj.page_size(),
            lower: adj.lower(),
            max_value,
            pad,
        }) {
            Some(v) => v,
            None => return, // already fully visible
        };
        // Snap the viewport top to a whole visual row so the first line is not
        // clipped under the title rule (the top edge has no clip box).
        //
        // Direction matters. When we are revealing a block's BOTTOM (it ended
        // below the fold), flooring the top to the nearest row *below* the
        // target scrolls the viewport back UP and re-hides the bottom we just
        // tried to show — the exact bug where the last explication clipped:
        // target 514 floored to row-top 450, losing 64px. For that case snap
        // UP to the nearest whole row at/above the target (clamped to the scroll
        // ceiling), which keeps the bottom in view. Otherwise (revealing a top)
        // floor as before so the revealed top isn't pushed under the title.
        let revealing_bottom = block_bottom > view_bottom - pad && block_top >= view_top + pad;
        let new_value = if revealing_bottom {
            self.snap_value_to_line_up(new_value)
        } else {
            self.snap_value_to_line(new_value)
        };
        adj.set_value(new_value);
        self.update_bottom_clip();
        self.bar_drawing.queue_draw();
    }

    /// Bar start for a block span: walk upward over any speaker heading line(s)
    /// directly above `start_line`, so the accent bar beside a Source block also
    /// covers its speaker label. Block ranges themselves start at the first
    /// verse line (`gloss_blocks` drops speaker tags from `display`, so
    /// `rebuild_block_ranges_from` can't match the label); extending only the
    /// DRAWN span keeps navigation/TTS block semantics untouched. Same
    /// tag-sniffing `color_audio_blocks` uses to color the label with its turn.
    fn bar_start_with_speaker(&self, start_line: i32) -> i32 {
        let buffer = self.gloss_view.buffer();
        let mut line = start_line;
        while line > 0 && line_is_speaker(&buffer, line - 1) {
            line -= 1;
        }
        line
    }

    /// Move the left accent bar to the selected cursor block and repaint. No-op
    /// when there are no blocks. Logs the landing block so j/k/gg/G navigation
    /// stays verifiable from the dev log.
    fn mark_cursor_block(&self) {
        let (kind, index) = match self.current_block() {
            Some(t) => t,
            None => return,
        };
        let span = self
            .blocks
            .borrow()
            .iter()
            .find(|r| r.kind == kind && r.index == index)
            .map(|r| (r.start_line, r.end_line));
        if let Some((start_line, end_line)) = span {
            // Extend BEFORE logging so the logged range matches the drawn bar
            // (the dev-log/screen agreement the debug workflow relies on).
            let start_line = self.bar_start_with_speaker(start_line);
            crate::log_fmt!(
                "GLOSS-CURSOR: cursor#{} -> {:?}#{} bar lines [{}, {}]",
                self.cursor_block.get(), kind, index, start_line, end_line
            );
            *self.bar_ranges.borrow_mut() = vec![BarRange { start_line, end_line }];
            self.bar_drawing.queue_draw();
        }
    }

    /// Redraw the left bar. In visual mode (anchor set) the bar spans every
    /// selected block (`anchor..=cursor`); otherwise it marks the single cursor
    /// block. Safe to call in both modes.
    fn refresh_selection_bar(&self) {
        let anchor = match self.synopsis_visual_anchor.get() {
            Some(a) => a,
            None => {
                self.mark_cursor_block();
                return;
            }
        };
        let blocks = self.blocks.borrow();
        if blocks.is_empty() {
            return;
        }
        let last = blocks.len() - 1;
        let cursor = self.cursor_block.get().min(last);
        let (s, e) = visual_block_range(anchor.min(last), cursor);
        let start_line = blocks[s].start_line;
        let end_line = blocks[e].end_line;
        drop(blocks);
        let start_line = self.bar_start_with_speaker(start_line);
        *self.bar_ranges.borrow_mut() = vec![BarRange { start_line, end_line }];
        self.bar_drawing.queue_draw();
    }

    /// Approximate height of one line of gloss text, derived from the view's
    /// font. Used ONLY as the per-press *step distance* for `scroll_gloss`
    /// (how far one j/k moves before snapping) — never as a snapping grid, since
    /// real wrapped rows vary in height (per-tag `pixels_above_lines`/`scale`).
    /// The font is read from the `font-size` tag when present to dodge the GTK
    /// CSS-application race that returns the previous font's metrics for one
    /// frame after a font change (see `viewport::descender_guard_px`).
    fn row_step(&self) -> f64 {
        let ctx = self.gloss_view.pango_context();
        let font_desc = self
            .gloss_view
            .buffer()
            .tag_table()
            .lookup("font-size")
            .and_then(|tag| tag.font_desc());
        let metrics = ctx.metrics(font_desc.as_ref(), None);
        let ascent = metrics.ascent() as f64 / pango::SCALE as f64;
        let descent = metrics.descent() as f64 / pango::SCALE as f64;
        let line = ascent + descent;
        (line + self.gloss_view.pixels_below_lines() as f64).max(12.0)
    }

    /// Snap a scroll value to the greatest *real* visual-row top at or below
    /// `target_y`, clamped to `[lower, upper - page_size]`, so the viewport top
    /// aligns to a whole wrapped row (no half row clipped under the title rule).
    ///
    /// Uses actual layout (`display_rows`) rather than a fixed row height,
    /// because the gloss/synopsis buffers apply per-tag `pixels_above_lines` and
    /// `scale`, making rows non-uniform. If the snapped boundary would exceed
    /// the scroll ceiling, the clamp pulls it back to `max_value`; an
    /// uncovered partial row there is hidden by the bottom clip.
    fn snap_value_to_line(&self, target_y: f64) -> f64 {
        let adj = self.gloss_scrolled.vadjustment();
        let lower = adj.lower();
        let max_value = (adj.upper() - adj.page_size()).max(lower);
        let target = target_y.clamp(lower, max_value);
        // Greatest real row top <= target.
        let mut best = lower;
        for (row_top, _row_bottom) in crate::ui::display_rows(&self.gloss_view) {
            if row_top <= target + 0.5 {
                best = best.max(row_top);
            } else {
                break;
            }
        }
        best.clamp(lower, max_value)
    }

    /// Snap a scroll value to the *least* real visual-row top at or above
    /// `target_y`, clamped to `[lower, max_value]`. The up-direction counterpart
    /// of `snap_value_to_line`, used when revealing a block's BOTTOM: flooring
    /// (the default) would scroll the viewport back up and re-hide the bottom we
    /// are trying to show. Snapping UP keeps a whole row at the top (no half row
    /// under the title rule) while never giving back the reveal. If no row top
    /// is >= target (target sits past the last row top but within the scroll
    /// ceiling), use `max_value` so the document end is reached.
    fn snap_value_to_line_up(&self, target_y: f64) -> f64 {
        let adj = self.gloss_scrolled.vadjustment();
        let lower = adj.lower();
        let max_value = (adj.upper() - adj.page_size()).max(lower);
        let row_tops: Vec<f64> = crate::ui::display_rows(&self.gloss_view)
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        snap_up_to_row(target_y, &row_tops, lower, max_value)
    }

    /// Reveal the stacked input card below the open synopsis/gloss card with the
    /// given heading and footer hint. Shared by the synopsis "ask" flow and the
    /// gloss add/edit prompts. The host shrinks the scroll viewport so the
    /// synopsis/gloss text ends ABOVE the ask card (the occlusion fix) and
    /// recomputes the clip; apply_font re-fonts the now-visible input.
    pub fn open_ask_card_with(&self, title: &str, hint: &str, block_fill: &str, block_fg: &str) {
        self.ask_host.open(title, hint, block_fill, block_fg);
        self.apply_font();
    }

    /// Hide the ask card and return focus + highlight to the synopsis. The host
    /// restores the scroll's stored closed height and recomputes the clip.
    pub fn close_ask_card(&self) {
        self.ask_host.close();
    }

    /// Read and clear the ask input's text.
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

    pub fn ask_is_open(&self) -> bool {
        self.ask_host.is_open()
    }

    pub fn show_loading(&self) {
        self.show_loading_message("Glossing...");
    }

    pub fn show_loading_message(&self, message: &str) {
        self.synopsis_label_ranges.borrow_mut().clear();        self.hi_ranges.borrow_mut().clear();
        self.blocks.borrow_mut().clear();
        self.paginated.set(false);
        *self.marker_glyph.borrow_mut() = None;
        // Size the card to the full reading area so the loading state reads as a
        // proper card (the same footprint the synopsis/gloss card will occupy)
        // rather than a label-sized box. Reuse the last card geometry; fall back
        // to the construction width if no card has been shown yet this session.
        let (cw, ch) = self.last_card_size.get();
        if cw > 0 {
            self.container.set_width_request(cw);
        }
        if ch > 0 {
            self.container.set_height_request(ch);
        }
        self.title.set_text(message);
        self.set_gloss_title_style();
        self.title.set_visible(true);
        self.title.set_vexpand(true);
        self.title.set_valign(Align::Center);
        self.title.set_halign(Align::Center);
        self.title.set_margin_start(0);
        self.hide_diff_labels();
        self.gloss_scroll_overlay.set_visible(false);
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);
        self.position_label.set_visible(false);
        self.ask_host.card().close();
        self.hint.set_visible(false);
        // Show the dim scrim so the loading state reads as a modal card over the
        // page, consistent with the synopsis/gloss cards (was hidden, which made
        // the message float as bare text with no backdrop).
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    pub fn scroll_gloss(&self, delta: i32) {
        let adj = self.gloss_scrolled.vadjustment();
        // Step by ~3 line-heights per press, then snap to a real visual-row
        // boundary so no partial row is left clipped at the viewport top.
        // `row_step` is only the step distance; the snap aligns to actual rows.
        let step = self.row_step();
        let raw_target = adj.value() + step * 3.0 * delta as f64;
        // ALWAYS snap the viewport top to a whole visual row — the top edge has
        // no clip box, so any fractional top reads as a half-line clipped under
        // the title rule. We do NOT shortcut to an unsnapped `max_value` to
        // "reveal the last row" the way the old code did: when the content
        // overflows by less than a full line (max_value < line height), that
        // shortcut left the top fractional (the visible top-clip bug). The
        // bottom-clip box masks whatever partial row remains at the viewport
        // bottom of the snapped last page, so the last partial line is hidden
        // cleanly rather than shown clipped — whole rows only, like the main
        // reading card.
        let target = self.snap_value_to_line(raw_target);
        adj.set_value(target);
        self.update_bottom_clip();
        self.bar_drawing.queue_draw();
        self.mark_cursor_block();
    }

    pub fn scroll_gloss_to_top(&self) {
        let adj = self.gloss_scrolled.vadjustment();
        adj.set_value(adj.lower());
        self.update_bottom_clip();
        self.bar_drawing.queue_draw();
        self.mark_cursor_block();
    }

    pub fn scroll_gloss_to_bottom(&self) {
        // Snap the top to a whole row at the bottom of the document. The raw
        // `upper - page_size` would land the top on a fractional row (clipped
        // under the title rule, since the top has no clip box); snapping floors
        // it to the greatest whole-row top that still shows the most content.
        // The bottom-clip box masks any partial row left at the viewport bottom,
        // so the document end reads as whole rows, not a clipped top + clipped
        // bottom.
        let adj = self.gloss_scrolled.vadjustment();
        let bottom = (adj.upper() - adj.page_size()).max(adj.lower());
        adj.set_value(self.snap_value_to_line(bottom));
        self.update_bottom_clip();
        self.bar_drawing.queue_draw();
        self.mark_cursor_block();
    }

    /// Re-show the overlay widget after it was hidden by `hide()` without
    /// rebuilding its content (the rendered blocks + cursor persist in the
    /// widget). Used when returning from the Ctrl+/ keybinds overlay, which
    /// hides the gloss/synopsis overlay on open. The ask card stays hidden —
    /// `hide()` reset it — matching a fresh navigation state.
    pub fn show_again(&self) {
        self.container.set_visible(true);
        self.scrim.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
        self.scrim.set_visible(false);
        // Reset the ask card so it never re-shows stale when the overlay reopens.
        self.ask_host.card().close();
    }

    pub fn set_position(&self, index: usize, total: usize) {
        self.gloss_pos.set((index, total));
        self.update_position_label();
    }

    /// Refresh the footer's right label to the bare render-page counter
    /// ("X / Y", hidden on a single page). Call after a (re)render / page turn —
    /// the page count drives the label. The cross-gloss index no longer appears
    /// here, but `set_position` still calls this so the label refreshes whenever
    /// the displayed gloss (and its page set) changes.
    fn update_position_label(&self) {
        // Footer right label shows ONLY the render-page counter as a bare
        // "X / Y" (no "page" word, no cross-gloss index). Hidden on a single
        // page. The page token is computed by the shared `pagination::page_token`
        // so the gloss/synopsis/journal footers stay in sync.
        let n_pages = self.pages.borrow().len();
        match self
            .paginated
            .get()
            .then(|| crate::ui::pagination::page_token(self.page_idx.get(), n_pages))
            .flatten()
        {
            Some(token) => {
                self.position_label.set_text(&token);
                self.position_label.set_visible(true);
            }
            None => self.position_label.set_visible(false),
        }
    }

    /// Show the open passage's citation range in the footer (gloss view only),
    /// e.g. "2H6 1.4.7–14". Pass the passage's start and end citation strings.
    /// Hidden when no usable citation is given.
    pub fn set_citation(&self, start_citation: &str, end_citation: &str) {
        match format_citation_range(start_citation, end_citation) {
            Some(text) => {
                self.citation_label.set_text(&text);
                self.citation_label.set_visible(true);
            }
            None => self.citation_label.set_visible(false),
        }
    }

    /// Hide the footer citation (non-gloss views: synopsis, diff, echoes).
    ///
    /// Blank the text but keep the label *visible* so its `hexpand` still holds
    /// the footer row's stretch. If it were `set_visible(false)`, the only
    /// hexpand child would vanish and the right-aligned `hint` would collapse to
    /// the left edge — the synopsis footer would then sit bottom-left instead of
    /// far-right like the gloss/journal overlays. (Journal keeps its empty
    /// `footer_left` visible for the same reason.)
    pub fn hide_citation(&self) {
        self.citation_label.set_text("");
        self.citation_label.set_visible(true);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }
}

/// True when `line` of `buffer` is rendered as a speaker heading — i.e. its
/// first character carries one of the speaker tags (`gloss-speaker`,
/// `gloss-speaker-first`, `gloss-speaker-source`). Used by `color_audio_blocks`
/// to decide whether a cached Source block should also recolor the heading on
/// the line above its first verse line.
fn line_is_speaker(buffer: &gtk4::TextBuffer, line: i32) -> bool {
    let iter = match buffer.iter_at_line(line) {
        Some(it) => it,
        None => return false,
    };
    iter.tags().iter().any(|t| {
        matches!(
            t.name().as_deref(),
            Some("gloss-speaker") | Some("gloss-speaker-first") | Some("gloss-speaker-source")
        )
    })
}

// ---------------------------------------------------------------------------
// Block height helpers for gloss overlay pagination
// ---------------------------------------------------------------------------

/// Conservative overhead for a Source (verse) block: the speaker heading's 36px
/// `pixels_above_lines` + its scale-0.9 label + 10px `pixels_below_lines`, plus
/// slack. OVER-estimate so a multi-line speech never clips its speaker label at a
/// page top. Tied to gloss_render.rs `gloss-speaker` (pixels_above_lines 36,
/// scale 0.9, pixels_below_lines 10) — the header restyle raised the label from
/// scale 0.75 (→ ~14pt) to 0.9 (→ ~17pt) and added the 10px gap, so this budget
/// grew from 56 to 72 to keep the over-estimate conservative.
const SPEAKER_BLOCK_OVERHEAD: i32 = 72;

/// Conservative per-block height overhead. For Source (verse) blocks WITH a
/// speaker heading, the heading carries `pixels_above_lines(36)` + a `scale(0.9)`
/// label + a 10px `pixels_below_lines` gap that a plain `pango::Layout` never
/// models, so we must over-estimate. A SPEAKERLESS source (prose gloss) renders
/// no heading and no per-line verse gaps — plain wrapped lines — so charging the
/// speaker reserve there only underfills pages (the "why is this 2 pages?"
/// artifact): it pays the paragraph pad like an explication instead.
///
/// For Explication (prose/synopsis) blocks we charge `text_h + line_h` — ONE
/// real measured line-height per block, mirroring journal_overlay.rs's
/// `repaginate` (`text_h + line_h` per paragraph, "so packing can never
/// under-count and clip a paragraph tail"). This REPLACES the former flat
/// `PROSE_PAD = 16px` constant, which under-charged the real per-block
/// trailing gap GTK renders (`pixels_below_lines` applies once per BUFFER LINE,
/// and each block plus each blank-line separator is its own buffer line — a
/// flat, non-font-scaled 16px never modeled that). Because this overhead is
/// now charged once PER BLOCK rather than once per PAGE, a page packing many
/// small blocks (e.g. a prose synopsis's one-line metadata paragraphs before
/// the Gist section) accumulates proportional headroom instead of a single
/// fixed page-level margin — the page-level `safe_budget` margin this file
/// used before (one `line_h` off the whole page, regardless of block count)
/// was insufficient exactly because it didn't scale with block count (2026-07
/// TWWLN Ch.1 "Gist:" bug: 8 blocks per page, shortfall > 1 line_h). NEVER
/// under-estimate a source-with-speaker block: too-tall just gives it its own
/// page; too-small clips the speaker label.
fn block_height_overhead(is_source: bool, has_speaker: bool, text_h: i32, line_h: i32) -> i32 {
    if is_source && has_speaker {
        // verse lines carry per-line gaps too -> 1.15 slack on the text height.
        (text_h as f32 * 1.15) as i32 + SPEAKER_BLOCK_OVERHEAD
    } else {
        text_h + line_h
    }
}

/// Pixel height of `block` when rendered in the gloss overlay, using a
/// conservative over-estimate for Source (verse) blocks to prevent the speaker
/// label from being clipped at a page boundary.
///
/// Calls `crate::ui::pagination::measure_text_height` for the raw text height,
/// then adds the appropriate overhead via `block_height_overhead`.
fn gloss_block_height(
    block: &GlossBlock,
    markup: Option<&str>,
    pctx: &pango::Context,
    family: &str,
    size_pt: i32,
    wrap_w: i32,
    has_speaker: bool,
    line_h: i32,
) -> i32 {
    // Leaded: the gloss view renders with `pixels_inside_wrap`
    // (ui::OVERLAY_LINE_LEADING), so wrap heights must charge the same.
    let text_h = crate::ui::pagination::measure_text_height_leaded(
        pctx, &block.display, size_pt, family, wrap_w,
    );
    let mut h = block_height_overhead(block.kind == BlockKind::Source, has_speaker, text_h, line_h);
    let line = size_pt + size_pt / 2;
    // Synopsis: lead label paragraph(s) ride ABOVE the block body (in `attached`).
    // Plain irrefutable `let`: Attachment has the single LeadLabel variant.
    for a in &block.attached {
        let crate::ui::gloss_block::Attachment::LeadLabel(s) = a;
        h += crate::ui::pagination::measure_text_height_leaded(pctx, s, size_pt, family, wrap_w)
            + line;
    }
    // Gloss: a trailing echo lives in the block's MARKUP (A3), not in `display`.
    // Each `<gloss>[...]</gloss>` echo renders as a quote line + a citation line;
    // reserve room (over-measure) so a paginated page never clips it. Count the
    // echo `<gloss>` tags in the markup beyond the block's own content.
    if let Some(m) = markup {
        for seg in m.split("<gloss>").skip(1) {
            let inner = seg.split("</gloss>").next().unwrap_or("").trim();
            // Only an echo bracket adds height beyond block.display; the
            // block's own explication is already in `display`.
            if inner.starts_with('[') {
                h += crate::ui::pagination::measure_text_height_leaded(
                    pctx, inner, size_pt, family, wrap_w,
                ) + line * 2;
            }
        }
    }
    h
}

#[cfg(test)]
mod apply_font_priority_tests {
    use super::*;
    use gtk4::prelude::*;

    /// After `show_glossing` (which renders a stage line then calls `apply_font`),
    /// the italic `gloss-stage` tag must outrank the buffer-wide `gloss-font` tag,
    /// or the font tag's regular (upright) style flattens the stage italic — the
    /// "stage directions not italic in the overlay" bug. Mirrors the priority
    /// dance the main reader does for its italic translation tag (src/app/font.rs)
    /// and the overlay already does for synopsis-label bold / audio-cached color.
    // #[ignore]: needs gtk4::init(), which panics if a second GTK-init test runs
    // in the same process ("init from two different threads"). Run the GTK-init
    // tests one at a time, e.g. `cargo test --bins -- --ignored stage_tag`.
    #[test]
    #[ignore]
    fn stage_tag_outranks_font_tag_after_apply_font() {
        if gtk4::init().is_err() {
            eprintln!("skip: no GTK display");
            return;
        }
        let overlay = GlossOverlay::new(1050, 80);
        // A source turn with a stage direction (build_source_header form).
        let doc = "<speaker>YORK</speaker>\n\
                   <verse>Lay hands upon these traitors and their trash.</verse>\n\
                   <stage>[To Jourdain.]</stage>\n\
                   <verse>Beldam, I think we watched you at an</verse>";
        overlay.show_glossing(doc, 1660, 1000, Some("#88aabb"));

        let table = overlay.gloss_view.buffer().tag_table();
        let stage = table
            .lookup("gloss-stage")
            .expect("gloss-stage tag should exist after rendering a <stage> line");
        let font = table
            .lookup("gloss-font")
            .expect("gloss-font tag should exist after apply_font");
        assert!(
            stage.priority() > font.priority(),
            "gloss-stage (prio {}) must outrank gloss-font (prio {}) so its italic \
             survives the buffer-wide font tag",
            stage.priority(),
            font.priority(),
        );
    }
}

#[cfg(test)]
mod block_height_tests {
    use super::block_height_overhead;

    #[test]
    fn source_block_height_exceeds_prose_for_equal_text() {
        // A Source block WITH a speaker heading (verse: heading + per-line gaps)
        // must measure TALLER than an Explication block of the same text — the
        // conservative over-estimate that prevents clipping the speaker label.
        // (Pure-arithmetic check on the overhead constants; no GTK pango here —
        // factor the overhead into a pure helper `block_height_overhead(is_source,
        // has_speaker, text_h, line_h)` that gloss_block_height calls, and test
        // THAT.)
        let line_h = 20;
        assert!(
            block_height_overhead(true, true, 100, line_h)
                > block_height_overhead(false, false, 100, line_h)
        );
        // Prose overhead is the journal's per-block `text_h + line_h` pattern.
        assert_eq!(block_height_overhead(false, false, 100, line_h), 100 + line_h);
        // A SPEAKERLESS source (prose gloss) renders no heading and no verse
        // gaps — it pays only the per-block line_h reserve, same as an
        // Explication, so pages fill instead of splitting on phantom height.
        assert_eq!(block_height_overhead(true, false, 100, line_h), 100 + line_h);
    }
}
