# Journal question-improve Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Improve the reader's question phrasing — automatically at entry creation, and on demand via `R` (which now first picks a target: question / answer / both; the question path auto-improves the phrasing and regenerates the answer).

**Architecture:** A separate `improve_question` Claude primitive (leaving the tuned prose answer prompt untouched). Creation calls it before answering. `R` opens a q/a/both chooser (confirm-mode idiom). The question/both paths reuse `rewrite_with_claude` (entry-anchored grounding + id-update + filter-aware re-render) to (re)generate the answer for the improved question — NOT `ask_claude` (which anchors on the origin band, wrong under a filter).

**Tech Stack:** Rust, gtk4-rs, the reader's `run_claude_request` async bridge, `active_prompt` (api_prompts), cargo test, headless cage/grim.

## Global Constraints

- Design: `docs/plans/2026-07-13-journal-question-improve-design.md`.
- Separate `improve_question` call (Option B). Do NOT restructure `journal_qa_prompt` into JSON.
- `improve_question` returns ONLY the improved question text; fence-stripped, trimmed; **falls back to the original question on empty/error — never lose the question.**
- `R` target chooser: question | answer | both. answer → existing flow; question → improve Q + regenerate answer; both → improve Q + free-instruction answer rewrite.
- Creation auto-improves the question, then answers the improved Q, saves both. Empty/failed improve → save the raw Q.
- Filter-aware: resolve the entry via `displayed_journal_page`; the question/both re-answer path reuses `rewrite_with_claude` (already id-update + `render_filtered_match` under a filter).
- No double `Q:` prefix (`prefix_question`).
- Build: `cargo build`. Test: `cargo test <name>` (bin-only crate). Headless: CLAUDE.md protocol.

## Verified pre-facts (do not re-derive)

- `ask_claude(state, question)` (journal.rs:1569) frames the answer from the CURRENT `journal_band`/`current_work`/`return_pos` (origin band) — WRONG for a filtered cross-work entry.
- `rewrite_with_claude(state, id, question, answer, instruction)` (journal.rs:1420) is entry-anchored (`band_for_rewrite(p)` + `rewrite_context`), updates by id, and (after prior work) re-renders `render_filtered_match` under a filter. REUSE THIS for the re-answer.
- `rewrite_user_message(context, question, answer, instruction)` builds the answer-rewrite prompt.
- Confirm-mode idiom: `show_delete_confirmation` → `InputMode::DeleteConfirm` → `handle_delete_confirm_key` (single-key modal). Mirror for the R-target chooser.
- `spawn_retag` shows the api_prompts+fallback pattern: `active_prompt(key).unwrap_or_else(|| FALLBACK.to_string())`.
- `displayed_journal_page(&s)` returns the filter match (under `f`) else the band page.
- `strip_code_fence` exists in `src/journal_tags.rs` (reuse for the improve reply).

## File Structure

- `src/input/actions/journal.rs` — `improve_question` + `FALLBACK_IMPROVE_QUESTION_PROMPT`; the R-target chooser openers + the question/both paths; create-flow change.
- `src/input/keymap.rs` — `R` opens the chooser; `handle_rewrite_target_key` (q/a/both); route.
- `src/app/mod.rs` — `InputMode::RewriteTargetChoice`.
- `src/journal_tags.rs` — make `strip_code_fence` `pub(crate)` (reuse for the improve reply).

---

## Task 1: `strip_code_fence` reuse + `improve_question` reply parse

**Files:**
- Modify: `src/journal_tags.rs` (`pub(crate) fn strip_code_fence`)
- Modify: `src/input/actions/journal.rs` (add `parse_improved_question` + test)
- Test: `src/input/actions/journal.rs` tests

**Interfaces:**
- Produces: `fn parse_improved_question(raw: &str, original: &str) -> String` — fence-strip + trim the reply; return `original` if the result is empty.

- [ ] **Step 1: Make strip_code_fence reusable**

In `src/journal_tags.rs`, change `fn strip_code_fence` to `pub(crate) fn strip_code_fence`. (Confirm the name; if it's inline inside `parse_terms`, extract a `pub(crate) fn strip_code_fence(&str)->&str` and have `parse_terms` call it — keep parse_terms' behavior identical.)

- [ ] **Step 2: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `src/input/actions/journal.rs`:

```rust
#[test]
fn improved_question_parse_strips_fence_and_falls_back() {
    // plain
    assert_eq!(
        parse_improved_question("What does 'fee simple' mean here?", "orig"),
        "What does 'fee simple' mean here?"
    );
    // fenced (model wrapped it)
    assert_eq!(
        parse_improved_question("```\nWhat is a fee simple?\n```", "orig"),
        "What is a fee simple?"
    );
    // empty / whitespace -> keep the original (never lose the question)
    assert_eq!(parse_improved_question("", "the original q"), "the original q");
    assert_eq!(parse_improved_question("   \n  ", "the original q"), "the original q");
}
```

- [ ] **Step 3: Implement**

```rust
/// Parse the improve-question reply: strip a markdown fence + trim. Returns
/// `original` when the reply is empty/whitespace so the question is never lost.
fn parse_improved_question(raw: &str, original: &str) -> String {
    let cleaned = crate::journal_tags::strip_code_fence(raw).trim().to_string();
    if cleaned.is_empty() {
        original.to_string()
    } else {
        cleaned
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cd ~/utono/linux-lit && cargo test improved_question_parse strip_code_fence`
Expected: pass; the existing `journal_tags` fence test still passes.

- [ ] **Step 5: Commit**

```bash
git add src/journal_tags.rs src/input/actions/journal.rs
git commit -m "feat(journal): parse_improved_question + pub(crate) strip_code_fence"
```

---

## Task 2: `improve_question` Claude primitive + fallback prompt

**Files:**
- Modify: `src/input/actions/journal.rs` (add `FALLBACK_IMPROVE_QUESTION_PROMPT` + `improve_question`)

**Interfaces:**
- Produces: `fn improve_question(state: &Rc<RefCell<AppState>>, question: String, on_done: impl Fn(&Rc<RefCell<AppState>>, String) + 'static)` — async; calls Claude with the improve prompt, parses via `parse_improved_question` (fallback = the input question), and invokes `on_done(state, improved)`. On API error, `on_done` is called with the ORIGINAL question (never lose it).

- [ ] **Step 1: Add the fallback prompt constant**

```rust
/// Fallback prompt for improving a reader's journal question when the
/// `journal.improve-question` api_prompts row is absent. Returns ONLY the
/// improved question (one line, no preamble, no JSON).
const FALLBACK_IMPROVE_QUESTION_PROMPT: &str = "\
You improve the phrasing of a reader's question about a literary work. Make it \
clear, specific, and well-formed while PRESERVING the reader's intent and \
meaning — do not answer it, do not add new sub-questions, do not change what is \
being asked. Fix grammar, tighten wording, and resolve vague references only as \
the surrounding intent allows. Return ONLY the improved question as a single \
line of plain text — no preamble, no quotes, no markdown, no explanation.";
```

- [ ] **Step 2: Add `improve_question`**

Model it on `spawn_retag`'s prompt-resolution + `run_claude_request` usage:

```rust
/// Improve a journal question's phrasing via Claude, then hand the improved
/// question (or the original on empty/error) to `on_done` on the main loop.
fn improve_question(
    state: &Rc<RefCell<AppState>>,
    question: String,
    on_done: impl Fn(&Rc<RefCell<AppState>>, String) + 'static,
) {
    let model = state.borrow().config.claude_model.clone();
    let prompt = crate::db::prompts::active_prompt("journal.improve-question")
        .unwrap_or_else(|| FALLBACK_IMPROVE_QUESTION_PROMPT.to_string());
    let original = question.clone();
    let original_err = question.clone();
    crate::input::actions::claude_bridge::run_claude_request(
        state,
        prompt,
        question, // the user message is the raw question
        model,
        move |st, reply| {
            let improved = parse_improved_question(&reply, &original);
            on_done(st, improved);
        },
        move |st, msg| {
            crate::logging::log(&format!("IMPROVE-Q: call failed ({msg}); keeping original"));
            on_done(st, original_err.clone());
        },
    );
}
```

