# Add Vocab Word Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `Ctrl+Alt+\` main-card keybind that opens an empty vim-input card to add a vocabulary word, looking up its definition (local `wn`/`dict`, Claude API fallback), then enabling highlighting and refreshing the vocab popup so the new word is live in the current view.

**Architecture:** A new pure `vocab_lookup` module shells out to `wn`/`dict`. A new `vocab_add` orchestrator module opens the input card (reusing the `gloss_overlay` edit buffer + a new `InputMode::AddVocab`, exactly like `segment_vim`), normalizes the submitted word, runs the lookup ladder, inserts via a new idempotent `insert_vocab_word` query, and converges on a shared `apply_after_add` refresh in `src/app/mod.rs`. Local hits complete synchronously; a Claude fallback inserts and refreshes in the async `run_claude_request` success callback. Separately, the redundant `H` popup toggle is removed (the `rr` chord already covers show/hide).

**Tech Stack:** Rust, GTK4, rusqlite (SQLite), Tokio (async Claude bridge), existing `claude_bridge::run_claude_request`, `wn` (WordNet CLI) + `dict -d gcide` (GNU dict) shell-outs.

## Global Constraints

- Build check only: `cargo build`. Do NOT run the app — the user runs `cargo run` / `crll` themselves.
- `vocab_words` is a GLOBAL table (no work/author scope). Column `word` is `UNIQUE`; columns are `id, word, definition, source, difficulty_level, created_at`.
- Every keybind change updates BOTH the compiled default (`keymap_config.rs`) AND `~/.config/linux-lit/keymap.json` (stowed from `~/tty-dotfiles/linux-lit/`), else the JSON silently shadows the compiled change.
- Every main-card keybind change updates the Ctrl+/ overlay (`src/ui/keybinds_overlay.rs` — keycap strip AND `describe()` arm).
- Bash CLI rules: use `rg`/`fd`, never `grep -r`/`find`. Non-interactive `\cp -f` / `command rm -f` to bypass safe-alias hangs.
- Timestamps (if any needed): `TZ='America/Chicago' date +"%Y-%m-%dT%H:%M:%SZ"`.
- `KeyCombo` constructor for the new bind: `KeyCombo::ctrl_alt("backslash")` (helper exists at `keymap_config.rs:42`).
- Toast helper: `crate::input::navigation::show_chapter_toast_secs(&s, text, secs)` (takes `&AppState`).

---

### Task 1: Remove the redundant `H` → ToggleVocabPopup bind

**Files:**
- Modify: `src/input/keymap_config.rs:352`
- Modify: `~/.config/linux-lit/keymap.json:47` and its stow source `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`
- Modify: `src/ui/keybinds_overlay.rs:74,326`

**Interfaces:**
- Consumes: nothing.
- Produces: nothing new (removal only). `Action::ToggleVocabPopup` and its handler at `keymap.rs:3770` STAY defined (unbound-by-default, like `HideVocabPopup`).

- [ ] **Step 1: Delete the compiled default bind**

In `src/input/keymap_config.rs`, find line 352:

```rust
        (KeyCombo::plain("H"), Action::ToggleVocabPopup),
```

Delete that entire line.

- [ ] **Step 2: Delete the keymap.json override (both copies)**

In `~/.config/linux-lit/keymap.json`, find line 47:

```json
    {"key": "H", "action": "ToggleVocabPopup"},
```

Delete that line. Then find the stow source and delete the same line there:

```bash
fd -H keymap.json ~/tty-dotfiles/linux-lit/
```

Edit that file to remove the identical `{"key": "H", "action": "ToggleVocabPopup"}` entry. (If `~/.config/linux-lit/keymap.json` is a symlink into the stow package, editing one edits both — verify with `ls -l ~/.config/linux-lit/keymap.json`; if it is a symlink, Step 2's two edits collapse into one.)

- [ ] **Step 3: Remove the overlay keycap + describe arm**

In `src/ui/keybinds_overlay.rs` line 74, the current entry is:

```rust
    key("h", "H", "dlg fwd", "H: auto vocab", &[("C-h", "synopsis")]),
```

The `"H"` shifted-cap detail (`"H: auto vocab"`) must go; keep the unshifted `h` ("dlg fwd") and the `C-h synopsis` chord. Change it to:

```rust
    key("h", "", "dlg fwd", "", &[("C-h", "synopsis")]),
```

(Confirm the `key(...)` signature arms — args are `unshifted_cap, shifted_cap, unshifted_label, shifted_label, chords`. If a different arity, blank only the shifted-cap + shifted-label slots that carried `"H"` / `"H: auto vocab"`.)

Then at line 326, delete the describe() arm:

```rust
        "auto vocab" => "Action::ToggleVocabPopup — src/app/vocab_popup.rs",
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: compiles clean (no unused-variant warning — `ToggleVocabPopup` is still referenced by its handler and `category()`).

- [ ] **Step 5: Restow if keymap.json is not a symlink**

If Step 2 found two separate files:

```bash
cd ~/tty-dotfiles && stow linux-lit
```

- [ ] **Step 6: Commit**

```bash
git add src/input/keymap_config.rs src/ui/keybinds_overlay.rs
git commit -m "$(cat <<'EOF'
feat(vocab): drop redundant H popup toggle (rr covers show/hide)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RcpEnoaGznf2FVHaAQtChG
EOF
)"
```

(The keymap.json in `~/.config` and `~/tty-dotfiles` are outside this repo — do NOT git add them from here; they are version-controlled in `tty-dotfiles`. Commit them separately in that repo if it is dirty.)

---

### Task 2: Pure definition lookup module (`vocab_lookup.rs`)

**Files:**
- Create: `src/vocab_lookup.rs`
- Modify: `src/main.rs` (or wherever `mod` declarations live — add `mod vocab_lookup;`)
- Test: inline `#[cfg(test)]` in `src/vocab_lookup.rs`

