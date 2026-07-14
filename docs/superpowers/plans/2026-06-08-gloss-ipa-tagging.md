# Gloss-driven OP IPA tagging — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the gloss pipeline emit sparse OP `/IPA/` on `<verse>` lines (full set for TTS) plus a `<pron>` teaching note (a subset shown to the reader), strip `/IPA/` from the on-screen verse, and feed the raw IPA-bearing verse to TTS.

**Architecture:** A `GlossBlock` gains a `display` field (IPA-stripped) beside its raw `text` (IPA-bearing, for TTS). A new `strip_ipa` helper removes `/…/` spans for display. A new `<pron>` tag is parsed and rendered as a styled, non-TTS note. The accent-bar matcher compares on the stripped form. Prompts are extended last (manual verification, live Claude API). Voice selection by speaker gender is out of scope here (separate spec) — TTS keeps using the single configured voice with the existing 402→Alice fallback.

**Tech Stack:** Rust, GTK4 (`gtk4::TextView` + `TextTag`), `cargo test --bins` (binary-only crate; `src/ui/gloss_overlay.rs` `#[cfg(test)]` modules run under it).

**Spec:** `docs/superpowers/specs/2026-06-08-gloss-ipa-tagging-design.md`

---

## Phase 1 — Pure logic (unit-tested, no GTK/API)

### Task 1: `strip_ipa` helper

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (add fn near `strip_brackets` at line ~1912; add tests to `mod block_tests` ~line 2006)

- [ ] **Step 1: Write failing tests** — add these four tests to `mod block_tests` in `src/ui/gloss_overlay.rs`:

```rust
    #[test]
    fn strip_ipa_removes_tagged_words() {
        assert_eq!(strip_ipa("To /biː/ or not to /biː/"), "To  or not to ");
    }

    #[test]
    fn strip_ipa_keeps_literal_slash() {
        // a bare slash between ordinary words is NOT an IPA span
        assert_eq!(strip_ipa("read and/or write"), "read and/or write");
    }

    #[test]
    fn strip_ipa_no_tags_is_identity() {
        assert_eq!(strip_ipa("plain modern line"), "plain modern line");
    }

    #[test]
    fn strip_ipa_handles_stress_marks() {
        assert_eq!(strip_ipa("the /ˈsʊfər/ of it"), "the  of it");
    }
```

- [ ] **Step 2: Run tests, verify they FAIL**

Run: `cargo test --bins strip_ipa`
Expected: FAIL — `cannot find function strip_ipa in this scope`.

- [ ] **Step 3: Implement `strip_ipa`** — add immediately after `strip_brackets` (after line 1924) in `src/ui/gloss_overlay.rs`:

```rust
/// Remove inline `/IPA/` pronunciation spans for DISPLAY. Mirrors
/// `strip_brackets`. An IPA span is `/…/` whose contents contain at least one
/// non-ASCII-letter / IPA-class character (length marks, stress marks, schwa,
/// etc.), so a bare literal slash between plain words ("and/or") is NOT treated
/// as a span and survives. The raw, IPA-bearing text is what TTS gets; this is
/// the reader-facing form. See the gloss-IPA spec, §4.
fn strip_ipa(text: &str) -> String {
    // Walk char-by-char. On '/', look ahead to the next '/'. If the enclosed
    // run contains any character outside [A-Za-z] (i.e. real IPA), drop the
    // whole span; otherwise keep it verbatim (it was an ordinary slash usage).
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' {
            if let Some(close_rel) = chars[i + 1..].iter().position(|&c| c == '/') {
                let close = i + 1 + close_rel;
                let inner = &chars[i + 1..close];
                let is_ipa = !inner.is_empty()
                    && inner.iter().any(|&c| !c.is_ascii_alphabetic());
                if is_ipa {
                    i = close + 1; // skip the whole /…/ span
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}
```

- [ ] **Step 4: Run tests, verify PASS**

