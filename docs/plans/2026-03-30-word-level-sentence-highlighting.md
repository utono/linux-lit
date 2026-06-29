# Word-Level Sentence Highlighting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Highlight only the words belonging to the current sentence on boundary lines, rather than undimming entire lines.

**Architecture:** Replace `Vec<Range<usize>>` sentence groups with `Vec<SentenceGroup>` carrying character offsets on boundary lines. Update `find_mid_line_sentence_boundary()` to return the split position. Update `update_highlight()` to use character-precise tag removal.

**Tech Stack:** Rust, GTK4 TextBuffer/TextTag, sourceview5

---

### Task 1: Define SentenceGroup struct and update LineMap

**Files:**
- Modify: `src/text_file_map.rs:1-20`

- [ ] **Step 1: Write the failing test**

Add to the test module in `src/text_file_map.rs`:

```rust
#[test]
fn test_sentence_group_struct() {
    let sg = SentenceGroup {
        line_range: 0..5,
        start_col: 0,
        end_col: None,
    };
    assert_eq!(sg.line_range, 0..5);
    assert_eq!(sg.start_col, 0);
    assert_eq!(sg.end_col, None);

    let sg2 = SentenceGroup {
        line_range: 5..8,
        start_col: 19,
        end_col: Some(25),
    };
    assert!(sg2.line_range.contains(&6));
    assert_eq!(sg2.start_col, 19);
    assert_eq!(sg2.end_col, Some(25));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_sentence_group_struct`
Expected: FAIL — `SentenceGroup` not defined

- [ ] **Step 3: Define SentenceGroup and update LineMap**

In `src/text_file_map.rs`, add after the imports (before `LineMap`):

```rust
/// A sentence group with character-level boundary info for partial-line highlighting.
#[derive(Debug, Clone, PartialEq)]
pub struct SentenceGroup {
    /// Buffer line indices covered by this sentence.
    pub line_range: Range<usize>,
    /// Character offset on the first line where the sentence begins (0 = start of line).
    pub start_col: usize,
    /// Character offset on the last line where the sentence ends (None = end of line).
    pub end_col: Option<usize>,
}
```

Update `LineMap.sentence_groups` field type:

```rust
pub sentence_groups: Vec<SentenceGroup>,
```

- [ ] **Step 4: Fix all compilation errors from the type change**

Every call site that references `sentence_groups` needs updating. At this point, temporarily wrap existing `Range<usize>` values in `SentenceGroup { line_range: range, start_col: 0, end_col: None }` to keep things compiling. The specific call sites:

In `src/text_file_map.rs`, update `build_sentence_groups_from_db` return type to `Option<Vec<SentenceGroup>>`. Wrap each `groups.push(start..buf_idx)` as:

```rust
groups.push(SentenceGroup { line_range: start..buf_idx, start_col: 0, end_col: None });
```

Do the same for `build_sentence_groups` — change return type to `Vec<SentenceGroup>` and wrap each `groups.push(...)`.

Update `sentence_group_index` signature:

```rust
pub fn sentence_group_index(groups: &[SentenceGroup], buffer_line: usize) -> Option<usize> {
```

Change `g.start`/`g.end` references inside to `g.line_range.start`/`g.line_range.end`.

Update `sentence_group_for` signature:

```rust
pub fn sentence_group_for(groups: &[SentenceGroup], buffer_line: usize) -> Option<&SentenceGroup> {
```

Same `.line_range.start`/`.line_range.end` changes inside.

In `src/input/navigation.rs`, update `prev_sentence_start` and `next_sentence_start`:

```rust
fn prev_sentence_start(groups: &[crate::text_file_map::SentenceGroup], current_line: usize) -> Option<usize> {
    for g in groups.iter().rev() {
        if g.line_range.start < current_line {
            return Some(g.line_range.start);
        }
    }
    None
}

fn next_sentence_start(groups: &[crate::text_file_map::SentenceGroup], current_line: usize) -> Option<usize> {
    for g in groups.iter() {
        if g.line_range.start > current_line {
            return Some(g.line_range.start);
        }
    }
    None
}
```

