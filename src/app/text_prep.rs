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
        // Stage directions normalize to empty in the line map, so folding them
        // doesn't disturb work-line mapping.
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
