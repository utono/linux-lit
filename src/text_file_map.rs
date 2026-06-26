use std::ops::Range;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::db::models::Line;
use crate::db::line_types;

/// A sentence group with character-level boundary info for partial-line highlighting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SentenceGroup {
    /// Buffer line indices covered by this sentence.
    pub line_range: Range<usize>,
    /// Character offset on the first line where the sentence begins (0 = start of line).
    pub start_col: usize,
    /// Character offset on the last line where the sentence ends (None = end of line).
    pub end_col: Option<usize>,
}

/// Bidirectional map between a plain-text file's line indices and DB work line indices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineMap {
    /// For each buffer line index, the DB work_lines index it maps to (None if unmatched).
    pub buffer_to_work: Vec<Option<usize>>,
    /// For each DB work_lines index, the buffer line index it maps to.
    pub work_to_buffer: Vec<usize>,
    /// Buffer line indices that contain dialogue (matched or unmatched).
    pub dialogue_buffer_lines: Vec<usize>,
    /// Contiguous ranges of buffer lines forming sentences (prose text_file works only).
    pub sentence_groups: Vec<SentenceGroup>,
    /// Buffer line indices where a new chapter starts (`Line.is_chapter == true`).
    /// Sorted ascending. Used by pagination to force page breaks at chapter boundaries.
    pub chapter_breaks: Vec<usize>,
    /// For each buffer line index, `true` if this line is the first line of a new
    /// `(div1, div2)` run — i.e. an authoritative scene/section boundary derived
    /// from the DB's div columns, not inferred from buffer text. The boundary is
    /// attributed to the first synthesized chrome line (ACT/===/Scene marker) of
    /// the transition run, so a page that opens on that chrome owns its heading
    /// while a later boundary inside a range clamps. Empty (`false` everywhere)
    /// for prose / single-column works. See `build_section_starts`.
    pub section_starts: Vec<bool>,
}

/// How `build_line_map` matches buffer lines to DB rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// Whole-line normalized equality: one buffer line == one DB row (plays/verse).
    WholeLine,
    /// Accumulate consecutive buffer lines until their concatenated normalized
    /// text equals a DB row, mapping the whole run to that row (first line
    /// canonical). Sentence-split prose (BCP from text_file).
    ParagraphAccumulate,
}

/// Normalize a line of text to match the DB's `normalized_text` column:
/// strip bracketed stage directions, trim, lowercase, strip non-alphanumeric
/// chars (keep spaces), collapse whitespace.
pub fn normalize(s: &str) -> String {
    // Single-pass normalize: strip brackets + lowercase + NFD-strip
    // combining marks + alphanumeric-only + collapse whitespace, all in one
    // String allocation. Profiling showed the multi-stage version was the
    // dominant cost in build_line_map (~1500ms on Bleak House at 71k calls).
    //
    // Fast-path: skip if input is empty.
    if s.is_empty() {
        return String::new();
    }
    let mut result = String::with_capacity(s.len());
    let mut depth = 0usize; // bracket nesting depth
    let mut last_was_space = true; // start treating leading whitespace as already-emitted
    for ch in s.nfd() {
        // Skip combining marks (diacritics).
        if unicode_normalization::char::is_combining_mark(ch) {
            continue;
        }
        // Track bracket depth, swallowing chars inside brackets.
        match ch {
            '[' => {
                depth += 1;
                continue;
            }
            ']' if depth > 0 => {
                depth -= 1;
                continue;
            }
            // A `]` with no matching `[` on this line closes a bracket opened on a
            // PREVIOUS line — this line is the tail of a multi-line stage direction
            // (the DB splits such directions across line_mapping rows, e.g.
            // "with Hume, aloft.]"). Everything before it is bracket content, so
            // discard what we've emitted so far. Without this the tail leaks
            // through as spurious dialogue ("with hume aloft") and breaks the
            // line-map confirmation check against the folded .txt (which renders
            // the whole direction as one bracketed, empty-normalizing line).
            ']' => {
                result.clear();
                last_was_space = true;
                continue;
            }
            _ if depth > 0 => continue,
            _ => {}
        }
        // Lowercase + alphanumeric-or-space filter + collapse whitespace.
        if ch.is_alphanumeric() {
            for low in ch.to_lowercase() {
                result.push(low);
            }
            last_was_space = false;
        } else if ch.is_whitespace() || ch == ' ' {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        }
        // Other chars (punctuation, etc.) silently dropped.
    }
    // Trim trailing space.
    if result.ends_with(' ') {
        result.pop();
    }
    result
}

fn is_inside_stage_direction_text(lines: &[String], line: usize) -> bool {
    let trimmed = lines[line].trim();
    if line_types::is_stage_direction(trimmed) {
        return true;
    }
    let start = line.saturating_sub(20);
    for i in (start..line).rev() {
        let prev = lines[i].trim();
        if prev.ends_with(']') {
            return false;
        }
        if prev.starts_with('[') && !prev.ends_with(']') {
            return true;
        }
    }
    false
}

/// True when `needle` is a prefix of `haystack` followed by a space — i.e. a
/// whole-word prefix match (so "the" matches "the cat" but not "theory").
fn is_word_prefix(haystack: &str, needle: &str) -> bool {
    haystack.starts_with(needle) && haystack.as_bytes().get(needle.len()) == Some(&b' ')
}

const WINDOW: usize = 50;

/// Build a LineMap for a BCP work whose body prayers were split one sentence
/// per buffer line. `file_lines` are the split (display) lines;
/// `source_index[i]` is the work-line index that split line `i` came from
/// (consecutive equal values are a prayer's sentence sub-lines).
///
/// Unlike `build_line_map` (which text-matches a .txt against DB rows), BCP is
/// built straight from `work.lines` — `display_work` produces `source_index` by
/// enumerating `work.lines`, so the mapping is an exact, ordered 1:1: source
/// group `g` IS work line `g`. We therefore assign directly rather than
/// text-match. (Text-matching would orphan every `[...]` rubric, since
/// `normalize()` strips bracketed text to empty.) Every sub-line of a prayer
/// maps to its one work row; the FIRST sub-line is canonical in `work_to_buffer`
/// / `chapter_breaks` / `section_starts` (timestamps, sync, `u`/`.`, and scene
/// snapping key off that first line). `section_starts` reuses the authoritative
/// `(div1,div2)` logic on the split view.
pub fn build_line_map_bcp(
    file_lines: &[String],
    source_index: &[usize],
    work_lines: &[Line],
) -> LineMap {
    assert_eq!(file_lines.len(), source_index.len());
    let n_split = file_lines.len();
    let n_work = work_lines.len();

    // BCP works are built directly from `work.lines` (one source group per work
    // line, in order), so the mapping is an exact 1:1 — group `g` is work line
    // `g`. We do NOT text-match (build_line_map's normalize() strips `[...]`
    // rubrics to empty, which would orphan every rubric line); instead we map
    // every line, including rubrics, the way the old line_map-less identity did.
    //
    // collapsed_first_split[g] = first split (buffer) line of work group g.
    let mut collapsed_first_split: Vec<usize> = Vec::with_capacity(n_work);
    let mut buffer_to_work: Vec<Option<usize>> = vec![None; n_split];
    {
        let mut i = 0usize;
        while i < n_split {
            let src = source_index[i];
            collapsed_first_split.push(i);
            let mut j = i;
            while j < n_split && source_index[j] == src {
                buffer_to_work[j] = Some(src);
                j += 1;
            }
            i = j;
        }
    }
    debug_assert_eq!(collapsed_first_split.len(), n_work);

    // work_to_buffer: each work line -> its FIRST split line (canonical for
    // timestamps / sync / u-. / scene snapping).
    let work_to_buffer: Vec<usize> = (0..n_work)
        .map(|wi| collapsed_first_split.get(wi).copied().unwrap_or(0))
        .collect();

    // dialogue_buffer_lines: every split sub-line of a dialogue work line.
    let mut dialogue_buffer_lines: Vec<usize> = Vec::new();
    for (split_idx, w) in buffer_to_work.iter().enumerate() {
        if let Some(wi) = w {
            if work_lines[*wi].is_dialogue {
                dialogue_buffer_lines.push(split_idx);
            }
        }
    }

    // chapter_breaks: first split line of each chapter work line.
    let mut chapter_breaks: Vec<usize> = Vec::new();
    for (wi, l) in work_lines.iter().enumerate() {
        if l.is_chapter {
            chapter_breaks.push(collapsed_first_split[wi]);
        }
    }

    // section_starts: reuse the authoritative (div1,div2)-change logic, which
    // works off buffer_to_work + the buffer text. Runs on the SPLIT view; a
    // boundary attributes to the split line where the new (div1,div2) first
    // appears (a prayer's first sentence line).
    let section_starts = build_section_starts(file_lines, &buffer_to_work, work_lines);

    LineMap {
        buffer_to_work,
        work_to_buffer,
        dialogue_buffer_lines,
        // BCP is not a prose work; no character-level sentence groups.
        sentence_groups: Vec::new(),
        chapter_breaks,
        section_starts,
    }
}

/// Build a bidirectional map between `file_lines` (raw lines from a plain-text
/// file) and `work_lines` (DB rows for a work).
///
/// Algorithm (ported from Lua):
/// - Pre-normalize all file lines.
/// - For each non-empty normalized file line, scan forward in work_lines within
///   a sliding window of `WINDOW` rows starting at the current DB cursor.
/// - On a match beyond the current cursor: verify the next non-empty file line
///   also matches the next DB row. If the confirmation check fails, skip it.
/// - Log match percentage; warn if < 80%.
///
/// Thin `WholeLine` wrapper kept as the documented default API and exercised by
/// tests; all production callers now go through `build_line_map_mode` (the BCP
/// path needs `ParagraphAccumulate`), so it is dead outside test builds.
#[cfg_attr(not(test), allow(dead_code))]
pub fn build_line_map(file_lines: &[String], work_lines: &[Line], is_prose: bool) -> LineMap {
    build_line_map_mode(file_lines, work_lines, is_prose, MatchMode::WholeLine)
}

/// Choose the matcher for a work. A BCP work rendered from its sentence-split
/// `.txt` needs paragraph accumulation; everything else (plays, future prose
/// with a 1:1 .txt) uses whole-line matching.
pub fn match_mode_for_work(abbrev: &str, has_text_file: bool) -> MatchMode {
    if has_text_file && crate::db::line_types::is_bcp_work(abbrev) {
        MatchMode::ParagraphAccumulate
    } else {
        MatchMode::WholeLine
    }
}