Run: `cargo test --bins strip_ipa`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): strip_ipa helper for hiding /IPA/ from display"
```

---

### Task 2: `<pron>` tag in the parser

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — `GlossElement` enum (line 1478), `parse_gloss_tags` (line 1609); tests in `mod block_tests`.

- [ ] **Step 1: Write failing test** — add to `mod block_tests`:

```rust
    #[test]
    fn parse_extracts_pron_element() {
        let g = "<verse>To /biː/</verse>\n<pron>BEE: be /biː/ keeps the long vowel.</pron>";
        let els = parse_gloss_tags(g);
        // Verse, then Pron.
        assert!(matches!(els[0], GlossElement::Verse(_)));
        assert!(
            matches!(&els[1], GlossElement::Pron(t) if t.contains("long vowel")),
            "expected a Pron element carrying the note, got {:?}", els.get(1)
        );
    }
```

(`GlossElement` derives nothing today — add `#[derive(Debug)]` to it in Step 3 so `{:?}` works.)

- [ ] **Step 2: Run test, verify FAIL**

Run: `cargo test --bins parse_extracts_pron`
Expected: FAIL — `no variant named Pron` / `GlossElement doesn't implement Debug`.

- [ ] **Step 3: Add the `Pron` variant + parse arm + Debug.** In `src/ui/gloss_overlay.rs` replace the enum at line 1478:

```rust
#[derive(Debug)]
enum GlossElement {
    Speaker(String),
    Verse(String),
    Gloss(String),
    Pron(String),
}
```

And in `parse_gloss_tags`, add a `pron` arm to the `try_extract` chain (after the `gloss` arm, ~line 1624):

```rust
            } else if let Some(el) = try_extract(after_open, "gloss") {
                elements.push(GlossElement::Gloss(el.0.to_string()));
                remaining = el.1;
            } else if let Some(el) = try_extract(after_open, "pron") {
                elements.push(GlossElement::Pron(el.0.to_string()));
                remaining = el.1;
            } else {
```

- [ ] **Step 4: Run test, verify PASS**

Run: `cargo test --bins parse_extracts_pron`
Expected: PASS.

- [ ] **Step 5: Fix non-exhaustive matches.** Adding a variant breaks any exhaustive `match` over `GlossElement`. Build to find them:

Run: `cargo build 2>&1 | rg "non-exhaustive|GlossElement::Pron"`
Expected: errors in `gloss_blocks` (line ~1581) and `populate_gloss_buffer_ex` (line ~1799). In `gloss_blocks`, add an arm that ignores Pron (it is not a cursor/TTS block):

```rust
            GlossElement::Speaker(_) => { /* drop speaker labels from source text */ }
            GlossElement::Pron(_) => { /* pronunciation note: not a cursor stop, not TTS */ }
            GlossElement::Verse(text) => pending_verses.push(text.trim().to_string()),
```

For `populate_gloss_buffer_ex`, the render arm is added in Task 5 — for now add a temporary no-op arm so it compiles:

```rust
            GlossElement::Pron(_) => { /* rendered in Task 5 */ }
```

- [ ] **Step 6: Run full bins tests + build**

Run: `cargo build && cargo test --bins -- --test-threads=1`
Expected: builds clean; all tests PASS (252 + the new ones).

- [ ] **Step 7: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): parse <pron> pronunciation-note tag"
```

---

### Task 3: `GlossBlock` carries raw + display text

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — `GlossBlock` (line 1551), `gloss_blocks` (line 1565); tests.

- [ ] **Step 1: Write failing test** — add to `mod block_tests`:

```rust
    #[test]
    fn source_block_keeps_raw_ipa_and_derives_clean_display() {
        let g = "<speaker>HAMLET</speaker>\n<verse>To /biː/ or not to /biː/</verse>";
        let blocks = gloss_blocks(g);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Source);
        // raw text (for TTS) keeps the IPA
        assert_eq!(blocks[0].text, "To /biː/ or not to /biː/");
        // display text (for the reader / accent-bar matcher) is stripped
        assert_eq!(blocks[0].display, "To  or not to ");
    }
