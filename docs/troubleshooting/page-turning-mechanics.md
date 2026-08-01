# Page Turning Mechanics

Reference for debugging page-forward (`x`), page-backward (`y`), and related
navigation in e-reader mode.

Consolidated 2026-07-28 — this file absorbed three former siblings, which are
now sections rather than separate documents:

- `testing-pinned-play-pagination.md` → *Testing pinned play pagination
  (the three tiers)*, below the general *Testing* section.
- `page-marker-positioning.md` → *The floating page marker*, near the end.
- `blank-line-spacing-too-tall.md` → *Dialogue spacing failures (plays)*.
  Its "Aftermath" half is the fingerprint blind spot that pairs with the
  font section below: per-tag `pixels_above_lines` are NOT fingerprinted, so
  a table recorded under broken typography still reads as a valid table hit.

A fourth section, *How changing the font affects pagination*, was written at
the same time to gather the font/pagination coupling that was previously
scattered across the staleness triggers, the prose-grid lessons, and the
testing notes.

## The authoritative-boundary principle (read this BEFORE touching pagination)

Every line in `lit.db` carries `(div1, div2)` (act, scene). **A scene/section
boundary is exactly where `(div1, div2)` changes — full stop.** The `ACT N` /
`=====` / `Scene N` lines you see in the buffer are display chrome that linux-lit
synthesizes; they are NOT the source of truth. The source of truth is the loaded
metadata.

linux-lit therefore precomputes a boundary bitmap at load
(`LineMap.section_starts`, built in `build_line_map`) and all pagination consults
it through one predicate (`AppState::is_section_start` / the `section_break_fn`
closure threaded into the pure helpers). **Never re-infer a boundary from buffer
text in pagination code.** `line_types::is_act_scene_marker` / `is_separator`
survive only as a mid-load fallback (before the line map exists) and for *display*
styling (title bar, synopsis) — never for deciding where a page ends.

Why this matters (the expensive lesson): for a long time the pagination paths
re-inferred "is this a section break?" from the raw `.txt` text. That inference
is fragile exactly at scene transitions — a scene-ending column reads `dialogue →
blank → [They exit.] → blank → ACT 2 → ===== → Scene 1`, and the text-based
"header-block skip" bridged across the exit/blanks straight into the `ACT 2`
marker and skipped it, so the column ran into the next act (the AWW 25-line `y
GAP`). Two attempts to patch the text heuristic caused catastrophic regressions
(169 test fails; `JumpEnd` → a 1-line page). The whole class dissolved the moment
the boundary was read from `(div1,div2)` instead of guessed from text. **If you
find yourself reasoning about which buffer lines "look like" a marker to decide a
page boundary, stop and read the bitmap instead.**

Pattern when adding/keeping a per-line structural fact (boundary, chapter,
dialogue, spoken-status): if the DB already encodes it, surface it through
`LineMap` / `Line` and read it — do not reconstruct it by classifying buffer
text. Reconstruction drifts from the data and the drift surfaces as a pagination
bug three transformations downstream.

**The same principle governs PAGE TOPS, and that half is easier to forget.**
Text inference is the famous version, but a *geometric* computation is the same
mistake wearing different clothes: measuring the viewport to decide where a page
begins re-derives a fact the pinned `play_pages` / `prose_pages` tables already
state. Five separate bugs in one day (2026-07-27) were this — four page-turn
cases plus a landing case — each a measurement quietly disagreeing with the
stored grid. Two structural traps make it recur:

- **A live walk silently substitutes for a missing table.** Helpers that check
  one table and fall through to geometry look correct on the engine they were
  written for and go wrong on the other. Check BOTH tables before falling back.
- **A bare `usize` page top cannot express a prose boundary.** Prose tops are
  `(line, row-offset px)` pairs; a signature that drops the offset forces every
  caller to re-derive it, which is how three call sites grew three different
  private workarounds. Carry the pair.

If you are about to compute a page top from `line - 1`, a page-height estimate,
or a forward walk — check `canonical_page_top_offset_for` first. See "A landing
that drops out of table mode" below.

### `PageTop` — half this class is now a compile error (2026-07-27)

A page position is ONE value, `input::page_top::PageTop { line, offset }`, with
PRIVATE fields. It replaced two loose public `AppState` fields
(`page_top_line` / `page_top_offset`) that had to change together and mostly
didn't: 29 sites assigned the line, 12 set the offset. That gap produced five
shipped bugs in a single day.

**You can no longer hand a bare line to `set_page_instant`.** The
journal-Escape bug and the cross-work landing bug are both compile errors now.

Two constructors, and the choice is the whole point:

- `PageTop::new(line, offset)` — a position read from a pinned table.
- `PageTop::at_line_start(line)` — offset 0, **a claim that no pinned prose
  grid can be active here.**

There is deliberately NO `From<usize>`: an implicit conversion would silently
restore the bug. That makes the audit a grep —

```
rg "at_line_start" src/
```

— and every hit is a claim to re-check. When the claim is wrong, it is a fresh
instance of the landing bug. The 25 existing sites were audited at the
migration and the non-obvious ones carry a comment saying why offset 0 is
correct (sonnet-sequence only / two-column only / scroll mode, none of which
can have a prose grid).

**`page_back_stack` is `Vec<PageTop>`**, not `Vec<(usize, i32)>`. Equality
compares BOTH halves, which is what resnap-style "am I already on the grid?"
checks depend on — comparing lines alone is how an off-grid position passes for
canonical.

The migration was compiler-driven (339 errors, 331 of them mechanical field
renames) and verified behaviour-identical: the nav-fuzz produced step-for-step
identical output on both engines, prose (205 steps) and play (192 steps).

## The pagination model (read this first)

linux-lit paginates a **flat buffer of lines** into pages. A play renders as a
**two-column spread**: the reader fills the LEFT column top-to-bottom, then the
RIGHT column, then turns to the next spread. Prose and translation mode use one
column.

> **Two engines, not one.** This section describes the LIVE engine, which
> computes pages on the fly. Since 2026-07-04 most works also have a PINNED
> page table in lit.db (`play_pages` / `prose_pages`, keyed by a layout
> fingerprint so a font/size/width change misses and regenerates) — see
> "Pinned page tables" below. When a table is active it is AUTHORITATIVE and
> `column_split` is not consulted for rendering. Mixing the two is the single
> most common source of pagination bugs: a table-chosen page top paired with a
> live-computed end renders a window neither engine would choose. Always
> establish WHICH engine is live before reasoning about a symptom
> (`PAGES: table hit` / `PAGES_PROSE: table hit` in the log).

**One function defines a spread: `column_split(top)`** (`viewport.rs`). Given a
page-top line it returns a `ColumnSplit { split, page_end, next_page_top }`:

- `split` — first line of the RIGHT column (left column is `[top, split)`).
- `page_end` — last visible line of the spread (bottom of the right column).
- `next_page_top` — first line of the FOLLOWING spread (`page_end + 1`).

Everything else is built on this. The renderer scrolls the left view to `top`
and the right view to `split`; `page_forward` advances `page_top` to
`next_page_top`; `prev_page_top` walks *backward* to find the spread before the
current one. **The cardinal rule is TILING:** consecutive spreads must abut with
no gap and no overlap — `column_split(top).next_page_top` is exactly the next
spread's `top`. Most pagination bugs are a tiling violation (a line shown twice,
or skipped).

## Which binds may turn the page (2026-07-27)

**BACKWARD segment binds never turn the page; FORWARD ones may** (revised
2026-07-27). The asymmetry is deliberate: reading is forward-biased, so a
forward bind that stopped at the page edge would block progress and force an
`x`, whereas a backward bind stopping there merely declines to leave the page
the reader is looking at.

**May turn (`Direction::Next`):**

- `q` / `J` — `JumpToNextSpeaker` (next speaker turn; next paragraph on prose)
- `'` / `Down` — `CursorNextDialogue` (seeks audio)
- `h` — `CursorNextDialogueNoSeek` (cursor only, MPV keeps playing)

**May NOT turn (`Direction::Prev`)** — target off-page ⇒ cursor REVERTED, key
is a no-op; crossing backward is the job of `y` / `{`:

- `,` / `K` — `JumpToPrevSpeaker`
- `;` / `Up` — `CursorPrevDialogue` (seeks audio)
- `t` — `CursorPrevDialogueNoSeek` (cursor only)

**Key names above are this user's actual layout** — resolved from the stowed
`~/.config/linux-lit/keymap.json` (`reader` scope), which OVERRIDES the
compiled defaults. Do not copy key names out of source doc comments: several
still say `j`/`k` for the segment binds, which this keymap rebinds to
`NextBookmark`/`PrevBookmark` — a different subsystem, unaffected by this rule.
The tagging is per HANDLER, not per key, so the rule holds whatever a key is
bound to; only the prose naming keys can go stale. Re-derive with:

```bash
python3 -c "import json;d=json.load(open('$HOME/.config/linux-lit/keymap.json'));[print(('+'.join([m for m in ('ctrl','alt','shift') if b.get(m)]+[b['key']])).ljust(12),b['action']) for b in d['reader'] if 'Dialogue' in b['action'] or 'Speaker' in b['action']]"
```

Implemented at one choke point, not per bind:
`navigation::keep_jump_if_on_page(state, prev_line, dir)` short-circuits to
true for `Direction::Next`; for `Direction::Prev` it consults
`scroll::jump_stays_on_page` and reverts `current_line` (returning false) so
the caller returns early. 11 call sites across
`jump_to_{next,prev}_dialogue`, `cursor_{next,prev}_dialogue`,
`cursor_{next,prev}_dialogue_no_seek`, `jump_to_{next,prev}_speaker` — 5
forward, 6 backward.

**History:** the rule originally barred BOTH directions (`f4b63088`). That
made `q` dead-end at every page edge, and — combined with a geometry/table
disagreement — produced the trapped-cursor bug in FAILURE MODE 1 below. The
forward half was lifted the same day.

**Deliberately exempt:** the scene/act jumps (`jump_to_next_scene` and
friends) — moving between divisions is their purpose. Scroll mode and the
translation overlay are exempt inside `jump_stays_on_page`: neither paginates,
so there is no page to leave.

This rule was requested as PROVISIONAL ("i am unsure if this is what i will
ultimately want"), which is why it lives in one guard rather than spread
through each bind — relaxing or re-scoping it means editing
`jump_stays_on_page`, not eleven call sites.

### FAILURE MODE 0 — a bug the nav-fuzz structurally CANNOT reach (open, 2026-08-01)

Before diagnosing any pagination bug from a green fuzz run, check WHAT HEIGHT
the run used. **`run-fuzz.sh` (cage) has no resize and always runs at the
1280x720 default — `text_view.height = 648`.** Production is **1096-1098**.
That is not a near-miss: on `R2-Arkangel` it is a different page grid
outright — **101 pages at 648px vs 62 at ~1130px**. Whole regions of the work,
including the end-of-work pages, are unreachable at 648px, so a green fuzz run
is evidence about a geometry the user never reads at.

**Tell:** a fuzz run passes with hundreds of steps, but the max page index in
the log is roughly double production's page count for that work.

Use `run-fuzz-niri.sh`, which resizes to 1920x1236 and reports the achieved
height in its summary line. It runs the WM the user actually runs, and its
usable height (~1132) lands far closer to production than cage at the same
output size (1164) — the kiosk compositor does not reserve what a tiling WM
does.

**The bug this surfaced, still OPEN:** seed 72, `R2-Arkangel`, step 29 —
a last-page `PageBackward` shows 70 lines twice.

```
PAGES: page 61/62 top=3661
NAV_TEST: step=29 PageBackward top=3665->3661 line=3738->3734
NAV_TEST: FAIL step=29 PageBackward y OVERLAP: back-page top=3661 runs to
  next_page_top=3735 PAST old top=3665 (70 lines shown twice)
```

It is **height-dependent, not compositor-dependent**: the identical seeded step
passes under cage at 1164px (`top=3662->3649`, no overlap) and fails under niri
at 1132px. Reproduce with:

```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz-niri.sh \
  --start-work R2-Arkangel --seed 72 --secs 90
```

Root cause not yet investigated. The suspicion to start from is the last-page
backward step: near the end of a work the backward target is computed from a
full-page walk that the final, SHORT page cannot satisfy, so it lands above
where it should and overlaps the page it came from. Confirm against
`column_split(...).next_page_top` before changing anything.

### FAILURE MODE 1 — `q` dead on an overflowing prose page (fixed 2026-07-27)

**Tell:** a dialogue/segment bind stops working on ONE page while the target
paragraph is plainly visible on screen. The log repeats the SAME transition
with no follow-up:

```
ACTION: JumpToNextSpeaker
PARAGRAPH_NEXT: 934 -> 935      <- repeats forever, cursor never moves
```

A working press is followed by `CURSOR_LINE: applied tag to line N` and
`SEEK:`; the dead ones have neither. That "computed a target, then bailed" pair
is the signature of `keep_jump_if_on_page` reverting the cursor.

**Root cause:** `jump_stays_on_page` re-walked LIVE geometry
(`is_line_start_visible`) while the renderer clipped at the STORED page end
(`d2696b1f`). On a page whose stored span OVERFLOWS the viewport the two
disagree — look for the overflow warning on the same page:

```
BOTTOM_CLIP_EXACT: widget_h=1098 total=1175 clip=0 page_top=931 end=936
CLIP_WARN: main-card two-col OVERFLOW total=1175 > widget_h=1098 (clip-prevention.md #12)
```

Lines past the geometric fit point are still PAINTED (the table decides the
clip), but the fit-walk called them invisible, so every jump into that band was
reverted. The cursor was trapped at 934 with 935 and 936 on screen.

**Fix:** make the guard table-authoritative —
`prose_pages::prose_table_line_on_current_page` (built on the pure
`line_on_stored_page`) answers from the stored page; the live walk is only the
fallback when no table is active or the top is off-grid. NOTE the two-column
branch needed NO change: `is_line_fully_visible` already consults
`table_end_for_top`. Only single-column prose's `is_line_start_visible` was a
pure geometry walk — that asymmetry is why the bug was prose-only.

**Generalized lesson (the recurring one):** every consumer that asks "is this
line on the current page?" must read the TABLE in table mode, never re-walk
live geometry. Same class as the `table_end_for_top` doc note (playback sync
and j-navigation walking past the rendered page end). When a nav bind dies but
the text is visible, suspect a geometry/table disagreement before suspecting
the bind.

**Repro:** `scripts/land-on.sh BH-Barrett 10.0`, resize to 1920x1236 (text_view
must log `-> 1098`), then `q` from line 931. Pre-fix the cursor sticks at 934;
post-fix it advances 935, 936, then correctly refuses (937 is off the stored
page — the no-page-turn rule still holds).

**How a spread's extent is decided** (inside `column_split`):

1. Fill the left column by pixel height (`visible_range` + a descender guard),
   then trim what would look broken at the split (a dangling speaker name, a
   half stage-direction). `split = left.last_fit + 1`.
2. **If the right column would BEGIN a new (non-final) ACT/SCENE** — skipping
   leading blanks/exits from `split` lands on a section marker — the page **ends
   in the left column**: `page_end` is before the marker, `next_page_top` is the
   marker. The new scene starts the next spread. (This is the "stop at a scene
   break" reading model — a scene-ending page may have a short/empty right
   column, and `y` from the next scene tiles into it exactly.)
3. Otherwise fill the right column, **clamped at a section break** so a new
   act/scene never appears mid-column — EXCEPT the work's final trailing section
   (an EPILOGUE), which has nowhere to be pushed and fills the right column.

**Two symmetric exceptions to "fill left first".** A short section at either END
of the work fills the RIGHT column rather than sitting alone in the left:
- **EPILOGUE (final spread):** a short tail fills the right column; `last_page_top`
  forward-pulls the top so it lands there (see *the asymmetry* below).
- **PROLOGUE / opening section (first spread):** a short COMPLETE opening section
  (Prologue, Induction, Chorus, or a brief opening scene) that ends at the first
  section boundary moves to the right column with the LEFT column EMPTY. The
  start-of-document mirror of the EPILOGUE rule. `column_split(state, 0)` returns
  `split = 0` (empty left), `page_end = N-1`, `next_page_top = N` — `next_page_top`
  is UNCHANGED from the empty-right behavior it replaces, so tiling is untouched;
  only the visual placement within the first spread changes. Requires the whole
  opening section to fit the right view's height (otherwise normal left-fill).

**The asymmetry that bites every backward/jump fix:** forward paging uses
`column_split`'s boundary; but the work's **final spread** is special. When the
tail is short, `last_page_top` (`navigation.rs`) FORWARD-PULLS the final top a
few lines so the tail fills the right column (a full spread, not a lonely left
column). That pulled top is NOT on the natural `column_split` chain, so:
- it must be reached the same way from EVERY entry point (startup, `G`, `x`,
  `j`, `y`) — see *Diagnosing § "FIVE paths"* in headless-testing.md;
- `y` from it cannot tile exactly (a small benign seam) — the fuzz exempts it.

A dialogue tail defeats the dialogue-below test: MND ends with the
remainder of Robin's spoken epilogue (plain 5.1 dialogue, no trailing
section), so "dialogue remains below `next`" is true even at the work's
real end. `last_page_top` therefore has a SECOND true-end signal: the
forward page chain ENDS at `next` (`next_page_top(next)` cannot advance).
Either signal triggers the case (a) pull-forward. A mid-work
scene-opening boundary always has further pages, so the chain-end signal
never fires mid-work (fixed 2026-07-04; before this, all MND-* editions
stranded one spread early and the last ~9 lines were unreachable). In the
same class: when `x`'s landing is REDIRECTED to the final-spread anchor
(`redirect_to_final_spread`), the cursor must be recomputed for the
ANCHOR page (its last on-page dialogue line, mirroring `jump_to_end`) —
`next_dialogue` was computed for the natural pre-redirect turn and the
pull-forward can strand it ABOVE the page (no visible highlight).

