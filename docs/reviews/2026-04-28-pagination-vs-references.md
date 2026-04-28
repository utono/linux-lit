# Pagination Review vs Reference Codebases

**Date:** 2026-04-28
**Linux-lit files reviewed:** `src/input/navigation.rs` (2738 lines, full read), `src/app.rs` pagination touchpoints (2695 lines, targeted)
**References consulted:** `~/Documents/repos/linux-lit/foliate-js/paginator.js` (1130 lines, targeted: `getVisibleRange`, `View.expand`, `Paginator.snap`/`#turnPage`/`#scrollNext`/`#scrollPrev`/`#getVisibleRange`/`#afterScroll`, ResizeObservers, `relocate` event); `~/Documents/repos/linux-lit/bk/src/view.rs` (444 lines, full read)

## Status — closed 2026-04-28

| Finding | Status | Reference |
|---------|--------|-----------|
| F1 page turn lock | shipped | `PageTurnLock` |
| F2 single `visible_range` | shipped | `visible_range` + `trim_trailing_speakers` |
| F3 `after_page_change` rendezvous | shipped | commits `5fb4d8a` + `7f752a2` |
| F4 `last_visible_range` cache | shipped | commit `1ed6cad`; later split into raw (visibility) vs trimmed (boundary placement) per `1873899` |
| F5 Pango descender guard | shipped | commits `e733ed6` + `e398dd4` |
| F6 resize observer | closed without action | already implemented via tick callback at `app.rs:888` |
| F7 exact backward boundary | shipped | commits `d550b88`, `e79b14c` |
| F8 `page_tops` binary-search index | shipped | commit `cfb7fe5` |
| F9 block-atom trim | shipped | commits `3de6b4c`, `745fb0c`, `2716fa7`, `b5a5760`, `1873899` |
| F10 `OverlayMode` trait | deferred | review noted "L; do as part of a larger keymap refactor, not standalone for pagination." Pagination payoff is indirect (resize hook), and F6's tick callback already covers the reader resize-resnap case. Revisit when adding a new overlay or hitting friction modifying `keymap.rs`. |

All findings with direct pagination correctness or perf impact have shipped. The review is closed; F10 belongs to a future keymap-focused brainstorm.

## Summary

Linux-lit's pagination is functionally close to foliate's, but the *shape* of the code has diverged: foliate centralises visibility math in one `getVisibleRange`, gates page turns through one `#turnPage` lock, and broadcasts a single `relocate` event after every scroll, while linux-lit re-implements visibility four times, has no turn lock, and lets each consumer recompute state ad hoc after page changes. The headline win: align linux-lit's pagination to foliate's three pivots (`visible_range`, `turn_lock`, `relocate`) so future paginator.js reads translate line-for-line and three open bug classes (re-entrancy, stale clip, descender drift) become structurally hard to reintroduce.

## Findings

### F1. Page turn has no lock; foliate's `#turnPage` does [bug-suspect]

**Reference shape:** `foliate-js/paginator.js:1060-1071` — `#turnPage` sets `#locked = true`, awaits scroll/animation, clears it. `goTo`/`prev`/`next` early-return when locked. One owner of the in-flight turn.

**Linux-lit shape:** `src/input/navigation.rs:1034-1175` — `set_page` runs 700 ms (Crossfade) / 250 ms (Slide) `adw::TimedAnimation`. New turns call `prev.skip()` on the in-flight animation, but nothing prevents a second turn from mutating `state.page_top_line` while the first snapshot is still on screen.

**Refactor toward reference:** Add `state.turn_lock: bool` mirroring `#locked`. Wrap `set_page` with `set_turn_lock(true)` on entry, clear in the animation's `connect_done`. Make `page_forward`, `page_backward`, `set_page`, and `scroll_paragraph_to_top` early-return when `turn_lock` is set — same shape as foliate's three callers.

**Leverage unlocked:** Future foliate `#turnPage` reads map directly to linux-lit's `set_page`. New page-mutating entry points (e.g., MPV-driven jumps, future "go to page N") inherit the guard for free instead of needing bespoke race avoidance.

**Risk if ignored:** Skipped lines or stuck snapshot overlay during fast paging or playback near a paragraph boundary. Realistic trigger: MPV `time-pos` calling `scroll_paragraph_to_top` (line 1370) mid-animation.

**Effort:** S

---

