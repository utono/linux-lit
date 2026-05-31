# Reorder echo ranking + add echo via line picker

**Date:** 2026-05-31
**Status:** Approved design, pending implementation plan

## Problem

In the echoes overlay the user wants two new capabilities:

1. **Up/Down arrows reorder the echo ranking** — move the selected echo earlier/later, persisted, marking it curated (reorder + auto-curate).
2. **`A` (capital) adds an echo** — open a fuzzy line-search picker over all Shakespeare lines; the chosen line becomes a new curated echo at the **absolute top** of the rankings.

These build on the existing `echo_links` model (`rank`, `curated`).

## Current model (verified in source)

- `StoredEchoLink { link_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank }` (`src/db/queries.rs:1051`).
- `echo_links` table: `curated INTEGER DEFAULT 0`, `rank INTEGER NOT NULL` (`:1089`).
- `load_echo_links(conn, turn_id)` orders `curated DESC, rank ASC` (`:1130`).
- `insert_echo_links` writes rows with explicit rank (curated defaults 0). `toggle_echo_curated(conn, link_id)` flips curated (`:1173`). `delete_noncurated_echo_links` (`:1186`).
- **No per-link rank UPDATE and no single-link insert exist** — both are needed.
- Echoes overlay keys: `handle_echoes_overlay_key` (`src/input/keymap.rs`). `a`=play echo, `n`/`p`=select+play, `s`=toggle curated, `R`=refresh, `Tab`=play turn, `Return`=open work, `Ctrl+Up`/`Ctrl+Down`=volume, `g`/`G`=first/last. Plain `Up`/`Down` and `A` are currently unbound.
- `toggle_curated` (`echoes.rs:743`) is the model for "mutate a link, reopen DB, reload via load_echo_links, keep selection on the same link_id, render_echoes + scroll_echo_into_view".
- Fuzzy picker pattern: `concordance_word_picker` (`src/ui/concordance_word_picker.rs`) — `Entry` + result list, `show()`/`filter_changed()`/`move_selection()`; dispatched via `handle_picker_key` on a dedicated `InputMode`.
- `line_mapping` columns: `id, canonical_text, normalized_text, speaker, div1, div2, line_in_div, work_abbrev`.
- `load_work_titles(conn) -> HashMap<abbrev,title>` exists (used by the echo list for "Title act.scene").

## Part 1 — Up/Down reorder + auto-curate

### Behavior

`Up`/`Down` move the selected echo one position earlier/later within the **curated group** and mark it curated. Curated items always sort above non-curated (the existing `curated DESC` is preserved). If the selected echo is not yet curated, the first `Up`/`Down` curates it and places it at the boundary of the curated group, then moves.

### DB

Add to `src/db/queries.rs`:

```rust
/// Set a link's rank and curated flag.
pub fn set_echo_link_rank(conn: &Connection, link_id: i64, rank: i64, curated: bool) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE echo_links SET rank = ?2, curated = ?3 WHERE id = ?1",
        rusqlite::params![link_id, rank, curated as i64],
    )?;
    Ok(())
}
```

### Reorder routine (`echoes.rs`)

`pub(crate) fn reorder_selected_echo(state_rc, delta: i32)` (delta -1 = up, +1 = down):
1. Read current `echo_overlay_links` and `echo_overlay_index`; resolve `turn_id`. Return if no turn.
2. Build the **curated working order**: the curated subset in current display order, plus the selected link forced into it (curate-on-move). Concretely:
   - Take the loaded links (already `curated DESC, rank ASC`). The curated ones form a contiguous prefix.
   - If the selected link is curated: compute its index within the curated prefix, swap with neighbor at `idx+delta` (clamp to `[0, curated_len-1]`; no-op if out of range).
   - If the selected link is NOT curated: it becomes curated and is appended to the end of the curated prefix, THEN the swap applies (so `Up` from the top non-curated pulls it up into the curated tail).
3. Assign sequential ranks `0..n` to the resulting curated order and persist each via `set_echo_link_rank(link_id, rank, curated=true)`. Non-curated links keep their existing rank/curated.
4. Reload via `load_echo_links`, keep selection on the moved `link_id`, `render_echoes`, `scroll_echo_into_view`.

### Keys

In `handle_echoes_overlay_key`, add (NOT under `is_ctrl` — Ctrl+Up/Down stay volume):

```rust
        "Up" => { crate::input::actions::echoes::reorder_selected_echo(state, -1); true }
        "Down" => { crate::input::actions::echoes::reorder_selected_echo(state, 1); true }
```

## Part 2 — `A` adds an echo via a line-search picker

### New DB query

```rust
/// Search every line whose canonical text contains `query` (case-insensitive),
/// across all works. Returns (work_abbrev, div1, div2, line_in_div, text), capped.
pub fn search_lines(conn: &Connection, query: &str, limit: i64)
    -> Result<Vec<(String, i64, i64, i64, String)>, rusqlite::Error>
{
    let pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT work_abbrev, div1, div2, line_in_div, canonical_text \
         FROM line_mapping \
         WHERE canonical_text LIKE ?1 COLLATE NOCASE \
         ORDER BY work_abbrev, div1, div2, line_in_div \
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![pattern, limit], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    rows.collect()
}
```

