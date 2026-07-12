# linux-lit audit opportunities

Numbered, safe-scope, behavior-preserving refactoring opportunities. Produced by
the `assess-maintainability` skill; consumed by the spec→plan→refactor→merge
pipeline. DONE entries stay (condensed) for numbering continuity — never reuse a
number.

**Two-tier by design:** a shipped entry lives here as a **one-liner** (`#5`–`#72`
below); an OPEN entry keeps its full analysis until it merges, then gets pruned
to a one-liner. This keeps the always-read bulk flat — the `assess-maintainability`
skill reads only the index of this file, never the whole thing.

**Full write-ups of pruned entries** move to `audit-opportunities-archive.md`
(the skill never reads it) and survive in each refactor commit. What stays
verbatim below is only what a fresh audit needs: the lessons, the standing
exclusions, and the larger-projects decisions.

<!-- #1–#4 were resolved before this ledger was created (shared AskCard,
     ask-card key intercept, AppState.picker rename, gloss_block_voice extract).
     Numbering continues from #5; those four numbers are retired. -->

## Lessons (keep applying)

- **The #11 lesson:** confirm bodies are byte-identical by direct side-by-side
  read before merging — a same-named pair (`shorten_title`) was behaviorally
  different. Never number from an agent's word or a grep hit alone.
- Two-site families are at the floor: number one only with a documented drift
  signal (a "mirror this fix" comment, a hand-copy that already drifted once);
  otherwise flag it and wait for a third site.
