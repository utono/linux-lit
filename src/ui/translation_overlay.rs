use crate::db::models::Line;
use crate::ui::pagination::{measure_text_height, paginate, page_containing_block, Page};
use gtk4::pango;
use gtk4::prelude::*;
use gtk4::{Align, Label, Orientation, Overlay, TextView};
use std::cell::{Cell, RefCell};

/// One render unit in the translation overlay: either a speaker's speech
/// (with original + translation paired per line) or a non-spoken interlude
/// (stage direction / scene header, shown full-width with no translation).
#[derive(Debug, Clone, PartialEq)]
pub struct TranslationBlock {
    /// Speaker label for a speech block; `None` for a non-spoken interlude.
    pub speaker: Option<String>,
    /// (original_text, translation_or_empty) per source line, in order.
    pub lines: Vec<(String, String)>,
    /// Inclusive range of `work.lines` indices this block covers.
    pub start_idx: usize,
    pub end_idx: usize,
}


/// Group a slice of scene lines into ordered blocks. Consecutive lines that
/// share the same `speaker` form one speech block; runs of `speaker == None`
/// lines (stage directions, scene headers) form non-spoken interlude blocks.
/// `idx_of(i)` maps the i-th element of `lines` back to its `work.lines` index;
/// `translation_of(line_id)` returns the modern translation if one exists.
pub fn group_scene_into_blocks(
    lines: &[Line],
    idx_of: impl Fn(usize) -> usize,
    translation_of: impl Fn(i64) -> Option<String>,
) -> Vec<TranslationBlock> {
    let mut blocks: Vec<TranslationBlock> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let work_idx = idx_of(i);
        let translation = if line.speaker.is_some() {
            translation_of(line.id).unwrap_or_default()
        } else {
            String::new()
        };

        let same_as_prev = blocks
            .last()
            .map(|b| b.speaker == line.speaker)
            .unwrap_or(false);

        if same_as_prev {
            let b = blocks.last_mut().unwrap();
            b.lines.push((line.text.clone(), translation));
            b.end_idx = work_idx;
        } else {
            blocks.push(TranslationBlock {
                speaker: line.speaker.clone(),
                lines: vec![(line.text.clone(), translation)],
                start_idx: work_idx,
                end_idx: work_idx,
            });
        }
    }

    blocks
}

/// One rendered block's views + source range, for cursor highlighting. `trans`
/// is None for a non-spoken interlude block (a single `orig` view).
struct BlockEntry {
    start_idx: usize,
    end_idx: usize,
    orig: gtk4::TextView,
    trans: Option<gtk4::TextView>,
}

/// Everything `render_page` needs to rebuild a page's widgets, captured at
/// `show()` so a cursor-driven page turn re-renders identically without the
/// caller re-supplying the theme/geometry.
#[derive(Clone)]
struct RenderCtx {
    text_fg: String,
    dim_fg: String,
    cursor_line_bg: String,
    font_family: String,
    body_font_size: i32,
    header_pt: i32,
    side_margin: i32,
    col_width: i32,
    full_width: i32,
    block_margin_top: i32,
    header_margin_bottom: i32,
    /// Per-line above/below spacing on every overlay TextView, matching the main
    /// card's `pixels_above_lines`/`pixels_below_lines`. Without it the cursor
    /// line's `paragraph_background` band ends flush at the line's logical bottom
    /// and clips the highlighted line's descenders.
    line_spacing: i32,
}

