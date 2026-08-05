# Task 1 verdict: is Pango the oracle for prose line heights?

## The "displayed" definition used, and why

Generation (`generate_and_store_prose` → `record_prose_pages`) fires once per
settled layout with the viewport anchored at `state.page_top.line()` for the
entire sweep — confirmed both by `TRACE-FINDINGS.md` (`page_top` constant
through generation) and by this task's own runs (`page_top` in
`PAGES_PROSE_PANGO_SPLIT` matches the resumed/landed scene every time). The
only lines with a genuine on-screen paint behind them at generation time are
therefore the single page's worth starting at `page_top`.

I compute the displayed window as `[page_top, displayed_end)`, where
`displayed_end` is found by walking forward from `page_top` and summing
`sweep1` (the SAME `line_yrange` heights generation itself used — not a fresh
GTK call, which would just re-ask the same lazily-validated cache and beg the
question) until the running total would exceed `usable_height` (computed the
same way the rest of the file does: `text_view.height() - descender_guard -
SINGLE_COLUMN_BOTTOM_MARGIN`). This mirrors exactly what the app's own
`visible_range`/bottom-clip logic does to decide what's on screen, so
"displayed" means "the app itself believes this line was painted," not an
independent guess.

Because that walk can itself be one row optimistic about the very last line
(if that line's own swept height was under-measured — the bug's own
signature), I exclude a 1-line margin band on each side of the boundary from
BOTH populations: `displayed` = `[page_top, displayed_end - 1)`, `offscreen`
= `[displayed_end + 1, line_count)`. A line inside that band is neither
cleanly displayed nor cleanly off-screen and is skipped rather than forced
into a bucket. Lines before `page_top` are unreachable in these runs (each
launch lands directly on its start scene) and are not counted as displayed
either, even though some (e.g. buffer line 0) may have been transiently
touched during navigation — that's a deliberate under-count of "displayed,"
biasing the split toward the SAFER interpretation for Option A (if anything,
some offscreen-bucketed lines may actually have been seen, which would only
strengthen a finding of offscreen-only disagreement — it does not explain
away the disagreement found on the strictly-displayed lines below).

## Method: three independent runs, three chapters, at production geometry

All runs: `LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_START_WORK=BH-Barrett
LIT_GEN_PAGE_TABLE=1 LIT_TRACE_PANGO=1`, resized to 1920x1200, confirmed
`RESIZE_TICK: text_view.height changed -1 -> 1128` before trusting any
number. A single run at one scroll position seemed thin, so I ran it at three
different `LIT_START_SCENE` landings — the reported failure chapter (37.0),
the chapter the prior fix's acceptance test already covers and calls "healthy"
(26.0), and an unrelated early chapter (10.0) — to see whether the pattern is
specific to the known-bad zone or general. (A true "drive forward then
regenerate" strengthening was not possible: `generate_and_store_prose` gates
on a one-shot `prose_page_table_gen_attempted` latch per process, so a second
in-place regeneration after scrolling cannot be triggered without a fresh
launch — hence three fresh launches at three landings instead.)

### Run 1 — chapter 37 (the reported failure), run twice for stability

