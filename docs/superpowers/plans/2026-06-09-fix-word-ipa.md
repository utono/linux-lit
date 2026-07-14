# Fix one word's OP-IPA in a gloss verse (`i` key) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A gloss-overlay `i` key that corrects a single word's Original-Pronunciation `/IPA/` in the cursor's source verse — accepting either a typed `/IPA/` (literal) or a plain-English hint (LLM) — then rewrites the stored gloss in place, deletes that source block's stale synthesized audio, and re-synthesizes + plays so the user hears the fix.

**Architecture:** A pure splice helper rewrites `word /IPA/` pairs within the cursor block's raw text and substitutes it back into `gloss_text`; `update_gloss` persists it (keeping the gloss id so the audio cache key survives); a new per-block `delete_gloss_audio_block` clears just that block's MP3s (all voices) + their files; the in-memory gloss list entry is patched and `play_source_tts_pausing_mpv` re-synthesizes + plays. The `i` key opens the existing stacked input card via a new `GlossPromptMode::FixIpa`; `submit_gloss_prompt` routes to the fix handler. Literal-vs-hint is detected by whether the input's tail contains an IPA span.

**Tech Stack:** Rust + GTK4 (gtk4 crate) + rusqlite + ElevenLabs/MPV. Binary-only crate: `cargo build`; tests `cargo test --bins -- --test-threads=1` (rare parallel flake → keep `--test-threads=1`). DO NOT run the GUI (`cargo run`) — the user runs it; audio/visual behavior is a user check.

**Spec:** `docs/superpowers/specs/2026-06-09-fix-word-ipa-design.md`.

**Load-bearing facts (verified at branch HEAD):**
- `GlossBlock { kind: BlockKind, index: i32, text: String (RAW, with /IPA/), display: String }` (`src/ui/gloss_overlay.rs:1553`); `pub fn gloss_blocks(gloss: &str) -> Vec<GlossBlock>` (`:1571`) segments `gloss_text` into Source/Explication blocks. The cursor's source block: `gloss_blocks(&gloss.gloss_text).iter().find(|b| b.kind==Source && b.index==idx)`.
- `SavedGloss { gloss_id: i64, gloss_text: String, gloss_type: String, … }` (`src/db/queries.rs:1114`); `gloss_list[gloss_index]` is a `SavedGloss`.
- `update_gloss(conn, gloss_id: i64, gloss_text: &str) -> Result<(),_>` (`src/db/queries.rs:35`) = `UPDATE glosses SET gloss_text=?1, timestamp=CURRENT_TIMESTAMP WHERE id=?2` — the in-place rewrite (keeps id). Do NOT use `save_gloss` (it INSERTs a new row).
- `delete_gloss_audio(conn, gloss_id)` (`:841`) deletes ALL of a gloss's audio — too broad; this plan adds a per-block variant.
- `gloss_audio` columns: `gloss_id, kind, paragraph_index, audio_path, voice_id, model_id`; UNIQUE `(gloss_id, kind, paragraph_index, voice_id)`.
- `GlossPromptMode { Add, Edit }` (`src/app.rs:79`); field `gloss_prompt_mode` (`app.rs:1631`). `show_prompt_dialog(state, mode)` (`gloss.rs:313`) sets the mode and calls `gloss_overlay.open_ask_card_with(title, hint)`. `submit_gloss_prompt` (`gloss.rs:1167`) reads `take_ask_text()` + `gloss_prompt_mode` and `match`es on the mode.
- `source_block_index(state) -> Option<i32>` (`gloss.rs`, added for r/R) — Source-block gate + "Source verse only" toast. `play_source_tts_pausing_mpv(state, index)` (`gloss.rs`) — pause MPV + `play_block_tts`. `show_tts_toast(state, msg)` (`gloss.rs:942`). `call_claude_with_prompt(system, user, model)` (`src/gloss.rs`) — the LLM call (async, via tokio_handle). `gloss_overlay.show_loading()` — loading affordance used by edit_gloss.
- `play_block_tts` reads `s.gloss_list[s.gloss_index].gloss_text` then `gloss_blocks(...)` → block `.text` → `ipa_for_tts` → synth. So the in-memory `gloss_list[idx].gloss_text` MUST be patched to the corrected text before the play call, or it re-synthesizes the OLD IPA.
- IPA-span detection heuristic (shared by `strip_ipa`/`ipa_for_tts`): a `/…/` whose inner is non-empty and has ≥1 non-ASCII-letter char. Currently inline/private; Task 1 extracts a reusable `contains_ipa_span`.
- `handle_gloss_key` plain-key match (`src/input/keymap.rs:752`); `i` is FREE in the gloss overlay.
- Per CLAUDE.md a new keybind also needs the Ctrl+/ overlay (`src/ui/keybinds_overlay.rs`) updated via the `update-cairo-keybinds-overlay` skill (Task 6). keymap.json is NOT involved (gloss-overlay internal keys are hardcoded in `handle_gloss_key`).