**`column_split` is the source of truth.** Render, the page-tiling fuzz
invariants, `prev_page_top`, and `last_page_top` all consult it. If you change
how a spread is measured, change `column_split` and everything follows; do NOT
add a parallel boundary calc (the historical `next_page_top()` single-column
helper diverged from `column_split` by a speaker block and caused a persistent
`y GAP` — backward nav now tiles against `column_split` in two-column mode).

## Architecture

Page state lives in `AppState`:

- `page_top_line: usize` — buffer line at the top of the current viewport
- `page_top_offset: i32` — pixels scrolled PAST `page_top_line`'s pixel top. 0 in
  the normal line-aligned case; non-zero ONLY while paging within an over-tall
  prose paragraph (see *Prose over-tall paragraph* below). Viewport top y =
  `line_yrange(page_top_line).y + page_top_offset`.
- `page_back_stack: Vec<(usize, i32)>` — history of previous
  `(page_top_line, page_top_offset)` pairs, pushed by `page_forward`, popped by
  `page_backward`. The offset is in the entry so `y` round-trips a mid-paragraph
  forward turn exactly.

## Pinned page tables (plays)

Two-column plays at the pinned layout (the user's Charter/1920x1200 reading
setup) do NOT run the forward-walk heuristics at all: pages come from lit.db
`play_pages` (keyed by `line_mapping` ids + a layout fingerprint), generated
once in-app by recording the live engine's walk and gating it behind the
invariant suite in `src/input/page_table.rs` (coverage, tail, fit,
watermark-sanity, determinism). `x`/`y`/`G`/`gg`/lookups are index arithmetic
(`PAGES:` log lines). EVERYTHING in this document still applies to the
fallback modes — fingerprint mismatch (font/resolution change, re-import),
interlinear translations, scroll mode, 1-col — which use the live engine
unchanged, and to the generator itself (the table is only as good as the
walk it records; a walk bug becomes a VALIDATE_FAIL or a bad stored table).
The final table page mirrors the canonical `G` spread (the forward-pulled
`last_page_top` anchor), so `G`/`x`-into-the-end land on the same stored page.

**Keyed by the edition's OWN abbrev, not canonical.** Each edition (`Rom`,
`Rom-BBCClassic`, …) has its own `line_mapping` ids, so a table stored under
the canonical base abbrev could never be loaded by a sibling edition (the
`db_fingerprint` fails closed) while editions overwrote each other's rows under
the shared key. Every edition therefore stores its own table; base works are
unchanged (their abbrev IS canonical). True cross-edition sharing is a hot.db
concern (`page_spread`, citation-keyed), not lit.db. Do not "fix" a missing
table for an edition by pointing it at the base work's rows.

**Sync page turns land on the table's grid too.** In table mode
`update_highlight_and_advance_page` / `_ensure_visible` (`highlight.rs`) get
the landing top from `page_table::table_top_for(state, line)` — the stored page
containing the spoken line — instead of the live `page_turn_top_state`. Without
this a sync-driven turn landed off the grid (force-top-aligning the spoken line;
self-correcting on the next `x`/`y` but visibly wrong). When no table serves the
line, both fall back to the live computation including the final-region
redirect.

**Staleness & revalidation — three triggers.** The fingerprint covers font AND
both window dimensions, so a stale table must be dropped whenever any of them
changes without a work reload:

- **Window resize (width OR height-only):** the resize tick in `app/mod.rs`
  schedules `page_table::revalidate_on_resize` after a 400ms settle delay so
  column geometry has finished reflowing before it fingerprints. A HEIGHT-only
  change (dwl stack-retiling) goes stale just as easily as a width change —
  this branch must fire for both (it once fired only on `width_changed`).
- **Font size/family change:** `adjust_font_size` / `reset_font_size` /
  `cycle_font` (`app/font.rs`) revalidate (drop + reload) BEFORE the resnap —
  the window size doesn't change, so the resize path never fires. Look for
  `PAGES: dropped table (layout changed)`; the new font's fingerprint then
  loads a matching stored table if one exists. **It does NOT regenerate on
  prose** — unlike the resize tick, the font path never clears the
  once-per-session generation latch, so the first font change usually drops
  the reader to the live engine for the rest of the session. See *How
  changing the font affects pagination* below, which covers this whole path.
- **Re-import:** the per-row `db_fingerprint` check at load fails closed.

To check which engine a RUNNING instance is on, use the
`check-page-table-usage` skill (reads the `PAGES: table hit/fallback/generated`
log lines).

**Heading chrome is un-snapped at load (the id round-trip loses it).** Act/
scene headings and separator rules are synthesized chrome with NO `line_mapping`
rows, so a page top falling on chrome was stored as the first MAPPED line at or
after it — a table-driven page then opened at the entrance stage direction
instead of `ACT 1 / Scene 1` (Rom page 2: generator walked top=21, table
resolved top=27). `load_for_work`'s `unsnap_top` walks back over contiguous
unmapped lines to the chrome start, then forward over leading blanks (the
previous page's trimmed tail). Applies to both `left_start` and `split`; a
no-op for mid-page tops and blank-only gaps. Load-side, so every stored table
is repaired without regeneration.

**Testing in table mode.** `LIT_NO_PAGE_TABLE=1` forces the live engine;
`LIT_GEN_PAGE_TABLE=1` forces generation at the current (e.g. headless)
geometry — `run-fuzz.sh` forwards both into the cage env. The nav-fuzz is
table-aware: check 2f asserts `x`/`y` move exactly one page (±1) through
`active_page_table`'s index space (edge-pinned, canonical G-seam allowed), and
the column-boundary checks (balance, layout, jump-to-end, clipping) read the
STORED spread via `effective_column_split` — re-deriving the boundary with a
fresh `column_split` re-infers a boundary the renderer didn't use (the same
assertion-re-inference class as the 2H6/Cor/Ham text-classifier false
positives; a stored `split == None` is the sanctioned empty-right page). Audit
stored tables with the `validate-play-pages` skill.

One benign representation difference: a one-line right column whose only line
is an unmapped blank/stage direction (no `line_mapping` row) stores as an
empty-right page (`split_id` NULL) — the blank renders below the left column's
clip instead of atop the right column; both are invisible, and extents match a
live recompute.

Key files: `src/input/page_table.rs` (`validate_spreads`, `layout_fingerprint`,
`record_spreads`, `generate_and_store`, `load_for_work` + `unsnap_top`,
`revalidate_on_resize`, `active_page_table`, `spread_for_top`, `table_top_for`,
`page_for_line`), `src/db/play_pages.rs` (rw layer, per-edition abbrev key),
`src/app/font.rs` (font-change revalidation), `src/app/mod.rs` (resize-tick
revalidation), `docs/superpowers/specs/2026-07-04-pinned-play-pagination-design.md`.

## How changing the font affects pagination

Changing the font is not a cosmetic operation — it invalidates the entire
page grid. Every stored page in `play_pages` / `prose_pages` was recorded at
one specific set of font metrics, and a different family or size means
different line heights, different wrap points, and therefore different page
boundaries. This section is the map of what actually happens on a font
change and which failures to expect.

**The binds.** `Shift+F` cycles the family forward through
`config::FONT_CYCLE` (Charter → Crimson Pro → Noto Serif → Source Serif 4 →
IBM Plex Serif → Cormorant Garamond, wrapping); `Ctrl+Shift+F` cycles back.
Size is `Ctrl+|` / `Ctrl+!`, reset is `0`. All four land in `app/font.rs`
and run the same sequence.

### Why the font is part of the fingerprint

`page_table::layout_fingerprint` hashes the font DIRECTLY (`font_family`,
`font_size`) **and** three derived Pango metrics — `ascent`, `descent`,
`approximate_char_width` — read from the live `pango_context`. The derived
metrics matter as much as the name: two families at the same nominal point
size have different ascent/descent, so they fit a different number of lines
in the same viewport. `prose_layout_fingerprint` builds on that base and
adds `cw` (the font-adaptive effective card width) and the `pvN` boundary
version, because the prose card is measured in characters and therefore
moves with font metrics too.

A real fingerprint, from a live log line, with the fields named in
`fingerprint_string` order:

```
fp=v5|Crimson Pro|16|19|4|8|1920x1200|6|40|1|74|1098|uh1072|cw1050|pv5
```

- `v5` schema version, then `font_family`, `font_size`, `ascent`, `descent`,
  `char_width` — the first six fields, four of them font-derived.
- `1920x1200` window width×height, then `line_spacing`, `text_margins`,
  `columns`, `top_spacer_height`, `view_height`.
- `uh…`, `cw…`, `pv5` are the PROSE suffix (usable height, effective card
  width, boundary version) appended by `prose_layout_fingerprint`; a play
  fingerprint ends at `view_height`.

The consequence: **a font change is a guaranteed fingerprint miss.** There
is no partial invalidation and no attempt to re-fit the old boundaries —
the stored grid is simply not for this font.

**What the fingerprint does NOT cover:** per-tag `pixels_above_lines` (the
speaker / stage-direction / act-header gaps). Those change how many rows fit
a column while leaving the fingerprint identical, so a table generated while
dialogue formatting was broken — or after retuning a gap value — is accepted
as a valid hit. See *Dialogue spacing failures (plays) → Aftermath*.

### What happens on a font change, in order

All three functions (`cycle_font` for the family, `adjust_font_size` and
`reset_font_size` for the size) run the same sequence, and the ORDER is the
load-bearing part:

1. `config.font_family` / `font_size` is updated.
2. `reapply_font` rebuilds the `font-size` tag on the buffer (this also
   restyles the translation overlay, which follows the reader font).
3. `page_table::revalidate_on_resize` + `prose_pages::revalidate_prose_on_resize`
   — fingerprint the new layout, DROP a table that no longer matches, and
   attempt a reload.
4. `pending_prose_cross = None` — a scheduled phrase-boundary page turn was
   computed against the OLD grid's boundary and must not fire against the new
   one.
5. `resnap_page` + `resnap_prose_to_table` — re-anchor the current position
   onto whatever grid is now active.
6. `config::save` and a `notify-send` toast showing `family` + `position/6`.

**Steps 3–5 must stay in this order.** Resnapping before revalidating would
anchor the reader to the grid that is about to be dropped. This is the same
"any path that SETS a page top must land on the stored grid" rule from the
prose-grid lessons, applied to the font path.

### The prose regeneration gap (verified 2026-07-28)

**`revalidate_*_on_resize` drops and RELOADS; it does not generate.** On a
prose work that has no stored table at the new font's fingerprint, the drop
therefore leaves the reader on the live engine — and because
`generate_and_store_prose` is latched by `prose_page_table_gen_attempted`
(a once-per-session one-shot, cleared only on WORK LOAD and by the RESIZE
tick, never by `app/font.rs`), nothing regenerates it for the rest of the
session.

There is a second, sharper consequence that is easy to miss: after the drop,
`state.prose_page_table` is `None`, and **both revalidate functions return
early when their table is already `None`.** So the very first font change
disables the revalidation path itself. Cycling onward — even cycling all the
way back to the ORIGINAL font, whose table is still sitting in lit.db — never
reloads it.

Observed end-to-end while adding the `Shift+F` bind, on BH-Barrett:

```
PAGES_PROSE: table hit (942 pages) for BH-Barrett
PAGES_PROSE: dropped table (layout changed)
PAGES_PROSE: no table for BH-Barrett fp=v5|Crimson Pro|16|…|pv5
FONT: cycled to Crimson Pro
FONT: cycled to Noto Serif          <- no dropped/hit/generated lines after this
… seven more cycles, back through Charter …
```

Nine font changes, exactly one `dropped` line, and no `generated` line at
all. The reader spent the whole session on the live engine, including the
return to Charter.

**Diagnostic tell:** a font change followed by `BOTTOM_CLIP_ROWFILL` where
you previously had `BOTTOM_CLIP_EXACT` means you have fallen to the live
row-fill engine. That is EXPECTED after a font change on prose today — do
not go hunting for an off-grid-top bug (the *A landing that drops out of
table mode* section) until you have confirmed a table is actually active.

**If you are fixing this**, the fix is to clear the latch and regenerate the
way the resize tick already does (`app/mod.rs`, guarded on "actually
dropped" so a no-op never triggers a full-document walk on a 7300-line
novel) — not to remove the early return, which exists so an inactive engine
costs nothing. Note the cost is real: regenerating a novel's grid walks the
whole document, and a font CYCLE can fire that repeatedly as the user taps
through six families.

### Plays behave differently from prose

- **Plays** revalidate and reload cleanly, and `generate_and_store` is not
  behind the same session latch, so a two-column play is more likely to end
  up back in table mode after a font change.
- **Prose** hits the gap above.

So "did my font change break pagination?" has different answers by work
type. Establish which engine is live FIRST (`PAGES:` vs `PAGES_PROSE:` log
lines, or the `check-page-table-usage` skill) — the two-engines warning at
the top of this file applies with full force here, because a font change is
the most common way a reader crosses between them mid-session.

### Consequences for reading position

A font change does NOT preserve the page number, and cannot: page 40 of 942
in Charter is not page 40 of some other count in Cormorant Garamond. What is
preserved is the CURSOR's line, with the page re-derived around it by the
resnap. Expect the visible page to shift — that is correct behaviour, not a
page-turn bug.

