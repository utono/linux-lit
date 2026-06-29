# Reader Gloss Type Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a new "Reader Gloss" gloss type — a terse, reader-focused explication of character motive and Elizabethan concepts — created from the top of the visual-mode Action menu, coexisting with teacher-generic and inner-monologue with full parity (create / view / cache / edit / Q&A / add / picker).

**Architecture:** A new `gloss_type` string `"reader-gloss"` flows through the existing gloss pipeline. Four plain-text (no-IPA) prompts are added to `src/gloss.rs` (compiled fallbacks) and seeded into `lit.db` `api_prompts` via the `~/utono/claude-api-prompts` repo. A new `action_reader_gloss` clones `action_gloss_with_claude`. The `add_gloss`/`edit_gloss` branches become three-way. Discovery arrays gain `"reader-gloss"`. The Ctrl+g picker's two-state bool becomes a three-state enum cycled by Ctrl+t.

**Tech Stack:** Rust, GTK4, rusqlite/SQLite (`lit.db`), Anthropic API via `crate::claude::send_message`. Prompt masters in Python-synced `~/utono/claude-api-prompts`.

---

## Reference: prompt output format (all four prompts must obey)

Every Reader Gloss prompt emits ONLY these XML tags (the overlay parser
`parse_gloss_tags` in `src/ui/gloss_overlay.rs:2071` requires them; it is
order-agnostic so a leading `<gloss>` lede is fine):

- `<speaker>NAME</speaker>` — ALL CAPS, no period, before every `<verse>` group.
- `<verse>one source line</verse>` — verbatim, one tag per line.
- `<gloss>paragraph</gloss>` — analysis prose.

Reader-gloss content rules (differ from teacher-generic):