Scope: ALL works (including the source work). Empty query → return nothing (don't dump 100k rows).

### New picker widget

`src/ui/echo_line_picker.rs` — `EchoLinePicker`, modeled on `concordance_word_picker`:
- An `Entry` (placeholder "Search Shakespeare lines…") + a scrolling `ListBox`.
- Holds the current result rows `Vec<(String,i64,i64,i64,String)>` and the work-titles map.
- Row label format: `"{text} — {Title} {div1}.{div2}"` (title via `load_work_titles`, fallback abbrev).
- Methods: `show()`, `hide()`, `set_results(rows, titles)`, `move_selection(delta)`, `selected_index()`, `search_entry()` (for the live-filter text), `selected_row()` accessor returning the chosen tuple.

### New input mode + wiring

- Add `InputMode::EchoLinePicker`.
- `A` in `handle_echoes_overlay_key`: open the picker (store the current `turn_id` for the add), `input_mode = EchoLinePicker`.
- `handle_echo_line_picker_key` (or extend `handle_picker_key`): on each keystroke, re-run `search_lines(entry_text, 200)` and `set_results`; `Ctrl+n`/`Ctrl+p` (or Up/Down) move selection; `Return` confirms the selection; `Escape` cancels back to the echoes overlay.

### Add flow (`echoes.rs`)

On `Return` in the picker, with the selected `(work, div1, div2, line_in_div, text)`:
1. Resolve `turn_id` (stored when the picker opened). If none, cancel.
2. If an existing link in the turn matches `(work, div1, div2, line_in_div)`: mark it curated and move it to the absolute top (rank 0 among curated, shifting others down) via the Part-1 rank machinery.
3. Else insert a new curated link at the absolute top:
   - New DB helper `add_curated_echo_link(conn, turn_id, work, div1, div2, line_in_div, text) -> link_id`: shift all existing curated ranks +1 (`UPDATE echo_links SET rank = rank + 1 WHERE turn_id = ?1 AND curated = 1`), then INSERT the new row with `curated=1, rank=0, similarity=0`.
4. Reload via `load_echo_links`, set `input_mode = EchoesOverlay`, select the new top link (index 0), `render_echoes`, `scroll_echo_into_view(0)`.

"Absolute top" = rank 0 in the curated group → first row of the list.

### Hint update

Add `A add` to the echoes footer hint in `show_echoes`.

## Out of scope

- FTS/fuzzy ranking (substring `LIKE` only, per decision).
- Editing/removing an added echo beyond the existing `s` (toggle curated) — un-curating a manual echo leaves it as a non-curated link (acceptable).
- Reordering across the curated/non-curated boundary beyond the auto-curate-on-move behavior described.

## Testing

- Unit (`src/db/queries.rs` in-memory): `search_lines` returns matching rows and respects the limit; `set_echo_link_rank` updates rank+curated; `add_curated_echo_link` shifts existing curated ranks and inserts at rank 0 (build a temp `echo_turns`/`echo_links` schema like the `line_start_time` test).
- Manual (`cargo run`): in the echoes overlay, `Up`/`Down` move the selected echo and mark it curated (★), order persists across close/reopen; `A` opens the line picker, typing filters Shakespeare lines, selecting one adds it as the first (curated) echo; adding an already-present line promotes it to top without duplicating.
- `cargo build` + `cargo clippy` clean; tests show only the 2 known pre-existing `block_atom_tests` failures.

## Picker wiring (resolved)

The `EchoLinePicker` joins the existing shared `handle_picker_key`
(`src/input/keymap.rs:216`), which already serves the Bookmark/Media/Concordance
(+Word/List/Works)/Authorship/Gloss pickers:

- **Nav keys: `Ctrl+n` / `Ctrl+p`** (the convention `handle_picker_key` provides
  via `resolve_picker_key` → `PickerAction::MoveDown`/`MoveUp` → `move_selection(±1)`).
- Add `InputMode::EchoLinePicker` to the `|`-joined dispatch list (the match arm
  at `keymap.rs:61-68`) and add an `EchoLinePicker` branch to each `mode` match
  inside `handle_picker_key` (move down/up, confirm, hide) — do NOT write a
  separate handler.
- **Live search:** wire `search_lines` re-query on the picker `Entry`'s
  `connect_changed` (mirroring `concordance_word_picker`'s `filter_changed`), NOT
  in the key handler. Typed characters reach the entry normally; the key handler
  only claims Ctrl+n/p, Return, Escape.
- `Return` (`PickerAction::Confirm`) runs the add flow; `Escape`
  (`PickerAction::Hide`) returns to the echoes overlay (`InputMode::EchoesOverlay`,
  not Reader).
