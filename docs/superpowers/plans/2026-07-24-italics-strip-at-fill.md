# Strip-at-fill for inline italics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the ~7.3s LoJ load regression from Phase B by stripping `_word_` delimiters in Rust BEFORE `set_text` (instead of ~45k per-underscore `buffer.delete` calls after fill). `apply_inline_italics` becomes a pure tag-application pass.

**Architecture:** A new pure `strip_italics_for_fill(lines) -> ItalicStripResult` runs `parse_italic_spans` per line and returns stripped lines + per-line italic spans + per-line removed-`_` positions. Both `rebuild_buffer_text` fill branches (block-aware for LoJ, default DB-join for BH/PP), gated on `work_type`, strip via this helper, `set_text` the CLEAN text, and stash spans + set the offset map. `apply_inline_italics` no longer parses or deletes — it reads the stashed spans and applies the per-span named italic tags with the net-zero removal (both Phase B fixes preserved verbatim). No buffer deletes at all → the GTK-iterator-safety risk is gone.

**Tech Stack:** Rust, GTK4 (gtk4-rs), cargo test (bin crate), cage/grim e2e.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-24-italics-strip-at-fill-design.md`. Supersedes the STRIPPING mechanism of Phase B (`d316321d`); the feature and parser/offset logic are UNCHANGED.
- **Bin crate:** `cargo test --bin linux-lit <name>` (NOT `--lib`; fall back to plain `cargo test <name>`). Also `cargo build --bin linux-lit`, `cargo clippy --bin linux-lit`.
- Do NOT run the app — user launches it. Headless via cage/grim.
- **Preserve verbatim (do NOT change behavior):** the parser (`parse_italic_spans`), `translate_offset`, the karaoke consumer (`apply_char_range_tag`), the per-span NAMED italic tags (`inline-italic-{i}-{k}` — the multi-span GTK-disjoint-range fix), and the net-zero `foreach`-remove of `inline-italic-*` (the leak fix).
- **Gate:** strip only for `work_type ∈ {prose, prose_book, epic_translation}`. Plays/BCP untouched → `_` stays literal, byte-identical to today.
- **Non-italic lines / non-gated works: byte-identical** (no `_` → not stripped, no span/removed entry; `translate_offset` identity on empty map).
- **Ordering invariant:** stripping happens at fill (inside `rebuild_buffer_text`), which is before `display_work`'s `build_vocab_matches` — so vocab tokenizes stripped text (holds by construction).
- `italic_line_spans` (NEW field) mirrors `italic_offset_map`'s lifecycle: declared `mod.rs:867`-area, init `2413`-area, cleared at the 4 `rebuild_buffer_text` return paths (4375/4457/4475/4489), set on the 2 gated fill branches.
- Branch per convention (worktree off master); commit on the feature branch; merge `--no-ff` from the main checkout. Do NOT stash/checkout/restore user files.

## File Structure

- `src/app/italics.rs` — `strip_italics_for_fill` + `ItalicStripResult` + tests (Task 1); remove `#[allow(dead_code)]` from `ItalicParse.stripped_text` (now load-bearing).
- `src/app/mod.rs` — `AppState.italic_line_spans` field + lifecycle; the two fill branches strip-at-fill (Task 2).
- `src/app/formatting.rs` — slim `apply_inline_italics` to tag-only (Task 3).
- Headless acceptance (Task 4) — no production code.

---

## Task 1: `strip_italics_for_fill` pure helper

**Files:**
- Modify: `src/app/italics.rs` (add struct + fn + tests; drop the `#[allow(dead_code)]` on `stripped_text`)

**Interfaces:**
- Produces:
  ```rust
  pub struct ItalicStripResult {
      pub stripped_lines: Vec<String>,
      pub line_spans: std::collections::HashMap<usize, Vec<(usize, usize)>>,
      pub line_removed: std::collections::HashMap<usize, Vec<usize>>,
  }
  /// For each input line (index = output buffer-line index): run
  /// parse_italic_spans. On Some -> push stripped_text, and record spans +
  /// removed_positions under that line index. On None (no `_` OR odd count) ->
  /// push the line verbatim, no map entry; if the line contained `_` (odd count
  /// defect), log ITALIC_UNPAIRED. Line count of stripped_lines == input len.
  pub fn strip_italics_for_fill(lines: &[String]) -> ItalicStripResult;
  ```

- [ ] **Step 1: Write the failing tests**

