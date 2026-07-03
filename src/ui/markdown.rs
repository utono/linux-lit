//! CommonMark → GTK4 `TextTag` renderer.
//!
//! **Pure layer:** `plan_markdown(src)` parses a Markdown string via
//! `pulldown-cmark` and returns `Vec<Span>` — no GTK, fully unit-testable.
//!
//! **GTK layer:** `MarkdownTags` holds one `gtk4::TextTag` per `Style`;
//! `apply_markdown_blocks(buffer, src, tags)` walks `plan_markdown`, inserts each
//! span's text at the buffer end iter, applies the matching tag over the inserted
//! byte range, and returns the rendered buffer's top-level `MdBlock`s (buffer-line
//! spans + a `stoppable` flag) so the journal accent-bar cursor aligns to the
//! painted text and skips heading/rule chrome.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

// ── Pure types ────────────────────────────────────────────────────────────────

/// Visual role of a text run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Style {
    Body,
    H1,
    H2,
    H3,
    Bold,
    Italic,
    BlockQuote,
    ListItem,
    Rule,
    Mono,
}

impl Style {
    /// Whether a block whose leading run has this style is a valid **cursor
    /// stop** for the journal accent bar. Headings and horizontal rules are
    /// chrome — the block cursor must never land on them (the user wants the
    /// accent bar only beside prose paragraphs and list items). Body,
    /// list-item, block-quote and mono (table/code) blocks are stoppable.
    pub fn is_stoppable_block(&self) -> bool {
        !matches!(self, Style::H1 | Style::H2 | Style::H3 | Style::Rule)
    }
}

/// A single styled text run.
#[derive(Debug, Clone)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

/// A rendered top-level block's buffer-line span + whether the accent-bar
/// cursor may stop on it. Produced by `render_markdown_blocks` so the journal
/// overlay can align the bar to the RENDERED buffer (not the raw Markdown
/// source, whose line structure differs) and skip heading/rule chrome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdBlock {
    /// First buffer line (0-based) of the block.
    pub start_line: i32,
    /// Last buffer line (0-based) of the block.
    pub end_line: i32,
    /// True for prose paragraphs / list items / quotes / tables; false for
    /// headings and horizontal rules (never a cursor stop).
    pub stoppable: bool,
}

/// A PLANNED top-level block: the single unit shared by pagination (measure),
/// rendering (insert + tag), and cursor navigation (stop/skip). Splitting the
/// span stream once — instead of paginating raw-Markdown paragraphs while
/// rendering styled blocks — is what keeps page fill, the accent bar, and j/k
/// stepping aligned to the same indices.
#[derive(Debug, Clone)]
pub struct PlannedBlock {
    /// The block's visible styled runs (separator whitespace excluded).
    pub spans: Vec<Span>,
    /// The block's role — the style of its LEADING run (a Bold run inside a
    /// Body paragraph does not change the block's kind).
    pub kind: Style,
    /// Whether the accent-bar cursor may stop here (`kind.is_stoppable_block()`).
    pub stoppable: bool,
}

