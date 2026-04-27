# Pagination Review vs Reference Codebases

**Date:** 2026-04-28
**Linux-lit files reviewed:** `src/input/navigation.rs` (2738 lines, full read), `src/app.rs` pagination touchpoints (2695 lines, targeted)
**References consulted:** `~/Documents/repos/linux-lit/foliate-js/paginator.js` (1130 lines, targeted: `getVisibleRange`, `View.expand`, `Paginator.snap`/`#turnPage`/`#scrollNext`/`#scrollPrev`/`#getVisibleRange`/`#afterScroll`, ResizeObservers, `relocate` event); `~/Documents/repos/linux-lit/bk/src/view.rs` (444 lines, full read)

## Summary

Linux-lit's pagination is correct in steady state but has structural weaknesses the references handle: re-entrancy during animated turns is unguarded (foliate uses an explicit lock), automatic re-pagination on widget resize is missing (foliate uses ResizeObservers), the height-summing loop is duplicated four times across files (drift risk already realised in past fixes), and there is no single "post-scroll" event for state derivation (foliate emits `relocate`). Headline: F1–F4 are all small, independent fixes that together would resolve a class of intermittent visual bugs; F1 first.

## Findings

### F1. No re-entrancy lock on animated page turns [bug-suspect]

**Reference:** `foliate-js/paginator.js:1060-1071` — `#turnPage` sets `#locked` true, awaits scroll/animation, clears it. `goTo`/`prev`/`next` early-return when locked.

**Linux-lit:** `src/input/navigation.rs:1034-1175` — `set_page` runs 700 ms (Crossfade) / 250 ms (Slide) `adw::TimedAnimation`. New turns call `prev.skip()` on the in-flight animation but nothing prevents a second turn from mutating `state.page_top_line` while the first snapshot is still on screen.

**Hypothesis / improvement:** Add `state.page_turn_locked: bool`. Set true at start of `set_page`, clear in `connect_done`. Early-return from `page_forward`, `page_backward`, `set_page`, `scroll_paragraph_to_top` when locked. Realistic trigger: MPV `time-pos` calling `scroll_paragraph_to_top` (line 1370) mid-animation.

**Risk if ignored:** Skipped lines or stuck snapshot overlay during fast paging or playback near a paragraph boundary.

**Effort:** S

---

### F2. Stale-state read in MPV/key races [bug-suspect]

**Reference:** `foliate-js/paginator.js:945-958` — `#getVisibleRange` runs synchronously inside `#afterScroll` and stores result in `#lastVisibleRange`. Any later consumer reads from the cached value, not from a re-computation that could see stale layout.

**Linux-lit:** `src/input/navigation.rs:1207-1217` — `snap_scroll_to_line` schedules `update_bottom_clip` via `glib::idle_add_local_once`. Between the scroll and the idle callback firing, any caller reading `bottom_clip` height_request (and any height-summing loop that reads `text_view.height()`) sees stale state. MPV time-pos handlers read viewport state on a different cadence than the GTK idle queue.

**Hypothesis / improvement:** Run `update_bottom_clip` synchronously after `adj.set_value(y)` and keep the idle re-run as a backstop. Cache the computed last-visible line on AppState so sync handlers read from cache, not recompute.

**Risk if ignored:** MPV-driven page turns occasionally compute against the previous page's clip. Symptoms overlap with F1.

**Effort:** M

---

### F3. Descender guard is a 20%-of-line-height estimate, not real font descent [bug-suspect]

**Reference:** `foliate-js/paginator.js:83-91` measures real rendered rects. Line 331's CSS workaround (`-webkit-line-box-contain: 'block glyphs replaced'`, comment "fix glyph clipping in WebKit") shows descender clipping needs an engine-specific fix, not a percentage estimate. Pango exposes the equivalent (`pango::FontMetrics::descent()`); linux-lit doesn't query it.

**Linux-lit:** `src/input/navigation.rs:1221-1230` — `descender_guard_px` returns `(line_height / 5).max(6)`, computed from the **page-top** line only. Mixed-size content (smaller translation lines, larger chapter titles) uses the wrong baseline.

