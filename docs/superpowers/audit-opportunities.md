# linux-lit audit opportunities

Numbered, safe-scope, behavior-preserving refactoring opportunities. Produced by
the `assess-maintainability` skill; consumed by the spec→plan→refactor→merge
pipeline. DONE entries stay for numbering continuity — never reuse a number.

Larger, behavior-CHANGING projects (god-struct split, app.rs module carve-up)
are tracked separately at the bottom under "## Larger projects (not safe-scope)"
— they are NOT numbered opportunities.

<!-- #1–#4 were resolved before this ledger was created (shared AskCard,
     ask-card key intercept, AppState.picker rename, gloss_block_voice extract).
     Numbering continues from #5; those four numbers are retired. -->

## #5 — footer-row-builder — DONE (commit c976e2c)

- **Status:** DONE
- **Signal:** gloss_overlay.rs + journal_overlay.rs repeat an identical
  gloss-hint footer row (margins, left-hexpand + right-hint layout).
- **Identical part (extracted):** `build_footer_row` → `FooterRow { container,
  left, hint }` in `src/ui/footer.rs`.
- **EXCLUDED:** concordance_bar, vocab_popup, library_picker footers
  (structurally different); pickers with no footer.
- **Safe-scope:** yes — pure widget-construction extraction.

## #6 — picker-nav-helper — DONE (commit 371abd8)

- **Status:** DONE
- **Signal:** 13 ListBox-index pickers repeat the identical select-if-exists
  tail of `move_selection`.
- **Identical part (extracted):** `select_row_at(&ListBox, i32)` in
  `src/ui/picker_nav.rs`.
- **Variants (stayed at call sites):** A guard+clamp (5); B unwrap_or(-1)+clamp
  (2); C unwrap_or(-1) no-clamp (5); D unwrap_or(0)+clamp (1).
- **EXCLUDED:** action_popup / keybinds / settings (rem_euclid over a Vec);
  library_picker (scroll-into-view). Deliberately NOT a full Picker trait.
- **Safe-scope:** yes — byte-identical tail extraction.

## #7 — claude-bridge-helper — DONE (commit d53580b)

- **Status:** DONE
- **Signal:** gloss add/edit + synopsis + journal repeat the
  spawn_future_local + tokio spawn + Ok(Err)/Err recovery bridge.
- **Identical part (extracted):** `run_claude_request` in
  `src/input/actions/claude_bridge.rs`.
- **Safe-scope:** yes — async-bridge helper, recovery arms preserved.

## #8 — sentinel-key-constants — DONE (commit 63c9779)

- **Status:** DONE
- **Signal:** whole-work / journal-work scene-key sentinel literals reused
  unnamed across sites.
- **Identical part (extracted):** named constants for the sentinels.
- **Safe-scope:** yes — literal → named constant, greppable.

## #9 — transient-toast-helper — DONE (commit pending)