impl PlannedBlock {
    /// The block's plain text (all runs concatenated) — the TTS / measurement
    /// input.
    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

/// Split `plan_markdown`'s span stream into top-level blocks. A separator is a
/// whitespace-only span CONTAINING a newline (the runs `plan_markdown` emits
/// after each paragraph / heading / rule / list item). A plain `" "` SoftBreak
/// span is NOT a separator — a soft-wrapped source paragraph stays one block.
pub fn plan_markdown_blocks(src: &str) -> Vec<PlannedBlock> {
    let mut blocks: Vec<PlannedBlock> = Vec::new();
    let mut cur: Vec<Span> = Vec::new();

    let close = |cur: &mut Vec<Span>, blocks: &mut Vec<PlannedBlock>| {
        if cur.is_empty() {
            return;
        }
        let kind = cur[0].style.clone();
        let stoppable = kind.is_stoppable_block();
        blocks.push(PlannedBlock { spans: std::mem::take(cur), kind, stoppable });
    };

    for span in plan_markdown(src) {
        if span.text.trim().is_empty() {
            if span.text.contains('\n') {
                close(&mut cur, &mut blocks);
            } else if !cur.is_empty() {
                // SoftBreak space inside a paragraph — keep it in the block.
                cur.push(span);
            }
            continue;
        }
        cur.push(span);
    }
    close(&mut cur, &mut blocks);
    blocks
}

// ── Block metrics (single source for tags AND pagination measurement) ────────

/// Font-scale multiplier a block's leading style renders at.
pub fn block_scale(style: &Style) -> f64 {
    match style {
        Style::H1 => 1.3,
        Style::H2 => 1.2,
        // H3 renders at BODY size (bold only) — it is the subtitle style, and
        // the user wants it no larger than the paragraphs it introduces.
        Style::H3 => 1.0,
        _ => 1.0,
    }
}

/// `(pixels_above, pixels_below)` paragraph spacing for a block of this style.
/// ALL inter-block space comes from these — rendered blocks are joined by a
/// single `\n`, never blank lines, so a standalone Pango layout plus these
/// values measures a page exactly.
pub fn block_spacing(style: &Style) -> (i32, i32) {
    match style {
        Style::H1 => (0, 10),
        // H2 above tuned so rule→H2 (rule_below 6 + 16 = 22) EQUALS
        // subtitle→rule (H3_below 6 + rule_above 16 = 22) — the user flagged
        // the asymmetry twice ("space between hr and heading should equal
        // space between subtitle and hr").
        Style::H2 => (16, 8),
        Style::H3 => (14, 6),
        Style::Rule => (16, 6),
        // Items breathe (was 10 — user: "bullet spacing needs to be improved
        // for readability"): item→item 16, intro-paragraph→list 16, and
        // list→closing-paragraph 18 — still tighter than para→para (18) so a
        // list reads as one unit, but each bullet separates cleanly.
        Style::ListItem => (2, 14),
        Style::BlockQuote => (6, 10),
        Style::Mono => (0, 12),
        _ => (4, 14), // Body paragraph gap (18 para→para with above+below)
    }
}

/// Extra left indent (px) a block of this style renders with, ON TOP of the
/// view's own left margin. Subtracted from the wrap width when measuring.
pub fn block_extra_indent(style: &Style) -> i32 {
    match style {
        Style::ListItem => LIST_INDENT,
        Style::BlockQuote => QUOTE_INDENT,
        _ => 0,
    }
}

/// Whether a block of this style renders bold (headings) — bold glyphs are
/// wider, so measurement must match or a heading that wraps measures short.
pub fn block_is_bold(style: &Style) -> bool {
    matches!(style, Style::H1 | Style::H2 | Style::H3)
}

// ── Pure parser ───────────────────────────────────────────────────────────────

/// Parse `src` (CommonMark) into a flat sequence of `Span`s.
///
/// Fully pure: no GTK, no I/O, unit-testable without a display.
pub fn plan_markdown(src: &str) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    // Style stack — topmost entry is the current style.
    let mut style_stack: Vec<Style> = Vec::new();
    // For list items: whether the current list is ordered + the next ordinal.
    let mut list_stack: Vec<Option<u64>> = Vec::new(); // None = unordered
    // Whether we are inside an Item paragraph/inline.
    let mut in_item = false;
    // Raw-capture accumulator for table / code-block content.
    let mut capture: Option<String> = None;
    // Depth inside a table (we may see nested tags).
    let mut table_depth: i32 = 0;
    // Whether we just started a table row (to suppress leading separator).
    let mut table_row_first_cell = true;

    let parser = Parser::new_ext(src, Options::ENABLE_TABLES);

    let current_style = |stack: &Vec<Style>, in_item: bool| -> Style {
        // When inside a list item, text is always ListItem regardless of emphasis stack
        // — but Bold/Italic inside list items still override.
        if let Some(top) = stack.last() {
            top.clone()
        } else if in_item {
            Style::ListItem
        } else {
            Style::Body
        }
    };

    let push = |spans: &mut Vec<Span>, text: String, style: Style| {
        if text.is_empty() {
            return;
        }
        spans.push(Span { text, style });
    };