**Interfaces:**
- Consumes: nothing (std only).
- Produces:
  - `pub fn lookup_local(word: &str) -> Option<(String, String)>` — returns `(definition, source)` where source is `"wordnet"` or `"gcide"`, or `None`.
  - `pub(crate) fn parse_wn(stdout: &str) -> Option<String>`
  - `pub(crate) fn parse_gcide(stdout: &str) -> Option<String>`

- [ ] **Step 1: Add the module declaration**

Find the module list (top of `src/main.rs`; confirm with `rg -n '^mod |^pub mod ' src/main.rs`). Add alphabetically:

```rust
mod vocab_lookup;
```

- [ ] **Step 2: Write failing tests for the parsers**

Create `src/vocab_lookup.rs`:

```rust
//! Local dictionary lookup for the add-vocab-word flow. Shells to WordNet
//! (`wn`) then GNU dict/gcide (`dict -d gcide`), mirroring litdb's
//! `scripts/vocab/definitions.py`. Parsing is split from the `Command`
//! invocation so it is unit-testable without the CLI tools installed.

use std::process::Command;

/// Parse `wn <word> -over` output: the first `-- (definition…)` gloss.
pub(crate) fn parse_wn(stdout: &str) -> Option<String> {
    // wn overview lines look like: "1. (12) word -- (the definition text)"
    for line in stdout.lines() {
        if let Some(idx) = line.find("-- (") {
            let rest = &line[idx + 4..];
            if let Some(end) = rest.rfind(')') {
                let def = rest[..end].trim();
                if !def.is_empty() {
                    return Some(def.to_string());
                }
            }
        }
    }
    None
}

/// Parse `dict -d gcide <word>` output: the first sense line after the
/// headword block. gcide senses are indented and often start with a POS tag.
pub(crate) fn parse_gcide(stdout: &str) -> Option<String> {
    // Take the first non-empty line that looks like a definition body:
    // skip the "From ... [gcide]:" header, the headword line, and blanks.
    let mut saw_header = false;
    for line in stdout.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("From ") && t.contains("[gcide]") {
            saw_header = true;
            continue;
        }
        if !saw_header {
            continue;
        }
        // First substantive line after the header is the headword; the sense
        // text follows. Heuristic: a line containing "Defn:" or the first
        // sentence-like line. Prefer the text after a "Defn:" marker.
        if let Some(idx) = t.find("Defn:") {
            let def = t[idx + 5..].trim();
            if !def.is_empty() {
                return Some(def.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wn_extracts_first_gloss() {
        let out = "\nOverview of noun brave\n\nThe noun brave has 1 sense\n\n1. (2) brave, courageous -- (a North American Indian warrior)\n";
        assert_eq!(
            parse_wn(out).as_deref(),
            Some("a North American Indian warrior")
        );
    }

    #[test]
    fn parse_wn_none_when_no_gloss() {
        assert_eq!(parse_wn("No information available for word\n"), None);
    }

    #[test]
    fn parse_gcide_extracts_defn() {
        let out = "1 definition found\n\nFrom The Collaborative International Dictionary of English v.0.48 [gcide]:\n\n  Brave \\Brave\\, a.\n     Defn: Bold; courageous; daring; intrepid.\n";
        assert_eq!(
            parse_gcide(out).as_deref(),
            Some("Bold; courageous; daring; intrepid.")
        );
    }

    #[test]
    fn parse_gcide_none_when_empty() {
        assert_eq!(parse_gcide(""), None);
    }
}
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test --bins vocab_lookup -- --nocapture`
Expected: 4 tests PASS. (These test pure parsers, so they pass immediately — TDD here is "tests define the parser contract"; if any fail, fix the parser to match the canned output.)

- [ ] **Step 4: Add the `lookup_local` shell-out driver**

Append to `src/vocab_lookup.rs` (above the `#[cfg(test)]` module):

