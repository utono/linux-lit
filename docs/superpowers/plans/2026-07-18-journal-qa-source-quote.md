# Journal Q&A Source-Quote Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In the journal Q&A overlay, render a passage entry's quoted source (small-caps speaker + hang-indented verse + compact citation + `———` separator) as navigable blocks above the question, on the entry's first page only.

**Architecture:** Two pure helpers in `journal.rs` build the source paragraphs (`format_source_citation`, `source_paragraphs`). `journal_overlay::show_page` gains a `source_para: Option<Vec<String>>` argument; when present it prepends those paragraphs to `all_paragraphs` so they become real navigable blocks with zero offset math. A post-`set_text` tag pass (`apply_source_style`, mirroring `apply_hi_color`) styles the source lines on `page_idx == 0`. The render call site in `journal.rs` (`nav_page`, the branch at ~503–517) builds `source_para` when `page.source_text` is non-empty.

**Tech Stack:** Rust, GTK4 (gtk4-rs), the existing linux-lit journal overlay + `cargo test`.

## Global Constraints

- Work in the isolated worktree `~/utono/linux-lit-wt/journal-source-quote` (branch `feat/journal-source-quote`), NOT the main checkout. Already created off master `9f58bca8`.
- Build/verify with `cargo build` and `cargo test` **from the worktree dir**. Do NOT `cargo run` — the user launches the app.
- Citation format is compact: `— Cymbeline, 1.1.1–3` (en-dash `–` U+2013 for the range). Collapse to `— Cymbeline, 1.1.1` when start line == end line. Omit the whole citation line when `start_citation` is absent/unparseable.
- Separator paragraph is exactly `———` (three U+2014 em-dashes), matching `journal_block.rs`.
- Source appears only on the entry's first rendered page (`page_idx == 0`).
- Render the source whenever `page.source_text` is non-empty after trim; do not gate on `scope`.
- `parse_citation(cite) -> Option<(i64, i64, i64)>` returns `(div1, div2, line)`; it lives at `src/app/mod.rs:4484`, exported `pub(crate)`, reachable as `crate::app::parse_citation`.

---

### Task 1: `format_source_citation` pure helper

