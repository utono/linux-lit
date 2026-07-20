# linux-lit audit opportunities — archive (full analyses)

Full Signal / Identical-part / Variants / EXCLUDED write-ups for **shipped**
opportunities, moved here when they were pruned to a one-liner in
`audit-opportunities.md`.

**The `assess-maintainability` skill does NOT read this file.** It exists only so
the reasoning behind a merged cut survives outside git history — consult it by
hand when re-examining a past decision, never as part of an audit run. The live
ledger (`audit-opportunities.md`) carries the one-line summary, the lessons, and
the standing exclusions that a fresh audit actually needs.

Newest batch first. Each entry is verbatim as it stood in the ledger the day it
shipped; the authoritative record is still the refactor commit it names.


## Batch 9 (shipped 2026-07-19) — archived full analyses

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
  at **2 sites**: `src/app/vocab_popup.rs:~100-124` (`position_vocab_popup`)
  and `src/input/actions/chat.rs:~2660-2681` (`size_panel` float arm; refs
  re-verified side-by-side 2026-07-19 after the chat-panel pagination
  rework — the twin SURVIVED it, which is itself a drift signal: the vocab
  comment still says "same as the chat float").
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

## #78 — titlecase_first route-to-shared (NARROWED 2026-07-19)

- **Status:** OPEN, narrowed — the chat.rs site was routed in f17ba45f
  (`titlecase_first` is now `pub(crate)`, journal.rs:23); ONE re-inline
  remains.
- **Signal:** `vocab_journal.rs:201-207` still re-inlines the
  character-identical body as a block expression (the
  `chars() → to_uppercase().collect() + as_str()` form).
- **Identical part (route):** the site becomes
  `journal::titlecase_first(...)` — the helper is already public and tested;
  zero new code.
- **EXCLUDED:** none found — no other first-letter-uppercase sites remain.
- **Safe-scope:** yes — route to an existing tested fn; smallest cut in the
  ledger. Fold into any other vocab_journal-touching PR as a rider.

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

## #88 — wl-copy arg-form clipboard helper

- **Status:** OPEN (rank #1 — 10 byte-identical one-liners across 8 files;
  the below-floor "arg-form (×4)" family from Batch 4 has since grown to 10,
  which is the stated re-open condition).
