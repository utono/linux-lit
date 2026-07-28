# Prose-aware canonical page top — design

_2026-07-27. Status: approved, implementing._

## Problem

Two user-reported bugs, reproduced deterministically twice (logs at 19:05 and
19:11 on 2026-07-27, byte-identical numbers). Both are the same defect:
**a geometric page computation overriding the authoritative pinned
`prose_pages` table.** This is the same class as the four bugs merged earlier
the same day.

### Bug B — Escape from the journal overlay re-frames the page

Drive: `Ctrl+j` (Q&A picker) → `Return` (journal overlay) → `Escape`.

```
[53083ms] RETURN_TO_READER: from JournalOverlay
[53092ms] PAINT: first frame for page_top=47     <- cursor line, not a page start
[53100ms] BOTTOM_CLIP_ROWFILL: page_top=47       <- live row-fill engine
```

versus the pre-Escape state:

```
[ 2326ms] BOTTOM_CLIP_EXACT: page_top=42 top_off=603   <- pinned table mode
```

The reader not only moved, it **fell out of table mode** into the live engine.
On screen the passage is pinned to the top of the card instead of sitting
mid-page where the grid puts it.

The source-jump itself is INTENDED. `open_picker_from_reader`
(`journal.rs:3094`) sets `entry_page_id = None` deliberately ("this open is
itself navigation"), so `toggle_overlay`'s close branch computes
`on_entry_page = false` and takes `jump_to_journal_source_start`. That is
correct behaviour. The bug is purely that the jump LANDS off-grid.

### Bug A — startup lands off-grid, then re-snaps late

```
[  623ms] DISPLAY_WORK: resumed saved position current_line=47 page_top=46
[ 2127ms] PAGES_PROSE: resnap off-grid (46,0) -> (42,603) (cursor 47)
[25756ms] PAINT: first frame for page_top=42 after 23628ms
```

`page_top=46` is not a page boundary. It is NOT stale persisted config —
`display_work` (`app/mod.rs:4321-4332`) RECOMPUTES it every launch as
`current_line.saturating_sub(1)` = 46. A geometric guess that ignores the
table.

The resnap corrects it at 2127ms, but in the second run the frame did not
paint for a further 23.6 seconds (it painted in 0ms in the first run). So the
reader can display the WRONG page for tens of seconds after launch. Fixing the
landing at source removes the resnap from the startup path entirely, and with
it the stall.

## Root cause

`canonical_page_top_for` (`navigation.rs:369`):

```rust
if let Some(table) = crate::input::page_table::active_page_table(state) { ... }
// falls through to a live geometric walk
```

It consults only the PLAY table. On prose `active_page_table` returns `None`,
so it walks the live engine, which disagrees with the pinned prose grid.

It also returns a bare `usize`. A prose boundary is a `(start_line,
start_off)` PAIR — here `(42, 603)`. Returning `42` alone still mis-frames by
603px.

That signature limitation is why two call sites already grew their OWN local
workarounds rather than fixing the helper:

- `search.rs:456` `snap_match_to_prose_grid`
- `navigation.rs:1882` `chapter_jump_land_ereader`

`jump_to_line` is the third site that needed it and never got one.

## Design

### New helper

```rust
pub(crate) fn canonical_page_top_offset_for(state: &AppState, target: usize) -> (usize, i32)
```

Order of authority:

1. prose table — `prose_pages::prose_table_boundary_for_line(state, target)`
2. play table — existing `active_page_table` branch
3. live geometric walk — existing fallback

Non-prose paths return `(top, 0)`, byte-identical to today's behaviour.

`canonical_page_top_for` remains as a thin wrapper returning `.0`, so the
existing callers compile unchanged. Only sites needing the offset migrate.

### Call-site changes

- **`jump_to_line`** (`navigation.rs:3030`) — Bug B. Use the new helper and
  `set_page_instant_offset(state, top, off)` instead of `set_page_instant`.
- **`display_work`** (`app/mod.rs:4321`) — Bug A. In the not-`near_end`
  branch, consult the new helper instead of `current_line - 1`, setting
  `page_top_offset` alongside `page_top_line`.
- **`chapter_jump_land_ereader`** (`navigation.rs:1882`) — delete the
  hand-rolled prose branch; becomes a plain call to the new helper.
  Behaviour-preserving: it already used `prose_table_boundary_for_line` with
  the same first-row rule.

### Explicitly NOT in scope

- **`search.rs` `snap_match_to_prose_grid`** stays. It deliberately anchors to
  the MATCH'S OWN wrapped row, not the line's first row, so a match on a later
  row is not hidden under the bottom clip. That is genuine behaviour, not
  duplication.
- **`navigation.rs:3288`** (vocab jump) stays on the wrapper — guarded by
  `is_line_fully_visible`.
- **`resnap_prose_to_table` is KEPT** as defence-in-depth. It still catches
  positions that predate a table regeneration, which is why it exists.

### Known latent issue (documented, not fixed)

`is_line_fully_visible` (`viewport.rs:1698`) is ALSO prose-table-blind on the
single-column path — it consults `last_visible_range`, a geometric cache. It
gates `jump_to_line`'s early return, so a jump to a line that is visually
on-screen but belongs to a DIFFERENT stored page would still skip the resnap.
Same family, did not fire in this repro. Recorded in the page-turning ledger
rather than widening this change.

## Testing

TDD per CLAUDE.md (pagination subsystem): failing repro first, then fix.

- **Unit** — the helper against a synthetic prose table: a line mid-page
  returns that page's `(start_line, start_off)`, not the line itself. Pure, no
  GTK. This is the regression guard for both bugs.
- **E2e (headless)** — reproduce the exact drive: land on BH-Barrett ch2,
  `Ctrl+j` → `Return` → `Escape`. Assert `BOTTOM_CLIP_EXACT ... page_top=42
  top_off=603` and NOT `BOTTOM_CLIP_ROWFILL page_top=47`.
- **Startup** — no `PAGES_PROSE: resnap off-grid` on a clean launch.
- **Nav-fuzz** — `jump_to_line` is on the bookmark/jump path.

## Risk

`jump_to_line` and `display_work` are hot paths. Mitigating factor: the helper
is strictly additive for plays and live-engine prose (returns `(top, 0)`,
identical to today). Only pinned-prose-table works change behaviour — the
intended blast radius.