    for event in parser {
        match event {
            // ── Block starts ──────────────────────────────────────────────────
            Event::Start(Tag::Heading { level, .. }) => {
                let s = match level {
                    HeadingLevel::H1 => Style::H1,
                    HeadingLevel::H2 => Style::H2,
                    _ => Style::H3,
                };
                style_stack.push(s);
            }
            Event::Start(Tag::Strong) => style_stack.push(Style::Bold),
            Event::Start(Tag::Emphasis) => style_stack.push(Style::Italic),
            Event::Start(Tag::BlockQuote(_)) => style_stack.push(Style::BlockQuote),

            Event::Start(Tag::List(start)) => {
                list_stack.push(start);
            }
            Event::Start(Tag::Item) => {
                in_item = true;
                let marker = match list_stack.last() {
                    Some(Some(n)) => format!("{}. ", n),
                    _ => "• ".to_string(),
                };
                push(&mut spans, marker, Style::ListItem);
                // Advance ordered list counter.
                if let Some(Some(n)) = list_stack.last_mut() {
                    *n += 1;
                }
            }

            // Code block: enter raw-capture mode.
            Event::Start(Tag::CodeBlock(_)) => {
                capture = Some(String::new());
            }
            // Table: enter raw-capture mode, track depth.
            Event::Start(Tag::Table(_)) => {
                table_depth += 1;
                if capture.is_none() {
                    capture = Some(String::new());
                }
                table_row_first_cell = true;
            }
            Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => {
                table_row_first_cell = true;
            }
            Event::Start(Tag::TableCell) => {
                if let Some(ref mut buf) = capture {
                    if !table_row_first_cell {
                        buf.push_str(" | ");
                    }
                    table_row_first_cell = false;
                }
            }

            // ── Block ends ────────────────────────────────────────────────────
            Event::End(TagEnd::Heading(_)) => {
                style_stack.pop();
                push(&mut spans, "\n\n".to_string(), Style::Body);
            }
            Event::End(TagEnd::Paragraph) => {
                push(&mut spans, "\n\n".to_string(), Style::Body);
            }
            Event::End(TagEnd::Strong) | Event::End(TagEnd::Emphasis) => {
                style_stack.pop();
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                style_stack.pop();
                push(&mut spans, "\n".to_string(), Style::Body);
            }
            Event::End(TagEnd::Item) => {
                in_item = false;
                push(&mut spans, "\n".to_string(), Style::Body);
            }
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
                push(&mut spans, "\n".to_string(), Style::Body);
            }

            // Code block end: flush capture as Mono.
            Event::End(TagEnd::CodeBlock) => {
                if let Some(text) = capture.take() {
                    push(&mut spans, text, Style::Mono);
                }
            }
            // Table cell end: nothing extra needed (separator added at next cell start).
            Event::End(TagEnd::TableCell) => {}
            // Table row end: newline between rows.
            Event::End(TagEnd::TableRow) | Event::End(TagEnd::TableHead) => {
                if let Some(ref mut buf) = capture {
                    buf.push('\n');
                }
                table_row_first_cell = true;
            }
            // Table end: flush capture as Mono.
            Event::End(TagEnd::Table) => {
                table_depth -= 1;
                if table_depth == 0 {
                    if let Some(text) = capture.take() {
                        push(&mut spans, text, Style::Mono);
                    }
                }
            }

            // ── Inline content ────────────────────────────────────────────────
            Event::Text(t) => {
                let text = t.to_string();
                if let Some(ref mut buf) = capture {
                    buf.push_str(&text);
                } else {
                    let style = if in_item && style_stack.is_empty() {
                        Style::ListItem
                    } else {
                        current_style(&style_stack, in_item)
                    };
                    push(&mut spans, text, style);
                }
            }
            // Inline code: always Mono, even if inside emphasis etc.
            Event::Code(t) => {
                push(&mut spans, t.to_string(), Style::Mono);
            }
            // Horizontal rule.
            Event::Rule => {
                push(&mut spans, "\n".to_string(), Style::Body);
                push(&mut spans, "─".repeat(40), Style::Rule);
                push(&mut spans, "\n\n".to_string(), Style::Body);
            }

            Event::SoftBreak => {
                if capture.is_none() {
                    push(&mut spans, " ".to_string(), Style::Body);
                }
            }
            Event::HardBreak => {
                if capture.is_none() {
                    push(&mut spans, "\n".to_string(), Style::Body);
                }
            }

            // Ignored events: Html, InlineHtml, FootnoteReference, TaskListMarker, etc.
            _ => {}
        }
    }

    spans
}

// ── GTK layer ─────────────────────────────────────────────────────────────────