```

- [ ] **Step 2: Run test, verify FAIL**

Run: `cargo test --bins source_block_keeps_raw_ipa`
Expected: FAIL — `no field display on GlossBlock`.

- [ ] **Step 3: Add the `display` field + populate it.** In `src/ui/gloss_overlay.rs` replace the struct (line 1551):

```rust
/// One cursor stop in the gloss, in document order.
pub struct GlossBlock {
    pub kind: BlockKind,
    /// 0-based index WITHIN its kind (source blocks numbered separately from
    /// explication paragraphs).
    pub index: i32,
    /// RAW text, including any inline `/IPA/` — this is what TTS synthesizes.
    /// For Source: the joined verse-line text (speaker labels excluded).
    /// For Explication: the paragraph prose.
    pub text: String,
    /// DISPLAY text: `text` with `/IPA/` stripped (`strip_ipa`). Used for the
    /// reader's buffer and the accent-bar block matcher.
    pub display: String,
}
```

In `gloss_blocks`, set `display` on both pushes. The `flush_source` closure (line 1573):

```rust
    let flush_source =
        |blocks: &mut Vec<GlossBlock>, source_idx: &mut i32, pending: &mut Vec<String>| {
            if !pending.is_empty() {
                let text = pending.join("\n");
                let display = strip_ipa(&text);
                blocks.push(GlossBlock {
                    kind: BlockKind::Source,
                    index: *source_idx,
                    text,
                    display,
                });
                *source_idx += 1;
                pending.clear();
            }
        };
```

The Explication push (line ~1591):

```rust
                flush_source(&mut blocks, &mut source_idx, &mut pending_verses);
                let text = text.trim().to_string();
                let display = strip_ipa(&text);
                blocks.push(GlossBlock {
                    kind: BlockKind::Explication,
                    index: expl_idx,
                    text,
                    display,
                });
                expl_idx += 1;
```

- [ ] **Step 4: Run test, verify PASS**

Run: `cargo test --bins source_block_keeps_raw_ipa`
Expected: PASS.

- [ ] **Step 5: Run full bins tests**

Run: `cargo test --bins -- --test-threads=1`
Expected: all PASS (the existing `block_tests` still pass — `text` is unchanged, `display` is additive).

- [ ] **Step 6: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): GlossBlock carries raw (TTS) + display (stripped) text"
```

---

## Phase 2 — Rendering (GTK; build-verified, visual confirmation by user)

### Task 4: Accent-bar matcher uses `display`, verse renders stripped

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — `rebuild_block_ranges` (line 1054), `populate_gloss_buffer_ex` Verse insert (line 1817).

- [ ] **Step 1: Matcher — match on stripped block text.** In `rebuild_block_ranges` (line 1054), the loop splits `b.text.lines()`. Change it to `b.display.lines()` so it matches the displayed (stripped) buffer:

```rust
        for b in blocks {
            let lines: Vec<&str> = b.display.lines().collect();
```