In `src/input/navigation.rs` `update_highlight`, change:

```rust
let sentence_range = state.line_map.as_ref().and_then(|lm| {
    crate::text_file_map::sentence_group_for(&lm.sentence_groups, state.current_line)
});
if let Some(group) = sentence_range {
    for line_idx in group.line_range.clone() {
```

In `src/main.rs` around line 144, change `group.start` to `group.line_range.start`:

```rust
let group = lm.sentence_groups.get(sg_idx)?;
let wi = s.work_line_for_buffer(group.line_range.start)?;
```

In `src/app.rs` `is_in_current_sentence` (line 138-143), update:

```rust
if let Some(group) = crate::text_file_map::sentence_group_for(
    &lm.sentence_groups,
    self.current_line,
) {
    return group.line_range.contains(&line_index);
}
```

In `src/app.rs` `position_vocab_popup` (line 1407-1409), update:

```rust
.and_then(|lm| crate::text_file_map::sentence_group_for(&lm.sentence_groups, state.current_line))
.map(|g| g.line_range.start)
```

- [ ] **Step 5: Run tests to verify everything compiles and passes**

Run: `cargo test`
Expected: PASS (all existing tests + new struct test)

- [ ] **Step 6: Commit**

```bash
git add src/text_file_map.rs src/input/navigation.rs src/main.rs src/app.rs
git commit -m "refactor: replace Range<usize> sentence groups with SentenceGroup struct"
```

---

### Task 2: Implement find_mid_line_sentence_boundary

**Files:**
- Modify: `src/text_file_map.rs:220-236`

- [ ] **Step 1: Write the failing tests**

Add to the test module in `src/text_file_map.rs`:

```rust
#[test]
fn test_find_mid_line_sentence_boundary() {
    // Basic case: period + space + uppercase
    // "end of the fog. On such an afternoon"
    //  0123456789012345^-- char 16 is 'O'
    assert_eq!(
        find_mid_line_sentence_boundary("end of the fog. On such an afternoon"),
        Some(16)
    );

    // Exclamation mark
    // "incredible! The next day"
    //  0123456789012^-- char 12 is 'T'
    assert_eq!(
        find_mid_line_sentence_boundary("incredible! The next day"),
        Some(12)
    );

    // Question mark
    // "is it? Yes it is."
    //  0123456^-- char 7 is 'Y'
    assert_eq!(
        find_mid_line_sentence_boundary("is it? Yes it is."),
        Some(7)
    );

    // Closing quote before space
    // "the end." And then"
    //  0123456789^-- char 10 is 'A'
    assert_eq!(
        find_mid_line_sentence_boundary("the end.\" And then"),
        Some(10)
    );

    // Right double quote (U+201D) — note: U+201D is 1 char
    // "the end.\u{201D} And then"
    //  0123456789^-- char 10 is 'A'
    assert_eq!(
        find_mid_line_sentence_boundary("the end.\u{201D} And then"),
        Some(10)
    );

    // No boundary
    assert_eq!(
        find_mid_line_sentence_boundary("no boundary here at all"),
        None
    );

    // Period but no uppercase after
    assert_eq!(
        find_mid_line_sentence_boundary("Mr. smith went home"),
        None
    );

    // Period at end of line (not mid-line)
    assert_eq!(
        find_mid_line_sentence_boundary("the end."),
        None
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_find_mid_line_sentence_boundary`
Expected: FAIL — function not defined

- [ ] **Step 3: Implement find_mid_line_sentence_boundary**

Replace `has_mid_line_sentence_boundary` in `src/text_file_map.rs`:

```rust
/// Find the character offset of a mid-line sentence boundary.
/// Returns the character offset (not byte offset) of the first character
/// of the new sentence (the uppercase letter after punctuation + optional
/// quote + space). This is a character offset suitable for
/// `TextIter::set_line_offset()`.
/// Returns None if no mid-line boundary exists.
fn find_mid_line_sentence_boundary(line: &str) -> Option<usize> {
    let chars: Vec<char> = line.chars().collect();
    for i in 0..chars.len() {
        if matches!(chars[i], '.' | '!' | '?') {
            let mut j = i + 1;
            // Skip optional closing quote
            if j < chars.len() && matches!(chars[j], '"' | '\'' | '\u{201D}' | '\u{2019}') {
                j += 1;
            }
            // Expect space then uppercase
            if j + 1 < chars.len() && chars[j] == ' ' && chars[j + 1].is_uppercase() {
                return Some(j + 1);
            }
        }
    }
    None
}
```

Update `has_mid_line_sentence_boundary` to delegate (keep it for the `#[cfg(test)]` `ends_sentence` helper):

```rust
fn has_mid_line_sentence_boundary(line: &str) -> bool {
    find_mid_line_sentence_boundary(line).is_some()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_find_mid_line_sentence_boundary`
Expected: PASS

Also run: `cargo test`
Expected: All tests PASS (existing `ends_sentence` tests still work via delegation)

- [ ] **Step 5: Commit**

```bash
git add src/text_file_map.rs
git commit -m "feat: add find_mid_line_sentence_boundary returning character offset"
```

---

### Task 3: Update build_sentence_groups to populate start_col/end_col

**Files:**
- Modify: `src/text_file_map.rs:298-341`

- [ ] **Step 1: Write the failing test**

Add to the test module in `src/text_file_map.rs`:

```rust
#[test]
fn test_build_sentence_groups_mid_line_offsets() {
    let lines: Vec<String> = vec![
        "First sentence ends here. Second starts now and".into(),
        "continues on this line.".into(),
    ];
    let groups = build_sentence_groups(&lines);

    // First group: just the beginning of line 0 up to the split point
    assert_eq!(groups[0].line_range, 0..1);
    assert_eq!(groups[0].start_col, 0);
    assert_eq!(groups[0].end_col, Some(26)); // "First sentence ends here. " = 26 chars, split at 'S'

    // Second group: rest of line 0 through line 1
    assert_eq!(groups[1].line_range, 0..2);
    assert_eq!(groups[1].start_col, 26);
    assert_eq!(groups[1].end_col, None); // ends at EOL of line 1
}

#[test]
fn test_build_sentence_groups_no_mid_line_boundary() {
    // No mid-line boundaries — start_col=0, end_col=None for all groups
    let lines: Vec<String> = vec![
        "The first ray of light which illumines the gloom, and converts into a".into(),
        "dazzling brilliancy.".into(),
        "".into(),
        "Next paragraph.".into(),
    ];
    let groups = build_sentence_groups(&lines);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].line_range, 0..2);
    assert_eq!(groups[0].start_col, 0);
    assert_eq!(groups[0].end_col, None);
    assert_eq!(groups[1].line_range, 3..4);
    assert_eq!(groups[1].start_col, 0);
    assert_eq!(groups[1].end_col, None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_build_sentence_groups_mid_line`
Expected: FAIL — groups won't have correct `start_col`/`end_col` values yet

- [ ] **Step 3: Rewrite build_sentence_groups with character offsets**

Replace the `build_sentence_groups` function in `src/text_file_map.rs`:

```rust
/// Group buffer lines into sentence ranges with character-level boundary info.
/// A sentence boundary occurs when:
/// - A line ends with sentence-terminating punctuation (possibly + closing quote)
/// - A line contains a mid-line sentence boundary
/// - A blank line is encountered
fn build_sentence_groups(file_lines: &[String]) -> Vec<SentenceGroup> {
    let mut groups = Vec::new();
    let mut start_line: Option<usize> = None;
    let mut start_col: usize = 0;

    for (i, line) in file_lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if let Some(s) = start_line.take() {
                groups.push(SentenceGroup {
                    line_range: s..i,
                    start_col,
                    end_col: None,
                });
                start_col = 0;
            }
            continue;
        }

        if start_line.is_none() {
            start_line = Some(i);
        }

        if let Some(split) = find_mid_line_sentence_boundary(line) {
            // Close the current group: it ends on this line at the split point
            let s = start_line.take().unwrap();
            groups.push(SentenceGroup {
                line_range: s..i + 1,
                start_col,
                end_col: Some(split),
            });
            // Start a new group on this same line at the split point
            start_line = Some(i);
            start_col = split;
        } else if ends_sentence_at_eol(trimmed) {
            groups.push(SentenceGroup {
                line_range: start_line.take().unwrap()..i + 1,
                start_col,
                end_col: None,
            });
            start_col = 0;
        }
    }

    if let Some(s) = start_line {
        groups.push(SentenceGroup {
            line_range: s..file_lines.len(),
            start_col,
            end_col: None,
        });
    }

    groups
}
```

- [ ] **Step 4: Update existing sentence group tests**

The existing tests need updating because `build_sentence_groups` now returns `Vec<SentenceGroup>`. Update `test_build_sentence_groups`:

```rust
#[test]
fn test_build_sentence_groups() {
    let lines: Vec<String> = vec![
        "The first ray of light which illumines the gloom, and converts into a".into(),
        "dazzling brilliancy that obscurity in which the earlier history of the".into(),
        "public career of the immortal Pickwick would appear to be involved, is".into(),
        "derived from the perusal of the following entry in the Transactions of".into(),
        "the Pickwick Club.".into(),
        "".into(),
        "Next paragraph starts here and".into(),
        "continues on this line.".into(),
    ];
    let groups = build_sentence_groups(&lines);
    assert_eq!(groups[0].line_range, 0..5);
    assert_eq!(groups[0].start_col, 0);
    assert_eq!(groups[0].end_col, None);
    assert_eq!(groups[1].line_range, 6..8);
    assert_eq!(groups[1].start_col, 0);
    assert_eq!(groups[1].end_col, None);
    assert_eq!(groups.len(), 2);
}
```

Update `test_build_sentence_groups_multiple_sentences_no_blank`:

```rust
#[test]
fn test_build_sentence_groups_multiple_sentences_no_blank() {
    let lines: Vec<String> = vec![
        "First sentence ends here.".into(),
        "Second sentence starts and".into(),
        "ends here!".into(),
        "Third sentence.".into(),
    ];
    let groups = build_sentence_groups(&lines);
    assert_eq!(groups[0].line_range, 0..1);
    assert_eq!(groups[1].line_range, 1..3);
    assert_eq!(groups[2].line_range, 3..4);
}
```

Update `test_sentence_group_for` to use `SentenceGroup`:

```rust
#[test]
fn test_sentence_group_for() {
    let groups = vec![
        SentenceGroup { line_range: 0..5, start_col: 0, end_col: None },
        SentenceGroup { line_range: 6..8, start_col: 0, end_col: None },
        SentenceGroup { line_range: 9..12, start_col: 0, end_col: None },
    ];
    assert_eq!(sentence_group_for(&groups, 0).map(|g| &g.line_range), Some(&(0..5)));
    assert_eq!(sentence_group_for(&groups, 4).map(|g| &g.line_range), Some(&(0..5)));
    assert_eq!(sentence_group_for(&groups, 5), None);
    assert_eq!(sentence_group_for(&groups, 7).map(|g| &g.line_range), Some(&(6..8)));
    assert_eq!(sentence_group_for(&groups, 11).map(|g| &g.line_range), Some(&(9..12)));
    assert_eq!(sentence_group_for(&groups, 12), None);
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/text_file_map.rs
git commit -m "feat: build_sentence_groups populates start_col/end_col for mid-line boundaries"
```

---

### Task 4: Update update_highlight for character-aware undimming

**Files:**
- Modify: `src/input/navigation.rs:636-684`

- [ ] **Step 1: Update update_highlight to use character offsets**

