use gtk4::prelude::*;

use crate::app::AppState;
use crate::db::models::TimeRange;

#[derive(Debug, Clone)]
pub struct TimestampSnapshot {
    pub citation: String,
    pub start_time: Option<f64>,
    pub end_time: Option<f64>,
    pub is_track_mark: bool,
}

#[derive(Debug, Clone)]
pub struct TimestampUndoState {
    pub line_mapping_id: i64,
    pub media_id: i64,
    /// None = row didn't exist before the operation (undo → DELETE)
    pub previous: Option<TimestampSnapshot>,
}

const NUDGE_STEP: f64 = 0.2;
const NOT_SPOKEN_TOAST: &str = "Not a spoken line — no timestamp set";

/// `u`/end-time are audio timestamps — meaningful only on a SPOKEN line. A stage
/// direction (`sub_line > 0`) that is not marked spoken (`is_spoken != Some(true)`)
/// must be rejected; dialogue lines (`sub_line == 0`) and spoken SDs pass.
fn timestamp_allowed(sub_line: i64, is_spoken: Option<bool>) -> bool {
    sub_line == 0 || is_spoken == Some(true)
}

#[cfg(test)]
mod timestamp_gate_tests {
    use super::timestamp_allowed;
    #[test]
    fn dialogue_line_allowed() {
        assert!(timestamp_allowed(0, None));
        assert!(timestamp_allowed(0, Some(false)));
    }
    #[test]
    fn unspoken_stage_direction_rejected() {
        assert!(!timestamp_allowed(1, None));
        assert!(!timestamp_allowed(2, Some(false)));
    }
    #[test]
    fn spoken_stage_direction_allowed() {
        assert!(timestamp_allowed(1, Some(true)));
    }
}

/// Check whether a timestamp can be written on the given line. Returns true if
/// the line is writable (dialogue or spoken stage direction); returns false and
/// logs + toasts if the line is an unspoken stage direction.
fn timestamp_writable(state: &AppState, line_idx: usize) -> bool {
    let work = match &state.current_work {
        Some(w) => w,
        None => return false,
    };
    let l = &work.lines[line_idx];
    if !timestamp_allowed(l.sub_line, l.is_spoken) {
        crate::logging::log(&format!(
            "TS: refused start/end time on unspoken stage direction (line {}, sub_line {})",
            line_idx, l.sub_line
        ));
        crate::input::navigation::show_chapter_toast(state, NOT_SPOKEN_TOAST);
        return false;
    }
    true
}

/// Open the read-write db, logging the `TS: open_db_rw failed` message on
/// failure and returning `None`. The shared head of the timestamp-write fns;
/// callers do `let Some(conn) = open_db_rw_or_log() else { return false; };`,
/// keeping their own early-return (the helper can't own the caller's `return`).
fn open_db_rw_or_log() -> Option<rusqlite::Connection> {
    match crate::db::queries::open_db_rw() {
        Ok(c) => Some(c),
        Err(e) => {
            crate::logging::log(&format!("TS: open_db_rw failed: {}", e));
            None
        }
    }
}

/// Extract the line id for a given work line index, or None if work not loaded or index out of bounds.
fn work_line_id(state: &AppState, line_idx: usize) -> Option<i64> {
    Some(state.current_work.as_ref()?.lines.get(line_idx)?.id)
}

/// Re-send timestamps to MPV client after a write, built from Line.timestamp (single source of truth).
fn resync_mpv_timestamps(state: &AppState) {
    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };
    let mut ts_data: Vec<(i64, f64, f64)> = Vec::new();
    let mut id_to_idx: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
    for (i, line) in work.lines.iter().enumerate() {
        id_to_idx.insert(line.id, i);
        if line.is_dialogue {
            if let Some(ts) = &line.timestamp {
                ts_data.push((line.id, ts.start, ts.end));
            }
        }
    }
    ts_data.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let _ = state
        .cmd_tx
        .try_send(crate::mpv::MpvCommand::SetTimestamps {
            timestamps: ts_data,
            line_id_to_index: id_to_idx,
        });
}