/// Build a LineMap with an explicit `MatchMode` (see `MatchMode`). `build_line_map`
/// is the thin `WholeLine` wrapper.
pub fn build_line_map_mode(
    file_lines: &[String],
    work_lines: &[Line],
    is_prose: bool,
    mode: MatchMode,
) -> LineMap {
    let norm_file: Vec<String> = file_lines.iter().map(|l| normalize(l)).collect();
    // Re-normalize DB text through the same pipeline (strip_brackets + diacritics)
    // so stage directions like "[Flourish...]" become empty on both sides.
    let norm_db: Vec<String> = work_lines.iter().map(|l| normalize(&l.text)).collect();

    let n_buf = file_lines.len();
    let n_work = work_lines.len();

    let mut buffer_to_work: Vec<Option<usize>> = vec![None; n_buf];
    // Default: each work line maps to buffer line 0 (will be overwritten for matched lines)
    let mut work_to_buffer: Vec<usize> = vec![0; n_work];

    let mut matched: usize = 0;

    match mode {
        MatchMode::WholeLine => {
            let mut db_cursor: usize = 0; // current position in work_lines

            for buf_idx in 0..n_buf {
                // Stage directions normalize to empty (brackets stripped), so the
                // spoken-line matcher below skips them. Match a stage buffer line
                // to its DB stage row(s) (sub_line > 0) by RAW trimmed text. A
                // single-line SD matches one row 1:1. A multi-line SD that
                // `clean_file_lines` FOLDED into one space-joined buffer line
                // matches a RUN of consecutive sub_line>0 rows joined the same way
                // — see the fallback below (the old "byte-identical 1:1" assumption
                // breaks for folded directions now that lit.db has sub_line rows).
                if line_types::is_stage_direction(file_lines[buf_idx].trim()) {
                    let want = file_lines[buf_idx].trim();
                    let window_end = (db_cursor + WINDOW).min(n_work);

                    // Fast path: a single DB stage row equals the buffer line
                    // (single-line SDs and unfolded directions).
                    let single_hit = (db_cursor..window_end).find(|&wi| {
                        work_lines[wi].sub_line > 0 && work_lines[wi].text.trim() == want
                    });
                    if let Some(wi) = single_hit {
                        buffer_to_work[buf_idx] = Some(wi);
                        work_to_buffer[wi] = buf_idx;
                        db_cursor = wi + 1;
                        matched += 1;
                        continue;
                    }

                    // Fallback: `clean_file_lines` folds a multi-line stage
                    // direction into ONE buffer line (space-joined). It then
                    // matches NO single DB row, but DOES match a run of
                    // consecutive `sub_line > 0` rows joined the same way. Find
                    // that run, map the folded line to its FIRST row, and point
                    // every consumed row's reverse lookup at the folded line.
                    for start in db_cursor..window_end {
                        if work_lines[start].sub_line == 0 {
                            continue; // runs begin on a stage row
                        }
                        let mut joined = String::new();
                        let mut end = start;
                        while end < window_end && work_lines[end].sub_line > 0 {
                            if !joined.is_empty() {
                                joined.push(' ');
                            }
                            joined.push_str(work_lines[end].text.trim());
                            if joined.len() > want.len() {
                                break; // overshot — this run can't equal `want`
                            }
                            if joined == want {
                                work_to_buffer[start..=end].fill(buf_idx);
                                buffer_to_work[buf_idx] = Some(start);
                                db_cursor = end + 1;
                                matched += 1;
                                break;
                            }
                            end += 1;
                        }
                        if buffer_to_work[buf_idx].is_some() {
                            break;
                        }
                    }
                    continue;
                }

                let nf = &norm_file[buf_idx];
                if nf.is_empty() {
                    continue;
                }

                // Search forward in work_lines within the window
                let window_end = (db_cursor + WINDOW).min(n_work);
                let mut found: Option<usize> = None;

                'outer: for wi in db_cursor..window_end {
                    let db_norm = &norm_db[wi];
                    if db_norm.is_empty() {
                        continue;
                    }
                    if db_norm == nf {
                        // If this match is beyond the current cursor, do a confirmation check:
                        // the next non-empty file line should match the next DB row after wi.
                        if wi > db_cursor {
                            // Find the next non-empty DB row after wi
                            let mut next_db_norm: Option<&String> = None;
                            for wi2 in (wi + 1)..n_work {
                                if !norm_db[wi2].is_empty() {
                                    next_db_norm = Some(&norm_db[wi2]);
                                    break;
                                }
                            }
                            // Check whether any of the next few non-empty file lines
                            // matches the next DB row. Speaker names, stage directions,
                            // act/scene markers, and separators in the file have no DB
                            // counterpart, so skip them and only count dialogue-like
                            // lines toward the lookahead limit.
                            if let Some(nd) = next_db_norm {
                                let mut confirmed = false;
                                let mut seen = 0;
                                for bi2 in (buf_idx + 1)..n_buf {
                                    if norm_file[bi2].is_empty() {
                                        continue;
                                    }
                                    if &norm_file[bi2] == nd {
                                        confirmed = true;
                                        break;
                                    }
                                    let raw = &file_lines[bi2];
                                    if line_types::is_speaker(raw)
                                        || line_types::is_act_scene_marker(raw)
                                        || line_types::is_separator(raw)
                                    {
                                        continue;
                                    }
                                    seen += 1;
                                    if seen >= 3 {
                                        break;
                                    }
                                }
                                if !confirmed {
                                    continue 'outer;
                                }
                            }
                        }
                        found = Some(wi);
                        break;
                    }
                }

                if let Some(wi) = found {
                    buffer_to_work[buf_idx] = Some(wi);
                    work_to_buffer[wi] = buf_idx;
                    db_cursor = wi + 1;
                    matched += 1;
                }
            }
        }
        MatchMode::ParagraphAccumulate => {
            // Walk non-empty buffer lines, accumulating their normalized text
            // (joined by single spaces) until the running `acc` equals the
            // current DB row `norm_db[wi]`; then map the whole run [run_start..=bi]
            // to `wi` and advance. Lines that never join into a matching row stay
            // None (chrome). A single .txt line covering >=2 consecutive title
            // rows is peeled by `consume_merged_rows`.
            let mut wi: usize = 0; // current DB row
            let mut acc = String::new(); // running concatenation of the current run
            let mut run_start: Option<usize> = None; // first buffer line of the run

            for bi in 0..n_buf {
                let nf = &norm_file[bi];
                if nf.is_empty() {
                    continue;
                }
                // Skip past empty DB rows (rubrics normalize to empty etc.).
                while wi < n_work && norm_db[wi].is_empty() {
                    wi += 1;
                }
                if wi >= n_work {
                    break;
                }

                // Try to extend the current run with this line.
                let candidate = if acc.is_empty() {
                    nf.clone()
                } else {
                    format!("{} {}", acc, nf)
                };

                if candidate == norm_db[wi] {
                    // Run complete: map every line of the run to wi.
                    let start = run_start.unwrap_or(bi);
                    for b in start..=bi {
                        if !norm_file[b].is_empty() {
                            buffer_to_work[b] = Some(wi);
                        }
                    }
                    work_to_buffer[wi] = start;
                    matched += 1;
                    wi += 1;
                    acc.clear();
                    run_start = None;
                } else if is_word_prefix(&norm_db[wi], &candidate) {
                    // The row continues past this line: keep accumulating.
                    acc = candidate;
                    if run_start.is_none() {
                        run_start = Some(bi);
                    }
                } else if run_start.is_none() {
                    // Not mid-run; try a merged-title peel: ONE buffer line
                    // covering >=2 consecutive rows that are successive prefixes.
                    let first = wi;
                    if consume_merged_rows(nf, &norm_db, &mut wi, &mut work_to_buffer, &mut matched, bi) {
                        buffer_to_work[bi] = Some(first);
                    } else if let Some(target) =
                        find_skip_target(nf, &norm_db, wi, n_work)
                    {
                        // The current DB row(s) are unmatchable heads/chrome with no
                        // buffer counterpart (e.g. a "St. Luke i. 68." reference the
                        // .txt merges into the canticle head). Skip forward to the
                        // row this buffer line actually starts, leaving the skipped
                        // rows unmapped, then map/begin-run against `target`.
                        wi = target;
                        if *nf == norm_db[wi] {
                            buffer_to_work[bi] = Some(wi);
                            work_to_buffer[wi] = bi;
                            matched += 1;
                            wi += 1;
                        } else {
                            // prefix start (guaranteed by find_skip_target)
                            acc = nf.clone();
                            run_start = Some(bi);
                        }
                    }
                    // else: genuine chrome — leave None, stay on the same wi.
                } else {
                    // Mid-run resync: the partial run failed to complete. Abandon
                    // it (those lines stay None) and retry THIS line fresh against
                    // the current row.
                    acc.clear();
                    run_start = None;
                    if *nf == norm_db[wi] {
                        buffer_to_work[bi] = Some(wi);
                        work_to_buffer[wi] = bi;
                        matched += 1;
                        wi += 1;
                    } else if is_word_prefix(&norm_db[wi], nf.as_str()) {
                        acc = nf.clone();
                        run_start = Some(bi);
                    }
                    // else: leave None as genuine chrome, stay on the same wi.
                }
            }
        }
    }

    // Collect dialogue buffer lines (matched via DB, unmatched via text classification)
    let mut dialogue_buffer_lines: Vec<usize> = Vec::new();
    for (buf_idx, work_idx_opt) in buffer_to_work.iter().enumerate() {
        match work_idx_opt {
            Some(wi) => {
                if work_lines[*wi].is_dialogue {
                    dialogue_buffer_lines.push(buf_idx);
                }
            }
            None => {
                if line_types::is_dialogue(&file_lines[buf_idx], is_prose)
                    && !is_inside_stage_direction_text(file_lines, buf_idx)
                {
                    dialogue_buffer_lines.push(buf_idx);
                }
            }
        }
    }

    // Log match statistics (percentage is against work_lines, not file lines)
    let pct = if n_work > 0 {
        (matched as f64 / n_work as f64) * 100.0
    } else {
        100.0
    };
    crate::logging::log(&format!(
        "LINEMAP: matched {}/{} work lines ({:.1}%)",
        matched, n_work, pct
    ));
    if pct < 80.0 {
        crate::logging::log("LINEMAP: WARNING — less than 80% matched, text file may be stale or wrong");
    }

    // Sentence groups: contiguous buffer-line ranges forming sentences
    let sentence_groups = {
        let mut groups = build_sentence_groups_from_db(&buffer_to_work, work_lines)
            .unwrap_or_else(|| build_sentence_groups(file_lines));
        apply_mid_line_offsets(&mut groups, file_lines);
        groups
    };

    let mut chapter_breaks = Vec::new();
    for (work_idx, line) in work_lines.iter().enumerate() {
        if line.is_chapter && work_idx < work_to_buffer.len() {
            chapter_breaks.push(work_to_buffer[work_idx]);
        }
    }

    let section_starts = build_section_starts(file_lines, &buffer_to_work, work_lines);

    LineMap {
        buffer_to_work,
        work_to_buffer,
        dialogue_buffer_lines,
        sentence_groups,
        chapter_breaks,
        section_starts,
    }
}

/// When one buffer line `nf` (normalized) covers several consecutive DB rows
/// (a TEI <head> merged what lit.db keeps as separate title rows), peel each row
/// that is a successive leading prefix of `nf`. Accept ONLY if >= 2 rows are
/// consumed AND they exactly cover the whole line. On accept: set
/// work_to_buffer[w]=bi for each consumed row, advance *wi, add to *matched,
/// return true. On reject: roll back fully (mutate locals, commit on accept).
fn consume_merged_rows(
    nf: &str,
    norm_db: &[String],
    wi: &mut usize,
    work_to_buffer: &mut [usize],
    matched: &mut usize,
    bi: usize,
) -> bool {
    let mut rest: &str = nf;
    let mut cur = *wi;
    let mut consumed: Vec<usize> = Vec::new();
    while cur < norm_db.len() && !norm_db[cur].is_empty() {
        let row = norm_db[cur].as_str();
        if rest == row {
            consumed.push(cur);
            cur += 1;
            rest = "";
            break;
        } else if let Some(tail) = rest.strip_prefix(row) {
            let tail = tail.strip_prefix(' ').unwrap_or(tail);
            consumed.push(cur);
            cur += 1;
            rest = tail;
        } else {
            break;
        }
    }
    if consumed.len() >= 2 && rest.is_empty() {
        for w in &consumed {
            work_to_buffer[*w] = bi;
        }
        *matched += consumed.len();
        *wi = cur;
        true
    } else {
        false
    }
}

