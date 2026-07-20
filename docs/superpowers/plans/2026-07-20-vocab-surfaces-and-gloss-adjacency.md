# Vocab Surfaces + Gloss Neighbor-Context Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Vocab highlighting, the `rr` popup, and `Ctrl+Alt+\` add-vocab work
on the gloss overlay, journal overlay, and chat panel; the two-column popup
becomes a compact float that Escape closes; the chat panel gets a focus rule;
reader-gloss generation receives neighboring glosses and stops recycling
their metaphors.

**Architecture:** One shared word-scanner feeds the main-card TextBuffer, both
overlay TextBuffers (TextTags), and the chat panel (Pango spans on Labels).
One `VocabPopup` widget (already attached above the overlay chain) gains a
corner placement and a words-explicit open path. Add-vocab moves off the
gloss-overlay widget onto its own AskCard instance so it can open over any
surface. Gloss generation fetches same-scene neighbor glosses via a new
query and appends them to the Claude user message.

**Tech Stack:** Rust, GTK4 (gtk4-rs), rusqlite, existing cage/grim headless
e2e harness.

**Spec:** `docs/superpowers/specs/2026-07-20-vocab-surfaces-and-gloss-adjacency-design.md`

## Global Constraints

- Branch: `feat/vocab-surfaces` off master (create in Task 1).
- Verify with `cargo build` / `cargo test --bins`; NEVER `cargo run` (user
  launches the app).
- The spacebar keysym is `"space"`.
- Every keybind change updates the surface's Ctrl+/ legend GROUPS const in
  the SAME commit.
- Overlay colors set at startup are NOT applied by `apply_theme_to_state` —
  any new themed color must be wired in `build_window` AND the theme-apply
  path.
- Picker/panel widgets are `add_overlay` layers, never in the size-bearing
  chain.
- All timestamps US Central. lit.db writes only while no other writer runs.
- After any clipping bug found during e2e: update
  `docs/troubleshooting/clip-prevention.md` (required).
- Commit messages end with the Co-Authored-By + Claude-Session trailer used
  by this session.

---

### Task 1: Branch + shared vocab word scanner

**Files:**
- Create: `src/vocab_scan.rs`
- Modify: `src/main.rs` (module decl), `src/app/mod.rs:4436-4515`
  (`build_vocab_matches` delegates)
- Test: inline `#[cfg(test)]` in `src/vocab_scan.rs`

**Interfaces:**
- Produces: `pub struct VocabSpan { pub word: String, pub line_index: usize,
  pub char_start: usize, pub char_end: usize }` and
  `pub fn scan_lines<'a, I>(lines: I, words: &HashSet<String>, skip_upper: bool)
  -> Vec<VocabSpan> where I: Iterator<Item = (usize, &'a str)>`.
  `skip_upper=true` skips lines that are entirely uppercase after trim
  (overlay speaker headers). Offsets are CHAR offsets (same convention as
  `AppState::VocabMatch`).

- [ ] **Step 1: Create the branch**

```bash
cd ~/utono/linux-lit && git checkout -b feat/vocab-surfaces
```