Add to `src/app/italics.rs` tests:

```rust
#[test]
fn strip_for_fill_mixed_lines() {
    let lines = vec![
        "plain roman".to_string(),          // 0: no `_`
        "he wrote _London_ later".to_string(), // 1: one span
        "(_Page_ 115, _note_ 4.)".to_string(), // 2: two spans
        "a stray _ underscore".to_string(), // 3: odd -> verbatim
    ];
    let r = strip_italics_for_fill(&lines);
    // stripped text: unchanged lines, `_` removed on 1 & 2, verbatim on 0 & 3
    assert_eq!(r.stripped_lines, vec![
        "plain roman",
        "he wrote London later",
        "(Page 115, note 4.)",
        "a stray _ underscore",            // odd -> left as-is
    ]);
    // spans: only 1 & 2 have entries (display coords)
    assert_eq!(r.line_spans.get(&0), None);
    assert_eq!(r.line_spans.get(&1), Some(&vec![(9, 15)]));   // "London"
    assert_eq!(r.line_spans.get(&2), Some(&vec![(1, 5), (11, 15)])); // "Page","note"
    assert_eq!(r.line_spans.get(&3), None);                  // odd -> no entry
    // removed: source `_` positions on 1 & 2
    assert_eq!(r.line_removed.get(&1), Some(&vec![9, 16]));
    assert_eq!(r.line_removed.get(&2), Some(&vec![1, 6, 13, 18]));
    assert_eq!(r.line_removed.get(&3), None);
}

#[test]
fn strip_for_fill_preserves_line_count_and_indices() {
    let lines: Vec<String> = (0..5).map(|i| format!("_x{i}_ tail")).collect();
    let r = strip_italics_for_fill(&lines);
    assert_eq!(r.stripped_lines.len(), 5);          // 1:1 with input
    for i in 0..5 { assert!(r.line_spans.contains_key(&i)); } // each has a span
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test --bin linux-lit strip_for_fill` — FAIL (fn not found).

- [ ] **Step 3: Implement**

```rust
pub struct ItalicStripResult {
    pub stripped_lines: Vec<String>,
    pub line_spans: std::collections::HashMap<usize, Vec<(usize, usize)>>,
    pub line_removed: std::collections::HashMap<usize, Vec<usize>>,
}

/// Strip paired `_` from each line for buffer-fill. Output index = buffer line
/// index. See parse_italic_spans for the None (no `_` / odd count) rule.
pub fn strip_italics_for_fill(lines: &[String]) -> ItalicStripResult {
    let mut stripped_lines = Vec::with_capacity(lines.len());
    let mut line_spans = std::collections::HashMap::new();
    let mut line_removed = std::collections::HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        // Fast path: no `_` -> verbatim, no entry.
        if !line.contains('_') {
            stripped_lines.push(line.clone());
            continue;
        }
        match parse_italic_spans(line) {
            Some(parse) => {
                stripped_lines.push(parse.stripped_text);
                line_spans.insert(i, parse.spans);
                line_removed.insert(i, parse.removed_positions);
            }
            None => {
                // odd `_` count (unpaired) -> render verbatim + log.
                crate::log_fmt!(
                    "ITALIC_UNPAIRED: line {} odd `_` count, rendered literal: {:?}",
                    i,
                    line.chars().take(60).collect::<String>()
                );
                stripped_lines.push(line.clone());
            }
        }
    }
    ItalicStripResult { stripped_lines, line_spans, line_removed }
}
```

