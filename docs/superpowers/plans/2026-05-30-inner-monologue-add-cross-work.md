# Inner Monologue Add: Cross-Work Passages — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `a` keybind in the gloss overlay context-sensitive — when viewing an inner-monologue gloss, it prompts the user to paste cross-work lines and generates a new inner-monologue treating those lines as the characters' unspoken inner voice.

**Architecture:** Add a `gloss_type` field to `GlossContext` so the overlay knows what type of gloss it's displaying. `show_amend_dialog` and `add_gloss` branch on this field to choose dialog labels, Claude prompt, message builder, and save type.

**Tech Stack:** Rust, GTK4, Claude API (via existing `call_claude_with_prompt`)

---

## File Map

- **Modify: `src/gloss.rs`** — Add `gloss_type` field to `GlossContext`, add `INNER_MONOLOGUE_ADD_PROMPT` constant, add `build_inner_monologue_add_message` function, update both `build_context` and `build_context_for_type` to set the new field.
- **Modify: `src/input/actions/gloss.rs`** — Update `show_amend_dialog` for context-sensitive labels, update `add_gloss` to branch on gloss_type, update `navigate_gloss_passage` to set `gloss_type` on its manual `GlossContext`.
- **Modify: `src/input/keymap.rs`** — Update the `GlossPicker` confirm handler's manual `GlossContext` construction to set `gloss_type`.

---

### Task 1: Add `gloss_type` field to `GlossContext`

**Files:**
- Modify: `src/gloss.rs:87-98` (struct definition)
- Modify: `src/gloss.rs:144-155` (build_context return)
- Modify: `src/gloss.rs:188-199` (build_context_for_type return)

- [ ] **Step 1: Add `gloss_type` field to the struct**

In `src/gloss.rs`, add `pub gloss_type: String` after `pub hash: String`:

```rust
pub struct GlossContext {
    pub work_abbrev: String,
    pub work_title: String,
    pub start_citation: String,
    pub end_citation: String,
    pub act: i64,
    pub scene: i64,
    pub speaker: String,
    pub source_text: String,
    pub source_line_numbers: Vec<i64>,
    pub hash: String,
    pub gloss_type: String,
}
```

- [ ] **Step 2: Update `build_context` to set `gloss_type`**

In `src/gloss.rs:144-155`, add `gloss_type: "teacher-generic".to_string()` to the `GlossContext` struct literal:

```rust
    Some(GlossContext {
        work_abbrev: base_abbrev.to_string(),
        work_title: work.title.clone(),
        start_citation,
        end_citation,
        act: first.div1,
        scene: first.div2,
        speaker,
        source_text,
        source_line_numbers,
        hash,
        gloss_type: "teacher-generic".to_string(),
    })
```

- [ ] **Step 3: Update `build_context_for_type` to set `gloss_type`**

In `src/gloss.rs:188-199`, add `gloss_type: gloss_type.to_string()`:

```rust
    Some(GlossContext {
        work_abbrev: base_abbrev.to_string(),
        work_title: work.title.clone(),
        start_citation,
        end_citation,
        act: first.div1,
        scene: first.div2,
        speaker,
        source_text,
        source_line_numbers,
        hash,
        gloss_type: gloss_type.to_string(),
    })
```

- [ ] **Step 4: Update manual `GlossContext` in `navigate_gloss_passage`**

In `src/input/actions/gloss.rs:57-68`, add `gloss_type: "teacher-generic".to_string()`:

```rust
    let ctx = crate::gloss::GlossContext {
        work_abbrev: passage.work_abbrev,
        work_title,
        start_citation: passage.start_citation,
        end_citation: passage.end_citation,
        act: passage.act,
        scene: passage.scene,
        speaker: passage.speaker,
        source_text: passage.source_text,
        source_line_numbers: Vec::new(),
        hash: String::new(),
        gloss_type: "teacher-generic".to_string(),
    };
```

- [ ] **Step 5: Update manual `GlossContext` in `GlossPicker` confirm handler**

In `src/input/keymap.rs:351-362`, add `gloss_type: "teacher-generic".to_string()`:

```rust
                        let ctx = crate::gloss::GlossContext {
                            work_abbrev: passage.work_abbrev,
                            work_title,
                            start_citation: passage.start_citation,
                            end_citation: passage.end_citation,
                            act: passage.act,
                            scene: passage.scene,
                            speaker: passage.speaker,
                            source_text: passage.source_text,
                            source_line_numbers: Vec::new(),
                            hash: String::new(),
                            gloss_type: "teacher-generic".to_string(),
                        };
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles successfully with no errors

- [ ] **Step 7: Commit**

```bash
git add src/gloss.rs src/input/actions/gloss.rs src/input/keymap.rs
git commit -m "Add gloss_type field to GlossContext"
```

---

### Task 2: Add `INNER_MONOLOGUE_ADD_PROMPT` and `build_inner_monologue_add_message`

**Files:**
- Modify: `src/gloss.rs` (add constant after `INNER_MONOLOGUE_PROMPT`, add function after `build_inner_monologue_message`)

- [ ] **Step 1: Add the prompt constant**

In `src/gloss.rs`, add after line 58 (the end of `INNER_MONOLOGUE_PROMPT`):

```rust
pub const INNER_MONOLOGUE_ADD_PROMPT: &str = "\
You are a director helping actors discover the inner monologue beneath \
a passage from a dramatic text.

The reader has selected a passage and provided lines from elsewhere in \
Shakespeare's corpus that share thematic or verbal echoes. Treat the \
provided lines as the unspoken inner voice — what the characters in the \
original passage might be thinking or hearing beneath their spoken words.

For each character in the original passage:
- How do the cross-work lines illuminate what this character is really \
thinking or feeling?
- What verbal echoes connect the two passages (shared words, inverted \
meanings, parallel structures)?
- What actable inner cues can an actor draw from the cross-work lines — \
short thoughts that sit beneath each spoken line?

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> for each character's analysis section (ALL CAPS)
- <verse>one line of quoted text</verse> for quoted lines (verbatim)
- <gloss>paragraph of analysis</gloss> for each analysis paragraph

