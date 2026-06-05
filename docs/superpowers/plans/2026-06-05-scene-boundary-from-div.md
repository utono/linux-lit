# Plan: Detect scene boundaries from (div1, div2), not text inference (Path A)

## Why

linux-lit's two-column pagination decides "is this line a section break?" by
re-inferring it from the **raw .txt text** — `line_types::is_act_scene_marker`
(text starts with "ACT "/"SCENE "/"CHAPTER "…) and `is_separator` (starts with
`=`). That inference is fragile around scene transitions: a scene-ending right
column reads `dialogue → blank → [They exit.] → blank → ACT 2 → ===== → Scene 1`,
and `clamp_at_section_break`'s header-skip bridges across the exit/blanks into the
ACT marker and skips it — so the right column runs into the next act and `y` from
the next scene skips this scene's tail (the AWW 25-line `y GAP`). Two attempts to
fix the text heuristic caused catastrophic regressions (169 fails; JumpEnd → a
1-line page) — caught by the new `clamp_*` unit tests and `shakespeare_pagination_*`.

**Key finding (litdb investigation):** the boundary is *already unambiguous in the
database*. Every line carries `div1`/`div2` (act/scene); a scene boundary is
exactly where `(div1, div2)` changes. The `ACT N`/`=====`/`Scene N` lines in the
buffer are display chrome linux-lit synthesizes; the authoritative structure is
the loaded `(div1, div2)`. So: stop inferring, use the data.

## Scope (linux-lit only — no schema/import/.txt changes)

1. **Build a boundary bitmap at load.** After the buffer + `line_map` are built,
   compute `section_starts: Vec<bool>` indexed by BUFFER line: `true` at the first
   buffer line of each new `(div1, div2)` run.
   - txt-aligned works: map buffer line → `buffer_to_work[line]` → `work_lines[idx]
     .div1/.div2`; a boundary is where the work line's `(div1,div2)` differs from
     the previous non-None buffer line's. The synthesized `ACT/===/Scene` chrome
     lines (no `buffer_to_work` entry) belong to the section they introduce — mark
     the FIRST chrome line of the run as the boundary.
   - db-only works: buffer line ↔ work line is direct; same `(div1,div2)`-change rule.
   - Store `section_starts` in `AppState` (next to `line_map`); rebuild on work load
     / resnap. Empty for prose (single column).

2. **One predicate.** `fn is_section_start(state, line) -> bool` reads the bitmap
   (fallback to the old text check if the bitmap is absent, e.g. mid-load).

3. **Replace the ~14 text-inference sites** in `viewport.rs` (and the pure-test
   mirror) that do `is_act_scene_marker(t) || is_separator(t)` for *pagination*
   (NOT display styling): `clamp_at_section_break`, the right-column "begins a new
   scene" block, `back_up_for_speaker`'s section guard, `scene_snap_top` /
   `scene_heading_start`. Thread the bitmap (or a closure `is_break: &dyn
   Fn(usize)->bool`) into the pure helpers so the unit tests drive it.
   - Leave alone: title-bar scene display, gloss/synopsis `(div1,div2)` formatting,
     anything that styles the marker text itself.

4. **The header-skip becomes trivial.** With an authoritative boundary, the
   "skip my own opening heading but clamp a later one" problem dissolves: a page
   that STARTS at a boundary has `is_section_start(page_top)==true` (skip its own
   heading); a later boundary inside the range clamps. No `[They exit.]` bridging.

## Verification (all three before declaring done)

- `cargo test` — the `clamp_*` contract tests + `shakespeare_pagination_*` must
  pass. Convert the `#[ignore]`d `clamp_when_split_dialogue_is_immediately_followed
  _by_exit_then_marker` to drive the boundary bitmap and un-ignore it.
- Headless fuzz: `run-fuzz-all-works.sh --stop-on-first-fail` must clear 1H4…AWW
  (AWW `y GAP` gone) and ideally all 38 plays.
- Manual 1H4/AWW: `2`/`3` to a scene start, `y` shows the previous scene's last
  page (no skipped dialogue, no overlap).

## Risks

- The synthesized-chrome alignment (txt path) is fiddly — `build_line_map` leaves
  chrome lines as `None` in `buffer_to_work`; the bitmap must attribute them
  correctly. Add a unit test on a synthetic buffer+linemap.
- db-only works: confirm `(div1,div2)` populated for all plays (investigation: yes).

## Follow-up (optional, separate task — Path B)

Add `line_mapping.is_scene_start` (additive migration, backfilled from
`div1`/`div2`, no XML re-import) so lit-reader/vscode/android/ios get the same
unambiguous boundary. linux-lit could then read the column directly instead of
computing the bitmap.

## Reference

- Boundary detection sites: `src/input/viewport.rs` (`clamp_at_section_break`
  ~456, right-column-begins-scene ~1017, `back_up_for_speaker`, `scene_snap_top`).
- Line→work map: `src/text_file_map.rs` `LineMap.buffer_to_work`; div fields:
  `src/db/models.rs:27-29` (`div1`/`div2`/`line_in_div`); load: `src/db/queries.rs:57`.
- Contract tests: `src/input/viewport.rs` `clamp_*` + `shakespeare_pagination_*`.
