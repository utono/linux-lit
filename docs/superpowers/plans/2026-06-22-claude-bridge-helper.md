# Claude async-bridge helper — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract a generic `run_claude_request` async-bridge helper and a `persist_and_render_gloss` helper, then route the four Claude overlay bridge sites (gloss add/edit, synopsis, journal) through them, with no behavior change beyond a unified `CLAUDE:` log prefix.

**Architecture:** New module `src/input/actions/claude_bridge.rs` holds `run_claude_request` (owns the `spawn_future_local` + tokio spawn + `Ok(Err)`/`Err` recovery arms). `src/input/actions/gloss.rs` gains a private `persist_and_render_gloss` for the byte-identical add/edit success body. Each call site keeps its preamble and moves its success/error bodies into closures.

**Tech Stack:** Rust, GTK4 (`gtk4::glib::spawn_future_local`), Tokio (`tokio::runtime::Handle::spawn`), `Rc<RefCell<AppState>>`.

**Spec:** `docs/superpowers/specs/2026-06-22-claude-bridge-helper-design.md`

## Global Constraints

- **No behavior change** except the accepted log-prefix unification to `CLAUDE:`. Every on-screen result (success AND error render) in every site must be byte-identical to today. Reviewer verifies closure bodies line-by-line against the original `Ok(Ok)`/`Ok(Err)`/`Err` arms.
- `call_claude_with_prompt` (`gloss.rs:702`) is a thin pass-through to `claude::send_message` (`claude.rs:22`) — identical signature and `Result<String, ClaudeError>` return. Routing synopsis (which currently calls `send_message` directly) through `call_claude_with_prompt` via the bridge is therefore behaviorally identical. **This is not a behavior change.**
- **No keybind change** → do NOT touch `src/ui/keybinds_overlay.rs`, `src/input/keymap_config.rs`, or `keymap.json`.
- **No new unit tests** — these bodies need a GTK `AppState`; a fake test would assert nothing (plan-justified). Verification = `cargo build` + `cargo clippy` clean + `cargo test --bins` green + reviewer equivalence + user cage pass.
- Bash/CLI rules (CLAUDE.md): use `rg`/`fd`, not `grep`/`find`; bypass interactive `mv`/`cp`/`rm` aliases with `\mv -f`/`\cp -f`/`command rm -f`.
- `request_ipa_then_apply`, Voyage `embed_query`, and ElevenLabs spawns are **out of scope** — do not touch them.

---

### Task 1: Add `run_claude_request` bridge module

**Files:**
- Create: `src/input/actions/claude_bridge.rs`
- Modify: `src/input/actions/mod.rs` (register module)

**Interfaces:**
- Consumes: `crate::gloss::call_claude_with_prompt(&str, &str, &str) -> Result<String, ClaudeError>`; `AppState.tokio_handle: tokio::runtime::Handle`.
- Produces: `pub(crate) fn run_claude_request(state_rc: &Rc<RefCell<AppState>>, system_prompt: String, user_msg: String, model: String, on_success: impl Fn(&Rc<RefCell<AppState>>, String) + 'static, on_error: impl Fn(&Rc<RefCell<AppState>>, &str) + 'static)`.

- [ ] **Step 1: Create the module file**

Create `src/input/actions/claude_bridge.rs`:

```rust
use crate::app::AppState;
use gtk4::glib;
use std::cell::RefCell;
use std::rc::Rc;

/// Spawn a Claude `call_claude_with_prompt` request off the GTK thread, then
/// dispatch back on the main loop: `on_success(state, reply)` on success, or
/// `on_error(state, msg)` on API error / tokio join panic (so the overlay is
/// never left stuck on the loading card). Callers must call the overlay's
/// `show_loading()` BEFORE invoking this.
///
/// `model` is moved into the spawned future; if the success body needs the
/// model id (e.g. to stamp a DB row) the caller captures its own clone in the
/// `on_success` closure.
pub(crate) fn run_claude_request(
    state_rc: &Rc<RefCell<AppState>>,
    system_prompt: String,
    user_msg: String,
    model: String,
    on_success: impl Fn(&Rc<RefCell<AppState>>, String) + 'static,
    on_error: impl Fn(&Rc<RefCell<AppState>>, &str) + 'static,
) {
    let tokio_handle = state_rc.borrow().tokio_handle.clone();
    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(&system_prompt, &user_msg, &model).await
            })
            .await;
        match result {
            Ok(Ok(reply)) => on_success(&state_for_result, reply),
            Ok(Err(e)) => {
                crate::logging::log(&format!("CLAUDE: API error: {}", e));
                on_error(&state_for_result, &format!("Error: {}", e));
            }
            Err(e) => {
                crate::logging::log(&format!("CLAUDE: tokio join error: {}", e));
                on_error(&state_for_result, "Internal error \u{2014} try again.");
            }
        }
    });
}
```