---

## Task 1: Pure splice + detection helpers (`gloss_overlay.rs`)

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (add `contains_ipa_span`, `replace_word_ipa`; tests)

These are the unit-testable core. Put them near `strip_ipa`/`ipa_for_tts`.

- [ ] **Step 1: Write failing tests.** Add to the `#[cfg(test)]` module that holds the `ipa_for_tts` tests:

```rust
    #[test]
    fn contains_ipa_span_detects_real_ipa() {
        assert!(contains_ipa_span("/ˈdeɪli/"));
        assert!(contains_ipa_span("daily /ˈdeɪli/"));
        assert!(!contains_ipa_span("hard a"));          // plain hint
        assert!(!contains_ipa_span("and/or"));          // literal slash, ascii-only
        assert!(!contains_ipa_span("/word/"));          // ascii-only inner, not IPA
        assert!(!contains_ipa_span(""));
    }

    #[test]
    fn replace_word_ipa_swaps_the_words_ipa() {
        // The screenshot case: daily ɛː -> eɪ.
        assert_eq!(
            replace_word_ipa("In daily /ˈdɛːli/ thanks, that gave /gɛːv/ us", "daily", "/ˈdeɪli/"),
            Some("In daily /ˈdeɪli/ thanks, that gave /gɛːv/ us".to_string())
        );
    }

    #[test]
    fn replace_word_ipa_all_occurrences() {
        assert_eq!(
            replace_word_ipa("good /gʊd/ and more good /gʊd/", "good", "/guːd/"),
            Some("good /guːd/ and more good /guːd/".to_string())
        );
    }

    #[test]
    fn replace_word_ipa_is_whole_word() {
        // 'day' must not match inside 'daily'.
        assert_eq!(replace_word_ipa("daily /ˈdɛːli/ here", "day", "/deɪ/"), None);
    }

    #[test]
    fn replace_word_ipa_word_without_following_ipa_is_none() {
        // 'thanks' has no /IPA/ after it -> nothing to replace.
        assert_eq!(replace_word_ipa("In daily /ˈdɛːli/ thanks", "thanks", "/θaŋks/"), None);
    }

    #[test]
    fn replace_word_ipa_case_insensitive_word_match() {
        assert_eq!(
            replace_word_ipa("Daily /ˈdɛːli/ thanks", "daily", "/ˈdeɪli/"),
            Some("Daily /ˈdeɪli/ thanks".to_string())
        );
    }
```

- [ ] **Step 2: Run, verify FAIL.** `cargo test --bins contains_ipa_span replace_word_ipa -- --test-threads=1` → FAIL (functions missing). Run filters separately if the multi-filter errors.

- [ ] **Step 3: Implement.** Add to `src/ui/gloss_overlay.rs`:

```rust
/// True if `s` contains an inline IPA span — a `/…/` whose inner is non-empty
/// and has at least one non-ASCII-letter char (length/stress marks, schwa, etc.).
/// Same heuristic `strip_ipa`/`ipa_for_tts` use to tell `/tɛːk/` from `and/or`.
/// Used to decide whether a fix-IPA input is a literal `/IPA/` or a plain hint.
pub(crate) fn contains_ipa_span(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' {
            if let Some(rel) = chars[i + 1..].iter().position(|&c| c == '/') {
                let close = i + 1 + rel;
                let inner = &chars[i + 1..close];
                if !inner.is_empty() && inner.iter().any(|&c| !c.is_ascii_alphabetic()) {
                    return true;
                }
                i = close + 1;
                continue;
            }
        }
        i += 1;
    }
    false
}

/// Replace the `/IPA/` that immediately follows each whole-word, case-insensitive
/// occurrence of `word` in `text` with `new_ipa` (which includes its slashes,
/// e.g. `"/ˈdeɪli/"`). Returns the rewritten text, or `None` if no
/// `word /IPA/` pair was found (nothing changed). A match requires the word as a
/// whole token (not a substring) directly followed (after one space) by an IPA
/// span. Used by the gloss-overlay `i` (fix-IPA) flow on a source block's text.
pub(crate) fn replace_word_ipa(text: &str, word: &str, new_ipa: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let wlc: Vec<char> = word.to_lowercase().chars().collect();
    if wlc.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut replaced = false;
    while i < chars.len() {
        // Try to match `word` as a whole token starting at i.
        let at_word_boundary = i == 0 || !chars[i - 1].is_alphanumeric();
        let word_matches = at_word_boundary
            && i + wlc.len() <= chars.len()
            && chars[i..i + wlc.len()]
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .eq(wlc.iter().copied())
            // whole word: next char (if any) is a non-alphanumeric boundary
            && chars
                .get(i + wlc.len())
                .map_or(true, |c| !c.is_alphanumeric());
        if word_matches {
            // After the word, allow exactly the spaces up to an IPA span.
            let mut j = i + wlc.len();
            let mut k = j;
            while k < chars.len() && chars[k] == ' ' {
                k += 1;
            }
            if k < chars.len() && chars[k] == '/' {
                if let Some(rel) = chars[k + 1..].iter().position(|&c| c == '/') {
                    let close = k + 1 + rel;
                    let inner = &chars[k + 1..close];
                    let is_ipa =
                        !inner.is_empty() && inner.iter().any(|&c| !c.is_ascii_alphabetic());
                    if is_ipa {
                        // Emit the word + the original spacing, then the new IPA.
                        out.extend(&chars[i..k]); // word + spaces verbatim
                        out.push_str(new_ipa);
                        i = close + 1;
                        replaced = true;
                        let _ = j; // (j retained for clarity; not used past here)
                        continue;
                    }
                }
            }
            let _ = j;
        }
        out.push(chars[i]);
        i += 1;
    }
    if replaced {
        Some(out)
    } else {
        None
    }
}
```

(Implementer: simplify the `j`/`k` bookkeeping if you can keep behavior identical — the intent is "word, then run-of-spaces, then an IPA span → replace just the span". Verify against the tests; do not change the test expectations.)

- [ ] **Step 4: Run, verify PASS.** `cargo test --bins contains_ipa_span replace_word_ipa -- --test-threads=1` → all PASS.

- [ ] **Step 5: Build + full tests.** `cargo build && cargo test --bins -- --test-threads=1` → clean; all pass. (A `dead_code` warning on the two new `pub(crate)` fns is EXPECTED until later tasks call them.)

- [ ] **Step 6: Commit.**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): replace_word_ipa + contains_ipa_span helpers (fix-IPA core)"
```

---

## Task 2: `delete_gloss_audio_block` (per-block audio delete) (`queries.rs`)

**Files:**
- Modify: `src/db/queries.rs` (add `delete_gloss_audio_block` returning the deleted rows' paths; tests)

- [ ] **Step 1: Write failing test.** Add to the queries `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn delete_gloss_audio_block_scopes_to_one_block() {
        let conn = Connection::open_in_memory().unwrap();
        // FK parent + table (mirror existing gloss_audio tests' seeding).
        conn.execute_batch(
            "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
             INSERT INTO glosses (id) VALUES (7);",
        ).unwrap();
        ensure_gloss_audio_table(&conn).unwrap();
        let ins = |kind: &str, idx: i64, voice: &str, path: &str| {
            conn.execute(
                "INSERT INTO gloss_audio (gloss_id, kind, paragraph_index, audio_path, voice_id, model_id)
                 VALUES (7, ?1, ?2, ?3, ?4, 'm')",
                rusqlite::params![kind, idx, path, voice],
            ).unwrap();
        };
        ins("source", 0, "vA", "/a0.mp3");
        ins("source", 0, "vB", "/a0b.mp3"); // same block, second voice
        ins("source", 1, "vA", "/a1.mp3");  // different block — must survive
        // delete source block 0 (both voices), get their paths back
        let paths = delete_gloss_audio_block(&conn, 7, "source", 0).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"/a0.mp3".to_string()));
        assert!(paths.contains(&"/a0b.mp3".to_string()));
        // block 1 row survives
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM gloss_audio WHERE gloss_id=7", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 1);
    }