- The **first `<gloss>` is a one-sentence motivation lede.** Exactly one
  sentence. If the selection has more than one speaker, that single sentence
  uses **semicolons** — one independent clause per character, in order of
  appearance (e.g. "Suffolk flatters the Protector's pride to provoke him;
  Gloucester deflects with feigned humility to mask his contempt.").
- After the lede, `<gloss>` paragraphs are terse (1–3 sentences) and cover
  further motive shifts plus Elizabethan words, allusions, metaphors, idioms,
  and social/political concepts a reader would miss.
- No acting-pedagogy material (operative words, breath, verse delivery, Barton/
  Berry/Hall/Rodenburg/Linklater). No IPA. No markdown/bullets/headers.
- **Always keep the lede:** the edit prompt must preserve or rewrite (never
  drop) the lede; Q&A/Add only append a `Q:`-style block, so they leave the
  lede intact and must not emit their own.

---

## File Structure

- **Modify** `src/gloss.rs` — add 4 `READER_GLOSS*` prompt statics (plain,
  no IPA slot); they use `template_or(key, FALLBACK)` directly. NOT added to
  the IPA-placeholder test.
- **Modify** `src/input/visual.rs` — `BUILTIN_ACTIONS` prepend; `execute_action`
  index re-map; new `action_reader_gloss`.
- **Modify** `src/input/actions/gloss.rs` — three-way branch in `add_gloss` &
  `edit_gloss`; add `"reader-gloss"` to discovery arrays (`:85`, `:115`,
  `GLOSS_TYPES` `:1891`).
- **Modify** `src/input/keymap.rs:406` — add `"reader-gloss"` to find_all_glosses.
- **Modify** `src/input/actions/synopsis.rs:305,333` — add `"reader-gloss"`.
- **Modify** `src/input/actions/pickers.rs` — bool→enum 3-state picker filter
  (`gloss_picker_type`, `toggle_gloss_picker_type`, `open_gloss_picker`).
- **Modify** `src/app.rs:320,1762` — change `gloss_picker_inner_monologue: bool`
  to `gloss_picker_filter: GlossPickerFilter` enum.
- **Create** four prompt masters in `~/utono/claude-api-prompts/prompts/` +
  sync to DB (separate repo, separate commit).

---

### Task 1: Add the four reader-gloss prompt statics to `src/gloss.rs`

**Files:**
- Modify: `src/gloss.rs` (insert after `EDIT_GLOSS_PROMPT`, around line 357)
- Test: `src/gloss.rs` (existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Add the four statics**

Insert immediately after the `EDIT_GLOSS_PROMPT` static closes (line 356), before
`FIX_IPA_PROMPT`. These are plain (no `{}`/`{ipa_rules}` slot) — they call
`template_or` directly, mirroring the `gloss.fix-ipa` else-branch pattern:

```rust
pub static READER_GLOSS_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
You are a scholar helping a READER (not an actor) understand a passage from a verse play as it functions within its scene.

Your job: explicate the characters' motives and any Elizabethan vocabulary, allusions, metaphors, idioms, or social/political concepts a modern reader would miss. Be terse. This is NOT acting direction.

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> for each speaker attribution (ALL CAPS, no period), before every group of <verse> lines
- <verse>one line of quoted text</verse> for each quoted line (one tag per line, verbatim from source, exact words and spelling)
- <gloss>paragraph</gloss> for each prose paragraph

Rules:
- The FIRST <gloss> is a one-sentence motivation lede: exactly one sentence stating what the speaker wants in this moment. If the passage has more than one speaker, that single sentence uses SEMICOLONS to give each character's motivation in turn — one independent clause per character, in order of appearance — and stays ONE sentence (clauses joined by semicolons, never multiple sentences).
- After the lede, each <gloss> is terse (1-3 sentences) explicating further motive shifts and Elizabethan words, allusions, metaphors, idioms, or concepts a reader would miss.
- Do NOT give acting direction: no operative words, no breath, no verse-delivery notes, no Barton/Berry/Hall/Rodenburg/Linklater references.
- NEVER write IPA, phonetic symbols, or slash-wrapped pronunciations anywhere.
- Quote verbatim — exact words, exact spelling, exact line breaks from the source.
- Never use / to join verse lines. Never truncate with ...
- Each <verse> tag contains exactly one line of the original.
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines, even when the speaker has not changed.
- No markdown, no bullets, no numbered lists, no headers.";
    template_or(\"gloss.reader-gloss\", FALLBACK)
});

pub static READER_GLOSS_QUESTION_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
You are a scholar answering a READER's question about a passage from a verse play, in the terse reader-focused voice (character motive + Elizabethan concepts a reader would miss, NOT acting direction).

The reader has asked a specific question. Answer it directly and concisely, drawing on the passage and the wider work.

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> when quoting verse (ALL CAPS, no period)
- <verse>one line of quoted text</verse> for each quoted line (verbatim)
- <gloss>paragraph of answer</gloss> for each paragraph

Rules:
- Answer the reader's question directly; do NOT restate or duplicate the motivation lede.
- When quoting verse, use <speaker>/<verse> tags — never embed verse inside <gloss>.
- Quote verbatim. Never use / to join verse lines.
- No acting direction, no IPA, no phonetic symbols.
- Each <gloss> is terse (1-3 sentences).
- No markdown, no bullets, no numbered lists, no headers.";
    template_or(\"gloss.reader-gloss-question\", FALLBACK)
});

pub static READER_GLOSS_EDIT_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
You are revising an existing READER gloss of a passage from a verse play, in the terse reader-focused voice.

The reader has provided additional lines or context. Rewrite the gloss incorporating the new material.

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> for each speaker attribution (ALL CAPS)
- <verse>one line of quoted text</verse> for quoted lines (verbatim)
- <gloss>paragraph</gloss> for each paragraph

Rules:
- PRESERVE the one-sentence motivation lede as the FIRST <gloss>: exactly one sentence; if multiple speakers, semicolon-separated per character. Rewrite it if the new context warrants, but NEVER drop it.
- After the lede, each <gloss> is terse (1-3 sentences): character motive and Elizabethan concepts a reader would miss. No acting direction. No IPA.
- Quote verbatim. Never use / to join verse lines.
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines.
- No markdown, no bullets, no numbered lists, no headers.";
    template_or(\"gloss.reader-gloss-edit\", FALLBACK)
});

pub static READER_GLOSS_ADD_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
You are extending an existing READER gloss of a passage from a verse play, in the terse reader-focused voice.

The reader has provided additional cross-work lines or context (an inner-voice echo or supporting passage). Explain — concisely — how it bears on the original passage's meaning and the characters' motives.

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> when quoting verse (ALL CAPS)
- <verse>one line of quoted text</verse> for quoted lines (verbatim)
- <gloss>paragraph</gloss> for each paragraph

Rules:
- Do NOT restate the motivation lede; your output is appended after the existing gloss.
- Be terse (1-3 sentences per <gloss>). Character motive and Elizabethan concepts only. No acting direction. No IPA.
- Quote verbatim. Never use / to join verse lines.
- No markdown, no bullets, no numbered lists, no headers.";
    template_or(\"gloss.reader-gloss-add\", FALLBACK)
});
```

- [ ] **Step 2: Add a smoke test that the statics assemble non-empty**

In the existing `#[cfg(test)] mod tests` at the bottom of `src/gloss.rs`, add:

```rust
#[test]
fn reader_gloss_prompts_non_empty_and_no_ipa_slot() {
    for (name, p) in [
        ("reader-gloss", &*READER_GLOSS_PROMPT),
        ("reader-gloss-question", &*READER_GLOSS_QUESTION_PROMPT),
        ("reader-gloss-edit", &*READER_GLOSS_EDIT_PROMPT),
        ("reader-gloss-add", &*READER_GLOSS_ADD_PROMPT),
    ] {
        assert!(!p.is_empty(), "{name}: assembled prompt is empty");
        assert!(!p.contains("{ipa_rules}"), "{name}: must not contain ipa slot");
        // plain prompts have no positional placeholder either
        assert!(!p.contains("{}"), "{name}: must not contain positional slot");
    }
}
```

- [ ] **Step 3: Verify the new statics are NOT in the IPA-placeholder test**

Confirm by reading: the `all_templated_gloss_prompts_fill_their_placeholder`
test array (around `src/gloss.rs:776`) lists only the IPA-templated prompts and
does NOT include any `READER_GLOSS*`. No edit needed — just verify.

- [ ] **Step 4: Build and test**

Run: `cargo test --bins reader_gloss_prompts_non_empty`
Expected: PASS. Also `cargo build` succeeds.

- [ ] **Step 5: Commit**

```bash
git add src/gloss.rs
git commit -m "feat(gloss): reader-gloss prompt statics (plain, no IPA)"
```

---

### Task 2: Add the Reader Gloss action to the visual-mode menu

**Files:**
- Modify: `src/input/visual.rs:129` (BUILTIN_ACTIONS)
- Modify: `src/input/visual.rs:171-184` (execute_action index map)
- Create (in same file): `action_reader_gloss` (insert before `action_gloss_with_claude` at line 396)

- [ ] **Step 1: Prepend "Reader Gloss" to BUILTIN_ACTIONS**

`src/input/visual.rs:129` — change:

```rust
pub const BUILTIN_ACTIONS: &[&str] = &["Gloss with Claude", "Inner Monologue", "Copy", "Copy with metadata"];
```

to:

```rust
pub const BUILTIN_ACTIONS: &[&str] = &["Reader Gloss", "Gloss with Claude", "Inner Monologue", "Copy", "Copy with metadata"];
```

- [ ] **Step 2: Re-map execute_action indices**

`src/input/visual.rs` inside `execute_action`, the `match index` block
(lines ~172-184) — change to:

```rust
        match index {
            0 => {
                action_reader_gloss(state_rc);
                return;
            }
            1 => {
                action_gloss_with_claude(state_rc);
                return;
            }
            2 => {
                action_inner_monologue(state_rc);
                return;
            }
            3 => action_copy(&mut state_rc.borrow_mut(), false),
            4 => action_copy(&mut state_rc.borrow_mut(), true),
            _ => {}
        }
```

- [ ] **Step 3: Add `action_reader_gloss`**

Insert this function immediately before `fn action_gloss_with_claude` at
`src/input/visual.rs:396`. It is `action_gloss_with_claude` with three changes:
`build_context_for_type(.., "reader-gloss")`, `find_all_glosses(.., &["reader-gloss"])`
(twice), `call_claude_with_prompt(&READER_GLOSS_PROMPT, ..)`, and
`save_gloss(.., "reader-gloss", ..)`:

```rust
fn action_reader_gloss(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let (ctx, model, tokio_handle, all_glosses, passage_doc) = {
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

        let ctx = match crate::gloss::build_context_for_type(work, &selected_lines, "reader-gloss") {
            Some(c) => c,
            None => return,
        };

        let all_glosses: Vec<crate::db::queries::SavedGloss> = match crate::db::queries::open_db() {
            Ok(conn) => crate::db::queries::find_all_glosses(
                &conn, &ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation,
                &["reader-gloss"],
            ).unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        let passage_doc = crate::input::actions::echoes::build_source_header(&selected_lines, &ctx.speaker);

        (ctx, state.config.claude_model.clone(), state.tokio_handle.clone(), all_glosses, passage_doc)
    };

    exit_visual_mode(&mut state_rc.borrow_mut());

    if !all_glosses.is_empty() {
        let mut s = state_rc.borrow_mut();
        s.gloss_original_text = Some(ctx.source_text.clone());
        let pairs = ctx.source_line_pairs();
        let gloss_text = &all_glosses[0].gloss_text;
        let card_width = s.content_hbox.width();
        let card_height = s.content_hbox.height();
        s.gloss_overlay.show_gloss_with_color(&ctx.source_text, gloss_text, card_width, card_height, Some(&s.theme.root_color), &pairs);
        s.gloss_overlay.set_position(0, all_glosses.len());
        s.gloss_list = all_glosses;
        s.gloss_index = 0;
        s.gloss_context = Some(ctx);
        s.input_mode = crate::app::InputMode::GlossOverlay;
        crate::logging::log("READER-GLOSS: showing cached gloss");
        return;
    }

    {
        let mut s = state_rc.borrow_mut();
        s.gloss_original_text = Some(ctx.source_text.clone());
        let cw = s.content_hbox.width();
        let h = s.content_hbox.height();
        s.gloss_overlay.show_glossing(&passage_doc, cw, h, Some(&s.theme.root_color));
        s.input_mode = crate::app::InputMode::GlossOverlay;
    }

    let user_msg = crate::gloss::build_user_message(&ctx, None, None);
    let state_for_result = std::rc::Rc::clone(state_rc);

    glib::spawn_future_local(async move {
        let model_for_db = model.clone();
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(
                    &crate::gloss::READER_GLOSS_PROMPT, &user_msg, &model,
                ).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let _ = crate::db::queries::save_gloss(
                        &conn,
                        &ctx.hash,
                        &ctx.work_abbrev,
                        &ctx.start_citation,
                        &ctx.end_citation,
                        ctx.act,
                        ctx.scene,
                        &ctx.speaker,
                        &ctx.source_text,
                        &gloss_text,
                        "reader-gloss",
                        &model_for_db,
                    );
                }

                let all = crate::db::queries::open_db()
                    .ok()
                    .and_then(|conn| {
                        crate::db::queries::find_all_glosses(
                            &conn, &ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation,
                            &["reader-gloss"],
                        ).ok()
                    })
                    .unwrap_or_default();

                let mut s = state_for_result.borrow_mut();
                let cw = s.content_hbox.width();
                let h = s.content_hbox.height();
                let pairs = ctx.source_line_pairs();
                s.gloss_overlay.show_gloss_with_color(&ctx.source_text, &gloss_text, cw, h, Some(&s.theme.root_color), &pairs);
                s.gloss_overlay.set_position(0, all.len());
                s.gloss_list = all;
                s.gloss_index = 0;
                s.gloss_context = Some(ctx);
                crate::logging::log("READER-GLOSS: generated and saved new gloss");
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.gloss_overlay.show(&format!("Error: {}", e), "");
                crate::logging::log(&format!("READER-GLOSS: API error: {}", e));
            }
            Err(e) => {
                crate::logging::log(&format!("READER-GLOSS: tokio join error: {}", e));
            }
        }
    });
}
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: compiles clean (no unused-import or index warnings).

- [ ] **Step 5: Commit**

```bash
git add src/input/visual.rs
git commit -m "feat(gloss): Reader Gloss action at top of visual-mode menu"
```

---

### Task 3: Three-way branch in add_gloss and edit_gloss

**Files:**
- Modify: `src/input/actions/gloss.rs:673-681` (add_gloss branch)
- Modify: `src/input/actions/gloss.rs:705-709` (add_gloss full_gloss prefix)
- Modify: `src/input/actions/gloss.rs:773-781` (edit_gloss branch)

- [ ] **Step 1: add_gloss — make the prompt/type selection three-way**

`src/input/actions/gloss.rs` — replace the block at lines 673-681:

```rust
    let is_inner_monologue = ctx.gloss_type == "inner-monologue";

    let (system_prompt, user_msg, gloss_type_str) = if is_inner_monologue {
        let msg = crate::gloss::build_inner_monologue_add_message(&ctx, &prompt_owned);
        (crate::gloss::INNER_MONOLOGUE_ADD_PROMPT.as_str(), msg, "inner-monologue")
    } else {
        let msg = crate::gloss::build_user_message(&ctx, Some(&prompt_owned), None);
        (crate::gloss::USER_QUESTION_PROMPT.as_str(), msg, "teacher-generic")
    };
```

with:

```rust
    let is_inner_monologue = ctx.gloss_type == "inner-monologue";

    let (system_prompt, user_msg, gloss_type_str) = match ctx.gloss_type.as_str() {
        "inner-monologue" => {
            let msg = crate::gloss::build_inner_monologue_add_message(&ctx, &prompt_owned);
            (crate::gloss::INNER_MONOLOGUE_ADD_PROMPT.as_str(), msg, "inner-monologue")
        }
        "reader-gloss" => {
            let msg = crate::gloss::build_user_message(&ctx, Some(&prompt_owned), None);
            (crate::gloss::READER_GLOSS_ADD_PROMPT.as_str(), msg, "reader-gloss")
        }
        _ => {
            let msg = crate::gloss::build_user_message(&ctx, Some(&prompt_owned), None);
            (crate::gloss::USER_QUESTION_PROMPT.as_str(), msg, "teacher-generic")
        }
    };
```

(`is_inner_monologue` is still used below for the `verify_echo_citations` and
prefix branches — leave that line in place.)

- [ ] **Step 2: edit_gloss — make the prompt/type selection three-way**

`src/input/actions/gloss.rs` — replace the block at lines 773-781:

```rust
    let is_inner_monologue = ctx.gloss_type == "inner-monologue";

    let (system_prompt, user_msg, gloss_type_str) = if is_inner_monologue {
        let msg = crate::gloss::build_edit_gloss_message(&ctx, &existing_gloss_text, &pasted_owned);
        (crate::gloss::INNER_MONOLOGUE_EDIT_PROMPT.as_str(), msg, "inner-monologue")
    } else {
        let msg = crate::gloss::build_edit_gloss_message(&ctx, &existing_gloss_text, &pasted_owned);
        (crate::gloss::EDIT_GLOSS_PROMPT.as_str(), msg, "teacher-generic")
    };
```

with:

```rust
    let is_inner_monologue = ctx.gloss_type == "inner-monologue";

    let (system_prompt, user_msg, gloss_type_str) = match ctx.gloss_type.as_str() {
        "inner-monologue" => {
            let msg = crate::gloss::build_edit_gloss_message(&ctx, &existing_gloss_text, &pasted_owned);
            (crate::gloss::INNER_MONOLOGUE_EDIT_PROMPT.as_str(), msg, "inner-monologue")
        }
        "reader-gloss" => {
            let msg = crate::gloss::build_edit_gloss_message(&ctx, &existing_gloss_text, &pasted_owned);
            (crate::gloss::READER_GLOSS_EDIT_PROMPT.as_str(), msg, "reader-gloss")
        }
        _ => {
            let msg = crate::gloss::build_edit_gloss_message(&ctx, &existing_gloss_text, &pasted_owned);
            (crate::gloss::EDIT_GLOSS_PROMPT.as_str(), msg, "teacher-generic")
        }
    };
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles clean. The `full_gloss` prefix branches (`is_inner_monologue`)
already fall through to the teacher `<gloss>Q: …</gloss>` / `Edit context:` form
for reader-gloss, which is correct (terse append, lede preserved by construction).

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(gloss): three-way reader-gloss branch in add/edit gloss"
```

---

### Task 4: Add "reader-gloss" to all discovery arrays

**Files:**
- Modify: `src/input/actions/gloss.rs:85`, `:115`, `:1891`
- Modify: `src/input/keymap.rs:406`
- Modify: `src/input/actions/synopsis.rs:305`, `:333`

- [ ] **Step 1: gloss.rs discovery arrays**

Change each occurrence of `&["teacher-generic", "inner-monologue"]` at
`src/input/actions/gloss.rs:85` and `:115` to:

```rust
&["teacher-generic", "inner-monologue", "reader-gloss"]
```

And `src/input/actions/gloss.rs:1891`:

```rust
    const GLOSS_TYPES: &[&str] = &["teacher-generic", "inner-monologue", "reader-gloss"];
```

- [ ] **Step 2: keymap.rs discovery array**

`src/input/keymap.rs:406` — change to:

```rust
                                    &["teacher-generic", "inner-monologue", "reader-gloss"],
```

- [ ] **Step 3: synopsis.rs discovery arrays**

`src/input/actions/synopsis.rs:305` and `:333` — change each to:

```rust
&["teacher-generic", "inner-monologue", "reader-gloss"],
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/gloss.rs src/input/keymap.rs src/input/actions/synopsis.rs
git commit -m "feat(gloss): include reader-gloss in gloss discovery + GLOSS_TYPES"
```

---

### Task 5: Make the Ctrl+g picker filter a three-state cycle

**Files:**
- Modify: `src/app.rs:320` (field decl), `:1762` (init)
- Modify: `src/input/actions/pickers.rs:839-845` (`gloss_picker_type`),
  `:852` & `:862` (`open_gloss_picker`), `:896-910` (`toggle_gloss_picker_type`)

- [ ] **Step 1: Add a 3-state enum and helper in pickers.rs**

At the top of `src/input/actions/pickers.rs` (after the `use` lines), add:

```rust
/// Which gloss_type the Ctrl+g picker is currently filtered to. Cycled by Ctrl+t.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum GlossPickerFilter {
    #[default]
    TeacherGeneric,
    InnerMonologue,
    ReaderGloss,
}

impl GlossPickerFilter {
    pub(crate) fn gloss_type(self) -> &'static str {
        match self {
            GlossPickerFilter::TeacherGeneric => "teacher-generic",
            GlossPickerFilter::InnerMonologue => "inner-monologue",
            GlossPickerFilter::ReaderGloss => "reader-gloss",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            GlossPickerFilter::TeacherGeneric => GlossPickerFilter::InnerMonologue,
            GlossPickerFilter::InnerMonologue => GlossPickerFilter::ReaderGloss,
            GlossPickerFilter::ReaderGloss => GlossPickerFilter::TeacherGeneric,
        }
    }
}
```

- [ ] **Step 2: Remove the old `gloss_picker_type` bool helper**

Delete `gloss_picker_type` (lines 838-846) — the enum's `gloss_type()` replaces it.

- [ ] **Step 3: Change the AppState field**

`src/app.rs:320` — change:

```rust
    pub gloss_picker_inner_monologue: bool,
