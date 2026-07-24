# Inline italics (Phase B) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render prose `_word_` markup as inline italic spans — paired underscore delimiters hidden, the enclosed run styled `pango::Style::Italic` — for prose/prose_book/epic_translation works. A work title `_London_` renders as italic *London*, no underscores.

**Architecture:** Follow the in-tree `apply_bcp_formatting` `^...^` idiom: per line, parse paired `_..._` (pure helper), delete the delimiters from the live buffer highest-offset-first (re-fetching iterators after every mutation), and tag the shifted spans italic. Record per stripped line the source offsets of removed `_` in a new `AppState.italic_offset_map`. The one buffer consumer that indexes by source-relative char offset — `phrase_highlight::apply_char_range_tag` (karaoke) — consults that map via a pure `translate_offset` so its DB spans land correctly on stripped lines. Every other consumer (search, vocab, word-copy, pagination, LineMap) already re-derives from the live buffer and needs no change.

**Tech Stack:** Rust, GTK4 (gtk4-rs, `pango::Style::Italic`), cargo test (bin crate), cage/grim e2e.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-24-inline-italics-phase-b-design.md`.
- **Scope:** `prose`/`prose_book`/`epic_translation` works only. **Plays (Ibsen) EXCLUDED** — gate on `work_type` (their `_` is in `[stage direction]` lines already whole-line-italicized). Non-italic lines and excluded works must be BYTE-IDENTICAL to today.
- **Bin crate:** `cargo test --bin linux-lit <name>` (NOT `--lib`; fall back to plain `cargo test <name>`). Also `cargo build --bin linux-lit`, `cargo clippy --bin linux-lit`.
- Do NOT run the app (`cargo run`) — user launches it. Headless via cage/grim (CLAUDE.md "Headless Verification").
- **Odd `_` count on a line → render VERBATIM (no strip, no italic) + LOG.** Never italicize-to-end-of-line on a stray `_`. The 61 LoJ odd-`_` rows stay visible.
- The mechanism MIRRORS `apply_bcp_formatting`'s `^...^` handler (`src/app/formatting.rs`, span loop ~452-495): delete highest-offset-first, RE-FETCH `iter_at_line` after EVERY `buffer.delete` (a delete invalidates all outstanding iterators → GTK critical otherwise).
- `italic_offset_map` lifecycle MIRRORS `block_indent_tiers` (declared `mod.rs:860`, init `2405`, cleared/set at every `rebuild_buffer_text` return path `4366/4427/4444/4451`): cleared to empty on every path, set only on the italic pass. Never leaks across works.
- Branch per convention (worktree off master); commit on the feature branch; merge `--no-ff` from the main checkout. Do NOT stash/checkout/restore user files.

## File Structure

- `src/app/text_prep.rs` (or a new `src/app/italics.rs`) — `parse_italic_spans` pure helper + `translate_offset` pure helper + tests (Tasks 1, 2).
- `src/app/mod.rs` — `AppState.italic_offset_map` field + lifecycle at the buffer-fill paths (Task 3).
- `src/app/formatting.rs` — `apply_inline_italics` pass (delete-and-tag, mirrors `^...^`) + wiring (Task 4).
- `src/input/phrase_highlight.rs` — `apply_char_range_tag` consults `translate_offset` (Task 5).
- Headless acceptance (Task 6) — no production code.

---

## Task 1: `parse_italic_spans` pure helper

**Files:**
- Create: `src/app/italics.rs` (new module; add `mod italics;` to `src/app/mod.rs`)
- Test: same file's `#[cfg(test)] mod tests`

**Interfaces:**
- Produces:
  ```rust
  pub struct ItalicParse {
      pub stripped_text: String,        // line with paired `_` removed
      pub spans: Vec<(usize, usize)>,   // italic char ranges in stripped_text (start, end)
      pub removed_positions: Vec<usize>,// SOURCE char offsets of removed `_`, sorted ascending
  }
  /// None when: no `_`, OR odd `_` count (unpaired — caller renders verbatim + logs).
  pub fn parse_italic_spans(line: &str) -> Option<ItalicParse>;
  ```