/// One `gtk4::TextTag` per `Style`, registered against a `TextTagTable`.
///
/// Build once via `MarkdownTags::register(buffer)` and reuse for every
/// `apply_markdown` call on that buffer.
pub struct MarkdownTags {
    pub body: gtk4::TextTag,
    pub h1: gtk4::TextTag,
    pub h2: gtk4::TextTag,
    pub h3: gtk4::TextTag,
    pub bold: gtk4::TextTag,
    pub italic: gtk4::TextTag,
    pub block_quote: gtk4::TextTag,
    pub list_item: gtk4::TextTag,
    pub rule: gtk4::TextTag,
    pub mono: gtk4::TextTag,
}

/// Hanging-indent left offset for list items (px), ON TOP of the view's left
/// margin (see `set_base_left_margin` — a `TextTag` left-margin REPLACES the
/// view's, it does not add to it).
const LIST_INDENT: i32 = 32;
/// Extra left offset for block quotes (px), on top of the view's left margin.
const QUOTE_INDENT: i32 = 32;
/// Negative first-line indent so the marker hangs left and wrapped text aligns
/// under the text start (past the `• ` / `N. ` prefix).
const LIST_HANG: i32 = -16;

impl MarkdownTags {
    /// Register or look up all markdown tags on `buffer`'s tag table.
    ///
    /// Safe to call multiple times: each tag is looked up by name first;
    /// if already present it is returned as-is.
    pub fn register(buffer: &gtk4::TextBuffer) -> Self {
        use gtk4::pango;
        use gtk4::prelude::*;

        let table = buffer.tag_table();

        let get_or_add = |name: &str, build: gtk4::TextTag| -> gtk4::TextTag {
            if let Some(existing) = table.lookup(name) {
                return existing;
            }
            table.add(&build);
            build
        };

        // Reading serif family — matches the journal/gloss overlay baseline.
        let serif = crate::ui::gloss_overlay::GLOSS_DEFAULT_FONT_FAMILY;
        // Monospace family — matches the in-place vim editor edit font.
        let mono_family = crate::ui::EDIT_FONT_FAMILY;

        // All paragraph spacing (pixels above/below) comes from block_spacing —
        // the SAME values pagination charges per block — so a page measures
        // exactly. Rendered blocks are joined by a single `\n`: no blank lines.
        let sp = block_spacing;

        // Body: serif, paragraph gap below.
        let (a, b) = sp(&Style::Body);
        let body = get_or_add(
            "md-body",
            gtk4::TextTag::builder()
                .name("md-body")
                .family(serif)
                .pixels_above_lines(a)
                .pixels_below_lines(b)
                .build(),
        );

        // Headings (H1 title / H2 section / H3 subtitle): bold serif, scale +
        // spacing entirely from block_scale / block_spacing (H1 ~1.3×, tuned down
        // from 2.0 so the title stays largest without dominating the card, per the
        // claude.ai artifact proportions; H2 ~1.2×; H3 body-size, bold only). One
        // closure builds all three — they differ only by name + Style variant
        // (audit #71).
        let heading = |name: &str, style: Style| {
            let (a, b) = sp(&style);
            get_or_add(
                name,
                gtk4::TextTag::builder()
                    .name(name)
                    .family(serif)
                    .weight(700)
                    .scale(block_scale(&style))
                    .pixels_above_lines(a)
                    .pixels_below_lines(b)
                    .build(),
            )
        };
        let h1 = heading("md-h1", Style::H1);
        let h2 = heading("md-h2", Style::H2);
        let h3 = heading("md-h3", Style::H3);

        // Bold: weight 700, inherits serif.
        let bold = get_or_add(
            "md-bold",
            gtk4::TextTag::builder()
                .name("md-bold")
                .weight(700)
                .build(),
        );

        // Italic: pango italic style.
        let italic = get_or_add(
            "md-italic",
            gtk4::TextTag::builder()
                .name("md-italic")
                .style(pango::Style::Italic)
                .build(),
        );

        // BlockQuote: muted italic. Its left margin is set per-render by
        // `set_base_left_margin` (a tag left-margin REPLACES the view's own
        // margin — an absolute value here would yank the quote to the view's
        // far-left edge, outside the card's centered column).
        let (a, b) = sp(&Style::BlockQuote);
        let block_quote = get_or_add(
            "md-blockquote",
            gtk4::TextTag::builder()
                .name("md-blockquote")
                .foreground("#888888")
                .style(pango::Style::Italic)
                .pixels_above_lines(a)
                .pixels_below_lines(b)
                .build(),
        );

        // ListItem: hanging indent — left margin set per-render by
        // `set_base_left_margin` (same override pitfall as the quote; the
        // absolute 32px it used to bake in rendered lists at the view's far
        // left, spilling outside the card), negative first-line indent.
        let (a, b) = sp(&Style::ListItem);
        let list_item = get_or_add(
            "md-listitem",
            gtk4::TextTag::builder()
                .name("md-listitem")
                .indent(LIST_HANG)
                .pixels_above_lines(a)
                .pixels_below_lines(b)
                .build(),
        );

        // Rule: light-grey foreground on the ─ run (thin hairline look).
        let (a, b) = sp(&Style::Rule);
        let rule = get_or_add(
            "md-rule",
            gtk4::TextTag::builder()
                .name("md-rule")
                .foreground("#cccccc")
                .pixels_above_lines(a)
                .pixels_below_lines(b)
                .build(),
        );

        // Mono (tables/code): monospace family.
        let (a, b) = sp(&Style::Mono);
        let mono = get_or_add(
            "md-mono",
            gtk4::TextTag::builder()
                .name("md-mono")
                .family(mono_family)
                .pixels_above_lines(a)
                .pixels_below_lines(b)
                .build(),
        );

        Self {
            body,
            h1,
            h2,
            h3,
            bold,
            italic,
            block_quote,
            list_item,
            rule,
            mono,
        }
    }