```rust
/// Try WordNet then gcide. Returns `(definition, source)` or `None` if both
/// are silent or the binaries are absent. A spawn error (tool not installed)
/// is treated as "no result", never a panic.
pub fn lookup_local(word: &str) -> Option<(String, String)> {
    if let Some(out) = run(&["wn", word, "-over"]) {
        if let Some(def) = parse_wn(&out) {
            return Some((def, "wordnet".to_string()));
        }
    }
    if let Some(out) = run_dict(word) {
        if let Some(def) = parse_gcide(&out) {
            return Some((def, "gcide".to_string()));
        }
    }
    None
}

fn run(args: &[&str]) -> Option<String> {
    let (cmd, rest) = args.split_first()?;
    let output = Command::new(cmd).args(rest).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_dict(word: &str) -> Option<String> {
    let output = Command::new("dict")
        .args(["-d", "gcide", word])
        .output()
        .ok()?;
    // dict exits non-zero (20/21) when the word is not found — that is a
    // legitimate "no definition", not an error to log.
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}
```

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 6: Commit**

```bash
git add src/vocab_lookup.rs src/main.rs
git commit -m "$(cat <<'EOF'
feat(vocab): local wn/dict definition lookup module

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RcpEnoaGznf2FVHaAQtChG
EOF
)"
```

---

### Task 3: Idempotent `insert_vocab_word` DB query

**Files:**
- Modify: `src/db/queries.rs` (add near `load_vocab_words` ~line 534 / `set_vocab_highlight` ~line 1148)
- Test: inline `#[cfg(test)]` in `src/db/queries.rs` (or the existing test module there)

**Interfaces:**
- Consumes: `rusqlite::Connection` (via `open_db_rw()`).
- Produces:
  - `pub enum VocabInsertOutcome { Added, AlreadyPresent }`
  - `pub fn insert_vocab_word(conn: &Connection, word: &str, definition: &str, source: &str) -> Result<VocabInsertOutcome, rusqlite::Error>`

- [ ] **Step 1: Write the failing test**

Add to the test module in `src/db/queries.rs` (find it with `rg -n '#\[cfg\(test\)\]' src/db/queries.rs`; if none, create one at the end of the file). The test builds an in-memory DB with the `vocab_words` shape:

```rust
#[cfg(test)]
mod vocab_insert_tests {
    use super::*;
    use rusqlite::Connection;

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE vocab_words (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                word TEXT NOT NULL UNIQUE,
                definition TEXT NOT NULL,
                difficulty_level INTEGER,
                created_at TEXT DEFAULT (datetime('now')),
                source TEXT
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn insert_new_word_reports_added() {
        let conn = mem_db();
        let out = insert_vocab_word(&conn, "brave", "courageous", "wordnet").unwrap();
        assert!(matches!(out, VocabInsertOutcome::Added));
        let def: String = conn
            .query_row("SELECT definition FROM vocab_words WHERE word='brave'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(def, "courageous");
    }

    #[test]
    fn reinsert_keeps_good_definition_reports_already_present() {
        let conn = mem_db();
        insert_vocab_word(&conn, "brave", "courageous", "wordnet").unwrap();
        let out = insert_vocab_word(&conn, "brave", "SOMETHING ELSE", "claude").unwrap();
        assert!(matches!(out, VocabInsertOutcome::AlreadyPresent));
        let def: String = conn
            .query_row("SELECT definition FROM vocab_words WHERE word='brave'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(def, "courageous"); // unchanged
    }

    #[test]
    fn reinsert_fills_empty_definition() {
        let conn = mem_db();
        insert_vocab_word(&conn, "brave", "", "wordnet").unwrap();
        let out = insert_vocab_word(&conn, "brave", "courageous", "gcide").unwrap();
        assert!(matches!(out, VocabInsertOutcome::Added));
        let def: String = conn
            .query_row("SELECT definition FROM vocab_words WHERE word='brave'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(def, "courageous");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins vocab_insert_tests -- --nocapture`
Expected: FAIL — `insert_vocab_word` / `VocabInsertOutcome` not defined.

- [ ] **Step 3: Implement**

Add near `set_vocab_highlight` in `src/db/queries.rs`:

```rust
/// Result of `insert_vocab_word`: whether the row was newly written / filled,
/// or already had a good definition and was left untouched.
pub enum VocabInsertOutcome {
    Added,
    AlreadyPresent,
}

/// Insert a vocab word, idempotent on the UNIQUE `word` column. A new word is
/// inserted; an existing word with an EMPTY definition is filled; an existing
/// word with a good definition is left intact. `word` is expected already
/// normalized (trimmed, lowercased) by the caller.
pub fn insert_vocab_word(
    conn: &Connection,
    word: &str,
    definition: &str,
    source: &str,
) -> Result<VocabInsertOutcome, rusqlite::Error> {
    let changed = conn.execute(
        "INSERT INTO vocab_words(word, definition, source) VALUES(?1, ?2, ?3)
         ON CONFLICT(word) DO UPDATE SET definition = excluded.definition, source = excluded.source
           WHERE vocab_words.definition = '' OR vocab_words.definition IS NULL",
        rusqlite::params![word, definition, source],
    )?;
    Ok(if changed > 0 {
        VocabInsertOutcome::Added
    } else {
        VocabInsertOutcome::AlreadyPresent
    })
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --bins vocab_insert_tests -- --nocapture`
Expected: 3 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "$(cat <<'EOF'
feat(vocab): idempotent insert_vocab_word query

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RcpEnoaGznf2FVHaAQtChG
EOF
)"
```

---

### Task 4: Word normalizer + `apply_after_add` refresh (in `src/app/mod.rs`)

**Files:**
- Modify: `src/app/mod.rs` (add `pub fn apply_after_add` near `apply_vocab_highlighting` ~line 4482; `build_vocab_matches` at 4400 is private to this module, so the refresh lives here)
- Create: normalizer as `pub fn normalize_vocab_word` in `src/vocab_lookup.rs` (pure, co-located with lookup)
- Test: inline tests in `src/vocab_lookup.rs`

**Interfaces:**
- Consumes: `insert_vocab_word` outcome (Task 3), `crate::db::queries::{load_vocab_words, set_vocab_highlight, open_db_rw}`, `apply_vocab_highlighting`, `remove_vocab_highlighting`, `build_vocab_matches` (module-private), `refresh_vocab_popup`.
- Produces:
  - `pub fn normalize_vocab_word(raw: &str) -> String` (in `vocab_lookup.rs`)
  - `pub fn apply_after_add(state: &mut AppState, word: &str, outcome_added: bool, source: &str)` (in `src/app/mod.rs`)

- [ ] **Step 1: Write the failing normalizer test**

Add to the `#[cfg(test)] mod tests` in `src/vocab_lookup.rs`:

