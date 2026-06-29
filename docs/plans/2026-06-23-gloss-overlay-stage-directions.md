# Gloss Overlay Stage Directions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show stage directions, interleaved in true position and rendered italic, in the gloss overlay's source verse — in both the loading card and the result card — so it closely resembles the main reading card.

**Architecture:** Add a `<stage>` element to the gloss markup vocabulary, rendered italic and treated as a non-cursor-stop inside the current source block. The loading card already builds its passage from the real selected lines via `build_source_header`; teach that function to emit `<stage>` for stage-direction lines. The result card renders from the stored gloss text (whose `<verse>` tags omit stage directions), so add a pure text transform that injects the missing `<stage>` lines into the stored gloss text by matching its verse sequence against the real source lines — purely additive, preserving all explication blocks, cursor stops, line numbers, and audio coloring.

**Tech Stack:** Rust, GTK4 (gtk4-rs), Pango text tags.

## Global Constraints

- Do not run the app (`cargo run`); the user runs it. Agent verifies with `cargo build`, `cargo test --bins`, `cargo clippy`. Visual criteria require the user to launch.
- Authoritative metadata: a stage-direction line is identified by `crate::db::line_types::is_stage_direction(&line.text)`, never by re-classifying buffer text in a pagination path. This change is display-only and does not touch pagination.
- Stage directions are display-only: never a cursor stop, never collected for TTS (same as `GlossElement::Pron`).
- No change to stored gloss data, the gloss prompt, or re-glossing.
- US Central time for any timestamps. Commit messages end with the repo's Co-Authored-By / Claude-Session trailer lines.

---

## File map

- `src/ui/gloss_block.rs` — add `GlossElement::Stage`; parse `<stage>`; keep `Stage` transparent to speaker carry-forward; include `Stage` in the current Source block (Task 1, 2).
- `src/ui/gloss_overlay.rs` — render `<stage>` italic in `populate_gloss_buffer_ex`; inject stage lines into the result-card gloss text inside `show_gloss_with_color` (Task 3, 5).
- `src/input/actions/echoes.rs` — `build_source_header` emits `<stage>` for stage-direction lines (Task 4).
- Tests live in the existing `#[cfg(test)] mod` blocks of each file (all pure, run under `cargo test --bins`).

---

### Task 1: `GlossElement::Stage` — parse `<stage>` tags

**Files:**
- Modify: `src/ui/gloss_block.rs` (enum at lines 8-13; `parse_gloss_tags` at 223-250; tests mod at 407+)
- Test: `src/ui/gloss_block.rs` (block_tests mod)

**Interfaces:**
- Produces: `GlossElement::Stage(String)` variant; `parse_gloss_tags` emits it for `<stage>…</stage>` spans.

- [ ] **Step 1: Write the failing test**

Add to the `block_tests` mod in `src/ui/gloss_block.rs`:

```rust
#[test]
fn parse_extracts_stage_element() {
    let g = "<verse>Lay hands upon these traitors and their trash.</verse>\n\
             <stage>[To Jourdain.]</stage>\n\
             <verse>Beldam, I think we watched you at an</verse>";
    let els = parse_gloss_tags(g);
    assert!(matches!(&els[0], GlossElement::Verse(_)));
    assert!(
        matches!(&els[1], GlossElement::Stage(t) if t == "[To Jourdain.]"),
        "expected a Stage element carrying the direction, got {:?}", els.get(1)
    );
    assert!(matches!(&els[2], GlossElement::Verse(_)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins parse_extracts_stage_element`
Expected: FAIL — `no variant named Stage found for enum GlossElement` (compile error).

- [ ] **Step 3: Add the enum variant**

In `src/ui/gloss_block.rs`, change the enum (lines 7-13) to:

```rust
#[derive(Debug)]
pub(crate) enum GlossElement {
    Speaker(String),
    Verse(String),
    Gloss(String),
    Pron(String),
    Stage(String),
}
```

- [ ] **Step 4: Parse the `<stage>` tag**