```

(Confirm the FK-parent + `ensure_gloss_audio_table` seeding matches the EXISTING gloss_audio tests in this module — read one and mirror it; adjust if the table is created differently.)

- [ ] **Step 2: Run, verify FAIL.** `cargo test --bins delete_gloss_audio_block_scopes -- --test-threads=1` → FAIL (function missing).

- [ ] **Step 3: Implement.** Add near `delete_gloss_audio` (`src/db/queries.rs:841`):

```rust
/// Delete the cached audio rows for ONE block of a gloss (all voices) and return
/// their `audio_path`s so the caller can remove the files. Scoped, unlike
/// `delete_gloss_audio` which clears a whole gloss. Used by the fix-IPA flow to
/// invalidate just the corrected source block before re-synthesis.
pub fn delete_gloss_audio_block(
    conn: &Connection,
    gloss_id: i64,
    kind: &str,
    index: i64,
) -> Result<Vec<String>, rusqlite::Error> {
    let paths: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT audio_path FROM gloss_audio
             WHERE gloss_id = ?1 AND kind = ?2 AND paragraph_index = ?3",
        )?;
        let rows = stmt.query_map(rusqlite::params![gloss_id, kind, index], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    conn.execute(
        "DELETE FROM gloss_audio WHERE gloss_id = ?1 AND kind = ?2 AND paragraph_index = ?3",
        rusqlite::params![gloss_id, kind, index],
    )?;
    Ok(paths)
}
```

- [ ] **Step 4: Run, verify PASS.** `cargo test --bins delete_gloss_audio_block_scopes -- --test-threads=1` → PASS.

- [ ] **Step 5: Build + full tests.** `cargo build && cargo test --bins -- --test-threads=1` → clean; all pass.

- [ ] **Step 6: Commit.**

```bash
git add src/db/queries.rs
git commit -m "feat(db): delete_gloss_audio_block — per-block audio delete returning file paths"
```

---

## Task 3: `GlossPromptMode::FixIpa` + submit routing (`app.rs`, `gloss.rs`)

**Files:**
- Modify: `src/app.rs` (enum variant)
- Modify: `src/input/actions/gloss.rs` (`show_prompt_dialog` title/hint; `submit_gloss_prompt` arm; stub `fix_word_ipa`)

- [ ] **Step 1: Add the enum variant.** In `src/app.rs`, `GlossPromptMode` (line 79):

```rust
pub enum GlossPromptMode {
    Add,
    Edit,
    /// Gloss-overlay `i`: correct one word's /IPA/ in the cursor's source verse.
    FixIpa,
}
```

- [ ] **Step 2: Build to find the non-exhaustive match.** `cargo build 2>&1 | rg "non-exhaustive|FixIpa|^error"` — expect an error in `submit_gloss_prompt`'s `match mode` (and possibly `show_prompt_dialog`'s `== Edit` check, which is a comparison not a match, so it compiles — but its title/hint should handle FixIpa). List the sites.

- [ ] **Step 3: Title/hint for FixIpa in `show_prompt_dialog`.** In `src/input/actions/gloss.rs:313`, the `is_edit` flag is `mode == Edit`. Add FixIpa handling so the card shows an appropriate title/hint. Minimal change: compute `is_fix_ipa = mode == GlossPromptMode::FixIpa` and branch the `title_text`/`hint_text`:

```rust
    let title_text = if is_fix_ipa {
        "FIX IPA — word /IPA/  OR  word <hint>"
    } else if is_edit {
        "EDIT GLOSS — PASTE SUBTEXT LINES"
    } else if is_inner_monologue {
        "INNER MONOLOGUE PASSAGE"
    } else {
        "GLOSS PROMPT"
    };
    let hint_text = if is_fix_ipa {
        "e.g. `daily /ˈdeɪli/` or `daily hard a`  ·  Ctrl+Enter submit  ·  Esc cancel"
    } else if is_edit {
        /* … existing … */
    } else if is_inner_monologue {
        /* … existing … */
    } else {
        /* … existing … */
    };
```

(Keep the existing edit/inner/default arms unchanged.)

- [ ] **Step 4: Route the submit.** In `submit_gloss_prompt` (`gloss.rs:1167`) add the arm:

```rust
    match mode {
        crate::app::GlossPromptMode::Add => add_gloss(state, &prompt),
        crate::app::GlossPromptMode::Edit => edit_gloss(state, &prompt),
        crate::app::GlossPromptMode::FixIpa => fix_word_ipa(state, &prompt),
    }
