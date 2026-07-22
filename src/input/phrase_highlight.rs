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
pub(crate) fn buffer_line_text(s: &AppState, bl: usize) -> String {
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
/// The vocab-sentence loop suppresses the phrase sweep entirely — the static
/// sentence tint alone marks the drilled sentence — whatever the class's
/// configured mode; restored implicitly when the mode exits.
fn active_mode(s: &AppState) -> PhraseHighlightMode {
    if s.vocab_loop.is_some() {
        return PhraseHighlightMode::Off;
    }
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
    // NOT gated on `sync_enabled`: the karaoke tint tracks the audio position
    // itself, so it must keep sweeping even when playback SYNC (the cursor/page
    // following the audio) is toggled off. It IS gated on the class mode being
    // on and on translations being hidden (the inflated translation buffer
    // misaligns character offsets).
    if !mode.is_on() || s.translations_visible {
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
    // Not gated on `sync_enabled` (see update_phrase_highlight) — a seek-driven
    // pending paint should show the karaoke tint at the target even with sync off.
    if !mode.is_on() || s.translations_visible {
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
        // HOLD-ON-TRANSIENT-MISS: on prose, a long paragraph straddling several
        // pages advances the page window but NOT `current_line`, so the spoken
        // line can drift outside `resolve_spoken_idx`'s bounded walk window and
        // return None for a stretch of ticks — the karaoke tint used to blank
        // and stay blank until things re-converged ("sometimes disappears on
        // TT"). Instead, KEEP the last-painted phrase when one is up (a fresh
        // resolve will replace it). Clear only when nothing is painted, so a
        // genuinely stopped/off-data state still shows nothing. Mirrors how
        // `phrase_at_time` already holds a span through inter-span gaps.
        if s.active_phrase.is_none() {
            clear_phrase_highlight(s);
        }
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
        // HOLD-ON-TRANSIENT-MISS (same rationale as the resolve-miss above): the
        // resolved line has no span active at `pos` — a partial/gappy timestamp
        // stretch. Keep the last-painted phrase rather than blanking mid-sweep;
        // a later tick with a real span replaces it. Clear only when nothing is
        // painted.
        if s.active_phrase.is_none() {
            clear_phrase_highlight(s);
        }
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

/// Seconds past a phrase's start within which `o` steps to the PREVIOUS
/// phrase instead of restarting the current one (the music-player back-button
/// hybrid: far into the phrase = replay it; near its start = go back one).
const PHRASE_RESTART_GRACE: f64 = 1.0;

/// Two spans whose start times differ by less than this are the SAME moment:
/// a spoken run crossing a verse line break stamps its whole window on every
/// covered span (both lines), so a "step" onto such a tied span is zero
/// motion. Step targets must beat the current start by at least this margin.
const PHRASE_TIE_EPS: f64 = 0.05;

/// Where an o/e phrase step should land, given the current line's spans.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PhraseStep {
    /// Seek to this time within the current line's spans.
    Within(f64),
    /// Step crosses into the neighboring timestamped line (true = next).
    CrossLine(bool),
}

/// Pure step-target selection for the phrase-aware o/e seeks. `idx` is the
/// currently highlighted span, `pos` the raw playback time. Forward: the next
/// span, else cross to the next line. Backward (hybrid): restart the current
/// span when more than `PHRASE_RESTART_GRACE` past its start, else the
/// previous span, else cross to the previous line.
pub(crate) fn phrase_step_target(
    spans: &[PhraseSpan],
    idx: usize,
    pos: f64,
    forward: bool,
) -> PhraseStep {
    let cur = spans[idx].start_time;
    if forward {
        match (idx + 1..spans.len()).find(|&j| spans[j].start_time > cur + PHRASE_TIE_EPS) {
            Some(j) => PhraseStep::Within(spans[j].start_time),
            None => PhraseStep::CrossLine(true),
        }
    } else if pos > cur + PHRASE_RESTART_GRACE {
        PhraseStep::Within(cur)
    } else {
        match (0..idx).rev().find(|&j| spans[j].start_time < cur - PHRASE_TIE_EPS) {
            Some(j) => PhraseStep::Within(spans[j].start_time),
            None => PhraseStep::CrossLine(false),
        }
    }
}

/// Step target on a NEIGHBOR line's spans: forward the first span starting
/// strictly after `cur_ref` (the current span's start), backward the last
/// span starting strictly before it; a spanless line falls back to its
/// line-timestamp start under the same strict rule. None = this line cannot
/// move playback (its spans share the current run's window) — the caller
/// tries the next candidate line.
pub(crate) fn cross_line_pick(
    nspans: &[PhraseSpan],
    line_start: Option<f64>,
    cur_ref: f64,
    fwd: bool,
) -> Option<f64> {
    let qualifies = |t: f64| {
        if fwd {
            t > cur_ref + PHRASE_TIE_EPS
        } else {
            t < cur_ref - PHRASE_TIE_EPS
        }
    };
    if nspans.is_empty() {
        return line_start.filter(|&t| qualifies(t));
    }
    if fwd {
        nspans.iter().map(|sp| sp.start_time).find(|&t| qualifies(t))
    } else {
        nspans.iter().rev().map(|sp| sp.start_time).find(|&t| qualifies(t))
    }
}

/// Phrase-aware o/e seek: when playback sync + karaoke highlighting are on
/// (and the line has phrase data), `e` seeks to the start time of the first
/// word of the NEXT phrase and `o` restarts the current phrase — or, within
/// `PHRASE_RESTART_GRACE` of its start, steps to the PREVIOUS phrase. Steps
/// crossing a line boundary move the cursor too, and when the target line is
/// off the current page the page turns to contain it (the o/e page-turn
/// to-do). Returns false when any gate fails so the caller falls back to the
/// raw ±seconds seek.
pub fn phrase_step_seek(s: &mut AppState, forward: bool) -> bool {
    let mode = active_mode(s);
    // This one KEEPS the `sync_enabled` gate: `o`/`e` is a phrase-aware SEEK
    // (it moves playback), not the passive karaoke display. With sync off the
    // caller falls back to a raw ±seconds seek. Only the display painters
    // (update_phrase_highlight / paint_pending_phrase) dropped the sync gate.
    if !mode.is_on() || !s.sync_enabled || s.translations_visible {
        return false;
    }
    let Some(media) = s.media_id else { return false };
    let pos = s.current_time_pos;
    let Some(cursor_wi) = s.work_line_for_buffer(s.current_line) else {
        return false;
    };
    let spoken_wi = {
        let Some(work) = s.current_work.as_ref() else { return false };
        let lines = &work.lines;
        resolve_spoken_idx(
            |i| lines.get(i).and_then(|l| l.timestamp.as_ref()).map(|t| (t.start, t.end)),
            lines.len(),
            cursor_wi,
            pos,
        )
    };
    let Some(spoken_wi) = spoken_wi else { return false };
    let line_id = match s.current_work.as_ref().and_then(|w| w.lines.get(spoken_wi)) {
        Some(l) => l.id,
        None => return false,
    };
    // Ensure the cache holds the spoken line's spans (same fill paint uses).
    let cache_stale = s
        .phrase_cache
        .as_ref()
        .map(|c| c.line_mapping_id != line_id || c.media_id != media)
        .unwrap_or(true);
    if cache_stale {
        let spans = crate::db::queries::open_db()
            .map(|conn| crate::db::queries::phrase_spans_for_line(&conn, line_id, media))
            .unwrap_or_default();
        s.phrase_cache = Some(PhraseCache { line_mapping_id: line_id, media_id: media, spans });
    }
    let spans = match s.phrase_cache.as_ref() {
        Some(c) if !c.spans.is_empty() => c.spans.clone(),
        // No phrase data on this line/media: raw seek fallback.
        _ => return false,
    };
    let step = match phrase_at_time(&spans, pos) {
        Some(idx) => phrase_step_target(&spans, idx, pos, forward),
        // Before the line's first span: forward = that first span;
        // backward = the previous line's last phrase.
        None if forward => PhraseStep::Within(spans[0].start_time),
        None => PhraseStep::CrossLine(false),
    };
    let (target_time, target_wi) = match step {
        PhraseStep::Within(t) => (t, spoken_wi),
        PhraseStep::CrossLine(fwd) => {
            let Some(work) = s.current_work.as_ref() else { return false };
            // The step must actually MOVE playback. A spoken run crossing a
            // verse line break stamps its whole window on both lines, so the
            // immediate neighbor's spans can share the current span's start —
            // stepping there seeks backward-in-place (MM-Arkangel 2026-07-22,
            // e pinned at 897.95s). Scan neighbors until one yields a span
            // strictly past the current span's start (cross_line_pick).
            let cur_ref = phrase_at_time(&spans, pos)
                .map(|i| spans[i].start_time)
                .unwrap_or(spans[0].start_time);
            let candidates: Box<dyn Iterator<Item = usize>> = if fwd {
                Box::new(spoken_wi + 1..work.lines.len())
            } else {
                Box::new((0..spoken_wi).rev())
            };
            let mut picked = None;
            for nwi in candidates {
                let line = &work.lines[nwi];
                if line.timestamp.is_none() {
                    continue;
                }
                let line_start = line.timestamp.as_ref().map(|t| t.start);
                let nspans = crate::db::queries::open_db()
                    .map(|conn| {
                        crate::db::queries::phrase_spans_for_line(&conn, line.id, media)
                    })
                    .unwrap_or_default();
                if let Some(t) = cross_line_pick(&nspans, line_start, cur_ref, fwd) {
                    picked = Some((t, nwi));
                    break;
                }
            }
            match picked {
                Some(x) => x,
                // Document edge, forward: no next phrase to step to — return
                // false so the caller falls through to the raw +3.5s seek —
                // Action semantics win over the step no-op.
                None if fwd => return false,
                // Document edge, backward: restart the current phrase, or
                // stay consumed (no-op) when even that isn't available
                // (unchanged from before).
                None => match phrase_at_time(&spans, pos) {
                    Some(i) => (spans[i].start_time, spoken_wi),
                    None => return true,
                },
            }
        }
    };
    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Seek(target_time));
    s.suppress_sync_until = Some(
        std::time::Instant::now() + crate::input::navigation::SYNC_SUPPRESS_SEEK,
    );
    // Move the cursor onto the target line; turn the page when it is off the
    // current spread (update_highlight_and_center routes through
    // set_page_instant, keeping the e-reader pagination state in sync).
    if target_wi != spoken_wi || target_wi != cursor_wi {
        if let Some(tb) = s.buffer_line_for_work(target_wi) {
            s.current_line = tb;
            let off_page = tb < s.page_top_line
                || s
                    .last_visible_range
                    .get()
                    .map_or(false, |vr| tb > vr.last_fit);
            if off_page {
                crate::input::highlight::update_highlight_and_center(s);
            } else {
                crate::input::highlight::update_highlight(s);
            }
        }
    } else if let Some(tb) = s.buffer_line_for_work(target_wi) {
        // Same-line step: on row-fill prose a page top like (L, 110) starts
        // MID-line, so the target phrase's wrapped row can sit on a different
        // stored page even though the line didn't change — `o` at the first
        // phrase of a straddling top line lands on the previous page's rows
        // and the page never turned. Land on the stored page holding the
        // phrase's display row.
        if let Some(ch) = spans
            .iter()
            .find(|sp| sp.start_time == target_time)
            .map(|sp| sp.start_char)
        {
            turn_to_phrase_page(s, tb, ch);
        }
    }
    if paint_pending_phrase(s, target_time) {
        s.phrase_paint_hold = s.suppress_sync_until;
    }
    crate::log_fmt!(
        "PHRASE_STEP: {} -> t={:.2} wi={} (pos was {:.2})",
        if forward { "next" } else { "prev/restart" },
        target_time,
        target_wi,
        pos
    );
    true
}

/// In single-column prose TABLE mode, make sure the display row holding
/// `char_off` of buffer line `bl` is on the current page: when the line
/// straddles stored pages, land on the page containing that row. No-op in
/// every other mode (play tables are line-granular, so a same-line phrase
/// step can never leave the page there).
fn turn_to_phrase_page(s: &mut AppState, bl: usize, char_off: usize) {
    let Some(table) = crate::input::prose_pages::active_prose_page_table(s) else {
        return;
    };
    let Some(cur) = crate::input::prose_pages::prose_page_for_position(
        &table,
        s.page_top_line,
        s.page_top_offset,
    ) else {
        return;
    };
    let Some(line_start) = s.buffer.iter_at_line(bl as i32) else {
        return;
    };
    // Top pixel (relative to the line's top) of the display row containing
    // `char_off`: walk the view's real row layout, keeping the last row whose
    // start char is at or before the target (display_row_char_at's inverse).
    let line_top = s.text_view.line_yrange(&line_start).0;
    let mut it = line_start;
    let row_top = loop {
        let y = s.text_view.iter_location(&it).y() - line_top;
        let mut next = it;
        if !s.text_view.forward_display_line(&mut next) || next.line() != line_start.line() {
            break y;
        }
        if next.line_offset().max(0) as usize > char_off {
            break y;
        }
        it = next;
    };
    let Some(target) = crate::input::prose_pages::prose_page_for_position(&table, bl, row_top)
    else {
        return;
    };
    if target != cur {
        let p = table[target];
        crate::logging::log_always(&format!(
            "PAGES_PROSE: phrase-step turn bl={} char={} row_px={} ({},{})->({},{})",
            bl, char_off, row_top, s.page_top_line, s.page_top_offset, p.start_line, p.start_off
        ));
        crate::input::scroll::set_page_instant_offset(s, p.start_line, p.start_off);
    }
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

/// Move the phrase sweep tag to `[start_char, end_char)` of buffer line `bl`.
fn apply_phrase_tag(s: &AppState, bl: usize, start_char: usize, end_char: usize) {
    let tag = s.phrase_tag.clone();
    let (bs, be) = s.buffer.bounds();
    s.buffer.remove_tag(&tag, &bs, &be);
    apply_char_range_tag(s, &tag, bl, start_char, end_char);
}

/// Apply `tag` to `[start_char, end_char)` of buffer line `bl`, clamped to
/// the line's char count (GTK iter offsets are unicode chars, matching the
/// Python backfill's str indices; clamping guards data drift). Does NOT
/// remove prior applications — callers own their tag's lifecycle.
pub(crate) fn apply_char_range_tag(
    s: &AppState,
    tag: &gtk4::TextTag,
    bl: usize,
    start_char: usize,
    end_char: usize,
) {
    let buffer = &s.buffer;
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
    buffer.apply_tag(tag, &a, &b);
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

    #[test]
    fn phrase_step_forward_next_span_then_cross_line() {
        let spans = [span(10.0, 12.0, 0, 1), span(12.0, 15.0, 0, 1)];
        assert_eq!(phrase_step_target(&spans, 0, 11.0, true), PhraseStep::Within(12.0));
        assert_eq!(phrase_step_target(&spans, 1, 13.0, true), PhraseStep::CrossLine(true));
    }

    #[test]
    fn phrase_step_backward_hybrid_restart_then_prev() {
        let spans = [span(10.0, 12.0, 0, 1), span(12.0, 15.0, 0, 1)];
        // Deep into span 1 (>1s past its 12.0 start): restart it.
        assert_eq!(phrase_step_target(&spans, 1, 14.0, false), PhraseStep::Within(12.0));
        // Near span 1's start: step back to span 0.
        assert_eq!(phrase_step_target(&spans, 1, 12.4, false), PhraseStep::Within(10.0));
        // Near span 0's start: cross to the previous line.
        assert_eq!(phrase_step_target(&spans, 0, 10.2, false), PhraseStep::CrossLine(false));
        // Deep into span 0: restart it, no cross.
        assert_eq!(phrase_step_target(&spans, 0, 11.5, false), PhraseStep::Within(10.0));
    }

    #[test]
    fn phrase_step_skips_spans_tied_to_the_current_window() {
        // A spoken run crossing a line break duplicates its time window on
        // every covered span (verse). Stepping onto a tied span is zero
        // motion — it must be skipped.
        let spans = [
            span(10.0, 15.0, 0, 10),
            span(10.0, 15.0, 11, 20),
            span(15.0, 18.0, 21, 30),
        ];
        // Forward from a tied pair: land on the first strictly-later span.
        assert_eq!(phrase_step_target(&spans, 0, 11.0, true), PhraseStep::Within(15.0));
        assert_eq!(phrase_step_target(&spans, 1, 11.0, true), PhraseStep::Within(15.0));
        // Backward near span 1's start: span 0 is tied — cross the line.
        assert_eq!(phrase_step_target(&spans, 1, 10.2, false), PhraseStep::CrossLine(false));
        // Forward when every remaining span is tied: cross the line.
        let all_tied = [span(10.0, 15.0, 0, 10), span(10.0, 15.0, 11, 20)];
        assert_eq!(phrase_step_target(&all_tied, 0, 11.0, true), PhraseStep::CrossLine(true));
    }

    #[test]
    fn cross_line_pick_requires_strict_motion() {
        // MM-Arkangel regression (2026-07-22): consecutive verse lines share
        // one window (897.945–902.347); the forward cross-line target took
        // the neighbor's first span and seeked BACKWARD to the current
        // phrase's start. The pick must be strictly later (earlier) than the
        // current span's start.
        let tied = [span(897.945, 902.347, 0, 39)];
        assert_eq!(cross_line_pick(&tied, Some(897.945), 897.945, true), None);
        let later = [span(902.688, 906.27, 0, 42)];
        assert_eq!(cross_line_pick(&later, Some(902.688), 897.945, true), Some(902.688));
        // A tied first span with a later second span: skip to the later one.
        let mixed = [span(897.945, 902.347, 0, 5), span(902.688, 906.27, 6, 20)];
        assert_eq!(cross_line_pick(&mixed, Some(897.945), 897.945, true), Some(902.688));
        // Backward: the neighbor's LAST strictly-earlier span.
        let prev = [span(893.0, 896.0, 0, 10), span(896.003, 897.104, 11, 27)];
        assert_eq!(cross_line_pick(&prev, Some(893.0), 897.945, false), Some(896.003));
        // Backward against a tied window: no pick.
        assert_eq!(cross_line_pick(&tied, Some(897.945), 897.945, false), None);
        // No spans on the neighbor line: fall back to its line start, same
        // strict rule.
        assert_eq!(cross_line_pick(&[], Some(902.0), 897.945, true), Some(902.0));
        assert_eq!(cross_line_pick(&[], Some(897.945), 897.945, true), None);
    }
}