/// Capture the current state of a timestamp row before mutating it.
/// Stores the snapshot in state.timestamp_undo for single-level undo.
fn capture_undo_snapshot(state: &mut AppState, line_mapping_id: i64, media_id: i64) {
    let conn = match crate::db::queries::open_db_rw() {
        Ok(c) => c,
        Err(_) => return,
    };
    let previous = crate::db::queries::get_timestamp_snapshot(&conn, line_mapping_id, media_id)
        .unwrap_or(None);
    state.timestamp_undo = Some(TimestampUndoState {
        line_mapping_id,
        media_id,
        previous,
    });
}

/// Set start time on current line from MPV position (u / Right).
pub fn set_start_time(state: &mut AppState) -> bool {
    let media_id = match state.media_id {
        Some(id) => id,
        None => {
            crate::logging::log("TS: set_start_time failed: no media_id");
            return false;
        }
    };
    let time_pos = (state.current_time_pos - 0.30).max(0.0);
    let line_idx = match state.work_line_for_buffer(state.current_line) {
        Some(i) => i,
        None => {
            crate::logging::log(&format!(
                "TS: set_start_time failed: no work line for buffer line {}",
                state.current_line
            ));
            return false;
        }
    };

    if !timestamp_writable(state, line_idx) {
        return false;
    }

    let Some(line_id) = work_line_id(state, line_idx) else { return false; };

    capture_undo_snapshot(state, line_id, media_id);

    // Compute end time from next dialogue line's start (if within 10s)
    let end_time = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line_count = work.lines.len();
        let mut next_end = 0.0f64;
        for i in (line_idx + 1)..line_count {
            if work.lines[i].is_dialogue {
                if let Some(ref ts) = work.lines[i].timestamp {
                    if ts.start > time_pos && ts.start - time_pos <= 10.0 {
                        next_end = (ts.start - 0.2).max(time_pos);
                    }
                }
                break;
            }
        }
        next_end
    };

    {
        let work = match &mut state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line = &mut work.lines[line_idx];

        let Some(conn) = open_db_rw_or_log() else { return false; };
        if let Err(e) = crate::db::queries::upsert_start_time(&conn, line.id, media_id, &line.citation, time_pos) {
            crate::logging::log(&format!("TS: upsert_start_time failed: {}", e));
            return false;
        }
        if let Err(e) = crate::db::queries::upsert_spoken_status(&conn, line.id, media_id, true) {
            // Non-fatal: the timestamp is already written; just log.
            crate::logging::log(&format!("TS: upsert_spoken_status failed: {}", e));
        }

        // Update in-memory
        match &mut line.timestamp {
            Some(ts) => {
                ts.start = time_pos;
                if end_time > 0.0 {
                    ts.end = end_time;
                }
            }
            None => line.timestamp = Some(TimeRange {
                start: time_pos,
                end: end_time,
                sentence_start: None,
                is_manual: true,
            }),
        }
        line.is_spoken = Some(true);

        if end_time > 0.0 {
            let _ = crate::db::queries::update_end_time(&conn, line.id, media_id, end_time);
        }
    }
    crate::logging::log(&format!("TS: set start={:.2} end={:.2} line={}", time_pos, end_time, line_idx));

    resync_mpv_timestamps(state);

    // Update sign column for this line (is_chapter_line intentionally not written here)
    let buffer_line = state.current_line;
    set_sign_columns(state, buffer_line, true, true, None);
    redraw_sign_gutters(state);

    // Advance the cursor to the next dialogue line WITHOUT seeking MPV: after
    // stamping a start time the user wants to keep listening from where the
    // audio is, not jump playback to the newly-selected line's start.
    crate::input::navigation::cursor_next_dialogue_no_seek(state);

    // Mark the new cursor segment. The advance above is no-seek
    // (PageChangeReason::Cursor), so the karaoke cue nav binds get from
    // seek_to_current_line never fires — and the stamping flow typically runs
    // under the indefinite sync suppression (an untimestamped landing arms
    // it), where every TimePos tick clears an UNHELD tint. On a timestamped
    // landing, karaoke_marks_cursor also suppresses the persistent cursor tint
    // (axis in karaoke mode + class mode on + media present with phrase rows +
    // cursor line timestamped — the karaoke tint is the only marking then), so
    // without this the cursor went invisible after every `b`. Paint the
    // pending phrase at the new line's start and hold it through the
    // suppression window, mirroring seek_to_current_line's karaoke block. An
    // untimestamped landing needs nothing here — update_highlight's
    // persistent-tint fallback marks it.
    if let Some(start) = state
        .work_line_for_buffer(state.current_line)
        .and_then(|wi| state.current_work.as_ref()?.lines.get(wi)?.timestamp.as_ref().map(|t| t.start))
    {
        if crate::input::phrase_highlight::paint_pending_phrase(state, start) {
            state.phrase_paint_hold = state.suppress_sync_until;
        }
    }

    true
}

