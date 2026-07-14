# Term-ground the First Ask From Scene Terms — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** On a new Scene/Passage journal ask, extract candidate terms of art from the scene text and feed them to the improve-question (phrasing) call, so the first question is sharpened around those terms.

**Architecture:** New `journal.scene-terms` prompt (synced to lit.db). In linux-lit, factor `current_scene_text` out of `ask_claude`, add `extract_scene_terms` (models `spawn_retag` but feeds terms forward), and route `submit_prompt`'s new-ask branch through it before `improve_question`. Reuses `improve_question(terms)`, `improve_terms_line`, and `parse_terms` from the prior feature.

**Tech Stack:** Rust (rusqlite, GTK4), Python sync scripts, SQLite (lit.db).

## Global Constraints

- Prompt source of truth is `~/utono/claude-api-prompts/prompts/<key>.md`; NEVER edit `api_prompts` rows directly — sync from master.
- `journal.scene-terms` output contract MUST match `journal.extract-terms`: a bare `{"terms":[...]}` object, ≤8 terms, canonical phrasing, `{"terms":[]}` when none — so `crate::journal_tags::parse_terms` parses it unchanged.
- Extraction uses `config.tag_extract_model` (the cheap model `spawn_retag` uses), not `config.claude_model`.
- Scene/Passage bands only. Work/Author bands and empty scene text → no extra API call, `terms = []`, behavior identical to today.
- Any extraction error/unparseable reply → `terms = []`; never block the ask.
- `AppState` has no test constructor — do NOT attempt to unit-fixture it. Test pure helpers; verify wiring by compile + behavioral pass.
- linux-lit reads the active `api_prompts` row at next launch (no hot reload).

---

### Task 1: `journal.scene-terms` prompt master + sync

**Files:**
- Create: `~/utono/claude-api-prompts/prompts/journal.scene-terms.md`

**Interfaces:**
- Produces: `journal.scene-terms` active row in `api_prompts`, consumed by Task 3's `extract_scene_terms`.

- [ ] **Step 1: Create the master**

```markdown
---
prompt_key: journal.scene-terms
consumer: linux-lit
has_placeholders: false
---

You extract the substantive terms of art that a reader working through the
following passage might want to ask about — legal, rhetorical, historical,
prosodic, or theological terms a reader might later look up (e.g. "fee simple",
"anaphora", "recusant").

Do NOT include ordinary vocabulary, character names, or the work's title.
Prefer the canonical phrasing of each term. Return AT MOST 8 terms.

Return ONLY a JSON object (no markdown fences, no commentary) with exactly one
key:

{"terms": ["term one", "term two"]}

If the passage has no such term, return {"terms": []}.
```

- [ ] **Step 2: Back up lit.db** (sync writes to it)

Run: `systemctl --user start lit-db-backup-local.service && command ls -1t ~/backups/lit-db/ | head -1`
Expected: a fresh `lit-<timestamp>.db` at the current time.

- [ ] **Step 3: Commit the master + sync**

```bash
cd ~/utono/claude-api-prompts
git add prompts/journal.scene-terms.md
git commit -m "feat: journal.scene-terms — extract passage terms for first-ask grounding"
python scripts/sync-to-db.py journal.scene-terms
```

- [ ] **Step 4: Verify active + contract**

```bash
python scripts/list-versions.py journal.scene-terms      # v1 active (*)
python scripts/render-prompt.py journal.scene-terms       # prints the body
```

Expected: v1 active; body contains the `{"terms": [...]}` contract line.

---

### Task 2: Factor `current_scene_text` out of `ask_claude`

**Files:**
- Modify: `~/utono/linux-lit/src/input/actions/journal.rs` — extract the scene-text assembly from `ask_claude` (~1839-1881) into a helper; call it from `ask_claude`.

**Interfaces:**
- Produces: `fn current_scene_text(s: &AppState) -> String` — the windowed scene/passage text for the current band + anchor, empty for Work/Author/unresolvable. Consumed by Task 3.

- [ ] **Step 1: Add the helper** (place just above `ask_claude`). Copy the exact assembly currently inline in `ask_claude`'s borrow block:

```rust
/// The windowed scene/passage text for the current journal band, anchored on the
/// reader's saved position — the same context `ask_claude` sends to the answer
/// prompt. Empty for Work/Author bands and unresolvable positions. Factored so
/// the answer path and the first-ask term extractor build it identically.
fn current_scene_text(s: &AppState) -> String {
    let anchor_work_line = s
        .journal
        .return_pos
        .and_then(|(buf, _top, _off)| s.work_line_for_buffer(buf))
        .unwrap_or(0);
    match &s.journal_band {
        JournalBand::Work => String::new(),
        JournalBand::Scene(d1, d2) => crate::app::scene_synopsis::scene_text_windowed(
            s, *d1, *d2, anchor_work_line, PROSE_CONTEXT_RADIUS,
        ),
        JournalBand::Passage { div1, div2, .. } => crate::app::scene_synopsis::scene_text_windowed(
            s, *div1, *div2, anchor_work_line, PROSE_CONTEXT_RADIUS,
        ),
        JournalBand::Author(_) => String::new(),
    }
}
```

- [ ] **Step 2: Call it from `ask_claude`.** Replace the inline `let scene_text = match band { ... };` block (the `anchor_work_line` let + the `match band` that produces `scene_text`) with:

```rust
        let scene_text = current_scene_text(&s);
```

(The surrounding tuple destructure that binds `band`, `scene_text`, etc. stays; only the assembly is replaced. `band` is still `s.journal_band.clone()` above.)

- [ ] **Step 3: Build to verify the refactor is behavior-preserving**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -3`
Expected: builds; no unused-variable warnings for `anchor_work_line` (it now lives inside the helper).

- [ ] **Step 4: Run the journal tests** (no behavior change expected)

Run: `cd ~/utono/linux-lit && cargo test --bin linux-lit input::actions::journal 2>&1 | rg "test result:"`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit
git add src/input/actions/journal.rs
git commit -m "refactor(journal): extract current_scene_text from ask_claude"
```

---

### Task 3: `extract_scene_terms` + route the new-ask branch through it

**Files:**
- Modify: `~/utono/linux-lit/src/input/actions/journal.rs` — add `FALLBACK_SCENE_TERMS_PROMPT`, `extract_scene_terms`; change `submit_prompt`'s new-ask branch (~1672-1676).

**Interfaces:**
- Consumes: `current_scene_text` (Task 2), `journal.scene-terms` prompt (Task 1), `crate::journal_tags::parse_terms`, `improve_question` + `improve_terms_line` (prior feature).
- Produces: `fn extract_scene_terms(state, question: String, on_done: impl Fn(&Rc<RefCell<AppState>>, String, Vec<String>) + 'static)` — resolves candidate terms (or empty), then hands `(state, question, terms)` to `on_done`.

- [ ] **Step 1: Add the fallback prompt** (place near `FALLBACK_IMPROVE_QUESTION_PROMPT`). Byte-mirror the master body:

```rust
/// Fallback for the scene-terms extractor when the `journal.scene-terms`
/// api_prompts row is absent. Mirrors the master so a missing row does not
/// silently disable first-ask term grounding. Same `{"terms":[...]}` contract as
/// the extract-terms prompt, so `parse_terms` handles the reply unchanged.
const FALLBACK_SCENE_TERMS_PROMPT: &str = "\
You extract the substantive terms of art that a reader working through the\n\
following passage might want to ask about — legal, rhetorical, historical,\n\
prosodic, or theological terms a reader might later look up (e.g. \"fee simple\",\n\
\"anaphora\", \"recusant\").\n\
\n\
Do NOT include ordinary vocabulary, character names, or the work's title.\n\
Prefer the canonical phrasing of each term. Return AT MOST 8 terms.\n\
\n\
Return ONLY a JSON object (no markdown fences, no commentary) with exactly one\n\
key:\n\
\n\
{\"terms\": [\"term one\", \"term two\"]}\n\
\n\
If the passage has no such term, return {\"terms\": []}.";
```

- [ ] **Step 2: Add `extract_scene_terms`** (place just above `improve_question` or near `spawn_retag`):

