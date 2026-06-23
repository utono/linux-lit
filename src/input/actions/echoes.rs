//! "Show echoes" feature: press `I` on a line to see cross-work passages
//! that echo the meaning of the cursor line's speaker turn, rendered in the
//! gloss overlay card. Read-only reference — no gloss is created.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;
use crate::db::models::Line;
use crate::db::queries::{EchoTurnKey, StoredEchoLink};

/// A sticky echo session, retained so `alt+i` can return to the turn's work
/// and reopen the overlay after the user jumps into an echo's work. Replaced
/// only when the user presses `I` on a new line.
#[derive(Clone)]
pub struct EchoSession {
    pub channel: crate::db::echo_channel::EchoChannel,
    pub turn_key: EchoTurnKey,
    pub turn_id: Option<i64>,
    pub links: Vec<StoredEchoLink>,
    pub selected: usize,
    pub titles: std::collections::HashMap<String, String>,
    pub source_doc: String,
    pub origin_work: String,
    pub origin_line_id: i64,
}

/// Gather the speaker turn containing the cursor line: the contiguous block
/// of lines by the same speaker. Returns the turn's lines and the inferred
/// addressee (next different speaker in the scene).
fn cursor_turn(state: &AppState) -> Option<(Vec<Line>, String, String)> {
    let work = state.current_work.as_ref()?;
    let work_idx = state.work_line_for_buffer(state.current_line)?;
    let cursor = work.lines.get(work_idx)?;
    let speaker = cursor.speaker.clone()?;

    let (div1, div2) = (cursor.div1, cursor.div2);

    // Expand backward and forward over the same speaker within the scene.
    let mut start = work_idx;
    while start > 0 {
        let prev = &work.lines[start - 1];
        if prev.div1 == div1 && prev.div2 == div2 && prev.speaker.as_deref() == Some(speaker.as_str()) {
            start -= 1;
        } else {
            break;
        }
    }
    let mut end = work_idx;
    while end + 1 < work.lines.len() {
        let next = &work.lines[end + 1];
        if next.div1 == div1 && next.div2 == div2 && next.speaker.as_deref() == Some(speaker.as_str()) {
            end += 1;
        } else {
            break;
        }
    }

    let turn: Vec<Line> = work.lines[start..=end].to_vec();

    // Addressee: next different speaker after the turn within the scene,
    // else the previous different speaker.
    let mut addressee = String::from("?");
    for line in work.lines[end + 1..].iter() {
        if line.div1 != div1 || line.div2 != div2 {
            break;
        }
        if let Some(s) = &line.speaker {
            if s != &speaker {
                addressee = s.clone();
                break;
            }
        }
    }
    if addressee == "?" {
        for line in work.lines[..start].iter().rev() {
            if line.div1 != div1 || line.div2 != div2 {
                break;
            }
            if let Some(s) = &line.speaker {
                if s != &speaker {
                    addressee = s.clone();
                    break;
                }
            }
        }
    }

    Some((turn, speaker, addressee))
}

/// Clip a Visual selection's work-line index range to valid bounds and return
/// the cloned lines. `start_wi`/`end_wi` are work-line indices (start <= end).
fn selection_turn_lines(work_lines: &[Line], start_wi: usize, end_wi: usize) -> Vec<Line> {
    if start_wi >= work_lines.len() {
        return Vec::new();
    }
    let end = end_wi.min(work_lines.len().saturating_sub(1));
    work_lines[start_wi..=end].to_vec()
}

/// Build an `EchoTurnKey` for an ad-hoc (possibly multi-turn, possibly
/// multi-speaker) selection. The speaker label is the first selected line's
/// speaker, falling back to "?" when absent. `turn_text` joins the selected
/// line texts with spaces, matching the cursor-turn key format.
fn selection_key(work_abbrev: &str, turn: &[Line]) -> crate::db::queries::EchoTurnKey {
    let first = turn.first().expect("selection_key called with empty turn");
    let last = turn.last().unwrap();
    let speaker = first.speaker.clone().unwrap_or_else(|| "?".to_string());
    let turn_text = turn.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ");
    crate::db::queries::EchoTurnKey {
        work_abbrev: work_abbrev.to_string(),
        div1: first.div1,
        div2: first.div2,
        start_line: first.line_in_div,
        end_line: last.line_in_div,
        speaker,
        turn_text,
    }
}

/// True if the current work is a Book of Common Prayer edition (BCP1549, …).
/// BCP works carry no speaker, so the speaker-block `cursor_turn` path can't
/// resolve them; the reader resolves them by chunk range instead.
fn current_work_is_bcp(state: &AppState) -> bool {
    state
        .current_work
        .as_ref()
        .map(|w| w.abbrev.starts_with("BCP"))
        .unwrap_or(false)
}

/// For a BCP work, resolve the cursor line to (work_abbrev, div1, line_in_div,
/// line_id). Used to look up the containing chunk's echo_turns row by range.
fn bcp_cursor_location(state: &AppState) -> Option<(String, i64, i64, i64)> {
    let work = state.current_work.as_ref()?;
    let work_idx = state.work_line_for_buffer(state.current_line)?;
    let cursor = work.lines.get(work_idx)?;
    Some((work.abbrev.clone(), cursor.div1, cursor.line_in_div, cursor.id))
}

/// BCP-reading path (BCP→Shakespeare direction): the reader is in a BCP work and
/// pressed the BCP-channel show key. Resolve the cursor line to its containing
/// chunk's echo_turns row (range match), load that turn's Shakespeare echoes,
/// and render. Returns true if it handled the event (always, for a BCP work —
/// either showing echoes or the no-echoes state); false if not a BCP work.
fn show_bcp_reading_echoes(
    state_rc: &Rc<RefCell<AppState>>,
    channel: crate::db::echo_channel::EchoChannel,
) -> bool {
    let loc = {
        let s = state_rc.borrow();
        if !current_work_is_bcp(&s) {
            return false;
        }
        bcp_cursor_location(&s)
    };
    let (work_abbrev, div1, line_in_div, origin_line_id) = match loc {
        Some(v) => v,
        None => return true, // BCP work but no resolvable cursor line — handled (no-op)
    };

    let resolved = crate::db::queries::open_db().ok().and_then(|conn| {
        let (turn_id, start_line, end_line, speaker, turn_text) =
            crate::db::queries::find_echo_turn_containing(&conn, &work_abbrev, div1, line_in_div)
                .ok()
                .flatten()?;
        let links = crate::db::queries::load_echo_links(&conn, turn_id, channel).ok()?;
        if links.is_empty() {
            return None;
        }
        let titles = crate::db::queries::load_work_titles(&conn).unwrap_or_default();
        Some((turn_id, start_line, end_line, speaker, turn_text, links, titles))
    });

    let mut s = state_rc.borrow_mut();
    match resolved {
        Some((turn_id, start_line, end_line, speaker, turn_text, links, titles)) => {
            let key = crate::db::queries::EchoTurnKey {
                work_abbrev: work_abbrev.clone(),
                div1,
                div2: 0,
                start_line,
                end_line,
                speaker: speaker.clone().unwrap_or_default(),
                turn_text: turn_text.clone(),
            };
            let source_doc = format!("{} {}.{}", work_abbrev, div1, start_line);
            s.echo_overlay_source = source_doc.clone();
            s.echo_overlay_links = links.clone();
            s.echo_overlay_index = 0;
            s.echo_overlay_titles = titles.clone();
            s.echo_overlay_turn_id = Some(turn_id);
            s.echo_overlay_turn_key = Some(key.clone());
            s.echo_session = Some(EchoSession {
                channel,
                turn_key: key,
                turn_id: Some(turn_id),
                links,
                selected: 0,
                titles,
                source_doc,
                origin_work: work_abbrev,
                origin_line_id,
            });
            s.input_mode = crate::app::InputMode::EchoesOverlay;
            render_echoes(&mut s);
            crate::logging::log("ECHOES: showing cached BCP-reading echoes");
        }
        None => {
            s.gloss_overlay.show("No echoes found for this passage.", "");
            s.input_mode = crate::app::InputMode::EchoesOverlay;
            crate::logging::log("ECHOES: BCP-reading, no echoes for this passage");
        }
    }
    true
}