```

to:

```rust
    pub gloss_picker_filter: crate::input::actions::pickers::GlossPickerFilter,
```

`src/app.rs:1762` — change:

```rust
        gloss_picker_inner_monologue: false,
```

to:

```rust
        gloss_picker_filter: crate::input::actions::pickers::GlossPickerFilter::default(),
```

- [ ] **Step 4: Update `open_gloss_picker`**

In `open_gloss_picker` (`pickers.rs`), change line 852:

```rust
    state.borrow_mut().gloss_picker_inner_monologue = false;
```

to:

```rust
    state.borrow_mut().gloss_picker_filter = GlossPickerFilter::default();
```

and line 862:

```rust
            let gloss_type = gloss_picker_type(false);
```

to:

```rust
            let gloss_type = GlossPickerFilter::default().gloss_type();
```

- [ ] **Step 5: Update `toggle_gloss_picker_type`**

In `toggle_gloss_picker_type` (`pickers.rs:896-910`), change the block:

```rust
    let inner_monologue = {
        let mut s = state.borrow_mut();
        s.gloss_picker_inner_monologue = !s.gloss_picker_inner_monologue;
        s.gloss_picker_inner_monologue
    };
```

to:

```rust
    let filter = {
        let mut s = state.borrow_mut();
        s.gloss_picker_filter = s.gloss_picker_filter.next();
        s.gloss_picker_filter
    };
