//! Pure vim motions over a char-indexed buffer. Each returns a NEW cursor
//! (char index), count-aware, clamped. No gtk deps.
use super::buffer::{clamp_cursor, col, line_bounds, line_index, line_start, nth_line_start};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindKind {
    ForwardOn,
    ForwardBefore,
    BackOn,
    BackBefore,
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

/// vim word class: 0 = whitespace, 1 = word char, 2 = punctuation.
fn class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

pub fn left(s: &str, c: usize, n: usize) -> usize {
    let ls = line_start(s, c);
    c.saturating_sub(n).max(ls)
}

pub fn right(s: &str, c: usize, n: usize) -> usize {
    let (ls, le) = line_bounds(s, c);
    // Normal mode: cursor may sit on the last char, not past it.
    let max = le.saturating_sub(1).max(ls);
    (c + n).min(max)
}

pub fn up(s: &str, c: usize, n: usize) -> usize {
    let li = line_index(s, c);
    if li == 0 {
        return c;
    }
    let target = li.saturating_sub(n);
    let want_col = col(s, c);
    let ts = nth_line_start(s, target);
    let (_, te) = line_bounds(s, ts);
    (ts + want_col).min(te.saturating_sub(1).max(ts))
}

pub fn down(s: &str, c: usize, n: usize) -> usize {
    let total_lines = s.chars().filter(|&ch| ch == '\n').count();
    let li = line_index(s, c);
    if li >= total_lines {
        return c;
    }
    let target = (li + n).min(total_lines);
    let want_col = col(s, c);
    let ts = nth_line_start(s, target);
    let (_, te) = line_bounds(s, ts);
    (ts + want_col).min(te.saturating_sub(1).max(ts))
}

pub fn line_zero(s: &str, c: usize) -> usize {
    line_start(s, c)
}

pub fn line_first_char(s: &str, c: usize) -> usize {
    let cs = chars(s);
    let (ls, le) = line_bounds(s, c);
    let mut i = ls;
    while i < le && cs.get(i).is_some_and(|ch| ch.is_whitespace()) {
        i += 1;
    }
    i.min(le)
}

pub fn line_last_char(s: &str, c: usize) -> usize {
    let (ls, le) = line_bounds(s, c);
    le.saturating_sub(1).max(ls)
}

pub fn buffer_start(_s: &str) -> usize {
    0
}

/// `G` / `{n}G`: 1-based line; n==0 => last line. Lands on first non-blank.
pub fn goto_line(s: &str, n: usize) -> usize {
    let total_lines = s.chars().filter(|&ch| ch == '\n').count();
    let target = if n == 0 { total_lines } else { (n - 1).min(total_lines) };
    let ts = nth_line_start(s, target);
    line_first_char(s, ts)
}

pub fn word_forward(s: &str, c: usize, n: usize) -> usize {
    let cs = chars(s);
    let len = cs.len();
    let mut i = clamp_cursor(s, c);
    for _ in 0..n {
        if i >= len {
            break;
        }
        let start_class = class(cs[i]);
        if start_class != 0 {
            while i < len && class(cs[i]) == start_class {
                i += 1;
            }
        }
        while i < len && class(cs[i]) == 0 {
            i += 1;
        }
    }
    i.min(len)
}

pub fn word_back(s: &str, c: usize, n: usize) -> usize {
    let cs = chars(s);
    let mut i = clamp_cursor(s, c);
    for _ in 0..n {
        if i == 0 {
            break;
        }
        i -= 1;
        while i > 0 && class(cs[i]) == 0 {
            i -= 1;
        }
        let cl = class(cs[i]);
        while i > 0 && class(cs[i - 1]) == cl {
            i -= 1;
        }
    }
    i
}

pub fn word_end(s: &str, c: usize, n: usize) -> usize {
    let cs = chars(s);
    let len = cs.len();
    if len == 0 {
        return 0;
    }
    let mut i = clamp_cursor(s, c).min(len - 1);
    for _ in 0..n {
        if i + 1 >= len {
            i = len - 1;
            break;
        }
        i += 1;
        while i < len && class(cs[i]) == 0 {
            i += 1;
        }
        let cl = class(cs.get(i).copied().unwrap_or(' '));
        while i + 1 < len && class(cs[i + 1]) == cl {
            i += 1;
        }
    }
    i.min(len - 1)
}

pub fn find_char(s: &str, c: usize, kind: FindKind, target: char) -> Option<usize> {
    let cs = chars(s);
    let (ls, le) = line_bounds(s, c);
    match kind {
        FindKind::ForwardOn | FindKind::ForwardBefore => {
            let mut i = c + 1;
            while i < le {
                if cs[i] == target {
                    return Some(if matches!(kind, FindKind::ForwardBefore) {
                        i.saturating_sub(1)
                    } else {
                        i
                    });
                }
                i += 1;
            }
            None
        }
        FindKind::BackOn | FindKind::BackBefore => {
            let mut i = c;
            while i > ls {
                i -= 1;
                if cs[i] == target {
                    return Some(if matches!(kind, FindKind::BackBefore) { i + 1 } else { i });
                }
            }
            None
        }
    }
}

pub fn match_pair(s: &str, c: usize) -> Option<usize> {
    let cs = chars(s);
    let ch = *cs.get(c)?;
    let (open, close, forward) = match ch {
        '(' => ('(', ')', true),
        ')' => ('(', ')', false),
        '[' => ('[', ']', true),
        ']' => ('[', ']', false),
        '{' => ('{', '}', true),
        '}' => ('{', '}', false),
        _ => return None,
    };
    let mut depth = 0i32;
    if forward {
        let mut i = c;
        while i < cs.len() {
            if cs[i] == open {
                depth += 1;
            } else if cs[i] == close {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            i += 1;
        }
    } else {
        let mut i = c as isize;
        while i >= 0 {
            let u = i as usize;
            if cs[u] == close {
                depth += 1;
            } else if cs[u] == open {
                depth -= 1;
                if depth == 0 {
                    return Some(u);
                }
            }
            i -= 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    const B: &str = "the quick fox\nbrown";

    #[test]
    fn horizontal_and_word() {
        assert_eq!(right(B, 0, 1), 1);
        assert_eq!(right(B, 12, 5), 12); // clamp at line end ('x' idx 12)
        assert_eq!(left(B, 0, 1), 0);
        assert_eq!(word_forward(B, 0, 1), 4); // 'the ' -> 'quick'
        assert_eq!(word_forward(B, 0, 2), 10); // -> 'fox'
        assert_eq!(word_back(B, 10, 1), 4); // 'fox' -> 'quick'
        assert_eq!(word_end(B, 0, 1), 2); // end of 'the' = 'e' idx 2
        assert_eq!(line_zero(B, 8), 0);
        assert_eq!(line_last_char(B, 0), 12); // 'x'
    }

    #[test]
    fn vertical_keeps_line() {
        let c = 5; // 'u'
        let d = down(B, c, 1);
        assert_eq!(super::line_index(B, d), 1);
    }

    #[test]
    fn goto_line_and_find() {
        assert_eq!(buffer_start(B), 0);
        assert_eq!(super::line_index(B, goto_line(B, 2)), 1);
        assert_eq!(super::line_index(B, goto_line(B, 0)), 1); // last line
        assert_eq!(find_char(B, 0, FindKind::ForwardOn, 'q'), Some(4));
        assert_eq!(find_char(B, 0, FindKind::ForwardBefore, 'q'), Some(3));
    }

    #[test]
    fn match_pair_parens() {
        let s = "a(bc)d";
        assert_eq!(match_pair(s, 1), Some(4));
        assert_eq!(match_pair(s, 4), Some(1));
    }
}
