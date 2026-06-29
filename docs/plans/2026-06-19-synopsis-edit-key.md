# Synopsis `E` edit key — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an `E` keybind to the synopsis overlay that sends the current scene synopsis plus a free-text edit instruction to the Claude API, replaces the synopsis with the returned `<p>`-tagged rewrite, persists it to `lit.db`, and makes it revertible with the existing `U` key.

**Architecture:** Reuse the entire `A` ("ask") flow in `src/input/actions/synopsis.rs` — the stacked input card, the async send→save→display→recolor pipeline, the `synopsis_undo` slot, and the `U` undo handler. The only material difference is the **system prompt**: `A` augments/explains, `E` edits literally. A new `SynopsisPromptKind` flag on `AppState` tells the shared Ctrl+Enter submit which path to run, and the shared async body is factored into one helper so `A` and `E` cannot drift.

**Tech Stack:** Rust, GTK4 (`gtk4`/`glib`), Tokio (via `state.tokio_handle`), SQLite (`rusqlite` through `crate::db::queries`), Claude API (`crate::claude::send_message`).

---

## Background the engineer needs

- The synopsis overlay is its own input mode (`InputMode::SynopsisOverlay`). Keys are routed by `handle_synopsis_overlay_key` in `src/input/keymap.rs:977` — a per-mode handler that matches the GTK `key_name` string **directly** (e.g. `"A"`, `"U"`). This is NOT the `keymap.json`/`Action` dispatch the reader uses, so binding `"E"` here does **not** conflict with the reader's `keymap.json` `E`→`SeekLongForward`. **No `keymap.json` change is required.**

- The existing `A` flow lives in `src/input/actions/synopsis.rs`:
  - `show_amend_prompt` — opens the input card, records the target scene.
  - `submit_amend_prompt` — Ctrl+Enter: takes the card text, closes it, calls `amend_synopsis` if non-empty.
  - `amend_synopsis` — builds the user message, shows a loading card, spawns the Claude call, and on success saves to `lit.db`, records undo, updates the cache, redisplays, recolors, logs.
  - `undo_amend` — `U`: restores the pre-edit text (cache + `lit.db` + display).

- The input card is generic: `GlossOverlay::open_ask_card_with(title, hint)`, `take_ask_text()`, `close_ask_card()`, `ask_is_open()` (in `src/ui/gloss_overlay.rs`).

- `AppState` synopsis fields are at `src/app.rs:346-358`; their initializers are at `src/app.rs:1774-1778`.

- The DB prompt override helper is `crate::db::prompts::active_prompt(key) -> Option<String>`; `A` uses key `"synopsis.amend"`. `E` will use `"synopsis.edit"`.

- `cargo test --bins` is the pure-logic suite (no GTK/cage). Build with `cargo build`. **Do not run the app** (`cargo run`) — the user does that.

## File structure

- **`src/app.rs`** — add `SynopsisPromptKind` enum + `synopsis_prompt_kind` field + initializer. (State only.)
- **`src/input/actions/synopsis.rs`** — add `SYNOPSIS_EDIT_PROMPT`, refactor the shared async body into `run_synopsis_revision`, add `edit_synopsis` and `show_edit_prompt`, and wire `synopsis_prompt_kind` into `show_amend_prompt` / `submit_amend_prompt`. (All behavior.)
- **`src/input/keymap.rs`** — add the `"E"` arm to `handle_synopsis_overlay_key`. (One line.)
- **`src/ui/gloss_overlay.rs`** — add `· E edit` to the synopsis footer string. (One line.)
- **`src/ui/keybinds_overlay.rs`** — add the synopsis-context `E` description (via the `update-cairo-keybinds-overlay` skill). (Docs/overlay.)

---

## Task 1: Add `SynopsisPromptKind` state to `AppState`

**Files:**
- Modify: `src/app.rs` (struct near line 358; initializer near line 1778; enum near the other small `pub enum`s, e.g. just above the `AppState` struct)

- [ ] **Step 1: Add the enum.** Place this `pub enum` immediately before the `pub struct AppState` declaration in `src/app.rs` (search for `pub struct AppState`):

