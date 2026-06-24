# Passage-scoped Journal Q&A — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a passage-scoped journal Q&A: ask Claude about a visually-selected passage; the journal page stores and displays the passage's source verse (italic stage directions) above the Q&A, and journal pages ↔ glosses on the same passage are linked (view or create the counterpart from either overlay).

**Architecture:** Extend `journal_entries` with citation range + source-verse columns; extract the gloss overlay's stage-aware verse renderer into a shared helper both overlays call; add a `JournalBand::Passage` and passage display; add a "Journal Q&A" visual-mode action mirroring the reader-gloss flow; add reciprocal create/view keybinds across the two overlays keyed on the shared citation pair.

**Tech Stack:** Rust, GTK4 (gtk4-rs / sourceview5), SQLite (rusqlite), the existing `claude_bridge` async request path.

## Branch

Work continues on `feat/journal-passage-qa` (spec already committed there).

## Global Constraints

- Do NOT run the app (`cargo run`) or launch a compositor; verify with `cargo build`, `cargo test --bins`, `cargo clippy`. Visual criteria are user-run.
- Reuse, don't duplicate: the passage verse render MUST reuse the gloss overlay's stage-aware renderer (extracted to a shared helper), so the `gloss-stage` italic-priority fix is inherited. The gloss overlay's existing tests (incl. `stage_tag_outranks_font_tag_after_apply_font`) MUST stay green.
- The journal page ↔ gloss link is the citation pair `(start_citation, end_citation)` in `ABBR.div1.div2.line_in_div` form — the same key `find_glosses_by_start` uses. No `passages`-table FK.
- Claude context for a passage question = the selected passage `source_text` + the enclosing scene text (`scene_text_for(d1,d2)`).
- Migration is additive + idempotent (mirror the existing `scope`-column `ALTER TABLE` guard in `ensure_journal_table`). Legacy scene/work pages leave new columns NULL.
- Any keybind add/change updates ALL THREE: `keymap.rs`, the keymap.json stow source (`~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`), and the Ctrl+/ overlay (`update-cairo-keybinds-overlay` skill / `src/ui/keybinds_overlay.rs`). Check `~/utono/rpd` for the GTK key name of any physical key.
- Commit trailer on every commit:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
  ```

---

## File map

- `src/db/journal.rs` — migration (3 columns), `JournalPage` fields, `save_passage_page`, `find_passage_pages`, SELECT updates, tests (Task 1).
- `src/ui/gloss_render.rs` (NEW) — the extracted shared stage-aware verse renderer (Task 2).
- `src/ui/gloss_overlay.rs` — call the extracted renderer (Task 2).
- `src/app/mod.rs` — `JournalBand::Passage` variant (Task 3).
- `src/ui/journal_overlay.rs` — passage display (verse-on-top via shared renderer) + passage position label (Task 4).
- `src/input/actions/journal.rs` — passage band navigation + `ask_claude` passage arm + `save_passage_page` save (Tasks 3, 5).
- `src/input/visual.rs` — `BUILTIN_ACTIONS` "Journal Q&A" + `execute_action` arm + `action_journal_qa` (Task 5).
- `src/input/keymap.rs` + keymap.json + `src/ui/keybinds_overlay.rs` — 4 reciprocal keybinds (Tasks 6, 7).

---

### Task 1: DB layer — passage columns, JournalPage fields, save/find

**Files:**
- Modify: `src/db/journal.rs` (struct lines 4-12; `ensure_journal_table` 14-40; SELECTs in `find_journal_pages`/`find_work_pages`/`find_all_pages_ordered`; add 2 fns; tests mod at 173)

**Interfaces:**
- Produces:
  - `JournalPage { …, pub start_citation: Option<String>, pub end_citation: Option<String>, pub source_text: Option<String> }`
  - `save_passage_page(conn, work_abbrev, div1, div2, start_cit, end_cit, source_text, question, answer, model) -> Result<i64>`
  - `find_passage_pages(conn, work_abbrev, start_cit, end_cit) -> Result<Vec<JournalPage>>` (scope='passage', matched by the citation pair)

- [ ] **Step 1: Write the failing test**

Add to the `tests` mod in `src/db/journal.rs`:

```rust
#[test]
fn passage_pages_roundtrip_and_isolate_from_scene_work() {
    let conn = mem();
    let id = save_passage_page(
        &conn, "2H6", 1, 4, "2H6.1.4.43", "2H6.1.4.50",
        "<speaker>YORK</speaker>\n<verse>Lay hands…</verse>\n<stage>[To Jourdain.]</stage>",
        "What is York doing?", "He arrests the conjurers.", "claude-opus-4-8",
    ).unwrap();
    assert!(id > 0);

    // A scene page and a work page in the same scene must NOT come back as passage pages.
    save_journal_page(&conn, "2H6", 1, 4, "SceneQ?", "SceneA.", "m", "scene").unwrap();
    save_journal_page(&conn, "2H6", -1, -1, "WorkQ?", "WorkA.", "m", "work").unwrap();

    let pages = find_passage_pages(&conn, "2H6", "2H6.1.4.43", "2H6.1.4.50").unwrap();
    assert_eq!(pages.len(), 1, "exactly the one passage page");
    let p = &pages[0];
    assert_eq!(p.question, "What is York doing?");
    assert_eq!(p.start_citation.as_deref(), Some("2H6.1.4.43"));
    assert_eq!(p.end_citation.as_deref(), Some("2H6.1.4.50"));
    assert!(p.source_text.as_deref().unwrap().contains("<stage>[To Jourdain.]</stage>"));

    // The passage page must NOT leak into scene/work queries.
    assert!(find_journal_pages(&conn, "2H6", 1, 4).unwrap().iter().all(|p| p.question != "What is York doing?"));
    assert!(find_work_pages(&conn, "2H6").unwrap().iter().all(|p| p.question != "What is York doing?"));

    // A different citation pair returns nothing.
    assert!(find_passage_pages(&conn, "2H6", "2H6.1.4.99", "2H6.1.4.99").unwrap().is_empty());
}

