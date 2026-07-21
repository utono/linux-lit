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

/// Left inset (past the accent bar) of the explication body — flush, the focus.
/// 12 → 20 (2026-07-02 readability pass): the bar was hugging the text.
pub(crate) const QUOTE_BODY_INDENT: i32 = 20;
/// Left inset (past the accent bar) of the speaker label heading the quoted
/// source turn. Echo quotes and citations share this indent.
pub(crate) const QUOTE_SPEAKER_INDENT: i32 = 48;
/// Left inset (past the accent bar) of the quoted source verse WHEN the turn
/// has a speaker label: one main-card dialogue step past the label, so the
/// source turn reads with the same speaker→dialogue hang-indent as the main
/// reading card. A speakerless source (prose works) has nothing to hang past,
/// so its lines sit at `QUOTE_SPEAKER_INDENT` instead — the deep indent read
/// as arbitrary there. This is the DEEPEST indent any gloss block uses — the
/// paginated height measurement (`GlossOverlay::repaginate`) subtracts it from
/// the wrap width so estimates over-count (never clip).
pub(crate) const QUOTE_VERSE_INDENT: i32 = QUOTE_SPEAKER_INDENT + crate::app::DIALOGUE_INDENT;

/// Apply italic styling to any `[bracket]` spans found after `base_offset` in
/// the buffer, using `bracket_tag`.
/// Core of the header tag style shared by the speaker/verse header tags
/// (audit #55): bold 700 at the card's FULL body size, indented to
/// `left_margin`. All overlay text renders at the main card's font size (the
/// user's rule); the header reads as a header through weight + indent alone.
/// Speaker callers override with `.scale(0.75)` — the one sanctioned shrink,
/// matching the main card's own speaker-name scale. Each caller adds its own
/// small-caps variant, pixels_above/below, and (via the `header_dim` closure)
/// the optional echoes-view dim color.
fn header_tag_base(name: &str, left_margin: i32) -> gtk4::builders::TextTagBuilder {
    gtk4::TextTag::builder()
        .name(name)
        .weight(700)
        .left_margin(left_margin)
}

/// True when `name` is a speaker label the renderer will actually display.
/// `populate_verse_buffer` drops empty / "UNKNOWN" labels (stored prose glosses
/// carry `<speaker>UNKNOWN</speaker>`), so anything deciding indent or height
/// by "does this doc have a speaker" must apply the SAME filter — a raw
/// `contains("<speaker>")` check sees the UNKNOWN tag and diverges.
pub(crate) fn is_displayed_speaker(name: &str) -> bool {
    !(name.trim().is_empty() || name.eq_ignore_ascii_case("UNKNOWN"))
}

/// True when `markup` contains a speaker label that will actually render
/// (parsed with the real tag parser + the render filter above). Used by
/// `GlossOverlay::repaginate` so measurement matches `populate_verse_buffer`.
pub(crate) fn markup_has_displayed_speaker(markup: &str) -> bool {
    parse_gloss_tags(markup)
        .iter()
        .any(|el| matches!(el, GlossElement::Speaker(n) if is_displayed_speaker(n)))
}

