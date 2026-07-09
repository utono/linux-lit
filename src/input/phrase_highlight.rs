//! Karaoke-style narration highlight (phrase- or line/sentence-width) during
//! MPV narration sync.
//!
//! Driven from the `MpvEvent::TimePos` handler at **raw** playback time (no
//! SYNC_PREROLL — the sync cursor leads the narration by the preroll, so the
//! spoken line is resolved independently near the cursor). Spans come from
//! `phrase_timestamps` via `queries::phrase_spans_for_line`, cached per
//! (line_mapping_id, media_id) in `AppState.phrase_cache`.
//! In LINE mode the tint widens to the whole buffer line (verse) or the
//! containing sentence (prose) via tint_range/sentence_bounds; the span
//! resolution and caching are identical in every mode.

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

/// Words whose trailing '.' does not end a sentence (titles/abbreviations
/// common in 19th-century prose — Bleak House is full of them). Matched
/// case-sensitively against the word immediately before the '.'.
const ABBREVIATIONS: &[&str] = &[
    "Mr", "Mrs", "Ms", "Dr", "St", "Prof", "Rev", "Hon", "Capt", "Col",
    "Gen", "Lieut", "Sgt", "Esq", "Jr", "Sr", "vol", "chap", "etc", "viz",
    "cf", "vs",
];

/// Closing punctuation that may trail a sentence terminator and still belong
/// to the sentence (curly + straight quotes, brackets).
const CLOSERS: &[char] = &['\u{2019}', '\u{201D}', '\'', '"', ')', ']'];

/// True when `chars[k]` ends a sentence: it is `.`/`!`/`?`, the next char
/// (skipping CLOSERS) is whitespace or end-of-text (rejects decimals like
/// "3.5" and mid-word dots), and for '.' the word before it is neither a
/// known abbreviation nor a single uppercase initial ("Mr. J. Smith").
fn is_sentence_end(chars: &[char], k: usize) -> bool {
    let c = chars[k];
    if c != '.' && c != '!' && c != '?' {
        return false;
    }
    let mut j = k + 1;
    while j < chars.len() && CLOSERS.contains(&chars[j]) {
        j += 1;
    }
    if j < chars.len() && !chars[j].is_whitespace() {
        return false;
    }
    if c == '.' {
        let mut w = k;
        while w > 0 && chars[w - 1].is_alphabetic() {
            w -= 1;
        }
        let word: String = chars[w..k].iter().collect();
        let mut cs = word.chars();
        if let (Some(first), None) = (cs.next(), cs.next()) {
            if first.is_uppercase() {
                return false; // single initial, e.g. "J."
            }
        }
        if ABBREVIATIONS.contains(&word.as_str()) {
            return false;
        }
    }
    true
}

/// `[start, end)` unicode-char range of the sentence(s) containing the span
/// `[start_char, end_char)`. A span crossing a sentence boundary extends over
/// BOTH sentences (backward from the span start, forward from the span end).
/// Out-of-range offsets clamp; mis-detection yields a wrong-width tint, never
/// a panic (apply_phrase_tag clamps again downstream).
pub fn sentence_bounds(text: &str, start_char: usize, end_char: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let sc = start_char.min(n);
    let ec = end_char.min(n).max(sc);
    // Backward: the sentence starts after the previous sentence's terminator
    // (+ trailing closers + whitespace); at 0 when there is none.
    let mut start = 0;
    for k in (0..sc).rev() {
        if is_sentence_end(&chars, k) {
            let mut j = k + 1;
            while j < n && CLOSERS.contains(&chars[j]) {
                j += 1;
            }
            while j < n && chars[j].is_whitespace() {
                j += 1;
            }
            start = j.min(sc);
            break;
        }
    }
    // Forward: through the next terminator (+ trailing closers); n when there
    // is none. Starts at ec-1 so a span already ending ON the terminator
    // doesn't extend into the following sentence.
    let mut end = n;
    for k in ec.saturating_sub(1)..n {
        if is_sentence_end(&chars, k) {
            let mut j = k + 1;
            while j < n && CLOSERS.contains(&chars[j]) {
                j += 1;
            }
            end = j;
            break;
        }
    }
    (start, end.max(ec))
}

