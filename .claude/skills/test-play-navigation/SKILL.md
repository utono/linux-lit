---
name: test-play-navigation
description: Use when testing page-turn navigation for plays and poetry — runs headless tests on Troilus and Cressida and other verse works to verify no gaps, repeats, or non-dialogue highlights during page forward/backward
argument-hint: <work-abbrev>
---

# Test Play/Poetry Navigation

Run headless page-turn tests for plays and poetry to verify page-forward
and page-backward invariants.

## How to Run

All page-turn tests (plays + prose + all-Shakespeare):

```bash
cargo test -- page_turn
```

Only all-Shakespeare tests:

```bash
cargo test -- all_shakespeare
```

## What It Tests

Tests in `src/input/navigation.rs` under `mod page_turn_tests`:

All-Shakespeare (38 plays):

- `test_page_forward_all_shakespeare_no_stuck` — every page turn advances
- `test_page_forward_backward_roundtrip_all_shakespeare` — forward tops strictly increasing; backward via stack round-trips exactly
- `test_x_y_roundtrip_with_clamping_all_shakespeare` — same with section-break clamping simulation
- `test_x_page_forward_no_mid_page_scene_breaks_all_shakespeare` — no scene marker in the interior of any page
- `test_y_after_scene_jump_returns_to_origin_all_shakespeare` — y after scene jump returns to exact origin
- `test_scene_synopsis_identification_all_shakespeare` — scene markers resolve to correct synopsis keys

Single-work (Troilus, Comedy of Errors):

- `test_page_forward_no_gaps_or_repeats` — Troilus: highlighted lines strictly increasing, all dialogue
- `test_x_page_forward_covers_every_line_errors` — Comedy of Errors: every non-blank line appears in at least one viewport
- `test_j_cursor_next_dialogue_covers_every_line_errors` — same coverage via j/q cursor navigation
- `test_y_after_chapter_jump_returns_to_origin` — y after [/{ returns to origin
- `test_x_x_x_scene_jump_y_y_sequence` — x x x 3 y returns to pre-jump page; second y has empty stack
- `test_chained_scene_jumps_only_last_origin_survives` — 3 3 y returns to page between the two jumps

## Data Source

Tests load cleaned text files from `~/utono/literature/shakespeare-william/folger-cleaned/`. No media files, no MPV, no audio — purely text-based algorithm testing. Page size approximated at 30 lines (no GTK pixel heights).

## In-App GTK Test Harness

For real pixel-height testing with actual GTK layout, use Ctrl+Shift+T in the running app. See `docs/troubleshooting/page-turning-mechanics.md` for details.

## Key Architecture

- `page_forward` pushes `page_top_line` onto `page_back_stack` before advancing
- `page_backward` pops from `page_back_stack`; falls back to `prev_page_top()` when empty
- Structural jumps (2, 3, [, {, gg, G, bookmarks, vocab, zt) clear the stack then push current `page_top_line` as a single return entry
- `clamp_at_section_break` always fires — scene headers never appear mid-page
- `page_forward`/`page_backward`/`page_backward_bottom` guard against `page_turn_lock` to prevent stack corruption during crossfade animations

## When to Use

- After changing `page_forward`, `page_backward`, or related functions in `navigation.rs`
- After changing `last_fully_visible_line`, `clamp_at_section_break`, `back_up_for_speaker`, or `trim_visible_range`
- After changing `page_back_stack` usage
- After changing line classification in `line_types.rs`
- Before committing pagination changes
