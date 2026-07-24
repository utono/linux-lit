use crate::db::models::Work;

/// First phase of preparing a work for display: file read + cleanup. Fast
/// (~50ms on Bleak House), produced off the GTK main thread. Lets us call
/// `state.buffer.set_text(filtered_contents)` and reveal the window
/// quickly without waiting for the slower line_map build.
#[derive(Clone)]
pub struct PreparedTextOnly {
    pub abbrev: String,
    pub work_type: String,
    pub file_lines_count: usize,
    pub cleaned_lines_count: usize,
    pub work_lines_count: usize,
    pub filtered_contents: String,
    pub cleaned_lines: Vec<String>,
    pub path: String,
    pub is_prose: bool,
}

/// Full prepared text including the line_map. Used by paths that want a
/// single-shot prep (no two-phase). Produced by
/// `prepare_text_for_display`.
#[derive(Clone)]
pub struct PreparedText {
    pub abbrev: String,
    pub work_type: String,
    pub file_lines_count: usize,
    pub cleaned_lines_count: usize,
    pub work_lines_count: usize,
    pub filtered_contents: String,
    pub line_map: crate::text_file_map::LineMap,
    pub path: String,
    pub is_prose: bool,
}

/// Result of spawn_blocking 1 in build_window's MRU path: either a fresh
/// PreparedTextOnly (cache miss, will require build_line_map in spawn_blocking 2)
/// or a fully-restored WorkSnapshot (cache hit, skip phase 2 entirely).
pub(crate) enum SnapshotOrPrep {
    Snapshot(crate::snapshot::WorkSnapshot),
    Prep(Option<PreparedTextOnly>),
}

/// Heavy precompute: read the work's text file from disk, clean it,
/// build the line map. Pure CPU + I/O — safe to run inside
/// `tokio::Handle::spawn_blocking`. The caller then calls
/// `display_work_with_prepared` on the GTK main thread to apply the
/// result via `state.buffer.set_text(...)`.
///
/// Returns None when the work has no text_file or the file read failed —
/// caller falls back to the default `display_work` path that joins
/// `work.lines` synchronously.
/// Phase 1: read file + clean. Cheap (~50ms on Bleak House). Off-thread
/// safe. Pair with `build_line_map_for_prepared` to get the full
/// `PreparedText`, or use directly via `display_work_text_only` to show
/// content immediately while the line_map builds in the background.
/// Clean raw source `.txt` lines for display: drop blank lines that precede a
/// speaker name, strip the `## ` markdown act/scene prefix, and fold multi-line
/// stage directions into a single line so they soft-wrap instead of keeping the
/// Folger source's mid-sentence hard breaks. Shared by `prepare_text_only` and
/// `prepare_text_for_display` so both produce identical buffer text.
fn clean_file_lines(file_lines: &[String]) -> Vec<String> {
    let mut result: Vec<String> = Vec::with_capacity(file_lines.len());
    let mut i = 0;
    while i < file_lines.len() {
        let line = &file_lines[i];
        if crate::db::line_types::is_blank(line) {
            let next_non_blank = file_lines[i + 1..]
                .iter()
                .find(|l| !crate::db::line_types::is_blank(l));
            if let Some(next) = next_non_blank {
                if crate::db::line_types::is_speaker(next) {
                    i += 1;
                    continue;
                }
            }
        }

        // Multi-line stage direction: the Folger source hard-wraps a single
        // bracketed direction across several lines (opens with `[`, no closing
        // `]`). Fold those source lines into one buffer line so GTK soft-wraps
        // the direction naturally instead of preserving the mid-sentence breaks.
        // The folded line no longer matches a single DB stage row 1:1, so
        // build_line_map's stage matcher re-joins consecutive sub_line>0 rows to
        // map it (see src/text_file_map.rs); keep the single-space join here in
        // sync with that matcher's join.
        let trimmed = line.trim();
        if trimmed.starts_with('[') && !trimmed.ends_with(']') {
            let mut joined = line.clone();
            let mut j = i + 1;
            let mut closed = false;
            while j < file_lines.len() {
                let cont = file_lines[j].trim();
                if cont.is_empty() {
                    break; // malformed (no closing bracket before a blank) — stop
                }
                joined.push(' ');
                joined.push_str(cont);
                let ends_here = cont.ends_with(']');
                j += 1;
                if ends_here {
                    closed = true;
                    break;
                }
            }
            if closed {
                result.push(joined);
                i = j;
                continue;
            }
        }

        if let Some(stripped) = line.strip_prefix("## ") {
            result.push(stripped.to_string());
        } else if crate::db::line_types::is_bcp_speaker(line) {
            // Strip the `@ ` BCP speaker-cue marker for display; apply_bcp_formatting
            // re-derives cue-ness from the work-line (which keeps the marker) and
            // styles the bare cue centered-italic.
            result.push(crate::db::line_types::strip_bcp_speaker_marker(line).to_string());
        } else {
            result.push(line.clone());
        }
        i += 1;
    }
    result
}