```

- [ ] **Step 5: Stub `fix_word_ipa` (compiles, no-op-ish) so the build is green this task.** Add a stub now; Task 4 fills it:

```rust
/// Gloss-overlay `i` submit: parse `word [/IPA/ | hint]` and fix the word's OP
/// IPA in the cursor's source verse. (Implemented in Task 4.)
pub(crate) fn fix_word_ipa(state_rc: &Rc<RefCell<AppState>>, input: &str) {
    let _ = (state_rc, input);
    show_tts_toast(state_rc, "Fix IPA: not yet wired");
}
```

- [ ] **Step 6: Build + tests.** `cargo build 2>&1 | rg "^error" || echo OK` (expect OK), `cargo test --bins -- --test-threads=1 2>&1 | rg "test result"` (all pass).

- [ ] **Step 7: Commit.**

```bash
git add src/app.rs src/input/actions/gloss.rs
git commit -m "feat(gloss): GlossPromptMode::FixIpa — card title/hint + submit routing (stub handler)"
```

---

## Task 4: `fix_word_ipa` — parse, splice, persist, delete audio, re-synth+play (`gloss.rs`)

**Files:**
- Modify: `src/input/actions/gloss.rs` (replace the Task-3 stub with the real handler)

This is the behavioral core. Build-verified; runtime is a user check.

- [ ] **Step 1: Replace the stub with the full handler.** The flow:

```rust
pub(crate) fn fix_word_ipa(state_rc: &Rc<RefCell<AppState>>, input: &str) {
    // 1. Parse `word <rest>`.
    let trimmed = input.trim();
    let (word, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((w, r)) => (w.trim(), r.trim()),
        None => {
            show_tts_toast(state_rc, "Usage: word /IPA/  or  word <hint>");
            return;
        }
    };
    if word.is_empty() || rest.is_empty() {
        show_tts_toast(state_rc, "Usage: word /IPA/  or  word <hint>");
        return;
    }

    // 2. Resolve the cursor's source block (index + its raw text + gloss id +
    //    the gloss_text). Toast + return off a source block or with no gloss.
    let (gloss_index_pos, gloss_id, block_index, gloss_text, block_text) = {
        let s = state_rc.borrow();
        let idx = match s.gloss_overlay.current_block() {
            Some((crate::ui::gloss_overlay::BlockKind::Source, i)) => i,
            _ => { drop(s); show_tts_toast(state_rc, "Source verse only"); return; }
        };
        let gpos = s.gloss_index;
        let gloss = match s.gloss_list.get(gpos) {
            Some(g) => g,
            None => { drop(s); show_tts_toast(state_rc, "No gloss"); return; }
        };
        let gtext = gloss.gloss_text.clone();
        let blocks = crate::ui::gloss_overlay::gloss_blocks(&gtext);
        let btext = match blocks.iter()
            .find(|b| b.kind == crate::ui::gloss_overlay::BlockKind::Source && b.index == idx)
        {
            Some(b) => b.text.clone(),
            None => { drop(s); show_tts_toast(state_rc, "Source verse only"); return; }
        };
        (gpos, gloss.gloss_id, idx, gtext, btext)
    };

    // 3. Literal IPA vs hint.
    if crate::ui::gloss_overlay::contains_ipa_span(rest) {
        // Literal: take the first /…/ span of `rest` as the new IPA.
        let new_ipa = first_ipa_span(rest); // Step 2 helper; rest IS the span or contains it
        apply_ipa_fix(state_rc, gloss_index_pos, gloss_id, block_index,
                      &gloss_text, &block_text, word, &new_ipa);
    } else {
        // Hint: ask Claude for the word's OP IPA, then apply (async).
        request_ipa_then_apply(state_rc, gloss_index_pos, gloss_id, block_index,
                               gloss_text, block_text, word.to_string(), rest.to_string());
    }
}
```

- [ ] **Step 2: Add `first_ipa_span`.** Returns the first `/…/` IPA span (with slashes) in a string, else the whole trimmed string if it already looks like a bare span:

```rust
/// The first inline IPA span (including its slashes) in `s`, e.g.
/// `"daily /ˈdeɪli/"` -> `"/ˈdeɪli/"`. Falls back to the trimmed input if it has
/// no detectable span (caller only calls this when `contains_ipa_span` is true,
/// so a span exists). 
fn first_ipa_span(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' {
            if let Some(rel) = chars[i + 1..].iter().position(|&c| c == '/') {
                let close = i + 1 + rel;
                let inner = &chars[i + 1..close];
                if !inner.is_empty() && inner.iter().any(|&c| !c.is_ascii_alphabetic()) {
                    return chars[i..=close].iter().collect();
                }
                i = close + 1;
                continue;
            }
        }
        i += 1;
    }
    s.trim().to_string()
}
```

- [ ] **Step 3: Add `apply_ipa_fix`** (the shared back half: splice → update_gloss → patch in-memory → delete block audio + files → play). It does all DB/state work, drops borrows before the play call:

```rust
/// Splice `new_ipa` into the block's text, persist, invalidate that block's
/// cached audio, patch the in-memory gloss, and re-synthesize + play.
fn apply_ipa_fix(
    state_rc: &Rc<RefCell<AppState>>,
    gloss_index_pos: usize,
    gloss_id: i64,
    block_index: i32,
    gloss_text: &str,
    block_text: &str,
    word: &str,
    new_ipa: &str,
) {
    // Rewrite the word's IPA within the block's text only.
    let new_block_text =
        match crate::ui::gloss_overlay::replace_word_ipa(block_text, word, new_ipa) {
            Some(t) => t,
            None => {
                show_tts_toast(state_rc, &format!("No IPA for {}", word));
                return;
            }
        };
    // Substitute the rewritten block text back into the full gloss_text. The
    // block text is a verbatim contiguous substring of gloss_text (gloss_blocks
    // copies it), so a single replace of the first occurrence is exact.
    let new_gloss_text = gloss_text.replacen(block_text, &new_block_text, 1);
    if new_gloss_text == gloss_text {
        // Defensive: block text not found verbatim (shouldn't happen).
        show_tts_toast(state_rc, "Could not apply IPA fix");
        return;
    }

    // Persist + invalidate that block's audio (collect file paths to remove).
    let mut removed_paths: Vec<String> = Vec::new();
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::queries::update_gloss(&conn, gloss_id, &new_gloss_text);
        removed_paths = crate::db::queries::delete_gloss_audio_block(
            &conn, gloss_id, "source", block_index as i64,
        ).unwrap_or_default();
    } else {
        show_tts_toast(state_rc, "Could not save IPA fix");
        return;
    }
    for p in &removed_paths {
        let _ = std::fs::remove_file(p);
    }

    // Patch the in-memory gloss so play_block_tts reads the corrected verse.
    {
        let mut s = state_rc.borrow_mut();
        if let Some(g) = s.gloss_list.get_mut(gloss_index_pos) {
            g.gloss_text = new_gloss_text;
        }
        // Re-render the open overlay so the (display) text reflects the gloss if
        // needed — display strips IPA, so the visible verse is unchanged, but
        // refresh keeps the block/cursor mapping consistent. (Optional: only if
        // the overlay caches parsed blocks; otherwise skip.)
    }

    crate::log_fmt!("GLOSS: fixed IPA for '{}' in gloss {} source block {} -> {}",
        word, gloss_id, block_index, new_ipa);

    // Re-synthesize + play the corrected line (pauses MPV first).
    play_source_tts_pausing_mpv(state_rc, block_index);
}
```

IMPORTANT borrow discipline: the `borrow_mut` patching `gloss_list` must be dropped before `play_source_tts_pausing_mpv(state_rc, …)` (it borrows state). The code above scopes it. Verify no borrow is held across the play call.

- [ ] **Step 4: Add `request_ipa_then_apply`** (the hint/LLM path). Mirror `edit_gloss`'s async structure: show loading, spawn the Claude call on `tokio_handle`, parse the first IPA span from the reply, then call `apply_ipa_fix` with the result. Read `edit_gloss` (`gloss.rs:446`) for the exact `glib::spawn_future_local` + `tokio_handle.spawn(call_claude_with_prompt(...))` shape and replicate it. The prompt: a focused system instruction "Return ONLY the Original-Pronunciation IPA (in forward slashes) for the given English word, honoring the hint; no prose." User message: the `word` and the `hint`. On a reply with no parseable IPA span (`first_ipa_span` returns something without a `/…/`), toast "Could not get IPA" and return. NOTE: `apply_ipa_fix` borrows `state_rc` — call it from the async block AFTER the await completes, with owned `gloss_text`/`block_text` captured into the closure.

Add a tiny prompt const in `src/gloss.rs` (e.g. `FIX_IPA_PROMPT`) for the system instruction, alongside the other prompt consts; keep it ~3 lines.

- [ ] **Step 5: Build.** `cargo build 2>&1 | rg "^error" || echo OK` → OK. The Task-1 `replace_word_ipa`/`contains_ipa_span` and Task-2 `delete_gloss_audio_block` dead_code warnings should now CLEAR (reached from here).

- [ ] **Step 6: Full tests.** `cargo test --bins -- --test-threads=1 2>&1 | rg "test result"` → all pass (no new tests here; the pure logic is tested in Task 1/2).

- [ ] **Step 7: Commit.**

```bash
git add src/input/actions/gloss.rs src/gloss.rs
git commit -m "feat(gloss): fix_word_ipa — splice IPA, update_gloss, drop block audio, re-synth+play"
```

---

## Task 5: Bind `i` in `handle_gloss_key` (`keymap.rs`)

**Files:**
- Modify: `src/input/keymap.rs` (`open_fix_ipa_prompt` opener call in the gloss plain-key match)
- Modify: `src/input/actions/gloss.rs` (`open_fix_ipa_prompt` — Source-gate + open card)

- [ ] **Step 1: Add `open_fix_ipa_prompt` to gloss.rs.** Source-gated open of the FixIpa card:

```rust
/// Gloss-overlay `i`: open the fix-IPA input card for the cursor's source verse.
/// No-op (toast) off a source block.
pub(crate) fn open_fix_ipa_prompt(state_rc: &Rc<RefCell<AppState>>) {
    if source_block_index(state_rc).is_none() {
        return; // not a source block — toast already shown
    }
    show_prompt_dialog(state_rc, crate::app::GlossPromptMode::FixIpa);
}
```

- [ ] **Step 2: Bind `i` in `handle_gloss_key`.** In `src/input/keymap.rs`, plain-key match (~line 752, beside `r`/`R`/`e`):

```rust
        "i" => {
            crate::input::actions::gloss::open_fix_ipa_prompt(state);
            true
        }
