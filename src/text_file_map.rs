use crate::db::models::Line;

/// Bidirectional map between a plain-text file's line indices and DB work line indices.
#[derive(Debug, Clone)]
pub struct LineMap {
    /// For each buffer line index, the DB work_lines index it maps to (None if unmatched).
    pub buffer_to_work: Vec<Option<usize>>,
    /// For each DB work_lines index, the buffer line index it maps to.
    pub work_to_buffer: Vec<usize>,
    /// Buffer line indices that contain dialogue (matched DB lines where is_dialogue is true).
    pub dialogue_buffer_lines: Vec<usize>,
}

/// Normalize a line of text to match the DB's `normalized_text` column:
/// trim, lowercase, strip non-alphanumeric chars (keep spaces), collapse whitespace.
pub fn normalize(s: &str) -> String {
    let lowered = s.trim().to_lowercase();
    let filtered: String = lowered
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .collect();
    // Collapse runs of whitespace to a single space and trim again
    filtered
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

const WINDOW: usize = 50;

/// Build a bidirectional map between `file_lines` (raw lines from a plain-text file)
/// and `work_lines` (DB rows for a work).
///
/// Algorithm (ported from Lua):
/// - Pre-normalize all file lines.
/// - For each non-empty normalized file line, scan forward in work_lines within a
///   sliding window of `WINDOW` rows starting at the current DB cursor position.
/// - On a match beyond the current cursor: verify that the next non-empty file line
///   also matches the next DB row. If the confirmation check fails, skip this candidate.
/// - Log match percentage; warn if < 80%.
pub fn build_line_map(file_lines: &[String], work_lines: &[Line]) -> LineMap {
    let norm_file: Vec<String> = file_lines.iter().map(|l| normalize(l)).collect();

    let n_buf = file_lines.len();
    let n_work = work_lines.len();

    let mut buffer_to_work: Vec<Option<usize>> = vec![None; n_buf];
    // Default: each work line maps to buffer line 0 (will be overwritten for matched lines)
    let mut work_to_buffer: Vec<usize> = vec![0; n_work];

    let mut db_cursor: usize = 0; // current position in work_lines
    let mut matched: usize = 0;

    for buf_idx in 0..n_buf {
        let nf = &norm_file[buf_idx];
        if nf.is_empty() {
            continue;
        }

        // Search forward in work_lines within the window
        let window_end = (db_cursor + WINDOW).min(n_work);
        let mut found: Option<usize> = None;

        'outer: for wi in db_cursor..window_end {
            let db_norm = &work_lines[wi].normalized;
            if db_norm.is_empty() {
                continue;
            }
            if db_norm == nf {
                // If this match is beyond the current cursor, do a confirmation check:
                // the next non-empty file line should match the next DB row after wi.
                if wi > db_cursor {
                    // Find the next non-empty file line
                    let mut next_buf: Option<(usize, &String)> = None;
                    for bi2 in (buf_idx + 1)..n_buf {
                        if !norm_file[bi2].is_empty() {
                            next_buf = Some((bi2, &norm_file[bi2]));
                            break;
                        }
                    }
                    // Find the next non-empty DB row after wi
                    let mut next_db: Option<(usize, &String)> = None;
                    for wi2 in (wi + 1)..n_work {
                        if !work_lines[wi2].normalized.is_empty() {
                            next_db = Some((wi2, &work_lines[wi2].normalized));
                            break;
                        }
                    }
                    if let (Some((_bi2, nb)), Some((_wi2, nd))) = (next_buf, next_db) {
                        if nb != nd {
                            // Confirmation failed — skip this candidate
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

    // Collect dialogue buffer lines
    let mut dialogue_buffer_lines: Vec<usize> = Vec::new();
    for (buf_idx, work_idx_opt) in buffer_to_work.iter().enumerate() {
        if let Some(wi) = work_idx_opt {
            if work_lines[*wi].is_dialogue {
                dialogue_buffer_lines.push(buf_idx);
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

    LineMap {
        buffer_to_work,
        work_to_buffer,
        dialogue_buffer_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::Line;

    fn make_line(id: i64, text: &str, normalized: &str, is_dialogue: bool) -> Line {
        Line {
            id,
            citation: String::new(),
            text: text.to_string(),
            normalized: normalized.to_string(),
            speaker: None,
            is_dialogue,
            timestamp: None,
            div1: 1,
            div2: 1,
            line_in_div: id,
        }
    }

    #[test]
    fn test_normalize() {
        assert_eq!(normalize("Who's there?"), "whos there");
        assert_eq!(normalize("Long live the King!"), "long live the king");
        assert_eq!(normalize("  Hello,  World!  "), "hello world");
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("He."), "he");
        assert_eq!(normalize("A"), "a");
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

        let map = build_line_map(&file_lines, &work_lines);

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

        // Both lines are dialogue
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

        let map = build_line_map(&file_lines, &work_lines);

        // "First line" at buf 0 should match work row 0 — db_cursor advances to 1
        assert_eq!(map.buffer_to_work[0], Some(0));

        // "He." at buf 1: db_cursor is 1, candidate is wi=1 (at cursor, not beyond),
        // so no confirmation needed — it should match directly.
        assert_eq!(map.buffer_to_work[1], Some(1));

        // "Something else entirely" at buf 2 does not match "different content" — no match.
        assert_eq!(map.buffer_to_work[2], None);
    }
}