- Prefer route-to-shared over new helpers: check whether an existing helper
  already covers the body (new code re-inlining a shipped helper is textbook
  drift — see #67).
- When a helper would need a guard/branch some sites don't have, that is a
  behavior change — split into per-variant helpers (#34) or exclude.

## Shipped (#5–#72 — all DONE or closed)

- **#5** footer-row-builder (c976e2c) — `ui/footer.rs::build_footer_row` for the
  gloss/journal footer row.
- **#6** picker-nav-helper (371abd8) — `picker_nav::select_row_at` tail of 13
  `move_selection`s; index-calc variants stayed at call sites.
- **#7** claude-bridge-helper (d53580b) — `claude_bridge::run_claude_request`.
- **#8** sentinel-key-constants (63c9779) — named whole-work/journal-work
  scene-key sentinels.
- **#9** transient-toast-helper — `ui/toast.rs::show_transient`; ~30 auto-hide
  closures. (Follow-on flagged: the `debug_icon` flash is the same primitive on
  another Label — future #N if touched.)
- **#10** subsequence-match-helper — `picker_filter::subsequence_match`, 5
  pickers.
- **#11** shorten-author-helper (narrowed) — `concordance::shorten_author`
  promoted; the two `shorten_title`s are NOT identical and stay split.
- **#12** sync-suppress-window-const — `SYNC_SUPPRESS_SEEK` (500ms, ×8) +
  `SYNC_SUPPRESS_INDEFINITE`.
- **#13** picker-attach-helper — `picker_attach::attach_panel`, 10 pickers.
- **#14** citation-format-helper — `db::models::citation`, ×6.
- **#15** listbox-clear-helper (017fe18) — `picker_nav::clear_list`, ~15 sites.
- **#16** block-visual-key-twin (fe8a563) — `handle_block_visual_key` +
  `SYNOPSIS/GLOSS_VISUAL_CFG` plain-fn-pointer configs (no trait).
- **#17** load-work-titles-helper (017fe18) —
  `queries::load_work_titles_or_default`, ×6.
- **#18** open-db-rw-or-log (e5bf8ec) — timestamps.rs file-local helper, the 5
  pure-A sites.
- **#19** open-db-message-const (017fe18) — `OPEN_DB_PANIC_MSG`, ×14.
- **#20** picker-list-scaffold (e5bf8ec) — `new_picker_list()`; builder (A) and
  imperative (C) variants deliberately NOT unified.
- **#21** picker-header-scrim (e5bf8ec) — `build_picker_scrim` +
  `build_picker_header`.
- **#22** select-first-row (bc5e612) — `picker_nav::select_first_row`, ×15.
- **#23** selected-index (bc5e612) — `picker_nav::selected_index`, 5 pickers
  delegate.
- **#24** preroll-seek-time (d06ddb8) — `navigation::preroll_seek_time`, ×9;
  `CHUNK_PREROLL`/`TURN_PREROLL` ab-loop prerolls excluded (distinct concept).
- **#25** mpv-set-property-cmd (681db30) — client.rs `set_property_cmd`, ×6.
- **#26** mpv-seek-absolute-cmd (681db30) — `seek_absolute_cmd`, ×4.
- **#27** card-side-margin (f8e9459) — `ui::card_side_margin`, 9 sites. **The
  echo `column_width / 8` family is a DIFFERENT concept — never merge it with
  `card_width / 4`** (a documented past bug class).
- **#28** parse-citation-reuse (95c7343) — 4 `cite_tail` closures deleted →
  `app::parse_citation`.
- **#29** journal-page-row-mapper (5900e79) — `map_journal_page_row` +
  `JOURNAL_PAGE_COLUMNS`.
- **#30** overlay-attach-body (694ac40) —
  `picker_attach::attach_overlay_panel`, ×3.
- **#31** reassert-italic-tags (43a5f71) — `ui::reassert_italic_tags`, ×2
  ("mirror this fix" comment was the drift signal).
- **#32** gloss-overlay-clip-route-to-shared (b9c7d26) — gloss's private
  `display_rows`/`recompute_bottom_clip` deleted; routes to `ui::mod` shared.
- **#33** two-label-picker-row (66c844a) — `picker_nav::two_label_row` +
  `speaker_prefixed_first_line`, 3 card pickers.
- **#34** picker-move-selection-two-families (6d7dc7f) —
  `move_selection_clamped` (×5) / `move_selection_from` (×4); TWO helpers keep
  the clamp-vs-no-clamp contract.
- **#35** picker-card-builder-600x400 (a2a7f54) —
  `picker_nav::build_picker_card`, ×4.
- **#36** gloss-normalize-abbrev-reuse (8abc18a) — 2 inline `-Amb` strips route
  to `gloss::normalize_abbrev`. **queries.rs's strip-suffix GUARD and
  `base_work_abbrev` (first-`-` superset) are NOT interchangeable with it.**
- **#37** column-exists-pragma (3b560fc) — `queries::column_exists`, ×3; the
  error-swallowing `works.default_voice_id` probe stays excluded.
- **#38** claude-bridge-async-render-tail (062ceed) —
  `persist_render_install_gloss`, ×4.
- **#39** overlay-close-position-restore (0a37259) — `restore_saved_position`
  (+`_resnap`), 7 of 8 sites; the search-Escape else-arm site excluded.
- **#40** timestamps-line-id (daba8bc) — `work_line_id`, ×5.
- **#41** timestamps-sign-column-setter (c2bb5e6) — `set_sign_columns`, ×4.
- **#42** unspoken-stage-direction-refusal (47909ed) — spoken-line gate helper +
  const toast literal, ×2; nudge/delete stay ungated by design.
- **#43** word-prefix-boundary (0256fe9) — `is_word_prefix`, ×3.
- **#44** gloss-render-current-row (959b09b) — `render_gloss_row`, ×2.
- **#45** gloss-row-map-closures (8d73eb4) — `row_to_saved_gloss` /
  `row_to_glossed_passage`.
- **#46** apply-font-to-views — `ui::apply_font_to_views` (gloss+journal
  per-view loop body).
- **#47** cached-coloring-span — `ui::apply_cached_coloring`.
- **#48** bar-stroke-loop — `ui::draw_bar_spans`.
- **#49** visual-mode tail (partial) — `visual_selection_count` only; the rest
  stays per-overlay until a third block-cursor overlay appears.
- **#50** keybinds-legend-wrapper — one `KeybindsLegend` struct; per-overlay
  files keep only their `GROUPS` const. echo_keybinds_overlay excluded (does not
  route through `build_legend`; folding would change its look).
- **#51** overlay-legend-show-mode-setter — `open_overlay_legend`, symmetric
  partner of the shared close path.
- **#52** overlay-panel-attach (9e9fc71) — panel draw-func + queue_draw wiring
  shared. **The clip-guard attach ORDER differs per overlay and is
  load-bearing — stays per-file.**
- **#53** set_rc_color family (305d02b) — `ui::set_rc_color`, ×5;
  `set_highlight_color` excluded (String + apply_hi_color, different shape).
- **#54** bar-draw shared prefix (aa85226) — `ui::draw_vim_block_cursor`; bar-x
  and block-x sources stay per-file.
- **#55** header-tag triple (305d02b) — `gloss_render::header_tag_base`, ×4.
  **The color-application split (gloss hex `&str` vs journal `RGBA`) stands and
  is load-bearing — do not reconcile without normalizing color storage.**
- **#56** PANEL_PAD / PANEL_RADIUS consts (9e9fc71).
- **#57** body-indent 60 — RESOLVED by the indent redesign (2b03c7a):
  `JOURNAL_BODY_INDENT` aliases `QUOTE_BODY_INDENT`; the raw gloss `- 60` is
  gone.
- **#58** BAR_TEXT_GAP — RESOLVED by the #57 redesign; no bare 12 remains.
- **#59** journal apply_font dead empty-guard removed (305d02b).
- **#60** OVERLAY_BOTTOM_PAD (journal 28 vs gloss 80) — **CLOSED, keep 80.** The
  on-screen check couldn't be run; the 80 may be load-bearing (denser gloss
  content + clip guard + pagination capacity). Re-open only with a visual check.
- **#61** buffer-line-for-line-id (8be7961) —
  `navigation::buffer_line_for_line_id`, 3 sites + 1 restructured variant.
  **The canonical-check resolvers (concordance, app resume/target) have
  different contracts/fallbacks — hard-excluded.**
- **#62** readonly-textview triple (bb03dc8) — `ui::set_view_readonly`, ×5; vim
  per-mode focus toggles stay inline (different lifecycle).
- **#63** gloss set_bar_color_from_root (a9a83dd) — draw-free private method,
  ×4. Deliberately NOT routed through `set_rc_color` (would add a redundant
  draw per show).
- **#64** gloss hide-diff-labels (a9a83dd) — `hide_diff_labels()`, ×5, +
  `set_prose_margins` rider.
- **#65** mpv discover-or-launch-blocking (576684b) —
  `discovery::discover_or_launch_blocking`, ×2; names the 60×50ms socket wait.
- **#66** current-line-id (8be7961) — `current_line_id` beside #61's helper.
- **#67** journal pragma-probes (refactor/audit-67-72) — routed 3 re-inlined
  probes to the existing `column_exists` (#37). Textbook route-to-shared drift.
- **#68** raise_tag_to_top (refactor/audit-67-72) — ×3, 2 files.
- **#69** AUTHOR_DIV sentinel dedup (refactor/audit-67-72) —
  `db::journal::AUTHOR_DIV` single source; app re-exports (db is the lower
  layer).
- **#70** journal toast literal consts (refactor/audit-67-72) — 3 consts, 9
  `show_transient` sites; durations stay inline.
- **#71** markdown heading-tag closure (refactor/audit-67-72) — heading-only
  local closure; the other 7 tag builders are not congruent, kept separate.
- **#72** current_work_abbrev getter (refactor/audit-67-72) — file-local, ×5.

## Standing exclusions — examined, do NOT re-propose

Each was analyzed in a past batch and rejected for cause. Re-open one only if
its stated condition changes (usually: a new site appears, or the divergence is
proven cosmetic on screen).

**Different-by-design (behavior-changing to unify):**

- Main reading card's PAGINATED `update_bottom_clip` (scroll.rs) vs the
  free-scroll partial-row mask — different strategies, never merge.
- Gloss vs journal `snap_value_to_line` — different algorithms (per-row snap vs
  uniform `row_step`), not duplicates.
- Gloss vs journal overlay top margins — per-surface intent, not drift.
- The dim-header mechanism — gloss bakes color into render-time tags (hex);
  journal post-render tags a line (RGBA). Structurally divergent; keep parallel.
- vim `exit_edit_buffer` near-twin — statement ORDER differs + an extra
  `clear_block_cursor`; vim lifecycle is behavior-risky to reorder.
- Citation/id → buffer-line resolution family (~12 sites): each differs in a
  load-bearing token (id vs tuple key, round-trip check present/absent, panic vs
  fallback access). The one byte-identical pair includes a test helper that
  DELIBERATELY duplicates prod. (The missing round-trip check at 5 sites is a
  latent /code-review flag, not a dedup.)
- Timestamp upsert family + `find_*` journal query skeletons + dynamic-IN
  param scaffolds + `ensure_*_table` bodies: the varying SQL is load-bearing;
  folding needs parameterized SQL/generics — out of scope permanently.

**Below the floor (flag; number only if the family grows):**

- `move_selection` third shape (`+.max(0)`: echo/echo_turns/authorship);
  voice/library carry extra logic — verify before ever routing.
- Standalone picker j/k arms (echo/echo_turns/library) — fix is routing through
  `picker_keys::resolve_picker_key`, a follow-on to picker-dispatch, not a cut.
- seek-then-suppress single statement; the wl-copy stdin-pipe (×3) vs arg-form
  (×4) split (merging changes arg→stdin behavior).
- settings_overlay arrow-label format (~14 sites, one file) — drive-by only.
- echo_picker vs echo_turns_picker row block (2 sites, field name differs).
- `"Error: {}"` prefix (different sinks); `format!("%{}%")` LIKE wildcard;
  `has_column`/`created_at strftime` SQL fragments.
- close-overlay-and-restore tails (2-site families with near-miss siblings);
  journal_overlay reveal/`restore_card_size` tails (single-file, low drift).
- `band_for_page` author remap (journal.rs, 2 sites — clean cut, deferred at
  the floor); author-sentinel predicate (1-line, value drift already killed by
  #69); close-journal-to-reader 4-liner (2 sites, near-misses differ).
- `return_pos = Some((s.current_line, s.page_top_line))` (journal ×5 + gloss
  ×3) — a cross-family `current_pos(s)` candidate; revisit as a cross-cutting
  cut only.
- app/mod.rs canonical-check resolver pair (resume vs concordance-target) —
  2 sites, one fn's flow, load-bearing startup logic.
- async reader-gloss spawn+save+render tail + cache-hit show-gloss block —
  extraction needs ~6 params or a claude_bridge callback-shape change (an API
  change, not pure extraction).

## Larger projects (not safe-scope) — all resolved or parked by decision

Behavior-changing, multi-PR work tracked here so it never gets numbered.

- **InputMode → picker dispatch** — DONE (`Picker` trait +
  `picker_for_mode`; nav + plain-hide arms collapsed; `open_picker_mode` for
  the open pairs). Confirm dispatch stays bespoke by design — honest
  duplication, do not abstract.
- **AppState god-struct grouping** — committed scope COMPLETE (Phases A–G: all
  seven contained clusters — nav_test, journal, word_cycle, echo_overlay,
  page_image, scansion, vocab_popup — grouped; both init variants and both
  verification tiers proven).
- **app.rs carve-up** — Phases 1–3 DONE (`mod.rs` 6735 → ~3,950 across
  vocab_popup/font/text_prep, formatting/scene_synopsis/translations, layout).
- **gloss_overlay.rs split** — DONE (gloss_block / gloss_ipa / gloss_util
  siblings; the GTK buffer-population code correctly stayed).
- **DECISION (2026-06-24, do not re-litigate):** the two remaining items stay
  parked permanently — grouping the AppState CORE fields (`buffer`,
  `current_line`, `current_work`, `config`, `text_view`, `input_mode`; 90–290
  render-tier sites each for ~zero readability gain) and the `build_window` /
  `display_work` split (blocked on that core split; mod.rs is already
  navigable). Only a specific concrete pain re-opens a slice — "finish the
  section" is the wrong reason.
- **Clip-prevention unification (2026-06-25)** — free-scroll covering math
  unified into the tested shared helper; the deliberate non-unifications are
  recorded under Standing exclusions.

## Batch 6 (audited 2026-07-12, post regex-search + tint-guard-rails + chat/vocab-float work)

Fresh scan over the post-2026-07-02 surface (regex search, theme.rs contrast
guard rails, chat panel, vocab popup 2-col float, segment-overlay cycle,
keybinds legends). Three parallel Explore finders; every entry below verified
by direct side-by-side read, not agent word (the #11 lesson). Ranked by
(duplication × drift_risk) ÷ scope_size.

## #73 — search match-iters triple

- **Status:** OPEN (rank #1 — 3 byte-identical 8-line bodies in the code the
  regex-search branch just touched twice).
- **Signal:** the match-char-range computation — `line_end` +
  `forward_to_line_end` guard, `line_text` slice, `char_start`/`char_end` via
  `.chars().count()`, `match_start`/`match_end` via `forward_chars` — is
  byte-identical at **3 sites** in `src/input/search.rs`: `apply_highlights`
  :541-551 (loop body), `apply_current_highlight` :564-573,
  `remove_current_highlight` :584-592.
- **Identical part (extract):** `fn match_iters(state: &AppState, m:
  &SearchMatch) -> Option<(TextIter, TextIter)>` returning the
  `(match_start, match_end)` pair; `None` on the `iter_at_line` failure. Each
  site keeps its own None arm (`continue` in the loop, `return` in the
  singles) and its own tail (apply `search_tag` / apply `search_current_tag`
  + log / remove `search_current_tag`).
- **EXCLUDED (named, why):** the `search_matches.is_empty()` +
  `[search_match_idx]` preamble of the two singles (the loop site iterates
  instead — different access shape, stays at call sites); the tag operations
  and the `log_fmt!` (the load-bearing per-site difference).
- **Safe-scope:** yes — pure iterator computation → Option helper; guards
  stay at call sites, zero control-flow change.

## #74 — column-float-rect twin (chat float ↔ vocab float)

- **Status:** OPEN (rank #2 — 2 sites, cross-file, hand-copied THIS WEEK with
  an explicit "same as the chat float" comment; the exact #31-class mirror
  signal).
- **Signal:** the two-column float geometry — `col.compute_bounds(&window)
  .map(|b| (b.x() as i32, b.width() as i32)).unwrap_or((24,
  MIN_TWO_COLUMN_COLUMN_WIDTH))` + the divider-extension block
  (`d_left`/`d_right`, `new_x = d_left.min(x); w += x - new_x; x = new_x`
  vs `w = w.max(d_right - x)`) — is byte-identical modulo the side predicate
  at **2 sites**: `src/app/vocab_popup.rs:109-124` (`position_vocab_popup`)
  and `src/input/actions/chat.rs:731-747` (`size_panel` float arm). The
  vocab doc comment says "mirroring the chat panel's float geometry (see
  `chat::size_panel`)".
- **Identical part (extract):** `pub(crate) fn column_float_rect(s: &AppState,
  over_right: bool) -> (i32, i32)` in `src/app/layout.rs` (the module owning
  `main_card_rect`), returning `(x, w)`. Normalize the predicate: vocab's
  `over_right` ≡ chat's `FloatRight` (the branch bodies line up exactly once
  the polarity is normalized — verify the arm pairing during the cut).
- **EXCLUDED (named, why):** each caller's tail — vocab
  `place_float(x, w, card_h)`; chat `set_margin_start(x.max(0))` +
  `add_css_class("chat-panel-float")` + `size_to(w, h)` — and chat's Pinned
  arm. The column-pick two-liner could fold in via the normalized flag but
  the `ChatPlacement` enum stays chat-side.
- **Safe-scope:** yes — pure geometry extraction; the next divider/margin fix
  currently has to be hand-copied between the two floats.

## #75 — search scan-loop block (execute ↔ collect twins)

- **Status:** OPEN (rank #3 — ~14 lines byte-identical modulo rustfmt line
  breaks, one file, same fresh regex-search family as #73).
- **Signal:** the matcher scan — `let mut new_matches = Vec::new(); if
  state.line_map.is_some() { buffer text + lines().enumerate() +
  collect_line } else { work.lines.iter().enumerate() + collect_line }
  state.search_matches = new_matches; apply_highlights(&state);` — is
  byte-identical (only the `.text(...)` call's line-wrapping differs) at
  **2 sites** in `src/input/search.rs`: `execute_search_with_query` :44-62
  and `collect_matches` :315-331.
- **Identical part (extract):** `fn scan_matches(state: &AppState, work:
  &Work, re: &Matcher) -> Vec<SearchMatch>` (the if/else + collect); each
  caller keeps its own `state.search_matches = ...` assignment +
  `apply_highlights` if preferred, or the helper covers through
  `apply_highlights` (both sites run the same two statements next — either
  boundary is behavior-preserving; pick one in the spec).
- **EXCLUDED (named, why):** the two functions' wider shared prologue
  (clear_highlights / `search_matches.clear()` / empty-query early-return /
  `last_search_query` set / `current_work` match) — near-identical but the
  receivers differ (`&mut AppState` flows vs the borrow shapes) and the
  empty-query arms differ (`update_counter(0,0)` only in one) — folding the
  whole body is a bigger, riskier cut; ONLY the scan block extracts.
- **Safe-scope:** yes — byte-identical block → helper, no control-flow change.

## #76 — theme.rs contrast_ratio routes to relative_luminance

- **Status:** OPEN (rank #4 — route-to-shared drift on the WCAG formula, the
  one place a constant tweak would silently fork the math).
- **Signal:** `contrast_ratio` (theme.rs:666-674) re-inlines the exact
  `relative_luminance` body (theme.rs:110-114) as local `lin`/`lum` closures
  — the sRGB linearize (`0.03928 / 12.92 / 1.055 / 2.4`) and the
  `0.2126/0.7152/0.0722` weighted sum are byte-identical.
- **Identical part (route):** delete the closures;
  `let (la, lb) = (relative_luminance(a_hex) + 0.05,
  relative_luminance(b_hex) + 0.05);`. Identical f64 result (same
  `hex_to_rgb`, same formula).
- **Rider (same-PR, same file):** `hue_distance` (theme.rs:647-648) calls
  `hex_to_rgb(cN)` three times per color to feed `rgb_to_hsl` —
  `complement_hex` (:657-658) already shows the clean destructured form. A
  tiny `fn hue_of(hex) -> f64` (or inline destructure) collapses both lines
  and the same idiom in the `complement_rotates_hue_180` test.
- **EXCLUDED (named, why):** theme.rs `hex_to_rgb` vs gloss_util
  `parse_hex_color` — NOT duplicates (infallible `.unwrap_or(0)` + `len<6`
  vs `Option` + strict `len!=6`; different contracts, keep both).
  `darken_color` vs `blend_colors` — different arithmetic, not a pair.
- **Safe-scope:** yes — route-to-shared (#32/#67-style) + a pure destructure.

## #77 — distinct-tint guard-rail literal consts (40.0 / 1.4 / 0.50)

- **Status:** OPEN (rank #5 — #12-style literal naming inside the guard-rail
  fn the recent tint commits kept editing).
- **Signal:** inside `ensure_gloss_color_min` (theme.rs): the hue-distance
  "reads as a different color" threshold `40.0` at **4 sites** (:703, :715,
  :717, :720); its paired fallback contrast `1.4` at **2 sites** (:717,
  :720); the saturation floor `s.max(0.50)` at **2 sites** (:730, :753).
  The doc comment (:677) already states the rule ("hue distance ≥ 40° OR
  contrast ≥ 1.4") — the values just aren't named.
- **Identical part (extract):** `const HUE_DISTINCT_MIN_DEG: f64 = 40.0;`,
  `const TINT_DISTINCT_MIN_CONTRAST: f64 = 1.4;`, `const TINT_MIN_SATURATION:
  f64 = 0.50;` beside the other `READER_GLOSS_*` consts. Optionally hoist the
  second `let s2 = s.max(...)` (:753) — `s` is not mutated between the two
  passes, so computing once is behavior-preserving.
- **EXCLUDED (named, why):** the `4.5` values — ALREADY named
  (`READER_GLOSS_MIN_CONTRAST` / `VOCAB_WORD_MIN_CONTRAST` /
  `VOCAB_POPUP_DIM_MIN_CONTRAST`), deliberately distinct semantics despite
  equal values — do NOT merge. The two lightness-rung arrays (:731-737 vs
  :754-758) differ in values AND order (fine-grained forward sweep vs
  reordered last-resort sweep) — load-bearing, do NOT unify. The
  `ensure_gloss_color_min` CALL sites (4) differ in every argument — a call
  cluster, not a duplication family; no cut there. Test-file copies of the
  literals may reference the consts or stay — implementer's choice.
- **Safe-scope:** yes — literal → named const, #8/#12-style.

## #78 — titlecase_first route-to-shared

- **Status:** OPEN (rank #6 — textbook #67-class drift: a tested helper
  exists and two newer sites re-inline it).
- **Signal:** `journal.rs:20 fn titlecase_first` (private, unit-tested) is
  re-inlined at **2 sites**: `vocab_journal.rs:201-207` (character-identical
  body as a block expression) and `chat.rs:361-364` (ASCII variant:
  `unit_label.get_mut(0..1)` + `make_ascii_uppercase`).
- **Identical part (route):** promote `titlecase_first` to `pub(crate)`;
  both sites become `let unit_label = titlecase_first(unit);`.
- **Equivalence note (verify in the spec):** chat's ASCII form is
  output-identical only because `unit` is always `genre_unit`'s static
  `"scene"`/`"chapter"`/`"book"` — ASCII lowercase. That domain fact makes
  the routing behavior-preserving; state it in the spec (the #66 precedent:
  trivially-equivalent shapes, helper picks one).
- **EXCLUDED:** none found — no other first-letter-uppercase sites.
- **Safe-scope:** yes — route to an existing tested fn; zero new code.

## #79 — vocab_popup internal twins (set_counter + clear_content)

- **Status:** OPEN (rank #7 — single-file, but the file was just rebuilt for
  the 2-col float and will be touched again).
- **Signal:** in `src/ui/vocab_popup.rs`: (a) the counter block `if total > 1
  { counter_label.set_text(&format!("{} / {}", index + 1, total));
  set_visible(true) } else { set_visible(false) }` byte-identical at **2
  sites** (:169-174 `update`, :306-311 `update_journal`); (b) the
  content-clear loop `while let Some(child) = self.content_box.first_child()
  { self.content_box.remove(&child); }` byte-identical at **3 sites** (:162,
  :248, :300).
- **Identical part (extract):** two private methods — `fn set_counter(&self,
  index: usize, total: usize)` and `fn clear_content(&self)`.
- **EXCLUDED (named, why):** `picker_nav::clear_list` is NOT the home for the
  clear loop — it takes a `ListBox`; this is a plain `GtkBox` content region
  (different type, keep the method local). `update_journal`'s extra
  `*self.journal_scroll.borrow_mut() = None;` (:303) stays at its site (not
  part of the clear).
- **Safe-scope:** yes — private-method extraction, byte-identical bodies.

## #80 — shared VIM_EDIT_GROUP legend const (gloss + synopsis)

- **Status:** OPEN (rank #8 — legend DATA that mirrors the ONE shared vim
  engine; a new vim bind currently needs three hand-edits).
- **Signal:** the 11-row `("Vim edit mode (after e)", &[...])` group is
  byte-identical at **2 sites** — gloss_keybinds_overlay.rs:35-47 and
  synopsis_keybinds_overlay.rs:27-39 — and identical-but-one-row at
  journal_keybinds_overlay.rs:36-48 (its Ctrl+v row reads "(also in the r
  ask prompt)" vs "(also in ask prompts)").
- **Identical part (extract):** a `pub const VIM_EDIT_GROUP` (whatever
  `Group`'s concrete type is) in `keybinds_legend.rs`; gloss + synopsis
  reference it in their `GROUPS` arrays.
- **EXCLUDED (named, why):** journal's copy — its one differing hint string
  is deliberate per-overlay wording (the journal ask prompt is bound to
  `r`); keep its full copy rather than parameterizing one row. This does NOT
  contradict #50's "GROUPS are data that must drift per overlay": the
  OVERLAY-specific groups stay per-file; only the vim-ENGINE group (same
  engine, same binds everywhere) is genuinely shared. If a `const` can't
  express the nested slice cleanly, a `pub fn vim_edit_group()` returning
  `&'static` data is the fallback — no macro.
- **Safe-scope:** yes — static-data dedup, rendered output identical.

## #81 — float-frame CSS fragment (vocab float + chat float)

- **Status:** OPEN (rank #9 — 2 sites, one format string, mirror documented
  in the vocab-float design doc).
- **Signal:** in `generate_css` (theme.rs) the float-frame recipe
  `background-color: {bg}; border: 1px solid alpha({fg}, 0.25);
  border-radius: 8px;` is identical at **2 selectors**:
  `.vocab-popup.vocab-popup-float` (:1016-1018) and `.chat-panel-float`
  (:1043-1045). Only the trailing `padding` differs (`12px 16px` vs `12px`).
- **Identical part (extract):** compute once as `let float_frame =
  format!("background-color: {bg}; border: 1px solid alpha({fg}, 0.25);
  border-radius: 8px;")` and interpolate `{float_frame}` into both rules;
  each keeps its own `padding`. (A shared `.float-frame` widget class would
  also work but touches two constructors — the format-local binding is the
  smaller, purely-textual cut with byte-identical CSS output.)
- **EXCLUDED (named, why):** the paddings (per-surface); the
  `.chat-panel-float .chat-panel-header/.chat-panel-rule` overrides
  (chat-only); `.vocab-popup`'s base (non-float) rule.
- **Safe-scope:** yes — identical generated CSS, verifiable by diffing
  `generate_css` output before/after.

## #82 — TEST_*_VIEWPORT_RECT log formatter

- **Status:** OPEN (rank #10, lowest — test instrumentation, but the e2e
  pixel harness GREPS these exact lines, so format drift breaks tests
  silently).
- **Signal:** the 7-line rect-format body `"{TAG} {} {} {}",
  r.x().round() as i32, r.y().round() as i32, r.width().round() as i32,
  r.height().round() as i32` recurs at **4 sites**:
  journal_overlay.rs:600-606 (`TEST_JOURNAL_VIEWPORT_RECT`),
  journal_overlay.rs:942-949 (`TEST_JOURNAL_ASK_VIEWPORT_RECT`),
  gloss_overlay.rs:1465-1472 (`TEST_OVERLAY_VIEWPORT_RECT`),
  scroll.rs:1115-1121 (`TEST_VIEWPORT_RECT`).
- **Identical part (extract):** `pub(crate) fn log_viewport_rect(tag: &str,
  r: &graphene::Rect)` in `src/logging.rs` (or ui/mod.rs) owning the rounded
  4-int format; callers pass their tag + already-computed rect.
- **EXCLUDED (named, why):** the bounds SOURCES differ (three use
  `sc.root()` + `compute_bounds(&root)`; scroll.rs uses
  `compute_bounds(&state.window)`) — stay at call sites. The guards differ
  (journal :598 adds `width>0 && height>0` inside `connect_changed`; the
  others are idle-once) — stay. The `unavailable (…)` else-log wording
  varies slightly per site — either pass the tag to a second tiny helper or
  leave the else arms inline (3 of 4 have one).
- **Safe-scope:** yes — format-body extraction; the greppable tag strings
  remain literal at call sites.

### Examined and EXCLUDED in Batch 6 (no clean cut — do NOT number)

- **execute_search_with_query ↔ collect_matches full-body merge** — the
  prologue/tail beyond #75's scan block near-match but the empty-query arms
  differ (`update_counter(0,0)` in one) and the receiver shapes differ.
  Only the scan block is numbered.
- **post-seek fade-skip pair** (search.rs :111-114 vs :260-265, plus a
  variant in highlight.rs) — only 2 common lines; the adjacent
  `suppress_sync_until` handling differs load-bearingly (conditional +
  clear-to-None vs unconditional). Below floor.
- **overlay_cycle close vs toggle_overlay close** — the `jumped` branch
  (jump-to-source vs always-restore) is the documented point of the cycle
  module (its own doc comment says so). Behavior-changing to merge.
- **echo_keybinds_overlay → KeybindsLegend routing** — still excluded (#50):
  echo is single-column, ungrouped, different CSS/widths; folding changes
  its look. Re-open only as a deliberate UX-normalization task, not dedup.
- **chat consolidate_transcript vs build_history_turns** — same `Q:`/`A:`
  surface, different data shape + dedupe logic. Not a pair.
- **`ensure_gloss_color_min` call cluster** (4 sites) — every argument
  differs load-bearingly; a shared call signature is not a duplication
  family.
- **vocab_popup label-builder chains** (~8 in-file) — differ by CSS class /
  margins / wrap-mode; collapsing needs a builder abstraction. Skip.
- **`(adj.upper() - adj.page_size()).max(0.0)` clamp** (chat_panel ×2,
  vocab_popup, journal_overlay) — 1-line GTK idiom, below floor.
- **`vocab_popup_ink(theme)` recomputed 3× in generate_css's format args** —
  CPU redundancy, not drift; bind once as a drive-by if touching the file.
- **chat flash / `flash_widget`** — already routed to the shared helper; the
  chat-only wash CSS + 160ms is single-site. No cut.
