# Add Vocab Word Design

**Date:** 2026-07-19
**Status:** Draft

## Overview

Add a main-card keybind that lets the reader add a vocabulary word on the fly.
Pressing the bind opens an empty modal input card; the reader types a word (a
lemma) and submits. The app looks up a dictionary definition — local `wn`/`dict`
first, the Claude API as a fallback — inserts the word into the global
`vocab_words` table, and then makes the word live in the current view: vocab
highlighting is turned on so the new word lights up immediately, and if the
vocab popup is open with the new word on the cursor line, the popup refreshes to
show its definition.

The same change also simplifies the popup show/hide binds: the redundant `H`
(Shift+h) toggle is removed, leaving the existing `rr` chord as the sole
show/hide path.

## Keybindings

### Removed

- `H` (Shift+h) — `Action::ToggleVocabPopup`. Redundant: the `rr` chord
  (`ChordState::PendingR`, `keymap.rs:305-318`) already shows the popup when
  hidden and hides it when visible. The compiled comment at
  `keymap_config.rs:304` already anticipates this ("HideVocabPopup is unbound —
  rr covers it"). `Action::ToggleVocabPopup` and its handler stay defined but
  unbound-by-default (like `HideVocabPopup` already is), so a user keymap can
  still bind it.

Removal touches four sites:

1. `src/input/keymap_config.rs:352` — delete
   `(KeyCombo::plain("H"), Action::ToggleVocabPopup)`.
2. `~/.config/linux-lit/keymap.json:47` **and** its stow source at
   `~/tty-dotfiles/linux-lit/…/keymap.json` — delete
   `{"key":"H","action":"ToggleVocabPopup"}`, then
   `cd ~/tty-dotfiles && stow linux-lit`. Without this the JSON silently
   re-binds `H` over the removed default.
3. `src/ui/keybinds_overlay.rs:74,326` — drop the `H: auto vocab` keycap entry
   and its `describe()` arm.

### Added

- `Ctrl+Alt+\` — `Action::AddVocabWord`. `Ctrl+Alt+backslash` is free (only
  `plain`, `alt`, and `ctrl` backslash are bound today) and groups on the same
  physical cap as `Alt+\` (`ToggleVocabHighlight`).

## Flow

1. `Ctrl+Alt+\` → `Action::AddVocabWord` → `vocab_add::open` opens an **empty**
   single-line vim-input card, suspending reader keys.
2. The reader types a lemma and submits (the vim editors' save verb).
3. `vocab_add::submit` reads and clears the card text, then `normalize()`s it
   (trim, lowercase, strip trailing `'s`/`’s`).
4. Guard: empty word → toast "nothing to add", close card. Word already pending
   a Claude lookup → ignore.
5. `lookup_local(word)` runs the local ladder:
   - `wn <word> -over` → parse the first `-- (definition…)` gloss.
   - On empty, `dict -d gcide <word>` → parse the first sense.
   - Returns `Some((definition, "wordnet"|"gcide"))` or `None`.
6. If `Some`: `insert_vocab_word` (synchronous) → `apply_after_add` → toast
   `added "word" (source)`. Card closes immediately.
7. If `None`: set the pending marker, toast `looking up "word"…`, close card,
   and fire `claude_bridge::run_claude_request` with a terse define prompt. Its
   success callback clears pending, inserts with `source="claude"`, and runs
   `apply_after_add`; its error callback clears pending and toasts the error —
   nothing is inserted on error.

Local hits refresh inline (sync); a Claude fallback inserts and refreshes later
in the async success callback, guarded by an in-flight pending-word marker so a
second submit of the same word does not double-insert.

## Components

New code concentrates in one new orchestrator module plus a small pure lookup
module; the rest is touch-ups to existing registration and refresh sites.

### 1. Definition lookup — `src/vocab_lookup.rs` (new, pure, sync)

```
fn lookup_local(word: &str) -> Option<(String /*definition*/, String /*source*/)>
```

Shells `wn <word> -over` and parses the first `-- (definition…)` gloss (mirrors
litdb's `fetch_wordnet` regex in `~/utono/litdb/scripts/vocab/definitions.py`);
on empty, shells `dict -d gcide <word>` and parses the first sense. The parsing
functions are split from the `Command` invocation so they can be unit-tested on
canned CLI stdout without `wn`/`dict` installed. A missing binary is a spawn
error treated as "no result" — fall through, never panic. Knows nothing about
`AppState`, the popup, or highlighting.

### 2. Persistence — `src/db/queries.rs`

```
fn insert_vocab_word(conn, word, definition, source) -> rusqlite::Result<InsertOutcome>
```

Uses:

```sql
INSERT INTO vocab_words(word, definition, source) VALUES(?,?,?)
ON CONFLICT(word) DO UPDATE SET definition=excluded.definition, source=excluded.source
  WHERE vocab_words.definition='' OR vocab_words.definition IS NULL
```

Idempotent on the `UNIQUE` `word` column: re-adding an existing word with a good
definition leaves it intact, but fills an empty one. The change count
distinguishes `Added` from `AlreadyPresent` for the toast wording. Opened via
`open_db_rw()`. Sits beside the existing `load_vocab_words`/`set_vocab_highlight`.

### 3. Input card — reuse the modal vim-input pattern

`Action::AddVocabWord` opens an empty single-line vim editor card (the same
engine the ask/journal prompts use), suspending reader keys. A dedicated
input-mode marker on `AppState` (e.g. `InputMode::AddVocab`) routes the editor's
save verb to `vocab_add::submit` and Escape/`:q` to a cancel that restores
reader keys. The card widget is generic; only the submit target is
feature-specific.

### 4. Orchestrator — `src/input/actions/vocab_add.rs` (new)

- `pub(crate) fn open(state_rc)` — opens the empty input card.
- `pub(crate) fn submit(state_rc)` — reads+clears card text, normalizes, guards
  empty/duplicate-in-flight, runs the lookup ladder, and dispatches to
  `apply_after_add`. On local miss, calls `run_claude_request` and wires its
  callbacks.
- Holds the sync/async fork and the in-flight pending-word marker.

This is the only unit that knows the whole sequence; everything it calls is a
smaller, testable piece.

### 5. View refresh — `vocab_add::apply_after_add(state, &word)`

Called identically from the sync path and the Claude success callback, so both
routes converge on one refresh:

1. Enable + persist `vocab_highlight` for the work (`set_vocab_highlight`, seed
   `vocab_highlight_visible = true`).
2. `remove_vocab_highlighting` → reload `state.vocab_words`
   (`load_vocab_words`) → `build_vocab_matches` → `apply_vocab_highlighting`.
3. If `state.vocab_popup.popup.is_visible()` **and** `state.vocab_matches` has
   the word on `state.current_line`, call `refresh_vocab_popup(state)`
   (`src/app/vocab_popup.rs:218`), which re-derives the cursor-line word list.
4. Toast the outcome.

Must run inside a single `borrow_mut` window without re-borrowing (the popup
refresh takes `&mut AppState`), matching the callback discipline in
`vocab_journal.rs`.

No snapshot invalidation is needed — vocab highlighting is not snapshotted, only
the tag apply/remove.

## Registration

Standard main-card action wiring:

1. `src/input/actions/mod.rs` — add `AddVocabWord` to `enum Action`; add to the
   `category()` match (`Category::Vocab`) and the `name()` match
   (`"AddVocabWord"`, used for logging and keymap.json parsing).
2. `src/input/keymap_config.rs` — add
   `(KeyCombo::ctrl_alt("backslash"), Action::AddVocabWord)` to
   `vocab_bindings()`.
3. `src/input/keymap.rs` — add a `dispatch_action` arm:
   `AddVocabWord => crate::input::actions::vocab_add::open(state)`.
4. `~/.config/linux-lit/keymap.json` — no entry required unless the reader wants
   to rebind.

## Error Handling

- `wn`/`dict` binary missing or non-zero exit → treated as "no result", fall
  through to the next source, then Claude. Logged via `log_fmt!`, never panics.
- DB write failure → logged, toast `couldn't save "word"`, no view change.
- Claude with no API key / network error → error callback toasts; word is not
  added (consistent with the no-empty-definition-rows decision).

## Edge Cases

- **Word not in the current work's text:** still inserted and highlighting
  enabled, but no visible highlight/popup change (it is not on screen). The
  toast still confirms the add.
- **Word already present with a good definition:** the `ON CONFLICT … WHERE
  definition=''` guard leaves it intact; `apply_after_add` still runs so
  highlighting turns on. Toast distinguishes `already have "word"` from
  `added "word"` via the change count.
- **Multi-word input** (e.g. "brave new world"): normalize keeps it a single
  string; the local lookup will likely miss and fall to Claude. Accepted as a
  known limitation, not a feature.
- **Cursor moves during a Claude fallback:** the success callback still inserts
  and re-highlights globally; the popup only refreshes if the word is on the
  then-current line. No stale-borrow risk — the pending marker gates the
  callback.

## Testing

- **Unit (`cargo test --bins`):** `normalize()` (trailing `'s`, case,
  whitespace); `lookup_local` parsing fed canned `wn`/`dict` stdout (parse fn
  split from `Command`); `insert_vocab_word` idempotency against an in-memory
  SQLite (`ON CONFLICT` leaves good definitions, fills empty).
- **Headless e2e (cage/grim/wtype, `test-headless-navigation` harness,
  `LIT_NO_MPV=1`):** drive `Ctrl+Alt+\`, type a word present in the open work,
  submit; assert the card closed, the word is highlighted (vocab tag applied),
  and — with the popup open on that line — its definition renders.
- **Keybind regression:** confirm `H` no longer toggles the popup and `rr`
  still does; confirm the Ctrl+/ overlay no longer shows `H: auto vocab`.
- **Manual hand-off:** final eyeball on the real GL renderer for the toast +
  highlight color (cage is software rendering).

## Doc Updates

- `src/ui/keybinds_overlay.rs` — remove the `H: auto vocab` keycap and
  `describe()` arm; no new main-card keycap is required for `Ctrl+Alt+\` unless
  desired (it is a modifier chord, consistent with other Ctrl/Alt binds not on
  the strip — decide during implementation).
- `docs/to-do/to-do.md` — mark the corresponding item `[X]` if one exists.
- No `clip-prevention.md` change (no clipping surface touched).
