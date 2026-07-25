use super::{AppState, DIALOGUE_INDENT, TWO_COLUMN_DIALOGUE_INDENT, BCP_SENTENCE_GAP};
use crate::logging::log;
use gtk4::prelude::*;

pub(crate) fn apply_scansion_marks(
    joined: &str,
    line_map: &crate::text_file_map::LineMap,
    work_lines: &[crate::db::models::Line],
    scansion: &std::collections::HashMap<i64, crate::scansion::LineScansion>,
    level: crate::scansion::ScanLevel,
    two_col: bool,
) -> (String, std::collections::HashMap<usize, usize>) {
    let mut out_lines: Vec<String> = Vec::new();
    let mut map: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (buf_idx, line) in joined.lines().enumerate() {
        let scan = line_map
            .buffer_to_work
            .get(buf_idx)
            .copied()
            .flatten()
            .and_then(|work_idx| work_lines.get(work_idx))
            .and_then(|wl| scansion.get(&wl.id));
        match scan {
            Some(s) => {
                let m = crate::scansion::mark_line(line, s, level);
                if two_col {
                    // No trailing line-type label in two-column mode: it would
                    // overflow the per-column width budget and break the verse
                    // flow across columns. Marks (and caesura) still render.
                    out_lines.push(m.text);
                } else {
                    // Single column: append the line-type label after the marked
                    // text and 3 separator spaces. Record where the label begins
                    // so rebuild_buffer_text can dim-tag it.
                    map.insert(buf_idx, m.text.chars().count() + 3);
                    out_lines.push(format!("{}   {}", m.text, m.label));
                }
            }
            None => out_lines.push(line.to_string()),
        }
    }
    (out_lines.join("\n"), map)
}

/// Apply dialogue indentation and tight spacing for text-file mode.
/// Scans buffer lines for speaker patterns. If speakers found:
/// - Sets global line spacing to 0
/// - Applies "dialogue-indent" tag (extra left margin) to dialogue lines
/// - Applies "speaker-gap" tag (extra pixels above) to speaker lines
/// - Applies "stage-direction-gap" tag to stage directions
/// Center-justify every bare stanza-number heading line in the buffer (a
/// `sonnet_sequence`'s "1", "2", …). The text region is made symmetric in
/// `apply_tiled_mode`, so center justification places the number over the
/// centered verse block. Idempotent: the tag is reused and re-applied.
fn apply_stanza_number_centering(state: &AppState) {
    use crate::db::line_types;
    let tag_table = state.buffer.tag_table();
    let tag = tag_table.lookup("stanza-number-center").unwrap_or_else(|| {
        let t = gtk4::TextTag::builder()
            .name("stanza-number-center")
            .justification(gtk4::Justification::Center)
            .build();
        tag_table.add(&t);
        t
    });
    let line_count = state.buffer.line_count() as usize;
    for i in 0..line_count {
        let Some(start) = state.buffer.iter_at_line(i as i32) else { continue };
        let mut end = start;
        if !end.ends_line() { end.forward_to_line_end(); }
        let text = state.buffer.text(&start, &end, false).to_string();
        if line_types::is_stanza_number(text.trim()) {
            state.buffer.apply_tag(&tag, &start, &end);
        }
    }
}

