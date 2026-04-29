# Pagination Review vs Reference Codebases

**Date:** 2026-04-29
**Linux-lit files reviewed:** `src/input/navigation.rs` (3973 lines), `src/app.rs` (3140 lines), `src/db/line_types.rs`, `src/text_file_map.rs`, `src/db/models.rs`
**References consulted:** `foliate-js/paginator.js` (1130 lines), `bk/src/view.rs` (444 lines)

## Summary

linux-lit's pagination is purely pixel-height-driven — page boundaries are set by which lines fit in the viewport, with no awareness of chapter or scene boundaries. Both foliate-js and bk treat chapters as isolated pagination units where a new chapter always starts on a fresh page. linux-lit already has `is_chapter`, `div1`, and `div2` fields on each `Line` in the DB but they are invisible to the page-boundary algorithm. The headline win is adding a chapter-break rule to `next_page_top` so that structural divisions force page breaks, mirroring both references' behavior.

## Findings

### F1. No forced page break at chapter/scene boundaries [missing-edge-case]

**Reference shape:** `foliate-js/paginator.js:253-283` — each section loads in its own `<iframe>`. `#turnPage` at lines 1060-1071 crosses to the next section when `atEnd` is true. A new section always starts from anchor 0 — there is no merging of a chapter's trailing whitespace with the next chapter's opening content. `bk/src/view.rs:181-186` — `next_chapter` sets `bk.line = 0` unconditionally; `scroll_down` at lines 193-199 detects end-of-chapter and calls `next_chapter` instead of scrolling further.

**Linux-lit shape:** `src/input/navigation.rs:219-237` — `next_page_top` computes the next page boundary solely from pixel height via `last_fully_visible_line`. It has no knowledge of `is_chapter`, `is_separator`, or `is_act_scene_marker`. If an act heading and the first lines of a new scene fit at the bottom of the current page, they appear there mid-page.

**Refactor toward reference:** In `next_page_top`, after computing `last_visible` from pixel height, scan `[top, last_visible]` for any line where the corresponding `Work.lines` entry has `is_chapter == true` OR the buffer text matches `is_act_scene_marker` / `is_separator`. If found at position `break_line`, clamp `last_visible` to `break_line - 1` so the chapter/scene boundary starts the next page. This mirrors bk's "chapter boundary = pagination boundary" model and foliate's per-section isolation, translated to linux-lit's single-buffer architecture.

**Leverage unlocked:** Chapter openings always appear at page tops, matching every e-reader users have seen. Future foliate section-boundary logic translates directly. The `[`/`]` chapter-jump keys and normal page-forward converge — both land on page tops.

**Risk if ignored:** Users reading prose or plays see chapter headings buried mid-page. The reading experience feels "unpaginated" compared to Kindle, Apple Books, Kobo, foliate, and bk.

**Effort:** M

---

### F2. `is_chapter` flag not propagated to pagination layer [schema-gap]

**Reference shape:** `bk/src/view.rs` — chapters are first-class `Vec<Chapter>` entries with their own line arrays. Chapter boundaries are structural, not heuristic. `foliate-js/paginator.js` — sections are separate documents; their identity is in the spine, not derived from content.

**Linux-lit shape:** `src/db/models.rs` — `Line` has `is_chapter: bool`, `div1: i64`, `div2: i64`. These are loaded into `Work.lines` by `load_work`. But `src/text_file_map.rs` `LineMap` has no chapter fields, and `src/input/navigation.rs` never reads `is_chapter` during page-boundary computation. The only consumer is `jump_to_prev/next_chapter` (`[`/`]` keys).

**Refactor toward reference:** Add a `chapter_breaks: Vec<usize>` field to `LineMap` (or compute it lazily from `Work.lines`) that maps buffer line indices where `is_chapter == true`. `next_page_top` consults this vec via binary search to detect whether a chapter boundary falls within the current page's visible range.

**Leverage unlocked:** Chapter-break logic in `next_page_top` becomes a simple `chapter_breaks.binary_search()` lookup — O(log n), no buffer text scanning needed. The `[`/`]` jump keys and page-forward share the same chapter index.

**Risk if ignored:** F1's implementation would have to scan buffer text line-by-line for `is_act_scene_marker`/`is_separator` on every page turn — O(lines_per_page) text classification per turn instead of O(log n) index lookup.

**Effort:** S

---

### F3. Prose works bypass all structural classification [missing-edge-case]

**Reference shape:** `foliate-js/paginator.js:658-664` — CSS `page-break-before: always` is rewritten to `column-break-before: always`, meaning any HTML element with a page-break declaration forces a new column (page) regardless of content type. `bk/src/view.rs` — every chapter is a separate entry; prose chapters get the same boundary treatment as play acts.