#[test]
fn passage_columns_migrate_idempotently() {
    let conn = mem();
    ensure_journal_table(&conn).unwrap(); // second call must not error
    for col in ["start_citation", "end_citation", "source_text"] {
        let has: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name=?1").unwrap()
            .exists([col]).unwrap();
        assert!(has, "column {col} should exist after ensure_journal_table");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins passage_pages_roundtrip_and_isolate_from_scene_work passage_columns_migrate_idempotently`
Expected: FAIL — `save_passage_page`/`find_passage_pages` not defined; `JournalPage` has no `start_citation`.

- [ ] **Step 3: Add the struct fields**

In `src/db/journal.rs`, extend `JournalPage` (after `timestamp`):

```rust
#[derive(Debug, Clone)]
pub struct JournalPage {
    pub id: i64,
    pub div1: i64,
    pub div2: i64,
    pub question: String,
    pub answer: String,
    pub claude_model: String,
    pub timestamp: String,
    pub start_citation: Option<String>,
    pub end_citation: Option<String>,
    pub source_text: Option<String>,
}
```

- [ ] **Step 4: Migrate the table (idempotent ALTER, mirroring the `scope` guard)**

In `ensure_journal_table`, after the existing `scope` ALTER guard, add the three columns the same way:

```rust
    for col in ["start_citation", "end_citation", "source_text"] {
        let has: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('journal_entries') WHERE name=?1")?
            .exists([col])?;
        if !has {
            conn.execute_batch(&format!(
                "ALTER TABLE journal_entries ADD COLUMN {col} TEXT;"
            ))?;
        }
    }
    Ok(())
```

(Also add the three columns to the `CREATE TABLE IF NOT EXISTS` body so fresh DBs have them: append `start_citation TEXT, end_citation TEXT, source_text TEXT,` before `timestamp`.)

- [ ] **Step 5: Update existing SELECTs to read the new columns**

`find_journal_pages`, `find_work_pages`, `find_all_pages_ordered` each `SELECT … timestamp` and build a `JournalPage`. Add the three columns to each SELECT and the row builder. For each, change the column list to:

```sql
SELECT id, div1, div2, question, answer, COALESCE(claude_model, ''), timestamp, start_citation, end_citation, source_text
```

and the row closure to add:

```rust
            start_citation: row.get(7)?,
            end_citation: row.get(8)?,
            source_text: row.get(9)?,
```

(`row.get::<_, Option<String>>` is inferred from the `Option<String>` field type.)

- [ ] **Step 6: Add `save_passage_page` and `find_passage_pages`**

```rust
pub fn save_passage_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    start_citation: &str,
    end_citation: &str,
    source_text: &str,
    question: &str,
    answer: &str,
    claude_model: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO journal_entries
            (work_abbrev, div1, div2, question, answer, claude_model, scope,
             start_citation, end_citation, source_text, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'passage', ?7, ?8, ?9, datetime('now'))",
        rusqlite::params![
            work_abbrev, div1, div2, question, answer, claude_model,
            start_citation, end_citation, source_text
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn find_passage_pages(
    conn: &Connection,
    work_abbrev: &str,
    start_citation: &str,
    end_citation: &str,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, div1, div2, question, answer, COALESCE(claude_model, ''), timestamp, \
                start_citation, end_citation, source_text \
         FROM journal_entries \
         WHERE work_abbrev = ?1 AND scope = 'passage' \
           AND start_citation = ?2 AND end_citation = ?3 \
         ORDER BY timestamp ASC, id ASC",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![work_abbrev, start_citation, end_citation],
        |row| Ok(JournalPage {
            id: row.get(0)?, div1: row.get(1)?, div2: row.get(2)?,
            question: row.get(3)?, answer: row.get(4)?, claude_model: row.get(5)?,
            timestamp: row.get(6)?, start_citation: row.get(7)?,
            end_citation: row.get(8)?, source_text: row.get(9)?,
        }),
    )?;
    rows.collect()
}
```

Also: the existing scene/work `find_*` queries must NOT return passage rows. `find_journal_pages` filters `scope='scene'` and `find_work_pages` filters `scope='work'`, so passage rows are already excluded — confirm those WHERE clauses are intact after your edits.

- [ ] **Step 7: Run tests to verify they pass + full suite**

Run: `cargo test --bins`
Expected: PASS — the 2 new tests plus the existing journal tests (which build a `JournalPage` and now must include the 3 new fields — update the existing test fixtures if any construct `JournalPage` literally; the existing tests use `save_journal_page` + `find_*` so they go through the row builder, no literal construction). 421 baseline + 2 new = 423.

- [ ] **Step 8: Commit**

```bash
git add src/db/journal.rs
git commit -m "$(cat <<'EOF'
feat(journal): passage scope — citation range + source_text columns, save/find

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 2: Extract the shared stage-aware verse renderer

**Files:**
- Create: `src/ui/gloss_render.rs`
- Modify: `src/ui/gloss_overlay.rs` (move `populate_gloss_buffer`/`populate_gloss_buffer_ex` out; call the shared fn), `src/ui/mod.rs` (add `pub(crate) mod gloss_render;`)

**Interfaces:**
- Produces: `crate::ui::gloss_render::populate_verse_buffer(view: &gtk4::TextView, doc: &str, bar_left: i32, source_line_numbers: &[(String, i64)], selected_echo: Option<usize>, dim_color: Option<&str>, speaker_accent: Option<&str>) -> (Vec<BarRange>, Vec<LineNumber>, Vec<i32>)` — the relocated `populate_gloss_buffer_ex`, plus `BarRange`/`LineNumber` (moved or re-exported).

This is a PURE REFACTOR — no behavior change. The gloss overlay must render byte-identically; its tests (incl. `stage_tag_outranks_font_tag_after_apply_font`) stay green.

- [ ] **Step 1: Create the module with the moved functions**

Create `src/ui/gloss_render.rs`. Move `populate_gloss_buffer_ex` (gloss_overlay.rs:1812-2086) and the thin `populate_gloss_buffer` wrapper into it, renaming the public entry to `populate_verse_buffer` (keep the wrapper name internal if the overlay still uses it). Move the `BarRange`/`LineNumber` structs they return (or re-export from gloss_overlay). Make the entry `pub(crate)`. Bring needed imports (`parse_gloss_tags`, `GlossElement`, `strip_ipa`, `apply_bracket_styling`, `split_echo`, `card_side_margin` if used, `pango`, `gtk4::prelude::*`). `apply_bracket_styling` (gloss_overlay.rs) must also move or become `pub(crate)`.

- [ ] **Step 2: Point the gloss overlay at the shared fn**

In `gloss_overlay.rs`, replace the bodies of the (now-removed) local functions with calls to `crate::ui::gloss_render::populate_verse_buffer(...)`. Every call site that used `populate_gloss_buffer`/`populate_gloss_buffer_ex` now calls the shared fn with the same args. Register the module in `src/ui/mod.rs`.

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: PASS. Resolve any visibility errors by making the moved helpers `pub(crate)`.

- [ ] **Step 4: Run the gloss overlay's tests (no behavior change)**

Run: `cargo test --bins`
Expected: PASS — `stage_tag_outranks_font_tag_after_apply_font` and all others green. Count unchanged from Task 1 (423).

- [ ] **Step 5: Confirm no duplication / clippy**

Run: `rg -n "fn populate_gloss_buffer_ex" src/` — expect ONE definition (in gloss_render.rs).
Run: `cargo clippy` — no new warnings.

- [ ] **Step 6: Commit**

```bash
git add src/ui/gloss_render.rs src/ui/gloss_overlay.rs src/ui/mod.rs
git commit -m "$(cat <<'EOF'
refactor(ui): extract shared stage-aware verse renderer to gloss_render

Pure move of populate_gloss_buffer_ex so the journal overlay can reuse the
gloss overlay's <speaker>/<verse>/<stage> rendering (incl. italic stage tags).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 3: `JournalBand::Passage` + band plumbing

**Files:**
- Modify: `src/app/mod.rs` (`JournalBand` enum at ~141), `src/input/actions/journal.rs` (`footer_left_text`, `ask_claude` save-scope match, page-reload match — every `match band` that today has only `Work`/`Scene`)

**Interfaces:**
- Produces: `JournalBand::Passage { div1: i64, div2: i64, start: String, end: String }` (carries the scene divs + the citation pair).

- [ ] **Step 1: Add the variant**

In `src/app/mod.rs`, the enum currently derives `#[derive(Clone, Copy, Debug, PartialEq, Eq)]`. `Passage` holds `String`s, so **drop `Copy`** (a `String` field is not `Copy`); `Clone`/`Debug`/`PartialEq`/`Eq` all still hold (`String: Eq`). Result:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JournalBand {
    Work,
    Scene(i64, i64),
    Passage { div1: i64, div2: i64, start: String, end: String },
}
```

Then fix every `let band = s.journal_band;` (a `Copy` move) to `.clone()`, and any other place that relied on `JournalBand: Copy` (the build-driven step below finds them as "use of moved value").

- [ ] **Step 2: Build to find every non-exhaustive `match band`**

Run: `cargo build 2>&1 | rg "non-exhaustive|match band|JournalBand|use of moved"`
Expected: errors at each `match` on `JournalBand` (footer_left_text, ask_claude scope match, page-reload match, nav fns) and any `Copy` use. Fix each:
- `footer_left_text`: `Passage { div1, div2, .. } => format!("{} {}.{} passage", abbrev, div1, div2)`.
- The save-scope match in `ask_claude` is handled in Task 5 (passage save). For THIS task, add a `Passage { .. } => unreachable!()` or route to the passage save stub so it compiles; Task 5 fills it.
- The page-reload match: `Passage { start, end, .. } => find_passage_pages(&conn, &work_abbrev, start, end).ok()`.

- [ ] **Step 3: Build clean**

Run: `cargo build`
Expected: PASS (all `JournalBand` matches exhaustive; `.clone()` where needed).

- [ ] **Step 4: Run tests**

Run: `cargo test --bins`
Expected: PASS, 423 (no behavior change yet — the Passage band is constructed in Task 5).

- [ ] **Step 5: Commit**

```bash
git add src/app/mod.rs src/input/actions/journal.rs
git commit -m "$(cat <<'EOF'
feat(journal): JournalBand::Passage variant + band plumbing

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 4: Journal overlay — render verse above Q&A for passage pages

**Files:**
- Modify: `src/ui/journal_overlay.rs` (`show_page` at 134-170; add a `show_passage_page` or extend `show_page` with an optional `source_text`)

**Interfaces:**
- Consumes: `crate::ui::gloss_render::populate_verse_buffer` (Task 2).
- Produces: when a page carries `source_text`, the overlay renders the parsed verse (italic stage directions) above the Q&A; scene/work pages render unchanged.

No standalone unit test (GTK render is visual, verified by the user). Deliverable: clean compile + the journal overlay uses the shared renderer for passage verse.

- [ ] **Step 1: Add a passage-aware render entry**

Add `show_passage_page(&self, footer_left, page_index, page_count, source_text, question, answer, card_width, card_height)`. It:
- sizes the card,
- renders `source_text` into a region via `populate_verse_buffer` (the verse), then appends a blank line + a rule + `question\n\n answer` — OR renders the verse into the existing `view` buffer first, then inserts the Q&A text after it with the existing plain styling. Simplest: build the combined buffer by (a) `populate_verse_buffer(&self.view, source_text, bar_left, &[], None, None, Some(accent))` to lay the verse, then (b) `self.view.buffer().insert(&mut end, "\n\n———\n\n{question}\n\n{answer}")` after it.
- calls `self.apply_font()`, which MUST re-assert the italic tags' priority — see next paragraph.
- sets the position label to "passage {div1}.{div2}.{start–end}".

**CONFIRMED required:** the journal overlay's `apply_font` (journal_overlay.rs) builds a buffer-wide `journal-font` tag via `.font("Family Size")` (upright style) and applies it over the whole buffer — the EXACT pattern that flattened stage italics in the gloss overlay, and it currently has NO italic re-assertion. So after applying the verse via `populate_verse_buffer`, the stage directions would render upright. Add the same re-assertion the gloss overlay's `apply_font` got (gloss_overlay.rs:444) to the journal overlay's `apply_font`, inside its per-view loop after applying `journal-font`:

```rust
            let top = table.size();
            for italic in ["gloss-stage", "gloss-bracket"] {
                if let Some(t) = table.lookup(italic) {
                    if top > 0 {
                        t.set_priority(top - 1);
                    }
                }
            }
```

(The verse tags are named `gloss-stage`/`gloss-bracket` because `populate_verse_buffer` builds them with those names — shared from Task 2's renderer.)

- [ ] **Step 2: Route passage pages to the new entry**

In `journal.rs` `render_current` (the fn that calls `show_page`), when the current band is `Passage` (or the current page has `source_text`), call `show_passage_page` with the page's `source_text`/`question`/`answer`; otherwise call the existing `show_page`. Grep `render_current` and the `show_page(` call sites.

- [ ] **Step 3: Build + tests + clippy**

Run: `cargo build && cargo test --bins && cargo clippy`
Expected: PASS, 423, no new warnings.

- [ ] **Step 4: Commit**

```bash
git add src/ui/journal_overlay.rs src/input/actions/journal.rs
git commit -m "$(cat <<'EOF'
feat(journal): render source verse (italic stage dirs) above Q&A for passage pages

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 5: "Journal Q&A" visual action — create a passage page from a selection

**Files:**
- Modify: `src/input/visual.rs` (`BUILTIN_ACTIONS` 129; `execute_action` 22-39; add `fn action_journal_qa`), `src/input/actions/journal.rs` (a passage-aware `ask_claude` path + `save_passage_page` save)

**Interfaces:**
- Consumes: `build_context_for_type`/`build_source_header` (the reader-gloss template), `JOURNAL_QA_PROMPT`, `claude_bridge::run_claude_request`, `save_passage_page` (Task 1), `JournalBand::Passage` (Task 3).
- Produces: a new visual-mode action that creates a passage-scoped journal page.

No pure unit test (the flow is async + GTK + Claude). Deliverable: the action builds the passage context, opens the journal ask card, sends passage+scene to Claude, saves a passage page, and shows it.

- [ ] **Step 1: Add the menu item**

In `src/input/visual.rs`:

```rust
pub const BUILTIN_ACTIONS: &[&str] = &["Reader Gloss", "Gloss with Claude", "Inner Monologue", "Journal Q&A", "Copy", "Copy with metadata"];
```

(Inserting before "Copy" keeps the gloss actions grouped. This shifts Copy to index 4 and Copy-with-metadata to index 5.)

- [ ] **Step 2: Update `execute_action` indices**

In `execute_action` (visual.rs:23-39), renumber to match the new array:

```rust
        match index {
            0 => { action_reader_gloss(state_rc); return; }
            1 => { action_gloss_with_claude(state_rc); return; }
            2 => { action_inner_monologue(state_rc); return; }
            3 => { action_journal_qa(state_rc); return; }
            4 => action_copy(&mut state_rc.borrow_mut(), false),
            5 => action_copy(&mut state_rc.borrow_mut(), true),
            _ => {}
        }
```

- [ ] **Step 3: Implement `action_journal_qa`**

Mirror `action_reader_gloss`'s Phase 1 (build `selected_lines`, `ctx` via `build_context_for_type(work, &selected_lines, "reader-gloss")` — reuse it just for the citation/source_text/speaker fields, the gloss_type label is irrelevant here; `passage_doc = build_source_header(&selected_lines, &ctx.speaker)`). Then, instead of the gloss overlay:
- store the passage context for the pending ask (a small `s.journal.pending_passage: Option<PendingPassage>` carrying `start_citation`, `end_citation`, `div1`, `div2`, `source_text=passage_doc`, `speaker`),
- exit visual mode, open the journal overlay in a new `Passage` band (`s.journal_band = JournalBand::Passage { div1, div2, start, end }`), set `input_mode = JournalOverlay`,
- open the ask card titled "Ask about this passage" (a new `JournalPromptMode` is not needed; reuse `Ask` but key the save on the band being `Passage`).

On submit, `ask_claude` (extended in Step 4) detects the `Passage` band and routes to the passage save.

- [ ] **Step 4: Extend `ask_claude` for the passage band**

In `journal.rs` `ask_claude`, add the `Passage` arm to the context + save matches:
- Context (user_msg): `format!("Work: {} by {}\nScene: {}\n\nScene text:\n{}\n\nPassage:\n{}\n\nReader's question:\n{}", title, author, scene_label, scene_text, passage_source_text, question)` — passage + enclosing scene.
- Save callback: `JournalBand::Passage { div1, div2, start, end } => save_passage_page(&conn, &work_abbrev, *div1, *div2, start, end, &passage_source_text, &question_owned, &answer, &model_for_db)`.
- Page reload: `find_passage_pages(&conn, &work_abbrev, start, end)`.

(The `passage_source_text` comes from `s.journal.pending_passage` captured before the async call.)

- [ ] **Step 5: Build + tests + clippy**

Run: `cargo build && cargo test --bins && cargo clippy`
Expected: PASS, 423, no new warnings.

- [ ] **Step 6: Commit**

```bash
git add src/input/visual.rs src/input/actions/journal.rs src/app/mod.rs
git commit -m "$(cat <<'EOF'
feat(journal): "Journal Q&A" visual action creates a passage-scoped page

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 6: Reciprocal create — journal-from-gloss and gloss-from-journal

**Files:**
- Modify: `src/input/keymap.rs` (gloss-overlay handler ~796-1064: add `J`; journal-overlay handler ~678-794: add a create-gloss key), `src/input/actions/gloss.rs` and/or `journal.rs` (the two create handlers)

**Interfaces:**
- Consumes: `gloss_context` (carries `start_citation`/`end_citation`/`source_text`) for journal-from-gloss; the current journal passage page's stored citations/source_text for gloss-from-journal.

- [ ] **Step 1: create-journal-from-gloss (`J` in the gloss overlay)**

Add a `"J"` arm to the gloss overlay key handler. It reads `s.gloss_context` (start/end citation, source_text, speaker, divs), sets up `s.journal.pending_passage` from those, opens the journal overlay in the `Passage` band for that citation pair, and opens the ask card — reusing the Task 5 passage-ask machinery (factor the "open passage ask for a given passage context" into a shared fn both Task 5 and this call).

- [ ] **Step 2: create-gloss-from-journal (a key in the journal overlay)**

Add a key (proposed `g`-less to avoid the gg chord — use a free bare key like `G`-adjacent or a Ctrl combo; per refs `Ctrl+g` is free in the journal overlay but reserved in Step-Task-7 for VIEW. Use a distinct create key, e.g. `Alt+g`). When the current journal page is a passage page, build the reader-gloss creation from its stored `source_text`/citations (reuse `action_reader_gloss`'s save path, or call a shared "create reader gloss for this passage context" fn). If the current page is not a passage page, toast "Not a passage page".

- [ ] **Step 3: Update keymap.json + Ctrl+/ overlay**

Add both binds to `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` and run the `update-cairo-keybinds-overlay` skill to add their `describe()` arms. (See Task 7 for the overlay specifics — do the overlay updates for all four binds together in Task 7; here just keymap.rs + keymap.json.)

- [ ] **Step 4: Build + tests + clippy**

Run: `cargo build && cargo test --bins && cargo clippy`
Expected: PASS, 423, no new warnings.

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs src/input/actions/gloss.rs src/input/actions/journal.rs
git commit -m "$(cat <<'EOF'
feat(journal): reciprocal create — J (journal from gloss), Alt+g (gloss from journal)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 7: Reciprocal view-toggle + keybinds overlay

**Files:**
- Modify: `src/input/keymap.rs` (gloss overlay: `Ctrl+j`; journal overlay: `Ctrl+g`), the two view handlers, keymap.json, `src/ui/keybinds_overlay.rs`

**Interfaces:**
- Consumes: `find_glosses_by_start` (journal→gloss view), `find_passage_pages` (gloss→journal view), `open_gloss_overlay` / journal passage-band open.

- [ ] **Step 1: view-gloss-from-journal (`Ctrl+g` in the journal overlay)**

When the current journal page is a passage page, parse its `start_citation` → look up `find_glosses_by_start(work_abbrev, start_citation, &["reader-gloss", "teacher-generic", "inner-monologue"])`. If non-empty, build the `GlossedPassage` + open the gloss overlay (`open_gloss_overlay`) on it; else `toast::show_transient(&s.chapter_toast, "No gloss for this passage", 3)`.

- [ ] **Step 2: view-journal-from-gloss (`Ctrl+j` in the gloss overlay)**

From the gloss overlay, read `gloss_context` citations → `find_passage_pages(work_abbrev, start, end)`. If non-empty, open the journal overlay in the `Passage` band on that pair (showing the first page); else `toast "No journal page for this passage"`.

- [ ] **Step 3: keymap.json + Ctrl+/ overlay for ALL FOUR binds**

Add `J`, `Alt+g`, `Ctrl+j`, `Ctrl+g` to `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`. Then run the `update-cairo-keybinds-overlay` skill to add the four `describe()` arms in `src/ui/keybinds_overlay.rs`'s Gloss/journal section (per refs §7: the `g` key UPPER_ROW and `j` key BOTTOM_ROW modifier slices, and the `describe()` block ~336-378). The four labels: "journal from gloss" (`J`), "gloss from journal" (`Alt+g`), "view journal" (`Ctrl+j`), "view gloss" (`Ctrl+g`). Follow the skill's mandatory three-pass cross-reference (no blank slot, no wrong label, every label has a describe arm).

- [ ] **Step 4: Build + tests + clippy + overlay-skill verification**

Run: `cargo build && cargo test --bins && cargo clippy`
Expected: PASS, 423, no new warnings. The keybinds-overlay skill's exhaustive check passes (every new bind described).

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs src/input/actions/gloss.rs src/input/actions/journal.rs src/ui/keybinds_overlay.rs
git commit -m "$(cat <<'EOF'
feat(journal): reciprocal view-toggle (Ctrl+j / Ctrl+g) + keybinds overlay

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
EOF
)"
```

---

### Task 8: Full verification + user visual gate

**Files:** none (verification only).

- [ ] **Step 1: Full build + suite + clippy**

Run: `cargo build && cargo test --bins && cargo clippy`
Expected: all PASS, no new clippy warnings, the gloss overlay's `stage_tag_outranks_font_tag_after_apply_font` still green, journal DB tests green.

- [ ] **Step 2: Deploy the keymap.json stow + ask the user to run the visual gate**

Tell the user to `cd ~/tty-dotfiles && stow linux-lit` (so the new binds load), then verify on a stage-bearing work (e.g. 2H6 1.4):
1. Visual-select a passage → action popup → "Journal Q&A" → ask a question → the journal page shows the **source verse with italic stage directions** above the Q&A.
2. From that journal page, `Alt+g` creates a gloss for the passage; `Ctrl+g` views an existing gloss (or toasts "No gloss for this passage").
3. From a gloss overlay on the same passage, `J` creates a journal page; `Ctrl+j` views the journal page (or toasts "No journal page for this passage").
4. The Ctrl+/ overlay shows the four new binds with descriptions.

Provide the manual launch from CLAUDE.md Headless Verification for eyeballing.

- [ ] **Step 3: Finish the branch**

After the user confirms, merge `feat/journal-passage-qa` `--no-ff` to master, re-verify, push, delete the branch.

---

## Self-Review

**Spec coverage:**
- §1 data model (citation range + source_text columns, JournalPage fields, save/find) → Task 1. ✓
- §2 entry points: (a) visual action → Task 5; (b) journal-from-gloss `J` → Task 6; (c) gloss-from-journal → Task 6. ✓
- §3 display (verse on top via shared renderer) → Task 2 (extract) + Task 4 (consume). ✓
- §4 passage band + view toggles + "no counterpart" toasts → Task 3 (band) + Task 7 (toggles/toasts). ✓
- Claude context = passage + scene → Task 5 Step 4. ✓
- Keybind coverage (keymap.rs + keymap.json + Ctrl+/ overlay) → Tasks 6, 7. ✓
- Testing (DB round-trip + migration; gloss tests stay green; visual gate) → Tasks 1, 2, 8. ✓
- Out-of-scope (re-derive verse, passages FK, cross-work, side-by-side) — none implemented. ✓

**Placeholder scan:** No TBD/"handle edge cases". Each code step shows complete code or an exact transformation. Two deliberately-deferred specifics, both grounded not vague: the create-gloss-from-journal key is pinned to `Alt+g` (with the reason `Ctrl+g` is reserved for view); the journal overlay's `apply_font` italic re-assertion (Task 4 Step 1) is conditional on reading that fn first — the instruction says to read it and mirror the gloss fix, which is concrete.

**Type consistency:** `JournalPage`'s 3 new `Option<String>` fields (Task 1) are read in Tasks 4/6/7. `save_passage_page`/`find_passage_pages` signatures (Task 1) are called identically in Tasks 3/5/7. `JournalBand::Passage { div1, div2, start, end }` (Task 3) is constructed in Task 5 and matched in Tasks 3/4/5/7. `populate_verse_buffer` (Task 2) is consumed in Task 4. The shared "open passage ask" / "create reader gloss for passage context" helpers (Tasks 5/6) are factored once and reused — flagged in both tasks.

**Risk note:** Task 2 (shared-render extraction) is the structural keystone and is a pure refactor guarded by the gloss tests; it lands before any journal consumer. Task 4's call-out to mirror the `gloss-stage` italic priority fix in the journal overlay's `apply_font` pre-empts re-introducing the just-fixed italic bug. Task 3's `JournalBand` losing `Copy` (now holds `String`) is the one wide mechanical change — the build-driven step catches every site.
