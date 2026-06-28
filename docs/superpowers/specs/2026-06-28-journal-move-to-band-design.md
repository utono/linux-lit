# Move a journal Q&A to a different band — design

_2026-06-28 (US Central)._

## Goal

Add a keybind in the journal overlay that moves the **current** Q&A entry to a
different "band": any scene/chapter in the work, or the whole-work band. The
entry is re-targeted in place in `lit.db`; the overlay then **follows** the entry
to its new band so the move is immediately visible.

## Background: how a Q&A is associated with a band

A Q&A entry's band lives in `journal_entries` (see `src/db/journal.rs`):

- `scope='scene'` — keyed by `(work_abbrev, div1, div2)`. `(div1,div2)` is
  `(act, scene)` for plays and `(chapter, 0)` for prose/chapter works.
- `scope='work'` — whole work. Stored at `(div1,div2) = (-1,-1)` (the
  `JOURNAL_WORK_DIV` constant in `src/app/mod.rs`).
- `scope='passage'` — citation-anchored: keyed by `(start_citation,
  end_citation)`, created only from a visual selection. Carries `source_text`.

The in-memory band is the `JournalBand` enum (`src/app/mod.rs`):
`Work`, `Scene(d1, d2)`, `Passage { div1, div2, start, end }`. The overlay's
current band is `AppState.journal_band`; the loaded entries for that band are
`AppState.journal.pages`, indexed by `journal.page_index`.

Note: there is **no separate `'chapter'` scope**. A chapter is just `scope='scene'`
with `(div1,div2) = (chapter, 0)`; `synopsis_label` renders it as "Chapter N".

Today there is **no DB path that changes an entry's band**: `update_journal_page`
updates only `question`/`answer`/`claude_model` by `id`. This feature adds one.

## Decisions (from brainstorming)

- **Target list = every scene in the work** (not only scenes that already have
  Q&As), plus a whole-work row. Enumerated from `work.lines`, unfiltered.
- **UX = reused picker overlay**, modeled on the existing `Ctrl+\` Q&A picker
  (`JournalQaPicker`): a filterable, scrollable selectable list; `Ctrl+n`/`Ctrl+p`
  (and arrows) move, Enter confirms, Escape cancels back to the journal overlay.
- **After the move, follow the entry to its destination band** and land on it.
- **Key: `Ctrl+Shift+J`** opens the move picker from the journal overlay.
- **Passage entries are not movable** (citation-anchored), and the target list
  never offers a passage target.

## Components

### 1. DB layer — `src/db/journal.rs`

New function (existing `update_journal_page` untouched):

```rust
pub fn move_journal_page(
    conn: &Connection,
    id: i64,
    scope: &str,
    div1: i64,
    div2: i64,
) -> Result<(), rusqlite::Error> {
    // UPDATE journal_entries SET scope = ?, div1 = ?, div2 = ? WHERE id = ?
}
```

Unit test: insert a `'scene'` entry, `move_journal_page` it to `'work'` at
`(-1,-1)`; assert it disappears from `find_journal_pages` and appears in
`find_work_pages`; then move it back and assert the reverse.

### 2. New picker — `src/ui/journal_move_picker.rs`

A near-clone of `JournalQaPicker`, built from the shared `picker_nav` card/list
helpers and `picker_filter` subsequence matching:

```rust
#[derive(Clone)]
pub struct MoveTargetRow {
    pub band: JournalBand,   // Work or Scene(d1, d2)
    pub label: String,       // "whole work" / "Act 3, Scene 2" / "Chapter 5"
}

