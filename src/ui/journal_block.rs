//! Pure paragraph-block splitter for the journal Q&A overlay. A journal page
//! buffer is plain text (`question\n\nanswer`, or verse + `———` + Q&A); blocks
//! are maximal runs of non-blank lines, separated by one-or-more blank lines.
//!
//! `journal_blocks` is currently exercised only by this module's `#[cfg(test)]`
//! tests; clippy excludes test-only usage from its dead-code analysis, so the
//! allow below silences the transient warning until `JournalOverlay` consumes
//! it (next task). Remove the allow once it has a production caller.
#![allow(dead_code)]

#[derive(Debug, Clone, PartialEq)]
pub struct JournalBlock {
    /// First buffer line (0-based) of the paragraph.
    pub start_line: i32,
    /// Last buffer line (0-based) of the paragraph.
    pub end_line: i32,
    /// The paragraph's lines, joined by '\n'.
    pub text: String,
}

/// Split `lines` (a buffer's text split on '\n') into paragraph blocks. A block
/// is a maximal run of lines that are not entirely whitespace; runs of blank
/// lines separate blocks and produce no block of their own. `start_line` /
/// `end_line` are 0-based buffer line indices. Empty / all-blank input yields an
/// empty vec.
pub fn journal_blocks(lines: &[&str]) -> Vec<JournalBlock> {
    let mut blocks = Vec::new();
    let mut run_start: Option<i32> = None;
    for (i, line) in lines.iter().enumerate() {
        let blank = line.trim().is_empty();
        if blank {
            if let Some(start) = run_start.take() {
                let end = i as i32 - 1;
                blocks.push(make_block(lines, start, end));
            }
        } else if run_start.is_none() {
            run_start = Some(i as i32);
        }
    }
    if let Some(start) = run_start {
        let end = lines.len() as i32 - 1;
        blocks.push(make_block(lines, start, end));
    }
    blocks
}

fn make_block(lines: &[&str], start: i32, end: i32) -> JournalBlock {
    let text = lines[start as usize..=end as usize].join("\n");
    JournalBlock { start_line: start, end_line: end, text }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(s: &str) -> Vec<JournalBlock> {
        let lines: Vec<&str> = s.split('\n').collect();
        journal_blocks(&lines)
    }

    #[test]
    fn plain_qa_two_blocks() {
        // "Q\n\nA" -> line 0 = Q, line 1 = blank, line 2 = A.
        let b = split("Q\n\nA");
        assert_eq!(b.len(), 2);
        assert_eq!(b[0], JournalBlock { start_line: 0, end_line: 0, text: "Q".into() });
        assert_eq!(b[1], JournalBlock { start_line: 2, end_line: 2, text: "A".into() });
    }

    #[test]
    fn multiline_paragraph_stays_one_block() {
        // A paragraph with a hard newline but no blank line is ONE block.
        let b = split("line one\nline two\n\nanswer");
        assert_eq!(b.len(), 2);
        assert_eq!(b[0], JournalBlock { start_line: 0, end_line: 1, text: "line one\nline two".into() });
        assert_eq!(b[1], JournalBlock { start_line: 3, end_line: 3, text: "answer".into() });
    }

    #[test]
    fn passage_page_blocks() {
        // verse(2 lines) blank sep blank Q blank A
        // lines: 0 v1, 1 v2, 2 blank, 3 ———, 4 blank, 5 Q, 6 blank, 7 A
        let b = split("v1\nv2\n\n———\n\nQ\n\nA");
        assert_eq!(b.len(), 4);
        assert_eq!(b[0], JournalBlock { start_line: 0, end_line: 1, text: "v1\nv2".into() });
        assert_eq!(b[1], JournalBlock { start_line: 3, end_line: 3, text: "———".into() });
        assert_eq!(b[2], JournalBlock { start_line: 5, end_line: 5, text: "Q".into() });
        assert_eq!(b[3], JournalBlock { start_line: 7, end_line: 7, text: "A".into() });
    }

    #[test]
    fn consecutive_and_edge_blanks_collapse() {
        // Leading blank, double blank between, trailing blank -> 2 blocks, no empties.
        let b = split("\nQ\n\n\nA\n");
        assert_eq!(b.len(), 2);
        assert_eq!(b[0], JournalBlock { start_line: 1, end_line: 1, text: "Q".into() });
        assert_eq!(b[1], JournalBlock { start_line: 4, end_line: 4, text: "A".into() });
    }

    #[test]
    fn empty_and_all_blank_yield_no_blocks() {
        assert_eq!(journal_blocks(&[]), Vec::new());
        assert_eq!(split("\n\n\n"), Vec::new());
        // split("") yields one empty line -> blank -> no blocks
        assert_eq!(split(""), Vec::new());
    }
}