```

- [ ] **Step 3: Build + tests.** `cargo build 2>&1 | rg "^error" || echo OK` (OK); `cargo test --bins -- --test-threads=1 2>&1 | rg "test result"` (all pass).

- [ ] **Step 4: Commit.**

```bash
git add src/input/keymap.rs src/input/actions/gloss.rs
git commit -m "feat(gloss): bind i in the gloss overlay to open the fix-IPA card"
```

---

## Task 6: Ctrl+/ keybinds overlay (`keybinds_overlay.rs`)

**Files:**
- Modify: `src/ui/keybinds_overlay.rs`

REQUIRED SUB-SKILL: Use the `update-cairo-keybinds-overlay` skill (the mandatory three-pass cross-reference).

- [ ] **Step 1: Add the `i` keycap + `describe()` arm.** Find `i`'s row in the keycap tables; add a gloss-overlay detail combo `("i", "fix IPA")` (mirroring how `v`/`r` carry gloss-overlay combos on a reader-mode cap), and a `describe()` arm:

```rust
        "fix IPA" => "Gloss overlay, on a source verse: open a card to correct one \
word's Original-Pronunciation IPA — type `word /IPA/` (used literally) or `word \
<hint>` (the LLM regenerates it). Rewrites the stored verse, drops that line's \
synthesized audio, and re-synthesizes + plays. \
-> open_fix_ipa_prompt — src/input/actions/gloss.rs",
```

(Match the exact describe() string shape of the existing gloss arms — read one first. If `i` has no reader-mode binding, add a bare cap as the other no-reader-bind keys do.)

- [ ] **Step 2: Three-pass cross-reference** (no blank slot, no wrong label, every label described). Confirm `i` renders a cap + non-blank detail.

- [ ] **Step 3: Build.** `cargo build 2>&1 | rg "^error" || echo OK` → OK.

- [ ] **Step 4: Commit.**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(keybinds): document gloss-overlay i (fix IPA) in the Ctrl+/ overlay"
```

