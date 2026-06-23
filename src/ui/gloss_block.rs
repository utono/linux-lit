//! Gloss/synopsis block model and text parsing extracted from `gloss_overlay`.
//! Pure (no GTK). Depends on `gloss_ipa::strip_ipa` for Source-block display.

use crate::ui::gloss_ipa::{replace_word_ipa, strip_ipa};
use crate::ui::gloss_util::split_echo;

#[derive(Debug)]
pub(crate) enum GlossElement {
    Speaker(String),
    Verse(String),
    Gloss(String),
    Pron(String),
}

/// True when a paragraph is a short standalone heading label — e.g.
/// `Shakespearean parallels:`. Such paragraphs are stored on their own `<p>`
/// and rendered in bold by `show_synopsis`. The rule is deliberately generic:
/// a trimmed paragraph that ends in a colon, is short, and contains no
/// sentence-internal period reads as a label rather than running prose.
fn is_label_paragraph(p: &str) -> bool {
    let t = p.trim();
    t.ends_with(':') && t.chars().count() <= 60 && !t[..t.len() - 1].contains('.')
}

/// Render a synopsis for display, honoring paragraph markup, and also return the
/// character ranges (start, end) — in GTK `TextBuffer` char offsets into the
/// joined string — of any standalone label paragraphs, so the caller can bold
/// them. Synopses may be stored either as plain text (one paragraph) or as one
/// or more `<p>...</p>` tags (one per paragraph, mirroring how glosses use
/// `<gloss>` per paragraph). `<p>` paragraphs are joined with a blank line so the
/// text view shows visible paragraph breaks; plain text with no `<p>` tags is
/// returned trimmed, so legacy single-paragraph synopses keep working.
pub fn render_synopsis_with_labels(synopsis: &str) -> (String, Vec<(usize, usize)>) {
    let mut paras: Vec<String> = Vec::new();
    let mut remaining = synopsis;
    while let Some(pos) = remaining.find("<p>") {
        let after = &remaining[pos..];
        if let Some((content, rest)) = try_extract(after, "p") {
            if !content.is_empty() {
                paras.push(content.to_string());
            }
            remaining = rest;
        } else {
            remaining = &remaining[pos + 3..];
        }
    }
    if paras.is_empty() {
        return (synopsis.trim().to_string(), Vec::new());
    }
    // Join with a blank line, tracking each paragraph's char offset so label
    // paragraphs can be located precisely in the assembled string.
    let mut out = String::new();
    let mut labels: Vec<(usize, usize)> = Vec::new();
    let mut char_off = 0usize;
    for (i, p) in paras.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
            char_off += 2;
        }
        let len = p.chars().count();
        if is_label_paragraph(p) {
            labels.push((char_off, char_off + len));
        }
        out.push_str(p);
        char_off += len;
    }
    (out, labels)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockKind {
    Source,
    Explication,
}

/// One cursor stop in the gloss, in document order.
pub struct GlossBlock {
    pub kind: BlockKind,
    /// 0-based index WITHIN its kind (source blocks numbered separately from
    /// explication paragraphs).
    pub index: i32,
    /// RAW text, including any inline `/IPA/` — this is what TTS synthesizes.
    /// For Source: the joined verse-line text (speaker labels excluded).
    /// For Explication: the paragraph prose.
    pub text: String,
    /// DISPLAY text: `text` with `/IPA/` stripped (`strip_ipa`). Used for the
    /// reader's buffer and the accent-bar block matcher.
    pub display: String,
}

/// Parse a `<p>`-tagged synopsis into cursor-stop blocks, one per paragraph,
/// each a `BlockKind::Explication` (synopses are prose, never verse). Label
/// Inclusive block range for a visual selection given the anchor and cursor
/// block indices. Direction-independent: the smaller index is the start.
pub fn visual_block_range(anchor: usize, cursor: usize) -> (usize, usize) {
    (anchor.min(cursor), anchor.max(cursor))
}

