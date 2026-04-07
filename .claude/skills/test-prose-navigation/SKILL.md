---
name: test-prose-navigation
description: Use when testing page-turn navigation for prose works like novels — runs headless tests on Bleak House and other prose to verify no gaps, repeats, or non-content highlights during page forward/backward
argument-hint: <work-abbrev>
---

# Test Prose Navigation

Run headless page-turn tests for prose works (novels, essays) to verify page-forward and page-backward invariants.

## Key Difference from Plays

Prose works use `is_dialogue(text, true)` which treats ALL non-blank lines as dialogue. There are no speaker names, stage directions, or act/scene markers to skip. Every non-blank line is content.

This means:
- `back_up_for_speaker` never triggers (no speaker lines in prose)
- `next_dialogue_from` just finds the next non-blank line
- Page turns should advance through ALL non-blank lines with no gaps

## How to Run

Add prose tests to `src/input/navigation.rs` in the `page_turn_tests` module. Example test:

```rust
#[test]
fn test_page_forward_prose_bleak_house() {
    let path = std::path::Path::new(
        "/home/mlj/utono/literature/dickens-charles/bleak-house-prepared.txt",
    );
    if !path.exists() {
        eprintln!("SKIP: Bleak House not found at {:?}", path);
        return;
    }
    let lines: Vec<String> = std::fs::read_to_string(path)
        .expect("read")
        .lines()
        .map(String::from)
        .collect();

    let page_size = 30;
    let line_count = lines.len();
    let is_prose = true;

    // Find first non-blank line
    let first = lines.iter().position(|l| !line_types::is_blank(l)).unwrap_or(0);
    let mut page_top = first;
    let mut current_line = first;
    let mut highlighted: Vec<usize> = vec![current_line];

    let mut iterations = 0;
    loop {
        iterations += 1;
        if iterations > 5000 { break; }

        let last_visible = (page_top + page_size).min(line_count.saturating_sub(1));
        // In prose, last dialogue = last non-blank in range
        let mut last = page_top;
        for i in page_top..=last_visible {
            if !line_types::is_blank(&lines[i]) {
                last = i;
            }
        }
        // Next non-blank after last
        let next = (last + 1..line_count).find(|&i| !line_types::is_blank(&lines[i]));
        let next = match next {
            Some(n) => n,
            None => break,
        };

        page_top = next;
        current_line = next;
        highlighted.push(current_line);
    }

    // Verify strictly increasing
    for i in 1..highlighted.len() {
        assert!(highlighted[i] > highlighted[i - 1],
            "Prose forward: line {} not after {} at page {}", highlighted[i], highlighted[i-1], i);
    }

    println!("Bleak House forward: {} pages, line {} to {} ({} total lines)",
        highlighted.len(), highlighted[0], highlighted.last().unwrap(), line_count);
}
```

## Available Prose Text Files

- `~/utono/literature/dickens-charles/bleak-house-prepared.txt` (Bleak House, ~2MB)
- `~/utono/literature/dickens-charles/the-pickwick-papers-prepared.txt` (Pickwick Papers)
- `~/utono/literature/dickens-charles/a-tale-of-two-cities-prepared.txt` (Tale of Two Cities)
- `~/utono/literature/dickens-charles/a-christmas-carol-prepared.txt` (Christmas Carol)

DB abbreviations: BH, PP, TTC, ACC

## Key Architecture

- `page_forward` pushes `page_top_line` onto `state.page_history` before advancing
- `page_backward` pops from `state.page_history` for exact reverse navigation (no heuristic)
- `page_history` is cleared on work load (`display_work_at`)
- In prose mode, `is_dialogue(text, true)` treats ALL non-blank lines as content

## What to Verify

1. **No gaps**: every non-blank line between two highlighted lines was on the previous page
2. **No repeats**: highlighted line is always strictly after the previous
3. **All content**: highlighted lines are non-blank (in prose mode, all non-blank = dialogue)
4. **Chapter boundaries**: chapter headers and blank lines are correctly skipped
5. **History round-trip**: forward then backward returns to the same page

## Interactive Testing

```bash
cargo run
```

Open a Dickens novel via Ctrl+P picker. Use Shift+Q / < to page through. Check:
- Text flows naturally between pages
- No lines repeated at page boundaries
- Chapter headers handled gracefully
- Long paragraphs with wrapping don't cause repeats

## When to Use

- After changing page-turn logic in `navigation.rs`
- After changing `is_dialogue` or `is_blank` in `line_types.rs` for prose mode
- After changing text file loading or the `-prepared.txt` format
- When adding support for a new prose work