pub struct TranslationOverlay {
    pub overlay: Overlay,
    scrim: gtk4::Box,
    container: gtk4::Box,
    title: Label,
    /// Vertical stack holding ONLY the current page's blocks. The overlay
    /// paginates (whole blocks that fit), so it never renders a partial row. It
    /// lives inside a non-scrolling `ScrolledWindow` (built in `new()`, owned by
    /// `container`) with `propagate_natural_height(false)` that hard-caps the card
    /// to its `height_request` so an under-measured page can't grow it.
    content_vbox: gtk4::Box,
    /// The non-scrolling `ScrolledWindow` wrapping `content_vbox`. Its height is
    /// PINNED in `show()` (min == max == height_request) so the card stays exactly
    /// `card_height` regardless of how little the current page holds — otherwise a
    /// short page would let the `valign=Center` container collapse to its content.
    scrolled: gtk4::ScrolledWindow,
    /// The whole scene's blocks (all pages).
    blocks: RefCell<Vec<TranslationBlock>>,
    /// Page boundaries over `blocks`.
    pages: RefCell<Vec<Page>>,
    /// The currently rendered page index.
    current_page: Cell<usize>,
    /// The blocks rendered on the CURRENT page (for highlighting).
    block_widgets: RefCell<Vec<BlockEntry>>,
    /// Render context captured at `show()` so a page turn re-renders identically.
    ctx: RefCell<Option<RenderCtx>>,
}

impl TranslationOverlay {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let scrim = gtk4::Box::new(Orientation::Vertical, 0);
        scrim.add_css_class("gloss-scrim");
        scrim.set_visible(false);

        let container = gtk4::Box::new(Orientation::Vertical, 0);
        container.set_halign(Align::Center);
        container.set_valign(Align::Center);
        container.add_css_class("gloss-overlay");
        container.set_visible(false);

        let title = Label::new(Some("Translation"));
        title.add_css_class("gloss-title");
        title.set_halign(Align::Start);
        title.set_margin_start(24);
        title.set_margin_top(24);
        title.set_margin_bottom(8);
        container.append(&title);

        // Page container, top-aligned. Wrapped in a non-scrolling ScrolledWindow
        // with `propagate_natural_height(false)` so the CARD honors its
        // `height_request` and NEVER grows to fit an over-tall page — the same
        // technique the gloss overlay uses. (We paginate, so the scrollbar never
        // shows; the wrapper is purely a height cap + overflow clip, a safety net
        // against any block-height under-measurement.)
        let content_vbox = gtk4::Box::new(Orientation::Vertical, 0);
        content_vbox.set_hexpand(true);
        content_vbox.set_valign(Align::Start);

        let scrolled = gtk4::ScrolledWindow::new();
        scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
        scrolled.set_vscrollbar_policy(gtk4::PolicyType::Never);
        scrolled.set_propagate_natural_height(false);
        scrolled.set_vexpand(true);
        scrolled.set_hexpand(true);
        scrolled.set_margin_bottom(20);
        scrolled.set_child(Some(&content_vbox));
        container.append(&scrolled);

