use crate::ui::gloss_block::{parse_gloss_tags, GlossElement};
use crate::ui::gloss_ipa::{strip_brackets, strip_ipa};
use crate::ui::gloss_util::split_echo;
use gtk4::pango;
use gtk4::prelude::*;

/// A buffer-line span to paint an accent bar beside (start_line..=end_line).
pub(crate) struct BarRange {
    pub(crate) start_line: i32,
    pub(crate) end_line: i32,
}

/// A verse-line number annotation: which buffer line carries a numbered source line.
pub(crate) struct LineNumber {
    pub(crate) buffer_line: i32,
    pub(crate) number: i64,
}

/// Apply italic styling to any `[bracket]` spans found after `base_offset` in
/// the buffer, using `bracket_tag`.
pub(crate) fn apply_bracket_styling(
    buffer: &gtk4::TextBuffer,
    base_offset: i32,
    bracket_tag: &gtk4::TextTag,
) {
    let text = buffer.text(
        &buffer.iter_at_offset(base_offset),
        &buffer.end_iter(),
        false,
    );
    let text_str = text.as_str();
    let mut pos = 0;
    while pos < text_str.len() {
        if let Some(open) = text_str[pos..].find('[') {
            let abs_open = pos + open;
            if let Some(close) = text_str[abs_open..].find(']') {
                let abs_close = abs_open + close + 1;
                let start = buffer.iter_at_offset(base_offset + abs_open as i32);
                let end = buffer.iter_at_offset(base_offset + abs_close as i32);
                buffer.apply_tag(bracket_tag, &start, &end);
                pos = abs_close;
            } else {
                break;
            }
        } else {
            break;
        }
    }
}

/// Populate `view`'s buffer with a rendered gloss/passage doc (no echo
/// highlight). Delegates to `populate_verse_buffer` with `selected_echo: None`.
pub(crate) fn populate_gloss_buffer(
    view: &gtk4::TextView,
    gloss: &str,
    _text_margins: i32,
    bar_left: i32,
    source_line_numbers: &[(String, i64)],
    gloss_dim: Option<&str>,
    speaker_accent: Option<&str>,
) -> (Vec<BarRange>, Vec<LineNumber>) {
    let (ranges, nums, _) = populate_verse_buffer(
        view,
        gloss,
        _text_margins,
        bar_left,
        source_line_numbers,
        None,
        gloss_dim,
        speaker_accent,
    );
    (ranges, nums)
}