- **Signal:** `let _ = std::process::Command::new("wl-copy").arg(&X).spawn();`
  is byte-identical (modulo the local's name: `&text`/`&copied`/`&s`) at
  **10 sites**: keymap.rs:2878, :2943; settings.rs:566, :592 (multi-line
  rustfmt of the same call); synopsis.rs:279; gloss.rs:336; echoes.rs:855;
  chat.rs:1276, :1667, :2309; journal.rs:2809.
- **Identical part (extract):** `pub(crate) fn copy_to_clipboard(text: &str)`
  (home: `src/ui/mod.rs` or a tiny `src/clipboard.rs`) owning exactly the
  arg-form spawn + `let _ =` discard. All 10 sites become one call.
- **EXCLUDED (named, why):** the stdin-pipe form (visual.rs:355,
  word_copy.rs:45, :105 — pipes via stdin AND logs failures; merging arg→stdin
  changes behavior, per the standing below-floor note) and keymap.rs:4125,
  :4171 (`if let Ok(mut child)` stdin form with wait) — both stay. Each site's
  surrounding toast (`"Copied {}"` at 5 sites with VARYING secs 2/3) stays at
  call sites — durations differ per surface, folding them changes behavior.
- **Safe-scope:** yes — byte-identical one-liner → helper; the classic #15/#22
  shape at codebase scale.

## #89 — chat_pagination base-padding-top table (drift ALREADY manifest)

- **Status:** OPEN (rank #2 — 2 sites in one file, but the two-site floor is
  cleared by a documented drift signal: the doc comments have ALREADY drifted
  from the code, and the fn's own SYNC note says three things must stay in
  lockstep).
- **Signal:** the per-class base `padding-top` table (speaker 14, verse 0,
  verse-flush 0, stage 8, stage-flush 8) appears twice in
  `src/ui/chat_pagination.rs` in different match shapes: `class_pad`'s arms
  (~:44-66) and `src_lead_extra_pad`'s `base_pt` match (~:106-111). The values
  agree today, but the PROSE around them has already desynced: `class_pad`'s
  comments claim gloss "= 21" and src-lead "= 47" while the code returns 29
  for both; `src_lead_extra_pad`'s doc block says the src-lead padding-top is
  44 (five times, with a worked +30/+44/+36 table) while the code constant is
  `SRC_LEAD_PADDING_TOP = 26` and its own tests assert the 26-based deltas.
  The 26 is correct (the "equalize gaps at 26px" commits, 7f2449d9); the
  comments are stale leftovers of the 44 era. Mis-reading them is exactly how
  the next pagination edit under- or over-counts a row (bottom clip).
- **Identical part (extract):** one `fn base_padding_top(class: &str) -> i32`
  (the 14/0/0/8/8 table) called by both fns, + rewrite the stale comment
  arithmetic (21→29, 47→29, 44→26 and the worked delta table) to match code.
  Comment corrections are behavior-preserving by definition; the extraction is
  a value-identical table dedup.
- **EXCLUDED (named, why):** `class_pad`'s `+3` blanket-bottom fold and its
  non-source arms (chat-q/chat-a/chip/error/saved) — class_pad's own concern;
  `SRC_LEAD_PADDING_TOP` itself (single-sited, correctly a const); the
  theme.rs CSS values (the OTHER side of the SYNC contract — the helper only
  dedups the Rust side).
- **Safe-scope:** yes — value-identical table → shared fn + stale-comment fix;
  the existing chat_pagination unit tests pin the outputs.

## #90 — chat/journal toast literal consts (#85-style)

- **Status:** OPEN (rank #3 — 15 sites / 7 strings, the exact shape #85
  shipped for navigation.rs).
- **Signal:** byte-identical toast strings at ≥2 sites each: chat.rs —
  `"No room for chat panel at this layout"` (:417, :559, + quoted in a doc
  comment :440), `"No passage at the cursor"` (:694, :700), `"Waiting for the
  previous reply…"` (:678, :2464), `"Entry is saved"` (:2376, :2385),
  `"Save failed"` (:2422, :2574); cross-file `"Rewritten"` (chat.rs:3113,
  journal.rs:2232 — the shared rewrite pipeline's completion toast);
  journal.rs `"Nothing to rewrite"` (:1781, :1843, :1934).
- **Identical part (extract):** `pub(crate) const` strings — the chat-local
  five in chat.rs, `TOAST_REWRITTEN` + `TOAST_NOTHING_TO_REWRITE` beside the
  existing #85 consts in navigation.rs (both files already import them from
  there). Update the :440 doc comment to name the const.
- **EXCLUDED (named, why):** durations stay inline (the #70 precedent);
  single-site in-progress toasts (`"Rewriting Q & A…"`, `"Consolidating…"`)
  — no family yet; `"Copied {}"` (×5) — format string with per-site secs,
  recorded under #88's exclusions.
- **Safe-scope:** yes — literal → named const, #8/#70/#85-style.

## #91 — chat placement CSS-class consts

- **Status:** OPEN (rank #4 — 10 sites of 3 class-name strings that must stay
  in lockstep with each other AND with theme.rs selectors; the add/remove
  pairs are split across three fns that the pagination rework just re-edited).
- **Signal:** `"chat-panel-float"` at chat.rs:256, :410, :2651, :2683;
  `"chat-panel-pinned"` at :257, :2652, :2682; `"card-chat-seam"` at :258,
  :2655, :2686 — spread over `close_chat_layout`, `regate_panel`, and both
  `size_panel` arms. A typo in one add/remove site silently breaks placement
  styling (no compile error).
- **Identical part (extract):** three `const CHAT_CLASS_FLOAT/PINNED/
  CARD_SEAM: &str` in chat.rs (or chat_panel.rs, which owns the container),
  referenced at all 10 sites.
- **EXCLUDED (named, why):** the theme.rs selector strings (CSS text inside
  `generate_css` — a different language surface; the const names the Rust
  side only, same boundary as #89); the flash-class strings
  (`chat-flash-*`, →#92); repo-wide picker CSS classes (`"picker-item-detail"`
  ×11 etc.) — GTK idiom across stable files with no lockstep add/remove
  clusters, not numbered.
- **Safe-scope:** yes — literal → named const.

## #92 — flash-css-class helper (chat_panel.rs trio)

- **Status:** OPEN (rank #5 — 3 sites, one file, byte-identical structure).
- **Signal:** clone widget → `add_css_class(K)` →
  `glib::timeout_add_local_once(D, move || w.remove_css_class(K))` at
  **3 sites** in `src/ui/chat_panel.rs`: `flash_input` (:139-143, K =
  `"chat-flash-active"`, D = 240ms), `flash_transcript` (:151-155,
  `"chat-flash-wash"`, 160ms), `flash_rows`'s inner per-row block (:347-351,
  `"chat-flash-row"`, 160ms).
- **Identical part (extract):** a private
  `fn flash_class(w: &gtk4::Widget, class: &'static str, ms: u64)` (or a
  free fn beside the panel impl); each site passes its widget/class/duration.
- **EXCLUDED (named, why):** the `crate::ui::flash_widget` preamble call
  (present at 2 of 3 sites — stays at call sites); `flash_rows`' idle-walk
  scaffold around the block (its own below-floor family, see Batch 9 notes);
  `ui::toast::show_transient` is NOT the home — it is Label-text-specific,
  this is the widget-CSS sibling idiom.
- **Safe-scope:** yes — 3-line byte-identical-structure block → helper with
  three params, no control-flow change.

## #93 — chat_rows pure-motion move (chat.rs is 3763 lines and climbing)

- **Status:** OPEN (rank #6 — the #84 shape: a documented pure island moved
  out of a fast-growing host file; larger scope than #88-#92 but purely
  mechanical).
- **Signal:** chat.rs has grown to **3763 lines** (from ~2700 pre-pagination).
  Its row-model core is explicitly written as an AppState-free island — the
  doc comments say "pure core … no AppState … unit-testable without
  constructing an AppState": `ChatMsgCtx`, `answer_row`, `question_row`,
  `widget_row_count`, `has_question_row`, `is_first_question_exchange`,
  `build_transcript_rows`, `build_single_exchange_rows` (~:919-1095, ~160
  lines) plus kindred pure fns (`wrap_index` :1236, `flip_view` :1312,
  `split_answer_paragraphs` :1323, `journal_view_rows` :1346,
  `clamp_journal_cursor` :1401, `landable_mask` :2045,
  `first_landable_at_or_after` :2069, `visual_selection_range` :2221,
  `consolidate_transcript` :2442, `build_history_turns` family :2750-2804,
  `parse_revised_qa` :2809) and their ~940 lines of pure test modules
  (:2823-3763).
- **Identical part (move):** relocate the pure fns + their test modules to a
  sibling `src/input/actions/chat_rows.rs` (mod declared from chat.rs;
  `pub(crate)`/`pub(super)` as needed). Pure motion — no signature or body
  changes; `cargo test` runs the moved test modules identically (the #84
  verification).
- **EXCLUDED (named, why):** the thin `&AppState` wrappers (`transcript_rows`
  :1001, `transcript_font`, the render_* fns) — they stay with the handlers;
  the placement math (`line_in_right_column`/`placement_for_range` :301-354)
  — pure but conceptually panel-geometry, not row-model; keep with placement
  code to avoid a grab-bag module. The sync-invariant doc comments (rows must
  match chat_panel/chat_pagination's view) MUST travel with the moved fns.
- **Safe-scope:** yes — pure motion, the #84 precedent; spec should pin "no
  diff beyond `mod`/`use` lines and the moved text."

## #94 — db migrations module (queries.rs ensure_* cluster move)

- **Status:** OPEN (rank #7 — same #84 pure-motion shape for queries.rs, 4037
  lines; lowest rank because the cluster is stable code with low drift risk).
- **Signal:** nine `ensure_*` schema-migration fns, each `(conn: &Connection)
  -> Result<(), rusqlite::Error>` with zero AppState references:
  queries.rs:890 (`ensure_claude_model_columns`), :1137, :1191, :1212, :1239,
  :1257, :1393, :1649, :1716 — ~200 lines of one-shot DDL probes interleaved
  with hot-path query code.
- **Identical part (move):** relocate the nine fns to `src/db/migrations.rs`;
  callers (startup + the lazy per-feature ensure calls) update paths. The
  Standing exclusion against DEDUPING their bodies (varying SQL is
  load-bearing) is untouched — this is motion, not folding.
- **EXCLUDED (named, why):** `column_exists` (:871) stays in queries.rs — it
  is the shared probe #37 shipped and non-migration code uses it; the
  canonical-abbrev family (`canonical_work_abbrev` :922,
  `ensure_canonical_artifact_abbrevs` :989, `migrate_variant_passages`,
  `rekey_journal_citations`) stays — despite the `ensure_` name of one, they
  are runtime lookup + data-repair logic coupled to it, not schema DDL; the
  voice-resolution cluster (:1464-1611) — a possible future move but a
  different topic, not this cut.
- **Safe-scope:** yes — pure motion of self-contained `&Connection` fns.

## Closed without shipping (2026-07-19 audit)

## #81 — float-frame CSS fragment (vocab float + chat float) — CLOSED

**Closure reason (2026-07-19):** invalidated by design divergence, not shipped.
The chat-panel float was reworked into a full-height edge panel
(`.chat-panel-float` now sets `border-left`/`border-right` only and
`border-radius: 0`, theme.rs ~:1371) while the vocab float kept the boxed frame
(`border: 1px solid …; border-radius: 8px`, ~:1307). The fragments are no
longer identical and the divergence is intentional per the panel redesign
comments in generate_css. Recorded as different-by-design; re-open only if the
two frames are deliberately re-unified on screen. Original analysis as it stood:

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

## Batch 8 (shipped 2026-07-17) — archived full analyses

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


## Batch 7 (shipped 2026-07-17) — archived full analyses

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

