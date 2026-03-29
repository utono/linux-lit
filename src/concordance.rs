/// A single occurrence of a word in a work's line_mapping.
#[derive(Debug, Clone)]
pub struct ConcordanceHit {
    pub work_abbrev: String,
    pub work_title: String,
    pub author: String,
    pub line_mapping_id: i64,
    pub div1: i64,
    pub div2: i64,
    pub line_in_div: i64,
    pub canonical_text: String,
    pub has_audio: bool,
}

/// Pre-loaded work data, ready to swap into AppState.
pub struct PreloadedWork {
    pub work_abbrev: String,
    pub work: crate::db::models::Work,
}

/// Cross-work concordance navigation state.
pub struct ConcordanceState {
    pub word: String,
    pub occurrences: Vec<ConcordanceHit>,
    pub current_index: usize,
    pub preloaded_work: Option<PreloadedWork>,
}

impl ConcordanceState {
    pub fn new(word: String, occurrences: Vec<ConcordanceHit>) -> Self {
        Self {
            word,
            occurrences,
            current_index: 0,
            preloaded_work: None,
        }
    }

    /// Work abbreviation of the next occurrence in a given direction.
    /// direction: 1 for forward, -1 for backward.
    pub fn next_work_abbrev(&self, direction: i32) -> Option<&str> {
        let next = if direction > 0 {
            if self.current_index + 1 < self.occurrences.len() {
                self.current_index + 1
            } else {
                return None;
            }
        } else if self.current_index > 0 {
            self.current_index - 1
        } else {
            return None;
        };
        self.occurrences.get(next).map(|h| h.work_abbrev.as_str())
    }

    /// Advance index forward. Returns false if already at the end.
    pub fn advance(&mut self) -> bool {
        if self.current_index + 1 < self.occurrences.len() {
            self.current_index += 1;
            true
        } else {
            false
        }
    }

    /// Move index backward. Returns false if already at the start.
    pub fn retreat(&mut self) -> bool {
        if self.current_index > 0 {
            self.current_index -= 1;
            true
        } else {
            false
        }
    }

    /// Current hit, if any.
    pub fn current_hit(&self) -> Option<&ConcordanceHit> {
        self.occurrences.get(self.current_index)
    }

    /// Format status bar text: "disapprobation [3/13]"
    pub fn status_label(&self) -> String {
        format!(
            "{} [{}/{}]",
            self.word,
            self.current_index + 1,
            self.occurrences.len(),
        )
    }

    /// Format status bar work info: "Boswell, Life of Johnson"
    pub fn status_work(&self) -> String {
        match self.current_hit() {
            Some(hit) => {
                let author = shorten_author(&hit.author);
                let title = shorten_title(&hit.work_title);
                format!("{}, {}", author, title)
            }
            None => String::new(),
        }
    }
}

fn shorten_author(author: &str) -> &str {
    if let Some(idx) = author.find(',') {
        &author[..idx]
    } else {
        author.rsplit_once(' ').map(|(_, last)| last).unwrap_or(author)
    }
}

fn shorten_title(title: &str) -> &str {
    let t = title.split(':').next().unwrap_or(title).trim();
    let t = t.strip_prefix("The ").unwrap_or(t);
    if t.len() > 25 {
        &t[..t[..25].rfind(' ').unwrap_or(25)]
    } else {
        t
    }
}