Also in this file: remove the `#[allow(dead_code)]` on `ItalicParse.stripped_text` (it is now read by `strip_italics_for_fill`). Update its doc comment (it is no longer "production does not read this").

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --bin linux-lit strip_for_fill` — PASS (2). Then `cargo test --bin linux-lit italics` — all italics tests (incl. Phase B's) green.

- [ ] **Step 5: Commit**

```bash
git add src/app/italics.rs
git commit -m "feat(reader): strip_italics_for_fill — strip _ + spans + removed per line (pre-set_text)"
```

---

## Task 2: `AppState.italic_line_spans` + strip-at-fill in both branches

**Files:**
- Modify: `src/app/mod.rs` (field + init + 4 clears; strip in the 2 gated fill branches)

**Interfaces:**
- Consumes: `strip_italics_for_fill` (Task 1).
- Produces: `state.italic_line_spans: HashMap<usize, Vec<(usize,usize)>>` (per-line italic spans, set at fill, consumed by Task 3). `italic_offset_map` now ALSO set at fill (from `line_removed`) instead of in the tag pass.

- [ ] **Step 1: Add the field + init + clears**

- Declare next to `italic_offset_map` (`mod.rs:867`):
  ```rust
  /// Per buffer-line italic spans (display coords) for the tag-application pass
  /// (apply_inline_italics). Set at buffer-fill by strip_italics_for_fill;
  /// cleared on every rebuild path. Mirrors italic_offset_map's lifecycle.
  pub italic_line_spans: std::collections::HashMap<usize, Vec<(usize, usize)>>,
  ```
- Init next to `italic_offset_map: …::new(),` (`2413`):
  ```rust
  italic_line_spans: std::collections::HashMap::new(),
  ```
- Add `state.italic_line_spans.clear();` next to EACH `state.italic_offset_map.clear();` (the 4 sites: ~4375, ~4457, ~4475, ~4489). Report the 4 line numbers.

- [ ] **Step 2: Strip at fill — block-aware branch (LoJ, ~mod.rs:4468-4482)**

Currently:
```rust
        let bb = crate::app::text_prep::prepare_block_buffer(&work.lines);
        let line_map = crate::text_file_map::build_line_map_blocks(
            &bb.buf_lines, &bb.source_index, &work.lines,
        );
        state.buffer.set_text(&bb.buf_lines.join("\n"));
        state.line_map = Some(line_map);
        state.block_indent_tiers = bb.indent_tiers;
        state.italic_offset_map.clear();
        crate::app::formatting::apply_block_typography(state);
        crate::app::formatting::apply_inline_italics(state);
        return;
```

Change to strip `bb.buf_lines` BEFORE `set_text`, gated on work_type:
```rust
        let bb = crate::app::text_prep::prepare_block_buffer(&work.lines);
        let line_map = crate::text_file_map::build_line_map_blocks(
            &bb.buf_lines, &bb.source_index, &work.lines,
        );
        // Strip inline `_word_` italics BEFORE set_text (prose/prose_book/
        // epic_translation only) so the buffer never contains `_` — no per-
        // underscore buffer.delete (the ~7.3s LoJ regression). The tag pass
        // (apply_inline_italics) then only APPLIES the italic tags.
        let is_italic_work = matches!(
            work.work_type.as_str(),
            "prose" | "prose_book" | "epic_translation"
        );
        let display_lines: Vec<String> = if is_italic_work {
            let strip = crate::app::italics::strip_italics_for_fill(&bb.buf_lines);
            state.italic_offset_map = strip.line_removed;
            state.italic_line_spans = strip.line_spans;
            strip.stripped_lines
        } else {
            state.italic_offset_map.clear();
            state.italic_line_spans.clear();
            bb.buf_lines
        };
        state.buffer.set_text(&display_lines.join("\n"));
        state.line_map = Some(line_map);
        state.block_indent_tiers = bb.indent_tiers;
        crate::app::formatting::apply_block_typography(state);
        crate::app::formatting::apply_inline_italics(state);
        return;
```

NOTE: `build_line_map_blocks` runs on `bb.buf_lines` (pre-strip) — that is FINE: the LineMap maps buffer-LINE ↔ work-row, and stripping `_` changes a line's char count but NOT the line count or line-to-row mapping (LineMap has no per-char field — verified in Phase A). Confirm `build_line_map_blocks` takes no char offsets (it does not).

- [ ] **Step 3: Strip at fill — default DB-join branch (BH/PP, ~mod.rs:4490-4499)**

Currently:
```rust
    let text: String = work
        .lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    state.buffer.set_text(&text);
    crate::app::formatting::apply_inline_italics(state);
```

Change to strip first (this branch already cleared the maps at the top via the 4th clear site; set them here when italic):
```rust
    let raw_lines: Vec<String> = work.lines.iter().map(|l| l.text.clone()).collect();
    let is_italic_work = matches!(
        work.work_type.as_str(),
        "prose" | "prose_book" | "epic_translation"
    );
    let display_lines: Vec<String> = if is_italic_work {
        let strip = crate::app::italics::strip_italics_for_fill(&raw_lines);
        state.italic_offset_map = strip.line_removed;
        state.italic_line_spans = strip.line_spans;
        strip.stripped_lines
    } else {
        raw_lines
    };
    state.buffer.set_text(&display_lines.join("\n"));
    crate::app::formatting::apply_inline_italics(state);