Two real effects follow:

- **Larger fonts mean more pages** and can push a work from "fits" to
  "over-tall" on a long prose paragraph — the sub-line paging path (*Prose
  over-tall paragraph*) becomes reachable at a size where it wasn't before.
- **A scheduled sync page turn is cancelled** (`pending_prose_cross = None`).
  If audio is playing across a font change, the next turn is re-derived from
  the new layout; a turn that seems "missed" at exactly the moment of a font
  change is this, not a sync bug.

### Testing a font-related pagination change

The font is a fingerprint input, so a headless run at the wrong font
paginates differently — the same trap as the wrong GEOMETRY (see *Headless
runs at production geometry*). Two rules:

- The headless config is `config-dev.json` (via `LIT_DEV=1`). Its
  `font_family` is what the run actually uses; a run that cycles the font
  REWRITES that value on exit, so a fuzz run can silently leave the next run
  on a different font. Check it before trusting a comparison.
- To exercise the TABLE path after a font change, force generation with
  `LIT_GEN_PAGE_TABLE=1` — otherwise the prose latch above means you are
  testing the live engine while believing you are testing the grid.

Key files: `src/app/font.rs` (`cycle_font`, `adjust_font_size`,
`reset_font_size`, `reapply_font`), `src/config.rs` (`FONT_CYCLE`),
`src/input/page_table.rs` (`layout_fingerprint`, `revalidate_on_resize`),
`src/input/prose_pages.rs` (`prose_layout_fingerprint`,
`revalidate_prose_on_resize`, `generate_and_store_prose` + the
`prose_page_table_gen_attempted` latch), `src/app/mod.rs` (the resize tick,
which DOES clear the latch — the model for fixing the gap above).

## Prose over-tall paragraph (sub-line paging)

**The trap:** prose stores ONE buffer line per paragraph, and a long paragraph
wraps TALLER than the viewport (Bleak House "On such an afternoon…" = 2529 chars,
1170px vs ~1067px usable). Pagination counts whole buffer lines via
`line_yrange`, so `visible_range` fits ZERO lines for an over-tall paragraph at
`page_top` (`last_fully_visible_line == page_top`). Without special handling
`next_page_top` then advances `new_top` to `page_top + 1` = the NEXT paragraph,
**dropping every wrapped row of the current paragraph below the fold** (the
classic "x skips a chunk of a long paragraph" bug). The render/clip side already
handled this (`update_bottom_clip`'s `range.count==0` branch reads the live scroll
and clips at a visual-row boundary), but page-forward did not continue by row.

**The fix — sub-line scroll within the paragraph.** When the paragraph at
`page_top` is taller than the viewport, `x` advances the SCROLL by one viewport
height WITHIN the same buffer line (a `page_top_offset`, snapped to a real
visual-row top), and only advances `page_top_line` to the next paragraph once the
paragraph is exhausted. `y` reverses it.

- `page_forward` (single column only): `overtall_forward_step` measures the
  paragraph height + usable height, asks the PURE
  `viewport::overtall_next_offset(offset, para_h, usable)` whether rows remain
  below the fold, snaps `y + raw` DOWN to a real visual-row top via
  `scroll::snap_value_to_display_row` (the main-card per-`display_rows` snap —
  sanctioned in clip-prevention.md), and on a within-paragraph step pushes
  `(page_top_line, page_top_offset)` and calls `set_page_instant_offset(state,
  top, new_off)` (page_top_line UNCHANGED). Falls through to the normal line turn
  when the paragraph is exhausted.
- `set_page` resets `page_top_offset = 0` on every whole-line turn; jumps/search/
  scene all go through it (offset 0). `set_page_instant_offset` /
  `snap_scroll_to_line_offset` carry a non-zero offset only on the over-tall
  forward step and the `page_backward` mid-paragraph restore.
- `page_backward` mirror: when the popped entry is the SAME buffer line behind the
  current scroll, restore via `set_page_instant_offset` (no line turn); else
  normal `set_page`. The stale-drop loop compares `(line, offset)`.
- Playback sync / dimming / two-column plays: unchanged. The over-tall guard
  `current_line > last_vis` is already false when the cursor is the same buffer
  line as `page_top` (over-tall → `last_vis == page_top`), so sync never spuriously
  turns; sub-line offset is manual-paging only.

Guarded by `viewport::overtall_offset_tests` (pure coverage + multi-step + safety)
— the old `test_page_forward_prose_bleak_house` models a fixed 30-line page and is
BLIND to an over-tall single buffer line, which is why this bug shipped. Visual
acceptance is pixel-level (the dropped tail must reappear; `x`/`y` must round-trip
the mid-paragraph stops) — verify on the real display.

## Prose grid lessons (pv3 chapter-at-top + landings, pv4 row-fit)

Rules added 2026-07-06 (pv3) and 2026-07-09 (pv4), all bug classes worth
re-checking after any prose pagination change:

**Chapter-at-top is a PAGINATION rule, in one place.** A `chapter_start` line
never renders mid-page: `prose_next_boundary` (navigation.rs) clamps the fill
boundary to the first chapter heading whose line-box top falls inside the
page's pixel window (`chapter_clamp` — the prose analog of the play engine's
`clamp_at_section_break`). Because the stored `prose_pages` grid is recorded by
walking that same function, the rule lands in the grid automatically. The page
before a chapter ends early — SHORT pages before headings are by design, and
`validate_prose_pages` permits them (it checks fit/adjacency/tail, not
fullness). **Whenever the meaning of a prose boundary changes, bump the `pvN`
tag in `prose_layout_fingerprint`** (prose_pages.rs) so every stored table
misses and regenerates — chapter-at-top was `pv2 → pv3`.

**Every prose jump landing must read the STORED grid, never the live walk.**
`canonical_page_top_for` consults only the PLAY table; for prose it falls into
the live whole-line engine, which knows nothing about the row-fill grid — so a
jump routed through it lands an off-grid page (observed: `{`/`[` showing the
chapter heading mid-page even though the pv3 grid was correct in lit.db). The
pattern is `prose_pages::prose_table_boundary_for_line(state, target)` +
`set_page_instant_offset` (see `chapter_jump_land_ereader` in navigation.rs);
the canonical-walk path is the fallback for gridless works only. This is the
prose twin of the play-side "read the TABLE, never re-walk live" lesson.

**A RESTORED position must be re-anchored to the grid too (2026-07-27).** The
same symptom — a chapter heading rendering mid-page while the stored grid is
provably correct — arrived a third time, through a new entry point: closing a
gloss/journal overlay. `restore_saved_position_resnap` (app/mod.rs) set
`(page_top_line, page_top_offset)` from the saved position and called
`resnap_page`, which scrolls to whatever pair it is handed *without checking it
against the active table*. A position saved before a table (re)generation is
then restored verbatim and the reader sits off-grid; the renderer draws a
window the pagination never chose, straddling the chapter break.

Diagnosis, for the next occurrence: every legitimate page logs
`PAGES_PROSE: page N/M top=(line,offset)`. Take the `page_top` from the
symptom and grep for it — **if no `page N/M` line ever reports that top, the
page top is off-grid and the grid is innocent.** (Observed: BH-Barrett
rendering from `page_top=697`, a line no stored page starts at, immediately
after two `RETURN_TO_READER` events, with "CHAPTER VIII" stranded mid-page.
All 67 chapter starts in that work's active table begin a page — verified by
querying `prose_pages`.)

Fix: `restore_saved_position_resnap` now calls `page_table::resnap_to_table`
and `prose_pages::resnap_prose_to_table` before `resnap_page`. Both helpers
already existed and already ran on font changes and at startup — the
overlay-return path simply never called them. Both are no-ops when their
engine is inactive or the position is already canonical.