pub fn prepare_text_only(work: &Work) -> Option<PreparedTextOnly> {
    let path = work.text_file.as_ref()?;
    let t_read = std::time::Instant::now();
    let contents = std::fs::read_to_string(path).ok()?;
    let file_lines: Vec<String> = contents.lines().map(String::from).collect();
    crate::logging::log(&format!("PREP: read+split {}ms", t_read.elapsed().as_millis()));

    let t_clean = std::time::Instant::now();
    let cleaned_lines = clean_file_lines(&file_lines);
    crate::logging::log(&format!("PREP: clean {}ms ({} -> {} lines)", t_clean.elapsed().as_millis(), file_lines.len(), cleaned_lines.len()));

    let is_prose = crate::db::line_types::is_prose_work(&work.work_type);
    let t_join = std::time::Instant::now();
    let filtered_contents = cleaned_lines.join("\n");
    crate::logging::log(&format!("PREP: join {}ms", t_join.elapsed().as_millis()));

    Some(PreparedTextOnly {
        abbrev: work.abbrev.clone(),
        work_type: work.work_type.clone(),
        file_lines_count: file_lines.len(),
        cleaned_lines_count: cleaned_lines.len(),
        work_lines_count: work.lines.len(),
        filtered_contents,
        cleaned_lines,
        path: path.clone(),
        is_prose,
    })
}

/// Phase 2: build_line_map from already-cleaned lines. Slow (~1000ms on
/// Bleak House). Off-thread safe. Used after `prepare_text_only` +
/// `display_work_text_only` to complete navigation setup.
pub fn build_line_map_for_prepared(
    cleaned_lines: &[String],
    work_lines: &[crate::db::models::Line],
    is_prose: bool,
    abbrev: &str,
    has_text_file: bool,
) -> crate::text_file_map::LineMap {
    let t_map = std::time::Instant::now();
    let mode = crate::text_file_map::match_mode_for_work(abbrev, has_text_file);
    let line_map =
        crate::text_file_map::build_line_map_mode(cleaned_lines, work_lines, is_prose, mode);
    crate::logging::log(&format!("PREP: build_line_map {}ms", t_map.elapsed().as_millis()));
    line_map
}

pub fn prepare_text_for_display(work: &Work) -> Option<PreparedText> {
    let path = work.text_file.as_ref()?;
    let t_read = std::time::Instant::now();
    let contents = std::fs::read_to_string(path).ok()?;
    let file_lines: Vec<String> = contents.lines().map(String::from).collect();
    crate::logging::log(&format!("PREP: read+split {}ms", t_read.elapsed().as_millis()));

    let t_clean = std::time::Instant::now();
    let cleaned_lines = clean_file_lines(&file_lines);
    crate::logging::log(&format!("PREP: clean {}ms ({} -> {} lines)", t_clean.elapsed().as_millis(), file_lines.len(), cleaned_lines.len()));

    let is_prose = crate::db::line_types::is_prose_work(&work.work_type);
    let t_map = std::time::Instant::now();
    let mode = crate::text_file_map::match_mode_for_work(&work.abbrev, work.text_file.is_some());
    let line_map = crate::text_file_map::build_line_map_mode(&cleaned_lines, &work.lines, is_prose, mode);
    crate::logging::log(&format!("PREP: build_line_map {}ms", t_map.elapsed().as_millis()));

    let t_join = std::time::Instant::now();
    let filtered_contents = cleaned_lines.join("\n");
    crate::logging::log(&format!("PREP: join {}ms", t_join.elapsed().as_millis()));

    Some(PreparedText {
        abbrev: work.abbrev.clone(),
        work_type: work.work_type.clone(),
        file_lines_count: file_lines.len(),
        cleaned_lines_count: cleaned_lines.len(),
        work_lines_count: work.lines.len(),
        filtered_contents,
        line_map,
        path: path.clone(),
        is_prose,
    })
}

