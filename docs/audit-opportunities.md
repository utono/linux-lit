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

## Batch 7 (audited 2026-07-16, post chat-panel + Tab/a keybind rework)

Fresh scan over the surface merged since Batch 6 (chat panel pin/regate, Tab/`a`
keybind ownership, picker abbrev ranking, echo legend). Three parallel Explore
finders; every entry below verified by my own direct side-by-side read, not agent
word (the #11 lesson) — two agent claims were corrected in the process (see #83's
scrim/box note and #85's `voice_picker` exclusion). Ranked by
(duplication × drift_risk) ÷ scope_size.

## #83 — echo legend route-to-shared (the #67 pattern, with visible drift)

- **Status:** OPEN (rank #1 — a shipped helper re-inlined, and the copy has
  ALREADY drifted on screen).
- **Signal:** `echo_keybinds_overlay.rs:1-95` hand-rolls the entire widget that
  #50's `keybinds_legend::{KeybindsLegend, build_legend}` owns: its own struct
  `{container, scrim}`, its own row loop, and `attach_to`/`show`/`hide` bodies
  that are **byte-identical** to `KeybindsLegend`'s (keybinds_legend.rs:28-41).
  `keybinds_legend.rs:4` names it outright — *"Modeled on `echo_keybinds_overlay`;
  factored here so the three legends share one layout"* — i.e. echo is the
  ORIGINAL that got factored and then left behind. It is the lone holdout at
  every legend seam: the wrapper (#50), the `open_overlay_legend` setter (#51 —
  its `slash` arm at keymap.rs:2903-2907 hand-inlines the show+set_mode pair),
  and the data-only file shape (the other three are pure `TITLE` + `GROUPS`).
- **NOT behavior-preserving — this is the entry's whole point.** The copy has
  already drifted VISIBLY: echo uses `gloss-scrim` (opaque) + `picker-box` (dark
  `{root}`, theme.rs:1296) where the shared path uses `legend-scrim` (30% dim,
  :1179) + `legend-box` (parchment `{bg}`/`{fg}`, :1301). So the echo legend
  renders as a dark card behind an opaque scrim while the other four render as
  parchment behind a dim. Routing it through the wrapper is a **visible fix**,
  not a pure extraction — ship it as a `fix(`, not a `refactor(`, and verify on
  screen (the four legends should look identical).
- **Identical part (deletes):** the ~65 lines of widget construction +
  attach/show/hide. Echo keeps only `TITLE` + its binds, converted from the flat
  `BINDS: &[(&str,&str)]` to grouped `GROUPS: &[Group]` — that grouping is a
  judgment call on its 17 rows (Navigation / TTS / Curate / View), NOT mechanical.
- **Variants:** echo's rows use `width_chars(16)` vs the shared `15`; its title
  card is `width_request(420)` vs the shared two-column 380×2 — both disappear
  into the shared layout (part of the visible fix).
- **EXCLUDED:** the 3 usage sites stay as-is in shape (app/mod.rs:647 field,
  :1639 ctor, keymap.rs:245 mode arm) — only the type name changes. The `slash`
  arm folding into `open_overlay_legend` is a second, separable cut; do #83's
  widget first.
- **Rank inputs:** copies=1-but-shipped-helper, drift_risk=**proven** (already
  drifted on screen), scope=small.

## #84 — db/queries.rs echo block → src/db/echoes.rs

- **Status:** OPEN (rank #2 — the one clean seam left in a 4335-line file).
- **Signal:** queries.rs:2316-2722 (~410 lines, one unbroken run from
  `pub struct EchoCandidate` to `delete_echo_link`) is the whole cross-work echo
  persistence layer: the 4 types, `decode_embedding`/`cosine_similarity`
  (private, used only here), `find_similar_passages`, `ensure_echo_tables`, and
  the echo_turns/echo_links CRUD.
- **Why it moves cleanly (verified directly, not from agent word):** the block
  uses **zero** of queries.rs's 16 file-level imports — I grepped all of them
  (`line_types`, `models::{Line,Work,MediaItem,Timestamp,TimeRange,WorkSummary}`,
  `scansion`, `HashMap`, `OpenFlags`, `open_db`, `db_path`, `OPEN_DB_PANIC_MSG`):
  0 hits each. It needs only `rusqlite::{Connection, OptionalExtension}` + 
  `params!`. Its sole crate deps — `crate::db::affect`, `crate::db::echo_channel`
  — are already `src/db/` siblings written as fully-qualified paths, so they move
  verbatim. Zero AppState coupling (the one `state.` hit in the range is the word
  "state" in a doc comment on toggle_echo_curated). `db/mod.rs` already has the
  sibling-module pattern (affect, echo_channel, chunks, concordance, play_pages).
- **Identical part (moves):** the whole run, with a fresh 2-line import header.
- **EXCLUDED:** the `ensure_*` migration cluster (called from one contiguous run
  at app/mod.rs:3064-3074 — moving it splits DDL from its queries, buys nothing);
  the audio/voice cluster (3 consumer files, shares `get_character_gender_age` /
  `resolve_prose_voice` with the rest of the file — more coupled, not this cut);
  `line_id_for_location`/`search_lines` (sit after the block, unrelated, stay).
- **Safe-scope:** yes — pure code motion, callers change `queries::` → `echoes::`
  on those names only (echoes.rs ×33, visual.rs ×3, app/mod.rs ×1, the 2 pickers
  ×1 each). A `pub use` re-export in queries.rs would make it zero-caller-churn.
- **Rank inputs:** copies=n/a (motion), drift_risk=low, scope=small-mechanical.

## #85 — overlay toast literals (Saved / Saved (:q to exit) / No matches / Copied)

- **Status:** OPEN (rank #3 — four literal families, 3-4 sites each, zero risk).
- **Signal:** each string+duration pair is byte-identical across its sites, all
  through `show_chapter_toast_secs`:
  - `"Saved (:q to exit)"`, 2s ×3 — synopsis.rs:363, gloss.rs:1200, journal.rs:1481
  - `"Saved"`, 2s ×4 — synopsis.rs:360, gloss.rs:1185, journal.rs:1477, chat.rs:616
  - `"No matches"`, 2s ×4 — gloss.rs:249,282, journal.rs:736,769
  - `"Copied"`, 2s ×3 — keymap.rs:1271, 2553, 2618
- **Identical part (extract):** four consts beside the shipped journal toast
  consts (#70's precedent). The `"Saved"` / `"Saved (:q to exit)"` pair sits at
  three of the SAME call sites (the `:q` variant is the in-overlay branch) — a
  `saved_toast(s, in_overlay: bool)` is tempting, but that adds a branch three
  sites currently express as two literal call sites. Per the #34 lesson, prefer
  two plain consts over one helper with a flag.
- **EXCLUDED:** the `"Saved"` / `"Copied"` occurrences in doc comments
  (navigation.rs:2550,2587, app/mod.rs:691,699, keymap.rs:3451) — prose, not
  call sites; `"Save failed"` (chat.rs:620,762) and `"Rewritten"` (chat.rs:1064,
  journal.rs:2027) are 2-site, at the floor, no drift signal — flag only.
- **Safe-scope:** yes — same string, same sink, same duration.
- **Rank inputs:** copies=14 across 4 families, drift_risk=med (a wording fix
  currently needs 3-4 hand-edits), scope=tiny.

## #86 — top-anchored picker preamble (margin_top 40 + width)

- **Status:** OPEN (rank #4 — 4-site preamble; the const IS the signpost).
- **Signal:** an identical 5-line preamble opens four picker constructors —
  `halign(Center)`, `valign(Start)`, `set_margin_top(40)`, `set_width_request(W)`,
  `add_css_class(...)`: echo_line_picker.rs:21-26, concordance_list_picker.rs:17-22,
  concordance_word_picker.rs:17-22, voice_picker.rs:26-33. Widths pair up
  (900 = line/occurrence lists ×2; 675 = word/voice lists ×2); `set_max_content_height(750)`
  repeats at echo_line:35, concordance_word:31, voice:45.
- **Identical part (extract):** `PICKER_TOP_MARGIN` (40), `PICKER_LIST_MAX_H`
  (750), and the width pair — or better, a `new_top_anchored_picker(width)` in
  picker_nav.rs beside #20's `new_picker_list()`.
- **EXCLUDED (corrected an agent claim — verify before scoping):** `voice_picker`
  takes `library-picker` (cream/themed), NOT `picker-box` (dark) — its comment at
  :31-32 says the divergence is deliberate. So a helper must take the css class
  as a param, or voice_picker is excluded from the css-bearing part and shares
  only the geometry. Also EXCLUDED: picker_nav.rs:138 (also 900, but the CENTERED
  `build_picker_card` 900×775 `library-picker` family — a false merge);
  concordance_picker.rs:24 `height_request(750)` (a fixed card height, NOT a
  scroll cap — same value, different concept); concordance_list_picker.rs:32 (940).
- **Rank inputs:** copies=4, drift_risk=med, scope=small.

**Batch 7 — below the floor (flag; number only if the family grows):**

- **segments.rs `cursor_lines` + div derivation** (selection_context:129-142 vs
  segment_context:169-179) — I read both directly: the filter_map body and the
  `.first().map(|l| (l.div1,l.div2)).unwrap_or((0,0))` tail ARE byte-identical,
  but `selection_context` has an `is_empty() → None` guard that `segment_context`
  deliberately lacks (it builds a `SegmentContext` with empty lines + `(0,0)`
  divs). A helper carrying the guard changes site 2's behavior; without it, the
  guard stays duplicated anyway. 2 sites, no drift signal → floor. NOTE the wider
  family: the same filter_map appears at visual.rs:525,562,671,780 — if a 5th+
  site lands, revisit as a cross-file `work_lines_in_range(state, work, range)`.
- **chat.rs free-space gate** (189-192 vs 269-272) — differs only by
  `main_card_rect(s)` vs `(&s)` (auto-deref, cosmetic), but the two failure
  branches differ (regate closes the panel; toggle only toasts). Extract at most
  `chat_free_space(s) -> i32`; 2 sites, weak signal → floor.
- **chat.rs `open_input(title, hint, &s.theme.cursor_bg, &s.theme.cursor_fg)`**
  ×4 (307, 311, 604, 747) — the trailing theme pair never varies, but each site's
  guard differs and it is already funneled through `prompt_title_hint(s)`. The
  real cut (drop the 2 params, pass `&s.theme`) is an API change → not this audit.
- **echo picker geometry 960×975** (echo_picker.rs:29-30, echo_turns_picker.rs:27-28)
  — 2 sites, same values, near-identical builders. Floor; revisit with the
  already-flagged echo_picker/echo_turns_picker row block.
- **chat panel left margin 24** (chat_panel.rs:31, chat.rs:82, 797, 798) — same
  concept as the SHIPPED `app::layout::CARD_OUTER_MARGIN` (=24); chat.rs:797
  already uses the const for `end` while hardcoding 24 for the left in the SAME
  expression. A route-to-shared drive-by, not a new const. Take it when next in
  the file.
- **passage prompt preamble** (chat.rs:679, 1014, journal.rs:1396) — similar, not
  byte-identical (journal interleaves an extra `{} text:` block); prompt wording
  is deliberately per-site. Judgment call, not a mechanical extraction → no.

**Rejected outright (verified, do NOT re-propose):**

- `save_passage_page` call shape (chat.rs:579-585 vs 721-727) — same shape, every
  argument expression differs (`e.div1` vs `div1`, `&e.question` vs `&q`). Only
  the 2-line `open_db_rw().and_then(|conn| {` scaffold is shared: too thin.
  journal.rs:2217's is one arm of a `JournalBand` match dispatching to three
  DIFFERENT fns — structurally different, exclude permanently.
- `handle_chat_prompt_key` vs `handle_chat_transcript_key` (keymap.rs:1283-1321 vs
  1326-1370) — same chords, but an `if`-chain vs a `match` with guard arms,
  dispatching to different fns (`focus_transcript` vs `focus_reader`), and the
  prompt tail falls through to `ask_vim_intercept` where transcript's `_ => true`
  swallows. Nothing byte-identical.
- **navigation.rs (4541) / keymap.rs (3979) / viewport.rs (3204) have NO clean
  seam** — 478 / 371 / 117 `state.` references; AppState-saturated by
  construction. The only pure fns are `preroll_seek_time` + `interpolate_cross_time`
  (~14 lines, already shipped as #24). Do not re-audit these for a carve-up: the
  honest answer is there is nothing to move.
- `"Q: "` / `"\nA: "` prefixes (~9 sites) — same two chars, but they split into
  display vs wire/prompt vs parse formatting with different separators. Genuinely
  different concepts; not one const.

## Batch 8 (audited 2026-07-17, post pinned-chat-panel + Question-view + transcript-nav batch)

Fresh scan over the ~30-commit chat-panel batch merged since Batch 7 (right-pinned
panel for single-column layouts, focused Question view, transcript gg/G + Ctrl-d/u
nav, auto-save follow-up Q&As, gloss/journal `t` toggle). Two focused Explore
finders (duplication + literals); the oversized-file finder was SKIPPED this batch
— Batch 7 established navigation/keymap/viewport have no clean seam and #84 already
claims queries.rs's one seam; the chat batch added no new large file. Every entry
verified by direct read (the #11 lesson). The batch is cleanly built — the
duplication finder returned **no qualifying duplication** (the render_transcript_*
trio, the landable-mask helpers, and the scroll family are all already minimally
decomposed). One numbered literal opportunity. Ranked trivially (only one).

## #87 — "Q: " question-row display prefix → route to prefix_question

- **Status:** OPEN (rank #1 — 5 raw sites of a display prefix that ALREADY has an
  idempotent shared form one module over; a wording change needs 5 hand-edits and
  the raw sites carry a latent double-prefix bug the shared form fixes).
- **Signal:** `format!("Q: {}", …)` builds a `TranscriptRow::Question` label
  byte-identically at **5 chat.rs sites** — build_transcript_rows:938,
  build_single_exchange_rows:973, journal_view_rows:1178,
  render_transcript_with_thinking:1540, render_saved_entry:2165 — while
  `ui/journal_overlay.rs:152 prefix_question` is the SAME `format!("Q: {}", …)`
  wrapped in an idempotent `starts_with("Q:")` guard. The five chat sites are the
  raw, unguarded copies of a helper that already exists.
- **Identical part (extract / route to):** the `"Q: "` display prefix. Route the
  five sites through `prefix_question` (or a chat-panel `question_row(q) ->
  TranscriptRow` wrapping it) so the label lives once. This is a route-to-shared
  (#32/#67 precedent), not a bare const — and it fixes a latent bug: a stored
  question already beginning `Q:` gets double-prefixed at the raw sites but not
  through `prefix_question`. (If any of the five must stay raw for a
  round-trip/parse reason, keep it and note why; verify each renders identically.)
- **EXCLUDED — the OTHER two "Q:" concepts stay separate (this is the Batch 7
  refinement):** Batch 7 rejected a single `"Q:"` const spanning all uses; that
  was right, but it missed that the DISPLAY sub-family alone is a clean cut. Keep
  excluded: (a) the WIRE/prompt form `"Q: {question}\nA: {answer}"` (journal.rs:2062,
  chat.rs:2013-2015) — Claude-prompt format, different concept; (b) the PARSE form
  `.strip_prefix("Q:")` (chat.rs:2339, vim/journal_doc.rs:8, journal_overlay.rs:153)
  — input parsing, different concept; (c) all `"Q: …"` occurrences in tests and
  doc comments. Only the display-label sub-family routes.
- **Safe-scope:** yes — identical rendered label (verify the five rows paint the
  same text; the double-prefix fix only changes already-`Q:`-prefixed input, which
  the raw sites render wrong today).
- **Rank inputs:** copies=5 (+1 shared form), drift_risk=med (5 hand-edits + a
  latent double-prefix bug), scope=small.

**Batch 8 — below the floor (flag; number only if the family grows):**

- **New chat toast literals, each 2 real call sites, one file, no drift signal:**
  `"No room for chat panel at this layout"`/3s (chat.rs:326,469 — +1 doc mention
  at :349, NOT a site); `"Waiting for the previous reply…"`/2s (chat.rs:588,2026);
  `"No passage at the cursor"`/2s (chat.rs:604,610); `"Entry is saved"`/2s
  (chat.rs:1938,1947); `"Save failed"`/3s (chat.rs:1984,2135). All at the floor.
  If a 3rd site of any lands, fold into #85's toast-const family (same precedent).
- **`from_millis(160)` chat flash duration** (chat_panel.rs:125 flash_transcript,
  :232 flash_rows) — 2 sites, same "brief wash fade-out" meaning; `CHAT_FLASH_MS`.
  EXCLUDE the `240` (flash_input, chat_panel.rs:113 — different value/widget) and
  the CSS `320ms` transitions (theme.rs — a distinct in-string value, not this
  Rust literal). Floor.
- **`reload_gloss_list` vs `reload_journal_list`** (chat.rs:1511 vs 1188) — the
  `open_db().ok().and_then(|conn| …find…().ok()).unwrap_or_default()` empty-on-
  failure skeleton, with a "mirrors reload_gloss_list's contract" comment at :1187
  (a drift signal). BUT only that skeleton is identical; the inner query
  (find_passage_pages vs find_glosses_by_start), its args, and the return type all
  differ — sharing needs a generic over the query. This is the SAME shape the
  Standing exclusions already retired for the citation-resolution / find_* query
  families (load-bearing varying token). Confirmed by the duplication finder
  independently. Do NOT number — record here so the "mirrors" comment doesn't lure
  a future batch.
- **chat.rs `"Q: {q}\nA: {a}"` wire/parse forms** — see #87's exclusions; the
  wire (×2) and parse (×3) sub-families are each below the floor on their own AND
  are different concepts from #87. Not one const.

**Batch 8 — rejected outright (verified, do NOT re-propose):**

- **render_transcript_thinking_gloss / _with_thinking / _with_error**
  (chat.rs:1499, 1529, 1546) — look like a family, but each builds a
  STRUCTURALLY different row set (`[GlossAnswer, Thinking]` vs `[Question,
  Thinking]` vs `transcript_rows + Error`). Only the `use … as R;` +
  `render_rows(&rows)` two-line scaffold repeats — too thin, same rejection basis
  as Batch 7's save_passage_page shape.
- **first/last/at-or-after landable helpers** (chat.rs:1660-1678) — each is a
  distinct one-liner over the mask (`position`/`rposition`/`find`); already the
  minimal decomposition, nothing to extract.
- **transcript_cursor_first vs _last** (chat.rs:1687 vs 1705) — 2 sites, differ in
  the edge arg (`false`/`true`) and index-fn (`first_`/`last_landable_index`);
  sharing needs a passed-in fn-pointer = abstraction, out of scope.
- **The `gg` PendingG chord preamble** (~9 sites incl. keymap.rs:1336) — a 2-line
  `chord == PendingG { chord = None;` idiom that predates this batch and diverges
  immediately after (different guards, returns, actions). Cross-overlay idiom, not
  new chat drift; too small/divergent to lift. (Its real fix is the long-parked
  picker/overlay key-dispatch project, already tracked.)
