use crate::db::models::Chunk;

#[derive(Debug, Clone, Default)]
pub struct AbRepeatState {
    pub a_line: Option<usize>,  // buffer line index
    pub b_line: Option<usize>,  // buffer line index
    pub a_time: Option<f64>,
    pub b_time: Option<f64>,
    pub loop_active: bool,
    pub chunks: Vec<Chunk>,
    pub chunk_index: Option<usize>,
}

impl AbRepeatState {
    pub fn clear(&mut self) {
        self.a_line = None;
        self.b_line = None;
        self.a_time = None;
        self.b_time = None;
        self.loop_active = false;
    }

    pub fn find_chunk_at_line(&self, line: usize, lines: &[crate::db::models::Line]) -> Option<usize> {
        if line >= lines.len() { return None; }
        let l = &lines[line];
        self.chunks.iter().position(|c| {
            c.div1 == l.div1
                && c.div2 == Some(l.div2)
                && l.line_in_div >= c.a_line
                && l.line_in_div <= c.b_line
        })
    }

    pub fn next_chunk(&mut self) -> Option<&Chunk> {
        let idx = self.chunk_index.map(|i| i + 1).unwrap_or(0);
        if idx < self.chunks.len() {
            self.chunk_index = Some(idx);
            Some(&self.chunks[idx])
        } else {
            None
        }
    }

    pub fn prev_chunk(&mut self) -> Option<&Chunk> {
        let idx = self.chunk_index.and_then(|i| i.checked_sub(1))?;
        self.chunk_index = Some(idx);
        Some(&self.chunks[idx])
    }
}
