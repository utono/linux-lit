# Concordance Picker Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace spawn-new-instance concordance navigation with in-place cross-work navigation via Ctrl+n/Ctrl+p, using a stopword-filtered author-scoped word list.

**Architecture:** Modify the existing concordance system (ConcordanceState, ConcordancePicker, find_word_occurrences) in-place. Add a stopwords module, change the navigation functions from work-filtered to full-list traversal, and replace the spawn logic with `display_work_at` calls. New actions `ConcordanceNext`/`ConcordancePrev` bound to Ctrl+n/Ctrl+p.

**Tech Stack:** Rust, GTK4, rusqlite, tokio

---

## File Map

| File | Change | Responsibility |
|------|--------|----------------|
| `src/db/stopwords.rs` | Create | ~150-word English stopword const array |
| `src/db/mod.rs` | Modify | Add `pub mod stopwords;` |
| `src/db/concordance.rs` | Modify | Add author param to `find_word_occurrences`, add `load_concordance_words` |
| `src/concordance.rs` | Modify | Replace `advance_within_work`/`retreat_within_work` with `advance`/`retreat` |
| `src/input/actions/mod.rs` | Modify | Add `ConcordanceNext`, `ConcordancePrev` variants |
| `src/input/keymap_config.rs` | Modify | Add Ctrl+n/Ctrl+p bindings in `vocab_bindings()` |
| `src/input/keymap.rs` | Modify | Wire `ConcordanceNext`/`ConcordancePrev` in `dispatch_action` |
| `src/input/actions/concordance.rs` | Modify | Remove spawn logic, add `concordance_next`/`concordance_prev`, change `open_picker` to use author word list |
| `src/app.rs` | Modify | Add `concordance_word_cache` field to `AppState` |

---

### Task 1: Create stopwords module

**Files:**
- Create: `src/db/stopwords.rs`
- Modify: `src/db/mod.rs:1-5`

- [ ] **Step 1: Create `src/db/stopwords.rs`**

```rust
pub const STOPWORDS: &[&str] = &[
    "a", "about", "above", "after", "again", "against", "all", "am", "an",
    "and", "any", "are", "as", "at", "be", "because", "been", "before",
    "being", "below", "between", "both", "but", "by", "can", "could", "did",
    "do", "does", "doing", "down", "during", "each", "few", "for", "from",
    "further", "get", "got", "had", "has", "have", "having", "he", "her",
    "here", "hers", "herself", "him", "himself", "his", "how", "i", "if",
    "in", "into", "is", "it", "its", "itself", "just", "ll", "me", "might",
    "more", "most", "must", "my", "myself", "no", "nor", "not", "now", "of",
    "off", "on", "once", "only", "or", "other", "our", "ours", "ourselves",
    "out", "over", "own", "re", "s", "same", "shall", "she", "should",
    "so", "some", "such", "t", "than", "that", "the", "their", "theirs",
    "them", "themselves", "then", "there", "these", "they", "this", "those",
    "through", "to", "too", "under", "until", "up", "upon", "us", "ve",
    "very", "was", "we", "were", "what", "when", "where", "which", "while",
    "who", "whom", "why", "will", "with", "would", "you", "your", "yours",
    "yourself", "yourselves",
];
```

- [ ] **Step 2: Add module to `src/db/mod.rs`**

Add `pub mod stopwords;` after the existing modules. The file currently contains:

```rust
pub mod chunks;
pub mod concordance;
pub mod line_types;
pub mod models;
pub mod queries;
```

Add:

```rust
pub mod stopwords;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles successfully (unused warning is fine at this stage)

- [ ] **Step 4: Commit**

```bash
git add src/db/stopwords.rs src/db/mod.rs
git commit -m "feat(concordance): add English stopwords module"
```

---

### Task 2: Add `load_concordance_words` query

**Files:**
- Modify: `src/db/concordance.rs:1-49`

- [ ] **Step 1: Write test for `load_concordance_words`**

Add at the bottom of `src/db/concordance.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let home = std::env::var("HOME").unwrap_or_default();
        let db_path = format!("{}/utono/litdb/data/lit.db", home);
        Connection::open(&db_path).expect("Failed to open lit.db for tests")
    }

    #[test]
    fn concordance_words_excludes_stopwords() {
        let conn = test_conn();
        let words = load_concordance_words(&conn, "Shakespeare, William").unwrap();
        // Stopwords should not appear
        assert!(!words.contains(&"the".to_string()));
        assert!(!words.contains(&"and".to_string()));
        assert!(!words.contains(&"is".to_string()));
        // Content words should appear
        assert!(words.contains(&"time".to_string()));
        assert!(words.contains(&"love".to_string()));
    }

    #[test]
    fn concordance_words_sorted_alphabetically() {
        let conn = test_conn();
        let words = load_concordance_words(&conn, "Shakespeare, William").unwrap();
        let mut sorted = words.clone();
        sorted.sort();
        assert_eq!(words, sorted);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib db::concordance::tests -- 2>&1 | tail -10`
Expected: FAIL with "cannot find function `load_concordance_words`"

- [ ] **Step 3: Implement `load_concordance_words`**

Add this function to `src/db/concordance.rs` after the existing `find_word_occurrences` function (after line 49):

```rust
/// Load all content words (minus stopwords) from the author's works.
/// Returns a deduplicated, alphabetically sorted list.
pub fn load_concordance_words(
    conn: &Connection,
    author: &str,
) -> Result<Vec<String>, rusqlite::Error> {
    use std::collections::HashSet;
    use crate::db::stopwords::STOPWORDS;

    let stopwords: HashSet<&str> = STOPWORDS.iter().copied().collect();

    let mut stmt = conn.prepare(
        "SELECT lm.normalized_text
         FROM line_mapping lm
         JOIN works w ON w.abbrev = lm.work_abbrev
         WHERE w.author = ?1",
    )?;
    let rows = stmt.query_map([author], |row| row.get::<_, String>(0))?;

    let mut words: HashSet<String> = HashSet::new();
    for row in rows {
        let line = row?;
        for token in line.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}') {
            let lower = token.to_lowercase();
            if lower.len() >= 2 && !stopwords.contains(lower.as_str()) {
                words.insert(lower);
            }
        }
    }

    let mut result: Vec<String> = words.into_iter().collect();
    result.sort();
    Ok(result)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib db::concordance::tests -- 2>&1 | tail -10`
Expected: both tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/db/concordance.rs
git commit -m "feat(concordance): add load_concordance_words with stopword filtering"
```

---

### Task 3: Add author parameter to `find_word_occurrences`

**Files:**
- Modify: `src/db/concordance.rs:19-49`
- Modify: `src/input/actions/concordance.rs:26` (call site)

- [ ] **Step 1: Write test for author-filtered search**

Add to the `tests` module in `src/db/concordance.rs`:

```rust
    #[test]
    fn find_occurrences_filters_by_author() {
        let conn = test_conn();
        let hits = find_word_occurrences(&conn, "love", "Shakespeare, William").unwrap();
        assert!(!hits.is_empty());
        for hit in &hits {
            assert_eq!(hit.author, "Shakespeare, William");
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib db::concordance::tests::find_occurrences_filters_by_author -- 2>&1 | tail -10`
Expected: FAIL — function signature mismatch (takes 2 args, expected 3)

- [ ] **Step 3: Add `author` parameter to `find_word_occurrences`**

Replace the existing function (lines 19-49) with:

```rust
/// Find all lines containing `word` across works by `author`.
/// Results ordered by work, position within work.
pub fn find_word_occurrences(
    conn: &Connection,
    word: &str,
    author: &str,
) -> Result<Vec<ConcordanceRow>, rusqlite::Error> {
    let pattern = format!("%{}%", word.to_lowercase());
    let mut stmt = conn.prepare(
        "SELECT lm.id, lm.work_abbrev, w.title, w.author,
                lm.div1, COALESCE(lm.div2, 0), lm.line_in_div, lm.canonical_text,
                EXISTS(
                    SELECT 1 FROM line_timestamps lt WHERE lt.line_mapping_id = lm.id
                ) AS has_audio
         FROM line_mapping lm
         JOIN works w ON w.abbrev = lm.work_abbrev
         WHERE w.author = ?1
           AND lm.normalized_text LIKE ?2
         ORDER BY lm.work_abbrev, lm.div1, COALESCE(lm.div2, 0), lm.line_in_div",
    )?;
    let rows = stmt.query_map(rusqlite::params![author, &pattern], |row| {
        Ok(ConcordanceRow {
            line_mapping_id: row.get(0)?,
            work_abbrev: row.get(1)?,
            title: row.get(2)?,
            author: row.get(3)?,
            div1: row.get(4)?,
            div2: row.get(5)?,
            line_in_div: row.get(6)?,
            canonical_text: row.get(7)?,
            has_audio: row.get::<_, i64>(8)? != 0,
        })
    })?;
    rows.collect()
}
```