pub(crate) fn show_echoes_for_cursor_line(
    state_rc: &Rc<RefCell<AppState>>,
    channel: crate::db::echo_channel::EchoChannel,
    tokio_handle: &tokio::runtime::Handle,
) {
    // BCP→Shakespeare: if the reader is in a BCP work, resolve by chunk range
    // (BCP lines have no speaker, so the speaker-block path below can't run).
    if channel == crate::db::echo_channel::EchoChannel::Bcp
        && show_bcp_reading_echoes(state_rc, channel)
    {
        return;
    }

    let (turn, speaker, addressee, source_work) = {
        let s = state_rc.borrow();
        let (turn, speaker, addressee) = match cursor_turn(&s) {
            Some(t) => t,
            None => {
                crate::logging::log("ECHOES: cursor line has no speaker turn");
                return;
            }
        };
        let work = match s.current_work.as_ref() {
            Some(w) => w.abbrev.clone(),
            None => return,
        };
        (turn, speaker, addressee, work)
    };

    let turn_text = turn.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ");

    // Build the turn cache key from the first/last line of the turn.
    let key = {
        let first = turn.first().unwrap();
        let last = turn.last().unwrap();
        crate::db::queries::EchoTurnKey {
            work_abbrev: source_work.clone(),
            div1: first.div1,
            div2: first.div2,
            start_line: first.line_in_div,
            end_line: last.line_in_div,
            speaker: speaker.clone(),
            turn_text: turn_text.clone(),
        }
    };

    // Cache hit: load stored links and render immediately, no API call.
    let cached = crate::db::queries::open_db().ok().and_then(|conn| {
        let turn_id = crate::db::queries::find_echo_turn(&conn, &key).ok().flatten()?;
        let links = crate::db::queries::load_echo_links(&conn, turn_id, channel).ok()?;
        if links.is_empty() { None } else { Some((turn_id, links)) }
    });

    let origin_line_id = turn.first().map(|l| l.id).unwrap_or(0);

    if let Some((turn_id, links)) = cached {
        let titles = crate::db::queries::open_db()
            .ok()
            .and_then(|conn| crate::db::queries::load_work_titles(&conn).ok())
            .unwrap_or_default();
        let source_doc = build_source_header(&turn, &speaker);
        let mut s = state_rc.borrow_mut();
        s.echo_overlay_source = source_doc.clone();
        s.echo_overlay_links = links.clone();
        s.echo_overlay_index = 0;
        s.echo_overlay_titles = titles.clone();
        s.echo_overlay_turn_id = Some(turn_id);
        s.echo_overlay_turn_key = Some(key.clone());
        s.echo_session = Some(EchoSession {
            channel,
            turn_key: key,
            turn_id: Some(turn_id),
            links,
            selected: 0,
            titles,
            source_doc,
            origin_work: source_work.clone(),
            origin_line_id,
        });
        s.input_mode = crate::app::InputMode::EchoesOverlay;
        render_echoes(&mut s);
        crate::logging::log("ECHOES: showing cached echoes");
        return;
    }

    // BCP echoes are cache-only: never trigger the Voyage search fallback.
    if channel == crate::db::echo_channel::EchoChannel::Bcp {
        let mut s = state_rc.borrow_mut();
        s.echo_overlay_turn_key = Some(key);
        s.gloss_overlay.show("No echoes found for this line.", "");
        s.input_mode = crate::app::InputMode::EchoesOverlay;
        crate::logging::log("ECHOES: BCP cache miss, no search fallback");
        return;
    }

    let query = format!("{} to {}: {}", speaker, addressee, turn_text);
    let key_for_async = key.clone();

    let affect_weight;
    {
        let mut s = state_rc.borrow_mut();
        affect_weight = s.config.echo_affect_weight;
        s.echo_overlay_turn_key = Some(key);
        s.gloss_overlay.show_loading_message("Searching for echoes...");
        s.input_mode = crate::app::InputMode::EchoesOverlay;
    }

    // Raw spoken text (not the enriched query) for the affect axis.
    let query_text = turn_text.clone();
    let state_for_result = Rc::clone(state_rc);
    let echo_handle = tokio_handle.clone();

    glib::spawn_future_local(async move {
        let embed_result = echo_handle
            .spawn(async move { crate::voyage::embed_query(&query).await })
            .await;

        let raw = match embed_result {
            Ok(Ok(embedding)) => crate::db::queries::open_db()
                .ok()
                .and_then(|conn| {
                    // Over-fetch; dedup by displayed first line removes some.
                    crate::db::queries::find_similar_passages(
                        &conn, &embedding, &query_text, &source_work, 60, affect_weight,
                    )
                    .ok()
                })
                .unwrap_or_default(),
            Ok(Err(e)) => {
                crate::logging::log(&format!("ECHOES: embed error: {}", e));
                Vec::new()
            }
            Err(e) => {
                crate::logging::log(&format!("ECHOES: embed join error: {}", e));
                Vec::new()
            }
        };

        // Dedup by the displayed first line (keep highest-similarity instance),
        // cap at 15.
        let mut seen = std::collections::HashSet::new();
        let mut candidates = Vec::new();
        for cand in raw {
            let key = first_sentence(&cand.passage_text).to_lowercase();
            if key.is_empty() || !seen.insert(key) {
                continue;
            }
            candidates.push(cand);
            if candidates.len() >= 15 {
                break;
            }
        }

        if candidates.is_empty() {
            let s = state_for_result.borrow();
            s.gloss_overlay.show("No echoes found for this line.", "");
            crate::logging::log("ECHOES: no candidates");
            return;
        }

        let titles = crate::db::queries::open_db()
            .ok()
            .and_then(|conn| crate::db::queries::load_work_titles(&conn).ok())
            .unwrap_or_default();

        // Order by work title, then act.scene — group echoes from the same work.
        candidates.sort_by(|a, b| {
            let ta = titles.get(&a.work_abbrev).map(|s| s.as_str()).unwrap_or(a.work_abbrev.as_str());
            let tb = titles.get(&b.work_abbrev).map(|s| s.as_str()).unwrap_or(b.work_abbrev.as_str());
            ta.cmp(tb)
                .then(a.div1.cmp(&b.div1))
                .then(a.div2.cmp(&b.div2))
        });

        // Persist: save the turn and its echo links, then read them back as
        // StoredEchoLinks (so the cache-hit and cache-miss render paths match).
        let (turn_id, links) = persist_and_load(&key_for_async, &candidates, channel);

        let mut s = state_for_result.borrow_mut();
        let source_doc = build_source_header(&turn, &speaker);
        s.echo_overlay_links = links.clone();
        s.echo_overlay_index = 0;
        s.echo_overlay_titles = titles.clone();
        s.echo_overlay_source = source_doc.clone();
        s.echo_overlay_turn_id = turn_id;
        s.echo_session = Some(EchoSession {
            channel,
            turn_key: key_for_async.clone(),
            turn_id,
            links,
            selected: 0,
            titles,
            source_doc,
            origin_work: source_work.clone(),
            origin_line_id,
        });
        render_echoes(&mut s);
        crate::logging::log("ECHOES: searched and cached echoes");
    });
}