### F2. Four height-summing loops; foliate has one `getVisibleRange` [pattern-alignment + bug-suspect]

**Reference shape:** `foliate-js/paginator.js:94-151` — single `getVisibleRange(view, start, end)` returns the visible range. Called from one place, `#getVisibleRange` (line 945), which caches the result in `#lastVisibleRange`. Every consumer reads the cache.

**Linux-lit shape:** `src/input/navigation.rs:119-152` (`last_fully_visible_line`), `:836-863` (`is_line_fully_visible`), `:1235-1318` (`update_bottom_clip`), `:1669-1702` (`lines_per_page`). Four loops over `line_yrange` heights against `widget_height - descender_guard - bottom_margin`. Three also apply trailing-speaker trim. Commit `800d8ae` partially unified one — the refactor is half-done.

**Refactor toward reference:** Extract `pub fn visible_range(state: &AppState, top: usize) -> VisibleRange { last_fit, total_height, count }` mirroring foliate's signature. Replace all four call sites with `visible_range(state, top)` (with optional trailing-speaker trim as a second-stage transform). Cache the latest result on `AppState` like foliate's `#lastVisibleRange` so `update_bottom_clip` and MPV sync read from cache, not recompute.

**Leverage unlocked:** Future descender, speaker-trim, or block-atom rules (see F6) land in one function; cross-references to `paginator.js#getVisibleRange` translate directly. Closes the bug class behind `d7f34dd`, `7559eb5`, `5f6c475`, `2467a01` (fixes that landed in some loops but not others).

**Risk if ignored:** Drift between the four loops keeps reintroducing the same class of off-by-one-line bugs.

**Effort:** M

---

### F3. No "post-scroll" event; foliate emits `relocate` [pattern-alignment]

**Reference shape:** `foliate-js/paginator.js:952-969` — `#afterScroll` fires a single `relocate` CustomEvent with `{ reason, range, index, fraction, page, pages, size }`. Page-label, TOC, bookmarks, and any future consumer all subscribe to this single source of truth.

**Linux-lit shape:** Page-label, MPV sync, vocab popup, and bookmark glyph each compute state independently after page turns from `state.page_top_line` / `state.current_line`. No guaranteed ordering; some paths run via `glib::idle_add_local_once`.

**Refactor toward reference:** Add `enum PageChangeReason { Forward, Backward, Resnap, MpvSync, GotoLine, GotoBookmark }` and `fn after_page_change(state: &mut AppState, reason: PageChangeReason)` called at the end of every page-mutating function. All current scattered "update X after a page change" calls move inside `after_page_change` in a deterministic order — same shape as foliate's `relocate` listeners.

**Leverage unlocked:** Future foliate `relocate` reads (annotations, progress reporting, location-addressing F-CFI translation) drop in as new branches inside `after_page_change`. Each new post-turn-dependent feature stops growing the "I forgot to call X" surface.

**Risk if ignored:** Each new post-turn consumer adds another scattered call site. Ordering bugs surface late and look unrelated.

**Effort:** M

---

### F4. Bottom-clip update is async-via-idle; foliate updates synchronously [bug-suspect]

**Reference shape:** `foliate-js/paginator.js:945-958` — `#getVisibleRange` runs synchronously inside `#afterScroll` (which fires immediately after scroll completes) and stores the result in `#lastVisibleRange`. No idle gap between scroll and visibility-state update.

**Linux-lit shape:** `src/input/navigation.rs:1207-1217` — `snap_scroll_to_line` schedules `update_bottom_clip` via `glib::idle_add_local_once`. Between scroll and idle callback firing, any caller reading `bottom_clip.height_request()` (and any height-summing loop reading `text_view.height()`) sees stale state. MPV time-pos handlers run on a different cadence than the GTK idle queue.

**Refactor toward reference:** Run `update_bottom_clip` synchronously immediately after `adj.set_value(y)`, then keep the idle re-run as a backstop for layout-pending cases. Combined with F2's cache, downstream consumers read from cache after a synchronous update — matching foliate's `#lastVisibleRange` invariant.

**Leverage unlocked:** Eliminates a class of MPV/key-race symptoms. After F2+F3 land, "did the cache update" becomes a single invariant to verify, not a per-call-site question.

**Risk if ignored:** MPV-driven page turns occasionally compute against the previous page's clip. Symptoms overlap with F1; both look like "sometimes a turn skips a line."