---

## Task 7: OP-IPA cheat-sheet (`docs/guides/op-ipa-cheatsheet.md`)

**Files:**
- Create: `docs/guides/op-ipa-cheatsheet.md`
- Modify: `docs/guides/elevenlabs-v3-custom-voices.md` (one-line cross-reference)

- [ ] **Step 1: Write the cheat-sheet.** A concise reference of the ~25 OP symbols the user will actually type, each with: the symbol, a lexical-set/feature name, a Shakespeare example word, and the modern↔OP contrast. Cover (per the spec §6): vowels/diphthongs `eɪ`/`eː` FACE (daily, gave), `əɪ` PRICE (wise, I), `ʊ` STRUT-class (love, blood), `ɛ`/`ɛː` DRESS+length, `ɔ`/`ɔː` THOUGHT, `ɑ`/`ɑː` PALM, `ə` schwa, `ɪ` KIT, `aʊ` MOUTH, `ɔɪ` CHOICE; consonants `ʃ ʒ tʃ dʒ θ ð ŋ` + rhotic `r` (OP is rhotic); marks `ˈ`/`ˌ`/`ː`. Lead with the exact `daily` fix (`/ˈdɛːli/` → `/ˈdeɪli/`) as the worked example. Follow the project's markdown rules (no box-drawing; prefer lists; tables <80 cols if any). Mention the `show-gloss-ipa-tts` skill for reading existing IPA and the `i` key for fixing it.

