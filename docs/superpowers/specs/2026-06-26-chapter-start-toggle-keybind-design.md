# Chapter-start toggle keybind for prose works

**Date:** 2026-06-26
**Status:** Design approved — implementation plan at
`docs/superpowers/plans/2026-06-26-chapter-start-toggle-keybind.md`
**Repo:** ~/utono/linux-lit (Rust/GTK4 reader). Depends on litdb tooling already
shipped (the `line_mapping.chapter_start` column + `chapter_divisions.py`).

## Problem

Prose works imported from audiobook transcripts (e.g. `Cromwell`) land as a
SINGLE `(div1, div2)` division — the whole book is one section. To get
per-chapter structure (navigation, synopses, bounded journal context), each
chapter must become its own `div1 > 0` division.

litdb already provides the mechanism: a `line_mapping.chapter_start` column marks
the paragraph that BEGINS a chapter, and `chapter_divisions.py derive` rewrites
`(div1, div2)` from those marks (chapter N → `div1=N, div2=0`; pre-first-mark
paragraphs → front matter `div1=0`; `line_in_div` stays global;
`journal_entries` re-mapped). `auto-detect` seeds the ~15 headings the narrator
reads aloud, but **~7 of Cromwell's ~23 chapter boundaries are not detectable
from text** and must be marked by a human reading the book.

There is no way to set those marks from the reader. The reader is exactly where a
human knows "this paragraph begins a chapter" — so the toggle belongs there.

## Goal

A keybind that, on the paragraph at the cursor, **toggles
`line_mapping.chapter_start`**, then **re-derives** the work's chapter divisions
and **reloads** the work in place (cursor preserved) so the new/removed chapter
boundary is immediately visible. Prose-only.

## Design

### One press = toggle → derive → reload

1. **Gate:** only when `is_prose_work(&work.work_type)` (`src/db/line_types.rs`).
   No-op (with a debug log) for plays/poems.
2. **Resolve the cursor's paragraph:** `current_line` (buffer) →
   `line_mapping_id_for_buffer` and the `Line` (`id`, `line_in_div`,
   `chapter_start`) via the in-memory `current_work.lines`. Capture
   `abbrev`, the cursor's `line_in_div`, and the cursor's `line_mapping.id`
   (to restore the cursor after reload) inside a short `state.borrow()`.
3. **Write + derive (off the UI thread, `spawn_blocking`):**
   - Open `lit.db` read-write (`open_db_rw`), toggle the column:
     `UPDATE line_mapping SET chapter_start = 1 - COALESCE(chapter_start, 0)
      WHERE id = ?1`, returning the new value.
   - Re-derive divisions by **shelling out** to the litdb tool (the single source
     of truth for derivation + `journal_entries` re-map):
     `python3 ~/utono/litdb/scripts/chapter_divisions.py derive --work <abbrev>`.
     Run it inside the same `spawn_blocking` (after the toggle write commits and
     its connection is dropped, so the Python process sees the new mark; WAL mode
     makes the concurrent open safe).
4. **Reload on the UI thread (copy `pickers::load_selected_work`):**
   `load_work` (fresh `div1/div2`) → `snapshot::read` (the `div1/div2`
   fingerprint auto-invalidates the stale `.text.bin`, forcing
   `prepare_text_for_display`, which rebuilds `section_starts`) →
   `display_work_at_with_prepared(..., target_line_id = Some(cursor line id))`
   to keep the reader on the same paragraph.
5. **Feedback:** a `notify-send` or status line "chapter start set / cleared"
   (optional; a debug log line is required).

### Why shell out (not reimplement derive in Rust)

`chapter_divisions.py` owns the derivation rule AND the `journal_entries` re-map
in one transaction. Reimplementing in Rust would duplicate that logic and risk
drift. The app already shells out extensively (`std::process::Command` in
`main.rs`, `mpv/discovery.rs`, `visual.rs`, `font.rs`). Cost: linux-lit assumes
the litdb checkout at `~/utono/litdb` with a `python3` that can import the
script's deps (only stdlib `sqlite3`/`argparse` — no venv needed for `derive`).

### Keybind choice

`c` plain is already `SetChapter` (the per-media AUDIO chapter mark, distinct
concept). Use a distinct combo for the structural toggle — proposed
`Ctrl+c` — declared as a compiled-in default in `keymap_config.rs`
(`nav_bindings` or `timestamp_bindings`); users can override via
`~/.config/linux-lit/keymap.json`. The implementing agent picks the final combo
if `Ctrl+c` collides; the plan lists the check.

### Toggle direction

The press reads the cursor paragraph's CURRENT `chapter_start` (already in
`current_work.lines[idx]`) and writes the opposite. Marking front-matter or the
very first paragraph is allowed (the derive rule handles a chapter at line 1 —
no front matter then). Unmarking the only mark collapses the work back to a
single division (front matter `div1=0` everywhere), which is correct.

## Out of scope

- The litdb derivation tool, the `chapter_start` column, and the `/synopses`
  prose path — already shipped.
- A bulk "auto-detect from the reader" command — the reader toggle is the
  hand-marking tool; `auto-detect` is run once from litdb.
- Verifying the first-ever multi-`div1` prose work renders correctly
  (pagination / `2`/`3` scene-jump / synopsis card) — a manual/headless check
  AFTER Cromwell is fully marked, noted in the plan's final task, not gated by
  this code.

## Testing

- **Unit (`cargo test --bins`):** the cursor→`line_in_div`/`id` resolution and
  the "compute opposite of current chapter_start" decision are the testable
  pure-ish parts. Extract any index/decision math into a small pure helper and
  test it (the GTK reload path is not unit-testable).
- **Build/clippy:** `cargo build`, `cargo test --bins`, `cargo clippy` stay
  green; clippy warning count must not increase.
- **Headless (agent self-check, per CLAUDE.md Headless Verification):** launch
  Cromwell in `cage`, screenshot, press the keybind on a known chapter-opening
  paragraph, screenshot again — confirm a section boundary appears and the
  cursor stays put. The user does the final real-session check.

## Files

- `src/input/actions/mod.rs` — `Action::ToggleChapterStart` (+ `category`,
  `name`).
- `src/input/keymap_config.rs` — default KeyCombo → action.
- `src/input/keymap.rs` — dispatch arm in `dispatch_action`.
- `src/input/actions/chapters.rs` (new) or extend `bookmarks.rs` — the action
  fn (gate, resolve, spawn_blocking write+derive, reload).
- `src/db/queries.rs` — `toggle_chapter_start(conn, line_mapping_id) ->
  Result<bool>` (column toggle, mirrors `upsert_chapter`).