- [ ] **Step 2: Register the module**

In `src/input/actions/mod.rs`, add `pub(crate) mod claude_bridge;` alongside the other `mod` declarations (match the existing visibility/ordering style — read the file first and place it in the alphabetical/grouped position the file uses).

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: `Finished`. `run_claude_request` is unused so far → expect a `dead_code`/unused warning for it; that is acceptable at this task boundary (Tasks 2–3 consume it). Do NOT add `#[allow(dead_code)]` — the warning clears when Task 2 lands.

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/claude_bridge.rs src/input/actions/mod.rs
git commit -m "refactor(bridge): add run_claude_request async-bridge helper

New src/input/actions/claude_bridge.rs owning the spawn_future_local +
tokio spawn + Ok(Err)/Err recovery arms for Claude overlay requests.
Unused until the gloss/synopsis/journal sites adopt it (Tasks 2-3).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 2: Extract `persist_and_render_gloss`; convert `add_gloss` + `edit_gloss`

**Files:**
- Modify: `src/input/actions/gloss.rs` (add `persist_and_render_gloss`; rewrite `add_gloss` ~668–811 and `edit_gloss` ~813–927)

**Interfaces:**
- Consumes: `run_claude_request` (Task 1); `crate::gloss::GlossContext` (fields `hash`, `work_abbrev`, `start_citation`, `end_citation`, `act`, `scene`, `speaker`, `source_text`, `gloss_type`; methods `source_line_pairs()`); `save_gloss`, `find_glosses_by_start`; `recolor_cached_blocks`; `verify_echo_citations`; `build_user_message`, `build_edit_gloss_message`, `build_inner_monologue_add_message`; the prompt consts (`INNER_MONOLOGUE_ADD_PROMPT`, `READER_GLOSS_QUESTION_PROMPT`, `USER_QUESTION_PROMPT`, `INNER_MONOLOGUE_EDIT_PROMPT`, `READER_GLOSS_EDIT_PROMPT`, `EDIT_GLOSS_PROMPT`).
- Produces: `fn persist_and_render_gloss(state_rc: &Rc<RefCell<AppState>>, ctx: &crate::gloss::GlossContext, full_gloss: &str, gloss_type: &str, model_for_db: &str, log_msg: &str)`.

- [ ] **Step 1: Read the current `add_gloss` and `edit_gloss` to confirm the byte-identical block**

Re-read `gloss.rs` ~668–927. Confirm the success body from `let mut new_gloss_id: i64 = -1;` through `recolor_cached_blocks(&s);` is identical between the two (modulo the trailing log string and the `full_gloss`/header built just above it). Treat the live code as source of truth.

- [ ] **Step 2: Add `persist_and_render_gloss`**

Add this private fn to `gloss.rs` (place it near `add_gloss`/`edit_gloss`):

```rust
/// Persist a freshly composed gloss, reload the start-citation gloss list,
/// select the new row, and render it into the gloss overlay. Shared by
/// `add_gloss` and `edit_gloss` (their success bodies were byte-identical here).
fn persist_and_render_gloss(
    state_rc: &Rc<RefCell<AppState>>,
    ctx: &crate::gloss::GlossContext,
    full_gloss: &str,
    gloss_type: &str,
    model_for_db: &str,
    log_msg: &str,
) {
    let mut new_gloss_id: i64 = -1;
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        if let Ok(id) = crate::db::queries::save_gloss(
            &conn, &ctx.hash, &ctx.work_abbrev,
            &ctx.start_citation, &ctx.end_citation,
            ctx.act, ctx.scene, &ctx.speaker,
            &ctx.source_text, full_gloss,
            gloss_type, model_for_db,
        ) {
            new_gloss_id = id;
        }
    }

    let all = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::queries::find_glosses_by_start(
                &conn, &ctx.work_abbrev, &ctx.start_citation,
                &["teacher-generic", "inner-monologue", "reader-gloss"],
            ).ok()
        })
        .unwrap_or_default();

    let new_idx = all.iter().position(|g| g.gloss_id == new_gloss_id).unwrap_or(0);

    let mut s = state_rc.borrow_mut();
    let cw = s.content_hbox.width();
    let h = s.content_hbox.height();
    let pairs = ctx.source_line_pairs();
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, full_gloss, cw, h,
        Some(&s.theme.root_color), &pairs,
    );
    s.gloss_overlay.set_position(new_idx, all.len());
    s.gloss_overlay.set_citation(&ctx.start_citation, &ctx.end_citation);
    s.gloss_list = all;
    s.gloss_index = new_idx;
    s.gloss_active_voice = 0;
    recolor_cached_blocks(&s);
    crate::logging::log(log_msg);
}
```

