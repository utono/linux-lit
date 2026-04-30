# Multi-Gloss Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Support multiple glosses per passage with navigation, add-from-prompt, delete, copy ID, and position indicator.

**Architecture:** Replace single `gloss_saved: Option<SavedGloss>` with `gloss_list: Vec<SavedGloss>` + `gloss_index: usize`. Add `find_all_glosses` query. Rewrite `handle_gloss_key` for new keybinds. Convert `amend_gloss` to `add_gloss` (inserts new row instead of updating). Add position indicator label to `GlossOverlay`.

**Tech Stack:** Rust, GTK4, rusqlite, wl-copy

---

### Task 1: Add `find_all_glosses` query

**Files:**
- Modify: `src/db/queries.rs:694-721`

- [ ] **Step 1: Add `find_all_glosses` function after `find_existing_gloss`**

Add at line 722 (after `find_existing_gloss` closing brace):

```rust
pub fn find_all_glosses(
    conn: &Connection,
    work_abbrev: &str,
    start_citation: &str,
    end_citation: &str,
) -> Result<Vec<SavedGloss>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT g.id, g.gloss_text, g.timestamp, p.id \
         FROM glosses g \
         JOIN passages p ON g.passage_id = p.id \
         WHERE p.work_abbrev = ?1 \
           AND p.start_citation = ?2 \
           AND p.end_citation = ?3 \
           AND g.gloss_type = 'teacher-generic' \
         ORDER BY g.timestamp DESC",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![work_abbrev, start_citation, end_citation],
        |row| {
            Ok(SavedGloss {
                gloss_id: row.get(0)?,
                gloss_text: row.get(1)?,
                timestamp: row.get(2)?,
                passage_id: row.get(3)?,
            })
        },
    )?;
    rows.collect()
}
```

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: compiles with no errors in queries.rs

- [ ] **Step 3: Commit**

```bash
git add src/db/queries.rs
git commit -m "Add find_all_glosses query returning all glosses for a passage"
```

---

### Task 2: Change AppState from single gloss to gloss list

**Files:**
- Modify: `src/app.rs:142-144` (field declarations)
- Modify: `src/app.rs:887-889` (initialization)

- [ ] **Step 1: Replace `gloss_saved` with `gloss_list` and `gloss_index` in field declarations**

At `src/app.rs:142-144`, change:

```rust
    pub gloss_original_text: Option<String>,
    pub gloss_saved: Option<crate::db::queries::SavedGloss>,
    pub gloss_context: Option<crate::gloss::GlossContext>,
```

to:

```rust
    pub gloss_original_text: Option<String>,
    pub gloss_list: Vec<crate::db::queries::SavedGloss>,
    pub gloss_index: usize,
    pub gloss_context: Option<crate::gloss::GlossContext>,
```

- [ ] **Step 2: Update initialization in AppState constructor**

At `src/app.rs:887-889`, change:

```rust
        gloss_original_text: None,
        gloss_saved: None,
        gloss_context: None,
```

to:

```rust
        gloss_original_text: None,
        gloss_list: Vec::new(),
        gloss_index: 0,
        gloss_context: None,
```

- [ ] **Step 3: Build — expect errors in keymap.rs and visual.rs (they still reference `gloss_saved`)**

Run: `cargo build 2>&1 | rg "gloss_saved"`
Expected: errors in keymap.rs and visual.rs referencing the old field name. These will be fixed in subsequent tasks.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "Replace gloss_saved with gloss_list/gloss_index in AppState"
```

---

### Task 3: Update GlossOverlay with position indicator and new hint text

**Files:**
- Modify: `src/ui/gloss_overlay.rs`

- [ ] **Step 1: Add `position_label` field to GlossOverlay struct**

At `src/ui/gloss_overlay.rs`, add to the struct after the `hint` field:

```rust
    position_label: Label,
```

- [ ] **Step 2: Create the position label in `new()`, insert before the hint**

After the hint label is created (around line 185) and before `container.append(&hint)`, add:

```rust
        let position_label = Label::new(None);
        position_label.add_css_class("gloss-hint");
        position_label.set_halign(Align::Center);
        position_label.set_margin_bottom(4);
        position_label.set_visible(false);
        container.append(&position_label);
```

- [ ] **Step 3: Update the hint text**

Change the hint Label text from:

```rust
        let hint = Label::new(Some("Esc = close  ·  a = amend  ·  r = regenerate"));
```

to:

```rust
        let hint = Label::new(Some("Esc close · a add · d delete · c copy id · Ctrl+n/p navigate"));
```

- [ ] **Step 4: Add `position_label` to struct constructor return**

Add `position_label,` to the struct literal in the constructor.

- [ ] **Step 5: Add `set_position` method**

Add to the `impl GlossOverlay` block:

```rust
    pub fn set_position(&self, index: usize, total: usize) {
        if total > 1 {
            self.position_label.set_text(&format!("{} / {}", index + 1, total));
            self.position_label.set_visible(true);
        } else {
            self.position_label.set_visible(false);
        }
    }