- [ ] **Step 4: Fix the call site in `src/input/actions/concordance.rs`**

At line 26, the call is:
```rust
crate::db::concordance::find_word_occurrences(&conn, &word_clone)
```

Change to:
```rust
crate::db::concordance::find_word_occurrences(&conn, &word_clone, &author_clone)
```

This requires capturing the author before the async block. At the start of `handle_word_selection` (around line 18), add author extraction:

```rust
let author = {
    let s = state.borrow();
    s.current_work.as_ref().map(|w| w.author.clone()).unwrap_or_default()
};
let author_clone = author.clone();
```

Then pass `author_clone` into the `spawn_blocking` closure (add it to the move capture).

- [ ] **Step 5: Verify it compiles and tests pass**

Run: `cargo build 2>&1 | tail -5 && cargo test --lib db::concordance::tests -- 2>&1 | tail -10`
Expected: compiles, all 3 tests pass

- [ ] **Step 6: Commit**

```bash
git add src/db/concordance.rs src/input/actions/concordance.rs
git commit -m "feat(concordance): scope find_word_occurrences to same author"
```

---

### Task 4: Replace `advance_within_work`/`retreat_within_work` with `advance`/`retreat`

**Files:**
- Modify: `src/concordance.rs:32-62`
- Modify: `src/input/actions/concordance.rs:98-140` (call sites)

- [ ] **Step 1: Write tests for new navigation functions**

Add a `#[cfg(test)] mod tests` block at the bottom of `src/concordance.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(n: usize) -> ConcordanceState {
        let hits: Vec<ConcordanceHit> = (0..n).map(|i| ConcordanceHit {
            work_abbrev: format!("work{}", i % 3),
            work_title: format!("Title {}", i),
            author: "Test Author".to_string(),
            line_mapping_id: i as i64,
            div1: 1,
            div2: 1,
            line_in_div: i as i64,
            canonical_text: format!("line {}", i),
            has_audio: false,
        }).collect();
        ConcordanceState::new("test".to_string(), hits)
    }

    #[test]
    fn advance_wraps_around() {
        let mut state = make_state(5);
        assert_eq!(state.current_index, 0);
        state.advance();
        assert_eq!(state.current_index, 1);
        state.current_index = 4;
        state.advance();
        assert_eq!(state.current_index, 0);
    }

    #[test]
    fn retreat_wraps_around() {
        let mut state = make_state(5);
        assert_eq!(state.current_index, 0);
        state.retreat();
        assert_eq!(state.current_index, 4);
    }

    #[test]
    fn advance_empty_returns_false() {
        let mut state = make_state(0);
        assert!(!state.advance());
    }

    #[test]
    fn retreat_empty_returns_false() {
        let mut state = make_state(0);
        assert!(!state.retreat());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib concordance::tests -- 2>&1 | tail -10`
Expected: FAIL — `advance` method not found

- [ ] **Step 3: Replace navigation methods in `src/concordance.rs`**

Remove `advance_within_work` (lines 33-46) and `retreat_within_work` (lines 48-62). Replace with:

```rust
    /// Advance to the next occurrence. Wraps around the full list.
    pub fn advance(&mut self) -> bool {
        if self.occurrences.is_empty() {
            return false;
        }
        self.current_index = (self.current_index + 1) % self.occurrences.len();
        true
    }

    /// Retreat to the previous occurrence. Wraps around the full list.
    pub fn retreat(&mut self) -> bool {
        if self.occurrences.is_empty() {
            return false;
        }
        let len = self.occurrences.len();
        self.current_index = (self.current_index + len - 1) % len;
        true
    }
```