pub(crate) fn apply_dialogue_formatting(state: &mut AppState) {
    use crate::db::line_types;

    // BCP works get liturgical typography (centered headings, centered/italic
    // rubrics, centered-italic speaker cues, small-caps, body gaps) instead of
    // play dialogue formatting — on BOTH load paths:
    //   - DB path (no text_file): the buffer is built from work.lines.
    //   - text_file path: the TEI-rendered .txt. Its blocks carry the same
    //     structural markers as the DB rows (## head, [rubric], @ speaker) — and
    //     apply_bcp_formatting re-derives every line type from the mapped
    //     work-line's ORIGINAL text (which always has the markers), not from the
    //     displayed buffer, so it works identically on the .txt buffer.
    if state
        .current_work
        .as_ref()
        .is_some_and(|w| line_types::is_bcp_work(&w.abbrev))
    {
        apply_bcp_formatting(state);
        return;
    }

    // Only in text-file mode
    if state.line_map.is_none() {
        state.dialogue_formatting_active = false;
        return;
    }

    // Block-aware works (verse/heading typography, e.g. LoJ) render their own
    // per-line tags via apply_block_typography. apply_dialogue_formatting's
    // all-line dialogue-indent fallback would override the verse tiers (its tag
    // is added later => higher priority), flattening the indent. Such works have
    // no genuine dialogue/speakers, so skip this pass entirely.
    if !state.block_indent_tiers.is_empty() {
        return;
    }

    // One section per page (sonnet_sequence): center the bare stanza-number
    // headings over the centered block (the text region is symmetric, set in
    // apply_tiled_mode). Verse lines keep the default left justification.
    // Runs before the speaker scan below — sonnets have no speakers, so the rest
    // of this function early-returns.
    if state.one_section_per_page() {
        apply_stanza_number_centering(state);
    }

    // Scan first 200 lines for any speaker
    let line_count = state.buffer.line_count() as usize;
    let scan_limit = line_count.min(200);
    let mut has_speakers = false;
    for i in 0..scan_limit {
        let iter = match state.buffer.iter_at_line(i as i32) {
            Some(it) => it,
            None => continue,
        };
        let end = if i + 1 < line_count {
            state.buffer.iter_at_line((i + 1) as i32).unwrap_or_else(|| state.buffer.end_iter())
        } else {
            state.buffer.end_iter()
        };
        let text = state.buffer.text(&iter, &end, false);
        let text = text.trim_end_matches('\n');
        if line_types::is_speaker(text) {
            has_speakers = true;
            break;
        }
    }

    if !has_speakers {
        state.dialogue_formatting_active = false;
        return;
    }

    state.dialogue_formatting_active = true;

    // Set global spacing to 0 for dialogue formatting
    state.text_view.set_pixels_above_lines(0);
    state.text_view.set_pixels_below_lines(0);

    // Remove old formatting tags if they exist
    let tag_table = state.buffer.tag_table();
    for name in &["dialogue-indent", "speaker-gap", "stage-direction-gap",
                   "speaker-name", "stage-direction-style", "act-scene-header",
                   "blank-line"] {
        if let Some(old) = tag_table.lookup(name) {
            tag_table.remove(&old);
        }
    }

    // Text column is already symmetrically inset by state.config.text_margins,
    // so speaker names sit at the same left edge as dialogue. Dialogue lines
    // get an additional indent via the per-tag margin below. In two-column
    // mode the columns are narrow, so the full +60 monocle indent pushes
    // verse lines past the column edge and they wrap; use a smaller indent
    // there so a 63-char verse line still clears the column width.
    let base_margin = state.text_view.left_margin();
    let dialogue_indent = if state.column_count() == 2 {
        TWO_COLUMN_DIALOGUE_INDENT
    } else {
        DIALOGUE_INDENT
    };

    let indent_tag = gtk4::TextTag::builder()
        .name("dialogue-indent")
        .left_margin(base_margin + dialogue_indent)
        .build();

    let speaker_gap_tag = gtk4::TextTag::builder()
        .name("speaker-gap")
        // Extra breathing room above each speaker heading so the dialogue's
        // speech-turn structure scans faster (larger than the inter-line gap).
        .pixels_above_lines(14)
        .build();

    let stage_gap_tag = gtk4::TextTag::builder()
        .name("stage-direction-gap")
        .pixels_above_lines(8)
        .build();

    // Speaker names stay flush-left at the text margin, aligned with stage
    // directions, so dialogue hangs-indents beneath them (standard
    // modern-edition look). Dialogue gets the +60 indent via dialogue-indent.
    let speaker_name_tag = gtk4::TextTag::builder()
        .name("speaker-name")
        .variant(pango::Variant::SmallCaps)
        .weight(400)
        .scale(0.75)
        .build();

    let stage_italic_tag = gtk4::TextTag::builder()
        .name("stage-direction-style")
        .style(pango::Style::Italic)
        .build();

    let act_scene_tag = gtk4::TextTag::builder()
        .name("act-scene-header")
        .weight(700)
        .pixels_above_lines(8)
        .build();

    let blank_line_tag = gtk4::TextTag::builder()
        .name("blank-line")
        .scale(0.25)
        .build();

    tag_table.add(&indent_tag);
    tag_table.add(&speaker_gap_tag);
    tag_table.add(&stage_gap_tag);
    tag_table.add(&speaker_name_tag);
    tag_table.add(&stage_italic_tag);
    tag_table.add(&act_scene_tag);
    tag_table.add(&blank_line_tag);

    // Apply tags per line. `in_stage_direction` tracks a multi-line stage
    // direction (one that spans several source lines, e.g. "[Enter Lucius,…\n
    // Guards, and an Attendant…]"). Its middle continuation lines start without
    // `[` and end without `]`, so `is_stage_direction` can't recognize them in
    // isolation — without this flag they'd fall through to the dialogue branch
    // and lose the italic styling, leaving the continuation mis-formatted.
    let mut in_stage_direction = false;
    for i in 0..line_count {
        let line_start = match state.buffer.iter_at_line(i as i32) {
            Some(iter) => iter,
            None => continue,
        };
        let line_end = if i + 1 < line_count {
            match state.buffer.iter_at_line((i + 1) as i32) {
                Some(iter) => iter,
                None => state.buffer.end_iter(),
            }
        } else {
            state.buffer.end_iter()
        };

        let text = state.buffer.text(&line_start, &line_end, false);
        let text = text.trim_end_matches('\n');
        let trimmed = text.trim();

        if line_types::is_blank(text) {
            in_stage_direction = false;
            state.buffer.apply_tag(&blank_line_tag, &line_start, &line_end);
        } else if in_stage_direction {
            // Continuation (or closing) line of a multi-line stage direction:
            // same indent + italic as the opening line, but no extra gap above.
            state.buffer.apply_tag(&indent_tag, &line_start, &line_end);
            state.buffer.apply_tag(&stage_italic_tag, &line_start, &line_end);
            // A line that closes the bracket ends the block.
            if trimmed.ends_with(']') {
                in_stage_direction = false;
            }
        } else if line_types::is_act_scene_marker(text) || line_types::is_separator(text) {
            // Check headings BEFORE is_speaker: standalone markers like EPILOGUE,
            // PROLOGUE, CHORUS, INDUCTION are all-caps and would otherwise match
            // is_speaker first and render in the smaller small-caps speaker style
            // instead of the bold ACT/SCENE heading style.
            state.buffer.apply_tag(&act_scene_tag, &line_start, &line_end);
        } else if line_types::is_speaker(text) {
            state.buffer.apply_tag(&speaker_gap_tag, &line_start, &line_end);
            state.buffer.apply_tag(&speaker_name_tag, &line_start, &line_end);
        } else if line_types::is_stage_direction(text) {
            state.buffer.apply_tag(&stage_gap_tag, &line_start, &line_end);
            state.buffer.apply_tag(&indent_tag, &line_start, &line_end);
            state.buffer.apply_tag(&stage_italic_tag, &line_start, &line_end);
            // Opening line of a multi-line stage direction: `[` with no closing
            // `]`. Carry the styling forward until the closing bracket.
            if trimmed.starts_with('[') && !trimmed.ends_with(']') {
                in_stage_direction = true;
            }
        } else {
            // Dialogue line — indent
            state.buffer.apply_tag(&indent_tag, &line_start, &line_end);
        }
    }

    log(&format!(
        "FORMATTING: applied dialogue formatting ({} lines)",
        line_count
    ));
}

