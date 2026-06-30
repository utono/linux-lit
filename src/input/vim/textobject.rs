//! Text objects for operator+object commands (`diw`, `ci"`, `da(`, `dip`, …).
//! Each returns a half-open char [`Range`] over the buffer, or `None` when the
//! object isn't found at the cursor.

use super::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextObjKind {
    Word,
    /// A delimiter pair, e.g. `('(', ')')` or `('"', '"')`.
    Pair(char, char),
    Paragraph,
}

fn class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

pub fn text_object(s: &str, cursor: usize, kind: TextObjKind, around: bool) -> Option<Range> {
    let cs: Vec<char> = s.chars().collect();
    if cs.is_empty() {
        return None;
    }
    let cursor = cursor.min(cs.len().saturating_sub(1));
    match kind {
        TextObjKind::Word => word_object(&cs, cursor, around),
        TextObjKind::Pair(open, close) => pair_object(&cs, cursor, open, close, around),
        TextObjKind::Paragraph => paragraph_object(&cs, cursor, around),
    }
}

fn word_object(cs: &[char], cursor: usize, around: bool) -> Option<Range> {
    let cl = class(cs[cursor]);
    let mut start = cursor;
    while start > 0 && class(cs[start - 1]) == cl {
        start -= 1;
    }
    let mut end = cursor + 1;
    while end < cs.len() && class(cs[end]) == cl {
        end += 1;
    }
    if around {
        // include trailing whitespace; if none, include leading whitespace
        let mut e = end;
        while e < cs.len() && class(cs[e]) == 0 {
            e += 1;
        }
        if e == end {
            let mut st = start;
            while st > 0 && class(cs[st - 1]) == 0 {
                st -= 1;
            }
            return Some(Range { start: st, end });
        }
        return Some(Range { start, end: e });
    }
    Some(Range { start, end })
}

fn pair_object(cs: &[char], cursor: usize, open: char, close: char, around: bool) -> Option<Range> {
    let same = open == close;
    // find opener at/left of cursor
    let open_idx = if same {
        // count quotes before cursor: if odd we're inside; find the bracketing pair
        find_quote_pair(cs, cursor, open)?.0
    } else {
        find_open_left(cs, cursor, open, close)?
    };
    let close_idx = if same {
        find_quote_pair(cs, cursor, open)?.1
    } else {
        find_close_right(cs, open_idx, open, close)?
    };
    if around {
        Some(Range {
            start: open_idx,
            end: close_idx + 1,
        })
    } else {
        Some(Range {
            start: open_idx + 1,
            end: close_idx,
        })
    }
}

fn find_open_left(cs: &[char], cursor: usize, open: char, close: char) -> Option<usize> {
    // If cursor is on the opener, use it.
    if cs.get(cursor) == Some(&open) {
        return Some(cursor);
    }
    let mut depth = 0i32;
    let mut i = cursor as isize;
    while i >= 0 {
        let u = i as usize;
        if cs[u] == close && u != cursor {
            depth += 1;
        } else if cs[u] == open {
            if depth == 0 {
                return Some(u);
            }
            depth -= 1;
        }
        i -= 1;
    }
    None
}

fn find_close_right(cs: &[char], open_idx: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = open_idx;
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
    None
}

fn find_quote_pair(cs: &[char], cursor: usize, q: char) -> Option<(usize, usize)> {
    // collect quote positions on the cursor's line
    let line_start = {
        let mut s = 0;
        for i in 0..cursor {
            if cs[i] == '\n' {
                s = i + 1;
            }
        }
        s
    };
    let line_end = {
        let mut e = cs.len();
        for (i, &c) in cs.iter().enumerate().skip(cursor) {
            if c == '\n' {
                e = i;
                break;
            }
        }
        e
    };
    let quotes: Vec<usize> = (line_start..line_end).filter(|&i| cs[i] == q).collect();
    // pair them (0,1),(2,3),...
    let mut k = 0;
    while k + 1 < quotes.len() {
        let (a, b) = (quotes[k], quotes[k + 1]);
        if cursor >= a && cursor <= b {
            return Some((a, b));
        }
        k += 2;
    }
    // cursor before first pair: use the first pair
    if quotes.len() >= 2 && cursor <= quotes[0] {
        return Some((quotes[0], quotes[1]));
    }
    None
}

fn paragraph_object(cs: &[char], cursor: usize, around: bool) -> Option<Range> {
    // a paragraph is a maximal run of non-blank lines; blank = empty line.
    let s: String = cs.iter().collect();
    let lines: Vec<&str> = s.split('\n').collect();
    // line index of cursor
    let cur_line = s[..byte_of(&s, cursor)].matches('\n').count();
    let is_blank = |i: usize| lines.get(i).map(|l| l.trim().is_empty()).unwrap_or(true);

    let mut first = cur_line;
    while first > 0 && !is_blank(first - 1) && !is_blank(first) {
        first -= 1;
    }
    // if cursor is on a blank line, treat that blank run
    let mut last = cur_line;
    while last + 1 < lines.len() && !is_blank(last + 1) && !is_blank(last) {
        last += 1;
    }
    if around {
        while last + 1 < lines.len() && is_blank(last + 1) {
            last += 1;
        }
    }
    let start = char_of_line_start(&lines, first);
    let end = char_of_line_start(&lines, last) + lines.get(last).map(|l| l.chars().count()).unwrap_or(0);
    Some(Range {
        start,
        end: end.min(cs.len()),
    })
}

fn byte_of(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

fn char_of_line_start(lines: &[&str], line: usize) -> usize {
    // sum of char counts of preceding lines + their '\n' separators
    let mut n = 0;
    for l in lines.iter().take(line) {
        n += l.chars().count() + 1; // + '\n'
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::vim::Range;

    #[test]
    fn inner_and_around_word() {
        let s = "foo bar baz";
        assert_eq!(text_object(s, 4, TextObjKind::Word, false), Some(Range { start: 4, end: 7 }));
        assert_eq!(text_object(s, 4, TextObjKind::Word, true), Some(Range { start: 4, end: 8 }));
    }

    #[test]
    fn inner_and_around_quotes() {
        let s = "say \"hi\" now"; // " at 4 and 7
        assert_eq!(text_object(s, 5, TextObjKind::Pair('"', '"'), false), Some(Range { start: 5, end: 7 }));
        assert_eq!(text_object(s, 5, TextObjKind::Pair('"', '"'), true), Some(Range { start: 4, end: 8 }));
    }

    #[test]
    fn inner_parens() {
        let s = "a(bc)d";
        assert_eq!(text_object(s, 2, TextObjKind::Pair('(', ')'), false), Some(Range { start: 2, end: 4 }));
        assert_eq!(text_object(s, 2, TextObjKind::Pair('(', ')'), true), Some(Range { start: 1, end: 5 }));
    }

    #[test]
    fn inner_parens_cursor_on_open() {
        let s = "a(bc)d";
        assert_eq!(text_object(s, 1, TextObjKind::Pair('(', ')'), false), Some(Range { start: 2, end: 4 }));
    }
}