**Hypothesis / improvement:** Compute guard from the *last fitting* line; ideally query `text_view.pango_context().metrics(None, None).descent() / pango::SCALE`.

**Risk if ignored:** Descenders clip when the bottom line uses a larger font than the top line. Reproduces with translations enabled or chapter pages.

**Effort:** S–M

---

### F4. No automatic re-pagination on viewport resize [missing-edge-case]

**Reference:** `foliate-js/paginator.js:211` and `:430` — two `ResizeObserver`s, one fires `expand()` on content size change, one fires `render()` on host resize. Fully reactive.

**Linux-lit:** `src/input/navigation.rs:1179` — `resnap_page` is called explicitly, only after font/size changes. Window resize, monocle/tiled transitions, monitor scale change, or DPI change leave `bottom_clip` height_request stale until the next page turn.

**Hypothesis / improvement:** Connect `text_view.connect_size_allocate` (debounced) to `resnap_page`.

**Risk if ignored:** Last visible line clipped or excess gap below text after window resize. Easy to misattribute to descender bugs (cf. `b01d021`, `f172ea8`, `7dc3788`).

**Effort:** S

---

### F5. Page-history backward fallback uses lpp instead of recomputed top [missing-edge-case]

**Reference:** Foliate has no equivalent — uses CFI for resume, so backward navigation always lands on a real previous viewport boundary (`paginator.js:1050-1054` `atStart`/`atEnd` use page indices, not approximation). bk also has no fallback because chapter-relative line offsets are exact (`bk/src/view.rs:200-207`).

**Linux-lit:** `src/input/navigation.rs:261-275` — when `page_history` is empty (resumed mid-book, or paged back through all history), `page_backward` falls back to `current - lpp`. Because `lpp` is computed from the *current* page's metrics, the resulting top can land mid-paragraph or split a speaker from dialogue. The forward path takes pains to call `back_up_for_speaker`; the backward fallback skips it.

**Hypothesis / improvement:** Run `back_up_for_speaker` + `next_dialogue_from` on the fallback top — same shape as the normal path. Better: walk backward summing heights to find the exact previous page-top (mirror of `next_page_top`).

**Risk if ignored:** First backward page turn after resume lands awkwardly. Subsequent turns are fine because `page_history` is now populated.

**Effort:** S

---

### F6. Grouped-content "fully visible" rule is per-line, not per-block [missing-edge-case]

**Reference:** `foliate-js/paginator.js:104-106` — "elements must be completely in view to be considered visible". Visibility is judged per *element*, so a stanza or stage-direction block is atomic.

**Linux-lit:** `src/input/navigation.rs:119-152` — `last_fully_visible_line` judges per buffer line. The trailing-speaker trim catches single dangling speakers, not multi-line group continuity (verse stanzas, multi-line stage directions).

**Hypothesis / improvement:** Add a "block atom" pass: if the last fitting line is inside a multi-line block, back up to its start. Requires marking block boundaries in `line_map` or detecting runs via `line_types`.

**Risk if ignored:** Verse stanzas and multi-line stage directions split mid-block. Existing trailing-speaker trim catches most user-visible cases.

**Effort:** M

---

### F7. Four near-identical viewport-height-summing loops [design-improvement]

**Reference:** `foliate-js/paginator.js:94-151` — one `getVisibleRange` function called from one place (`#getVisibleRange`, line 945).

**Linux-lit:** `src/input/navigation.rs:119-152` (`last_fully_visible_line`), `:836-863` (`is_line_fully_visible`), `:1235-1318` (`update_bottom_clip`), `:1669-1702` (`lines_per_page`). All four loop over lines summing `line_yrange` heights against `widget_height - descender_guard - bottom_margin`. Three also apply trailing-speaker trim. Commit `800d8ae` partially unified one — the refactor is half-done.

**Hypothesis / improvement:** Extract `pub fn visible_lines(state: &AppState, top: usize) -> VisibleLines { last_fit, total_height, count }`. Trailing-speaker trim becomes a separate step callers apply when needed.