/// Liturgical typography for Book of Common Prayer works. Mirrors
/// apply_dialogue_formatting's per-line tag application, but styles the
/// `## ` headings, `[...]` rubrics, and whole-word GOD/LORD that the BCP data
/// carries. Reuses the same TextTag primitives (centered, italic, small-caps,
/// indent) the speaker/stage-direction code already proves out.
pub(crate) fn apply_bcp_formatting(state: &mut AppState) {
    use crate::db::line_types;

    if state.line_map.is_none() {
        state.dialogue_formatting_active = false;
        return;
    }
    state.dialogue_formatting_active = true;
    state.text_view.set_pixels_above_lines(0);
    state.text_view.set_pixels_below_lines(0);
    // NB: full (Fill) justification is intentionally NOT used. GtkTextView does
    // not implement fill-justify for word-wrapped text — both view-level
    // set_justification(Fill) and a TextTag Justification::Fill render
    // ragged-right (verified empirically; GTK docs scope Justification::Fill to
    // GtkLabel). The Kindle's justified look would require a custom Pango/Cairo
    // text widget, which is out of scope. Block spacing + a continuation-block
    // left-indent carry the paragraph-separation the layout needs instead.

    let tag_table = state.buffer.tag_table();
    for name in &[
        "bcp-heading", "bcp-rubric-centered",
        "bcp-divine-name", "bcp-blank", "bcp-body", "bcp-body-indent",
        "bcp-opening", "bcp-speaker-centered",
    ] {
        if let Some(old) = tag_table.lookup(name) {
            tag_table.remove(&old);
        }
    }

    let heading_tag = gtk4::TextTag::builder()
        .name("bcp-heading")
        .justification(gtk4::Justification::Center)
        .weight(700)
        .scale(1.1)
        .pixels_above_lines(12)
        .pixels_below_lines(6)
        .build();
    let rubric_centered_tag = gtk4::TextTag::builder()
        .name("bcp-rubric-centered")
        .justification(gtk4::Justification::Center)
        .style(pango::Style::Italic)
        .pixels_above_lines(6)
        .build();
    let divine_name_tag = gtk4::TextTag::builder()
        .name("bcp-divine-name")
        .variant(pango::Variant::SmallCaps)
        .build();
    // A speaker cue (`Priest.`, `Aunswere.`) on its own row (the TEI emits it as
    // its own `@ `-marked line; the marker is stripped at buffer-build): the
    // Kindle layout centers it in italic above the response.
    let speaker_centered_tag = gtk4::TextTag::builder()
        .name("bcp-speaker-centered")
        .justification(gtk4::Justification::Center)
        .style(pango::Style::Italic)
        .pixels_above_lines(6)
        .build();
    let blank_tag = gtk4::TextTag::builder()
        .name("bcp-blank")
        .scale(0.25)
        .build();
    // Body prayers/petitions: BCP body lines are split one sentence per buffer
    // line (in display_work, via split_bcp_sentences). A gap above each sentence
    // line gives the airy, separated layout the printed/Kindle text has. (No
    // Fill justification — see the note at the top of this fn; no continuation
    // indent — the gap alone separates sentences and prayers.)
    let body_tag = gtk4::TextTag::builder()
        .name("bcp-body")
        .pixels_above_lines(BCP_SENTENCE_GAP)
        .build();
    // The opening word of a prayer, set small-caps (LYGHTEN, WHOSOEVER,
    // ALMIGHTY). Layered over the body line tag like the divine-name span.
    let opening_tag = gtk4::TextTag::builder()
        .name("bcp-opening")
        .variant(pango::Variant::SmallCaps)
        .build();

    tag_table.add(&heading_tag);
    tag_table.add(&rubric_centered_tag);
    tag_table.add(&divine_name_tag);
    tag_table.add(&blank_tag);
    tag_table.add(&body_tag);
    tag_table.add(&opening_tag);
    tag_table.add(&speaker_centered_tag);

    let line_count = state.buffer.line_count() as usize;
    // Heading-ness and speaker-cue-ness are re-derived per buffer line from the
    // mapped work-line's ORIGINAL text (which retains the `## ` / `@ ` marker);
    // the displayed buffer text has had those markers stripped at buffer-build
    // time, so we can't detect them there. Precomputed before the loop to avoid
    // borrowing `state` while mutating the buffer inside it.
    // (heading, speaker, rubric) per buffer line. Rubric-ness is re-derived from
    // the work-line too (not the buffer): on the text_file path the displayed
    // rubric is cosmetically indented, not `[...]`-bracketed, so a buffer test
    // would miss it; the work-line (DB row) always has the `[...]` marker.
    let line_kinds: Vec<(bool, bool, bool)> = {
        let work_lines = state.current_work.as_ref().map(|w| &w.lines);
        (0..line_count)
            .map(|i| {
                let orig = state
                    .work_line_for_buffer(i)
                    .and_then(|wi| work_lines.and_then(|wl| wl.get(wi)));
                (
                    orig.is_some_and(|l| line_types::is_bcp_heading(&l.text)),
                    orig.is_some_and(|l| line_types::is_bcp_speaker(&l.text)),
                    orig.is_some_and(|l| line_types::is_rubric(&l.text)),
                )
            })
            .collect()
    };
    let heading_line: Vec<bool> = line_kinds.iter().map(|k| k.0).collect();
    let speaker_line: Vec<bool> = line_kinds.iter().map(|k| k.1).collect();
    let mut rubric_line: Vec<bool> = line_kinds.iter().map(|k| k.2).collect();

    // Strip the `[...]` brackets from rubric lines for display (the Oxford Kindle
    // shows no brackets), bottom-up so earlier offsets stay valid. On the
    // text_file path the rubric row is unmapped (it normalizes to empty), so
    // rubric_line[i] (work-line-derived) is false there; detect from the buffer
    // `[...]` instead and record it, so the tagging loop still centers it.
    for i in (0..line_count).rev() {
        let Some(line_start) = state.buffer.iter_at_line(i as i32) else { continue };
        let line_end = if i + 1 < line_count {
            state.buffer.iter_at_line((i + 1) as i32).unwrap_or_else(|| state.buffer.end_iter())
        } else {
            state.buffer.end_iter()
        };
        let raw = state.buffer.text(&line_start, &line_end, false);
        let raw = raw.trim_end_matches('\n');
        if !line_types::is_rubric(raw) {
            continue;
        }
        rubric_line[i] = true;
        // Delete the trailing `]` then the leading `[` (a leading-`¶` stays as a
        // visible pilcrow). Compute char positions on the de-newlined text.
        let lead_ws = raw.len() - raw.trim_start().len();
        let content_len = raw.trim().chars().count();
        // Trailing ']' : last non-ws char.
        let mut del = state.buffer.iter_at_line(i as i32).unwrap();
        del.forward_chars((lead_ws + content_len - 1) as i32);
        let mut del_end = del;
        del_end.forward_char();
        state.buffer.delete(&mut del, &mut del_end);
        // Leading '[' : first non-ws char.
        let mut s = state.buffer.iter_at_line(i as i32).unwrap();
        s.forward_chars(lead_ws as i32);
        let mut s_end = s;
        s_end.forward_char();
        state.buffer.delete(&mut s, &mut s_end);
    }

    // Small-caps spans: the TEI pipeline wraps `<hi rend="sc">` runs in `^...^`
    // (BCP_SMALLCAPS_MARK). Render each marked span in small-caps and delete the
    // carets for display. Per line, process spans highest-offset-first so a
    // deletion never shifts an earlier span's offsets; the rubric strip above
    // already ran, so the carets sit at stable positions in the cleaned text.
    for i in 0..line_count {
        let Some(line_start) = state.buffer.iter_at_line(i as i32) else { continue };
        let line_end = if i + 1 < line_count {
            state.buffer.iter_at_line((i + 1) as i32).unwrap_or_else(|| state.buffer.end_iter())
        } else {
            state.buffer.end_iter()
        };
        let text = state.buffer.text(&line_start, &line_end, false);
        let text = text.trim_end_matches('\n').to_string();
        let spans = line_types::bcp_smallcaps_spans(&text);
        // (open_caret_char, close_caret_char) offsets in `text`. Apply from the
        // last span to the first so earlier offsets stay valid after deletes.
        for &(open_c, close_c) in spans.iter().rev() {
            // RE-FETCH the line-start iterator after EVERY buffer mutation: a
            // delete invalidates all outstanding iterators (GTK warns and the
            // next delete hits a stale/foreign-buffer iter — the source of the
            // "Invalid text buffer iterator" / gtk_text_buffer_delete CRITICAL
            // on BCP load). Offsets stay valid because spans run highest-first.
            // Delete the close caret first (higher offset).
            let mut ce = state.buffer.iter_at_line(i as i32).unwrap();
            ce.forward_chars(close_c as i32);
            let mut ce_end = ce;
            ce_end.forward_char();
            state.buffer.delete(&mut ce, &mut ce_end);
            // Delete the open caret (re-fetch; the delete above invalidated ce).
            let mut oc = state.buffer.iter_at_line(i as i32).unwrap();
            oc.forward_chars(open_c as i32);
            let mut oc_end = oc;
            oc_end.forward_char();
            state.buffer.delete(&mut oc, &mut oc_end);
            // Tag the inner span (now between former open and close positions,
            // shifted left by one after deleting the open caret). Re-fetch again.
            let line_base = state.buffer.iter_at_line(i as i32).unwrap();
            let mut span_start = line_base;
            span_start.forward_chars(open_c as i32);
            let mut span_end = line_base;
            span_end.forward_chars((close_c - 1) as i32);
            state.buffer.apply_tag(&opening_tag, &span_start, &span_end);
        }
    }

    for i in 0..line_count {
        let Some(line_start) = state.buffer.iter_at_line(i as i32) else { continue };
        let line_end = if i + 1 < line_count {
            state.buffer.iter_at_line((i + 1) as i32).unwrap_or_else(|| state.buffer.end_iter())
        } else {
            state.buffer.end_iter()
        };
        let text = state.buffer.text(&line_start, &line_end, false);
        let text = text.trim_end_matches('\n').to_string();

        if line_types::is_blank(&text) {
            state.buffer.apply_tag(&blank_tag, &line_start, &line_end);
        } else if heading_line[i] {
            state.buffer.apply_tag(&heading_tag, &line_start, &line_end);
            // Ornamental ❧ flourishes on rite titles (is_bcp_rite_title) are
            // deferred: GTK TextTags cannot inject glyphs, and editing buffer
            // text here would desync search/navigation offsets. A future pass
            // injects ornaments at buffer-build time (where `## ` is stripped),
            // or draws them via an overlay. Centered bold is the title look
            // until then.
            let _ = line_types::is_bcp_rite_title(&text);
        } else if speaker_line[i] {
            // Standalone speaker cue (marker stripped at buffer-build): centered
            // italic above the response, like a centered rubric.
            state.buffer.apply_tag(&speaker_centered_tag, &line_start, &line_end);
        } else if rubric_line[i] {
            // rubric_line[i] is set from the work-line (DB path) or from the
            // bracketed buffer line in the strip pre-pass above (text_file path,
            // where the rubric row is unmapped). Brackets are already stripped.
            // ALL BCP rubrics render centered-italic, matching the Oxford Kindle
            // edition (which centers even long instructional rubrics).
            state.buffer.apply_tag(&rubric_centered_tag, &line_start, &line_end);
        } else {
            // A body prayer/petition — one sentence per line. Block-space above
            // every sentence line so prayers and their sentences read airily.
            state.buffer.apply_tag(&body_tag, &line_start, &line_end);
            // Opening-word small-caps is applied by the `^...^` marker pre-pass
            // above (the TEI carries the exact small-caps spans), not re-derived
            // from text here.
        }
        // Divine-name small-caps applies on ANY non-blank line (headings,
        // rubrics, body), layered over the line tag above.
        if !line_types::is_blank(&text) {
            for (s, e) in line_types::divine_name_spans(&text) {
                // Build span iters with the codebase's idiom (iter_at_line +
                // forward_chars, as in the label-span code ~app.rs:3416), not
                // iter_at_line_offset. divine_name_spans returns BYTE offsets;
                // convert to CHAR offsets so multi-byte chars (¶, curly quotes)
                // earlier in the line don't misplace the span.
                let Some(mut span_start) = state.buffer.iter_at_line(i as i32) else { continue };
                span_start.forward_chars(char_offset(&text, s) as i32);
                let mut span_end = span_start;
                span_end.forward_chars((char_offset(&text, e) - char_offset(&text, s)) as i32);
                state.buffer.apply_tag(&divine_name_tag, &span_start, &span_end);
            }
        }
    }

    log(&format!(
        "FORMATTING: applied BCP formatting ({} lines)",
        line_count
    ));
}