        Self {
            overlay,
            scrim,
            container,
            title,
            content_vbox,
            scrolled,
            blocks: RefCell::new(Vec::new()),
            pages: RefCell::new(Vec::new()),
            current_page: Cell::new(0),
            block_widgets: RefCell::new(Vec::new()),
            ctx: RefCell::new(None),
        }
    }

    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_overlay_panel(
            &self.overlay, child, &self.scrim, &self.container,
        );
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
        self.scrim.set_visible(false);
    }

    /// Populate and reveal the overlay. `blocks` come from
    /// `group_scene_into_blocks`. `cursor_work_idx` selects the page to open on.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &self,
        title: &str,
        blocks: &[TranslationBlock],
        card_width: i32,
        card_height: i32,
        text_fg: &str,
        dim_fg: &str,
        body_font_size: i32,
        font_family: &str,
        cursor_line_bg: &str,
        line_spacing: i32,
        cursor_work_idx: Option<usize>,
    ) {
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.title.set_text(title);

        // Tighter side margins than the main reading card (was /12): with no-wrap
        // columns a long translation overflows its `col_width` rightward, and the
        // right `side_margin` + block `margin_end` is what clips it at the card
        // edge. A smaller margin gives the (rightmost) translation column more room
        // before the clip so it isn't truncated mid-word.
        let side_margin = card_width / 24;
        let col_width = ((card_width - 2 * side_margin) / 2 - 12).max(120);
        let full_width = (card_width - 2 * side_margin).max(120);
        // Speaker header point size: 0.75 of the body (reader) font, matching the
        // main card's `speaker-name` tag (scale 0.75).
        let header_pt = ((body_font_size as f64) * 0.75).round().max(8.0) as i32;

        let ctx = RenderCtx {
            text_fg: text_fg.to_string(),
            dim_fg: dim_fg.to_string(),
            cursor_line_bg: cursor_line_bg.to_string(),
            font_family: font_family.to_string(),
            body_font_size,
            header_pt,
            side_margin,
            col_width,
            full_width,
            block_margin_top: 14,
            header_margin_bottom: 4,
            line_spacing,
        };

        // Measure each block and paginate. Pango measurement is synchronous and
        // deterministic — no GTK widget-allocation settle race. The context comes
        // from a realized widget so it uses the real font map / DPI.
        let pctx = self.content_vbox.pango_context();
        // Page budget = card height minus the title chrome and the bottom margin,
        // then a conservative safety factor. Standalone `pango::Layout` measurement
        // under-shoots the rendered TextView height (the view's intrinsic line
        // leading isn't captured), so a page measured at the raw budget renders a
        // little taller. The `scrolled` wrapper hard-caps the card regardless, but
        // an over-budget page would push a block's bottom under that cap (re-
        // clipping); under-packing keeps every block fully visible.
        let title_h = self.title.preferred_size().1.height().max(48);
        let raw_budget = (card_height - title_h - 20).max(120);
        let page_height = raw_budget * 9 / 10;
        // A single speaker's speech longer than one page is one indivisible
        // `TranslationBlock`; splitting it at line boundaries first keeps every
        // page ≤ the card height, so no page overflows the pinned scroll (the old
        // over-tall-block-grows-the-card bug). The speaker label repeats on each
        // continued sub-page.
        let blocks: Vec<TranslationBlock> =
            split_oversize_blocks(blocks, &pctx, &ctx, page_height);
        let heights: Vec<i32> = blocks.iter().map(|b| block_height(b, &pctx, &ctx)).collect();
        let pages = paginate(&heights, page_height);

        // PIN the scroll to exactly the content budget (min == max ==
        // height_request). `propagate_natural_height(false)` alone leaves the
        // scroll free to report its child's (small) natural height, so a short
        // page lets the `valign=Center` container collapse to its content and the
        // card visibly shrinks on a page turn. Pinning all three forces the card
        // to stay `card_height` regardless of how little the page holds — the same
        // technique the gloss overlay's `size_scroll` uses.
        self.scrolled.set_height_request(raw_budget);
        self.scrolled.set_min_content_height(raw_budget);
        self.scrolled.set_max_content_height(raw_budget);

        let start_page = cursor_work_idx
            .and_then(|w| block_for_work_idx(&blocks, w))
            .map(|bi| page_containing_block(&pages, bi))
            .unwrap_or(0);

        *self.blocks.borrow_mut() = blocks;
        *self.pages.borrow_mut() = pages;
        *self.ctx.borrow_mut() = Some(ctx);

        self.render_page(start_page);

        self.scrim.set_visible(true);
        self.container.set_visible(true);

        if let Some(w) = cursor_work_idx {
            self.highlight_work_line(w);
        }
    }

    /// Render the blocks of `page_idx` into `content_vbox`, replacing whatever was
    /// there. Sets `current_page` and rebuilds `block_widgets` for the page.
    fn render_page(&self, page_idx: usize) {
        // Clear the previous page.
        while let Some(child) = self.content_vbox.first_child() {
            self.content_vbox.remove(&child);
        }
        self.block_widgets.borrow_mut().clear();

        let pages = self.pages.borrow();
        let blocks = self.blocks.borrow();
        let ctx_ref = self.ctx.borrow();
        let (Some(page), Some(ctx)) = (pages.get(page_idx), ctx_ref.as_ref()) else {
            return;
        };
        self.current_page.set(page_idx);

        for block in &blocks[page.start..page.end.min(blocks.len())] {
            let entry = render_block(&self.content_vbox, block, ctx);
            self.block_widgets.borrow_mut().push(entry);
        }
    }

    /// The work-line index to move the reader cursor to for a page turn: the
    /// FIRST block's `start_idx` on the page adjacent to the current one
    /// (`forward` → next page, else previous). Returns `None` when there is no
    /// such page (already at the first/last page) so the caller can no-op. The
    /// caller moves the real reader cursor there (which seeks MPV) and then syncs
    /// the overlay, so page-turning stays consistent with the reader + playback.
    pub fn page_turn_target(&self, forward: bool) -> Option<usize> {
        let pages = self.pages.borrow();
        let blocks = self.blocks.borrow();
        let cur = self.current_page.get();
        let target_page = if forward {
            if cur + 1 < pages.len() { cur + 1 } else { return None }
        } else if cur > 0 {
            cur - 1
        } else {
            return None;
        };
        let page = pages.get(target_page)?;
        blocks.get(page.start).map(|b| b.start_idx)
    }

    /// Follow the reader cursor: turn to the page containing `work_idx`'s block
    /// (re-rendering only if the page changed), then highlight that block. The
    /// page is rendered synchronously, so the highlight paints immediately — no
    /// scroll-settle timing (the old scroll model's "highlight only after a nav
    /// key" bug).
    pub fn show_for_cursor(&self, work_idx: usize) {
        let target_page = {
            let blocks = self.blocks.borrow();
            let pages = self.pages.borrow();
            block_for_work_idx(&blocks, work_idx).map(|bi| page_containing_block(&pages, bi))
        };
        if let Some(p) = target_page {
            if p != self.current_page.get() {
                self.render_page(p);
            }
        }
        self.highlight_work_line(work_idx);
    }

    /// Highlight the cursor's source line `work_idx` in BOTH columns, IF its block
    /// is on the current page. Clears any prior highlight first. Off-page → no-op
    /// (callers page first via `show_for_cursor`).
    pub fn highlight_work_line(&self, work_idx: usize) {
        let entries = self.block_widgets.borrow();

        for e in entries.iter() {
            clear_cursor_tag(&e.orig.buffer());
            if let Some(t) = &e.trans {
                clear_cursor_tag(&t.buffer());
            }
        }

        let ranges: Vec<(usize, usize)> =
            entries.iter().map(|e| (e.start_idx, e.end_idx)).collect();
        let Some((bi, off)) = locate_line(&ranges, work_idx) else { return };
        let entry = &entries[bi];

        apply_cursor_tag(&entry.orig.buffer(), off as i32);
        if let Some(t) = &entry.trans {
            apply_cursor_tag(&t.buffer(), off as i32);
        }
    }
}

