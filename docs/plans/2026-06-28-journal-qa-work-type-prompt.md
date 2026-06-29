# Work-type-aware journal Q&A prompt — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the journal Q&A prompt speak in the work's own genre (novel/chapter, epic/book, play/scene, …) instead of always saying "play/scene", so answers stop opening with a genre correction.

**Architecture:** A pure `genre_unit(work_type)` lookup in `src/gloss.rs` feeds a new per-request `journal_qa_prompt(work_type)` that substitutes `{genre}`/`{unit}`/`{units}` into the template (DB-active row, else the rewritten FALLBACK). Both journal call sites pass `current_work.work_type` and relabel their user messages. The live DB prompt (`api_prompts` `journal.qa`, currently v3) is updated to v4 with the placeholders via the `claude-api-prompts` repo.

**Tech Stack:** Rust, GTK4, rusqlite (lit.db); the `~/utono/claude-api-prompts` master/sync repo (Python scripts) for the DB prompt.

## Global Constraints

- Do NOT run `cargo run`; build with `cargo build`, the user launches the app.
- US Central timestamps where any are needed.
- DB prompt has no hot reload — v4 applies on the next launch; linux-lit must be CLOSED during `sync-to-db.py`.
- lit.db uses `prose` (not `novel`) as the Dickens work_type; `genre_unit` maps `prose`→"novel".
- linux-lit FALLBACK text and the DB v4 master are kept textually identical (repo invariant).
- Match surrounding code style; no unrelated refactors.

---

### Task 1: `genre_unit` lookup + tests

**Files:**
- Modify: `src/gloss.rs` (add `pub fn genre_unit` near `JOURNAL_QA_PROMPT`, ~line 148; add a `#[cfg(test)] mod` at end of file)

**Interfaces:**
- Produces: `pub fn genre_unit(work_type: &str) -> (&'static str, &'static str, &'static str)` returning `(genre, unit, units_plural)`.

- [ ] **Step 1: Write the failing tests** — add at the end of `src/gloss.rs`:

```rust
#[cfg(test)]
mod genre_unit_tests {
    use super::genre_unit;

    #[test]
    fn maps_every_known_work_type() {
        assert_eq!(genre_unit("play"), ("play", "scene", "scenes"));
        assert_eq!(genre_unit("prose"), ("novel", "chapter", "chapters"));
        assert_eq!(genre_unit("prose_book"), ("novel", "chapter", "chapters"));
        assert_eq!(genre_unit("bible_book"), ("book", "chapter", "chapters"));
        assert_eq!(genre_unit("epic"), ("epic poem", "book", "books"));
        assert_eq!(genre_unit("epic_translation"), ("epic poem", "book", "books"));
        assert_eq!(genre_unit("narrative_poem"), ("narrative poem", "section", "sections"));
        assert_eq!(genre_unit("poem"), ("poem", "section", "sections"));
        assert_eq!(genre_unit("sonnet_sequence"), ("sequence", "sonnet", "sonnets"));
        assert_eq!(genre_unit("verse_essay"), ("essay", "section", "sections"));
        assert_eq!(genre_unit("essay_collection"), ("collection", "essay", "essays"));
        assert_eq!(genre_unit("anthology"), ("anthology", "selection", "selections"));
    }

    #[test]
    fn unknown_and_empty_fall_back_to_generic() {
        assert_eq!(genre_unit(""), ("work", "section", "sections"));
        assert_eq!(genre_unit("future_type"), ("work", "section", "sections"));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins genre_unit -- --nocapture`
Expected: FAIL — `cannot find function genre_unit`.

- [ ] **Step 3: Add the implementation** — insert just above `pub static JOURNAL_QA_PROMPT` in `src/gloss.rs`:

```rust
/// Genre vocabulary for a work's `work_type`: `(genre, unit, units_plural)`.
/// Used to parameterize the journal Q&A prompt and user message so a novel is
/// discussed in terms of chapters, an epic in terms of books, etc., rather than
/// the play/scene defaults. Unknown or empty types fall back to the neutral
/// (work, section, sections). The genre noun is independent of
/// `line_types::is_prose_work`; this is the single source for the genre word
/// (note lit.db stores `prose`, not `novel`).
pub fn genre_unit(work_type: &str) -> (&'static str, &'static str, &'static str) {
    match work_type {
        "play" => ("play", "scene", "scenes"),
        "prose" | "prose_book" => ("novel", "chapter", "chapters"),
        "bible_book" => ("book", "chapter", "chapters"),
        "epic" | "epic_translation" => ("epic poem", "book", "books"),
        "narrative_poem" => ("narrative poem", "section", "sections"),
        "poem" => ("poem", "section", "sections"),
        "sonnet_sequence" => ("sequence", "sonnet", "sonnets"),
        "verse_essay" => ("essay", "section", "sections"),
        "essay_collection" => ("collection", "essay", "essays"),
        "anthology" => ("anthology", "selection", "selections"),
        _ => ("work", "section", "sections"),
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --bins genre_unit -- --nocapture`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/gloss.rs
git commit -m "feat(gloss): genre_unit lookup for work-type-aware prompts"
```

---

### Task 2: `journal_qa_prompt(work_type)` replaces the static prompt

**Files:**
- Modify: `src/gloss.rs` — replace `pub static JOURNAL_QA_PROMPT: LazyLock<String>` (~lines 148–162) with a function; rewrite the FALLBACK to use placeholders.
- Modify: `src/input/actions/journal.rs` — both call sites (the `JOURNAL_QA_PROMPT.to_string()` at the `ask_claude` request ~line 509 and the `submit_edit_rewrite` request ~line 629).

**Interfaces:**
- Consumes: `genre_unit` (Task 1).
- Produces: `pub fn journal_qa_prompt(work_type: &str) -> String`.

- [ ] **Step 1: Write the failing test** — add to `src/gloss.rs` (new test mod or extend Task 1's):

```rust
#[cfg(test)]
mod journal_qa_prompt_tests {
    use super::journal_qa_prompt;

    #[test]
    fn prose_prompt_says_novel_and_chapter_not_play() {
        let p = journal_qa_prompt("prose");
        assert!(p.contains("novel"), "expected 'novel' in: {p}");
        assert!(p.contains("chapter"), "expected 'chapter' in: {p}");
        assert!(!p.contains("a play"), "should not call a novel a play: {p}");
        // No leftover unsubstituted tokens.
        assert!(!p.contains("{genre}") && !p.contains("{unit}") && !p.contains("{units}"));
    }

    #[test]
    fn play_prompt_still_says_play_and_scene() {
        let p = journal_qa_prompt("play");
        assert!(p.contains("play") && p.contains("scene"));
        assert!(!p.contains("{genre}"));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins journal_qa_prompt -- --nocapture`
Expected: FAIL — `cannot find function journal_qa_prompt`.

- [ ] **Step 3: Replace the static with the function** — in `src/gloss.rs`, replace the whole `pub static JOURNAL_QA_PROMPT … });` block with:

```rust
/// Assemble the journal Q&A system prompt for a work of `work_type`. Reads the
/// active DB template (`journal.qa`) or the compiled FALLBACK, then substitutes
/// the genre vocabulary `{genre}` / `{unit}` / `{units}` from `genre_unit`. Was a
/// `LazyLock<String>` before genre-awareness; now resolved per request because
/// the substitution depends on the work. (DB prompt changes still need a restart
/// — `active_prompt` is read each call, but `run_claude_request` is invoked once
/// per ask, so per-call resolution is cheap.)
pub fn journal_qa_prompt(work_type: &str) -> String {
    const FALLBACK: &str = "\
You are a literary interlocutor in conversation with a reader who is working through a {genre}, one {unit} at a time. The reader has asked a question while reading a specific {unit}. The verbatim text of that {unit} is provided.

Answer the question substantively and in plain prose. Ground your answer in the {unit} text provided, but DO situate the {unit} within the whole {genre}: trace how this moment echoes earlier {units} and foreshadows or is answered by later ones, and how it participates in the work's larger arcs of character, theme, and image. Drawing such connections across the full {genre} is encouraged — this is a study companion for a reader engaging the entire work, not a spoiler-free first-read assistant, so do not withhold connections to later {units}.

Open the answer with a single-sentence first paragraph that serves as a prologue to the rest: one sentence, standing alone as its own paragraph, that hooks the reader and previews the gist or direction of the answer without unpacking it. This opening paragraph MUST be exactly one sentence — no more — and must be followed by a blank line before the body of the answer begins. The remaining paragraphs then develop the answer in full.

Keep the body paragraphs short. Each body paragraph should run two to four sentences, and you must start a new paragraph whenever the topic shifts — a new work, a new period, a new character, a new strand of the argument. Never let a paragraph grow into a long block; when in doubt, break sooner rather than later. Separate every paragraph with a blank line.

NEVER quote the source text. Do not reproduce any wording from the work's prose or verse, whether inside quotation marks or not, and do not set off phrases from the text in quotes. Refer to moments, images, and speeches by describing or paraphrasing them in your own words. (Proper nouns — the work's title, place names, and character names — are not source quotation and may be used normally.) If a precise phrase from the text seems essential, paraphrase its sense rather than reproducing it.