/// Update the three sign-column `RefCell<Vec<bool>>` vecs for a single buffer
/// line.  Pass `is_chapter: None` to leave `is_chapter_line` untouched (used by
/// `set_start_time`, which never wrote that column).
fn set_sign_columns(state: &AppState, buffer_line: usize, has_ts: bool, is_manual: bool, is_chapter: Option<bool>) {
    {
        let mut ht = state.has_timestamp.borrow_mut();
        if buffer_line < ht.len() {
            ht[buffer_line] = has_ts;
        }
        let mut manual = state.is_manual.borrow_mut();
        if buffer_line < manual.len() {
            manual[buffer_line] = is_manual;
        }
    }
    if let Some(ch_val) = is_chapter {
        let mut ch = state.is_chapter_line.borrow_mut();
        if buffer_line < ch.len() {
            ch[buffer_line] = ch_val;
        }
    }
}

/// Queue a redraw on both column gutter renderers so the sign appears
/// immediately. In two-column mode the edited line may be in the right column,
/// whose renderer must be redrawn too — redrawing only the left gutter left the
/// sign missing until a page turn re-queried the right gutter.
pub(crate) fn redraw_sign_gutters(state: &AppState) {
    if let Some(ref renderer) = state.gutter_renderer {
        renderer.queue_draw();
    }
    if let Some(ref renderer) = state.right_gutter_renderer {
        renderer.queue_draw();
    }
}

/// Play the current line from its start time (a).
pub fn play_current_line(state: &mut AppState) -> bool {
    let line_idx = match state.work_line_for_buffer(state.current_line) {
        Some(i) => i,
        None => return false,
    };
    let start = match state.current_work.as_ref().and_then(|w| w.lines[line_idx].timestamp.as_ref()) {
        Some(ts) => ts.start,
        None => return false,
    };
    // Prose straddler at the page top: the segment's head rows (where the
    // seeked start_time's text lives) are on an EARLIER page, so replaying
    // from the start would park the narration and karaoke tint above the
    // fold. Turn back to the page holding the segment's first row first —
    // the cursor stays on the segment and playback is visible from its start.
    if state.is_prose() && state.current_line == state.page_top.line() && state.page_top.offset() > 0 {
        if let Some(table) = crate::input::prose_pages::active_prose_page_table(state) {
            if let Some(pi) = crate::input::prose_pages::prose_page_for_position(
                &table,
                state.current_line,
                0,
            ) {
                let p = table[pi];
                if (state.page_top.line(), state.page_top.offset()) != (p.start_line, p.start_off) {
                    crate::logging::log(&format!(
                        "PLAY_LINE: straddler at page top — back to page {} top=({},{})",
                        pi + 1,
                        p.start_line,
                        p.start_off
                    ));
                    crate::input::scroll::set_page_instant_offset(state, p.start_line, p.start_off);
                    crate::input::navigation::after_page_change(
                        state,
                        crate::input::navigation::PageChangeReason::Backward,
                    );
                }
            }
        } else {
            // Live-engine fallback: no stored page grid — put the segment's
            // first row at the page top so its start is visible.
            crate::logging::log("PLAY_LINE: straddler at page top — live fallback, segment row 0 to top");
            crate::input::scroll::set_page_instant(state, state.current_line);
            crate::input::navigation::after_page_change(
                state,
                crate::input::navigation::PageChangeReason::Backward,
            );
        }
    }
    let seek_time = crate::input::navigation::preroll_seek_time(start);
    let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::ResumeAndSeek(seek_time));
    // Suppress cursor sync so cursor stays on this line
    state.suppress_sync_until =
        Some(std::time::Instant::now() + crate::input::navigation::SYNC_SUPPRESS_SEEK);
    true
}