In `parse_gloss_tags` (`src/ui/gloss_block.rs`), add a `stage` arm. Place it alongside the other `try_extract` arms, immediately after the `verse` arm (lines 233-235), so the chain reads:

```rust
            } else if let Some(el) = try_extract(after_open, "verse") {
                elements.push(GlossElement::Verse(el.0.to_string()));
                remaining = el.1;
            } else if let Some(el) = try_extract(after_open, "stage") {
                elements.push(GlossElement::Stage(el.0.to_string()));
                remaining = el.1;
            } else if let Some(el) = try_extract(after_open, "gloss") {
```

- [ ] **Step 5: Make `carry_forward_block_speakers` exhaustive**

`carry_forward_block_speakers` (lines 264-291) matches on `&el`. Add a `Stage` arm that is transparent (does not reset `prev_was_gloss`, does not change `last_speaker`). Change the `Pron` arm line (286) region to:

```rust
            GlossElement::Gloss(_) => prev_was_gloss = true,
            GlossElement::Pron(_) => {}
            GlossElement::Stage(_) => {}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test --bins parse_extracts_stage_element`
Expected: PASS.

- [ ] **Step 7: Build to catch other non-exhaustive matches**

Run: `cargo build`
Expected: this will FAIL with non-exhaustive `match` errors in `gloss_blocks` (gloss_block.rs ~195) and `populate_gloss_buffer_ex` (gloss_overlay.rs ~1950). Those are fixed in Tasks 2 and 3. If you want a green build at this commit, add a temporary `GlossElement::Stage(_) => {}` arm to each; Tasks 2 and 3 replace them. (Recommended: do Task 2 before committing so the build stays green.)

- [ ] **Step 8: Commit (after Task 2 if keeping build green)**

