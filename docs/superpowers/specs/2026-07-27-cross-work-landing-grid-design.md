# Cross-work + centering landings read the pinned grid — design

_2026-07-27. Status: approved, implementing._

Follow-on to `2026-07-27-prose-canonical-page-top-design.md`. That fix made
`jump_to_line` and the saved-position resume table-aware. The close-to-reader
audit (recorded in `page-turning-mechanics.md`) then found three MORE landings
in the same class. This fixes all three.

## The three defects

Same root class throughout: **a geometric computation standing in for the
authoritative `prose_pages` table.** All are no-ops on plays.

### 1. Cross-work jump landing — `display_work_at_with_prepared`

`src/app/mod.rs`, the `if let Some(target_id) = target_line_id` branch:

```rust
state.current_line = buf_idx;
state.page_top_line = buf_idx;   // target line forced to the top; no offset
```

Shared root of every cross-work jump landing: concordance `r`/`R` across works,
echo source jumps, `toggle_previous_work` / `load_work_at` with a target line.
Live on all 13 prose works with pinned tables.

**Cannot be fixed in place.** The page tables are not loaded until ~40 lines
LATER (`page_table::load_for_work` / `prose_pages::load_for_prose_work`), so at
the target branch there is no table to consult.

### 2. Centering landing — `update_highlight_and_center`

`src/input/highlight.rs`:

```rust
let new_top = state.current_line.saturating_sub(lpp / 2);
set_page_instant(state, new_top);
```

A pure geometric guess. Reached by the concordance same-work landing
(`concordance_position_cursor`), `pickers::jump_to_line_mapping_id`, and the
echo jump in `keymap.rs` — plus `nav_test.rs`, `phrase_highlight.rs`,
`keymap.rs:3653`, and `escape.rs:81` (AB-loop clear), which inherit the fix.

### 3. Translations hide — `hide_translations` (was latent)

`src/app/translations.rs` remaps `current_line` / `page_top_line` through
`map_line_before_insert` and NEVER assigns `page_top_offset`, then anchors the
single-column path with a raw pixel `adj.set_value(...)`.

Currently unreachable — all 43 works with `line_translations` rows are plays and
none has a `prose_pages` table — but fixed now rather than left as a trap.

## Design

### The shared rule

Every landing resolves its page through
`navigation::canonical_page_top_offset_for` (prose table → play table → live
walk) and applies BOTH halves via `scroll::set_page_instant_offset`. Non-prose
paths get `(top, 0)`, byte-identical to today.

### Fix 1 — snap AFTER the table load

Insert between `prose_pages::load_for_prose_work(state)` and
`update_highlight_and_show(state)` — the only point where both the target line
and the table are known:

```rust
if target_line_id.is_some() {
    let (top, off) =
        crate::input::navigation::canonical_page_top_offset_for(state, state.current_line);
    state.page_top_line = top;
    state.page_top_offset = off;
}
```

Gated on `target_line_id.is_some()` so ordinary work loads and the
saved-position resume (already fixed) are untouched.

**Companion change — `update_highlight_and_show` must honour the offset.** It
currently scrolls to `line_yrange(scroll_to).0` with no offset term, which would
discard a non-zero `page_top_offset`. Add the offset to that computation. It is
`+ 0` for every pre-existing caller, so nothing else changes.

**Cold-start caveat, deliberately accepted.** `prose_layout_fingerprint` encodes
the live widget height, so on a fresh start the load can MISS and the table is
absent here. The helper then falls through to the live walk and this is a no-op;
`resnap_prose_to_table` remains the safety net for that case. Not a regression —
it is today's behaviour.

### Fix 2 — table first, centering as fallback

```rust
pub fn update_highlight_and_center(state: &mut AppState) {
    update_highlight(state);
    if let Some((top, off)) =
        crate::input::prose_pages::prose_table_boundary_for_line(state, state.current_line)
    {
        crate::input::scroll::set_page_instant_offset(state, top, off);
    } else {
        let lpp = lines_per_page(state);
        set_page_instant(state, state.current_line.saturating_sub(lpp / 2));
    }
    auto_show_vocab_popup(state);
}
```

**Deliberate, user-visible behaviour change.** On a pinned prose work the cursor
is no longer vertically centred after a concordance/echo/vocab jump — it lands
wherever the stored page places it. This is the correct e-reader behaviour and
matches what page-turning already does; approved explicitly. Plays and
live-engine prose still centre exactly as before.

Uses `prose_table_boundary_for_line` (not the full helper) so the play path
keeps its centring rather than being pulled onto the play grid — plays are not
part of this defect and must not change.

### Fix 3 — `hide_translations` resnaps

After the line-number remap, re-anchor onto the grid instead of leaving a
half-restored position:

- assign `page_top_offset` alongside the remapped `page_top_line`
- call `prose_pages::resnap_prose_to_table(state)` on the single-column path

The two-column branch is deliberately untouched: it defers its entire re-snap to
RESIZE_TICK for documented reasons (the left view still has its single-column
width; scrolling there pollutes the settled resnap).

## Testing

TDD per CLAUDE.md — pagination subsystem.

- **Unit** — branch selection in `update_highlight_and_center`: with a table the
  landing is the stored `(start_line, start_off)`; without one it is the
  centring guess.
- **Headless A/B** — concordance jump on BH-Barrett against the pre-fix commit.
  Baseline must land `BOTTOM_CLIP_ROWFILL`, fixed must land
  `BOTTOM_CLIP_EXACT` with the stored offset. **The drive MUST cross a page
  boundary** — a same-page jump returns early via `is_line_fully_visible` and
  proves nothing (this is exactly the false pass that nearly shipped last time).
- Build, clippy, full suite, nav-fuzz.

## Risk

Higher than the previous fix. `display_work_at_with_prepared` runs on every work
load; `update_highlight_and_center` has eight callers. Mitigated: both changes
are inert without an active prose table (every play unaffected), and Fix 1 is
gated on `target_line_id.is_some()`.

## Known latent, still NOT fixed

`is_line_fully_visible` (`viewport.rs`) remains prose-table-blind on the
single-column path — it reads the geometric `last_visible_range` cache. It gates
`jump_to_line`'s early return, so a jump to a line that is visually on-screen but
belongs to a DIFFERENT stored page still skips the snap. Carried forward from the
previous spec; not observed in the wild.