```
[  134ms] RESIZE_TICK: text_view.height changed -1 -> 1128
[ 4176ms] PAGES_PROSE_PANGO: lines=7300 disagree=6420 delta_sum=-32967 worst=line 510 pango=884 yrange=1024 wrap_w=899 above=6 below=6 ex=[L0:yr=40/pango=41 L1:yr=293/pango=237 L2:yr=124/pango=125 L3:yr=462/pango=406 L4:yr=715/pango=631 L5:yr=40/pango=41 L6:yr=40/pango=41 L7:yr=96/pango=97]
[ 4176ms] PAGES_PROSE_PANGO_SPLIT: page_top=4223 displayed_end=4230 usable_height=1071 displayed_lines=6 displayed_disagree=6 displayed_delta_sum=-79 offscreen_lines=3069 offscreen_disagree=2483 offscreen_delta_sum=2312 displayed_ex=[L4223:yr=124/pango=125 L4224:yr=40/pango=41 L4225:yr=40/pango=41 L4226:yr=293/pango=266 L4227:yr=349/pango=322 L4228:yr=153/pango=125] offscreen_ex=[L4231:yr=124/pango=125 L4232:yr=40/pango=41 L4233:yr=68/pango=69 L4234:yr=40/pango=41 L4235:yr=68/pango=69 L4236:yr=40/pango=41 L4237:yr=68/pango=69 L4238:yr=40/pango=41]
[ 4761ms] PAGES_PROSE: generated 802 pages for BH-Barrett fp=v5|Charis|16|22|5|9|1920x1200|6|40|1|44|1128|uh1071|cw1050|pv7
```

Re-run immediately after (fresh launch, identical scene/geometry):

```
[  155ms] RESIZE_TICK: text_view.height changed -1 -> 1128
[ 4046ms] PAGES_PROSE_PANGO: lines=7300 disagree=6448 delta_sum=-35187 worst=line 510 pango=884 yrange=1024 wrap_w=899 above=6 below=6 ex=[...]
[ 4046ms] PAGES_PROSE_PANGO_SPLIT: page_top=4223 displayed_end=4230 usable_height=1071 displayed_lines=6 displayed_disagree=6 displayed_delta_sum=-79 offscreen_lines=3069 offscreen_disagree=2483 offscreen_delta_sum=2312 displayed_ex=[L4223:yr=124/pango=125 L4224:yr=40/pango=41 L4225:yr=40/pango=41 L4226:yr=293/pango=266 L4227:yr=349/pango=322 L4228:yr=153/pango=125] offscreen_ex=[L4231:yr=124/pango=125 L4232:yr=40/pango=41 L4233:yr=68/pango=69 L4234:yr=40/pango=41 L4235:yr=68/pango=69 L4236:yr=40/pango=41 L4237:yr=68/pango=69 L4238:yr=40/pango=41]
[ 4615ms] PAGES_PROSE: generated 803 pages for BH-Barrett fp=v5|Charis|16|22|5|9|1920x1200|6|40|1|44|1128|uh1071|cw1050|pv7
```

The displayed-window split is byte-identical across both runs even though the
generated page count differs (802 vs 803, the already-known race) — `page_top`
here comes from `LIT_START_SCENE`, not scroll history, so it is deterministic.

### Run 2 — chapter 26 (the chapter the existing acceptance test calls healthy)

```
[  123ms] RESIZE_TICK: text_view.height changed -1 -> 1128
[ 3999ms] PAGES_PROSE_PANGO: lines=7300 disagree=6371 delta_sum=-29031 worst=line 510 pango=884 yrange=1024 wrap_w=899 above=6 below=6 ex=[...]
[ 3999ms] PAGES_PROSE_PANGO_SPLIT: page_top=2910 displayed_end=2914 usable_height=1071 displayed_lines=3 displayed_disagree=3 displayed_delta_sum=-26 offscreen_lines=4385 offscreen_disagree=3584 offscreen_delta_sum=3356 displayed_ex=[L2910:yr=209/pango=181 L2911:yr=40/pango=41 L2912:yr=40/pango=41] offscreen_ex=[L2916:yr=68/pango=69 L2917:yr=40/pango=41 L2918:yr=40/pango=41 L2919:yr=40/pango=41 L2920:yr=40/pango=41 L2921:yr=40/pango=41 L2922:yr=40/pango=41 L2923:yr=40/pango=41]
[ 4572ms] PAGES_PROSE: generated 799 pages for BH-Barrett fp=v5|Charis|16|22|5|9|1920x1200|6|40|1|44|1128|uh1071|cw1050|pv7
```

`L2910` is `page_top` itself — the very first line of the page, unambiguously
on screen — and it disagrees by a full row (`yr=209/pango=181`, -28px).