pub struct JournalMovePicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    pub items: Vec<MoveTargetRow>,
}
```

Same surface as `JournalQaPicker`: `new`, `attach`, `set_items`, `show`, `hide`,
`is_visible`, `search_entry`, `populate_list`, `move_selection`,
`selected_index`. Rows render a single label (the target band's label).

Registered in `src/input/picker_dispatch.rs`: `impl_picker!(JournalMovePicker)`
and a `picker_for_mode` arm for `InputMode::JournalMovePicker`.

### 3. Target-list builder — `src/input/actions/journal.rs`

```rust
fn move_target_rows(s: &AppState, current: &JournalBand) -> Vec<MoveTargetRow>
```

- Row 0: whole work → `JournalBand::Work`, label `"whole work"`.
- Then one row per **unique** `(div1, div2)` in `work.lines`, in reading order
  (reuse the `ordered_synopsis_scenes` collection pattern — iterate lines,
  insert into a `HashSet`, keep first-seen order — but **without** the
  synopsis-cache filter, so every scene is listed). Label via
  `crate::app::scene_synopsis::synopsis_label(s, d1, d2)`.
- **Omit the entry's `current` band** so only different targets are offered.

### 4. Action handlers — `src/input/actions/journal.rs`

```rust
pub(crate) fn open_move_picker(state: &Rc<RefCell<AppState>>)
pub(crate) fn confirm_move_picker(state: &Rc<RefCell<AppState>>)
```

`open_move_picker`:
- If `journal.pages` is empty → toast `"No page to move"`, stay in the overlay.
- If `journal_band` is `Passage { .. }` → toast `"Can't move a passage page"`,
  stay. (Passages are citation-anchored.)
- Build rows via `move_target_rows(&s, &s.journal_band)`, `set_items`, `show`,
  set `input_mode = InputMode::JournalMovePicker`.

`confirm_move_picker`:
- Read the selected `MoveTargetRow.band`; read the current entry's `id` from
  `journal.pages[journal.page_index]`.
- Map band → `(scope, div1, div2)`: `Work → ("work", -1, -1)`;
  `Scene(d1,d2) → ("scene", d1, d2)`.
- `move_journal_page(conn, id, scope, d1, d2)` on a read-write connection.
- Hide picker; set `journal_band` = destination band; `render_current` (loads
  the destination band's pages); set `page_index` to the moved entry's position
  (matched by `id`, mirroring `confirm_picker`); `render_current` again; set
  `input_mode = InputMode::JournalOverlay`.
- Toast `"Moved to {label}"`.

### 5. Keymap wiring — `src/input/keymap.rs`

- In `handle_journal_key`'s `is_ctrl` block, add `"J" => open_move_picker(...)`.
  Ctrl+Shift+J arrives as `key_name = "J"` (uppercase, shifted) with
  `is_ctrl = true` — distinct from the existing `Ctrl+"j"` (close overlay), so
  there is no collision and `is_shift` need not be threaded into the handler.
- Add `InputMode::JournalMovePicker` to the `InputMode` enum.
- Add it to the `handle_picker_key` dispatch group (alongside `JournalPicker`),
  to that handler's `Hide` arm (hide picker → return to `JournalOverlay`,
  mirroring `JournalPicker`), and its `Confirm` arm (→ `confirm_move_picker`).

### 6. Keybinds overlay — `src/ui/keybinds_overlay.rs`

Per the project rule, reflect the new bind in the `Ctrl+/` overlay (keycap +
`describe()` arm), following the precedent of the existing journal-overlay keys.
Run the `update-cairo-keybinds-overlay` skill for the mandatory cross-reference
pass.

## Testing

- **Unit (`cargo test --bins`):**
  - `move_journal_page` round-trip at the DB layer (scene → work → scene).
  - `move_target_rows`: whole-work row first; omits the current band; labels
    correct for a play (scene labels) and a chapter work (chapter labels).
- **Runtime (user-verified):** the picker shows the right scene list, Enter moves
  the entry, and the overlay follows it to the destination band. This is a
  visual/runtime criterion, so per the project's no-`cargo run`-by-the-agent
  rule, the agent builds clean and asks the user to verify on a rendered run.

## Out of scope (YAGNI)

- Moving passage entries to/from other bands.
- A distinct `'chapter'` scope (chapter is `'scene'` with `div2=0`).
- Changing any existing `find_*` query.
