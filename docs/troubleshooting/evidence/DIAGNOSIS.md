# Diagnosis: BH-Barrett Chapter 37 last-line clip, "Whose compliments, Charley?"

Worktree: `/home/mlj/utono/linux-lit-wt/diag/ch37-clip` at `b0ccf5c9`
(post pv7 fix + pv-bump docs). Raw log saved alongside this file:
`ch37-diag-raw.log` (full run) — was written outside the repo dir via
`LIT_LOG_PATH` pointed at the agent scratchpad, to avoid the default log
path (`~/utono/linux-lit/linux-lit-dev.log`, hardcoded in `src/main.rs`,
i.e. the user's MAIN checkout) being written to from this worktree.

## 1. Does it reproduce headlessly at 1920x1200 on chapter 37?

**Yes — decisively, and far worse than the single reported line suggests.**

Launch: `cage` headless, `LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_NO_MPV=1
LIT_START_WORK=BH-Barrett LIT_START_SCENE=37.0`, resized via `wlr-randr
--custom-mode 1920x1200`. Confirmed production geometry landed:

```
[57726ms] RESIZE_TICK: text_view.height changed 1164 -> 1128
[58170ms] PAGES_PROSE: table hit (808 pages) for BH-Barrett
```

`widget_h=1128`, 808 pages — matches the pv7 fingerprint
(`v5|Charis|16|22|5|9|1920x1200|6|40|1|44|1128|uh1071|cw1050|pv7`) and its
`prose_pages_meta` row (`page_count=808, validated=1,
generated_at=epoch:1785947627`) exactly.

Driving forward (`x`) 20 pages from the chapter start (page 492/808,
top=4224) produced **14 distinct `CLIP_WARN … OVERFLOW … clip=0`** page
tops, essentially back-to-back, not an isolated line:

```
CLIP_WARN: ... total=1147 > widget_h=1128 clip=0 page_top=4224 ...  (+19px)
CLIP_WARN: ... total=1215 > widget_h=1128 clip=0 page_top=4259 ...  (+87px)
CLIP_WARN: ... total=1242 > widget_h=1128 clip=0 page_top=4269 ...  (+114px)
CLIP_WARN: ... total=1141 > widget_h=1128 clip=0 page_top=4297 ...  (+13px)
CLIP_WARN: ... total=1215 > widget_h=1128 clip=0 page_top=4329 ...  (+87px)
CLIP_WARN: ... total=1141 > widget_h=1128 clip=0 page_top=4337 ...  (+13px)
CLIP_WARN: ... total=1181 > widget_h=1128 clip=0 page_top=4345 ...  (+53px)
CLIP_WARN: ... total=1203 > widget_h=1128 clip=0 page_top=4352 ...  (+75px)
CLIP_WARN: ... total=1210 > widget_h=1128 clip=0 page_top=4361 ...  (+82px)
CLIP_WARN: ... total=1176 > widget_h=1128 clip=0 page_top=4380 ...  (+48px)
CLIP_WARN: ... total=1143 > widget_h=1128 clip=0 page_top=4389 ...  (+15px)
CLIP_WARN: ... total=1169 > widget_h=1128 clip=0 page_top=4402 ...  (+41px)
CLIP_WARN: ... total=1191 > widget_h=1128 clip=0 page_top=4408 ...  (+63px)
CLIP_WARN: ... total=1164 > widget_h=1128 clip=0 page_top=4416 ...  (+36px)
```

## 2. Which engine served the page?

**The stored pv7 table — `table hit`, never `generated` or a live-engine
fallback.** Confirmed via `PAGES_PROSE: page N/808 top=(...)` lines for
every one of the overflowing pages above (e.g. `page 495/808
top=(4259,62)`), all served from the same 808-page table hit at
`58170ms`. No regeneration occurred during the drive. The pv7-table
analysis is squarely on point.

## 3. The offending page's exact arithmetic (the reported line)

Re-navigated (via `y`/PageBackward from deeper pages) onto page 492/808,
`top=(4224,0)`, the page ending in "Whose compliments, Charley?":

```
[242322ms] RENDER_HEIGHTS: page_top=4224 end=4233 top_off=0 tv_width=1050 \
  left_margin=20 right_margin=131 wrap_w=899 \
  heights=[40, 40, 293, 349, 153, 68, 40, 124, 40]
[242322ms] BOTTOM_CLIP_EXACT: widget_h=1128 total=1147 allowance=5 clip=0 \
  page_top=4224 top_off=0 end=4233
[242322ms] CLIP_WARN: main-card prose-1col OVERFLOW total=1147 > \
  widget_h=1128 clip=0 page_top=4224 top_off=0 bottom_head=None end=4233 \
  (clip-prevention.md #12)
```

Lines 4225–4233 (`bottom_head=None`, so a plain sum, `top_off=0`):

| line_in_div | text | height (px) |
|---|---|---|
| 4225 | CHAPTER XXXVII | 40 |
| 4226 | Jarndyce and Jarndyce | 40 |
| 4227 | "If the secret I had to keep…" (long para) | 293 |
| 4228 | "The difficulty that I felt…" (long para) | 349 |
| 4229 | "We were to stay a month…" | 153 |
| 4230 | "Oh! If you please, miss…" | 68 |
| 4231 | "Why, Charley," said I… | 40 |
| 4232 | "I don't know, miss," … | 124 |
| 4233 | "Whose compliments, Charley?" | 40 |

Sum = 40+40+293+349+153+68+40+124+40 = **1147**. `widget_h=1128`.
**Overflow = 19px**, clip floors to 0 — matches the user's measured ~8px of
a 28px line box surviving (line box top rendered, ~19-20px of it masked
by nothing, i.e. clip=0 leaves the whole overflow unmasked and the last
row simply runs off the card).

This is the smallest overflow in the sample (19px); neighboring pages in
the same chapter overflow by up to 114px, so ch. 37's problem is not
isolated to this one page — it's the first of a run.

`lit.db` cross-check (`line_mapping`, `work_abbrev='BH-Barrett'`) confirms
`line_in_div=4233` canonical_text = `"Whose compliments, Charley?"`,
`div1=37`, matching the screenshot's cut text exactly.

## 4. Why did the validator accept this page?

Per `prose_pages_meta`: `validated=1`, `generated_at=epoch:1785947627`
(2026-08-05 11:33:47 local) — **after** both fix commits
(`7bb16afe` "prose generation must measure AFTER the font tag applies",
10:53:11, and `4efb4b8e` "bump ... to pv7", 11:28:15). So this is not a
stale pre-fix table being served past the fingerprint bump — it was
generated by the fixed code.

Per the doc's own algebraic argument (`clip-prevention.md #12` and the
`log_generation_height_drift` comment in `prose_pages.rs`), `page_px`
(generation/validator) and `exact_page_content_height` (render) are the
same formula on `end_off == 0`/`bottom_head=None` pages — a disagreement
can only mean the **heights** differed between the two moments. I could
not directly pull the generation-time height vector without forcing a
fresh generation (`LIT_GEN_PAGE_TABLE=1` + `LIT_TRACE_HEIGHTS`), which the
task explicitly excluded to keep testing the table the user is actually
hitting. So the generation-side heights for this exact page are not
captured here — only inferred.

