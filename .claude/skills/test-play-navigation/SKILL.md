---
name: test-play-navigation
description: Use when testing page-turn navigation for plays and poetry — runs headless tests on Troilus and Cressida and other verse works to verify no gaps, repeats, or non-dialogue highlights during page forward/backward
argument-hint: <work-abbrev>
---

# Test Play/Poetry Navigation

Run headless page-turn tests for plays and poetry to verify the page-forward and page-backward invariants.

## What It Tests

- **Page forward**: highlighted line is strictly increasing, always dialogue, no large gaps
- **Page backward**: highlighted line is strictly decreasing, always dialogue
- **Speaker backup**: page top backs up to show speaker name when dialogue starts a page
- **Line classification**: all lines correctly classified as dialogue/speaker/stage-direction/blank

## How to Run

Run the existing headless tests:

```bash
cd ~/utono/linux-lit && cargo test page_turn_tests -- --nocapture
```

These tests load the actual cleaned text file for Troilus and Cressida from `~/utono/literature/shakespeare-william/folger-cleaned/troilus-and-cressida.txt` and simulate page-turning through all dialogue lines.

## Adding Tests for Other Works

The tests are in `src/input/navigation.rs` under `#[cfg(test)] mod page_turn_tests`. To test another play or poem:

1. The work must have a cleaned text file (plays in `folger-cleaned/`, poems similarly)
2. Add a new test function that loads the file and runs the same forward/backward checks
3. For poetry (sonnets, Venus and Adonis), use `is_dialogue(text, false)` — same as plays since poems don't have speaker lines

## Available Play/Poetry Text Files

All in `~/utono/literature/shakespeare-william/folger-cleaned/`:

- `troilus-and-cressida.txt` (currently tested)
- `hamlet.txt`, `king-lear.txt`, `othello.txt`, `macbeth.txt`, etc. (38 plays)
- `shakespeares-sonnets.txt`, `venus-and-adonis.txt`, `lucrece.txt`, `the-phoenix-and-turtle.txt` (4 poems)

Also: `~/utono/literature/milton-john/paradise-lost-with-arguments.txt` (epic poetry)

## Interactive Testing

For visual verification (line wrapping, bottom clip, actual GTK rendering), the user must run the app:

```bash
cargo run
```

Then use Shift+Q (page forward) and < (page backward) to step through pages. Check:

- Highlighted line at page top is the next unread dialogue line
- Speaker name visible above the highlighted line when applicable
- No repeated lines between page transitions
- Bottom clip doesn't cut off text mid-line

## When to Use

- After changing `page_forward`, `page_backward`, or related functions in `navigation.rs`
- After changing `last_fully_visible_line`, `last_dialogue_in_page`, `next_dialogue_from`, or `back_up_for_speaker`
- After changing line classification in `line_types.rs`
- After changing the cleaned text file format or extraction script