/// Build the yank text for a synopsis visual selection: the `display` (clean,
/// `<p>`-stripped) text of cursor-stop blocks `start..=end`, joined by a blank
/// line. Uses `synopsis_blocks` so the indices match the on-screen cursor
/// stops exactly (label paragraphs already excluded). Out-of-range indices are
/// clamped; an empty synopsis yields an empty string.
pub fn selected_blocks_text(synopsis: &str, start: usize, end: usize) -> String {
    let blocks = synopsis_blocks(synopsis);
    if blocks.is_empty() {
        return String::new();
    }
    let last = blocks.len() - 1;
    let (s, e) = (start.min(last), end.min(last));
    let (s, e) = (s.min(e), s.max(e));
    blocks[s..=e]
        .iter()
        .map(|b| b.display.clone())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// paragraphs (`is_label_paragraph`, e.g. "Shakespearean parallels:") are shown
/// in the buffer but are NOT cursor stops, so they are skipped here — exactly
/// the paragraphs `render_synopsis_with_labels` marks for bolding. Synopsis text
/// carries no inline `/IPA/`, so `text == display`. Legacy untagged prose (no
/// `<p>`) is returned as a single block. Indices count the emitted (non-label)
/// blocks from 0, matching the cache `paragraph_index`.
pub fn synopsis_blocks(synopsis: &str) -> Vec<GlossBlock> {
    let mut paras: Vec<String> = Vec::new();
    let mut remaining = synopsis;
    while let Some(pos) = remaining.find("<p>") {
        let after = &remaining[pos..];
        if let Some((content, rest)) = try_extract(after, "p") {
            if !content.is_empty() {
                paras.push(content.to_string());
            }
            remaining = rest;
        } else {
            remaining = &remaining[pos + 3..];
        }
    }
    if paras.is_empty() {
        let t = synopsis.trim();
        if t.is_empty() {
            return Vec::new();
        }
        return vec![GlossBlock {
            kind: BlockKind::Explication,
            index: 0,
            text: t.to_string(),
            display: t.to_string(),
        }];
    }
    let mut blocks: Vec<GlossBlock> = Vec::new();
    let mut index = 0i32;
    for p in &paras {
        if is_label_paragraph(p) {
            continue;
        }
        blocks.push(GlossBlock {
            kind: BlockKind::Explication,
            index,
            text: p.clone(),
            display: p.clone(),
        });
        index += 1;
    }
    blocks
}

/// Parse a gloss into ordered cursor-stop blocks: each contiguous
/// `<speaker>`/`<verse>` run is one Source block; each non-echo `<gloss>` is one
/// Explication block. Echo `<gloss>` brackets are excluded. Source and
/// explication indices increment independently.
pub fn gloss_blocks(gloss: &str) -> Vec<GlossBlock> {
    let mut blocks = Vec::new();
    let mut source_idx = 0i32;
    let mut expl_idx = 0i32;
    let mut pending_verses: Vec<String> = Vec::new();

    let flush_source =
        |blocks: &mut Vec<GlossBlock>, source_idx: &mut i32, pending: &mut Vec<String>| {
            if !pending.is_empty() {
                let text = pending.join("\n");
                let display = strip_ipa(&text);
                blocks.push(GlossBlock {
                    kind: BlockKind::Source,
                    index: *source_idx,
                    text,
                    display,
                });
                *source_idx += 1;
                pending.clear();
            }
        };

    for el in parse_gloss_tags(gloss) {
        match el {
            GlossElement::Speaker(_) => { /* drop speaker labels from source text */ }
            GlossElement::Verse(text) => pending_verses.push(text.trim().to_string()),
            GlossElement::Gloss(text) => {
                if split_echo(&text).is_some() {
                    continue; // echo bracket: not a cursor stop
                }
                // A real explication paragraph ends the current source run.
                flush_source(&mut blocks, &mut source_idx, &mut pending_verses);
                let text = text.trim().to_string();
                let display = strip_ipa(&text);
                blocks.push(GlossBlock {
                    kind: BlockKind::Explication,
                    index: expl_idx,
                    text,
                    display,
                });
                expl_idx += 1;
            }
            GlossElement::Pron(_) => { /* pronunciation note: not a cursor stop, not TTS */ }
        }
    }
    // Trailing source run (gloss that ends on verse).
    flush_source(&mut blocks, &mut source_idx, &mut pending_verses);
    blocks
}


pub(crate) fn parse_gloss_tags(gloss: &str) -> Vec<GlossElement> {
    let mut elements = Vec::new();
    let mut remaining = gloss;

    while !remaining.is_empty() {
        if let Some(pos) = remaining.find('<') {
            let after_open = &remaining[pos..];
            if let Some(el) = try_extract(after_open, "speaker") {
                elements.push(GlossElement::Speaker(el.0.to_string()));
                remaining = el.1;
            } else if let Some(el) = try_extract(after_open, "verse") {
                elements.push(GlossElement::Verse(el.0.to_string()));
                remaining = el.1;
            } else if let Some(el) = try_extract(after_open, "gloss") {
                elements.push(GlossElement::Gloss(el.0.to_string()));
                remaining = el.1;
            } else if let Some(el) = try_extract(after_open, "pron") {
                elements.push(GlossElement::Pron(el.0.to_string()));
                remaining = el.1;
            } else {
                remaining = &remaining[pos + 1..];
            }
        } else {
            break;
        }
    }
    carry_forward_block_speakers(elements)
}

/// Repair speaker-less verse blocks. A verse block normally opens with a
/// `<speaker>` (which supplies BOTH the label and the 36px breathing-room above
/// the block — `verse_tag` itself has no top spacing). When the gloss model
/// omits the speaker for a continued speech (an observed defect in stored data,
/// e.g. gloss 21730's middle block), the block renders with neither label nor
/// gap, jammed against the preceding `<gloss>` prose.
///
/// Here we detect a `Verse` that begins a new block — the previous element is a
/// `Gloss` — with no `Speaker` of its own, and splice in a synthetic `Speaker`
/// carrying the last-seen speaker name. The synthetic element flows through the
/// normal Speaker render arm, so the block regains both its label and its gap
/// with no special-casing downstream (block ranges, cursor, bars all follow).
fn carry_forward_block_speakers(elements: Vec<GlossElement>) -> Vec<GlossElement> {
    let mut out: Vec<GlossElement> = Vec::with_capacity(elements.len());
    let mut last_speaker: Option<String> = None;
    let mut prev_was_gloss = false;
    for el in elements {
        match &el {
            GlossElement::Speaker(name) => {
                last_speaker = Some(name.clone());
                prev_was_gloss = false;
            }
            GlossElement::Verse(_) => {
                // A verse opening a new block (right after prose) with no speaker
                // of its own: re-insert the carried speaker so the block keeps
                // its label and top spacing.
                if prev_was_gloss {
                    if let Some(name) = &last_speaker {
                        out.push(GlossElement::Speaker(name.clone()));
                    }
                }
                prev_was_gloss = false;
            }
            GlossElement::Gloss(_) => prev_was_gloss = true,
            GlossElement::Pron(_) => {}
        }
        out.push(el);
    }
    out
}

fn try_extract<'a>(s: &'a str, tag: &str) -> Option<(&'a str, &'a str)> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if !s.starts_with(&open) {
        return None;
    }
    let content_start = open.len();
    if let Some(end_pos) = s[content_start..].find(&close) {
        let content = s[content_start..content_start + end_pos].trim();
        let after = &s[content_start + end_pos + close.len()..];
        Some((content, after))
    } else {
        None
    }
}

