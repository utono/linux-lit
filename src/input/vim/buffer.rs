//! Char-index buffer geometry for the vim engine. Everything is in CHAR units
//! (not bytes) so multibyte text is handled uniformly. Lines are `\n`-separated.

pub fn char_count(s: &str) -> usize {
    s.chars().count()
}

pub fn clamp_cursor(s: &str, cursor: usize) -> usize {
    cursor.min(char_count(s))
}

/// Char index of the start of the line containing `cursor`.
pub fn line_start(s: &str, cursor: usize) -> usize {
    let cursor = clamp_cursor(s, cursor);
    let mut start = 0;
    for (i, c) in s.chars().enumerate() {
        if i >= cursor {
            break;
        }
        if c == '\n' {
            start = i + 1;
        }
    }
    start
}

/// Char index of the line end (index of the trailing `\n`, or `char_count`).
pub fn line_end(s: &str, cursor: usize) -> usize {
    let cursor = clamp_cursor(s, cursor);
    for (i, c) in s.chars().enumerate() {
        if i >= cursor && c == '\n' {
            return i;
        }
    }
    char_count(s)
}

pub fn line_bounds(s: &str, cursor: usize) -> (usize, usize) {
    (line_start(s, cursor), line_end(s, cursor))
}

pub fn line_index(s: &str, cursor: usize) -> usize {
    let cursor = clamp_cursor(s, cursor);
    s.chars().take(cursor).filter(|&c| c == '\n').count()
}

pub fn nth_line_start(s: &str, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let mut seen = 0;
    for (i, c) in s.chars().enumerate() {
        if c == '\n' {
            seen += 1;
            if seen == n {
                return i + 1;
            }
        }
    }
    // Fewer than n newlines: clamp to the last line's start.
    line_start(s, char_count(s))
}

pub fn col(s: &str, cursor: usize) -> usize {
    let cursor = clamp_cursor(s, cursor);
    cursor - line_start(s, cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    // buffer: "ab\ncd\n\nef"  chars: a(0)b(1)\n(2)c(3)d(4)\n(5)\n(6)e(7)f(8)
    const B: &str = "ab\ncd\n\nef";

    #[test]
    fn line_geometry() {
        assert_eq!(char_count(B), 9);
        assert_eq!(line_bounds(B, 0), (0, 2)); // "ab"
        assert_eq!(line_bounds(B, 4), (3, 5)); // "cd"
        assert_eq!(line_bounds(B, 6), (6, 6)); // empty line
        assert_eq!(line_bounds(B, 8), (7, 9)); // "ef"
        assert_eq!(line_index(B, 4), 1);
        assert_eq!(line_index(B, 7), 3);
        assert_eq!(nth_line_start(B, 0), 0);
        assert_eq!(nth_line_start(B, 1), 3);
        assert_eq!(nth_line_start(B, 3), 7);
        assert_eq!(nth_line_start(B, 99), 7); // clamp to last line
        assert_eq!(col(B, 4), 1);
        assert_eq!(clamp_cursor(B, 99), 9);
    }

    #[test]
    fn multibyte_is_char_indexed() {
        let s = "é\nxy"; // é(0) \n(1) x(2) y(3)
        assert_eq!(char_count(s), 4);
        assert_eq!(line_bounds(s, 0), (0, 1));
        assert_eq!(nth_line_start(s, 1), 2);
    }
}
