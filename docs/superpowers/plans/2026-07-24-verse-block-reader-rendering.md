# Verse-Block Reader Rendering (Facet 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render `block_type='verse'` blocks in prose works as line-broken,
indented verse and `block_type='heading'` rows as centered small-caps — reading
the new `block_type` column litdb's Phase-0 reimport added to `line_mapping`.

**Architecture:** Read `block_type` into the `Line` struct. When a work carries
any non-`prose` row, take a new block-aware buffer-fill path that splits verse
rows on their embedded `\n` into N buffer lines (recording per-line leading-space
indent) and builds a `LineMap` mapping every split line back to its one source
row — the exact pattern `build_line_map_bcp` already uses for sentence-split BCP
rows. A new `apply_block_typography` formatting pass then tags verse lines with
per-tier `left_margin` and heading rows with centered small-caps. Karaoke tints a
verse block whole (block granularity). Non-`prose` data is the only gate; works
without it render byte-identical to today.

**Tech Stack:** Rust, GTK4 (gtk4-rs), rusqlite, cargo test, cage/grim/wtype e2e.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-23-prose-inline-rendering-design.md`
  ("Facet 2 — reader design (RESOLVED)").
- This cycle is **Facet 2 (verse + centered headings) ONLY**. Inline italics
  (Facet 1) are OUT of scope; `_…_` stays literal in the text.
- **Data-driven, no feature flag.** Activation is: `work.lines` contains a row
  whose `block_type != "prose"`. Non-LoJ works are all `"prose"` → unchanged.
- `block_type` values: `{"prose","verse","heading"}`. Default `"prose"` when the
  DB column is absent (older DBs) or NULL.
- Verse rows carry embedded `\n` between verse lines with leading spaces as the
  indent (2-space tiers). Split on `\n`; the displayed line has leading spaces
  STRIPPED; the stripped count drives the indent tier.
- Build only: `cargo build` / `cargo test` / `cargo clippy`. Do NOT run the app
  (`cargo run`) — the user launches it. Headless verification uses cage/grim.
- Heading rows render UNIFORMLY centered — NO content heuristic to split
  speaker/title/chapter (honors the one-flag producer contract; avoids text
  inference).
- `layout.rs` whole-work `is_verse` is NOT touched — LoJ stays
  `is_prose_work=true`; verse indent is sub-paragraph, layered via TextTags.
- Commit after every task on `master` (project convention for this work); stage
  only the task's own files.
- Reader theme/config independent; `config-dev.json` for `cargo run` (not used
  here). Debug log: `linux-lit-dev.log`.

## File Structure

- `src/db/models.rs` — add `block_type: String` to `Line` (Task 1).
- `src/db/queries.rs` — select `block_type` in `load_work` (Task 1).
- `src/db/line_types.rs` — small `block_type` predicate helpers (Task 2).
- `src/text_file_map.rs` — new `build_line_map_blocks` + its tests (Task 3).
- `src/app/text_prep.rs` — new `prepare_block_buffer` helper: split verse rows,
  produce `(buf_lines, source_index, indent_tiers)` (Task 4).
- `src/app/mod.rs` — new block-aware branch in `rebuild_buffer_text` (Task 5).
- `src/app/formatting.rs` — new `apply_block_typography` + verse/heading tags
  (Task 6).
- `src/input/phrase_highlight.rs` — whole-block verse tint (Task 7).
- `tests/` + cage/grim harness — headless on-screen acceptance (Task 8).

---

### Task 1: Read `block_type` into the `Line` struct

**Files:**
- Modify: `src/db/models.rs:43-63` (add field)
- Modify: `src/db/queries.rs:173-205` (select + construct)

**Interfaces:**
- Produces: `Line.block_type: String` (`"prose"` default). Consumed by Tasks
  2–7. The `load_work` SELECT gains `block_type` via
  `COALESCE(block_type,'prose')` so an older DB without the column would error at
  prepare time — acceptable: the litdb migration is a hard dependency of this
  feature, documented in the spec.

- [ ] **Step 1: Write the failing test**

Add to `src/db/queries.rs` under a `#[cfg(test)] mod tests` block (create the
block if absent; seed an in-memory DB the same way other query tests do — check
the bottom of the file for an existing seed helper and reuse it):