- [ ] **Step 2: Write failing tests** in a new `src/vocab_scan.rs` (module not
  yet declared, so write the file first; the test run in Step 3 fails to
  compile until Step 4's `mod` line — that counts as the red run):

```rust
//! Shared vocab-word scanner: tokenizes text lines against the lit.db word
//! set. Used by the main reading buffer, the gloss/journal overlay buffers,
//! and the chat panel's label specs. Word chars: alphanumeric, ' and ’
//! (same rule build_vocab_matches always used).

use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct VocabSpan {
    pub word: String,
    pub line_index: usize,
    pub char_start: usize,
    pub char_end: usize,
}

pub fn scan_lines<'a, I>(lines: I, words: &HashSet<String>, skip_upper: bool) -> Vec<VocabSpan>
where
    I: Iterator<Item = (usize, &'a str)>,
{
    let mut out = Vec::new();
    for (line_index, line_text) in lines {
        let trimmed = line_text.trim();
        if skip_upper
            && !trimmed.is_empty()
            && trimmed.chars().any(|c| c.is_alphabetic())
            && trimmed == trimmed.to_uppercase()
        {
            continue;
        }
        scan_line(line_text, line_index, words, &mut out);
    }
    out
}

/// Scan one line, pushing matches. CHAR offsets, not bytes.
pub fn scan_line(text: &str, line_index: usize, words: &HashSet<String>, out: &mut Vec<VocabSpan>) {
    let mut char_offset = 0usize;
    let mut in_word = false;
    let mut word_start = 0usize;
    let mut word_buf = String::new();
    let mut flush = |buf: &str, start: usize, end: usize, out: &mut Vec<VocabSpan>| {
        let lower = buf.to_lowercase();
        if words.contains(&lower) {
            out.push(VocabSpan { word: lower, line_index, char_start: start, char_end: end });
        }
    };
    for ch in text.chars() {
        let is_word_char = ch.is_alphanumeric() || ch == '\'' || ch == '\u{2019}';
        if is_word_char {
            if !in_word {
                word_start = char_offset;
                word_buf.clear();
                in_word = true;
            }
            word_buf.push(ch);
        } else if in_word {
            flush(&word_buf, word_start, char_offset, out);
            in_word = false;
        }
        char_offset += 1;
    }
    if in_word {
        flush(&word_buf, word_start, char_offset, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> HashSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn finds_words_with_char_offsets() {
        let spans = scan_lines(
            [(0usize, "Should censure thus on lovely gentlemen.")].into_iter(),
            &words(&["censure"]),
            false,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].word, "censure");
        assert_eq!(spans[0].char_start, 7);
        assert_eq!(spans[0].char_end, 14);
    }

    #[test]
    fn matches_are_case_insensitive_and_apostrophe_aware() {
        let spans = scan_lines(
            [(3usize, "PARLE and parle\u{2019}d")].into_iter(),
            &words(&["parle", "parle\u{2019}d"]),
            false,
        );
        // skip_upper=false: both tokens scanned; "PARLE" lowercases to a hit.
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].line_index, 3);
    }

    #[test]
    fn skip_upper_skips_speaker_header_lines() {
        let spans = scan_lines(
            [(0usize, "LUCETTA"), (1usize, "censure me")].into_iter(),
            &words(&["lucetta", "censure"]),
            true,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].word, "censure");
    }

    #[test]
    fn trailing_word_at_end_of_line_is_flushed() {
        let spans = scan_lines(
            [(0usize, "with parle")].into_iter(),
            &words(&["parle"]),
            false,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].char_end, 10);
    }
}
```

- [ ] **Step 3: Run tests, verify red** (module unreachable → compile error is
  the expected failure):

```bash
cargo test --bins vocab_scan 2>&1 | tail -5
```

- [ ] **Step 4: Declare the module.** In `src/main.rs`, next to the existing
  `mod vocab_lookup;` line, add:

```rust
mod vocab_scan;
```

- [ ] **Step 5: Delegate the main card.** In `src/app/mod.rs`
  `build_vocab_matches` (line 4436), replace the inner tokenizer loop (the
  `let mut char_offset = 0usize;` block through the trailing `if in_word`
  flush, lines ~4475-4513) with:

```rust
        let mut spans = Vec::new();
        crate::vocab_scan::scan_line(scan_text, line_idx, &state.vocab_words, &mut spans);
        state.vocab_matches.extend(spans.into_iter().map(|s| VocabMatch {
            word: s.word,
            line_index: s.line_index,
            char_start: s.char_start,
            char_end: s.char_end,
        }));
```

Keep the existing act/scene-marker skip, separator skip, and scansion-label
truncation exactly as they are — they are main-card-only policy and stay in
`build_vocab_matches`.

- [ ] **Step 6: Run tests, verify green**:

```bash
cargo test --bins vocab_scan 2>&1 | tail -3
cargo test --bins 2>&1 | tail -3
```

Expected: all tests pass (1008+ including 4 new).

- [ ] **Step 7: Commit**

```bash
git add src/vocab_scan.rs src/main.rs src/app/mod.rs
git commit -m "refactor(vocab): extract shared word scanner (vocab_scan)"
```

---

### Task 2: Compact two-column popup + Escape closes it (reader)

**Files:**
- Modify: `src/ui/vocab_popup.rs:118-131` (`place_float`),
  `src/theme.rs:1346-1360` (float CSS),
  `src/input/actions/escape.rs:8` (new first branch),
  `src/app/vocab_popup.rs:99-122` (call site, log line)

**Interfaces:**
- Produces: `VocabPopup::place_float(x, w, h)` keeps its signature but now
  renders a compact centered card; new log line
  `VOCAB POPUP: float rect x=.. w=..` for the e2e assertion (Task 12).

- [ ] **Step 1: Rewrite `place_float`** in `src/ui/vocab_popup.rs` (keep the
  doc comment location, replace body + comment):

```rust
    /// Two-column placement: a COMPACT card centered in the reading column
    /// the cursor is NOT in (x/w = that column's window-coord rect from
    /// layout::column_float_rect). Natural height, capped width — never the
    /// full column panel it used to be. `h` caps the card so long content
    /// can't overrun the card vertically.
    pub fn place_float(&self, x: i32, w: i32, h: i32) {
        self.container.add_css_class("vocab-popup-float");
        let width = (w - 48).clamp(200, 420);
        let centered_x = x + (w - width) / 2;
        self.container.set_halign(gtk4::Align::Start);
        self.container.set_valign(gtk4::Align::Center);
        self.container.set_margin_start(centered_x.max(0));
        self.container.set_margin_end(0);
        self.container.set_margin_bottom(0);
        self.container.set_width_request(width);
        self.container.set_height_request(-1);
        // Never taller than the card: content itself is short (definition +
        // etymology); the Journal view already caps its body height.
        let _ = h;
    }
```

- [ ] **Step 2: Fix the float contrast in `src/theme.rs`.** The base
  `.vocab-popup` colors (`{vocab_popup_fg}` etc.) are tuned for the `{root}`
  background, but `.vocab-popup-float` overrides the background to `{bg}` —
  that mismatch is the unreadable popup in the user's screenshot. Make the
  float keep the root background; it differs from the strip only by border
  and tighter padding. Replace the `.vocab-popup.vocab-popup-float` rule
  (theme.rs:1348-1350) with:

```text
         .vocab-popup.vocab-popup-float {{ background-color: {root}; \
           border: 1px solid alpha({fg}, 0.35); border-radius: 12px; \
           padding: 14px 18px; }} \
```

- [ ] **Step 3: Log the float rect** for e2e. In `src/app/vocab_popup.rs`
  `position_vocab_popup`, inside the `column_count() == 2` arm after
  `place_float`, add:

```rust
        crate::logging::log(&format!(
            "VOCAB POPUP: float col_x={x} col_w={w} card_h={card_h}"
        ));
```

- [ ] **Step 4: Escape closes the popup first.** In
  `src/input/actions/escape.rs` `escape_reader_mode`, insert as the FIRST
  block (before the toast block at line 15):

```rust
    // A visible vocab popup closes first — Escape inside the popup dismisses
    // it and nothing else (spec 2026-07-20 vocab-surfaces).
    {
        let mut s = state.borrow_mut();
        if s.vocab_popup.popup.is_visible() {
            s.vocab_popup.auto = false;
            crate::app::vocab_popup::close_vocab_popup(&mut s);
            crate::logging::log("ESCAPE: closed vocab popup");
            return;
        }
    }
```

- [ ] **Step 5: Build + unit tests**:

```bash
cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | tail -3
```

- [ ] **Step 6: Commit**

```bash
git add src/ui/vocab_popup.rs src/theme.rs src/input/actions/escape.rs src/app/vocab_popup.rs
git commit -m "fix(vocab): compact centered 2-col popup float; Escape closes the popup"
```

---

### Task 3: Neighbor-gloss query

**Files:**
- Modify: `src/db/queries.rs` (after `find_glossed_passages`, ~line 2051)
- Test: inline `#[cfg(test)]` beside the other queries tests (temp DB)

**Interfaces:**
- Consumes: `citation_parts` — `src/app/mod.rs:4607` parses
  `"ABBR.div1.div2.line"`; if it is private to app, parse locally as below.
- Produces:
  `pub struct NeighborGloss { pub start_citation: String, pub end_citation:
  String, pub gloss_text: String }` and
  `pub fn find_neighbor_glosses(conn: &Connection, work_abbrev: &str, div1:
  i64, div2: i64, start_line: i64, end_line: i64, gloss_type: &str, n: usize)
  -> Result<Vec<NeighborGloss>, rusqlite::Error>` — the `n` nearest
  preceding + `n` nearest following same-scene glossed passages, in reading
  order, excluding any passage overlapping [start_line, end_line].

- [ ] **Step 1: Write the failing test** (temp SQLite; append to
  `src/db/queries.rs` tests or its existing `#[cfg(test)]` module):

```rust
    #[test]
    fn neighbor_glosses_same_scene_nearest_two_each_side() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE passages (id INTEGER PRIMARY KEY, hash TEXT, \
               work_abbrev TEXT, start_citation TEXT, end_citation TEXT, \
               div1 INTEGER, div2 INTEGER, character TEXT, source_text TEXT); \
             CREATE TABLE glosses (id INTEGER PRIMARY KEY, passage_id INTEGER, \
               gloss_type TEXT, gloss_text TEXT);",
        )
        .unwrap();
        // Scene 1.2 line ranges: 1-3, 4-8, 9-12, 14-20, 21-25; scene 1.3: 1-4.
        let rows: &[(i64, &str, &str, i64, i64)] = &[
            (1, "TGV.1.2.1", "TGV.1.2.3", 1, 2),
            (2, "TGV.1.2.4", "TGV.1.2.8", 1, 2),
            (3, "TGV.1.2.9", "TGV.1.2.12", 1, 2),
            (4, "TGV.1.2.14", "TGV.1.2.20", 1, 2),
            (5, "TGV.1.2.21", "TGV.1.2.25", 1, 2),
            (6, "TGV.1.3.1", "TGV.1.3.4", 1, 3),
        ];
        for (id, s, e, d1, d2) in rows {
            conn.execute(
                "INSERT INTO passages (id, work_abbrev, start_citation, end_citation, div1, div2, source_text) \
                 VALUES (?1, 'TGV', ?2, ?3, ?4, ?5, '')",
                rusqlite::params![id, s, e, d1, d2],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO glosses (passage_id, gloss_type, gloss_text) \
                 VALUES (?1, 'reader-gloss', 'gloss for ' || ?2)",
                rusqlite::params![id, s],
            )
            .unwrap();
        }
        // New passage 1.2.9-12 == row 3's span: glossing lines 9..=12.
        let got = find_neighbor_glosses(&conn, "TGV", 1, 2, 9, 12, "reader-gloss", 2).unwrap();
        let cites: Vec<&str> = got.iter().map(|g| g.start_citation.as_str()).collect();
        // 2 nearest before (1-3, 4-8) + 2 nearest after (14-20, 21-25), in
        // reading order; the overlapping row 3 and scene 1.3 are excluded.
        assert_eq!(cites, vec!["TGV.1.2.1", "TGV.1.2.4", "TGV.1.2.14", "TGV.1.2.21"]);

        // n=1 keeps only the immediate neighbors.
        let got1 = find_neighbor_glosses(&conn, "TGV", 1, 2, 9, 12, "reader-gloss", 1).unwrap();
        let cites1: Vec<&str> = got1.iter().map(|g| g.start_citation.as_str()).collect();
        assert_eq!(cites1, vec!["TGV.1.2.4", "TGV.1.2.14"]);
    }
```

- [ ] **Step 2: Run, verify red**:

```bash
cargo test --bins neighbor_glosses 2>&1 | tail -5
```

Expected: FAIL — `find_neighbor_glosses` not found.

- [ ] **Step 3: Implement** (below `find_glossed_passages`):

```rust
pub struct NeighborGloss {
    pub start_citation: String,
    pub end_citation: String,
    pub gloss_text: String,
}

/// The `n` nearest preceding and `n` nearest following glossed passages in
/// the SAME scene (work + div1 + div2), by the trailing line number of the
/// citation, excluding any passage whose line range overlaps
/// [start_line, end_line]. Returned in reading order. Feeds the reader-gloss
/// "neighboring glosses — do not recycle their devices" prompt block.
pub fn find_neighbor_glosses(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    start_line: i64,
    end_line: i64,
    gloss_type: &str,
    n: usize,
) -> Result<Vec<NeighborGloss>, rusqlite::Error> {
    // Trailing-line-number extraction idiom shared with find_glossed_passages.
    let line_of = |col: &str| {
        format!("CAST(replace({col}, rtrim({col}, '0123456789'), '') AS INTEGER)")
    };
    let s_line = line_of("p.start_citation");
    let e_line = line_of("p.end_citation");
    let sql = format!(
        "SELECT p.start_citation, p.end_citation, g.gloss_text, {s_line} AS s_ln \
         FROM passages p JOIN glosses g ON g.passage_id = p.id \
         WHERE p.work_abbrev = ?1 AND p.div1 = ?2 AND p.div2 = ?3 \
           AND g.gloss_type = ?4 \
           AND NOT ({s_line} <= ?6 AND {e_line} >= ?5) \
         ORDER BY s_ln"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<(NeighborGloss, i64)> = stmt
        .query_map(
            rusqlite::params![work_abbrev, div1, div2, gloss_type, start_line, end_line],
            |r| {
                Ok((
                    NeighborGloss {
                        start_citation: r.get(0)?,
                        end_citation: r.get(1)?,
                        gloss_text: r.get(2)?,
                    },
                    r.get::<_, i64>(3)?,
                ))
            },
        )?
        .collect::<Result<_, _>>()?;
    let before: Vec<&(NeighborGloss, i64)> =
        rows.iter().filter(|(_, ln)| *ln < start_line).collect();
    let after: Vec<&(NeighborGloss, i64)> =
        rows.iter().filter(|(_, ln)| *ln > end_line).collect();
    let mut out: Vec<NeighborGloss> = Vec::new();
    for (g, _) in before.iter().rev().take(n).rev() {
        out.push(NeighborGloss {
            start_citation: g.start_citation.clone(),
            end_citation: g.end_citation.clone(),
            gloss_text: g.gloss_text.clone(),
        });
    }
    for (g, _) in after.iter().take(n) {
        out.push(NeighborGloss {
            start_citation: g.start_citation.clone(),
            end_citation: g.end_citation.clone(),
            gloss_text: g.gloss_text.clone(),
        });
    }
    Ok(out)
}
```

- [ ] **Step 4: Run, verify green**:

```bash
cargo test --bins neighbor_glosses 2>&1 | tail -3
```

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat(gloss): find_neighbor_glosses — same-scene nearest glossed passages"
```

---

### Task 4: Inject neighbors into the reader-gloss user message

**Files:**
- Modify: `src/gloss.rs:699-718` (`build_user_message` + new helper),
  `src/input/actions/chat.rs:1708-1760` (`request_reader_gloss`),
  `src/input/actions/gloss.rs:1436-1503` (`add_gloss` reader-gloss arm and
  the reader-gloss-edit path in the same file)
- Test: inline `#[cfg(test)]` in `src/gloss.rs`

**Interfaces:**
- Consumes: `find_neighbor_glosses` (Task 3), `GlossContext`
  (`src/gloss.rs:556` — has `work_abbrev` [canonical], `act`, `scene`,
  `source_line_numbers`).
- Produces: `pub fn neighbor_block(neighbors: &[crate::db::queries::NeighborGloss])
  -> Option<String>`; `pub fn neighbors_for_ctx(ctx: &GlossContext) ->
  Vec<crate::db::queries::NeighborGloss>`;
  `build_user_message` gains a 4th parameter
  `neighbors: &[crate::db::queries::NeighborGloss]`.

- [ ] **Step 1: Write failing tests** (in `src/gloss.rs`'s test module):

```rust
    #[test]
    fn neighbor_block_formats_citation_spans_and_rule() {
        use crate::db::queries::NeighborGloss;
        let n = vec![NeighborGloss {
            start_citation: "TGV.1.2.1".into(),
            end_citation: "TGV.1.2.3".into(),
            gloss_text: "<gloss>Julia fishes for advice.</gloss>".into(),
        }];
        let block = neighbor_block(&n).unwrap();
        assert!(block.contains("Neighboring glosses"));
        assert!(block.contains("do NOT recycle"));
        assert!(block.contains("TGV.1.2.1-TGV.1.2.3"));
        assert!(block.contains("Julia fishes for advice."));
        assert!(neighbor_block(&[]).is_none());
    }
```

- [ ] **Step 2: Run, verify red**:

```bash
cargo test --bins neighbor_block 2>&1 | tail -5
```

- [ ] **Step 3: Implement in `src/gloss.rs`.** Add near
  `build_user_message`:

```rust
/// The "don't recycle your neighbors' devices" prompt block. None when there
/// are no neighbors (the marker text must then be absent from the message).
pub fn neighbor_block(neighbors: &[crate::db::queries::NeighborGloss]) -> Option<String> {
    if neighbors.is_empty() {
        return None;
    }
    let mut block = String::from(
        "---\nNeighboring glosses (already written for ADJACENT passages in \
         this scene). Do NOT recycle their characterizing verbs, metaphors, \
         images, or other rhetorical devices — choose fresh, equally precise \
         language:\n",
    );
    for n in neighbors {
        block.push_str(&format!(
            "\n[{}-{}]\n{}\n",
            n.start_citation, n.end_citation, n.gloss_text
        ));
    }
    Some(block)
}

/// Fetch the 2-nearest-per-side same-scene reader-gloss neighbors for a
/// context. Failures degrade to no neighbors (generation must never block on
/// this).
pub fn neighbors_for_ctx(ctx: &GlossContext) -> Vec<crate::db::queries::NeighborGloss> {
    let (Some(first), Some(last)) = (
        ctx.source_line_numbers.first().copied(),
        ctx.source_line_numbers.last().copied(),
    ) else {
        return Vec::new();
    };
    match crate::db::queries::open_db().and_then(|conn| {
        crate::db::queries::find_neighbor_glosses(
            &conn, &ctx.work_abbrev, ctx.act, ctx.scene, first, last, "reader-gloss", 2,
        )
        .map_err(Into::into)
    }) {
        Ok(n) => n,
        Err(e) => {
            crate::logging::log(&format!("GLOSS NEIGHBORS: lookup failed: {e}"));
            Vec::new()
        }
    }
}
```

(If `open_db()`'s error type does not convert from `rusqlite::Error` with
`map_err(Into::into)`, match the two calls separately — follow the error
pattern of the surrounding `gloss.rs` DB helpers.)

Then change `build_user_message` to take and append neighbors:

```rust
pub fn build_user_message(
    ctx: &GlossContext,
    user_prompt: Option<&str>,
    existing_gloss: Option<&str>,
    neighbors: &[crate::db::queries::NeighborGloss],
) -> String {
    let mut msg = format!(
        "Play: \"{}\"\nAct: {}, Scene: {}\nSpeaker: {}\n\n{}",
        ctx.work_title, ctx.act, ctx.scene, ctx.speaker, ctx.source_text
    );

    if let Some(prompt) = user_prompt {
        msg.push_str(&format!("\n\n---\nUser question: {}", prompt));
    }

    if let Some(existing) = existing_gloss {
        msg.push_str(&format!("\n\n---\nPrevious gloss for reference:\n{}", existing));
    }

    if let Some(block) = neighbor_block(neighbors) {
        msg.push_str("\n\n");
        msg.push_str(&block);
    }

    msg
}
```

- [ ] **Step 4: Fix all call sites.** Find them:

```bash
rg -n 'build_user_message\(' src/
```

For the three READER-GLOSS paths, fetch and pass neighbors, and log:

  - `src/input/actions/chat.rs:1716` (`request_reader_gloss`):

```rust
    let neighbors = crate::gloss::neighbors_for_ctx(&ctx);
    crate::logging::log(&format!(
        "GLOSS NEIGHBORS: {} neighbor(s) for {}-{}",
        neighbors.len(), ctx.start_citation, ctx.end_citation
    ));
    let user_msg = crate::gloss::build_user_message(&ctx, None, None, &neighbors);
```

  - `src/input/actions/gloss.rs` `add_gloss` reader-gloss arm (~line 1463)
    and the reader-gloss-edit path: same pattern, keeping their existing
    `Some(&prompt_owned)` / existing-gloss arguments.
  - Every NON-reader-gloss call site passes `&[]`.

- [ ] **Step 5: Build + tests green**:

```bash
cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | tail -3
```

- [ ] **Step 6: Commit**

```bash
git add src/gloss.rs src/input/actions/chat.rs src/input/actions/gloss.rs
git commit -m "feat(gloss): inject same-scene neighbor glosses into reader-gloss prompts"
```

---

### Task 5: Prompt v8 — de-bias verbs + no-recycle rule

**Files:**
- Modify: `~/utono/claude-api-prompts/prompts/gloss.reader-gloss.md` (v8),
  `~/utono/claude-api-prompts/prompts/gloss.reader-gloss-question.md`,
  `~/utono/claude-api-prompts/prompts/gloss.reader-gloss-edit.md`
- Modify: `src/gloss.rs:431-452` (`READER_GLOSS_PROMPT` FALLBACK)
- Data: `~/utono/litdb/data/lit.db` `api_prompts` inserts

**Interfaces:**
- Consumes: the user-message "Neighboring glosses" marker text from Task 4's
  `neighbor_block` (the system prompt references it by name).

- [ ] **Step 1: Edit the master** `gloss.reader-gloss.md`:
  1. In the lede-verb example list, delete `fishes for, angles for` (keep the
     rest verbatim).
  2. Append to the Rules list:

```text
- If the user message contains a "Neighboring glosses" block, those glosses
  cover the passages immediately before/after this one and will be read
  back-to-back with yours. NEVER reuse their characterizing verbs, governing
  metaphors, images, or other rhetorical devices; say something new with
  fresh, equally precise language.
```

  Apply the same appended rule (only — they have no verb list) to
  `gloss.reader-gloss-question.md` and `gloss.reader-gloss-edit.md`.

- [ ] **Step 2: Load into api_prompts** (safe while the reader runs — prompts
  are read at launch only; the insert is brief). Follow the load/versioning
  procedure in `~/utono/claude-api-prompts/CLAUDE.md` (scripts/ has the
  loader). If loading manually, the shape per key is: insert the new text as
  `version = max(version)+1`, clear `is_active` on the old row, set it on the
  new row — the partial unique index enforces one active row per key.
  Verify:

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT prompt_key, version, is_active FROM api_prompts WHERE prompt_key LIKE 'gloss.reader-gloss%' ORDER BY prompt_key, version DESC;" | head -9
```

Expected: the three keys each show a new top version with `is_active=1`, and
the new `gloss.reader-gloss` text contains "Neighboring glosses" and not
"fishes for".

- [ ] **Step 3: Update the Rust FALLBACK** in `src/gloss.rs`
  `READER_GLOSS_PROMPT` — append the same no-recycle rule sentence to the
  FALLBACK's rules text (the fallback is the offline net; keep it terse).

- [ ] **Step 4: Build; commit both repos**:

```bash
cargo build 2>&1 | tail -2
cd ~/utono/claude-api-prompts && git add prompts/ && git commit -m "gloss.reader-gloss v8: drop fishes/angles examples; no-recycle-neighbors rule"
cd ~/utono/linux-lit && git add src/gloss.rs && git commit -m "feat(gloss): fallback prompt gains the neighboring-gloss no-recycle rule"
```

---

### Task 6: Popup scope refactor — words-explicit open + corner placement + chord hoist

**Files:**
- Modify: `src/app/vocab_popup.rs` (`open_vocab_popup` split, new
  `open_vocab_popup_for_words`), `src/ui/vocab_popup.rs` (`place_corner`),
  `src/input/keymap.rs:308-341` (hoist PendingR above the mode dispatch at
  keymap.rs:208)

**Interfaces:**
- Produces:
  - `pub enum VocabScope { CursorLine, Words(Vec<String>) }` (in
    `src/app/vocab_popup.rs`)
  - `pub fn open_vocab_popup_scoped(state: &mut AppState, scope: VocabScope,
    corner: bool)` — loads definitions/etymology/gloss for the words, shows
    the popup; `corner=true` uses `place_corner()` instead of
    `position_vocab_popup`.
  - `VocabPopup::place_corner(&self)` — lower-right, natural size.
  - `pub(crate) fn vocab_chord_toggle(state: &Rc<RefCell<AppState>>, scope:
    VocabScope, corner: bool)` in keymap.rs — the shared second-tap body.

- [ ] **Step 1: Add `place_corner`** to `src/ui/vocab_popup.rs` (below
  `place_float`):

```rust
    /// Overlay/chat placement: compact card anchored to the window's lower
    /// right, natural size (the popup floats above the overlay chain).
    pub fn place_corner(&self) {
        self.container.remove_css_class("vocab-popup-float");
        self.container.set_halign(gtk4::Align::End);
        self.container.set_valign(gtk4::Align::End);
        self.container.set_margin_start(0);
        self.container.set_margin_end(24);
        self.container.set_margin_bottom(24);
        self.container.set_width_request(-1);
        self.container.set_height_request(-1);
    }
```

(`place_strip` must also start with `remove_css_class("vocab-popup-float")` —
it already resets the class.)

- [ ] **Step 2: Split `open_vocab_popup`.** In `src/app/vocab_popup.rs`,
  refactor so word collection and data loading are separable:

```rust
pub enum VocabScope {
    CursorLine,
    Words(Vec<String>),
}

pub fn open_vocab_popup_scoped(state: &mut AppState, scope: VocabScope, corner: bool) {
    use crate::ui::vocab_popup::{VocabView, VocabWordData};
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };
    let work_abbrev = state.current_work.as_ref().map(|w| w.abbrev.clone());
    let citation = state.current_work.as_ref().and_then(|work| {
        let work_line = state.work_line_for_buffer(state.current_line)?;
        let line = work.lines.get(work_line)?;
        Some(line.citation.clone())
    });
    let words: Vec<String> = match scope {
        VocabScope::CursorLine => {
            let current_line = state.current_line;
            let mut seen = std::collections::HashSet::new();
            state
                .vocab_matches
                .iter()
                .filter(|m| m.line_index == current_line)
                .filter(|m| seen.insert(m.word.clone()))
                .map(|m| m.word.clone())
                .collect()
        }
        VocabScope::Words(w) => {
            let mut seen = std::collections::HashSet::new();
            w.into_iter().filter(|w| seen.insert(w.clone())).collect()
        }
    };
    if words.is_empty() {
        crate::logging::log("VOCAB POPUP: no vocab words in scope");
        return;
    }
    state.vocab_popup.data = words
        .into_iter()
        .map(|w| {
            let definition =
                crate::db::queries::load_vocab_definition(&conn, &w).map(|(d, _)| d);
            let etymology_markup = crate::db::queries::load_vocab_etymology(&conn, &w)
                .map(|e| format_etymology(&e, &crate::theme::vocab_popup_accent(&state.theme)));
            let gloss = match (&work_abbrev, &citation) {
                (Some(abbrev), Some(cit)) => {
                    crate::db::queries::load_vocab_gloss(&conn, &w, abbrev, cit)
                }
                _ => None,
            };
            VocabWordData { word: w, definition, etymology_markup, gloss }
        })
        .collect();
    state.vocab_popup.index = 0;
    state.vocab_popup.view = VocabView::Definition;
    state.vocab_popup.journal = None;
    state.vocab_popup.line = Some(state.current_line);
    if corner {
        state.vocab_popup.popup.place_corner();
    } else {
        position_vocab_popup(state);
    }
    show_vocab_popup(state);
}
```

Rewrite the existing `open_vocab_popup` body as
`open_vocab_popup_scoped(state, VocabScope::CursorLine, false)`; delete the
now-duplicated collection/loading code from it. `refresh_vocab_popup` keeps
its own cursor-line logic (it is a sync-follow path, main card only).

- [ ] **Step 3: Hoist the PendingR check.** In `src/input/keymap.rs`, MOVE the
  whole `if key_state.borrow().chord == ChordState::PendingR { ... }` block
  (lines 308-341) to ABOVE the `InputMode` mode dispatch (before line ~208),
  and generalize it:

```rust
    // rr chord, all vocab surfaces: a second quick `r` toggles the popup.
    // Runs BEFORE mode dispatch so the overlay/chat handlers share it; armed
    // only by surfaces that bind `r` to the vocab tap.
    if key_state.borrow().chord == ChordState::PendingR {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "r" && !is_ctrl && !is_shift && !is_alt {
            let mode = state.borrow().input_mode;
            match mode {
                crate::app::InputMode::Reader => {
                    vocab_chord_toggle(state, crate::app::vocab_popup::VocabScope::CursorLine, false);
                    return true;
                }
                crate::app::InputMode::GlossOverlay => {
                    let words = gloss_overlay_scope_words(state);
                    vocab_chord_toggle(state, crate::app::vocab_popup::VocabScope::Words(words), true);
                    return true;
                }
                crate::app::InputMode::JournalOverlay => {
                    let words = journal_overlay_scope_words(state);
                    vocab_chord_toggle(state, crate::app::vocab_popup::VocabScope::Words(words), true);
                    return true;
                }
                crate::app::InputMode::ChatTranscript => {
                    let words = chat_scope_words(state);
                    vocab_chord_toggle(state, crate::app::vocab_popup::VocabScope::Words(words), true);
                    return true;
                }
                _ => {}
            }
        }
    }
```

with the shared toggle body (a new fn in keymap.rs, extracted from the old
block so Reader behavior is IDENTICAL — including the highlight-enable +
persist on open):

```rust
fn vocab_chord_toggle(
    state: &Rc<RefCell<crate::app::AppState>>,
    scope: crate::app::vocab_popup::VocabScope,
    corner: bool,
) {
    let mut s = state.borrow_mut();
    if s.vocab_popup.popup.is_visible() {
        s.vocab_popup.auto = false;
        crate::app::vocab_popup::close_vocab_popup(&mut s);
        return;
    }
    if !s.vocab_highlight_visible {
        s.vocab_highlight_visible = true;
        crate::app::refresh_vocab_matches(&mut s);
        crate::app::apply_vocab_highlighting(&s);
        if let Some(abbrev) = s.current_work.as_ref().map(|w| w.abbrev.clone()) {
            if let Err(e) = crate::db::queries::open_db_rw().and_then(|conn| {
                crate::db::queries::set_vocab_highlight(&conn, &abbrev, true)
            }) {
                crate::logging::log(&format!("VOCAB: persist failed for {abbrev}: {e}"));
            }
        }
    }
    s.vocab_popup.auto = true;
    crate::app::vocab_popup::open_vocab_popup_scoped(&mut s, scope, corner);
}
```

The three `*_scope_words` helpers are written in Tasks 7-9; for THIS task add
them as stubs returning `Vec::new()` with a `// filled by Task 7/8/9` note
so the build stays green (a stub returning empty means "no vocab words in
scope" — correct until the surface is wired).

- [ ] **Step 4: Build + tests + commit**:

```bash
cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | tail -3
git add src/app/vocab_popup.rs src/ui/vocab_popup.rs src/input/keymap.rs
git commit -m "refactor(vocab): words-scoped popup open, corner placement, chord hoisted above mode dispatch"
```

---

### Task 7: Gloss overlay — vocab tag, r/rr, Ctrl+r rewrite, Escape, legend

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (vocab_tag field + `set_vocab_color` +
  `apply_vocab_tags` + `current_block_text`),
  `src/input/keymap.rs` (`handle_gloss_key` arms ~2294/2396/2434, the
  `gloss_overlay_scope_words` stub, gloss `is_ctrl` block ~2088+99),
  `src/input/actions/gloss.rs` (hook tag application after populate),
  `src/app/mod.rs` (startup color wiring next to the other overlay colors),
  `src/ui/gloss_keybinds_overlay.rs:9-48` (GROUPS)

**Interfaces:**
- Consumes: `vocab_scan::scan_lines` (Task 1), `vocab_chord_toggle` +
  `VocabScope::Words` (Task 6), `state.vocab_words`,
  `state.vocab_highlight_visible`.
- Produces: `GlossOverlay::apply_vocab_tags(&self, words: &HashSet<String>)`,
  `GlossOverlay::set_vocab_color(&self, color: &str)`,
  `GlossOverlay::current_block_text(&self) -> Option<String>`;
  `gloss_overlay_scope_words(state) -> Vec<String>` (fills Task 6's stub).

- [ ] **Step 1: Tag + color + apply.** In `GlossOverlay`: add a
  `vocab_tag: gtk4::TextTag` field, created in `new()` exactly like
  `search_tag` (placeholder color, registered once on
  `gloss_view.buffer()`'s tag table). Add:

```rust
    /// Theme wiring for the vocab-word tint (mirrors the main card's
    /// vocab_tag color). Called from build_window AND the theme-apply path.
    pub fn set_vocab_color(&self, color: &str) {
        self.vocab_tag.set_foreground(Some(color));
    }

    /// Re-scan the CURRENT buffer text and tint vocab words. Idempotent per
    /// populate: remove-then-apply so page turns never stack stale tags.
    pub fn apply_vocab_tags(&self, words: &std::collections::HashSet<String>) {
        let buffer = self.gloss_view.buffer();
        let (start, end) = (buffer.start_iter(), buffer.end_iter());
        buffer.remove_tag(&self.vocab_tag, &start, &end);
        if words.is_empty() {
            return;
        }
        let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        let spans = crate::vocab_scan::scan_lines(
            text.lines().enumerate(),
            words,
            true, // skip all-caps speaker header lines
        );
        for s in spans {
            if let Some(mut line_iter) = buffer.iter_at_line(s.line_index as i32) {
                let mut a = line_iter.clone();
                a.forward_chars(s.char_start as i32);
                line_iter.forward_chars(s.char_end as i32);
                buffer.apply_tag(&self.vocab_tag, &a, &line_iter);
            }
        }
    }
```

- [ ] **Step 2: Hook every populate path.** In `src/input/actions/gloss.rs`,
  the render/page fns that call `populate_gloss_buffer`/`set_text` for the
  gloss overlay also run the cached-audio recoloring
  (`recolor_cached_blocks_rc`, gloss.rs:1767). Immediately after each such
  populate/recolor call add:

```rust
    if s.vocab_highlight_visible {
        s.gloss_overlay.apply_vocab_tags(&s.vocab_words);
    }
```

Find every site with:

```bash
rg -n 'populate_gloss_buffer|recolor_cached_blocks' src/input/actions/gloss.rs src/ui/gloss_overlay.rs
```

Also call it after `apply_font` re-renders (font cycling re-populates).

- [ ] **Step 3: Startup + theme color.** In `src/app/mod.rs` `build_window`,
  next to the existing `gloss_overlay` color setters, add
  `gloss_overlay.set_vocab_color(...)` using the same theme color the main
  card's `vocab_tag` uses (see its construction at `mod.rs:1317-1321` for
  the exact theme field/helper). Add the same call in
  `apply_theme_to_state`.

- [ ] **Step 4: Keys.** In `handle_gloss_key`:
  - Replace the `"r" => true` no-op arm (keymap.rs:~2434) with:

```rust
        "r" if !is_ctrl => {
            if state.borrow().vocab_popup.popup.is_visible() {
                let mut s = state.borrow_mut();
                crate::app::vocab_popup::vocab_popup_next(&mut s);
            }
            KeyState::start_chord(key_state, ChordState::PendingR);
            true
        }
```

  (this requires threading `key_state` into `handle_gloss_key` — change its
  signature to accept `key_state: &Rc<RefCell<KeyState>>` and pass it at the
  mode-dispatch call site, keymap.rs:229).
  - Move the old `"R"` rewrite arm (2294-2297) behind Ctrl: in the gloss
    handler's `is_ctrl` block add `"r"` → `gloss::begin_rewrite(state)`;
    make plain `"R"` a consumed no-op with a comment
    (`// vocab R reserved unbound, mirrors main card`).
  - Escape precedence: at the TOP of the existing Escape arm (2396) insert:

```rust
                if state.borrow().vocab_popup.popup.is_visible() {
                    let mut s = state.borrow_mut();
                    s.vocab_popup.auto = false;
                    crate::app::vocab_popup::close_vocab_popup(&mut s);
                    return true;
                }
```

- [ ] **Step 5: Scope words.** Fill Task 6's stub in keymap.rs:

```rust
fn gloss_overlay_scope_words(state: &Rc<RefCell<crate::app::AppState>>) -> Vec<String> {
    let s = state.borrow();
    let text = match s.gloss_overlay.current_block_text() {
        Some(t) => t,
        None => return Vec::new(),
    };
    crate::vocab_scan::scan_lines(text.lines().enumerate(), &s.vocab_words, true)
        .into_iter()
        .map(|sp| sp.word)
        .collect()
}
```

`GlossOverlay::current_block_text` exposes the text of the overlay's current
cursor block — implement it on `GlossOverlay` by reusing the SAME block
resolution the Ctrl+Space read-current-block TTS path uses
(`begin_current_block` in `src/input/actions/gloss.rs` resolves the block
index + text; lift that resolution into the overlay method and have the TTS
path call it too, so there is exactly one "current block" definition).

- [ ] **Step 6: Legend.** In `src/ui/gloss_keybinds_overlay.rs` GROUPS:
  change line 30's `("R", "ask Claude to rewrite this gloss")` to
  `("C-r", "ask Claude to rewrite this gloss")`, and add to the reading
  group: `("r", "vocab popup (rr toggles \u{b7} r next word)")` and
  `("Esc", "close vocab popup / close")` on the Esc row (adjust the Esc row
  text at line 48).

- [ ] **Step 7: Build + tests + commit**:

```bash
cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | tail -3
git add src/ui/gloss_overlay.rs src/input/keymap.rs src/input/actions/gloss.rs src/app/mod.rs src/ui/gloss_keybinds_overlay.rs
git commit -m "feat(gloss-overlay): vocab tint + rr popup; rewrite moves to Ctrl+r"
```

---

### Task 8: Journal overlay — vocab tag, r/rr, Ctrl+r ask, Ctrl+w rewrite, legend

**Files:**
- Modify: `src/ui/journal_overlay.rs` (vocab_tag + `set_vocab_color` +
  `apply_vocab_tags` + `current_block_text`), `src/input/keymap.rs`
  (`handle_journal_key` arms ~1905/1912/2057, `journal_overlay_scope_words`),
  `src/input/actions/journal.rs` (hook after populate paths),
  `src/app/mod.rs` (startup color), `src/ui/journal_keybinds_overlay.rs:24-25`

**Interfaces:**
- Consumes: same as Task 7.
- Produces: `JournalOverlay::{set_vocab_color, apply_vocab_tags,
  current_block_text}` (same signatures as GlossOverlay's);
  `journal_overlay_scope_words(state) -> Vec<String>`.

- [ ] **Step 1: Tag + apply.** Mirror Task 7 Step 1 on `JournalOverlay`
  (buffer = `self.view.buffer()`; register next to `search_tag`,
  journal_overlay.rs:135). Same `apply_vocab_tags` body. Hook it after every
  body populate (`self.view.buffer().set_text(&body)` sites at 805/859/663,
  the `populate_gloss_buffer` site at 842, and the `apply_font` re-render),
  gated on `vocab_highlight_visible` at the action-layer call sites in
  `src/input/actions/journal.rs` (`render_current` and its page-turn
  siblings — find with `rg -n 'render_current|set_text' src/input/actions/journal.rs src/ui/journal_overlay.rs`).
  `current_block_text` reuses the journal block resolution that
  `begin_current_journal_block` (keymap.rs:1912 area → `gloss.rs`) uses —
  same lift-into-method move as Task 7.

- [ ] **Step 2: Startup + theme color** — same two wiring points as Task 7
  Step 3, for `journal_overlay`.

- [ ] **Step 3: Keys.** In `handle_journal_key` (thread `key_state` through,
  same signature change as Task 7):
  - `"r"` (1905-1908): replace `journal::begin_ask(state)` with the vocab
    tap arm (same code as Task 7 Step 4's `"r"` arm). NOTE: keep the
    term-filter intercept at 1894-1899 ABOVE it unchanged (filter active →
    clear-filter toast still wins).
  - `"R"` (1912-1915): consumed no-op with comment.
  - In the journal `is_ctrl` block (starts ~keymap.rs line 1803 within the
    handler; the block that handles Ctrl+Shift+n/p/r): add plain-ctrl arms
    `"r"` → `crate::input::actions::journal::begin_ask(state); true` and
    `"w"` → `crate::input::actions::journal::open_rewrite_target(state);
    true` (ensure they sit AFTER the Ctrl+Shift+r revision arm so shift
    still wins).
  - Escape arm (2057): same popup-first insert as Task 7 Step 4.
  - `journal_overlay_scope_words`: same shape as Task 7 Step 5 with
    `s.journal_overlay.current_block_text()`.

- [ ] **Step 4: Legend.** `src/ui/journal_keybinds_overlay.rs`: line 24
  `("r", "ask a new question")` → `("C-r", "ask a new question")`; line 25
  `("R", "ask Claude to rewrite this Q&A")` →
  `("C-w", "ask Claude to rewrite this Q&A")`; add
  `("r", "vocab popup (rr toggles \u{b7} r next word)")` to the reading
  group.

- [ ] **Step 5: Build + tests + commit**:

```bash
cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | tail -3
git add src/ui/journal_overlay.rs src/input/keymap.rs src/input/actions/journal.rs src/app/mod.rs src/ui/journal_keybinds_overlay.rs
git commit -m "feat(journal-overlay): vocab tint + rr popup; ask/rewrite move to Ctrl+r/Ctrl+w"
```

---

### Task 9: Chat panel — vocab spans, r/rr, Ctrl+r/Ctrl+w, legend

**Files:**
- Modify: `src/ui/chat_panel.rs:407-441` (`append_spec_label` + highlight
  state), `src/input/keymap.rs:1595-1615` (`handle_chat_transcript_key` +
  `chat_scope_words`), `src/input/actions/chat.rs` (set highlight state on
  render), `src/ui/chat_keybinds_overlay.rs:28`
- Test: extend `src/ui/chat_panel.rs` `#[cfg(test)]` (it exists —
  `row_widget_specs_explodes_gloss_and_marks_groups` at line 682)

**Interfaces:**
- Consumes: `vocab_scan::scan_line`, `vocab_chord_toggle` (Task 6).
- Produces: `ChatPanel::set_vocab_highlight(&self, words:
  std::collections::HashSet<String>, color: Option<String>)` (stored in new
  `RefCell` fields; empty set or None color disables);
  `pub(crate) fn vocab_markup(text: &str, words: &HashSet<String>, color:
  &str) -> Option<String>` (free fn in chat_panel.rs — returns Pango markup
  with matches wrapped, or None when no match);
  `chat_scope_words(state) -> Vec<String>`.

- [ ] **Step 1: Write the failing test** for the markup helper:

```rust
    #[test]
    fn vocab_markup_escapes_and_wraps_matches() {
        let words: std::collections::HashSet<String> =
            ["censure".to_string()].into_iter().collect();
        let m = vocab_markup("Should censure <thus> on gentlemen.", &words, "#ffcc66").unwrap();
        assert!(m.contains("&lt;thus&gt;"), "text must be escaped: {m}");
        assert!(m.contains("<span foreground=\"#ffcc66\">censure</span>"), "{m}");
        assert!(vocab_markup("no matches here", &words, "#ffcc66").is_none());
    }
```

Run red:

```bash
cargo test --bins vocab_markup 2>&1 | tail -5
```

- [ ] **Step 2: Implement `vocab_markup`** (in chat_panel.rs):

```rust
/// Pango markup for a chat label: vocab matches wrapped in a colored span,
/// everything escaped. None when the text has no match (caller keeps plain
/// set_text — cheaper and avoids markup parsing for the common case).
pub(crate) fn vocab_markup(
    text: &str,
    words: &std::collections::HashSet<String>,
    color: &str,
) -> Option<String> {
    let mut spans = Vec::new();
    crate::vocab_scan::scan_line(text, 0, words, &mut spans);
    if spans.is_empty() {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut pos = 0usize;
    for s in &spans {
        let before: String = chars[pos..s.char_start].iter().collect();
        let hit: String = chars[s.char_start..s.char_end].iter().collect();
        out.push_str(&glib::markup_escape_text(&before));
        out.push_str(&format!(
            "<span foreground=\"{}\">{}</span>",
            color,
            glib::markup_escape_text(&hit)
        ));
        pos = s.char_end;
    }
    let rest: String = chars[pos..].iter().collect();
    out.push_str(&glib::markup_escape_text(&rest));
    Some(out)
}
```

- [ ] **Step 3: Store + apply.** Add to `ChatPanel`:

```rust
    /// Vocab-highlight word set + span color for transcript labels; empty
    /// set disables. Set by chat render paths from AppState (the panel has
    /// no state access of its own).
    vocab_words: std::cell::RefCell<std::collections::HashSet<String>>,
    vocab_color: std::cell::RefCell<Option<String>>,
```

(init empty in `new()`), plus:

```rust
    pub fn set_vocab_highlight(
        &self,
        words: std::collections::HashSet<String>,
        color: Option<String>,
    ) {
        *self.vocab_words.borrow_mut() = words;
        *self.vocab_color.borrow_mut() = color;
    }
```

In `append_spec_label`, after the CSS classes are added and BEFORE returning,
apply markup when enabled (GlossAnswer rows arrive as plain text specs —
`chat_gloss_rows` output is text, so escaping is safe everywhere):

```rust
        if let Some(color) = self.vocab_color.borrow().as_deref() {
            let words = self.vocab_words.borrow();
            if !words.is_empty() {
                if let Some(markup) = vocab_markup(&w.text, &words, color) {
                    label.set_markup(&markup);
                }
            }
        }
```

- [ ] **Step 4: Feed it from state.** In `src/input/actions/chat.rs`, in the
  transcript render path (the fn that calls the panel's
  `rebuild_from_specs`/`render_page` — find with
  `rg -n 'render_page|rebuild_from_specs' src/input/actions/chat.rs src/ui/chat_panel.rs`),
  set the highlight state first:

```rust
    let (words, color) = if s.vocab_highlight_visible {
        (s.vocab_words.clone(), Some(crate::theme::vocab_popup_accent(&s.theme)))
    } else {
        (std::collections::HashSet::new(), None)
    };
    s.chat_panel.set_vocab_highlight(words, color);
```

(Use the same theme color as the main card's vocab_tag — check
`mod.rs:1317-1321`; if it is not `vocab_popup_accent`, use that field.)

- [ ] **Step 5: Keys.** In `handle_chat_transcript_key` (thread `key_state`
  through like Tasks 7/8):
  - `"r"` (1595-1605) → the vocab tap arm (same as Task 7 Step 4).
  - `"R"` (1606-1615) → consumed no-op with comment.
  - In its ctrl section add: `"r" if is_ctrl && !is_shift` → the OLD `r`
    body verbatim (Journal view → `chat::focus_prompt_insert`; else
    `chat::regloss_pinned(state)`); `"w" if is_ctrl` → the OLD `R` body
    verbatim (Journal view → `chat::rewrite_journal_entry`; else
    `chat::regloss_pinned`).
  - Escape arm (1682): popup-first insert as in Task 7.
  - `chat_scope_words`: selected exchange's text via the same accessor the
    yank path uses (`row_widget_texts`, chat_panel.rs:507) filtered to the
    SELECTED exchange, scanned per line:

```rust
fn chat_scope_words(state: &Rc<RefCell<crate::app::AppState>>) -> Vec<String> {
    let s = state.borrow();
    let texts = s.chat_panel.selected_row_texts(); // add: the selected exchange's label texts
    let mut out = Vec::new();
    for t in &texts {
        crate::vocab_scan::scan_line(t, 0, &s.vocab_words, &mut out);
    }
    out.into_iter().map(|sp| sp.word).collect()
}
```

  Add `ChatPanel::selected_row_texts(&self) -> Vec<String>` by reusing the
  selection bookkeeping the `s`/yank paths use (the panel knows the selected
  exchange for `save_selected_exchange`).

- [ ] **Step 6: Legend.** `src/ui/chat_keybinds_overlay.rs` line 28:
  `("r / R", "Gloss view: re-gloss \u{b7} Journal view: ask / rewrite")` →
  two rows: `("C-r", "Gloss view: re-gloss \u{b7} Journal view: ask")`,
  `("C-w", "Gloss view: re-gloss \u{b7} Journal view: rewrite")`, plus
  `("r", "vocab popup (rr toggles \u{b7} r next word)")`.

- [ ] **Step 7: Green + commit**:

```bash
cargo test --bins 2>&1 | tail -3
git add src/ui/chat_panel.rs src/input/keymap.rs src/input/actions/chat.rs src/ui/chat_keybinds_overlay.rs
git commit -m "feat(chat): vocab spans in transcript + rr popup; re-gloss/rewrite move to Ctrl+r/Ctrl+w"
```

---

### Task 10: Dedicated add-vocab card + open from any surface

**Files:**
- Modify: `src/input/actions/vocab_add.rs` (whole open/close path),
  `src/app/mod.rs` (new `vocab_add_card: crate::ui::ask_card::AskCard` field
  + attach in build_window + `vocab_add_return_mode: Option<InputMode>`
  field), `src/input/keymap.rs` (AddVocab mode handler at 132-135 feeds the
  new card; `Ctrl+Alt+backslash` arms in the gloss/journal/chat handlers),
  `src/app/mod.rs:4552` (`apply_after_add` cross-surface refresh)

**Interfaces:**
- Consumes: `AskCard::new(text_margins, return_focus)` +
  `open(title, hint, legend, card_width, block_fill, block_fg)` (see
  chat_panel.rs:104/445 for the call shapes), the vim feed the ChatPrompt
  handler uses (`handle_chat_prompt_key`, keymap.rs:1434) as the model for
  key feeding.
- Produces: `vocab_add::open(state_rc)` now opens over ANY of
  Reader/GlossOverlay/JournalOverlay/ChatTranscript and restores that mode
  on close; `vocab_add::submit` unchanged externally.

- [ ] **Step 1: AppState + widget.** In `src/app/mod.rs` add fields:

```rust
    /// Compact floating add-vocab input (Ctrl+Alt+\). Its own AskCard so it
    /// can open OVER the gloss/journal overlays (the old gloss-overlay reuse
    /// couldn't — the gloss overlay was either busy or below the journal).
    pub vocab_add_card: crate::ui::ask_card::AskCard,
    /// Mode to restore when the add-vocab card closes (it can open from
    /// Reader, either overlay, or the chat transcript).
    pub vocab_add_return_mode: Option<InputMode>,
```

In `build_window`, construct it next to the chat panel construction
(`AskCard::new(0, &text_view /* return_focus */)`), add CSS class
`"vocab-add-card"`, set `halign Center / valign Center`, cap size
(`set_input_height(56)`; width via `set_size_request(560, -1)` on
`container()`), and attach:
`corpus_search_popup.overlay.add_overlay(vocab_add_card.container());`
(same layer as the vocab popup — above the overlay chain). Theme: give
`.vocab-add-card` the `.vocab-popup` background/border treatment in
`theme.rs generate_css`.

- [ ] **Step 2: Rework `vocab_add.rs` open/close**:

```rust
pub(crate) fn open(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    if s.current_work.is_none() {
        return;
    }
    let prior = s.input_mode;
    s.vocab_add_return_mode = Some(prior);
    s.vocab_add_card.open(
        "Add vocab word",
        ":w add \u{b7} Esc cancel",
        "",
        0,
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
    );
    // Seed INSERT so the reader can type immediately (mirror the chat `a`).
    s.vocab_add_card.seed_insert();
    s.input_mode = crate::app::InputMode::AddVocab;
    crate::logging::log("VOCAB ADD: opened input card");
}

pub(crate) fn close(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    s.vocab_add_card.close();
    let back = s.vocab_add_return_mode.take().unwrap_or(crate::app::InputMode::Reader);
    if back == crate::app::InputMode::Reader {
        crate::app::return_to_reader_mode(&mut s);
    } else {
        s.input_mode = back;
    }
    crate::logging::log("VOCAB ADD: closed");
}
```

`submit` keeps its body but reads
`state_rc.borrow().vocab_add_card.take_text()` instead of
`gloss_overlay.edit_buffer_text()`. Match the REAL AskCard method names —
check with `rg -n 'pub fn' src/ui/ask_card.rs`: use its open/close/text/vim
feed methods exactly as the journal ask host does
(`rg -n 'ask_host|input\.' src/input/actions/journal.rs src/ui/journal_overlay.rs`
shows the living call pattern). If AskCard lacks a "seed insert" helper,
feed `VimKey::Char('i')` the way the old code did.

- [ ] **Step 3: AddVocab key handler.** The AddVocab mode arm
  (keymap.rs:132-135) currently feeds `gloss_overlay`; repoint it to feed
  `vocab_add_card`'s vim engine, mirroring `handle_chat_prompt_key`'s feed
  loop (`:w` → `vocab_add::submit`, `:q`/Escape → `vocab_add::close`).

- [ ] **Step 4: Ctrl+Alt+\ from the three surfaces.** Reader already
  dispatches `Action::AddVocabWord`. Add to each of
  `handle_gloss_key` / `handle_journal_key` / `handle_chat_transcript_key`,
  near their other early ctrl checks:

```rust
    if is_ctrl && is_alt && key_name == "backslash" {
        crate::input::actions::vocab_add::open(state);
        return true;
    }
```

(The gloss/journal handlers take `is_alt`; if a handler doesn't, thread it
through from the dispatch site like `is_ctrl`.)

- [ ] **Step 5: Cross-surface refresh.** At the END of `apply_after_add`
  (`src/app/mod.rs:4552`), refresh whichever surfaces are showing:

```rust
    if state.gloss_overlay.is_visible() {
        state.gloss_overlay.apply_vocab_tags(&state.vocab_words);
    }
    if state.journal_overlay.is_visible() {
        state.journal_overlay.apply_vocab_tags(&state.vocab_words);
    }
    if state.chat.pinned() {
        // re-render transcript so labels pick up the new word set
        crate::input::actions::chat::refresh_transcript_render(state);
    }
```

(Use the panel's existing whole-transcript re-render fn — find it with
`rg -n 'fn .*render' src/input/actions/chat.rs`; if the visibility accessors
differ (`is_visible` vs a state flag), use the flags the Escape arms use.
The popup-refresh block above this point uses cursor-line scope — keep it,
it is correct for the Reader case; for overlay/chat return modes the popup
refresh is skipped by its own is_visible check.)

- [ ] **Step 6: Green + commit**:

```bash
cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | tail -3
git add src/input/actions/vocab_add.rs src/app/mod.rs src/input/keymap.rs src/theme.rs src/ui/ask_card.rs
git commit -m "feat(vocab): dedicated add-vocab card opens over any surface (Ctrl+Alt+\\)"
```

---

### Task 11: Main Ctrl+/ overlay — add the missing Ctrl+Alt+\ entry

**Files:**
- Modify: `src/ui/keybinds_overlay.rs:63` (backslash keycap row) and the
  describe()/detail arms (~319-331, 463-505)

- [ ] **Step 1:** Invoke the `update-cairo-keybinds-overlay` skill and follow
  its three-pass cross-reference to add `("M-C-\\", "add vocab")` to the
  backslash row (line 63) and a matching describe() detail
  (`Action::AddVocabWord — src/input/actions/vocab_add.rs`). No other main
  card binds changed in this feature (r/rr/Ctrl+r were already documented).

- [ ] **Step 2: Build + commit**:

```bash
cargo build 2>&1 | tail -2
git add src/ui/keybinds_overlay.rs
git commit -m "docs(overlay): Ctrl+Alt+\\ add-vocab in the main keybinds overlay"
```

---

### Task 12: Chat-panel focus rule

**Files:**
- Modify: `src/ui/chat_panel.rs` (header rule), `src/app/mod.rs` (card rule
  overlay child + AppState field), `src/theme.rs` (`.focus-rule` CSS),
  `src/input/actions/chat.rs` (visibility driver)

**Interfaces:**
- Produces: `ChatPanel::set_focus_rule_visible(&self, on: bool)`;
  `pub(crate) fn update_focus_rules(s: &AppState)` in chat.rs, called from
  `focus_reader` / `focus_transcript` / `focus_prompt_in_mode` and the panel
  open/close/regate paths.

- [ ] **Step 1: CSS** in `theme.rs generate_css` (near `.chat-panel` rules):

```text
         .focus-rule {{ background-color: alpha({fg}, 0.55); \
           border-radius: 1px; }} \
```

- [ ] **Step 2: Panel rule.** In `ChatPanel::new`, before
  `container.append(&transcript_scroll)`:

```rust
        // Focus cue: a short rule (~ three hyphens) centered at the panel
        // top, visible only while the panel (transcript or prompt) has
        // focus. The main card has a twin; exactly one shows at a time.
        let focus_rule = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        focus_rule.add_css_class("focus-rule");
        focus_rule.set_size_request(24, 2);
        focus_rule.set_halign(gtk4::Align::Center);
        focus_rule.set_margin_bottom(6);
        focus_rule.set_visible(false);
        container.append(&focus_rule);
```

Store `focus_rule` as a field; add:

```rust
    pub fn set_focus_rule_visible(&self, on: bool) {
        self.focus_rule.set_visible(on);
    }
```

- [ ] **Step 3: Card rule.** In `build_window` (mod.rs ~1502, where
  `page_turn_overlay` wraps `card_vbox`), add an overlay child:

```rust
    let card_focus_rule = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    card_focus_rule.add_css_class("focus-rule");
    card_focus_rule.set_size_request(24, 2);
    card_focus_rule.set_halign(gtk4::Align::Center);
    card_focus_rule.set_valign(gtk4::Align::Start);
    card_focus_rule.set_margin_top(36); // inside TOP_SPACER_HEIGHT=74
    card_focus_rule.set_visible(false);
    page_turn_overlay.add_overlay(&card_focus_rule);
```

Store on AppState as `pub card_focus_rule: gtk4::Box`.

- [ ] **Step 4: Driver** in `src/input/actions/chat.rs`:

```rust
/// One rule shows at a time, only while the chat panel is open: the panel's
/// when the panel has focus (transcript or prompt), the card's when the
/// reader does. Both hidden when the panel is closed.
pub(crate) fn update_focus_rules(s: &AppState) {
    let open = panel_is_open(s); // use the SAME predicate the Tab toggle uses
    let panel_focused = matches!(
        s.input_mode,
        crate::app::InputMode::ChatTranscript | crate::app::InputMode::ChatPrompt
    );
    s.chat_panel.set_focus_rule_visible(open && panel_focused);
    s.card_focus_rule.set_visible(open && !panel_focused);
}
```

(`panel_is_open`: reuse the pinned/visibility predicate the existing
open/close/Tab code uses — find with
`rg -n 'pinned|panel_open|container.is_visible' src/input/actions/chat.rs | head`.)
Call `update_focus_rules(s)` at the end of `focus_reader`,
`focus_transcript`, `focus_prompt_in_mode`, and in the panel open, close,
and regate paths (the regate is tick-deferred — call inside the deferred
closure, per the chat-layout memory). Also log for e2e:

```rust
    crate::logging::log(&format!(
        "FOCUS RULE: panel={} card={}",
        open && panel_focused, open && !panel_focused
    ));
```

- [ ] **Step 5: Green + commit**:

```bash
cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | tail -3
git add src/ui/chat_panel.rs src/app/mod.rs src/theme.rs src/input/actions/chat.rs
git commit -m "feat(chat): focus rule above the focused surface while the panel is open"
```

---

### Task 13: Headless e2e verification

**Files:**
- Create: `tests/vocab_popup_2col.rs` (`#[ignore]`d, harness-based)
- Modify: none expected (log lines were added in Tasks 2 and 12)

- [ ] **Step 1: Write the e2e test** following the `tests/overlay_clipping.rs`
  pattern (Harness::launch with `LIT_START_WORK` on a TWO-COLUMN play with
  vocab words — use the same work existing vocab e2e/docs use; confirm one
  with `sqlite3 ~/utono/litdb/data/lit.db "SELECT COUNT(*) FROM vocab_words;"`
  and a known play like Cym):
  - drive `r`,`r` (two `h.key("r", 250)` calls), wait, then assert the log
    (this run's `LIT_LOG_PATH`) contains `VOCAB POPUP: float col_x=` and
    parse `col_x/col_w`; assert `col_w - 48 >= 200` popup width fits the
    column (geometry sanity, matching Task 2's clamp).
  - screenshot via `h.screenshot()` into `target/ui/` for the visual pass.
  - send Escape; assert a subsequent log line `ESCAPE: closed vocab popup`.
  Use the harness helpers exactly as `tests/journal_clipping.rs` does
  (`wait_for` + log polling — copy its wait helper usage).

- [ ] **Step 2: Run the e2e battery**:

```bash
./scripts/e2e-env.sh cargo test --test vocab_popup_2col -- --ignored --nocapture 2>&1 | tail -15
./scripts/e2e-env.sh cargo test -- --ignored --nocapture 2>&1 | tail -15
```

Expected: PASS. Per the UI review protocol, OPEN every PNG in `target/ui/`
and report what is visible (popup readable? compact? centered in the
non-cursor column? no clipping). If any clipping is found: fix, and add the
failure mode to `docs/troubleshooting/clip-prevention.md` (required).

- [ ] **Step 3: Overlay + chat surfaces smoke.** Use the
  `verify-overlay-ui` skill for the gloss/journal overlay invariants after
  the vocab-tag changes, and drive one manual cage session for the chat
  focus rule (`Tab` cycling) asserting the `FOCUS RULE:` log lines flip.

- [ ] **Step 4: Full unit battery + clippy**:

```bash
cargo test --bins 2>&1 | tail -3 && cargo clippy 2>&1 | tail -5
```

- [ ] **Step 5: Commit**

```bash
git add tests/vocab_popup_2col.rs docs/troubleshooting/clip-prevention.md
git commit -m "test(e2e): 2-col vocab popup geometry + Escape close"
```

---

### Task 14: Finish the branch

- [ ] **Step 1:** Verify clean tree + full battery green
  (`cargo build`, `cargo test --bins`, e2e battery from Task 13).
- [ ] **Step 2:** Prompt the user to choose final testing (project rule):
  headless run already done — offer the exact manual steps (open a 2-col
  play → `rr` → Escape; open gloss/journal overlays → `rr`, `Ctrl+r`,
  `Ctrl+Alt+\`; Tab-cycle the chat panel and watch the rule; generate one
  new gloss next to an existing one and read it for recycled metaphors) OR
  rely on the headless pass — ask, don't assume.
- [ ] **Step 3:** After user approval: merge per the house convention —
  `git checkout master && git merge --no-ff feat/vocab-surfaces`, re-verify
  build, `git push origin master`, `git branch -d feat/vocab-surfaces`.
- [ ] **Step 4:** Update `ac` (context break likely after a feature this
  size): current state, the api_prompts v8 rollout note (restart required),
  and the pending live-eyeball list.

---

## Self-Review Notes (resolved)

- Spec coverage: A→Tasks 3-5; B→Task 2; C→Tasks 1, 6-10; D→Tasks 7-9, 11;
  E→Task 12; testing→Tasks 13-14. Memory-practice note was saved during the
  spec phase (agent memory, no task).
- Type consistency: `VocabSpan`/`VocabScope`/`NeighborGloss` names used
  consistently across Tasks 1/3/4/6/7/8/9; `vocab_chord_toggle` (Task 6)
  consumed by 7-9; `apply_vocab_tags` signature identical on both overlays.
- Known adaptation points (deliberate, with lookup commands in place):
  AskCard method names (Task 10), the current-block resolution lift
  (Tasks 7/8), the chat re-render fn name (Task 10), `is_alt` threading
  (Task 10). Each names the exact existing code to mirror.