/// Rewrite the `/IPA/` after `word` (whole-word, all occurrences) within ONLY
/// the source block at `source_index`, operating on the TAGGED `gloss_text`
/// (each verse line is wrapped in `<verse>…</verse>`). Returns the full updated
/// gloss_text, or None if that block has no `word /IPA/` pair. Other blocks are
/// untouched even if they contain the same word.
///
/// Scoped by POSITION, not text: each `<verse>` is identified by its
/// document-order ordinal, and only verses belonging to the target source run
/// (per `gloss_blocks`' exact flush rule) are rewritten. This distinguishes
/// byte-identical verse lines that appear in different source blocks (e.g. a
/// repeated refrain) — a text-membership match would wrongly rewrite both.
pub(crate) fn replace_word_ipa_in_source_block(
    gloss_text: &str,
    source_index: i32,
    word: &str,
    new_ipa: &str,
) -> Option<String> {
    // Phase 1: which verse ORDINALS (0-based, document order) belong to the
    // target source block? Mirror gloss_blocks' flush rule exactly: a non-echo
    // <gloss> flushes the pending source run, and the source index advances
    // ONLY when that pending run is non-empty (matching flush_source).
    let mut target_ordinals: std::collections::HashSet<usize> = std::collections::HashSet::new();
    {
        let mut cur_source = 0i32;
        let mut verse_ord = 0usize;
        let mut pending_ords: Vec<usize> = Vec::new();
        for el in parse_gloss_tags(gloss_text) {
            match el {
                GlossElement::Verse(_) => {
                    pending_ords.push(verse_ord);
                    verse_ord += 1;
                }
                GlossElement::Gloss(text) => {
                    if split_echo(&text).is_some() {
                        continue; // echo bracket: does not flush
                    }
                    // non-echo gloss flushes the current source run (if non-empty)
                    if !pending_ords.is_empty() {
                        if cur_source == source_index {
                            target_ordinals.extend(pending_ords.iter().copied());
                        }
                        cur_source += 1;
                        pending_ords.clear();
                    }
                }
                GlossElement::Speaker(_) | GlossElement::Pron(_) => {}
            }
        }
        // trailing run (gloss that ends on verse)
        if !pending_ords.is_empty() && cur_source == source_index {
            target_ordinals.extend(pending_ords.iter().copied());
        }
    }
    if target_ordinals.is_empty() {
        return None; // no such source block / no verses
    }

    // Phase 2: walk raw <verse>…</verse> spans by ordinal (same document order
    // as Phase 1, since parse_gloss_tags emits one Verse per tag in order);
    // rewrite only target ones, copy everything else verbatim.
    let mut out = String::with_capacity(gloss_text.len());
    let mut rest = gloss_text;
    let mut ord = 0usize;
    let mut any = false;
    while let Some(open) = rest.find("<verse>") {
        let after_open = open + "<verse>".len();
        out.push_str(&rest[..after_open]);
        let tail = &rest[after_open..];
        if let Some(close_rel) = tail.find("</verse>") {
            let inner = &tail[..close_rel];
            if target_ordinals.contains(&ord) {
                if let Some(fixed) = replace_word_ipa(inner, word, new_ipa) {
                    out.push_str(&fixed);
                    any = true;
                } else {
                    out.push_str(inner);
                }
            } else {
                out.push_str(inner);
            }
            out.push_str("</verse>");
            rest = &tail[close_rel + "</verse>".len()..];
            ord += 1;
        } else {
            // malformed: no closing tag — copy the remainder and stop
            out.push_str(tail);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    if any {
        Some(out)
    } else {
        None
    }
}

#[cfg(test)]
mod block_tests {
    use super::*;

    #[test]
    fn parse_extracts_pron_element() {
        let g = "<verse>To /biː/</verse>\n<pron>BEE: be /biː/ keeps the long vowel.</pron>";
        let els = parse_gloss_tags(g);
        assert!(matches!(els[0], GlossElement::Verse(_)));
        assert!(
            matches!(&els[1], GlossElement::Pron(t) if t.contains("long vowel")),
            "expected a Pron element carrying the note, got {:?}", els.get(1)
        );
    }

    #[test]
    fn speakerless_verse_block_carries_forward_prior_speaker() {
        // Gloss 21730's defect: a continued speech's middle verse block omits
        // its <speaker>, so it rendered with neither label nor top spacing.
        // parse_gloss_tags must splice the carried speaker back in.
        let gloss = "<speaker>KING</speaker>\n\
                     <verse>You were ever good at sudden commendations,</verse>\n\
                     <gloss>The King opens with a rebuke.</gloss>\n\
                     <verse>To me you cannot reach. You play the spaniel,</verse>\n\
                     <gloss>Blunt and final.</gloss>\n\
                     <speaker>KING</speaker>\n\
                     <verse>Good man, sit down.</verse>";
        let els = parse_gloss_tags(gloss);
        // The speaker-less middle block must now open with a carried KING.
        assert!(
            matches!(&els[3], GlossElement::Speaker(n) if n == "KING"),
            "expected a carried-forward KING speaker before the middle verse \
             block, got {:?}",
            els.get(3)
        );
        assert!(matches!(&els[4], GlossElement::Verse(t) if t.starts_with("To me")));
        // The original two real speakers plus one synthetic = three speakers.
        let speakers = els
            .iter()
            .filter(|e| matches!(e, GlossElement::Speaker(_)))
            .count();
        assert_eq!(speakers, 3, "got elements: {:?}", els);
        // The synthetic speaker is dropped by gloss_blocks, so it must NOT add a
        // spurious block: still 3 source + 2 explication = 5 blocks.
        let blocks = gloss_blocks(gloss);
        let sources = blocks.iter().filter(|b| b.kind == BlockKind::Source).count();
        assert_eq!(sources, 3, "synthetic speaker must not add a source block");
    }

    #[test]
    fn blocks_in_document_order_with_kinds() {
        let gloss = "<speaker>CRANMER</speaker>\n\
                     <verse>Ah, my good Lord of Winchester, I thank you.</verse>\n\
                     <verse>You are always my good friend.</verse>\n\
                     <gloss>Cranmer opens with cutting irony.</gloss>\n\
                     <speaker>CRANMER</speaker>\n\
                     <verse>'Tis my undoing. Love and meekness, lord,</verse>\n\
                     <gloss>The tone shifts from irony to sincere counsel.</gloss>\n\
                     <gloss>[\"a quote\" — Macbeth 1.1]</gloss>";
        let blocks = gloss_blocks(gloss);
        assert_eq!(blocks.len(), 4); // source, explication, source, explication (echo excluded)

        assert_eq!(blocks[0].kind, BlockKind::Source);
        assert_eq!(blocks[0].index, 0);
        assert_eq!(
            blocks[0].text,
            "Ah, my good Lord of Winchester, I thank you.\nYou are always my good friend."
        );

        assert_eq!(blocks[1].kind, BlockKind::Explication);
        assert_eq!(blocks[1].index, 0);
        assert_eq!(blocks[1].text, "Cranmer opens with cutting irony.");

        assert_eq!(blocks[2].kind, BlockKind::Source);
        assert_eq!(blocks[2].index, 1);
        assert_eq!(blocks[2].text, "'Tis my undoing. Love and meekness, lord,");

        assert_eq!(blocks[3].kind, BlockKind::Explication);
        assert_eq!(blocks[3].index, 1);
        assert_eq!(blocks[3].text, "The tone shifts from irony to sincere counsel.");
    }

    #[test]
    fn all_echo_gloss_has_only_source_block() {
        let gloss = "<speaker>HAMLET</speaker>\n\
                     <verse>To be, or not to be</verse>\n\
                     <gloss>[\"q\" — Lr 1.1]</gloss>";
        let blocks = gloss_blocks(gloss);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Source);
        assert_eq!(blocks[0].text, "To be, or not to be");
    }

    #[test]
    fn source_block_keeps_raw_ipa_and_derives_clean_display() {
        let g = "<speaker>HAMLET</speaker>\n<verse>To /biː/ or not to /biː/</verse>";
        let blocks = gloss_blocks(g);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Source);
        // raw text (for TTS) keeps the IPA
        assert_eq!(blocks[0].text, "To /biː/ or not to /biː/");
        // display text (for the reader / accent-bar matcher) is stripped and
        // whitespace-normalized — no doubled gaps, no trailing space.
        assert_eq!(blocks[0].display, "To or not to");
    }

    #[test]
    fn lone_pron_note_produces_no_block() {
        // a <pron> note is neither a source nor explication block
        let g = "<pron>BEE: be /biː/ keeps the long vowel.</pron>";
        let blocks = gloss_blocks(g);
        assert_eq!(blocks.len(), 0);
    }

    #[test]
    fn tts_field_is_raw_display_field_is_stripped() {
        // play_block_tts (gloss.rs) clones `.text` for synthesis; the reader
        // path uses `.display`. This locks that the two diverge as intended:
        // raw keeps /IPA/, display strips it.
        let g = "<verse>/biː/ or not</verse>";
        let b = &gloss_blocks(g)[0];
        assert!(b.text.contains('/'), "TTS text must keep raw /IPA/");
        assert!(!b.display.contains('/'), "display text must be stripped");
    }

    #[test]
    fn explication_block_keeps_raw_ipa_and_strips_display() {
        // The explication push is a SEPARATE code path from the source push;
        // ensure it also keeps raw /IPA/ in `text` and strips it in `display`.
        let g = "<gloss>The operative word /ˈsʊfər/ carries the line.</gloss>";
        let blocks = gloss_blocks(g);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Explication);
        assert_eq!(blocks[0].text, "The operative word /ˈsʊfər/ carries the line.");
        assert_eq!(blocks[0].display, "The operative word carries the line.");
    }
}


