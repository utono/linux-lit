# Cross-band Ctrl+n/p journal traversal — design

_2026-06-28 (US Central)_

## Problem

In the journal Q&A overlay, `Ctrl+n` / `Ctrl+p` flip pages **only within the
current band** (`nav_page` clamps inside `s.journal.pages`, the loaded band's
pages). So from the Work band you only cycle whole-work Q&As; at the last page of
a chapter, `Ctrl+n` is a dead key. The user wants `Ctrl+n` at the last page of a
band to roll into the **first Q&A of the next chapter/scene** and continue from
there (and `Ctrl+p` symmetrically) — a flat traversal across every Q&A in the
work.

## Design

`Ctrl+n/p` becomes a flat traversal across ALL the work's Q&As, in the SAME order
the `Ctrl+\` picker and `find_all_pages_ordered` already use:

> whole-work (`scope='work'`) pages first, then by `div1 ASC, div2 ASC`, and
> within a band by `timestamp ASC, id ASC`.

Passage Q&As interleave inside their `(div1,div2)` scene band (consistent with
how `find_scene_band_pages` renders scene+passage together). No wrap — clamps at
the work's first / last Q&A (matching today's no-wrap feel).

### `nav_page(delta)` (rewrite, `src/input/actions/journal.rs`)

1. Read the current page's `id` and the work abbrev. (No current page → no-op.)
2. Load the flat list: `find_all_pages_ordered(conn, work_abbrev)`.
3. `pos` = index of the current `id` in the flat list. (Not found → no-op.)
4. `next = (pos + delta).clamp(0, len-1)`. If `next == pos` → no-op (already at
   the work's first/last Q&A).
5. The target page = `flat[next]`. Resolve its band with the existing
   `band_for_page` (Work if `div1 < 0`, else Scene(div1,div2) — passages fold
   into their scene band). Land on it via the shared `land_on_page` helper.

### `land_on_page(s, band, target_id)` (new shared helper)

Extracted from the body `confirm_picker` already uses:

```rust
fn land_on_page(s: &mut AppState, band: JournalBand, target_id: i64) {
    s.journal_band = band;
    s.journal.page_index = 0;
    render_current(s);                       // loads the band's pages
    if let Some(pos) = s.journal.pages.iter().position(|p| p.id == target_id) {
        s.journal.page_index = pos;
        render_current(s);
    }
}
```

`confirm_picker` is refactored to call it (DRY; no behavior change).

## Why this ordering

It is the one the user already sees in the `Ctrl+\` picker and the only ordering
already materialized in the DB layer (`find_all_pages_ordered`). Reusing it means
`Ctrl+n/p` walks the picker list in order — predictable and consistent.

## Out of scope

- Wrapping at the ends (kept clamped).
- Changing `Alt+n/p` (`nav_scene`, jumps between scenes-with-pages) or `Alt+w`
  (`nav_to_work_band`). Those remain band-level jumps.

## Testing

- Unit-test the pure step+clamp index math (a small `flat_step(pos, delta, len)`
  helper) — first/last clamp, middle step. The band-landing is GTK/DB and is
  covered by the existing `confirm_picker` path it reuses.
- Build + `cargo test --bins` green.
- Visual (user): in a work with Q&As in several bands, `Ctrl+n` from the last
  whole-work page lands on the first chapter's first Q&A and continues across
  chapters; `Ctrl+p` reverses; it stops at the very first/last Q&A.

## Files

- `src/input/actions/journal.rs` — rewrite `nav_page`; add `land_on_page` +
  `flat_step`; refactor `confirm_picker` to use `land_on_page`.
- `src/ui/keybinds_overlay.rs` — update the "journal tog" describe arm: "Ctrl+n /
  Ctrl+p flip pages within the band" → "step through every Q&A in the work across
  bands".