```rust
#[test]
fn load_work_reads_block_type() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE works (abbrev TEXT PRIMARY KEY, title TEXT, author TEXT, \
             work_type TEXT, text_file TEXT, vocab_highlight INTEGER, image_dir TEXT);
         CREATE TABLE line_mapping (id INTEGER PRIMARY KEY, work_abbrev TEXT, \
             canonical_text TEXT, normalized_text TEXT, speaker TEXT, div1 INTEGER, \
             div2 INTEGER, line_in_div INTEGER, sub_line INTEGER, \
             block_type TEXT NOT NULL DEFAULT 'prose');
         INSERT INTO works VALUES ('T','t','a','prose_book',NULL,NULL,NULL);
         INSERT INTO line_mapping (id,work_abbrev,canonical_text,normalized_text,\
             speaker,div1,div2,line_in_div,sub_line,block_type) VALUES \
             (1,'T','Ordinary.','ordinary',NULL,1,0,1,0,'prose'),\
             (2,'T','Line one,\n  Line two;','line one line two',NULL,1,0,2,0,'verse'),\
             (3,'T','MELIBOEUS.','meliboeus',NULL,1,0,3,0,'heading');",
    )
    .unwrap();
    let work = load_work(&conn, "T").unwrap();
    assert_eq!(work.lines[0].block_type, "prose");
    assert_eq!(work.lines[1].block_type, "verse");
    assert_eq!(work.lines[2].block_type, "heading");
    // verse row keeps its embedded newline in text
    assert!(work.lines[1].text.contains('\n'));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib load_work_reads_block_type`
Expected: FAIL — `Line` has no field `block_type` (compile error).

- [ ] **Step 3: Add the struct field**

In `src/db/models.rs`, add to `struct Line` (after `is_spoken`, line 62):

```rust
    /// Typography class from `line_mapping.block_type`:
    /// `"prose"` (default), `"verse"` (embedded `\n`, leading-space indent),
    /// or `"heading"` (centered). Drives the block-aware buffer-fill + formatting.
    pub block_type: String,
```

- [ ] **Step 4: Select and construct it**

In `src/db/queries.rs`, change the SELECT (line 174) to add the column:

```rust
        "SELECT id, canonical_text, normalized_text, speaker, div1, div2, line_in_div, sub_line, COALESCE(block_type,'prose') \
         FROM line_mapping WHERE work_abbrev = ?1 \
         ORDER BY div1, div2, line_in_div, sub_line",
```

In the `query_map` closure, read it before the `Ok(Line { … })` (after line 186):

```rust
            let block_type: String = row.get(8)?;
```