**Verify against the live code:** the `save_gloss` argument order, the gloss-type slice in `find_glosses_by_start`, and the exact `gloss_overlay` calls must match the originals. If the live code differs from the snippet above, the live code wins — adjust the helper to match it exactly.

- [ ] **Step 3: Rewrite `add_gloss`'s `glib::spawn_future_local(...)` block**

Keep `add_gloss`'s preamble unchanged through the `let (system_prompt, user_msg, gloss_type_str) = match ctx.gloss_type.as_str() { ... };` and the `let state_for_result = ...; let gloss_type_owned = ...;` lines. Replace the entire `glib::spawn_future_local(async move { ... });` block (the spawn + match) with:

```rust
    let model_for_db = model.clone();
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        system_prompt.to_string(),
        user_msg,
        model,
        move |st, gloss_text| {
            let verified_text = if is_inner_monologue {
                crate::gloss::verify_echo_citations(&gloss_text, &ctx.work_abbrev)
            } else {
                gloss_text.clone()
            };
            let full_gloss = if is_inner_monologue {
                format!("<gloss>Inner voice from:</gloss>\n\n{}\n\n{}", prompt_owned, verified_text)
            } else {
                format!("<gloss>Q: {}</gloss>\n\n{}", prompt_owned, verified_text)
            };
            persist_and_render_gloss(
                st, &ctx, &full_gloss, &gloss_type_owned, &model_for_db,
                &format!("GLOSS: added new {} gloss", gloss_type_owned),
            );
        },
        |st, msg| {
            st.borrow().gloss_overlay.show(msg, "");
        },
    );
```

Note: `state_for_result` is no longer needed in `add_gloss` (the bridge clones `state_rc` internally). Remove the now-unused `let state_for_result = Rc::clone(state_rc);` line if it becomes unused; `cargo build` will flag it.

- [ ] **Step 4: Rewrite `edit_gloss`'s `glib::spawn_future_local(...)` block**

Identical to Step 3 except the success closure builds the edit headers and log:

```rust
    let model_for_db = model.clone();
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        system_prompt.to_string(),
        user_msg,
        model,
        move |st, gloss_text| {
            let verified_text = if is_inner_monologue {
                crate::gloss::verify_echo_citations(&gloss_text, &ctx.work_abbrev)
            } else {
                gloss_text.clone()
            };
            let full_gloss = if is_inner_monologue {
                format!("<gloss>Re-glossed with:</gloss>\n\n{}\n\n{}", pasted_owned, verified_text)
            } else {
                format!("<gloss>Edit context:</gloss>\n\n{}\n\n{}", pasted_owned, verified_text)
            };
            persist_and_render_gloss(
                st, &ctx, &full_gloss, &gloss_type_owned, &model_for_db,
                &format!("GLOSS: edited {} gloss (added new)", gloss_type_owned),
            );
        },
        |st, msg| {
            st.borrow().gloss_overlay.show(msg, "");
        },
    );
```

Remove the now-unused `state_for_result` line if `cargo build` flags it.

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: `Finished`, no errors, and the Task-1 `run_claude_request` unused-warning now cleared. Fix any unused-variable warnings introduced (e.g. a leftover `state_for_result`).

- [ ] **Step 6: Clippy**

Run: `cargo clippy`
Expected: no new warnings.

- [ ] **Step 7: Tests stay green**

Run: `cargo test --bins`
Expected: same pass count as before (413 at last check), 0 failed.

- [ ] **Step 8: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "refactor(gloss): add/edit via run_claude_request + shared persist/render

Extract the byte-identical add_gloss/edit_gloss success body into
persist_and_render_gloss, and route both through run_claude_request.
On-screen result unchanged; error log prefix now CLAUDE:.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 3: Convert `synopsis` and `journal` bridges