**Best-supported hypothesis, not confirmed by a generation-time trace:**
the `record_prose_pages` fix (`queue_resize()` + bounded
`MainContext::iteration` pump, then a two-sweep convergence guard) is
real but incomplete — it eliminated the *font-tag* timing race the ch. 26
guard test exercises, but something in this deeper region of the document
(chapter 37, buffer lines ~4224-4430+) still produces heights at
generation time that are systematically ~1-8% smaller than what the
renderer measures later. The pervasiveness (14 consecutive overflowing
pages, non-heading lines included) argues against a one-off race and for
a **structural** difference between generation-time and render-time
layout for this stretch of buffer — see §5.

## 5. Is chapter 37 special?

**Only weakly, and NOT in the way that explains the overflow.**

- Chapter 37 does open with a heading pair (`CHAPTER XXXVII` /
  `Jarndyce and Jarndyce`, `line_in_div` 4225/4226, `block_type='heading'`
  in `line_mapping`). But their measured heights (40px each) are
  identical to ordinary short single-row dialogue lines in the same page
  (e.g. line 4231, also 40px) — there is no visible scale/weight/centering
  premium on these two lines' box height.
- Source-level check: `apply_bcp_formatting` (the only code path in this
  codebase that applies `Justification::Center` / `scale(1.1)` / extra
  `pixels_above_lines` to a heading) is BCP-liturgical-text-only — it is
  never invoked for a plain novel like BH-Barrett. No heading-specific
  TextTag exists for prose chapter headings in `src/app/formatting.rs`,
  `src/app/font.rs`, or elsewhere. `is_chapter` only gates
  boundary/navigation logic (`prose_next_boundary`'s chapter-clamp,
  `validate_prose_pages`'s "chapter starts a page" rule), never visual
  styling.
- Decisive evidence against "chapter 37 is special": of the 14 overflowing
  pages sampled, only ONE (page 492, top=4224) contains the chapter
  heading. The other 13 (page_top 4259, 4269, 4297, 4329, 4337, 4345,
  4352, 4361, 4380, 4389, 4402, 4408, 4416) are deep in ordinary body
  paragraphs with zero headings, yet overflow by comparable or larger
  amounts (up to 114px, more than 5x the heading page's 19px). If heading
  styling were the cause, only page 492 would be affected.

**Conclusion on chapter 37 structure:** it is not a heading/centered-title
styling defect. Chapter 37 is simply the first place this drive happened
to sample; the same class of overflow appears to be pervasive through a
large stretch of the stored 808-page pv7 table for BH-Barrett, well past
the chapter boundary, in plain prose paragraphs. Whether chapter 26 (the
one guard test covers) is actually clean, or was simply the one place
spot-checked on the real display after the fix, was not re-verified in
this session — this diagnosis only re-confirms chapter 26 is what the
existing `deep_prose_pages_never_overflow_the_card` test exercises, not
that it is representative of the whole table.

## Summary

The pv5→pv7 fix did NOT fully resolve the prose overflow defect. It
reproduces headlessly, at production geometry, served by the correct
pv7 stored table (generated by the post-fix code, `validated=1`), and is
pervasive across at least ~14 consecutive pages spanning chapter 37 —
not an isolated chapter-37-heading quirk. The root cause of *why*
generation-time heights still diverge from render-time heights here
remains open; confirming it needs a fresh, traced generation
(`LIT_GEN_PAGE_TABLE=1 LIT_TRACE_HEIGHTS=<range>`) at this exact
geometry to capture the generation-time height vector and diff it
line-by-line against the `RENDER_HEIGHTS` vector already captured above
— explicitly out of scope for this diagnosis-only pass.