and add `block_type,` to the `Line { … }` initializer (after `is_spoken: None,`).

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib load_work_reads_block_type`
Expected: PASS. Then `cargo build` to confirm every other `Line { … }`
constructor still compiles (there are Line literals in tests/other modules — the
compiler will name any that now need `block_type`; add `block_type: "prose".into()`
to each).

- [ ] **Step 6: Commit**

```bash
git add src/db/models.rs src/db/queries.rs
git commit -m "feat(reader): read line_mapping.block_type into Line"
```

---

### Task 2: `block_type` predicates

**Files:**
- Modify: `src/db/line_types.rs` (add helpers + tests)

**Interfaces:**
- Produces:
  - `fn is_verse_line(block_type: &str) -> bool` → `block_type == "verse"`.
  - `fn is_heading_line(block_type: &str) -> bool` → `block_type == "heading"`.
  - `fn work_has_blocks(lines: &[Line]) -> bool` → any line with
    `block_type != "prose"`. This is the activation gate for Task 5.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `src/db/line_types.rs` (reuse any existing
`Line` test-builder in that module; if none, construct literals with all fields):

```rust
#[test]
fn block_predicates_and_activation() {
    assert!(is_verse_line("verse"));
    assert!(!is_verse_line("prose"));
    assert!(is_heading_line("heading"));
    assert!(!is_heading_line("verse"));

    let mk = |bt: &str| Line {
        id: 0, citation: String::new(), text: String::new(), normalized: String::new(),
        speaker: None, is_dialogue: false, timestamp: None, div1: 1, div2: 0,
        line_in_div: 1, sub_line: 0, is_chapter: false, is_spoken: None,
        block_type: bt.into(),
    };
    assert!(!work_has_blocks(&[mk("prose"), mk("prose")]));
    assert!(work_has_blocks(&[mk("prose"), mk("verse")]));
    assert!(work_has_blocks(&[mk("heading")]));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib block_predicates_and_activation`
Expected: FAIL — `is_verse_line` not found.

- [ ] **Step 3: Add the helpers**

In `src/db/line_types.rs` (top-level, near the other predicates):

```rust
/// True for a verse block row (embedded `\n`, leading-space indent).
pub fn is_verse_line(block_type: &str) -> bool {
    block_type == "verse"
}

/// True for a heading row (rendered centered small-caps).
pub fn is_heading_line(block_type: &str) -> bool {
    block_type == "heading"
}

/// True if any line carries non-prose typography — the activation gate for the
/// block-aware buffer-fill path. Prose-only works (the whole non-LoJ corpus by
/// the migration default) return false and render unchanged.
pub fn work_has_blocks(lines: &[crate::db::models::Line]) -> bool {
    lines.iter().any(|l| l.block_type != "prose")
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib block_predicates_and_activation`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/db/line_types.rs
git commit -m "feat(reader): block_type predicates + work_has_blocks activation gate"
```

---

### Task 3: `build_line_map_blocks` — the split-and-map spine

**Files:**
- Modify: `src/text_file_map.rs` (add fn + tests)

**Interfaces:**
- Consumes: `file_lines: &[String]`, `source_index: &[usize]` (parallel: buffer
  line → its source `work.lines` index), `work_lines: &[Line]`.
- Produces: `fn build_line_map_blocks(file_lines: &[String], source_index: &[usize], work_lines: &[Line]) -> LineMap`.
  Same contract as `build_line_map_bcp`: `buffer_to_work[b] = Some(src)`,
  `work_to_buffer[wi]` = first buffer line of source row `wi`. `chapter_breaks`
  and `dialogue_buffer_lines` derived as in BCP; `sentence_groups` empty;
  `section_starts` all-`false` (LoJ is flat `div1=1,div2=0`; no scene chrome).

The producer (Task 4) guarantees `source_index` is non-decreasing and every
`work_lines` index appears at least once, so the BCP first-split logic applies
directly.

- [ ] **Step 1: Write the failing test**

Add to `src/text_file_map.rs` tests (mirror
`folded_multiline_stage_direction_maps_to_its_rows` near line 1252):

```rust
#[test]
fn build_line_map_blocks_splits_verse_row_maps_back() {
    // work rows: 0 prose, 1 verse (4 visual lines), 2 heading, 3 prose
    let mk = |bt: &str, txt: &str| Line {
        id: 0, citation: String::new(), text: txt.into(), normalized: String::new(),
        speaker: None, is_dialogue: false, timestamp: None, div1: 1, div2: 0,
        line_in_div: 1, sub_line: 0, is_chapter: false, is_spoken: None,
        block_type: bt.into(),
    };
    let work = vec![
        mk("prose", "Ordinary prose."),
        mk("verse", "l1\n  l2\n  l3\nl4"),
        mk("heading", "MELIBOEUS."),
        mk("prose", "After the verse."),
    ];
    // buffer as the producer would emit it (verse split, spaces already stripped
    // for display — but mapping only cares about source_index):
    let file_lines: Vec<String> = vec![
        "Ordinary prose.".into(),
        "l1".into(), "l2".into(), "l3".into(), "l4".into(),
        "MELIBOEUS.".into(),
        "After the verse.".into(),
    ];
    let source_index: Vec<usize> = vec![0, 1, 1, 1, 1, 2, 3];

    let m = build_line_map_blocks(&file_lines, &source_index, &work);

    // every verse buffer line maps back to work row 1
    assert_eq!(m.buffer_to_work[1], Some(1));
    assert_eq!(m.buffer_to_work[2], Some(1));
    assert_eq!(m.buffer_to_work[3], Some(1));
    assert_eq!(m.buffer_to_work[4], Some(1));
    // work_to_buffer points each row at its FIRST buffer line (sync/u-. anchor)
    assert_eq!(m.work_to_buffer[0], 0);
    assert_eq!(m.work_to_buffer[1], 1);
    assert_eq!(m.work_to_buffer[2], 5);
    assert_eq!(m.work_to_buffer[3], 6);
    // buffer line count == expanded lines
    assert_eq!(m.buffer_to_work.len(), 7);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib build_line_map_blocks_splits_verse_row_maps_back`
Expected: FAIL — `build_line_map_blocks` not found.

- [ ] **Step 3: Implement the builder**

In `src/text_file_map.rs`, add (reusing the exact first-split logic from
`build_line_map_bcp:164-219`):

```rust
/// Build a LineMap for a block-aware work (verse rows split into N buffer lines).
/// Same 1:1-per-group contract as `build_line_map_bcp`: `source_index[b]` is the
/// work-line index that buffer line `b` belongs to, non-decreasing, every work
/// line present. Verse rows contribute several consecutive buffer lines sharing
/// one source index; prose/heading rows contribute one.
pub fn build_line_map_blocks(
    file_lines: &[String],
    source_index: &[usize],
    work_lines: &[Line],
) -> LineMap {
    assert_eq!(file_lines.len(), source_index.len());
    let n_split = file_lines.len();
    let n_work = work_lines.len();

    let mut collapsed_first_split: Vec<usize> = Vec::with_capacity(n_work);
    let mut buffer_to_work: Vec<Option<usize>> = vec![None; n_split];
    {
        let mut i = 0usize;
        while i < n_split {
            let src = source_index[i];
            collapsed_first_split.push(i);
            let mut j = i;
            while j < n_split && source_index[j] == src {
                buffer_to_work[j] = Some(src);
                j += 1;
            }
            i = j;
        }
    }
    debug_assert_eq!(collapsed_first_split.len(), n_work);

    let work_to_buffer: Vec<usize> = (0..n_work)
        .map(|wi| collapsed_first_split.get(wi).copied().unwrap_or(0))
        .collect();

    let mut dialogue_buffer_lines: Vec<usize> = Vec::new();
    for (split_idx, w) in buffer_to_work.iter().enumerate() {
        if let Some(wi) = w {
            if work_lines[*wi].is_dialogue {
                dialogue_buffer_lines.push(split_idx);
            }
        }
    }

    let mut chapter_breaks: Vec<usize> = Vec::new();
    for (wi, l) in work_lines.iter().enumerate() {
        if l.is_chapter {
            chapter_breaks.push(collapsed_first_split[wi]);
        }
    }

    LineMap {
        buffer_to_work,
        work_to_buffer,
        dialogue_buffer_lines,
        sentence_groups: Vec::new(),
        chapter_breaks,
        section_starts: vec![false; n_split],
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib build_line_map_blocks`
Expected: PASS (the new test).

- [ ] **Step 5: Commit**

```bash
git add src/text_file_map.rs
git commit -m "feat(reader): build_line_map_blocks (verse row split, maps to source row)"
```

---

### Task 4: `prepare_block_buffer` — split verse rows, capture indent tiers

**Files:**
- Modify: `src/app/text_prep.rs` (add fn + tests)

**Interfaces:**
- Consumes: `work_lines: &[Line]`.
- Produces: `fn prepare_block_buffer(work_lines: &[Line]) -> BlockBuffer` where
  ```rust
  pub struct BlockBuffer {
      pub buf_lines: Vec<String>,     // display text, verse leading spaces STRIPPED
      pub source_index: Vec<usize>,   // buf line -> work-line index (for Task 3)
      pub indent_tiers: Vec<u8>,      // buf line -> 0/1/2 (verse only; 0 otherwise)
  }
  ```
  Verse rows split on `\n`; each split line's leading-space count → tier
  (`0 → 0`, `1..=2 → 1`, `>=3 → 2`), then leading spaces trimmed for display.
  Prose/heading rows contribute one buf line (unchanged text), tier 0.

- [ ] **Step 1: Write the failing test**

Add to `src/app/text_prep.rs` tests:

```rust
#[test]
fn prepare_block_buffer_splits_verse_and_tiers_indent() {
    let mk = |bt: &str, txt: &str| Line {
        id: 0, citation: String::new(), text: txt.into(), normalized: String::new(),
        speaker: None, is_dialogue: false, timestamp: None, div1: 1, div2: 0,
        line_in_div: 1, sub_line: 0, is_chapter: false, is_spoken: None,
        block_type: bt.into(),
    };
    let work = vec![
        mk("prose", "Ordinary prose."),
        mk("verse", "l1\n  l2\n    l3"),   // tiers 0, 1, 2
        mk("heading", "MELIBOEUS."),
    ];
    let b = prepare_block_buffer(&work);
    assert_eq!(b.buf_lines, vec![
        "Ordinary prose.", "l1", "l2", "l3", "MELIBOEUS.",
    ]);
    assert_eq!(b.source_index, vec![0, 1, 1, 1, 2]);
    assert_eq!(b.indent_tiers, vec![0, 0, 1, 2, 0]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib prepare_block_buffer_splits_verse_and_tiers_indent`
Expected: FAIL — `prepare_block_buffer` / `BlockBuffer` not found.

- [ ] **Step 3: Implement it**

In `src/app/text_prep.rs`:

```rust
/// Buffer lines + mapping + per-line indent tier for a block-aware work.
pub struct BlockBuffer {
    pub buf_lines: Vec<String>,
    pub source_index: Vec<usize>,
    pub indent_tiers: Vec<u8>,
}

/// Leading-space count -> indent tier (0/1/2). 0 spaces = tier 0, 1-2 = tier 1,
/// 3+ = tier 2. Matches the producer's 2-space `&nbsp;&nbsp;` tiers with slack.
/// Returns (tier, leading_space_byte_count) — spaces are ASCII so byte==char.
fn leading_space_tier(line: &str) -> (u8, usize) {
    let n = line.chars().take_while(|c| *c == ' ').count();
    let tier = if n == 0 { 0 } else if n <= 2 { 1 } else { 2 };
    (tier, n)
}

pub fn prepare_block_buffer(work_lines: &[crate::db::models::Line]) -> BlockBuffer {
    let mut buf_lines = Vec::new();
    let mut source_index = Vec::new();
    let mut indent_tiers = Vec::new();
    for (wi, l) in work_lines.iter().enumerate() {
        if crate::db::line_types::is_verse_line(&l.block_type) {
            for vline in l.text.split('\n') {
                let (tier, n) = leading_space_tier(vline);
                buf_lines.push(vline[n..].to_string()); // strip leading spaces
                source_index.push(wi);
                indent_tiers.push(tier);
            }
        } else {
            buf_lines.push(l.text.clone());
            source_index.push(wi);
            indent_tiers.push(0);
        }
    }
    BlockBuffer { buf_lines, source_index, indent_tiers }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib prepare_block_buffer_splits_verse_and_tiers_indent`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/text_prep.rs
git commit -m "feat(reader): prepare_block_buffer splits verse rows + captures indent tiers"
```

---

### Task 5: Block-aware branch in `rebuild_buffer_text`

**Files:**
- Modify: `src/app/mod.rs:4423-4426` (insert branch before the default fallback)

**Interfaces:**
- Consumes: `work_has_blocks` (Task 2), `prepare_block_buffer`/`BlockBuffer`
  (Task 4), `build_line_map_blocks` (Task 3).
- Produces: on the block path, `state.buffer` is filled with the split lines,
  `state.line_map = Some(map)`, and the indent tiers are stashed for Task 6 on
  `state` as `state.block_indent_tiers: Vec<u8>` (add this field to `AppState`,
  default empty; cleared to empty on the non-block paths).

This branch goes AFTER the BCP branch and BEFORE the `// Default: join
work.lines` fallback (`mod.rs:4425`).

- [ ] **Step 1: Add the AppState field**

Find the `AppState` struct definition (search `struct AppState`) and add:

```rust
    /// Per-buffer-line verse indent tier (0/1/2) for the block-aware path;
    /// empty on all other works. Consumed by apply_block_typography.
    pub block_indent_tiers: Vec<u8>,
```

Initialize it to `Vec::new()` in `AppState`'s constructor(s) — the compiler will
flag each site.

- [ ] **Step 2: Insert the branch**

In `src/app/mod.rs`, immediately before the `// Default: join work.lines` comment
(line 4425), insert:

```rust
    // Block-aware path: works whose rows carry non-prose block_type (verse /
    // heading) — currently LoJ post verse-preserving reimport. Split verse rows
    // on their embedded `\n` into N buffer lines (leading spaces -> indent tier),
    // and build a LineMap so timestamps / sync / u-. / concordance still resolve
    // through work_line_for_buffer (the buffer==work identity would be off-by-one
    // per extra verse line). Formatting (apply_block_typography) tags them next.
    if crate::db::line_types::work_has_blocks(&work.lines) {
        let bb = crate::app::text_prep::prepare_block_buffer(&work.lines);
        let line_map = crate::text_file_map::build_line_map_blocks(
            &bb.buf_lines, &bb.source_index, &work.lines,
        );
        state.buffer.set_text(&bb.buf_lines.join("\n"));
        state.line_map = Some(line_map);
        state.block_indent_tiers = bb.indent_tiers;
        return;
    }

```

- [ ] **Step 3: Clear the field on the other paths**

On the two earlier successful paths that `return` (the generic prose path around
`mod.rs:4361-4374` and the BCP path around `mod.rs:4417-4422`), set
`state.block_indent_tiers = Vec::new();` before their `return;`, and also on the
default fallback (before `mod.rs:4426` `state.line_map = None;`). This guarantees
a stale tier vector from a previous work never leaks into a prose work.

- [ ] **Step 4: Build + regression check**

Run: `cargo build`
Expected: compiles. Then `cargo test --lib` — all prior tasks' tests still pass.
There is no new unit test here (it's wiring); Task 8's headless run is its
acceptance. The branch reads `bb.indent_tiers`, matching Task 4's
`BlockBuffer.indent_tiers` field — confirm they agree (both `indent_tiers`).

- [ ] **Step 5: Commit**

```bash
git add src/app/mod.rs src/app/text_prep.rs
git commit -m "feat(reader): block-aware buffer-fill branch for verse/heading works"
```

---

### Task 6: `apply_block_typography` — verse indent + centered headings

**Files:**
- Modify: `src/app/formatting.rs` (add fn + tags)
- Modify: `src/app/mod.rs` (call it after the block-path buffer fill)

**Interfaces:**
- Consumes: `state.line_map`, `state.block_indent_tiers`, `work.lines[wi].block_type`.
- Produces: `pub fn apply_block_typography(state: &mut AppState)`. For each buffer
  line: if its source row is `verse`, apply a per-tier `left_margin` tag (tier 0 =
  base verse margin, +indent_px per tier) and stanza `pixels_above_lines` at the
  first line of each verse block; if `heading`, apply a centered small-caps tag.

- [ ] **Step 1: Create the tags**

In `src/app/formatting.rs`, where the other `TextTag`s are created, add a helper
that (idempotently) ensures these tags on the buffer's tag table — follow the
existing creation idiom (e.g. `speaker-name`, `stanza-number-center`):

```rust
// Verse indent tiers (left_margin in px; base = prose-ish inset, +step per tier).
// left_margin is used (not TextTag indent / justify) because GTK collapses
// leading whitespace and indent/justify render unreliably (project convention).
fn verse_margin_px(tier: u8) -> i32 {
    let base = 48;   // verse block inset from the card's text start
    let step = 32;   // per-tier additional indent
    base + step * tier as i32
}
```

Add tag creation (one tag per distinct margin, plus a centered-heading tag):

```rust
    // verse-indent-{0,1,2}: left_margin per tier.
    for tier in 0u8..=2 {
        let name = format!("verse-indent-{tier}");
        if buffer.tag_table().lookup(&name).is_none() {
            buffer.create_tag(Some(&name), &[("left-margin", &verse_margin_px(tier))]);
        }
    }
    // verse-stanza-gap: space above the first line of a verse block.
    if buffer.tag_table().lookup("verse-stanza-gap").is_none() {
        buffer.create_tag(Some("verse-stanza-gap"), &[("pixels-above-lines", &12i32)]);
    }
    // block-heading-center: centered small-caps. Copy speaker-name's EXACT
    // small-caps property (the code map found it uses a pango Variant::SmallCaps
    // object, NOT a "font-features" string — mirror whatever speaker-name sets)
    // and ADD centered justification.
    if buffer.tag_table().lookup("block-heading-center").is_none() {
        buffer.create_tag(Some("block-heading-center"), &[
            ("justification", &gtk4::Justification::Center),
            // + the same small-caps property speaker-name uses (variant object)
        ]);
    }
```

First open `src/app/formatting.rs`, find the `speaker-name` tag creation, and copy
its small-caps property verbatim into the array above. Do NOT guess
`font-features`; use the precedent's actual property (a `pango::Variant` /
`("variant", …)` pair, per the code map).

- [ ] **Step 2: Write `apply_block_typography`**

```rust
/// Tag verse lines (per-tier left_margin + stanza gap) and heading rows
/// (centered small-caps) for a block-aware work. No-op when block_indent_tiers
/// is empty (every non-block work).
pub fn apply_block_typography(state: &mut AppState) {
    if state.block_indent_tiers.is_empty() {
        return;
    }
    let Some(map) = state.line_map.clone() else { return };
    let Some(work) = state.current_work.clone() else { return };
    let buffer = state.buffer.clone();
    let n = map.buffer_to_work.len();
    let mut prev_src: Option<usize> = None;
    for bl in 0..n {
        let Some(wi) = map.buffer_to_work[bl] else { continue };
        let bt = work.lines[wi].block_type.as_str();
        let Some(start) = buffer.iter_at_line(bl as i32) else { continue };
        let mut end = start;
        if !end.ends_line() { end.forward_to_line_end(); }
        if crate::db::line_types::is_verse_line(bt) {
            let tier = state.block_indent_tiers.get(bl).copied().unwrap_or(0);
            buffer.apply_tag_by_name(&format!("verse-indent-{tier}"), &start, &end);
            if prev_src != Some(wi) {
                buffer.apply_tag_by_name("verse-stanza-gap", &start, &end);
            }
        } else if crate::db::line_types::is_heading_line(bt) {
            buffer.apply_tag_by_name("block-heading-center", &start, &end);
        }
        prev_src = Some(wi);
    }
}
```

- [ ] **Step 3: Call it**

In `src/app/mod.rs`, in the Task-5 block branch, after
`state.block_indent_tiers = bb.indent_tiers;` and before `return;`, add:

```rust
        crate::app::formatting::apply_block_typography(state);
```

(Confirm the tag-creation helper from Step 1 runs before this — if tags are
created lazily inside `apply_block_typography`, fold Step 1's block into the top
of that function so the buffer has them; otherwise call the creation helper here
first. Pick one and keep tag creation in a single place.)

- [ ] **Step 4: Build**

Run: `cargo build && cargo clippy`
Expected: compiles clean. `cargo test --lib` still green.

- [ ] **Step 5: Commit**

```bash
git add src/app/formatting.rs src/app/mod.rs
git commit -m "feat(reader): apply_block_typography — verse indent tiers + centered headings"
```

---

### Task 7: Whole-block verse karaoke tint

**Files:**
- Modify: `src/input/phrase_highlight.rs`

**Interfaces:**
- Consumes: `state.line_map`, `work.lines[wi].block_type`, the active cursor/
  playback source row.
- Produces: when the active row is a verse block, the phrase highlighter tints ALL
  of that block's buffer lines (block granularity) instead of a single line/
  phrase. Non-verse behavior unchanged.

- [ ] **Step 1: Write the failing test**

`phrase_highlight.rs` logic is buffer/GTK-coupled, so add a PURE helper that
computes the block's buffer-line range and unit-test THAT (not the GTK apply):

```rust
#[test]
fn verse_block_range_covers_all_split_lines() {
    // buffer_to_work: line 0 prose(row0), 1-4 verse(row1), 5 heading(row2)
    let b2w = vec![Some(0usize), Some(1), Some(1), Some(1), Some(1), Some(2)];
    // active on any verse buffer line -> full [1,5) range
    assert_eq!(block_buffer_range(&b2w, 3), (1, 5));
    assert_eq!(block_buffer_range(&b2w, 1), (1, 5));
    // active on a non-verse line -> single-line range
    assert_eq!(block_buffer_range(&b2w, 0), (0, 1));
    assert_eq!(block_buffer_range(&b2w, 5), (5, 6));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib verse_block_range_covers_all_split_lines`
Expected: FAIL — `block_buffer_range` not found.

- [ ] **Step 3: Implement the range helper**

In `src/input/phrase_highlight.rs`:

```rust
/// Given buffer_to_work and an active buffer line, return the [start, end)
/// buffer-line range of the contiguous run sharing the same source work-line.
/// For a verse row (split into N buffer lines) this is the whole block; for a
/// 1:1 row it is a single line. Used to tint a verse block as one unit.
pub(crate) fn block_buffer_range(
    buffer_to_work: &[Option<usize>],
    active: usize,
) -> (usize, usize) {
    let src = buffer_to_work.get(active).copied().flatten();
    let mut start = active;
    while start > 0 && buffer_to_work.get(start - 1).copied().flatten() == src {
        start -= 1;
    }
    let mut end = active + 1;
    while buffer_to_work.get(end).copied().flatten() == src {
        end += 1;
    }
    (start, end)
}
```

- [ ] **Step 4: Identify the existing whole-line tint call**

Before writing code, locate the function this file already uses to tint one whole
buffer line as the active/current line (the Line-mode path, distinct from the
per-phrase `apply_char_range_tag`). Search:

```bash
rg -n "fn (tint|highlight|apply).*line|apply_tag.*line_start|line_tint" src/input/phrase_highlight.rs
```

Note its exact name and signature (call it `EXISTING_LINE_TINT` below). It already
takes a buffer line index (or a start/end iter you build from one) and applies the
karaoke tint tag. You will call it per line across the block — do NOT create a new
tag.

- [ ] **Step 5: Use `block_buffer_range` in the tint path**

At the active-line tint site (the Line-mode branch, ~`phrase_highlight.rs:290-330,523`),
when the active source row's `block_type == "verse"`, iterate the block range and
apply `EXISTING_LINE_TINT` to every line instead of the single active line. Guard
exactly like the existing gates (`mode.is_on()` / `sync_enabled` /
`!translations_visible`). Keep prose/heading on the current single-line path:

```rust
    // Verse: tint the whole block (one timestamp = one lit unit).
    let active_src = map.buffer_to_work.get(active_bl).copied().flatten();
    let is_verse_active = active_src
        .map(|wi| crate::db::line_types::is_verse_line(&work.lines[wi].block_type))
        .unwrap_or(false);
    if is_verse_active {
        let (bs, be) = block_buffer_range(&map.buffer_to_work, active_bl);
        for bl in bs..be {
            EXISTING_LINE_TINT(state, bl); // the call found in Step 4 — NOT a new tag
        }
        return;
    }
```

Replace `EXISTING_LINE_TINT` with the real function from Step 4, adapting its
argument shape (some take `(state, bl)`, some take iters — build
`buffer.iter_at_line(bl as i32)` if so).

- [ ] **Step 6: Run tests**

Run: `cargo test --lib verse_block_range_covers_all_split_lines && cargo build`
Expected: PASS + compiles.

- [ ] **Step 7: Commit**

```bash
git add src/input/phrase_highlight.rs
git commit -m "feat(reader): whole-block verse karaoke tint at block granularity"
```

---

### Task 8: Headless on-screen acceptance (non-optional gate)

**Files:**
- No production code. Uses the cage/grim harness + the 400-row LoJ trial data
  already in `lit.db`.

**Interfaces:** none — this is the acceptance gate the project marks mandatory
for any UI change (spec §6). It verifies the visible surface, not just a build.

- [ ] **Step 1: Build**

Run: `cd ~/utono/linux-lit && cargo build`
Expected: clean.

- [ ] **Step 2: Launch headless on LoJ and screenshot a verse page**

Use the harness (per `CLAUDE.md` Headless Verification). Launch with
`LIT_NO_MPV=1 GSK_RENDERER=cairo` under cage via the harness `run_in_background`;
resize to production geometry (`wlr-randr --output HEADLESS-1 --custom-mode
1920x1200`); drive to LoJ and page to a spread containing a known verse block —
the **Horace ode** (one row, 4 lines, one timestamp — confirmed live in the
trial) is the target. Prefer `land-on.sh LoJ <div.div>` if a citation lands near
it; otherwise page with `wtype` `l`/`space` and re-screenshot.

- [ ] **Step 3: Open the PNG and verify by eye + pixel-measure**

Open every `target/ui/*.png`. Confirm, and quote the on-screen text inline:
- verse lines are LINE-BROKEN (not reflowed into a wrapped paragraph);
- the 2-tier indent renders (tier-1/tier-2 lines start further right — pixel-
  measure the left edge of an indented vs a flush verse line, don't eyeball);
- the `MELIBOEUS.`-class heading is CENTERED small-caps;
- nothing clips top/bottom (per `docs/troubleshooting/clip-prevention.md`; if the
  visible result contradicts logs, relaunch with `LIT_DEBUG_CLIP_COLOR='#ff0000'`).

- [ ] **Step 4: Verify sync + the regression guard**

- With a verse block active during playback, confirm the WHOLE block tints (all 4
  ode lines), not one line. (Drive via the sync harness or check the tint tag
  covers the block's buffer-line range.)
- Screenshot a NON-LoJ prose work (BH or PP) and confirm it renders IDENTICAL to
  pre-change (the `block_type='prose'` default path is untouched).

- [ ] **Step 5: Hand off the real-GL command**

Because cage is software rendering (can disagree with the real GL renderer on
layout — `CLAUDE.md`), give the user the exact command to eyeball the verse page
on their real renderer, and report what you observed headless.

- [ ] **Step 6: Cleanup**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

(NEVER a bare `pkill -f target/debug/linux-lit` — it kills the user's live
instance.)

---

## Post-implementation

- Merge convention: this work is on `master` per the plan's Global Constraints;
  if a branch was used, finish per `CLAUDE.md` (merge `--no-ff`, re-verify build,
  push, delete branch).
- **Data caveat:** LoJ is currently the 400-row TRIAL stub. Verse rendering is
  correct on that slice; the full 18,542-block corpus reimport is still deferred
  pending the litdb classification-method decision. When the full reimport lands,
  no reader change is needed — the same code renders it.
- **Follow-up cycles (out of scope here):** Facet 1 inline italics (its own
  spec→plan); splitting `heading` into speaker/title/chapter styles if the
  on-screen check shows uniform centering reads wrong.
```