Write for a thoughtful reader: clear, specific, and concrete. No markdown, no bullet lists, no numbered lists, no headers — flowing prose paragraphs only. Do not use the = sign; write paraphrases as prose. Be substantive but not padded.";
    let (genre, unit, units) = genre_unit(work_type);
    template_or("journal.qa", FALLBACK)
        .replace("{genre}", genre)
        .replace("{units}", units)
        .replace("{unit}", unit)
}
```

Note: replace `{units}` BEFORE `{unit}` so the plural token is not half-consumed by the singular replace (`{unit}` is a prefix of `{units}`).

- [ ] **Step 4: Update the two call sites** in `src/input/actions/journal.rs`.

In `submit_edit_rewrite` (the block that captures `(edit_id, model, context)` ~line 470), also capture the work type. Change:

```rust
    let (edit_id, model, context) = {
        let s = state.borrow();
        let Some(p) = s.journal.pages.get(s.journal.page_index) else {
            return;
        };
```

to:

```rust
    let (edit_id, model, context, work_type) = {
        let s = state.borrow();
        let Some(p) = s.journal.pages.get(s.journal.page_index) else {
            return;
        };
        let work_type = s
            .current_work
            .as_ref()
            .map(|w| w.work_type.clone())
            .unwrap_or_default();
```

and change the tuple return at the end of that block from `(p.id, model, context)` to `(p.id, model, context, work_type)`.

Then change that path's request prompt from:

```rust
        crate::gloss::JOURNAL_QA_PROMPT.to_string(),
```

to:

```rust
        crate::gloss::journal_qa_prompt(&work_type),
```

In `ask_claude`, the capture block already binds `s.current_work`; add `work_type` to the destructured tuple. Change the let-binding header:

```rust
    let (work_title, work_author, work_abbrev, band, scene_text, model) = {
```

to:

```rust
    let (work_title, work_author, work_abbrev, work_type, band, scene_text, model) = {
```

Inside, where `(title, author, abbrev)` is matched from `s.current_work`, also pull `w.work_type.clone()`; the simplest is to add a separate line after that match:

```rust
        let work_type = s
            .current_work
            .as_ref()
            .map(|w| w.work_type.clone())
            .unwrap_or_default();
```

and add `work_type` to the returned tuple (between `abbrev` and `band`). Then change `ask_claude`'s request prompt from:

```rust
        crate::gloss::JOURNAL_QA_PROMPT.to_string(),
```

to:

```rust
        crate::gloss::journal_qa_prompt(&work_type),
```

(Leave the user-message wording for Task 3; this task only swaps the system prompt and threads `work_type`.)

- [ ] **Step 5: Run tests + build**

Run: `cargo test --bins journal_qa_prompt -- --nocapture && cargo build`
Expected: tests PASS; build clean (no `JOURNAL_QA_PROMPT` references remain — grep to confirm: `rg -n "JOURNAL_QA_PROMPT" src/` returns nothing).

- [ ] **Step 6: Commit**

```bash
git add src/gloss.rs src/input/actions/journal.rs
git commit -m "feat(journal): work-type-aware Q&A system prompt"
```

---

### Task 3: Parameterize the user messages

**Files:**
- Modify: `src/input/actions/journal.rs` — `ask_claude`'s `user_msg` match (~lines 602–624) and `rewrite_context` (~lines 368–402).

**Interfaces:**
- Consumes: `genre_unit` (Task 1); `work_type` already threaded into `ask_claude` (Task 2). `rewrite_context` gains a `work_type` parameter.

- [ ] **Step 1: Add a title-case helper test** — in `src/input/actions/journal.rs` test mod, add:

```rust
    #[test]
    fn title_case_first_letter() {
        assert_eq!(super::titlecase_first("chapter"), "Chapter");
        assert_eq!(super::titlecase_first("scene"), "Scene");
        assert_eq!(super::titlecase_first(""), "");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins title_case_first_letter -- --nocapture`
Expected: FAIL — `cannot find function titlecase_first`.

- [ ] **Step 3: Add the helper** near the top of `src/input/actions/journal.rs` (after the `use` lines):

```rust
/// Capitalize the first character of `s` (ASCII), leaving the rest unchanged.
/// Used to turn a unit noun (`chapter`) into a user-message field label
/// (`Chapter:`). Empty input returns empty.
fn titlecase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --bins title_case_first_letter -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Rewrite `ask_claude`'s `user_msg`** — replace the `let user_msg = match band { … };` block (~lines 602–624) with:

```rust
    let (genre, unit, _units) = crate::gloss::genre_unit(&work_type);
    let unit_label = titlecase_first(unit);
    let user_msg = match band {
        JournalBand::Work => format!(
            "Work type: {}\nWork: {} by {}\n\nReader's question about the {} as a whole:\n{}",
            genre, work_title, work_author, genre, question,
        ),
        JournalBand::Scene(d1, d2) => format!(
            "Work type: {}\nWork: {} by {}\n{}: {}\n\n{} text:\n{}\n\nReader's question:\n{}",
            genre,
            work_title,
            work_author,
            unit_label,
            crate::app::scene_synopsis::scene_label(d1, d2),
            unit_label,
            scene_text,
            question,
        ),
        JournalBand::Passage { div1, div2, .. } => format!(
            "Work type: {}\nWork: {} by {}\n{}: {}\n\n{} text:\n{}\n\nPassage:\n{}\n\nReader's question:\n{}",
            genre,
            work_title,
            work_author,
            unit_label,
            crate::app::scene_synopsis::scene_label(div1, div2),
            unit_label,
            scene_text,
            passage_source_text,
            question,
        ),
    };
```

(`unit_label` is bound once; `_units` is genuinely unused here so the underscore is correct.)

- [ ] **Step 6: Parameterize `rewrite_context`** — change its signature to take `work_type`, and relabel "Scene text:". Replace the function body's `match band { … }` field strings:

Change the signature:

```rust
fn rewrite_context(
    s: &AppState,
    band: &JournalBand,
    anchor_work_line: usize,
    passage_source: &str,
) -> String {
```

to:

```rust
fn rewrite_context(
    s: &AppState,
    band: &JournalBand,
    work_type: &str,
    anchor_work_line: usize,
    passage_source: &str,
) -> String {
```

Add after the `(title, author)` binding:

```rust
    let (_genre, unit, _units) = crate::gloss::genre_unit(work_type);
    let unit_label = titlecase_first(unit);
```

Change the Scene arm format from:

```rust
            format!(
                "Work: {} by {}\nThis Q&A is filed under: {}\n\nScene text:\n{}",
                title, author, crate::app::scene_synopsis::scene_label(*d1, *d2), scene_text,
            )
```

to:

```rust
            format!(
                "Work: {} by {}\nThis Q&A is filed under: {}\n\n{} text:\n{}",
                title, author, crate::app::scene_synopsis::scene_label(*d1, *d2), unit_label, scene_text,
            )
```

Change the Passage arm format from:

```rust
            format!(
                "Work: {} by {}\nThis Q&A is filed under a PASSAGE in {}\n\nScene text:\n{}\n\nPassage:\n{}",
                title, author, crate::app::scene_synopsis::scene_label(*div1, *div2),
                scene_text, passage_source,
            )
```

to:

```rust
            format!(
                "Work: {} by {}\nThis Q&A is filed under a PASSAGE in {}\n\n{} text:\n{}\n\nPassage:\n{}",
                title, author, crate::app::scene_synopsis::scene_label(*div1, *div2),
                unit_label, scene_text, passage_source,
            )
```

- [ ] **Step 7: Update the `rewrite_context` caller** — in `submit_edit_rewrite`, where `context` is built (~line 491), pass `work_type`:

Change:

```rust
        let context = rewrite_context(&s, &band, anchor_work_line, &passage_source);
```

to:

```rust
        let context = rewrite_context(&s, &band, &work_type, anchor_work_line, &passage_source);
```

(`work_type` is captured in this block from Task 2's edit.)

- [ ] **Step 8: Fix the existing `rewrite_context` unit tests** — the test mod calls `rewrite_context` with the old 4-arg signature. Find them (`rg -n "rewrite_context\(" src/input/actions/journal.rs`) and add a `work_type` argument. For a play-context test pass `"play"`; the asserted substring `"Scene text:"` stays valid for `"play"` (unit = scene → label "Scene"). If a test asserts on `"Scene text:"`, leave the work_type as `"play"` so the label is unchanged and the assertion holds.

- [ ] **Step 9: Run tests + build**

Run: `cargo test --bins -- --nocapture 2>&1 | rg "test result|error\[" && cargo build`
Expected: all bins tests PASS; build clean.

- [ ] **Step 10: Commit**

```bash
git add src/input/actions/journal.rs
git commit -m "feat(journal): genre-aware Q&A user messages (Chapter/Scene/etc.)"
```

---

### Task 4: Verify the assembled prompt for a prose work (logic gate)

**Files:** none (verification only).

- [ ] **Step 1: Full bins suite + clippy parity**

Run: `cargo test --bins 2>&1 | rg "test result"`
Expected: all PASS (the suite count is +4 from Tasks 1 & 3 new tests over the pre-feature baseline).

Run: `cargo clippy 2>&1 | rg -c "^warning: "`
Expected: equals the pre-feature baseline count (no new warnings). Compare with `git stash`/`git stash pop` against the merge base if unsure.

- [ ] **Step 2: Confirm no stale references**

Run: `rg -n "JOURNAL_QA_PROMPT" src/`
Expected: no matches (the static is gone).

Run: `rg -n "the play as a whole|Scene:|Scene text:" src/input/actions/journal.rs`
Expected: no literal `play`/`Scene` field strings remain except inside test fixtures that intentionally use `"play"`.

- [ ] **Step 3: Commit (if any doc/notes touched)** — usually nothing to commit here.

---

### Task 5: DB prompt v4 in the claude-api-prompts repo

**Files:**
- Create: `~/utono/claude-api-prompts/prompts/journal.qa.md` (new master; the key has no master today).

**Pre-req:** linux-lit must be CLOSED for the duration of this task (the running app rewrites config on exit and reads the prompt at launch; the sync writes lit.db). Ask the user to close it before syncing.

- [ ] **Step 1: Inspect an existing master for the frontmatter shape**

Run: `cat ~/utono/claude-api-prompts/prompts/synopsis.amend.md`
Expected: shows the YAML frontmatter keys (`prompt_key`, etc.) + body convention to mirror.

- [ ] **Step 2: Create `prompts/journal.qa.md`** — frontmatter matching the repo convention (copy the key names from Step 1; set `prompt_key: journal.qa`), body = the FALLBACK text from Task 2 Step 3 VERBATIM (same `{genre}`/`{unit}`/`{units}` placeholders). The body and the linux-lit FALLBACK must be byte-identical (the repo invariant). Confirm with:

Run: `rg -n "\{genre\}|\{unit\}|\{units\}" ~/utono/claude-api-prompts/prompts/journal.qa.md`
Expected: the placeholders are present.

- [ ] **Step 3: Commit the master FIRST** (the commit subject becomes the DB version note):

```bash
cd ~/utono/claude-api-prompts
git add prompts/journal.qa.md
git commit -m "journal.qa v4: work-type placeholders ({genre}/{unit}/{units})"
```

- [ ] **Step 4: Sync to lit.db (writes v4 active, demotes v3)** — with linux-lit closed:

```bash
cd ~/utono/claude-api-prompts
python scripts/sync-to-db.py journal.qa
```

Expected: output reports a new active version (v4) for `journal.qa`.

- [ ] **Step 5: Verify the active row**

Run: `python ~/utono/claude-api-prompts/scripts/list-versions.py journal.qa`
Expected: v4 marked active (`*`).

Run: `sqlite3 ~/utono/litdb/data/lit.db "SELECT version,is_active FROM api_prompts WHERE prompt_key='journal.qa' ORDER BY version;"`
Expected: v4 `is_active=1`, v1–v3 `is_active=0`.

- [ ] **Step 6: Render-check the assembled prompt**

Run: `rg -n "\{genre\}|\{unit\}" <(sqlite3 ~/utono/litdb/data/lit.db "SELECT text FROM api_prompts WHERE prompt_key='journal.qa' AND is_active=1;")`
Expected: placeholders present in the stored v4 (they are substituted at runtime by linux-lit, not in the DB).

---

### Task 6: Visual acceptance (user)

**Files:** none.

- [ ] **Step 1:** Ask the user to launch linux-lit (fresh, after the v4 sync), open *Bleak House*, open the journal overlay on the WHOLE-WORK band (Alt+w), press `A`, and ask: "When was Bleak House written? Put it in the context of the Charles Dickens corpus." 

- [ ] **Step 2:** Confirm the answer:
  - does NOT open with a genre correction ("not a play", "your instinct to read it scene by scene"),
  - refers to the work as a novel / its chapters where it refers to structure.

- [ ] **Step 3:** Spot-check a play (e.g. a Shakespeare work) is unchanged: a scene-band Q&A still talks in scenes.

---

## Notes for the implementer

- `{units}` must be replaced before `{unit}` (singular is a prefix of plural).
- The journal TTS feature already on this branch (`feat/journal-qa-tts`) is unrelated; keep these commits separate.
- If `cargo clippy` flags `titlecase_first` as unused before Task 3 Step 5 wires it, that resolves once the user messages call it — don't delete it.
