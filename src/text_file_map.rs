use std::ops::Range;

use crate::db::models::Line;
use crate::db::line_types;

/// A sentence group with character-level boundary info for partial-line highlighting.
#[derive(Debug, Clone, PartialEq)]
pub struct SentenceGroup {
    /// Buffer line indices covered by this sentence.
    pub line_range: Range<usize>,
    /// Character offset on the first line where the sentence begins (0 = start of line).
    pub start_col: usize,
    /// Character offset on the last line where the sentence ends (None = end of line).
    pub end_col: Option<usize>,
}

/// Bidirectional map between a plain-text file's line indices and DB work line indices.
#[derive(Debug, Clone)]
pub struct LineMap {
    /// For each buffer line index, the DB work_lines index it maps to (None if unmatched).
    pub buffer_to_work: Vec<Option<usize>>,
    /// For each DB work_lines index, the buffer line index it maps to.
    pub work_to_buffer: Vec<usize>,
    /// Buffer line indices that contain dialogue (matched or unmatched).
    pub dialogue_buffer_lines: Vec<usize>,
    /// Dialogue buffer lines filtered to only spoken lines (for media-aware navigation).
    /// Excludes lines where `is_spoken == Some(false)`.
    #[allow(dead_code)]
    pub spoken_dialogue_buffer_lines: Vec<usize>,
    /// Contiguous ranges of buffer lines forming sentences (prose text_file works only).
    pub sentence_groups: Vec<SentenceGroup>,
}

/// Normalize a line of text to match the DB's `normalized_text` column:
/// strip bracketed stage directions, trim, lowercase, strip non-alphanumeric
/// chars (keep spaces), collapse whitespace.
pub fn normalize(s: &str) -> String {
    // Remove bracketed stage directions like [To Fool.] that appear in text
    // files but not in DB canonical_text
    let without_brackets = strip_brackets(s);
    let lowered = without_brackets.trim().to_lowercase();
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

/// Remove all `[...]` bracketed text from a string.
fn strip_brackets(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut depth = 0usize;
    for ch in s.chars() {
        match ch {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => result.push(ch),
            _ => {}
        }
    }
    result
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
pub fn build_line_map(file_lines: &[String], work_lines: &[Line], is_prose: bool) -> LineMap {
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
                if line_types::is_dialogue(&file_lines[buf_idx], is_prose) {
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

    // Spoken dialogue lines: exclude lines explicitly marked as not spoken
    let spoken_dialogue_buffer_lines: Vec<usize> = dialogue_buffer_lines
        .iter()
        .filter(|&&buf_idx| {
            match buffer_to_work[buf_idx] {
                Some(wi) => work_lines[wi].is_spoken != Some(false),
                None => true, // unmatched lines: keep (no spoken data)
            }
        })
        .copied()
        .collect();

    // Sentence groups: contiguous buffer-line ranges forming sentences (prose only)
    let sentence_groups = if is_prose {
        let mut groups = build_sentence_groups_from_db(&buffer_to_work, work_lines)
            .unwrap_or_else(|| build_sentence_groups(file_lines));
        apply_mid_line_offsets(&mut groups, file_lines);
        groups
    } else {
        Vec::new()
    };

    LineMap {
        buffer_to_work,
        work_to_buffer,
        dialogue_buffer_lines,
        spoken_dialogue_buffer_lines,
        sentence_groups,
    }
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
            is_chapter: false,
            is_spoken: None,
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
}