```rust
    #[test]
    fn normalize_trims_lowercases_strips_possessive() {
        assert_eq!(normalize_vocab_word("  Brave  "), "brave");
        assert_eq!(normalize_vocab_word("King's"), "king");
        assert_eq!(normalize_vocab_word("kings’"), "kings"); // only trailing 's/’s stripped
        assert_eq!(normalize_vocab_word("Hamlet’s"), "hamlet");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins vocab_lookup -- --nocapture`
Expected: FAIL — `normalize_vocab_word` not defined.

- [ ] **Step 3: Implement the normalizer**

Add to `src/vocab_lookup.rs` (above the test module):

```rust
/// Normalize a submitted vocab word: trim, lowercase, strip a trailing
/// possessive `'s` / `’s`. Matches how highlighting/variants normalize so the
/// stored word lines up with `load_vocab_words`' LOWER(word).
pub fn normalize_vocab_word(raw: &str) -> String {
    let mut w = raw.trim().to_lowercase();
    for suffix in ["'s", "\u{2019}s"] {
        if let Some(stripped) = w.strip_suffix(suffix) {
            w = stripped.to_string();
            break;
        }
    }
    w
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --bins vocab_lookup -- --nocapture`
Expected: all vocab_lookup tests PASS.

- [ ] **Step 5: Implement `apply_after_add` in `src/app/mod.rs`**

Add immediately after `apply_vocab_highlighting` (~line 4482 region). This mirrors the persistence + re-highlight sequence from the `ToggleVocabHighlight` handler:

```rust
/// Shared post-add refresh: enable + persist vocab highlighting, reload the
/// word set, rebuild matches, re-apply the tag, and (if the popup is open with
/// the word on the cursor line) refresh the popup. Called from both the sync
/// local-lookup path and the async Claude success callback.
pub fn apply_after_add(state: &mut AppState, word: &str, outcome_added: bool, source: &str) {
    // Enable highlighting for this work and persist it (source of truth is the
    // per-work lit.db column, like ToggleVocabHighlight).
    state.vocab_highlight_visible = true;
    if let Some(abbrev) = state.current_work.as_ref().map(|w| w.abbrev.clone()) {
        match crate::db::queries::open_db_rw()
            .and_then(|conn| crate::db::queries::set_vocab_highlight(&conn, &abbrev, true))
        {
            Ok(()) => {}
            Err(e) => crate::logging::log(&format!("VOCAB ADD: persist highlight failed: {e}")),
        }
        // Reload the global word set so the just-added word is included.
        if let Ok(conn) = crate::db::queries::open_db() {
            if let Ok(words) = crate::db::queries::load_vocab_words(&conn, &abbrev) {
                state.vocab_words = words;
            }
        }
    }

    remove_vocab_highlighting(state);
    build_vocab_matches(state);
    apply_vocab_highlighting(state);

    // Refresh the popup only if it is open AND the word is on the cursor line.
    let on_line = state
        .vocab_matches
        .iter()
        .any(|m| m.line_index == state.current_line && m.word == word);
    if state.vocab_popup.popup.is_visible() && on_line {
        crate::app::vocab_popup::refresh_vocab_popup(state);
    }

    let verb = if outcome_added { "added" } else { "already have" };
    crate::input::navigation::show_chapter_toast_secs(
        state,
        &format!("{verb} \u{201c}{word}\u{201d} ({source})"),
        3,
    );
    crate::logging::log(&format!("VOCAB ADD: {verb} '{word}' ({source})"));
}
```

- [ ] **Step 6: Build**

Run: `cargo build`
Expected: compiles clean. (If `build_vocab_matches` visibility errors appear, it is because `apply_after_add` was placed outside `src/app/mod.rs` — it MUST be in that module to call the private `build_vocab_matches`.)

- [ ] **Step 7: Commit**

```bash
git add src/vocab_lookup.rs src/app/mod.rs
git commit -m "$(cat <<'EOF'
feat(vocab): word normalizer + apply_after_add view refresh

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RcpEnoaGznf2FVHaAQtChG
EOF
)"
```

---

### Task 5: `InputMode::AddVocab` + the input-card open/close/key-routing

**Files:**
- Modify: `src/app/mod.rs` (add `AddVocab` to `enum InputMode` ~line 88)
- Create: `src/input/actions/vocab_add.rs`
- Modify: `src/input/actions/mod.rs` (register the sub-module: add `pub(crate) mod vocab_add;`)
- Modify: `src/input/keymap.rs` (route `InputMode::AddVocab` keys before mode dispatch, ~line 127 region; add the `unreachable!` arm ~line 232)

**Interfaces:**
- Consumes: `crate::vocab_lookup::normalize_vocab_word`, `crate::app::apply_after_add`, `insert_vocab_word` (Task 3), `gloss_overlay` edit-buffer API (`enter_edit_buffer`, `edit_buffer_text`, `exit_edit_buffer`, `feed_edit_key`), `return_to_reader_mode`.
- Produces:
  - `pub(crate) fn open(state_rc: &Rc<RefCell<AppState>>)`
  - `pub(crate) fn close(state_rc: &Rc<RefCell<AppState>>)`
  - `pub(crate) fn submit(state_rc: &Rc<RefCell<AppState>>)` (Task 6 fills the lookup body; Task 5 stubs it to just close + toast)

- [ ] **Step 1: Add the InputMode variant**

In `src/app/mod.rs`, `enum InputMode` (~line 88), add after `SegmentVim` (find it with `rg -n 'SegmentVim,' src/app/mod.rs`):

```rust
    /// Typing a word into the empty vim-input card to add a vocab word
    /// (Ctrl+Alt+\). All keys route to the gloss_overlay edit buffer; the
    /// save verb (:w) submits (lookup + insert), :q/Esc cancels.
    AddVocab,
