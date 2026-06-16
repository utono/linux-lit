# BCP Decorative Typography Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render Book of Common Prayer works (`BCP*` abbrevs) in the reading view with liturgical typography — centered headings, italic centered/hanging rubrics, and small-caps divine names — reusing the existing Pango TextTag pipeline.

**Architecture:** Add BCP-specific line-type predicates to `src/db/line_types.rs`, then a parallel `apply_bcp_formatting()` pass in `src/app.rs` that `apply_dialogue_formatting()` delegates to when the current work is a BCP work. Styling uses GTK4 `TextTag`s (small-caps, italic, centered, indented) exactly as the existing speaker/stage-direction/stanza code does. No new rendering machinery; the Shakespeare path is untouched.

**Tech Stack:** Rust, GTK4 (`gtk4::TextTag`, `pango::Variant`/`Style`, `gtk4::Justification`), `regex` + `OnceLock`, `cargo test`. Spec: `docs/superpowers/specs/2026-06-16-bcp-decorative-typography-design.md`.

---

## Conventions (read first)

- **Build:** `cd ~/utono/linux-lit && cargo build`. Do NOT run the app (`cargo run` is the user's). Tests: `cargo test`. Lint: `cargo clippy`.
- **Run one test:** `cargo test <test_name>`. Run a module: `cargo test line_types::`.
- Predicates live in `src/db/line_types.rs`, use `OnceLock<Regex>` for any regex, take `&str`, and `text.trim()` first. Each gets a thorough `#[cfg(test)]` truth table (see the existing tests there).
- Tag pattern (from `apply_stanza_number_centering`, app.rs:3506): `tag_table.lookup("name").unwrap_or_else(|| { let t = TextTag::builder()…build(); tag_table.add(&t); t })`, then `state.buffer.apply_tag(&t, &start, &end)`.
- The current work is `state.current_work: Option<Work>` with fields `.abbrev: String` and `.work_type: String`.

## File Structure

- Modify `src/db/line_types.rs` — add `is_bcp_work`, `is_rubric`, `is_bcp_heading`, `rubric_is_centered`, `divine_name_spans` + tests.
- Modify `src/app.rs` — add `apply_bcp_formatting(state)`; delegate to it from `apply_dialogue_formatting` when the work is BCP.

---

## Task 1: BCP detection + heading/rubric predicates

**Files:**
- Modify: `src/db/line_types.rs`
- Test: same file (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write failing tests** — append inside `mod tests`:

```rust
#[test]
fn test_is_bcp_work() {
    assert!(is_bcp_work("BCP1559"));
    assert!(is_bcp_work("BCP1559M"));
    assert!(is_bcp_work("BCP1662"));
    assert!(!is_bcp_work("Ham"));
    assert!(!is_bcp_work("bcp1559")); // case-sensitive, matches echo-channel convention
}

#[test]
fn test_is_bcp_heading() {
    assert!(is_bcp_heading("## THE SUPPER"));
    assert!(is_bcp_heading("## An Order for Morning"));
    assert!(!is_bcp_heading("THE SUPPER")); // no marker
    assert!(!is_bcp_heading("[a rubric]"));
}

#[test]
fn test_is_rubric() {
    assert!(is_rubric("[The Priest shall say.]"));
    assert!(is_rubric("[¶ The Morning prayer shall be used.]"));
    assert!(!is_rubric("## A heading"));
    assert!(!is_rubric("Our Father, which art in heaven."));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test line_types:: 2>&1 | tail -20`
Expected: compile error / FAIL — `is_bcp_work`, `is_bcp_heading`, `is_rubric` not found.

- [ ] **Step 3: Implement the three predicates** — add near the other `pub fn` predicates (after `is_stage_direction`, before `is_act_scene_marker`):

```rust
/// A Book of Common Prayer work, identified by its abbrev prefix. Mirrors the
/// `LIKE 'BCP%'` echo-channel rule (src/db/echo_channel.rs) and the inline
/// `abbrev.starts_with("BCP")` test in src/input/actions/echoes.rs.
pub fn is_bcp_work(abbrev: &str) -> bool {
    abbrev.starts_with("BCP")
}

/// A BCP heading line, carrying the `## ` marker from extract_blocks. Kept
/// distinct from `is_act_scene_marker` so BCP headings get centered liturgical
/// styling rather than the play act/scene treatment.
pub fn is_bcp_heading(text: &str) -> bool {
    text.trim().starts_with("## ")
}

/// A BCP rubric (stage direction / instruction), wrapped in `[ ]` by
/// extract_blocks. Distinct from `is_stage_direction` (which also matches
/// multi-line bracket fragments) because BCP rubrics are whole-line `[...]`.
pub fn is_rubric(text: &str) -> bool {
    let t = text.trim();
    t.starts_with('[') && t.ends_with(']') && t.len() >= 2
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test line_types:: 2>&1 | tail -20`
Expected: all `line_types` tests pass.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit
git add src/db/line_types.rs
git commit -m "feat(bcp): add is_bcp_work / is_bcp_heading / is_rubric predicates"
```

---

## Task 2: Rubric layout heuristic (centered vs hanging)

**Files:**
- Modify: `src/db/line_types.rs`
- Test: same file

- [ ] **Step 1: Write failing tests** — append inside `mod tests`:

```rust
#[test]
fn test_rubric_is_centered() {
    // Short transition/speaker cues -> centered.
    assert!(rubric_is_centered("The Priest."));
    assert!(rubric_is_centered("The Answer."));
    assert!(rubric_is_centered("Then likewise he shall say."));
    // A leading pilcrow does not change the decision.
    assert!(rubric_is_centered("¶ Then the Collect of the day."));
    // Long instructional prose -> hanging (not centered).
    assert!(!rubric_is_centered(
        "At the beginning both of Morning Prayer, and likewise of Evening \
         Prayer, the Minister shall read with a loud voice, some one of these \
         sentences of the Scriptures that follow."
    ));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test line_types::tests::test_rubric_is_centered 2>&1 | tail -20`
Expected: FAIL — `rubric_is_centered` not found.

- [ ] **Step 3: Implement** — add to `src/db/line_types.rs` near the BCP predicates:

```rust
/// Max words for a rubric to be treated as a short centered cue rather than a
/// hanging-indent instructional paragraph. Tunable; 8 fits the Oxford text.
const RUBRIC_CENTER_MAX_WORDS: usize = 8;

/// Decide a rubric's layout. Pass the rubric's INNER text (no surrounding
/// brackets). Short cues with no sentence-internal period ("The Priest.",
/// "Then likewise he shall say.") center; longer instructional prose hangs.
/// Display heuristic only — a wrong call misplaces alignment, never text.
pub fn rubric_is_centered(inner: &str) -> bool {
    let t = inner.trim().trim_start_matches('¶').trim();
    let words = t.split_whitespace().count();
    if words == 0 || words > RUBRIC_CENTER_MAX_WORDS {
        return false;
    }
    // A period anywhere but the very end signals multi-sentence instruction.
    let trimmed_end = t.trim_end_matches('.');
    !trimmed_end.contains('.')
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test line_types::tests::test_rubric_is_centered 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/db/line_types.rs
git commit -m "feat(bcp): rubric_is_centered layout heuristic"
```

---

## Task 3: Divine-name span finder (word-level small-caps)

**Files:**
- Modify: `src/db/line_types.rs`
- Test: same file

- [ ] **Step 1: Write failing tests** — append inside `mod tests`:

```rust
#[test]
fn test_divine_name_spans() {
    // Whole-word GOD / LORD -> byte ranges of each.
    let line = "O Lord GOD, Lamb of GOD";
    let spans = divine_name_spans(line);
    // "GOD" at byte 7..10 and 20..23; "Lord" is title-case, not all-caps -> skip.
    assert_eq!(spans, vec![(7, 10), (20, 23)]);
}

#[test]
fn test_divine_name_spans_ignores_partials_and_lowercase() {
    assert_eq!(divine_name_spans("god is good"), vec![]); // lowercase
    assert_eq!(divine_name_spans("GODLY living"), vec![]); // not whole word
    assert_eq!(divine_name_spans("the LORDES table"), vec![]); // LORDES != LORD
    // Whole-word all-caps LORD is found.
    assert_eq!(divine_name_spans("the LORD reigneth"), vec![(4, 8)]);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test line_types::tests::test_divine_name 2>&1 | tail -20`
Expected: FAIL — `divine_name_spans` not found.

- [ ] **Step 3: Implement** — add to `src/db/line_types.rs`:

```rust
fn divine_name_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Whole-word all-caps GOD or LORD. \b word boundaries reject GODLY/LORDES.
    RE.get_or_init(|| Regex::new(r"\b(GOD|LORD)\b").unwrap())
}

/// Byte ranges (start, end) of whole-word all-caps divine names (GOD, LORD) in
/// `line`, for word-level small-caps tagging. Title-case ("Lord") and partials
/// ("GODLY", "LORDES") are not matched — only the source's emphatic all-caps.
pub fn divine_name_spans(line: &str) -> Vec<(usize, usize)> {
    divine_name_re()
        .find_iter(line)
        .map(|m| (m.start(), m.end()))
        .collect()
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test line_types::tests::test_divine_name 2>&1 | tail -20`
Expected: both pass.

- [ ] **Step 5: Commit**

```bash
git add src/db/line_types.rs
git commit -m "feat(bcp): divine_name_spans finds whole-word GOD/LORD ranges"
```

---

## Task 4: BCP formatting pass + delegation

**Files:**
- Modify: `src/app.rs`

This task wires the predicates into a rendering pass. There is no Rust unit test for GTK rendering (the buffer/TextView needs a GTK main context); verification is `cargo build` + `cargo clippy` + the headless self-check. The logic-bearing parts (predicates, spans) were unit-tested in Tasks 1–3.

- [ ] **Step 1: Add the delegation gate** — at the very top of `apply_dialogue_formatting` (src/app.rs:3529), immediately after `use crate::db::line_types;` (line 3530), insert:

```rust
    // BCP works get liturgical typography instead of play dialogue formatting.
    if state
        .current_work
        .as_ref()
        .is_some_and(|w| line_types::is_bcp_work(&w.abbrev))
    {
        apply_bcp_formatting(state);
        return;
    }
```

- [ ] **Step 2: Build to confirm the gate compiles (apply_bcp_formatting missing -> error expected)**

Run: `cargo build 2>&1 | tail -15`
Expected: FAIL — `cannot find function apply_bcp_formatting`.

- [ ] **Step 3: Implement `apply_bcp_formatting`** — add as a new `pub fn` immediately AFTER `apply_dialogue_formatting` ends (after its closing brace at src/app.rs:3717, before `pub fn apply_authorship_formatting`):

```rust
/// Liturgical typography for Book of Common Prayer works. Mirrors
/// apply_dialogue_formatting's per-line tag application, but styles the
/// `## ` headings, `[...]` rubrics, and whole-word GOD/LORD that the BCP data
/// carries. Reuses the same TextTag primitives (centered, italic, small-caps,
/// indent) the speaker/stage-direction code already proves out.
pub fn apply_bcp_formatting(state: &mut AppState) {
    use crate::db::line_types;

    if state.line_map.is_none() {
        state.dialogue_formatting_active = false;
        return;
    }
    state.dialogue_formatting_active = true;
    state.text_view.set_pixels_above_lines(0);
    state.text_view.set_pixels_below_lines(0);

    let tag_table = state.buffer.tag_table();
    for name in &[
        "bcp-heading", "bcp-rubric-centered", "bcp-rubric-hanging",
        "bcp-divine-name", "bcp-blank",
    ] {
        if let Some(old) = tag_table.lookup(name) {
            tag_table.remove(&old);
        }
    }

    let base_margin = state.text_view.left_margin();

    let heading_tag = gtk4::TextTag::builder()
        .name("bcp-heading")
        .justification(gtk4::Justification::Center)
        .weight(700)
        .scale(1.1)
        .pixels_above_lines(12)
        .pixels_below_lines(6)
        .build();
    let rubric_centered_tag = gtk4::TextTag::builder()
        .name("bcp-rubric-centered")
        .justification(gtk4::Justification::Center)
        .style(pango::Style::Italic)
        .pixels_above_lines(6)
        .build();
    // Hanging indent: the paragraph is pushed in by `left_margin`, while the
    // first line is pulled back out by a negative `indent`, so wrapped lines
    // sit indented under a flush opening — the printed-rubric look.
    let rubric_hanging_tag = gtk4::TextTag::builder()
        .name("bcp-rubric-hanging")
        .style(pango::Style::Italic)
        .left_margin(base_margin + 24)
        .indent(-24)
        .pixels_above_lines(6)
        .build();
    let divine_name_tag = gtk4::TextTag::builder()
        .name("bcp-divine-name")
        .variant(pango::Variant::SmallCaps)
        .build();
    let blank_tag = gtk4::TextTag::builder()
        .name("bcp-blank")
        .scale(0.25)
        .build();

    tag_table.add(&heading_tag);
    tag_table.add(&rubric_centered_tag);
    tag_table.add(&rubric_hanging_tag);
    tag_table.add(&divine_name_tag);
    tag_table.add(&blank_tag);

    let line_count = state.buffer.line_count() as usize;
    for i in 0..line_count {
        let Some(line_start) = state.buffer.iter_at_line(i as i32) else { continue };
        let line_end = if i + 1 < line_count {
            state.buffer.iter_at_line((i + 1) as i32).unwrap_or_else(|| state.buffer.end_iter())
        } else {
            state.buffer.end_iter()
        };
        let text = state.buffer.text(&line_start, &line_end, false);
        let text = text.trim_end_matches('\n').to_string();
        let trimmed = text.trim();

        if line_types::is_blank(&text) {
            state.buffer.apply_tag(&blank_tag, &line_start, &line_end);
        } else if line_types::is_bcp_heading(&text) {
            state.buffer.apply_tag(&heading_tag, &line_start, &line_end);
        } else if line_types::is_rubric(&text) {
            let inner = &trimmed[1..trimmed.len() - 1]; // strip [ ]
            if line_types::rubric_is_centered(inner) {
                state.buffer.apply_tag(&rubric_centered_tag, &line_start, &line_end);
            } else {
                state.buffer.apply_tag(&rubric_hanging_tag, &line_start, &line_end);
            }
        }
        // Divine-name small-caps applies on ANY non-blank line (headings,
        // rubrics, body), layered over the line tag above.
        if !line_types::is_blank(&text) {
            for (s, e) in line_types::divine_name_spans(&text) {
                // Build span iters with the codebase's idiom (iter_at_line +
                // forward_chars, as in the label-span code ~app.rs:3416), not
                // iter_at_line_offset. divine_name_spans returns BYTE offsets;
                // convert to CHAR offsets so multi-byte chars (¶, curly quotes)
                // earlier in the line don't misplace the span.
                let Some(mut span_start) = state.buffer.iter_at_line(i as i32) else { continue };
                span_start.forward_chars(char_offset(&text, s) as i32);
                let mut span_end = span_start;
                span_end.forward_chars((char_offset(&text, e) - char_offset(&text, s)) as i32);
                state.buffer.apply_tag(&divine_name_tag, &span_start, &span_end);
            }
        }
    }

    crate::logging::log(&format!(
        "FORMATTING: applied BCP formatting ({} lines)",
        line_count
    ));
}

/// GTK text iters address characters, but divine_name_spans returns BYTE
/// offsets (regex match positions). Convert a byte offset within `line` to a
/// char offset so `iter_at_line_offset` lands correctly even with multi-byte
/// characters (¶, curly quotes) earlier in the line.
fn char_offset(line: &str, byte_off: usize) -> usize {
    line[..byte_off].chars().count()
}
```

- [ ] **Step 4: Build and lint**

Run: `cargo build 2>&1 | tail -15`
Expected: builds clean.
Run: `cargo clippy 2>&1 | tail -15`
Expected: no new warnings on the added code.

- [ ] **Step 5: Run the full test suite (no regressions)**

Run: `cargo test 2>&1 | tail -15`
Expected: all tests pass (the new `line_types` tests + existing suite).

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(bcp): apply_bcp_formatting — centered headings, rubrics, small-caps GOD/LORD"
```

---

## Task 5: Rite-title predicate (ornament rendering deferred)

**Files:**
- Modify: `src/app.rs`, `src/db/line_types.rs`

**Scope note — ornaments are deliberately deferred to a follow-up.** The spec
listed `❧` ornaments on rite titles as in-scope (approach (a): inject the glyph
into displayed heading text). On contact with the code this is riskier than the
spec assumed: GTK `TextTag`s cannot inject glyphs, so the only way to show an
ornament is to edit the buffer text at build time — but the buffer is indexed by
search, navigation, cursor-landing, and MPV sync, all of which address character
offsets. Adding glyphs to heading lines would shift those offsets and risk
desyncing several subsystems. That work deserves its own brainstorm/spec rather
than a bolted-on task here.

This task therefore lands only the `is_bcp_rite_title` predicate (cheap, tested,
useful for the future ornament pass and as a hook) and documents the deferral in
code. The centered bold heading from Task 4 is the rite-title treatment until
ornaments land. **Flag this deferral to the user at plan-completion.**

- [ ] **Step 1: Add a predicate test** — in `src/db/line_types.rs` `mod tests`:

```rust
#[test]
fn test_is_bcp_rite_title() {
    assert!(is_bcp_rite_title("## THE SUPPER"));
    assert!(is_bcp_rite_title("## AN ORDER FOR MORNING"));
    // Mixed-case heading is a sub-heading, not a rite title.
    assert!(!is_bcp_rite_title("## The third Collect: for grace."));
    assert!(!is_bcp_rite_title("Our Father")); // not a heading
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test line_types::tests::test_is_bcp_rite_title 2>&1 | tail -20`
Expected: FAIL — `is_bcp_rite_title` not found.

- [ ] **Step 3: Implement the predicate** — in `src/db/line_types.rs`:

```rust
/// A top-level BCP rite title: a `## ` heading whose text is all-caps (e.g.
/// "## THE SUPPER"). Distinguished from mixed-case sub-headings so only rite
/// titles get ornamental flourishes.
pub fn is_bcp_rite_title(text: &str) -> bool {
    let Some(rest) = text.trim().strip_prefix("## ") else { return false };
    let rest = rest.trim();
    !rest.is_empty()
        && rest.chars().any(|c| c.is_alphabetic())
        && rest.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase())
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test line_types::tests::test_is_bcp_rite_title 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Apply ornaments in the heading branch** — in `apply_bcp_formatting` (src/app.rs), the ornament is buffer text, so it cannot be added in the tag pass. Instead, layer it as a tag-driven visual only where safe: GTK tags cannot inject glyphs, so for the first cut we render the ornament by widening the heading's vertical space and rely on the centered bold style as the title treatment. Record the deferral explicitly:

Add this comment in the `is_bcp_heading` branch of `apply_bcp_formatting`, right after applying `heading_tag`:

```rust
            // Ornamental ❧ flourishes on rite titles (is_bcp_rite_title) are
            // deferred: GTK TextTags cannot inject glyphs, and editing buffer
            // text here would desync search/navigation offsets. A future pass
            // injects ornaments at buffer-build time (where `## ` is stripped),
            // or draws them via an overlay. Centered bold is the title look
            // until then.
            let _ = line_types::is_bcp_rite_title(&text);
```

- [ ] **Step 6: Build, lint, test**

Run: `cargo build 2>&1 | tail -10` — clean.
Run: `cargo clippy 2>&1 | tail -10` — no new warnings.
Run: `cargo test 2>&1 | tail -10` — all pass.

- [ ] **Step 7: Commit**

```bash
git add src/db/line_types.rs src/app.rs
git commit -m "feat(bcp): is_bcp_rite_title predicate; document ornament deferral"
```

---

## Task 6: Verification pass

**Files:** none (verification only).

- [ ] **Step 1: Full build + lint + test**

Run: `cd ~/utono/linux-lit && cargo build && cargo clippy && cargo test 2>&1 | tail -15`
Expected: clean build, no new clippy warnings, all tests pass.

- [ ] **Step 2: Confirm Shakespeare path is untouched**

Verify by inspection that a non-BCP work never reaches `apply_bcp_formatting`: the gate in `apply_dialogue_formatting` only delegates when `is_bcp_work(&w.abbrev)`. Run the existing dialogue-formatting-adjacent tests:

Run: `cargo test 2>&1 | rg -i "dialogue|speaker|stage|line_types" | tail -20`
Expected: all green.

- [ ] **Step 3: Headless visual self-check (per CLAUDE.md)**

Follow the headless verification path in `~/utono/linux-lit/CLAUDE.md` to launch with a BCP work (e.g. `LIT_START_WORK=BCP1559M`) without touching the user's running instance. Confirm: centered bold rite titles; italic rubrics (short ones centered, long ones hanging-indented); small-caps GOD/LORD in body lines. If the headless path is unavailable, STOP and ask the user to verify `cargo run` with `LIT_START_WORK=BCP1559M`.

- [ ] **Step 4: Final commit (if any verification fixups were needed)**

```bash
git add -A
git commit -m "test(bcp): verify decorative typography renders; Shakespeare unaffected"
```