/// The PLAIN rendered text `populate_verse_buffer` would insert for `markup`,
/// with NO GTK buffer and NO tags — purely the character sequence.
///
/// This MUST mirror `populate_verse_buffer`'s insertion logic exactly (same
/// element filter, same `"\n"` separators, same per-element transforms), because
/// the gloss rewrite-diff highlight indexes into the rendered buffer text: to map
/// full-body diff ranges onto a paginated page we need each page's rendered char
/// length, and to compute the diff against the WHOLE gloss (not just the visible
/// page-1 buffer) we need the whole gloss's rendered text. Keep the two in sync.
///
/// Note the `<pron>` / dropped-speaker subtlety: `populate_verse_buffer` runs the
/// `if !first { insert "\n" }` separator for EVERY surviving element before the
/// per-element match, and `<pron>` elements insert no body — so a `<pron>` that is
/// not the first element still contributes a leading `"\n"`. Dropped speakers are
/// filtered out entirely (they never reach the separator), matching the render.
pub(crate) fn rendered_verse_text(markup: &str) -> String {
    let elements: Vec<GlossElement> = parse_gloss_tags(markup)
        .into_iter()
        .filter(|el| !matches!(el, GlossElement::Speaker(n) if !is_displayed_speaker(n)))
        .collect();

    let mut out = String::new();
    let mut first = true;
    for el in &elements {
        if !first {
            out.push('\n');
        }
        first = false;
        match el {
            GlossElement::Speaker(name) => out.push_str(name),
            GlossElement::Segment(text) => {
                out.push_str(&crate::ui::gloss_block::strip_hi_spans(&strip_ipa(text)).0);
            }
            GlossElement::Gloss(text) => {
                if let Some((quote, citation)) = split_echo(text) {
                    out.push_str(&strip_ipa(&quote));
                    out.push('\n');
                    out.push_str(&strip_ipa(&citation));
                } else {
                    out.push_str(&crate::ui::gloss_block::strip_hi_spans(&strip_ipa(text)).0);
                }
            }
            GlossElement::Stage(text) => {
                out.push_str(&crate::ui::gloss_block::strip_hi_spans(text).0);
            }
            // <pron> inserts no body text (see doc comment): the separator above
            // already ran.
            GlossElement::Pron(_) => {}
        }
    }
    out
}

/// One display row of a gloss rendered for the chat panel's transcript,
/// carrying enough type information for the caller to apply a distinct CSS
/// class per row (the panel's `TranscriptRow::Answer` is a single plain
/// label, so styling happens by emitting several typed rows instead of one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatGlossRowKind {
    /// Speaker name heading a quoted source turn (e.g. "CYMBELINE").
    Speaker,
    /// A line of quoted verse/prose source text.
    Verse,
    /// A stage direction inside the quoted source turn.
    Stage,
    /// Explication prose — the gloss's own commentary (the "focus" body).
    Gloss,
}

/// Split raw gloss markup (`<speaker>`/`<segment>`/`<stage>`/`<gloss>`/`<pron>`
/// tags, as stored in lit.db) into typed display rows, so a caller with only
/// plain-label widgets (no `TextTag` styling) can still visually distinguish
/// the quoted source from the model's own commentary.
///
/// Mirrors `populate_verse_buffer`'s element filter (drops empty/"UNKNOWN"
/// speakers, drops `<pron>` notes — TTS-only, no display text) and
/// `rendered_verse_text`'s per-element cleanup (`strip_ipa`, `strip_hi_spans`,
/// echo-bracket quote/citation split). Returns one row per source LINE (each
/// `<segment>` may itself carry embedded `\n`-joined lines from
/// `gloss_block_markups`, so those are split too) rather than joining verse
/// lines into one paragraph — the chat panel wraps each row as its own
/// label, so a joined multi-line block would lose its line breaks.
///
/// Returns an empty `Vec` when `markup` contains none of the recognized tags
/// (e.g. a plain prose journal-Q&A answer) — callers should treat that as
/// "not gloss markup" and fall back to rendering the text as-is.
pub(crate) fn chat_gloss_rows(markup: &str) -> Vec<(ChatGlossRowKind, String)> {
    let elements = parse_gloss_tags(markup);
    let mut rows: Vec<(ChatGlossRowKind, String)> = Vec::new();
    let mut prev_was_verse_el = false;
    for el in &elements {
        match el {
            GlossElement::Speaker(name) => {
                if is_displayed_speaker(name) {
                    rows.push((ChatGlossRowKind::Speaker, name.clone()));
                }
            }
            GlossElement::Segment(text) => {
                // Two consecutive <segment> ELEMENTS are two source SEGMENTS
                // (prose paragraphs): render a blank line between them so the
                // segments read separately (the user's rule). Lines WITHIN one
                // element stay contiguous.
                if prev_was_verse_el {
                    rows.push((ChatGlossRowKind::Verse, String::new()));
                }
                let clean = crate::ui::gloss_block::strip_hi_spans(&strip_ipa(text)).0;
                for line in clean.lines() {
                    if !line.trim().is_empty() {
                        rows.push((ChatGlossRowKind::Verse, line.to_string()));
                    }
                }
            }
            GlossElement::Stage(text) => {
                let clean = crate::ui::gloss_block::strip_hi_spans(text).0;
                if !clean.trim().is_empty() {
                    rows.push((ChatGlossRowKind::Stage, clean));
                }
            }
            GlossElement::Gloss(text) => {
                if let Some((quote, citation)) = split_echo(text) {
                    rows.push((ChatGlossRowKind::Verse, strip_ipa(&quote)));
                    rows.push((ChatGlossRowKind::Stage, strip_ipa(&citation)));
                } else {
                    let clean = crate::ui::gloss_block::strip_hi_spans(&strip_ipa(text)).0;
                    if !clean.trim().is_empty() {
                        rows.push((ChatGlossRowKind::Gloss, clean));
                    }
                }
            }
            // <pron> is TTS-only (see rendered_verse_text's doc comment): no
            // display row.
            GlossElement::Pron(_) => {}
        }
        prev_was_verse_el = matches!(el, GlossElement::Segment(_));
    }
    rows
}

