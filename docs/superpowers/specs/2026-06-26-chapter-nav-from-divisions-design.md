# Source the reader's chapter nav from divisions, not the audio flag

**Date:** 2026-06-26
**Status:** Design approved — future pass (no implementation plan yet).
**Repo:** ~/utono/linux-lit (Rust/GTK4 reader).
Completes the work-vs-media separation begun in
`docs/superpowers/specs/2026-06-26-chapter-start-toggle-keybind-design.md` and
the litdb `is_chapter`→`is_track_mark` rename
(`~/utono/litdb/docs/specs/2026-06-26-rename-is-chapter-to-track-mark-design.md`).

## Problem

The reader's chapter machinery — the `(`/`)` chapter-jump nav, the gutter chapter
sign, the synopsis card's "which chapter am I in" — all read `Line.is_chapter`,
which is **sourced from the active media's `line_timestamps.is_track_mark`** flag:

- `src/db/queries.rs:197,223` (in `load_work`): `line.is_chapter =
  chapter_map.contains_key(&line.id)`, where `chapter_map` is built only from
  timestamps where `ts.media_id == mid && ts.is_track_mark`.
- `src/input/actions/pickers.rs:542,549` (reload path): same, from a
  `chapter_set` of `is_track_mark` timestamps.

So "what is a chapter" in the reader depends on which recording is loaded — and
a work with **no media has no chapters** in the reader, even when its text has
clear chapter structure.

This contradicts the now-established model: **a chapter is a property of the
WORK** (`line_mapping.chapter_start` → `(div1, div2)` divisions, media-independent),
and **a track mark is export-only metadata** — a rare per-media marker that an
ffmpeg step reads from lit.db to embed chapter markers *into* a media file. Track
marks are NOT a reader-navigation concept.

## Goal

Repoint the reader's chapter nav, gutter sign, and chapter-number logic to read
chapter boundaries from `(div1, div2)` divisions for ALL work types.
`is_track_mark` stops driving anything in the reader. The reader then shows and
navigates chapters with no media loaded.

## Design

### What "a chapter boundary" means (all work types)

`Line.is_chapter` becomes "this line begins a new top-level division" — the first
line of a new `div1`. Concretely:

- **Prose** (`is_prose_work`): `div1` is the chapter (chapter N = `(N, 0)`, set by
  `chapter_divisions.py` from `chapter_start`). `is_chapter` = true on the first
  line of each `div1 > 0`. Front matter (`div1 = 0`) is not a chapter.
- **Plays / poems / epics**: `div1` is the act (or top-level part). `is_chapter` =
  true on the first line of each new `div1`. Chapter-jump then moves act-to-act —
  the natural "big section" jump. (Scene-level `(div1,div2)` jumps already exist
  via the `2`/`3` keys; this nav is the coarser div1 jump.)

This is a single, media-independent rule: **`is_chapter` marks the first line of
each `div1` boundary.** The existing `build_section_starts`
(`src/text_file_map.rs:678`) already detects `(div1,div2)` changes; the div1-only
boundary is a trivially related computation (a `div1` change is a subset).

### Where the source changes

`Line.is_chapter` keeps its name and type (`bool`) — only its SOURCE changes.
Two assignment sites, both currently fed by the audio flag, switch to the
div1-boundary rule:

1. **`load_work`** (`queries.rs:~197-223`): delete the `is_track_mark`-based
   `chapter_map`. Compute `is_chapter` from the ordered `lines`: a line is a
   chapter start if its `div1 > 0` (prose) or `div1` differs (non-prose) from the
   previous work-line's `div1`. (The `lines` are already `ORDER BY div1, div2,
   line_in_div, sub_line`, so a single pass sets the flag.)
2. **`pickers.rs` reload** (`~542-549`): same — drop the `chapter_set` from
   `is_track_mark`; set `line.is_chapter` from the div1 boundary in the same pass
   that re-attaches timestamps.

Extract the boundary rule into one pure helper so both sites and tests share it:

```rust
/// Set `is_chapter = true` on the first line of each div1 boundary.
/// Prose: each div1 > 0 (front matter div1=0 is not a chapter).
/// Non-prose: each change of div1 from the previous line.
/// `lines` MUST be in canonical (div1, div2, line_in_div, sub_line) order.
fn mark_chapter_starts(lines: &mut [Line], is_prose: bool)
```

### What stops reading the audio flag

After this, NO reader code reads `line_timestamps.is_track_mark` to decide a
chapter. The `is_track_mark` column remains (export metadata) but is read only by
a future ffmpeg embed step (out of scope, likely litdb). `load_work` still SELECTs
`lt.is_track_mark` into `Timestamp.is_track_mark`? — No: once nothing consumes it,
the SELECT column and the `Timestamp.is_chapter` field that holds it become dead.
Remove them too (the `Timestamp` struct field + the SELECT term at
`queries.rs:146`) unless a remaining consumer is found — verify with `rg` during
implementation. (If the ffmpeg step ends up living in linux-lit after all, keep
the field; the planner decides based on consumers at that time.)

### Consumers that work unchanged

These read `Line.is_chapter` and need NO change — they automatically reflect
divisions once the source flips:

- `src/input/navigation.rs:1092,1095,1145,1155,1693` — chapter-jump nav.
- `src/app/scene_synopsis.rs:22,47,54` — `is_chapter_work`,
  `current_chapter_number`, `chapter_number_from_flags`.
- `src/gutter.rs:55`, `src/app/mod.rs` `is_chapter_line` map — the gutter sign.
- `src/text_file_map.rs:216,569` — chapter-start buffer indices.

### The `c` key stays the track-mark setter

`Action::SetChapter` / the `c` key still SETS `is_track_mark` (the export marker)
via `timestamps::set_chapter`. It no longer affects nav/sign (those follow
divisions). **Relabel for clarity** so its purpose is unambiguous:

- `set_chapter` → keep behavior; the function may be renamed `set_track_mark`
  (optional, planner's call) since it writes `is_track_mark`.
- Keybinds overlay (`src/ui/keybinds_overlay.rs:59,526,691`): "set chapter" →
  "set track mark"; its long help should say "set an audio track mark on this
  line (export metadata for ffmpeg chapter embedding), distinct from the
  structural chapter that `Ctrl+c` creates."
- The `Ctrl+c` `ToggleChapterStart` (structural chapter) is unchanged and is now
  the ONLY thing that affects chapter nav/sign.

### Gutter-sign label cleanup (deferred Minor, fold in here)

The gutter sign-type names still say "chapter" for what is now the track-mark/
audio concept (`src/input/timestamps.rs` sign columns; litdb
`.claude/commands/litdb/timestamps-signs.md`: `lit_signs_chapter`, `chapter`,
`chapter_a/b/loop`). Since the gutter chapter sign now reflects DIVISIONS, decide
per sign:

- The **chapter sign** that marks a division boundary line: keep "chapter" (it is
  now correctly a structural chapter).
- Any sign that specifically reflected the **audio track mark** (A/B-loop status
  on a track-marked line): rename to a `track_mark`-based name in lockstep with
  the reader, and update `timestamps-signs.md`.
- The planner audits the sign table against the post-repoint behavior and renames
  only the signs whose meaning changed. (Note: `media_manager.py`'s stdout label
  was already renamed to `track_marks` in the litdb rename pass — no action.)

## Out of scope

- **The ffmpeg track-mark embed pipeline** (read `is_track_mark` from lit.db →
  write chapter markers into a media file). It is the sole remaining consumer of
  `is_track_mark`; it most likely lives in litdb as an export script. Spec'd
  separately when built.
- **Re-deriving / changing `(div1,div2)` data.** This is a reader SOURCING change
  only; division data is owned by litdb (`chapter_divisions.py`).
- **The `2`/`3` scene-jump keys** — they already read `(div1,div2)` and are
  unchanged.

## Testing

- **Unit (`cargo test --bins`):** `mark_chapter_starts` is the testable core.
  Cases: prose with `div1` 0,1,1,2,2 → chapter starts at the first 1 and first 2,
  not front matter; a play with `div1` 1,1,2,2 → starts at first 1 and first 2;
  empty/single-division input; non-prose vs prose front-matter difference.
- **Integration (data-gated, lit.db present):** load Cromwell (divided prose,
  no media needed) and assert `Line.is_chapter` is true at each chapter's first
  line and the count equals the number of `div1 > 0` divisions — crucially with
  NO media loaded, proving the media-independence. Load a play and assert chapter
  starts fall on act (div1) boundaries.
- **Build/clippy:** `cargo build`, `cargo test --bins`, `cargo clippy` green;
  clippy warning count not above baseline (currently 119).
- **Headless visual (per CLAUDE.md):** open Cromwell with no audio, confirm the
  gutter chapter signs render at chapter boundaries and `(`/`)` jumps between
  them; open a play, confirm `(`/`)` jumps act-to-act. The user does the final
  live-session check.

## Files (for the eventual plan)

- `src/db/queries.rs` — `load_work`: replace the `is_track_mark` chapter_map with
  `mark_chapter_starts`; possibly drop the now-dead `is_track_mark` SELECT term +
  `Timestamp` field.
- `src/text_file_map.rs` or a new small module — `mark_chapter_starts` pure helper
  + its tests (or place beside `build_section_starts`, which already does the
  div-boundary walk).
- `src/input/actions/pickers.rs` — reload path: same source change.
- `src/ui/keybinds_overlay.rs` — relabel "set chapter" → "set track mark" + help.
- `src/input/timestamps.rs` — optional `set_chapter`→`set_track_mark` rename;
  gutter sign-name audit.
- `~/utono/litdb/.claude/commands/litdb/timestamps-signs.md` — sign-name doc
  update (litdb side, in lockstep).