Rules:
- Quote verbatim — exact words, exact spelling, exact line breaks
- Never use / to join verse lines
- Each <verse> tag contains exactly one line of the original
- Each <gloss> tag contains one flowing prose paragraph (3-6 sentences)
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines
- No markdown, no bullets, no numbered lists, no headers";
```

- [ ] **Step 2: Add the message builder function**

In `src/gloss.rs`, add after the `build_inner_monologue_message` function (after line 247):

```rust
pub fn build_inner_monologue_add_message(
    ctx: &GlossContext,
    pasted_lines: &str,
) -> String {
    format!(
        "Play: {}\nAct: {}, Scene: {}\nSpeaker: {}\n\n\
         --- ORIGINAL PASSAGE ---\n{}\n\n\
         --- CROSS-WORK LINES (inner voice) ---\n{}",
        ctx.work_title, ctx.act, ctx.scene, ctx.speaker,
        ctx.source_text,
        pasted_lines,
    )
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles successfully (new items are `pub` but unused warnings are fine)

- [ ] **Step 4: Commit**

```bash
git add src/gloss.rs
git commit -m "Add INNER_MONOLOGUE_ADD_PROMPT and build_inner_monologue_add_message"
```

---

### Task 3: Make `show_amend_dialog` context-sensitive

**Files:**
- Modify: `src/input/actions/gloss.rs:202-260` (show_amend_dialog function)

- [ ] **Step 1: Read gloss_type and set labels**

Replace the title and hint label creation in `show_amend_dialog` (lines 222-225 and 244) with context-sensitive labels. The function needs to read `gloss_type` from `gloss_context` before building the dialog:

Replace lines 202-260 of `show_amend_dialog` with:

```rust
pub(crate) fn show_amend_dialog(state_rc: &Rc<RefCell<AppState>>) {
    let is_inner_monologue = {
        let s = state_rc.borrow();
        s.gloss_context.as_ref()
            .map(|ctx| ctx.gloss_type == "inner-monologue")
            .unwrap_or(false)
    };

    let overlay_parent = {
        let s = state_rc.borrow();
        s.action_popup_widget.container.parent()
    };
    let overlay_parent = match overlay_parent {
        Some(p) => p.downcast::<gtk4::Overlay>().ok(),
        None => None,
    };
    let overlay_parent = match overlay_parent {
        Some(o) => o,
        None => return,
    };

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_halign(gtk4::Align::Center);
    container.set_valign(gtk4::Align::Center);
    container.set_width_request(600);
    container.add_css_class("amend-dialog");

    let title_text = if is_inner_monologue {
        "INNER MONOLOGUE PASSAGE"
    } else {
        "GLOSS PROMPT"
    };
    let title = gtk4::Label::new(Some(title_text));
    title.add_css_class("amend-title");
    title.set_halign(gtk4::Align::Start);
    container.append(&title);

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_min_content_height(120);
    scrolled.set_margin_start(22);
    scrolled.set_margin_end(22);
    scrolled.set_margin_top(8);
    scrolled.set_margin_bottom(8);

    let text_view = gtk4::TextView::new();
    text_view.set_wrap_mode(gtk4::WrapMode::Word);
    text_view.set_top_margin(8);
    text_view.set_bottom_margin(8);
    text_view.set_left_margin(4);
    text_view.set_right_margin(4);
    text_view.add_css_class("amend-text");
    scrolled.set_child(Some(&text_view));
    container.append(&scrolled);

    let hint_text = if is_inner_monologue {
        "Paste lines from another work  \u{00b7}  Ctrl+Enter submit  \u{00b7}  Esc cancel"
    } else {
        "Ctrl+Enter submit  \u{00b7}  Esc cancel"
    };
    let hint = gtk4::Label::new(Some(hint_text));
    hint.add_css_class("amend-hint");
    hint.set_halign(gtk4::Align::Center);
    container.append(&hint);

    overlay_parent.add_overlay(&container);

    {
        let mut s = state_rc.borrow_mut();
        s.gloss_prompt_container = Some(container.downgrade());
        s.gloss_prompt_overlay = Some(overlay_parent.downgrade());
        s.gloss_prompt_textview = Some(text_view.downgrade());
        s.input_mode = crate::app::InputMode::GlossPrompt;
    }

    text_view.grab_focus();
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "Make show_amend_dialog context-sensitive for inner-monologue"
```

---

### Task 4: Make `add_gloss` context-sensitive

**Files:**
- Modify: `src/input/actions/gloss.rs:262-334` (add_gloss function)

- [ ] **Step 1: Replace `add_gloss` with branching implementation**

Replace the entire `add_gloss` function (lines 262-334) with:

```rust
pub(crate) fn add_gloss(state_rc: &Rc<RefCell<AppState>>, prompt: &str) {
    let (ctx, model, tokio_handle) = {
        let state = state_rc.borrow();
        let ctx = match &state.gloss_context {
            Some(c) => c.clone(),
            None => return,
        };
        (ctx, state.config.claude_model.clone(), state.tokio_handle.clone())
    };

    state_rc.borrow().gloss_overlay.show_loading();

    let prompt_owned = prompt.to_string();
    let is_inner_monologue = ctx.gloss_type == "inner-monologue";

    let (system_prompt, user_msg, gloss_type_str) = if is_inner_monologue {
        let msg = crate::gloss::build_inner_monologue_add_message(&ctx, &prompt_owned);
        (crate::gloss::INNER_MONOLOGUE_ADD_PROMPT, msg, "inner-monologue")
    } else {
        let msg = crate::gloss::build_user_message(&ctx, Some(&prompt_owned), None);
        (crate::gloss::USER_QUESTION_PROMPT, msg, "teacher-generic")
    };

    let state_for_result = Rc::clone(state_rc);
    let gloss_type_owned = gloss_type_str.to_string();

    glib::spawn_future_local(async move {
        let system_prompt = system_prompt.to_string();
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(
                    &system_prompt, &user_msg, &model,
                ).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
                let full_gloss = if is_inner_monologue {
                    format!("<gloss>Inner voice from:</gloss>\n\n{}\n\n{}", prompt_owned, gloss_text)
                } else {
                    format!("<gloss>Q: {}</gloss>\n\n{}", prompt_owned, gloss_text)
                };
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let _ = crate::db::queries::save_gloss(
                        &conn, &ctx.hash, &ctx.work_abbrev,
                        &ctx.start_citation, &ctx.end_citation,
                        ctx.act, ctx.scene, &ctx.speaker,
                        &ctx.source_text, &full_gloss,
                        &gloss_type_owned,
                    );
                }

                let all = crate::db::queries::open_db()
                    .ok()
                    .and_then(|conn| {
                        crate::db::queries::find_all_glosses(
                            &conn, &ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation,
                            &gloss_type_owned,
                        ).ok()
                    })
                    .unwrap_or_default();

                let mut s = state_for_result.borrow_mut();
                let h = s.scrolled_window.height();
                let pairs = ctx.source_line_pairs();
                s.gloss_overlay.show_gloss_with_color(
                    &ctx.source_text, &full_gloss, h,
                    Some(&s.theme.root_color), &pairs,
                );
                s.gloss_overlay.set_position(0, all.len());
                s.gloss_list = all;
                s.gloss_index = 0;
                crate::logging::log(&format!("GLOSS: added new {} gloss", gloss_type_owned));
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.gloss_overlay.show(&format!("Error: {}", e), "");
                crate::logging::log(&format!("GLOSS: add error: {}", e));
            }
            Err(e) => {
                crate::logging::log(&format!("GLOSS: tokio join error: {}", e));
            }
        }
    });
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles successfully

- [ ] **Step 3: Run clippy**

Run: `cargo clippy 2>&1 | tail -10`
Expected: no new warnings

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "Make add_gloss context-sensitive for inner-monologue cross-work passages"
```