- [ ] **Step 2: Cross-reference.** Add one line to `docs/guides/elevenlabs-v3-custom-voices.md` (near its IPA section) pointing to the cheat-sheet.

- [ ] **Step 3: Commit.**

```bash
git add docs/guides/op-ipa-cheatsheet.md docs/guides/elevenlabs-v3-custom-voices.md
git commit -m "docs(guide): OP-IPA cheat-sheet for the fix-IPA typing path"
```

---

## Self-review notes

- **Spec coverage:** dual input (literal/hint) → Task 4 Step 1 (`contains_ipa_span` branch) + Task 1. Splice all occurrences, whole-word, in-place → Task 1 `replace_word_ipa` + Task 4 `replacen` back into gloss_text. `update_gloss` not `save_gloss` → Task 4 Step 3. Per-block audio delete + file removal → Task 2 + Task 4 Step 3. Patch in-memory gloss before play → Task 4 Step 3 (the load-bearing "play_block_tts reads gloss_list[idx].gloss_text" fact). Auto-play via `play_source_tts_pausing_mpv` → Task 4. `i` key + Source gate → Task 5. Card mode → Task 3. Cheat-sheet → Task 7. Ctrl+/ overlay → Task 6.
- **The block-substring substitution** (`gloss_text.replacen(block_text, new_block_text, 1)`) relies on `block.text` being a verbatim contiguous substring of `gloss_text` — TRUE because `gloss_blocks` copies the raw run. The defensive `== gloss_text` check (no change) toasts rather than silently corrupting. If a future gloss had two identical source blocks, `replacen(.., 1)` would patch the first — acceptable (rare; and the cursor block index already disambiguates which block's text we rewrote, we just substitute its first verbatim occurrence). Flagged as a known minor edge.
- **Borrow discipline:** every `state_rc.borrow()/borrow_mut()` is scoped and dropped before `play_source_tts_pausing_mpv` / `show_tts_toast` (which re-borrow). The async hint path calls `apply_ipa_fix` after the await with no held borrow. Called out in Task 4.
- **Cache key unaffected by text:** the audio cache keys on `(gloss_id, kind, index, voice_id)` — rewriting `gloss_text` doesn't change keys, which is exactly why we must explicitly `delete_gloss_audio_block` (else the stale MP3 replays). Captured in the spec + Task 2.
- **Type consistency:** `replace_word_ipa(&str,&str,&str)->Option<String>`, `contains_ipa_span(&str)->bool`, `first_ipa_span(&str)->String`, `delete_gloss_audio_block(&Connection,i64,&str,i64)->Result<Vec<String>,_>`, `fix_word_ipa(&Rc<RefCell<AppState>>,&str)`, `open_fix_ipa_prompt(&Rc<RefCell<AppState>>)`, `apply_ipa_fix(...)`. `GlossPromptMode::FixIpa`. All consistent across tasks.
- **Deferred (per spec):** per-occurrence disambiguation, explication IPA, visual IPA picker — out of scope.