```rust
/// Resolve candidate terms of art for a BRAND-NEW ask by extracting them from
/// the current scene text, then hand `(state, question, terms)` to `on_done`.
/// Empty scene text (Work/Author band, unresolvable position) or any extraction
/// error yields an empty term list with NO added latency beyond the (skipped)
/// call — the ask then proceeds ungrounded, exactly as before this feature.
///
/// BORROW SAFETY: scene text + model are read under one scoped borrow that drops
/// before `run_claude_request` (which re-borrows `state`), mirroring
/// `spawn_retag`. `on_done` runs later inside the request callbacks.
fn extract_scene_terms(
    state: &Rc<RefCell<AppState>>,
    question: String,
    on_done: impl Fn(&Rc<RefCell<AppState>>, String, Vec<String>) + 'static,
) {
    let (scene_text, model) = {
        let s = state.borrow();
        (current_scene_text(&s), s.config.tag_extract_model.clone())
    };
    if scene_text.trim().is_empty() {
        on_done(state, question, Vec::new());
        return;
    }
    let prompt = crate::db::prompts::active_prompt("journal.scene-terms")
        .unwrap_or_else(|| FALLBACK_SCENE_TERMS_PROMPT.to_string());
    let on_done = Rc::new(on_done);
    let on_done_err = Rc::clone(&on_done);
    let q_ok = question.clone();
    let q_err = question;
    crate::input::actions::claude_bridge::run_claude_request(
        state,
        prompt,
        scene_text,
        model,
        move |st, reply| {
            let terms = crate::journal_tags::parse_terms(&reply);
            on_done(st, q_ok.clone(), terms);
        },
        move |st, msg| {
            crate::logging::log(&format!("SCENE-TERMS: extract failed ({msg}); no grounding"));
            on_done_err(st, q_err.clone(), Vec::new());
        },
    );
}
```

- [ ] **Step 3: Route the new-ask branch.** In `submit_prompt`, replace the current new-ask call:

```rust
    state.borrow().journal_overlay.show_loading(&text);
    // A brand-new ask has no saved entry yet, so no tags exist to ground on.
    improve_question(state, text, &[], move |st, improved| {
        ask_claude(st, &improved);
    });
```

with:

```rust
    state.borrow().journal_overlay.show_loading(&text);
    // A brand-new ask has no saved entry/tags yet — derive candidate terms from
    // the scene text first, then ground the phrasing on them.
    extract_scene_terms(state, text, move |st, question, terms| {
        improve_question(st, question, &terms, move |st2, improved| {
            ask_claude(st2, &improved);
        });
    });
```

- [ ] **Step 4: Build**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -3`
Expected: builds clean (closure move/borrow discipline compiles; `improve_question` takes `&terms`).

- [ ] **Step 5: Full journal tests**

Run: `cd ~/utono/linux-lit && cargo test --bin linux-lit input::actions::journal 2>&1 | rg "test result:"`
Expected: all pass (pure helpers unaffected; wiring compiles).

- [ ] **Step 6: Commit**

```bash
cd ~/utono/linux-lit
git add src/input/actions/journal.rs
git commit -m "feat(journal): first ask grounds phrasing on scene-extracted terms"
```

---

### Task 4: Add `journal.scene-terms` to the `update-api-prompt` skill

**Files:**
- Modify: `~/utono/claude-api-prompts/.claude/skills/update-api-prompt/SKILL.md`

- [ ] **Step 1: Add to the key list** (after `journal.improve-question`):

```markdown
- `journal.scene-terms` — extracts terms of art from the current passage to
  ground a brand-new ask's phrasing (fed via `improve_question`'s `{terms}`)
```

Also add `journal.scene-terms` to the frontmatter `description` list.

- [ ] **Step 2: Commit**

```bash
cd ~/utono/claude-api-prompts
git add .claude/skills/update-api-prompt/SKILL.md
git commit -m "docs(skill): list journal.scene-terms in update-api-prompt"
```

---

### Task 5: End-to-end verification + finish

- [ ] **Step 1: Full test suite + build (linux-lit)**

Run: `cd ~/utono/linux-lit && cargo test --bin linux-lit 2>&1 | rg "test result:" && cargo build 2>&1 | tail -1`
Expected: all tests pass; build clean.

- [ ] **Step 2: DB-active prompt contract check** (mirror the prior feature's Python check)

Verify the active `journal.scene-terms` body parses through `parse_terms`' contract: run a Python snippet that reads the active row and asserts it contains `{"terms":` and mentions "terms of art".

- [ ] **Step 3: Behavioral sanity (optional, live/nondeterministic)** — launch the reader, open a Scene-band ask on a term-rich passage (Rom 3.1), type a vague question, confirm the improved question sharpens toward the passage's terms; confirm a Work-band ask is unaffected. Note this fires 3 API calls.

- [ ] **Step 4: Merge + push** (per workspace finishing-a-branch rule): clean tree + tests, `git checkout master`, `git merge --no-ff`, re-verify build, `git push origin master`, `git branch -d`. claude-api-prompts commits already on master — push it too.