    /// Re-anchor the indent-carrying tags to the VIEW's current left margin.
    /// A `TextTag` left-margin REPLACES the view default (it does not add), so
    /// list/quote margins must be `view_left_margin + indent`, recomputed
    /// whenever the view margin changes (card resize). Call before each render.
    pub fn set_base_left_margin(&self, base: i32) {
        use gtk4::prelude::*;
        self.list_item.set_left_margin(base + LIST_INDENT);
        self.block_quote.set_left_margin(base + QUOTE_INDENT);
    }

    fn tag_for(&self, style: &Style) -> &gtk4::TextTag {
        match style {
            Style::Body => &self.body,
            Style::H1 => &self.h1,
            Style::H2 => &self.h2,
            Style::H3 => &self.h3,
            Style::Bold => &self.bold,
            Style::Italic => &self.italic,
            Style::BlockQuote => &self.block_quote,
            Style::ListItem => &self.list_item,
            Style::Rule => &self.rule,
            Style::Mono => &self.mono,
        }
    }
}

/// Render a slice of PLANNED blocks into `buffer`, one buffer paragraph per
/// block, joined by a single `\n` — NO blank lines (all inter-block space is
/// tag `pixels_above/below`, the same values pagination charges). Each block's
/// kind tag is applied over its whole range INCLUDING the trailing newline so
/// GTK's paragraph-level attributes (margins, pixel spacing) come uniformly
/// from the block tag; inline Bold/Italic/Mono runs are tagged on top.
///
/// Returns each rendered block's buffer-line span + stoppable flag, index-
/// aligned 1:1 with `blocks` — the invariant the journal cursor relies on.
///
/// Appends to whatever is already in `buffer` — callers should call
/// `buffer.set_text("")` first for a clean render.
pub fn render_markdown_blocks(
    buffer: &gtk4::TextBuffer,
    blocks: &[PlannedBlock],
    tags: &MarkdownTags,
) -> Vec<MdBlock> {
    use gtk4::prelude::*;

    let mut out: Vec<MdBlock> = Vec::with_capacity(blocks.len());
    for (i, block) in blocks.iter().enumerate() {
        // A horizontal rule at the TOP of a page is a section break made
        // redundant by the page break itself — suppress its ink (the user
        // flagged these as "orphaned hr"). Keep a degenerate placeholder so
        // the returned list stays index-aligned 1:1 with `blocks` (the
        // invariant the journal cursor projection relies on); it is chrome,
        // so the cursor never lands on it and the bar never reads its lines.
        if i == 0 && block.kind == Style::Rule {
            out.push(MdBlock { start_line: 0, end_line: 0, stoppable: false });
            continue;
        }
        let block_start_off = buffer.end_iter().offset();
        for span in &block.spans {
            let start_offset = buffer.end_iter().offset();
            let mut end_iter = buffer.end_iter();
            buffer.insert(&mut end_iter, &span.text);
            // Inline override tags only — whitespace runs (SoftBreak spaces)
            // carry no styling and must not smuggle Body paragraph attrs into
            // a list/quote block.
            if span.style != block.kind && !span.text.trim().is_empty() {
                let start = buffer.iter_at_offset(start_offset);
                let end = buffer.end_iter();
                buffer.apply_tag(tags.tag_for(&span.style), &start, &end);
            }
        }
        // One newline between blocks (none after the last — no trailing blank
        // paragraph to inflate the page).
        if i + 1 < blocks.len() {
            let mut end_iter = buffer.end_iter();
            buffer.insert(&mut end_iter, "\n");
        }
        let start = buffer.iter_at_offset(block_start_off);
        let end = buffer.end_iter();
        buffer.apply_tag(tags.tag_for(&block.kind), &start, &end);
        out.push(MdBlock {
            start_line: start.line(),
            // `end` sits at the start of the NEXT block (past the newline);
            // the block's own last visible line is one back for non-final
            // blocks. Guard with max() for empty-span blocks.
            end_line: if i + 1 < blocks.len() {
                (end.line() - 1).max(start.line())
            } else {
                end.line()
            },
            stoppable: block.stoppable,
        });
    }
    out
}