/// Re-flow every `<segment>` body in a gloss markup into ONE line (breaks →
/// single spaces) so PROSE source passages wrap at the panel width instead of
/// keeping the txt's own ~74-column breaks. Verse works must never pass
/// through here — their line breaks are the text. Everything outside
/// `<segment>…</segment>` (gloss commentary, `<stage>`, `<speaker>`) is untouched,
/// and a dangling unclosed `<segment>` is left as-is rather than eaten.
pub(crate) fn reflow_verse_markup(markup: &str) -> String {
    const OPEN: &str = "<segment>";
    const CLOSE: &str = "</segment>";
    let mut out = String::with_capacity(markup.len());
    let mut rest = markup;
    while let Some(start) = rest.find(OPEN) {
        let body_start = start + OPEN.len();
        let Some(body_len) = rest[body_start..].find(CLOSE) else { break };
        out.push_str(&rest[..body_start]);
        let joined: Vec<&str> = rest[body_start..body_start + body_len]
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        out.push_str(&joined.join(" "));
        out.push_str(CLOSE);
        rest = &rest[body_start + body_len + CLOSE.len()..];
    }
    out.push_str(rest);
    out
}

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

/// Apply the `gloss-hi` highlight background over each `(start, end)` CHAR range
/// (relative to `base_offset`) of a just-inserted body piece. Ranges come from
/// `gloss_block::strip_hi_spans` on the already-IPA-stripped text, so they index
/// the SHOWN text. No-op when `ranges` is empty.
pub(crate) fn apply_hi_ranges(
    buffer: &gtk4::TextBuffer,
    base_offset: i32,
    ranges: &[(usize, usize)],
    hi_tag: &gtk4::TextTag,
) {
    for &(s, e) in ranges {
        let start = buffer.iter_at_offset(base_offset + s as i32);
        let end = buffer.iter_at_offset(base_offset + e as i32);
        buffer.apply_tag(hi_tag, &start, &end);
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
) -> (Vec<BarRange>, Vec<LineNumber>) {
    let (ranges, nums, _) = populate_verse_buffer(
        view,
        gloss,
        _text_margins,
        bar_left,
        source_line_numbers,
        None,
        gloss_dim,
    );
    (ranges, nums)
}

/// Populate `view`'s buffer with a stage-aware verse rendering of `doc`.
///
/// Supports `<speaker>`, `<segment>`, `<stage>`, `<gloss>`, `<pron>` markup.
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

    // The `<hi>` highlight background tag. Created here (color is a neutral
    // default); the overlay re-asserts the theme color in `apply_font` (like it
    // does for `synopsis-label`). Reused across renders, so look up first.
    let hi_tag = match tag_table.lookup("gloss-hi") {
        Some(t) => t,
        None => {
            let t = gtk4::TextTag::builder()
                .name("gloss-hi")
                .background(crate::ui::DEFAULT_HIGHLIGHT_BG)
                .build();
            tag_table.add(&t);
            t
        }
    };

    // Drop empty / "UNKNOWN" speaker labels: prose works (novels) have no
    // speaker, and a stored gloss may carry `<speaker>UNKNOWN</speaker>`. Filtering
    // here (rather than skipping mid-loop) keeps the spacing logic correct — the
    // first real element becomes `first`. New glosses no longer emit the tag at
    // all (build_source_header), so this guards pre-existing stored data.
    let elements: Vec<GlossElement> = parse_gloss_tags(gloss)
        .into_iter()
        .filter(|el| !matches!(el, GlossElement::Speaker(n) if !is_displayed_speaker(n)))
        .collect();
    let has_speaker = elements
        .iter()
        .any(|el| matches!(el, GlossElement::Speaker(_)));

    // The explication (FOCUS body) sits flush ~12px right of the accent bar, like
    // the journal answer. The speaker label + verse (the dim source header) are
    // INDENTED further right so the source reads as a set-off block quote above
    // the flush explication; when the turn has a speaker label the verse hangs
    // one main-card dialogue step PAST it, keeping the reading card's
    // speaker→dialogue rhythm. A speakerless source (prose) sits at the speaker
    // level — there is no label to hang past, and the deep indent read as
    // arbitrary over-indentation.
    let quote_body = bar_left + QUOTE_BODY_INDENT; // explication (gloss-para) — flush, the focus
    let quote_speaker = bar_left + QUOTE_SPEAKER_INDENT;
    let quote_verse = bar_left
        + if has_speaker {
            QUOTE_VERSE_INDENT
        } else {
            QUOTE_SPEAKER_INDENT
        };

    // Speaker + verse "header" styling: bold 700 at full body size, space
    // below, the speaker in small-caps (it's a name label). The header renders in the
    // FULL body foreground on the gloss surfaces (no color threaded — the
    // dimmed source read as too recessed); only the ECHOES view passes
    // `dim_color`, so its fixed source-turn header still recedes behind the
    // echo list.
    let header_dim = |b: gtk4::builders::TextTagBuilder| -> gtk4::builders::TextTagBuilder {
        match dim_color {
            Some(c) => b.foreground(c),
            None => b,
        }
    };

    let speaker_tag = header_dim(
        header_tag_base("gloss-speaker", quote_speaker)
            .variant(pango::Variant::SmallCaps)
            // Match the main reading card's speaker-name size: the card uses
            // scale 0.75 (see formatting.rs `speaker-name`) — the one
            // sanctioned shrink; the verse tag renders at full body size.
            .scale(0.75)
            // EVEN RHYTHM with the source→gloss gap: gloss → next speaker is
            // the explication's pixels_below_lines(10) + this 14 = 24, equal
            // to the explication's own pixels_above_lines(24). The former 36
            // made the between-unit gap (10+36=46) read almost double the
            // within-unit gap — the same unevenness the speakerless overlay's
            // 20/20 fix removed.
            .pixels_above_lines(14)
            .pixels_below_lines(10),
    )
    .build();

    let verse_tag = header_dim(
        header_tag_base("gloss-verse", quote_verse),
        // NO pixels_below_lines: each verse line is its own paragraph, so a
        // per-line gap reads as loose double-spacing. Verse lines sit at the
        // view's natural single leading (like the journal answer body); the
        // speaker tag's pixels_below_lines(10) supplies the gap ABOVE the verse.
    )
    .build();

    // Stage direction inside the quoted source turn: italic, at the SPEAKER
    // level — matching the main reading card, where stage directions sit at the
    // speaker margin and dialogue hang-indents past them. Not a cursor stop,
    // not TTS.
    let stage_tag = gtk4::TextTag::builder()
        .name("gloss-stage")
        .left_margin(quote_speaker)
        .style(pango::Style::Italic)
        .build();

    // Prose gloss is the FOCUS ("hero"): full size (scale 1.0), generous 10px
    // leading like the journal answer body, so the reader's eye lands on the
    // explication. The speaker + verse above are the header (bold, full ink;
    // set off by the hang-indent).
    //
    // Spacing is an EVEN rhythm on both layouts: source → gloss and gloss →
    // next unit get the SAME gap. WITH a speaker the explication carries 24
    // above / 10 below, and the speaker tag's 14 top gap completes the
    // between-unit side (10+14=24 == 24). SPEAKERLESS (prose) has no speaker
    // tag, so both sides live on this tag directly (20/20; the earlier
    // 10-above/32-below heading-style grouping read as unequal spacing on
    // the overlay).
    let (para_above, para_below) = if has_speaker { (24, 10) } else { (20, 20) };
    let para_builder = gtk4::TextTag::builder()
        .name("gloss-para")
        .left_margin(quote_body)
        .pixels_above_lines(para_above)
        .pixels_below_lines(para_below)
        .scale(1.0);
    let para_tag = match dim_color {
        Some(c) => para_builder.foreground(c).build(),
        None => para_builder.build(),
    };

    let speaker_first_tag = header_dim(
        header_tag_base("gloss-speaker-first", quote_speaker)
            .variant(pango::Variant::SmallCaps)
            // Match the main reading card's speaker-name size (scale 0.75), like
            // `gloss-speaker` above.
            .scale(0.75)
            .pixels_below_lines(10),
    )
    .build();

    // Speaker label inside the quoted source turn (before the echo list). The
    // turn may span several speakers; keep them tightly spaced to match the
    // reader's 8px speaker rhythm rather than the 36px echo-section gap.
    let speaker_source_tag = header_dim(
        header_tag_base("gloss-speaker-source", quote_speaker)
            .variant(pango::Variant::SmallCaps)
            // Match the main reading card's speaker-name size (scale 0.75).
            .scale(0.75)
            .pixels_above_lines(8)
            .pixels_below_lines(10),
    )
    .build();

    // [Bracket] spans inside verse: italic only, full body size — like stage
    // directions on the main reading card.
    let bracket_tag = gtk4::TextTag::builder()
        .name("gloss-bracket")
        .style(pango::Style::Italic)
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

    // Citation line: same indent as its echo quote, full body size, dimmer.
    // Use the theme's dim foreground when provided so the source citations
    // recede behind the echo quotes through ink alone. (NOT quote_verse — the
    // deep verse hang-indent belongs to the quoted source turn only.)
    let citation_builder = gtk4::TextTag::builder()
        .name("gloss-citation")
        .left_margin(quote_speaker);
    let citation_tag = match dim_color {
        Some(c) => citation_builder.foreground(c).build(),
        None => citation_builder.build(),
    };

    // Pronunciation teaching note beneath its verse block: italic at full body
    // size, dimmed with the theme's dim foreground (like the citation/para
    // tags) so it reads as a recessed teaching aside.
    let pron_builder = gtk4::TextTag::builder()
        .name("gloss-pron")
        .left_margin(quote_speaker)
        .style(pango::Style::Italic);
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
            GlossElement::Segment(text) => {
                only_speakers_so_far = false;
                let (shown, hi) = crate::ui::gloss_block::strip_hi_spans(&strip_ipa(text));
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, &shown);
                let start = buffer.iter_at_offset(offset);
                buffer.apply_tag(&verse_tag, &start, &buffer.end_iter());
                apply_bracket_styling(&buffer, offset, &bracket_tag);
                apply_hi_ranges(&buffer, offset, &hi, &hi_tag);

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
                    let (shown, hi) = crate::ui::gloss_block::strip_hi_spans(&strip_ipa(text));
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, &shown);
                    let start = buffer.iter_at_offset(offset);
                    buffer.apply_tag(&para_tag, &start, &buffer.end_iter());
                    apply_hi_ranges(&buffer, offset, &hi, &hi_tag);
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
                let (shown, hi) = crate::ui::gloss_block::strip_hi_spans(text);
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, &shown);
                let start = buffer.iter_at_offset(offset);
                buffer.apply_tag(&stage_tag, &start, &buffer.end_iter());
                apply_hi_ranges(&buffer, offset, &hi, &hi_tag);
                // No line-number gutter entry: stage directions are not numbered
                // verse lines.
            }
        }
    }

    (bar_ranges, line_nums, echo_lines)
}