#[cfg(test)]
mod synopsis_blocks_tests {
    use super::{synopsis_blocks, BlockKind};

    #[test]
    fn each_p_becomes_one_explication_block_skipping_labels() {
        let syn = "<p>First paragraph of action.</p>\
                   <p>Shakespearean parallels:</p>\
                   <p>Second paragraph continues.</p>";
        let blocks = synopsis_blocks(syn);
        // Label paragraph ("…parallels:") is skipped as a cursor stop.
        assert_eq!(blocks.len(), 2);
        assert!(blocks.iter().all(|b| b.kind == BlockKind::Explication));
        assert_eq!(blocks[0].index, 0);
        assert_eq!(blocks[1].index, 1);
        assert_eq!(blocks[0].text, "First paragraph of action.");
        assert_eq!(blocks[0].display, "First paragraph of action.");
        assert_eq!(blocks[1].text, "Second paragraph continues.");
    }

    #[test]
    fn legacy_plain_text_is_one_block() {
        let blocks = synopsis_blocks("Just plain text, no tags.");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Explication);
        assert_eq!(blocks[0].index, 0);
        assert_eq!(blocks[0].text, "Just plain text, no tags.");
    }

    #[test]
    fn empty_yields_no_blocks() {
        assert_eq!(synopsis_blocks("").len(), 0);
    }
}

