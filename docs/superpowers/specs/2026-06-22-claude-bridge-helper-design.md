# Claude async-bridge helper — design

## Goal

Remove the duplicated Claude-request boilerplate across the overlay bridge sites
(`gloss::add_gloss`, `gloss::edit_gloss`, `synopsis` amend/edit, `journal` Q&A),
and collapse the ~40 byte-identical "persist + render gloss" lines that
`add_gloss` and `edit_gloss` share. This is audit opportunity #7.

## Background — the duplication today

Each site follows the same async-bridge shape:

```rust
state_rc.borrow().<overlay>.show_loading();
let state_for_result = Rc::clone(state_rc);
glib::spawn_future_local(async move {
    let model_for_db = model.clone();
    let result = tokio_handle
        .spawn(async move {
            crate::gloss::call_claude_with_prompt(&system_prompt, &user_msg, &model).await
        })
        .await;
    match result {
        Ok(Ok(reply)) => { /* SITE-SPECIFIC success body */ }
        Ok(Err(e))    => { /* render "Error: {e}" into this overlay + log */ }
        Err(e)        => { /* render "Internal error — try again." + log */ }
    }
});
```

The **outer shape** (spawn + the two error arms) is identical across the three
overlay sites; only the `Ok(Ok)` success body differs. Additionally,
`add_gloss` (`gloss.rs` ~668–811) and `edit_gloss` (~813–927) have a
**byte-for-byte-identical** success body from `let mut new_gloss_id: i64 = -1;`
through `recolor_cached_blocks(&s);` — `save_gloss` → `find_glosses_by_start` →
`show_gloss_with_color` → set position/citation/`gloss_list`/`gloss_index`/
`gloss_active_voice` → recolor. They differ only in: the prompt-selection
`match`, the user-message builder, the `<gloss>…</gloss>` header text, and one
log string.

The three overlay bridge sites (current line refs, to re-confirm at implementation):
- `gloss::add_gloss` — `gloss.rs` ~733; success → persist+render gloss; error → `gloss_overlay.show("Error: …", "")`.
- `gloss::edit_gloss` — `gloss.rs` ~849; success → persist+render gloss; error → `gloss_overlay.show("Error: …", "")`.
- `synopsis` amend/edit — `synopsis.rs` ~158; success → upsert `save_synopsis` + `show_synopsis(label, text, color, cw, h)` + cache/scene update; error → `show_synopsis(label, "Error: …", color, cw, h)`.
- `journal` Q&A — `journal.rs` ~266; success → `save_journal_page`/`update_journal_page` + reload pages + `render_current`; error → `journal_overlay.show_message("Error: …")`.

## Components

### Component A — `run_claude_request` (generic bridge)

A `pub(crate)` free function in a **new module** `src/input/actions/claude_bridge.rs`
(register with `pub(crate) mod claude_bridge;` in `src/input/actions/mod.rs`). It
owns the spawn and the two error arms; callers supply prompt/message/model and
two closures.

```rust
use crate::app::AppState;
use std::cell::RefCell;
use std::rc::Rc;
use gtk4::glib;

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

**Decisions:**
- `system_prompt` is `String` (not `&'static str`), so the synopsis DB-backed
  `active_prompt(prompt_key)` path works unchanged.
- **Uniform log prefix `CLAUDE:`** for both error arms (per-site prefixes like
  `GLOSS: add error` are NOT preserved — logs are diagnostic-only). This is the
  one accepted log-wording change.
- The user-visible error message is built by the bridge (`"Error: {e}"` /
  `"Internal error — try again."`) and handed to `on_error`, which renders it in
  the site's own overlay — so the on-screen recovery is preserved exactly.

### Component B — `persist_and_render_gloss` (gloss twin body)

A private free function in `src/input/actions/gloss.rs` holding the ~40 identical
lines.

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

The implementer must verify this matches the live `add_gloss`/`edit_gloss`
success bodies verbatim before extracting (the `gloss_type` and `model_for_db`
are passed as `&str`; the original used owned `gloss_type_owned` / `model_for_db`).

## Call-site shape after both

`add_gloss` keeps its preamble (read ctx/model/handle, `show_loading()`, the
`match ctx.gloss_type` that yields `(system_prompt, user_msg, gloss_type_str)`),
clones what the closure needs (`ctx`, `model`, `gloss_type_owned`, `prompt_owned`,
`is_inner_monologue`), then:

```rust
let model_for_db = model.clone();
run_claude_request(
    state_rc,
    system_prompt.to_string(),
    user_msg,
    model,
    move |st, gloss_text| {
        let verified = if is_inner_monologue {
            crate::gloss::verify_echo_citations(&gloss_text, &ctx.work_abbrev)
        } else {
            gloss_text.clone()
        };
        let full_gloss = if is_inner_monologue {
            format!("<gloss>Inner voice from:</gloss>\n\n{}\n\n{}", prompt_owned, verified)
        } else {
            format!("<gloss>Q: {}</gloss>\n\n{}", prompt_owned, verified)
        };
        persist_and_render_gloss(st, &ctx, &full_gloss, &gloss_type_owned, &model_for_db,
            &format!("GLOSS: added new {} gloss", gloss_type_owned));
    },
    |st, msg| { st.borrow().gloss_overlay.show(msg, ""); },
);
```

`edit_gloss` is identical except: user-message via `build_edit_gloss_message`,
headers `"<gloss>Re-glossed with:</gloss>…"` / `"<gloss>Edit context:</gloss>…"`,
and log `"GLOSS: edited {} gloss (added new)"`.

`synopsis` and `journal` call `run_claude_request` with their own `on_success`
(synopsis: `save_synopsis` upsert + `synopsis_undo`/`synopsis_cache` update +
`show_synopsis` + recolor + `input_mode = SynopsisOverlay`; journal: band-based
`save_journal_page`/`update_journal_page` + reload pages + `render_current`) and
their own `on_error` (synopsis: `show_synopsis(label, msg, color, cw, h)` +
restore `synopsis_overlay_scene`/`input_mode`; journal: `show_message(msg)`).
These success/error bodies move verbatim from the existing `Ok(Ok)` / `Ok(Err)`
arms into the closures.

## Ownership note

Today each site clones `model_for_db` inside the `async move` block because
`model` is moved into the inner `tokio_handle.spawn`. With the bridge owning the
spawn, **each caller clones `model` before calling `run_claude_request`** and
moves that clone into its `on_success` closure (which is `'static` and captures
owned data: `ctx`, `model_for_db`, `prompt_owned`, `gloss_type_owned`, flags).
The `ctx` is already `.clone()`d in every site's preamble; no new clone semantics
beyond relocating where the clone is captured.

**No double-borrow:** `run_claude_request` passes `&state_for_result` to the
closures and holds NO outstanding `borrow()`/`borrow_mut()` when it calls them, so
a closure that does `st.borrow_mut()` (e.g. synopsis/journal success bodies) is
safe — exactly as the original inline `match` arms were.

## Global Constraints

- **No behavior change** except the accepted log-prefix unification to `CLAUDE:`.
  Every on-screen result (success render AND error render) in every site must be
  identical to today. The reviewer verifies the success/error closure bodies
  against the original `Ok(Ok)`/`Ok(Err)`/`Err` arms line-by-line.
- **No keybind change** → do NOT touch `keybinds_overlay.rs`, `keymap_config.rs`,
  `keymap.json`.
- New module `claude_bridge.rs` registered in `src/input/actions/mod.rs`.
- `cargo build` + `cargo clippy` clean; `cargo test --bins` green.
- Bash/CLI rules (CLAUDE.md): `rg`/`fd` not `grep`/`find`; `\mv -f`/`\cp -f`/
  `command rm -f` to bypass interactive aliases.

## Testing

These bodies need a GTK `AppState` and are not unit-testable in this harness; a
fake test would assert nothing (forbidden by the review rubric). Therefore **no
new unit tests**. Verification = build + clippy + `cargo test --bins` green +
reviewer equivalence trace + the user's cage pass:

- **gloss**: add a gloss (`A` + question), edit a gloss (`E` + paste) — both
  render the new gloss; force an API error if feasible and confirm the
  `"Error: …"` card shows (not a stuck spinner).
- **synopsis**: amend (`A`) and edit (`E`) a synopsis — both re-render; error →
  `"Error: …"` in the synopsis card.
- **journal**: ask (`A`) and edit (`E`) a Q&A page — both render; error →
  `"Error: …"` message.

Per the headless-verification protocol, the agent cannot reliably drive cage on
the live dwl session, so this is handed to the user.

## Out of scope

- `request_ipa_then_apply` (`gloss.rs` ~668, the IPA-fix path) also uses
  `call_claude_with_prompt` but its success body is `apply_ipa_fix` (no overlay
  render) and its error handling differs. It MAY adopt `run_claude_request` later
  but is **not** required here — keep scope to the three overlay bridges.
- The Voyage `embed_query` and ElevenLabs `synthesize`/`list_voices` spawns —
  different result types and downstream handling; not Claude bridges.
- Other audit refactors (#5 footer/hint builder, #6 Picker trait, #8 sentinel
  constants) — each its own spec.
