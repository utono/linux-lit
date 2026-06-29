# Citation-format helper — implementation plan

Audit opportunity #14. See
`docs/superpowers/specs/2026-06-22-citation-format-helper-design.md`.

## Task 1 — add the helper

`src/db/models.rs`: `pub fn citation(abbrev: &str, div1: i64, div2: i64,
line_in_div: i64) -> String` wrapping the `{}.{}.{}.{}` format.

## Task 2 — repoint the 6 sites

- `db/queries.rs:117` (abbrev), `db/queries.rs:1293` (work_abbrev).
- `gloss.rs:534/535` and `gloss.rs:579/580` (base_abbrev + first./last. fields).

## Guard — do NOT touch (EXCLUDED)

`parse_citation`, `format_citation_range` in gloss_overlay.rs; any non-4-field
format.

## Verify

`cargo build` + `cargo test --bins`. Grep: no remaining
`format!("{}.{}.{}.{}", ...)` for a citation outside the helper.
