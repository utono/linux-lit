#[derive(Debug, Clone)]
pub struct Work {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
    pub lines: Vec<Line>,
    pub timestamps: Vec<Timestamp>,
    pub media_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub id: i64,
    pub text: String,
    pub normalized: String,
    pub speaker: Option<String>,
    pub is_dialogue: bool,
    pub timestamp: Option<TimeRange>,
}

#[derive(Debug, Clone, Copy)]
pub struct TimeRange {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone)]
pub struct Timestamp {
    pub line_id: i64,
    pub start: f64,
    pub end: f64,
    pub media_id: i64,
}

#[derive(Debug, Clone)]
pub struct WorkSummary {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
}