### Run 3 — chapter 10 (unrelated, further triangulation)

```
[  174ms] RESIZE_TICK: text_view.height changed -1 -> 1128
[ 4140ms] PAGES_PROSE_PANGO: lines=7300 disagree=6445 delta_sum=-34794 worst=line 510 pango=884 yrange=1024 wrap_w=899 above=6 below=6 ex=[...]
[ 4141ms] PAGES_PROSE_PANGO_SPLIT: page_top=930 displayed_end=935 usable_height=1071 displayed_lines=4 displayed_disagree=4 displayed_delta_sum=-82 offscreen_lines=6364 offscreen_disagree=5554 offscreen_delta_sum=-22938 displayed_ex=[L930:yr=237/pango=209 L931:yr=40/pango=41 L932:yr=40/pango=41 L933:yr=434/pango=378] offscreen_ex=[L936:yr=406/pango=350 L937:yr=293/pango=266 L938:yr=518/pango=462 L939:yr=799/pango=687 L940:yr=153/pango=125 L941:yr=293/pango=266 L942:yr=406/pango=378 L943:yr=321/pango=266]
```

Again `page_top` itself (`L930:yr=237/pango=209`, -28px) and a second
displayed line (`L933:yr=434/pango=378`, -56px = two rows) disagree by a
whole-row multiple.

## The two populations, aggregated

| run | chapter | displayed_lines | displayed_disagree | whole-row misses among them |
|---|---|---|---|---|
| 1 (x2) | 37 (failure zone) | 6 | 6 (100%) | 3 of 6 (-27, -27, -28px) |
| 2 | 26 ("healthy") | 3 | 3 (100%) | 1 of 3 (-28px, on page_top itself) |
| 3 | 10 (unrelated) | 4 | 4 (100%) | 2 of 4 (-28px, -56px) |