/// Buffer lines + mapping + per-line indent tier for a block-aware work.
pub struct BlockBuffer {
    pub buf_lines: Vec<String>,
    pub source_index: Vec<usize>,
    pub indent_tiers: Vec<u8>,
}

/// Leading-space count -> indent tier (0/1/2). 0 spaces = tier 0, 1-2 = tier 1,
/// 3+ = tier 2. Matches the producer's 2-space `&nbsp;&nbsp;` tiers with slack.
/// Returns (tier, leading_space_byte_count) — spaces are ASCII so byte==char.
fn leading_space_tier(line: &str) -> (u8, usize) {
    let n = line.chars().take_while(|c| *c == ' ').count();
    let tier = if n == 0 { 0 } else if n <= 2 { 1 } else { 2 };
    (tier, n)
}

/// Splits verse rows on embedded `\n` into display lines, strips leading
/// spaces (recording an indent tier per line), and produces the
/// `source_index` that `build_line_map_blocks` (src/text_file_map.rs)
/// consumes to map buffer lines back to DB rows. Emits every work-line
/// index exactly once, in order — the non-decreasing/full-coverage
/// precondition `build_line_map_blocks` relies on.
pub fn prepare_block_buffer(work_lines: &[crate::db::models::Line]) -> BlockBuffer {
    let mut buf_lines = Vec::new();
    let mut source_index = Vec::new();
    let mut indent_tiers = Vec::new();
    for (wi, l) in work_lines.iter().enumerate() {
        if crate::db::line_types::is_verse_line(&l.block_type) {
            for vline in l.text.split('\n') {
                let (tier, n) = leading_space_tier(vline);
                buf_lines.push(vline[n..].to_string()); // strip leading spaces
                source_index.push(wi);
                indent_tiers.push(tier);
            }
        } else {
            buf_lines.push(l.text.clone());
            source_index.push(wi);
            indent_tiers.push(0);
        }
    }

    // Contract guarantee for build_line_map_blocks's consumer precondition:
    // every work-line index appears at least once, and source_index never
    // decreases. A violation here would silently misalign (or panic) the
    // consumer, so fail fast in debug builds.
    debug_assert_eq!(
        source_index.iter().collect::<std::collections::BTreeSet<_>>().len(),
        work_lines.len(),
        "prepare_block_buffer must emit every work-line index"
    );
    debug_assert!(
        source_index.windows(2).all(|w| w[0] <= w[1]),
        "source_index must be non-decreasing"
    );

    BlockBuffer { buf_lines, source_index, indent_tiers }
}

#[cfg(test)]
mod block_buffer_tests {
    use super::*;
    use crate::db::models::Line;

    fn mk(bt: &str, txt: &str) -> Line {
        Line {
            id: 0, citation: String::new(), text: txt.into(), normalized: String::new(),
            speaker: None, is_dialogue: false, timestamp: None, div1: 1, div2: 0,
            line_in_div: 1, sub_line: 0, is_chapter: false, is_spoken: None,
            block_type: bt.into(),
        }
    }

    #[test]
    fn prepare_block_buffer_splits_verse_and_tiers_indent() {
        let work = vec![
            mk("prose", "Ordinary prose."),
            mk("verse", "l1\n  l2\n    l3"),   // tiers 0, 1, 2
            mk("heading", "MELIBOEUS."),
        ];
        let b = prepare_block_buffer(&work);
        assert_eq!(b.buf_lines, vec![
            "Ordinary prose.", "l1", "l2", "l3", "MELIBOEUS.",
        ]);
        assert_eq!(b.source_index, vec![0, 1, 1, 1, 2]);
        assert_eq!(b.indent_tiers, vec![0, 0, 1, 2, 0]);
    }

    #[test]
    fn prepare_block_buffer_source_index_covers_every_row_non_decreasing() {
        // mix of prose + a multi-line verse + a heading
        let work = vec![
            mk("prose", "Opening prose."),
            mk("verse", "a\n  b\n    c\nd"),
            mk("heading", "SCENE I."),
            mk("prose", "Closing prose."),
        ];
        let b = prepare_block_buffer(&work);
        // non-decreasing
        assert!(b.source_index.windows(2).all(|w| w[0] <= w[1]));
        // distinct count == input row count (every work-line index emitted exactly once)
        let distinct: std::collections::BTreeSet<_> = b.source_index.iter().collect();
        assert_eq!(distinct.len(), work.len());
    }
}
