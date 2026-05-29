---
name: test-prose-navigation
description: Use when testing page-turn navigation for prose works like novels — runs headless tests on Bleak House and other prose to verify no gaps, repeats, or non-content highlights during page forward/backward
argument-hint: <work-abbrev>
---

# Test Prose Navigation

Run headless page-turn tests for prose works (novels, essays) to verify
page-forward and page-backward invariants.

## How to Run

```bash
cargo test -- page_turn_tests::test_page_forward_prose
cargo test -- page_turn_tests::test_page_backward_prose
```

Or run all page-turn tests (includes prose):

```bash
cargo test -- page_turn
```

## What It Tests

Tests in `src/input/navigation.rs` under `mod page_turn_tests`:

- `test_page_forward_prose_bleak_house` — every page turn advances to next non-blank line, strictly increasing
- `test_page_backward_prose_bleak_house` — forward all the way recording history, then backward pops history; exact round-trip verified

## Key Difference from Plays

Prose works use `is_dialogue(text, true)` which treats ALL non-blank lines
as dialogue. No speaker names, stage directions, or act/scene markers to
skip. `back_up_for_speaker` never triggers. Page turns advance through all
non-blank lines with no gaps.

## Data Source

Tests load `~/utono/literature/dickens-charles/bleak-house-prepared.txt`.
No media files — purely text-based algorithm testing. Page size approximated
at 30 lines.

## Available Prose Text Files

- `~/utono/literature/dickens-charles/bleak-house-prepared.txt` (BH)
- `~/utono/literature/dickens-charles/the-pickwick-papers-prepared.txt` (PP)
- `~/utono/literature/dickens-charles/a-tale-of-two-cities-prepared.txt` (TTC)
- `~/utono/literature/dickens-charles/a-christmas-carol-prepared.txt` (ACC)

## Key Architecture

- `page_forward` pushes `page_top_line` onto `page_back_stack` before advancing
- `page_backward` pops from `page_back_stack`; falls back to `prev_page_top()` when empty
- In prose mode, `is_dialogue(text, true)` treats all non-blank lines as content

## When to Use

- After changing page-turn logic in `navigation.rs`
- After changing `is_dialogue` or `is_blank` in `line_types.rs` for prose mode
- After changing text file loading or the `-prepared.txt` format
- When adding support for a new prose work