```rust
/// Which Claude system prompt the open synopsis input card will use on submit.
/// `A` opens it as `Ask` (augment/explain); `E` opens it as `Edit` (structural
/// edit). Read by `submit_amend_prompt` to dispatch to the right revision path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SynopsisPromptKind {
    Ask,
    Edit,
}
```

- [ ] **Step 2: Add the field.** In `struct AppState`, immediately after the `synopsis_undo` field (currently `src/app.rs:358`), add:

```rust
    /// Which prompt the currently-open synopsis input card will run on submit
    /// (set by `A` -> Ask / `E` -> Edit). Meaningful only while the card is open.
    pub synopsis_prompt_kind: SynopsisPromptKind,
```

- [ ] **Step 3: Add the initializer.** In the `AppState { ... }` constructor, immediately after `synopsis_undo: None,` (currently `src/app.rs:1778`), add:

```rust
        synopsis_prompt_kind: SynopsisPromptKind::Ask,
```

- [ ] **Step 4: Build to verify it compiles.**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished` (no errors). Pre-existing dead-code warnings are fine. (The new enum may warn "never constructed" until Task 2; that is expected and clears in Task 2.)

- [ ] **Step 5: Commit.**

```bash
git add src/app.rs
git commit -m "feat(synopsis): add SynopsisPromptKind state for ask vs edit

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Refactor the shared revision body + add the edit path

This task does the core work: factor `amend_synopsis`'s body into a reusable helper, add the edit system prompt and `edit_synopsis`, add `show_edit_prompt`, and make `show_amend_prompt`/`submit_amend_prompt` aware of the prompt kind.

**Files:**
- Modify: `src/input/actions/synopsis.rs` (the `A` flow, lines ~17-155)

- [ ] **Step 1: Add the edit system prompt constant.** In `src/input/actions/synopsis.rs`, immediately after the existing `SYNOPSIS_AMEND_PROMPT` constant (ends ~line 31), add:

```rust
/// System prompt for the synopsis EDIT call. Unlike the amend prompt (which
/// answers a reader's question by weaving an explanation in), this one applies
/// the reader's edit instruction literally — split/merge paragraphs, reword,
/// tighten, reorder — while keeping the scene's facts accurate. It returns the
/// FULL revised synopsis (not a diff), in the same <p>-tagged format the
/// synopsis card renders.
const SYNOPSIS_EDIT_PROMPT: &str = "\
You are a careful editor revising a Shakespeare scene synopsis. You will be \
given a play, an act and scene, the current synopsis for that scene, and an \
edit instruction from the reader. Apply the edit instruction faithfully and \
literally (for example: split or merge paragraphs, reword a sentence, tighten \
or expand, reorder events). Preserve the factual accuracy of the scene — do \
not invent events that are not already implied by the current synopsis, and do \
not drop plot points unless the instruction tells you to. Do not add a heading, \
preamble, or commentary about what you changed.\n\n\
FORMAT: Return the FULL revised synopsis split into readable paragraphs, each \
wrapped in <p>...</p> tags, like:\n\
<p>First paragraph of the synopsis.</p>\n\
<p>Second paragraph that continues the action.</p>\n\
Output ONLY the <p>-tagged paragraphs, nothing else.";
```

- [ ] **Step 2: Extract the shared async helper.** Replace the entire existing `amend_synopsis` function (currently `src/input/actions/synopsis.rs:65-155`) with the helper below **plus** two thin callers. The helper is `amend_synopsis`'s body verbatim, with three parameters added: the system-prompt key, the compiled-in fallback prompt, and the log verb.

Replace from the doc comment `/// Send the question + current synopsis to Claude, ...` through the closing `}` of `amend_synopsis` with:

```rust
/// Send the instruction + current synopsis to Claude, then show and persist the
/// revised synopsis. Shared by the `A` amend flow and the `E` edit flow; the
/// caller supplies the prompt key / fallback prompt / log verb. Mirrors the
/// gloss add async pattern.
fn run_synopsis_revision(
    state_rc: &Rc<RefCell<AppState>>,
    instruction: &str,
    prompt_key: &'static str,
    fallback_prompt: &'static str,
    log_verb: &'static str,
) {
    let (div1, div2) = state_rc.borrow().synopsis_amend_scene;

    let (work_title, work_abbrev, original, model, tokio_handle, label) = {
        let s = state_rc.borrow();
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return,
        };
        let abbrev = crate::app::base_work_abbrev(&work.abbrev).to_string();
        let original = match s.synopsis_cache.get(&(div1, div2)) {
            Some(t) => t.clone(),
            None => return,
        };
        let label = crate::app::synopsis_label(&s, div1, div2);
        (
            work.title.clone(),
            abbrev,
            original,
            s.config.claude_model.clone(),
            s.tokio_handle.clone(),
            label,
        )
    };
    let user_msg = format!(
        "Play: {}\n{}\n\nCurrent synopsis:\n{}\n\n---\nReader's request: {}",
        work_title, label, original, instruction,
    );

    state_rc.borrow().gloss_overlay.show_loading();

    let system_prompt = crate::db::prompts::active_prompt(prompt_key)
        .unwrap_or_else(|| fallback_prompt.to_string());

    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        // Keep a copy for the DB stamp; `model` itself is moved into the spawn.
        let model_for_db = model.clone();
        let result = tokio_handle
            .spawn(async move {
                crate::claude::send_message(&system_prompt, &user_msg, &model).await
            })
            .await;

        match result {
            Ok(Ok(revised)) => {
                let revised = revised.trim().to_string();
                // Persist (upsert) to lit.db, stamping the authoring model.
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    if let Err(e) = crate::db::queries::save_synopsis(
                        &conn, &work_abbrev, div1, div2, &revised, &model_for_db,
                    ) {
                        crate::logging::log(&format!("SYNOPSIS: save error: {}", e));
                    }
                }
                let mut s = state_for_result.borrow_mut();
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
            }
            Ok(Err(e)) => {
                let mut s = state_for_result.borrow_mut();
                let cw = s.content_hbox.width();
                let h = s.content_hbox.height();
                let root_color = s.theme.root_color.clone();
                s.gloss_overlay
                    .show_synopsis(&label, &format!("Error: {}", e), Some(&root_color), cw, h);
                s.synopsis_overlay_scene = (div1, div2);
                crate::input::actions::gloss::recolor_cached_blocks(&s);
                s.input_mode = crate::app::InputMode::SynopsisOverlay;
                crate::logging::log(&format!("SYNOPSIS: {} error: {}", log_verb, e));
            }
            Err(e) => {
                crate::logging::log(&format!("SYNOPSIS: tokio join error: {}", e));
            }
        }
    });
}

/// Send the question + current synopsis to Claude (augment/explain). `A` path.
pub(crate) fn amend_synopsis(state_rc: &Rc<RefCell<AppState>>, question: &str) {
    run_synopsis_revision(
        state_rc,
        question,
        "synopsis.amend",
        SYNOPSIS_AMEND_PROMPT,
        "amended",
    );
}

/// Send the instruction + current synopsis to Claude (literal edit). `E` path.
pub(crate) fn edit_synopsis(state_rc: &Rc<RefCell<AppState>>, instruction: &str) {
    run_synopsis_revision(
        state_rc,
        instruction,
        "synopsis.edit",
        SYNOPSIS_EDIT_PROMPT,
        "edited",
    );
}
```

Note the only string differences from the original: the user-message field is now `Reader's request:` (covers both a question and an edit instruction), and the success/error log lines use the `log_verb` parameter.

- [ ] **Step 3: Set the prompt kind when each card opens, and dispatch on submit.** Update three functions near the top of the file.

In `show_amend_prompt` (currently `src/app`/`synopsis.rs:36-47`), after `drop(s);`, set the kind to `Ask`. Replace its final two lines:

```rust
    drop(s);
    state_rc.borrow_mut().synopsis_amend_scene = scene;
```

with:

```rust
    drop(s);
    let mut s = state_rc.borrow_mut();
    s.synopsis_amend_scene = scene;
    s.synopsis_prompt_kind = crate::app::SynopsisPromptKind::Ask;
```

Add the new `show_edit_prompt` immediately after `show_amend_prompt`:

```rust
/// Open the stacked "edit" card below the synopsis card (same widget as the ask
/// card, edit framing). On Ctrl+Enter the typed instruction is sent to Claude
/// with the structural-editor prompt. No-op if a card is already open.
pub(crate) fn show_edit_prompt(state_rc: &Rc<RefCell<AppState>>) {
    let s = state_rc.borrow();
    if s.gloss_overlay.ask_is_open() {
        return;
    }
    let scene = s.synopsis_overlay_scene;
    s.gloss_overlay.open_ask_card_with(
        "EDIT THIS SCENE",
        "Describe the edit (split/merge paragraphs, reword, reorder)  \u{00b7}  Tab switch  \u{00b7}  Ctrl+Enter submit  \u{00b7}  Esc cancel",
    );
    drop(s);
    let mut s = state_rc.borrow_mut();
    s.synopsis_amend_scene = scene;
    s.synopsis_prompt_kind = crate::app::SynopsisPromptKind::Edit;
}
```

In `submit_amend_prompt` (currently `synopsis.rs:57-63`), dispatch on the kind. Replace its body:

```rust
pub(crate) fn submit_amend_prompt(state: &Rc<RefCell<AppState>>) {
    let question = state.borrow().gloss_overlay.take_ask_text();
    close_amend_prompt(state);
    if !question.trim().is_empty() {
        amend_synopsis(state, &question);
    }
}
```

with:

```rust
pub(crate) fn submit_amend_prompt(state: &Rc<RefCell<AppState>>) {
    let text = state.borrow().gloss_overlay.take_ask_text();
    let kind = state.borrow().synopsis_prompt_kind;
    close_amend_prompt(state);
    if text.trim().is_empty() {
        return;
    }
    match kind {
        crate::app::SynopsisPromptKind::Ask => amend_synopsis(state, &text),
        crate::app::SynopsisPromptKind::Edit => edit_synopsis(state, &text),
    }
}
```

- [ ] **Step 4: Build to verify it compiles.**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished` (no errors). The Task-1 "enum never constructed" warning is now gone.

- [ ] **Step 5: Run the pure-logic test suite.**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: `test result: ok` (no failures). This change adds no new pure tests (the path mirrors the existing, untested `A` async flow — see Testing note), but the suite must stay green.

- [ ] **Step 6: Commit.**

```bash
git add src/input/actions/synopsis.rs
git commit -m "feat(synopsis): E edit via Claude, sharing the amend pipeline

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Route `E` in the synopsis overlay

**Files:**
- Modify: `src/input/keymap.rs` (the `match key_name` inside `handle_synopsis_overlay_key`, near the `"A"` arm at ~line 1062)

- [ ] **Step 1: Add the `"E"` arm.** In `src/input/keymap.rs`, find the `"A"` arm:

```rust
        "A" => {
            crate::input::actions::synopsis::show_amend_prompt(state);
            true
        }
```

Immediately after it, add:

```rust
        "E" => {
            crate::input::actions::synopsis::show_edit_prompt(state);
            true
        }
```

- [ ] **Step 2: Build to verify it compiles.**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished` (no errors).

- [ ] **Step 3: Commit.**

```bash
git add src/input/keymap.rs
git commit -m "feat(synopsis): bind E to edit prompt in overlay routing

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Add `E edit` to the synopsis footer hint

**Files:**
- Modify: `src/ui/gloss_overlay.rs:978` (the `show_synopsis` footer string)

- [ ] **Step 1: Update the hint string.** In `src/ui/gloss_overlay.rs`, find (this is the line already changed earlier from `⇧Space` to `Shift+Space`):

```rust
        self.hint.set_text("Esc close · j/k block · Space play · n/p scene · Shift+Space synth · Ctrl+g glosses · A ask · U undo");
```

Replace it with (insert `· E edit` after `A ask`):

```rust
        self.hint.set_text("Esc close · j/k block · Space play · n/p scene · Shift+Space synth · Ctrl+g glosses · A ask · E edit · U undo");
```