/// Visual-mode `i`: show echoes for the selected range (one or more speaker
/// turns). Mirrors `show_echoes_for_cursor_line` but builds its turn from the
/// Visual selection. Exits Visual mode, then lands in the EchoesOverlay state.
pub(crate) fn show_echoes_for_selection(
    state_rc: &Rc<RefCell<AppState>>,
    channel: crate::db::echo_channel::EchoChannel,
    tokio_handle: &tokio::runtime::Handle,
) {
    let (turn, speaker, source_work) = {
        let s = state_rc.borrow();
        let (start, end) = match &s.visual_selection {
            Some(sel) => sel.range(),
            None => {
                crate::logging::log("ECHOES: no visual selection");
                return;
            }
        };
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return,
        };
        let start_wi = match s.work_line_for_buffer(start) {
            Some(i) => i,
            None => {
                crate::logging::log("ECHOES: selection start has no work line");
                return;
            }
        };
        let end_wi = s.work_line_for_buffer(end).unwrap_or(start_wi);
        let (lo, hi) = (start_wi.min(end_wi), start_wi.max(end_wi));
        let turn = selection_turn_lines(&work.lines, lo, hi);
        if turn.is_empty() {
            crate::logging::log("ECHOES: empty selection turn");
            return;
        }
        let speaker = turn.first().and_then(|l| l.speaker.clone()).unwrap_or_else(|| "?".to_string());
        (turn, speaker, work.abbrev.clone())
    };

    crate::input::visual::exit_visual_mode(&mut state_rc.borrow_mut());

    let turn_text = turn.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ");
    let key = selection_key(&source_work, &turn);
    let origin_line_id = turn.first().map(|l| l.id).unwrap_or(0);

    let cached = crate::db::queries::open_db().ok().and_then(|conn| {
        let turn_id = crate::db::queries::find_echo_turn(&conn, &key).ok().flatten()?;
        let links = crate::db::queries::load_echo_links(&conn, turn_id, channel).ok()?;
        if links.is_empty() { None } else { Some((turn_id, links)) }
    });

    if let Some((turn_id, links)) = cached {
        let titles = crate::db::queries::open_db()
            .ok()
            .and_then(|conn| crate::db::queries::load_work_titles(&conn).ok())
            .unwrap_or_default();
        let source_doc = build_source_header(&turn, &speaker);
        let mut s = state_rc.borrow_mut();
        s.echo_overlay_source = source_doc.clone();
        s.echo_overlay_links = links.clone();
        s.echo_overlay_index = 0;
        s.echo_overlay_titles = titles.clone();
        s.echo_overlay_turn_id = Some(turn_id);
        s.echo_overlay_turn_key = Some(key.clone());
        s.echo_session = Some(EchoSession {
            channel,
            turn_key: key,
            turn_id: Some(turn_id),
            links,
            selected: 0,
            titles,
            source_doc,
            origin_work: source_work.clone(),
            origin_line_id,
        });
        s.input_mode = crate::app::InputMode::EchoesOverlay;
        render_echoes(&mut s);
        crate::logging::log("ECHOES: showing cached echoes (selection)");
        return;
    }

    // BCP echoes are cache-only: never trigger the Voyage search fallback.
    if channel == crate::db::echo_channel::EchoChannel::Bcp {
        let mut s = state_rc.borrow_mut();
        s.echo_overlay_turn_key = Some(key);
        s.gloss_overlay.show("No echoes found for this line.", "");
        s.input_mode = crate::app::InputMode::EchoesOverlay;
        crate::logging::log("ECHOES: BCP cache miss, no search fallback (selection)");
        return;
    }

    let query = format!("{}: {}", speaker, turn_text);
    let key_for_async = key.clone();

    let affect_weight;
    {
        let mut s = state_rc.borrow_mut();
        affect_weight = s.config.echo_affect_weight;
        s.echo_overlay_turn_key = Some(key);
        s.gloss_overlay.show_loading_message("Searching for echoes...");
        s.input_mode = crate::app::InputMode::EchoesOverlay;
    }

    let query_text = turn_text.clone();
    let state_for_result = Rc::clone(state_rc);
    let echo_handle = tokio_handle.clone();

    glib::spawn_future_local(async move {
        let embed_result = echo_handle
            .spawn(async move { crate::voyage::embed_query(&query).await })
            .await;

        let raw = match embed_result {
            Ok(Ok(embedding)) => crate::db::queries::open_db()
                .ok()
                .and_then(|conn| {
                    crate::db::queries::find_similar_passages(
                        &conn, &embedding, &query_text, &source_work, 60, affect_weight,
                    )
                    .ok()
                })
                .unwrap_or_default(),
            Ok(Err(e)) => {
                crate::logging::log(&format!("ECHOES: embed error: {}", e));
                Vec::new()
            }
            Err(e) => {
                crate::logging::log(&format!("ECHOES: embed join error: {}", e));
                Vec::new()
            }
        };

        let mut seen = std::collections::HashSet::new();
        let mut candidates = Vec::new();
        for cand in raw {
            let dedup_key = first_sentence(&cand.passage_text).to_lowercase();
            if dedup_key.is_empty() || !seen.insert(dedup_key) {
                continue;
            }
            candidates.push(cand);
            if candidates.len() >= 15 {
                break;
            }
        }

        if candidates.is_empty() {
            let s = state_for_result.borrow();
            s.gloss_overlay.show("No echoes found for this selection.", "");
            crate::logging::log("ECHOES: no candidates (selection)");
            return;
        }

        let titles = crate::db::queries::open_db()
            .ok()
            .and_then(|conn| crate::db::queries::load_work_titles(&conn).ok())
            .unwrap_or_default();

        candidates.sort_by(|a, b| {
            let ta = titles.get(&a.work_abbrev).map(|s| s.as_str()).unwrap_or(a.work_abbrev.as_str());
            let tb = titles.get(&b.work_abbrev).map(|s| s.as_str()).unwrap_or(b.work_abbrev.as_str());
            ta.cmp(tb)
                .then(a.div1.cmp(&b.div1))
                .then(a.div2.cmp(&b.div2))
        });

        let (turn_id, links) = persist_and_load(&key_for_async, &candidates, channel);

        let mut s = state_for_result.borrow_mut();
        let source_doc = build_source_header(&turn, &speaker);
        s.echo_overlay_links = links.clone();
        s.echo_overlay_index = 0;
        s.echo_overlay_titles = titles.clone();
        s.echo_overlay_source = source_doc.clone();
        s.echo_overlay_turn_id = turn_id;
        s.echo_session = Some(EchoSession {
            channel,
            turn_key: key_for_async.clone(),
            turn_id,
            links,
            selected: 0,
            titles,
            source_doc,
            origin_work: source_work.clone(),
            origin_line_id,
        });
        render_echoes(&mut s);
        crate::logging::log("ECHOES: searched and cached echoes (selection)");
    });
}