Every single run: **100% of displayed lines disagree with Pango**, and in
every run at least one of those disagreements is a whole-row multiple
(±28/56px, the app's own row pitch) — not sub-pixel rounding noise. The
remaining displayed disagreements are small (±1px), consistent with a
harmless rounding-mode difference between Pango's `pixel_size()` and GTK's
internal line-box accounting — but the whole-row misses are not explained by
rounding, and they appear on `page_top` itself in two of the three runs,
which is the least ambiguous "definitely displayed" line available.

I looked for a font-string mismatch as an alternative explanation (the probe
formats the font as `crate::ui::font_string(state.config.font_family.as_str(),
state.config.font_size as i32)`, same call `reapply_font` uses for the real
buffer-wide `font-size` tag — `font_size` is a plain `u32`, e.g. `16`, so
there is no integer-truncation-of-a-fractional-size difference to point to).
The font construction is byte-identical between the real tag and the probe.
I did not find a specific mechanism for the whole-row misses; I'm reporting
the measurement, not a diagnosis of Pango's internals.

## Verdict: **Option B**

Pango does **not** agree with `line_yrange` even on lines the app itself
believes were fully displayed. The disagreement is not confined to the
off-screen population — every run found displayed-line disagreement,
including whole-row-magnitude misses, and this held across three unrelated
chapters, not just the reported failure zone. Adopting Pango wholesale (Task
2A) would trade one wrong measurement for a different wrong measurement; per
the plan, Task 2 should force validation of every line before sweeping
(Task 2B) rather than switch to Pango.

## Confidence and what would change this verdict

High confidence for the headline call (Pango is not a safe drop-in
replacement for `line_yrange`): the finding reproduced across 3 independent
launches / chapters with 0 counterexamples (13 displayed lines total, 13
disagreements, 6 of them whole-row). The displayed sample per run is small
(3-6 lines) because a single page only has that many lines whose top is also
`page_top` before the next page boundary — this is an inherent consequence of
generation only ever having one page painted, not a methodological choice I
could easily widen (see the note above on why a second in-place regeneration
after scrolling isn't possible with the current one-shot gate).

What would change the verdict: if the ±1px jitter turns out to be the ENTIRE
story and the whole-row misses I found are somehow an artifact of my
"displayed" definition being wrong at the boundary — e.g. if `displayed_end`
is systematically overshooting by exactly one line so the "displayed" bucket
is silently absorbing the FIRST off-screen line every time. I mitigated this
with the 1-line margin exclusion specifically to guard against that, and
`page_top` itself (which needs no boundary walk at all — it is trivially
displayed) independently shows a whole-row miss in 2 of 3 runs, which isn't
subject to that boundary-walk concern at all. I consider that concern
addressed, not open, but it's the one place a reviewer should double-check.

## Task 1b

**Premise tested:** the controller's amendment hypothesized that Task 1's
whole-row misses are an artifact of the probe's `wrap_w = tv.width() -
tv.left_margin() - tv.right_margin()` being blind to a PER-LINE TextTag
margin (specifically `dialogue-indent`, `DIALOGUE_INDENT = 60px`), which would
make Pango assume a wider column than the view actually wraps to and
under-count rows on exactly the multi-row lines that show the deficit.

### Per-line properties found (code read, then confirmed empirically)

Reading `src/app/formatting.rs` before touching the probe: **`dialogue-indent`
never applies to BH-Barrett at all.** `apply_dialogue_formatting` (the only
place that tag is created/applied) early-returns at its `block_indent_tiers`
check (`formatting.rs:110`) for any "block-aware" work — and BH-Barrett is
exactly that: `work_has_blocks` is true (its `line_mapping.block_type` column
has `blockquote`/`heading` rows alongside `prose`, confirmed via
`sqlite3 lit.db "select block_type, count(*) from line_mapping where
work_abbrev='BH-Barrett' group by block_type"` → `blockquote|20 heading|135
prose|7145`). Block-aware works instead run `apply_block_typography`
(`formatting.rs:730`), which applies margin tags ONLY to non-prose block
types: `verse-indent-{0,1,2}` (verse, 48+32×tier px), `block-blockquote-indent`
(blockquote, 64px symmetric), `block-heading-center` (heading, no margin —
justification only). **Ordinary `prose` lines get NO margin tag at all** —
they inherit the view-level margin unmodified. Chapter 37 (the failure zone)
has only 5 blockquote + 2 heading lines out of 149 in that chapter; the
disagreeing multi-row lines sampled in Task 1 were plain prose, not
blockquote/heading.

### Corrected-width instrumentation

Extended the `LIT_TRACE_PANGO` probe (still inside its existing guard,
`src/input/prose_pages.rs`) to resolve the effective per-line left/right
margin from `iter.tags()` at each line's start — for each property
independently, the highest-`priority()` tag that has it explicitly set
(`is_left_margin_set`/`is_right_margin_set`), falling back to the view-level
margin when no tag sets it — then re-measures that same line's Pango layout
at the corrected width. Two new log lines: `PAGES_PROSE_PANGO_CORRECTED`
(whole-file) and `PAGES_PROSE_PANGO_CORRECTED_SPLIT` (displayed/offscreen,
same window as Task 1's split, reusing the identical `page_top`/boundary
computation for a line-for-line comparable result).

### Corrected wrap-width formula

```
eff_left  = highest-priority tag with left_margin  set, else tv.left_margin()
eff_right = highest-priority tag with right_margin set, else tv.right_margin()
corrected_wrap_w = tv.width() - eff_left - eff_right
```

### Before/after, three chapters (production geometry, 1920×1236 resize
confirmed via `RESIZE_TICK: text_view.height changed -1 -> 1164` each run —
1164 not 1128 this run's cage/decoration state, but the SAME grid used for
generation, so before/after stays comparable; PROSE_PAGES generated at
`1920x1236` fingerprint each time, not the 720p fallback):

| chapter | displayed_lines | base disagree | corrected disagree | corrected_wrap_w seen |
|---|---|---|---|---|
| 37 | 7 | 7 (100%) | 7 (100%), byte-identical deltas | 899 (== base 899), every sampled line |
| 26 | 3 | 3 (100%) | 3 (100%), byte-identical deltas | 899 (== base 899) |
| 10 | 4 | 4 (100%) | 4 (100%), byte-identical deltas | 899 (== base 899) |

Example (ch37): `L4226:yr=293/pango=266` uncorrected → `L4226:yr=293/cpango=266/cw=899`
corrected — same disagreement, same wrap width, because no tag applied.
`page_top` whole-row misses reproduce again at the corrected width: ch26
`L2910:yr=209/cpango=181` (-28px), ch10 `L930:yr=237/cpango=209` (-28px) and
`L933:yr=434/cpango=378` (-56px, two rows) — identical to Task 1's uncorrected
numbers.

Whole-run summary confirms this isn't a sampling artifact of the truncated
`ex=[...]` lists: `PAGES_PROSE_PANGO_CORRECTED`'s `disagree`/`delta_sum`
matched `PAGES_PROSE_PANGO`'s exactly in every run (e.g. ch37
`disagree=6453` both passes), and grepping all three logs for every
`cw=` value emitted found `cw=899` and nothing else — the corrected width
never differed from the base width for any line the probe touched.

### +1px single-row offset

Unaffected by the correction (expected — single-row lines can't show a
width-driven row-count effect). Still a flat `pango = yr + 1` on every
single-row example across all three runs (e.g. ch37 `L0:yr=40/pango=41`,
`L2:yr=124/pango=125`), consistent with Task 1's finding of a constant
rounding-mode difference, not something that varies with width.

### Verdict: **B CONFIRMED**

The hypothesized mechanism does not apply to this work. `dialogue-indent` is
never in effect for BH-Barrett (block-aware works skip that code path
entirely), and the block-aware margin tags that DO apply
(`block-blockquote-indent`, `verse-indent-*`) don't land on the prose lines
in either sampled example set — every corrected wrap width measured equals
the uncorrected 899px. Feeding Pango the "true" per-line width (by this
formula) produces byte-identical disagreement to the uncorrected probe,
including the same whole-row-magnitude misses on displayed lines (`page_top`
itself, twice). Task 1's Option B verdict stands: Pango is not a safe
drop-in replacement for `line_yrange`, and the remaining whole-row deficit is
unexplained by any per-line margin tag. Task 2 should proceed with forcing
validation (2B), not adopting Pango (2A).

### What remains unexplained

The mechanism for the whole-row deficit is still open. Ruled out by this
task: per-line left/right margin tags (dialogue-indent, block-blockquote,
verse-indent). Not yet checked: per-line `indent` (first-line indent, a
distinct Pango/GTK property from left_margin — `is_indent_set()` was not
included in this pass since no tag in this codebase's block-typography or
dialogue-formatting sets it, confirmed by `rg -n "set_indent\|\.indent\("
src/app/`), and any interaction between GTK's line-box accounting and Pango's
`pixel_size()` at wrap boundaries beyond a simple width/rounding difference
(e.g. hyphenation-adjacent word-break behavior, or a pixel/subpixel rounding
mode that compounds per wrapped row rather than being a flat per-line
constant).

### Confidence

High. The corrected-width mechanism was implemented and run at production
geometry across the same three chapters Task 1 used, with the SAME
displayed/offscreen boundary logic (so the two splits are line-for-line
comparable), and the result was unambiguous and reproduced identically three
times: `corrected_wrap_w` never differed from the base view-width in any
sampled line, so the correction is a no-op for this work. The one soft spot
is that `text_view.height` landed at 1164px rather than 1128px this session
(a minor cage/decoration-state difference from Task 1's original run, not a
geometry regression — generation still ran at the 1920×1236 resize, not the
720p fallback) — this affects `usable_height` slightly but not which tags
apply to which lines, so it does not weaken the wrap-width finding.
