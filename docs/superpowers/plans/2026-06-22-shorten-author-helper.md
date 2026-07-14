# Shared shorten_author helper — implementation plan

Audit opportunity #11 (narrowed to shorten_author only — see spec). See
`docs/superpowers/specs/2026-06-22-shorten-author-helper-design.md`.

## Task 1 — promote the core copy

`src/concordance.rs`: `fn shorten_author` → `pub(crate) fn shorten_author`.

## Task 2 — delete the UI copy + repoint

`src/ui/concordance_list_picker.rs`: delete local `fn shorten_author`; change its
call to `crate::concordance::shorten_author(&hit.author)`.

## Guard — do NOT touch (EXCLUDED)

Both `shorten_title` functions (behaviorally different — truncating vs not) and
`truncate_around_center` stay.

## Verify

`cargo build` + `cargo test --bins`. Grep: exactly one `fn shorten_author`
remains (in concordance.rs); two `fn shorten_title` remain (unchanged).
