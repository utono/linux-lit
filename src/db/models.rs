/// Format the canonical citation address `abbrev.div1.div2.line_in_div`.
/// The single source of truth for how a line citation string is built.
pub fn citation(abbrev: &str, div1: i64, div2: i64, line_in_div: i64) -> String {
    format!("{}.{}.{}.{}", abbrev, div1, div2, line_in_div)
}

/// Parse a citation address back to `(div1, div2, line_in_div)`. The abbrev
/// prefix (including `-Amb`-style suffixes) is irrelevant — only the trailing
/// three numbers matter. Inverse of [`citation`].
pub fn parse_citation(cite: &str) -> Option<(i64, i64, i64)> {
    let mut parts = cite.rsplitn(4, '.');
    let line = parts.next()?.parse().ok()?;
    let div2 = parts.next()?.parse().ok()?;
    let div1 = parts.next()?.parse().ok()?;
    Some((div1, div2, line))
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Work {
    pub abbrev: String,
    /// The abbrev under which this work's shared artifacts (glosses, journal
    /// Q&A, scene synopses) are stored and looked up. For a variant edition
    /// (`Cym-Amb`, `Cym-BBC`) this is the base work (`Cym`), so artifacts are
    /// shared across all editions; for everything else it equals `abbrev`.
    /// Resolved by `db::queries::canonical_work_abbrev` at load.
    pub canonical_abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
    pub text_file: Option<String>,
    pub vocab_highlight: bool,
    pub lines: Vec<Line>,
    pub timestamps: Vec<Timestamp>,
    pub media_paths: Vec<String>,
    /// Parallel to media_paths: the media_id for each path.
    pub media_ids: Vec<i64>,
    pub media_id: Option<i64>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Line {
    pub id: i64,
    pub citation: String,
    pub text: String,
    pub normalized: String,
    pub speaker: Option<String>,
    pub is_dialogue: bool,
    pub timestamp: Option<TimeRange>,
    pub div1: i64,
    pub div2: i64,
    pub line_in_div: i64,
    /// Sub-line within a spoken line: 0 = the spoken line itself; 1..N = stage
    /// directions sharing that line's `line_in_div` (document order). Stage rows
    /// have `speaker=None` and `is_dialogue=false`.
    pub sub_line: i64,
    /// Whether this line is a chapter marker.
    pub is_chapter: bool,
    /// Whether this line is spoken in the active media file.
    /// None = no spoken-status data (treat as spoken). Some(false) = skip on dialogue nav.
    pub is_spoken: Option<bool>,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct TimeRange {
    pub start: f64,
    pub end: f64,
    pub sentence_start: Option<f64>,
    pub is_manual: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Timestamp {
    pub line_id: i64,
    pub start: f64,
    pub end: f64,
    pub media_id: i64,
    pub sentence_start: Option<f64>,
    pub is_manual: bool,
    pub is_track_mark: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Chunk {
    pub id: i64,
    pub a_line: i64,    // line_in_div of first line
    pub b_line: i64,    // line_in_div of last line (inclusive)
    pub a_time: Option<f64>,
    pub b_time: Option<f64>,
    pub div1: i64,
    pub div2: Option<i64>,
}

/// A page-scan image (one leaf) and the canonical `line_mapping` rows that fall
/// on it. `start_line_id`/`end_line_id` reference `line_mapping(id)` inclusively;
/// they are `None` until the in-app calibration marks the page's range. The
/// reader shows `image_path` (resolved against `works.image_dir`) for the page
/// whose [start, end] range contains the cursor line.
#[derive(Debug, Clone)]
pub struct PageImage {
    pub image_path: String,
    pub page_order: i64,
    pub start_line_id: Option<i64>,
    pub end_line_id: Option<i64>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WorkSummary {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
}

#[derive(Debug, Clone)]
pub struct MediaItem {
    pub media_id: i64,
    pub path: String,
    pub display_name: Option<String>,
    pub priority: i64,
}

#[derive(Debug, Clone)]
pub struct BookmarkItem {
    pub line_mapping_id: i64,
    pub line_text: String,
    pub speaker: String,
    pub citation: String,
}