use crate::app::AppState;
use crate::config::PhraseHighlightMode;
use gtk4::prelude::*;

/// Char range to tag for the active span: the span itself in Phrase mode;
/// in Line mode the whole buffer line (verse) or the containing sentence
/// (prose). Off never reaches here (gated in update_phrase_highlight).
pub fn tint_range(
    mode: PhraseHighlightMode,
    is_prose: bool,
    line_text: &str,
    span: PhraseSpan,
) -> (usize, usize) {
    match mode {
        PhraseHighlightMode::Line if is_prose => {
            sentence_bounds(line_text, span.start_char, span.end_char)
        }
        PhraseHighlightMode::Line => (0, line_text.chars().count()),
        _ => (span.start_char, span.end_char),
    }
}

/// Text of buffer line `bl` (no trailing newline). Empty when out of range.
fn buffer_line_text(s: &AppState, bl: usize) -> String {
    let buffer = &s.buffer;
    let Some(start) = buffer.iter_at_line(bl as i32) else {
        return String::new();
    };
    let mut end = start;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    buffer.text(&start, &end, false).to_string()
}

/// The karaoke mode for the current work's class (prose vs verse flag).
fn active_mode(s: &AppState) -> PhraseHighlightMode {
    if s.is_prose() {
        s.config.phrase_highlight_prose
    } else {
        s.config.phrase_highlight_verse
    }
}

/// Per-TimePos driver. Gates: class flag (prose vs verse), sync on,
/// translations hidden (inflated buffer misaligns offsets). While paused or
/// mid-load the tint is KEPT as-is — it marks where the audio stopped, or the
/// pending phrase painted at startup / by a seek keybind. During sync
/// suppression (manual seeks/nav) the tint clears UNLESS a pending-phrase
/// paint is holding it (do_mpv_seek paints the seek target and holds it
/// through its own suppression window).
pub fn update_phrase_highlight(s: &mut AppState, pos: f64) {
    let mode = active_mode(s);
    if !mode.is_on() || !s.sync_enabled || s.translations_visible {
        clear_phrase_highlight(s);
        return;
    }
    if !s.mpv_playing || s.loading_work.get() {
        return;
    }
    let suppressed = s
        .suppress_sync_until
        .map(|until| std::time::Instant::now() < until)
        .unwrap_or(false);
    if suppressed {
        let held = s
            .phrase_paint_hold
            .map(|until| std::time::Instant::now() < until)
            .unwrap_or(false);
        if !held {
            clear_phrase_highlight(s);
        }
        return;
    }
    paint_phrase_at(s, pos, false);
}

/// Paint the phrase at an arbitrary timecode (the time a seek keybind just
/// sent to MPV, or the resume line's start time at startup) so the karaoke
/// tint shows what WILL be narrated there, without waiting for live TimePos
/// ticks. Returns true when the mode gates passed and the paint path ran, so
/// the caller can hold the tint through its sync-suppression window.
pub fn paint_pending_phrase(s: &mut AppState, pos: f64) -> bool {
    let mode = active_mode(s);
    if !mode.is_on() || !s.sync_enabled || s.translations_visible {
        return false;
    }
    paint_phrase_at(s, pos, true);
    true
}

/// Startup / work-load: tint the phrase that will begin to play — the phrase
/// at the cursor (resume) line's start time — so the resume point is visible
/// before playback starts. No-op while playing (live sync owns the tint),
/// or when the line is untimestamped / the work has no phrase data.
pub fn show_startup_phrase(s: &mut AppState) {
    if s.mpv_playing {
        return;
    }
    let Some(start) = s.work_line_for_buffer(s.current_line).and_then(|wi| {
        s.current_work.as_ref()?.lines.get(wi)?.timestamp.as_ref().map(|t| t.start)
    }) else {
        return;
    };
    paint_pending_phrase(s, start);
}

