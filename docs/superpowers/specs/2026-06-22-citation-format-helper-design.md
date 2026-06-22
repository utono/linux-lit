# Citation-format helper — design

## Goal

Remove the duplicated `format!("{}.{}.{}.{}", abbrev, div1, div2, line_in_div)`
citation template, repeated byte-identically at 6 sites, via one helper fn — with
**zero behavior change**. Audit opportunity #14.

## The duplication

The citation string is the canonical `abbrev.div1.div2.line_in_div` address that
populates `Line.citation`. The exact `format!` template appears at:

- `src/gloss.rs:534, 535` (start/end of a gloss span, from `first`/`last` lines)
- `src/gloss.rs:579, 580` (same, second gloss path)
- `src/db/queries.rs:117` (building a `Line`)
- `src/db/queries.rs:1293` (building a `BookmarkItem`)

Byte-identical template; only the argument source varies (`first.div1` struct
fields vs row-derived `div1` locals).

## Component

A free `pub fn citation` at the top of `src/db/models.rs` — the module that owns
the `Line { citation: String, .. }` field this builds:

```rust
/// Format the canonical citation address `abbrev.div1.div2.line_in_div`.
/// The single source of truth for how a line citation string is built.
pub fn citation(abbrev: &str, div1: i64, div2: i64, line_in_div: i64) -> String {
    format!("{}.{}.{}.{}", abbrev, div1, div2, line_in_div)
}
```

## Call-site changes

- `queries.rs:117`: `let citation = crate::db::models::citation(abbrev, div1, div2, line_in_div);`
- `queries.rs:1293`: `... = crate::db::models::citation(work_abbrev, div1, div2, line_in_div);`
- `gloss.rs:534/535/579/580`:
  `crate::db::models::citation(base_abbrev, first.div1, first.div2, first.line_in_div)`
  (and `last.` for the end).

## Explicitly EXCLUDED

- **`parse_citation` / `format_citation_range`** (`gloss_overlay.rs`) — the inverse
  (split a citation) and a range formatter; different concern, not this template.
- **Any `format!` with a different arity or separator** — only the exact 4-field
  `{}.{}.{}.{}` template is shared.

## Verification

Pure helper extraction; output is the identical string. `cargo build` +
`cargo test --bins` (the citation-range tests in gloss_overlay are unaffected —
they parse, they don't build).
