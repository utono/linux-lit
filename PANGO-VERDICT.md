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