/// Build one block's widget subtree, append it to `parent`, and return its
/// `BlockEntry`. Shared by `render_page` for every block on the page.
fn render_block(parent: &gtk4::Box, block: &TranslationBlock, ctx: &RenderCtx) -> BlockEntry {
    let block_box = gtk4::Box::new(Orientation::Vertical, 0);
    block_box.set_margin_start(ctx.side_margin);
    block_box.set_margin_end(ctx.side_margin);
    block_box.set_margin_top(ctx.block_margin_top);

    let (orig_view, trans_view): (gtk4::TextView, Option<gtk4::TextView>) =
        if let Some(speaker) = &block.speaker {
            let header = Label::new(None);
            header.set_halign(Align::Start);
            // Match the main card's speaker-name tag: the reading-card font family
            // (Charter) in small-caps.
            header.set_markup(&format!(
                "<span face='{}' foreground='{}' font_variant='small-caps' font_weight='normal' size='{}pt'>{}</span>",
                glib_escape(&ctx.font_family),
                ctx.text_fg,
                ctx.header_pt,
                glib_escape(speaker),
            ));
            header.set_margin_bottom(ctx.header_margin_bottom);
            block_box.append(&header);

            let cols = gtk4::Box::new(Orientation::Horizontal, 0);
            let orig = make_column(ctx.col_width, &ctx.text_fg, false, ctx.line_spacing);
            let trans = make_column(ctx.col_width, &ctx.dim_fg, true, ctx.line_spacing);
            let (orig_text, trans_text) = block_column_texts(block);
            orig.buffer().set_text(&orig_text);
            trans.buffer().set_text(&trans_text);
            ensure_cursor_tag(&orig.buffer(), &ctx.cursor_line_bg);
            ensure_cursor_tag(&trans.buffer(), &ctx.cursor_line_bg);

            // No visible divider between columns. Preserve the inter-column gap
            // (formerly the divider's 12px+12px margins) as a left margin on the
            // translation column so the two columns keep the same spacing.
            trans.set_margin_start(24);

            cols.append(&orig);
            cols.append(&trans);
            block_box.append(&cols);
            (orig, Some(trans))
        } else {
            let view = TextView::new();
            crate::ui::set_view_readonly(&view);
            // No wrapping for interludes (stage directions / scene headers) either
            // (per user): each stays on one visual row, matching the speech columns.
            // Full-width, so overflow risk is minimal.
            view.set_wrap_mode(gtk4::WrapMode::None);
            view.set_pixels_above_lines(ctx.line_spacing);
            view.set_pixels_below_lines(ctx.line_spacing);
            view.add_css_class("gloss-text");
            let text = interlude_text(block);
            view.buffer().set_text(&text);
            ensure_cursor_tag(&view.buffer(), &ctx.cursor_line_bg);
            block_box.append(&view);
            (view, None)
        };

    parent.append(&block_box);
    BlockEntry {
        start_idx: block.start_idx,
        end_idx: block.end_idx,
        orig: orig_view,
        trans: trans_view,
    }
}