#[cfg(test)]
mod rendered_text_tests {
    use super::rendered_verse_text;

    // A speaker + a verse render as "NAME\nverse". Dropped ("UNKNOWN") speakers
    // vanish entirely, matching populate_verse_buffer's filter.
    #[test]
    fn speaker_and_verse() {
        let m = "<speaker>HAMLET</speaker><segment>To be or not to be</segment>";
        assert_eq!(rendered_verse_text(m), "HAMLET\nTo be or not to be");
        let m2 = "<speaker>UNKNOWN</speaker><segment>a prose line</segment>";
        assert_eq!(rendered_verse_text(m2), "a prose line");
    }

    // IPA spans are stripped from the rendered text (they carry no visible
    // chars), and the doubled space they leave is collapsed — so diff offsets
    // index the SHOWN characters. Matches populate_verse_buffer's strip_ipa call.
    #[test]
    fn strips_ipa() {
        let m = "<segment>Dread /drɛːd/ sovereign</segment>";
        assert_eq!(rendered_verse_text(m), "Dread sovereign");
    }

    // <hi> highlight tags are stripped from the rendered text (the highlight is a
    // tag, not visible chars), matching populate_verse_buffer's strip_hi_spans.
    #[test]
    fn strips_hi() {
        let m = "<gloss>the <hi>quick</hi> fox</gloss>";
        assert_eq!(rendered_verse_text(m), "the quick fox");
    }