```bash
git add src/ui/gloss_block.rs
git commit -m "$(cat <<'EOF'
feat(gloss): parse <stage> directions into GlossElement::Stage

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 2: `Stage` stays inside the source block (no extra cursor stop)

**Files:**
- Modify: `src/ui/gloss_block.rs` — `gloss_blocks` (lines 172-220)
- Test: `src/ui/gloss_block.rs` (block_tests mod)

**Interfaces:**
- Consumes: `GlossElement::Stage` (Task 1).
- Produces: `gloss_blocks` includes Stage lines in the current Source block's text/span; a Stage line is NOT its own cursor stop.

- [ ] **Step 1: Write the failing test**

Add to `block_tests`:

```rust
#[test]
fn stage_line_stays_in_source_block() {
    // A stage direction between two verses by the same speaker must not split
    // the source block or create an extra cursor stop.
    let gloss = "<speaker>YORK</speaker>\n\
                 <verse>Lay hands upon these traitors and their trash.</verse>\n\
                 <stage>[To Jourdain.]</stage>\n\
                 <verse>Beldam, I think we watched you at an</verse>\n\
                 <gloss>York gloatingly arrests the conjurers.</gloss>";
    let blocks = gloss_blocks(gloss);
    // Exactly one Source block + one Explication block.
    let sources: Vec<_> = blocks.iter()
        .filter(|b| b.kind == BlockKind::Source).collect();
    assert_eq!(sources.len(), 1, "stage line must not split the source block");
    // The source block's text includes the stage line.
    assert!(
        sources[0].text.contains("[To Jourdain.]"),
        "source block text should carry the stage line, got {:?}", sources[0].text
    );
    // And both verses.
    assert!(sources[0].text.contains("Lay hands"));
    assert!(sources[0].text.contains("Beldam"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins stage_line_stays_in_source_block`
Expected: FAIL — either a non-exhaustive `match` compile error in `gloss_blocks`, or (if a temp arm was added in Task 1) the assertion `source block text should carry the stage line` fails because the stage line was dropped.

- [ ] **Step 3: Accumulate Stage into the pending source run**

In `gloss_blocks` (`src/ui/gloss_block.rs`), the element loop (lines 194-216) matches each element. Add a `Stage` arm right after the `Verse` arm (line 197) so stage lines join `pending_verses` like verse lines:

```rust
            GlossElement::Verse(text) => pending_verses.push(text.trim().to_string()),
            GlossElement::Stage(text) => pending_verses.push(text.trim().to_string()),
```

(If a temporary `Stage(_) => {}` arm was added at the bottom of this match in Task 1, remove it now — a duplicate arm is a compile error.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins stage_line_stays_in_source_block`
Expected: PASS.

- [ ] **Step 5: Verify the whole pure suite still passes**

Run: `cargo test --bins`
Expected: PASS (the existing `speakerless_verse_block_carries_forward_prior_speaker` and `parse_extracts_pron_element` tests still pass; `gloss_overlay.rs` still fails to *compile* if its match isn't yet exhaustive — if so, proceed to Task 3 before relying on a green `cargo build`).

- [ ] **Step 6: Commit**

```bash
git add src/ui/gloss_block.rs
git commit -m "$(cat <<'EOF'
feat(gloss): keep <stage> lines inside the source block, no extra cursor stop

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 3: Render `<stage>` italic in `populate_gloss_buffer_ex`

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — tag cleanup list (line 1800), tag construction (1828-1922), element loop (1941-2048)

**Interfaces:**
- Consumes: `GlossElement::Stage` (Task 1).
- Produces: stage lines render italic at the verse left margin; no line-number gutter entry for a stage line.

This task has no standalone unit test (GTK rendering is verified visually by the user per the Global Constraints). The deliverable is a clean compile plus correct on-screen italic, confirmed in Task 6.

- [ ] **Step 1: Add `gloss-stage` to the tag cleanup list**

In `populate_gloss_buffer_ex` (`src/ui/gloss_overlay.rs:1800`), add `"gloss-stage"` to the names array:

```rust
    for name in &["gloss-speaker", "gloss-speaker-first", "gloss-speaker-source", "gloss-verse", "gloss-stage", "gloss-para", "gloss-bracket", "gloss-quote", "gloss-quote-cont", "gloss-citation", "gloss-pron"] {
```

- [ ] **Step 2: Build the `stage_tag`**

In `populate_gloss_buffer_ex`, immediately after the `verse_tag` definition (lines 1828-1831), add:

```rust
    // Stage direction inside the quoted source turn: same indent as verse, but
    // italic — matching the main reading card. Not a cursor stop, not TTS.
    let stage_tag = gtk4::TextTag::builder()
        .name("gloss-stage")
        .left_margin(quote_verse)
        .style(pango::Style::Italic)
        .build();
```

- [ ] **Step 3: Register the tag**

In the `tag_table.add(...)` block (lines 1913-1922), add after `tag_table.add(&verse_tag);`:

```rust
    tag_table.add(&verse_tag);
    tag_table.add(&stage_tag);
```

- [ ] **Step 4: Add the `Stage` render arm**

In the element loop, add a `GlossElement::Stage(text)` arm. Place it right after the `GlossElement::Verse(text) => { … }` arm (ends at line 1979), before `GlossElement::Gloss`:

```rust
            GlossElement::Stage(text) => {
                only_speakers_so_far = false;
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, text);
                let start = buffer.iter_at_offset(offset);
                buffer.apply_tag(&stage_tag, &start, &buffer.end_iter());
                // No line-number gutter entry: stage directions are not numbered
                // verse lines.
            }
```

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: PASS (all `GlossElement` matches now exhaustive).

- [ ] **Step 6: Run the pure suite + clippy**

Run: `cargo test --bins && cargo clippy`
Expected: PASS, no new warnings.

- [ ] **Step 7: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "$(cat <<'EOF'
feat(gloss): render <stage> directions italic in the overlay source block

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 4: `build_source_header` emits `<stage>` (fixes the loading card)

**Files:**
- Modify: `src/input/actions/echoes.rs` — `build_source_header` (lines 661-673); tests mod (~1714+)

**Interfaces:**
- Consumes: `crate::db::line_types::is_stage_direction` (existing).
- Produces: `build_source_header(&[Line], &str) -> String` emits `<stage>{text}</stage>` for stage-direction lines, with no `<speaker>` change across them; verse lines unchanged.

This fixes BOTH the loading card (`show_glossing`, which already renders `build_source_header` output) and the echoes source header.

- [ ] **Step 1: Write the failing test**

Add to the tests mod in `src/input/actions/echoes.rs` (near the existing `build_source_header` tests ~1714). Use the existing `line(id, speaker, div1, div2, line_in_div, text)` test helper (defined at line 1643 in this same mod) — do not invent a new constructor.

```rust
#[test]
fn build_source_header_emits_stage_for_directions() {
    let turn = vec![
        line(20, Some("YORK"), 1, 4, 43, "Lay hands upon these traitors and their trash."),
        line(21, Some("YORK"), 1, 4, 44, "[To Jourdain.]"),
        line(22, Some("YORK"), 1, 4, 45, "Beldam, I think we watched you at an"),
    ];
    let doc = build_source_header(&turn, "YORK");
    // The stage direction is a <stage> tag, not a <verse> tag.
    assert!(doc.contains("<stage>[To Jourdain.]</stage>"),
        "stage line must be tagged <stage>, got:\n{doc}");
    assert!(!doc.contains("<verse>[To Jourdain.]</verse>"),
        "stage line must NOT be a <verse>, got:\n{doc}");
    // The speaker is emitted once for the whole same-speaker turn; the stage
    // line does not re-trigger a <speaker>.
    assert_eq!(doc.matches("<speaker>YORK</speaker>").count(), 1,
        "stage line must not re-emit the speaker, got:\n{doc}");
    // Real verse lines remain <verse>.
    assert!(doc.contains("<verse>Beldam, I think we watched you at an</verse>"));
}
```

(The `line(...)` helper sets `is_dialogue: true`; `is_stage_direction` keys on the `text` (`^\[.*\]$`), not on `is_dialogue`, so the `[To Jourdain.]` line is still detected as a stage direction.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins build_source_header_emits_stage_for_directions`
Expected: FAIL — `stage line must be tagged <stage>` (the current code wraps every line in `<verse>`).

- [ ] **Step 3: Emit `<stage>` for stage-direction lines**

Replace `build_source_header` (`src/input/actions/echoes.rs:661-673`) with:

```rust
pub(crate) fn build_source_header(turn: &[Line], speaker: &str) -> String {
    let mut doc = String::new();
    let mut current: Option<String> = None;
    for line in turn {
        // Stage directions render as their own italic line in the overlay; they
        // carry no speaker and must not reset the current speaker run.
        if crate::db::line_types::is_stage_direction(&line.text) {
            doc.push_str(&format!("<stage>{}</stage>\n", line.text));
            continue;
        }
        let label = line.speaker.as_deref().unwrap_or(speaker).to_uppercase();
        if current.as_deref() != Some(label.as_str()) {
            doc.push_str(&format!("<speaker>{}</speaker>\n", label));
            current = Some(label);
        }
        doc.push_str(&format!("<verse>{}</verse>\n", line.text));
    }
    doc
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins build_source_header_emits_stage_for_directions`
Expected: PASS.

- [ ] **Step 5: Run the existing `build_source_header` tests**

Run: `cargo test --bins build_source_header`
Expected: PASS — the three existing tests (lines 1714, 1733, 1754) still pass (their fixtures contain no stage directions, so behavior is unchanged).

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/echoes.rs
git commit -m "$(cat <<'EOF'
feat(gloss): build_source_header emits <stage> for stage directions

Fixes the Glossing… loading card and the echoes source header so stage
directions appear interleaved with the verse.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 5: Inject `<stage>` into the result-card gloss text

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — new helper `inject_stage_directions`; call it at the top of `show_gloss_with_color` (line 605)
- Test: `src/ui/gloss_overlay.rs` (add a `#[cfg(test)] mod` if none exists in this file, or extend the existing one)

**Interfaces:**
- Consumes: `crate::ui::gloss_ipa::{strip_ipa, strip_brackets}`, `crate::db::line_types::is_stage_direction`, `crate::ui::gloss_block::parse_gloss_tags` is NOT needed — operate on raw `<verse>` spans for safety.
- Produces: `fn inject_stage_directions(gloss_text: &str, source_text: &str) -> String` — returns `gloss_text` with `<stage>…</stage>` lines inserted before the `<verse>` line that follows them in the real source order. Purely additive; if nothing matches, returns `gloss_text` unchanged.

**Why this shape:** The result card renders the stored `gloss_text`, whose `<verse>` tags omit stage directions, while the model interleaves `<gloss>` ledes between verse blocks (so we cannot wholesale-replace the source turn). `source_text` (= `ctx.source_text`, every selected line verbatim) and the stored verse sequence are both in document order. We walk the stored verse lines, advancing a cursor through the real source lines; any stage-direction line the cursor passes over is spliced in as a `<stage>` line at that point. This preserves every explication block, cursor stop, line number, and audio color, because it only adds `<stage>` lines.

- [ ] **Step 1: Write the failing test**

Add (creating the `mod tests` if needed) at the end of `src/ui/gloss_overlay.rs`:

```rust
#[cfg(test)]
mod stage_inject_tests {
    use super::inject_stage_directions;

    #[test]
    fn injects_stage_between_verses() {
        // Stored gloss: verse only, with an explication lede between blocks.
        let gloss = "<speaker>YORK</speaker>\n\
                     <verse>Lay hands upon these traitors and their trash.</verse>\n\
                     <verse>Beldam, I think we watched you at an</verse>\n\
                     <gloss>York gloatingly arrests the conjurers.</gloss>";
        // Real source: same lines with a stage direction interleaved.
        let source = "Lay hands upon these traitors and their trash.\n\
                      [To Jourdain.]\n\
                      Beldam, I think we watched you at an";
        let out = inject_stage_directions(gloss, source);
        // The stage line is injected, before the verse that follows it.
        assert!(out.contains("<stage>[To Jourdain.]</stage>"),
            "expected injected stage line, got:\n{out}");
        let stage_at = out.find("[To Jourdain.]").unwrap();
        let beldam_at = out.find("Beldam").unwrap();
        assert!(stage_at < beldam_at,
            "stage must precede the following verse, got:\n{out}");
        // Explication is untouched.
        assert!(out.contains("<gloss>York gloatingly arrests the conjurers.</gloss>"));
    }

    #[test]
    fn no_stage_in_source_is_identity() {
        let gloss = "<speaker>YORK</speaker>\n<verse>Lay hands.</verse>";
        let source = "Lay hands.";
        assert_eq!(inject_stage_directions(gloss, source), gloss);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins injects_stage_between_verses`
Expected: FAIL — `cannot find function inject_stage_directions`.

- [ ] **Step 3: Implement the helper**

Add this free function in `src/ui/gloss_overlay.rs` (near `populate_gloss_buffer`, e.g. just above `fn populate_gloss_buffer` at line 1788). It rewrites the raw `<verse>…</verse>` spans in order, inserting any intervening `<stage>` lines:

```rust
/// Insert `<stage>…</stage>` lines into a stored gloss's source verse so the
/// result card shows stage directions the model omitted. `source_text` is the
/// real selected passage (one line per `\n`, verbatim from `line.text`),
/// `gloss_text` is the stored gloss whose `<verse>` tags carry only verse.
///
/// We walk the stored `<verse>` spans in document order, advancing a cursor
/// through the real source lines. Each real stage-direction line the cursor
/// passes (before the line that matches the next stored verse) is spliced in as
/// a `<stage>` line immediately before that verse. Purely additive: explication
/// `<gloss>` blocks, line numbers, cursor stops, and audio coloring are
/// untouched. If a stored verse never matches a real line, the cursor does not
/// advance for it, so we only ever inject unambiguously-positioned stage lines.
fn inject_stage_directions(gloss_text: &str, source_text: &str) -> String {
    use crate::db::line_types::is_stage_direction;
    use crate::ui::gloss_ipa::{strip_brackets, strip_ipa};

    let source_lines: Vec<&str> = source_text.lines().collect();
    // Fast exit: nothing to inject.
    if !source_lines.iter().any(|l| is_stage_direction(l)) {
        return gloss_text.to_string();
    }

    // Normalize a verse/source line for matching: drop inline IPA and brackets,
    // trim. Mirrors the line-number gutter match in populate_gloss_buffer_ex.
    let norm = |s: &str| -> String { strip_brackets(&strip_ipa(s)).trim().to_string() };

    let mut out = String::with_capacity(gloss_text.len() + 64);
    let mut src_cursor = 0usize; // next unconsumed real source line
    let mut rest = gloss_text;

    while let Some(open) = rest.find("<verse>") {
        let after_open = open + "<verse>".len();
        let close = match rest[after_open..].find("</verse>") {
            Some(c) => after_open + c,
            None => break, // malformed; bail, leaving remainder intact below
        };
        let verse_inner = &rest[after_open..close];
        let want = norm(verse_inner);

        // Find this verse in the real source from the cursor; collect any stage
        // lines passed on the way.
        let mut pending_stage: Vec<&str> = Vec::new();
        let mut matched_at: Option<usize> = None;
        let mut i = src_cursor;
        while i < source_lines.len() {
            let line = source_lines[i];
            if is_stage_direction(line) {
                pending_stage.push(line);
                i += 1;
                continue;
            }
            if norm(line) == want {
                matched_at = Some(i);
                break;
            }
            // A real verse line that doesn't match this stored verse: stop
            // scanning so we don't swallow stage lines belonging to a later
            // verse. Leave the cursor; this stored verse simply isn't matched.
            break;
        }

        // Emit everything up to and including this </verse>, with any stage
        // lines spliced in immediately before the <verse> open tag.
        out.push_str(&rest[..open]);
        if matched_at.is_some() {
            for sd in &pending_stage {
                out.push_str(&format!("<stage>{}</stage>\n", sd));
            }
        }
        let verse_end = close + "</verse>".len();
        out.push_str(&rest[open..verse_end]);

        if let Some(m) = matched_at {
            src_cursor = m + 1;
        }
        rest = &rest[verse_end..];
    }
    out.push_str(rest);
    out
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins injects_stage_between_verses no_stage_in_source_is_identity`
Expected: PASS.

- [ ] **Step 5: Call the helper in `show_gloss_with_color`**

In `show_gloss_with_color` (`src/ui/gloss_overlay.rs:605`), the first parameter `_original` is the real source text (every caller passes `&ctx.source_text`). Use it to inject, then render the injected text. At the very top of the function body (right after the doc-comment, before line 607 `self.synopsis_label_ranges...`), add:

```rust
    pub fn show_gloss_with_color(&self, _original: &str, gloss: &str, card_width: i32, card_height: i32, root_color: Option<&str>, source_line_numbers: &[(String, i64)]) {
        // Splice in any stage directions the stored gloss omitted, so the source
        // block matches the main reading card. `_original` is the real passage
        // (every caller passes ctx.source_text).
        let gloss_injected = inject_stage_directions(gloss, _original);
        let gloss = gloss_injected.as_str();
        // No synopsis label bolding in gloss view.
        self.synopsis_label_ranges.borrow_mut().clear();
```

The rest of the function already uses the local `gloss` binding (e.g. `populate_gloss_buffer(... gloss ...)` at 648, `self.rebuild_block_ranges(gloss)` at 659), so it now operates on the injected text with no further changes.

- [ ] **Step 6: Build, test, clippy**

Run: `cargo build && cargo test --bins && cargo clippy`
Expected: PASS, no new warnings. (`_original` is now read; if clippy or the compiler suggests renaming the now-used `_original` parameter to `original`, do so and update the one use — purely cosmetic.)

- [ ] **Step 7: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "$(cat <<'EOF'
feat(gloss): inject stage directions into the result card's source verse

Walk the stored gloss's <verse> sequence against the real selected lines and
splice in the omitted <stage> directions. Additive only — explication blocks,
cursor stops, line numbers, and audio coloring are preserved.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 6: Full verification + user visual check

**Files:** none (verification only).

- [ ] **Step 1: Full build + pure suite + clippy**

Run: `cargo build && cargo test --bins && cargo clippy`
Expected: all PASS, no new warnings.

- [ ] **Step 2: Confirm no pagination/test regressions in the bins suite**

Run: `cargo test --bins`
Expected: PASS — this change is display-only; no `nav_test`/viewport assertions should move.

- [ ] **Step 3: Ask the user to visually verify (visual criterion)**

Per the Global Constraints the agent must not launch the app. Ask the user to:
1. Open 2H6 and select the passage at 1.4.43–50 (the York "Lay hands…" turn, which includes `[To Jourdain.]`, `[The Guard arrest…]`, `[To the Duchess, aloft.]`).
2. Press the reader-gloss key to open the gloss overlay (both the `Glossing…` loading card and the result card).
3. Confirm the source block now interleaves the italic stage directions in their true positions, matching the main reading card (image #6) — and that `j`/`k` skip over the stage lines (they are not cursor stops).

Provide the manual launch command from CLAUDE.md's *Headless Verification* if they prefer a throwaway compositor:

```bash
cd ~/utono/linux-lit && cargo build
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
```

- [ ] **Step 4: Finish the branch**

Once the user confirms the visual result, follow the repo's "Finishing a Branch" convention (merge `--no-ff` to master, re-verify, push, delete the branch).

---

## Self-Review

**Spec coverage:**
- "New `<stage>` element / parse" → Task 1. ✓
- "`<stage>` is part of the current Source block, not a cursor stop, not TTS" → Task 2 (block) + Task 3 (no gutter; render only). ✓
- "Render `<stage>` italic in `populate_gloss_buffer_ex`" → Task 3. ✓
- "`build_source_header` emits `<stage>`; fixes loading card + echoes" → Task 4. ✓
- "Result card builds source from real lines (with stage directions)" → Task 5 (realized as additive injection rather than source-run replacement — see note below). ✓
- "Out of scope: no stored-data/prompt/TTS changes; no unrelated file split" → honored; no `source_block.rs` module was extracted. ✓
- Testing (pure unit tests for parse/blocks/build_source_header + injection; visual check) → Tasks 1,2,4,5,6. ✓
- Acceptance criteria (both cards; not a cursor stop; line numbers/coloring/explication unchanged; build/test/clippy pass) → Tasks 3,5,6. ✓

**Deviation from spec, noted intentionally:** The spec §3 proposed extracting a `source_block` module and splicing a rebuilt source header over the stored `<verse>` run for the result card. During planning the stored gloss was confirmed to interleave `<gloss>` ledes between multiple verse blocks, which makes wholesale source-run replacement ambiguous and risks dropping the model's per-speaker ledes. The plan instead injects `<stage>` lines additively into the existing verse sequence (Task 5) — same user-visible outcome (interleaved italic stage directions sourced from the real lines), strictly safer for explication/cursor/line-number/coloring invariants, and no module extraction needed (the loading card already routes through `build_source_header`, so the "one source of truth for stage emission" goal is met by Task 4 alone). This is an implementation refinement within the approved design, surfaced here rather than applied silently.

**Placeholder scan:** No TBD/TODO/"handle edge cases"; every code step shows complete code. Task 4's fixture uses the real `line(id, speaker, div1, div2, line_in_div, text)` helper (verified at echoes.rs:1643), so no field is guessed.

**Type consistency:** `GlossElement::Stage(String)` is defined in Task 1 and consumed identically in Tasks 2 and 3. `inject_stage_directions(gloss_text: &str, source_text: &str) -> String` is defined and called with the same signature in Task 5. `build_source_header(&[Line], &str) -> String` keeps its existing signature (Task 4). `is_stage_direction`, `strip_ipa`, `strip_brackets` are used with their real signatures verified in the source.
