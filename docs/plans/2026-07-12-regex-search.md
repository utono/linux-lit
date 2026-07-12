# Regex `/` Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `/` (and `?`) incremental search accept regex patterns, with a silent literal fallback when the pattern fails to compile.

**Architecture:** All changes live in `src/input/search.rs`. The query is compiled once per search into a `regex::Regex` (smart-case via the `case_insensitive` builder flag; compile failure retries with `regex::escape`, which always compiles). `collect_line` collapses to one `find_iter` loop over the original line text, which also fixes the latent non-ASCII byte-offset bug in the old lowercasing path. Zero-width matches are skipped.

**Tech Stack:** Rust, `regex = "1"` (already in Cargo.toml). Spec: `docs/plans/2026-07-12-regex-search-design.md`.

## Global Constraints

- Do NOT run the app (`cargo run`) — the user launches it themselves. Verify with `cargo build`, `cargo test --bins`, `cargo clippy`.
- Smart-case rule: query containing an *unescaped* uppercase letter → case-sensitive; otherwise case-insensitive. `\W`-style escapes must not trigger case-sensitivity.
- Byte offsets in `SearchMatch` must index the ORIGINAL line text (they drive GTK buffer highlights).
- No UI, keybind, or overlay changes (verified: the Ctrl+/ overlay entry for `/` just says "search").

---

### Task 1: Matcher helpers — `has_unescaped_uppercase` + `build_matcher`

**Files:**
- Modify: `src/input/search.rs` (add two private fns above `collect_line`, ~line 328, and a `#[cfg(test)]` module at end of file)

**Interfaces:**
- Produces: `fn build_matcher(query: &str) -> regex::Regex` — compiles `query` with smart-case; on compile error falls back to `regex::escape(query)` with the same case flag. Task 2 calls this from both search entry points.
- Produces: `fn has_unescaped_uppercase(query: &str) -> bool` — internal helper, also used directly by tests.

- [ ] **Step 1: Write the failing tests**

Append at the very end of `src/input/search.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_case_lowercase_query_is_insensitive() {
        assert!(!has_unescaped_uppercase("jack cade"));
        let re = build_matcher("jack cade");
        assert!(re.is_match("Jack Cade"));
        assert!(re.is_match("JACK CADE"));
    }

    #[test]
    fn smart_case_uppercase_query_is_sensitive() {
        assert!(has_unescaped_uppercase("Jack"));
        let re = build_matcher("Jack");
        assert!(re.is_match("Jack Cade"));
        assert!(!re.is_match("jack cade"));
    }

    #[test]
    fn escaped_uppercase_does_not_trigger_case_sensitivity() {
        // \W is a regex class, not an uppercase literal
        assert!(!has_unescaped_uppercase(r"jack\Wcade"));
        let re = build_matcher(r"jack\Wcade");
        assert!(re.is_match("Jack Cade"));
    }

    #[test]
    fn inline_flag_overrides_smart_case() {
        // uppercase in query, but (?i) forces insensitivity
        let re = build_matcher("(?i)Jack");
        assert!(re.is_match("jack cade"));
    }

    #[test]
    fn regex_alternation_matches() {
        let re = build_matcher("jack|john");
        assert!(re.is_match("John Holland"));
        assert!(re.is_match("Jack Cade"));
        assert!(!re.is_match("Dick the butcher"));
    }

    #[test]
    fn invalid_pattern_falls_back_to_literal() {
        // Half-typed regex: unclosed group
        let re = build_matcher("jack(");
        assert!(re.is_match("this jack( literal"));
        assert!(!re.is_match("jack"));
        // Unclosed class falls back too
        let re = build_matcher("cade[");
        assert!(re.is_match("cade[ bracket"));
    }

    #[test]
    fn valid_metachar_queries_are_regexes_not_literals() {
        // `what?` COMPILES as a regex (optional `t`) — no fallback occurs,
        // so it matches "wha"/"what", by design of always-regex mode.
        let re = build_matcher("what?");
        assert!(re.is_match("whale"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd ~/utono/linux-lit && cargo test --bins search::tests 2>&1 | tail -20
```

Expected: compile error — `has_unescaped_uppercase` / `build_matcher` not found.

- [ ] **Step 3: Write the implementation**

Insert directly above `fn collect_line` (currently `src/input/search.rs:328`):