**Effort:** M

---

### F5. Descender guard is a 20%-of-line-height estimate; foliate measures the engine [bug-suspect]

**Reference shape:** `foliate-js/paginator.js:83-91` measures real rendered rects. Line 331's CSS workaround (`-webkit-line-box-contain: 'block glyphs replaced'`, comment "fix glyph clipping in WebKit") shows descender clipping needs an engine-specific measurement, not a percentage estimate. Pango exposes the equivalent (`pango::FontMetrics::descent()`).

**Linux-lit shape:** `src/input/navigation.rs:1221-1230` — `descender_guard_px` returns `(line_height / 5).max(6)`, computed from the **page-top** line only. Mixed-size content (smaller translation lines, larger chapter titles) uses the wrong baseline.

**Refactor toward reference:** Replace `descender_guard_px` with a Pango-driven `descender_for(line: usize)` that queries `text_view.pango_context().metrics(None, None).descent() / pango::SCALE` against the *last fitting* line, not the top. Live inside the F2 `visible_range` function so all four consumers pick up the fix together.

**Leverage unlocked:** Mixed-font-size pages (translations, chapter titles) stop clipping. Future foliate descender reads translate to a single Pango-querying function.

**Risk if ignored:** Descenders clip when the bottom line uses a larger font than the top line. Reproduces with translations enabled or chapter pages.

**Effort:** S–M

---

### F6. No `ResizeObserver` equivalent; foliate has two [missing-edge-case]

**Reference shape:** `foliate-js/paginator.js:211` and `:430` — two `ResizeObserver`s, one fires `expand()` on content size change, one fires `render()` on host resize. Fully reactive — no caller has to remember to invalidate.

**Linux-lit shape:** `src/input/navigation.rs:1179` — `resnap_page` is called explicitly, only after font/size changes. Window resize, monocle/tiled transitions, monitor scale change, or DPI change leave the `bottom_clip` height_request stale until the next page turn.

**Refactor toward reference:** Connect `text_view.connect_size_allocate` (debounced via a 50 ms `glib::timeout`) to a new `on_viewport_resize` handler that calls `resnap_page` then `after_page_change(PageChangeReason::Resnap)` (F3). Mirrors the host-resize observer in foliate.

**Leverage unlocked:** Resize handling becomes declarative like foliate's. New consumers (e.g., dynamic split panes, sidebar toggles) inherit re-pagination automatically. Eliminates the "did you call resnap_page?" question.

**Risk if ignored:** Last visible line clipped or excess gap below text after window resize. Easy to misattribute to descender bugs (cf. `b01d021`, `f172ea8`, `7dc3788`).

**Effort:** S

---

### F7. Backward-fallback is approximate; foliate's index math is exact [missing-edge-case]

**Reference shape:** Foliate has no equivalent — uses CFI for resume, so backward navigation always lands on a real previous viewport boundary (`paginator.js:1050-1054` `atStart`/`atEnd` use page indices, not approximation). bk also has no fallback because chapter-relative line offsets are exact (`bk/src/view.rs:200-207`).

**Linux-lit shape:** `src/input/navigation.rs:261-275` — when `page_history` is empty (resumed mid-book, or paged back through all history), `page_backward` falls back to `current - lpp`. `lpp` is computed from the *current* page's metrics, so the resulting top can land mid-paragraph or split a speaker from dialogue. The forward path takes pains to call `back_up_for_speaker`; the backward fallback skips it.

**Refactor toward reference:** Replace the fallback with `prev_page_top(state, current)` — a backward mirror of `next_page_top` that walks heights to find the exact previous boundary, then runs `back_up_for_speaker` + `next_dialogue_from` like the forward path. Same shape symmetry as bk's chapter offsets.

**Leverage unlocked:** F8's page-top cache becomes naturally bidirectional (binary-search either way). First-backward-after-resume stops being a special case.

**Risk if ignored:** First backward page turn after resume lands awkwardly. Subsequent turns are fine because `page_history` is now populated.

**Effort:** S

---

### F8. `viewport_page_for_line` walks from line 0; bk caches offsets [pattern-alignment]

**Reference shape:** `bk/src/view.rs:55-71` — page number is a constant-time formula via cached chapter-relative offsets. Linux-lit's substrate doesn't allow the formula directly (variable line heights), but the *cached-offset* shape does.

