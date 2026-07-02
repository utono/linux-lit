//! CommonMark → GTK4 `TextTag` renderer.
//!
//! **Pure layer:** `plan_markdown(src)` parses a Markdown string via
//! `pulldown-cmark` and returns `Vec<Span>` — no GTK, fully unit-testable.
//!
//! **GTK layer:** `MarkdownTags` holds one `gtk4::TextTag` per `Style`;
//! `apply_markdown(buffer, src, tags)` walks `plan_markdown`, inserts each
//! span's text at the buffer end iter, and applies the matching tag over the
//! inserted byte range.

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

/// A single styled text run.
#[derive(Debug, Clone)]
pub struct Span {
    pub text: String,
    pub style: Style,
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

/// Hanging-indent left margin for list items (px).
const LIST_INDENT: i32 = 32;
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

        // Body: serif, paragraph leading below.
        let body = get_or_add(
            "md-body",
            gtk4::TextTag::builder()
                .name("md-body")
                .family(serif)
                .pixels_below_lines(6)
                .build(),
        );

        // H1 (title): bold, ~2× scale, space below.
        let h1 = get_or_add(
            "md-h1",
            gtk4::TextTag::builder()
                .name("md-h1")
                .family(serif)
                .weight(700)
                .scale(2.0)
                .pixels_below_lines(12)
                .build(),
        );

        // H2 (section): bold, ~1.3× scale, space above.
        let h2 = get_or_add(
            "md-h2",
            gtk4::TextTag::builder()
                .name("md-h2")
                .family(serif)
                .weight(700)
                .scale(1.3)
                .pixels_above_lines(16)
                .pixels_below_lines(6)
                .build(),
        );

        // H3 (subtitle): bold, ~1.15× scale, small space below.
        let h3 = get_or_add(
            "md-h3",
            gtk4::TextTag::builder()
                .name("md-h3")
                .family(serif)
                .weight(700)
                .scale(1.15)
                .pixels_below_lines(4)
                .build(),
        );

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

        // BlockQuote: left-margin bump + muted foreground.
        let block_quote = get_or_add(
            "md-blockquote",
            gtk4::TextTag::builder()
                .name("md-blockquote")
                .left_margin(32)
                .foreground("#888888")
                .style(pango::Style::Italic)
                .build(),
        );

        // ListItem: hanging indent — left-margin with negative first-line indent.
        let list_item = get_or_add(
            "md-listitem",
            gtk4::TextTag::builder()
                .name("md-listitem")
                .left_margin(LIST_INDENT)
                .indent(LIST_HANG)
                .build(),
        );

        // Rule: light-grey foreground on the ─ run (thin hairline look).
        let rule = get_or_add(
            "md-rule",
            gtk4::TextTag::builder()
                .name("md-rule")
                .foreground("#cccccc")
                .build(),
        );

        // Mono (tables/code): monospace family.
        let mono = get_or_add(
            "md-mono",
            gtk4::TextTag::builder()
                .name("md-mono")
                .family(mono_family)
                .pixels_below_lines(2)
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

/// Parse `src` as CommonMark, insert all text at the end of `buffer`, and
/// apply the matching tag from `tags` over each inserted run.
///
/// Appends to whatever is already in `buffer` — callers should call
/// `buffer.set_text("")` first if a clean render is desired.
pub fn apply_markdown(buffer: &gtk4::TextBuffer, src: &str, tags: &MarkdownTags) {
    use gtk4::prelude::*;

    for span in plan_markdown(src) {
        let start_offset = buffer.end_iter().offset();
        let mut end_iter = buffer.end_iter();
        buffer.insert(&mut end_iter, &span.text);
        let start = buffer.iter_at_offset(start_offset);
        let end = buffer.end_iter();
        buffer.apply_tag(tags.tag_for(&span.style), &start, &end);
    }
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
}
