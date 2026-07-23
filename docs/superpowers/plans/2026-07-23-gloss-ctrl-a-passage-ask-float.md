# Gloss-overlay Ctrl+a floats the passage-ask — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ctrl+a inside the gloss overlay opens the gloss overlay's own right-floated ask card ("Ask a question about this passage") instead of closing the overlay and opening the journal overlay's stacked card; on submit the gloss overlay closes and the existing journal passage-ask Claude flow runs (answer read in the journal overlay); on cancel it stays in the gloss overlay.

**Architecture:** Add `GlossPromptMode::PassageQa`. A new `open_passage_qa_float` sets `journal_band`/`pending_passage` (as `begin_passage_ask` does, minus opening the journal overlay) and opens the gloss overlay's floated ask card via the existing `show_prompt_dialog`/`open_ask_card_with` float path. `submit_gloss_prompt` gains a `PassageQa` arm that closes the gloss overlay then calls a `submit_passage_question` helper factored from the journal's `submit_prompt` new-Q&A tail. The floated ask card's submit/cancel already route through `submit_gloss_prompt`/`close_gloss_prompt` (keymap.rs:2313-2314), so no key wiring changes.

**Tech Stack:** Rust, GTK4 (gtk4-rs). Reuses the `.gloss-ask-float` panel + `AskCardHost` float mode shipped 2026-07-23.

## Global Constraints

- New panels are `add_overlay` layers, never in the size-bearing chain. (`feedback_picker_overlay_not_chain`) — satisfied by reusing the existing float.
- Journal overlay layout unchanged; reader visual-mode Ctrl+a unchanged. (spec, Scope)
- Verify with `cargo build`; do NOT run the app (`cargo run`) — the user launches it. (`feedback_no_cargo_run`)
- Gloss lookups key by `Work.canonical_abbrev`. (`project_gloss_lookup_normalize_abbrev`)
- Cage is software rendering; not final confirmation — hand the user the real-GL steps.
- Timestamps US Central: `TZ='America/Chicago' date +"%Y-%m-%dT%H:%M:%SZ"`.
- Commit trailer: the Co-Authored-By / Claude-Session lines this repo's recent commits use.

---

## File Structure

- `src/app/mod.rs` — `GlossPromptMode` gains `PassageQa`.
- `src/input/actions/journal.rs` — extract `submit_passage_question(state, &text)` from `submit_prompt`'s new-Q&A tail; `submit_prompt` delegates to it.
- `src/input/actions/gloss.rs` —
  - factor the passage-args extraction shared by `ask_journal_for_passage` + the new path into `gloss_passage_args(state) -> Option<(i64,i64,String,String,String)>`;
  - `open_passage_qa_float(state)` — set band/pending_passage, open the floated ask card, no gloss close;
  - `show_prompt_dialog` PassageQa title arm;
  - `submit_gloss_prompt` PassageQa arm.
- `src/input/keymap.rs` — the gloss-overlay Ctrl+a arm (~2456) calls `open_passage_qa_float`.

Task order: (1) enum variant + exhaustive-match fixups, (2) factor journal submit helper, (3) factor gloss passage-args + open path, (4) submit routing + title + keymap wiring, (5) headless verify. Each task ends green and committed.

---

### Task 1: Add `GlossPromptMode::PassageQa`

**Files:**
- Modify: `src/app/mod.rs:200-205` (enum)

**Interfaces:**
- Produces: `GlossPromptMode::PassageQa` variant, consumed by Tasks 3-4.

- [ ] **Step 1: Add the variant**

In `src/app/mod.rs`, extend the enum:

```rust
#[derive(Clone, Copy, PartialEq)]
pub enum GlossPromptMode {
    Add,
    Edit,
    /// Gloss-overlay `i`: correct one word's /IPA/ in the cursor's source verse.
    FixIpa,
    /// Gloss-overlay Ctrl+a: journal passage Q&A, typed in the gloss overlay's
    /// floated ask card. On submit the gloss overlay closes and the journal
    /// passage-ask Claude flow runs (answer read in the journal overlay).
    PassageQa,
}
```