/// Set chapter on current line from MPV position (.).
pub fn set_chapter(state: &mut AppState) -> bool {
    let media_id = match state.media_id {
        Some(id) => id,
        None => return false,
    };
    let time_pos = state.current_time_pos;
    let line_idx = match state.work_line_for_buffer(state.current_line) {
        Some(i) => i,
        None => return false,
    };

    // If not already a track mark, reject if another track mark sits within ±10s.
    {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        let already = work
            .timestamps
            .iter()
            .any(|t| t.line_id == work.lines[line_idx].id
                && t.media_id == media_id
                && t.is_track_mark);
        if !already {
            let nearby = work.timestamps.iter().any(|t| {
                t.media_id == media_id
                    && t.is_track_mark
                    && t.line_id != work.lines[line_idx].id
                    && (t.start - time_pos).abs() <= 10.0
            });
            if nearby {
                crate::logging::log(&format!(
                    "TS: track mark rejected — another within 10s of {:.2}",
                    time_pos,
                ));
                return false;
            }
        }
    }

    let Some(line_id) = work_line_id(state, line_idx) else { return false; };

    capture_undo_snapshot(state, line_id, media_id);

    let new_val = {
        let work = match &mut state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line = &mut work.lines[line_idx];

        let Some(conn) = open_db_rw_or_log() else { return false; };
        let new_val = match crate::db::queries::upsert_chapter(&conn, line.id, media_id, &line.citation, time_pos) {
            Ok(v) => v,
            Err(e) => {
                crate::logging::log(&format!("TS: upsert_chapter failed: {}", e));
                return false;
            }
        };

        // Update in-memory: only set start_time if no timestamp exists yet
        if line.timestamp.is_none() {
            line.timestamp = Some(TimeRange { start: time_pos, end: 0.0, sentence_start: None, is_manual: true });
        }
        new_val
    };
    crate::logging::log(&format!("TS: toggle track mark is_track_mark={} start_time={:.2} line={}", new_val, time_pos, line_idx));

    resync_mpv_timestamps(state);

    // Update sign column for this line. The track-mark setter must NOT touch the
    // structural is_chapter sign (that follows divisions now) — pass None.
    let buffer_line = state.current_line;
    set_sign_columns(state, buffer_line, true, true, None);
    redraw_sign_gutters(state);

    true
}

/// Set end time on current line from MPV position (i).
pub fn set_end_time(state: &mut AppState) -> bool {
    let media_id = match state.media_id {
        Some(id) => id,
        None => return false,
    };
    let time_pos = state.current_time_pos;
    let line_idx = match state.work_line_for_buffer(state.current_line) {
        Some(i) => i,
        None => return false,
    };

    if !timestamp_writable(state, line_idx) {
        return false;
    }

    let Some(line_id) = work_line_id(state, line_idx) else { return false; };

    capture_undo_snapshot(state, line_id, media_id);

    let start_time = {
        let work = match &mut state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line = &mut work.lines[line_idx];

        // Guard: must have existing timestamp
        let start_time = match &line.timestamp {
            Some(ts) => ts.start,
            None => return false,
        };

        let Some(conn) = open_db_rw_or_log() else { return false; };
        if let Err(e) = crate::db::queries::update_end_time(&conn, line.id, media_id, time_pos) {
            crate::logging::log(&format!("TS: update_end_time failed: {}", e));
            return false;
        }

        // Update in-memory
        line.timestamp.as_mut().unwrap().end = time_pos;
        start_time
    };
    crate::logging::log(&format!("TS: set end_time={:.2} line={}", time_pos, line_idx));

    resync_mpv_timestamps(state);

    // Seek to start and resume playback
    let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::ResumeAndSeek(start_time));
    true
}