- [ ] **Step 2: Build to verify it compiles.**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished` (no errors).

- [ ] **Step 3: Commit.** (Also captures the earlier `⇧Space → Shift+Space` change in the same file, which is still uncommitted in the working tree.)

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(synopsis): show E edit (and clarify Shift+Space) in footer

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Document `E` in the Ctrl+/ keybinds overlay

The Ctrl+/ overlay (`src/ui/keybinds_overlay.rs`) is a hand-maintained mirror with no compile-time enforcement. Per the project rule, any keybind change must update both the keycap and the per-key detail panel, and the `update-cairo-keybinds-overlay` skill carries the mandatory three-pass cross-reference.

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (the `E` cap at line 71, and its `describe()` arms)

- [ ] **Step 1: Invoke the skill.** Use the `update-cairo-keybinds-overlay` skill to add the synopsis-context `E` binding. The `E` key already documents reader `seek +3.5` / Shift `+60` and the BCP echo modifiers; the addition is the **synopsis-overlay** behavior: "E (synopsis overlay): open the EDIT card — type an instruction (split/merge paragraphs, reword, reorder) sent to Claude to rewrite the scene synopsis; Ctrl+Enter submits, U reverts. -> synopsis::show_edit_prompt — src/input/actions/synopsis.rs". Follow the skill's three-pass check so no label is blank, no label names the wrong action, and every label has a `describe()` arm.

- [ ] **Step 2: Build to verify it compiles.**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished` (no errors).

- [ ] **Step 3: Commit.**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(keybinds): document synopsis E edit in Ctrl+/ overlay

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Verify and finish the branch

- [ ] **Step 1: Full build + pure-logic tests.**

Run: `cargo build 2>&1 | tail -3 && cargo test --bins 2>&1 | tail -5`
Expected: `Finished`; `test result: ok`.

- [ ] **Step 2: Ask the user to verify on screen.** The acceptance criterion is visual (the card opens, the edited paragraphs render, `U` reverts). The agent cannot reliably launch cage from the live dwl session, so request user verification with these exact steps:

  1. `cargo run`, open a play, navigate to a scene with a synopsis.
  2. Press `h` to open the synopsis overlay; confirm the footer shows `A ask · E edit · U undo`.
  3. Press `E`; confirm the card titled `EDIT THIS SCENE` opens.
  4. Type e.g. `split the first paragraph into two after the first sentence`; press `Ctrl+Enter`.
  5. Confirm the synopsis is rewritten (loading card → edited `<p>` paragraphs).
  6. Press `U`; confirm it reverts to the pre-edit text.
  7. Optionally press `A` and confirm the ask flow still works (augment/explain), proving the shared helper didn't regress it.

- [ ] **Step 3: Finish the branch per project rule.** Once the user confirms (and only then), follow the "Finishing a Branch" rule in `~/CLAUDE.md`: verify tree clean, `git checkout master`, `git merge --no-ff`, re-verify build/tests, `git push origin master`, delete the feature branch. (If this work was done directly on `master`, skip the merge and just push after user confirmation.)

---

## Testing notes

- **No new pure unit tests.** This feature is system-prompt selection plus an async GTK pipeline that mirrors the existing, deliberately-untested `A` path (it touches `glib::spawn_future_local`, `gloss_overlay`, and a live Claude call — none unit-testable without GTK + network). `cargo test --bins` must stay green but gains no case here, consistent with how `A` was added.
- **Visual acceptance** is the real check (Task 6, Step 2), done by the user.

## Self-review

- **Spec coverage:** `E` bind (Task 3), edit system prompt + `edit_synopsis` (Task 2), `SynopsisPromptKind` flag (Task 1), shared helper refactor (Task 2), shared `U` undo (unchanged — `run_synopsis_revision` writes `synopsis_undo`, Task 2), footer `· E edit` (Task 4), Ctrl+/ overlay (Task 5), no `keymap.json` change (documented in Background). All spec sections covered.
- **Placeholder scan:** none — every code step shows complete code; the only "skill-driven" step (Task 5) names the exact label text and `describe()` content to add.
- **Type/name consistency:** `SynopsisPromptKind::{Ask,Edit}`, `synopsis_prompt_kind`, `run_synopsis_revision`, `edit_synopsis`, `show_edit_prompt`, `amend_synopsis`, `submit_amend_prompt` are used identically across Tasks 1-3. Prompt key `"synopsis.edit"` matches the constant `SYNOPSIS_EDIT_PROMPT`.