- Pairing: non-greedy, left-to-right — a `_` opens, the next `_` closes (rule `_([^_]+)_`, but implement by scanning `_` positions and pairing consecutively). Even count → pairs; odd count → `None`.
- Char offsets are UNICODE CHAR indices (GTK `set_line_offset` uses char offsets), NOT bytes.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Option<ItalicParse> { parse_italic_spans(s) }

    #[test]
    fn no_underscore_is_none() {
        assert!(p("plain roman text").is_none());
    }

    #[test]
    fn single_pair_strips_and_spans() {
        let r = p("he wrote _London_ later").unwrap();
        assert_eq!(r.stripped_text, "he wrote London later");
        // "he wrote " = 9 chars; "London" = chars 9..15
        assert_eq!(r.spans, vec![(9, 15)]);
        // `_` removed at source offsets 9 and 16 ("he wrote _" -> _ at 9; close after London at 16)
        assert_eq!(r.removed_positions, vec![9, 16]);
    }

    #[test]
    fn two_adjacent_pairs_are_two_spans_not_one_run() {
        // _A_, _B_  -> italic A and italic B, comma+space roman between
        let r = p("_A_, _B_").unwrap();
        assert_eq!(r.stripped_text, "A, B");
        assert_eq!(r.spans, vec![(0, 1), (3, 4)]);
        assert_eq!(r.removed_positions, vec![0, 2, 5, 7]);
    }

    #[test]
    fn word_internal_weld_italicizes_inner() {
        // John_son_ -> "Johnson" with "son" italic (pairing the two `_`)
        let r = p("John_son_").unwrap();
        assert_eq!(r.stripped_text, "Johnson");
        assert_eq!(r.spans, vec![(4, 7)]);       // "son" at chars 4..7 of "Johnson"
        assert_eq!(r.removed_positions, vec![4, 8]);
    }

    #[test]
    fn currency_measure_italic_letter() {
        // 120_l_.  -> "120l." with "l" italic
        let r = p("120_l_.").unwrap();
        assert_eq!(r.stripped_text, "120l.");
        assert_eq!(r.spans, vec![(3, 4)]);
        assert_eq!(r.removed_positions, vec![3, 5]);
    }

    #[test]
    fn odd_count_is_none_verbatim() {
        assert!(p("a stray _ underscore").is_none());          // 1 `_`
        assert!(p("_open but _no close_ here _").is_none());    // 4? -> count; if odd -> None
    }

    #[test]
    fn multibyte_before_span_offsets_are_char_not_byte() {
        // "café _x_" — é is 2 bytes but 1 char; span must be char-indexed
        let r = p("café _x_").unwrap();
        assert_eq!(r.stripped_text, "café x");
        assert_eq!(r.spans, vec![(5, 6)]);   // "café " = 5 chars; x at 5..6
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin linux-lit parse_italic` — FAIL (module/fn not found).

- [ ] **Step 3: Implement `parse_italic_spans`**

```rust
//! Inline `_word_` italic parsing (Phase B). Pure, buffer-agnostic.

pub struct ItalicParse {
    pub stripped_text: String,
    pub spans: Vec<(usize, usize)>,
    pub removed_positions: Vec<usize>,
}

/// Parse paired `_..._` runs in a line. `None` when there is no `_` or an ODD
/// number of `_` (unpaired — the caller renders the line verbatim and logs it,
/// so a stray `_` never italicizes to end-of-line). Offsets are UNICODE CHAR
/// indices. Non-greedy left-to-right pairing: `_` opens, next `_` closes.
pub fn parse_italic_spans(line: &str) -> Option<ItalicParse> {
    // char-index positions of every `_`
    let underscores: Vec<usize> = line
        .chars()
        .enumerate()
        .filter(|(_, c)| *c == '_')
        .map(|(i, _)| i)
        .collect();
    if underscores.is_empty() || underscores.len() % 2 != 0 {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    let mut stripped: Vec<char> = Vec::with_capacity(chars.len() - underscores.len());
    let mut spans = Vec::new();
    let removed_positions = underscores.clone(); // already sorted ascending

    // Walk source chars; drop `_` at paired positions; record span bounds in the
    // STRIPPED coordinate space. Pairs are (underscores[2k], underscores[2k+1]).
    let mut pair_iter = underscores.chunks_exact(2);
    let mut next_pair = pair_iter.next();
    let mut span_open_display: Option<usize> = None;
    for (src_i, &c) in chars.iter().enumerate() {
        if let Some(&[open, close]) = next_pair {
            if src_i == open {
                // opening delimiter: drop it; the span begins at the current
                // stripped length.
                span_open_display = Some(stripped.len());
                continue;
            }
            if src_i == close {
                // closing delimiter: drop it; close the span.
                if let Some(start) = span_open_display.take() {
                    spans.push((start, stripped.len()));
                }
                next_pair = pair_iter.next();
                continue;
            }
        }
        stripped.push(c);
    }
    Some(ItalicParse {
        stripped_text: stripped.into_iter().collect(),
        spans,
        removed_positions,
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin linux-lit parse_italic` — PASS (all cases). If a span/offset assertion fails, fix the IMPLEMENTATION (the tests encode the spec); re-derive the expected values only if you can prove the test's arithmetic wrong.

- [ ] **Step 5: Commit**

```bash
git add src/app/italics.rs src/app/mod.rs
git commit -m "feat(reader): parse_italic_spans — pure _word_ parser (strip + spans + removed positions)"
```

---

## Task 2: `translate_offset` pure helper

**Files:**
- Modify: `src/app/italics.rs` (add fn + tests)

**Interfaces:**
- Produces:
  ```rust
  /// Translate a SOURCE char offset to the DISPLAY (stripped) offset by
  /// subtracting the number of removed `_` at-or-before it. Identity when
  /// `removed` is empty (non-italic line → zero cost, no shift).
  pub fn translate_offset(removed: &[usize], source_offset: usize) -> usize;
  ```
- `removed` is sorted ascending (as `ItalicParse.removed_positions` is).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn translate_identity_when_empty() {
    assert_eq!(translate_offset(&[], 0), 0);
    assert_eq!(translate_offset(&[], 42), 42);
}

#[test]
fn translate_subtracts_removed_before_offset() {
    // removed `_` at source 9 and 16 (the _London_ case)
    let removed = vec![9usize, 16];
    assert_eq!(translate_offset(&removed, 5), 5);    // before both -> unchanged
    assert_eq!(translate_offset(&removed, 10), 9);   // 1 removed (<=10) -> -1
    assert_eq!(translate_offset(&removed, 20), 18);  // 2 removed (<=20) -> -2
}

#[test]
fn translate_offset_exactly_at_removed_position_counts_it() {
    // a `_` exactly AT the offset is at-or-before -> counted
    assert_eq!(translate_offset(&[9, 16], 9), 8);   // one removed <= 9
    assert_eq!(translate_offset(&[9, 16], 16), 14); // two removed <= 16
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test --bin linux-lit translate_offset` — FAIL (fn not found).

- [ ] **Step 3: Implement**

```rust
pub fn translate_offset(removed: &[usize], source_offset: usize) -> usize {
    // `removed` is sorted ascending; count entries <= source_offset.
    let n = removed.partition_point(|&p| p <= source_offset);
    source_offset - n
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --bin linux-lit translate_offset` — PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/italics.rs
git commit -m "feat(reader): translate_offset — source->display char offset for italic lines"
```

---

## Task 3: `AppState.italic_offset_map` field + lifecycle

**Files:**
- Modify: `src/app/mod.rs` (field decl, init, clear at buffer-fill return paths)

**Interfaces:**
- Produces: `state.italic_offset_map: std::collections::HashMap<usize, Vec<usize>>` — keyed by BUFFER LINE index, value = that line's `removed_positions`. Only italic lines have an entry; absent = empty = identity. Populated by Task 4's pass; consumed by Task 5.

- [ ] **Step 1: Declare the field**

In `src/app/mod.rs`, next to `block_indent_tiers` (~line 860):

```rust
    /// Per buffer-line source→display offset data for inline-italic lines
    /// (Phase B): buffer line index -> sorted source char offsets of removed
    /// `_` delimiters. Only lines that had paired `_` stripped have an entry;
    /// absent = no shift (identity). Consumed by phrase_highlight::apply_char_range_tag
    /// to keep karaoke spans correct on italic lines. Rebuilt on every
    /// rebuild_buffer_text; never leaks across works.
    pub italic_offset_map: std::collections::HashMap<usize, Vec<usize>>,
```

- [ ] **Step 2: Initialize it**

In the AppState constructor (~line 2405, next to `block_indent_tiers: Vec::new(),`):

```rust
        italic_offset_map: std::collections::HashMap::new(),
```

- [ ] **Step 3: Clear it on every buffer-fill return path**

In `rebuild_buffer_text`, mirror the `block_indent_tiers = Vec::new();` sites — add `state.italic_offset_map.clear();` at EACH path that currently clears `block_indent_tiers` (~4366, ~4427, ~4451) and at the block-aware branch (~4444, after it sets tiers). Task 4's italic pass (called from these paths, prose branch) REPOPULATES it. The invariant: after `rebuild_buffer_text` returns, `italic_offset_map` reflects ONLY the current work's italic lines (empty for non-italic/excluded works).

- [ ] **Step 4: Build**

Run: `cargo build --bin linux-lit` — compiles. `cargo test --bin linux-lit` — suite green (no behavior change yet; map is populated in Task 4).

- [ ] **Step 5: Commit**

```bash
git add src/app/mod.rs
git commit -m "feat(reader): AppState.italic_offset_map field + rebuild lifecycle"
```

---

## Task 4: `apply_inline_italics` pass (strip + tag)

**Files:**
- Modify: `src/app/formatting.rs` (new pass, mirrors the `^...^` handler ~452-495)
- Modify: `src/app/mod.rs` (call it in `rebuild_buffer_text` for prose/epic works; populate `italic_offset_map`)

**Interfaces:**
- Consumes: `parse_italic_spans` (Task 1), `state.buffer`, `work.work_type`.
- Produces: `pub fn apply_inline_italics(state: &mut AppState)` — for each buffer line of a prose/epic_translation work, parse `_`; on `Some`, delete the paired `_` from the buffer (highest-offset-first, re-fetch iters after each delete) and apply an `italic` tag to each span; record the line's `removed_positions` into `state.italic_offset_map`. On `None` (no `_` or odd count), leave the line verbatim; on odd count, LOG it. No-op for excluded work types.

- [ ] **Step 1: Create the italic tag + gate**

In `src/app/formatting.rs`, add the pass. Create/ensure a `pango::Style::Italic` tag once (idempotent lookup-then-create, like `ensure_block_typography_tags`):

```rust
/// Inline `_word_` italics for prose/epic_translation works (Phase B). Mirrors
/// the apply_bcp_formatting `^...^` delete-and-retag idiom: per line, delete the
/// paired `_` from the buffer highest-offset-first (re-fetching iterators after
/// every mutation — a delete invalidates outstanding iters → GTK critical), tag
/// each stripped span italic, and record removed `_` source offsets in
/// state.italic_offset_map for karaoke offset translation.
pub fn apply_inline_italics(state: &mut AppState) {
    // Gate: prose / prose_book / epic_translation only. Plays excluded.
    let wt = state.current_work.as_ref().map(|w| w.work_type.clone()).unwrap_or_default();
    if !matches!(wt.as_str(), "prose" | "prose_book" | "epic_translation") {
        return;
    }
    // Ensure the italic tag exists.
    let tag_table = state.buffer.tag_table();
    if tag_table.lookup("inline-italic").is_none() {
        let t = gtk4::TextTag::builder().name("inline-italic").style(pango::Style::Italic).build();
        tag_table.add(&t);
    }

    let line_count = state.buffer.line_count() as usize;
    for i in 0..line_count {
        let Some(line_start) = state.buffer.iter_at_line(i as i32) else { continue };
        let mut line_end = line_start;
        if !line_end.ends_line() { line_end.forward_to_line_end(); }
        let text = state.buffer.text(&line_start, &line_end, false).to_string();

        // Fast path: no `_` -> nothing to do.
        if !text.contains('_') { continue; }

        match crate::app::italics::parse_italic_spans(&text) {
            None => {
                // odd `_` count (unpaired) OR (no `_` — excluded above): if the
                // line has `_` but did not parse, it is an unbalanced-underscore
                // data defect. Render verbatim (do nothing) + LOG.
                if text.contains('_') {
                    log_fmt!("ITALIC_UNPAIRED: line {} odd `_` count, rendered literal: {:?}",
                        i, text.chars().take(60).collect::<String>());
                }
            }
            Some(parse) => {
                // Delete paired `_` highest-offset-first so earlier offsets stay
                // valid; re-fetch line-start after EVERY delete.
                for &pos in parse.removed_positions.iter().rev() {
                    let mut d = state.buffer.iter_at_line(i as i32).unwrap();
                    d.forward_chars(pos as i32);
                    let mut d_end = d;
                    d_end.forward_char();
                    state.buffer.delete(&mut d, &mut d_end);
                }
                // Tag each span (offsets are DISPLAY-relative in the now-stripped
                // line). Re-fetch the line base each time.
                for &(sc, ec) in &parse.spans {
                    let base = state.buffer.iter_at_line(i as i32).unwrap();
                    let mut a = base;
                    a.forward_chars(sc as i32);
                    let mut b = base;
                    b.forward_chars(ec as i32);
                    state.buffer.apply_tag_by_name("inline-italic", &a, &b);
                }
                state.italic_offset_map.insert(i, parse.removed_positions);
            }
        }
    }
}
```

(Confirm `pango` and `log_fmt!` are already imported in `formatting.rs`; the `^...^` handler uses both, so they are.)

- [ ] **Step 2: Wire the call — at the END of `rebuild_buffer_text` (ordering CONFIRMED during planning)**

Call `apply_inline_italics(state)` at the END of `rebuild_buffer_text`, on the prose/generic path (NOT the BCP branch — BCP has its own `^...^` handling), AFTER the buffer text is set. Mirror how `apply_block_typography` is already called from the block-aware branch of `rebuild_buffer_text` — the italic pass is the same shape (a sub-line tag pass that runs inside `rebuild_buffer_text`, before `display_work`'s later passes).

**Why the END of `rebuild_buffer_text`, definitively** (the `display_work` sequence was traced during planning, `src/app/mod.rs` ~3864-3918):

```
display_work:
  rebuild_buffer_text(state)          <- buffer text set HERE; add apply_inline_italics at its end
  apply_dialogue_formatting(state)    <- whole-line tags (no char-offset dependency on `_`)
  apply_authorship_formatting(state)
  build_vocab_matches(state)          <- tokenizes state.buffer.text() -> MUST see the STRIPPED buffer
  apply_vocab_highlighting(state)
  apply_reader_gloss_highlighting(state)
```

`build_vocab_matches` re-derives from the live buffer text. Because `apply_inline_italics` runs INSIDE `rebuild_buffer_text` (step 1), the `_` are already gone before vocab tokenizes (step 4) → vocab sees `London`, matches `London`, offsets correct. This is the required order and it holds by construction — do NOT call `apply_inline_italics` from `display_work` AFTER `build_vocab_matches`, which would shift vocab. `apply_dialogue_formatting`/`apply_authorship_formatting` are whole-line/line-index passes with no `_`-relative char offset, so running before them is fine too.

Confirm on read that `rebuild_buffer_text`'s prose path is where prose/prose_book/epic_translation land (Phase A added the block-aware branch; prose without block rows falls through to the generic `set_text` path — the italic pass goes there, after the text is set). State the exact insertion line in the commit message.

- [ ] **Step 3: Build + regression**

Run: `cargo build --bin linux-lit && cargo test --bin linux-lit` — compiles, suite green. `cargo clippy --bin linux-lit` — no new warnings. No unit test here (GTK buffer mutation; acceptance is Task 6). 

- [ ] **Step 4: Commit**

```bash
git add src/app/formatting.rs src/app/mod.rs
git commit -m "feat(reader): apply_inline_italics — hide _ delimiters + tag spans italic"
```

---

## Task 5: karaoke consults `translate_offset`

**Files:**
- Modify: `src/input/phrase_highlight.rs` (`apply_char_range_tag`, ~775-803)

**Interfaces:**
- Consumes: `state.italic_offset_map` (Task 3/4), `translate_offset` (Task 2).
- Produces: `apply_char_range_tag` translates its SOURCE-relative `start_char`/`end_char` through the line's `removed_positions` (from the map; absent → empty → identity) BEFORE clamping/applying. On non-italic lines and excluded works (empty map) the behavior is byte-identical.

- [ ] **Step 1: Apply the translation**

In `apply_char_range_tag` (`src/input/phrase_highlight.rs:775`), BEFORE the `let sc = start_char.min(line_chars);` line, translate the incoming source offsets:

```rust
    // Inline italics (Phase B) may have removed `_` from THIS buffer line, so the
    // DB span (source-relative) must shift to the stripped display offset. Empty
    // map entry (non-italic line, or a work without italics) = identity.
    let (start_char, end_char) = match s.italic_offset_map.get(&bl) {
        Some(removed) => (
            crate::app::italics::translate_offset(removed, start_char),
            crate::app::italics::translate_offset(removed, end_char),
        ),
        None => (start_char, end_char),
    };
```

(Then the existing `let sc = start_char.min(line_chars);` etc. run on the translated values.)

- [ ] **Step 2: Build + regression**

Run: `cargo build --bin linux-lit && cargo test --bin linux-lit && cargo clippy --bin linux-lit`
Expected: green, no new warnings. Guard: for any line with no map entry (every non-italic line, every excluded/non-LoJ work, all current LoJ since LoJ has 0 phrase_timestamps anyway), the `None` arm makes this byte-identical to before.

- [ ] **Step 3: Commit**

```bash
git add src/input/phrase_highlight.rs
git commit -m "feat(reader): karaoke apply_char_range_tag translates source offsets on italic lines"
```

---

## Task 6: Headless on-screen acceptance (non-optional gate)

**Files:** none — cage/grim against live data.

**Interfaces:** the visible-surface gate. A green build is NOT acceptance.

- [ ] **Step 1: Build** — `cd <worktree> && cargo build` (clean).

- [ ] **Step 2: Italic render on LoJ**

Launch headless (CLAUDE.md: `LIT_NO_MPV=1 GSK_RENDERER=cairo LIT_DEV=1`, cage via harness `run_in_background`, fresh `XDG_RUNTIME_DIR`, prefer `scripts/land-on.sh LoJ 1.0`, resize 1920x1200, re-send first post-resize chord). Land on a LoJ passage with `_`-marked titles (the reference #8918 is full of them — e.g. search `/London` or a page with a work title). Screenshot.

- [ ] **Step 3: Verify — open the PNG, pixel/glyph-inspect, report inline**

- A `_word_`-marked run renders as ITALIC (slanted glyphs), NO underscores visible. Quote the on-screen text and name the italic word.
- Surrounding roman text is unaffected (not italic).
- No clipping (clip-prevention ledger).
- An odd-`_` line (if landable) renders VERBATIM with literal underscores (the render-literal rule) — and the log has an `ITALIC_UNPAIRED` line for it.

- [ ] **Step 4: Consumer regressions on an italic line**

- **Search:** search for the italic word's text (e.g. `London`) → it matches and highlights the displayed (underscore-free) word.
- **Vocab:** if a vocab word coincides with/near an italic span, its highlight lands correctly (this is the ordering check from Task 4 Step 2 — vocab must tokenize the stripped buffer).
- **word-copy:** copy the italic word → clipboard has `London`, not `_London_`.

- [ ] **Step 5: Karaoke reconciliation on PP (the offset-translate proof)**

Switch to **PP (Pickwick Papers)** — 244 italic rows + 87,857 phrase_timestamps. Land on a PP page where an italic word sits on a phrase-timed line, start playback/seek, and confirm the karaoke sweep lands on the CORRECT characters after `_` removal (not shifted left/right by the removed delimiters). This is the Task-5 translate_offset proof. If MPV isn't available headless, drive a seek and confirm the tint char-range via the log / a screenshot of the tint position.

- [ ] **Step 6: Regression + cleanup**

- A prose work with NO `_` (or a page with none) screenshots IDENTICAL to pre-change.
- A play (e.g. Ham) renders unchanged (excluded from the italic pass — confirm its `[stage direction]` `_` still shows whatever it showed before, not newly-processed).
- `pkill -f "cage -- ./target/debug/linux-lit"` (EXACTLY this).

- [ ] **Step 7: Real-GL handoff** — give the user the command to eyeball the italic rendering on the real GL renderer.

---

## Post-implementation

- Finish per convention: merge `--no-ff` to master from the MAIN checkout, re-verify build+tests, push, remove worktree, delete branch.
- **Follow-ups (own cycles):** Phase C (verse karaoke line-by-line + `block_buffer_range` deletion + the Phase-A empty-row limitations; data-gated on litdb `backfill-phrase-timestamps` for LoJ). LoJ load-time fix. Fidelity eval vs `~/Downloads/pg8918-images.html` (after C).
- **Limitation to record:** LoJ karaoke-over-italics not on-screen-verified (0 phrase_timestamps) — the reconciliation is proven on PP; LoJ's own proof lands with its Phase-C backfill.

## Self-Review

- **Spec coverage:** parse (odd→None, non-greedy, weld, currency) → Task 1; translate_offset (identity on empty) → Task 2; offset-map field/lifecycle → Task 3; strip+tag pass (gate on work_type, `^...^` idiom, log odd) → Task 4; karaoke consults translate → Task 5; headless (italic render, consumer regressions, PP karaoke proof, play/no-`_` regression) → Task 6. All covered.
- **Placeholders:** Task 4 Step 2's italic-vs-vocab ordering is a confirm-on-read (with the concrete fix if wrong) — a verification gate, not a TODO.
- **Type consistency:** `ItalicParse{stripped_text, spans, removed_positions}`, `parse_italic_spans -> Option<ItalicParse>`, `translate_offset(&[usize], usize) -> usize`, `italic_offset_map: HashMap<usize, Vec<usize>>`, `apply_inline_italics(&mut AppState)`, `apply_char_range_tag(s, tag, bl, start_char, end_char)` — all read from current master during planning.