/// Extract the first complete sentence from a passage, PRESERVING the
/// original verse line breaks. Accumulate lines until one ends a sentence
/// (. ? !), truncating that final line at the punctuation.
fn first_sentence_verse(passage: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in passage.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Does this line contain a sentence end?
        let mut cut = None;
        for (i, ch) in line.char_indices() {
            if matches!(ch, '.' | '?' | '!') {
                cut = Some(i + ch.len_utf8());
                break;
            }
        }
        if let Some(c) = cut {
            out.push(line[..c].trim().to_string());
            break;
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

/// Single-line form of the first sentence (line breaks collapsed to spaces),
/// for dedup keys and clipboard copy.
fn first_sentence(passage: &str) -> String {
    first_sentence_verse(passage)
        .lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build the `<speaker>`/`<verse>` header for the source turn. Emits a
/// `<speaker>` tag at the start and again whenever the speaker changes, so a
/// multi-speaker selection attributes each run to its own speaker. Lines
/// without their own speaker fall back to `speaker` and do not start a new run.
///
/// Shared with the gloss "Glossing…" loading card so the passage shown while a
/// gloss is being generated uses the same `<speaker>`/`<verse>` markup (and thus
/// the same single-column formatting) as the original passage in the result.
pub(crate) fn build_source_header(turn: &[Line], speaker: &str) -> String {
    let mut doc = String::new();
    let mut current: Option<String> = None;
    for line in turn {
        let label = line.speaker.as_deref().unwrap_or(speaker).to_uppercase();
        if current.as_deref() != Some(label.as_str()) {
            doc.push_str(&format!("<speaker>{}</speaker>\n", label));
            current = Some(label);
        }
        doc.push_str(&format!("<verse>{}</verse>\n", line.text));
    }
    doc
}

/// Render the echoes document (source header + echo list) into the gloss
/// overlay, highlighting the selected echo. Curated echoes get a ★ marker.
fn render_echoes(s: &mut AppState) {
    let source_doc = s.echo_overlay_source.clone();
    let mut echo_doc = String::new();
    for link in &s.echo_overlay_links {
        let title = s.echo_overlay_titles.get(&link.echo_work_abbrev)
            .cloned()
            .unwrap_or_else(|| link.echo_work_abbrev.clone());
        let star = if link.curated { "★ " } else { "" };
        echo_doc.push_str(&format!(
            "<gloss>[{}\"{}\" — {} {}.{}]</gloss>\n",
            star, link.echo_text, title, link.echo_div1, link.echo_div2
        ));
    }

    // card_width/card_height size the overlay to match the reading card's outer
    // box. content_hbox can report 0 when render runs before layout settles (e.g.
    // an alt+i reopen right after a cross-work load, or an R refresh). A 0 here
    // collapses the overlay and lets the reader bleed through — so fall back to
    // the window dimensions.
    let cw = {
        let w = s.content_hbox.width();
        if w > 0 { w } else { gtk4::prelude::WidgetExt::width(&s.window).max(1) }
    };
    let h = {
        let sw = s.content_hbox.height();
        if sw > 0 { sw } else { gtk4::prelude::WidgetExt::height(&s.window).max(1) }
    };
    let root = s.theme.root_color.clone();
    let dim = s.theme.dim_fg.clone();
    s.gloss_overlay.show_echoes(&source_doc, &echo_doc, cw, h, Some(&root), Some(&dim), s.echo_overlay_index);
}

/// Persist the search candidates as echo links for the turn, then read them
/// back (so display order is the stored, curated-first order).
fn persist_and_load(
    key: &crate::db::queries::EchoTurnKey,
    candidates: &[crate::db::queries::EchoCandidate],
    channel: crate::db::echo_channel::EchoChannel,
) -> (Option<i64>, Vec<crate::db::queries::StoredEchoLink>) {
    let conn = match crate::db::queries::open_db_rw() {
        Ok(c) => c,
        Err(_) => return (None, Vec::new()),
    };
    let turn_id = match crate::db::queries::save_echo_turn(&conn, key) {
        Ok(id) => id,
        Err(_) => return (None, Vec::new()),
    };
    let rows: Vec<(String, i64, i64, i64, String, f32, i64)> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            (
                c.work_abbrev.clone(),
                c.div1,
                c.div2,
                c.start_line,
                first_sentence_verse(&c.passage_text),
                c.similarity,
                i as i64,
            )
        })
        .collect();
    let _ = crate::db::queries::insert_echo_links(&conn, turn_id, &rows);
    let links = crate::db::queries::load_echo_links(&conn, turn_id, channel).unwrap_or_default();
    (Some(turn_id), links)
}

/// Copy the active overlay state into the sticky session so a later alt+i
/// restores the current links/selection.
fn sync_session(s: &mut AppState) {
    if let Some(sess) = s.echo_session.as_mut() {
        sess.links = s.echo_overlay_links.clone();
        sess.selected = s.echo_overlay_index;
        sess.titles = s.echo_overlay_titles.clone();
        sess.source_doc = s.echo_overlay_source.clone();
        sess.turn_id = s.echo_overlay_turn_id;
    }
}

pub(crate) fn move_echo_selection(
    state_rc: &Rc<RefCell<AppState>>,
    delta: i32,
    tokio_handle: &tokio::runtime::Handle,
) {
    {
        let mut s = state_rc.borrow_mut();
        let len = s.echo_overlay_links.len();
        if len == 0 {
            return;
        }
        let new_idx = ((s.echo_overlay_index as i32 + delta).rem_euclid(len as i32)) as usize;
        if new_idx == s.echo_overlay_index {
            return;
        }
        s.echo_overlay_index = new_idx;
        render_echoes(&mut s);
        s.gloss_overlay.scroll_echo_into_view(new_idx);
        sync_session(&mut s);
    }
    // n/p audition the echo they move to. The borrow above is dropped first so
    // play_selected_echo can borrow state itself.
    play_selected_echo(state_rc, tokio_handle);
}

/// Move the accent-bar selection to the first echo (`gg`) and scroll the echo
/// list to its top. The source turn is a fixed header, so it stays visible.
pub(crate) fn select_first_echo(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    if s.echo_overlay_links.is_empty() {
        return;
    }
    s.echo_overlay_index = 0;
    render_echoes(&mut s);
    s.gloss_overlay.scroll_gloss_to_top();
    sync_session(&mut s);
}

/// Move the accent-bar selection to the last echo (`G`).
pub(crate) fn select_last_echo(state_rc: &Rc<RefCell<AppState>>) {
    let last = {
        let s = state_rc.borrow();
        s.echo_overlay_links.len().saturating_sub(1)
    };
    select_echo_index(state_rc, last);
}

fn select_echo_index(state_rc: &Rc<RefCell<AppState>>, idx: usize) {
    let mut s = state_rc.borrow_mut();
    let len = s.echo_overlay_links.len();
    if len == 0 {
        return;
    }
    let new_idx = idx.min(len - 1);
    s.echo_overlay_index = new_idx;
    render_echoes(&mut s);
    s.gloss_overlay.scroll_echo_into_view(new_idx);
    sync_session(&mut s);
}

pub(crate) fn copy_selected_echo(state_rc: &Rc<RefCell<AppState>>) {
    let s = state_rc.borrow();
    if let Some(link) = s.echo_overlay_links.get(s.echo_overlay_index) {
        let title = s.echo_overlay_titles.get(&link.echo_work_abbrev)
            .cloned()
            .unwrap_or_else(|| link.echo_work_abbrev.clone());
        let sentence = link.echo_text.lines().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
        let text = format!("\"{}\" — {} {}.{}", sentence, title, link.echo_div1, link.echo_div2);
        let _ = std::process::Command::new("wl-copy").arg(&text).spawn();
        crate::logging::log(&format!("ECHOES: copied \"{}\"", text));
    }
}

const TURN_PREROLL: f64 = 0.5;

/// `Tab` in the echoes overlay: reload the source-turn media, re-arm the source
/// AB-loop, and play from the source turn's first line. The displayed work is
/// the source work, so its Arkangel media is used.
pub(crate) fn play_source_turn(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();

    // Resolve the turn's (a, b) timestamps from the session key against the
    // currently displayed (source) work.
    let range = s.echo_session.as_ref().and_then(|sess| {
        let key = &sess.turn_key;
        let work = s.current_work.as_ref()?;
        if work.abbrev != key.work_abbrev {
            return None;
        }
        let first = work.lines.iter().find(|l| {
            l.div1 == key.div1 && l.div2 == key.div2 && l.line_in_div == key.start_line
        })?;
        let last = work.lines.iter().find(|l| {
            l.div1 == key.div1 && l.div2 == key.div2 && l.line_in_div == key.end_line
        })?;
        let a = first.timestamp.as_ref()?.start;
        let b = last.timestamp.as_ref()?.end;
        Some((a, b))
    });

    let (a, b) = match range {
        Some(r) => r,
        None => {
            // No resolvable turn range — just toggle whatever is loaded.
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::TogglePause);
            crate::logging::log("ECHOES: toggled playback (no turn range)");
            return;
        }
    };

    // If the source turn is already the active loop (a prior Tab armed it),
    // a subsequent Tab just toggles play/pause — no reload, no re-seek.
    if s.ab_repeat.loop_active
        && s.ab_repeat.a_time == Some(a)
        && s.ab_repeat.b_time == Some(b)
    {
        let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::TogglePause);
        crate::logging::log("ECHOES: toggled source turn play/pause");
        return;
    }

    // The source work's Arkangel media (fall back to first media path).
    let source_media = s.current_work.as_ref().and_then(|w| {
        w.media_paths.iter()
            .find(|p| p.contains("/aax-Arkangel/"))
            .or_else(|| w.media_paths.first())
            .cloned()
    });

    let loop_a = (a - TURN_PREROLL).max(0.0);
    // Reload the source media (a may have swapped MPV to an echo file), then set
    // the loop. LoadFileAndSeek resumes playback on file-loaded.
    if let Some(path) = source_media {
        // Load + (on file-loaded) seek and set the ab-loop together, so the
        // loadfile-replace doesn't clear an ab-loop set too early.
        let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::LoadFileSeekAndLoop(path, loop_a, b));
    } else {
        // No reload needed — media already loaded; set the loop directly.
        let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SetAbLoop { a: loop_a, b });
    }
    s.ab_repeat.a_time = Some(a);
    s.ab_repeat.b_time = Some(b);
    s.ab_repeat.loop_active = true;
    s.suppress_sync_until =
        Some(std::time::Instant::now() + crate::input::navigation::SYNC_SUPPRESS_SEEK);
    crate::logging::log(&format!("ECHOES: re-armed source turn loop [{:.1}, {:.1}]", loop_a, b));
}

