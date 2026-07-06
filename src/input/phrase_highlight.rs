//! Karaoke-style spoken-phrase highlight during MPV narration sync.
//!
//! Driven from the `MpvEvent::TimePos` handler at **raw** playback time (no
//! SYNC_PREROLL — the sync cursor leads the narration by the preroll, so the
//! spoken line is resolved independently near the cursor). Spans come from
//! `phrase_timestamps` via `queries::phrase_spans_for_line`, cached per
//! (line_mapping_id, media_id) in `AppState.phrase_cache`.

use crate::db::queries::PhraseSpan;

/// Cached phrase spans for the (line, media) currently being narrated. An
/// EMPTY `spans` vec is a valid negative result (work/paragraph without
/// phrase data) — kept so we don't re-query every TimePos tick.
pub struct PhraseCache {
    pub line_mapping_id: i64,
    pub media_id: i64,
    pub spans: Vec<PhraseSpan>,
}

/// How many work lines around the sync cursor to scan when resolving which
/// line is actually being spoken at raw time. The cursor leads by at most one
/// line (SYNC_PREROLL) in normal sync; 8 tolerates gap-jumps and stale cursors.
const SPOKEN_LINE_WALK: usize = 8;

/// Index of the phrase active at `pos`: the LAST span whose start_time <= pos.
/// Holds through inter-phrase gaps and past the final span's end (no flicker;
/// the next paragraph's spans take over once the spoken line advances).
/// None before the first span starts.
pub fn phrase_at_time(spans: &[PhraseSpan], pos: f64) -> Option<usize> {
    if spans.is_empty() || pos < spans[0].start_time {
        return None;
    }
    let n = spans.partition_point(|sp| sp.start_time <= pos);
    Some(n - 1)
}

/// Resolve which work line is being SPOKEN at raw time `pos`, scanning a
/// bounded window around the sync cursor's work line. Returns the last
/// timestamped line in the window whose start <= pos (timestamps are
/// monotonic, so the scan breaks at the first future line). None when the
/// narration is behind every timestamped line in the window.
pub fn resolve_spoken_idx(
    ts_of: impl Fn(usize) -> Option<(f64, f64)>,
    len: usize,
    cursor_wi: usize,
    pos: f64,
) -> Option<usize> {
    let lo = cursor_wi.saturating_sub(SPOKEN_LINE_WALK);
    let hi = (cursor_wi + SPOKEN_LINE_WALK + 1).min(len);
    let mut best = None;
    for i in lo..hi {
        if let Some((start, _end)) = ts_of(i) {
            if start <= pos {
                best = Some(i);
            } else {
                break;
            }
        }
    }
    best
}

use crate::app::AppState;
use gtk4::prelude::*;

/// Per-TimePos driver. Gates: class flag (prose vs verse), sync on, not
/// loading, translations hidden (inflated buffer misaligns offsets), not
/// sync-suppressed (manual seeks/nav clear the tint). Pause KEEPS the last
/// phrase visible — it marks where the audio stopped.
pub fn update_phrase_highlight(s: &mut AppState, pos: f64) {
    let enabled = if s.is_prose() {
        s.config.phrase_highlight_prose
    } else {
        s.config.phrase_highlight_verse
    };
    let suppressed = s
        .suppress_sync_until
        .map(|until| std::time::Instant::now() < until)
        .unwrap_or(false);
    if !enabled || !s.sync_enabled || s.loading_work.get() || s.translations_visible || suppressed
    {
        clear_phrase_highlight(s);
        return;
    }
    if !s.mpv_playing {
        return;
    }
    let Some(media) = s.media_id else {
        clear_phrase_highlight(s);
        return;
    };
    let Some(cursor_wi) = s.work_line_for_buffer(s.current_line) else {
        return;
    };
    // The sync cursor leads by SYNC_PREROLL, so resolve the line actually
    // being spoken at raw `pos` in a bounded window around the cursor.
    let spoken = {
        let Some(work) = s.current_work.as_ref() else { return };
        let lines = &work.lines;
        resolve_spoken_idx(
            |i| lines.get(i).and_then(|l| l.timestamp.as_ref()).map(|t| (t.start, t.end)),
            lines.len(),
            cursor_wi,
            pos,
        )
        .map(|wi| (wi, lines[wi].id))
    };
    let Some((spoken_wi, line_id)) = spoken else {
        clear_phrase_highlight(s);
        return;
    };
    let cache_stale = s
        .phrase_cache
        .as_ref()
        .map(|c| c.line_mapping_id != line_id || c.media_id != media)
        .unwrap_or(true);
    if cache_stale {
        let spans = crate::db::queries::open_db()
            .map(|conn| crate::db::queries::phrase_spans_for_line(&conn, line_id, media))
            .unwrap_or_default();
        crate::logging::log(&format!(
            "PHRASE_HL: cache fill line_id={} media={} spans={}",
            line_id,
            media,
            spans.len()
        ));
        s.phrase_cache = Some(PhraseCache { line_mapping_id: line_id, media_id: media, spans });
    }
    let hit = s
        .phrase_cache
        .as_ref()
        .and_then(|c| phrase_at_time(&c.spans, pos).map(|i| (c.spans[i], i)));
    let Some((span, span_idx)) = hit else {
        clear_phrase_highlight(s);
        return;
    };
    let Some(bl) = s.buffer_line_for_work(spoken_wi) else {
        return;
    };
    if s.active_phrase == Some((bl, span_idx)) {
        return;
    }
    apply_phrase_tag(s, bl, span.start_char, span.end_char);
    s.active_phrase = Some((bl, span_idx));
}