(Everything below — `first_needle`, `last_needle`, `find_line` — is unchanged; they now operate on stripped text, which is what's in the buffer.)

- [ ] **Step 2: Verse insert — strip IPA before inserting.** In `populate_gloss_buffer_ex`, the Verse arm (line 1814), strip the text for display while keeping the line-number lookup working. Replace the arm body:

```rust
            GlossElement::Verse(text) => {
                only_speakers_so_far = false;
                let shown = strip_ipa(text);
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, &shown);
                let start = buffer.iter_at_offset(offset);
                buffer.apply_tag(&verse_tag, &start, &buffer.end_iter());
                apply_bracket_styling(&buffer, offset, &bracket_tag);

                // line-number gutter: match on bracket+IPA-stripped, trimmed text
                let stripped = strip_brackets(&shown);
                if let Some(&num) = line_lookup.get(stripped.trim()) {
                    line_nums.push(LineNumber { buffer_line: line, number: num });
                }
            }
```

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | rg "^error" || echo OK`
Expected: `OK`.

- [ ] **Step 4: Run bins tests**

Run: `cargo test --bins -- --test-threads=1`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): render verse with /IPA/ stripped; accent bar matches stripped text"
```

- [ ] **Step 6: User visual check (cannot self-verify headlessly).** Ask the user to `cargo run`, open a gloss whose verse contains `/IPA/`, and confirm: (a) the verse shows NO `/slashes/`, (b) the accent bar lands on the first source block on open and tracks `j`/`k`. Per CLAUDE.md this is user-run.

---

### Task 5: Render `<pron>` as a styled teaching note

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — add a `pron_tag` TextTag near `bracket_tag` (line ~1730), and the `GlossElement::Pron` render arm (replacing the Task 2 no-op at line ~1799).

- [ ] **Step 1: Create a `pron_tag`.** Near where `bracket_tag` is built (line ~1730 in `populate_gloss_buffer_ex`), add a dim italic tag (mirror the bracket/citation tag construction already there):

```rust
    let pron_tag = buffer.tag_table().lookup("gloss-pron").unwrap_or_else(|| {
        let t = gtk4::TextTag::builder()
            .name("gloss-pron")
            .style(gtk4::pango::Style::Italic)
            .scale(0.92)
            .build();
        if let Some(dim) = dim_color {
            t.set_property("foreground", dim);
        }
        buffer.tag_table().add(&t);
        t
    });
```

- [ ] **Step 2: Render the Pron arm.** Replace the temporary no-op `GlossElement::Pron(_) => {}` (added in Task 2) with:

```rust
            GlossElement::Pron(text) => {
                only_speakers_so_far = false;
                // The <pron> note's IPA is MEANT to be visible (teaching tier),
                // so do NOT strip it. Render dim+italic beneath its verse block.
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, text);
                let start = buffer.iter_at_offset(offset);
                buffer.apply_tag(&pron_tag, &start, &buffer.end_iter());
            }
```

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | rg "^error" || echo OK`
Expected: `OK`.

- [ ] **Step 4: Run bins tests**

Run: `cargo test --bins -- --test-threads=1`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): render <pron> note dim+italic, IPA shown (teaching tier)"
```

- [ ] **Step 6: User visual check.** Ask the user to `cargo run`, open a gloss containing a `<pron>` note, and confirm it shows beneath its verse in a dimmer italic style WITH its IPA visible (this tier is not stripped), and that `<pron>` is NOT a cursor stop (j/k skips it) and not read by Space/TTS.

---

## Phase 3 — Prompt + TTS wiring (manual / live-API verification)

### Task 6: TTS sends raw IPA (confirm + lock with a test)

**Files:**
- Test only: `src/ui/gloss_overlay.rs` `mod block_tests` (the contract that `text` keeps IPA is already enforced by Task 3's test — this task adds a guard that the TTS path reads `.text`, not `.display`).

`play_block_tts` (`src/input/actions/gloss.rs:640`) already does `b.text.clone()` → synth, so raw IPA already flows to TTS. No code change; add a regression guard so a future refactor can't switch it to `display`.

- [ ] **Step 1: Add guard test** to `mod block_tests`:

```rust
    #[test]
    fn tts_field_is_raw_display_field_is_stripped() {
        // play_block_tts (gloss.rs) clones `.text` for synthesis; the reader
        // path uses `.display`. This locks that the two diverge as intended.
        let g = "<verse>/biː/ or not</verse>";
        let b = &gloss_blocks(g)[0];
        assert!(b.text.contains('/'), "TTS text must keep raw /IPA/");
        assert!(!b.display.contains('/'), "display text must be stripped");
    }
```

- [ ] **Step 2: Run, verify PASS** (it documents existing behavior)

Run: `cargo test --bins tts_field_is_raw`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "test(gloss): lock TTS=raw-IPA / display=stripped invariant"
```

---

### Task 7: Extend `TEACHER_GENERIC_PROMPT` for `<verse>` IPA + `<pron>`

**Files:**
- Modify: `src/gloss.rs` — `TEACHER_GENERIC_PROMPT` (line 194-218).

No automated test (prompt output is a live-Claude, judgment call). Verify by generating a real gloss and eyeballing.

- [ ] **Step 1: Add IPA + `<pron>` instructions.** In `src/gloss.rs`, inside `TEACHER_GENERIC_PROMPT`, add to the analysis bullet list and the output-format/rules sections. Insert after the existing "verse structure, and breath patterns" bullet:

```
- On each <verse> line, tag for Original Pronunciation ONLY the few words you have already identified as operative / accent-bearing / metrically stressed — per word, never per phrase. Tagging every word destabilizes synthesis and muddies the teaching. Write the pronunciation as IPA wrapped in forward slashes inline, e.g. /tɛːk/ for "take", /ˈsʊfər/ for "suffer". Encode OP vowels (rhotic finals, FACE/GOAT monophthongs, the MEAT–MEET split) and ˈ/ˌ stress where metre or rhetoric demands — but let line structure, not IPA, govern syllable count (leave -ed/-ion to the metre).
```

And add a new output-format tag line (after the `<gloss>` format line):

```
- <pron>note</pron> AFTER a verse block: name only the 1-3 most pedagogically striking pronunciations from that block and say which OP feature each illustrates (a vowel shift, a rhotic, a stress that changes the scansion). This is a STRICT SUBSET of the words you tagged in <verse>.
```

And add to the Rules list:

```
- Tag sparsely: only operative/accent-bearing words get /IPA/. A 40-word line should have far fewer than 40 tags.
- The <pron> note shows its IPA to the reader; the <verse> /IPA/ is hidden from the reader and used only for audio. Do not explain this in the output — just produce both.
```

- [ ] **Step 2: Build (compile-check the string literal)**

Run: `cargo build 2>&1 | rg "^error" || echo OK`
Expected: `OK`.

- [ ] **Step 3: Mirror into the add/edit variants.** Apply the same `<verse>` IPA tagging rule (NOT the `<pron>` note — keep that initial-gloss only, to limit churn) to `USER_QUESTION_PROMPT` (line 4) and `EDIT_GLOSS_PROMPT` (line 172) so regenerated glosses keep IPA. Add the single "tag operative words with inline /IPA/" sentence to each prompt's rules.

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | rg "^error" || echo OK`
Expected: `OK`.

- [ ] **Step 5: Commit**

```bash
git add src/gloss.rs
git commit -m "feat(gloss): teacher-generic prompt emits sparse <verse> IPA + <pron> note"
```

- [ ] **Step 6: User live-API check.** Ask the user to `cargo run`, gloss a verse passage (Ctrl+g), and confirm: the stored gloss has `/IPA/` on a sparse set of operative words and a `<pron>` note; the reader shows clean verse + the `<pron>` note; Space/TTS on the source block reads with OP. Iterate on prompt wording from real output.

---

### Task 8: Extend `INNER_MONOLOGUE_PROMPT` for `<verse>` IPA (no `<pron>`)

**Files:**
- Modify: `src/gloss.rs` — `INNER_MONOLOGUE_PROMPT` (line 25-106), and `INNER_MONOLOGUE_ADD/EDIT_PROMPT` (lines 108, 140).

- [ ] **Step 1: Add the `<verse>` IPA rule only.** In `INNER_MONOLOGUE_PROMPT`, add to the Rules section (the inner-monologue `<gloss>` stays strictly the bracketed echo — NO `<pron>`):

```
- On each <verse> line, tag for Original Pronunciation ONLY the few operative / accent-bearing / metrically stressed words, per word never per phrase, as inline IPA in forward slashes (e.g. /tɛːk/). Encode OP vowels and ˈ/ˌ stress; let line structure govern syllable count. Do NOT add a <pron> note — the <gloss> remains exactly the single bracketed echo.
```

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | rg "^error" || echo OK`
Expected: `OK`.

- [ ] **Step 3: Mirror the verse-IPA sentence into `INNER_MONOLOGUE_ADD_PROMPT` and `INNER_MONOLOGUE_EDIT_PROMPT`** so re-glossed inner-monologue verse keeps IPA.

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | rg "^error" || echo OK`
Expected: `OK`.

- [ ] **Step 5: Commit**

```bash
git add src/gloss.rs
git commit -m "feat(gloss): inner-monologue prompt emits <verse> IPA (no <pron>)"
```

- [ ] **Step 6: User live-API check.** Gloss a passage in inner-monologue mode; confirm `<verse>` has sparse `/IPA/`, `<gloss>` is still only the bracketed echo (no prose, no `<pron>`), reader verse is clean.

---

### Task 9: Cache-invalidation contract on IPA edit

**Files:**
- Verify: `src/input/actions/gloss.rs` (edit path), `src/db/queries.rs` (`delete_gloss_audio`).

The spec requires that editing a gloss's verse IPA invalidates its cached audio. The existing edit path already calls `delete_gloss_audio` on gloss removal/edit. Confirm an edit of gloss text triggers it; if the edit path does NOT already clear audio, add the call.

- [ ] **Step 1: Inspect the edit path.**

Run: `rg -n "delete_gloss_audio|edit_gloss" src/input/actions/gloss.rs src/db/queries.rs`
Expected: shows where `delete_gloss_audio` is called. Read `edit_gloss` and the gloss-delete handler.

- [ ] **Step 2: If edit already clears cached audio:** no change — note it in the commit. **If not:** in the `edit_gloss` success path (where new gloss text replaces old), call `crate::db::queries::delete_gloss_audio(&conn, gloss_id)` before/after saving the new text, so stale OP audio (synthesized from the old IPA) is dropped.

- [ ] **Step 3: Build + bins tests**

Run: `cargo build && cargo test --bins -- --test-threads=1`
Expected: clean, all PASS.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "fix(gloss): invalidate cached audio when a gloss (its IPA) is edited"
```

---

## Self-review notes

- **Spec coverage:** §1 markup → Tasks 2,3,7,8. §2 prompts → 7,8. §3 storage (inline blob, three views) → 3 (views derived) + existing `gloss_text` (no schema change, per spec). §4 render (strip_ipa, GlossBlock both forms, matcher on stripped, `<pron>` styled) → 1,3,4,5. §5 TTS raw text → 6; voice-selection by gender is explicitly out of scope (separate spec) and noted in the header; best-of-N + STT are future (not in this plan); cache invalidation → 9. Input cap: a single block is far under 5k, no task needed (noted in spec).
- **Out of scope (correct):** A-OP/A-OP-F gender voice switch (separate `2026-06-08-character-gender-design.md` plan), best-of-N take selection, phonetic-STT auto-score. TTS here uses the single configured voice + existing 402→Alice fallback.
- **Type consistency:** `GlossBlock { kind, index, text, display }` used identically in Tasks 3,4,6. `GlossElement::Pron(String)` in Tasks 2,5. `strip_ipa(&str)->String` in Tasks 1,3,4.
- **Risk:** the `strip_ipa` IPA-vs-literal-slash heuristic (non-ASCII-letter inside `/…/`) could misclassify an all-ASCII IPA span (rare — most OP IPA has length/stress/schwa marks). Acceptable; the prompt always emits real IPA. If a pure-ASCII span ever needs stripping, tighten the heuristic later.