/// Pixel height a planned block will render at: its plain text measured by a
/// standalone `pango::Layout` at the block's REAL style (scale, bold, extra
/// indent) plus the block's `pixels_above + pixels_below`. Because the render
/// inserts no blank lines, `Σ measure_planned_block` over a page's blocks is
/// the page's rendered height — pagination and rendering share one unit.
pub fn measure_planned_block(
    pctx: &gtk4::pango::Context,
    block: &PlannedBlock,
    base_size_pt: i32,
    family: &str,
    wrap_px: i32,
) -> i32 {
    use gtk4::pango;

    let layout = pango::Layout::new(pctx);
    let mut desc = pango::FontDescription::from_string(family);
    let scaled = (base_size_pt as f64 * block_scale(&block.kind) * pango::SCALE as f64) as i32;
    desc.set_size(scaled);
    if block_is_bold(&block.kind) {
        desc.set_weight(pango::Weight::Bold);
    }
    // The md-blockquote tag renders the whole block italic; italic glyph
    // widths differ from regular, so the measurement must match or the
    // wrap-line count can disagree with the render.
    if matches!(block.kind, Style::BlockQuote) {
        desc.set_style(pango::Style::Italic);
    }
    layout.set_font_description(Some(&desc));
    let w = (wrap_px - block_extra_indent(&block.kind)).max(1);
    layout.set_width(w * pango::SCALE);
    // List items render with a hanging first line (tag indent LIST_HANG);
    // pango models the same with a negative layout indent.
    if matches!(block.kind, Style::ListItem) {
        layout.set_indent(LIST_HANG * pango::SCALE);
    }
    layout.set_wrap(pango::WrapMode::WordChar);
    // Measure the block as MARKUP mirroring the render's inline tags: bold
    // runs are wider than regular, so a span-heavy paragraph measured as
    // plain text wraps to FEWER lines than it renders — the page over-packs
    // and its tail line clips at the card bottom (user-reported). Inline
    // Style::Mono spans are NOT face-switched here because the journal's
    // font tag overrides the family at render time (rows render serif).
    let markup: String = block
        .spans
        .iter()
        .map(|s| {
            let esc = gtk4::glib::markup_escape_text(&s.text);
            match s.style {
                Style::Bold => format!("<b>{esc}</b>"),
                Style::Italic => format!("<i>{esc}</i>"),
                _ => esc.to_string(),
            }
        })
        .collect();
    layout.set_markup(&markup);
    let (above, below) = block_spacing(&block.kind);
    // A block can render as SEVERAL buffer paragraphs: a table arrives as ONE
    // Mono block whose rows are joined by inner newlines, and GTK applies the
    // tag's pixels_above/below to EVERY paragraph. Charging the spacing once
    // under-counted the sourcing-map table by ~130px, over-packed its page,
    // and clipped the following block's tail at the card bottom.
    let paras = 1 + block.plain_text().matches('\n').count() as i32;
    layout.pixel_size().1 + (above + below) * paras
}