    // The load-bearing property for pagination: rendering block markups joined by
    // "\n" equals rendering each block and joining the results by "\n". This is
    // why current_page_char_span can sum per-block rendered lengths (+1 per join)
    // and land on the same offsets the paginated buffer uses.
    #[test]
    fn join_distributes_over_render() {
        let a = "<speaker>KING</speaker><segment>Now is the winter</segment>";
        let b = "<gloss>An explication paragraph.</gloss>";
        let joined = format!("{a}\n{b}");
        let per_block = format!("{}\n{}", rendered_verse_text(a), rendered_verse_text(b));
        assert_eq!(rendered_verse_text(&joined), per_block);
    }

    // A <pron> element inserts no body text but still consumes a separator when it
    // is not the first element — the whole-gloss render leaves that "\n" in place.
    #[test]
    fn pron_inserts_no_body_but_separator_runs() {
        let m = "<segment>line one</segment><pron>the note</pron><segment>line two</segment>";
        // verse, then "\n" before the (empty) pron, then "\n" before verse two.
        assert_eq!(rendered_verse_text(m), "line one\n\nline two");
    }

    // The rewrite diff-highlight indexes the REBUILT basis
    // (`gloss_block_markups(gloss).join("\n")`, via `full_rendered_gloss_text`),
    // and the single-page gloss buffer must render that SAME basis or the tint
    // lands on the wrong words. `gloss_block_markups` defers echo `<gloss>[…]`
    // brackets to the source-run tail, so for an echo gloss the rebuilt render
    // differs from a raw render — this asserts the rebuilt basis is what both the
    // diff and the (now-rebuilt) single-page buffer use, so they cannot diverge.
    #[test]
    fn echo_gloss_rebuilt_basis_reorders_vs_raw() {
        // A source run with an echo bracket interleaved between verses. The
        // single-page gloss buffer renders `gloss_block_markups(gloss).join("\n")`
        // (the REBUILT basis) so it matches the offsets the rewrite diff-highlight
        // indexes via `full_rendered_gloss_text` (same rebuilt basis). Rendering
        // the RAW markup instead would mis-tint, because `gloss_block_markups`
        // defers the echo `<gloss>[…]` bracket to the source-run tail.
        let raw = "<speaker>ROMEO</speaker><segment>But soft</segment>\
                   <gloss>[echo — a resonance]</gloss>\
                   <segment>what light</segment>\
                   <gloss>An explication.</gloss>";
        let rebuilt = crate::ui::gloss_block::gloss_block_markups(raw).join("\n");
        let raw_render = rendered_verse_text(raw);
        let rebuilt_render = rendered_verse_text(&rebuilt);
        // The reordering is real: raw keeps the echo between the two verses;
        // rebuilt moves it after "what light". If this ever stops holding, the
        // single-page render basis no longer needs the rebuild — but until then,
        // rendering raw on a single page would desync the diff offsets.
        assert_ne!(raw_render, rebuilt_render);
        // In the rebuilt (buffer + diff) basis the echo trails both verses.
        let soft = rebuilt_render.find("But soft").unwrap();
        let light = rebuilt_render.find("what light").unwrap();
        let echo = rebuilt_render.find("echo").unwrap();
        assert!(soft < light && light < echo, "echo must trail both verses in the rebuilt basis: {rebuilt_render:?}");
    }
}

