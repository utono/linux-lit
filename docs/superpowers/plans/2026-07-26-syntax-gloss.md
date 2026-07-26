# syntax-gloss Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Cairo syntax diagram with `syntax-gloss`, a sixth `gloss_type` stored as prose and rendered by the existing gloss overlay.

**Architecture:** `syntax-gloss` follows `reader-gloss` exactly — build a `GlossContext`, check for a saved gloss, otherwise call Claude and persist through `persist_render_install_gloss`. The prompt returns block markup (`<segment>`/`<gloss>`) instead of JSON, so no new renderer exists. The Cairo surface, its keybinds legend, its `InputMode`, and the band/POS geometry are deleted.

**Tech Stack:** Rust, GTK4, SQLite (rusqlite), the existing `crate::gloss` request pipeline.

Spec: `docs/superpowers/specs/2026-07-26-syntax-gloss-design.md`

## Global Constraints

- Work in a worktree off master: `git worktree add ~/utono/linux-lit-wt/feat-syntax-gloss -b feat/syntax-gloss`. Merge back from the MAIN checkout.
- Master is at `445570e9`. Baselines to hold: **1163 tests passing**, **clippy 181 warnings**. Do not exceed clippy; test count changes as tests are added and deleted (see each task's expected number).
- Verify with `cargo build`; do NOT run `cargo run`. The user runs the app.
- `gloss_type` is a free-text column — **no schema migration, no litdb change.**
- `src/db/syntax.rs` (`load_line_syntax`) is **KEPT**: the `line_syntax` enrichment still feeds the prompt.
- Per-word POS tags are **dropped entirely** — the prompt stops requesting them. The POS legend and the queued `PUNCT` removal are moot.
- The stored gloss body uses ONLY the existing markup vocabulary: `<segment>`, `<gloss>`, `<speaker>`, `<pron>`. No new tags — a new tag would need a new renderer, which this spec exists to avoid.
- Deletion is real: ~1,315 lines across five files, all merged to master earlier today. This is intended.

---

## File Structure

**Modify:**
- `src/gloss.rs` — add `syntax_gloss_prompt()` beside `reader_gloss_prompt()`; add the structure-section builder + its tests.
- `src/input/visual.rs` — `action_syntax_diagram` becomes `action_syntax_gloss`, modelled on `action_reader_gloss` (same file, ~40 lines above it).
- `src/input/actions/pickers.rs:17-40` — `GlossPickerFilter` gains a `SyntaxGloss` variant.
- `src/input/actions/overlay_cycle.rs` — syntax-gloss joins the `\` rotation.
- `src/input/keymap.rs`, `src/input/keymap_config.rs`, `src/input/actions/mod.rs` — drop the diagram action/mode/bind, keep the `Return` underline entry pointing at the new handler.
- `src/app/mod.rs`, `src/main.rs`, `src/ui/mod.rs` — drop the overlay field/registration.
- `src/ui/keybinds_overlay.rs` — drop the `n` toggle-note entry.

**Delete:**
- `src/ui/syntax_overlay.rs` (1,091 lines), `src/ui/syntax_keybinds_overlay.rs` (15), `src/syntax_diagram.rs` (209), `src/input/actions/syntax.rs` (284).

**Keep:** `src/db/syntax.rs` (272 lines).

---

## Task 1: The structure-section builder

The one piece of real logic: bands plus text in, indented markup out. Pure — no GTK, no DB — so it carries the tests.

**Files:**
- Modify: `src/gloss.rs` (append near the other prompt helpers)
- Test: inline `#[cfg(test)] mod tests` in `src/gloss.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub struct SyntaxBand { pub start_char: usize, pub end_char: usize, pub label: String, pub depth: u8 }`
  - `pub fn structure_section(text: &str, bands: &[SyntaxBand]) -> String`

  Returns one line per band: `depth × 2` spaces of indent, the label, ` — `, then the band's own words with the middle elided when long. Empty string when `bands` is empty.

- [ ] **Step 1: Write the failing tests**

Append to the `#[cfg(test)] mod tests` block in `src/gloss.rs`:

```rust
    fn sb(start: usize, end: usize, depth: u8, label: &str) -> SyntaxBand {
        SyntaxBand { start_char: start, end_char: end, label: label.to_string(), depth }
    }

    #[test]
    fn structure_indents_by_depth() {
        let text = "Alpha beta gamma delta.";
        let bands = vec![
            sb(0, 23, 0, "main clause"),
            sb(6, 15, 1, "subject"),
        ];
        let out = structure_section(text, &bands);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "main clause — Alpha beta gamma delta.");
        assert_eq!(lines[1], "  subject — beta gamma");
    }

    #[test]
    fn structure_elides_the_middle_of_a_long_span() {
        // 100+ chars: the quote keeps the head and tail so the reader can
        // locate the span, and drops the middle so a row stays one line.
        let text = "one two three four five six seven eight nine ten eleven twelve \
thirteen fourteen fifteen sixteen seventeen eighteen.";
        let bands = vec![sb(0, text.chars().count(), 0, "main clause")];
        let out = structure_section(text, &bands);
        assert!(out.contains('…'), "long span must elide: {out}");
        assert!(out.starts_with("main clause — one two"), "keeps the head: {out}");
        assert!(out.trim_end().ends_with("eighteen."), "keeps the tail: {out}");
        assert!(out.lines().count() == 1, "stays one line: {out}");
    }

    #[test]
    fn structure_is_empty_when_there_are_no_bands() {
        assert_eq!(structure_section("Anything at all.", &[]), "");
    }

    #[test]
    fn structure_clamps_an_out_of_range_span() {
        // A stale or malformed span must not panic mid-render.
        let text = "Short.";
        let bands = vec![sb(3, 900, 0, "predicate")];
        let out = structure_section(text, &bands);
        assert!(out.starts_with("predicate — "), "{out}");
    }

    #[test]
    fn structure_handles_multibyte_text_by_chars() {
        let text = "Æsop wrote it. Naïve reader—café.";
        let bands = vec![sb(15, 33, 0, "second sentence")];
        let out = structure_section(text, &bands);
        assert!(out.contains("Naïve"), "char offsets, not bytes: {out}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd ~/utono/linux-lit-wt/feat-syntax-gloss && cargo test --bins gloss::tests::structure 2>&1 | tail -15`

Expected: FAIL to compile — `cannot find function structure_section in this scope`.

- [ ] **Step 3: Implement**

Add to `src/gloss.rs`, above the `#[cfg(test)]` block:

```rust
/// One band of a syntax gloss: a span of the passage and what it grammatically
/// IS. `depth` is nesting depth, 0 = outermost.
#[derive(Debug, Clone)]
pub struct SyntaxBand {
    pub start_char: usize,
    pub end_char: usize,
    pub label: String,
    pub depth: u8,
}

/// Longest quoted span before the middle is elided, in characters.
const SPAN_QUOTE_MAX: usize = 64;

/// Render the Structure section of a syntax gloss: one line per band, indented
/// by nesting depth, each quoting the words that band covers.
///
/// Quoting the span rather than pointing at a position is the whole reason
/// this replaces the Cairo drawing — nothing has to align, so a line wrap is
/// irrelevant and there is no geometry to get wrong.
///
/// Out-of-range spans are clamped, not rejected: a stale offset should degrade
/// to a short quote, never panic in the middle of rendering a gloss.
pub fn structure_section(text: &str, bands: &[SyntaxBand]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    for b in bands {
        let start = b.start_char.min(chars.len());
        let end = b.end_char.min(chars.len()).max(start);
        let span: String = chars[start..end].iter().collect();
        let span = span.split_whitespace().collect::<Vec<_>>().join(" ");

        let quoted = if span.chars().count() > SPAN_QUOTE_MAX {
            let head: String = span.chars().take(SPAN_QUOTE_MAX / 2).collect();
            let tail: String = span
                .chars()
                .skip(span.chars().count() - SPAN_QUOTE_MAX / 2)
                .collect();
            format!("{}…{}", head.trim_end(), tail.trim_start())
        } else {
            span
        };

        for _ in 0..b.depth {
            out.push_str("  ");
        }
        out.push_str(&b.label);
        out.push_str(" — ");
        out.push_str(&quoted);
        out.push('\n');
    }
    out
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bins gloss::tests::structure 2>&1 | tail -8`

Expected: PASS — 5 passed.

- [ ] **Step 5: Verify build and clippy**

Run: `cargo build 2>&1 | rg -c '^error'; cargo clippy 2>&1 | rg -c '^warning'`

Expected: 0 errors; clippy `181`.

- [ ] **Step 6: Commit**

```bash
git add src/gloss.rs
git commit -m "feat(gloss): structure-section builder for syntax glosses"
```

---

## Task 2: The syntax-gloss prompt

**Files:**
- Modify: `src/gloss.rs`
- Test: inline tests in `src/gloss.rs`

**Interfaces:**
- Consumes: `structure_section` from Task 1 (referenced in the prompt's own instructions, not called here).
- Produces: `pub fn syntax_gloss_prompt() -> &'static str`

- [ ] **Step 1: Write the failing test**

`reader_gloss_prompt` has sibling tests at `src/gloss.rs:1331` and `:1347` — put this beside them:

```rust
    #[test]
    fn syntax_gloss_prompt_asks_for_markup_not_json() {
        let p = syntax_gloss_prompt();
        assert!(!p.is_empty());
        // Prose markup, not the JSON the Cairo diagram used.
        assert!(p.contains("<segment>"), "must ask for segment markup");
        assert!(p.contains("<gloss>"), "must ask for gloss markup");
        assert!(!p.contains("\"bands\""), "must NOT ask for the old JSON schema");
        // The three body sections the spec requires.
        assert!(p.contains("Structure"), "must ask for the structure section");
        assert!(p.contains("Terms"), "must ask for the terms section");
        // POS tags are dropped entirely.
        assert!(!p.to_lowercase().contains("part-of-speech"), "POS tags are dropped");
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --bins syntax_gloss_prompt 2>&1 | tail -8`

Expected: FAIL to compile — `cannot find function syntax_gloss_prompt`.

- [ ] **Step 3: Implement**

Add to `src/gloss.rs` beside `reader_gloss_prompt`:

```rust
/// System prompt for a `syntax-gloss`: the passage's grammatical structure as
/// prose, in the block markup the gloss overlay already renders.
///
/// Returns markup rather than JSON because a syntax gloss is stored and drawn
/// like every other gloss type. Per-word POS tags are deliberately absent —
/// they existed to fill the old Cairo tag row, and in prose they are noise.
pub fn syntax_gloss_prompt() -> &'static str {
    "\
You analyze the grammatical structure of a passage of literature and return \
prose, formatted with the markup described below. Return ONLY that markup — no \
commentary outside it, no JSON, no markdown fences.

Emit exactly three sections, in this order.

1. The passage itself, wrapped in a <segment>...</segment> pair.

2. A line reading `Structure:` followed by one line per grammatical span. Each \
line is: two spaces of indent per level of nesting, then what the span IS, \
then ` — `, then the span's own words. Nesting means containment: a span \
indented under another is inside it. Use terms a reader would meet in a \
grammar — main clause, relative clause, appositive, subject, predicate, \
conjoined predicate, participial modifier, adverbial phrase. Quote the span's \
actual words; if a span runs longer than about sixty characters, keep its \
first and last words and put … between them.

3. A line reading `What the structure is doing:` followed by two or three \
sentences, wrapped in a <gloss>...</gloss> pair, on what the structure \
achieves rhetorically — what the arrangement does that a plainer one would \
not.

4. A line reading `Terms:` followed by one <gloss>...</gloss> pair per \
DISTINCT term you used in the Structure section, in the order they first \
appear. Each reads `term: definition.` and defines the term GENERALLY — what a \
relative clause is in any sentence — not what this particular span does. If \
you used the same term three times, define it once.

Do not list parts of speech for individual words.

Where a dependency parse is supplied, anchor your analysis on it. Where it is \
absent, analyze the text directly — the parse is an enrichment, never a \
requirement. The passage is early modern or nineteenth-century English; \
analyze the grammar as written, not as it would be phrased today."
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --bins gloss:: 2>&1 | rg 'test result'`

Expected: PASS, 6 more than the pre-Task-1 gloss count.

- [ ] **Step 5: Commit**

```bash
git add src/gloss.rs
git commit -m "feat(gloss): syntax-gloss prompt returning markup, not JSON"
```

---

## Task 3: The syntax-gloss action

Rewires the existing entry points to produce a saved gloss. Model this on `action_reader_gloss` in the SAME file (`src/input/visual.rs:586-699`) — read it first; this is a parallel of it, not a new pattern.

**Files:**
- Modify: `src/input/visual.rs:557-584` (replace `action_syntax_diagram`)
- Modify: `src/input/actions/syntax.rs` — reduce to the underline entry point only, or fold into `visual.rs` and delete (implementer's call; state which in the report)

**Interfaces:**
- Consumes: `syntax_gloss_prompt()` from Task 2; `crate::db::syntax::load_line_syntax` (unchanged); `crate::gloss::build_context_for_type`, `call_claude_with_prompt`, `crate::input::actions::gloss::persist_render_install_gloss`.
- Produces: `pub(crate) fn action_syntax_gloss(state_rc: &Rc<RefCell<AppState>>)`.

- [ ] **Step 1: Read the template**

Read `src/input/visual.rs:586-699` (`action_reader_gloss`) in full before writing anything. The new function differs in exactly four ways: the type string is `"syntax-gloss"`, the prompt is `syntax_gloss_prompt()`, the user message appends the `line_syntax` token table when the work has one, and there are no neighbor glosses.

- [ ] **Step 2: Replace the action**

Replace `action_syntax_diagram` (`src/input/visual.rs:557`) with:

```rust
/// Visual-mode "Syntax": build (or reuse) a `syntax-gloss` for the selection.
///
/// A parallel of `action_reader_gloss` below — same context build, same
/// cache-then-fetch shape, same persist path. Differences: the gloss type, the
/// prompt, the `line_syntax` enrichment appended to the user message, and no
/// neighbor glosses (a grammatical analysis does not need what the adjacent
/// passages were told).
pub(crate) fn action_syntax_gloss(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let (ctx, model, tokio_handle, all_glosses, passage_doc, parse_table) = {
        let state = state_rc.borrow();
        let (start, end) = match &state.visual_selection {
            Some(s) => s.range(),
            None => return,
        };
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        let selected_lines: Vec<crate::db::models::Line> = (start..=end)
            .filter_map(|buf_line| {
                state.work_line_for_buffer(buf_line)
                    .and_then(|wi| work.lines.get(wi).cloned())
            })
            .collect();

        let ctx = match crate::gloss::build_context_for_type(work, &selected_lines, "syntax-gloss") {
            Some(c) => c,
            None => return,
        };

        // `line_syntax` enrichment: sent where the work has a parse, omitted
        // where it does not. 5 of 306 works are parsed, so the text-only path
        // is the common one — it is a first-class path, not a fallback.
        let line_ids: Vec<i64> = selected_lines.iter().map(|l| l.id).collect();
        let all_glosses: Vec<crate::db::queries::SavedGloss>;
        let parse_table: String;
        match crate::db::queries::open_db() {
            Ok(conn) => {
                all_glosses = crate::db::queries::find_glosses_by_start(
                    &conn, &ctx.work_abbrev, &ctx.start_citation, &["syntax-gloss"],
                ).unwrap_or_default();
                let toks = crate::db::syntax::load_line_syntax(&conn, &line_ids);
                crate::logging::log(&format!(
                    "SYNTAX-GLOSS: {} parsed tokens for {} lines", toks.len(), line_ids.len()
                ));
                parse_table = crate::db::syntax::tokens_as_table(&toks);
            }
            Err(_) => {
                all_glosses = Vec::new();
                parse_table = String::new();
            }
        }

        let passage_doc = crate::input::actions::echoes::build_source_header(&selected_lines, &ctx.speaker);
        (ctx, state.config.claude_model.clone(), state.tokio_handle.clone(), all_glosses, passage_doc, parse_table)
    };

    exit_visual_mode(&mut state_rc.borrow_mut());

    // Cache hit: show the saved gloss, no API call.
    if let Some(idx) = all_glosses.iter().position(|g| g.gloss_type == "syntax-gloss") {
        let mut s = state_rc.borrow_mut();
        s.gloss_original_text = Some(ctx.source_text.clone());
        let pairs = ctx.source_line_pairs();
        let gloss_text = &all_glosses[idx].gloss_text;
        let card_width = s.content_hbox.width();
        let card_height = crate::app::layout::overlay_card_height(&s);
        let head = crate::app::scene_synopsis::synopsis_head(&s, ctx.act, ctx.scene);
        s.gloss_overlay.show_gloss_with_color(&ctx.source_text, gloss_text, card_width, card_height, Some(&s.theme.root_color), &pairs, (&head.0, &head.1));
        s.gloss_overlay.set_position(idx, all_glosses.len());
        s.gloss_overlay.set_citation(&ctx.start_citation, &ctx.end_citation);
        s.gloss_list = all_glosses;
        s.gloss_index = idx;
        s.gloss_context = Some(ctx);
        s.record_last_gloss("syntax-gloss");
        s.input_mode = crate::app::InputMode::GlossOverlay;
        crate::logging::log("SYNTAX-GLOSS: showing cached gloss");
        return;
    }

    {
        let mut s = state_rc.borrow_mut();
        s.gloss_original_text = Some(ctx.source_text.clone());
        let cw = s.content_hbox.width();
        let h = crate::app::layout::overlay_card_height(&s);
        s.gloss_overlay.show_glossing(&passage_doc, cw, h, Some(&s.theme.root_color));
        s.input_mode = crate::app::InputMode::GlossOverlay;
    }

    let mut user_msg = crate::gloss::build_user_message(&ctx, None, None, &[]);
    if !parse_table.is_empty() {
        user_msg.push_str("\n\nDependency parse for these lines:\n");
        user_msg.push_str(&parse_table);
    }
    let state_for_result = std::rc::Rc::clone(state_rc);

    glib::spawn_future_local(async move {
        let model_for_db = model.clone();
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(crate::gloss::syntax_gloss_prompt(), &user_msg, &model).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
                let mut s = state_for_result.borrow_mut();
                crate::input::actions::gloss::persist_render_install_gloss(
                    &mut s, ctx, &gloss_text, "syntax-gloss", &model_for_db,
                    "SYNTAX-GLOSS: generated and saved new gloss",
                );
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.gloss_overlay.show(&format!("Error: {}", e), "");
                crate::logging::log(&format!("SYNTAX-GLOSS: API error: {}", e));
            }
            Err(e) => {
                let s = state_for_result.borrow();
                s.gloss_overlay.show("Internal error \u{2014} try again.", "");
                crate::logging::log(&format!("SYNTAX-GLOSS: tokio join error: {}", e));
            }
        }
    });
}
```

Update the `BUILTIN_ACTIONS` dispatch at `src/input/visual.rs:281` (index 6) to call `action_syntax_gloss`. The array entry stays `"Syntax"`. **That array and the `match` are coupled POSITIONALLY** — the file says so; changing one without the other fires the wrong action.

- [ ] **Step 3: Point the underline entry at the new action**

`OpenSyntaxDiagramForUnderlined` in `src/input/keymap.rs:4493` calls `open_syntax_diagram_for_underlined`. That function (in `src/input/actions/syntax.rs`) resolves the underlined words to a sentence span, then opens the diagram. Keep the sentence resolution; change its terminal call to build a `syntax-gloss` instead. The sentence-span logic in `src/input/sentence.rs` is untouched.

- [ ] **Step 4: Verify build**

Run: `cargo build 2>&1 | rg '^error' -A5 | head -20`

Expected: errors ONLY from the not-yet-deleted Cairo surface (Task 4 removes it). If any error names `visual.rs` or `gloss.rs`, fix it here.

- [ ] **Step 5: Commit**

```bash
git add src/input/visual.rs src/input/actions/syntax.rs src/input/keymap.rs
git commit -m "feat(syntax-gloss): build and persist a gloss instead of drawing"
```

---

## Task 4: Delete the Cairo surface

**Files:**
- Delete: `src/ui/syntax_overlay.rs`, `src/ui/syntax_keybinds_overlay.rs`, `src/syntax_diagram.rs`
- Modify: `src/ui/mod.rs`, `src/main.rs`, `src/app/mod.rs`, `src/input/keymap.rs`, `src/input/keymap_config.rs`, `src/input/actions/mod.rs`, `src/ui/keybinds_overlay.rs`

- [ ] **Step 1: Delete the files**

```bash
git rm src/ui/syntax_overlay.rs src/ui/syntax_keybinds_overlay.rs src/syntax_diagram.rs
```

- [ ] **Step 2: Remove every reference**

Run `cargo build 2>&1 | rg '^error' -A4` and work through it. The references are in: `src/ui/mod.rs` (two `pub mod` lines), `src/main.rs` (one `mod syntax_diagram;`), `src/app/mod.rs` (the `syntax_overlay` / `syntax_keybinds_overlay` fields, their constructor entries, and `syntax_return_mode`), `src/input/keymap.rs` (`handle_syntax_diagram_key`, its dispatch arm, the `InputMode::SyntaxDiagram` / `SyntaxKeybindsOverlay` arms), `src/input/keymap_config.rs` (nothing to remove — `Return` stays bound), `src/input/actions/mod.rs` (`InputMode` variants if declared there).

Also remove from `src/ui/keybinds_overlay.rs`: the `RETURN_KEY` cap, its row-2 chain entry, the `"diagram sentence"` describe arm and short-label entry. The `Return` BIND stays — it now opens a syntax gloss — so update that describe text rather than deleting it:

```rust
        "diagram sentence" => "Action::OpenSyntaxDiagramForUnderlined (Return; \
opens a syntax gloss for the sentence containing the words underlined by -/_. \
Does nothing when no words are underlined) — src/input/visual.rs",
```

- [ ] **Step 3: Verify the build is clean**

Run: `cargo build 2>&1 | rg -c '^error'`

Expected: `0`.

- [ ] **Step 4: Run tests and clippy**

Run: `cargo test --bins 2>&1 | rg 'test result'; cargo clippy 2>&1 | rg -c '^warning'`

Expected: tests pass; the count DROPS by the ~20 tests that lived in the deleted files, then rises by Task 1's 5 and Task 2's 1. Report the actual number. Clippy must be ≤ 181 — deleting code should lower it.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(syntax): delete the Cairo diagram surface"
```

---

## Task 5: Picker and overlay-cycle parity

**Files:**
- Modify: `src/input/actions/pickers.rs:17-40`
- Modify: `src/input/actions/overlay_cycle.rs`

**Interfaces:**
- Consumes: the `"syntax-gloss"` type string from Task 3.
- Produces: `GlossPickerFilter::SyntaxGloss`.

- [ ] **Step 1: Write the failing test**

Add to `src/input/actions/pickers.rs`'s test module (create one if absent):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_filter_cycles_through_every_type_including_syntax() {
        // Alt+t must reach syntax-gloss, and the cycle must return to its
        // start — a filter that cannot be cycled back to is unreachable.
        let start = GlossPickerFilter::default();
        let mut seen = vec![start.gloss_type()];
        let mut f = start.next();
        while f != start {
            seen.push(f.gloss_type());
            f = f.next();
        }
        assert!(seen.contains(&"syntax-gloss"), "cycle must reach it: {seen:?}");
        assert!(seen.contains(&"reader-gloss"));
        assert!(seen.contains(&"teacher-generic"));
        assert!(seen.contains(&"inner-monologue"));
        assert_eq!(seen.len(), 4, "one entry per type, no duplicates: {seen:?}");
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --bins picker_filter_cycles 2>&1 | tail -8`

Expected: FAIL — the assertion on `"syntax-gloss"`, or a compile error on the missing variant.

- [ ] **Step 3: Add the variant**

In `src/input/actions/pickers.rs`:

```rust
pub(crate) enum GlossPickerFilter {
    TeacherGeneric,
    InnerMonologue,
    SyntaxGloss,
    #[default]
    ReaderGloss,
}

impl GlossPickerFilter {
    pub(crate) fn gloss_type(self) -> &'static str {
        match self {
            GlossPickerFilter::TeacherGeneric => "teacher-generic",
            GlossPickerFilter::InnerMonologue => "inner-monologue",
            GlossPickerFilter::SyntaxGloss => "syntax-gloss",
            GlossPickerFilter::ReaderGloss => "reader-gloss",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            GlossPickerFilter::TeacherGeneric => GlossPickerFilter::InnerMonologue,
            GlossPickerFilter::InnerMonologue => GlossPickerFilter::SyntaxGloss,
            GlossPickerFilter::SyntaxGloss => GlossPickerFilter::ReaderGloss,
            GlossPickerFilter::ReaderGloss => GlossPickerFilter::TeacherGeneric,
        }
    }
}
```

- [ ] **Step 4: Add it to the `\` cycle**

Read `src/input/actions/overlay_cycle.rs` — the rotation is journal Q&A → gloss → synopsis, one `cycle_from_*` function per surface. Insert syntax-gloss after gloss, following the exact shape of the neighbouring functions (each hides the current overlay, restores the anchor position, then opens the next). State in the report which functions you changed.

- [ ] **Step 5: Verify**

Run: `cargo test --bins 2>&1 | rg 'test result'; cargo clippy 2>&1 | rg -c '^warning'`

Expected: pass; clippy ≤ 181.

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/pickers.rs src/input/actions/overlay_cycle.rs
git commit -m "feat(syntax-gloss): picker filter and overlay-cycle parity"
```

---

## Task 6: On-screen verification

Mandatory per CLAUDE.md. Cage disagreed with the real GL renderer on every layout defect this feature hit, so a cage pass is necessary but not sufficient.

**Files:**
- Modify: `docs/superpowers/plans/2026-07-26-syntax-gloss.md` (record results)

- [ ] **Step 1: Build and launch headless**

```bash
cd ~/utono/linux-lit-wt/feat-syntax-gloss && cargo build
./scripts/land-on.sh BH-Barrett 3.0
```

BH-Barrett uses `div2=0`, so `3.0` is valid and `1.1` is not. Take the printed `XDG_RUNTIME_DIR` from the output — do not assume it. Launch via the harness `run_in_background`; a detached or `timeout`-wrapped launch dies immediately.

- [ ] **Step 2: Drive a syntax gloss**

```bash
export XDG_RUNTIME_DIR=<printed>  WAYLAND_DISPLAY=wayland-0
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
sleep 2
wtype -k Escape          # re-send after resize; the first chord is dropped
for i in $(seq 1 12); do wtype -k j; sleep 0.25; done
wtype -k minus
sleep 1
wtype -k Return
sleep 30
grim -o HEADLESS-1 /tmp/sg-1.png
```

- [ ] **Step 3: Open the PNG and report what you see**

Per the UI review protocol, a passing exit code is not enough. Read the capture and confirm:

1. The gloss overlay opens (NOT a Cairo diagram — that code is gone).
2. All three sections render: the passage, `Structure:` with indented rows, `What the structure is doing:`, `Terms:`.
3. Structure rows quote real words from the passage and indent by nesting.
4. A repeated term appears ONCE under Terms.

Confirm the save happened:

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT gloss_type, substr(gloss_text,1,80) FROM glosses WHERE gloss_type='syntax-gloss' ORDER BY id DESC LIMIT 1;"
```

Note: `land-on.sh` uses a PRIVATE DB copy at `/tmp/land-on-lit.db`, so query THAT file, not the live lit.db.

- [ ] **Step 4: Verify the cache path**

Press Escape, then repeat the same selection and `Return`. The log must show `SYNTAX-GLOSS: showing cached gloss` and NO new API call — that is the persistence win this whole change is for.

- [ ] **Step 5: Verify the picker**

Open the gloss picker (Alt+g) and cycle with Alt+t until the placeholder reads `Filter syntax-gloss glosses...`. Confirm the saved gloss is listed.

- [ ] **Step 6: Clean up**

Run as its own step — `pkill` exits nonzero on no match and aborts an `&&` chain:

```bash
pkill -f "cage -- target/debug/linux-lit" || true
```

- [ ] **Step 7: Record results and commit**

Append a "## Verification results" section to this plan with what each check showed, then commit it.

- [ ] **Step 8: Hand off for real-renderer confirmation**

```bash
cd ~/utono/linux-lit-wt/feat-syntax-gloss && cargo run
```

Give the user the four criteria from Step 3. Do NOT merge before they confirm.

---

## Task 7: Documentation

**Files:**
- Modify: `CLAUDE.md`, `docs/troubleshooting/clip-prevention.md`, `docs/guides/keybind-consistency-guide.md`

- [ ] **Step 1: Update CLAUDE.md**

The Key Files list names `src/ui/syntax_overlay.rs`; remove that line. Add `syntax-gloss` to the gloss-type list wherever the file enumerates them.

- [ ] **Step 2: Close out clip-prevention entry 15**

Entry 15 documents the Cairo diagram's layout defects. Append:

```markdown
    **RETIRED (2026-07-26).** The surface this entry describes no longer
    exists. After four rounds of layout fixes in one day — each exposing the
    next, with cage passing layouts the real GL renderer rejected — the Cairo
    diagram was replaced by `syntax-gloss`, a prose gloss type rendered by the
    existing overlay (spec:
    `docs/superpowers/specs/2026-07-26-syntax-gloss-design.md`). The entry is
    kept because its LESSONS generalize to any annotation layer drawn over
    text: derive offsets from measured content rather than constants, never
    anchor two stacked elements at the same origin, and test the real renderer
    before believing a headless pass. The specific fix history is now
    archaeology.
```

- [ ] **Step 3: Update the keybind consistency guide**

The 2026-07-26 entry describes `-`/`_` + `Return` opening a syntax diagram. Append a line noting the target is now a syntax gloss; the binds themselves are unchanged.

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md docs/troubleshooting/clip-prevention.md docs/guides/keybind-consistency-guide.md
git commit -m "docs: retire the Cairo diagram, record syntax-gloss"
```

---

## Self-Review

**Spec coverage.** Storage (`gloss_type`, no migration) → Task 3. The three body sections → Tasks 1 and 2. Prompt rewrite and the dropped POS tags → Task 2. Entry points and full picker/cycle parity → Tasks 3 and 5. Deletions → Task 4. `src/db/syntax.rs` kept → Task 3 uses it. Error handling inherits the gloss paths → Task 3's match arms. Testing → Tasks 1, 5, 6.

**Placeholder scan.** No TBDs. Two steps direct the implementer to read existing code before writing (Task 3 Step 1, Task 5 Step 4) — those are genuine read-then-mirror instructions with the file and line range named and the required outcome stated, not vagueness. Task 3 Step 2 leaves one judgment call explicit (fold `actions/syntax.rs` in or reduce it) and requires the choice be reported.

**Type consistency.** `SyntaxBand { start_char, end_char, label, depth }` and `structure_section(&str, &[SyntaxBand]) -> String` are defined in Task 1; Task 2's prompt describes the same output shape in prose. `syntax_gloss_prompt() -> &'static str` is defined in Task 2 and called in Task 3. `"syntax-gloss"` is the type string in Tasks 3 and 5 — one spelling throughout. `GlossPickerFilter::SyntaxGloss` is defined and tested in Task 5 only.

**Known risk.** Task 4 deletes ~1,315 lines merged to master earlier today. The build is the safety net: the compiler names every dangling reference. The riskier part is Task 3's positional coupling in `BUILTIN_ACTIONS` — flagged in the step, because a silent off-by-one there fires the wrong visual-mode action rather than failing to compile.
