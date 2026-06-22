# Shared subsequence-match helper — design

## Goal

Remove the byte-identical char-level `subsequence_match` fuzzy-filter function
copied verbatim into 5 pickers, via one shared free helper — with **zero behavior
change**. This is audit opportunity #10, scoped to the pure char-subsequence core
(NOT the work-typed wrappers).

## The duplication

Five pickers contain the same function body (only the name differs):

- `media_picker.rs:202` — `fn subsequence_match(filter, target) -> bool`
- `journal_picker.rs:157` — same
- `bookmark_picker.rs:175` — same
- `gloss_picker.rs:155` — same
- `library_picker.rs:550` — `fn subsequence_chars(filter, target) -> bool` (same body)

```rust
fn subsequence_match(filter: &str, target: &str) -> bool {
    let mut target_chars = target.chars();
    for fc in filter.chars() {
        if !target_chars.any(|tc| tc == fc) {
            return false;
        }
    }
    true
}
```

Verified byte-identical across all five. Every call site **already lowercases
both `filter` and `target` before calling** (`filter_lower` + `target.to_lowercase()`),
so the function is purely a char-level subsequence test with no case-folding of
its own — it can be shared as-is.

## Component

A `pub mod picker_filter;` module at `src/ui/picker_filter.rs` (registered in
`src/ui/mod.rs`). Pure, no GTK, no `AppState`:

```rust
/// True when every char of `filter` appears in `target` in order (a
/// subsequence match). Case-sensitive: callers lowercase both sides first.
pub(crate) fn subsequence_match(filter: &str, target: &str) -> bool {
    let mut target_chars = target.chars();
    for fc in filter.chars() {
        if !target_chars.any(|tc| tc == fc) {
            return false;
        }
    }
    true
}
```

## Call-site changes

- **media / journal / bookmark / gloss pickers:** delete the local
  `fn subsequence_match` and change each call to
  `crate::ui::picker_filter::subsequence_match(&filter_lower, &target)`.
- **library_picker:** delete local `fn subsequence_chars`; point its two
  wrappers (`subsequence_match_work`, `author_name_matches`) at
  `crate::ui::picker_filter::subsequence_match`. The wrappers themselves STAY —
  they build the composite/work-typed target before delegating.

## Explicitly EXCLUDED (stay as-is)

- **`subsequence_match_work(filter, &WorkSummary)`** (library_picker) — builds a
  `format!("{title} {author} {abbrev}").to_lowercase()` target; work-typed, not a
  char-level helper. Keeps its body, delegates its tail to the shared fn.
- **`author_name_matches(filter, author)`** (library_picker, `pub`) — does its own
  lowercasing then delegates; public API consumed elsewhere. Keeps its body.
- **The test-only `fn subsequence_match(filter, &WorkSummary)`** in
  library_picker's `#[cfg(test)]` mod — an alias over `subsequence_match_work` for
  existing tests; unrelated to the char-level helper, leave untouched.

## Why a new module, not picker_nav

`src/ui/picker_nav.rs` (#6) is for ListBox *navigation*. A string-filter predicate
is a different concern; a dedicated `picker_filter` keeps each shared helper
single-purpose. Rejected: a `Picker`/`Filterable` trait — speculative, and the
call sites differ in how they build `target`, which legitimately stays local.

## Verification

Pure function extraction; no control-flow change. `cargo build` +
`cargo test --bins` (library_picker's existing subsequence tests must stay green —
they exercise the wrappers that now delegate to the shared fn).