- [ ] **Step 4: Fix call sites in `src/input/actions/concordance.rs`**

In `jump_to_next_vocab` (line 107), change:
```rust
conc.advance_within_work(abbrev)
```
to:
```rust
conc.advance()
```

In `jump_to_prev_vocab` (line 130), change:
```rust
conc.retreat_within_work(abbrev)
```
to:
```rust
conc.retreat()
```

Also remove the `current_abbrev` variable and the `let (Some(conc), Some(ref abbrev))` destructuring in both functions since we no longer need the work_abbrev. Simplify to:

For `jump_to_next_vocab` (lines 98-117):
```rust
pub(crate) fn jump_to_next_vocab(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let has_concordance = state.borrow().concordance_state.is_some();
    if has_concordance {
        let advanced = {
            let mut s = state.borrow_mut();
            if let Some(conc) = s.concordance_state.as_mut() {
                conc.advance()
            } else {
                false
            }
        };
        if advanced {
            concordance_jump_to_current(state, tokio_handle);
        }
    } else {
        navigation::jump_to_next_vocab(&mut state.borrow_mut());
    }
}
```

For `jump_to_prev_vocab` (lines 121-140):
```rust
pub(crate) fn jump_to_prev_vocab(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let has_concordance = state.borrow().concordance_state.is_some();
    if has_concordance {
        let retreated = {
            let mut s = state.borrow_mut();
            if let Some(conc) = s.concordance_state.as_mut() {
                conc.retreat()
            } else {
                false
            }
        };
        if retreated {
            concordance_jump_to_current(state, tokio_handle);
        }
    } else {
        navigation::jump_to_prev_vocab(&mut state.borrow_mut());
    }
}
```

- [ ] **Step 5: Verify it compiles and tests pass**

Run: `cargo build 2>&1 | tail -5 && cargo test --lib concordance::tests -- 2>&1 | tail -10`
Expected: compiles, all 4 tests pass

- [ ] **Step 6: Commit**

```bash
git add src/concordance.rs src/input/actions/concordance.rs
git commit -m "feat(concordance): replace work-scoped nav with full-list advance/retreat"
```

---

### Task 5: Add `ConcordanceNext`/`ConcordancePrev` actions and keybinds

**Files:**
- Modify: `src/input/actions/mod.rs:30-136` (Action enum)
- Modify: `src/input/actions/mod.rs:138-232` (category impl)
- Modify: `src/input/actions/mod.rs:234-314` (name impl)
- Modify: `src/input/keymap_config.rs:256-271` (vocab_bindings)
- Modify: `src/input/keymap.rs:854-855` (dispatch_action)

- [ ] **Step 1: Add variants to the Action enum**

In `src/input/actions/mod.rs`, after line 85 (`JumpToPrevVocab,`), add:

```rust
    ConcordanceNext,
    ConcordancePrev,
```

- [ ] **Step 2: Add category mapping**

In the `category()` match, add the two new variants to the Vocab arm. After line 185 (`| Action::OpenConcordanceListPicker => Category::Vocab,`), the new variants are already covered if we add them in the Vocab section. Add before the `=> Category::Vocab` closing:

```rust
            | Action::ConcordanceNext
            | Action::ConcordancePrev
```

Place these after `Action::OpenConcordanceListPicker` in the Vocab arm.

- [ ] **Step 3: Add name mapping**

In the `name()` match, add after line 281 (`Action::OpenGlossPicker => "OpenGlossPicker",`):

```rust
            Action::ConcordanceNext => "ConcordanceNext",
            Action::ConcordancePrev => "ConcordancePrev",
```

- [ ] **Step 4: Add keybinds in `src/input/keymap_config.rs`**

In `vocab_bindings()` (after line 268), add:

```rust
        (KeyCombo::ctrl("n"), Action::ConcordanceNext),
        (KeyCombo::ctrl("p"), Action::ConcordancePrev),
```

- [ ] **Step 5: Wire in `dispatch_action` in `src/input/keymap.rs`**

After line 855 (`JumpToPrevVocab => ...`), add:

```rust
        ConcordanceNext => crate::input::actions::concordance::concordance_next(state, tokio_handle),
        ConcordancePrev => crate::input::actions::concordance::concordance_prev(state, tokio_handle),
```

- [ ] **Step 6: Add stub functions to `src/input/actions/concordance.rs`**

Add after the `jump_to_prev_vocab` function:

```rust
/// Ctrl+n: advance to next concordance hit (cross-work). No-op if no state.
pub(crate) fn concordance_next(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let advanced = {
        let mut s = state.borrow_mut();
        if let Some(conc) = s.concordance_state.as_mut() {
            conc.advance()
        } else {
            false
        }
    };
    if advanced {
        concordance_jump_to_current(state, tokio_handle);
    }
}

/// Ctrl+p: retreat to previous concordance hit (cross-work). No-op if no state.
pub(crate) fn concordance_prev(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let retreated = {
        let mut s = state.borrow_mut();
        if let Some(conc) = s.concordance_state.as_mut() {
            conc.retreat()
        } else {
            false
        }
    };
    if retreated {
        concordance_jump_to_current(state, tokio_handle);
    }
}
```

- [ ] **Step 7: Simplify `jump_to_next_vocab`/`jump_to_prev_vocab`**

Now that dedicated `concordance_next`/`concordance_prev` actions exist, `r`/`R` should always do plain vocab jump. Remove the concordance-state check from both functions:

Replace `jump_to_next_vocab` with:
```rust
pub(crate) fn jump_to_next_vocab(
    state: &Rc<RefCell<AppState>>,
    _tokio_handle: &tokio::runtime::Handle,
) {
    navigation::jump_to_next_vocab(&mut state.borrow_mut());
}
```

Replace `jump_to_prev_vocab` with:
```rust
pub(crate) fn jump_to_prev_vocab(
    state: &Rc<RefCell<AppState>>,
    _tokio_handle: &tokio::runtime::Handle,
) {
    navigation::jump_to_prev_vocab(&mut state.borrow_mut());
}
```

- [ ] **Step 8: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles successfully

- [ ] **Step 9: Run all tests**

Run: `cargo test 2>&1 | tail -15`
Expected: all tests pass

- [ ] **Step 10: Commit**

```bash
git add src/input/actions/mod.rs src/input/keymap_config.rs src/input/keymap.rs src/input/actions/concordance.rs
git commit -m "feat(concordance): add ConcordanceNext/Prev actions on Ctrl+n/p"
```

---

### Task 6: Implement cross-work navigation in `concordance_jump_to_current`

**Files:**
- Modify: `src/input/actions/concordance.rs:184-243`

- [ ] **Step 1: Replace the cross-work branch with in-place `display_work_at`**

The current `concordance_jump_to_current` function (lines 184-243) spawns a new instance for cross-work jumps. Replace the entire function with:

```rust
/// Jump to the current concordance occurrence.
/// Loads the work in-place if different from current, positions cursor on the line.
pub fn concordance_jump_to_current(
    state: &Rc<RefCell<AppState>>,
    _handle: &tokio::runtime::Handle,
) {
    let (target_abbrev, target_line_id) = {
        let s = state.borrow();
        let conc = match &s.concordance_state {
            Some(c) => c,
            None => return,
        };
        let hit = match conc.current_hit() {
            Some(h) => h,
            None => return,
        };
        (hit.work_abbrev.clone(), hit.line_mapping_id)
    };

    let current_abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());

    crate::logging::log(&format!(
        "CONC_JUMP: target_abbrev='{}' target_line_id={} current_abbrev={:?}",
        target_abbrev, target_line_id, current_abbrev
    ));

    if current_abbrev.as_deref() != Some(&target_abbrev) {
        // Cross-work jump: load the new work in-place.
        crate::logging::log(&format!(
            "CONC_JUMP: loading '{}' in-place for line_id={}", target_abbrev, target_line_id
        ));

        // Save position of current work
        crate::app::save_position(&mut state.borrow_mut());

        // Quit current MPV connection
        {
            let s = state.borrow();
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Quit);
        }

        // Load the new work asynchronously
        let state_clone = Rc::clone(state);
        let abbrev_for_load = target_abbrev.clone();
        let handle = state.borrow().tokio_handle.clone();
        glib::spawn_future_local(async move {
            let result = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                    let work = crate::db::queries::load_work(&conn, &abbrev_for_load)?;
                    let prepared = crate::app::prepare_text_for_display(&work);
                    Ok::<_, rusqlite::Error>((work, prepared))
                })
                .await;
            match result {
                Ok(Ok((work, prepared))) => {
                    {
                        let mut s = state_clone.borrow_mut();
                        crate::app::clear_display(&mut s);
                        crate::app::display_work_at_with_prepared(
                            &mut s,
                            work,
                            Some(target_line_id),
                            prepared,
                        );
                    }
                    // Update concordance bar after work loads
                    let s = state_clone.borrow();
                    concordance_update_bar(&s);
                }
                Ok(Err(e)) => {
                    crate::logging::log(&format!("CONC_JUMP: load_work error: {}", e));
                }
                Err(e) => {
                    crate::logging::log(&format!("CONC_JUMP: spawn_blocking error: {}", e));
                }
            }
        });
    } else {
        // Same work, just move cursor
        crate::logging::log("CONC_JUMP: same work, positioning cursor");
        let mut s = state.borrow_mut();
        concordance_position_cursor(&mut s, target_line_id);
        concordance_update_bar(&s);
    }
}
```

