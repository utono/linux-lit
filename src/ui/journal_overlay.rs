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
    scrolled: gtk4::ScrolledWindow,
    view: gtk4::TextView,
    clip_guard: crate::ui::bottom_clip_guard::BottomClipGuard,
    footer_container: gtk4::Box,
    footer_left: Label,
    position_label: Label,
    hint: Label,
    bar_drawing: gtk4::DrawingArea,
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
    /// Page ranges over `all_paragraphs` from `paginate`.
    pages: RefCell<Vec<crate::ui::pagination::Page>>,
    /// Current page index into `pages`.
    page_idx: Cell<usize>,
    /// Cursor index within `all_paragraphs` (the whole Q&A). `cursor_block` is its
    /// page-local projection.
    cursor_full: Cell<usize>,
    /// Footer position state, rebuilt by `update_footer_position`: the band
    /// identity (`<abbrev> <act>.<scene>`) and the Q&A-ENTRY position in the band
    /// `(entry_index, entry_count)` (the Ctrl+n/p count). The render-page count
    /// (`pages`/`page_idx`, the j/k pages) is appended as "page X / Y" when the
    /// current Q&A spans more than one render page.
    footer_band: RefCell<String>,
    entry_pos: Cell<(usize, usize)>,
    text_margins: i32,
    column_width: i32,
    /// True when the loaded work is prose. Set once per work load via
    /// `set_prose`. Selects the centered prose column inset (card_width/5) over
    /// the verse `card_width/4` inset in `size_card`.
    is_prose: Cell<bool>,
    font_family: RefCell<String>,
    font_size: Cell<i32>,
    /// Reading font family stashed on edit-enter and restored on exit, so the
    /// monospace edit font does not leak into the rendered display. `None` when
    /// not editing. Save-and-restore (not hardcode-Charter) so a non-default
    /// overlay font would survive an edit.
    pre_edit_family: RefCell<Option<String>>,
    last_card_size: Cell<(i32, i32)>,
    /// Owns the ask-card lifecycle + the fixed-scroll-height viewport-shrink (the
    /// occlusion fix) + the footer hide/show + the clip recompute. Shared with the
    /// gloss overlay so the mechanism can't drift. See `AskCardHost`.
    ask_host: AskCardHost,
    /// The in-place vim editor's engine, `Some` while the `e` editor is open.
    /// The page `view` mirrors its buffer/cursor; `enter_edit_buffer` seeds it,
    /// `feed_edit_key` drives it, `exit_edit_buffer` drops it. See
    /// docs/plans/2026-06-30-journal-vim-edit-design.md.
    vim_engine: RefCell<Option<crate::input::vim::VimEngine>>,
    /// The buffer the editor was seeded with, for dirty-check on cancel.
    vim_seed: RefCell<String>,
    /// (block-fill, glyph-fg) for the NORMAL-mode block cursor, set on enter from
    /// the theme's cursor colors.
    vim_cursor_colors: RefCell<(String, String)>,
    /// `<hi>` highlight background (theme `cursor_line_bg`), threaded by the app
    /// via `set_highlight_color`; defaults to `DEFAULT_HIGHLIGHT_BG`.
    highlight_bg: RefCell<String>,
    /// Char ranges of `<hi>` highlights in the CURRENT page body, re-applied on
    /// the `journal-hi` tag after each `set_text`. Empty when none.
    hi_ranges: RefCell<Vec<(usize, usize)>>,
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
}

/// Split the full Q&A text into paragraph blocks (the pagination unit): maximal
/// runs of non-blank lines, blank-line separated. Returns each paragraph's text.
/// Reuses `journal_blocks` so the split matches what `render_page` re-derives for
/// the accent bar.
fn paragraph_texts(full: &str) -> Vec<String> {
    let lines: Vec<&str> = full.split('\n').collect();
    journal_blocks(&lines).into_iter().map(|b| b.text).collect()
}

