# Library picker: hide edition-leak base works; don't auto-load bundle media

**Date:** 2026-07-03
**Status:** design, approved (proceeding to implement)

## Two problems

1. **A base work should not appear in Ctrl+p when its media really belongs to
   its specific editions.** E.g. `AWW` (All's Well) is associated with the same
   file as `AWW-BBC`; the base is redundant with that edition.
2. **Ctrl+m on a base work whose only media is a multi-play bundle silently
   auto-loads the bundle**, which starts at the wrong play. `Rom`'s only media
   (media_id 80) is a Hamlet+Macbeth+Romeo m4b (also on `Ham`, `Mac`); Ctrl+m
   loads it and drops you into Hamlet's audio — "no media available" in effect.

## Key definition — "multi-work bundle"

A media file that **contains more than one work** = a `media_files` row
associated (via `work_media_associations`) with **more than one distinct BASE
work**, where base = the abbrev before the first `-` (so `Rom` and `Rom-BBC`
are the same base). This is the user's "exception for media files that contain
more than one work."

- media_id 80 → base works {Ham, Mac, Rom} → **bundle**.
- A `-BBC` single-play file on both the base and its `-BBC` edition → same base
  → **not** a bundle (it's an edition-leak).

## Decisions

- **Bundle media KEEPS the base work shown** (user's exception): you can only
  reach a multi-play recording through the base, so don't hide it. → `Rom`,
  `MND` stay.
- **Edition-leak hides the base:** hide a base work when it has editions AND
  every one of its media is (a) NOT a bundle AND (b) shared with one of its own
  editions — i.e. none of its media is bundle media or dedicated-to-base-only.
  Verified hide-set on current lit.db: **`AWW` only** (`Cym` stays — it has a
  base-only dedicated file besides the Cym-BBC leak).
- **Ctrl+m does not silently auto-load bundle media:** the single-media
  auto-select path fires only when that one media is NOT a bundle. If it IS a
  bundle, show the picker (so the user chooses knowingly) instead of dumping
  them into the wrong play.

## Design

### Fix 1 — `list_works` edition-leak filter (`src/db/queries.rs`)

Extend the existing `EXISTS(work_media_associations)` filter (added in
`c04849e`) to ALSO exclude edition-leak base works. Keep works that:

- are NOT a base with editions (editions and prose/single works unaffected), OR
- have at least one media that is a bundle (multi-base), OR
- have at least one media that is dedicated to this base only (not shared with
  any of its own editions).

Concretely, using a `base_of` CTE (media_id → distinct base count) for the
bundle test and a correlated NOT-EXISTS for the edition-leak test. Hide only
when: base has editions, has NO bundle media, and EVERY media is shared with one
of its own editions.

A `test_list_works` assertion is added: `AWW` is excluded, `Rom`/`MND`/`Cym`/
`Ham` remain, and every listed work still has a media association.

### Fix 2 — Ctrl+m: don't auto-load a bundle (`src/input/actions/pickers.rs`
+ a helper in `src/db/queries.rs`)

New `pub fn is_bundle_media(conn, media_id) -> bool` — true when the media is
associated with >1 distinct base work:

```sql
SELECT COUNT(*) > 1 FROM (
  SELECT DISTINCT CASE WHEN instr(work_abbrev,'-')>0
                        THEN substr(work_abbrev,1,instr(work_abbrev,'-')-1)
                        ELSE work_abbrev END AS base
  FROM work_media_associations WHERE media_id = ?1
)
```

In `open_media_picker`, the `if items.len() == 1` auto-select becomes
`if items.len() == 1 && !is_bundle_media(&conn, items[0].media_id)`. When it's a
single bundle, fall through to the normal "show the picker" path (which already
handles the 0-or-many case). The picker then lists the bundle so the user can
select it deliberately (or Escape out). The bundle check runs inside the same
`spawn_blocking` that already opens the DB and lists media, so it costs one more
cheap query, no extra connection.

Log a line (`MEDIA_PICKER: single media is a multi-work bundle — showing picker`)
so the behavior is visible in the debug log.

## Files touched

- `src/db/queries.rs` — extend `list_works`; new `is_bundle_media`; test update.
- `src/input/actions/pickers.rs` — gate the single-media auto-select on
  `!is_bundle_media`.

No new AppState, no keybind/config/overlay changes.

## Out of scope / YAGNI

- Splitting a multi-play bundle into per-play chapter offsets (that's a
  litdb/import concern, not linux-lit).
- Changing which edition is the "default" or auto-priority.
- Any change to the media picker's confirm path or MPV loading.

## Testing

- `cargo build` + `cargo clippy` + `cargo test --bins` (with the `list_works`
  assertion covering the hide-set).
- Headless (cage, `LIT_HEADLESS_TEST=1`): Ctrl+p — `AWW` absent, `Rom`/`MND`
  present. Note: MPV is skipped headlessly, so Ctrl+m's load can't be observed
  on screen, but the picker-vs-auto-load branch is logged; verify via the log
  line that `Rom` (single bundle) takes the show-picker branch and a work with a
  single dedicated file still auto-loads. A live-app eyeball confirms Ctrl+m on
  Rom now shows the picker.