#[cfg(test)]
mod reflow_verse_markup_tests {
    use super::reflow_verse_markup;

    #[test]
    fn joins_lines_within_each_verse_body() {
        let m = "<segment>It is somewhere about five or six o'clock,\n\
                 and a balmy fragrance of warm tea\nhovers in Cook's Court.</segment>\n\
                 <gloss>keeps\nits own\nbreaks</gloss>\n\
                 <segment>second\nsegment</segment>";
        assert_eq!(
            reflow_verse_markup(m),
            "<segment>It is somewhere about five or six o'clock, \
             and a balmy fragrance of warm tea hovers in Cook's Court.</segment>\n\
             <gloss>keeps\nits own\nbreaks</gloss>\n\
             <segment>second segment</segment>"
        );
    }

    #[test]
    fn trims_and_drops_blank_interior_lines() {
        assert_eq!(
            reflow_verse_markup("<segment>  a \n\n  b  </segment>"),
            "<segment>a b</segment>"
        );
    }

    #[test]
    fn untagged_and_dangling_markup_pass_through() {
        assert_eq!(reflow_verse_markup("no tags\nhere"), "no tags\nhere");
        assert_eq!(reflow_verse_markup("<segment>open\nnever closed"), "<segment>open\nnever closed");
    }
}

#[cfg(test)]
mod chat_gloss_rows_tests {
    use super::{chat_gloss_rows, ChatGlossRowKind as K};