(Confirm `config.claude_model` is the right field for the model — match what `rewrite_with_claude`/`ask_claude` use. `run_claude_request`'s on_error takes `&str`.)

- [ ] **Step 3: Build**

Run: `cargo build` — compiles (unused until Tasks 3-4; dead_code warnings OK, no `#[allow]`).

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/journal.rs
git commit -m "feat(journal): improve_question Claude primitive + fallback prompt"
```

---

## Task 3: Auto-improve the question at creation

**Files:**
- Modify: `src/input/actions/journal.rs` (`begin_ask` / the create entry point that calls `ask_claude`)

**Interfaces:**
- Consumes: `improve_question` (Task 2), the existing `ask_claude`.

- [ ] **Step 1: Find the create entry point**

Read how a new question flows to `ask_claude` (the ask-card submit → `ask_claude(state, &text)`). Confirm the submit site (likely in `submit_prompt` / `begin_ask`'s completion).

- [ ] **Step 2: Interpose improve_question before ask_claude**

At the create submit site, instead of `ask_claude(state, &text)` directly:

```rust
improve_question(state, text.to_string(), move |st, improved| {
    ask_claude(st, &improved);
});
```

`ask_claude` already frames + answers + saves with the question it's given, so passing the improved question stores the polished Q + its answer. Show the loading card immediately (before the improve call returns) so the UI isn't dead during the extra round-trip — reuse `show_loading` with the raw text as a placeholder, or a "Improving…" hint. (Confirm `ask_claude` calls `show_loading` itself; if so, add a brief loading state around the improve call too.)

- [ ] **Step 3: Build + full test**

Run: `cargo build && cargo test` — clean; all pass.

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/journal.rs
git commit -m "feat(journal): auto-improve the question at entry creation"
```

---

## Task 4: `R` target chooser (question / answer / both)

**Files:**
- Modify: `src/app/mod.rs` (`InputMode::RewriteTargetChoice`)
- Modify: `src/input/keymap.rs` (`R` opens the chooser; `handle_rewrite_target_key`)
- Modify: `src/input/actions/journal.rs` (`open_rewrite_target`, `rewrite_question_path`, wire `both`)

**Interfaces:**
- Consumes: `improve_question`, `rewrite_with_claude`, `begin_rewrite`, `displayed_journal_page`.

- [ ] **Step 1: Add the InputMode + chooser opener**

`src/app/mod.rs`: add `RewriteTargetChoice` to `InputMode` (beside DeleteConfirm/UndoConfirm).

`journal.rs`: `open_rewrite_target(state)` — show a small chooser (mirror `show_delete_confirmation`'s toast/hint mechanism: a transient prompt "Rewrite: q question · a answer · b both · Esc cancel") and set `InputMode::RewriteTargetChoice`. (Confirm the confirm-mode display API — reuse whatever `show_delete_confirmation` uses to show its hint.)

- [ ] **Step 2: Route `R` to the chooser**

In `keymap.rs handle_journal_key`, change the `R` arm (currently `begin_rewrite`) to `open_rewrite_target(state)`. (R is already un-gated under a filter; keep it that way.)

- [ ] **Step 3: The chooser key handler**

`keymap.rs`: add `InputMode::RewriteTargetChoice => handle_rewrite_target_key(state, key_name)` to the mode dispatch, and:

```rust
fn handle_rewrite_target_key(state: &Rc<RefCell<AppState>>, key_name: &str) -> bool {
    match key_name {
        "a" => { back_to_overlay(state); crate::input::actions::journal::begin_rewrite(state); true }
        "q" => { back_to_overlay(state); crate::input::actions::journal::rewrite_question_path(state, false); true }
        "b" => { back_to_overlay(state); crate::input::actions::journal::rewrite_question_path(state, true); true }
        "Escape" => { back_to_overlay(state); true }
        _ => true, // consume other keys in the chooser
    }
}
```

(`back_to_overlay` = set `input_mode = JournalOverlay` + dismiss the chooser hint; inline it or reuse the confirm-mode dismissal. Confirm the exact reset the delete/undo confirm handlers use on their non-matching keys.)

- [ ] **Step 4: `rewrite_question_path`**

```rust
/// R → question (or both): improve the DISPLAYED entry's question, then
/// regenerate its answer via rewrite_with_claude (entry-anchored, id-update,
/// filter-aware). When `also_answer_instruction` is true (the `both` case),
/// after the question improves, open the answer-rewrite instruction card
/// (begin_rewrite) instead of a plain regenerate.
pub(crate) fn rewrite_question_path(state: &Rc<RefCell<AppState>>, both: bool) {
    let Some(page) = ({ let s = state.borrow(); displayed_journal_page(&s) }) else {
        crate::ui::toast::show_transient(&state.borrow().chapter_toast, "Nothing to rewrite", 2);
        return;
    };
    let (id, old_q, answer) = (page.id, page.question.trim().to_string(), page.answer.trim().to_string());
    crate::ui::toast::show_persistent(&state.borrow().chapter_toast, "Improving question\u{2026}");
    improve_question(state, old_q, move |st, improved_q| {
        // Persist the improved question immediately (answer unchanged for now),
        // and update the in-memory filter match so the view shows it.
        {
            let s = st.borrow();
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let model = {
                    // reuse the entry's model
                    displayed_journal_page(&s).map(|p| p.claude_model).unwrap_or_default()
                };
                let _ = crate::db::journal::update_journal_page(&conn, id, &improved_q, &answer, &model);
            }
        }
        // Then (re)generate the answer for the improved question. `both` opens
        // the instruction card; otherwise regenerate with a fixed "answer the
        // (possibly reworded) question afresh" instruction.
        if both {
            // stash and open the answer-rewrite instruction card for the NEW q
            begin_rewrite_with(st, id, &improved_q, &answer);
        } else {
            rewrite_with_claude(st, id, &improved_q, &answer,
                "The question was reworded for clarity; answer this (possibly reworded) question afresh, grounded as before.");
        }
    });
}
```

Add a tiny `begin_rewrite_with(state, id, q, a)` (factor from `begin_rewrite` — it currently reads `displayed_journal_page`; extract the body that stashes `vim_rewrite` + opens the ask card given explicit `(id,q,a)`), so the `both` path opens the instruction card with the IMPROVED question.

(During implementation confirm: `rewrite_with_claude` already updates by id + re-renders filter-aware, and saving the improved question first then re-answering means the final `update_journal_page` in rewrite_with_claude's on_success writes `(improved_q, revised_answer)` — pass `question=improved_q` so it persists the new Q with the new A. Verify rewrite_with_claude writes the `question` arg it's given, not the stale one.)

- [ ] **Step 5: Build + full test**

Run: `cargo build && cargo test` — clean; all pass.

- [ ] **Step 6: Commit**

```bash
git add src/app/mod.rs src/input/keymap.rs src/input/actions/journal.rs
git commit -m "feat(journal): R target chooser (question/answer/both) + question-improve path"
```

---

## Task 5: Headless end-to-end verification

**Files:** none.

- [ ] **Step 1: Create improves the question**

Back up: pick a scene with journal entries, or create in a throwaway spot. Headless-drive: open the journal overlay, `r`, type a deliberately clumsy question ("wat mercutio mean fee simple thing"), send. After the round-trip, check the stored question was IMPROVED (DB: the new entry's `question` is well-formed, not the raw text). Note the new id; delete it after (restore baseline).

- [ ] **Step 2: R → question improves + regenerates answer**

On an existing entry, `R` → `q`. Confirm: the chooser appeared, the question text changed to an improved form, and the answer regenerated (different from before). Back up + restore the entry (id) — capture original q/a first, restore after.

- [ ] **Step 3: R → question on a FILTERED f-entry**

`f` → term → Return (cross-work Rom 3.1). `R` → `q`. Confirm the DISPLAYED entry's (id=20) question improved + answer regenerated, the FILTERED view re-rendered (footer stays filter form, not origin band), and DB id=20 changed. RESTORE id=20 fully (back up first).

- [ ] **Step 4: R → answer still works; R → both**

`R` → `a` → the existing instruction card + answer-only rewrite (unchanged). `R` → `b` → question improves THEN the instruction card opens for the answer. Restore any touched entry.

- [ ] **Step 5: Report + cleanup**

Report each step PASS/FAIL with DB evidence; confirm all touched entries restored byte-identical; scoped `pkill` cleanup.

---

## Self-Review notes

- **Reuse over new surface:** the re-answer path reuses `rewrite_with_claude` (entry-anchored + id-update + filter-aware), NOT `ask_claude` (origin-band-anchored). `improve_question` mirrors `spawn_retag`'s prompt+bridge pattern. The chooser mirrors the delete/undo confirm-mode idiom.
- **No data loss:** `parse_improved_question` and `improve_question`'s on_error both fall back to the original question.
- **Filter-correct:** all target resolution via `displayed_journal_page`; re-render via `rewrite_with_claude`'s existing filter branch.
- **Answer prompt untouched:** `journal_qa_prompt` unchanged (Option B).
- **Type consistency:** `improve_question(state, String, Fn(&Rc<RefCell<AppState>>, String))`; `parse_improved_question(&str,&str)->String`; `rewrite_question_path(state, both: bool)`.
- Open impl detail (Task 4): confirm `rewrite_with_claude` persists the `question` arg it's passed (so the improved Q is stored with the new A). If it re-reads the entry's question internally, adjust to use the passed improved Q.