/// Maximum number of consecutive unmatchable DB rows the accumulator will skip
/// when re-syncing a buffer line onto a later row. Keeps the skip local so a
/// genuinely-unmatched buffer line (chrome) does not chew through real rows.
const ACC_SKIP_WINDOW: usize = 8;

/// When the current DB row at `wi` cannot be matched or prefix-extended by the
/// buffer line `nf` (e.g. it is a head/reference row the .txt merged away), look
/// ahead up to `ACC_SKIP_WINDOW` non-empty rows for the first row that `nf`
/// either equals or begins (`row.starts_with(nf + ' ')`). Returns that row index
/// so the caller can advance past the intervening unmatchable rows. Returns
/// `None` when no nearby row resyncs (the line is genuine buffer chrome).
fn find_skip_target(nf: &str, norm_db: &[String], wi: usize, n_work: usize) -> Option<usize> {
    let mut seen = 0usize;
    let mut cur = wi + 1; // wi itself already failed
    while cur < n_work && seen < ACC_SKIP_WINDOW {
        let row = norm_db[cur].as_str();
        if row.is_empty() {
            cur += 1;
            continue;
        }
        if row == nf || is_word_prefix(row, nf) {
            return Some(cur);
        }
        seen += 1;
        cur += 1;
    }
    None
}

/// Compute the authoritative scene/section-boundary bitmap from the DB's
/// `(div1, div2)` columns.
///
/// A boundary is exactly where `(div1, div2)` changes between two consecutive
/// *mapped* buffer lines (lines with a `buffer_to_work` entry). The `ACT N`,
/// `=====`, and `Scene N` lines in the buffer are display chrome that
/// `build_line_map` leaves unmapped (`None`) — they normalize to empty and have
/// no DB counterpart. We attribute each boundary to the FIRST chrome line
/// (act/scene marker or separator) of the transition run, so:
///   - a page that OPENS on that chrome line has `section_starts[page_top] ==
///     true` and owns its whole stacked heading (no self-clamp),
///   - a later boundary inside a page's range clamps before its chrome line.
///
/// Trailing `[They exit.]` / blank lines that precede the chrome belong to the
/// ENDING section, so they are NOT marked — only the marker line is.
///
/// Returns an all-`false` vec for single-`(div1,div2)` works (prose / single
/// scene), which the pagination predicate treats as "no boundaries".
/// Set `is_chapter = true` on the first line of each div1 boundary, clearing it
/// elsewhere (idempotent — safe to re-call on already-flagged lines).
///
/// Prose: each `div1 > 0` (front matter `div1 == 0` is not a chapter).
/// Non-prose: each change of `div1` from the previous line (the first mapped
/// line always counts, as a change from "no previous").
///
/// `lines` MUST be in canonical (div1, div2, line_in_div, sub_line) order — the
/// same order `load_work` SELECTs them in.
pub(crate) fn mark_chapter_starts(lines: &mut [crate::db::models::Line], is_prose: bool) {
    let mut prev_div1: Option<i64> = None;
    for line in lines.iter_mut() {
        let is_start = if is_prose {
            // A new div1 boundary where div1 > 0; front matter (0) never counts.
            line.div1 > 0 && Some(line.div1) != prev_div1
        } else {
            // Any change of div1 (including the first mapped line).
            Some(line.div1) != prev_div1
        };
        line.is_chapter = is_start;
        prev_div1 = Some(line.div1);
    }
}

fn build_section_starts(
    file_lines: &[String],
    buffer_to_work: &[Option<usize>],
    work_lines: &[Line],
) -> Vec<bool> {
    let n_buf = file_lines.len();
    let mut section_starts = vec![false; n_buf];

    let div_of = |buf: usize| -> Option<(i64, i64)> {
        buffer_to_work
            .get(buf)
            .copied()
            .flatten()
            .and_then(|wi| work_lines.get(wi))
            .map(|l| (l.div1, l.div2))
    };

    let mut prev_mapped: Option<(usize, (i64, i64))> = None;
    for buf in 0..n_buf {
        let Some(cur_div) = div_of(buf) else { continue };
        match prev_mapped {
            None => {
                // First mapped line: the work's opening section. Mark its
                // boundary at the first chrome/marker line at or before it (the
                // opening "ACT 1 / Scene 1" stacked title), falling back to the
                // line itself when there is no preceding chrome.
                let start = first_chrome_at_or_before(file_lines, buf);
                section_starts[start] = true;
            }
            Some((prev_buf, prev_div)) if prev_div != cur_div => {
                // (div1,div2) changed → scene boundary. Attribute it to the
                // first marker/separator line strictly after the previous
                // mapped line; if none (e.g. db-only with no chrome), use the
                // first non-mapped line after prev, else the current line.
                let start = first_chrome_after(file_lines, prev_buf, buf);
                section_starts[start] = true;
            }
            _ => {}
        }
        prev_mapped = Some((buf, cur_div));
    }

    section_starts
}

/// Boundary line for the work's OPENING section, given `buf` = the first mapped
/// (dialogue) line. The whole run of non-dialogue preamble above `buf` —
/// blanks, the act/scene title (`ACT 1` / `=====` / `Scene 1`), entrance stage
/// directions, and the leading speaker name — belongs to the opening section,
/// so the boundary is the TOPMOST act/scene marker (or separator) in that run.
/// We walk backward across the full preamble (not just chrome) to find it, and
/// fall back to the topmost preamble line when the work has no opening marker
/// (e.g. a poem that opens directly on text).
fn first_chrome_at_or_before(file_lines: &[String], buf: usize) -> usize {
    // Walk back over ALL non-dialogue preamble to the top of the leading block.
    let mut top = buf;
    while top > 0 {
        let t = file_lines[top - 1].trim();
        if t.is_empty()
            || line_types::is_act_scene_marker(t)
            || line_types::is_separator(t)
            || line_types::is_speaker(t)
            || line_types::is_stage_direction(t)
            || line_types::is_stanza_number(t)
            || is_title_above_separator(file_lines, top - 1)
        {
            top -= 1;
        } else {
            break;
        }
    }
    // Within [top, buf), prefer the first heading line the reader should see at
    // the page top: an act/scene marker, a separator, a bare stanza number, or a
    // plain TITLE line sitting directly above a separator (the anthology header
    // form `Sonnet 116` / `=====`, where the title matches none of the other
    // predicates but heads the section). Checked title-first so the boundary
    // lands on the title, not the underline beneath it. Else use `top`. A
    // `sonnet_sequence` heads each section with a bare number ("1"), which
    // `is_stanza_number` matches but the act/scene/separator predicates do not —
    // without it the boundary lands on the blank above the number.
    for i in top..buf {
        let t = file_lines[i].trim();
        if is_title_above_separator(file_lines, i)
            || line_types::is_act_scene_marker(t)
            || line_types::is_separator(t)
            || line_types::is_stanza_number(t)
        {
            return i;
        }
    }
    top
}

/// True if `file_lines[i]` is a plain title line that sits directly above a
/// `=====` separator — the anthology header form (`Sonnet 116` / `=====`).
/// Such a title heads its section but matches none of the act/scene/separator/
/// stanza-number predicates, so the section boundary would otherwise land on
/// the separator below it and orphan the title onto its own truncated page.
/// Folger plays never trip this: their pre-separator line is an `ACT N` marker,
/// which `is_act_scene_marker` already claims.
fn is_title_above_separator(file_lines: &[String], i: usize) -> bool {
    let t = file_lines[i].trim();
    if t.is_empty()
        || line_types::is_act_scene_marker(t)
        || line_types::is_separator(t)
        || line_types::is_stanza_number(t)
        || line_types::is_speaker(t)
        || line_types::is_stage_direction(t)
    {
        return false;
    }
    matches!(file_lines.get(i + 1), Some(next) if line_types::is_separator(next.trim()))
}

/// Find the boundary line for a `(div1,div2)` transition occurring between
/// mapped lines `prev` (last line of the ending section) and `cur` (first line
/// of the new section). Returns the first act/scene-marker or separator line in
/// `(prev, cur]`; if there is no such chrome line (db-only works with no
/// synthesized markers), returns `cur` itself.
fn first_chrome_after(file_lines: &[String], prev: usize, cur: usize) -> usize {
    for i in (prev + 1)..=cur.min(file_lines.len().saturating_sub(1)) {
        let t = file_lines[i].trim();
        if is_title_above_separator(file_lines, i)
            || line_types::is_act_scene_marker(t)
            || line_types::is_separator(t)
            || line_types::is_stanza_number(t)
        {
            return i;
        }
    }
    cur
}

/// Returns true if `line` ends with sentence-terminating punctuation,
/// optionally followed by closing quotes.
/// Check if the line ends with sentence-terminating punctuation (possibly + closing quote).
fn ends_sentence_at_eol(line: &str) -> bool {
    let trimmed = line.trim_end();
    if trimmed.is_empty() {
        return false;
    }
    let mut chars = trimmed.chars().rev();
    let last = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    let effective = if matches!(last, '"' | '\'' | '\u{201D}' | '\u{2019}') {
        chars.next().unwrap_or(last)
    } else {
        last
    };
    matches!(effective, '.' | '!' | '?')
}

/// Common abbreviations that end with a period but don't end a sentence.
const ABBREVIATIONS: &[&str] = &[
    "Mr", "Mrs", "Ms", "Dr", "St", "Rev", "Prof", "Gen", "Gov", "Sgt",
    "Capt", "Lt", "Col", "Jr", "Sr", "vs", "etc", "Vol", "No",
    "Hon", "Esq", "Messrs", "Dept", "Inc", "Corp", "Bros",
];

/// Check if the word immediately before `dot_pos` (a period) in `chars` is an abbreviation.
fn is_abbreviation(chars: &[char], dot_pos: usize) -> bool {
    // Walk backwards from the character before the dot to find the word start
    if dot_pos == 0 {
        return false;
    }
    let end = dot_pos; // exclusive — the dot itself
    let mut start = dot_pos;
    for k in (0..dot_pos).rev() {
        if chars[k].is_alphabetic() {
            start = k;
        } else {
            break;
        }
    }
    if start == end {
        return false;
    }
    let word: String = chars[start..end].iter().collect();
    ABBREVIATIONS.iter().any(|&abbr| abbr == word)
}

/// Find the character offset of a mid-line sentence boundary.
/// Returns the character offset (not byte offset) of the first character
/// of the new sentence (the uppercase letter after punctuation + optional
/// quote + space). This is a character offset suitable for
/// `TextIter::set_line_offset()`.
/// Returns None if no mid-line boundary exists.
fn find_mid_line_sentence_boundary(line: &str) -> Option<usize> {
    let chars: Vec<char> = line.chars().collect();
    for i in 0..chars.len() {
        if matches!(chars[i], '.' | '!' | '?') {
            // Skip abbreviations (only relevant for periods)
            if chars[i] == '.' && is_abbreviation(&chars, i) {
                continue;
            }
            let mut j = i + 1;
            // Skip optional closing quote
            if j < chars.len() && matches!(chars[j], '"' | '\'' | '\u{201D}' | '\u{2019}') {
                j += 1;
            }
            // Expect space then uppercase
            if j + 1 < chars.len() && chars[j] == ' ' && chars[j + 1].is_uppercase() {
                return Some(j + 1);
            }
        }
    }
    None
}

