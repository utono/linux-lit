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
- **#73** search-match-iters (849f9cfb) — `search::match_iters`, the byte→char
  range walk shared by the 3 highlight fns (returns char offsets so the
  SEARCH_HL log stayed byte-identical).
- **#74** column-float-rect (be8587c5) — `layout::column_float_rect`; vocab +
  chat floats share the divider-extension geometry (vocab `over_right` ≡ chat
  `FloatRight`).
- **#75** search-scan-loop (849f9cfb) — `search::scan_matches`; callers keep
  assignment + apply_highlights tails.
- **#76** contrast-ratio-route (07ed437b) — contrast_ratio routes to
  `relative_luminance`; hue_distance destructures hex_to_rgb once.
- **#77** tint-guard-consts (07ed437b) — HUE_DISTINCT_MIN_DEG /
  TINT_DISTINCT_MIN_CONTRAST / TINT_MIN_SATURATION; 4.5-valued consts stay
  distinct; rung arrays untouched.
- **#78** titlecase-route (6c518521; chat half in f17ba45f) — last re-inline
  (vocab_journal) routed; rider: CORPUS_NONE_FOUND const.
- **#79** vocab-popup-twins (0368550e) — `set_counter` + `clear_content`
  private methods (GtkBox, so clear_list was not the home).
- **#80** vim-edit-group (189d54f1) — `keybinds_legend::VIM_EDIT_GROUP` shared
  by gloss + synopsis; journal keeps its worded copy.
- **#81** float-frame-css — **CLOSED 2026-07-19, do not re-open.** Chat float
  diverged BY DESIGN (full-height edge panel); recorded in Standing
  exclusions.
- **#82** viewport-rect-log (d71e0ed2) — `logging::log_viewport_rect`; tags
  stay literal at the 4 sites.
- **#88** copy-to-clipboard (410fa6f0) — `ui::copy_to_clipboard`, the 9
  byte-identical wl-copy arg-form spawns (settings:592 EXCLUDED during
  implementation — it waits on `.status()`, a different contract; stdin-pipe
  family also stays).
- **#89** chat-base-padding-top (c0110266) — one `base_padding_top` table for
  class_pad + src_lead_extra_pad; stale 44px-era doc arithmetic corrected to
  the shipped 26/29 values.
- **#90** chat-toast-consts (abd52ae6) — 5 chat.rs toast consts +
  TOAST_REWRITTEN / TOAST_NOTHING_TO_REWRITE beside the #85 block.
- **#91** chat-placement-classes (3c5e94dd) — CLASS_PANEL_FLOAT/PINNED +
  CLASS_CARD_CHAT_SEAM, 10 sites.
