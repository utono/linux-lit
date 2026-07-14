# Per-work vocab highlighting

## Problem

Vocab-word coloring in the reading card is currently a single **global**
setting: `config.vocab_highlight_visible` (a bool in
`~/.config/linux-lit/config{,-dev}.json`), toggled with **Alt+\\**
(`ToggleVocabHighlight`). The user wants the choice to be **per-work** instead,
driven by a column in `lit.db`, so e.g. Shakespeare and Dickens never color
vocab while a study text can.

The `works` table already carries an (unused-by-the-app) column
`vocab_highlight INTEGER DEFAULT 1`. The app does not read it today.

## Decisions

- **Per-work column is the single source of truth.** The global config flag is
  retired.
- **Alt+\\ flips the current work's column and persists it to lit.db**
  (read-write), so the choice sticks per-work across sessions.
- **New / unset works default OFF** (matches the user's recent "off by default"
  intent). This default governs only genuinely-new works, never existing values.
- **Existing per-work values are preserved.** The user's DB already has a
  deliberate split (see below); the app migration must NOT backfill or reset it.

### Existing data is intentional — do not clobber

The user's `~/utono/litdb/data/lit.db` has 199 works with a real, curated split,
NOT a uniform schema default:

- 42 already OFF: the plain Shakespeare plays/poems (`1H4`, `Ham`, `Son`,
  `Ven`, ...).
- 157 ON: bible books (71), `-Amb`/`-BBC` Shakespeare play variants (51),
  prose (20), epics, etc.

A blanket `UPDATE works SET vocab_highlight = 0` would destroy this. The app
migration therefore performs **no backfill**.

## Architecture

Four code changes plus one one-time data edit. The code migration and the data
edit are **separate** and must not be conflated.

### 1. DB: column + read path (`src/db/queries.rs`, `src/db/models.rs`)

- `Work` (`models.rs`) gains `pub vocab_highlight: bool`.
- `load_work` reads the column:
  `SELECT vocab_highlight FROM works WHERE abbrev = ?1`, with
  `.unwrap_or(0)` → `false` when the column or value is absent — mirroring the
  existing `text_file` graceful fallback at `queries.rs:99-104`. Map `1` → true,
  anything else (0, NULL) → false.
- New `set_vocab_highlight(conn, abbrev, on: bool)` on a read-write connection
  (`open_db_rw`), `UPDATE works SET vocab_highlight = ?2 WHERE abbrev = ?1`.
- New idempotent migration `ensure_vocab_highlight_column(conn)` mirroring
  `ensure_claude_model_columns`:
  - If the column is absent (only on a fresh/other DB — never the user's), 
    `ALTER TABLE works ADD COLUMN vocab_highlight INTEGER DEFAULT 0` so new rows
    default OFF.
  - **No backfill, no UPDATE.** On a DB that already has the column (the user's),
    this is a complete no-op and every current value is left exactly as-is.
  - Registered in the startup `ensure_*` block in `src/app/mod.rs` (~line 2383)
    alongside `ensure_bookmarks_table` etc.

### 2. Runtime state wiring (`src/app/mod.rs`)

- `display_work` sets `AppState.vocab_highlight_visible` **from the loaded
  work's column** instead of from config. Today the value is taken from
  `config.vocab_highlight_visible` (~line 1368); change it to read
  `work.vocab_highlight`.
- The existing render gate (~line 2767,
  `if state.vocab_highlight_visible { apply_vocab_highlighting(state) }`) is
  unchanged — it just gets its value from the work now. Switching works re-reads
  the per-work value automatically because `display_work` runs per switch.

### 3. Alt+\\ persists per-work (`src/input/keymap.rs`)

The `ToggleVocabHighlight` arm (~lines 2209-2219):
- Flip `s.vocab_highlight_visible` (unchanged).
- `apply_vocab_highlighting` / `remove_vocab_highlighting` (unchanged).
- **Replace** the `s.config.vocab_highlight_visible = ...` write with
  `set_vocab_highlight(&open_db_rw()?, current_work_abbrev, s.vocab_highlight_visible)`.
  Use the work's base abbrev if abbrev mangling matters (check
  `base_work_abbrev`); the column is keyed by the row's `abbrev`, so write the
  exact abbrev `load_work` was called with.
- Keep the log line.

### 4. Retire the global config flag (`src/config.rs`)

- Remove the `vocab_highlight_visible` struct field, its
  `#[serde(default = ...)]`, `default_vocab_highlight_visible()`, and the
  `Default` initializer.
- Stale keys in `config-dev.json` / `config.json` become inert (serde ignores
  unknown keys on read and simply stops writing them). Not hand-edited.

## One-time data edit (user's lit.db only — NOT the app migration)

Apply once to `~/utono/litdb/data/lit.db` to set the user's desired end-state.
Author values are exact (verified: `Shakespeare`, `Charles Dickens` — no false
positives, no other-author works *about* them):

```sql
UPDATE works SET vocab_highlight = 0 WHERE author = 'Shakespeare';
UPDATE works SET vocab_highlight = 0 WHERE author = 'Charles Dickens';
```

Effect: 43 Shakespeare (the `-Amb`/`-BBC` variants + `BenCrystalOP` anthology +
`LC`) and 5 Dickens (`ACC`, `BH`, `BR`, `PP`, `TTC`) flip on→off; 48 rows total.
The other 109 ON works and the 42 already-OFF works are untouched. This is a
deliberate user data choice, run by hand (or a litdb tool), never by the app's
idempotent migration.

## Testing

Pure-logic (`cargo test --bins`):

- `ensure_vocab_highlight_column` idempotency: run twice on an in-memory DB; a
  fresh add defaults new rows to 0; a second run is a no-op and does not alter
  existing values (set a row to 1 first, ensure it stays 1). Mirrors the
  `characters_table_*` migration tests.
- `load_work` returns the stored bool, and falls back to `false` when the column
  is absent.
- `set_vocab_highlight` round-trip (write true/false, read back).

GUI (needs the user's e2e run — `./scripts/e2e-env.sh ...`):

- A work whose column is on colors vocab; a work whose column is off does not.
- Alt+\\ toggles coloring and the choice survives a work switch and relaunch
  (because it's now persisted in lit.db, not config).

## Out of scope (YAGNI)

- No new UI indicator of the current state beyond the existing coloring.
- No vocab column in the library picker.
- No keybinds-overlay redraw, but ONE required text fix: the Alt+\\
  `describe()` arm (`keybinds_overlay.rs:463-465`, key `"vocab hi"`) currently
  says "state saved to config" — change it to reflect per-work lit.db
  persistence (e.g. "state saved per-work in lit.db"). This is a wording change
  only; the keycap/detail-row structure is unchanged.
