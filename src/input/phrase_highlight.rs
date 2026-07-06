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