**Linux-lit shape:** `src/input/navigation.rs:197-222` — `viewport_page_for_line` runs `next_page_top` from line 0; each call walks heights. O(line_count²) GTK metric lookups per overlay-label refresh on long prose.

**Refactor toward reference:** Add `state.page_tops: Vec<usize>` populated lazily, invalidated on `loading_work` flip and on font/size change (and on F6's resize hook). `viewport_page_for_line` becomes `page_tops.binary_search(...)`. Same shape as bk's offset cache.

**Leverage unlocked:** Cache is also load-bearing for a future "go to page N" feature and for foliate-style `pages`/`page` fields in the F3 `relocate` payload. Future bk-style location-math reads translate directly.

**Risk if ignored:** Frame stutter on overlay-label refresh; perf cliff scales with work length and inverse font size. Blocks F3's `relocate` payload completeness.

**Effort:** M

---

### F9. Per-line "fully visible" rule; foliate is per-block [missing-edge-case]

**Reference shape:** `foliate-js/paginator.js:104-106` — "elements must be completely in view to be considered visible". Visibility is judged per *element*, so a stanza or stage-direction block is atomic.

**Linux-lit shape:** `src/input/navigation.rs:119-152` — `last_fully_visible_line` judges per buffer line. The trailing-speaker trim catches single dangling speakers, not multi-line group continuity (verse stanzas, multi-line stage directions).

**Refactor toward reference:** After F2 lands, add a `block_atom` post-pass inside `visible_range`: if the last fitting line is inside a multi-line block, back up to its start. Requires marking block boundaries in `line_map` or detecting runs via `line_types`. Same shape as foliate's per-element visibility.

**Leverage unlocked:** Verse stanzas and multi-line stage directions stop splitting mid-block. Future foliate per-element annotations / selection translate to per-block linux-lit logic.

**Risk if ignored:** Verse stanzas and multi-line stage directions split mid-block. Existing trailing-speaker trim catches most user-visible cases.

**Effort:** M

---

### F10. Layered if/else dispatch; bk uses a `View` trait [pattern-alignment]

**Reference shape:** `bk/src/view.rs:13-18` — `View` trait with `render`/`on_key`/`on_mouse`/`on_resize`. Each mode is a struct; mode swap is `bk.view = &Page`.

**Linux-lit shape:** `src/input/keymap.rs` is a layered if/else dispatch keyed on overlay visibility. Each new overlay grows the chain; isolated testing requires reproducing full state. No uniform `on_resize` hook.

**Refactor toward reference:** Define `trait OverlayMode { fn on_key(&self, state, key) -> KeyResult; fn on_resize(&self, state); }`. Pagination-relevant payoff: `on_resize` becomes the uniform hook that fires `resnap_page` (F6) for the active overlay.

**Leverage unlocked:** Future bk dispatch reads translate directly. Pagination payoff is indirect (it's primarily a navigation-review concern), so ranked last.

**Risk if ignored:** Refactor pressure as more overlays are added. Pagination payoff is indirect.

**Effort:** L (do as part of a larger keymap refactor, not standalone for pagination).

## Out of scope

- **Touch / scroll-velocity snap** (`paginator.js:804-822`) — linux-lit has no touch input plan; the gamepad path is discrete-event.
- **CFI as a portable location format** — pagination-adjacent but properly belongs to a `location-addressing` review.
- **Scroll-mode (non-paginated) flow** (`paginator.js:292-308` `scrolled`) — linux-lit's Scroll mode is `center_cursor`-based and works differently; comparison would be substrate-level, not algorithmic.
- **Foliate's column / RTL / vertical writing-mode handling** (`paginator.js:178-187` `getDirection`) — linux-lit is single-column LTR; not applicable today.
- **Foliate's `setStyles` re-flow on font load** (`paginator.js:1116`) — linux-lit reloads the buffer rather than restyling; different mental model.

## Suggested next step

Implement F2 (`visible_range`) and F3 (`after_page_change`) as a paired refactor — they are the two structural pivots that make every other finding land cleanly. F1 (turn lock) drops in trivially once F3 exists. F4 (synchronous clip update) and F5 (Pango descender) become single-site fixes inside the new `visible_range`. F6–F8 are independent and can ship in any order after the pivots. F9 and F10 are larger and should each get their own design pass before implementation.
