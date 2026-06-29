# Gloss Action Extraction (F4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move ~400 lines of gloss business logic (navigation, delete, amend dialog, Claude API calls) from `keymap.rs` into a dedicated `actions/gloss.rs` module, so `keymap.rs` becomes pure key routing.

**Architecture:** Create `src/input/actions/gloss.rs` and move 7 functions from `keymap.rs` into it. `handle_gloss_key` in `keymap.rs` calls into the new module. No behavior changes — pure code motion.

**Tech Stack:** Rust, GTK4, glib async

---

### Task 1: Create `actions/gloss.rs` with navigate_gloss_passage

**Files:**
- Create: `src/input/actions/gloss.rs`
- Modify: `src/input/actions/mod.rs` (add `pub mod gloss;`)

- [ ] **Step 1: Add module declaration**

In `src/input/actions/mod.rs`, add after the existing module declarations:

```rust
pub mod gloss;
```

- [ ] **Step 2: Create `gloss.rs` with navigate_gloss_passage**

Create `src/input/actions/gloss.rs` with the `navigate_gloss_passage` function moved from `keymap.rs:617-688`:

```rust
use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;

pub fn navigate_gloss_passage(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();

    let work_abbrev = match &s.gloss_context {
        Some(ctx) => ctx.work_abbrev.clone(),
        None => return,
    };

    if s.gloss_passages.is_empty() {
        if let Ok(conn) = crate::db::queries::open_db() {
            s.gloss_passages = crate::db::queries::find_glossed_passages(&conn, &work_abbrev)
                .unwrap_or_default();
        }
        if s.gloss_passages.is_empty() {
            return;
        }
        if let Some(ctx) = &s.gloss_context {
            s.gloss_passage_index = s.gloss_passages.iter()
                .position(|p| p.start_citation == ctx.start_citation && p.end_citation == ctx.end_citation)
                .unwrap_or(0);
        }
    }

    let len = s.gloss_passages.len();
    let new_idx = ((s.gloss_passage_index as i32 + delta).rem_euclid(len as i32)) as usize;
    if new_idx == s.gloss_passage_index && len > 1 {
        return;
    }
    s.gloss_passage_index = new_idx;

    let passage = s.gloss_passages[new_idx].clone();

    let all_glosses = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::queries::find_all_glosses(
                &conn, &passage.work_abbrev, &passage.start_citation, &passage.end_citation,
            ).ok()
        })
        .unwrap_or_default();

    if all_glosses.is_empty() {
        return;
    }

    let source_lines: Vec<(String, i64)> = Vec::new();

    let work_title = s.current_work.as_ref().map(|w| w.title.clone()).unwrap_or_default();
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
    };

    let h = s.scrolled_window.height();
    let gloss_text = &all_glosses[0].gloss_text;
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, gloss_text, h,
        Some(&s.theme.root_color), &source_lines,
    );
    s.gloss_overlay.set_position(0, all_glosses.len());
    s.gloss_list = all_glosses;
    s.gloss_index = 0;
    s.gloss_context = Some(ctx);
}
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: Compiles (the old function in keymap.rs still exists — we'll remove it next).

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/gloss.rs src/input/actions/mod.rs
git commit -m "Create actions/gloss.rs with navigate_gloss_passage"
```

---

### Task 2: Move navigate_gloss, copy_gloss_id, delete_current_gloss

**Files:**
- Modify: `src/input/actions/gloss.rs` (add 3 functions)

- [ ] **Step 1: Add navigate_gloss**

Add to `src/input/actions/gloss.rs`:

```rust
pub fn navigate_gloss(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    let len = s.gloss_list.len();
    if len == 0 {
        return;
    }
    let new_idx = ((s.gloss_index as i32 + delta).rem_euclid(len as i32)) as usize;
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

- [ ] **Step 2: Add copy_gloss_id**

```rust
pub fn copy_gloss_id(state: &Rc<RefCell<AppState>>) {
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

- [ ] **Step 3: Add delete_current_gloss**

```rust
pub fn delete_current_gloss(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
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

- [ ] **Step 4: Build**

Run: `cargo build`

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "Add navigate_gloss, copy_gloss_id, delete_current_gloss to actions/gloss"
```

---

### Task 3: Move show_delete_confirmation, show_amend_dialog, add_gloss

**Files:**
- Modify: `src/input/actions/gloss.rs` (add 3 functions)

- [ ] **Step 1: Add show_delete_confirmation**

Add to `gloss.rs`. This function builds a GTK confirmation dialog — add the necessary GTK imports at the top of the file:

```rust
use gtk4::prelude::*;
```

Then add the function (moved from `keymap.rs:723-790`):

```rust
pub fn show_delete_confirmation(state_rc: &Rc<RefCell<AppState>>) {
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

    let hint = gtk4::Label::new(Some("y = confirm  \u{00b7}  Esc = cancel"));
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
```

- [ ] **Step 2: Add show_amend_dialog**

Add to `gloss.rs` (moved from `keymap.rs:1456-1513`):

```rust
pub fn show_amend_dialog(state_rc: &Rc<RefCell<AppState>>) {
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

    let title = gtk4::Label::new(Some("GLOSS PROMPT"));
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

    let hint = gtk4::Label::new(Some("Ctrl+Enter submit  \u{00b7}  Esc cancel"));
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

- [ ] **Step 3: Add add_gloss**

Add to `gloss.rs` (moved from `keymap.rs:1516-1586`):

```rust
pub fn add_gloss(state_rc: &Rc<RefCell<AppState>>, prompt: &str) {
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
    let user_msg = crate::gloss::build_user_message(
        &ctx, Some(&prompt_owned), None,
    );
    let state_for_result = Rc::clone(state_rc);

    glib::spawn_future_local(async move {
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(
                    crate::gloss::USER_QUESTION_PROMPT, &user_msg, &model,
                ).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
                let full_gloss = format!("<gloss>Q: {}</gloss>\n\n{}", prompt_owned, gloss_text);
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let _ = crate::db::queries::save_gloss(
                        &conn, &ctx.hash, &ctx.work_abbrev,
                        &ctx.start_citation, &ctx.end_citation,
                        ctx.act, ctx.scene, &ctx.speaker,
                        &ctx.source_text, &full_gloss,
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
                    &ctx.source_text, &full_gloss, h,
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

- [ ] **Step 4: Build**

Run: `cargo build`

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "Add show_delete_confirmation, show_amend_dialog, add_gloss to actions/gloss"
```

---

### Task 4: Rewire keymap.rs to call actions/gloss.rs and remove old functions

**Files:**
- Modify: `src/input/keymap.rs` (replace function calls, delete old functions)

- [ ] **Step 1: Update handle_gloss_key to call actions::gloss**

In `handle_gloss_key`, update the match arms that call the old functions:

```rust
        "a" => {
            crate::input::actions::gloss::show_amend_dialog(state);
            true
        }
        "c" => {
            crate::input::actions::gloss::copy_gloss_id(state);
            true
        }
        "d" => {
            crate::input::actions::gloss::show_delete_confirmation(state);
            true
        }
```

Update the `navigate_gloss` calls in `handle_gloss_key`:

```rust
            "n" => {
                crate::input::actions::gloss::navigate_gloss(state, -1);
                return true;
            }
            "p" => {
                crate::input::actions::gloss::navigate_gloss(state, 1);
                return true;
            }
```

Update the `navigate_gloss_passage` calls:

```rust
            "n" => {
                crate::input::actions::gloss::navigate_gloss_passage(state, 1);
                return true;
            }
            "p" => {
                crate::input::actions::gloss::navigate_gloss_passage(state, -1);
                return true;
            }
```

- [ ] **Step 2: Update handle_gloss_prompt_key to call actions::gloss::add_gloss**

In `handle_gloss_prompt_key`, replace the call to the old `add_gloss`:

```rust
        if !prompt.trim().is_empty() {
            crate::input::actions::gloss::add_gloss(state, &prompt);
        }
```

- [ ] **Step 3: Delete old functions from keymap.rs**

Remove these functions from `keymap.rs`:
- `navigate_gloss_passage` (lines 617-688)
- `navigate_gloss` (lines 690-710)
- `copy_gloss_id` (lines 712-721)
- `show_delete_confirmation` (lines 723-790)
- `delete_current_gloss` (lines 792-820)
- `show_amend_dialog` (lines 1456-1513)
- `add_gloss` (lines 1516-1586)

- [ ] **Step 4: Build and test**

Run: `cargo build && cargo test`
Expected: Clean build, all tests pass. ~400 lines removed from keymap.rs.

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs src/input/actions/gloss.rs
git commit -m "Rewire gloss key handlers to actions/gloss, remove old inline functions from keymap.rs"
```