/// GTK text iters address characters, but divine_name_spans returns BYTE
/// offsets (regex match positions). Convert a byte offset within `line` to a
/// char offset so `forward_chars` lands correctly even with multi-byte
/// characters (¶, curly quotes) earlier in the line.
fn char_offset(line: &str, byte_off: usize) -> usize {
    line[..byte_off].chars().count()
}

pub(crate) fn apply_authorship_formatting(state: &mut AppState) {
    let tag_table = state.buffer.tag_table();
    if let Some(old) = tag_table.lookup("authorship-italic") {
        let (start, end) = state.buffer.bounds();
        state.buffer.remove_tag(&old, &start, &end);
    }

    if !state.authorship_enabled || state.authorship_line_ids.is_empty() {
        return;
    }

    let line_count = state.buffer.line_count() as usize;
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return,
    };

    for buf_line in 0..line_count {
        let work_idx = if let Some(ref lm) = state.line_map {
            match lm.buffer_to_work.get(buf_line).and_then(|o| *o) {
                Some(wi) => wi,
                None => continue,
            }
        } else {
            buf_line
        };

        let line = match work.lines.get(work_idx) {
            Some(l) => l,
            None => continue,
        };

        if state.authorship_line_ids.contains(&line.id) {
            let line_start = match state.buffer.iter_at_line(buf_line as i32) {
                Some(it) => it,
                None => continue,
            };
            let line_end = if buf_line + 1 < line_count {
                match state.buffer.iter_at_line((buf_line + 1) as i32) {
                    Some(it) => it,
                    None => {
                        let (_, e) = state.buffer.bounds();
                        e
                    }
                }
            } else {
                let (_, e) = state.buffer.bounds();
                e
            };
            state.buffer.apply_tag(&state.authorship_tag, &line_start, &line_end);
        }
    }
}