/// Delete timestamp on current line (BackSpace).
pub fn delete_timestamp(state: &mut AppState) -> bool {
    let media_id = match state.media_id {
        Some(id) => id,
        None => {
            crate::logging::log("TS: delete_timestamp failed: no media_id");
            return false;
        }
    };
    let line_idx = match state.work_line_for_buffer(state.current_line) {
        Some(i) => i,
        None => {
            crate::logging::log(&format!(
                "TS: delete_timestamp failed: no work line for buffer line {}",
                state.current_line
            ));
            return false;
        }
    };

    let Some(line_id) = work_line_id(state, line_idx) else { return false; };

    capture_undo_snapshot(state, line_id, media_id);

    {
        let work = match &mut state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line = &mut work.lines[line_idx];

        // Guard: must have existing timestamp
        if line.timestamp.is_none() {
            crate::logging::log(&format!(
                "TS: delete_timestamp failed: no timestamp on work line {} (buffer line {})",
                line_idx, state.current_line
            ));
            return false;
        }

        let Some(conn) = open_db_rw_or_log() else { return false; };
        if let Err(e) = crate::db::queries::delete_timestamp(&conn, line.id, media_id) {
            crate::logging::log(&format!("TS: delete_timestamp failed: {}", e));
            return false;
        }

        line.timestamp = None;
    }
    crate::logging::log(&format!("TS: deleted timestamp line={}", line_idx));

    resync_mpv_timestamps(state);

    // Update sign column for this line
    let buffer_line = state.current_line;
    set_sign_columns(state, buffer_line, false, false, Some(false));
    redraw_sign_gutters(state);

    true
}

/// Nudge start time by delta seconds (p = -0.2, P = +0.2).
pub fn nudge_start_time(state: &mut AppState, delta: f64) -> bool {
    let media_id = match state.media_id {
        Some(id) => id,
        None => return false,
    };
    let line_idx = match state.work_line_for_buffer(state.current_line) {
        Some(i) => i,
        None => return false,
    };

    let Some(line_id) = work_line_id(state, line_idx) else { return false; };

    capture_undo_snapshot(state, line_id, media_id);

    let new_start = {
        let work = match &mut state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line = &mut work.lines[line_idx];

        // Guard: must have existing timestamp
        let current_start = match &line.timestamp {
            Some(ts) => ts.start,
            None => return false,
        };

        let new_start = (current_start + delta).max(0.0);

        let Some(conn) = open_db_rw_or_log() else { return false; };
        if let Err(e) = crate::db::queries::upsert_start_time(&conn, line.id, media_id, &line.citation, new_start) {
            crate::logging::log(&format!("TS: nudge upsert failed: {}", e));
            return false;
        }

        // Update in-memory
        line.timestamp.as_mut().unwrap().start = new_start;
        new_start
    };
    crate::logging::log(&format!("TS: nudge start_time={:.2} delta={:.1} line={}", new_start, delta, line_idx));

    resync_mpv_timestamps(state);

    // Seek to new position
    let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::Seek(new_start));
    true
}

