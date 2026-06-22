# Shared subsequence-match helper — implementation plan

Audit opportunity #10. See
`docs/superpowers/specs/2026-06-22-subsequence-match-helper-design.md`.

## Task 1 — add the helper

- New `src/ui/picker_filter.rs` with
  `pub(crate) fn subsequence_match(&str, &str) -> bool`.
- Register `pub mod picker_filter;` in `src/ui/mod.rs`.
- `cargo build`.

## Task 2 — delegate the 4 free copies

In `media_picker.rs`, `journal_picker.rs`, `bookmark_picker.rs`,
`gloss_picker.rs`: delete the local `fn subsequence_match`; change the call to
`crate::ui::picker_filter::subsequence_match(&filter_lower, &target)`.

## Task 3 — library_picker

Delete local `fn subsequence_chars`; repoint `subsequence_match_work` and
`author_name_matches` tails at the shared fn. Keep both wrappers and the
test-only alias.

## Guard — do NOT touch (EXCLUDED)

`subsequence_match_work`, `author_name_matches` bodies, and the `#[cfg(test)]`
`subsequence_match` alias stay.

## Verify

`cargo build` + `cargo test --bins` (library_picker subsequence tests stay green).
Grep: no remaining `fn subsequence_match`/`fn subsequence_chars` outside the new
module and the test alias.