#[cfg(test)]
fn ends_sentence(line: &str) -> bool {
    ends_sentence_at_eol(line) || find_mid_line_sentence_boundary(line).is_some()
}

/// Scan adjacent sentence groups for mid-line boundaries and populate
/// `start_col`/`end_col` on shared boundary lines. Works on both DB-driven
/// and heuristic groups.
fn apply_mid_line_offsets(groups: &mut [SentenceGroup], file_lines: &[String]) {
    for i in 0..groups.len().saturating_sub(1) {
        let cur_last = groups[i].line_range.end.saturating_sub(1);
        let next_first = groups[i + 1].line_range.start;

        // Only applies when adjacent groups share a boundary line or are on consecutive lines
        // Check the last line of the current group for a mid-line sentence boundary
        if cur_last < file_lines.len() {
            if let Some(split) = find_mid_line_sentence_boundary(&file_lines[cur_last]) {
                // Current group ends at the split point on its last line
                if groups[i].end_col.is_none() {
                    groups[i].end_col = Some(split);
                }
                // Next group starts at the split point — extend its range to include the shared line
                if next_first == cur_last {
                    // Already shares the line
                    if groups[i + 1].start_col == 0 {
                        groups[i + 1].start_col = split;
                    }
                } else if next_first == cur_last + 1 {
                    // Adjacent lines — the next group's first line is the line after;
                    // the split is on cur_last, so extend the next group to include it
                    groups[i + 1].line_range.start = cur_last;
                    groups[i + 1].start_col = split;
                }
            }
        }
    }
}

/// Build sentence groups from DB-provided sentence_start_time values.
///
/// Groups consecutive buffer lines that share the same sentence_start_time.
/// Returns None if no sentence time data exists (triggers text-heuristic fallback).
fn build_sentence_groups_from_db(
    buffer_to_work: &[Option<usize>],
    work_lines: &[Line],
) -> Option<Vec<SentenceGroup>> {
    // Check if any lines have sentence time data
    let has_data = work_lines.iter().any(|l| {
        l.timestamp.as_ref().and_then(|t| t.sentence_start).is_some()
    });
    if !has_data {
        return None;
    }

    let mut groups: Vec<SentenceGroup> = Vec::new();
    let mut group_start: Option<usize> = None;
    let mut current_sentence_start: Option<f64> = None;

    for (buf_idx, work_idx_opt) in buffer_to_work.iter().enumerate() {
        let sentence_start = work_idx_opt
            .and_then(|wi| work_lines[wi].timestamp.as_ref())
            .and_then(|t| t.sentence_start);

        match (sentence_start, current_sentence_start) {
            (Some(ss), Some(css)) if (ss - css).abs() < 0.001 => {
                // Same sentence — extend the group
            }
            (Some(ss), _) => {
                // New sentence — close previous group if any
                if let Some(start) = group_start {
                    groups.push(SentenceGroup { line_range: start..buf_idx, start_col: 0, end_col: None });
                }
                group_start = Some(buf_idx);
                current_sentence_start = Some(ss);
            }
            (None, _) => {
                // No sentence data (blank line, unmapped line) — close group
                if let Some(start) = group_start {
                    groups.push(SentenceGroup { line_range: start..buf_idx, start_col: 0, end_col: None });
                }
                group_start = None;
                current_sentence_start = None;
            }
        }
    }
    // Close trailing group
    if let Some(start) = group_start {
        groups.push(SentenceGroup { line_range: start..buffer_to_work.len(), start_col: 0, end_col: None });
    }

    Some(groups)
}

/// Group buffer lines into sentence ranges with character-level boundary info.
/// A sentence boundary occurs when:
/// - A line ends with sentence-terminating punctuation (possibly + closing quote)
/// - A line contains a mid-line sentence boundary
/// - A blank line is encountered
fn build_sentence_groups(file_lines: &[String]) -> Vec<SentenceGroup> {
    let mut groups = Vec::new();
    let mut start_line: Option<usize> = None;
    let mut start_col: usize = 0;

    for (i, line) in file_lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if let Some(s) = start_line.take() {
                groups.push(SentenceGroup {
                    line_range: s..i,
                    start_col,
                    end_col: None,
                });
                start_col = 0;
            }
            continue;
        }

        if start_line.is_none() {
            start_line = Some(i);
        }

        if let Some(split) = find_mid_line_sentence_boundary(line) {
            // Close the current group: it ends on this line at the split point
            let s = start_line.take().unwrap();
            groups.push(SentenceGroup {
                line_range: s..i + 1,
                start_col,
                end_col: Some(split),
            });
            // Start a new group on this same line at the split point
            start_line = Some(i);
            start_col = split;
        } else if ends_sentence_at_eol(trimmed) {
            groups.push(SentenceGroup {
                line_range: start_line.take().unwrap()..i + 1,
                start_col,
                end_col: None,
            });
            start_col = 0;
        }
    }

    if let Some(s) = start_line {
        groups.push(SentenceGroup {
            line_range: s..file_lines.len(),
            start_col,
            end_col: None,
        });
    }

    groups
}