- [ ] **Step 2: Build to surface every non-exhaustive match**

Run: `cargo build 2>&1 | rg "error\[E0004\]|non-exhaustive|not covered|Finished" | head`
Expected: compile errors at each `match … GlossPromptMode` lacking a `PassageQa` arm (at least `submit_gloss_prompt` in `gloss.rs:3713` and the `is_edit`/`is_fix_ipa` reads in `show_prompt_dialog`, which use `==` not `match`, so those won't error — only true `match` sites will). Note the error locations; Tasks 3-4 fill the real behavior. For any OTHER match site not covered by Tasks 3-4 (e.g. a debug `Debug`/log arm), add a minimal correct arm now.

- [ ] **Step 3: Add a temporary catch-all ONLY if a match site is unrelated to Tasks 3-4**

If Step 2 reveals a `match` site outside `submit_gloss_prompt` (unlikely), give it the same behavior as the closest existing mode and leave a `// PassageQa: see 2026-07-23 plan Task 4` note. Do NOT add a blanket `_ =>` to `submit_gloss_prompt` (Task 4 gives it a real arm). If the only match site is `submit_gloss_prompt`, skip this step — Task 4 handles it; the crate may not build green until Task 4, which is acceptable mid-task (commit at the end of Task 4, not here).

- [ ] **Step 4: Commit (enum only) if the crate builds; else defer to Task 4**

If `cargo build` is green (all match sites already had catch-alls), commit:

```bash
git add src/app/mod.rs
git commit -m "feat(gloss): GlossPromptMode::PassageQa variant

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01PMwzR5rsFEcxRx5FgxQFk6"
```

If `submit_gloss_prompt`'s match makes the build red, do NOT commit yet — fold this file into Task 4's commit (note it in Task 4 Step 6's `git add`).

---

### Task 2: Factor `submit_passage_question` from the journal submit

Extract the journal's new-Q&A ask tail so the gloss submit can reuse it without duplicating the loading-card + extract-terms + improve-question + `ask_claude` chain.

**Files:**
- Modify: `src/input/actions/journal.rs:2218-2259` (`submit_prompt`)

**Interfaces:**
- Produces: `pub(crate) fn submit_passage_question(state: &Rc<RefCell<AppState>>, text: &str)` — runs the loading card + `extract_scene_terms`→`improve_question`→`ask_claude` chain against the CURRENT `journal_band` + `journal.pending_passage`. Consumed by Task 4.
- Consumes: the existing `extract_scene_terms`, `improve_question`, `ask_claude`, `scene_synopsis::cursor_head`, `journal_overlay.set_running_head`/`show_loading` (all already in `journal.rs`).

- [ ] **Step 1: Write the failing test (pure guard: empty text is a no-op)**

`submit_passage_question` on empty/whitespace text must NOT start a Claude call. The chain is GTK/async and not unit-testable directly, so unit-test the empty-guard decision as a pure helper. Add to a `#[cfg(test)]` module in `journal.rs`:

```rust
#[cfg(test)]
mod passage_submit_tests {
    #[test]
    fn blank_question_is_skipped() {
        assert!(super::is_blank_question("   \n\t "));
        assert!(super::is_blank_question(""));
        assert!(!super::is_blank_question("why the tub?"));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins passage_submit_tests 2>&1 | rg "error\[|test result|FAILED" | head`
Expected: FAIL — `cannot find function is_blank_question`.

- [ ] **Step 3: Add the pure guard + the extracted helper**

In `journal.rs`, add the pure guard:

```rust
/// A journal question with no non-whitespace content is not asked.
pub(crate) fn is_blank_question(text: &str) -> bool {
    text.trim().is_empty()
}
```

Add the extracted helper (the exact body of `submit_prompt`'s new-Q&A tail):

```rust
/// Run the new-Q&A ask chain for the current band/pending_passage: show the
/// loading card, derive scene terms, improve the phrasing, then call Claude.
/// Factored from `submit_prompt` so the gloss-overlay Ctrl+a passage-ask can
/// reuse the exact flow. `text` is the raw typed question. No-op if blank.
pub(crate) fn submit_passage_question(state: &Rc<RefCell<AppState>>, text: &str) {
    if is_blank_question(text) {
        return;
    }
    // Show the loading card immediately with the raw text so the UI isn't
    // dead during the improve-question round-trip; `ask_claude` re-shows it
    // with the improved phrasing once that call returns.
    {
        let s = state.borrow();
        let head = crate::app::scene_synopsis::cursor_head(&s);
        s.journal_overlay.set_running_head(&head.0, &head.1);
        s.journal_overlay.show_loading(text);
    }
    let text_owned = text.to_string();
    // A brand-new ask has no saved entry/tags yet — derive candidate terms from
    // the scene text first, then ground the phrasing on them.
    extract_scene_terms(state, &text_owned, move |st, question, terms| {
        improve_question(st, question, &terms, move |st2, improved| {
            ask_claude(st2, &improved);
        });
    });
}
```

(Check `extract_scene_terms`'s exact signature for the `text` argument — `submit_prompt` passes `text` by value/ref; mirror it. If it takes `&str`, pass `&text_owned`; if `String`, pass `text_owned`. The plan's job is to reuse the existing call shape verbatim.)

- [ ] **Step 4: Delegate `submit_prompt`'s new-Q&A tail to the helper**

Replace `submit_prompt`'s tail (lines 2240-2258, from `if text.trim().is_empty()` through the `extract_scene_terms` block) with:

```rust
    submit_passage_question(state, &text);
```

`submit_prompt`'s rewrite branch (above the tail) is unchanged. Net behavior for the journal overlay's own submit is identical (same guard, same chain).

- [ ] **Step 5: Run the test to verify it passes + build**

Run: `cargo test --bins passage_submit_tests 2>&1 | rg "test result" | head`
Expected: `test result: ok. 1 passed`.
Run: `cargo build 2>&1 | rg "^error|Finished" | tail -2`
Expected: clean build.

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/journal.rs
git commit -m "refactor(journal): factor submit_passage_question from submit_prompt

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01PMwzR5rsFEcxRx5FgxQFk6"
```

---

### Task 3: Factor gloss passage-args + add `open_passage_qa_float`

**Files:**
- Modify: `src/input/actions/gloss.rs` (`ask_journal_for_passage` ~3212; new fns nearby)

**Interfaces:**
- Produces:
  - `fn gloss_passage_args(state: &Rc<RefCell<AppState>>) -> Option<(i64, i64, String, String, String)>` — `(div1, div2, start_citation, end_citation, source_text)`, the extraction currently inline in `ask_journal_for_passage`.
  - `pub(crate) fn open_passage_qa_float(state: &Rc<RefCell<AppState>>)` — set band/pending_passage, open the floated ask card in `PassageQa` mode, NO gloss close. Consumed by Task 4 (keymap wiring).
- Consumes: `journal::begin_passage_ask`'s state setup (replicated inline, no journal-overlay open); `show_prompt_dialog` (Task 4 adds the title arm).

- [ ] **Step 1: Extract `gloss_passage_args`**

Cut the extraction block from `ask_journal_for_passage` (`gloss.rs:3216-3254`, the `let passage_args = { … };` through building the `(div1,div2,start,end,source_text)` tuple) into:

```rust
/// The current gloss passage as `(div1, div2, start_citation, end_citation,
/// source_text)`, preferring the exact start..end citation range and falling
/// back to the whole scene. `None` when there is no gloss context / current
/// work. Shared by the journal-handoff path and the in-overlay float path.
fn gloss_passage_args(
    state: &Rc<RefCell<AppState>>,
) -> Option<(i64, i64, String, String, String)> {
    let s = state.borrow();
    let ctx = s.gloss_context.as_ref()?;
    let work = s.current_work.as_ref()?;
    let selected_lines: Vec<crate::db::models::Line> = match (
        crate::app::parse_citation(&ctx.start_citation),
        crate::app::parse_citation(&ctx.end_citation),
    ) {
        (Some((sd1, sd2, s_lid)), Some((_, _, e_lid))) => work
            .lines
            .iter()
            .filter(|l| {
                l.div1 == sd1 && l.div2 == sd2 && l.line_in_div >= s_lid && l.line_in_div <= e_lid
            })
            .cloned()
            .collect(),
        _ => work
            .lines
            .iter()
            .filter(|l| l.div1 == ctx.act && l.div2 == ctx.scene)
            .cloned()
            .collect(),
    };
    let markup = crate::input::actions::echoes::build_source_header(&selected_lines, &ctx.speaker);
    Some((
        ctx.act,
        ctx.scene,
        ctx.start_citation.clone(),
        ctx.end_citation.clone(),
        markup,
    ))
}
```

Rewrite `ask_journal_for_passage` to use it (keeping its close-then-journal behavior — it may still be called elsewhere; grep before removing):

```rust
pub(crate) fn ask_journal_for_passage(state: &Rc<RefCell<AppState>>) {
    let Some((div1, div2, start, end, source_text)) = gloss_passage_args(state) else {
        return;
    };
    close_gloss_to_reader(state);
    crate::input::actions::journal::begin_passage_ask(state, div1, div2, start, end, source_text);
    crate::logging::log("JOURNAL-FROM-GLOSS: opened passage ask from gloss overlay");
}
```

- [ ] **Step 2: Add `open_passage_qa_float`**

Add beside it. It sets the SAME journal state `begin_passage_ask` sets (so the eventual `ask_claude` has band + pending_passage), MINUS opening the journal overlay and MINUS `input_mode = JournalOverlay` (we stay in the gloss overlay), then opens the gloss overlay's floated ask card:

```rust
/// Gloss-overlay Ctrl+a: open the journal passage Q&A in the gloss overlay's
/// OWN floated ask card (gloss commentary stays left, ask floats right) instead
/// of closing to the journal overlay. Sets the journal band + pending_passage
/// so a submit runs the journal passage flow; the gloss overlay is NOT closed.
pub(crate) fn open_passage_qa_float(state: &Rc<RefCell<AppState>>) {
    let Some((div1, div2, start, end, source_text)) = gloss_passage_args(state) else {
        return;
    };
    {
        let mut s = state.borrow_mut();
        // Mirror begin_passage_ask's state setup (journal.rs:1590-1599) so the
        // eventual ask_claude reads the right band + pending_passage — but do
        // NOT open the journal overlay or switch input_mode (we stay in the
        // gloss overlay; its ask-card intercept routes the typed keys).
        s.journal.return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
        s.journal.entry_page_id = None;
        s.journal.prompt_mode = crate::app::JournalPromptMode::Ask;
        let band = crate::app::JournalBand::Passage { div1, div2, start, end };
        s.journal.pending_passage = Some(crate::input::actions::journal::PendingPassage {
            source_text,
            band: band.clone(),
        });
        s.journal_band = band;
        s.journal.page_index = 0;
    }
    // Open the gloss overlay's floated ask card in PassageQa mode, then INSERT.
    show_prompt_dialog(state, crate::app::GlossPromptMode::PassageQa);
    let _ = state
        .borrow()
        .gloss_overlay
        .feed_ask_vim_key(crate::input::vim::VimKey::Char('i'));
}
```

Verify `PendingPassage` and `JournalBand` are importable at these paths (they are used by `journal.rs`; confirm the `pub(crate)` visibility and adjust the `use`/path — `PendingPassage` may live at `crate::input::actions::journal::PendingPassage` or a models module; grep and use the real path).

- [ ] **Step 3: Build (expect the PassageQa title + submit arms still missing → Task 4)**

Run: `cargo build 2>&1 | rg "error\[|Finished" | head`
Expected: MAY still error on `submit_gloss_prompt`'s non-exhaustive match (Task 4) and possibly `show_prompt_dialog` if it `match`es the mode. If `show_prompt_dialog` uses `==` (it does, per gloss.rs:712-715), the title just falls to the `else` default until Task 4 — no compile error there. Do not commit yet; Task 4 completes the build.

---

### Task 4: Submit routing + title + keymap wiring

**Files:**
- Modify: `src/input/actions/gloss.rs` (`show_prompt_dialog` title ~717; `submit_gloss_prompt` ~3713)
- Modify: `src/input/keymap.rs` (gloss-overlay Ctrl+a arm ~2456)

**Interfaces:**
- Consumes: `open_passage_qa_float` (Task 3), `submit_passage_question` (Task 2), `GlossPromptMode::PassageQa` (Task 1).

- [ ] **Step 1: PassageQa title in `show_prompt_dialog`**

In `show_prompt_dialog` (`gloss.rs:703`), add a mode check and title. After the existing `is_fix_ipa`/`is_edit` lets, add:

```rust
    let is_passage_qa = mode == crate::app::GlossPromptMode::PassageQa;
```

Prepend a `PassageQa` branch to the `title_text` chain (before `is_fix_ipa`):

```rust
    let title_text = if is_passage_qa {
        "Ask a question about this passage"
    } else if is_fix_ipa {
        "FIX IPA — word /IPA/  OR  word <hint>"
    } else if is_edit {
        "Rewrite instruction — questions welcome"
    } else if is_inner_monologue {
        "Inner monologue passage"
    } else if is_reader_gloss {
        "Ask a question about the passage"
    } else {
        "Ask a question about the passage"
    };
```

Hint: the existing default `"Ctrl+Enter submit"` (the `else` hint arm) applies for PassageQa — no change needed. Legend `""` (default) — no change.

- [ ] **Step 2: PassageQa arm in `submit_gloss_prompt`**

In `submit_gloss_prompt` (`gloss.rs:3706`), add the arm to the `match mode`:

```rust
    match mode {
        crate::app::GlossPromptMode::Add if is_empty => {}
        crate::app::GlossPromptMode::Add => add_gloss(state, &prompt),
        crate::app::GlossPromptMode::Edit if is_empty => {
            edit_gloss(state, EDIT_GLOSS_DEFAULT_INSTRUCTION)
        }
        crate::app::GlossPromptMode::Edit => edit_gloss(state, &prompt),
        crate::app::GlossPromptMode::FixIpa if is_empty => {}
        crate::app::GlossPromptMode::FixIpa => fix_word_ipa(state, &prompt),
        // Empty passage question → stay in the gloss overlay (nothing to ask);
        // close_gloss_prompt (called above) already hid the float.
        crate::app::GlossPromptMode::PassageQa if is_empty => {}
        // Non-empty → close the gloss overlay and run the journal passage flow.
        // The band + pending_passage were set in open_passage_qa_float, so
        // submit_passage_question's ask_claude reads them and lands the answer
        // in the journal overlay (today's post-submit behavior).
        crate::app::GlossPromptMode::PassageQa => {
            close_gloss_to_reader(state);
            crate::input::actions::journal::submit_passage_question(state, &prompt);
        }
    }
```

Note `close_gloss_prompt(state)` (which hides the float) is already called at the top of `submit_gloss_prompt` (`gloss.rs:3711`) before the match — so the PassageQa arm only needs the `close_gloss_to_reader` + journal submit.

- [ ] **Step 3: Wire the keymap Ctrl+a arm**

In `src/input/keymap.rs`, the gloss-overlay Ctrl+a arm (~2456) currently calls `ask_journal_for_passage`. Change it:

```rust
            // Ctrl+a: journal passage Q&A, typed in the gloss overlay's OWN
            // floated ask card (gloss commentary stays visible). On submit the
            // overlay closes and the journal passage flow runs (answer in the
            // journal overlay); on cancel it stays in the gloss overlay.
            "a" => {
                crate::input::actions::gloss::open_passage_qa_float(state);
                return true;
            }
```

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | rg "^error|error\[|Finished" | tail -5`
Expected: clean build (all `GlossPromptMode` matches now exhaustive; `open_passage_qa_float` / `submit_passage_question` resolve).

- [ ] **Step 5: Journal overlay untouched check**

Run: `git diff --stat` — expected files: `src/app/mod.rs`, `src/input/actions/journal.rs`, `src/input/actions/gloss.rs`, `src/input/keymap.rs`. `src/ui/journal_overlay.rs` and `src/ui/gloss_overlay.rs` MUST NOT appear (no UI change — the float widget is reused as-is).

- [ ] **Step 6: Unit tests + commit**

Run: `cargo test --bins 2>&1 | rg "test result" | tail -3`
Expected: all pass.

```bash
git add src/app/mod.rs src/input/actions/gloss.rs src/input/keymap.rs
# (src/app/mod.rs only if not already committed in Task 1)
git commit -m "feat(gloss): Ctrl+a floats the passage-ask in the gloss overlay

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01PMwzR5rsFEcxRx5FgxQFk6"
```

---

### Task 5: Headless verification

Confirm on-screen that Ctrl+a in the gloss overlay floats the passage-ask (gloss left full height, ask right, centered pair), the title reads "Ask a question about this passage", and Escape re-centers the gloss card without leaving the overlay.

**Files:** none (verification only). Fixes go back to the relevant task file.

- [ ] **Step 1: clippy + line-clipping e2e (no-regression)**

Run: `cargo clippy 2>&1 | rg "gloss.rs|journal.rs|keymap.rs" | rg "warning|error" | head`
Expected: no new lints in the changed files.
Run: `./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture 2>&1 | rg "test result" | tail -2`
Expected: PASS (the gloss card scroll is unchanged).

- [ ] **Step 2: Cage drive — Ctrl+g then Ctrl+a**

Launch cage per the Headless Verification protocol (full harness env: `WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman WLR_RENDERER_ALLOW_SOFTWARE=1 WLR_HEADLESS_OUTPUTS=1 GDK_BACKEND=wayland LIBGL_ALWAYS_SOFTWARE=1 GALLIUM_DRIVER=llvmpipe`, plus `LIT_DEV=1 LIT_NO_MPV=1 GSK_RENDERER=cairo`). The app takes the cage's `wayland-N` (N≥1); read the socket from `/proc/<app-pid>/environ` if the glob races. Resize `HEADLESS-1` to 1920x1200. On a work with a reader-gloss (e.g. `TT`, `last_work`):

```
wtype -M ctrl -k g -m ctrl   # open gloss overlay
sleep 1.5
wtype -M ctrl -k a -m ctrl   # Ctrl+a → float the passage-ask
sleep 2
grim -o HEADLESS-1 target/ui/ctrl-a-float.png
```

- [ ] **Step 3: Pixel-measure the centered pair + read the title**

Open `target/ui/ctrl-a-float.png`. Assert (pixel-scan the cream/teal boundaries at mid-height, as the Ctrl+r feature did):
- two cards: gloss commentary LEFT (full height), ask panel RIGHT (bordered `.gloss-ask-float`);
- L/R gutters within ~15px (centered pair);
- the right panel title reads **"Ask a question about this passage"** (quote it from the screenshot).

If the title reads "Ask a question about the passage" (the reader-gloss default), the PassageQa title arm did not take — fix Task 4 Step 1.

- [ ] **Step 4: Escape re-centers, stays in overlay**

```
wtype -k Escape -k Escape   # double-Escape force-cancels the vim ask card
sleep 2
grim -o HEADLESS-1 target/ui/ctrl-a-closed.png
```

Assert the single gloss card is re-centered (L≈R) and the gloss overlay is STILL open (gloss commentary visible, not the plain reader). If it fell back to the reader, the cancel path wrongly closed the overlay — check that `close_gloss_prompt` (not `close_gloss_to_reader`) is the cancel callback (keymap.rs:2314, unchanged).

- [ ] **Step 5: Scoped cage cleanup**

Run ONLY: `pkill -f "cage -- ./target/debug/linux-lit"` (never a bare `pkill -f target/debug/linux-lit` — that kills the user's live instance).

- [ ] **Step 6: Hand the user the real-GL + submit steps**

Cage is software rendering AND the submit→journal-answer leg needs a live Claude call. Give the user:

```bash
cd ~/utono/linux-lit && cargo run
# open a reader-glossed passage → Ctrl+g → Ctrl+a
#   expect: gloss commentary left (full height), ask panel right titled
#   "Ask a question about this passage", pair centered.
# type a question, Ctrl+Enter:
#   expect the gloss overlay closes and the answer renders + persists in the
#   journal overlay (as before).
# reopen, Ctrl+a, Escape x2: expect the gloss card re-centers, overlay stays open.
```

- [ ] **Step 7: Final commit (if fixes landed)**

```bash
git add -A
git commit -m "fix(gloss): correct Ctrl+a passage-ask float from headless review

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01PMwzR5rsFEcxRx5FgxQFk6"
```

---

## Self-Review

**Spec coverage:**
- Ctrl+a does not close, floats the gloss ask card → Task 3 (`open_passage_qa_float`) + Task 4 Step 3 (keymap). ✓
- Title "Ask a question about this passage" → Task 4 Step 1. ✓
- Submit closes gloss + runs journal passage flow, answer in journal overlay → Task 4 Step 2 (`close_gloss_to_reader` + `submit_passage_question`) reusing the journal `ask_claude` path (Task 2). ✓
- Cancel stays in the gloss overlay → the ask card cancel callback is `close_gloss_prompt` (unchanged, keymap.rs:2314), verified Task 5 Step 4. ✓
- New `PassageQa` mode, not overloaded → Task 1. ✓
- Journal overlay layout unchanged; reader visual-mode Ctrl+a unchanged → only the gloss-overlay Ctrl+a arm changes (Task 4 Step 3); `submit_prompt` behavior preserved (Task 2 Step 4); verified Task 4 Step 5. ✓
- Reuses `.gloss-ask-float` / float host, no UI/theme change → no `theme.rs`/`*_overlay.rs` edits (Task 4 Step 5). ✓
- Empty-submit = no-op, stay in overlay → Task 4 Step 2 (`PassageQa if is_empty => {}`). ✓ (Decision from spec: leave band/pending_passage; a re-Ctrl+a re-sets them.)

**Placeholder scan:** every code step shows the actual code. The two "confirm the real path" notes (`extract_scene_terms` arg shape in Task 2; `PendingPassage`/`JournalBand` import path in Task 3) are concrete verification instructions against named symbols, not vague placeholders — the compiler enforces them.

**Type consistency:** `GlossPromptMode::PassageQa` (Task 1) ↔ matched in `submit_gloss_prompt` (Task 4) and `show_prompt_dialog` (Task 4) and passed to `show_prompt_dialog` (Task 3). `gloss_passage_args -> Option<(i64,i64,String,String,String)>` (Task 3) destructured identically in `ask_journal_for_passage` and `open_passage_qa_float`. `submit_passage_question(&Rc<RefCell<AppState>>, &str)` (Task 2) called with `(state, &prompt)` (Task 4). `is_blank_question(&str)->bool` (Task 2) used by `submit_passage_question`. ✓

## Out of scope (carried from spec)

- Reader visual-mode Ctrl+a; journal overlay layout / its stacked ask card;
  rendering the answer inside the gloss overlay; any Claude-prompt / schema /
  Q&A-content change.