/// Populate `view`'s buffer with a stage-aware verse rendering of `doc`.
///
/// Supports `<speaker>`, `<verse>`, `<stage>`, `<gloss>`, `<pron>` markup.
/// Returns `(bar_ranges, line_numbers, echo_lines)`.
///
/// - `bar_ranges`: accent bar spans for the selected echo (if any).
/// - `line_numbers`: buffer-line → verse line-number mappings.
/// - `echo_lines`: buffer line of each `<gloss>` echo's first quote line.
///
/// This is the shared renderer used by the gloss overlay and the journal overlay.
pub(crate) fn populate_verse_buffer(
    view: &gtk4::TextView,
    gloss: &str,
    _text_margins: i32,
    bar_left: i32,
    source_line_numbers: &[(String, i64)],
    selected_echo: Option<usize>,
    dim_color: Option<&str>,
    speaker_accent: Option<&str>,
) -> (Vec<BarRange>, Vec<LineNumber>, Vec<i32>) {
    let buffer = view.buffer();
    buffer.set_text("");

    let tag_table = buffer.tag_table();
    for name in &[
        "gloss-speaker",
        "gloss-speaker-first",
        "gloss-speaker-source",
        "gloss-verse",
        "gloss-stage",
        "gloss-para",
        "gloss-bracket",
        "gloss-quote",
        "gloss-quote-cont",
        "gloss-citation",
        "gloss-pron",
    ] {
        if let Some(old) = tag_table.lookup(name) {
            tag_table.remove(&old);
        }
    }

    let quote_speaker = bar_left + 60;
    let quote_verse = quote_speaker + 60;

    // Speaker headings: small-caps, tinted with the accent (root) color so they
    // read as structural labels rather than body text. Falls back to inherited
    // fg when no accent is supplied.
    let apply_accent = |b: gtk4::builders::TextTagBuilder| -> gtk4::builders::TextTagBuilder {
        match speaker_accent {
            Some(c) => b.foreground(c),
            None => b,
        }
    };

    let speaker_tag = apply_accent(
        gtk4::TextTag::builder()
            .name("gloss-speaker")
            .variant(pango::Variant::SmallCaps)
            .weight(400)
            .scale(0.75)
            .left_margin(quote_speaker)
            .pixels_above_lines(36),
    )
    .build();

    let verse_tag = gtk4::TextTag::builder()
        .name("gloss-verse")
        .left_margin(quote_verse)
        .build();

    // Stage direction inside the quoted source turn: same indent as verse, but
    // italic — matching the main reading card. Not a cursor stop, not TTS.
    let stage_tag = gtk4::TextTag::builder()
        .name("gloss-stage")
        .left_margin(quote_verse)
        .style(pango::Style::Italic)
        .build();

    // Prose gloss recedes behind the verse it explains: dimmer color, slightly
    // smaller, looser line spacing for the dense commentary. The verse stays the
    // full-ink "hero".
    let para_builder = gtk4::TextTag::builder()
        .name("gloss-para")
        .left_margin(quote_speaker)
        .pixels_above_lines(24)
        .pixels_below_lines(6)
        .scale(0.92);
    let para_tag = match dim_color {
        Some(c) => para_builder.foreground(c).build(),
        None => para_builder.build(),
    };

    let speaker_first_tag = apply_accent(
        gtk4::TextTag::builder()
            .name("gloss-speaker-first")
            .variant(pango::Variant::SmallCaps)
            .weight(400)
            .scale(0.75)
            .left_margin(quote_speaker),
    )
    .build();

    // Speaker label inside the quoted source turn (before the echo list). The
    // turn may span several speakers; keep them tightly spaced to match the
    // reader's 8px speaker rhythm rather than the 36px echo-section gap.
    let speaker_source_tag = apply_accent(
        gtk4::TextTag::builder()
            .name("gloss-speaker-source")
            .variant(pango::Variant::SmallCaps)
            .weight(400)
            .scale(0.75)
            .left_margin(quote_speaker)
            .pixels_above_lines(8),
    )
    .build();

    let bracket_tag = gtk4::TextTag::builder()
        .name("gloss-bracket")
        .style(pango::Style::Italic)
        .scale(0.9)
        .build();

    // Echo quote line: same indent as the paragraph, italic.
    let quote_tag = gtk4::TextTag::builder()
        .name("gloss-quote")
        .left_margin(quote_speaker)
        .pixels_above_lines(24)
        .style(pango::Style::Italic)
        .build();

    // Continuation line of a multi-line verse echo: no top spacing.
    let quote_cont_tag = gtk4::TextTag::builder()
        .name("gloss-quote-cont")
        .left_margin(quote_speaker)
        .style(pango::Style::Italic)
        .build();

    // Citation line: indented further, smaller and dimmer. Use the theme's
    // dim foreground when provided so the source citations recede behind the
    // echo quotes.
    let citation_builder = gtk4::TextTag::builder()
        .name("gloss-citation")
        .left_margin(quote_verse)
        .scale(0.85);
    let citation_tag = match dim_color {
        Some(c) => citation_builder.foreground(c).build(),
        None => citation_builder.build(),
    };

    // Pronunciation teaching note beneath its verse block: italic and slightly
    // smaller (like the bracket tag), dimmed with the theme's dim foreground
    // (like the citation/para tags) so it reads as a recessed teaching aside.
    let pron_builder = gtk4::TextTag::builder()
        .name("gloss-pron")
        .left_margin(quote_verse)
        .style(pango::Style::Italic)
        .scale(0.92);
    let pron_tag = match dim_color {
        Some(c) => pron_builder.foreground(c).build(),
        None => pron_builder.build(),
    };

    tag_table.add(&speaker_tag);
    tag_table.add(&speaker_first_tag);
    tag_table.add(&speaker_source_tag);
    tag_table.add(&verse_tag);
    tag_table.add(&stage_tag);
    tag_table.add(&para_tag);
    tag_table.add(&bracket_tag);
    tag_table.add(&quote_tag);
    tag_table.add(&quote_cont_tag);
    tag_table.add(&citation_tag);
    tag_table.add(&pron_tag);

    let elements = parse_gloss_tags(gloss);
    let mut first = true;
    let mut only_speakers_so_far = true;
    // Whether we have reached the echo list (`<gloss>` elements). Speaker
    // labels before this belong to the quoted source turn and stay tight.
    let mut in_echoes = false;
    let mut bar_ranges: Vec<BarRange> = Vec::new();
    let mut line_nums: Vec<LineNumber> = Vec::new();
    let mut echo_lines: Vec<i32> = Vec::new();
    let mut echo_idx: usize = 0;

    // Build lookup: trimmed verse text → line_in_div
    let line_lookup: std::collections::HashMap<&str, i64> = source_line_numbers
        .iter()
        .map(|(text, num)| (text.trim(), *num))
        .collect();

    for el in &elements {
        if !first {
            let mut end = buffer.end_iter();
            buffer.insert(&mut end, "\n");
        }
        first = false;

        let line = buffer.end_iter().line();
        let offset = buffer.end_iter().offset();
        match el {
            GlossElement::Speaker(name) => {
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, name);
                let start = buffer.iter_at_offset(offset);
                let tag = if only_speakers_so_far {
                    &speaker_first_tag
                } else if in_echoes {
                    &speaker_tag
                } else {
                    // Subsequent speaker within the quoted source turn: tight.
                    &speaker_source_tag
                };
                buffer.apply_tag(tag, &start, &buffer.end_iter());
            }
            GlossElement::Verse(text) => {
                only_speakers_so_far = false;
                let shown = strip_ipa(text);
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, &shown);
                let start = buffer.iter_at_offset(offset);
                buffer.apply_tag(&verse_tag, &start, &buffer.end_iter());
                apply_bracket_styling(&buffer, offset, &bracket_tag);

                // line-number gutter: match on bracket+IPA-stripped, trimmed text
                let stripped = strip_brackets(&shown);
                if let Some(&num) = line_lookup.get(stripped.trim()) {
                    line_nums.push(LineNumber {
                        buffer_line: line,
                        number: num,
                    });
                }
            }
            GlossElement::Gloss(text) => {
                only_speakers_so_far = false;
                in_echoes = true;

                if let Some((quote, citation)) = split_echo(text) {
                    let quote = strip_ipa(&quote);
                    let citation = strip_ipa(&citation);
                    // Echo: quote on one line, citation indented below it.
                    let quote_line = buffer.end_iter().line();
                    echo_lines.push(quote_line);
                    let is_selected = selected_echo == Some(echo_idx);
                    echo_idx += 1;

                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, &quote);
                    let qstart = buffer.iter_at_offset(offset);
                    let quote_end_offset = buffer.end_iter().offset();
                    let quote_end_iter = buffer.iter_at_offset(quote_end_offset);

                    // Apply quote_tag (with top spacing) to the first visual
                    // line, quote_cont_tag (no spacing) to continuation lines.
                    let first_line_end = {
                        let mut it = qstart.clone();
                        if !it.ends_line() {
                            it.forward_to_line_end();
                        }
                        if it.offset() > quote_end_offset {
                            quote_end_iter.clone()
                        } else {
                            it
                        }
                    };
                    buffer.apply_tag(&quote_tag, &qstart, &first_line_end);
                    if first_line_end.offset() < quote_end_offset {
                        buffer.apply_tag(&quote_cont_tag, &first_line_end, &quote_end_iter);
                    }

                    // The left accent bar (bar_ranges, below) marks the
                    // selected echo; no background highlight needed.

                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, "\n");
                    let cit_offset = buffer.end_iter().offset();
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, &citation);
                    let cstart = buffer.iter_at_offset(cit_offset);
                    buffer.apply_tag(&citation_tag, &cstart, &buffer.end_iter());

                    // Accent bar beside the selected echo: span the quote's
                    // first line through the citation line.
                    if is_selected {
                        bar_ranges.push(BarRange {
                            start_line: quote_line,
                            end_line: buffer.end_iter().line(),
                        });
                    }
                } else {
                    let shown = strip_ipa(text);
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, &shown);
                    let start = buffer.iter_at_offset(offset);
                    buffer.apply_tag(&para_tag, &start, &buffer.end_iter());
                }
            }
            GlossElement::Pron(_) => {
                only_speakers_so_far = false;
                // <pron> notes are no longer shown to the reader: IPA is not
                // helpful pedagogy and is TTS-only. Already-stored notes are
                // silently dropped from display. (The tag stays defined; just
                // unused now.)
            }
            GlossElement::Stage(text) => {
                only_speakers_so_far = false;
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, text);
                let start = buffer.iter_at_offset(offset);
                buffer.apply_tag(&stage_tag, &start, &buffer.end_iter());
                // No line-number gutter entry: stage directions are not numbered
                // verse lines.
            }
        }
    }

    (bar_ranges, line_nums, echo_lines)
}