// ── Unit tests (pure — no GTK) ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_and_emphasis_plan() {
        let spans = plan_markdown("## Cry\n\nload **it** now");
        // The heading text appears as an H2 span; "it" appears as a Bold span.
        assert!(spans.iter().any(|s| s.text.contains("Cry")
            && matches!(s.style, Style::H2)));
        assert!(spans.iter().any(|s| s.text == "it" && matches!(s.style, Style::Bold)));
    }

    #[test]
    fn bullet_list_items_are_listitem() {
        let spans = plan_markdown("- one\n- two");
        let items: Vec<_> = spans.iter().filter(|s| matches!(s.style, Style::ListItem)).collect();
        assert!(items.iter().any(|s| s.text.contains("one")));
        assert!(items.iter().any(|s| s.text.contains("two")));
    }

    #[test]
    fn table_becomes_mono() {
        let spans = plan_markdown("| a | b |\n|---|---|\n| 1 | 2 |");
        assert!(spans.iter().any(|s| matches!(s.style, Style::Mono)));
    }

    #[test]
    fn soft_break_does_not_split_a_paragraph_block() {
        // A source paragraph hard-wrapped across lines (SoftBreak) is ONE
        // block — the " " SoftBreak span must not be mistaken for a
        // block separator (only newline-bearing whitespace separates).
        let blocks = plan_markdown_blocks("first line\nsecond line of same para");
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].plain_text().contains("first line second line"));
    }

    #[test]
    fn blocks_split_and_flag_chrome() {
        let blocks =
            plan_markdown_blocks("# Title\n\nA para.\n\n---\n\n## Section\n\n1. one\n2. two");
        let kinds: Vec<(Style, bool)> =
            blocks.iter().map(|b| (b.kind.clone(), b.stoppable)).collect();
        assert_eq!(
            kinds,
            vec![
                (Style::H1, false),
                (Style::Body, true),
                (Style::Rule, false),
                (Style::H2, false),
                (Style::ListItem, true),
                (Style::ListItem, true),
            ]
        );
        assert_eq!(blocks[4].plain_text(), "1. one");
        assert_eq!(blocks[5].plain_text(), "2. two");
    }

    #[test]
    fn bold_inside_paragraph_stays_one_body_block() {
        let blocks = plan_markdown("load **it** now")
            .into_iter()
            .collect::<Vec<_>>();
        assert!(blocks.iter().any(|s| matches!(s.style, Style::Bold)));
        let planned = plan_markdown_blocks("load **it** now");
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].kind, Style::Body);
        assert_eq!(planned[0].plain_text(), "load it now");
    }

    #[test]
    fn rule_gaps_are_symmetric() {
        // The gap ABOVE a rule (subtitle→rule) must equal the gap BELOW it
        // (rule→section heading) — user-flagged asymmetry, twice.
        let (_, h3_below) = block_spacing(&Style::H3);
        let (rule_above, rule_below) = block_spacing(&Style::Rule);
        let (h2_above, _) = block_spacing(&Style::H2);
        assert_eq!(h3_below + rule_above, rule_below + h2_above);
    }

    #[test]
    fn list_items_tighter_than_paragraphs_but_breathing() {
        // item→item < item→paragraph <= paragraph→paragraph, all nonzero.
        let (item_above, item_below) = block_spacing(&Style::ListItem);
        let (body_above, body_below) = block_spacing(&Style::Body);
        let item_gap = item_below + item_above;
        let item_para_gap = item_below + body_above;
        let para_gap = body_below + body_above;
        assert!(item_gap >= 8, "items need breathing room, got {item_gap}");
        assert!(item_gap < item_para_gap);
        assert!(item_para_gap <= para_gap);
    }

    #[test]
    fn headings_and_rules_are_not_cursor_stops() {
        // The accent bar must never land on a heading or a horizontal rule —
        // only on prose paragraphs, list items, quotes, and tables.
        assert!(!Style::H1.is_stoppable_block());
        assert!(!Style::H2.is_stoppable_block());
        assert!(!Style::H3.is_stoppable_block());
        assert!(!Style::Rule.is_stoppable_block());
        assert!(Style::Body.is_stoppable_block());
        assert!(Style::ListItem.is_stoppable_block());
        assert!(Style::BlockQuote.is_stoppable_block());
        assert!(Style::Mono.is_stoppable_block());
    }
}