/// `(orig, trans)` column text for a speech block: one source/translation line
/// per buffer line, trailing newline trimmed.
fn block_column_texts(block: &TranslationBlock) -> (String, String) {
    let mut orig = String::new();
    let mut trans = String::new();
    for (o, t) in &block.lines {
        orig.push_str(o);
        orig.push('\n');
        trans.push_str(t);
        trans.push('\n');
    }
    (
        orig.trim_end_matches('\n').to_string(),
        trans.trim_end_matches('\n').to_string(),
    )
}

/// Full-width text for a non-spoken interlude block (originals only).
fn interlude_text(block: &TranslationBlock) -> String {
    let mut text = String::new();
    for (o, _) in &block.lines {
        text.push_str(o);
        text.push('\n');
    }
    text.trim_end_matches('\n').to_string()
}

/// Block index whose inclusive `start_idx..=end_idx` work-line range contains
/// `work_idx`.
fn block_for_work_idx(blocks: &[TranslationBlock], work_idx: usize) -> Option<usize> {
    blocks
        .iter()
        .position(|b| work_idx >= b.start_idx && work_idx <= b.end_idx)
}

/// Rendered height of a block, measured with Pango against `pctx` (a widget's
/// pango context — synchronous, deterministic, no GTK widget allocation). Speech
/// block = top margin + header height + header bottom margin + max(orig, trans)
/// column text height. Interlude = top margin + full-width text height.
fn block_height(block: &TranslationBlock, pctx: &pango::Context, ctx: &RenderCtx) -> i32 {
    // The rendered TextViews add `line_spacing` above AND below every paragraph
    // (each source line is one paragraph); the standalone `pango::Layout` does
    // not. Add it back so pagination doesn't over-pack now that lines are taller.
    let spacing = |paras: usize| 2 * ctx.line_spacing * paras as i32;
    if block.speaker.is_some() {
        let (orig_text, trans_text) = block_column_texts(block);
        let header_h = measure_text_height(pctx, "Mg", ctx.header_pt, &ctx.font_family, ctx.col_width);
        let orig_h = measure_text_height(pctx, &orig_text, ctx.body_font_size, &ctx.font_family, ctx.col_width);
        let trans_h = measure_text_height(pctx, &trans_text, ctx.body_font_size, &ctx.font_family, ctx.col_width);
        ctx.block_margin_top + header_h + ctx.header_margin_bottom + orig_h.max(trans_h) + spacing(block.lines.len())
    } else {
        let text = interlude_text(block);
        ctx.block_margin_top
            + measure_text_height(pctx, &text, ctx.body_font_size, &ctx.font_family, ctx.full_width)
            + spacing(block.lines.len())
    }
}