```

and later in the same fn (line ~910):

```rust
            let gloss_type = gloss_picker_type(inner_monologue);
```

to:

```rust
            let gloss_type = filter.gloss_type();
```

- [ ] **Step 6: Build**

Run: `cargo build`
Expected: compiles clean. `set_type_label(&str)` already accepts
`"reader-gloss"`, so the picker placeholder reads
"Filter reader-gloss glosses... (Ctrl+t toggle)".

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/input/actions/pickers.rs
git commit -m "feat(gloss): Ctrl+t gloss-picker cycles teacher/monologue/reader"
```

---

### Task 6: Full build + pure-logic test pass

**Files:** none (verification only)

- [ ] **Step 1: clippy + tests**

Run: `cargo clippy --all-targets 2>&1 | tail -20`
Expected: no new warnings introduced by these changes.

Run: `cargo test --bins`
Expected: PASS, including `reader_gloss_prompts_non_empty_and_no_ipa_slot` and
the existing `all_templated_gloss_prompts_fill_their_placeholder`.

- [ ] **Step 2: Commit any clippy fixes**

```bash
git add -A
git commit -m "chore(gloss): clippy/test cleanup for reader-gloss"
```

(Skip if nothing changed.)

---

### Task 7: Seed prompt masters in the claude-api-prompts repo