#[cfg(test)]
mod visual_range_tests {
    use super::*;

    #[test]
    fn range_is_direction_independent() {
        assert_eq!(visual_block_range(2, 5), (2, 5));
        assert_eq!(visual_block_range(5, 2), (2, 5));
    }

    #[test]
    fn single_block_range() {
        assert_eq!(visual_block_range(3, 3), (3, 3));
    }

    #[test]
    fn range_from_zero() {
        assert_eq!(visual_block_range(0, 4), (0, 4));
        assert_eq!(visual_block_range(4, 0), (0, 4));
    }

    #[test]
    fn selects_paragraph_range_blank_line_joined() {
        let syn = "<p>One.</p><p>Two.</p><p>Three.</p>";
        // blocks 0..=1 -> first two paragraphs
        assert_eq!(selected_blocks_text(syn, 0, 1), "One.\n\nTwo.");
    }

    #[test]
    fn selects_single_paragraph() {
        let syn = "<p>One.</p><p>Two.</p>";
        assert_eq!(selected_blocks_text(syn, 1, 1), "Two.");
    }

    #[test]
    fn selection_skips_label_paragraph_like_the_cursor() {
        // synopsis_blocks excludes label paragraphs, so block indices count only
        // cursor-stop paragraphs. "Shakespearean parallels:" is a label.
        let syn = "<p>Plot.</p><p>Shakespearean parallels:</p><p>The parallel.</p>";
        // cursor-stop blocks are [Plot., The parallel.] -> indices 0,1
        assert_eq!(selected_blocks_text(syn, 0, 1), "Plot.\n\nThe parallel.");
    }

