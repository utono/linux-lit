use gtk4::prelude::*;

use crate::app::AppState;
use crate::db::models::TimeRange;

#[derive(Debug, Clone)]
pub struct TimestampSnapshot {
    pub citation: String,
    pub start_time: Option<f64>,
    pub end_time: Option<f64>,
    pub is_chapter: bool,
}

#[derive(Debug, Clone)]
pub struct TimestampUndoState {
    pub line_mapping_id: i64,
    pub media_id: i64,
    /// None = row didn't exist before the operation (undo → DELETE)
    pub previous: Option<TimestampSnapshot>,
}

const NUDGE_STEP: f64 = 0.2;

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

    let line_id = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        work.lines[line_idx].id
    };

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

        let conn = match crate::db::queries::open_db_rw() {
            Ok(c) => c,
            Err(e) => {
                crate::logging::log(&format!("TS: open_db_rw failed: {}", e));
                return false;
            }
        };
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

    // Update sign column for this line
    let buffer_line = state.current_line;
    {
        let mut ht = state.has_timestamp.borrow_mut();
        if buffer_line < ht.len() {
            ht[buffer_line] = true;
        }
        let mut manual = state.is_manual.borrow_mut();
        if buffer_line < manual.len() {
            manual[buffer_line] = true;
        }
    }
    if let Some(ref renderer) = state.gutter_renderer {
        renderer.queue_draw();
    }

    crate::input::navigation::cursor_next_dialogue(state);

    true
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
    let seek_time = (start - crate::input::navigation::SEEK_PREROLL).max(0.0);
    let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::ResumeAndSeek(seek_time));
    // Suppress cursor sync so cursor stays on this line
    state.suppress_sync_until = Some(
        std::time::Instant::now() + std::time::Duration::from_millis(500),
    );
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

    // If not already a chapter, check for nearby chapters within ±10 seconds
    {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line = &work.lines[line_idx];
        if !line.is_chapter {
            let nearby = work.lines.iter().enumerate().any(|(i, other)| {
                i != line_idx
                    && other.is_chapter
                    && other.timestamp.as_ref().map_or(false, |ts| {
                        (ts.start - time_pos).abs() <= 10.0
                    })
            });
            if nearby {
                crate::logging::log(&format!(
                    "TS: chapter rejected — another chapter within 10s of {:.2}",
                    time_pos,
                ));
                return false;
            }
        }
    }

    let line_id = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        work.lines[line_idx].id
    };

    capture_undo_snapshot(state, line_id, media_id);

    {
        let work = match &mut state.current_work {
            Some(w) => w,
            None => return false,
        };
        let line = &mut work.lines[line_idx];

        let conn = match crate::db::queries::open_db_rw() {
            Ok(c) => c,
            Err(e) => {
                crate::logging::log(&format!("TS: open_db_rw failed: {}", e));
                return false;
            }
        };
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
        line.is_chapter = new_val;
    }
    let is_ch = state.current_work.as_ref().map(|w| w.lines[line_idx].is_chapter).unwrap_or(false);
    crate::logging::log(&format!("TS: toggle chapter is_chapter={} start_time={:.2} line={}", is_ch, time_pos, line_idx));

    resync_mpv_timestamps(state);

    // Update sign column for this line
    let buffer_line = state.current_line;
    {
        let mut ht = state.has_timestamp.borrow_mut();
        if buffer_line < ht.len() {
            ht[buffer_line] = true;
        }
        let mut manual = state.is_manual.borrow_mut();
        if buffer_line < manual.len() {
            manual[buffer_line] = true;
        }
    }
    {
        let mut ch = state.is_chapter_line.borrow_mut();
        if buffer_line < ch.len() {
            ch[buffer_line] = is_ch;
        }
    }
    if let Some(ref renderer) = state.gutter_renderer {
        renderer.queue_draw();
    }

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

    let line_id = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        work.lines[line_idx].id
    };

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

        let conn = match crate::db::queries::open_db_rw() {
            Ok(c) => c,
            Err(e) => {
                crate::logging::log(&format!("TS: open_db_rw failed: {}", e));
                return false;
            }
        };
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

    let line_id = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        work.lines[line_idx].id
    };

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

        let conn = match crate::db::queries::open_db_rw() {
            Ok(c) => c,
            Err(e) => {
                crate::logging::log(&format!("TS: open_db_rw failed: {}", e));
                return false;
            }
        };
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
    {
        let mut ht = state.has_timestamp.borrow_mut();
        if buffer_line < ht.len() {
            ht[buffer_line] = false;
        }
        let mut manual = state.is_manual.borrow_mut();
        if buffer_line < manual.len() {
            manual[buffer_line] = false;
        }
    }
    {
        let mut ch = state.is_chapter_line.borrow_mut();
        if buffer_line < ch.len() {
            ch[buffer_line] = false;
        }
    }
    if let Some(ref renderer) = state.gutter_renderer {
        renderer.queue_draw();
    }

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

    let line_id = {
        let work = match &state.current_work {
            Some(w) => w,
            None => return false,
        };
        work.lines[line_idx].id
    };

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

        let conn = match crate::db::queries::open_db_rw() {
            Ok(c) => c,
            Err(e) => {
                crate::logging::log(&format!("TS: open_db_rw failed: {}", e));
                return false;
            }
        };
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
                snap.is_chapter,
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
    let (buffer_line, has_ts, is_man, is_ch) = {
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
                line.is_chapter = false;
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
                line.is_chapter = snap.is_chapter;
            }
        }

        let has_ts = line.timestamp.is_some();
        let is_man = line.timestamp.as_ref().map_or(false, |t| t.is_manual);
        let is_ch = line.is_chapter;
        let work_idx = work.lines.iter().position(|l| l.id == undo.line_mapping_id);
        (work_idx, has_ts, is_man, is_ch)
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

    // Update sign column
    if let Some(bl) = buffer_line {
        {
            let mut ht = state.has_timestamp.borrow_mut();
            if bl < ht.len() {
                ht[bl] = has_ts;
            }
            let mut manual = state.is_manual.borrow_mut();
            if bl < manual.len() {
                manual[bl] = is_man;
            }
        }
        {
            let mut ch = state.is_chapter_line.borrow_mut();
            if bl < ch.len() {
                ch[bl] = is_ch;
            }
        }
    }

    if let Some(ref renderer) = state.gutter_renderer {
        renderer.queue_draw();
    }

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