/// Toggle the curated flag on the selected echo, persist, and re-render
/// (curated echoes re-sort to the top).
pub(crate) fn toggle_curated(state_rc: &Rc<RefCell<AppState>>) {
    let (turn_id, link_id, channel) = {
        let s = state_rc.borrow();
        let link = match s.echo_overlay_links.get(s.echo_overlay_index) {
            Some(l) => l,
            None => return,
        };
        let channel = s.echo_session.as_ref().map(|x| x.channel).unwrap_or(crate::db::echo_channel::EchoChannel::Shakespeare);
        (s.echo_overlay_turn_id, link.link_id, channel)
    };
    let turn_id = match turn_id {
        Some(id) => id,
        None => return,
    };

    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::queries::toggle_echo_curated(&conn, link_id);
    }

    // Reload from DB to pick up the new curated-first ordering.
    let links = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::queries::load_echo_links(&conn, turn_id, channel).ok())
        .unwrap_or_default();

    let mut s = state_rc.borrow_mut();
    // Keep selection on the same link after re-sort.
    let new_idx = links.iter().position(|l| l.link_id == link_id).unwrap_or(0);
    s.echo_overlay_links = links;
    s.echo_overlay_index = new_idx;
    render_echoes(&mut s);
    s.gloss_overlay.scroll_echo_into_view(new_idx);
    sync_session(&mut s);
    crate::logging::log("ECHOES: toggled curated");
}

/// `d`: delete the selected echo link, then reload the list keeping the
/// selection near where it was.
pub(crate) fn delete_selected_echo(state_rc: &Rc<RefCell<AppState>>) {
    let (turn_id, link_id, old_idx, channel) = {
        let s = state_rc.borrow();
        let link = match s.echo_overlay_links.get(s.echo_overlay_index) {
            Some(l) => l,
            None => return,
        };
        let channel = s.echo_session.as_ref().map(|x| x.channel).unwrap_or(crate::db::echo_channel::EchoChannel::Shakespeare);
        match s.echo_overlay_turn_id {
            Some(id) => (id, link.link_id, s.echo_overlay_index, channel),
            None => return,
        }
    };

    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::queries::delete_echo_link(&conn, link_id);
    }

    let links = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::queries::load_echo_links(&conn, turn_id, channel).ok())
        .unwrap_or_default();

    let mut s = state_rc.borrow_mut();
    // Clamp the selection to the (now shorter) list, keeping the cursor roughly
    // in place rather than jumping to the top.
    let new_idx = old_idx.min(links.len().saturating_sub(1));
    s.echo_overlay_links = links;
    s.echo_overlay_index = new_idx;
    render_echoes(&mut s);
    s.gloss_overlay.scroll_echo_into_view(new_idx);
    sync_session(&mut s);
    crate::logging::log("ECHOES: deleted selected echo");
}

/// `D`: delete ALL echo links (curated and non-curated) for the current source
/// turn, leaving the list empty.
pub(crate) fn delete_all_echoes(state_rc: &Rc<RefCell<AppState>>) {
    let turn_id = match state_rc.borrow().echo_overlay_turn_id {
        Some(id) => id,
        None => return,
    };

    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::queries::delete_all_echo_links(&conn, turn_id);
    }

    let mut s = state_rc.borrow_mut();
    s.echo_overlay_links = Vec::new();
    s.echo_overlay_index = 0;
    render_echoes(&mut s);
    sync_session(&mut s);
    crate::logging::log("ECHOES: deleted all echoes for turn");
}

/// Reorder the selected echo within the curated group (delta -1 = up, +1 = down),
/// marking it curated. Curated items always sort above non-curated; this moves
/// the selection among them and persists sequential ranks. Mirrors toggle_curated's
/// reload-and-keep-selection pattern.
pub(crate) fn reorder_selected_echo(state_rc: &Rc<RefCell<AppState>>, delta: i32) {
    let (turn_id, sel_link_id, links, channel) = {
        let s = state_rc.borrow();
        let link = match s.echo_overlay_links.get(s.echo_overlay_index) {
            Some(l) => l.clone(),
            None => return,
        };
        let channel = s.echo_session.as_ref().map(|x| x.channel).unwrap_or(crate::db::echo_channel::EchoChannel::Shakespeare);
        match s.echo_overlay_turn_id {
            Some(id) => (id, link.link_id, s.echo_overlay_links.clone(), channel),
            None => return,
        }
    };

    // Curated prefix in current display order (links are loaded curated DESC, rank ASC).
    let mut curated: Vec<i64> = links.iter().filter(|l| l.curated).map(|l| l.link_id).collect();
    let sel_is_curated = links.iter().any(|l| l.link_id == sel_link_id && l.curated);

    // Index of the selected link within the curated order (curate-on-move if not).
    let from = if sel_is_curated {
        curated.iter().position(|&id| id == sel_link_id).unwrap_or(0)
    } else {
        // Not yet curated: append to the curated tail, then move from there.
        curated.push(sel_link_id);
        curated.len() - 1
    };
    let to = from as i32 + delta;
    if to < 0 || to >= curated.len() as i32 {
        // At an edge of the curated group. If we just curated it, still persist;
        // otherwise no-op.
        if sel_is_curated {
            return;
        }
    }
    let to = to.clamp(0, curated.len() as i32 - 1) as usize;
    curated.swap(from, to);

    // Persist sequential ranks for the curated order; all curated=true.
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        for (rank, link_id) in curated.iter().enumerate() {
            let _ = crate::db::queries::set_echo_link_rank(&conn, *link_id, rank as i64, true);
        }
    }

    // Reload, keep selection on the moved link.
    let links = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::queries::load_echo_links(&conn, turn_id, channel).ok())
        .unwrap_or_default();
    let mut s = state_rc.borrow_mut();
    let new_idx = links.iter().position(|l| l.link_id == sel_link_id).unwrap_or(0);
    s.echo_overlay_links = links;
    s.echo_overlay_index = new_idx;
    render_echoes(&mut s);
    s.gloss_overlay.scroll_echo_into_view(new_idx);
    sync_session(&mut s);
    crate::logging::log("ECHOES: reordered echo");
}

/// `A` in the echoes overlay: open the line-search picker to add an echo to the
/// current turn. Stashes the turn_id for the deferred add.
pub(crate) fn open_add_echo_picker(state_rc: &Rc<RefCell<AppState>>) {
    let turn_id = state_rc.borrow().echo_overlay_turn_id;
    if turn_id.is_none() {
        return;
    }
    // Set up picker state under a mutable borrow that ENDS before show().
    {
        let mut s = state_rc.borrow_mut();
        s.echo_add_turn_id = turn_id;
        let titles = s.echo_overlay_titles.clone();
        s.echo_line_picker.set_results(Vec::new(), &titles);
        s.input_mode = crate::app::InputMode::EchoLinePicker;
    }
    // show() calls set_text(""), which synchronously fires the entry's
    // connect_changed -> refresh_add_echo_search (which borrows state). Hold only
    // a short immutable borrow across show(); refresh_add_echo_search is
    // re-entrancy-safe (try_borrow/try_borrow_mut) so the spurious initial fire
    // bails instead of panicking.
    state_rc.borrow().echo_line_picker.show();
    crate::logging::log("ECHOES: opened add-echo line picker");
}

