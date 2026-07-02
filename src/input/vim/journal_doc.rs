//! Journal-specific framing for the vim editor: the page shows `Q: <question>`
//! then a blank line then the answer; this builds that buffer and parses it
//! back into (question, answer). Kept OUT of the engine so the engine stays a
//! generic text editor.

fn strip_q_prefix(line: &str) -> &str {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("Q:") {
        rest.trim_start()
    } else {
        line
    }
}

pub fn build_buffer(question: &str, answer: &str) -> String {
    format!("Q: {}\n\n{}", strip_q_prefix(question), answer)
}

/// A `note` entry has no question and stores raw Markdown; its editor buffer is
/// the raw Markdown verbatim (no `Q:` seed line). Round-trips losslessly.
pub fn build_note_buffer(answer: &str) -> String {
    answer.to_string()
}

pub fn parse_note_back(buffer: &str) -> String {
    buffer.to_string()
}

pub fn parse_back(buffer: &str) -> (String, String) {
    let first = buffer.split('\n').next().unwrap_or("");
    let question = strip_q_prefix(first).to_string();

    if let Some(idx) = buffer.find("\n\n") {
        let answer = buffer[idx + 2..].trim().to_string();
        (question, answer)
    } else {
        let rest: String = buffer
            .splitn(2, '\n')
            .nth(1)
            .unwrap_or("")
            .trim()
            .to_string();
        (question, rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_parse_roundtrip() {
        let b = build_buffer("Compare X", "Line one.\n\nLine two.");
        assert_eq!(b, "Q: Compare X\n\nLine one.\n\nLine two.");
        let (q, a) = parse_back(&b);
        assert_eq!(q, "Compare X");
        assert_eq!(a, "Line one.\n\nLine two.");
    }

    #[test]
    fn build_strips_existing_q_prefix() {
        assert_eq!(build_buffer("Q: Already", "ans"), "Q: Already\n\nans");
    }

    #[test]
    fn parse_back_without_blank_line() {
        let (q, a) = parse_back("Q: just a question line\nand a stray answer line");
        assert_eq!(q, "just a question line");
        assert_eq!(a, "and a stray answer line");
    }

    #[test]
    fn note_buffer_is_raw_markdown_roundtrip() {
        let md = "## Cry\n\n- load it\n- **then** drop it";
        let b = build_note_buffer(md);
        assert_eq!(b, md); // no Q: seed, verbatim
        assert_eq!(parse_note_back(&b), md);
    }
}