```

- [ ] **Step 2: Create the vocab_add module (open/close + stub submit)**

Create `src/input/actions/vocab_add.rs`:

```rust
use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;

/// Ctrl+Alt+\ on the main card: open an EMPTY vim-input card to type a vocab
/// word. Reuses the gloss_overlay edit buffer, exactly like segment_vim, but
/// starts blank and in Normal mode (the reader immediately types `i` or `a`,
/// or we could seed Insert — kept Normal for vim consistency with the other
/// editors). On :w the word is looked up + inserted; :q/Esc cancels.
pub(crate) fn open(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    if s.current_work.is_none() {
        return;
    }
    let (cw, h) = crate::app::layout::overlay_card_size(&s);
    s.gloss_overlay
        .show_gloss_with_color("Add vocab word", "", cw, h, Some(&s.theme.root_color), &[]);
    let (fill, fg) = (s.theme.cursor_bg.clone(), s.theme.cursor_fg.clone());
    s.gloss_overlay.set_edit_copy_only(false); // saving IS allowed here
    s.gloss_overlay.enter_edit_buffer("", &fill, &fg);
    // Seed Insert mode so the reader can type immediately.
    let _ = s
        .gloss_overlay
        .feed_edit_key(crate::input::vim::VimKey::Char('i'));
    s.input_mode = crate::app::InputMode::AddVocab;
    crate::logging::log("VOCAB ADD: opened input card");
}

/// Close the input card without saving and return to the reader.
pub(crate) fn close(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    s.gloss_overlay.exit_edit_buffer();
    s.gloss_overlay.hide();
    crate::app::return_to_reader_mode(&mut s);
    crate::logging::log("VOCAB ADD: cancelled");
}

/// :w in the input card. Task 6 fills in lookup + insert + refresh. For now,
/// just read the word, close, and toast so the wiring is testable.
pub(crate) fn submit(state_rc: &Rc<RefCell<AppState>>) {
    let raw = state_rc.borrow().gloss_overlay.edit_buffer_text();
    let word = crate::vocab_lookup::normalize_vocab_word(&raw);
    close(state_rc);
    let s = state_rc.borrow();
    if word.is_empty() {
        crate::input::navigation::show_chapter_toast_secs(&s, "nothing to add", 2);
    } else {
        crate::input::navigation::show_chapter_toast_secs(
            &s,
            &format!("(stub) would add \u{201c}{word}\u{201d}"),
            2,
        );
    }
}
```

- [ ] **Step 3: Register the sub-module**

In `src/input/actions/mod.rs`, find the `mod` declarations (`rg -n '^pub\(crate\) mod |^mod ' src/input/actions/mod.rs`) and add alphabetically near `segment_vim`:

```rust
pub(crate) mod vocab_add;
```

- [ ] **Step 4: Route AddVocab keys in keymap.rs**

In `src/input/keymap.rs`, right after the `SegmentVim` routing block (~line 127-129), add a parallel block:

```rust
    // AddVocab (Ctrl+Alt+\ input card) owns ALL keys, like SegmentVim.
    if state.borrow().input_mode == crate::app::InputMode::AddVocab {
        return handle_add_vocab_key(state, key_name, key_char, is_ctrl);
    }