/// Re-run the line search for the picker's current entry text (called on each
/// keystroke). Empty query clears the list.
pub(crate) fn refresh_add_echo_search(state_rc: &Rc<RefCell<AppState>>) {
    // This handler is fired synchronously by the entry's connect_changed, which
    // open_add_echo_picker triggers via set_text("") while holding an immutable
    // borrow. Use try_borrow/try_borrow_mut so that re-entrant call bails instead
    // of panicking; on normal keystrokes no outer borrow is held and both succeed.
    let query = match state_rc.try_borrow() {
        Ok(s) => s.echo_line_picker.entry().text().to_string(),
        Err(_) => return,
    };
    let results = if query.trim().is_empty() {
        Vec::new()
    } else {
        crate::db::queries::open_db()
            .ok()
            .and_then(|conn| crate::db::queries::search_lines(&conn, query.trim(), 200).ok())
            .unwrap_or_default()
    };
    if let Ok(mut s) = state_rc.try_borrow_mut() {
        let titles = s.echo_overlay_titles.clone();
        s.echo_line_picker.set_results(results, &titles);
    }
}

/// Confirm the selected line in the add-echo picker: add it as a curated echo at
/// the top of the rankings (or promote an existing matching echo), then return
/// to the echoes overlay.
pub(crate) fn confirm_add_echo(state_rc: &Rc<RefCell<AppState>>) {
    let hit = state_rc.borrow().echo_line_picker.selected_hit();
    let hit = match hit {
        Some(h) => h,
        None => {
            cancel_add_echo(state_rc);
            return;
        }
    };
    let turn_id = state_rc.borrow().echo_add_turn_id;
    let turn_id = match turn_id {
        Some(id) => id,
        None => {
            cancel_add_echo(state_rc);
            return;
        }
    };
    let (work, div1, div2, line_in_div, text) = hit;

    let existing_id = state_rc.borrow().echo_overlay_links.iter()
        .find(|l| l.echo_work_abbrev == work && l.echo_div1 == div1
                  && l.echo_div2 == div2 && l.echo_start_line == line_in_div)
        .map(|l| l.link_id);

    let new_link_id = if let Ok(conn) = crate::db::queries::open_db_rw() {
        if let Some(id) = existing_id {
            // Promote: shift other curated +1, set this to curated rank 0.
            let _ = conn.execute(
                "UPDATE echo_links SET rank = rank + 1 WHERE turn_id = ?1 AND curated = 1",
                [turn_id],
            );
            let _ = crate::db::queries::set_echo_link_rank(&conn, id, 0, true);
            Some(id)
        } else {
            crate::db::queries::add_curated_echo_link(&conn, turn_id, &work, div1, div2, line_in_div, &text).ok()
        }
    } else {
        None
    };

    let channel = state_rc.borrow().echo_session.as_ref().map(|x| x.channel).unwrap_or(crate::db::echo_channel::EchoChannel::Shakespeare);
    let links = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::queries::load_echo_links(&conn, turn_id, channel).ok())
        .unwrap_or_default();
    let mut s = state_rc.borrow_mut();
    s.echo_line_picker.hide();
    s.echo_add_turn_id = None;
    let new_idx = new_link_id
        .and_then(|id| links.iter().position(|l| l.link_id == id))
        .unwrap_or(0);
    s.echo_overlay_links = links;
    s.echo_overlay_index = new_idx;
    s.input_mode = crate::app::InputMode::EchoesOverlay;
    render_echoes(&mut s);
    s.gloss_overlay.scroll_echo_into_view(new_idx);
    sync_session(&mut s);
    crate::logging::log("ECHOES: added echo from line picker");
}

/// Cancel the add-echo picker, returning to the echoes overlay.
pub(crate) fn cancel_add_echo(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    s.echo_line_picker.hide();
    s.echo_add_turn_id = None;
    s.input_mode = crate::app::InputMode::EchoesOverlay;
}

/// Re-run the search for the current turn, overwriting non-curated links;
/// curated links are always kept.
pub(crate) fn refresh_echoes(
    state_rc: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let (key, turn_id, channel) = {
        let s = state_rc.borrow();
        let channel = s.echo_session.as_ref().map(|x| x.channel).unwrap_or(crate::db::echo_channel::EchoChannel::Shakespeare);
        match (&s.echo_overlay_turn_key, s.echo_overlay_turn_id) {
            (Some(k), Some(id)) => (k.clone(), id, channel),
            _ => return,
        }
    };

    let affect_weight = state_rc.borrow().config.echo_affect_weight;
    state_rc.borrow().gloss_overlay.show_loading_message("Refreshing echoes...");

    let query = format!("{} to {}: {}", key.speaker, "?", key.turn_text);
    let query_text = key.turn_text.clone();
    let source_work = key.work_abbrev.clone();
    let state_for_result = Rc::clone(state_rc);
    let echo_handle = tokio_handle.clone();

    glib::spawn_future_local(async move {
        let embed_result = echo_handle
            .spawn(async move { crate::voyage::embed_query(&query).await })
            .await;

        let raw = match embed_result {
            Ok(Ok(embedding)) => crate::db::queries::open_db()
                .ok()
                .and_then(|conn| {
                    crate::db::queries::find_similar_passages(
                        &conn, &embedding, &query_text, &source_work, 60, affect_weight,
                    )
                    .ok()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };

        let mut seen = std::collections::HashSet::new();
        let mut candidates = Vec::new();
        for cand in raw {
            let k = first_sentence(&cand.passage_text).to_lowercase();
            if k.is_empty() || !seen.insert(k) {
                continue;
            }
            candidates.push(cand);
            if candidates.len() >= 15 {
                break;
            }
        }

        // Delete non-curated, re-insert fresh; curated links untouched.
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            let _ = crate::db::queries::delete_noncurated_echo_links(&conn, turn_id);
            let rows: Vec<(String, i64, i64, i64, String, f32, i64)> = candidates
                .iter()
                .enumerate()
                .map(|(i, c)| (
                    c.work_abbrev.clone(), c.div1, c.div2, c.start_line,
                    first_sentence_verse(&c.passage_text), c.similarity, i as i64,
                ))
                .collect();
            let _ = crate::db::queries::insert_echo_links(&conn, turn_id, &rows);
        }

        let links = crate::db::queries::open_db()
            .ok()
            .and_then(|conn| crate::db::queries::load_echo_links(&conn, turn_id, channel).ok())
            .unwrap_or_default();

        let mut s = state_for_result.borrow_mut();
        s.echo_overlay_links = links;
        s.echo_overlay_index = 0;
        render_echoes(&mut s);
        sync_session(&mut s);
        crate::logging::log("ECHOES: refreshed echoes");
    });
}

/// Enter: jump to the selected echo's work, cursor on the echoed line. The
/// echo session is kept so alt+i can return.
pub(crate) fn jump_to_selected_echo(
    state_rc: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let (work, div1, div2, line_in_div) = {
        let s = state_rc.borrow();
        match s.echo_overlay_links.get(s.echo_overlay_index) {
            Some(l) => (l.echo_work_abbrev.clone(), l.echo_div1, l.echo_div2, l.echo_start_line),
            None => return,
        }
    };

    // Make sure the session reflects the current selection before we leave.
    sync_session(&mut state_rc.borrow_mut());

    let line_id = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::queries::line_id_for_location(&conn, &work, div1, div2, line_in_div));
    let line_id = match line_id {
        Some(id) => id,
        None => {
            state_rc.borrow().gloss_overlay.show("Could not locate the echoed line.", "");
            crate::logging::log("ECHOES: could not resolve echo line");
            return;
        }
    };

    let was_playing = state_rc.borrow().mpv_playing;
    {
        let mut s = state_rc.borrow_mut();
        s.gloss_overlay.hide();
        s.echo_overlay_links.clear();
        s.input_mode = crate::app::InputMode::Reader;
    }

    load_work_at_line(state_rc, tokio_handle, &work, line_id, was_playing);
    crate::logging::log(&format!("ECHOES: jumped to echo in {} line_id={}", work, line_id));
}