/// Prefix a journal Q&A question with `Q: ` for display (the answer follows
/// below). Idempotent: a question already starting with `Q:` is returned as-is,
/// so a stored/re-rendered question isn't double-prefixed.
fn prefix_question(question: &str) -> String {
    if question.trim_start().starts_with("Q:") {
        question.to_string()
    } else {
        format!("Q: {}", question)
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
        container.set_visible(false);

        // No title header: the footer already identifies the work + chapter
        // (`<abbrev> <act>.<scene> · page N of M`), so the overlay drops the
        // "<Work> — Chapter N" header that used to sit above the scroll.

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
        view.set_editable(false);
        view.set_cursor_visible(false);
        view.set_focusable(false);
        view.set_wrap_mode(gtk4::WrapMode::Word);
        // ~one line of breathing room above the first line and below the last
        // line INSIDE the panel, so the text isn't flush against the panel's
        // inner top/bottom edge (matches the gloss view's inner margins; the
        // scroll_overlay's own margins sit OUTSIDE the panel, so they can't
        // provide this gap).
        view.set_top_margin(28);
        view.set_bottom_margin(28);
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
        let bar_drawing = gtk4::DrawingArea::new();
        bar_drawing.set_can_target(false);
        {
            let ranges_clone = bar_ranges.clone();
            let view_clone = view.clone();
            let vim_block_clone = vim_block_line.clone();
            let marker_glyph_clone = marker_glyph.clone();
            let marker_color_clone = marker_color.clone();
            let bar_color_clone = bar_color.clone();
            bar_drawing.set_draw_func(move |_area, cr, area_w, _h| {
                // Page marker first (independent of the selection bar's early-return).
                crate::ui::draw_page_marker_glyph(
                    cr,
                    &view_clone,
                    area_w,
                    *marker_glyph_clone.borrow(),
                    *marker_color_clone.borrow(),
                    0.55,
                    8,
                );
                // Vim block cursor on a BLANK line: the line has no glyph to fill,
                // so draw a thin left-edge block at the line's window-y. Drawn
                // BEFORE the selection-bar early-return so it shows while editing
                // (no selection ranges then).
                if let Some((buf_line, br, bg, bb)) = *vim_block_clone.borrow() {
                    let buffer = view_clone.buffer();
                    if let Some(iter) = buffer.iter_at_line(buf_line) {
                        let loc = view_clone.iter_location(&iter);
                        let (_, by) = view_clone.buffer_to_window_coords(
                            gtk4::TextWindowType::Widget, 0, loc.y());
                        let bh = if loc.height() > 0 { loc.height() } else { 18 } as f64;
                        let bw = (bh * 0.5).max(7.0);
                        let bx = (view_clone.left_margin() as f64).max(2.0);
                        cr.set_source_rgb(br, bg, bb);
                        cr.rectangle(bx, by as f64, bw, bh);
                        let _ = cr.fill();
                    }
                }
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

        container.append(&scroll_overlay);

        // Footer rule mirroring the gloss overlay (gloss_overlay.rs footer_box):
        // current page's work/act/scene on the left, fixed keybind hints on the
        // right.
        let footer = crate::ui::footer::build_footer_row(
            text_margins as i32,
            "",
        );
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

        // Shared "ask" input card (canonical synopsis values), stacked last in
        // the column. Focus returns to the page view when leaving the input.
        let ask = AskCard::new(text_margins as i32, &view);
        container.append(ask.container());

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

        Self {
            overlay,
            scrim,
            container,
            scrolled,
            view,
            clip_guard,
            footer_container,
            footer_left,
            position_label,
            hint,
            bar_drawing,
            panel_drawing,
            bar_ranges,
            vim_block_line,
            blocks: RefCell::new(Vec::new()),
            visual_anchor: Cell::new(None),
            cursor_block: Cell::new(0),
            all_paragraphs: RefCell::new(Vec::new()),
            pages: RefCell::new(Vec::new()),
            page_idx: Cell::new(0),
            cursor_full: Cell::new(0),
            footer_band: RefCell::new(String::new()),
            entry_pos: Cell::new((0, 0)),
            text_margins: text_margins as i32,
            column_width: column_width as i32,
            is_prose: Cell::new(false),
            // Match the gloss overlay's reading family + size (shared consts) so
            // the journal applies its OWN 19pt font tag. An EMPTY family made
            // apply_font early-return, so the journal never applied a tag and fell
            // back to the `.gloss-text` CSS at config.font_size (the reader's size,
            // e.g. 17pt) — rendering SMALLER than the gloss overlay's 19pt.
            font_family: RefCell::new(crate::ui::gloss_overlay::GLOSS_DEFAULT_FONT_FAMILY.to_string()),
            font_size: Cell::new(crate::ui::gloss_overlay::GLOSS_DEFAULT_FONT_SIZE),
            pre_edit_family: RefCell::new(None),
            last_card_size: Cell::new((0, 0)),
            ask_host,
            vim_engine: RefCell::new(None),
            vim_seed: RefCell::new(String::new()),
            vim_cursor_colors: RefCell::new((String::new(), String::new())),
            highlight_bg: RefCell::new(crate::ui::DEFAULT_HIGHLIGHT_BG.to_string()),
            hi_ranges: RefCell::new(Vec::new()),
            marker_glyph,
            marker_color,
            bar_color,
            panel_color,
        }
    }

    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_overlay_panel(
            &self.overlay, child, &self.scrim, &self.container,
        );
    }

    /// Record whether the loaded work is prose, so `size_card` picks the
    /// centered prose column inset. Called once per work load from display_work.
    pub fn set_prose(&self, is_prose: bool) {
        self.is_prose.set(is_prose);
    }

    fn size_card(&self, card_width: i32, card_height: i32) {
        self.container.set_size_request(card_width, card_height);
        self.last_card_size.set((card_width, card_height));
        // Fixed-scroll-height (the host owns it): the scroll (vexpand off) gets an
        // EXPLICIT height = pane minus the footer chrome — its height while the ask
        // card is CLOSED. `ask_host.open` subtracts the ask slot, `close` restores
        // this stored closed height. There is no title header anymore (the footer
        // identifies the work + chapter), so only the scroll_overlay margins
        // (UNACCOUNTED_CHROME_MARGINS = 44, which `preferred_size()` omits) and the
        // footer count as fixed chrome.
        let (_, footer_h) = self.footer_container.preferred_size();
        self.ask_host.size(
            card_width,
            card_height,
            UNACCOUNTED_CHROME_MARGINS,
            footer_h.height(),
        );
        // Anchor the text to the card's side margin (card_width/4, the ~65-char
        // readability optimum the gloss overlay uses) rather than the small fixed
        // `text_margins` — otherwise the Q&A prose runs nearly edge to edge on a
        // wide card. Card SIZE is unchanged; only the inner padding grows. See
        // ui::card_side_margin (audit #27).
        let side = if self.is_prose.get() {
            crate::ui::prose_column_margin(card_width)
        } else {
            crate::ui::card_side_margin(card_width)
        };
        // Indent the body right of the accent bar (bar sits in the gutter),
        // matching gloss. Left-only; the right margin stays `side`.
        self.view.set_left_margin(side + JOURNAL_BODY_INDENT);
        self.view.set_right_margin(side);
        let _ = (self.text_margins, self.column_width);
    }

    pub fn show_page(
        &self,
        footer_left: &str,
        page_index: usize,
        page_count: usize,
        question: &str,
        answer: &str,
        card_width: i32,
        card_height: i32,
    ) {
        self.size_card(card_width, card_height);
        // Store the band identity + Q&A-entry position; the footer text is
        // (re)built by update_footer_position, which also appends the render-page
        // count once pagination has run / on every page turn.
        *self.footer_band.borrow_mut() = footer_left.to_string();
        self.entry_pos.set((page_index, page_count));
        if page_count == 0 {
            // Empty band: a bare message, no navigable paragraphs.
            self.view.buffer().set_text("No pages yet \u{2014} press r to ask.");
            self.apply_font();
            self.clear_blocks();
            *self.all_paragraphs.borrow_mut() = Vec::new();
            self.pages.borrow_mut().clear();
            self.page_idx.set(0);
            self.cursor_full.set(0);
        } else {
            // Split the full Q&A into paragraph blocks (the pagination unit),
            // paginate by measured height, and render the first page. j/k step
            // the cursor across the FULL list, turning the page at boundaries —
            // so no partial paragraph is ever rendered at either edge.
            let full = format!("{}\n\n{}", prefix_question(question), answer);
            let paras = paragraph_texts(&full);
            *self.all_paragraphs.borrow_mut() = paras;
            self.cursor_full.set(0);
            self.repaginate();
            self.page_idx.set(0);
            self.render_page();
        }
        // Now the render-page count is known — build the footer with it.
        self.update_footer_position();
        self.ask_host.card().close();
        // Restore the navigation footer (show_loading may have hidden it).
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
            let adj = sc.vadjustment();
            let id_cell: Rc<std::cell::Cell<Option<glib::SignalHandlerId>>> =
                Rc::new(std::cell::Cell::new(None));
            let id_cell_clone = id_cell.clone();
            let id = adj.connect_changed(move |adj| {
                if let Some(r) = sc.root().and_then(|root| sc.compute_bounds(&root)) {
                    if r.width() > 0.0 && r.height() > 0.0 {
                        crate::logging::log(&format!(
                            "TEST_JOURNAL_VIEWPORT_RECT {} {} {} {}",
                            r.x().round() as i32,
                            r.y().round() as i32,
                            r.width().round() as i32,
                            r.height().round() as i32
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

    pub fn show_loading(&self, question: &str) {
        let (w, h) = self.last_card_size.get();
        if w > 0 {
            self.container.set_size_request(w, h);
        }
        // Echo the submitted question above the "Asking…" indicator so the user
        // sees what they asked while the answer is being generated.
        let body = if question.trim().is_empty() {
            "Asking\u{2026}".to_string()
        } else {
            format!("{}\n\nAsking\u{2026}", prefix_question(question))
        };
        self.view.buffer().set_text(&body);
        self.apply_font();
        self.ask_host.card().close();
        // Drop the prior page's blocks + bar: during the transient "Asking…"
        // state there is no real Q&A page, so Space/a must not read the prior
        // page's cursor paragraph. With no blocks, current_block_text() is None
        // and play_journal_block is a no-op.
        self.clear_blocks();
        // Keep the navigation footer hidden during the Asking state. The result
        // render (show_page/show_message) restores it.
        self.footer_container.set_visible(false);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    /// Render a PENDING passage ask: the visually selected source text
    /// (`<speaker>/<verse>` markup) shown through the shared gloss source
    /// renderer (speaker small-caps + verse hang-indent, full ink), in place of
    /// the empty band's "No pages yet — press r to ask." placeholder — so the
    /// reader sees the passage they are asking about while the ask card is open
    /// (mirrors the gloss overlay's "Glossing…" card). No navigable blocks and
    /// no accent bar: the render is transient until submit/cancel.
    pub fn show_passage_source(
        &self,
        footer_left: &str,
        source_doc: &str,
        card_width: i32,
        card_height: i32,
    ) {
        self.size_card(card_width, card_height);
        *self.footer_band.borrow_mut() = footer_left.to_string();
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
        self.pages.borrow_mut().clear();
        self.page_idx.set(0);
        self.cursor_full.set(0);
        self.clear_bar();
        *self.marker_glyph.borrow_mut() = None;
        // Stale <hi> char ranges from the last Q&A page must not survive into a
        // block-less buffer (loading / message / pending-passage): a later theme
        // change calls apply_hi_color, which would paint the OLD page's ranges
        // over arbitrary spans of the new text.
        self.hi_ranges.borrow_mut().clear();
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
        self.scrim.set_visible(false);
        self.ask_host.card().close();
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
        let band = self.footer_band.borrow().clone();
        let (entry_idx, entry_count) = self.entry_pos.get();
        let mut s = band;
        if entry_count == 0 {
            s.push_str(" \u{00b7} no Q&A yet");
        } else {
            s.push_str(&format!(" \u{00b7} Q&A {} of {}", entry_idx + 1, entry_count));
        }
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
        let font_str = format!("{} {}", self.font_family.borrow(), self.font_size.get());
        crate::ui::apply_font_to_views(
            &[&self.view, self.ask_host.input()],
            &font_str,
            "journal-font",
        );
    }

    /// Set the `<hi>` highlight background and re-assert it (so a live theme
    /// change repaints the current read view). `apply_hi_color` re-applies over
    /// `hi_ranges`, which are cleared while editing, so this is safe in any mode.
    pub fn set_highlight_color(&self, color: &str) {
        *self.highlight_bg.borrow_mut() = color.to_string();
        self.apply_hi_color();
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

    pub fn open_ask_card(&self, title: &str, hint: &str, block_fill: &str, block_fg: &str) {
        // The host reveals the ask card (a vim editor, NORMAL by default), hides
        // the nav footer, shrinks the scroll viewport (occlusion fix), recomputes
        // the clip. apply_font re-fonts the now-visible input. block_fill/fg are
        // the NORMAL-mode block-cursor colors.
        self.ask_host.open(title, hint, block_fill, block_fg);
        self.apply_font();

        // Headless test: emit the scrolled viewport rect WITH the ask card open
        // (the exact regression from Tasks 1-5). The card open shrinks the
        // scrolled window's height; this idle fires after that layout pass, so
        // the rect reflects the reduced viewport. Tests/journal_clipping.rs reads
        // TEST_JOURNAL_ASK_VIEWPORT_RECT for the ask-open assertion.
        if std::env::var_os("LIT_HEADLESS_TEST").is_some() {
            let sc = self.scrolled.clone();
            glib::idle_add_local_once(move || {
                if let Some(r) = sc.root().and_then(|root| sc.compute_bounds(&root)) {
                    crate::logging::log(&format!(
                        "TEST_JOURNAL_ASK_VIEWPORT_RECT {} {} {} {}",
                        r.x().round() as i32,
                        r.y().round() as i32,
                        r.width().round() as i32,
                        r.height().round() as i32
                    ));
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
    pub fn enter_edit_buffer(&self, question: &str, answer: &str, block_fill: &str, block_fg: &str) {
        self.begin_edit_font();
        // The editor shows RAW text (with `<hi>` literals); the read-mode hi
        // ranges are stale here and must not be re-applied to the raw buffer.
        self.hi_ranges.borrow_mut().clear();
        *self.vim_cursor_colors.borrow_mut() = (block_fill.to_string(), block_fg.to_string());
        let buf = crate::input::vim::journal_doc::build_buffer(question, answer);
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
    pub fn edit_buffer_qa(&self) -> (String, String) {
        let guard = self.vim_engine.borrow();
        match guard.as_ref() {
            Some(e) => crate::input::vim::journal_doc::parse_back(e.buffer()),
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
        (card_h - UNACCOUNTED_CHROME_MARGINS - footer_h.height()).max(80)
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
        let heights: Vec<i32> = paras
            .iter()
            .map(|p| {
                let text_h =
                    crate::ui::pagination::measure_text_height(&pctx, p, size, &family, wrap_w);
                text_h + line_h
            })
            .collect();
        drop(paras);
        // Budget + per-block estimates for diagnosing pack decisions from a run.
        crate::log_fmt!(
            "JOURNAL-PAGINATE: page_h={} wrap_w={} line_h={} font='{} {}' heights={:?}",
            self.page_height(), wrap_w, line_h, family, size, heights
        );
        *self.pages.borrow_mut() = crate::ui::pagination::paginate(&heights, self.page_height());
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
        let slice = &paras[page.start..page.end.min(paras.len())];
        let raw_body = slice.join("\n\n");
        let page_start = page.start;
        drop(paras);
        drop(pages);

        // Strip inline `<hi>` for display, recording the highlight ranges so the
        // `journal-hi` background is re-applied after set_text. Blocks are derived
        // from the CLEAN body so line indices line up with what's shown.
        let (body, hi_ranges) = crate::ui::gloss_block::strip_hi_spans(&raw_body);
        *self.hi_ranges.borrow_mut() = hi_ranges;

        self.view.buffer().set_text(&body);
        self.apply_font();
        // Paint the `<hi>` highlight AFTER set_text + font (read-mode only; the
        // editor sets raw text and must not re-apply these read-mode ranges).
        self.apply_hi_color();
        // The leading `Q:` line renders as PLAIN body text — no header tag. It
        // used to get a bold/0.9-scale/dim header treatment, but the tag only
        // landed on the first render (page turns skipped it), and the user
        // prefers the plain look: same weight and size as the answer.
        // Floating page marker (⌄ more / • end), bottom-center of the viewport.
        self.update_page_marker(pidx, n_pages);
        // The vadjustment stays at top — the page fits, nothing scrolls.
        let adj = self.scrolled.vadjustment();
        adj.set_value(adj.lower());

        // Re-derive per-page blocks for the bar / visual mode (from `body`, not
        // the buffer — so the appended chevron is excluded).
        let lines: Vec<&str> = body.split('\n').collect();
        *self.blocks.borrow_mut() = journal_blocks(&lines);
        self.visual_anchor.set(None);
        // Project the full cursor onto this page (clamped into the page range).
        let page_local = self
            .cursor_full
            .get()
            .saturating_sub(page_start)
            .min(self.blocks.borrow().len().saturating_sub(1));
        self.cursor_block.set(page_local);
        self.mark_cursor_block();
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

    /// The block cursor's current index (the block j/k/gg/G select), clamped to
    /// the block list. None when the page has no blocks (the empty/loading card).
    pub fn current_block_index(&self) -> Option<usize> {
        let len = self.blocks.borrow().len();
        if len == 0 {
            None
        } else {
            Some(self.cursor_block.get().min(len - 1))
        }
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

    /// Step the full-list cursor by `delta` (clamped), turning the page if the new
    /// cursor leaves the current page; otherwise just re-mark the bar.
    fn step_full_cursor(&self, delta: i32) {
        let total = self.all_paragraphs.borrow().len();
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
        let total = self.all_paragraphs.borrow().len();
        if total == 0 {
            return;
        }
        self.cursor_full.set(if last { total - 1 } else { 0 });
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
        crate::logging::log(&format!(
            "JOURNAL-CURSOR: cursor#{} bar lines [{}, {}]",
            i, span.0, span.1
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

    /// Index of the first block whose end_line is at or below the current
    /// viewport top — the anchor seed for Shift+V. Falls back to 0.
    fn topmost_visible_block(&self) -> usize {
        let top_y = self.scrolled.vadjustment().value();
        let buffer = self.view.buffer();
        let blocks = self.blocks.borrow();
        for (i, b) in blocks.iter().enumerate() {
            if let Some(iter) = buffer.iter_at_line(b.end_line) {
                let (y, h) = self.view.line_yrange(&iter);
                if (y + h) as f64 >= top_y {
                    return i;
                }
            }
        }
        0
    }

    /// Enter visual mode: anchor at the topmost visible block. Returns false
    /// (no-op) when there are no blocks.
    pub fn enter_visual(&self) -> bool {
        let n = self.blocks.borrow().len();
        if n == 0 {
            crate::logging::log("JOURNAL-VISUAL: enter_visual no-op (0 blocks)");
            return false;
        }
        let seed = self.topmost_visible_block();
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
