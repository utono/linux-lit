#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Work {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
    pub text_file: Option<String>,
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
    pub is_chapter: bool,
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
