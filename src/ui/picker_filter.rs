/// True when every char of `filter` appears in `target` in order (a subsequence
/// match). Case-sensitive: callers lowercase both sides first. The shared
/// fuzzy-filter predicate behind every ListBox picker's search box.
pub(crate) fn subsequence_match(filter: &str, target: &str) -> bool {
    let mut target_chars = target.chars();
    for fc in filter.chars() {
        if !target_chars.any(|tc| tc == fc) {
            return false;
        }
    }
    true
}