Lesson: **any path that SETS a page top — generate, jump, resnap, or restore —
must land on the stored grid.** A grid that is correct in lit.db proves nothing
about what renders; the top is one half of the contract. The play engine hit
the same class the same week (`last_page_top` walking the live chain, see
clip-prevention.md #12).

**…and the page END is the OTHER half (2026-07-27).** The very next report of
this symptom had a CORRECT top, so the rule above did not cover it — do not
stop diagnosing once the top checks out. The single-column prose bottom clip
was purely GEOMETRIC: it covers from the last visual row that fits
`usable_height` down to the card edge and never consulted the stored page.
That is right when a page ends because it RAN OUT OF ROOM, and wrong when it
ends EARLY BY RULE — which is exactly what the chapter clamp does, so the
viewport painted straight through the boundary.

The two-column path never had this bug: it always passes the stored split as
`exact_end`. `scroll::prose_exact_end_for_current_page` is the single-column
counterpart, derived from `prose_table_last_line_for_top`.

**Both clip-scheduling sites must use it.** `refresh_bottom_clip` gated
`exact_end` on `column_count() == 2`, so fixing only the render path left a
second live route to the same bug. They now share one helper — if you add a
third scheduling site, route it through the helper too.

Diagnosing the two apart, from the log:

- `page_top` is NOT a stored page start ⇒ off-grid TOP (the previous entry).
- `page_top` IS a stored page start but the clip line reads
  `BOTTOM_CLIP_ROWFILL` ⇒ the geometric clip is running where the stored END
  should govern. An `exact_end` page logs `BOTTOM_CLIP_EXACT` instead.

(Observed: BH-Barrett page 82 = `(686,0)..(697,0)`; buffer 697 is the
"CHAPTER VIII" line, so the table stops exactly at the heading and page 83
opens on it — yet the reader painted past it with `row_clip=0`.)

`last_rendered_line` (prose_pages.rs) is the pure inclusive/exclusive
conversion the clip depends on, with a regression test built from those real
page-82 values: the clip's `+ 1` makes the exclusive end 697, the heading
line, which must not paint. **An off-by-one there IS this bug.**

**A RESIZE used to disable pinned prose pagination for the rest of the session
(fixed 2026-07-27).** `revalidate_prose_on_resize` drops a table whose
fingerprint no longer matches and retries a LOAD — but generation is latched
once per session by `prose_page_table_gen_attempted`, and that latch resets
only on WORK LOAD. So after any window resize the reader silently fell back to
the live engine until the work was reloaded, losing the pinned grid and with
it the chapter-at-top rule baked into it. The resize tick now clears the latch
and regenerates when the drop leaves nothing pinned (guarded on "actually
dropped", so a no-op resize never triggers a full-document walk on a
7300-line novel).

This also unblocked HEADLESS verification of every table-mode prose bug: under
cage the `wlr-randr` resize lands after the app maps, so the first table is
built at 720p and dropped — with no regeneration the run had no table at all
and table-mode bugs could not reproduce. Tell: `PAGES_PROSE: dropped table
(layout changed)` followed by `no table for …` and never a `generated`.

**Guard.** `validate_prose_pages` gained a `chapter` invariant (2026-07-27):
every `chapter_start` buffer line must begin a page at offset 0. Previously
the suite checked only geometry, so a regression in `chapter_clamp` — or a
table generated before `mark_chapter_starts` ran — would strand headings
mid-page with every invariant still green. Works with no chapter data (PP,
TTC: `chapter_start=0` on every line) pass vacuously.

**pv4: a row whose INK fits stays on its page (row-fit correction,
2026-07-09).** `prose_next_boundary` snaps the raw fill boundary (`y0 +
usable`) DOWN to the nearest display-row top. When that raw pixel lands in
the ink-free gap AFTER a row's bottom (inter-paragraph spacing), the snap
put the boundary at that row's TOP — assigning a fully-fitting row to the
NEXT page. But the live bottom clip (`bottom_clip_height`) admits any row
whose ink bottom fits the budget, so the reader SAW the row on the current
page while the grid disagreed: the sync turn fired one visible row early and
the next page re-showed a row already read (BH "at a loss how to receive it.
I hinted that the climate—"). Fix: `next_row_top_if_row_fits` (scroll.rs)
advances the boundary to the next row's top when the snapped row's ink
bottom is within the raw budget, bounded by `prose_fit_slack` (paragraph
spacing + wrap spacing + rounding); `validate_prose_pages` tolerates the
same slack in its fit check, since the box-space overshoot is trailing
whitespace, never ink. The grid and the clip now agree on which rows a page
shows. Boundary meaning changed → `pv3 → pv4`.

**Testing trap: geometry luck.** A 720p headless run can land the same page the
grid demands while a 1920×1200 session lands off-grid — a heading-at-top
screenshot at cage geometry does NOT verify the landing path. Verify by
querying the stored rows for the LIVE fingerprint (`prose_pages` where
`layout_fingerprint` matches the session's `PAGES_PROSE: … fp=` log line) and
asserting the landing equals that page's `(start_line_id, start_row_offset)`.

Page boundaries are computed by `next_page_top()` in `viewport.rs`, which:

1. Calls `last_fully_visible_line(state, top)` to find where the current page
   ends (pixel-height walk with descender guard, trimmed by
   `trim_visible_range`)
2. Finds the last dialogue line on the visible page via `last_dialogue_in_page`
3. Finds the next dialogue after that via `next_dialogue_from`
4. Backs up over speakers/stage-directions/scene-headers via
   `back_up_for_speaker` to get the new page top

Key files:

- `src/input/navigation.rs` — `page_forward`, `page_backward`,
  `page_backward_bottom`, all jump functions
- `src/input/viewport.rs` — `next_page_top`, `prev_page_top`,
  `last_fully_visible_line`, `visible_range`, `trim_visible_range`,
  `clamp_at_section_break`, `section_break_fn`, `back_up_for_speaker`
  (+ `_state` wrappers), `is_dialogue_line`, `is_inside_stage_direction`
- `src/text_file_map.rs` — `build_line_map`, `build_section_starts`
  (the `(div1,div2)` boundary bitmap), `LineMap.section_starts`
- `src/app.rs` — `AppState::is_section_start` / `section_starts` (read the bitmap)
- `src/input/scroll.rs` — `set_page`, `set_page_instant`, `snap_scroll_to_line`
- `src/db/line_types.rs` — `is_dialogue`, `is_stage_direction`, `is_speaker`,
  `is_act_scene_marker`, `is_separator` (text classifiers — for the line-map
  build and the mid-load pagination FALLBACK only; not the boundary source of
  truth, see *The authoritative-boundary principle*)
- `src/db/models.rs` — `Line.div1` / `div2` / `line_in_div` (the authoritative
  per-line act/scene metadata)

### A landing that drops out of table mode (2026-07-27)

**Tell.** After some jump, the reader shows the target line pinned to the TOP
of the card instead of sitting mid-page where the grid puts it. The log switches
engines at that moment:

```
BOTTOM_CLIP_EXACT:   page_top=42 top_off=603   <- before (pinned table)
BOTTOM_CLIP_ROWFILL: page_top=47               <- after  (live row-fill)
```

`PAINT: first frame for page_top=<the CURSOR line>` is the giveaway — a stored
page top is rarely the line you jumped to.

**Arriving from a clipping complaint?** This is clip-prevention.md #20, and the
distinguishing test is there: if the page top IS a stored `start_line`, you want
#19 (the clip ignored the stored end) — if it is NOT, you are in the right
place. Note this class fires NO `CLIP_WARN`, so an empty grep clears nothing.

**Root cause.** `canonical_page_top_for` consulted only the PLAY table
(`active_page_table`). On prose that returns `None`, so it fell through to the
live geometric walk, which disagrees with the pinned `prose_pages` grid. It also
returned a bare `usize`, but prose page tops are `(line, row-offset px)` PAIRS —
so even the right line still mis-framed by the offset (603px here).

Two call sites had already worked around this LOCALLY rather than fixing the
helper (`search.rs snap_match_to_prose_grid`, `chapter_jump_land_ereader`).
`jump_to_line` was the third that needed it and never got one — which is how the
journal picker's Escape source-jump landed off-grid.

**Fix.** `canonical_page_top_offset_for` (`navigation.rs`) is the single choke
point: prose table → play table → live walk, returning `(top, offset)`. Callers
on a prose grid MUST use it and pass the offset to `set_page_instant_offset`.
`canonical_page_top_for` remains a wrapper for the play/live callers, where the
offset is always 0.

`search.rs` is deliberately NOT collapsed into it: that path anchors to the
MATCH'S OWN wrapped row, not the line's first row, so a match on a later row is
not hidden under the bottom clip. Different rule, not duplication.

#### Audit of every close-to-reader path (2026-07-27)

All overlay Escape paths were swept after the fix. **Every overlay close is
correct** — the remaining off-grid landings are elsewhere. Three shapes exist;
knowing which one a path uses tells you immediately whether it can go off-grid:

- **(A) restore** — `restore_saved_position_resnap` (`app/mod.rs`). Safe: it
  sets the saved triple then calls BOTH resnaps. Used by the overlay cycle and
  by every "peek-and-Escape" branch.
- **(B) jump** — `navigation::jump_to_line`. Safe since this fix (it reads
  `canonical_page_top_offset_for`). Used by the gloss/journal source-jumps.
- **(C) neither** — sets `page_top_line`/scroll by other means, or only calls
  `return_to_reader_mode`. **`return_to_reader_mode` does NOT restore position**
  — it only sets `input_mode`, `last_overlay`, gloss tint, and the cursor flash.
  A (C) path is a BUG only if that surface actually MOVED the reader.

Verified safe (A/B): gloss overlay, journal overlay, overlay cycle (`\`),
journal→synopsis return, bookmark-picker confirm.

Verified safe because the surface NEVER MOVES THE READER — a bare
`return_to_reader_mode` is correct here, do not "fix" these:

- **Synopsis overlay.** `scene_synopsis.rs` contains no assignment to
  `current_line`/`page_top_line` at all; it has no return-position field
  because it needs none.
- **All pickers** (journal, recent-Q&A, gloss, library, term input). Display-
  only while open. `journal.return_pos` IS captured on picker open and dropped
  unused on Escape — harmless, since nothing moved; it exists for the CONFIRM
  path, which reveals the overlay.
- **Chat panel, echo Escape, SegmentVim, vocab_add.** No position surface.

**Genuinely off-grid, and NOT on any Escape path — ALL FIXED 2026-07-27** (they
predated the `canonical_page_top_offset_for` fix; a follow-on branch closed
them, spec `2026-07-27-cross-work-landing-grid-design.md`):

1. **`display_work_at_with_prepared` target branch** (`app/mod.rs`) did
   `state.page_top_line = buf_idx` — the target line forced as the page top,
   no offset, no table consult. Shared root of every CROSS-WORK jump landing:
   concordance across works, echo source jumps, and
   `toggle_previous_work`/`load_work_at` with a target line.
   **The trap when fixing it: the page tables are not loaded yet.**
   `page_table::load_for_work` / `prose_pages::load_for_prose_work` run ~40
   lines BELOW that assignment, so the branch itself cannot read the grid. The
   snap therefore lives AFTER those loads and before
   `update_highlight_and_show`, gated on `target_line_id.is_some()`.
   **Companion requirement:** `update_highlight_and_show` scrolled to
   `line_yrange(scroll_to).0` with no offset term and would have discarded the
   snap; it now adds `page_top_offset` (`+ 0` for every other caller).
2. **`update_highlight_and_center`** (`highlight.rs`) computed
   `current_line - lpp/2` then `set_page_instant`. It now reads
   `prose_table_boundary_for_line` first and centres only as a fallback.
   Reached by the concordance same-work landing, `jump_to_line_mapping_id`, the
   echo jump, `nav_test.rs`, `phrase_highlight.rs`, and the AB-loop clear.
   - **Deliberate behaviour change:** on a pinned prose work a jump no longer
     centres the cursor — it lands where the stored page puts it, matching
     page-turning. Plays and live-engine prose still centre exactly as before.
   - It reads `prose_table_boundary_for_line`, NOT the full
     `canonical_page_top_offset_for`, on purpose: plays were never part of this
     defect, and routing them through the play table would change their
     landing too.
   - Proven by A/B (concordance `equanimity` on BH-Barrett, identical start
     `page_top=38`): baseline landed `page_top=45` / `BOTTOM_CLIP_ROWFILL`
     (the `47 - 4/2` guess), fixed lands `page_top=42 top_off=603` /
     `BOTTOM_CLIP_EXACT`.
3. **`hide_translations`** (`app/translations.rs`) never assigned
   `page_top_offset` after remapping line numbers, and anchored the
   single-column path by a raw pixel `adj.set_value(...)`. It now zeroes the
   stale offset at the remap and lands on the cursor's stored page when a prose
   grid is active.
   - It lands EXPLICITLY via `set_page_instant_offset` rather than calling
     `resnap_prose_to_table`: the remapped `(line, 0)` pair can coincidentally
     satisfy resnap's already-on-grid check and no-op, leaving the reader on a
     real boundary that is not the one holding the cursor.
   - The two-column branch is deliberately untouched — it defers its entire
     re-snap to RESIZE_TICK because the left view still has its single-column
     width (see the comment there).
   - **Latent, not live:** all 43 works with `line_translations` rows are plays
     and ZERO have a `prose_pages` table, so this path cannot fire today. Fixed
     as a trap. Confirm before assuming otherwise:

```sql
SELECT DISTINCT lm.work_abbrev FROM line_translations lt
JOIN line_mapping lm ON lm.id = lt.line_mapping_id
WHERE EXISTS (SELECT 1 FROM prose_pages pp
              WHERE pp.work_abbrev = lm.work_abbrev);
```

**Same defect, second site.** `display_work` recomputed the resume page as
`current_line - 1` — off-grid by construction. Every launch opened mid-page and
depended on `resnap_prose_to_table` to fix it; in one reproduction the corrected
frame did not paint for 23.6s, leaving the WRONG page on screen. Startup now
reads the table directly. Note the resnap is KEPT as defence-in-depth (it still
catches positions predating a table regeneration) — but it should no longer fire
on a clean launch. `PAGES_PROSE: resnap off-grid` at startup now means something
upstream still guessed.

**Still latent.** `is_line_fully_visible` (`viewport.rs`) is also prose-table-
blind on the single-column path — it reads the geometric `last_visible_range`
cache. It gates `jump_to_line`'s early return, so a jump to a line that is
visually on-screen but belongs to a DIFFERENT stored page skips the snap. Not
observed in the wild yet; fix it there if a jump ever "does nothing" on prose.

## page_back_stack rules

Every function that changes `page_top_line` must interact with the stack:

- **page_forward (`x`)** — pushes old `page_top_line` before turning
- **page_backward (`y`)** — pops; falls back to `prev_page_top()` when empty
- **page_backward_bottom (Shift+comma)** — pops (same as `page_backward`)
- **Structural jumps (gg, G, `[`, `{`, bookmarks, vocab, zt)** — clear the stack
  then push current `page_top_line` as a single return entry. `y` after such a
  jump returns to the page the user was on when they jumped; a second `y` has an
  empty stack and falls through to `prev_page_top()`
- **Scene jumps (2, 3)** — clear the stack but do NOT push. A scene jump can skip
  many pages, so `y` should page back one viewport into the skipped content via
  `prev_page_top()`, not teleport to the jump origin (see `jump_to_next_scene` /
  `jump_to_prev_scene`)
- **Line-by-line dialogue navigation (comma, q, j, k)** — no stack interaction;
  incidental page turns from `scroll_after_jump_forward/backward` don't touch the
  stack. These follow a plain reading-order model (see *Dialogue navigation
  reading model* below) and do NOT scene-snap — scene snapping is the 2/3 jumps'
  job
- **Search jumps (`/`, `n`, `N`)** — push current `page_top_line` (with dedup)
  before `update_highlight_and_center`. This means `y` after dismissing a
  search with Escape returns to the pre-search page. The dedup avoids
  polluting the stack when `execute_search` fires on every keystroke during
  live-search (only pushes if the top of stack differs from current
  `page_top_line`)
- **MPV sync (scroll_paragraph_to_top, highlight auto-advance)** — no stack
  interaction; system-driven, not user navigation

If a new navigation function is added that calls `set_page` or
`set_page_instant`, it must either push/pop/clear `page_back_stack` or
document why it doesn't.

## Debugging page-forward stuck states

### Symptom

Pressing `x` doesn't advance, or advances by only a few lines then gets
stuck oscillating between two nearby page tops.

### Debug log entries

`page_forward` already logs at the `PAGE_FWD:` prefix:

```
PAGE_FWD: page_top=177 new_top=177 next_dialogue=185 line_count=4548
PAGE_FWD: candidate_top=185 effective_top=185 (from new_top=177)
```

Check:

- **`new_top <= page_top`** — means `back_up_for_speaker(next_dialogue)` pulled
  the top behind a section break. The fallback sets `candidate_top =
  next_dialogue`. If this happens repeatedly from the same page_top, the
  section-break clamping is too aggressive.
- **`next_dialogue` never advancing** — the dialogue classifier is
  misidentifying a non-dialogue line as dialogue. Check multi-line stage
  directions (see below).
- **`effective_top <= page_top`** — `clamp_page_top_to_scroll_ceiling` hit the
  GTK scroll ceiling; falls through to `jump_to_end`.

### Adding detailed diagnostics

To trace the full page-boundary computation, add temporary logging inside
`next_page_top` in `viewport.rs`:

```rust
pub(crate) fn next_page_top(state: &AppState, top: usize) -> NextPage {
    let line_count = state.effective_line_count();
    // ... existing early returns ...
    let last_visible = last_fully_visible_line(state, top);
    let last = last_dialogue_in_page(&state.buffer, top, last_visible.saturating_sub(top) + 1, line_count);
    let next_dialogue = next_dialogue_from(&state.buffer, last + 1, line_count);
    crate::log_fmt!("NEXT_PAGE_TOP: top={} last_visible={} last_dialogue={} next_dialogue={}",
                    top, last_visible, last, next_dialogue);
    // ... rest of function ...
}
```

To trace section-break clamping, add inside `clamp_at_section_break`:

```rust
crate::log_fmt!("SECTION_CLAMP: page_top={} break_line={} clamped_last={} orig_last_fit={}",
                page_top, break_line, clamped_last, range.last_fit);
```

Remove after diagnosing — these fire on every page turn and duplicate work.

## Page-turn animation lock

`set_page` in `scroll.rs` acquires `page_turn_lock` for the duration of the
crossfade/slide animation (700ms for crossfade). While the lock is held,
subsequent `set_page` calls return early without updating `page_top_line`.

`page_forward`, `page_backward`, and `page_backward_bottom` all check
`page_turn_lock.is_locked()` at the top and return early if held. This
prevents stack/cursor mutations from running when the page turn would be
silently dropped by `set_page`.

Without this guard, pressing `y` during a crossfade would pop an entry from
`page_back_stack` and update `current_line`, but `set_page` would discard the
turn — the stack entry is consumed and lost, causing the next `y` to skip a
page.

The same applies to `page_forward`: pressing `x` during a crossfade would
push a stale `page_top_line` onto the stack and update `current_line` without
the page actually turning.

Rule: any function that modifies `page_back_stack` or `current_line` before
calling `set_page` must guard against `page_turn_lock` first.

## Debugging page-backward wrong destination

### Symptom

Pressing `y` after `x` doesn't return to the previous page — it jumps much
further back or to an unrelated position.

### Diagnosis

1. Check the log for `PAGE_BWD: stack pop` vs `PAGE_BWD: prev_page_top`.
   Stack pop means the back-stack had an entry; `prev_page_top` means it
   was empty and had to recompute.
2. **How `prev_page_top` works now:** it walks the forward chain from line 0
   using the SAME boundary the renderer uses — `column_split(probe).next_page_top`
   in two-column mode (not the single-column `next_page_top()` helper, which
   diverges by a speaker block and caused a persistent 3-line `y GAP`). It
   returns the page `probe` whose forward boundary hits `current_top` exactly
   (`next == current_top` → perfect tile), or the last boundary that does not
   overshoot. It returns that boundary VERBATIM — never a
   `back_up_for_speaker(next_dialogue_from(...))` re-derivation, which shifts off
   the boundary and re-creates a gap.
3. **If it still gaps/overlaps, suspect `current_top` itself.** It may not be on
   the `column_split` chain at all: a scene jump (`2`/`3`) lands at a scene
   heading, and the forward-pulled final spread (`last_page_top`) sits off the
   chain. Then no boundary tiles exactly. For the final spread that seam is
   benign (exempt). For a scene start it should tile — check that `column_split`
   ends the previous page at the scene boundary (the "right column begins a new
   scene" rule under *Section-break clamping*).
4. Check whether the navigation that preceded `y` pushed to or cleared the
   stack. If a jump function forgot to clear, the stack has stale entries.

### Common causes

- **New jump function doesn't clear the stack** — add
  `state.page_back_stack.clear()` before its `set_page`/`set_page_instant`
  call.
- **`current_top` not a `column_split` boundary** — a scene jump or the
  forward-pulled final spread produced a `page_top` the forward chain skips. The
  fix is to make `column_split` produce a boundary there (scene-ends-in-left
  rule), or to exempt the genuinely un-tileable final spread, NOT to fudge
  `prev_page_top`.

## Multi-line stage directions

Folger-cleaned Shakespeare texts have multi-line stage directions:

```
[Enter the King of England, Humphrey Duke of
Gloucester, Bedford, Clarence, Warwick, Westmoreland,
and Exeter, with other Attendants.]
```

`is_stage_direction` in `line_types.rs` recognizes:

- Single-line: `^\[.*\]$`
- Multi-line opener: starts with `[`, no closing `]`
- Multi-line closer: ends with `]`, no opening `[`

Continuation lines ("Gloucester, Bedford...") are detected by
`is_inside_stage_direction` in `viewport.rs`, which scans backward up to
20 lines looking for an unclosed `[` opener. This function is used by
`is_dialogue_line`, `next_dialogue_from`, `last_dialogue_in_page`, and
`back_up_for_speaker` to ensure multi-line stage directions are never
treated as dialogue.

If a new multi-line pattern appears that isn't caught, `next_dialogue_from`
will return one of its lines as "the next dialogue", and
`back_up_for_speaker` may pull the page top behind a section break,
creating a stuck loop.

## Section-break clamping

> **Boundaries are AUTHORITATIVE, not inferred.** A scene/section boundary is
> exactly where a line's `(div1, div2)` changes — that is unambiguous in the DB.
> At load, `build_line_map` precomputes a `LineMap.section_starts: Vec<bool>`
> bitmap (one bit per buffer line, `true` on the FIRST line of each new
> `(div1,div2)` run) and every pagination decision below reads it via the
> `is_section_start(line)` predicate / `section_break_fn` closure. Do **not**
> re-derive a boundary from buffer text (`is_act_scene_marker` / `is_separator`)
> in pagination code — see *The authoritative-boundary principle* at the top of
> this file. (Those text checks survive only as a mid-load FALLBACK inside the
> helpers, used before the line map exists.)

A new ACT/SCENE must start a fresh spread, never appear mid-column. `column_split`
enforces this in three places, all driven by the `section_starts` bitmap:

- **Left column:** `clamp_at_section_break` scans `(page_top, left.last_fit]` for
  the first line where `is_section_start` is true and clamps `last_fit` to the
  line before it. A page that STARTS at a boundary (`is_section_start(page_top)`)
  never self-clamps because the scan begins at `page_top + 1` — the boundary line
  is the page's own opening heading. (No more text-based "header-block skip":
  with an authoritative single-line boundary there is nothing to bridge across,
  which is what eliminated the AWW `y GAP` where the old header-skip ran straight
  through an `ACT 2` marker hidden behind a `[They exit.]`.)
- **Right column would BEGIN a new scene:** if skipping leading blanks/exits from
  `split` lands on a boundary line (`is_section_start(hi)`), the previous scene
  ended in the left column. `column_split` ends the page there: `page_end` before
  the boundary, `next_page_top` = the boundary, right column empty. The new scene
  starts the next spread, so `y` from it tiles into this page exactly
  (`column_split(prev).next_page_top == scene_top`).
  - **First-spread exception (short opening section → right column).** When this
    case fires on the VERY FIRST spread (`page_top == 0`) and the whole opening
    section `[0, hi)` fits the right view's height, `column_split` instead returns
    `split = 0` so the section renders in the RIGHT column with an EMPTY left —
    the start-of-document mirror of the EPILOGUE final-spread rule. `next_page_top`
    stays `hi` (identical to the empty-right branch), so tiling is unchanged: `y`
    from Act 1's spread still tiles back here exactly. `update_bottom_clip` treats
    `exact_end == 0` (`end <= page_top`) as an empty column and clips the left
    view's full height. Verify visually (H8) — a rendered-spread criterion; the
    `FIRST_SPREAD_SPLIT split=0 …` log line (under `LIT_HEADLESS_TEST`, asserted by
    `tests/startup_column_layout.rs`) confirms the rule fired.
- **Right column interior:** `clamp_at_section_break` again, so a boundary partway
  down the right column starts the next spread.

**The one exemption: the work's FINAL trailing section** (an EPILOGUE, e.g. AWW's
`div1=6, div2=0`). It has nowhere to be pushed and `last_page_top`/`G` expect it
to fill the right column, so when clamping would empty the right column AND the
unclamped range already reaches the work's end, `column_split` keeps it
unclamped. Detected by "no further `is_section_start` after this one".

**Non-dialogue tail skip (the AWW Scene-1→2 underfill).** A scene's last spread
ends on its last *dialogue* line; the trailing `[They exit.]` / blank lines the
trim drops are NOT a page of their own. `column_split` therefore advances
`next_page_top` past a pure non-dialogue tail to the next real page top
(`back_up_for_speaker` of the next dialogue). Without this, `prev_page_top` would
tile a tiny dialogue-less spread on the way back (a 2-line UNBALANCED spread).

Edge case: when the boundary is very close to `page_top` (1-2 lines), the clamped
page is trivially small. `next_page_top` then computes a `next_dialogue` whose
`back_up_for_speaker` pulls back behind the break, producing `new_top <=
page_top` (no progress). `page_forward` handles this with the fallback
`candidate_top = next_dialogue`.

## Two-column right-column positioning

In two-column mode the right column is a separate `right_view` sharing the
buffer; `snap_scroll_to_line` (`scroll.rs`) scrolls it so `cs.split` (from
`column_split`) sits at its top, and `update_bottom_clip`'s `exact_end` path
clips it at `cs.page_end`. Two failure modes, both worst near the document end:

**Right column duplicates the left / shows the buffer start.** The right
view's `set_value` to `cs.split` clamped low because the right view's `upper`
was too small — either layout wasn't settled yet (stale `upper`) or the right
view had no bottom-margin headroom for a near-the-end split. Fixes:
`ensure_scroll_range` now extends the **right** view's bottom margin too (it
used to extend only the left `text_view`), `snap_scroll_to_line` calls
`ensure_scroll_range` before scrolling the right view, and the right-view scroll
runs synchronously **and** on an idle + 100ms backstop (`scroll_right_view_to_split`)
so a stale-`upper` first pass is corrected post-layout. If the right column
still shows line 0, check the right view's `upper` vs `cs.split`'s y.

**Right column unscrolled on first paint.** The startup resize-tick reveal calls
`snap_scroll_to_line` (which positions both columns), but the 500ms-grace and
5s-stuck-fallback reveals only set opacity. If the resize tick never fires (e.g.
its two-column width-settling guard never passes — common in a headless cage),
the fallback reveals the window with the right column at line 0. Both fallbacks
now call `reveal_snap` (`ensure_scroll_range` + `snap_scroll_to_line`) before
`set_opacity(1.0)`.

**Page-forward stuck on the final spread.** `scene_snap_top` may return a scene
start (`cs.split`) that sits past the scroll ceiling; `set_page` then clamps it
back below `page_top_line`, so the view never advances and `x` oscillates
(`scene-snap page_top=N -> new_top=M` repeating with the same N). `page_forward`
now clamps the snap target with `clamp_page_top_to_scroll_ceiling` and only
takes the snap when it yields real forward progress (`clamped > page_top_line`);
otherwise it falls through to the normal path, which recognizes end-of-document
(`next_dialogue >= line_count`) and stops cleanly.

Key files: `src/input/scroll.rs` (`snap_scroll_to_line`,
`scroll_right_view_to_split`, `ensure_scroll_range`), `src/app.rs`
(`reveal_snap` and the 500ms/5s reveal timeouts, resize-tick `do_reveal`),
`src/input/navigation.rs` (`page_forward` scene-snap guard, `scene_snap_top`,
`jump_to_end`), `src/input/viewport.rs` (`column_split`).

### "Empty right column" is NOT just `page_end < split`

When a spread ENDS at a scene break, `column_split` takes the scene-break branch
(`viewport.rs`, the `at_break && !is_final_section` return). It leaves the
scene's trailing exit/blank lines in the right range `[split, page_end]` — which
the bottom-clip then hides — and sets `next_page_top` to the next scene's marker.
The key subtlety: when a SINGLE trailing line sits at the split, that branch
returns `page_end == split` (not `< split`). The right column is then VISUALLY
empty (its one line is a clipped `[They exit.]` / blank), yet the common
`page_end < split` test reports it as NON-empty.

Observed on H8 1.3 (`split=804 page_end=804 next_page_top=805`): the right
column shows nothing, but `page_end < split` is false. The robust test for
"right column is visually empty" is the one the next-scene watermark uses
(`update_next_scene_watermark` in `scroll.rs`): authoritatively, **`next_page_top`
is a DB section start (`is_section_start`) AND the right range carries no
dialogue (`is_dialogue_line`)** — with `page_end < split` kept as the sufficient
condition for the strict lone-tail geometry, and `next_page_top < line_count`
excluding end-of-work and the empty-LEFT first-spread mirror.

**`would_empty_right_column` (`viewport.rs`) carries the same too-strict test**
(`cs.split >= line_count || cs.page_end < cs.split`). It works for its current
callers (the lone-EPILOGUE geometry it guards has `split >= line_count`), but if
`G` / `jump_to_end` / `page_forward`'s final-spread guard ever mis-tiles at a
scene boundary whose new scene opens after exactly one trailing exit line, this
is the root cause: `would_empty_right_column` returns `false` for a spread whose
right column is in fact empty. The fix would mirror the watermark's predicate
(section-start + no-dialogue), not loosen `page_end < split` to `<=` (which would
mis-flag genuine one-line right columns).

## Testing

Two layers: headless tests verify the page-turn algorithm across many works
cheaply (no display server needed); the in-app test harness verifies
integration with real GTK pixel layout on the current work.

**Clip invariant (pixel-level, e2e under cage):** `tests/line_clipping.rs` asserts
the MAIN reading card never clips its first/last line (top/mid/end), driven by
the `TEST_VIEWPORT_RECT` the app logs on reveal. `tests/overlay_clipping.rs`
extends the same invariant to the synopsis OVERLAY (opens it with `h`, scrolls to
the bottom with `j`), driven by `TEST_OVERLAY_VIEWPORT_RECT`. Both are `#[ignore]`d;
run via `./scripts/e2e-env.sh cargo test --test line_clipping --test overlay_clipping
-- --ignored --nocapture`.

### Headless tests

`src/input/navigation.rs` has headless tests that simulate page turning
using text-only line counts (no GTK). They approximate
`last_fully_visible_line` with a fixed `page_size = 30` lines and simulate
`clamp_at_section_break`, `back_up_for_speaker`, and the page_back_stack.

Algorithm tests (all Shakespeare plays, ~38 works):

- `test_page_forward_all_shakespeare_no_stuck` — every page turn advances
  (no stuck states)
- `test_page_forward_backward_roundtrip_all_shakespeare` — forward tops
  strictly increasing; backward via history round-trips exactly
- `test_x_y_roundtrip_with_clamping_all_shakespeare` — same as above but
  with section-break clamping simulation
- `test_x_page_forward_no_mid_page_scene_breaks_all_shakespeare` — no
  scene marker or separator in the interior of any page (after the opening
  header block)
- `test_y_after_scene_jump_returns_to_origin_all_shakespeare` — y after a
  scene jump (3) returns to the exact jump origin
- `test_scene_synopsis_identification_all_shakespeare` — scene markers
  resolve to correct synopsis keys via the database

Single-work tests:

- `test_page_forward_no_gaps_or_repeats` — Troilus: every highlighted line
  is dialogue, strictly increasing, gaps bounded by page_size
- `test_x_page_forward_covers_every_line_errors` — Comedy of Errors: every
  non-blank line appears in at least one visited viewport
- `test_j_cursor_next_dialogue_covers_every_line_errors` — same coverage
  via j/q cursor navigation
- `test_y_after_chapter_jump_returns_to_origin` — Troilus: y after [/{
  returns to jump origin
- `test_x_x_x_scene_jump_y_y_sequence` — Troilus: x x x 3 y returns to
  pre-jump page; second y has empty stack
- `test_chained_scene_jumps_only_last_origin_survives` — Troilus: 3 3 y
  returns to page between the two jumps
- `test_page_forward_prose_bleak_house` — prose forward: every page turn
  advances to next non-blank line
- `test_page_backward_prose_bleak_house` — prose backward via history:
  exact round-trip

Run all page-turn tests:

```bash
cargo test -- page_turn
```

Run only the all-Shakespeare tests:

```bash
cargo test -- all_shakespeare
```

### In-app test harness (Ctrl+Shift+T)

Toggles a deterministic test mode on the currently loaded work with real
GTK layout. Calls the same navigation functions that key dispatch and
playback sync use. Press `gg` first to start from the beginning.

Three modes configured via `/configure-nav-test`:

- **sync-only** — pure playback sync simulation (1s per line advance,
  walks cursor line-by-line triggering page turns via
  `update_highlight_and_advance_page`). Best for catching scene breaks
  mid-page and viewport fill issues during sustained playback
- **jumps-only** — key-press navigation only (x, y, 2, 3, [, {, search
  jump at 300ms each). Tests forward progress, round-trip, structural jump
  return, and search-then-page-back return
- **full** — both interleaved: jump sequences at 300ms with 20-line sync
  runs at 1s each (~20s of simulated playback per run)

Six invariants checked after every step:

- **Forward progress on x** — page_top_line strictly increases
- **y round-trips x** — page_top_line returns to pre-x value
- **y after structural jump returns** — page_top_line returns to pre-jump
  value (also covers search jumps)
- **No scene break mid-page** — no marker/separator in the interior of the
  visible range
- **Viewport fill** — visible content fills at least 10% of viewport height
  (real pixel measurement)
- **current_line is dialogue** — cursor on a dialogue line (plays)

Toast shows "NAV TEST: running…" while active, "NAV TEST: done (N steps,
M fail)" on completion. All steps and failures logged with `NAV_TEST:`
prefix to the debug log. Runs up to 500 steps.

What the in-app harness tests that headless cannot: real GTK pixel heights,
actual line wrapping, real section-break clamping with pixel measurements,
viewport fill percentage, set_page/set_page_instant scroll plumbing,
page_turn_lock interaction with animation timing.

### Testing pinned play pagination (the three tiers)

(Formerly `testing-pinned-play-pagination.md`. Design:
`docs/superpowers/specs/2026-07-04-pinned-play-pagination-design.md`; plan:
`docs/superpowers/plans/2026-07-04-pinned-play-pagination.md`.)

Once pages are rows in lit.db, testing moves BELOW the GUI: the slow headless
fuzz becomes the last line of defense rather than the primary proof.

**Tier 1 — data-level audits (no app, no display).** Most of what the nav-fuzz
used to prove by driving the GUI for ~330 seconds per work becomes a data audit
running in seconds for every play at once: full coverage (every dialogue line in
exactly one page interval), monotone non-overlapping gap-free intervals, tail
reachability, sane boundaries (`left_start ≤ split ≤ end`; empty-right watermark
pages only where the next page opens a real `(div1,div2)` section), and row/meta
consistency. Read-only — never writes lit.db:

```bash
.claude/skills/validate-play-pages/validate-play-pages.sh --all
```

```bash
.claude/skills/validate-play-pages/validate-play-pages.sh MND
```

Run it after any lit.db re-import, font/layout change, or suspected drift. FAIL
means delete that work's rows (the script prints the command) and let the app
regenerate on next load. Staleness against the current text (`db_fingerprint`)
and the geometric fit/determinism invariants are enforced by the app itself at
load/generation time — a stale table logs `PAGES: fallback (...)` and is
replaced on the next load.

**Tier 2 — pure unit and property tests (no GTK).** Navigation over the table is
index arithmetic, so properties that historically only the fuzz could check are
millisecond `cargo test` targets in `src/input/page_table.rs`: the invariant
suite itself (`validate_spreads`), `page_for_line` binary search, fingerprint
composition (deterministic, sensitive to every layout input), and round-trip
properties (`x` then `y` returns to the same page; `G` is idempotent;
cursor-landing picks an on-page line).

```bash
cargo test --bin linux-lit page_table
```

**Tier 3 — headless e2e (the watchdog).** The cage + grim + wtype fuzz stays,
with a `PAGES: page N/M` assertion (must move exactly ±1 on `x`/`y`, or pin at
the first/last page) and two engine-selecting env flags:

```bash
LIT_GEN_PAGE_TABLE=1 ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work MND
```

```bash
LIT_NO_PAGE_TABLE=1 ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work MND
```

`LIT_GEN_PAGE_TABLE=1` forces generation at the current (headless) geometry so
the fuzz exercises the TABLE path; `LIT_NO_PAGE_TABLE=1` forces the LIVE engine
so the fallback path (font changes, translations, scroll mode, other machines)
keeps its own coverage. Pixel-level clip checks are unchanged
(`tests/line_clipping.rs`, `tests/overlay_clipping.rs`).

**What each tier catches:** unreachable tail, non-idempotent `G`, page
gaps/overlaps → Tier 1 (instantly, all plays) and Tier 2 (per property);
navigation landing/cursor rules, engine selection, fallback behaviour → Tier 2 +
Tier 3; rendering, clipping, focus/input, reveal timing → Tier 3 only (pixels).

Generated test tables carry a headless fingerprint key so they can never clobber
production rows. Clean up after a run:

```bash
sqlite3 ~/utono/litdb/data/lit.db "DELETE FROM play_pages WHERE layout_fingerprint LIKE '%|1280x720|%'; DELETE FROM play_pages_meta WHERE layout_fingerprint LIKE '%|1280x720|%';"
```

#### Headless runs at PRODUCTION geometry

Cage's virtual output defaults to **1280×720** (the wlroots headless backend's
built-in mode; cage has no size flag). A 720p run paginates differently than the
real session, and with page tables it exercises only the fallback engine (the
fingerprint won't match). Cage implements wlr-output-management, so resize the
output live (verified 2026-07-04):

```bash
wlr-randr --output HEADLESS-1 --custom-mode 1920x1236
```

Issue it after the cage launch, then give the app a few seconds to re-paginate,
whenever the criterion depends on production geometry: page-table
generation/consumption, spread boundaries, spread balance. **Use 1236, not
1200** — pagination keys on the TEXT VIEW height, and only 1236 reproduces
production's `text_view.height = 1098` (the repo CLAUDE.md has the full
rationale; 1200 gives 1062, a 36px miss that changes the page grid and can hide
a bug entirely).

**The font is a fingerprint input too.** Matching the geometry is only half of
matching production — see *Testing a font-related pagination change* above.

## Playback sync

Playback sync advances the cursor to match MPV audio position. The pipeline:

1. **MPV emits `time-pos`** — the IPC listener in `mpv/client.rs` parses the
   JSON property-change event and extracts the current playback position (seconds)
2. **`find_line_for_time`** — binary search (`partition_point`) over sorted
   `(line_id, start, end)` timestamps to find which line contains
   `time_pos + SYNC_PREROLL` (currently 0.0s). Emits `MpvEvent::CursorSync(work_line_index)`
3. **CursorSync handler** (`main.rs`) — translates work-line index to
   buffer-line index via `line_map` (if present), then:
   - Skips if `sync_enabled` is false, work is loading, search is active,
     chunk mode is active, or `suppress_sync_until` hasn't elapsed
   - Guards against aberrant timestamps (>50 lines from current position)
   - Guards against `pending_advance_ignore_bl` pulling cursor backward
4. **Scene transition** (plays only) — compares the new line's `(div1, div2)`
   against `current_sync_scene`. On scene change, computes the header-block
   top via `back_up_for_speaker` and snaps the viewport with
   `set_page_instant` (unless the page top is already correct). Skips
   paragraph scroll when a scene scroll fired
5. **Paragraph transition** — calls `current_paragraph_range()` to detect
   whether the cursor crossed into a new paragraph (contiguous non-blank
   lines). If so, calls `scroll_paragraph_to_top()` which in e-reader mode
   page-turns so the paragraph start is at the viewport top (only if
   off-screen and `para_start >= page_top_line` — never scrolls backward to
   a paragraph that started on a previous page). Skipped when a scene scroll
   already happened
6. **`update_highlight_and_advance_page`** — applies highlight tags, then
   checks if `current_line > last_raw_visible_line` (the untrimmed last
   visible line — not `last_fully_visible_line`, which trims trailing
   speakers/blanks for pagination and would cause premature turns). If so,
   computes the landing top and calls `set_page` with forward direction. This
   is how playback sync triggers page turns. In table mode the landing top
   comes from `page_table::table_top_for` (the stored page containing the
   spoken line — see *Pinned page tables*); otherwise, and as the fallback
   when no table serves the line, `page_turn_top(current_line)` with the
   final-region redirect
7. **`after_page_change(MpvSync)`** — runs post-page-turn housekeeping. Does
   not seek MPV (sync-driven, not user-initiated)

### Pending advance (pending_advance)

Scheduled when the current timestamped line ends and the next dialogue line
has no timestamp. Scene boundaries are NOT handled here — CursorSync's
scene-transition detection (step 4 above) picks up scene changes naturally
when `find_line_for_time` lands on a line in the new scene.

- `pending_advance = Some((end_time, next_buffer_line, source_work_index))`
- On each `TimePos` event, if `pos >= end_time`: advance cursor directly,
  set `pending_advance_ignore_bl` to prevent CursorSync from pulling back

When a manually-set timestamp has no valid end time (`end <= start`), the
fallback `end_time` is: the next timestamped line's `start - 0.2s` (clamped
to at least `start`), or `start + 5.0s` if no next timestamp exists.

### Prose straddling paragraph: scheduled phrase-boundary turn (pending_prose_cross)

The whole-line rule (step 6) cannot turn a page whose boundary falls INSIDE
the spoken paragraph — the cursor stays on the straddling line, `current >
last_vis` never fires, and the karaoke tint runs off the bottom of the page.
The CursorSync handler (main.rs) therefore schedules a TimePos-driven turn
whenever the cursor's paragraph is the stored prose page's `end_line` and a
next page exists:

- `pending_prose_cross = Some((fire_at, next_page_idx))`; on each `TimePos`
  event, `pos >= fire_at` turns to the stored next page top via
  `set_page_instant_offset` (cursor unchanged — only the window advances)
- `fire_at` comes from `prose_cross_time` (navigation.rs): the page-boundary
  pixel offset is converted to a char offset by walking the straddling
  line's real display rows (`display_row_char_at`; uniform pixel-fraction is
  the fallback), then `phrase_crossing_time` (db/queries.rs) resolves the
  first `phrase_timestamps` span whose `end_char` extends past that offset
- **The fire time is ALWAYS that phrase's `start_time`** (2026-07-09): the
  page turns the moment the first word of a phrase that continues onto the
  next page is highlighted, so its continuation is readable as it is
  narrated. This holds for a phrase that STRADDLES the boundary too — its
  on-page head (often one turned-under word, BH "…behind the door, | where")
  is knowingly cut by the turn. The earlier mid-phrase char-fraction
  interpolation turned only when narration reached the boundary character,
  which parked the tint on the old page while the phrase ran off-screen
- Degenerate boundary (`crossing time <= line start`) → do not schedule; the
  normal whole-line advance handles it. Fire time already past the current
  position → turn immediately (waiting for the next TimePos lands ~1.5s late)
- No `phrase_timestamps` rows for the (line, media) pair → fall back to
  whole-line char-fraction interpolation across the line's audio window

Log prefix: `SYNC_PROSE_CROSS:` (scheduled / fired / fired-immediately /
skip degenerate / phrase hit vs interpolate). Cleared by explicit seek paths
(search, concordance), work/font changes, and non-prose works.

### Suppression

Manual navigation (comma, q, j, k) sets `suppress_sync_until` to a future
`Instant`, preventing CursorSync from overriding the user's position for a
brief window.

### SetTimestamps dialogue filter

`SetTimestamps` — the timestamp data sent to the MPV client for
`find_line_for_time` — is filtered to include only `is_dialogue` lines.
This prevents `CursorSync` from landing on stage directions, speaker names,
or other non-dialogue lines. The filter is applied at all three build sites:

- `app.rs` `display_work_at_with_prepared` — primary load path
- `app.rs` MPV discovery callback — when switching active `media_id`
- `timestamps.rs` `resync_mpv_timestamps` — after manual timestamp edits

### Always-on logging

These log prefixes are written regardless of debug mode (`Ctrl+d`):

- `CURSOR_SYNC:` — every sync event that changes `current_line`
- `SYNC_ADVANCE:` — the page-turn decision point
- `SYNC_PAGE_TURN:` — confirms a sync-driven page turn
- `SYNC_SCENE_SCROLL:` — scene transition snap
- `PAGE_TURN:` — every `set_page` call (sync and navigation)

Additional detail (`CURSOR_LINE:`, `SEEK:`, `CURSOR_SYNC: SUPPRESSED`)
requires debug mode.

Key files: `src/mpv/client.rs` (TimePos parsing, `find_line_for_time`),
`src/main.rs` (CursorSync + TimePos handlers),
`src/input/highlight.rs` (`update_highlight_and_advance_page`)

## Scenes

Scenes are encoded in the database via `div1` (act) and `div2` (scene)
fields on each line. `line_in_div` gives the line's position within its
scene. These are loaded in `db/queries.rs` and stored on each `Line` struct.

### Scene markers in the text buffer (display chrome, NOT the boundary source)

Act/scene markers are lines like `ACT 1`, `SCENE 2`, `## Act 3, Scene 1`,
`PROLOGUE`, `EPILOGUE`, or `INDUCTION`. `line_types::is_act_scene_marker()`
(strips optional `## ` prefix, uppercases, checks keyword prefixes) and
`is_separator()` (`=====`) detect them. **These are synthesized display chrome.**
The authoritative scene boundary is the `(div1,div2)` change captured in
`LineMap.section_starts` (see *The authoritative-boundary principle*). The text
classifiers are used to BUILD that bitmap (and as a mid-load fallback), not to
make pagination decisions at runtime.

### Scene headers and page boundaries

`back_up_for_speaker` positions page tops. When a dialogue line would be the
first on a new page, it backs up over blanks, speaker names, and entrance stage
directions — and, when the authoritative bitmap is present, it STOPS at the
`is_section_start` boundary line (the chrome line that opens the scene). This puts
the scene header (`ACT 1 / SCENE 2 / =====`) at the page top instead of splitting
it across pages. (Call it via the `back_up_for_speaker_state` wrapper, which
builds the boundary closure from `state.section_starts()`; the bare
`back_up_for_speaker(buffer, line, is_break)` is for the pure test mirror.)

`clamp_at_section_break` clamps `last_fit` to the line before the first
`is_section_start` boundary in the visible range, so new scenes start fresh.

### Title bar scene display

`update_title_bar_scene()` in `app.rs` reads the current line's `div1`/`div2`
and formats a label like "Act 1, Scene 2" (or "Act 1, Chorus" when
`div2==0`). Scene synopses are loaded from the `scene_synopses` table and
cached in `state.synopsis_cache` keyed by `(div1, div2)`.

### Scene-snap on navigation

When FORWARD dialogue navigation (`q`/`'`) or playback sync lands the cursor on
the first dialogue line of a new scene that's off-page, the viewport snaps so the
scene header is at the top of the new spread. Detection uses
`is_first_dialogue_of_scene` in `viewport.rs`, which walks backward from the
cursor — if it hits a scene marker or separator before any dialogue line, the
cursor is the scene's first dialogue; `back_up_for_speaker` then finds the full
header-block top. This applies to plays only (`!is_prose`).

- **Forward (`q`/`'`):** scene-snap fires in `scroll_after_jump_forward`.
- **Sync:** scene-snap fires via the `(div1, div2)` comparison in the CursorSync
  handler (`main.rs`).
- **Backward (`,`/`;`/`K`):** does NOT scene-snap. `scroll_after_jump_backward`
  follows the plain reading model below — scene snapping a backward step caused
  cursor oscillation in the final-spread region, and a reader pressing `,`/`;`
  expects to step to the previous dialogue, not jump a scene header to the top.
- **Scene jumps (`2`/`3`):** a separate path (`jump_to_next_scene` /
  `jump_to_prev_scene`), not these handlers — that is where intentional
  scene-to-page-top snapping lives.

### Dialogue navigation reading model (forward `q` `'` `h`; backward `,` `;` `t` `K`)

The forward segment binds (next dialogue/speaker) and the backward ones
(previous dialogue/speaker) move the cursor one dialogue line. In two columns
the cursor walks down the left column, down the right column, then onto the
next spread — backward is the mirror.

**Turning differs by direction** (see *Which binds may turn the page*): forward
binds turn when the cursor leaves the visible spread; backward binds do NOT
turn — the jump is reverted and the key is a no-op at the page top. The handlers in `navigation.rs`
set `current_line` to the next/prev dialogue, then call the scroll-after fns in
`scroll.rs`:

- **`scroll_after_jump_forward`** — if the new line is still visible, nothing to
  do. Otherwise turn forward: `page_turn_top(current_line)` makes the cursor the
  FIRST dialogue at the new spread's top-left. If that would leave the right
  column empty (the work's short tail, e.g. a lone EPILOGUE), redirect to
  `navigation::last_page_top` so the tail fills the RIGHT column (cursor in it)
  rather than sitting alone in the left.
- **`scroll_after_jump_backward`** — if still visible, nothing to do. Otherwise
  the cursor stepped above the page top: turn to `prev_page_top`, then set the
  cursor to that spread's LAST visible dialogue line (bottom of the right column)
  — what a reader expects from `,`/`;` at the page top. Backing up off trailing
  non-dialogue is via `prev_dialogue_line(last_fully_visible_line + 1)`.

`navigation::last_page_top(target)` (shared with `jump_to_end`/`G`) walks the
forward page chain (`column_split(top).next_page_top`) from a safe early start.
When the next whole-page turn `would_empty_right_column` (the tail is short), it
does NOT just keep the current spread — the natural page boundary can *skip* a
better final spread. It **pulls the top forward** to the smallest top whose
spread leaves no dialogue below its forward boundary and still has a non-empty
right column, so the short tail (a lone EPILOGUE) fills the RIGHT column of the
canonical last spread. Returning the earlier full spread instead orphans the
EPILOGUE one spread past the end (the 4308-vs-4316 bug: `G` landed showing
"…welcome is the sweet" in the right column with the EPILOGUE unreachable below;
`x`/`G` then did nothing because that spread looked final).
`viewport::would_empty_right_column(top)` is the predicate; both paths and the
sync page-turn (`update_highlight_and_advance_page`) and sync scene-snap use it.

#### `G` / jump-to-end: land on the CANONICAL final spread (EPILOGUE in the right column)

Two coupled requirements, both about the short-tail case (a lone `EPILOGUE` that
opens with a section-break marker):

1. **The page must be the canonical last spread.** `last_page_top` must not stop
   at the last *full* two-column spread when a later spread fits the tail into
   its right column (see `last_page_top` above — it pulls the top forward). The
   wrong spread shows "…welcome is the sweet" in the right column with the
   EPILOGUE orphaned below; the fuzz catches it via invariant 7
   (`JUMP-TO-END not at end: next_page_top < line_count — content still below`),
   and on the real display `x`/`G` do nothing there because the spread looks
   final.
2. **The cursor must be on that page.** `jump_to_end` lands the page first
   (`set_page_instant(last_page_top(target))`), then sets the cursor to the last
   dialogue line actually within that spread
   (`prev_dialogue_line(cs.page_end + 1)` clamped to `[new_top, cs.page_end]`) —
   mirroring the forward final-spread guard in `page_forward`. Without this the
   highlight lands ~10 lines off-page (fuzz `JumpEnd landing off-page`).

With both, the EPILOGUE renders as the right column of the canonical last spread,
the cursor sits on it, and `j`/`q` walking into the tail resolve to the same
spread.

Key files: `src/db/models.rs` (Line struct with div1/div2/line_in_div),
`src/db/queries.rs` (load_work, scene synopses),
`src/input/viewport.rs` (`back_up_for_speaker`, `clamp_at_section_break`,
`is_first_dialogue_of_scene`, `would_empty_right_column`, `prev_page_top`,
`last_fully_visible_line`, `prev_dialogue_line`),
`src/input/navigation.rs` (`last_page_top`, the four nav handlers),
`src/input/scroll.rs` (`scroll_after_jump_forward`, `scroll_after_jump_backward`),
`src/db/line_types.rs` (`is_act_scene_marker`, `is_separator`)

## Dialogue detection

Dialogue classification determines which lines the cursor can land on during
navigation and playback sync. Computed at load time in `db/queries.rs` and
stored as `line.is_dialogue: bool`.

### Play mode (is_prose = false)

A line is dialogue if it is NOT any of:

- Blank (empty or whitespace-only)
- Separator (starts with `=`)
- Act/scene marker (ACT, SCENE, CHAPTER, PROLOGUE, EPILOGUE, INDUCTION)
- Speaker name (all-caps, 2+ characters, optional trailing `.`; may include
  bracketed stage direction like `LUCIANA, [to Adriana]`)
- Stage direction (wrapped in `[...]`, or multi-line opener/closer)

### Prose mode (is_prose = true)

A line is dialogue if it is not blank, not a separator, and not an
act/scene marker. Speaker names and stage directions are treated as content.

### Multi-line stage directions

Folger-cleaned texts have multi-line stage directions spanning 2-19 lines
(the largest is in Henry VIII with 17 continuation lines between opener and
closer). `is_stage_direction` detects single-line (`[...\]`), openers
(`[...` without closing `]`), and closers (`...]` without opening `[`).
Continuation lines in between are caught by `is_inside_stage_direction` in
`viewport.rs` and `is_inside_stage_direction_text` in `text_file_map.rs`,
which scan backward up to 20 lines for an unclosed `[` opener.

### Runtime usage

- **Playback sync** — `pending_advance` finds the next dialogue buffer line
  to advance to when the current timestamp ends
- **Page navigation** — `next_dialogue_from`, `last_dialogue_in_page`,
  `next_dialogue_line`, `prev_dialogue_line` all skip non-dialogue lines
- **Dialogue nav keys** (comma, q, j, k) — move between dialogue lines only
- **Buffer-level check** — `viewport.rs::is_dialogue_line` re-checks the
  buffer text (not the precomputed bool) for viewport math, which also
  catches multi-line stage direction interiors via `is_inside_stage_direction`

Key files: `src/db/line_types.rs` (all classification functions),
`src/input/viewport.rs` (`is_dialogue_line`, `is_inside_stage_direction`),
`src/db/queries.rs` (assignment at load time)

## The floating page marker (the Label-in-Overlay saga)

(Formerly `page-marker-positioning.md`.)

The floating page marker is the small glyph at the bottom-center of a paginated
overlay page: **`⌄`** when more pages follow, **`•`** on the last page, nothing
on single-page content. It sits just below the last rendered line. Both the
**journal** and **gloss/synopsis** overlays have one.

This records why it is **drawn with Cairo on the accent-bar `DrawingArea`**
rather than positioned as a `Label` — the Label approach failed in a way that
took several rounds to diagnose, and the wrong fix is tempting.

### TL;DR

- **Do NOT** re-introduce a `Label` positioned by `set_margin_top` inside the
  scroll `Overlay`. Its **allocation lags the margin change by several frames**,
  so on a page turn to a shorter page the glyph paints at the *previous* page's
  y (off the short page) until an unrelated relayout.
- The marker is drawn in `ui::draw_page_marker_glyph` from **both overlays'
  `bar_drawing` `set_draw_func`**. The draw reads live `buffer_to_window_coords`
  every paint, so there is no allocation step and no timing race.
- Render the glyph via **Pango** (`pangocairo::functions::show_layout`), NOT
  `cairo::Context::show_text`. Cairo's toy text API does no font fallback, so
  `⌄` (U+2304) rendered as a **tofu box** on fonts lacking the glyph.

### The symptoms (in the order they appeared)

1. **Chevron stranded mid-page.** The marker sat far above the last line.
2. **Chevron missing on the last page** until the user pressed `j` (block-nav),
   which forced a fresh render.
3. **Reappeared after toggling the dwl tag** with the app — an unrelated full
   relayout fixed it. The decisive clue: the *geometry* was right, the
   *timing/allocation* was wrong.
4. After switching to Cairo: **tofu box** instead of the glyph.

### Root causes (three, stacked)

**1. `marker.preferred_size()` as the footer reserve (stranding).** The clamp
`top = (bottom+gap).min(viewport_h - reserve)` used
`marker.preferred_size().height()` as `reserve`. For an `Overlay` child with
`set_measure_overlay(false)`, that measured height **balloons to the whole
overlay allocation (~800px)**, so `viewport_h - reserve` went tiny and `top` was
clamped far above the last line. (Fixed at the time with a fixed 28px reserve;
now moot.)

**2. Overlay-child allocation lags `set_margin_top` (the core bug).** The Label
was `valign=Start` inside the scroll `Overlay`; its y was `margin_top`. On a page
turn we measured the new last-line bottom and called `set_margin_top(new_y)`.
**Logging proved** `set_margin_top(449)` was called while the Label's
*allocation* stayed at `y=810` (the previous full page's bottom) for several
frames:

```
MARKER-POS: bottom=441 top=449 ... alloc=(762,810,12,25)   # margin says 449, alloc still 810
```

`queue_resize()` did not force a synchronous re-allocation — GTK batches layout,
and an `Overlay` child's allocation is driven by the parent's layout pass, which
had not run yet. A single `idle_add_local_once` reposition **races the reflow**;
and because these overlays always render a page that FITS, the scroll range does
not change between same-fitting pages, so the `vadjustment::changed` "settle"
hook never fires to correct it. Only an unrelated relayout re-allocated the child
— hence "reappears after toggling the tag."

Attempts that did **not** fully fix it (don't repeat these):

- One-shot idle reposition — races the reflow.
- Tick-callback that stops on the first non-zero measurement — accepts the stale
  *previous-page* geometry (a valid non-zero value) before the reflow.
- Tick-callback that waits for two stable frames — correct but "slow to appear."
- `queue_resize()` after `set_margin_top` — the allocation still lagged.

The real problem is not *when we measure* but that **`set_margin_top` on an
overlay child does not take effect this frame**. GTK-rs does not expose
`OverlayLayout` / a `get_child_position` vfunc, so there is no declarative way to
place the child synchronously either.

**3. Cairo toy fonts don't fall back (tofu).** `cr.show_text("⌄")` produced a
missing-glyph box: `cairo::Context::show_text` uses the toy font API with **no
font substitution**, and the selected face lacked U+2304. The fix is to render a
`pango::Layout` (automatic font fallback, exactly like the old CSS-styled Label)
via `pangocairo::functions::show_layout`. Added the `pangocairo` dependency
(0.20, matching `pango` 0.20).

### The fix (current design)

- `ui::measure_last_line_bottom(view)` — last text line's bottom in **widget
  coords** (`line_yrange(end_iter)` → `buffer_to_window_coords(Widget, …)`), the
  same scroll-aware path the accent bar uses.
- `ui::draw_page_marker_glyph(cr, view, area_w, glyph, rgb, alpha, gap)` — draws
  the glyph centered horizontally, `gap` px below the last line, via a Pango
  layout at ~20px in the theme dim color. No-op when `glyph` is `None` or
  geometry isn't up yet (the next repaint catches it).
- Each overlay holds `marker_glyph: Rc<RefCell<Option<&'static str>>>` and
  `marker_color`, drawn at the **top** of its `bar_drawing` draw func (before the
  selection-bar early-return, so it shows while editing too).
- `update_page_marker` sets `marker_glyph` from `pagination::page_marker`, then
  `bar_drawing.queue_draw()` **plus** an `idle_add_local_once(queue_draw)` so the
  bar also repaints after the page-turn reflow (the scroll range may be
  unchanged).
- Color is `theme.dim_fg`, threaded via `set_marker_color` at startup and in
  `apply_theme_to_state` (responsive to a dwl theme change).

The accent bar has always been reliable because it draws this same way; the
marker now inherits that reliability.

### Rules for the future

- **Keep the marker in the Cairo draw path.** To move/restyle it, edit
  `draw_page_marker_glyph` and the per-overlay `marker_glyph`/`marker_color`
  state — do not add a positioned widget.
- **Never position a floating overlay child by `set_margin_top` and expect it to
  take effect the same frame.** The allocation lags. Draw it, or accept a
  multi-frame settle.
- **Draw text over the text view with Pango, not `cairo::show_text`** — you need
  font fallback for non-ASCII glyphs.
- Any change here is **pixel-level**: verify on screen (page down to a *short*
  last page, page up, and after a highlight `;wq`), not from logs.

## Dialogue spacing failures (plays)

(Formerly `blank-line-spacing-too-tall.md`.) Frequency-ordered. Check #1 first —
it presents as "spacing is GONE" rather than "spacing is wrong", and it is
load-order dependent, so it does NOT reproduce when you open the affected play
directly.

### 1. NO dialogue formatting at all — stale `block_indent_tiers` (2026-07-25)

**Tell.** A two-column play renders with **every** dialogue affordance missing at
once: speaker labels (KING, LAERTES) in plain body text instead of small-caps, no
gap above speakers, dialogue not indented, stage directions upright instead of
italic, act/scene headers not bold, blank lines at full height. "All of it
missing" is the diagnostic signature — when gaps are merely too tall or too
short, see #2 below (a tuning problem; this is a total no-op).

Confirm from the log before touching any code:

```bash
rg -n "TEXT_FILE:|FORMATTING:|TIMING: apply_dialogue" linux-lit-dev.log
```

- Healthy: `FORMATTING: applied dialogue formatting (N lines)` present and
  `TIMING: apply_dialogue_formatting` non-zero (~40ms for Hamlet).
- Broken: **no `FORMATTING:` line at all** and `TIMING: apply_dialogue_formatting
  0ms` for a multi-thousand-line play — the function early-returned without
  touching the buffer.

**Root cause.** `state.block_indent_tiers` left over from the PREVIOUSLY loaded
work. `apply_dialogue_formatting` (`src/app/formatting.rs`) early-returns
outright when that vec is non-empty — the guard added in `6cdc8490` so
block-aware verse typography isn't clobbered:

```rust
if !state.block_indent_tiers.is_empty() { return; }
```

The off-thread text-file fast path in `display_work_at_with_prepared`
(`src/app/mod.rs`) set `buffer` + `line_map` but never cleared the field, so
tiers from a block-aware work survived into the next work. Every branch in
`rebuild_buffer_text` cleared it; that one path did not.

Reproduction is a **work switch**, not a single load: (1) load a block-aware work
— one with non-prose `line_mapping.block_type` rows (BH-Barrett has 135 `heading`
+ 20 `blockquote`; LoJ likewise); (2) switch to a text-file play (Ham-Arkangel)
via the library picker; (3) the play loads through the off-thread fast path with
the tiers still set. Launching straight into the play formats correctly — which
is why "it works when I open it directly" is not evidence against it.

**Fix.** `state.clear_block_typography_state()` in the off-thread fast path,
alongside the `line_map` assignment. The helper resets all three fields keyed by
buffer-line index: `block_indent_tiers`, `italic_offset_map`,
`italic_line_spans`. Guarded by `block_typography_reset_tests` in
`src/app/mod.rs`, which asserts every buffer-fill branch performs the reset.

**General rule.** Any state keyed by **buffer-line index** must be reset by
**every** buffer-fill path. A guard that reads such state is only as safe as the
least careful fill branch.

#### Aftermath: pinned tables generated while formatting was broken

Fixing #1 exposes a second, separate symptom — **one row too many at the bottom
of each column**, the last line clipped by the card edge (sometimes with a
scrollbar). The formatting is correct; the PAGINATION is stale.

A table generated while dialogue formatting was suppressed measured rows with NO
speaker gaps (14px) and NO stage-direction gaps (8px), so it packed more rows per
column than now fit once those gaps came back.

**The layout fingerprint does NOT catch this** — and this is the important
lesson, because it is the one blind spot in the otherwise-thorough fingerprint
described in *How changing the font affects pagination*. `layout_fingerprint()`
hashes font family/size, ascent/descent, char width, window geometry, line
spacing, margins, columns, and top-spacer height — **nothing about whether
per-tag `pixels_above_lines` are applied.** A table generated under broken
formatting still reads as a valid `PAGES: table hit` and is accepted. So: the
fingerprint protects you from font and geometry drift, but NOT from a table
recorded while the typography itself was wrong.

Tell: `PAGES: table hit (N pages)` where the generation timestamp falls inside
the window when formatting was broken.

```bash
sqlite3 ~/utono/litdb/data/lit.db \
  "SELECT work_abbrev, layout_fingerprint, page_count, generated_at
     FROM play_pages_meta
    WHERE CAST(REPLACE(generated_at,'epoch:','') AS INTEGER) > <epoch>;"
```

Fix: delete the affected rows from BOTH `play_pages` and `play_pages_meta`
(matching `work_abbrev` + exact `layout_fingerprint`); the app regenerates on
next load at that geometry. Back up lit.db first and make sure no instance is
running (it rewrites config/state on exit).

Concretely on 2026-07-25/26: Ham-Arkangel's `v4 … 1920x1200` table was generated
at 21:52, one minute before the bug report, and pinned **81** pages. Regenerated
with formatting restored it is **85** — four more spreads for the same text. It
was the only affected play table.

Regenerating headlessly has two traps: `page_table_gen_attempted` is a
**one-shot latch per session** (`generate_and_store`), so resizing AFTER the
first layout settles never regenerates — resize during startup, before the layout
settles. And `LIT_GEN_PAGE_TABLE=1` does not override an existing table whose
fingerprint still matches — delete the rows first, then let it generate. (Same
latch family as the prose gap in the font section above.)

### 2. Blank lines between speakers/stage directions too tall

**Symptom.** In dialogue-formatted works, the blank lines separating speakers,
and between dialogue and stage directions, consumed too much vertical space — the
gap was roughly a full text line tall instead of a compact separator.

**Root cause.** In `apply_dialogue_formatting()`, blank lines were detected by
`line_types::is_blank()` and then skipped with `continue`. No formatting tag was
applied, so GTK rendered them at the full font height. Speaker names carried a
`speaker-gap` tag with `pixels_above_lines(speaker_gap * 5)` and stage directions
`stage-direction-gap` with `pixels_above_lines(10)`; those pixel gaps stacked on
top of the full-height blank line, doubling the separation.

**Fix, in four rounds** (the intermediate states are recorded because two of them
overshot in opposite directions):

1. **Shrink blank lines** — added a `blank-line` tag with `scale(0.25)` and
   applied it instead of skipping. Registered in the cleanup list so it is
   removed and recreated when formatting re-applies.
2. **Remove redundant pixel gaps** — dropped `pixels_above_lines` from both tags.
   Result: too little space; speakers ran into each other.
3. **Restore moderate gaps** — `pixels_above_lines(8)` on both `speaker-gap` and
   `stage-direction-gap`. Combined with the scaled blank line this gives a
   compact but visible break.
4. **Reduce the act/scene header gap** — `pixels_above_lines(20)` combined with
   the blanks above and below created too much whitespace around scene
   transitions; reduced to 8 to match.

**Final values:** blank lines `scale(0.25)`; speaker names, stage directions, and
act/scene headers all `pixels_above_lines(8)` (headers also `weight(700)`); the
`speaker_gap` variable (formerly `line_spacing * 5`) removed.

**Pagination note:** these gaps are exactly the per-tag `pixels_above_lines` the
fingerprint does not hash, so retuning any of them changes how many rows fit a
column WITHOUT invalidating stored tables. After changing a spacing value, delete
and regenerate the affected tables as in the Aftermath section above.

## Synopsis/gloss overlay anti-clipping

The synopsis, gloss, and echoes overlay cards (`src/ui/gloss_overlay.rs`)
scroll their own text in a `gtk4::TextView` inside a `gtk4::ScrolledWindow`
(`gloss_view` in `gloss_scrolled`), separate from the main reading card. They
reuse the **same line-snapping + bottom-clip technique** the main card uses, so
a partial (half) line never sits clipped against the title rule (top) or footer
rule (bottom). A CSS `mask-image` fade was tried first and does **not** work —
GTK4 (4.22) silently ignores `mask-image` on widgets, so do not use it here.

### Open-at-top (`reset_scroll_top`)

Called by `show_synopsis` / `show_gloss_with_color` / `show_echoes` after the
buffer text is set. Snapping to the top inline — or on a single idle tick — is
**timing-dependent and unreliable**: `set_visible` and `apply_font` recompute
the vadjustment range on a later layout pass, which on a slow real display
lands after the idle fires, leaving the card scrolled down with the first lines
clipped. Instead `reset_scroll_top` connects a **one-shot handler on the
vadjustment `changed` signal** (emitted when the range is recomputed, i.e. when
layout settles): it snaps to `lower()`, recomputes the bottom clip, then
disconnects. This reacts to the actual layout event rather than guessing a
delay. An `idle_add_local_once` backstop covers the case where `changed` fired
before the handler connected.

### Top edge — line-snapped scrolling (`scroll_gloss`, `snap_value_to_line`)

`scroll_gloss(delta)` no longer steps by a fixed pixel amount (the old fixed
60px step is what left partial lines). It computes a raw target
`value + 3 * line_height * delta`, then `snap_value_to_line(target_y)` returns
the greatest line-top `y` at or below the target — found by walking lines via
`view.line_yrange(&iter)` — clamped to `[lower, upper - page_size]`. This is the
overlay's local analogue of `snap_scroll_to_line` in `scroll.rs`: the viewport
top always aligns to a whole line.

### Bottom edge — invisible clip box (`recompute_overlay_bottom_clip`)

`bottom_clip` is a `gtk4::Box` overlaid on `gloss_scroll_overlay` (valign=End,
halign=Fill, `can_target=false`, `add_css_class("gloss-bottom-clip")` so it
paints the card background and hides — rather than recolors — whatever is beneath
it).

The clip math walks **real per-visual-row rects** (`display_rows`, which steps
`forward_display_line` and reads each row's `iter_location` rect), **not**
`line_yrange`. This is deliberate: the synopsis/gloss buffers join paragraphs
into single multi-row buffer lines and apply per-tag `pixels_above_lines`/`scale`,
so rows are not uniform and `line_yrange` (logical-line granular) would collapse a
wrapped paragraph to one paragraph-tall "row" and clip the wrong amount. It finds
the bottom of the last visual row that fits **entirely** above the viewport bottom
(`top_y + page_size`), then sets the clip height to
`viewport_bottom − last_full_bottom` so the leftover partial row at the bottom is
covered. Two guards: if the document ends inside the viewport it covers only the
slack below `content_h`; if a single row is taller than the viewport (nothing
fits) the clip stays at 0 so that row is not blanked.

The gloss overlay's `&self` entry point `update_bottom_clip` is a one-line call to
the shared `crate::ui::recompute_overlay_bottom_clip(view, clip, scrolled)` (see
"shared helpers" below); it no longer carries its own copy of the algorithm.

**Recompute on EVERY scroll, not just on the named scroll methods.** The clip is
recomputed from (a) `reset_scroll_top`'s `changed`-signal handler + idle backstop
during an open, (b) the explicit `update_bottom_clip()` calls inside
`scroll_gloss` / `scroll_gloss_to_top` / `scroll_gloss_to_bottom`, **and** (c) a
dedicated handler on the vadjustment's **`value_changed`** signal (connected in
`new()` right after `bottom_clip` is created). Path (c) is the catch-all: the
`changed` handler fires only while the adjustment *range* shifts (during an
open), so once the user scrolls and the range is stable the clip would keep its
stale open-time height. Recomputing on every *value* change keeps the bottom
mask aligned no matter how the scroll position moved.

**The clip box only masks the BOTTOM edge.** There is no top clip box — the top
edge is kept clean entirely by line-snapping the viewport top to a whole row
(`snap_value_to_line`). If a scroll lands the viewport top on a fractional row,
the first line shows clipped under the title rule with no mask to hide it, so
the snap must be correct or the top clips. See the section above on
`scroll_gloss` for the snap.

**Coordinate-space gotcha — `display_rows` must add `top_margin`.** Both the
bottom-clip and the top-snap walk visual rows via `display_rows`, which reads
each row rect with `iter_location`. `iter_location` returns **buffer**
coordinates (y = 0 at the first line of text; the view's `top_margin` is NOT
included), but the vadjustment scrolls over `top_margin + text + bottom_margin`,
so `adj.value()` / `adj.upper()` are `top_margin` larger. Comparing the two
directly shifts every row up by `top_margin`. Symptom (both edges clipped at
once): the bottom-clip under-counts the last partial row so it pokes through
under the footer rule, AND `snap_value_to_line` returns a top `top_margin` px
above the real row top so the first line clips under the title rule after a
scroll. `display_rows` therefore adds `view.top_margin()` to every row so its
output is in vadjustment space. (The main reading card avoids this entirely by
using `line_yrange`, whose y already includes the relevant offsets — but the
overlay can't, because its multi-row paragraphs need per-visual-row rects.)

### The journal overlay shares this clip (and once didn't — descender bug)

The **journal Q&A overlay** (`src/ui/journal_overlay.rs`) renders prose with the
same non-uniform rows as the gloss overlay (paragraph gaps, a larger title row,
descenders), so it needs the same per-row bottom clip. It originally used a
**uniform row-step estimate** instead: `update_bottom_clip` took the first
line's `line_yrange` as a fixed `step` and clipped `page_size − floor(page/step)
× step`. That assumes every row is `step` tall, so on overflowing prose the last
visible line's **descenders were cut by the footer rule** — the exact failure
this section warns `line_yrange` causes. (It was masked until the journal text
padding was widened to `card_side_margin`, which changed the wrap and pushed a
descender-bearing line to the bottom edge.)

The fix made both overlays share one implementation. The descender-correct logic
lives as free helpers in `src/ui/mod.rs`:

- `display_rows(view)` — the per-visual-row walk (`forward_display_line` +
  `iter_location`, `top_margin` added), for TextView-content overlays.
- `bottom_clip_height(rows, top_y, viewport_h, content_h)` — the **pure** clip
  math (last-full-row bottom → viewport bottom, with the empty-viewport,
  document-ends-inside, and single-tall-row guards). Unit-tested in
  `ui::bottom_clip_tests`, including a non-uniform-row case that a uniform-step
  estimate gets wrong. **This is now the single covering algorithm for every
  free-scroll surface** (overlays AND scroll-mode).
- `recompute_overlay_bottom_clip(view, clip, scrolled)` — the GTK wrapper for a
  TextView-content scrolled window (uses `display_rows`).
- `line_yrange_rows(view, top_val, viewport_h)` — the logical-line analog of
  `display_rows`, for scroll-mode (j/k) which clips on whole-line `line_yrange`
  geometry, not wrapped rows. `scroll.rs::scrolloff_bottom_clip_widgets` builds
  these rows and feeds them to `bottom_clip_height` (it was a verbatim copy of
  that algorithm — now it shares it).
- `recompute_overlay_bottom_clip_box(clip, scrolled)` — the variant for an
  overlay whose scrolled child is a widget **Box**, not a TextView (the
  translation overlay's column stack). A Box lays out whole child widgets that
  GTK never splits across the edge, so there is no wrapped partial row to mask —
  it covers only trailing slack below the content when the content ends inside
  the viewport (else clips 0). The translation overlay had **no** bottom clip
  before; this guard is what keeps a short translation's trailing slack from
  reading as a clipped edge.

Both the gloss `update_bottom_clip` and the journal `update_bottom_clip` are now
one-line calls to `recompute_overlay_bottom_clip` — neither carries its own copy.

**Not unified (deliberately, do NOT "dedup"):** the main reading card's
`scroll.rs::update_bottom_clip` is a *paginated* clip — it sums `line_yrange`
heights from a known `page_top` to a column-split/section boundary, with
`descender_guard`/`BASE_BOTTOM_MARGIN`/`exact_end` logic. That is a different
strategy from the free-scroll partial-row mask above; merging them would change
behavior. Likewise the gloss vs journal `snap_value_to_line` are different
algorithms (per-`display_rows`-row snap vs uniform `row_step` rounding), not
duplicates. See `docs/superpowers/specs/2026-06-25-clip-prevention-design.md`.

**Lesson: any overlay clipping a multi-row prose buffer must use per-row geometry
— never a uniform row-step — or the last line's descenders clip.**

### Margins (cosmetic, separate from clipping)

`gloss_scroll_overlay` carries `set_margin_top(24)` and `set_margin_bottom(20)`
so there is breathing room below the title rule and above the footer; the
line-snap and bottom-clip work on top of these. The `gloss_view` also keeps its
construction-time `set_top_margin`/`set_bottom_margin` (internal padding that
scrolls with the content).

### Verifying

Real GTK pixel layout is what matters here and headless rendering (the
`cage` + `grim` flow in the repo CLAUDE.md) lays out fonts/metrics differently —
it confirms the mechanism runs and roughly looks right but cannot prove
pixel-exact edge alignment. Confirm on the real display: open a long synopsis
(`h`), scroll with `j`/`k`, and check both edges show only whole lines.

Key files: `src/ui/gloss_overlay.rs` (`reset_scroll_top`, `scroll_gloss`,
`snap_value_to_line`, `update_bottom_clip`), `src/ui/mod.rs` (`display_rows`,
`bottom_clip_height`, `recompute_overlay_bottom_clip`, `line_yrange_rows`,
`recompute_overlay_bottom_clip_box` — the shared free-scroll helpers),
`src/input/scroll.rs` (`snap_scroll_to_line`, `update_bottom_clip` — the main
card's *paginated* clip, NOT the same algorithm; `scrolloff_bottom_clip_widgets`
— scroll-mode, now routed through the shared helper), `src/input/viewport.rs`
(`visible_range`)

## Translations overlay (`i`)

Unlike the synopsis/gloss overlay, the translation view is **not** a separate
widget — `show_translations` (`app.rs`) inserts a smaller italic translation
line directly into the main buffer below each original line, so the reader keeps
using `state.text_view` / `state.scrolled_window` and all the normal `page_top_line`
viewport math. `hide_translations` removes the inserted lines and restores the
two-column layout. `translations_visible` forces `column_count()` to 1.

### Card width and margins

Translation mode renders two logical columns (original + translation), so its
card is sized like the **two-column** layout, not a narrow single column:
`target_card_width` (`app.rs`) takes the `column_count >= 2 || translations`
branch (proportional `window_width * TWO_COLUMN_WIDTH_FRACTION`, floored at the
two-column width). The text is then inset like the gloss/synopsis cards —
`apply_tiled_mode` sets `left_bump` and the right margin to ~`card_width/4`
(clamped to the window, so it degrades to 0 on a narrow display). This
translation branch runs **before** the `tiled` short-circuit, because `tiled` is
computed against `column_width` (the single-column config), not the wider
translation card.

### No verse line numbers

The right-gutter every-5th foliation is suppressed in translation mode:
`rebuild_line_number_gutter` (`app.rs`) gates `show_numbers` on
`!state.translations_visible`. The interleaved original/translation rows would
otherwise make the numbers misleading. The sign column (left gutter `u`/`.`
markers) is separately suppressed via `sign_column_visible.set(false)` in
`show_translations`. The gutter teardown at the top of
`rebuild_line_number_gutter` runs unconditionally, so toggling translations off
reinstalls the numbers.

### Anti-clipping — two distinct fixes (toggle + navigation)

**Symptom:** with translations on, the top line is half-clipped at the top edge
and the bottom line is half-clipped at the bottom edge — both on first toggle and
while navigating with `j`/`k`. Without translations the same card snaps cleanly.

The translation view does **not** page-turn like the normal reader. It scrolls
**continuously** with a vim-style scrolloff (`cursor_next_dialogue` /
`cursor_prev_dialogue` take the `translations_visible` branch and call
`scroll_cursor_into_view_scrolloff`, not `scroll_after_jump_*`). That breaks the
two assumptions the paged anti-clipping machinery relies on, in two places:

**(a) Toggle-on anchor — snap top to `page_top_line`, clip bottom immediately.**
Inserting ~3000 translation lines shifts every buffer index; `show_translations`
remaps `page_top_line` via `map_line_after_insert` (correct — it always lands on
an original line). The deferred re-anchor idle used to set `adj.set_value` to the
**cursor's** old screen-y, which leaves the scroll between line tops. It now snaps
to `page_top_line`'s **exact pixel top** via `line_yrange`, clamped to
`[0, upper - page_size]`, **and then calls `scrolloff_bottom_clip_widgets`** to
cover the partial bottom line on the very first reveal — before any j/k. (The
earlier version relied on the paged `refresh_bottom_clip` here, whose scheduled
idles are unreliable right after the big insert, so the bottom clipped on open.)
The anchor must stay in the idle: GTK hasn't re-laid the grown buffer
synchronously, so `line_yrange`/`upper` are stale until the layout pass. Confirm
via the `TRANSLATIONS_SHOW: idle snap to page_top` log line showing `clamped == y`.

**(b) Navigation scroll — line-snap the target + scroll-aware bottom clip.**
`scroll_cursor_into_view_scrolloff` (`scroll.rs`) computed a raw pixel target
(`cursor_top - margin` / `cursor_bottom + margin - page_size`) and set it directly,
landing between line boundaries (top clip). It now:

- snaps that target down to a whole-line top via `snap_value_to_line_top` (which
  uses `TextView::line_at_y` for O(1) y→line mapping), and
- covers the partial bottom line with `update_scrolloff_bottom_clip` — a
  **scroll-position-aware** clip. Its widget-level core
  (`scrolloff_bottom_clip_widgets`) builds whole-line rows from the current
  `adj.value()` via `line_yrange_rows` (`line_at_y` + `forward_line`) and feeds
  them to the shared pure `bottom_clip_height`, so scroll-mode runs the SAME
  covering algorithm as the overlays (it used to inline a verbatim copy). Shared
  with the toggle-on idle in (a), which holds the widgets but not `AppState`.

The paged `update_bottom_clip` (`scroll.rs`) is **page_top-relative** and assumes
the scroll is snapped to `page_top` (offset 0); it is the wrong tool for the
continuously-scrolling translation view, which is why the navigation path uses its
own scroll-aware clip instead. (`update_bottom_clip` does now also add the
`scroll_offset` it had been computing-but-ignoring, which helps any off-boundary
paged case, but the translation nav path does not rely on it.)

**(c) Stale visibility cache.** `is_line_fully_visible` consults the
`last_visible_range` cache of line indices. After the buffer is remapped by a
translation toggle those indices are stale, so the check would mis-report
off-screen lines as visible. `show_translations` / `hide_translations` now clear
`state.last_visible_range` alongside `invalidate_page_tops`.

The failure chain to look for if clipping regresses: in translation mode, j/k →
`cursor_next/prev_dialogue` → `scroll_cursor_into_view_scrolloff`. If that sets a
non-line-aligned `adj.value` (top clip) or skips `update_scrolloff_bottom_clip`
(bottom clip), edges clip. Note `update_bottom_clip`'s scheduled idles may not log
during the toggle in the headless cage — verify on the real display.

### Verifying

Headless `cage` + `grim` confirms the mechanism runs and the gutter is clean,
but (as with the gloss overlay) cannot prove pixel-exact edges. Confirm on the
real display: press `i`, then `x`/`y` to page through, and check both edges show
only whole original+translation pairs and no right-gutter numbers.

Key files: `src/app.rs` (`show_translations`, `hide_translations`,
`map_line_after_insert`, `rebuild_line_number_gutter`, `target_card_width`,
`apply_tiled_mode`), `src/input/scroll.rs`
(`scroll_cursor_into_view_scrolloff`, `snap_value_to_line_top`, `line_at_value`,
`update_scrolloff_bottom_clip`, `scrolloff_bottom_clip_widgets`,
`snap_scroll_to_line`, `update_bottom_clip`, `refresh_bottom_clip`),
`src/input/navigation.rs` (`cursor_next_dialogue` / `cursor_prev_dialogue`
translation branch), `src/input/viewport.rs` (`is_line_fully_visible`,
`visible_range`)