// Verse indent tiers (left_margin in px; base = prose-ish inset, +step per
// tier). left_margin is used (not TextTag indent / justify) because GTK
// collapses leading whitespace and indent/justify render unreliably
// (project convention — see the "Fill justification" note above).
fn verse_margin_px(tier: u8) -> i32 {
    let base = 48; // verse block inset from the card's text start
    let step = 32; // per-tier additional indent
    base + step * tier as i32
}

/// Idempotently ensure the block-typography tags exist on the buffer's tag
/// table: `verse-indent-{0,1,2}` (per-tier left_margin), `verse-stanza-gap`
/// (space above the first line of a verse block), and `block-heading-center`
/// (centered small-caps, for heading rows). Creation is kept in this single
/// place so `apply_block_typography` never creates duplicate-named tags.
fn ensure_block_typography_tags(state: &AppState) {
    let tag_table = state.buffer.tag_table();

    for tier in 0u8..=2 {
        let name = format!("verse-indent-{tier}");
        if tag_table.lookup(&name).is_none() {
            let tag = gtk4::TextTag::builder()
                .name(&name)
                .left_margin(verse_margin_px(tier))
                .build();
            tag_table.add(&tag);
        }
    }

    if tag_table.lookup("verse-stanza-gap").is_none() {
        let tag = gtk4::TextTag::builder()
            .name("verse-stanza-gap")
            .pixels_above_lines(12)
            .build();
        tag_table.add(&tag);
    }

    // Centered small-caps heading. The small-caps property is copied verbatim
    // from the "speaker-name" tag above (`.variant(pango::Variant::SmallCaps)`)
    // — NOT a "font-features" string — plus centered justification, matching
    // how "stanza-number-center" sets Center in this same file.
    if tag_table.lookup("block-heading-center").is_none() {
        let tag = gtk4::TextTag::builder()
            .name("block-heading-center")
            .justification(gtk4::Justification::Center)
            .variant(pango::Variant::SmallCaps)
            .build();
        tag_table.add(&tag);
    }
}

