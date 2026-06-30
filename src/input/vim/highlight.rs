//! Pure `<hi>` highlight tag toggling over the editor's raw text buffer.
//!
//! The vim editors for the gloss/synopsis/journal store highlights as an inline
//! `<hi>…</hi>` tag in the RAW text (the same grammar as `<verse>`/`<gloss>`).
//! Visual-mode `H` toggles a highlight over the selected char range:
//!
//! - If the selection overlaps or lies inside an existing `<hi>…</hi>` run, that
//!   whole run is UN-highlighted (its tag pair removed).
//! - Otherwise the selection is WRAPPED in a fresh `<hi>…</hi>`, clamped so it
//!   never splits another tag's `<…>` delimiter, then adjacent `<hi>` runs are
//!   coalesced so the raw text never accumulates `<hi><hi>`.
//!
//! Everything here is char-index based and GTK-free so it unit-tests in isolation.

pub const OPEN: &str = "<hi>";
pub const CLOSE: &str = "</hi>";

/// One `<hi>…</hi>` run found in the buffer, as half-open CHAR ranges.
/// `open`/`close` are the tag delimiters; `inner` is the highlighted text between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HiRun {
    open_start: usize,  // char index of '<' in "<hi>"
    inner_start: usize, // char index just after "<hi>"
    inner_end: usize,   // char index of '<' in "</hi>"
    close_end: usize,   // char index just after "</hi>"
}