- [ ] **Step 2: Add missing import**

At the top of `src/input/actions/concordance.rs`, ensure `Rc` is imported (it already is from line 1-2).

- [ ] **Step 3: Remove spawn-based cross-work logic from `handle_word_selection`**

In `handle_word_selection` (lines 43-79), the code partitions hits into `current_work_hits` and `other_works`, then spawns instances for other works. Replace the partitioning and spawn logic with simply storing ALL hits in ConcordanceState.

Replace the body inside the `glib::spawn_future_local` async block (after `if hits.is_empty() { return; }`) with:

```rust
        let current_abbrev = state_clone
            .borrow()
            .current_work
            .as_ref()
            .map(|w| w.abbrev.clone())
            .unwrap_or_default();

        // Convert all hits to ConcordanceHit
        let all_hits: Vec<crate::concordance::ConcordanceHit> = hits
            .into_iter()
            .map(|h| crate::concordance::ConcordanceHit {
                work_abbrev: h.work_abbrev,
                work_title: h.title,
                author: h.author,
                line_mapping_id: h.line_mapping_id,
                div1: h.div1,
                div2: h.div2,
                line_in_div: h.line_in_div,
                canonical_text: h.canonical_text,
                has_audio: h.has_audio,
            })
            .collect();

        if all_hits.is_empty() {
            return;
        }

        // Find the first hit in the current work to start from, else start at 0
        let start_index = all_hits
            .iter()
            .position(|h| h.work_abbrev == current_abbrev)
            .unwrap_or(0);

        let mut conc_state = crate::concordance::ConcordanceState::new(
            word.clone(),
            all_hits,
        );
        conc_state.current_index = start_index;

        {
            let mut s = state_clone.borrow_mut();
            s.concordance_bar.update(&conc_state.status_label(), &conc_state.status_work());
            s.concordance_state = Some(conc_state);
        }
        concordance_jump_to_current(&state_clone, &handle);
```

- [ ] **Step 4: Remove unused imports**

Remove `std::collections::BTreeMap` usage (it was only used for `other_works`). The `use std::process::Command` pattern (if any) can also be removed — but check if it's imported at the file level or inline. In the current code it's used as `std::process::Command::new` inline, so no import to remove.

- [ ] **Step 5: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles successfully

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/concordance.rs
git commit -m "feat(concordance): in-place cross-work navigation via display_work_at"
```

---

### Task 7: Change `open_picker` to use author-scoped word list with cache

**Files:**
- Modify: `src/app.rs:63-212` (AppState struct — add field)
- Modify: `src/input/actions/concordance.rs:144-176` (open_picker function)

- [ ] **Step 1: Add `concordance_word_cache` field to AppState**

In `src/app.rs`, after line 182 (`pub concordance_state: Option<crate::concordance::ConcordanceState>,`), add:

```rust
    pub concordance_word_cache: Option<(String, Vec<String>)>,