- **#92** flash-class-helper (c4fcbe64) — chat_panel `flash_class(w, class,
  ms)` for the add/timeout/remove trio.
- **#93** chat-rows-move (445d07f2) — pure row-model core + 11 pure test mods
  → `chat_rows.rs` (chat.rs 3763→2563). ChatMsgCtx stayed (prompt-context,
  not row-model — a deliberate narrowing vs the entry).
- **#94** db-migrations-move (6a7b3f67) — 9 `ensure_*` fns + their column-DDL
  consts → `db/migrations.rs`; column_exists and the canonical-abbrev family
  stay in queries.rs.
- **#83** echo-legend-shared (43d3206) — echo_keybinds_overlay reduced to TITLE+GROUPS data; renders via shared KeybindsLegend (fixes the dark-card/opaque-scrim drift).
- **#84** echoes-module (5477d7a) — moved the ~410-line echo subsystem out of queries.rs to src/db/echoes.rs (+7 tests); pure motion.
- **#85** overlay-toast-consts (00d585f) — TOAST_SAVED/SAVED_IN_OVERLAY/NO_MATCHES/COPIED in navigation.rs, 14 sites.
- **#86** picker-preamble (a0f8ed5) — new_top_anchored_picker_box(width, css_class) + PICKER_TOP_MARGIN/WIDE_W/NARROW_W/LIST_MAX_H, 4 pickers.
- **#87** q-prefix-route (701ff71) — 5 raw format!("Q: {}") chat sites route to journal_overlay::prefix_question via question_row().

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
- Vocab-float vs chat-float CSS frames (ex-#81) — diverged BY DESIGN
  2026-07: chat float is a full-height edge panel (side borders only,
  radius 0); vocab float keeps the boxed frame. Never re-unify in CSS.

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
- echo legend `slash`-arm fold into `open_overlay_legend` (follow-up left open
  by #83, which shared the widget). keymap.rs's echoes `slash` arm hand-inlines
  the `show()` + `input_mode = EchoKeybindsOverlay` pair that the other three
  legends route through `open_overlay_legend(OverlayLegend::…)`. Adding an
  `OverlayLegend::Echo` arm is a small cut but touches the InputMode dispatch —
  number it only if that dispatch is being edited anyway.

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

## Batch 9 (audited 2026-07-19, post chat-pagination + add-vocab-word + journal-source work)

Audited 2026-07-19 over the post-2026-07-16 surface; ALL seven entries (#88–#94)
plus the whole #73–#82 backlog shipped the same day on refactor/audit-73-94
(merge 50be66fb). One-liners above; full analyses in the archive. What remains
below is the standing exclusion knowledge from this batch.

### Examined and EXCLUDED in Batch 9 (no clean cut — do NOT number)

- **vocab_journal_ask ↔ journal::ask_claude structural mirror**
  (vocab_journal.rs:120-276 vs journal.rs:2391-2540) — the file's own doc
  comment says "mirrors journal::ask_claude", but every shared sub-block
  varies in a load-bearing token (entity key word-vs-band, save fn, display
  enum, pending-guard storage). Folding needs a generic ask-flow abstraction
  — speculative generality, the vocab-add/vocab-journal/journal trio stays
  parallel by design. Larger-project material only if a FOURTH ask flow
  appears.
- **wrap_index concept triplicate** (chat.rs:1236 usize+delta `((x%n+n)%n)`;
  chat_pagination.rs:225 and picker_nav.rs:225 i32 `rem_euclid`) — same
  concept, three different signatures/idioms; unifying changes call-site
  types (an API change, not extraction). The chat.rs one could locally adopt
  `rem_euclid` as a drive-by, not a numbered cut.
- **vocab_lookup run vs run_dict** (vocab_lookup.rs:73-80 vs :82-90) — the
  status-check difference is load-bearing (dict exits 20/21 on miss and MUST
  be ignored); 2 sites, keep split. parse_wn vs parse_gcide: same skeleton,
  different markers/offsets — merely similar.
- **corpus_search_popup.rs** — verified CLEAN: routes through all shared
  picker helpers (new_picker_list, build_picker_scrim, clear_list,
  two_label_row, select_first_row, move_selection_clamped,
  attach_overlay_panel); no re-inlining. The picker plumbing investment is
  paying off — no entry.
- **Citation display assembly** (`{d1}.{d2}.{line}` at journal.rs:383/385,
  vocab_journal.rs:66, corpus_search.rs:42) — separators and arity differ
  per display context (em-dash range vs indented hit vs 2-level header);
  `db::models::citation` is the 4-part JOIN format, a different contract.
  Per-site formats are intentional.

**Below the floor (Batch 9 additions — flag; number only if the family grows):**

- `run_claude_request` success/error closure preamble (`s.chat.pending =
  false;` first-line ×6 in chat.rs) — one statement; the closure bodies
  diverge immediately.
- Idle child-walk scaffold (`first_child()`/`next_sibling()` + index) —
  chat_panel.rs render_page :271, flash_rows :341 (+ rebuild_from_specs'
  index-free variant :400). 2.5 sites; extraction needs a closure param.
- `transcript_rows + landable_mask` two-line preamble (~5 chat.rs sites) —
  both fns are already the shared single-source; the pair is too thin.
- `"(none found)"` sentinel — SHIPPED as #78's rider (`CORPUS_NONE_FOUND`).
- `JournalDisplay` set + `show_vocab_popup` triad (vocab_journal.rs ×4) —
  variant fields differ per arm.
- Source-markup tag literals (`"<speaker>"`/`"<verse>"`/`"<stage>"` ± closing
  forms re-spelled in corpus_search.rs:74, journal.rs:392-404, :988-1007) —
  three different strippers with different algorithms (whole-element vs
  prefix-trim vs single-element); naming the tags is possible but each
  stripper is different-by-design; revisit only if a fourth stripper appears.
- `genre_unit(work_type) → titlecase_first(unit)` two-line pairing
  (journal.rs:2439, chat.rs:715, vocab_journal.rs:200 post-#78) — wait for a
  fourth site.
- Test-only `Exchange { … }` 10-field constructors (×7 chat.rs test modules)
  — a shared `#[cfg(test)] fn test_exchange()` is a test-hygiene drive-by,
  not a numbered prod cut; note for whoever next touches those tests.