```

- [ ] **Step 6: Build to verify (may have errors from other files, but gloss_overlay.rs should be clean)**

Run: `cargo build 2>&1 | rg "gloss_overlay.rs"`
Expected: no errors from this file

- [ ] **Step 7: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "Add position indicator and update hint text in gloss overlay"
```

---

### Task 4: Update visual.rs to use gloss_list and find_all_glosses

**Files:**
- Modify: `src/input/visual.rs`

- [ ] **Step 1: Update existing-gloss check in `action_gloss_with_claude`**

In `action_gloss_with_claude`, replace the `find_existing_gloss` call and the `if let Some(ref saved) = existing` block (around lines 416-437) with:

```rust
    let all_glosses: Vec<crate::db::queries::SavedGloss> = match crate::db::queries::open_db() {
        Ok(conn) => crate::db::queries::find_all_glosses(
            &conn, &ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation,
        ).unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    exit_visual_mode(&mut state_rc.borrow_mut());

    if !all_glosses.is_empty() {
        let mut s = state_rc.borrow_mut();
        s.gloss_original_text = Some(ctx.source_text.clone());
        let pairs = ctx.source_line_pairs();
        let gloss_text = &all_glosses[0].gloss_text;
        s.gloss_overlay.show_gloss_with_color(&ctx.source_text, gloss_text, s.scrolled_window.height(), Some(&s.theme.root_color), &pairs);
        s.gloss_overlay.set_position(0, all_glosses.len());
        s.gloss_list = all_glosses;
        s.gloss_index = 0;
        s.gloss_context = Some(ctx);
        s.input_mode = crate::app::InputMode::GlossOverlay;
        crate::logging::log("GLOSS: showing cached gloss");
        return;
    }
```

- [ ] **Step 2: Update the API result handler**

In the `Ok(Ok(gloss_text))` match arm where the new gloss is displayed after API call, replace the section that sets `gloss_saved` (around line 485):

```rust
                let saved = crate::db::queries::open_db()
                    .ok()
                    .and_then(|conn| {
                        crate::db::queries::find_all_glosses(
                            &conn, &ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation,
                        ).ok()
                    })
                    .unwrap_or_default();

                let mut s = state_for_result.borrow_mut();
                let h = s.scrolled_window.height();
                let pairs = ctx.source_line_pairs();
                s.gloss_overlay.show_gloss_with_color(&ctx.source_text, &gloss_text, h, Some(&s.theme.root_color), &pairs);
                s.gloss_overlay.set_position(0, saved.len());
                s.gloss_list = saved;
                s.gloss_index = 0;
                s.gloss_context = Some(ctx);
                crate::logging::log("GLOSS: generated and saved new gloss");
```

- [ ] **Step 3: Build to verify visual.rs compiles**

Run: `cargo build 2>&1 | rg "visual.rs"`
Expected: no errors from visual.rs

- [ ] **Step 4: Commit**

```bash
git add src/input/visual.rs
git commit -m "Use find_all_glosses and gloss_list in visual mode gloss action"
```

---

### Task 5: Rewrite handle_gloss_key and gloss toggle for multi-gloss

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Update the Shift+Tab / Ctrl+g toggle handler**

Replace the block at lines 166-180 that checks `gloss_saved.is_some()`:

```rust
    // Shift+Tab or Ctrl+g: toggle gloss overlay for last-viewed gloss
    if key_name == "ISO_Left_Tab" || (is_ctrl && key_name == "g") {
        let has_gloss = !state.borrow().gloss_list.is_empty();
        if has_gloss {
            let s = state.borrow();
            let idx = s.gloss_index;
            let gloss = &s.gloss_list[idx];
            let ctx = s.gloss_context.as_ref().unwrap();
            let h = s.scrolled_window.height();
            let pairs = ctx.source_line_pairs();
            s.gloss_overlay.show_gloss_with_color(&ctx.source_text, &gloss.gloss_text, h, Some(&s.theme.root_color), &pairs);
            s.gloss_overlay.set_position(idx, s.gloss_list.len());
            drop(s);
            state.borrow_mut().input_mode = crate::app::InputMode::GlossOverlay;
            return true;
        }
        return false;
    }
```

- [ ] **Step 2: Rewrite `handle_gloss_key` with new keybinds**

Replace the entire `handle_gloss_key` function:

```rust
fn handle_gloss_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
) -> bool {
    if is_ctrl {
        match key_name {
            "n" => {
                navigate_gloss(state, -1);
                return true;
            }
            "p" => {
                navigate_gloss(state, 1);
                return true;
            }
            _ => {}
        }
    }
    match key_name {
        "a" => {
            show_amend_dialog(state);
            true
        }
        "u" => {
            navigate_gloss(state, 1);
            true
        }
        "c" => {
            copy_gloss_id(state);
            true
        }
        "d" => {
            show_delete_confirmation(state);
            true
        }
        "j" => {
            state.borrow().gloss_overlay.scroll_gloss(1);
            true
        }
        "k" => {
            state.borrow().gloss_overlay.scroll_gloss(-1);
            true
        }
        "Escape" | "n" => {
            {
                let mut s = state.borrow_mut();
                s.gloss_overlay.hide();
                s.input_mode = crate::app::InputMode::Reader;
            }
            true
        }
        _ => true,
    }
}
```

- [ ] **Step 3: Update `handle_gloss_key` call site to pass `is_ctrl`**

In the mode dispatch (around line 112), change:

```rust
            crate::app::InputMode::GlossOverlay => handle_gloss_key(state, key_name),
```

to:

```rust
            crate::app::InputMode::GlossOverlay => handle_gloss_key(state, key_name, is_ctrl),
```

- [ ] **Step 4: Add `navigate_gloss` helper function**

Add after `handle_gloss_key`:

```rust
fn navigate_gloss(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    if s.gloss_list.is_empty() {
        return;
    }
    let new_idx = (s.gloss_index as i32 + delta)
        .max(0)
        .min(s.gloss_list.len() as i32 - 1) as usize;
    if new_idx == s.gloss_index {
        return;
    }
    s.gloss_index = new_idx;
    let gloss = &s.gloss_list[new_idx];
    let ctx = s.gloss_context.as_ref().unwrap();
    let h = s.scrolled_window.height();
    let pairs = ctx.source_line_pairs();
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, &gloss.gloss_text, h,
        Some(&s.theme.root_color), &pairs,
    );
    s.gloss_overlay.set_position(new_idx, s.gloss_list.len());
}
```

- [ ] **Step 5: Add `copy_gloss_id` helper function**

```rust
fn copy_gloss_id(state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    if let Some(gloss) = s.gloss_list.get(s.gloss_index) {
        let id = gloss.gloss_id.to_string();
        let _ = std::process::Command::new("wl-copy")
            .arg(&id)
            .spawn();
        crate::logging::log(&format!("GLOSS: copied id {} to clipboard", id));
    }
}
```

- [ ] **Step 6: Build to check for errors**

Run: `cargo build 2>&1 | rg "^error"`
Expected: may still have errors from `regenerate_gloss` references and `amend_gloss` — those are fixed in the next tasks.