```

Then add the key handler function (place it beside `handle_segment_vim_key`, ~line 1300). It differs from segment_vim in ONE way: `Save`/`SaveQuit` submit instead of refusing:

```rust
/// Key handler for the add-vocab input card (InputMode::AddVocab). Same vim
/// engine + gloss_overlay edit buffer as SegmentVim, but :w / :wq SUBMIT the
/// typed word (lookup + insert) instead of being refused. :q/:q!/double-Esc
/// cancel.
fn handle_add_vocab_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    key_char: Option<char>,
    is_ctrl: bool,
) -> bool {
    use crate::input::vim::{EditorAction, VimKey};

    if key_name == "Escape" && !is_ctrl {
        if is_double_esc() {
            crate::input::actions::vocab_add::close(state);
            return true;
        }
        let _ = state.borrow().gloss_overlay.feed_edit_key(VimKey::Esc);
        return true;
    }

    let Some(vk) = gtk_key_to_vim(key_name, key_char, is_ctrl) else {
        return true;
    };

    let action = state.borrow().gloss_overlay.feed_edit_key(vk);
    match action {
        EditorAction::Save | EditorAction::SaveQuit => {
            crate::input::actions::vocab_add::submit(state);
            true
        }
        EditorAction::OpenRewrite => true, // R is inert here
        EditorAction::Cancel | EditorAction::CancelForce => {
            crate::input::actions::vocab_add::close(state);
            true
        }
        EditorAction::CopyToClipboard(text) => {
            copy_to_clipboard(&text);
            true
        }
        _ => true,
    }
}
```

- [ ] **Step 5: Add the `unreachable!` mode-dispatch arm**

At ~line 232 (the `match` over `input_mode` that has `SegmentVim => unreachable!(...)`), add:

```rust
            crate::app::InputMode::AddVocab => unreachable!("AddVocab handled before mode dispatch"),
```

- [ ] **Step 6: Build**

Run: `cargo build`
Expected: compiles clean. Resolve any `EditorAction` variant mismatch by matching the exact arms `handle_segment_vim_key` uses (confirm with `rg -n 'enum EditorAction' -A 15 src/input/vim/`).

- [ ] **Step 7: Commit**

```bash
git add src/app/mod.rs src/input/actions/vocab_add.rs src/input/actions/mod.rs src/input/keymap.rs
git commit -m "$(cat <<'EOF'
feat(vocab): AddVocab input-card mode + key routing (stub submit)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RcpEnoaGznf2FVHaAQtChG
EOF
)"
```

---

### Task 6: Wire submit → lookup ladder → insert → refresh (sync + Claude fallback)

**Files:**
- Modify: `src/input/actions/vocab_add.rs` (replace the stub `submit`)

**Interfaces:**
- Consumes: `crate::vocab_lookup::{normalize_vocab_word, lookup_local}`, `crate::db::queries::{open_db_rw, insert_vocab_word, VocabInsertOutcome}`, `crate::app::apply_after_add`, `crate::input::actions::claude_bridge::run_claude_request`, `state.config.claude_model`.
- Produces: final `submit` behavior; a module-level in-flight pending-word marker.

- [ ] **Step 1: Add the in-flight guard field**

The async fallback needs a pending-word marker so a second submit of the same word does not double-insert. Add a field to `AppState` (find the struct in `src/app/mod.rs`, `rg -n 'pub vocab_highlight_visible' src/app/mod.rs` for a nearby vocab field). Add:

```rust
    /// Word currently awaiting a Claude definition fallback (add-vocab). Guards
    /// against a duplicate paid request / double insert on repeat submit.
    pub vocab_add_pending: Option<String>,