```rust
/// Smart-case probe: true if the query contains an uppercase letter that is
/// not part of a `\X` escape (so `\W`, `\S` etc. stay case-insensitive).
fn has_unescaped_uppercase(query: &str) -> bool {
    let mut escaped = false;
    for c in query.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c.is_uppercase() {
            return true;
        }
    }
    false
}

/// Compile the search query as a regex, smart-cased. An invalid pattern
/// (half-typed `jack(`, literal `what?`) silently falls back to an escaped
/// literal with identical substring semantics — incremental search must
/// never error mid-keystroke.
fn build_matcher(query: &str) -> regex::Regex {
    let insensitive = !has_unescaped_uppercase(query);
    regex::RegexBuilder::new(query)
        .case_insensitive(insensitive)
        .build()
        .unwrap_or_else(|_| {
            regex::RegexBuilder::new(&regex::escape(query))
                .case_insensitive(insensitive)
                .build()
                .expect("escaped literal always compiles")
        })
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd ~/utono/linux-lit && cargo test --bins search::tests 2>&1 | tail -20
```

Expected: all 7 tests PASS. (`dead_code` warnings for the new fns are fine until Task 2 wires them in; do not add `#[allow]`.)

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/search.rs && git commit -m "feat(search): regex matcher with smart-case and literal fallback"
```

---

### Task 2: Rewire `collect_line` and both call sites

**Files:**
- Modify: `src/input/search.rs` — `collect_line` (~line 328), `execute_search_with_query` (~line 42 and the two loops at ~lines 47–58), `collect_matches` (~line 305 and loops at ~lines 308–319), plus new tests in the `tests` module

**Interfaces:**
- Consumes: `build_matcher(query: &str) -> regex::Regex` from Task 1.
- Produces: `fn collect_line(line_text: &str, re: &regex::Regex, line_idx: usize, out: &mut Vec<SearchMatch>)` — replaces the old `(line_text, query, case_sensitive, line_idx, out)` signature. Everything above `collect_matches` (n/N, landing, seek) is untouched.

- [ ] **Step 1: Write the failing tests**

Add inside `mod tests`:

```rust
    fn collect(line: &str, query: &str) -> Vec<(usize, usize)> {
        let re = build_matcher(query);
        let mut out: Vec<crate::app::SearchMatch> = Vec::new();
        collect_line(line, &re, 0, &mut out);
        out.iter().map(|m| (m.byte_start, m.byte_end)).collect()
    }

    #[test]
    fn collect_line_finds_all_occurrences() {
        assert_eq!(collect("cade and Cade and CADE", "cade"), vec![(0, 4), (9, 13), (18, 22)]);
    }

    #[test]
    fn collect_line_offsets_index_original_text_non_ascii() {
        // 'İ' (2 bytes) lowercases to "i̇" (3 bytes): the old lowercase-then-find
        // path shifted offsets on lines like this. Offsets must slice the
        // ORIGINAL text.
        let line = "İstanbul cade here";
        let got = collect(line, "cade");
        assert_eq!(got.len(), 1);
        let (s, e) = got[0];
        assert_eq!(&line[s..e], "cade");
    }

    #[test]
    fn collect_line_skips_zero_width_matches() {
        // `a*` matches empty at every position while typing; none may land
        assert!(collect("bbb", "a*").is_empty());
        // but real (non-empty) hits of the same pattern still match
        assert_eq!(collect("baab", "a*"), vec![(1, 3)]);
    }

    #[test]
    fn collect_line_regex_class_matches() {
        assert_eq!(collect("Jack Cade", r"jack\Wcade"), vec![(0, 9)]);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd ~/utono/linux-lit && cargo test --bins search::tests 2>&1 | tail -20
```

Expected: compile error — `collect_line` still has the old 5-arg signature.

- [ ] **Step 3: Replace `collect_line` and update both call sites**

Replace the whole `collect_line` fn (including its doc comment) with:

```rust
/// Push every non-empty regex match of `re` in `line_text` onto `out`.
/// Byte offsets index the original line text (they drive buffer highlights).
/// Zero-width matches (e.g. `a*` mid-typing) are skipped.
fn collect_line(
    line_text: &str,
    re: &regex::Regex,
    line_idx: usize,
    out: &mut Vec<SearchMatch>,
) {
    for m in re.find_iter(line_text) {
        if m.start() == m.end() {
            continue;
        }
        out.push(SearchMatch { line_index: line_idx, byte_start: m.start(), byte_end: m.end() });
    }
}
```

In `execute_search_with_query`, replace:

```rust
    // Smart-case: if query has uppercase, match case-sensitively;
    // otherwise match case-insensitively.
    // Always search in line.text to keep byte offsets consistent with the buffer.
    let case_sensitive = query.chars().any(|c| c.is_uppercase());
```

with:

```rust
    // Query is a regex, smart-cased; invalid patterns degrade to literal.
    // Always search in line.text to keep byte offsets consistent with the buffer.
    let re = build_matcher(query);
```

and change both loop bodies from
`collect_line(line_text, &query, case_sensitive, line_idx, &mut new_matches);` /
`collect_line(&line.text, &query, case_sensitive, line_idx, &mut new_matches);` to

```rust
            collect_line(line_text, &re, line_idx, &mut new_matches);
```
```rust
            collect_line(&line.text, &re, line_idx, &mut new_matches);
```

In `collect_matches`, replace:

```rust
    let case_sensitive = query.chars().any(|c| c.is_uppercase());
```

with:

```rust
    let re = build_matcher(&query);
```

and update its two `collect_line(...)` calls the same way (`&query, case_sensitive` → `&re`).

- [ ] **Step 4: Run tests + build to verify**

```bash
cd ~/utono/linux-lit && cargo test --bins search::tests 2>&1 | tail -20 && cargo build 2>&1 | tail -5
```

Expected: all 11 tests PASS; build succeeds with no `dead_code` warnings.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/search.rs && git commit -m "feat(search): / and ? queries are regexes with literal fallback"
```

---

### Task 3: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Full test suite + clippy**

```bash
cd ~/utono/linux-lit && cargo test --bins 2>&1 | tail -10 && cargo clippy 2>&1 | tail -10
```

Expected: all tests pass, clippy clean (no new warnings in `search.rs`).

- [ ] **Step 2: Hand the user a live check**

Do NOT `cargo run`. Tell the user to verify in their own session:
`/jack cade` (matches both cases), `/Jack Cade` (case-sensitive), `/jack|john` (alternation), `/cade(` (invalid pattern — falls back to the literal text `cade(`), and `n`/`N` stepping after Escape. Note one visible semantic shift: valid regexes with metachars are regexes now — `/what?` matches "wha"/"what" (optional `t`), not the literal string `what?`.
