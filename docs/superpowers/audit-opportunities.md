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

- **`InputMode → picker` dispatch accessor.** The handler audit found ~18
  `move_selection` dispatch arms, 7 Escape `hide(); input_mode = Reader` arms, and
  ~5 picker-open `show(); set input_mode` pairs that all hand-write
  `state.<specific_picker>.<op>()`. A `picker_for_mode(mode) -> &dyn Picker`
  accessor (or a `dyn Picker` trait) would collapse all three — but it touches
  control flow and would have to unify `move_selection`'s empty-start variants,
  which #6 DELIBERATELY preserved. Behavior-risky, multi-PR → not a numbered
  safe-scope opportunity. Revisit only with a dedicated spec.

These are real maintainability issues but are behavior-CHANGING and multi-PR.
They are NOT numbered opportunities; do not run them through the safe-scope
pipeline as a single refactor.

- **AppState god-struct** (`src/app.rs`) — ~217 fields, de-facto global. Grouping
  into domain sub-structs touches nearly every `&mut AppState` signature.
- **app.rs module carve-up** — 6765 lines, 13 concerns; `build_window` ~1419
  lines. Extracting window_builder / layout / work_loader is behavior-risky.
- **gloss_overlay.rs** — 3606 lines; the ~1100 lines of pure buffer helpers
  (no GlossOverlay coupling) MAY qualify as a safe-scope move — evaluate as a
  numbered opportunity if the block is genuinely self-contained.