**Files:**
- Modify: `src/input/actions/synopsis.rs` (the bridge at ~157–227)
- Modify: `src/input/actions/journal.rs` (the bridge at ~264–335)

**Interfaces:**
- Consumes: `run_claude_request` (Task 1). Synopsis success uses `save_synopsis`, `gloss_overlay.show_synopsis`, `recolor_cached_blocks`, `AppState` fields `synopsis_undo`/`synopsis_cache`/`synopsis_overlay_scene`/`input_mode`. Journal success uses `save_journal_page`/`update_journal_page`, `find_work_pages`/`find_journal_pages`, `render_current`, `JournalBand`, `JournalPromptMode`.
- Produces: nothing new.

- [ ] **Step 1: Read both current bridges to confirm the bodies**

Re-read `synopsis.rs` ~157–227 and `journal.rs` ~264–335. Treat the live `Ok(Ok)`/`Ok(Err)`/`Err` arms as the source of truth for the closure bodies below.

- [ ] **Step 2: Rewrite the synopsis bridge**

In the synopsis fn, keep the preamble through `let system_prompt = crate::db::prompts::active_prompt(prompt_key).unwrap_or_else(|| fallback_prompt.to_string());`. Replace the `let state_for_result = Rc::clone(state_rc); glib::spawn_future_local(async move { ... });` block with:

```rust
    let model_for_db = model.clone();
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        system_prompt,
        user_msg,
        model,
        move |st, revised| {
            let revised = revised.trim().to_string();
            // Persist (upsert) to lit.db, stamping the authoring model.
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                if let Err(e) = crate::db::queries::save_synopsis(
                    &conn, &work_abbrev, div1, div2, &revised, &model_for_db,
                ) {
                    crate::logging::log(&format!("SYNOPSIS: save error: {}", e));
                }
            }
            let mut s = st.borrow_mut();
            // Remember the pre-revision text so `U` can revert this edit.
            s.synopsis_undo = Some(((div1, div2), original.clone()));
            s.synopsis_cache.insert((div1, div2), revised.clone());
            let cw = s.content_hbox.width();
            let h = s.content_hbox.height();
            let root_color = s.theme.root_color.clone();
            s.gloss_overlay.show_synopsis(&label, &revised, Some(&root_color), cw, h);
            s.synopsis_overlay_scene = (div1, div2);
            crate::input::actions::gloss::recolor_cached_blocks(&s);
            s.input_mode = crate::app::InputMode::SynopsisOverlay;
            crate::logging::log(&format!(
                "SYNOPSIS: {} {} ({},{})",
                log_verb, work_abbrev, div1, div2
            ));
        },
        move |st, msg| {
            let mut s = st.borrow_mut();
            let cw = s.content_hbox.width();
            let h = s.content_hbox.height();
            let root_color = s.theme.root_color.clone();
            s.gloss_overlay.show_synopsis(&label_err, msg, Some(&root_color), cw, h);
            s.synopsis_overlay_scene = (div1, div2);
            crate::input::actions::gloss::recolor_cached_blocks(&s);
            s.input_mode = crate::app::InputMode::SynopsisOverlay;
        },
    );
```

**Capture note:** both closures need `label`, `work_abbrev`, `div1`, `div2`. `label` is a `String` captured by-move into the success closure; the error closure also needs it — clone it into a second binding (`let label_err = label.clone();`) before the call, OR capture `label` by reference is impossible across `'static` closures, so make `label_err` a clone used in the error closure (shown above). Add `let label_err = label.clone();` just before `run_claude_request`. Likewise `work_abbrev` is used only in success here; `div1`/`div2` are `Copy`. Verify which originals each arm referenced and clone exactly those that both closures need.

**Behavior note:** the original logged `SYNOPSIS: <verb> error` / `SYNOPSIS: tokio join error` in the error arms; the bridge now logs the uniform `CLAUDE:` prefix instead. The on-screen `show_synopsis(..., "Error: {e}" / "Internal error — try again.", ...)` is preserved (the bridge passes the same message string). This is the accepted log change.

- [ ] **Step 3: Rewrite the journal bridge**

In the journal fn, keep the preamble through the `user_msg` construction and `let question_owned = question.to_string();`. Replace the `let state_for_result = Rc::clone(state_rc); glib::spawn_future_local(async move { ... });` block with a `run_claude_request` call whose success closure contains the exact body from the original `Ok(Ok(answer))` arm (the band-based `save_journal_page`/`update_journal_page`, the page reload, `render_current`, and the `JOURNAL: saved page` log) and whose error closure renders `st.borrow().journal_overlay.show_message(msg)`:

```rust
    let model_for_db = model.clone();
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        crate::gloss::JOURNAL_QA_PROMPT.to_string(),
        user_msg,
        model,
        move |st, answer| {
            // For a save, the scope and (div1,div2) come from the band.
            let (scope, sdiv1, sdiv2) = match band {
                JournalBand::Work => ("work", -1_i64, -1_i64),
                JournalBand::Scene(d1, d2) => ("scene", d1, d2),
            };
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let write_result = if mode == JournalPromptMode::Edit && edit_id >= 0 {
                    crate::db::journal::update_journal_page(
                        &conn, edit_id, &question_owned, &answer, &model_for_db,
                    )
                } else {
                    crate::db::journal::save_journal_page(
                        &conn, &work_abbrev, sdiv1, sdiv2, &question_owned, &answer,
                        &model_for_db, scope,
                    )
                    .map(|_| ())
                };
                if let Err(e) = write_result {
                    crate::logging::log(&format!("JOURNAL: db write failed: {}", e));
                }
            }
            let pages = crate::db::queries::open_db()
                .ok()
                .and_then(|conn| match band {
                    JournalBand::Work => {
                        crate::db::journal::find_work_pages(&conn, &work_abbrev).ok()
                    }
                    JournalBand::Scene(d1, d2) => {
                        crate::db::journal::find_journal_pages(&conn, &work_abbrev, d1, d2).ok()
                    }
                })
                .unwrap_or_default();
            let new_index = if mode == JournalPromptMode::Edit && edit_id >= 0 {
                pages.iter().position(|p| p.id == edit_id).unwrap_or(0)
            } else {
                pages.len().saturating_sub(1)
            };
            let mut s = st.borrow_mut();
            s.journal_band = band;
            s.journal_page_index = new_index;
            render_current(&mut s);
            crate::logging::log("JOURNAL: saved page");
        },
        move |st, msg| {
            st.borrow().journal_overlay.show_message(msg);
        },
    );
```

This is the verbatim original `Ok(Ok(answer))` body with `st` substituted for
`state_for_result`. The closure captures by-move: `band` (`JournalBand` is
`Copy` — it is read twice, matching the original `async move`), `mode`,
`edit_id`, `work_abbrev`, `question_owned`, `model_for_db`. The system prompt
`crate::gloss::JOURNAL_QA_PROMPT` matches the original (`journal.rs:271`).
The implementer must confirm against live code at Step 1 and adjust if the live
body differs.

**Behavior note:** the original journal `Ok(Err)` arm logged `JOURNAL: claude error` and the `Err` arm logged `JOURNAL: tokio join error` + `show_message("Internal error — try again.")`. The bridge now logs `CLAUDE:` and renders the same `show_message` strings. Accepted log change; on-screen unchanged.

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: `Finished`, no errors. Resolve any unused-import warnings — e.g. if `glib::spawn_future_local` is no longer used in `synopsis.rs`/`journal.rs`, remove the now-unused `use` (only if the file has no other `spawn_future_local` use — check with `rg "spawn_future_local" src/input/actions/synopsis.rs`).

- [ ] **Step 5: Clippy**

Run: `cargo clippy`
Expected: no new warnings.

- [ ] **Step 6: Tests stay green**

Run: `cargo test --bins`
Expected: same pass count, 0 failed.

- [ ] **Step 7: Commit**

```bash
git add src/input/actions/synopsis.rs src/input/actions/journal.rs
git commit -m "refactor(synopsis,journal): route Claude bridges via run_claude_request

Move the synopsis amend/edit and journal Q&A success/error bodies into
run_claude_request closures. On-screen results unchanged; error log prefix
now CLAUDE:. Synopsis now calls Claude via call_claude_with_prompt (a thin
pass-through to send_message) — behaviorally identical.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

## Verification (after all tasks)

- `cargo build` + `cargo clippy` clean, `cargo test --bins` green.
- Reviewer confirms each closure body is line-by-line equivalent to the original arms (the synopsis `label`/`work_abbrev` capture split and the journal verbatim body are the highest-risk spots).
- **User cage pass** (runtime acceptance): gloss add (`A`)/edit (`E`); synopsis amend (`A`)/edit (`E`); journal ask (`A`)/edit (`E`) — each renders its result; on a forced API error each shows its `"Error: …"` card (no stuck spinner).