/// `a` in the echoes overlay: play the selected echo's media in the existing
/// MPV instance without opening its work. Always (re)starts playback from the
/// echo's start time, whether currently playing or paused. The source-turn
/// loop range is preserved so `Tab` can restore it; the reader display is
/// untouched.
pub(crate) fn play_selected_echo(
    state_rc: &Rc<RefCell<AppState>>,
    _tokio_handle: &tokio::runtime::Handle,
) {
    let link = {
        let s = state_rc.borrow();
        match s.echo_overlay_links.get(s.echo_overlay_index) {
            Some(l) => l.clone(),
            None => return,
        }
    };

    // Resolve the echo line, its Arkangel media, and its start time.
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };
    let line_id = match crate::db::queries::line_id_for_location(
        &conn, &link.echo_work_abbrev, link.echo_div1, link.echo_div2, link.echo_start_line,
    ) {
        Some(id) => id,
        None => {
            state_rc.borrow().gloss_overlay.show("Could not locate the echoed line.", "");
            crate::logging::log("ECHOES: could not resolve echo line for playback");
            return;
        }
    };
    let media = match crate::db::queries::list_media_for_work(&conn, &link.echo_work_abbrev) {
        Ok(items) if !items.is_empty() => {
            // Prefer Arkangel; fall back to the highest-priority media (first).
            items.iter().find(|m| m.path.contains("/aax-Arkangel/"))
                .cloned()
                .unwrap_or_else(|| items[0].clone())
        }
        _ => {
            state_rc.borrow().gloss_overlay.show("No media for this echo's work.", "");
            crate::logging::log("ECHOES: no media for echo work");
            return;
        }
    };
    let start = match crate::db::queries::line_start_time(&conn, line_id, media.media_id) {
        Some(t) => t,
        None => {
            state_rc.borrow().gloss_overlay.show("No timestamp for the echoed line.", "");
            crate::logging::log("ECHOES: no timestamp for echo line");
            return;
        }
    };
    let seek = (start - crate::input::navigation::SEEK_PREROLL).max(0.0);

    let mut s = state_rc.borrow_mut();
    // Don't loop the source turn while auditioning the echo; keep the remembered
    // (a_time, b_time) so `Tab` can re-arm it.
    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::ClearAbLoop);
    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::LoadFileAndSeek(media.path.clone(), seek));
    s.ab_repeat.loop_active = false;
    s.suppress_sync_until =
        Some(std::time::Instant::now() + crate::input::navigation::SYNC_SUPPRESS_SEEK);
    crate::logging::log(&format!(
        "ECHOES: playing echo {} line_id={} @{:.1}", link.echo_work_abbrev, line_id, seek
    ));
}

/// alt+i: return to the turn's work and line, then reopen the echoes overlay
/// from the sticky session.
pub(crate) fn reopen_echoes(
    state_rc: &Rc<RefCell<AppState>>,
    _channel: crate::db::echo_channel::EchoChannel,
    tokio_handle: &tokio::runtime::Handle,
) {
    let session = match state_rc.borrow().echo_session.clone() {
        Some(s) => s,
        None => return,
    };

    let current_work = state_rc.borrow().current_work.as_ref().map(|w| w.abbrev.clone());
    let was_playing = state_rc.borrow().mpv_playing;
    let restore = Rc::clone(state_rc);
    let sess_for_restore = session.clone();
    let reopen = move |state_rc: &Rc<RefCell<AppState>>| {
        let mut s = state_rc.borrow_mut();
        s.echo_overlay_source = sess_for_restore.source_doc.clone();
        s.echo_overlay_links = sess_for_restore.links.clone();
        s.echo_overlay_index = sess_for_restore.selected.min(sess_for_restore.links.len().saturating_sub(1));
        s.echo_overlay_titles = sess_for_restore.titles.clone();
        s.echo_overlay_turn_id = sess_for_restore.turn_id;
        s.echo_overlay_turn_key = Some(sess_for_restore.turn_key.clone());
        s.input_mode = crate::app::InputMode::EchoesOverlay;
        let idx = s.echo_overlay_index;
        render_echoes(&mut s);
        s.gloss_overlay.scroll_echo_into_view(idx);
    };

    if current_work.as_deref() == Some(session.origin_work.as_str()) {
        // Already on the origin work — just reopen the overlay.
        reopen(&restore);
        crate::logging::log("ECHOES: reopened overlay (same work)");
        return;
    }

    // Cross-work: load the origin work at the turn line, then reopen.
    load_work_at_line_then(
        state_rc,
        tokio_handle,
        &session.origin_work,
        session.origin_line_id,
        was_playing,
        Some(Box::new(move || reopen(&restore))),
    );
    crate::logging::log("ECHOES: returning to turn work and reopening overlay");
}

/// Load `work_abbrev` and place the cursor on `line_id` (line_mapping.id).
fn load_work_at_line(
    state_rc: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
    work_abbrev: &str,
    line_id: i64,
    was_playing: bool,
) {
    load_work_at_line_then(state_rc, tokio_handle, work_abbrev, line_id, was_playing, None);
}

type AfterLoad = Box<dyn FnOnce()>;

/// Load `work_abbrev` at `line_id`, switch MPV to its media (resuming playback
/// if `was_playing`), then optionally run `after`. Mirrors the concordance
/// cross-work load path.
fn load_work_at_line_then(
    state_rc: &Rc<RefCell<AppState>>,
    _tokio_handle: &tokio::runtime::Handle,
    work_abbrev: &str,
    line_id: i64,
    was_playing: bool,
    after: Option<AfterLoad>,
) {
    {
        let mut s = state_rc.borrow_mut();
        crate::app::save_position(&mut s);
    }
    let state_clone = Rc::clone(state_rc);
    let abbrev = work_abbrev.to_string();
    let handle = state_rc.borrow().tokio_handle.clone();

    glib::spawn_future_local(async move {
        let result = handle
            .spawn_blocking(move || {
                let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                let work = crate::db::queries::load_work(&conn, &abbrev)?;
                let prepared = crate::app::text_prep::prepare_text_for_display(&work);
                Ok::<_, rusqlite::Error>((work, prepared))
            })
            .await;
        if let Ok(Ok((work, prepared))) = result {
            {
                let mut s = state_clone.borrow_mut();
                s.skip_mpv_discovery = true;
                crate::app::clear_display(&mut s);
                crate::app::display_work_at_with_prepared(&mut s, work, Some(line_id), prepared);
                crate::input::highlight::update_highlight_and_center(&mut s);
            }

            // Switch MPV to the target work's media (auto-select Arkangel) and
            // seek to the target line, preserving the prior play/pause state.
            switch_mpv_to_current_line(&state_clone, line_id, was_playing);

            if let Some(cb) = after {
                cb();
            }
        } else {
            crate::logging::log("ECHOES: failed to load work for jump");
        }
    });
}

/// Auto-select the Arkangel media for the loaded work and load it into MPV,
/// seeking to `line_id`'s timestamp. Resumes playback if `was_playing`, else
/// stays paused. Falls back to the media picker if no Arkangel media is found.
fn switch_mpv_to_current_line(state_rc: &Rc<RefCell<AppState>>, line_id: i64, was_playing: bool) {
    let auto_media = {
        let s = state_rc.borrow();
        s.current_work.as_ref().and_then(|w| {
            w.media_paths.iter().zip(w.media_ids.iter())
                .find(|(p, _)| p.contains("/aax-Arkangel/"))
                .map(|(p, id)| (p.clone(), *id))
        })
    };

    let seek_time = {
        let s = state_rc.borrow();
        s.current_work.as_ref()
            .and_then(|w| w.lines.iter().find(|l| l.id == line_id))
            .and_then(|l| l.timestamp.as_ref())
            .map(|ts| (ts.start - crate::input::navigation::SEEK_PREROLL).max(0.0))
    };

    if let Some((path, media_id)) = auto_media {
        let already_connected = state_rc.borrow().mpv_connected;
        if already_connected {
            let s = state_rc.borrow();
            match (seek_time, was_playing) {
                (Some(t), true) => {
                    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::LoadFileAndSeek(path.clone(), t));
                }
                (Some(t), false) => {
                    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::LoadFileSeekPaused(path.clone(), t));
                }
                (None, _) => {
                    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::LoadFile(path.clone()));
                    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
                }
            }
            crate::logging::log(&format!(
                "ECHOES: switched MPV to Arkangel media_id={} seek={:?} playing={}",
                media_id, seek_time, was_playing
            ));
        }
        state_rc.borrow_mut().media_id = Some(media_id);
    } else {
        let handle = state_rc.borrow().tokio_handle.clone();
        crate::input::actions::pickers::open_media_picker(state_rc, &handle);
    }
}