**Files:**
- Modify: `src/input/actions/journal.rs` (add fn near the other citation helpers, ~after `band_for_rewrite` at line 364; add unit tests in the file's existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::app::parse_citation(&str) -> Option<(i64,i64,i64)>`
- Produces: `fn format_source_citation(title: &str, start_citation: Option<&str>, end_citation: Option<&str>) -> Option<String>`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `src/input/actions/journal.rs` (find it with `rg -n "mod tests" src/input/actions/journal.rs`):

```rust
#[test]
fn source_citation_range() {
    assert_eq!(
        format_source_citation("Cymbeline", Some("Cym.1.1.1"), Some("Cym.1.1.3")),
        Some("\u{2014} Cymbeline, 1.1.1\u{2013}3".to_string())
    );
}

#[test]
fn source_citation_single_locator_collapses() {
    // start line == end line -> no range dash
    assert_eq!(
        format_source_citation("Cymbeline", Some("Cym.1.1.5"), Some("Cym.1.1.5")),
        Some("\u{2014} Cymbeline, 1.1.5".to_string())
    );
}

#[test]
fn source_citation_missing_start_is_none() {
    assert_eq!(format_source_citation("Cymbeline", None, Some("Cym.1.1.3")), None);
    assert_eq!(format_source_citation("Cymbeline", Some("garbage"), Some("Cym.1.1.3")), None);
}

#[test]
fn source_citation_missing_end_uses_start_only() {
    // No end citation -> single locator from start.
    assert_eq!(
        format_source_citation("Cymbeline", Some("Cym.2.3.10"), None),
        Some("\u{2014} Cymbeline, 2.3.10".to_string())
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd ~/utono/linux-lit-wt/journal-source-quote && cargo test --lib source_citation 2>&1 | tail -20`
Expected: FAIL — `cannot find function format_source_citation`.

- [ ] **Step 3: Write minimal implementation**

Add after `band_for_rewrite` (line 364) in `src/input/actions/journal.rs`:

```rust
/// Compact source citation for a journal passage, e.g. `— Cymbeline, 1.1.1–3`.
/// `title` is the work title; the div/line numbers come from parsing
/// `start_citation`/`end_citation` (`ABBR.div1.div2.line`). Collapses to a
/// single locator (`— Cymbeline, 1.1.5`) when start line == end line or no end
/// citation is given. Returns `None` when the start citation is absent or does
/// not parse (never fabricate a locator).
fn format_source_citation(
    title: &str,
    start_citation: Option<&str>,
    end_citation: Option<&str>,
) -> Option<String> {
    let (d1, d2, start_line) = crate::app::parse_citation(start_citation?)?;
    let end_line = end_citation
        .and_then(crate::app::parse_citation)
        .map(|(_, _, l)| l)
        .unwrap_or(start_line);
    let locator = if end_line > start_line {
        format!("{}.{}.{}\u{2013}{}", d1, d2, start_line, end_line)
    } else {
        format!("{}.{}.{}", d1, d2, start_line)
    };
    Some(format!("\u{2014} {}, {}", title, locator))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd ~/utono/linux-lit-wt/journal-source-quote && cargo test --lib source_citation 2>&1 | tail -20`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit-wt/journal-source-quote
git add src/input/actions/journal.rs
git commit -m "feat(journal): format_source_citation helper (— Title, d.s.l–l)"
```

---

### Task 2: `source_paragraphs` pure helper

**Files:**
- Modify: `src/input/actions/journal.rs` (add fn near `format_source_citation`; add tests to the same `mod tests`)

**Interfaces:**
- Consumes: `format_source_citation` (Task 1); the existing local `first_plain_source_line` is a sibling but only returns ONE line — do NOT reuse it; parse all verse lines here.
- Produces: `fn source_paragraphs(source_text: &str, citation: Option<&str>) -> Vec<String>` — the ordered paragraph strings to prepend: `[speaker?, verse_line_1, verse_line_2, ..., citation?, "———"]`. Speaker omitted when the markup has no displayed speaker; citation paragraph omitted when `citation` is `None`. Each verse line is its own paragraph so blocks/cursor treat them as separate stops (matches the mockup's per-line quote).

**Note on parsing:** `source_text` markup looks like:
```
<speaker>FIRST GENTLEMAN</speaker>
<verse>You do not meet a man but frowns. Our bloods</verse>
<verse>No more obey the heavens than our courtiers’</verse>
<verse>Still seem as does the King’s.</verse>
```
Strip the tags to plain text. Speaker paragraph is the inner text of `<speaker>` (skip if empty or `UNKNOWN`). Each `<verse>`/`<stage>` element becomes one paragraph (a `<verse>` body may itself contain embedded `\n` — split those into separate paragraphs too).

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/input/actions/journal.rs`:

```rust
#[test]
fn source_paragraphs_speaker_verse_citation_separator() {
    let src = "<speaker>FIRST GENTLEMAN</speaker>\n\
               <verse>You do not meet a man but frowns. Our bloods</verse>\n\
               <verse>No more obey the heavens than our courtiers\u{2019}</verse>\n\
               <verse>Still seem as does the King\u{2019}s.</verse>";
    let got = source_paragraphs(src, Some("\u{2014} Cymbeline, 1.1.1\u{2013}3"));
    assert_eq!(
        got,
        vec![
            "FIRST GENTLEMAN".to_string(),
            "You do not meet a man but frowns. Our bloods".to_string(),
            "No more obey the heavens than our courtiers\u{2019}".to_string(),
            "Still seem as does the King\u{2019}s.".to_string(),
            "\u{2014} Cymbeline, 1.1.1\u{2013}3".to_string(),
            "\u{2014}\u{2014}\u{2014}".to_string(),
        ]
    );
}

#[test]
fn source_paragraphs_no_citation_omits_citation_para() {
    let src = "<speaker>KING</speaker>\n<verse>Now is the winter</verse>";
    let got = source_paragraphs(src, None);
    assert_eq!(
        got,
        vec![
            "KING".to_string(),
            "Now is the winter".to_string(),
            "\u{2014}\u{2014}\u{2014}".to_string(),
        ]
    );
}

#[test]
fn source_paragraphs_speakerless_prose_drops_speaker() {
    let src = "<speaker>UNKNOWN</speaker>\n<verse>a prose line</verse>";
    let got = source_paragraphs(src, Some("\u{2014} Bleak House, 1.1.1"));
    assert_eq!(
        got,
        vec![
            "a prose line".to_string(),
            "\u{2014} Bleak House, 1.1.1".to_string(),
            "\u{2014}\u{2014}\u{2014}".to_string(),
        ]
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd ~/utono/linux-lit-wt/journal-source-quote && cargo test --lib source_paragraphs 2>&1 | tail -20`
Expected: FAIL — `cannot find function source_paragraphs`.

- [ ] **Step 3: Write minimal implementation**

Add near `format_source_citation` in `src/input/actions/journal.rs`. Strip tags with a small local parser (do not pull in the gloss renderer; keep it pure text):

```rust
/// Inner text of the first `<TAG>…</TAG>` on `line`, or `None` if `line` is not
/// a single `<tag>text</tag>` element for one of `tags`. Whitespace-trimmed.
fn tag_inner<'a>(line: &'a str, tags: &[&str]) -> Option<&'a str> {
    let l = line.trim();
    for t in tags {
        let open = format!("<{}>", t);
        let close = format!("</{}>", t);
        if let Some(rest) = l.strip_prefix(&open) {
            if let Some(inner) = rest.strip_suffix(&close) {
                return Some(inner.trim());
            }
        }
    }
    None
}

/// Build the ordered source paragraphs to prepend above a passage Q&A:
/// `[speaker?, verse/stage line(s)…, citation?, "———"]`. The speaker paragraph
/// is dropped when empty or `UNKNOWN` (prose works). Each `<verse>`/`<stage>`
/// element — and each embedded `\n`-joined line within one — is its own
/// paragraph, so the overlay treats every quoted line as a separate navigable
/// block. The trailing `———` separates the quote from the question.
fn source_paragraphs(source_text: &str, citation: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in source_text.lines() {
        if let Some(sp) = tag_inner(raw, &["speaker"]) {
            if !sp.is_empty() && sp != "UNKNOWN" {
                out.push(sp.to_string());
            }
        } else if let Some(body) = tag_inner(raw, &["verse", "stage"]) {
            for seg in body.split('\n') {
                let seg = seg.trim();
                if !seg.is_empty() {
                    out.push(seg.to_string());
                }
            }
        }
    }
    if let Some(c) = citation {
        out.push(c.to_string());
    }
    out.push("\u{2014}\u{2014}\u{2014}".to_string());
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd ~/utono/linux-lit-wt/journal-source-quote && cargo test --lib source_paragraphs 2>&1 | tail -20`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit-wt/journal-source-quote
git add src/input/actions/journal.rs
git commit -m "feat(journal): source_paragraphs helper (speaker/verse/citation/———)"
```

---

### Task 3: `show_page` accepts `source_para` and prepends it

**Files:**
- Modify: `src/ui/journal_overlay.rs` — `show_page` signature (~555) and its non-empty-band arm (~615–620); add a `source_para_count: Cell<usize>` field to the overlay struct so `render_page` knows how many leading paragraphs are the source (for styling).
- Modify: `src/input/actions/journal.rs` — the ONE existing `show_page(...)` call site (~516) to pass `None` for now (keeps it compiling; Task 5 fills it in).

**Interfaces:**
- Consumes: nothing new (helpers land in Task 5's call).
- Produces: `show_page(&self, _footer_left, page_index, page_count, question, answer, kind, source_para: Option<Vec<String>>, card_width, card_height)`. When `source_para` is `Some(v)` and `kind != "note"`, the overlay sets `all_paragraphs = [v…, Q, A…]` and stores `source_para_count = v.len()`; otherwise `source_para_count = 0`.

- [ ] **Step 1: Add the field**

Find the overlay struct field block (near `all_paragraphs: RefCell<Vec<String>>` at ~line 40). Add:

```rust
    /// Number of leading `all_paragraphs` entries that are the prepended passage
    /// source (speaker/verse/citation/———), styled on page 0 by
    /// `apply_source_style`. 0 when the entry has no source block.
    source_para_count: std::cell::Cell<usize>,
```

And initialize it in the constructor (find where `all_paragraphs: RefCell::new(Vec::new())` is set, ~line 479):

```rust
            source_para_count: std::cell::Cell::new(0),
```

- [ ] **Step 2: Extend the signature and the Q&A arm**

Change `show_page`'s signature (line 555) to add `source_para: Option<Vec<String>>` before `card_width`:

```rust
    pub fn show_page(
        &self,
        _footer_left: &str,
        page_index: usize,
        page_count: usize,
        question: &str,
        answer: &str,
        kind: &str,
        source_para: Option<Vec<String>>,
        card_width: i32,
        card_height: i32,
    ) {
```

In the `else` (non-note) arm (currently lines 615–620), prepend the source paragraphs:

```rust
            } else {
                self.note_blocks.borrow_mut().clear();
                let full = format!("{}\n\n{}", prefix_question(question), answer);
                let mut paras = paragraph_texts(&full);
                let src = source_para.unwrap_or_default();
                self.source_para_count.set(src.len());
                if !src.is_empty() {
                    let mut combined = src;
                    combined.extend(paras);
                    paras = combined;
                }
                *self.all_paragraphs.borrow_mut() = paras;
                self.cursor_full.set(0);
            }
```

In the note arm and the empty-band arm, reset the count to 0. In the note arm (the `if is_note {` branch ~601) add near its end (after `self.cursor_full.set(first_stop);`):

```rust
                self.source_para_count.set(0);
```

In the empty-band arm (`if page_count == 0 {` ~582) add after `self.cursor_full.set(0);`:

```rust
            self.source_para_count.set(0);
```

- [ ] **Step 3: Update the existing call site to pass None (temporary)**

In `src/input/actions/journal.rs`, the current call (~line 516):

```rust
    s.journal_overlay
        .show_page(&footer_left, s.journal.page_index, count, &q, &a, &kind, cw, h);
```

becomes:

```rust
    s.journal_overlay
        .show_page(&footer_left, s.journal.page_index, count, &q, &a, &kind, None, cw, h);
```

- [ ] **Step 4: Build to verify it compiles**

Run: `cd ~/utono/linux-lit-wt/journal-source-quote && cargo build 2>&1 | tail -15`
Expected: builds clean (a warning about unused `source_para_count` read is fine — Task 4 reads it). No other `show_page` call sites exist (verify: `rg -n "\.show_page(" src/`).

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit-wt/journal-source-quote
git add src/ui/journal_overlay.rs src/input/actions/journal.rs
git commit -m "feat(journal): show_page prepends optional source paragraphs"
```

---

### Task 4: `apply_source_style` — style the source lines on page 0

**Files:**
- Modify: `src/ui/journal_overlay.rs` — add `apply_source_style(&self)`, call it from `render_page` after `apply_hi_color()` (~line 1661), and register the three tags.

**Interfaces:**
- Consumes: `self.source_para_count` (Task 3), `self.page_idx`, the buffer.
- Produces: `fn apply_source_style(&self)` — a no-op unless `page_idx == 0 && source_para_count > 0`. Applies, by buffer line: small-caps to the speaker line (line 0 IF the source has a speaker — detect by "not a verse/citation/separator"), a hang-indent left-margin to the verse lines, dim + right-justify to the citation line (the line immediately before the `———`), and centers the `———` line. The last source paragraph is always `———`; the citation, if present, is the paragraph just before it.

**Rendering detail:** paragraphs are joined by `"\n\n"` in `render_page`'s `body`, so paragraph *i* occupies buffer line `2*i` (blank line between). The speaker/verse/citation/separator paragraphs are the first `source_para_count` paragraphs, i.e. buffer lines `0, 2, 4, … 2*(count-1)`.

- [ ] **Step 1: Write a targeted test for the line-mapping helper**

The buffer/tag work needs GTK, but the paragraph→buffer-line mapping is pure. Extract it and test it. Add to `mod tests` at the bottom of `src/ui/journal_overlay.rs` (create the block if absent; check with `rg -n "mod tests" src/ui/journal_overlay.rs`):

```rust
#[test]
fn source_line_roles_maps_paragraphs_to_buffer_lines() {
    // 6 source paras: speaker, v1, v2, v3, citation, ———
    let roles = source_line_roles(6, /* has_speaker */ true, /* has_citation */ true);
    assert_eq!(roles.speaker_line, Some(0));
    assert_eq!(roles.verse_lines, vec![2, 4, 6]);        // buffer lines 2,4,6
    assert_eq!(roles.citation_line, Some(8));            // 5th para -> line 8
    assert_eq!(roles.separator_line, 10);                // 6th para -> line 10
}

#[test]
fn source_line_roles_no_speaker_no_citation() {
    // 2 source paras: v1, ———
    let roles = source_line_roles(2, false, false);
    assert_eq!(roles.speaker_line, None);
    assert_eq!(roles.verse_lines, vec![0]);
    assert_eq!(roles.citation_line, None);
    assert_eq!(roles.separator_line, 2);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd ~/utono/linux-lit-wt/journal-source-quote && cargo test --lib source_line_roles 2>&1 | tail -20`
Expected: FAIL — `cannot find function source_line_roles` / `SourceLineRoles`.

- [ ] **Step 3: Implement the pure mapping + the GTK styling that consumes it**

Add near the top of `journal_overlay.rs` (module scope, by `paragraph_texts`):

```rust
/// Which buffer lines the prepended source paragraphs occupy, by role.
/// Paragraphs render joined by "\n\n", so paragraph i is buffer line 2*i.
/// Order of source paragraphs is: [speaker?] verse+ [citation?] separator.
struct SourceLineRoles {
    speaker_line: Option<i32>,
    verse_lines: Vec<i32>,
    citation_line: Option<i32>,
    separator_line: i32,
}

fn source_line_roles(count: usize, has_speaker: bool, has_citation: bool) -> SourceLineRoles {
    let line = |para_idx: usize| (para_idx * 2) as i32;
    let last = count - 1; // separator is always last
    let separator_line = line(last);
    let citation_para = if has_citation { Some(last - 1) } else { None };
    let speaker_para = if has_speaker { Some(0) } else { None };
    let first_verse = if has_speaker { 1 } else { 0 };
    let last_verse = citation_para.map(|c| c).unwrap_or(last).saturating_sub(1);
    let verse_lines = (first_verse..=last_verse).map(line).collect();
    SourceLineRoles {
        speaker_line: speaker_para.map(line),
        verse_lines,
        citation_line: citation_para.map(line),
        separator_line,
    }
}
```

Then the GTK styling method (place beside `apply_hi_color`, ~1181). It reconstructs `has_speaker`/`has_citation` from the stored source paragraphs — store those two booleans alongside the count. Simplest: widen `source_para_count` bookkeeping to also stash the two flags. Add two more cells in Task 3's field block instead of recomputing:

```rust
    /// Whether the prepended source block has a speaker line / a citation line
    /// (set with `source_para_count`), so `apply_source_style` maps roles.
    source_has_speaker: std::cell::Cell<bool>,
    source_has_citation: std::cell::Cell<bool>,
```

(initialize both `Cell::new(false)` in the constructor; set them in Task 3's Q&A arm — see Step 4 amendment below.)

```rust
    /// Style the prepended passage source on page 0: small-caps speaker,
    /// hang-indented verse, dim right-aligned citation, centered separator.
    /// No-op off page 0 or when there is no source block. Runs after
    /// `set_text` (like `apply_hi_color`), applying tags by buffer line.
    fn apply_source_style(&self) {
        if self.page_idx.get() != 0 {
            return;
        }
        let count = self.source_para_count.get();
        if count == 0 {
            return;
        }
        let buffer = self.view.buffer();
        let table = buffer.tag_table();
        // Create the tags once (idempotent lookups).
        if table.lookup("journal-src-speaker").is_none() {
            table.add(
                &gtk4::TextTag::builder()
                    .name("journal-src-speaker")
                    .scale(0.82)
                    .weight(600)
                    .build(),
            );
        }
        if table.lookup("journal-src-verse").is_none() {
            table.add(
                &gtk4::TextTag::builder()
                    .name("journal-src-verse")
                    .left_margin(self.view.left_margin() + 28)
                    .indent(-28)
                    .build(),
            );
        }
        if table.lookup("journal-src-citation").is_none() {
            table.add(
                &gtk4::TextTag::builder()
                    .name("journal-src-citation")
                    .justification(gtk4::Justification::Right)
                    .style(gtk4::pango::Style::Italic)
                    .build(),
            );
        }
        if table.lookup("journal-src-sep").is_none() {
            table.add(
                &gtk4::TextTag::builder()
                    .name("journal-src-sep")
                    .justification(gtk4::Justification::Center)
                    .build(),
            );
        }
        let roles = source_line_roles(
            count,
            self.source_has_speaker.get(),
            self.source_has_citation.get(),
        );
        let apply_line = |name: &str, line: i32| {
            if let Some(tag) = table.lookup(name) {
                let start = buffer.iter_at_line(line).unwrap_or_else(|| buffer.start_iter());
                let mut end = start.clone();
                if !end.ends_line() {
                    end.forward_to_line_end();
                }
                buffer.apply_tag(&tag, &start, &end);
            }
        };
        if let Some(l) = roles.speaker_line {
            apply_line("journal-src-speaker", l);
        }
        for l in &roles.verse_lines {
            apply_line("journal-src-verse", *l);
        }
        if let Some(l) = roles.citation_line {
            apply_line("journal-src-citation", l);
        }
        apply_line("journal-src-sep", roles.separator_line);
    }
```

Call it in `render_page` right after `self.apply_hi_color();` (line 1661):

```rust
        self.apply_hi_color();
        self.apply_source_style();
```

- [ ] **Step 4: Amend Task 3's Q&A arm to set the flags**

In `show_page`'s Q&A arm (Task 3 Step 2), after `self.source_para_count.set(src.len());` add — computing the flags from `src` BEFORE it is moved:

```rust
                self.source_para_count.set(src.len());
                self.source_has_citation.set(
                    src.len() >= 2 && src[src.len() - 1] == "\u{2014}\u{2014}\u{2014}"
                        && src[src.len() - 2].starts_with('\u{2014}')
                        && src[src.len() - 2] != "\u{2014}\u{2014}\u{2014}",
                );
                // A speaker line is present iff the FIRST source paragraph is not
                // a verse/citation/separator — approximated as: more paras than
                // (verse=1 + separator) when no citation, i.e. first para exists
                // and isn't the separator/citation. Simplest robust check: the
                // helper is told has_speaker only when src[0] is not "———" and not
                // the citation. Speaker text never starts with an em-dash.
                self.source_has_speaker.set(
                    !src.is_empty() && !src[0].starts_with('\u{2014}'),
                );
```

Also set both flags to `false` in the note arm and empty-band arm next to their `source_para_count.set(0)`:

```rust
            self.source_has_speaker.set(false);
            self.source_has_citation.set(false);
```

- [ ] **Step 5: Run the pure tests + build**

Run: `cd ~/utono/linux-lit-wt/journal-source-quote && cargo test --lib source_line_roles 2>&1 | tail -20 && cargo build 2>&1 | tail -15`
Expected: tests PASS (2); build clean.

- [ ] **Step 6: Commit**

```bash
cd ~/utono/linux-lit-wt/journal-source-quote
git add src/ui/journal_overlay.rs
git commit -m "feat(journal): apply_source_style — small-caps/hang/citation on page 0"
```

---

### Task 5: Wire the render dispatch in `nav_page`

**Files:**
- Modify: `src/input/actions/journal.rs` — the branch at ~503–517 (the "plain Q&A only" comment block) to build `source_para` and pass it.

**Interfaces:**
- Consumes: `format_source_citation` (Task 1), `source_paragraphs` (Task 2), `show_page(..., source_para, ...)` (Task 3), `s.current_work` (`Option<Work>` with `.title`).
- Produces: the visible feature — source block above the Q&A on entry page 0.

- [ ] **Step 1: Replace the dispatch branch**

In `src/input/actions/journal.rs`, the block currently at 503–517 (the comment "Every Q&A … source block is intentionally NOT shown …" through the `show_page` call). Replace the comment + the `show_page` call with:

```rust
    // A passage Q&A (source_text present) shows its quoted source — speaker,
    // verse, citation, ——— separator — as leading navigable paragraphs above
    // the question. Built here and passed to show_page, which prepends them to
    // all_paragraphs (page 0 only; apply_source_style styles them). Notes and
    // source-less entries pass None (unchanged plain Q&A).
    let current_page = if count == 0 {
        None
    } else {
        Some(&pages[s.journal.page_index])
    };
    let (q, a, kind) = current_page
        .map(|p| (p.question.clone(), p.answer.clone(), p.kind.clone()))
        .unwrap_or_else(|| (String::new(), String::new(), "qa".to_string()));

    let source_para = current_page.and_then(|p| {
        let src = p.source_text.as_deref().unwrap_or("").trim();
        if src.is_empty() {
            return None;
        }
        let title = s
            .current_work
            .as_ref()
            .map(|w| w.title.clone())
            .unwrap_or_default();
        let citation = format_source_citation(
            &title,
            p.start_citation.as_deref(),
            p.end_citation.as_deref(),
        );
        Some(source_paragraphs(src, citation.as_deref()))
    });

    s.journal_overlay.show_page(
        &footer_left,
        s.journal.page_index,
        count,
        &q,
        &a,
        &kind,
        source_para,
        cw,
        h,
    );
```

Note: this REPLACES the temporary `None` call added in Task 3. Delete the old `let current_page`/`let (q,a,kind)`/`show_page(..., None, ...)` lines (503–517) — do not leave duplicates. Verify the surrounding `if s.journal.pending_passage … { … return; }` guard just above (487–501) is untouched.

- [ ] **Step 2: Build**

Run: `cd ~/utono/linux-lit-wt/journal-source-quote && cargo build 2>&1 | tail -15`
Expected: builds clean, no unused-warning for `format_source_citation`/`source_paragraphs`.

- [ ] **Step 3: Run the whole test suite**

Run: `cd ~/utono/linux-lit-wt/journal-source-quote && cargo test 2>&1 | tail -25`
Expected: all pass EXCEPT the known-pre-existing `theme_cycle_defaults_to_reading_themes` (failing before this session). If any OTHER test fails, stop and investigate.

- [ ] **Step 4: Commit**

```bash
cd ~/utono/linux-lit-wt/journal-source-quote
git add src/input/actions/journal.rs
git commit -m "feat(journal): render passage source above the question (entry #12 etc.)"
```

---

### Task 6: Headless visual verification

**Files:**
- No source changes (verification only). Uses the cage/grim harness from `linux-lit/CLAUDE.md`.

- [ ] **Step 1: Build (release-parity debug) in the worktree**

Run: `cd ~/utono/linux-lit-wt/journal-source-quote && cargo build 2>&1 | tail -3`
Expected: clean.

- [ ] **Step 2: Drive the journal overlay headlessly to entry #12 (Cym)**

Use the `test-headless-navigation` skill / the cage flow (mandatory env: `GSK_RENDERER=cairo LIT_NO_MPV=1 LIT_DEV=1 WLR_BACKENDS=headless WLR_RENDERER=pixman`). Launch the worktree's `./target/debug/linux-lit`, open Cymbeline (Arkangel), enter reader-gloss on the "You do not meet a man but frowns" opening, open the journal overlay to the passage entry (#12), and `grim` a screenshot. Resize to production geometry (`wlr-randr --output HEADLESS-1 --custom-mode 1920x1200`) before capturing. Cleanup ONLY with `pkill -f "cage -- ./target/debug/linux-lit"`.

- [ ] **Step 3: Open the PNG and confirm on screen (UI review protocol)**

Read the screenshot and verify, quoting the on-screen text:
- `FIRST GENTLEMAN` renders small-caps above the verse.
- The three verse lines appear, hang-indented, wrapping under themselves.
- `— Cymbeline, 1.1.1–3` is right-aligned and dim below the last verse line.
- `———` separator sits between the source and the question.
- The question (`Q: When the character delivers…`) and answer follow below.
- No top/bottom clipping of the card (per clip-prevention).

- [ ] **Step 4: Confirm continued-page has no repeated source**

`Ctrl+n` to a continued answer page (if the entry paginates), screenshot, confirm the source block is NOT repeated.

- [ ] **Step 5: Regression — a note (no source_text) is unchanged**

Navigate to a scene/author NOTE entry (no source_text), screenshot, confirm it renders exactly as before (no stray source block, no `———`).

- [ ] **Step 6: Record the result**

No commit (verification only). Note the observed result in the finish-up summary. If anything is off (clipping, wrong indent, missing small-caps), loop back to the relevant task.

---

## Finish-up (after all tasks pass)

Per project convention (merge back to master locally, then push):

```bash
# From the MAIN checkout (git refuses master in two worktrees):
cd ~/utono/linux-lit
git checkout master
git merge --no-ff feat/journal-source-quote
cargo build 2>&1 | tail -3          # re-verify on master
cargo test 2>&1 | tail -20          # only the known theme_cycle failure allowed
git push origin master
git worktree remove ~/utono/linux-lit-wt/journal-source-quote
git branch -d feat/journal-source-quote
```

Before merging, prompt the user to choose headless-self-check (Task 6, already done) vs. a final manual eyeball on the real GL renderer, per the project's "Testing Before Completion" rule.

## Self-Review

- **Spec coverage:** citation format (Task 1), source assembly incl. speakerless + no-citation (Task 2), styled render (Task 4), stoppable/navigable via real paragraphs (Task 3), first-page-only gating (Task 4 `page_idx==0`), dispatch on non-empty `source_text` (Task 5), edge cases (missing citation → omit line, start==end collapse — Task 1 tests; speakerless — Task 2 test), testing (unit Tasks 1–4, headless Task 6, regression Task 6 Step 5). All spec sections map to a task.
- **Placeholder scan:** no TBD/TODO; every code step shows complete code.
- **Type consistency:** `format_source_citation(&str, Option<&str>, Option<&str>) -> Option<String>` and `source_paragraphs(&str, Option<&str>) -> Vec<String>` used consistently in Task 5; `show_page`'s new `source_para: Option<Vec<String>>` param matches Task 3's definition and Task 5's call; `source_para_count`/`source_has_speaker`/`source_has_citation` cells defined in Task 3/4 and read in Task 4; `source_line_roles(usize, bool, bool) -> SourceLineRoles` consistent between test and impl.