**Risk if ignored:** Future descender / speaker-trim fixes land in some loops but not others — already the bug class behind `d7f34dd`, `7559eb5`, `5f6c475`, `2467a01`.

**Effort:** M

---

### F8. No "post-scroll" event for downstream consumers [design-improvement]

**Reference:** `foliate-js/paginator.js:952-969` — `#afterScroll` fires a `relocate` CustomEvent with `{ reason, range, index, fraction, page, pages, size }`. All consumers subscribe to this single source of truth.

**Linux-lit:** Page-label, MPV sync, vocab popup, bookmark glyph all compute state independently after page turns from `state.page_top_line` / `state.current_line`. No guaranteed ordering; some paths run via idle callbacks.

**Hypothesis / improvement:** Add `fn after_page_change(state: &mut AppState, reason: PageChangeReason)` called from every page-mutating function. Consumers move inside in a deterministic order.

**Risk if ignored:** Each new post-turn-dependent feature adds another scattered call site, growing the "I forgot to call X" surface.

**Effort:** M

---

### F9. `viewport_page_for_line` replays the full forward walk on every render [design-improvement]

**Reference:** `bk/src/view.rs:55-71` — page number is a constant-time formula via cached chapter-relative offsets. linux-lit's substrate doesn't allow the formula, but the *cache* does.

**Linux-lit:** `src/input/navigation.rs:197-222` — runs `next_page_top` from line 0; each call walks heights. O(line_count²) GTK metric lookups per overlay-label refresh on long prose.

**Hypothesis / improvement:** Cache `Vec<usize>` of page-top indices on AppState, invalidate on `loading_work` flip or font/size change. `viewport_page_for_line` becomes a `binary_search`. Cache is also load-bearing for a future "go to page N" feature.

**Risk if ignored:** Frame stutter on overlay-label refresh; perf cliff scales with work length and inverse font size.

**Effort:** M

---

### F10. View-trait pattern for overlay/mode dispatch [design-improvement]

**Reference:** `bk/src/view.rs:13-18` — `View` trait with `render`/`on_key`/`on_mouse`/`on_resize`. Each mode is a struct; mode swap is `bk.view = &Page`.

**Linux-lit:** `src/input/keymap.rs` is a layered if/else dispatch keyed on overlay visibility. Each new overlay grows the chain; isolated testing requires reproducing full state.

**Hypothesis / improvement:** Define `trait OverlayMode { fn on_key(&self, state, key) -> KeyResult; fn on_resize(&self, state); }`. Pagination-relevant payoff: `on_resize` becomes a uniform hook that fires `resnap_page` (F4) for the active overlay.

**Risk if ignored:** Refactor pressure as more overlays are added. Pagination payoff is indirect — ranked last.

**Effort:** L (do as part of a larger keymap refactor, not standalone for pagination).

## Out of scope

- **Touch / scroll-velocity snap** (`paginator.js:804-822`) — linux-lit has no touch input plan; the gamepad path is discrete-event.
- **CFI as a portable location format** — pagination-adjacent but properly belongs to a `location-addressing` review.
- **Scroll-mode (non-paginated) flow** (`paginator.js:292-308` `scrolled`) — linux-lit's Scroll mode is `center_cursor`-based and works differently; comparison would be substrate-level, not algorithmic.
- **Foliate's column / RTL / vertical writing-mode handling** (`paginator.js:178-187` `getDirection`) — linux-lit is single-column LTR; not applicable today.
- **Foliate's `setStyles` re-flow on font load** (`paginator.js:1116`) — linux-lit reloads the buffer rather than restyling; different mental model.

## Suggested next step

Implement F1 first — smallest change, highest likelihood of fixing a class of intermittent user-visible bugs. F2's synchronous clip update naturally pairs with F1 since both relate to the timing of state mutations around `set_page`; consider F1+F2 as a single batch. F3 (descender guard) and F4 (resize observer) are independent S-effort wins that can ship in any order. F5 cleans up the resume edge case. F6–F10 are larger and should each get their own design pass before implementation.