/// Open the echo-turns picker: list every turn in the current work that has
/// echoes (Ctrl+Shift+G). Empty work -> toast and stay in Reader.
pub(crate) fn open_echo_turns_picker(state_rc: &Rc<RefCell<AppState>>, channel: crate::db::echo_channel::EchoChannel) {
    let work_abbrev = match state_rc.borrow().current_work.as_ref() {
        Some(w) => w.abbrev.clone(),
        None => return,
    };

    let (turns, titles) = {
        let conn = match crate::db::queries::open_db() {
            Ok(c) => c,
            Err(e) => {
                crate::logging::log(&format!("ECHO-TURNS: open_db failed: {e}"));
                show_no_echo_turns_toast(state_rc);
                return;
            }
        };
        let turns = crate::db::queries::list_echo_turns_for_work(&conn, &work_abbrev, channel)
            .unwrap_or_default();
        let titles = crate::db::queries::load_work_titles(&conn).unwrap_or_default();
        (turns, titles)
    };

    if turns.is_empty() {
        crate::logging::log("ECHO-TURNS: no echo turns in this work");
        show_no_echo_turns_toast(state_rc);
        return;
    }

    let mut s = state_rc.borrow_mut();
    s.echo_turns_picker.channel = channel;
    s.echo_turns_picker.set_titles(titles);
    s.echo_turns_picker.set_items(turns, work_abbrev);
    s.echo_turns_picker.show();
    s.input_mode = crate::app::InputMode::EchoTurnsPicker;
}

fn show_no_echo_turns_toast(state_rc: &Rc<RefCell<AppState>>) {
    let s = state_rc.borrow();
    crate::ui::toast::show_transient(&s.chapter_toast, "No echoes in this work", 3);
}

/// Confirm the echo-turns picker selection: jump the cursor to the turn's
/// first line, then open its echoes overlay via the normal cursor path
/// (cache hit, no API call).
pub(crate) fn confirm_echo_turns_pick(
    state_rc: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let channel = state_rc.borrow().echo_turns_picker.channel;
    let picked = {
        let s = state_rc.borrow();
        s.echo_turns_picker
            .selected_index()
            .and_then(|idx| s.echo_turns_picker.items.get(idx).cloned())
    };
    let picked = match picked {
        Some(p) => p,
        None => {
            let s = state_rc.borrow();
            s.echo_turns_picker.hide();
            return;
        }
    };

    // Resolve (div1, div2, start_line) -> buffer line index and jump.
    let jumped = {
        let mut s = state_rc.borrow_mut();
        s.echo_turns_picker.hide();
        s.input_mode = crate::app::InputMode::Reader;

        let work_idx = s.current_work.as_ref().and_then(|w| {
            w.lines.iter().position(|l| {
                l.div1 == picked.div1
                    && l.div2 == picked.div2
                    && l.line_in_div == picked.start_line
            })
        });
        match work_idx {
            Some(wi) => {
                let buf_idx = match s.line_map {
                    Some(ref lm) => lm.work_to_buffer[wi],
                    None => wi,
                };
                s.current_line = buf_idx;
                crate::input::highlight::update_highlight_and_center(&mut s);
                true
            }
            None => {
                crate::logging::log(&format!(
                    "ECHO-TURNS: turn line {}.{}.{} not found in loaded work",
                    picked.div1, picked.div2, picked.start_line
                ));
                false
            }
        }
    };

    if jumped {
        show_echoes_for_cursor_line(state_rc, channel, tokio_handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::Line;

    fn line(id: i64, speaker: Option<&str>, div1: i64, div2: i64, line_in_div: i64, text: &str) -> Line {
        Line {
            id,
            citation: String::new(),
            text: text.to_string(),
            normalized: String::new(),
            speaker: speaker.map(|s| s.to_string()),
            is_dialogue: true,
            timestamp: None,
            div1,
            div2,
            line_in_div,
            is_chapter: false,
            is_spoken: None,
        }
    }

    fn sample_work_lines() -> Vec<Line> {
        vec![
            line(10, Some("HAMLET"), 1, 2, 1, "To be, or not to be"),
            line(11, Some("HAMLET"), 1, 2, 2, "that is the question"),
            line(12, Some("OPHELIA"), 1, 2, 3, "Good my lord"),
            line(13, Some("OPHELIA"), 1, 2, 4, "How does your honour"),
            line(14, Some("HAMLET"), 1, 2, 5, "I humbly thank you"),
        ]
    }

    #[test]
    fn selection_turn_lines_clips_and_collects_range() {
        let work = sample_work_lines();
        let got = selection_turn_lines(&work, 1, 3);
        let ids: Vec<i64> = got.iter().map(|l| l.id).collect();
        assert_eq!(ids, vec![11, 12, 13]);
    }

    #[test]
    fn selection_turn_lines_clamps_end_past_bounds() {
        let work = sample_work_lines();
        let got = selection_turn_lines(&work, 3, 999);
        let ids: Vec<i64> = got.iter().map(|l| l.id).collect();
        assert_eq!(ids, vec![13, 14]);
    }

    #[test]
    fn selection_turn_lines_empty_when_start_past_end_of_work() {
        let work = sample_work_lines();
        assert!(selection_turn_lines(&work, 99, 100).is_empty());
    }

    #[test]
    fn selection_key_uses_first_and_last_line_div_and_line_in_div() {
        let work = sample_work_lines();
        let turn = selection_turn_lines(&work, 1, 3);
        let key = selection_key("HAM", &turn);
        assert_eq!(key.work_abbrev, "HAM");
        assert_eq!(key.div1, 1);
        assert_eq!(key.div2, 2);
        assert_eq!(key.start_line, 2);
        assert_eq!(key.end_line, 4);
        assert_eq!(key.speaker, "HAMLET");
        assert_eq!(key.turn_text, "that is the question Good my lord How does your honour");
    }

    #[test]
    fn source_header_single_speaker_emits_one_tag() {
        // Regression guard: a single-speaker turn must render exactly one
        // <speaker> tag above all verse lines, unchanged from prior behavior.
        let turn = vec![
            line(10, Some("HAMLET"), 1, 2, 1, "To be, or not to be"),
            line(11, Some("HAMLET"), 1, 2, 2, "that is the question"),
        ];
        let doc = build_source_header(&turn, "Hamlet");
        assert_eq!(
            doc,
            "<speaker>HAMLET</speaker>\n\
             <verse>To be, or not to be</verse>\n\
             <verse>that is the question</verse>\n"
        );
    }

    #[test]
    fn source_header_multi_speaker_emits_tag_per_run() {
        // A selection spanning multiple speakers must emit a <speaker> tag at
        // each change of speaker, not a single label over everything.
        let turn = vec![
            line(11, Some("HAMLET"), 1, 2, 2, "that is the question"),
            line(12, Some("OPHELIA"), 1, 2, 3, "Good my lord"),
            line(13, Some("OPHELIA"), 1, 2, 4, "How does your honour"),
            line(14, Some("HAMLET"), 1, 2, 5, "I humbly thank you"),
        ];
        let doc = build_source_header(&turn, "Hamlet");
        assert_eq!(
            doc,
            "<speaker>HAMLET</speaker>\n\
             <verse>that is the question</verse>\n\
             <speaker>OPHELIA</speaker>\n\
             <verse>Good my lord</verse>\n\
             <verse>How does your honour</verse>\n\
             <speaker>HAMLET</speaker>\n\
             <verse>I humbly thank you</verse>\n"
        );
    }

    #[test]
    fn source_header_lines_without_speaker_use_fallback_label() {
        // Lines whose own speaker is None fall back to the passed label and do
        // not introduce a spurious extra <speaker> tag mid-run.
        let turn = vec![
            line(10, None, 1, 2, 1, "A stage direction perhaps"),
            line(11, None, 1, 2, 2, "still no speaker"),
        ];
        let doc = build_source_header(&turn, "Hamlet");
        assert_eq!(
            doc,
            "<speaker>HAMLET</speaker>\n\
             <verse>A stage direction perhaps</verse>\n\
             <verse>still no speaker</verse>\n"
        );
    }
}