/// Remove the phrase tint everywhere. Cheap no-op when nothing is applied.
pub fn clear_phrase_highlight(s: &mut AppState) {
    if s.active_phrase.is_none() {
        return;
    }
    let (bs, be) = s.buffer.bounds();
    s.buffer.remove_tag(&s.phrase_tag, &bs, &be);
    s.active_phrase = None;
}

/// Move the tag to `[start_char, end_char)` of buffer line `bl`, clamped to
/// the line's char count (GTK iter offsets are unicode chars, matching the
/// Python backfill's str indices; clamping guards data drift).
fn apply_phrase_tag(s: &AppState, bl: usize, start_char: usize, end_char: usize) {
    let buffer = &s.buffer;
    let (bs, be) = buffer.bounds();
    buffer.remove_tag(&s.phrase_tag, &bs, &be);
    let Some(line_start) = buffer.iter_at_line(bl as i32) else {
        return;
    };
    let line_chars = {
        let mut e = line_start;
        if !e.ends_line() {
            e.forward_to_line_end();
        }
        e.line_offset().max(0) as usize
    };
    let sc = start_char.min(line_chars);
    let ec = end_char.min(line_chars).max(sc);
    if ec == sc {
        return;
    }
    let mut a = line_start;
    a.set_line_offset(sc as i32);
    let mut b = line_start;
    b.set_line_offset(ec as i32);
    buffer.apply_tag(&s.phrase_tag, &a, &b);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start_time: f64, end_time: f64, start_char: usize, end_char: usize) -> PhraseSpan {
        PhraseSpan { start_time, end_time, start_char, end_char }
    }

    #[test]
    fn phrase_at_time_basic_gap_hold_and_edges() {
        let spans = vec![
            span(10.0, 11.8, 0, 20),
            span(12.0, 13.5, 20, 40),
            span(15.0, 17.0, 40, 60),
        ];
        assert_eq!(phrase_at_time(&spans, 9.9), None); // before first
        assert_eq!(phrase_at_time(&spans, 10.0), Some(0)); // exact start
        assert_eq!(phrase_at_time(&spans, 11.0), Some(0)); // inside
        assert_eq!(phrase_at_time(&spans, 11.9), Some(0)); // gap: hold prev
        assert_eq!(phrase_at_time(&spans, 14.0), Some(1)); // gap: hold prev
        assert_eq!(phrase_at_time(&spans, 16.0), Some(2));
        assert_eq!(phrase_at_time(&spans, 99.0), Some(2)); // past end: hold last
        assert_eq!(phrase_at_time(&[], 5.0), None); // empty
    }

    #[test]
    fn resolve_spoken_idx_walks_near_cursor() {
        // Lines 0..6; lines 2 and 5 untimestamped (e.g. chapter headings).
        let ts = [
            Some((0.0, 4.0)),
            Some((5.0, 9.0)),
            None,
            Some((10.0, 14.0)),
            Some((15.0, 19.0)),
            None,
            Some((20.0, 24.0)),
        ];
        let f = |i: usize| ts.get(i).copied().flatten();
        // Cursor leads (preroll): cursor on 4, narration still on 3.
        assert_eq!(resolve_spoken_idx(f, ts.len(), 4, 12.0), Some(3));
        // Cursor in step: pos inside cursor line's window.
        assert_eq!(resolve_spoken_idx(f, ts.len(), 3, 12.0), Some(3));
        // Cursor lags: narration moved ahead.
        assert_eq!(resolve_spoken_idx(f, ts.len(), 3, 21.0), Some(6));
        // Inter-line gap: pos between line 3 end and line 4 start -> hold 3.
        assert_eq!(resolve_spoken_idx(f, ts.len(), 4, 14.5), Some(3));
        // Before everything in window -> None.
        assert_eq!(resolve_spoken_idx(f, ts.len(), 0, -1.0), None);
        // Empty work.
        assert_eq!(resolve_spoken_idx(f, 0, 0, 12.0), None);
    }
}