```

Initialize it to `None` in the AppState constructor (same place `vocab_highlight_visible: false` is set, ~line 2074).

- [ ] **Step 2: Replace `submit` with the full ladder**

In `src/input/actions/vocab_add.rs`, replace the stub `submit` with:

```rust
/// :w in the input card: normalize, look up locally, insert + refresh. On a
/// local miss, fall back to the Claude API (async) with an in-flight guard.
pub(crate) fn submit(state_rc: &Rc<RefCell<AppState>>) {
    let raw = state_rc.borrow().gloss_overlay.edit_buffer_text();
    let word = crate::vocab_lookup::normalize_vocab_word(&raw);
    close(state_rc);

    if word.is_empty() {
        let s = state_rc.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "nothing to add", 2);
        return;
    }

    // Duplicate in-flight guard.
    if state_rc.borrow().vocab_add_pending.as_deref() == Some(word.as_str()) {
        return;
    }

    // Local ladder first (synchronous).
    if let Some((definition, source)) = crate::vocab_lookup::lookup_local(&word) {
        insert_and_refresh(state_rc, &word, &definition, &source);
        return;
    }

    // Local miss → Claude fallback (async).
    let model = state_rc.borrow().config.claude_model.clone();
    state_rc.borrow_mut().vocab_add_pending = Some(word.clone());
    {
        let s = state_rc.borrow();
        crate::input::navigation::show_chapter_toast_secs(
            &s,
            &format!("looking up \u{201c}{word}\u{201d}\u{2026}"),
            2,
        );
    }
    crate::logging::log(&format!("VOCAB ADD: local miss, asking Claude for '{word}'"));

    let system = "You are a concise dictionary. Given a single English word, \
                  reply with ONE clear dictionary-style definition of it — a \
                  single sentence, no headword, no part-of-speech tag, no \
                  numbering, no quotation marks."
        .to_string();
    let user = word.clone();
    let word_ok = word.clone();
    let word_err = word;
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        system,
        user,
        model,
        move |st, answer| {
            // Clear the guard regardless of the UI state.
            if st.borrow().vocab_add_pending.as_deref() == Some(word_ok.as_str()) {
                st.borrow_mut().vocab_add_pending = None;
            }
            let definition = answer.trim().to_string();
            insert_and_refresh(st, &word_ok, &definition, "claude");
        },
        move |st, msg| {
            if st.borrow().vocab_add_pending.as_deref() == Some(word_err.as_str()) {
                st.borrow_mut().vocab_add_pending = None;
            }
            let s = st.borrow();
            crate::input::navigation::show_chapter_toast_secs(
                &s,
                &format!("no definition for \u{201c}{word_err}\u{201d}: {msg}"),
                3,
            );
            crate::logging::log(&format!("VOCAB ADD: claude failed for '{word_err}': {msg}"));
        },
    );
}

/// Insert the word and run the shared view refresh. Used by both the sync
/// local path and the async Claude success callback.
fn insert_and_refresh(state_rc: &Rc<RefCell<AppState>>, word: &str, definition: &str, source: &str) {
    let outcome = match crate::db::queries::open_db_rw() {
        Ok(conn) => crate::db::queries::insert_vocab_word(&conn, word, definition, source),
        Err(e) => Err(e),
    };
    let mut s = state_rc.borrow_mut();
    match outcome {
        Ok(o) => {
            let added = matches!(o, crate::db::queries::VocabInsertOutcome::Added);
            crate::app::apply_after_add(&mut s, word, added, source);
        }
        Err(e) => {
            crate::logging::log(&format!("VOCAB ADD: db write failed for '{word}': {e}"));
            crate::input::navigation::show_chapter_toast_secs(
                &s,
                &format!("couldn't save \u{201c}{word}\u{201d}"),
                3,
            );
        }
    }
}
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles clean. Confirm the `run_claude_request` closure signatures match (Task reference: `on_success: Fn(&Rc<RefCell<AppState>>, String)`, `on_error: Fn(&Rc<RefCell<AppState>>, &str)` — see `src/input/actions/claude_bridge.rs:15`).

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/vocab_add.rs src/app/mod.rs
git commit -m "$(cat <<'EOF'
feat(vocab): add-vocab submit ladder — local lookup, Claude fallback, refresh

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RcpEnoaGznf2FVHaAQtChG
EOF
)"
```

---

### Task 7: Register the `Action::AddVocabWord` keybind

**Files:**
- Modify: `src/input/actions/mod.rs` (enum, `category()`, `name()`)
- Modify: `src/input/keymap_config.rs` (`vocab_bindings()` ~line 300)
- Modify: `src/input/keymap.rs` (dispatch arm ~line 3877)

**Interfaces:**
- Consumes: `crate::input::actions::vocab_add::open` (Task 5).
- Produces: the `AddVocabWord` action bound to `Ctrl+Alt+\`.

- [ ] **Step 1: Add the Action variant**

In `src/input/actions/mod.rs`, add to `enum Action` near the other vocab actions (`rg -n 'ToggleVocabHighlight,' src/input/actions/mod.rs`):

```rust
    /// Ctrl+Alt+\ on the main card: open the add-vocab-word input card.
    AddVocabWord,
```

- [ ] **Step 2: Add to `category()` and `name()`**

In the `category()` match, add `AddVocabWord` to the `Category::Vocab` arm (the block ending `=> Category::Vocab,` ~line 336):

```rust
            | Action::ToggleVocabHighlight
            | Action::AddVocabWord
```

In the `name()` match (~line 468), add:

```rust
            Action::AddVocabWord => "AddVocabWord",
```

- [ ] **Step 3: Add the default bind**

In `src/input/keymap_config.rs`, `vocab_bindings()`, add after the `ToggleVocabHighlight` line (~line 316):

```rust
        (KeyCombo::ctrl_alt("backslash"), Action::AddVocabWord),
```

- [ ] **Step 4: Add the dispatch arm**

In `src/input/keymap.rs`, `dispatch_action`, near `OpenSegmentVim` (~line 3877):

```rust
        AddVocabWord => crate::input::actions::vocab_add::open(state),
