# Co-Author Attribution Display

Port the `lit_authorship` Neovim plugin feature to linux-lit: collaborator-attributed lines render in italics, on by default, togglable via keybind.

## Data Source

Two existing tables in `lit.db`:

- **`attribution_sets`** — one row per scholarly attribution hypothesis per work. Columns: `id`, `work_abbrev`, `name`, `display_name`, `primary_author`, `secondary_author`, `description`, `source_citation`, `created_at`. Unique on `(work_abbrev, name)`.
- **`line_authorship`** — one row per collaborator-attributed line. Columns: `id`, `attribution_set_id` (FK), `citation` (e.g. `H8.0.0.1`), `author`, `confidence`, `notes`. Unique on `(attribution_set_id, citation)`.

Current data covers 4 works (H8, TNK, Tim, Per) with 3,609 total lines across collaborators Fletcher, Middleton, and Wilkins.

## Behavior

- When a work loads, query `attribution_sets` for its `work_abbrev`. If results exist, auto-select the first set and apply italic styling to all secondary-author lines.
- `Ctrl+a` toggles authorship display on/off. Shows a toast indicating state ("Authorship: on/off").
- `Ctrl+Shift+A` opens a picker listing all attribution sets for the current work. The user selects one; the display updates. If only one set exists, show a toast ("Only one attribution set available") instead of the picker.
- Works without attribution data: keybinds show a "No authorship data" toast. No other effect.

## Architecture

### New AppState fields

```rust
authorship_line_ids: HashSet<i64>,       // line_mapping IDs for the active set's secondary-author lines
authorship_enabled: bool,                // toggle state, default true
authorship_sets: Vec<AttributionSet>,    // all sets for the current work (empty if none)
active_attribution_set_id: Option<i64>,  // currently selected set ID
```

### New data structs

```rust
pub struct AttributionSet {
    pub id: i64,
    pub work_abbrev: String,
    pub name: String,
    pub display_name: String,
    pub primary_author: String,
    pub secondary_author: String,
}
```

### New DB queries (in `src/db/queries.rs` or a new `src/db/authorship.rs`)

1. `load_attribution_sets(conn, work_abbrev) -> Vec<AttributionSet>` — query `attribution_sets` for the given work.
2. `load_secondary_line_ids(conn, set_id, work_abbrev) -> HashSet<i64>` — join `line_authorship` on `attribution_sets`, match `citation` to `line_mapping` rows by constructing the citation key (`work_abbrev.div1.div2.line_in_div`), return the `line_mapping.id` values for secondary-author lines.

### Rendering

The existing rendering pipeline uses GTK `TextTag`s, not Pango markup. Add a new tag:

- **`authorship-italic`** — `pango::Style::Italic`, created at startup alongside the other structural tags in `build_window`. Same pattern as `stage-direction-style`.

After `apply_dialogue_formatting(state)` in `display_work`, call a new `apply_authorship_formatting(state)` function that:

1. Skips if `authorship_line_ids` is empty or `authorship_enabled` is false.
2. Iterates buffer lines, maps each to a `line_mapping` ID (via `line_map` or direct index).
3. For each line whose ID is in `authorship_line_ids`, applies the `authorship-italic` tag to that line's range.

Toggling (`Ctrl+a`) flips `authorship_enabled`, removes or re-applies the tag across the buffer, and shows a toast.

### Tag interaction with stage directions

Stage directions already use `stage-direction-style` (italic). Collaborator lines that are also stage directions will have both tags applied — both are italic, so no visual conflict. No special handling needed.

### Keybinds

- `Ctrl+a` — `Action::ToggleAuthorship`
- `Ctrl+Shift+A` — `Action::PickAttributionSet`

Add to both `keymap_config.rs` (compiled defaults) and `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`.

### New Action variants

```rust
Action::ToggleAuthorship
Action::PickAttributionSet
```

### Attribution set picker

Reuse the existing picker pattern (like media picker or library picker). A simple `vim.ui.select`-style list showing `display_name` for each set. On selection, update `active_attribution_set_id`, re-query `load_secondary_line_ids`, update `authorship_line_ids`, and re-apply formatting.

### Work-load flow

In `display_work_at_with_prepared`, after `apply_dialogue_formatting(state)`:

1. Query `load_attribution_sets(conn, &work.abbrev)` → store in `state.authorship_sets`.
2. If non-empty, set `state.active_attribution_set_id = Some(sets[0].id)`.
3. Query `load_secondary_line_ids(conn, set_id, &work.abbrev)` → store in `state.authorship_line_ids`.
4. Call `apply_authorship_formatting(state)`.

### File changes

- `src/db/authorship.rs` (new) — `AttributionSet` struct, `load_attribution_sets`, `load_secondary_line_ids`
- `src/app.rs` — new AppState fields, `authorship-italic` tag creation, `apply_authorship_formatting` function, call in `display_work`
- `src/input/actions/mod.rs` — `ToggleAuthorship`, `PickAttributionSet` variants
- `src/input/keymap.rs` — dispatch for the two new actions
- `src/input/keymap_config.rs` — compiled-in default bindings
- `src/ui/authorship_picker.rs` (new) — attribution set picker widget
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — add the two bindings

## Out of scope

- Per-line confidence display or notes
- Adding new works or attribution sets (data entry is done in litdb)
- Color-coding by specific collaborator (all secondary authors get the same italic style)
- Persisting the user's chosen attribution set across sessions