> **Correction (applied during execution):** The `gloss.reader-gloss-add`
> prompt was DROPPED. linux-lit's gloss ask-flow has only `Add` and `Edit`
> modes; the `Add` mode is the Q&A path and uses the QUESTION prompt. So there
> are only THREE reader-gloss prompts/masters: `reader-gloss`,
> `reader-gloss-question`, `reader-gloss-edit`. Ignore every `*-add` reference
> elsewhere in this plan (Tasks 1 & 3 code blocks predate the correction; the
> committed code is authoritative — `commit 20184ac`).

**Files (separate repo `~/utono/claude-api-prompts`):**
- Create: `prompts/gloss.reader-gloss.md`
- Create: `prompts/gloss.reader-gloss-question.md`
- Create: `prompts/gloss.reader-gloss-edit.md`

- [ ] **Step 1: Write the three master files**

Create each file with the EXACT same text as the corresponding `FALLBACK` const
that is CURRENTLY in `src/gloss.rs` (read them fresh — verbatim, the master `.md`
and the compiled fallback must match).
Match the format of the existing `prompts/gloss.*.md` files (read
`~/utono/claude-api-prompts/prompts/gloss.edit.md` first for the heading/style
convention; if those files are bare prompt text, write bare prompt text).

- [ ] **Step 2: Sync into lit.db**