- [ ] **Step 7: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Rewrite gloss overlay keybinds for multi-gloss navigation"
```

---

### Task 6: Convert amend_gloss to add_gloss (insert new row)

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Rename `amend_gloss` to `add_gloss` and change from `update_gloss` to `save_gloss`**

Replace the entire `amend_gloss` function (starting at line 1316) with:

```rust
fn add_gloss(state_rc: &Rc<RefCell<AppState>>, prompt: &str) {
    let (ctx, model, tokio_handle, existing_text) = {
        let state = state_rc.borrow();
        let ctx = match &state.gloss_context {
            Some(c) => c.clone(),
            None => return,
        };
        let existing_text = state.gloss_list
            .get(state.gloss_index)
            .map(|g| g.gloss_text.clone())
            .unwrap_or_default();
        (ctx, state.config.claude_model.clone(), state.tokio_handle.clone(), existing_text)
    };

    state_rc.borrow().gloss_overlay.show_loading();

    let prompt_owned = prompt.to_string();
    let user_msg = crate::gloss::build_user_message(
        &ctx, Some(&prompt_owned), Some(&existing_text),
    );
    let state_for_result = Rc::clone(state_rc);

    glib::spawn_future_local(async move {
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude(&user_msg, &model).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let _ = crate::db::queries::save_gloss(
                        &conn, &ctx.hash, &ctx.work_abbrev,
                        &ctx.start_citation, &ctx.end_citation,
                        ctx.act, ctx.scene, &ctx.speaker,
                        &ctx.source_text, &gloss_text,
                    );
                }

                let all = crate::db::queries::open_db()
                    .ok()
                    .and_then(|conn| {
                        crate::db::queries::find_all_glosses(
                            &conn, &ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation,
                        ).ok()
                    })
                    .unwrap_or_default();

                let mut s = state_for_result.borrow_mut();
                let h = s.scrolled_window.height();
                let pairs = ctx.source_line_pairs();
                s.gloss_overlay.show_gloss_with_color(
                    &ctx.source_text, &gloss_text, h,
                    Some(&s.theme.root_color), &pairs,
                );
                s.gloss_overlay.set_position(0, all.len());
                s.gloss_list = all;
                s.gloss_index = 0;
                crate::logging::log("GLOSS: added new gloss");
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

- [ ] **Step 2: Update `show_amend_dialog` to call `add_gloss` instead of `amend_gloss`**

In `show_amend_dialog`, find the line:
```rust
                amend_gloss(&state_for_key, &prompt);
```
Change to:
```rust
                add_gloss(&state_for_key, &prompt);
```

- [ ] **Step 3: Delete the `regenerate_gloss` function entirely**

Remove the entire `regenerate_gloss` function (starts around line 1158, ~60 lines).

- [ ] **Step 4: Build to verify**

Run: `cargo build 2>&1 | rg "^error"`
Expected: clean build

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Convert amend to add (inserts new row), remove regenerate_gloss"
```

---

### Task 7: Add delete confirmation overlay

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Add `show_delete_confirmation` function**

Add after `copy_gloss_id`:

```rust
fn show_delete_confirmation(state_rc: &Rc<RefCell<AppState>>) {
    let gloss_id = {
        let s = state_rc.borrow();
        match s.gloss_list.get(s.gloss_index) {
            Some(g) => g.gloss_id,
            None => return,
        }
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

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    container.set_halign(gtk4::Align::Center);
    container.set_valign(gtk4::Align::Center);
    container.set_width_request(400);
    container.add_css_class("amend-dialog");

    let label = gtk4::Label::new(Some(&format!("Delete gloss {}?", gloss_id)));
    label.add_css_class("amend-title");
    label.set_halign(gtk4::Align::Start);
    container.append(&label);

    let hint = gtk4::Label::new(Some("y = confirm  ·  Esc = cancel"));
    hint.add_css_class("amend-hint");
    hint.set_halign(gtk4::Align::Center);
    container.append(&hint);

    overlay_parent.add_overlay(&container);
    container.set_can_focus(true);
    container.grab_focus();

    let state_for_key = Rc::clone(state_rc);
    let container_weak = container.downgrade();
    let overlay_weak = overlay_parent.downgrade();

    let key_controller = gtk4::EventControllerKey::new();
    key_controller.connect_key_pressed(move |_ctrl, keyval, _code, _modifier| {
        let key_name = keyval.name().unwrap_or_default();
        match key_name.as_str() {
            "y" => {
                if let (Some(c), Some(o)) = (container_weak.upgrade(), overlay_weak.upgrade()) {
                    o.remove_overlay(&c);
                }
                delete_current_gloss(&state_for_key);
                glib::Propagation::Stop
            }
            "Escape" | "n" => {
                if let (Some(c), Some(o)) = (container_weak.upgrade(), overlay_weak.upgrade()) {
                    o.remove_overlay(&c);
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Stop,
        }
    });
    container.add_controller(key_controller);
}

fn delete_current_gloss(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    let idx = s.gloss_index;
    if let Some(gloss) = s.gloss_list.get(idx) {
        let gloss_id = gloss.gloss_id;
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            let _ = crate::db::queries::delete_gloss(&conn, gloss_id);
        }
        crate::logging::log(&format!("GLOSS: deleted gloss {}", gloss_id));
        s.gloss_list.remove(idx);

        if s.gloss_list.is_empty() {
            s.gloss_overlay.hide();
            s.input_mode = crate::app::InputMode::Reader;
            return;
        }

        s.gloss_index = idx.min(s.gloss_list.len() - 1);
        let new_idx = s.gloss_index;
        let gloss = &s.gloss_list[new_idx];
        let ctx = s.gloss_context.as_ref().unwrap();
        let h = s.scrolled_window.height();
        let pairs = ctx.source_line_pairs();
        s.gloss_overlay.show_gloss_with_color(
            &ctx.source_text, &gloss.gloss_text, h,
            Some(&s.theme.root_color), &pairs,
        );
        s.gloss_overlay.set_position(new_idx, s.gloss_list.len());
    }
}
```

- [ ] **Step 2: Build to verify**

Run: `cargo build 2>&1 | rg "^error"`
Expected: clean build

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Add delete gloss with y/n confirmation overlay"
```

---

### Task 8: Final cleanup and verify

**Files:**
- All modified files

- [ ] **Step 1: Remove any remaining references to `gloss_saved`**

Run: `rg "gloss_saved" src/`
Expected: no matches. If any remain, update them to use `gloss_list`/`gloss_index`.

- [ ] **Step 2: Remove any remaining references to `regenerate_gloss`**

Run: `rg "regenerate_gloss" src/`
Expected: no matches.

- [ ] **Step 3: Full build**

Run: `cargo build`
Expected: clean build with no errors

- [ ] **Step 4: Commit any remaining cleanup**

```bash
git add -A
git commit -m "Clean up remaining gloss_saved references"
```

- [ ] **Step 5: Push**

```bash
git push
```