```
(The `state.italic_offset_map.clear()` / `italic_line_spans.clear()` for this branch already ran earlier per Step 1's 4th clear site — the `is_italic_work` else path leaves them empty. Confirm the 4th clear precedes this code; if not, add the clears to the else path.)

- [ ] **Step 4: Build + regression**

Run: `cargo build --bin linux-lit` (compiles — `apply_inline_italics` still has its old body reading the buffer; that's Task 3. It will still WORK here because the buffer no longer has `_`, so its `!text.contains('_')` fast-path skips every line and it applies NO tags — TEMPORARILY italics won't render between Task 2 and Task 3. That's an expected intermediate; Task 3 makes the tag pass read the spans.) `cargo test --bin linux-lit` — suite green (no unit test here; Task 4 is acceptance). Note the intermediate state in the commit.

- [ ] **Step 5: Commit**

```bash
git add src/app/mod.rs
git commit -m "feat(reader): strip italics at buffer-fill (both fill branches) + italic_line_spans field"
```

---

## Task 3: Slim `apply_inline_italics` to tag-only

**Files:**
- Modify: `src/app/formatting.rs` (`apply_inline_italics`)

**Interfaces:**
- Consumes: `state.italic_line_spans` (Task 2). No longer reads buffer text, parses, or deletes.
- Produces: applies per-span named `inline-italic-{i}-{k}` italic tags from the stashed spans, with the net-zero removal. `italic_offset_map` is NO LONGER set here (Task 2 sets it at fill).

- [ ] **Step 1: Replace the body**

Keep the leading net-zero `foreach`-remove block (verbatim). Replace the whole `for i in 0..line_count { … buffer.text … parse … delete … tag … insert }` loop with a loop over `italic_line_spans`:

```rust
pub fn apply_inline_italics(state: &mut AppState) {
    // Gate: prose / prose_book / epic_translation only. (Redundant with the
    // fill-path gate, but keeps this pass a no-op if called for a non-italic
    // work — italic_line_spans is empty then anyway.)
    let wt = state.current_work.as_ref().map(|w| w.work_type.clone()).unwrap_or_default();
    if !matches!(wt.as_str(), "prose" | "prose_book" | "epic_translation") {
        return;
    }

    // NET-ZERO tag lifecycle (unchanged from Phase B): remove any inline-italic-*
    // tags left by a prior run before re-applying, so the app-lifetime shared tag
    // table doesn't grow across reloads / scansion toggles.
    let tag_table = state.buffer.tag_table();
    {
        let mut stale: Vec<gtk4::TextTag> = Vec::new();
        tag_table.foreach(|t| {
            if t.name().map(|n| n.starts_with("inline-italic-")).unwrap_or(false) {
                stale.push(t.clone());
            }
        });
        for t in stale {
            tag_table.remove(&t);
        }
    }

    // Apply italic tags from the spans computed at buffer-fill
    // (strip_italics_for_fill). The `_` are already gone from the buffer, so
    // there is NO parsing and NO buffer.delete here — this pass only tags.
    //
    // ONE fresh NAMED tag PER SPAN (inline-italic-{line}-{span}): GTK4/Pango
    // renders only the LAST of multiple disjoint ranges of one style=Italic tag
    // within a single paragraph, so a per-span distinct tag is required (a
    // per-line shared tag would be that same failing case). Named so the
    // net-zero removal above can drop them next run.
    let spans = state.italic_line_spans.clone();
    for (&i, line_spans) in spans.iter() {
        let Some(base0) = state.buffer.iter_at_line(i as i32) else { continue };
        let _ = base0; // bounds check
        for (k, &(sc, ec)) in line_spans.iter().enumerate() {
            let base = state.buffer.iter_at_line(i as i32).unwrap();
            let mut a = base;
            a.forward_chars(sc as i32);
            let mut b = base;
            b.forward_chars(ec as i32);
            let span_tag = gtk4::TextTag::builder()
                .name(&format!("inline-italic-{i}-{k}"))
                .style(pango::Style::Italic)
                .build();
            tag_table.add(&span_tag);
            state.buffer.apply_tag(&span_tag, &a, &b);
        }
    }
}
```

(`state.italic_line_spans.clone()` avoids a borrow conflict — reading the map while mutating `state.buffer`. The map is small — one entry per italic line, a handful of span tuples — so the clone is cheap relative to the render. If the borrow checker allows iterating without clone via a scoped borrow, prefer that; otherwise clone.)

- [ ] **Step 2: Build + clippy**

Run: `cargo build --bin linux-lit && cargo clippy --bin linux-lit` — compiles, no new warnings. `cargo test --bin linux-lit` — suite green. The `parse_italic_spans`/`removed_positions` references and the delete loop are GONE from this function; confirm no dead imports remain (e.g. `parse_italic_spans` may now be unused in formatting.rs — remove that `use` if so).

- [ ] **Step 3: Commit**

```bash
git add src/app/formatting.rs
git commit -m "feat(reader): apply_inline_italics is tag-only (reads fill-time spans, no delete)"
```

---

## Task 4: Headless acceptance — italic renders + load time dropped

**Files:** none — cage/grim against live LoJ.

**Interfaces:** the visible-surface + performance gate. Must confirm BOTH the render is preserved AND the load-time regression is fixed.

- [ ] **Step 1: Build** — `cd <worktree> && cargo build` (clean).

- [ ] **Step 2: Launch headless at FIXED 1920x1200 (the cached-page-table path).**

Per CLAUDE.md: `LIT_NO_MPV=1 GSK_RENDERER=cairo LIT_DEV=1`, cage via `run_in_background`, fresh `XDG_RUNTIME_DIR`, stderr + app log captured. Start at 1920x1200 (avoid a 720p→resize which forces a page-table regen artifact and muddies the timing). LoJ as the work.

- [ ] **Step 3: RENDER — italic still correct.**

Navigate to a LoJ multi-span line (`/Life of Johnson` — "Life of Johnson" + "Pre-Crokerian" both italic) and a single-span (`/Rasselas`). Screenshot, zoom. Confirm: `_word_` renders ITALIC, underscores hidden, multi-span line has ALL spans italic, roman text unaffected. Regression: BH (no-`_` prose) + a play (Ham) unchanged.

- [ ] **Step 4: LOAD TIME — the regression is fixed (THE point of this work).**

Grep the app log for `TIMING: rebuild_buffer_text` and `TIMING: display_work total` on the LoJ load. Report the ms. PASS = `rebuild_buffer_text` is back to sub-second-ish (Phase B had it at ~7.3s; strip-at-fill should drop it by ~7s). Compare against the recorded Phase-B number. This is the deliverable — quote the before (7293ms) and the after.

- [ ] **Step 5: Leak + iterator safety still clean.**

- Reuse the Phase-B `ITALIC_TAGCOUNT` probe approach (or just reload LoJ 2-3× and grep for unbounded tag growth) — tag count stable across reloads.
- `rg -i "invalid.*iterator|text_buffer|Gtk-CRITICAL" <stderr>` — only the 2 pre-existing startup criticals; no buffer-iterator critical (trivially true now — zero buffer deletes).

- [ ] **Step 6: Cleanup** — `pkill -f "cage -- ./target/debug/linux-lit"`. Tree clean (revert any probe).

- [ ] **Step 7: Real-GL handoff** — give the user the command to confirm italic render + snappier LoJ load on the real GL renderer.

---

## Post-implementation

- Finish per convention: merge `--no-ff` to master from the MAIN checkout, re-verify build+tests, push, remove worktree, delete branch.
- **Update `ac`:** the LoJ load-time item is done (strip-at-fill); note the before/after ms.
- **Follow-ups (own cycles, unchanged):** Phase C (verse karaoke, data-gated on litdb phrase-timestamp backfill); the fidelity eval vs `~/Downloads/pg8918-images.html` (after Phase C).

## Self-Review

- **Spec coverage:** strip helper → Task 1; field + both fill branches strip-at-fill + offset-map-at-fill → Task 2; tag-only pass → Task 3; render + load-time + leak/iterator headless → Task 4. All covered.
- **Placeholders:** Task 2 Step 3's "confirm the 4th clear precedes this code" and Task 3's clone-vs-scoped-borrow are confirm-on-read, not TODOs. Task 2 Step 4 documents the deliberate intermediate state (italics briefly not rendering between T2 and T3).
- **Type consistency:** `ItalicStripResult{stripped_lines, line_spans, line_removed}`, `strip_italics_for_fill(&[String]) -> ItalicStripResult`, `italic_line_spans: HashMap<usize, Vec<(usize,usize)>>`, `italic_offset_map: HashMap<usize, Vec<usize>>`, `apply_inline_italics(&mut AppState)` — all read from current master (d316321d) during planning.