/// The fixed (line-independent) chrome height of a block: top margin, plus the
/// speaker header + its bottom margin for a speech block. A split sub-block
/// repeats this chrome (the speaker label shows on every continued page).
fn block_chrome_height(block: &TranslationBlock, pctx: &pango::Context, ctx: &RenderCtx) -> i32 {
    if block.speaker.is_some() {
        let header_h = measure_text_height(pctx, "Mg", ctx.header_pt, &ctx.font_family, ctx.col_width);
        ctx.block_margin_top + header_h + ctx.header_margin_bottom
    } else {
        ctx.block_margin_top
    }
}

/// Rendered height of ONE source line within a block (the taller of its original
/// / translation column line, plus the per-paragraph above+below spacing). Used
/// to split an over-tall block at line boundaries.
fn line_height(block: &TranslationBlock, i: usize, pctx: &pango::Context, ctx: &RenderCtx) -> i32 {
    let (orig, trans) = &block.lines[i];
    let width = if block.speaker.is_some() { ctx.col_width } else { ctx.full_width };
    let oh = measure_text_height(pctx, orig, ctx.body_font_size, &ctx.font_family, width);
    let th = if trans.is_empty() {
        0
    } else {
        measure_text_height(pctx, trans, ctx.body_font_size, &ctx.font_family, width)
    };
    oh.max(th) + 2 * ctx.line_spacing
}

/// Split any block taller than `page_height` into contiguous sub-blocks that
/// each fit on a page, breaking only at source-line boundaries. A sub-block
/// carries the same `speaker` (so the label repeats on each continued page) and
/// the correct `start_idx`/`end_idx` sub-range, so cursor↔block mapping and
/// highlighting keep working unchanged. Blocks that already fit pass through
/// untouched. This is what keeps a long speech from forcing the card taller than
/// its fixed height (an un-splittable over-tall block used to overflow the pin).
fn split_oversize_blocks(
    blocks: &[TranslationBlock],
    pctx: &pango::Context,
    ctx: &RenderCtx,
    page_height: i32,
) -> Vec<TranslationBlock> {
    let budget = page_height.max(1);
    let mut out: Vec<TranslationBlock> = Vec::new();
    for block in blocks {
        if block_height(block, pctx, ctx) <= budget {
            out.push(block.clone());
            continue;
        }
        // Greedily pack this block's lines into sub-blocks that fit the budget.
        let chrome = block_chrome_height(block, pctx, ctx);
        let mut seg_start = 0usize; // index into block.lines of the current sub-block
        let mut acc = chrome;
        for i in 0..block.lines.len() {
            let lh = line_height(block, i, pctx, ctx);
            // Close the current sub-block before adding a line that would overflow
            // it (but never emit an empty sub-block — a single line taller than the
            // budget still gets its own sub-block rather than being dropped).
            if i > seg_start && acc + lh > budget {
                out.push(sub_block(block, seg_start, i));
                seg_start = i;
                acc = chrome;
            }
            acc += lh;
        }
        out.push(sub_block(block, seg_start, block.lines.len()));
    }
    out
}

/// Build a sub-block covering `block.lines[from..to]`, carrying the parent's
/// speaker and the correct work-index sub-range.
/// The parent's lines are contiguous work indices (`start_idx + from` down to
/// `start_idx + to - 1`), so the offset arithmetic is exact.
fn sub_block(block: &TranslationBlock, from: usize, to: usize) -> TranslationBlock {
    TranslationBlock {
        speaker: block.speaker.clone(),
        lines: block.lines[from..to].to_vec(),
        start_idx: block.start_idx + from,
        end_idx: block.start_idx + to - 1,
    }
}

