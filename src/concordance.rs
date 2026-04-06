/// A single occurrence of a word in a work's line_mapping.
#[derive(Debug, Clone)]
#[allow(dead_code)]
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

/// Cross-work concordance navigation state.
pub struct ConcordanceState {
    pub word: String,
    pub occurrences: Vec<ConcordanceHit>,
    pub current_index: usize,
}

impl ConcordanceState {
    pub fn new(word: String, occurrences: Vec<ConcordanceHit>) -> Self {
        Self {
            word,
            occurrences,
            current_index: 0,
        }
    }

    /// Advance to the next occurrence in the same work. Wraps within same-work hits.
    pub fn advance_within_work(&mut self, work_abbrev: &str) -> bool {
        if self.occurrences.is_empty() {
            return false;
        }
        let len = self.occurrences.len();
        for i in 1..=len {
            let idx = (self.current_index + i) % len;
            if self.occurrences[idx].work_abbrev == work_abbrev {
                self.current_index = idx;
                return true;
            }
        }
        false
    }

    /// Retreat to the previous occurrence in the same work. Wraps within same-work hits.
    pub fn retreat_within_work(&mut self, work_abbrev: &str) -> bool {
        if self.occurrences.is_empty() {
            return false;
        }
        let len = self.occurrences.len();
        for i in 1..=len {
            let idx = (self.current_index + len - i) % len;
            if self.occurrences[idx].work_abbrev == work_abbrev {
                self.current_index = idx;
                return true;
            }
        }
        false
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