/// Find the index of the sentence group containing `buffer_line`, if any.
#[allow(dead_code)]
pub fn sentence_group_index(groups: &[SentenceGroup], buffer_line: usize) -> Option<usize> {
    groups
        .binary_search_by(|g| {
            if buffer_line < g.line_range.start {
                std::cmp::Ordering::Greater
            } else if buffer_line >= g.line_range.end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .ok()
}

/// Find the sentence group containing `buffer_line`, if any.
/// Note: mid-line boundaries cause adjacent groups to share a line (e.g. [0..1, 0..2]).
/// Binary search may return either group for the shared line. This is acceptable because
/// update_highlight uses whichever group contains current_line, and during playback the
/// cursor advances to the later group naturally.
pub fn sentence_group_for(groups: &[SentenceGroup], buffer_line: usize) -> Option<&SentenceGroup> {
    groups
        .binary_search_by(|g| {
            if buffer_line < g.line_range.start {
                std::cmp::Ordering::Greater
            } else if buffer_line >= g.line_range.end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .ok()
        .map(|idx| &groups[idx])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::Line;

    /// Regression for the reader-gloss main-card coloring on `-Amb` editions
    /// under CITATION-IDENTITY matching (the over-coloring fix replaced the old
    /// text-matching). Gloss passages are stored against the BASE work's
    /// citations; base and production editions (-Amb/-BBC/-DC) are now
    /// byte-identical in line_mapping (litdb folger-stage-directions), so each
    /// glossed line's `(div1,div2,line_in_div)` resolves identically on `2H6-Amb`
    /// and falls inside a passage's citation range. This asserts the previously
    /// reported `-Amb` lines DO fall within a glossed passage range (so they
    /// color), validating that the citation approach keeps `-Amb` coloring that
    /// motivated the original text-match workaround. Skipped when lit.db or the
    /// `-Amb` rows are unavailable.
    #[test]
    fn h6_amb_glossed_lines_match_by_citation() {
        let conn = match crate::db::queries::open_db() {
            Ok(c) => c,
            Err(_) => {
                eprintln!("skip: no lit.db");
                return;
            }
        };
        let passages = crate::db::queries::find_glossed_passages(&conn, "2H6", &["reader-gloss"])
            .unwrap_or_default();
        if passages.is_empty() {
            eprintln!("skip: no 2H6 reader-gloss passages");
            return;
        }
        let ranges: Vec<(String, String)> = passages
            .iter()
            .map(|p| (p.start_citation.clone(), p.end_citation.clone()))
            .collect();

        let amb = match crate::db::queries::load_work(&conn, "2H6-Amb") {
            Ok(w) => w,
            Err(_) => {
                eprintln!("skip: 2H6-Amb not loaded");
                return;
            }
        };
        for needle in [
            "Mother Jourdain, be you prostrate and grovel on",
            "the earth. John Southwell,",
            "read you; and let us to our work.",
        ] {
            let line = amb.lines.iter().find(|l| l.text.trim() == needle);
            let line = match line {
                Some(l) => l,
                None => {
                    eprintln!("skip: 2H6-Amb missing line {:?}", needle);
                    return;
                }
            };
            assert!(
                crate::app::line_in_any_passage(line.div1, line.div2, line.line_in_div, &ranges),
                "-Amb line {:?} ({}.{}.{}) is not inside any glossed passage range — \
                 it would render uncolored",
                needle, line.div1, line.div2, line.line_in_div
            );
        }
    }

    /// Regression guard: litdb folger-stage-directions made `2H6` and `2H6-Amb`
    /// byte-identical in `line_mapping`. Assert they stay in sync so a future
    /// litdb import cannot silently diverge and break the text-match coloring.
    /// Skipped when lit.db or the rows for either work are unavailable.
    #[test]
    fn base_and_amb_line_mapping_are_parity() {
        let conn = match crate::db::queries::open_db() {
            Ok(c) => c,
            Err(_) => {
                eprintln!("skip: no lit.db");
                return;
            }
        };
        let q = "SELECT div1,div2,line_in_div,sub_line,canonical_text \
                 FROM line_mapping \
                 WHERE work_abbrev=?1 \
                 ORDER BY div1,div2,line_in_div,sub_line";
        let rows = |abbrev: &str| -> Vec<(i64, i64, i64, i64, String)> {
            let mut st = conn.prepare(q).unwrap();
            st.query_map([abbrev], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .filter_map(Result::ok)
            .collect()
        };
        let base = rows("2H6");
        let amb = rows("2H6-Amb");
        if base.is_empty() || amb.is_empty() {
            eprintln!("skip: 2H6 or 2H6-Amb rows unavailable");
            return;
        }
        assert_eq!(base, amb, "2H6 and 2H6-Amb line_mapping must be byte-identical");
    }

    fn make_line(id: i64, text: &str, normalized: &str, is_dialogue: bool) -> Line {
        make_line_div(id, text, normalized, is_dialogue, 1, 1)
    }

    fn make_line_div(
        id: i64, text: &str, normalized: &str, is_dialogue: bool, div1: i64, div2: i64,
    ) -> Line {
        Line {
            id,
            citation: String::new(),
            text: text.to_string(),
            normalized: normalized.to_string(),
            speaker: None,
            is_dialogue,
            timestamp: None,
            div1,
            div2,
            line_in_div: id,
            sub_line: 0,
            is_chapter: false,
            is_spoken: None,
        }
    }

    /// A stage-direction work line (sub_line > 0). normalize() of stage text is
    /// empty, so pass "" as normalized (matches how the spoken-line matcher skips it).
    fn make_stage_line(id: i64, text: &str, div1: i64, div2: i64, line_in_div: i64, sub_line: i64) -> Line {
        Line {
            id,
            citation: String::new(),
            text: text.to_string(),
            normalized: String::new(),
            speaker: None,
            is_dialogue: false,
            timestamp: None,
            div1,
            div2,
            line_in_div,
            sub_line,
            is_chapter: false,
            is_spoken: None,
        }
    }

    #[test]
    fn folded_multiline_stage_direction_maps_to_its_rows() {
        // Buffer: dialogue, then a FOLDED SD (clean_file_lines joined two source
        // lines with a space), then more dialogue.
        let file_lines: Vec<String> = vec![
            "Lay hands upon these traitors and their trash.".into(),
            "[The Guard arrest Margery Jourdain and her accomplices and seize their papers.]".into(),
            "Beldam, I think we watched you at an".into(),
        ];
        // DB: the dialogue rows + the SD split across TWO sub_line>0 rows.
        let work_lines = vec![
            make_line_div(1, "Lay hands upon these traitors and their trash.",
                "lay hands upon these traitors and their trash", true, 1, 4),
            make_stage_line(2, "[The Guard arrest Margery Jourdain and her", 1, 4, 43, 1),
            make_stage_line(3, "accomplices and seize their papers.]", 1, 4, 43, 2),
            make_line_div(4, "Beldam, I think we watched you at an",
                "beldam i think we watched you at an", true, 1, 4),
        ];
        let lm = build_line_map(&file_lines, &work_lines, false);
        // Folded buffer line 1 -> first SD row (work idx 1).
        assert_eq!(lm.buffer_to_work[1], Some(1), "folded SD must map to its first DB row");
        // BOTH SD rows' reverse lookup point at the folded buffer line 1.
        assert_eq!(lm.work_to_buffer[1], 1);
        assert_eq!(lm.work_to_buffer[2], 1);
        // Surrounding dialogue unaffected.
        assert_eq!(lm.buffer_to_work[0], Some(0));
        assert_eq!(lm.buffer_to_work[2], Some(3));
    }

    #[test]
    fn single_line_stage_direction_still_maps_1to1() {
        let file_lines: Vec<String> = vec!["[To Jourdain.]".into()];
        let work_lines = vec![make_stage_line(1, "[To Jourdain.]", 1, 4, 43, 3)];
        let lm = build_line_map(&file_lines, &work_lines, false);
        assert_eq!(lm.buffer_to_work[0], Some(0));
        assert_eq!(lm.work_to_buffer[0], 0);
    }

    #[test]
    fn unmatched_folded_stage_direction_stays_none() {
        // A folded SD whose join matches no DB run leaves buffer_to_work None.
        let file_lines: Vec<String> = vec!["[Nobody arrests anyone at all here.]".into()];
        let work_lines = vec![
            make_stage_line(1, "[The Guard arrest Margery Jourdain and her", 1, 4, 43, 1),
            make_stage_line(2, "accomplices and seize their papers.]", 1, 4, 43, 2),
        ];
        let lm = build_line_map(&file_lines, &work_lines, false);
        assert_eq!(lm.buffer_to_work[0], None);
    }

    #[test]
    fn test_normalize() {
        assert_eq!(normalize("Who's there?"), "whos there");
        assert_eq!(normalize("Long live the King!"), "long live the king");
        assert_eq!(normalize("  Hello,  World!  "), "hello world");
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("He."), "he");
        assert_eq!(normalize("A"), "a");
        // Accented characters should be stripped to base form
        assert_eq!(
            normalize("Long live Lord Titus, my belovèd brother,"),
            "long live lord titus my beloved brother"
        );
        assert_eq!(normalize("circumscribèd"), "circumscribed");
        assert_eq!(normalize("damnèd"), "damned");
    }

    #[test]
    fn normalize_strips_unmatched_closing_bracket_tail() {
        // The DB splits a multi-line stage direction across rows; the tail row
        // ends with `]` but has no `[` (it opened on a prior row). That tail is
        // bracket content and must normalize to empty so it matches the folded
        // .txt (whole direction = one empty-normalizing line). Regression for the
        // 2H6-Amb `read you; and let us to our work.` u-bind failure.
        assert_eq!(normalize("with Hume, aloft.]"), "");
        assert_eq!(normalize("then the Spirit riseth.]"), "");
        assert_eq!(normalize("Buckingham, on the other.]"), "");
        // A complete inline bracket on one line still strips only the bracketed
        // span, keeping the surrounding dialogue.
        assert_eq!(normalize("He said [aside] no more."), "he said no more");
        // Text AFTER an unmatched close (rare) is kept.
        assert_eq!(normalize("aloft.] Well said"), "well said");
    }

    #[test]
    fn test_build_line_map_basic() {
        // Simulated file: ACT header, speaker name, blank line, two dialogue lines
        let file_lines: Vec<String> = vec![
            "ACT I".to_string(),
            "HAMLET".to_string(),
            "".to_string(),
            "To be, or not to be".to_string(),
            "that is the question".to_string(),
        ];

        // DB only contains the two dialogue lines (headers/speakers stripped)
        let work_lines = vec![
            make_line(1, "To be, or not to be", "to be or not to be", true),
            make_line(2, "that is the question", "that is the question", true),
        ];

        let map = build_line_map(&file_lines, &work_lines, false);

        // Buffer lines 3 and 4 should match work lines 0 and 1
        assert_eq!(map.buffer_to_work[3], Some(0));
        assert_eq!(map.buffer_to_work[4], Some(1));

        // Header lines and blank line should be unmatched
        assert_eq!(map.buffer_to_work[0], None);
        assert_eq!(map.buffer_to_work[1], None);
        assert_eq!(map.buffer_to_work[2], None);

        // Reverse map
        assert_eq!(map.work_to_buffer[0], 3);
        assert_eq!(map.work_to_buffer[1], 4);

        // Both matched lines are dialogue; unmatched ACT/HAMLET/blank are not (play mode)
        assert_eq!(map.dialogue_buffer_lines, vec![3, 4]);
    }

    #[test]
    fn test_build_line_map_confirmation_check() {
        // "He." is a very short line that could match many places.
        // The confirmation check should prevent a false positive match when the
        // following file line does NOT match the next DB row after the candidate.
        //
        // File:      ["He.", "Something else entirely"]
        // DB row 0:  "he"    (normalized "He.")
        // DB row 1:  "different content"
        //
        // The file's second non-empty line "something else entirely" does NOT match
        // "different content", so the match at row 0 should be rejected if the cursor
        // is already past it (we force that by placing a confirmed earlier match first).

        let file_lines: Vec<String> = vec![
            "First line".to_string(),  // matches DB row 0
            "He.".to_string(),          // candidate: DB row 1 ("he") — next file line won't match DB row 2
            "Something else entirely".to_string(), // does NOT match "different content"
        ];

        let work_lines = vec![
            make_line(1, "First line", "first line", false),
            make_line(2, "He.", "he", true),
            make_line(3, "Different content", "different content", false),
        ];

        let map = build_line_map(&file_lines, &work_lines, false);

        // "First line" at buf 0 should match work row 0 — db_cursor advances to 1
        assert_eq!(map.buffer_to_work[0], Some(0));

        // "He." at buf 1: db_cursor is 1, candidate is wi=1 (at cursor, not beyond),
        // so no confirmation needed — it should match directly.
        assert_eq!(map.buffer_to_work[1], Some(1));

        // "Something else entirely" at buf 2 does not match "different content" — no match.
        assert_eq!(map.buffer_to_work[2], None);
    }

    #[test]
    fn test_confirmation_skips_structural_lines_at_act_boundary() {
        let file_lines: Vec<String> = vec![
            "Why, this it is: my heart accords thereto.".to_string(),
            "[Aside.]".to_string(),
            "And yet a thousand times it answers no.".to_string(),
            "[They exit.]".to_string(),
            "".to_string(),
            "ACT 2".to_string(),
            "=====".to_string(),
            "".to_string(),
            "Scene 1".to_string(),
            "=======".to_string(),
            "".to_string(),
            "[Enter Valentine and Speed, carrying a glove.]".to_string(),
            "".to_string(),
            "SPEED".to_string(),
            "Sir, your glove.".to_string(),
        ];

        let work_lines = vec![
            make_line(1, "Why, this it is: my heart accords thereto.", "why this it is my heart accords thereto", true),
            make_line(2, "[Aside.]", "aside", true),
            make_line(3, "And yet a thousand times it answers no.", "and yet a thousand times it answers no", true),
            make_line(4, "[They exit.]", "they exit", false),
            make_line(5, "[Enter Valentine and Speed, carrying a glove.]", "enter valentine and speed carrying a glove", false),
            make_line(6, "Sir, your glove.", "sir your glove", true),
        ];

        let map = build_line_map(&file_lines, &work_lines, false);

        assert_eq!(map.buffer_to_work[0], Some(0));
        assert_eq!(map.buffer_to_work[2], Some(2));
        assert_eq!(map.buffer_to_work[14], Some(5));
    }

    #[test]
    fn test_section_starts_attributes_chrome_to_the_marker() {
        // The AWW y-GAP shape: a scene ends with dialogue, then [They exit.] /
        // blank, then the stacked ACT 2 / === / Scene 1 chrome, then the next
        // scene's dialogue. The (div1,div2) change is between the two dialogue
        // lines (1,1)->(2,1); the boundary must be attributed to the FIRST
        // chrome line (ACT 2), NOT the [They exit.] that belongs to scene 1.
        let file_lines: Vec<String> = vec![
            "And yet a thousand times it answers no.".into(), // 0 dialogue (1,1)
            "[They exit.]".into(),                            // 1 stage dir (scene 1's)
            "".into(),                                        // 2 blank
            "ACT 2".into(),                                   // 3 chrome  <-- boundary
            "=====".into(),                                   // 4 separator
            "Scene 1".into(),                                 // 5 chrome
            "[Enter Valentine.]".into(),                      // 6 stage dir
            "SPEED".into(),                                   // 7 speaker
            "Sir, your glove.".into(),                        // 8 dialogue (2,1)
        ];
        let work_lines = vec![
            make_line_div(1, "And yet a thousand times it answers no.", "and yet a thousand times it answers no", true, 1, 1),
            make_line_div(2, "[They exit.]", "they exit", false, 1, 1),
            make_line_div(3, "[Enter Valentine.]", "enter valentine", false, 2, 1),
            make_line_div(4, "Sir, your glove.", "sir your glove", true, 2, 1),
        ];

        let map = build_line_map(&file_lines, &work_lines, false);

        // First mapped line (buf 0) is the work's opening section — its boundary
        // is at-or-before it; with no preceding chrome it's line 0 itself.
        assert!(map.section_starts[0], "opening section boundary at the first mapped line");
        // The scene boundary is the ACT 2 marker (buf 3), not [They exit.] (buf 1).
        assert!(map.section_starts[3], "boundary attributed to the ACT 2 chrome line");
        assert!(!map.section_starts[1], "[They exit.] belongs to the ending scene, not a boundary");
        assert!(!map.section_starts[2], "trailing blank is not a boundary");
        assert!(!map.section_starts[8], "the new scene's dialogue is not the boundary line");
        // Exactly two boundaries total.
        assert_eq!(map.section_starts.iter().filter(|b| **b).count(), 2);
    }

    #[test]
    fn test_section_starts_anthology_title_above_separator() {
        // Anthology (work_type='anthology', e.g. DavidCrystalOP) header shape:
        // a plain TITLE line directly above a `=====` separator heads each
        // excerpt. The title matches none of the act/scene/separator/stanza
        // predicates, so without is_title_above_separator the boundary would
        // land on the `=====` and orphan the title onto a one-line page (the
        // bug: only "Sonnet 116" rendered). Each excerpt is its own (div1,div2).
        let file_lines: Vec<String> = vec![
            "Sonnet 116".into(),                              // 0 title  <-- boundary
            "==========".into(),                              // 1 separator
            "".into(),                                        // 2 blank
            "Let me not to the marriage of true minds".into(),// 3 verse (1,1)
            "".into(),                                        // 4 blank
            "".into(),                                        // 5 blank
            "Hamlet".into(),                                  // 6 title  <-- boundary
            "======".into(),                                  // 7 separator
            "Act 3, Scene 1".into(),                          // 8 subheader
            "".into(),                                        // 9 blank
            "To be or not to be—that is the question:".into(),// 10 verse (2,1)
        ];
        let work_lines = vec![
            make_line_div(1, "Let me not to the marriage of true minds",
                          "let me not to the marriage of true minds", true, 1, 1),
            make_line_div(2, "To be or not to be—that is the question:",
                          "to be or not to be that is the question", true, 2, 1),
        ];

        let map = build_line_map(&file_lines, &work_lines, false);

        // Opening excerpt boundary lands on the title (buf 0), NOT the separator.
        assert!(map.section_starts[0], "opening boundary on the title 'Sonnet 116'");
        assert!(!map.section_starts[1], "the ===== underline is not the boundary");
        // Second excerpt boundary lands on its title (buf 6), NOT the separator.
        assert!(map.section_starts[6], "second excerpt boundary on the title 'Hamlet'");
        assert!(!map.section_starts[7], "the ===== underline is not the boundary");
        // Exactly two boundaries (one per excerpt).
        assert_eq!(map.section_starts.iter().filter(|b| **b).count(), 2);
    }

    #[test]
    fn test_section_starts_sonnet_sequence_pins_to_number_heading() {
        // A sonnet sequence: each sonnet is a bare-number heading ("1"), a blank,
        // the body, a blank, then the next heading. The headings are UNMAPPED
        // (front matter / chrome, no (div1,div2)); each body is mapped with
        // div1 = sonnet number. The boundary must pin to the NUMBER heading, not
        // the blank above the body — without `is_stanza_number` recognition it
        // landed on the blank (sonnet 1: line 1) and on the body (sonnet 2:
        // line 7), splitting the heading from its body across pages.
        let file_lines: Vec<String> = vec![
            "1".into(),                                  // 0 heading  <-- boundary
            "".into(),                                   // 1 blank
            "From fairest creatures we desire increase,".into(), // 2 body (1,0)
            "That thereby beauty’s rose might never die,".into(), // 3 body (1,0)
            "His tender heir might bear his memory.".into(),      // 4 body (1,0)
            "".into(),                                   // 5 blank
            "2".into(),                                  // 6 heading  <-- boundary
            "".into(),                                   // 7 blank
            "When forty winters shall besiege thy brow".into(),  // 8 body (2,0)
            "And dig deep trenches in thy beauty’s field,".into(), // 9 body (2,0)
        ];
        let work_lines = vec![
            make_line_div(1, "From fairest creatures we desire increase,", "from fairest creatures we desire increase", true, 1, 0),
            make_line_div(2, "That thereby beauty’s rose might never die,", "that thereby beautys rose might never die", true, 1, 0),
            make_line_div(3, "His tender heir might bear his memory.", "his tender heir might bear his memory", true, 1, 0),
            make_line_div(4, "When forty winters shall besiege thy brow", "when forty winters shall besiege thy brow", true, 2, 0),
            make_line_div(5, "And dig deep trenches in thy beauty’s field,", "and dig deep trenches in thy beautys field", true, 2, 0),
        ];

        let map = build_line_map(&file_lines, &work_lines, false);

        assert!(map.section_starts[0], "sonnet 1 boundary pins to the '1' heading");
        assert!(map.section_starts[6], "sonnet 2 boundary pins to the '2' heading");
        assert!(!map.section_starts[1], "the blank above sonnet 1's body is not the boundary");
        assert!(!map.section_starts[2], "sonnet 1's body first line is not the boundary");
        assert!(!map.section_starts[7], "the blank above sonnet 2's body is not the boundary");
        assert!(!map.section_starts[8], "sonnet 2's body first line is not the boundary");
        assert_eq!(map.section_starts.iter().filter(|b| **b).count(), 2,
            "exactly one boundary per sonnet, pinned to its number heading");
    }

    #[test]
    fn test_section_starts_empty_for_single_section() {
        // A work entirely in one (div1,div2): only the opening boundary is set.
        let file_lines: Vec<String> = vec![
            "First line of the only scene.".into(),
            "Second line.".into(),
        ];
        let work_lines = vec![
            make_line_div(1, "First line of the only scene.", "first line of the only scene", true, 1, 1),
            make_line_div(2, "Second line.", "second line", true, 1, 1),
        ];
        let map = build_line_map(&file_lines, &work_lines, false);
        assert!(map.section_starts[0], "opening boundary set");
        assert_eq!(map.section_starts.iter().filter(|b| **b).count(), 1,
            "no interior boundaries within a single section");
    }

    #[test]
    fn test_ends_sentence() {
        assert!(ends_sentence("conducted."));
        assert!(ends_sentence("world!"));
        assert!(ends_sentence("really?"));
        assert!(ends_sentence("end.\""));
        assert!(ends_sentence("end.\u{201D}"));
        assert!(ends_sentence("end.'"));
        // Mr. ends with `.` so returns true — known acceptable edge case
        assert!(ends_sentence("Mr."));
    }

    #[test]
    fn test_build_sentence_groups() {
        let lines: Vec<String> = vec![
            "The first ray of light which illumines the gloom, and converts into a".into(),
            "dazzling brilliancy that obscurity in which the earlier history of the".into(),
            "public career of the immortal Pickwick would appear to be involved, is".into(),
            "derived from the perusal of the following entry in the Transactions of".into(),
            "the Pickwick Club.".into(),
            "".into(),
            "Next paragraph starts here and".into(),
            "continues on this line.".into(),
        ];
        let groups = build_sentence_groups(&lines);
        assert_eq!(groups[0].line_range, 0..5);
        assert_eq!(groups[0].start_col, 0);
        assert_eq!(groups[0].end_col, None);
        assert_eq!(groups[1].line_range, 6..8);
        assert_eq!(groups[1].start_col, 0);
        assert_eq!(groups[1].end_col, None);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn test_build_sentence_groups_multiple_sentences_no_blank() {
        let lines: Vec<String> = vec![
            "First sentence ends here.".into(),
            "Second sentence starts and".into(),
            "ends here!".into(),
            "Third sentence.".into(),
        ];
        let groups = build_sentence_groups(&lines);
        assert_eq!(groups[0].line_range, 0..1);
        assert_eq!(groups[0].start_col, 0);
        assert_eq!(groups[0].end_col, None);
        assert_eq!(groups[1].line_range, 1..3);
        assert_eq!(groups[1].start_col, 0);
        assert_eq!(groups[1].end_col, None);
        assert_eq!(groups[2].line_range, 3..4);
        assert_eq!(groups[2].start_col, 0);
        assert_eq!(groups[2].end_col, None);
    }

    #[test]
    fn test_sentence_group_for() {
        let groups = vec![
            SentenceGroup { line_range: 0..5, start_col: 0, end_col: None },
            SentenceGroup { line_range: 6..8, start_col: 0, end_col: None },
            SentenceGroup { line_range: 9..12, start_col: 0, end_col: None },
        ];
        assert_eq!(sentence_group_for(&groups, 0).map(|g| &g.line_range), Some(&(0..5)));
        assert_eq!(sentence_group_for(&groups, 4).map(|g| &g.line_range), Some(&(0..5)));
        assert_eq!(sentence_group_for(&groups, 5), None);
        assert_eq!(sentence_group_for(&groups, 7).map(|g| &g.line_range), Some(&(6..8)));
        assert_eq!(sentence_group_for(&groups, 11).map(|g| &g.line_range), Some(&(9..12)));
        assert_eq!(sentence_group_for(&groups, 12), None);
    }

    #[test]
    fn test_build_sentence_groups_mid_line_offsets() {
        let lines: Vec<String> = vec![
            "First sentence ends here. Second starts now and".into(),
            "continues on this line.".into(),
        ];
        let groups = build_sentence_groups(&lines);

        // First group: just the beginning of line 0 up to the split point
        assert_eq!(groups[0].line_range, 0..1);
        assert_eq!(groups[0].start_col, 0);
        assert_eq!(groups[0].end_col, Some(26)); // "First sentence ends here. " = 26 chars, split at 'S'

        // Second group: rest of line 0 through line 1
        assert_eq!(groups[1].line_range, 0..2);
        assert_eq!(groups[1].start_col, 26);
        assert_eq!(groups[1].end_col, None); // ends at EOL of line 1
    }

    #[test]
    fn test_build_sentence_groups_no_mid_line_boundary() {
        // No mid-line boundaries — start_col=0, end_col=None for all groups
        let lines: Vec<String> = vec![
            "The first ray of light which illumines the gloom, and converts into a".into(),
            "dazzling brilliancy.".into(),
            "".into(),
            "Next paragraph.".into(),
        ];
        let groups = build_sentence_groups(&lines);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].line_range, 0..2);
        assert_eq!(groups[0].start_col, 0);
        assert_eq!(groups[0].end_col, None);
        assert_eq!(groups[1].line_range, 3..4);
        assert_eq!(groups[1].start_col, 0);
        assert_eq!(groups[1].end_col, None);
    }

    #[test]
    fn test_find_mid_line_sentence_boundary() {
        // Basic case: period + space + uppercase
        // "end of the fog. On such an afternoon"
        //  0123456789012345^-- char 16 is 'O'
        assert_eq!(
            find_mid_line_sentence_boundary("end of the fog. On such an afternoon"),
            Some(16)
        );

        // Exclamation mark
        // "incredible! The next day"
        //  0123456789012^-- char 12 is 'T'
        assert_eq!(
            find_mid_line_sentence_boundary("incredible! The next day"),
            Some(12)
        );

        // Question mark
        // "is it? Yes it is."
        //  0123456^-- char 7 is 'Y'
        assert_eq!(
            find_mid_line_sentence_boundary("is it? Yes it is."),
            Some(7)
        );

        // Closing quote before space
        // "the end." And then"
        //  0123456789^-- char 10 is 'A'
        assert_eq!(
            find_mid_line_sentence_boundary("the end.\" And then"),
            Some(10)
        );

        // Right double quote (U+201D) — note: U+201D is 1 char
        // "the end.\u{201D} And then"
        //  0123456789^-- char 10 is 'A'
        assert_eq!(
            find_mid_line_sentence_boundary("the end.\u{201D} And then"),
            Some(10)
        );

        // No boundary
        assert_eq!(
            find_mid_line_sentence_boundary("no boundary here at all"),
            None
        );

        // Period but no uppercase after
        assert_eq!(
            find_mid_line_sentence_boundary("Mr. smith went home"),
            None
        );

        // Abbreviation with uppercase after — not a sentence boundary
        assert_eq!(
            find_mid_line_sentence_boundary("Mr. Tangle on his legs again."),
            None
        );

        // Multiple abbreviations in a line
        assert_eq!(
            find_mid_line_sentence_boundary("Dr. Smith and Mrs. Jones arrived."),
            None
        );

        // Abbreviation followed by real sentence boundary
        // "said Mr. Tangle prematurely. In reference,"
        //  0         1         2
        //  0123456789012345678901234567890
        // The "Mr. T" is an abbreviation (skip), but ". I" at pos 27 is a real boundary
        assert_eq!(
            find_mid_line_sentence_boundary("said Mr. Tangle prematurely. In reference,"),
            Some(29)
        );

        // Period at end of line (not mid-line)
        assert_eq!(
            find_mid_line_sentence_boundary("the end."),
            None
        );
    }

    #[test]
    fn test_sentence_group_struct() {
        let sg = SentenceGroup {
            line_range: 0..5,
            start_col: 0,
            end_col: None,
        };
        assert_eq!(sg.line_range, 0..5);
        assert_eq!(sg.start_col, 0);
        assert_eq!(sg.end_col, None);

        let sg2 = SentenceGroup {
            line_range: 5..8,
            start_col: 19,
            end_col: Some(25),
        };
        assert!(sg2.line_range.contains(&6));
        assert_eq!(sg2.start_col, 19);
        assert_eq!(sg2.end_col, Some(25));
    }

    #[test]
    fn test_build_sentence_groups_mid_line_at_first_line() {
        // Mid-line boundary on the very first line
        let lines: Vec<String> = vec![
            "Done. Now we begin a new".into(),
            "paragraph of text.".into(),
        ];
        let groups = build_sentence_groups(&lines);
        // "Done. " ends at split, "Now" starts the second sentence
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].line_range, 0..1);
        assert_eq!(groups[0].start_col, 0);
        assert!(groups[0].end_col.is_some());
        assert_eq!(groups[1].line_range, 0..2);
        assert!(groups[1].start_col > 0);
        assert_eq!(groups[1].end_col, None);
    }

    #[test]
    fn test_build_sentence_groups_consecutive_mid_line() {
        // Two sentences on one line, each ending mid-line
        // "A. B. C continues" has two boundaries
        // find_mid_line_sentence_boundary returns only the FIRST boundary
        let lines: Vec<String> = vec![
            "A. B. C continues here.".into(),
        ];
        let groups = build_sentence_groups(&lines);
        // First boundary splits at "B", so group 0 = "A. ", group 1 starts at "B. C continues here."
        // The second boundary "B. C" is detected when processing group starting at "B"
        // but find_mid_line_sentence_boundary scans from the start of the trimmed line,
        // so the line "A. B. C continues here." starting from the perspective of the full line
        // will find the first boundary "A. B" — the function only returns the first match.
        // This means "B. C" won't be detected as a separate boundary on the same line.
        // This is acceptable for word-level splitting — a known limitation.
        assert!(groups.len() >= 2);
    }

    #[test]
    fn test_apply_mid_line_offsets_on_db_style_groups() {
        // Simulate DB-produced groups: each line belongs to one group, no char offsets
        let file_lines: Vec<String> = vec![
            "Jarndyce and Jarndyce shall be got out of the office. Shirking and".into(),
            "sharking in all their many varieties.".into(),
        ];
        let mut groups = vec![
            SentenceGroup { line_range: 0..1, start_col: 0, end_col: None },
            SentenceGroup { line_range: 1..2, start_col: 0, end_col: None },
        ];
        apply_mid_line_offsets(&mut groups, &file_lines);

        // First group should now end at the split point on line 0
        assert_eq!(groups[0].end_col, Some(54)); // "...office. " then 'S' at char 54
        // Second group should now start at line 0 with start_col at the split
        assert_eq!(groups[1].line_range.start, 0);
        assert_eq!(groups[1].start_col, 54);
    }

    #[test]
    fn test_apply_mid_line_offsets_no_boundary() {
        // Adjacent groups where the boundary line has no mid-line split
        let file_lines: Vec<String> = vec![
            "This line ends with a period.".into(),
            "This line starts a new sentence.".into(),
        ];
        let mut groups = vec![
            SentenceGroup { line_range: 0..1, start_col: 0, end_col: None },
            SentenceGroup { line_range: 1..2, start_col: 0, end_col: None },
        ];
        apply_mid_line_offsets(&mut groups, &file_lines);

        // No mid-line boundary — groups unchanged
        assert_eq!(groups[0].end_col, None);
        assert_eq!(groups[1].start_col, 0);
        assert_eq!(groups[1].line_range.start, 1);
    }

    #[test]
    fn test_build_line_map_bcp_sublines_map_to_one_row() {
        // A rite title (its own work line), then a 2-sentence prayer split into
        // two buffer lines (both from work line 1), then a 1-line litany prayer.
        // source_index indexes work_lines directly (1:1, every work line present).
        let file_lines = vec![
            "## THE PREFACE.".to_string(),                    // work 0 (heading)
            "O GOD merciful father, oppress us.".to_string(), // work 1, sentence 1
            "And graciously hear us, our Lord.".to_string(),  // work 1, sentence 2
            "Lord have mercy upon us.".to_string(),           // work 2, single sentence
        ];
        let source_index = vec![0, 1, 1, 2];

        let work_lines = vec![
            make_line(1, "## THE PREFACE.", "the preface", false),
            make_line(
                2,
                "O GOD merciful father, oppress us. And graciously hear us, our Lord.",
                "o god merciful father oppress us and graciously hear us our lord",
                true,
            ),
            make_line(3, "Lord have mercy upon us.", "lord have mercy upon us", true),
        ];

        let map = build_line_map_bcp(&file_lines, &source_index, &work_lines);

        // Heading line maps to its own work row (identity preserved).
        assert_eq!(map.buffer_to_work[0], Some(0));
        // BOTH sentence sub-lines map to the same prayer work row (index 1).
        assert_eq!(map.buffer_to_work[1], Some(1));
        assert_eq!(map.buffer_to_work[2], Some(1));
        // The litany line maps to its own row.
        assert_eq!(map.buffer_to_work[3], Some(2));

        // work_to_buffer points at the FIRST sub-line of each work line.
        assert_eq!(map.work_to_buffer[0], 0);
        assert_eq!(map.work_to_buffer[1], 1, "prayer canonical = its first sentence line");
        assert_eq!(map.work_to_buffer[2], 3);

        // Dialogue lines: the two prayer sentences + litany; not the heading.
        assert!(!map.dialogue_buffer_lines.contains(&0));
        assert!(map.dialogue_buffer_lines.contains(&1));
        assert!(map.dialogue_buffer_lines.contains(&2));
        assert!(map.dialogue_buffer_lines.contains(&3));
    }

    // Helper for ParagraphAccumulate tests: builds a Line with explicit
    // (div1,div2,line_in_div) and `normalized` derived via the real normalize().
    // Named distinctly from the existing 4-arg `make_line` to avoid a signature
    // clash.
    fn make_acc_line(id: i64, text: &str, div1: i64, div2: i64, line_in_div: i64) -> Line {
        Line {
            id,
            citation: format!("T.{}.{}.{}", div1, div2, line_in_div),
            is_dialogue: true,
            text: text.to_string(),
            normalized: normalize(text),
            speaker: None,
            timestamp: None,
            div1,
            div2,
            line_in_div,
            sub_line: 0,
            is_chapter: false,
            is_spoken: None,
        }
    }

    #[test]
    fn test_build_line_map_accumulates_sentences_into_paragraph_row() {
        let work_lines = vec![
            make_acc_line(10, "Lord have mercy upon us. Christ have mercy upon us.", 0, 0, 1),
        ];
        let file_lines = vec![
            "Lord have mercy upon us.".to_string(),
            "Christ have mercy upon us.".to_string(),
        ];
        let map = build_line_map_mode(&file_lines, &work_lines, false, MatchMode::ParagraphAccumulate);
        assert_eq!(map.buffer_to_work, vec![Some(0), Some(0)]);
        assert_eq!(map.work_to_buffer[0], 0);
    }

    #[test]
    fn test_build_line_map_accumulate_skips_chrome_between_rows() {
        let work_lines = vec![
            make_acc_line(10, "First prayer here.", 0, 0, 1),
            make_acc_line(11, "Second prayer here.", 0, 0, 2),
        ];
        let file_lines = vec![
            "First prayer here.".to_string(),
            "        A Centered Head".to_string(),
            "Second prayer here.".to_string(),
        ];
        let map = build_line_map_mode(&file_lines, &work_lines, false, MatchMode::ParagraphAccumulate);
        assert_eq!(map.buffer_to_work, vec![Some(0), None, Some(1)]);
    }

    #[test]
    fn test_build_line_map_accumulate_merged_head_covers_two_rows() {
        // lit.db keeps a split title as TWO rows; the TEI <head> merges them into ONE
        // .txt line. The merged line maps to the first row; the matcher advances past
        // BOTH so the following prayer maps. work_to_buffer for both title rows = the
        // merged buffer line.
        let work_lines = vec![
            make_acc_line(10, "The Order for Morning Prayer,", 1, 0, 1),
            make_acc_line(11, "Daily Throughout the Year.", 1, 0, 2),
            make_acc_line(12, "O Lord, open thou our lips.", 1, 0, 3),
        ];
        let file_lines = vec![
            "      The Order for Morning Prayer, Daily Throughout the Year".to_string(),
            "O Lord, open thou our lips.".to_string(),
        ];
        let map = build_line_map_mode(&file_lines, &work_lines, false, MatchMode::ParagraphAccumulate);
        assert_eq!(map.buffer_to_work, vec![Some(0), Some(2)]);
        assert_eq!(map.work_to_buffer[0], 0);
        assert_eq!(map.work_to_buffer[1], 0);
        assert_eq!(map.work_to_buffer[2], 1);
    }

    #[test]
    fn test_build_line_map_accumulate_skips_unmatchable_head_row() {
        // The Benedictus stall shape: lit.db keeps a scripture-reference head row
        // ("St. Luke i. 68.") that the .txt merges into the canticle title line
        // ("Benedictus. St. Luke i. 68.") — so the reference row matches NO buffer
        // line. Without find_skip_target the accumulator parks on that row forever
        // and every following verse desyncs. The matcher must skip the unmatchable
        // head and resume on the next verse.
        let work_lines = vec![
            make_acc_line(10, "St. Luke i. 68.", 4, 0, 1),       // head, no buffer line
            make_acc_line(11, "Blessed be the Lord God of Israel.", 4, 0, 2),
            make_acc_line(12, "And hath raised up a mighty salvation.", 4, 0, 3),
        ];
        let file_lines = vec![
            "                Benedictus. St. Luke i. 68.".to_string(), // merged head (chrome)
            "Blessed be the Lord God of Israel.".to_string(),
            "And hath raised up a mighty salvation.".to_string(),
        ];
        let map = build_line_map_mode(&file_lines, &work_lines, false, MatchMode::ParagraphAccumulate);
        // The merged head line is genuine chrome (matches neither the head row nor
        // a later verse), so it stays None — but the verses still map because the
        // matcher skipped the unmatchable head row 0.
        assert_eq!(map.buffer_to_work, vec![None, Some(1), Some(2)]);
        assert_eq!(map.work_to_buffer[1], 1);
        assert_eq!(map.work_to_buffer[2], 2);
    }

    #[test]
    fn test_build_line_map_accumulate_mid_run_abandons_partial_then_resyncs() {
        // A partial run that fails to complete must be abandoned (its lines stay
        // None) and THIS line retried fresh. Here the first buffer line is a
        // prefix of row 0's text, so a run starts — but the second buffer line is
        // unrelated chrome that neither completes row 0 nor continues it. The run
        // is abandoned; the chrome is then matched fresh against row 0... which it
        // is not, so it stays None and wi holds. The third line completes row 0's
        // real text on its own; the fourth maps row 1.
        let work_lines = vec![
            make_acc_line(10, "Glory be to the Father.", 0, 0, 1),
            make_acc_line(11, "As it was in the beginning.", 0, 0, 2),
        ];
        let file_lines = vec![
            "Glory be to".to_string(),            // 0 prefix of row 0 -> starts a run
            "        A Rubric Interrupts".to_string(), // 1 chrome -> abandon run, stays None
            "Glory be to the Father.".to_string(),// 2 completes row 0 fresh
            "As it was in the beginning.".to_string(), // 3 row 1
        ];
        let map = build_line_map_mode(&file_lines, &work_lines, false, MatchMode::ParagraphAccumulate);
        // Abandoned partial-run line (0) and the interrupting chrome (1) stay None;
        // the matcher resyncs and maps row 0 at line 2, row 1 at line 3.
        assert_eq!(map.buffer_to_work, vec![None, None, Some(0), Some(1)]);
        assert_eq!(map.work_to_buffer[0], 2);
        assert_eq!(map.work_to_buffer[1], 3);
    }

    #[test]
    fn test_match_mode_for_work_picks_accumulate_for_bcp_textfile() {
        assert_eq!(match_mode_for_work("BCP1662", true), MatchMode::ParagraphAccumulate);
        assert_eq!(match_mode_for_work("Ham", true), MatchMode::WholeLine);   // play with .txt stays whole-line
        assert_eq!(match_mode_for_work("BCP1662", false), MatchMode::WholeLine); // no text_file -> whole-line
    }

    #[test]
    #[ignore] // needs ~/utono/litdb/data/lit.db + the regenerated .txt on disk
    fn bcp1662_accumulate_maps_most_rows() {
        let conn = crate::db::queries::open_db().expect("open lit.db");
        let work = crate::db::queries::load_work(&conn, "BCP1662").expect("load BCP1662");
        let path = work.text_file.clone().expect("BCP1662 has a text_file");
        let contents = std::fs::read_to_string(&path).expect("read .txt");
        let file_lines: Vec<String> = contents.lines().map(String::from).collect();
        let map = build_line_map_mode(&file_lines, &work.lines, false, MatchMode::ParagraphAccumulate);
        let matched = (0..work.lines.len())
            .filter(|wi| map.buffer_to_work.iter().any(|o| *o == Some(*wi)))
            .count();
        let pct = 100.0 * matched as f64 / work.lines.len() as f64;
        eprintln!("BCP1662 accumulate: {}/{} rows matched ({:.1}%)", matched, work.lines.len(), pct);
        assert!(pct >= 95.0, "only {:.1}% matched (want >= 95%)", pct);
    }

    #[test]
    fn stage_lines_map_to_their_db_rows() {
        // .txt has a spoken line, a multi-line stage direction, another stage line,
        // then a spoken line — mirroring 2H6 1.4.43.
        let file_lines: Vec<String> = vec![
            "Lay hands upon these traitors and their trash.".into(),
            "[The Guard arrest Margery Jourdain and her".into(),
            "accomplices and seize their papers.]".into(),
            "[To Jourdain.]".into(),
            "Beldam, I think we watched you at an".into(),
        ];
        // DB rows in (line_in_div, sub_line) order. sub_line>0 are stage rows.
        let mk = |id: i64, text: &str, sub: i64, dialogue: bool| crate::db::models::Line {
            id, citation: String::new(), text: text.into(),
            normalized: super::normalize(text), speaker: None,
            is_dialogue: dialogue, timestamp: None, div1: 1, div2: 4,
            line_in_div: if id < 4 { 43 } else { 44 }, sub_line: sub,
            is_chapter: false, is_spoken: None,
        };
        let work_lines = vec![
            mk(1, "Lay hands upon these traitors and their trash.", 0, true),
            mk(2, "[The Guard arrest Margery Jourdain and her", 1, false),
            mk(3, "accomplices and seize their papers.]", 2, false),
            mk(3, "[To Jourdain.]", 3, false), // id reused only for line_in_div branch; fine for test
            mk(4, "Beldam, I think we watched you at an", 0, true),
        ];
        let lm = super::build_line_map(&file_lines, &work_lines, false);
        // Every buffer line, including the three stage lines, maps to a work row.
        assert_eq!(lm.buffer_to_work[0], Some(0), "spoken line maps");
        assert_eq!(lm.buffer_to_work[1], Some(1), "stage line 1 (multi-line open) maps");
        assert_eq!(lm.buffer_to_work[2], Some(2), "stage line 2 (multi-line close) maps");
        assert_eq!(lm.buffer_to_work[3], Some(3), "[To Jourdain.] maps");
        assert_eq!(lm.buffer_to_work[4], Some(4), "next spoken line maps");
    }

    #[test]
    fn test_build_line_map_bcp_repeated_litany_and_rubric() {
        // Repeated identical litany lines must map to distinct DB rows in order
        // (the matcher advances its cursor); a rubric line maps too; a
        // 2-sentence prayer splits across two buffer lines -> one row.
        let file_lines = vec![
            "Lord have mercy upon us.".to_string(),     // row 0
            "Christ have mercy upon us.".to_string(),   // row 1
            "Lord have mercy upon us.".to_string(),     // row 2 (repeat)
            "[Then shall the Priest say.]".to_string(), // row 3 (rubric)
            "O GOD our refuge, defend us.".to_string(), // row 4 sentence 1
            "Grant us peace, our Lord.".to_string(),    // row 4 sentence 2
        ];
        let source_index = vec![0, 1, 2, 3, 4, 4];
        let work_lines = vec![
            make_line(1, "Lord have mercy upon us.", "lord have mercy upon us", true),
            make_line(2, "Christ have mercy upon us.", "christ have mercy upon us", true),
            make_line(3, "Lord have mercy upon us.", "lord have mercy upon us", true),
            make_line(4, "[Then shall the Priest say.]", "then shall the priest say", false),
            make_line(
                5,
                "O GOD our refuge, defend us. Grant us peace, our Lord.",
                "o god our refuge defend us grant us peace our lord",
                true,
            ),
        ];

        let map = build_line_map_bcp(&file_lines, &source_index, &work_lines);

        assert_eq!(map.buffer_to_work[0], Some(0));
        assert_eq!(map.buffer_to_work[1], Some(1));
        assert_eq!(map.buffer_to_work[2], Some(2), "repeat maps to the 2nd matching row, not the 1st");
        assert_eq!(map.buffer_to_work[3], Some(3));
        assert_eq!(map.buffer_to_work[4], Some(4));
        assert_eq!(map.buffer_to_work[5], Some(4), "prayer's 2nd sentence shares its row");
        assert_eq!(map.work_to_buffer[4], 4, "prayer canonical = first sentence line");
    }

    /// Regression for the folded-SD coloring bug: build the map through the SAME
    /// clean_file_lines fold the app uses (NOT raw .txt — that was the misleading
    /// repro), and assert the folded `[The Guard arrest...]` SD in 2H6-Amb 1.4 maps
    /// to (1,4,43) instead of staying UNMAPPED. Skipped when lit.db is unavailable.
    #[test]
    fn h6_amb_folded_guard_sd_maps_through_clean_path() {
        let conn = match crate::db::queries::open_db() {
            Ok(c) => c,
            Err(_) => { eprintln!("skip: no lit.db"); return; }
        };
        let work = match crate::db::queries::load_work(&conn, "2H6-Amb") {
            Ok(w) => w,
            Err(_) => { eprintln!("skip: 2H6-Amb not loaded"); return; }
        };
        let prepared = match crate::app::text_prep::prepare_text_only(&work) {
            Some(p) => p,
            None => { eprintln!("skip: no text_file"); return; }
        };
        let is_prose = crate::db::line_types::is_prose_work(&work.work_type);
        let lm = build_line_map(&prepared.cleaned_lines, &work.lines, is_prose);

        // The folded SD renders as one cleaned buffer line containing "Guard arrest".
        let sd_buf = prepared.cleaned_lines.iter()
            .position(|l| l.contains("Guard arrest"));
        let sd_buf = match sd_buf {
            Some(b) => b,
            None => { eprintln!("skip: SD not in cleaned text"); return; }
        };
        let wi = lm.buffer_to_work[sd_buf];
        assert!(wi.is_some(),
            "folded SD buffer line {sd_buf} must map (was the bug: UNMAPPED -> uncolored)");
        let l = &work.lines[wi.unwrap()];
        assert_eq!((l.div1, l.div2, l.line_in_div), (1, 4, 43),
            "folded SD must map to citation 1.4.43");
    }
}

#[cfg(test)]
mod mark_chapter_starts_tests {
    use super::mark_chapter_starts;
    use crate::db::models::Line;

    /// Minimal Line with only div1 set; everything else defaulted.
    fn line(div1: i64) -> Line {
        Line {
            id: 0,
            citation: String::new(),
            text: String::new(),
            normalized: String::new(),
            speaker: None,
            is_dialogue: false,
            timestamp: None,
            div1,
            div2: 0,
            line_in_div: 0,
            sub_line: 0,
            is_chapter: false,
            is_spoken: None,
        }
    }

    fn flags(lines: &[Line]) -> Vec<bool> {
        lines.iter().map(|l| l.is_chapter).collect()
    }

    #[test]
    fn prose_skips_front_matter_marks_each_div1() {
        // div1: 0,0,1,1,2,2 -> chapter at first 1 and first 2, NOT front matter.
        let mut lines = vec![line(0), line(0), line(1), line(1), line(2), line(2)];
        mark_chapter_starts(&mut lines, true);
        assert_eq!(flags(&lines), vec![false, false, true, false, true, false]);
    }

    #[test]
    fn play_marks_each_div1_change_including_first() {
        // div1: 1,1,2,2 -> chapter at first 1 and first 2 (non-prose: any change,
        // and the first mapped line is a change from "no previous").
        let mut lines = vec![line(1), line(1), line(2), line(2)];
        mark_chapter_starts(&mut lines, false);
        assert_eq!(flags(&lines), vec![true, false, true, false]);
    }

    #[test]
    fn prose_first_div1_is_one_marks_first_line() {
        // No front matter: div1 1,1,2 -> first 1 is a chapter.
        let mut lines = vec![line(1), line(1), line(2)];
        mark_chapter_starts(&mut lines, true);
        assert_eq!(flags(&lines), vec![true, false, true]);
    }

    #[test]
    fn empty_input_is_noop() {
        let mut lines: Vec<Line> = vec![];
        mark_chapter_starts(&mut lines, true);
        assert!(lines.is_empty());
    }

    #[test]
    fn single_div1_zero_prose_no_chapter() {
        let mut lines = vec![line(0)];
        mark_chapter_starts(&mut lines, true);
        assert_eq!(flags(&lines), vec![false]);
    }

    #[test]
    fn single_div1_zero_nonprose_is_chapter() {
        // Non-prose treats the first mapped line as a div1 boundary regardless of value.
        let mut lines = vec![line(0)];
        mark_chapter_starts(&mut lines, false);
        assert_eq!(flags(&lines), vec![true]);
    }

    #[test]
    fn clears_stale_flags_idempotent() {
        // Reload path may call on lines with stale true flags; helper must reset.
        let mut lines = vec![line(0), line(1)];
        lines[0].is_chapter = true; // stale
        mark_chapter_starts(&mut lines, true);
        assert_eq!(flags(&lines), vec![false, true]);
    }
}