/// Ensure the buffer has a `cursor-line` tag painting the paragraph background
/// with the theme's cursor-line color. Idempotent (lookup before add).
fn ensure_cursor_tag(buffer: &gtk4::TextBuffer, cursor_line_bg: &str) {
    if buffer.tag_table().lookup("cursor-line").is_none() {
        let tag = gtk4::TextTag::builder()
            .name("cursor-line")
            .paragraph_background(cursor_line_bg)
            .build();
        buffer.tag_table().add(&tag);
    }
}

/// Remove the `cursor-line` tag from the whole buffer (if the tag exists).
fn clear_cursor_tag(buffer: &gtk4::TextBuffer) {
    if let Some(tag) = buffer.tag_table().lookup("cursor-line") {
        let (start, end) = buffer.bounds();
        buffer.remove_tag(&tag, &start, &end);
    }
}

/// Apply the `cursor-line` tag to buffer line `line` (0-based). No-op if the
/// line or tag is missing.
fn apply_cursor_tag(buffer: &gtk4::TextBuffer, line: i32) {
    let Some(tag) = buffer.tag_table().lookup("cursor-line") else { return };
    let Some(start) = buffer.iter_at_line(line) else { return };
    let mut end = start;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    buffer.apply_tag(&tag, &start, &end);
}

fn make_column(width: i32, color: &str, italic: bool, line_spacing: i32) -> TextView {
    let view = TextView::new();
    crate::ui::set_view_readonly(&view);
    // No wrapping (per user): each source/translation line stays on ONE visual
    // row so the columns line up 1:1 and the cursor-line highlight band is a
    // single row. A translation longer than the column overflows into the card's
    // side margin rather than wrapping to a second row (mirrors the reading card,
    // where verse lines never wrap). `width` is a MINIMUM request, so an
    // overflowing line simply extends past it.
    view.set_wrap_mode(gtk4::WrapMode::None);
    // Per-line above/below spacing so the cursor line's `paragraph_background`
    // band has room below the descenders (matches the main reading card).
    view.set_pixels_above_lines(line_spacing);
    view.set_pixels_below_lines(line_spacing);
    view.set_size_request(width, -1);
    view.add_css_class("gloss-text");
    if italic {
        view.add_css_class("translation-col");
    }
    // Color comes from the .gloss-text / .translation-col CSS classes; `color`
    // reserved for a future inline tag.
    let _ = color;
    view
}