/// Undo the last timestamp operation (U).
pub fn undo_timestamp(state: &mut AppState) -> bool {
    let undo = match state.timestamp_undo.take() {
        Some(u) => u,
        None => return false,
    };

    let conn = match crate::db::queries::open_db_rw() {
        Ok(c) => c,
        Err(e) => {
            crate::logging::log(&format!("TS: undo open_db_rw failed: {}", e));
            return false;
        }
    };

    match &undo.previous {
        None => {
            // Row didn't exist before — delete it
            if let Err(e) = crate::db::queries::delete_timestamp(&conn, undo.line_mapping_id, undo.media_id) {
                crate::logging::log(&format!("TS: undo delete failed: {}", e));
                return false;
            }
        }
        Some(snap) => {
            // Restore the previous row state
            if let Err(e) = crate::db::queries::restore_timestamp(
                &conn,
                undo.line_mapping_id,
                undo.media_id,
                &snap.citation,
                snap.start_time,
                snap.end_time,
                snap.is_track_mark,
            ) {
                crate::logging::log(&format!("TS: undo restore failed: {}", e));
                return false;
            }
        }
    }

    // `u` (set_start_time) also marks the line is_spoken=1, so undoing it must
    // clear that flag back to 0. Non-fatal on error: the timestamp undo above
    // already succeeded; just log (mirrors set_start_time's treatment).
    if let Err(e) = crate::db::queries::upsert_spoken_status(&conn, undo.line_mapping_id, undo.media_id, false) {
        crate::logging::log(&format!("TS: undo upsert_spoken_status(false) failed: {}", e));
    }

    // Update in-memory state, then extract values for sign column update.
    // Must drop the mutable borrow of current_work before accessing
    // state.line_map, state.has_timestamp, etc.
    let (buffer_line, has_ts, is_man, is_tm) = {
        let work = match &mut state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line = match work.lines.iter_mut().find(|l| l.id == undo.line_mapping_id) {
            Some(l) => l,
            None => return false,
        };

        // Clear the spoken flag set by `u` (see DB clear above).
        line.is_spoken = Some(false);

        match &undo.previous {
            None => {
                line.timestamp = None;
            }
            Some(snap) => {
                match (snap.start_time, snap.end_time) {
                    (Some(start), end) => {
                        line.timestamp = Some(TimeRange {
                            start,
                            end: end.unwrap_or(0.0),
                            sentence_start: None,
                            is_manual: true,
                        });
                    }
                    (None, _) => {
                        line.timestamp = None;
                    }
                }
            }
        }

        let has_ts = line.timestamp.is_some();
        let is_man = line.timestamp.as_ref().map_or(false, |t| t.is_manual);
        // Log-only: the structural is_chapter sign follows divisions, so the
        // sign update below passes None — this value is not written to any sign.
        let is_tm = match &undo.previous {
            Some(snap) => snap.is_track_mark,
            None => false,
        };
        let work_idx = work.lines.iter().position(|l| l.id == undo.line_mapping_id);
        (work_idx, has_ts, is_man, is_tm)
    };
    // buffer_line here is the work_idx; resolve to actual buffer line
    let buffer_line = match buffer_line {
        Some(idx) => {
            if let Some(ref lm) = state.line_map {
                lm.work_to_buffer.get(idx).copied()
            } else {
                Some(idx)
            }
        }
        None => None,
    };

    crate::logging::log(&format!(
        "TS: undo line_mapping_id={} restored={}", undo.line_mapping_id, undo.previous.is_some()
    ));

    resync_mpv_timestamps(state);

    crate::logging::log(&format!("TS: undo is_track_mark={}", is_tm));

    // Update sign column. Undo of a track-mark must NOT touch the structural
    // is_chapter sign (that follows divisions now) — pass None.
    if let Some(bl) = buffer_line {
        set_sign_columns(state, bl, has_ts, is_man, None);
    }

    redraw_sign_gutters(state);

    true
}

/// Nudge start backward by 0.2s.
pub fn nudge_start_backward(state: &mut AppState) -> bool {
    nudge_start_time(state, -NUDGE_STEP)
}

/// Nudge start forward by 0.2s.
pub fn nudge_start_forward(state: &mut AppState) -> bool {
    nudge_start_time(state, NUDGE_STEP)
}