    #[test]
    fn speaker_verse_gloss_become_typed_rows() {
        let m = "<speaker>CYMBELINE</speaker>\n\
                 <segment>Stand by my side, you whom the gods have made</segment>\n\
                 <gloss>Cymbeline honors the disguised Belarius.</gloss>";
        let rows = chat_gloss_rows(m);
        assert_eq!(
            rows,
            vec![
                (K::Speaker, "CYMBELINE".to_string()),
                (K::Verse, "Stand by my side, you whom the gods have made".to_string()),
                (K::Gloss, "Cymbeline honors the disguised Belarius.".to_string()),
            ]
        );
    }

    // Dropped ("UNKNOWN"/empty) speakers vanish entirely, matching
    // populate_verse_buffer's filter — no blank Speaker row.
    #[test]
    fn unknown_speaker_is_dropped() {
        let m = "<speaker>UNKNOWN</speaker>\n<segment>a prose line</segment>";
        let rows = chat_gloss_rows(m);
        assert_eq!(rows, vec![(K::Verse, "a prose line".to_string())]);
    }

    // TWO consecutive <segment> ELEMENTS are two source SEGMENTS (prose
    // paragraphs): a blank Verse row separates them so the panel renders a
    // blank line between segments. Lines WITHIN one element stay contiguous,
    // and a verse element after a gloss gets no separator.
    #[test]
    fn blank_row_between_consecutive_verse_elements() {
        let m = "<segment>segment one</segment>\n<segment>segment two</segment>\n\
                 <gloss>commentary</gloss>\n<segment>segment three</segment>";
        let rows = chat_gloss_rows(m);
        assert_eq!(
            rows,
            vec![
                (K::Verse, "segment one".to_string()),
                (K::Verse, String::new()),
                (K::Verse, "segment two".to_string()),
                (K::Gloss, "commentary".to_string()),
                (K::Verse, "segment three".to_string()),
            ]
        );
    }