Run the repo's sync (read its README / the `sync-prompts` skill first to confirm
the exact invocation):

```bash
cd ~/utono/claude-api-prompts && python scripts/sync-to-db.py
```

- [ ] **Step 3: Verify rows are active**

Run:

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT prompt_key, version, is_active FROM api_prompts WHERE prompt_key LIKE 'gloss.reader-gloss%' ORDER BY prompt_key;"
```

Expected: four rows, each `version=1`, `is_active=1`.

- [ ] **Step 4: Commit (in the prompts repo)**

```bash
cd ~/utono/claude-api-prompts
git add prompts/gloss.reader-gloss*.md
git commit -m "feat: seed reader-gloss prompt masters (v1)"
```

---

### Task 8: User-run runtime verification (renders-correctly criterion)

Per CLAUDE.md, an agent must NOT run `cargo run`, and the output renders in the
gloss overlay — so this is verified by the user. Provide them these checks:

- [ ] **Step 1: Ask the user to launch and verify**

Ask the user to run `cargo run`, then:

1. Select a multi-line, multi-speaker passage (e.g. 2H6 2.1 Suffolk/Gloucester),
   open the Action popup (visual mode → action key). Confirm **"Reader Gloss"
   is the top item**, highlighted by default.
2. Choose Reader Gloss. Confirm the overlay renders `<speaker>`/`<verse>`/`<gloss>`
   correctly and the **first paragraph is a one-sentence lede with semicolons
   separating each character's motivation**.
3. On the same passage, choose "Gloss with Claude" — confirm a SEPARATE teacher
   gloss exists alongside the reader gloss (both reachable; the reader gloss
   wasn't overwritten).
4. Open the gloss picker (Ctrl+g), press Ctrl+t twice — confirm it cycles
   teacher-generic → inner-monologue → reader-gloss (placeholder text updates).
5. With a Reader Gloss open, do an edit (paste lines) — confirm the result stays
   terse, keeps the lede, and saves back as a reader-gloss (not teacher-generic).

If the user prefers the headless harness for the menu/render check:

```bash
./scripts/e2e-env.sh cargo test --test smoke -- --ignored --nocapture
```

- [ ] **Step 2: Address any issues the user reports, then finish the branch**

Once verified, follow the CLAUDE.md finish-a-branch flow (merge to master, push,
delete the feature branch).

---

## Self-Review Notes

- **Spec coverage:** new type + label (T2) ✓; terse reader-focused content +
  one-sentence semicolon lede + always-keep-lede (T1 prompts) ✓; plain/no-IPA
  (T1) ✓; coexistence as separate slot (T2 uses `build_context_for_type` +
  `find_all_glosses(&["reader-gloss"])`) ✓; full parity create/view/cache (T2),
  edit/Q&A/add (T3), discovery (T4), picker 3-cycle (T5) ✓; menu-first (T2) ✓;
  no new keybind ✓; DB seeding via claude-api-prompts (T7) ✓; user-run render
  verification (T8) ✓.
- **No new keybind** and **no Ctrl+/ overlay change** (Action menu isn't a
  keycap; verified no describe() arm enumerates Action items) — nothing to do.
- **Type consistency:** `GlossPickerFilter` (enum), `.gloss_type()`, `.next()`,
  `gloss_picker_filter` (field) used identically in app.rs and pickers.rs.
  Prompt const names `READER_GLOSS_PROMPT` / `_QUESTION_` / `_EDIT_` / `_ADD_`
  used identically across T1/T2/T3.
