# Regex `/` Search — Design

**Date:** 2026-07-12
**Status:** Approved
**Scope:** `src/input/search.rs` only (plus unit tests). The `regex` crate
is already a dependency.

## Problem

The `/` (and `?`) incremental search is a plain substring match with
vim-style smart-case. There is no way to express alternation
(`jack|john`), character classes, anchors, or an explicit
case-insensitivity override. Users coming from vim expect `/` to accept
a pattern.

## Decision

Every query is treated as a regex, with a silent literal fallback when
the pattern fails to compile ("always regex, literal fallback" — chosen
over strict-regex and opt-in-prefix alternatives). Because search runs
on every keystroke, half-typed patterns like `jack(` are momentarily
invalid; the fallback keeps incremental search identical to today's
behavior in that window and for queries that are not valid regexes
(`[Enter.`). Note: queries that DO compile are regexes — `what?`
matches "wha"/"what" (optional `t`), not the literal string `what?`.

## Matching pipeline

`execute_search_with_query` compiles the query once per search (not per
line) into a single `regex::Regex`:

1. `RegexBuilder::new(query)` with
   `case_insensitive(!smart_case_sensitive)`.
2. On compile error, retry with `regex::escape(query)` — which always
   compiles — restoring exact substring semantics.

`collect_line` collapses from two hand-rolled loops (case-sensitive /
lowercased) into one `find_iter` loop over the original line text,
pushing byte offsets straight from the regex match.

- **Fixes a latent offset bug:** the current insensitive path lowercases
  the line and reports offsets on the lowercased string, which shifts
  byte offsets for non-ASCII text (`İ` lowercases to a longer byte
  sequence). Matching on the original text with the regex
  case-insensitive flag removes the discrepancy.
- **Zero-width matches are skipped** (`start == end`, e.g. `a*` while
  typing) so they cannot produce a highlight at every position.

## Smart-case

Unchanged rule, new implementation: a query containing an unescaped
uppercase letter is case-sensitive; otherwise insensitive, via the
regex `case_insensitive` flag. "Unescaped" means an uppercase letter
not immediately preceded by `\`, so `\W` / `\S` do not force
case-sensitivity. Inline `(?i)` / `(?-i)` work natively as explicit
overrides in either direction.

## Untouched

Search bar UI, `n`/`N` MRU reactivation, match landing / canonical
spread / MPV seek logic — all sit above `collect_matches` and inherit
regex support. No keybind changes, so no Ctrl+/ overlay edits (verify
the `describe()` text for `/` does not claim substring-only matching).

## Testing

Unit tests in `search.rs` (pure helpers; `cargo test --bins`):

- plain literal query matches as before
- alternation `jack|john`
- smart-case: lowercase query insensitive, uppercase query sensitive
- `(?i)` override forces insensitivity despite uppercase
- escaped-uppercase (`\W`) does not trigger case-sensitivity
- invalid pattern falls back to literal (`jack(` matches literal
  `jack(`)
- zero-width matches skipped, no infinite loop
- non-ASCII line: byte offsets index the original text correctly

No e2e needed — rendering and navigation are unchanged.
