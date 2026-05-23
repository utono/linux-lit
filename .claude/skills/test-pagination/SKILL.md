---
name: test-pagination
description: Use when testing page-turn pagination for an author's works or a single work — runs headless forward/backward pagination tests to verify no dangling speakers, no orphaned stage directions, no stanza splits, and page advancement across all page sizes
argument-hint: <author-path> | <work-abbrev>
---

# Test Pagination

Run headless pagination tests that simulate x/y page turns through all works by an author or a single work. Tests forward pass (x presses from start to end) and backward pass (y presses back) at multiple page sizes.

## How to Run

All Shakespeare works at 3 page sizes (25, 35, 45 lines per page):

```bash
cargo test headless_pagination -- --nocapture
```

Single page size:

```bash
cargo test shakespeare_pagination_35lpp -- --nocapture
```

## What It Validates

- **No dangling stage directions**: page top never starts with a stage direction like "[Countess exits.]" without preceding speaker context
- **No dangling speakers**: page bottom never ends with a speaker name without dialogue
- **Page advancement**: every x press strictly advances page_top (no infinite loops)
- **Round-trip**: backward pass using forward page_tops produces the same validation results
- **Section breaks**: act/scene markers push content to next page without creating blank pages
- **Stanza atomicity**: verse stanzas (with optional stanza numbers) are never split across pages

## Adding an Author

In `src/input/viewport.rs`, `headless_pagination_tests` module — add a test:

```rust
#[test]
fn chaucer_pagination_35lpp() {
    run_author_pagination("chaucer-geoffrey", 35);
}
```

The `author_path_fragment` matches against the `text_file` column in `works` table (e.g., `"shakespeare-william"` matches all paths containing that string).

## Adding a Single Work

```rust
#[test]
fn hamlet_pagination() {
    let path = "/home/mlj/utono/literature/shakespeare-william/folger-cleaned/hamlet.txt";
    if !std::path::Path::new(path).exists() { return; }
    let result = run_pagination_test(path, false, 35);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}
```

Second argument `is_prose`: `false` for plays/poetry, `true` for novels.

## Key Files

- `src/input/viewport.rs` — `headless_pagination_tests` module (tests + pure pagination simulation)
- `src/input/viewport.rs` — `trim_visible_range`, `trim_block_atoms_pure`, `block_start_for_line_pure`, `clamp_at_section_break` (production code being tested)
- `src/input/viewport.rs` — `back_up_for_speaker` (entrance vs exit stage direction heuristic)
- `src/db/line_types.rs` — line classifiers (blank, speaker, stage direction, stanza number)

## When to Use

- After changing pagination logic in `viewport.rs` or `navigation.rs`
- After changing `back_up_for_speaker`, `clamp_at_section_break`, or `trim_block_atoms`
- After changing line classification in `line_types.rs`
- After modifying cleaned text file format or adding stanza numbers
- Before committing pagination changes