/// Resolve the spoken line near the sync cursor and tint the phrase active at
/// raw `pos`. Clears the tint when no line/span matches (pos outside phrase
/// data). `snap_forward` widens the miss case for pending paints: a `pos`
/// BEFORE the resolved line's first span tints that first span (the phrase
/// that will play) instead of clearing.
fn paint_phrase_at(s: &mut AppState, pos: f64, snap_forward: bool) {
    let mode = active_mode(s);
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
    let hit = s.phrase_cache.as_ref().and_then(|c| {
        phrase_at_time(&c.spans, pos)
            .or_else(|| if snap_forward && !c.spans.is_empty() { Some(0) } else { None })
            .map(|i| (c.spans[i], i))
    });
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
    let line_text = buffer_line_text(s, bl);
    let (sc, ec) = tint_range(mode, s.is_prose(), &line_text, span);
    apply_phrase_tag(s, bl, sc, ec);
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

    #[test]
    fn sentence_bounds_first_mid_last_sentence() {
        let text = "First sentence here. Second one is longer. Third ends.";
        // Span inside the middle sentence ("one").
        assert_eq!(sentence_bounds(text, 28, 31), (21, 42));
        // Span in the first sentence.
        assert_eq!(sentence_bounds(text, 0, 5), (0, 20));
        // Span in the last sentence (no trailing terminator scan overrun).
        assert_eq!(sentence_bounds(text, 43, 48), (43, 54));
    }

    #[test]
    fn sentence_bounds_span_crossing_boundary_covers_both() {
        let text = "First sentence here. Second one is longer. Third ends.";
        // Span from inside sentence 1 into sentence 2 -> both tinted.
        assert_eq!(sentence_bounds(text, 15, 25), (0, 42));
    }

    #[test]
    fn sentence_bounds_abbreviation_and_initial_guard() {
        let text = "Mr. Tulkinghorn arrives. He waits.";
        // "Mr." must not end the sentence; "arrives." does.
        assert_eq!(sentence_bounds(text, 16, 23), (0, 24));
        assert_eq!(sentence_bounds(text, 25, 27), (25, 34));
        // Single-letter initials ("J.") must not end the sentence either.
        let text2 = "Mr. J. Smith spoke. So did I.";
        assert_eq!(sentence_bounds(text2, 7, 12), (0, 19));
    }

    #[test]
    fn sentence_bounds_closing_quotes_and_decimals() {
        // Terminator followed by a closing quote: quote belongs to the sentence.
        let text = "\u{201C}Stop!\u{201D} Then left.";
        assert_eq!(sentence_bounds(text, 1, 5), (0, 7));
        assert_eq!(sentence_bounds(text, 8, 12), (8, 18));
        // A decimal point is not a sentence end.
        let text2 = "It cost 3.5 pounds. Yes.";
        assert_eq!(sentence_bounds(text2, 12, 18), (0, 19));
    }

    #[test]
    fn sentence_bounds_clamps_out_of_range_span() {
        // Offsets beyond the text clamp instead of panicking (data drift guard).
        assert_eq!(sentence_bounds("Hi.", 10, 20), (3, 3));
    }

    #[test]
    fn tint_range_by_mode_and_class() {
        use crate::config::PhraseHighlightMode::{Line, Phrase};
        let sp = span(10.0, 11.0, 4, 7); // "two" in the text below
        let text = "One two. Three four.";
        // Phrase mode: exactly the span, both classes.
        assert_eq!(tint_range(Phrase, true, text, sp), (4, 7));
        assert_eq!(tint_range(Phrase, false, text, sp), (4, 7));
        // Line mode, verse: the whole line.
        assert_eq!(tint_range(Line, false, text, sp), (0, 20));
        // Line mode, prose: the sentence containing the span.
        assert_eq!(tint_range(Line, true, text, sp), (0, 8));
    }
}