    // A multi-line <segment> body (as gloss_block_markups joins verse lines with
    // "\n") splits into one row per non-blank line, so the panel doesn't lose
    // line breaks by cramming them into one label.
    #[test]
    fn multiline_verse_splits_into_one_row_per_line() {
        let m = "<segment>line one\nline two</segment>";
        let rows = chat_gloss_rows(m);
        assert_eq!(
            rows,
            vec![
                (K::Verse, "line one".to_string()),
                (K::Verse, "line two".to_string()),
            ]
        );
    }

    #[test]
    fn stage_direction_is_its_own_row() {
        let m = "<segment>Lay hands upon these traitors</segment>\n\
                 <stage>[To Jourdain.]</stage>\n\
                 <segment>Beldam, I think we watched you</segment>";
        let rows = chat_gloss_rows(m);
        assert_eq!(
            rows,
            vec![
                (K::Verse, "Lay hands upon these traitors".to_string()),
                (K::Stage, "[To Jourdain.]".to_string()),
                (K::Verse, "Beldam, I think we watched you".to_string()),
            ]
        );
    }

    // IPA and <hi> are stripped for display, same as rendered_verse_text.
    #[test]
    fn strips_ipa_and_hi() {
        let m = "<segment>Dread /drɛːd/ <hi>sovereign</hi></segment>";
        let rows = chat_gloss_rows(m);
        assert_eq!(rows, vec![(K::Verse, "Dread sovereign".to_string())]);
    }

    // An echo bracket inside a <gloss> splits into a quoted-verse-like row plus
    // a citation row (mirrors rendered_verse_text's split_echo handling), not a
    // plain Gloss row.
    #[test]
    fn echo_gloss_splits_quote_and_citation() {
        let m = "<gloss>[\"a quote\" \u{2014} Macbeth 1.1]</gloss>";
        let rows = chat_gloss_rows(m);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, K::Verse);
        assert!(rows[0].1.contains("a quote"));
        assert_eq!(rows[1].0, K::Stage);
        assert!(rows[1].1.contains("Macbeth"));
    }

    // A <pron> note is TTS-only and produces no display row.
    #[test]
    fn pron_produces_no_row() {
        let m = "<segment>To be</segment>\n<pron>BEE: /bi\u{720}/</pron>";
        let rows = chat_gloss_rows(m);
        assert_eq!(rows, vec![(K::Verse, "To be".to_string())]);
    }

    // Plain prose with none of the recognized tags yields an empty Vec — the
    // caller's signal to fall back to rendering the text as-is (a journal Q&A
    // answer, not a gloss).
    #[test]
    fn plain_prose_yields_no_rows() {
        let rows = chat_gloss_rows("Just a plain prose answer, no tags here.");
        assert!(rows.is_empty());
    }
}
