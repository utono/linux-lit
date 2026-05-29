---
name: test-pagination
description: Use when testing page-turn pagination for an author's works or a single work — runs headless forward/backward pagination tests to verify no dangling speakers, no orphaned stage directions, no stanza splits, and page advancement across all page sizes
argument-hint: <author-path> | <work-abbrev>
---

# Test Pagination

Run headless pagination tests that simulate x/y page turns through all
works by an author or a single work. Tests forward pass (x presses from
start to end) and backward pass (y presses back) at multiple page sizes.

## How to Run

All Shakespeare works at 3 page sizes (25, 35, 45 lines per page):

```bash
cargo test headless_pagination -- --nocapture
```

Single page size:

```bash
cargo test shakespeare_pagination_35lpp -- --nocapture
```

All page-turn tests (navigation.rs suite — different from the viewport.rs
pagination suite above):

```bash
cargo test -- page_turn
```

## What It Validates

The viewport.rs `headless_pagination_tests` module:

- **No dangling stage directions**: page top never starts with an exit stage direction without preceding speaker context
- **No dangling speakers**: page bottom never ends with a speaker name without dialogue
- **Page advancement**: every x press strictly advances page_top (no infinite loops)
- **Round-trip**: backward pass using forward page_tops produces the same validation results
- **Section breaks**: act/scene markers push content to next page without creating blank pages
- **Stanza atomicity**: verse stanzas (with optional stanza numbers) are never split across pages

The navigation.rs `page_turn_tests` module (run via `cargo test -- page_turn`):

- Forward progress, y round-trip, structural jump return, no mid-page scene breaks, cursor on dialogue, coverage (see `/test-play-navigation` for full list)

## Adding an Author

In `src/input/viewport.rs`, `headless_pagination_tests` module:

```rust
#[test]
fn chaucer_pagination_35lpp() {
    run_author_pagination("chaucer-geoffrey", 35);
}
```

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

## In-App GTK Test Harness

For real pixel-height testing with actual GTK layout, use Ctrl+Shift+T in
the running app. See `docs/troubleshooting/page-turning-mechanics.md`.

## Key Files

- `src/input/viewport.rs` — `headless_pagination_tests` module (viewport pagination suite)
- `src/input/navigation.rs` — `page_turn_tests` module (navigation page-turn suite)
- `src/input/viewport.rs` — `trim_visible_range`, `trim_block_atoms_pure`, `clamp_at_section_break`, `back_up_for_speaker`
- `src/db/line_types.rs` — line classifiers

## When to Use

- After changing pagination logic in `viewport.rs` or `navigation.rs`
- After changing `back_up_for_speaker`, `clamp_at_section_break`, or `trim_block_atoms`
- After changing line classification in `line_types.rs`
- After modifying cleaned text file format or adding stanza numbers
- Before committing pagination changes