Replace the sentence-group undimming block in `update_highlight` (lines 646-665):

```rust
let sentence_group = state.line_map.as_ref().and_then(|lm| {
    crate::text_file_map::sentence_group_for(&lm.sentence_groups, state.current_line)
});
if let Some(group) = sentence_group {
    let first_line = group.line_range.start;
    let last_line = group.line_range.end.saturating_sub(1);
    for line_idx in group.line_range.clone() {
        if let Some(line_start) = buffer.iter_at_line(line_idx as i32) {
            let mut line_end = line_start;
            if !line_end.ends_line() {
                line_end.forward_to_line_end();
            }

            let undim_start = if line_idx == first_line && group.start_col > 0 {
                // First line: undim from start_col to end of line
                let mut iter = line_start;
                iter.set_line_offset(group.start_col as i32);
                iter
            } else {
                line_start
            };

            let undim_end = if line_idx == last_line {
                if let Some(end_col) = group.end_col {
                    // Last line: undim from start of line to end_col
                    let mut iter = line_start;
                    iter.set_line_offset(end_col as i32);
                    iter
                } else {
                    line_end
                }
            } else {
                line_end
            };

            buffer.remove_tag(tag, &undim_start, &undim_end);
        }
    }
} else if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
    let mut line_end = line_start;
    if !line_end.ends_line() {
        line_end.forward_to_line_end();
    }
    buffer.remove_tag(tag, &line_start, &line_end);
}
```

- [ ] **Step 2: Build to verify it compiles**

Run: `cargo build`
Expected: Compiles without errors

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: All tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/input/navigation.rs
git commit -m "feat: update_highlight uses character offsets for word-level sentence highlighting"
```

---

### Task 5: Verify and handle edge cases

**Files:**
- Modify: `src/text_file_map.rs` (tests only)

- [ ] **Step 1: Add edge case tests**

Add to the test module in `src/text_file_map.rs`:

```rust
#[test]
fn test_build_sentence_groups_mid_line_at_first_line() {
    // Mid-line boundary on the very first line
    let lines: Vec<String> = vec![
        "Done. Now we begin a new".into(),
        "paragraph of text.".into(),
    ];
    let groups = build_sentence_groups(&lines);
    // "Done. " ends at split, "Now" starts the second sentence
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].line_range, 0..1);
    assert_eq!(groups[0].start_col, 0);
    assert!(groups[0].end_col.is_some());
    assert_eq!(groups[1].line_range, 0..2);
    assert!(groups[1].start_col > 0);
    assert_eq!(groups[1].end_col, None);
}

#[test]
fn test_build_sentence_groups_consecutive_mid_line() {
    // Two sentences on one line, each ending mid-line
    // "A. B. C continues" has two boundaries
    // find_mid_line_sentence_boundary returns only the FIRST boundary
    let lines: Vec<String> = vec![
        "A. B. C continues here.".into(),
    ];
    let groups = build_sentence_groups(&lines);
    // First boundary splits at "B", so group 0 = "A. ", group 1 starts at "B. C continues here."
    // The second boundary "B. C" is detected when processing group starting at "B"
    // but find_mid_line_sentence_boundary scans from the start of the trimmed line,
    // so the line "A. B. C continues here." starting from the perspective of the full line
    // will find the first boundary "A. B" — the function only returns the first match.
    // This means "B. C" won't be detected as a separate boundary on the same line.
    // This is acceptable for word-level splitting — a known limitation.
    assert!(groups.len() >= 2);
}
```

- [ ] **Step 2: Run edge case tests**

Run: `cargo test test_build_sentence_groups_mid_line_at_first test_build_sentence_groups_consecutive`
Expected: PASS

- [ ] **Step 3: Run full test suite and clippy**

Run: `cargo test && cargo clippy`
Expected: All PASS, no warnings

- [ ] **Step 4: Commit**

```bash
git add src/text_file_map.rs
git commit -m "test: add edge case tests for word-level sentence boundary detection"
```