```

- [ ] **Step 2: Initialize the field in the AppState construction**

Find where AppState is constructed (search for `concordance_state: None`). Add after it:

```rust
        concordance_word_cache: None,
```

- [ ] **Step 3: Replace `open_picker` to use author word list**

Replace the `open_picker` function in `src/input/actions/concordance.rs` with:

```rust
/// Open the concordance picker, populating it with all content words
/// from the current author's works (minus stopwords). Called from `Ctrl+\`.
pub(crate) fn open_picker(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let author = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.author.clone());
    let author = match author {
        Some(a) => a,
        None => return,
    };

    // Check cache
    let cached = state.borrow().concordance_word_cache.as_ref()
        .filter(|(a, _)| a == &author)
        .map(|(_, words)| words.clone());

    if let Some(words) = cached {
        let mut s = state.borrow_mut();
        s.concordance_picker.set_words(
            words.iter().map(|w| (w.clone(), 0usize)).collect()
        );
        s.concordance_picker.show();
        s.input_mode = crate::app::InputMode::ConcordancePicker;
        drop(s);
        state.borrow().concordance_picker.search_entry().set_text("");
    } else {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        let author_clone = author.clone();
        glib::spawn_future_local(async move {
            let words = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::concordance::load_concordance_words(&conn, &author_clone)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            {
                let mut s = state_clone.borrow_mut();
                s.concordance_word_cache = Some((author.clone(), words.clone()));
                s.concordance_picker.set_words(
                    words.iter().map(|w| (w.clone(), 0usize)).collect()
                );
                s.concordance_picker.show();
                s.input_mode = crate::app::InputMode::ConcordancePicker;
            }
            state_clone.borrow().concordance_picker.search_entry().set_text("");
        });
    }
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles successfully

- [ ] **Step 5: Run all tests**

Run: `cargo test 2>&1 | tail -15`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/input/actions/concordance.rs
git commit -m "feat(concordance): author-scoped word list with caching in picker"
```

---

### Task 8: Remove spawn-on-startup logic and clean up

**Files:**
- Modify: `src/app.rs` (env var spawn handling)
- Modify: `src/main.rs` (concordance spawn app_id logic)

- [ ] **Step 1: Identify and remove concordance spawn-on-startup code**

Search for `LINUX_LIT_CONC_WORD` and `LINUX_LIT_WORK`/`LINUX_LIT_LINE_ID` concordance-related startup logic. The spawn-based flow used env vars to open a specific work+line in a new instance. Since we no longer spawn, this env-var-based startup code for concordance can remain (it's useful for other launch scenarios) OR be removed if it's only for concordance spawns.

Check what the env var startup code does:

Run: `rg -n "LINUX_LIT_CONC_WORD\|CONC_WORD" src/`

If the only purpose of `LINUX_LIT_CONC_WORD` was to set up concordance state in spawned instances, remove that logic. Keep `LINUX_LIT_WORK` and `LINUX_LIT_LINE_ID` if they serve other purposes (e.g., external scripting).

- [ ] **Step 2: Remove `LINUX_LIT_CONC_WORD` handling**

If found, remove the code that reads this env var and sets up concordance state on launch. The spawned-instance concordance mode is no longer needed.

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo build 2>&1 | tail -5 && cargo test 2>&1 | tail -15`
Expected: compiles, all tests pass

- [ ] **Step 4: Commit**

```bash
git add -u
git commit -m "chore(concordance): remove spawn-instance env var handling"
```

---

### Task 9: Final integration test

**Files:** None modified — verification only.

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1 | tail -20`
Expected: no errors (warnings are acceptable)

- [ ] **Step 3: Verify build**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles successfully

- [ ] **Step 4: Manual test checklist (for the user)**

The user should verify:
1. `Ctrl+\` opens picker with alphabetical content words from the author
2. Selecting a word shows concordance bar with hit count
3. `Ctrl+n` advances to next hit (same work: cursor moves; different work: new work loads)
4. `Ctrl+p` retreats to previous hit
5. Media opens correctly when navigating to a new work
6. `r`/`R` always does plain vocab jump regardless of concordance state
7. Concordance state persists through manual work switches
8. Picking a new word replaces the old concordance state