    #[test]
    fn plain_untagged_synopsis_is_one_block() {
        let syn = "Just one paragraph, no tags.";
        assert_eq!(selected_blocks_text(syn, 0, 0), "Just one paragraph, no tags.");
    }
}

#[cfg(test)]
mod label_tests {
    use super::*;

    #[test]
    fn bolds_standalone_label_paragraph() {
        let syn = "<p>Plot stuff here.</p><p>Shakespearean parallels:</p><p>The Court of Chancery is Elsinore.</p>";
        let (text, labels) = render_synopsis_with_labels(syn);
        assert_eq!(
            text,
            "Plot stuff here.\n\nShakespearean parallels:\n\nThe Court of Chancery is Elsinore."
        );
        assert_eq!(labels.len(), 1, "exactly one label paragraph");
        let (s, e) = labels[0];
        let chars: Vec<char> = text.chars().collect();
        let slice: String = chars[s..e].iter().collect();
        assert_eq!(slice, "Shakespearean parallels:");
    }

    #[test]
    fn does_not_bold_running_prose() {
        let syn = "<p>The fog descends on London. It is November.</p>";
        let (_text, labels) = render_synopsis_with_labels(syn);
        assert!(labels.is_empty());
    }

    #[test]
    fn plain_text_synopsis_has_no_labels() {
        let (text, labels) = render_synopsis_with_labels("Just plain text.");
        assert_eq!(text, "Just plain text.");
        assert!(labels.is_empty());
    }
}

