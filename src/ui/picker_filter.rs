//! Shared fuzzy-filter scoring behind every ListBox picker's search box.
//!
//! `match_score` ranks a candidate `target` against a typed `filter`: a
//! contiguous substring beats a scattered subsequence, an earlier / word-start
//! match beats a later one, and non-matches score `None`. Callers lowercase both
//! sides first (matching is case-sensitive here). `subsequence_match` is the
//! boolean predicate retained for callers that only need yes/no; it is now just
//! `match_score(..).is_some()`, so it keeps the same loose fuzzy tolerance while
//! the score drives ranking wherever a picker sorts its rows.

/// Score `target` against `filter`. Higher is a better match; `None` = no match.
///
/// Scoring tiers (best first):
/// - **Contiguous substring** — `target` contains `filter` verbatim. Scored
///   high, with bonuses for matching at the very start and at a word boundary,
///   and a small penalty the later in the string the match begins.
/// - **Scattered subsequence** — every `filter` char appears in order but with
///   gaps. Scored low, penalized by the total gap span, so a loosely-scattered
///   hit (e.g. "romeo" inside "ralph waldo emerson") ranks far below a real
///   substring hit and, with a threshold, can be excluded entirely.
///
/// An empty filter matches everything with a neutral score of 0.
pub(crate) fn match_score(filter: &str, target: &str) -> Option<i32> {
    if filter.is_empty() {
        return Some(0);
    }

    // Tier 1: contiguous substring.
    if let Some(pos) = target.find(filter) {
        let mut score = 1000;
        // Prefer matches that start earlier in the string.
        score -= pos as i32;
        // Prefix bonus: the target begins with the filter.
        if pos == 0 {
            score += 500;
        } else {
            // Word-boundary bonus: the char before the match is a separator,
            // i.e. the filter starts a word (e.g. "juliet" in "romeo and juliet").
            let prev = target[..pos].chars().next_back();
            if matches!(prev, Some(c) if !c.is_alphanumeric()) {
                score += 250;
            }
        }
        return Some(score);
    }

    // Tier 2: scattered subsequence, penalized by how far it spreads.
    subsequence_score(filter, target)
}

/// Score a scattered-subsequence match. Returns `None` if `filter` is not a
/// subsequence of `target`. The score starts low (so any Tier-1 substring
/// outranks it) and loses points for the span between the first and last matched
/// char and for the number of gaps, so a tight subsequence beats a scattered one.
fn subsequence_score(filter: &str, target: &str) -> Option<i32> {
    let target_chars: Vec<char> = target.chars().collect();
    let mut ti = 0usize;
    let mut first: Option<usize> = None;
    let mut last = 0usize;
    let mut gaps = 0i32;

    for fc in filter.chars() {
        let start = ti;
        while ti < target_chars.len() && target_chars[ti] != fc {
            ti += 1;
        }
        if ti == target_chars.len() {
            return None; // ran out before matching this filter char
        }
        if first.is_none() {
            first = Some(ti);
        } else if ti > start {
            gaps += 1;
        }
        last = ti;
        ti += 1;
    }

    let span = (last - first.unwrap_or(0)) as i32;
    // Base well below the Tier-1 floor (~700 after penalties) so a substring
    // always wins; then subtract the spread and the gap count.
    Some(200 - span - gaps * 2)
}

/// True when `filter` matches `target` (contiguous or scattered subsequence).
/// Case-sensitive: callers lowercase both sides first. Retained for callers that
/// only need a boolean; equivalent to `match_score(..).is_some()`.
pub(crate) fn subsequence_match(filter: &str, target: &str) -> bool {
    match_score(filter, target).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substring_beats_scattered() {
        // "romeo" as a contiguous substring must outrank "romeo" scattered
        // through an unrelated author name.
        let real = match_score("romeo", "romeo and juliet shakespeare rom").unwrap();
        let scatter = match_score("romeo", "ralph waldo emerson").unwrap();
        assert!(real > scatter, "real={real} scatter={scatter}");
    }

    #[test]
    fn prefix_beats_midword() {
        let prefix = match_score("ham", "hamlet").unwrap();
        let midword = match_score("ham", "the hamlet").unwrap();
        assert!(prefix > midword);
    }

    #[test]
    fn word_boundary_beats_interior() {
        let boundary = match_score("jul", "romeo and juliet").unwrap();
        let interior = match_score("ome", "romeo and juliet").unwrap();
        assert!(boundary > interior);
    }

    #[test]
    fn empty_filter_matches() {
        assert_eq!(match_score("", "anything"), Some(0));
    }

    #[test]
    fn no_match_is_none() {
        assert_eq!(match_score("xyz", "hamlet"), None);
    }

    #[test]
    fn subsequence_match_preserves_boolean() {
        assert!(subsequence_match("hml", "hamlet"));
        assert!(!subsequence_match("xyz", "hamlet"));
    }
}