```

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: compiles clean, no non-exhaustive-match error (all three match sites updated).

- [ ] **Step 6: Run the full unit suite**

Run: `cargo test --bins`
Expected: PASS (vocab_lookup + vocab_insert tests green; nothing else regressed).

- [ ] **Step 7: Commit**

```bash
git add src/input/actions/mod.rs src/input/keymap_config.rs src/input/keymap.rs
git commit -m "$(cat <<'EOF'
feat(vocab): bind Ctrl+Alt+\ to AddVocabWord

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RcpEnoaGznf2FVHaAQtChG
EOF
)"
```

---

### Task 8: Headless e2e verification + doc/to-do updates

**Files:**
- Modify: `docs/to-do/to-do.md` (mark item `[X]` if present)
- Verify: headless drive; no source change expected unless a bug surfaces.

**Interfaces:**
- Consumes: the whole feature.
- Produces: verification evidence + a checked-off to-do.

- [ ] **Step 1: Build for e2e**

Run: `cargo build`
Expected: clean.

- [ ] **Step 2: Headless drive — open card, type a word, submit**

Launch under cage (see linux-lit CLAUDE.md Headless Verification), resize to production geometry, wait ~3s for map, then drive. Confirm current key names first (`rg -n 'backslash|Escape' src/input/keymap_config.rs`). Note `\` on RPD emits key_name `backslash`; the chord is `Ctrl+Alt`:

```bash
cd ~/utono/linux-lit
LIT_NO_MPV=1 LIT_DEV=1 GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
sleep 3
export WAYLAND_DISPLAY=$(basename $(ls -t /run/user/1000/wayland-* | grep -v '\.lock' | head -1))
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
sleep 1
# Open the add-vocab card:
wtype -M ctrl -M alt -k backslash -m alt -m ctrl
sleep 1
# Type a word known to be in the open work (Insert mode is pre-seeded):
wtype "brave"
# Save/submit — the vim save verb. Confirm the editor's save chord in
# src/input/vim/ (typically Escape then :w<Enter>):
wtype -k Escape
wtype ":w"
wtype -k Return
sleep 2
grim /tmp/vocab-add.png
```

Read `/tmp/vocab-add.png` and `rg -n 'VOCAB ADD' *.log` (find the fresh log by mtime). Expected: a toast `added "brave" (wordnet)` (or `(claude)`), and — with the popup open on a line containing "brave" — its definition visible.

- [ ] **Step 3: Verify highlighting + the H/rr regression**

Still headless: confirm the added word is highlighted (vocab tag). Confirm `H` no longer toggles the popup and `rr` still does:

```bash
wtype "H"      # should NOT open the popup
sleep 1; grim /tmp/vocab-H.png
wtype "rr"     # should toggle the popup
sleep 1; grim /tmp/vocab-rr.png
```

Read both PNGs; verify `H` did nothing and `rr` toggled. Cleanup:

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

- [ ] **Step 4: Mark the to-do item**

Check `docs/to-do/to-do.md` for an add-vocab-word request (`rg -n -i 'vocab.*add|add.*vocab' docs/to-do/to-do.md`). If present, prefix it with `[X]` (never delete).

- [ ] **Step 5: Commit any doc change**

```bash
git add docs/to-do/to-do.md
git commit -m "$(cat <<'EOF'
docs(to-do): mark add-vocab-word done

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RcpEnoaGznf2FVHaAQtChG
EOF
)"
```

- [ ] **Step 6: Hand off for real-renderer eyeball**

Per project rules, cage is software rendering. Give the user the exact command to run the app themselves (`crll`) and the steps: press `Ctrl+Alt+\`, type a word present on screen, `Esc :w Enter`; confirm the toast, the highlight color, and the popup definition on the real GL renderer.

---

## Notes for the implementer

- **Vim save/cancel verbs:** the exact `EditorAction` variants and the `:w`/`:q` key sequence come from `src/input/vim/`. Before Task 5, read `rg -n 'enum EditorAction' -A 15 src/input/vim/mod.rs` (or the relevant file) and `rg -n 'Save|SaveQuit|Cancel|CancelForce|OpenRewrite|CopyToClipboard' src/input/vim/` so the `match` arms in `handle_add_vocab_key` are exhaustive and correct. Mirror `handle_segment_vim_key` (`src/input/keymap.rs:1300`) — it is the closest working template.
- **`gloss_overlay` API:** `show_gloss_with_color`, `enter_edit_buffer`, `edit_buffer_text`, `exit_edit_buffer`, `set_edit_copy_only`, `feed_edit_key` are all on `src/ui/gloss_overlay.rs`. Confirm `show_gloss_with_color`'s exact parameters (`rg -n 'fn show_gloss_with_color' -A 3 src/ui/gloss_overlay.rs`) — the title/body/size/color/spans order used in Task 5 follows `segment_vim::open`.
- **`overlay_card_size`:** `crate::app::layout::overlay_card_size(&s)` — confirm the return tuple order `(cw, h)` (`rg -n 'fn overlay_card_size' -A 3 src/app/layout.rs`).
- **Do NOT** touch the snapshot cache — vocab highlighting is not snapshotted.
- **`~/.config/linux-lit/keymap.json`** edits are outside this repo; if it is a stow symlink into `tty-dotfiles`, one edit suffices — otherwise edit both and restow. Commit those in the `tty-dotfiles` repo, not here.