- **Status:** DONE
- **Signal:** 30 `timeout_add_local_once` auto-hide closures + ~9 ad-hoc named
  toast wrappers across app.rs / keymap.rs / search.rs / actions/* repeat the
  identical show + clone + 3s/2s hide tail.
- **Identical part (extracted):** `show_transient(&Label, &str, u64)` in
  `src/ui/toast.rs`; named wrappers keep message construction and delegate the tail.
- **Variants (stayed at call sites):** which label (chapter/speed/search) and
  duration (2s confirmations vs 3s).
- **EXCLUDED:** `show_chapter_toast` (navigation.rs, generation-guarded);
  `show_persistent_tts_toast`/`hide_tts_toast` (gloss.rs, no auto-hide); the
  5s startup-reveal / 6s nav-fuzz timers; the 500ms chord-reset; vocab-fade and
  word-bold gen-guarded closures.
- **Safe-scope:** yes — pure widget show+schedule extraction, ~107 net code lines
  removed; build + 413 bin tests green, headless launch verified.
- **Follow-on candidate:** the `debug_icon` flash (keymap.rs:2240, app.rs:1595,
  nav_test) is the same primitive on a different Label — out of #9's toast scope;
  consider as a future #N.

## #10 — subsequence-match-helper — DONE (commit pending)

- **Status:** DONE
- **Signal:** byte-identical char-level `subsequence_match` (named
  `subsequence_chars` in library_picker) copied into 5 pickers.
- **Identical part (extracted):** `subsequence_match(&str, &str) -> bool` in new
  `src/ui/picker_filter.rs` (pure, no GTK). All call sites already lowercase both
  sides, so the shared fn stays case-sensitive.
- **Variants (stayed at call sites):** how each picker builds its `target`
  (display string vs `format!` of speaker+text vs work title+author+abbrev).
- **EXCLUDED:** `subsequence_match_work` (work-typed) and `author_name_matches`
  (pub) — kept as wrappers that delegate their tail; the `#[cfg(test)]`
  `subsequence_match` alias in library_picker.
- **Safe-scope:** yes — pure fn extraction; build + 413 bin tests green
  (incl. 5 library_picker subsequence tests via the delegated path).

## #11 — shorten-author-helper — DONE (commit pending, narrowed)

- **Status:** DONE (narrowed from the original "shorten_author + shorten_title"
  entry after verification).
- **Signal:** `shorten_author` byte-identical in `concordance.rs` and
  `ui/concordance_list_picker.rs`.
- **Identical part (extracted):** promoted `concordance::shorten_author` to
  `pub(crate)`; deleted the UI copy, repointed its call site.
- **EXCLUDED — and why the original #11 was wrong:** the two `shorten_title`
  functions are NOT identical — `concordance.rs` truncates titles >25 chars at a
  word boundary; the picker does only the prefix strip (it truncates downstream
  via `truncate_around_center`). Merging them would change one site's output, so
  `shorten_title` stays split. `truncate_around_center` is single-copy.
- **Safe-scope:** yes — pure fn move; build + 413 bin tests green.
- **Lesson:** the "confirm bodies match before merging" caution earned its keep —
  a same-named pair was behaviorally different. The audit skill's "verify the
  byte-identical part" step is what caught it.

## #12 — sync-suppress-window-const — DONE (commit pending)

- **Signal:** `Some(Instant::now() + Duration::from_millis(500))` repeated at 8
  sites (search.rs ×2, timestamps.rs, keymap.rs, gamepad.rs, echoes.rs ×2,
  concordance.rs) as the "brief suppression while MPV seeks" window.
- **Identical part (extracts):** a named `const SYNC_SUPPRESS_SEEK: Duration` (500ms).
  Sites become `Some(Instant::now() + SYNC_SUPPRESS_SEEK)`.
- **Variant — name separately, do NOT merge:** navigation.rs:1736 uses
  `from_secs(86400)` — a distinct "suppress indefinitely" sentinel; give it its
  own `SYNC_SUPPRESS_INDEFINITE` const.
- **EXCLUDED:** the two unrelated `from_millis(500)` GTK `timeout_add_local_once`
  (app.rs:1972, keymap.rs:28) — same number, different meaning (a UI timer, not a
  sync window). The navigation.rs:1725 max-guard logic stays inline (folding it
  into a helper would add a guard at the plain sites = behavior change).
- **Safe-scope:** yes — literal → named const, #8-style. Highest copy count.

## #13 — picker-attach-helper — DONE (commit pending)

- **Status:** DONE
- **Signal:** picker `attach(&self, base)` body
  (`overlay.set_child(Some(base)); overlay.add_overlay(&picker_box); picker_box.set_visible(false)`)
  byte-identical in 10 pickers (scanner said 11; voice/echo_line/echo_turns/
  concordance_works have no attach).
- **Identical part (extracted):** `attach_panel(&Overlay, base, Option<&Box> scrim,
  &Box panel)` in new `src/ui/picker_attach.rs`.
- **Variants (folded via params):** scrim pickers pass `Some(&scrim)` (echo_picker,
  library_picker); authorship_picker passes `&container` as the panel;
  library_picker calls the helper then keeps its responsive-resize block.
- **EXCLUDED:** all `*_overlay.rs` attach/attach_to (different signatures/bodies);
  library_picker's resize block (stays inline after the helper call).
- **Safe-scope:** yes — pure widget-construction extraction; build + 413 bin tests
  green; headless cage launch renders (overlay wiring confirmed).

## #14 — citation-format-helper — DONE (commit pending)

- **Status:** DONE
- **Signal:** `format!("{}.{}.{}.{}", abbrev, div1, div2, line_in_div)` at 6 sites
  (gloss.rs ×4, queries.rs ×2).
- **Identical part (extracted):** `pub fn citation(abbrev, div1, div2, line_in_div)
  -> String` in `src/db/models.rs` (the module owning `Line.citation`).
- **Variants:** field source (`first./last.` structs vs row-derived locals) — stays
  at call site (just the 4 args differ).
- **EXCLUDED:** `parse_citation` / `format_citation_range` (gloss_overlay.rs) — the
  inverse + a range formatter, different concern.
- **Safe-scope:** yes — literal template → helper fn; build + 413 bin tests green.

---

## Larger projects (not safe-scope)

- **`InputMode → picker` dispatch accessor — DONE (nav + plain-hide scope).**
  Shipped via the `Picker` trait + `picker_for_mode(&AppState, mode) -> Option<&dyn
  Picker>` accessor in `src/input/picker_dispatch.rs`. Collapsed the nav
  `MoveDown`/`MoveUp` arms (Ctrl+n/p, ~20 arms) and the 7 plain Escape
  `hide(); → Reader` arms in `handle_picker_key`. The parked concern (unifying #6's
  preserved `move_selection` variants) did NOT apply: those variants live inside
  each picker's `move_selection` body, not the dispatch arms, so routing through
  the trait left them untouched. Gloss/Journal/EchoLine Escape arms kept explicit;
  settings/voice/library handlers untouched. Spec/plan under docs/superpowers/;
  user-verified (Ctrl+n/p moves selection, Escape closes). See merge commit.
  **Open-pairs follow-on — DONE.** The `show()`/open mode-set pairs were unified
  via `open_picker_mode(&mut AppState, mode)` in `src/input/actions/pickers.rs`
  (8 sites, 7 pickers), which also normalized 4 redundant double-RefCell-borrows
  to one `borrow_mut`. `show()` itself varies per picker (no-arg / args /
  prepare-finish) so only the mode-set is shared; library_picker excluded. Spec/
  plan under docs/superpowers/; headless boot smoke + 413 tests green.
  **Only the Confirm dispatch remains deferred** — its arms are genuinely bespoke
  (different `selected_X()` return types + post-selection handlers); abstracting it
  would add complexity, not remove it. Left as honest duplication by design.

These are real maintainability issues but are behavior-CHANGING and multi-PR.
They are NOT numbered opportunities; do not run them through the safe-scope
pipeline as a single refactor.

- **AppState god-struct** (`src/app.rs`) — ~217 fields, de-facto global. Grouping
  into domain sub-structs touches nearly every `&mut AppState` signature.
- **app.rs module carve-up — Phase 1 (leaf modules) DONE (merge 1bd1df3).**
  `src/app.rs` was converted to a directory module (`src/app/mod.rs`) and three
  self-contained leaf families were extracted into sibling modules via pure
  behavior-preserving code motion: `vocab_popup.rs` (239, the vocab-popup widget
  fns), `font.rs` (194, font-size / line-number-gutter rebuild), `text_prep.rs`
  (205, GTK-free text preparation). `mod.rs` dropped from 6735 → 6105 lines. No
  facade — every external call site repathed directly (`crate::app::X` →
  `crate::app::<mod>::X`). Four visibility bumps, all real + minimal: two planned
  (`font::reapply_font`, `text_prep::SnapshotOrPrep` → `pub(crate)`) and two
  compiler-forced (`vocab_popup::update_vocab_popup_margin` → `pub(super)`,
  `font::rebuild_line_number_gutter` → `pub(crate)`) because non-group `mod.rs`
  fns call them. The three modules are genuine independent leaves (no
  cross-edges; only inbound reverse-deps from `mod.rs`). 413 tests + clippy 115
  unchanged throughout. Spec/plan under docs/superpowers/ (2026-06-22).
- **app.rs module carve-up — Phase 2 (tier-a families) DONE (merge 42e126c).**
  The three remaining tier-a topical families were extracted into sibling
  modules via pure code motion, in dependency order: `formatting.rs` (610, the
  per-line reader-buffer typographers — dialogue/BCP/scansion/stanza/authorship),
  `scene_synopsis.rs` (508, scene-boundary derivation + synopsis keys/labels/
  overlay + scene title bar), `translations.rs` (635, the inline-gloss interleave
  path + two-column translation overlay). `mod.rs` dropped 6105 → 4360 lines. No
  facade. Visibility bumps, all real + minimal: `apply_dialogue_formatting`,
  `apply_authorship_formatting`, `apply_scansion_marks`, `apply_bcp_formatting`,
  `scene_heading_start` → `pub(crate)`, and `vocab_popup::update_vocab_popup_margin`
  `pub(super)` → `pub(crate)` (a sibling can't see a `pub(super)` item). The only
  new inter-module edge is `translations → scene_synopsis` (overlay cluster needs
  `current_scene_divs`/`synopsis_label`), which is why scene_synopsis extracted
  first; the graph stays an acyclic DAG. 413 tests + clippy 115 unchanged. Spec/
  plan under docs/superpowers/ (2026-06-23). **The entire tier-a (safe-scope)
  carve-up is now complete** — across Phases 1+2, `mod.rs` went 6735 → 4360 with
  six focused sibling modules.
- **app.rs module carve-up — Phase 3 (layout module, tier-b start) DONE (merge
  8eab5aa).** The first **tier-b** slice. The structural inventory established
  that the audit's three tier-b targets are NOT equally tractable:
  `build_window`'s body is dominated by the ~218-field `AppState` struct literal
  + closures that capture `state` built mid-function, so it **cannot** be split
  by pure code motion without first grouping the god-struct (a separate
  behavior-changing project — build_window is *blocked on* the god-struct, not
  merely adjacent to it). But the **layout free functions are callable**
  (`&mut AppState`-in / widgets-out), so they move like tier-a. Phase 3 extracted
  the layout cluster into `layout.rs` (406): `apply_tiled_mode`,
  `apply_card_sizing`, `apply_column_layout`, `target_card_width`,
  `is_tiled_layout`, `current_block_text_width`, `verse_left_offset`,
  `overlay_card_size`, `line_number_gutter_geometry` + `SONNET_BLOCK_SAMPLE` +
  the `card_width`/`column_default` test modules. `mod.rs` 4360 → 3959. Bumps:
  `apply_tiled_mode`/`apply_card_sizing`/`apply_column_layout` → `pub(crate)`,
  and `setup_gutter` (stays in mod.rs) `fn` → `pub(super)` (a child-module
  reverse-call). Two-column/spacer consts stayed in mod.rs (shared with
  build_window/display_work). No facade; sibling + external call sites repathed.
  **Verification was tier-b, not tier-a:** `cargo test --bins` (413) covers only
  the pure sizing math (the moved unit tests); the widget-bound fns
  (`apply_tiled_mode`/`apply_card_sizing`/`apply_column_layout`/
  `current_block_text_width`) render to screen, so the real proof was a
  **user-run nav-fuzz on `H8-Amb` (two-column play) + `Son` (sonnet sequence),
  both clean**. Spec/plan under docs/superpowers/ (2026-06-23). **Still parked:**
  `build_window`'s body + `display_work_at_with_prepared` (the remaining tier-b,
  higher e2e burden), and the **AppState god-struct** grouping (~217 fields) —
  which is the prerequisite for any real `build_window` split. Pre-existing flake
  (not a carve-up defect): `db::queries::tests::test_bookmark_toggle` flakes
  ~1-in-5 full-suite runs on shared read-write lit.db parallelism (passes in
  isolation); candidate future cleanup to isolate its DB.
- **gloss_overlay.rs — DONE (merge 81acba8).** The ~1100 lines of pure helpers
  (block model, OP-IPA markup, geometry/citation) + their ~750 lines of tests
  were extracted into three sibling modules: `gloss_block.rs` (707),
  `gloss_ipa.rs` (480, leaf), `gloss_util.rs` (404, leaf). gloss_overlay.rs is
  now 2043 lines (the GlossOverlay widget + GTK buffer-population code that
  intentionally stayed). Clean acyclic graph (a cross-task fix moved
  `replace_word_ipa_in_source_block` into gloss_block to keep gloss_ipa a leaf);
  call sites repathed, no facade. 413 tests unchanged. Spec/plan under
  docs/superpowers/. Confirms the "MAY qualify as safe-scope" hypothesis was
  correct for the pure tail; the GTK buffer-population code was correctly left
  as behavior-risky.