**Linux-lit shape:** `src/db/line_types.rs` — `is_dialogue` for `is_prose == true` returns `true` for every non-blank line unconditionally. It never calls `is_separator`, `is_act_scene_marker`, or any other classifier. A line starting with `= Chapter One` in a prose file is returned as dialogue content. `src/input/navigation.rs` — `next_dialogue_from` skips only blank and non-dialogue lines; in prose mode nothing is non-dialogue, so separator lines are treated as dialogue and can become `current_line` targets.

**Refactor toward reference:** In prose mode, `is_dialogue` should return `false` for `is_separator` and `is_act_scene_marker` lines — they are structural, not content. This makes them behave like speakers in plays: `next_dialogue_from` skips them, `back_up_for_speaker` backs over them, and `trim_trailing_speakers` trims them from page bottoms. The F1 chapter-break rule then applies uniformly to both prose and plays.

**Leverage unlocked:** Prose works get the same structural-element handling that plays already have. Chapter headings in prose become page-break points, speaker-like preamble to the first content line of a chapter.

**Risk if ignored:** F1's chapter-break logic would fire on `is_chapter` flags from the DB, but separator/header lines in the text would still be treated as dialogue — the cursor could land on `= Chapter One` as a "dialogue" line, and `trim_trailing_speakers` would not trim chapter headers from page bottoms.

**Effort:** S

---

### F4. `trim_visible_range` has no chapter-break awareness [pattern-alignment]

**Reference shape:** `foliate-js/paginator.js:658-664` — `break-before: column` prevents content after a break declaration from appearing on the same column as content before it. The break is respected by the CSS layout engine before any column-filling occurs.

**Linux-lit shape:** `src/input/navigation.rs:1588-1598` — `trim_visible_range` runs three passes (trim trailing speakers, trim block atoms, trim trailing speakers again). None of these passes check for chapter/scene boundaries. A chapter heading in the middle of the visible range is not treated as a split point.

**Refactor toward reference:** Add a fourth pass (or pre-pass) to `trim_visible_range`: if any line in `[top, last_fit]` is a chapter break (from the F2 index), clamp `last_fit` to the line before that break. This mirrors CSS `break-before: column` — content after the break is pushed to the next page. Apply before the existing three passes so that the speaker/stanza trimming operates on the already-clamped range.

**Leverage unlocked:** `trim_visible_range` becomes the single point where page-break rules are enforced. Future break rules (e.g., "don't split a speaker + first dialogue line") land in the same function, mirroring how CSS break properties all funnel through the column layout engine.

**Risk if ignored:** Without this, F1's implementation in `next_page_top` alone would not prevent the chapter heading from appearing at the bottom of the current page — `last_fully_visible_line` would still report it as "visible" and the page would render with the heading mid-page before the next page turn.

**Effort:** S

---

### F5. Last page of a chapter shows trailing whitespace to page bottom [pattern-alignment]

**Reference shape:** `bk/src/view.rs:318-393` — `render()` uses `min(bk.line + bk.rows, c.lines.len())` to clamp to chapter length. The last page of a chapter shows fewer than `bk.rows` lines — no padding. `foliate-js` — last CSS column is partially filled; no padding.

**Linux-lit shape:** `src/input/navigation.rs:2005` — `update_bottom_clip` computes `clip = widget_height - total_height` and sizes the bottom_clip overlay to cover remaining space. This correctly covers the gap. However, there is no visual separator indicating "end of chapter" — the clip just shows card background.

**Refactor toward reference:** This is already handled correctly for visual layout. Optionally, if F1 introduces chapter breaks, the last page of a chapter could display a centered `* * *` or `§` marker in the bottom_clip zone to signal chapter end, as physical books do. Low priority — the blank space alone is acceptable per both references.

**Leverage unlocked:** Visual parity with printed books. Minor.

**Risk if ignored:** None — current behavior is acceptable.

**Effort:** S

## Out of scope

- **Widows/orphans logic** — neither reference implements it in code; both delegate to CSS/terminal. Not a leverage-unlocking refactor.
- **`getVisibleRange` DOM binary search** — foliate-js needs this for arbitrary HTML; linux-lit's pre-indexed line arrays make it unnecessary. Different substrate.
- **Multi-column CSS layout** — foliate-js's fundamental approach (CSS columns in iframes) doesn't translate to GTK4 TextView.
- **`atStart`/`atEnd` naming** — foliate's section-boundary flags could rename linux-lit's `page_top_line == 0` / `next_page_top >= line_count` checks, but the leverage is minimal since linux-lit uses a single-buffer model.
- **Audio sync at chapter boundaries** — if chapter breaks affect page_top placement, MPV seek-to-line timing may need adjustment. Belongs in an audio-sync review.

## Suggested next step

Implement F2 (propagate `is_chapter` into a chapter-breaks index), then F3 (make prose classify separators as non-dialogue), then F1+F4 together (chapter-break rule in `next_page_top` + `trim_visible_range`). F2 and F3 are both S-effort prerequisites that make F1+F4 clean. Write an implementation plan covering all four as a single feature: "chapter-aware pagination."