fn glib_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Given each block's inclusive (start_idx, end_idx) work-line range in order,
/// return (block_index, line_offset) for the block containing `work_idx`.
fn locate_line(ranges: &[(usize, usize)], work_idx: usize) -> Option<(usize, usize)> {
    for (i, (start, end)) in ranges.iter().enumerate() {
        if work_idx >= *start && work_idx <= *end {
            return Some((i, work_idx - start));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::Line;

    fn mk(id: i64, text: &str, speaker: Option<&str>) -> Line {
        Line {
            id,
            citation: String::new(),
            text: text.to_string(),
            normalized: String::new(),
            speaker: speaker.map(|s| s.to_string()),
            is_dialogue: speaker.is_some(),
            timestamp: None,
            div1: 1,
            div2: 1,
            line_in_div: 0,
            sub_line: 0,
            is_chapter: false,
            is_spoken: None,
        }
    }

    #[test]
    fn groups_consecutive_speaker_lines_into_one_block() {
        let lines = vec![
            mk(10, "She shall be to the happiness of England", Some("CRANMER")),
            mk(11, "An aged princess; many days shall see her,", Some("CRANMER")),
            mk(12, "O lord", Some("KING")),
        ];
        let trans = |id: i64| match id {
            10 => Some("She shall be to England's happiness".to_string()),
            11 => Some("An aged princess; many days will see her,".to_string()),
            12 => Some("O lord".to_string()),
            _ => None,
        };
        let blocks = group_scene_into_blocks(&lines, |i| i, trans);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].speaker.as_deref(), Some("CRANMER"));
        assert_eq!(blocks[0].lines.len(), 2);
        assert_eq!(blocks[0].lines[0].0, "She shall be to the happiness of England");
        assert_eq!(blocks[0].lines[0].1, "She shall be to England's happiness");
        assert_eq!(blocks[0].start_idx, 0);
        assert_eq!(blocks[0].end_idx, 1);
        assert_eq!(blocks[1].speaker.as_deref(), Some("KING"));
        assert_eq!(blocks[1].start_idx, 2);
        assert_eq!(blocks[1].end_idx, 2);
    }

    #[test]
    fn non_spoken_lines_form_their_own_block_with_blank_translation() {
        let lines = vec![
            mk(20, "Enter KING and CRANMER", None),
            mk(21, "Thou speakest wonders.", Some("KING")),
        ];
        let blocks = group_scene_into_blocks(&lines, |i| i, |_| None);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].speaker, None);
        assert_eq!(blocks[0].lines[0].0, "Enter KING and CRANMER");
        assert_eq!(blocks[0].lines[0].1, "");
        assert_eq!(blocks[1].speaker.as_deref(), Some("KING"));
    }

    #[test]
    fn idx_of_maps_back_to_work_indices() {
        let lines = vec![
            mk(30, "first", Some("A")),
            mk(31, "second", Some("A")),
        ];
        let blocks = group_scene_into_blocks(&lines, |i| 100 + i, |_| None);
        assert_eq!(blocks[0].start_idx, 100);
        assert_eq!(blocks[0].end_idx, 101);
    }

    #[test]
    fn empty_input_yields_no_blocks() {
        let blocks = group_scene_into_blocks(&[], |i| i, |_| None);
        assert!(blocks.is_empty());
    }

    #[test]
    fn locate_line_finds_block_and_offset() {
        let ranges = vec![(10usize, 12usize), (13, 13)];
        assert_eq!(locate_line(&ranges, 10), Some((0, 0)));
        assert_eq!(locate_line(&ranges, 12), Some((0, 2)));
        assert_eq!(locate_line(&ranges, 13), Some((1, 0)));
    }

    #[test]
    fn locate_line_returns_none_outside_any_block() {
        let ranges = vec![(10usize, 12usize)];
        assert_eq!(locate_line(&ranges, 9), None);
        assert_eq!(locate_line(&ranges, 13), None);
        assert_eq!(locate_line(&[], 0), None);
    }

    #[test]
    fn block_for_work_idx_finds_containing_block() {
        let blocks = vec![
            TranslationBlock { speaker: Some("A".into()), lines: vec![], start_idx: 10, end_idx: 12 },
            TranslationBlock { speaker: Some("B".into()), lines: vec![], start_idx: 13, end_idx: 13 },
        ];
        assert_eq!(block_for_work_idx(&blocks, 10), Some(0));
        assert_eq!(block_for_work_idx(&blocks, 12), Some(0));
        assert_eq!(block_for_work_idx(&blocks, 13), Some(1));
        assert_eq!(block_for_work_idx(&blocks, 99), None);
    }

    #[test]
    fn sub_block_preserves_speaker_and_work_range() {
        // A 4-line speech at work indices 20..=23. A sub-block covering lines
        // [1,3) must carry the same speaker, its two lines, and the exact
        // work-index sub-range 21..=22 — so a cursor at work line 22 still maps
        // into this sub-block after an over-tall split.
        let block = TranslationBlock {
            speaker: Some("CRANMER".into()),
            lines: vec![
                ("a".into(), "A".into()),
                ("b".into(), "B".into()),
                ("c".into(), "C".into()),
                ("d".into(), "D".into()),
            ],
            start_idx: 20,
            end_idx: 23,
        };
        let sb = sub_block(&block, 1, 3);
        assert_eq!(sb.speaker.as_deref(), Some("CRANMER"));
        assert_eq!(sb.lines, vec![("b".into(), "B".into()), ("c".into(), "C".into())]);
        assert_eq!(sb.start_idx, 21);
        assert_eq!(sb.end_idx, 22);
        // A full-span sub-block is identical to the original range.
        let full = sub_block(&block, 0, 4);
        assert_eq!(full.start_idx, 20);
        assert_eq!(full.end_idx, 23);
        assert_eq!(full.lines.len(), 4);
    }
}