/// Tag verse lines (per-tier left_margin + stanza gap) and heading rows
/// (centered small-caps) for a block-aware work. No-op when
/// `block_indent_tiers` is empty (every non-block work).
pub fn apply_block_typography(state: &mut AppState) {
    if state.block_indent_tiers.is_empty() {
        return;
    }
    let Some(work) = state.current_work.clone() else { return };
    let Some(map) = state.line_map.clone() else { return };

    ensure_block_typography_tags(state);

    let buffer_line_count = state.buffer.line_count() as usize;
    let mut prev_src: Option<usize> = None;
    for bl in 0..buffer_line_count.min(map.buffer_to_work.len()) {
        let Some(wi) = map.buffer_to_work[bl] else { continue };
        let Some(line) = work.lines.get(wi) else { continue };
        let bt = line.block_type.as_str();

        let Some(start) = state.buffer.iter_at_line(bl as i32) else { continue };
        // `left_margin` is a paragraph property: a tag range that stops before
        // the line's `\n` (as `forward_to_line_end()` does) does NOT mark the
        // paragraph, so left_margin silently no-ops. Build `end` as the START
        // of the NEXT buffer line (i.e. past this line's terminator) instead —
        // the same idiom apply_dialogue_formatting uses above.
        let end = if bl + 1 < buffer_line_count {
            match state.buffer.iter_at_line((bl + 1) as i32) {
                Some(iter) => iter,
                None => state.buffer.end_iter(),
            }
        } else {
            state.buffer.end_iter()
        };

        if crate::db::line_types::is_verse_line(bt) {
            // An empty verse row is a stanza-gap separator: gap tag only, no
            // indent tag, no cursor/karaoke target.
            if line.text.trim().is_empty() {
                state.buffer.apply_tag_by_name("verse-stanza-gap", &start, &end);
            } else {
                let tier = state.block_indent_tiers.get(bl).copied().unwrap_or(0);
                state
                    .buffer
                    .apply_tag_by_name(&format!("verse-indent-{tier}"), &start, &end);
                if prev_src != Some(wi) {
                    state.buffer.apply_tag_by_name("verse-stanza-gap", &start, &end);
                }
            }
        } else if crate::db::line_types::is_heading_line(bt) {
            state.buffer.apply_tag_by_name("block-heading-center", &start, &end);
        }
        prev_src = Some(wi);
    }
}

