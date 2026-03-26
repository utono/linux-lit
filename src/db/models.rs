#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Work {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
    pub lines: Vec<Line>,
    pub timestamps: Vec<Timestamp>,
    pub media_paths: Vec<String>,
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
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct TimeRange {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Timestamp {
    pub line_id: i64,
    pub start: f64,
    pub end: f64,
    pub media_id: i64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WorkSummary {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
}