/// Find all `<hi>…</hi>` runs (non-nested; first close after each open).
fn find_hi_runs(s: &str) -> Vec<HiRun> {
    let cs: Vec<char> = s.chars().collect();
    let open: Vec<char> = OPEN.chars().collect();
    let close: Vec<char> = CLOSE.chars().collect();
    let mut runs = Vec::new();
    let mut i = 0usize;
    while i < cs.len() {
        if matches_at(&cs, i, &open) {
            let inner_start = i + open.len();
            // first close at or after inner_start
            let mut j = inner_start;
            let mut found = None;
            while j < cs.len() {
                if matches_at(&cs, j, &close) {
                    found = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(inner_end) = found {
                runs.push(HiRun {
                    open_start: i,
                    inner_start,
                    inner_end,
                    close_end: inner_end + close.len(),
                });
                i = inner_end + close.len();
                continue;
            }
        }
        i += 1;
    }
    runs
}

fn matches_at(cs: &[char], at: usize, pat: &[char]) -> bool {
    at + pat.len() <= cs.len() && cs[at..at + pat.len()] == *pat
}

/// True if char index `at` falls strictly INSIDE any `<…>` tag delimiter in `s`
/// (i.e. after a `<` and before its matching `>`). Used to clamp a wrap so it
/// can't split another tag. A position exactly on `<` or just after `>` is fine.
fn inside_tag_delim(s: &str, at: usize) -> bool {
    let cs: Vec<char> = s.chars().collect();
    let mut depth_open: Option<usize> = None;
    for (idx, &ch) in cs.iter().enumerate() {
        if ch == '<' {
            depth_open = Some(idx);
        } else if ch == '>' {
            if let Some(o) = depth_open.take() {
                // `at` strictly inside (o, idx]: between '<' and including '>'
                if at > o && at <= idx {
                    return true;
                }
            }
        }
    }
    false
}

/// Move `at` left until it is no longer strictly inside a `<…>` delimiter.
/// (Used to pull a wrap endpoint back to the tag's `<`.)
fn clamp_left(s: &str, mut at: usize) -> usize {
    while at > 0 && inside_tag_delim(s, at) {
        at -= 1;
    }
    at
}

/// Move `at` right until it is no longer strictly inside a `<…>` delimiter.
fn clamp_right(s: &str, mut at: usize, max: usize) -> usize {
    while at < max && inside_tag_delim(s, at) {
        at += 1;
    }
    at
}

/// Coalesce `</hi><hi>` seams (with only whitespace between, none, included) so
/// two adjacent highlights merge into one. Also drops empty `<hi></hi>`.
fn coalesce(s: &str) -> String {
    let mut out = s.to_string();
    // Drop empty highlights first.
    let empty = format!("{OPEN}{CLOSE}");
    while out.contains(&empty) {
        out = out.replace(&empty, "");
    }
    // Merge directly-adjacent runs `</hi><hi>` -> "".
    let seam = format!("{CLOSE}{OPEN}");
    while out.contains(&seam) {
        out = out.replace(&seam, "");
    }
    out
}

/// Toggle a `<hi>` highlight over the half-open CHAR range `[start, end)` of `s`.
/// Returns the new buffer text. See module docs for the rule.
pub fn toggle(s: &str, start: usize, end: usize) -> String {
    if end <= start {
        return s.to_string();
    }
    let runs = find_hi_runs(s);
    // Does the selection overlap or lie inside an existing run's INNER text (or
    // its tags)? Use the full run extent [open_start, close_end) for overlap so a
    // selection touching the tags still un-highlights.
    if let Some(run) = runs
        .iter()
        .find(|r| start < r.close_end && end > r.open_start)
    {
        // REMOVE this run's tag pair, keeping the inner text. Delete close first
        // (higher indices) so the open indices stay valid.
        let cs: Vec<char> = s.chars().collect();
        let mut out: String = cs[..run.open_start].iter().collect();
        let inner: String = cs[run.inner_start..run.inner_end].iter().collect();
        let rest: String = cs[run.close_end..].iter().collect();
        out.push_str(&inner);
        out.push_str(&rest);
        return coalesce(&out);
    }

    // WRAP: clamp endpoints out of any tag delimiter so we never split a tag.
    let n = s.chars().count();
    let ws = clamp_left(s, start);
    let we = clamp_right(s, end, n);
    if we <= ws {
        return s.to_string();
    }
    let cs: Vec<char> = s.chars().collect();
    let before: String = cs[..ws].iter().collect();
    let mid: String = cs[ws..we].iter().collect();
    let after: String = cs[we..].iter().collect();
    let wrapped = format!("{before}{OPEN}{mid}{CLOSE}{after}");
    coalesce(&wrapped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ci(s: &str, sub: &str) -> usize {
        s.chars().take(s.find(sub).unwrap()).count()
    }

    #[test]
    fn wraps_a_plain_span() {
        let s = "hello world";
        let start = ci(s, "world");
        let end = s.chars().count();
        assert_eq!(toggle(s, start, end), "hello <hi>world</hi>");
    }

    #[test]
    fn toggles_off_an_identical_reselect() {
        let s = "hello <hi>world</hi>";
        // select the inner "world"
        let start = ci(s, "world");
        let end = start + "world".chars().count();
        assert_eq!(toggle(s, start, end), "hello world");
    }

    #[test]
    fn toggles_off_via_partial_overlap() {
        let s = "hello <hi>brave world</hi> end";
        // select just "brave" — a partial overlap removes the whole run
        let start = ci(s, "brave");
        let end = start + "brave".chars().count();
        assert_eq!(toggle(s, start, end), "hello brave world end");
    }

    #[test]
    fn coalesces_adjacent_runs() {
        // Two adjacent highlights produced by wrapping the gap between them.
        let s = "<hi>foo</hi><hi>bar</hi>";
        // re-wrapping shouldn't keep the seam; coalesce on any toggle output.
        assert_eq!(coalesce(s), "<hi>foobar</hi>");
    }

    #[test]
    fn drops_empty_highlight() {
        assert_eq!(coalesce("a<hi></hi>b"), "ab");
    }

    #[test]
    fn refuses_to_split_a_verse_tag() {
        // Selection spans from inside a <verse> open tag across into the text.
        let s = "<verse>To be</verse>";
        // start INSIDE "<verse>" (between 'v' and 'e'), end after "To"
        let start = 2; // inside the "<verse>" delimiter
        let end = ci(s, "To") + "To".chars().count();
        let out = toggle(s, start, end);
        // The "<verse>" tag must remain intact — no "<hi>" inside the delimiter.
        assert!(out.contains("<verse>"), "verse open tag intact: {out}");
        assert!(!out.contains("<v<hi>"), "did not split the tag: {out}");
    }

    #[test]
    fn empty_selection_is_noop() {
        assert_eq!(toggle("abc", 1, 1), "abc");
    }

    #[test]
    fn round_trip_wrap_then_find() {
        let s = "alpha beta gamma";
        let start = ci(s, "beta");
        let end = start + "beta".chars().count();
        let wrapped = toggle(s, start, end);
        assert_eq!(wrapped, "alpha <hi>beta</hi> gamma");
        // toggling the same inner range back off restores the original
        let inner_start = ci(&wrapped, "beta");
        let inner_end = inner_start + "beta".chars().count();
        assert_eq!(toggle(&wrapped, inner_start, inner_end), s);
    }
}