/// Inline `_word_` italics for prose/prose_book/epic_translation works (Phase
/// B). Mirrors apply_bcp_formatting's `^...^` delete-and-retag idiom: per
/// line, delete the paired `_` from the buffer highest-offset-first
/// (re-fetching iterators after every mutation — a delete invalidates
/// outstanding iters, which is the source of the "Invalid text buffer
/// iterator" GTK critical if reused), tag each stripped span italic, and
/// record removed `_` source offsets in state.italic_offset_map for karaoke
/// offset translation. No-op (byte-identical) for excluded work types and for
/// lines with no `_`.
pub fn apply_inline_italics(state: &mut AppState) {
    // Gate: prose / prose_book / epic_translation only. (Redundant with the
    // fill-path gate, but keeps this pass a no-op if called for a non-italic
    // work — italic_line_spans is empty then anyway.)
    let wt = state
        .current_work
        .as_ref()
        .map(|w| w.work_type.clone())
        .unwrap_or_default();
    if !matches!(wt.as_str(), "prose" | "prose_book" | "epic_translation") {
        return;
    }

    // NET-ZERO tag lifecycle (unchanged from Phase B): remove any inline-italic-*
    // tags left by a prior run before re-applying, so the app-lifetime shared tag
    // table doesn't grow across reloads / scansion toggles.
    let tag_table = state.buffer.tag_table();
    {
        let mut stale: Vec<gtk4::TextTag> = Vec::new();
        tag_table.foreach(|t| {
            if t.name().map(|n| n.starts_with("inline-italic-")).unwrap_or(false) {
                stale.push(t.clone());
            }
        });
        for t in stale {
            tag_table.remove(&t);
        }
    }

    // Apply italic tags from the spans computed at buffer-fill
    // (strip_italics_for_fill). The `_` are already gone from the buffer, so
    // there is NO parsing and NO buffer.delete here — this pass only tags.
    //
    // ONE fresh NAMED tag PER SPAN (inline-italic-{line}-{span}): GTK4/Pango
    // renders only the LAST of multiple disjoint ranges of one style=Italic tag
    // within a single paragraph, so a per-span distinct tag is required (a
    // per-line shared tag would be that same failing case). Named so the
    // net-zero removal above can drop them next run.
    let spans = state.italic_line_spans.clone();
    for (&i, line_spans) in spans.iter() {
        for (k, &(sc, ec)) in line_spans.iter().enumerate() {
            let Some(base) = state.buffer.iter_at_line(i as i32) else { continue };
            let mut a = base;
            a.forward_chars(sc as i32);
            let mut b = base;
            b.forward_chars(ec as i32);
            let span_tag = gtk4::TextTag::builder()
                .name(&format!("inline-italic-{i}-{k}"))
                .style(pango::Style::Italic)
                .build();
            tag_table.add(&span_tag);
            state.buffer.apply_tag(&span_tag, &a, &b);
        }
    }
}