#[cfg(test)]
mod replace_in_source_block_tests {
    use super::*;

    #[test]
    fn replace_in_source_block_rewrites_multiline_verse() {
        let g = "<speaker>GARDINER</speaker>\n<verse>In daily /ˈdɛːli/ thanks</verse>\n<verse>that gave /gɛːv/ us</verse>\n<gloss>note</gloss>";
        let out = replace_word_ipa_in_source_block(g, 0, "daily", "/ˈdeɪli/").unwrap();
        assert!(out.contains("daily /ˈdeɪli/"));
        assert!(out.contains("gave /gɛːv/")); // other word untouched
        assert!(out.contains("<gloss>note</gloss>")); // tags intact
        assert!(out.contains("<verse>"));
    }

    #[test]
    fn replace_in_source_block_none_when_word_absent() {
        let g = "<verse>In daily /ˈdɛːli/ thanks</verse>";
        assert!(replace_word_ipa_in_source_block(g, 0, "missing", "/x/").is_none());
    }

    #[test]
    fn replace_in_source_block_scopes_to_the_block() {
        // 'good' appears in TWO source blocks; fixing block 1 must not touch block 0.
        let g = "<verse>good /gʊd/ first</verse>\n<gloss>a</gloss>\n<verse>good /gʊd/ second</verse>\n<gloss>b</gloss>";
        let out = replace_word_ipa_in_source_block(g, 1, "good", "/guːd/").unwrap();
        // block 0 (index 0) keeps old IPA; block 1 (index 1) gets new.
        let first = out.find("first").unwrap();
        let second = out.find("second").unwrap();
        assert!(out[..first].contains("good /gʊd/")); // block 0 unchanged
        assert!(out[..second].contains("good /guːd/")); // block 1 changed
    }

    #[test]
    fn replace_in_source_block_distinguishes_identical_lines_across_blocks() {
        // Same verse line text in TWO source blocks; fixing block 1 must leave
        // block 0's identical line untouched (position-scoped, not text-scoped).
        let g = "<verse>good /gʊd/ same</verse>\n<gloss>a</gloss>\n<verse>good /gʊd/ same</verse>\n<gloss>b</gloss>";
        let out = replace_word_ipa_in_source_block(g, 1, "good", "/guːd/").unwrap();
        // exactly ONE rewrite: block 1's. Block 0 keeps /gʊd/.
        assert_eq!(out.matches("good /guːd/ same").count(), 1);
        assert_eq!(out.matches("good /gʊd/ same").count(), 1);
    }
}
