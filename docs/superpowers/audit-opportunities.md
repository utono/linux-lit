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

## Batch 2 (audited 2026-06-24, after the carve-up + grouping + flake-fix work)

Fresh scan over the post-refactor tree. Ranked by (duplication × drift_risk) ÷
scope_size. Each verified by direct grep, not agent word alone.

## #15 — listbox-clear-helper — DONE (merge 017fe18)

- **Status:** OPEN (highest-value remaining; pure win)
- **Signal:** `while let Some(row) = self.list_box.first_child() { self.list_box.remove(&row); }`
  — the "remove all children" loop — at ~15 sites across 11 picker/overlay files
  (`*_picker.rs`, `vocab_popup.rs`, `translation_overlay.rs`).
- **Identical part (extract):** `pub fn clear_list(list_box: &gtk4::ListBox)` in
  `src/ui/picker_nav.rs` (the module that already owns `select_row_at`).
- **Variants:** binds `row` vs `child` — cosmetic, disappears inside the helper.
  Receiver is always a `ListBox` reference.
- **EXCLUDED:** any `first_child`/`remove` loop over a non-`ListBox` container with
  extra per-child logic (none found that mixed in work — but exclude on sight).
- **Safe-scope:** yes — a 3-line GTK loop → one call; no inputs but the list_box.

## #16 — block-visual-key-twin — DONE (merge fe8a563, render-verified 2026-06-24)

- **Status:** DONE — merged AND render-verified. The owed runtime check was run
  by the user in the synopsis overlay (`h` → Shift+V → j/k/gg/G/y/Escape): all
  behaviors correct, the unification is confirmed behavior-preserving. (The check
  surfaced a pre-existing UX wish — Escape leaves the cursor at the moving end
  rather than the entry block — but that is the ORIGINAL pre-refactor behavior,
  not a #16 regression; it became the separate `escape-restores-anchor`
  enhancement, see below.)
- **Shipped:** `handle_synopsis_visual_key` + `handle_gloss_visual_key` →
  one `handle_block_visual_key(state, key_state, key_name, cfg)` +
  `SYNOPSIS_VISUAL_CFG`/`GLOSS_VISUAL_CFG` consts of plain `fn` pointers (no
  trait). The yank/escape exit asymmetry is captured via separate
  `yank_exit`/`escape_exit` config slots. Log output byte-equivalent; net -20
  lines; 413 + clippy 115. Spec under docs/superpowers/ (2026-06-24).

- **Status (orig):** OPEN (most concentrated single duplication found)
- **Signal:** `handle_synopsis_visual_key` (keymap.rs:1328) and
  `handle_gloss_visual_key` (keymap.rs:1392) are near-identical whole functions —
  the second's own comment says "Mirrors `handle_synopsis_visual_key`". The
  `gg`-chord preamble + `j/k/G/g` visual-step match arms are byte-identical.
- **Identical part (extract):** one `fn handle_block_visual_key(state, key_state,
  key_name, cfg: BlockVisualCfg)` taking a 4-field config of plain `fn` pointers /
  enum values (text-getter, log tag, exit fn, return mode + hint fn).
- **Variants:** ONLY the `y` (yank) and `Escape|V` (exit) arms differ, by 4
  substitutions: `visual_selection_text` vs `visual_selection_buffer_text`;
  `"SYNOPSIS:"` vs `"GLOSS:"` log; `exit_visual` vs `exit_visual_to_start`;
  `SynopsisOverlay`/`set_synopsis_hint` vs `GlossOverlay`/`set_gloss_hint`.
- **EXCLUDED:** the central `handle_picker_key` dispatcher (picker-dispatch
  territory); any other `handle_*_visual_key` would need its own variant check.
- **Safe-scope:** yes — plain `fn`-pointer config struct, NO trait/generic. The
  one place this audit found a near-whole-function clone.

## #17 — load-work-titles-helper — DONE (merge 017fe18)

- **Status:** OPEN (cleanest db-side cut: 6× byte-identical)
- **Signal:** the 4-line chain `let titles = crate::db::queries::open_db().ok()
  .and_then(|conn| crate::db::queries::load_work_titles(&conn).ok())
  .unwrap_or_default();` — byte-identical at 6 sites (`visual.rs` ×2, `echoes.rs` ×4).
- **Identical part (extract):** `pub fn load_work_titles_or_default() -> <its type>`
  in `src/db/queries.rs` (beside `load_work_titles`).
- **Variants:** none — all 6 are byte-identical (variant A only).
- **EXCLUDED:** other `open_db().ok().and_then(...)` chains that call a *different*
  loader (`load_echo_links`, embeddings) — not this family.
- **Safe-scope:** yes — pure fn wrapping an existing query; zero behavior change.

## #18 — open-db-rw-or-log-helper — DONE (merge e5bf8ec)

- **Status:** OPEN (file-local, 5 byte-identical)
- **Signal:** `let conn = match crate::db::queries::open_db_rw() { Ok(c) => c,
  Err(e) => { crate::logging::log(&format!("TS: open_db_rw failed: {}", e));
  return false; } };` — 5 byte-identical occurrences in `src/input/timestamps.rs`.
- **Identical part (extract):** a file-local helper `fn open_db_rw_or_log() ->
  Option<Connection>` (call sites become `let Some(conn) = open_db_rw_or_log()
  else { return false; };`) — OR keep the early-return shape via a small macro.
- **Variants:** B — `timestamps.rs:533` logs `"TS: undo open_db_rw failed"`
  (different prefix → either parameterize the tag or leave B out). C —
  `timestamps.rs:52` is silent + returns `()` not `false` → EXCLUDE.
- **EXCLUDED:** the C silent/unit-return form; the `open_db().expect(...)` panic
  form (different error policy — that's #19); other files' `if let Ok(conn)` shape.
- **Safe-scope:** yes for the 5 pure-A sites — scoped to one file. Confirm the
  helper's early-return shape preserves the exact `return false` control flow.

## #19 — open-db-message-const — DONE (merge 017fe18)

- **Status:** OPEN (literal de-dup; lowest risk, modest payoff)
- **Signal:** `crate::db::queries::open_db().expect("Failed to open lit.db")` — the
  identical panic message at 14 sites (action files: `pickers.rs` ×6,
  `concordance.rs` ×3, `echoes.rs`, `bookmarks.rs`; plus main/app/queries).
- **Identical part (extract):** hoist the message to a `pub const
  OPEN_DB_PANIC_MSG: &str` in `src/db/queries.rs`, OR a `fn open_db_or_panic() ->
  Connection` wrapper. (Const is the more conservative cut — no call-shape change.)
- **Variants:** none in the message; binding context (`let conn =` vs inline) varies
  but the literal is identical.
- **EXCLUDED:** the graceful `open_db_rw()`-match form (#18) and the
  `.ok().and_then(...)` form (#17) — different error policies, not this literal.
- **Safe-scope:** yes — literal → const, #8-style. Prevents the panic text drifting.

## #20 — picker-list-scaffold-helper — DONE (merge e5bf8ec)

- **Status:** OPEN (narrow it to the byte-identical pair, per the variant analysis)
- **Signal:** picker `new()` bodies repeat the `list_box` + `scrolled` construction
  after #5/#13 already took the footer/attach. The byte-identical pair
  `let list_box = ListBox::builder().selection_mode(Single).build(); let scrolled
  = ScrolledWindow::builder().child(&list_box).vexpand(true).build();` recurs at
  ~6 Variant-A picker `new()`s (bookmark/gloss/journal/media + others).
- **Identical part (extract):** `fn new_picker_list() -> (gtk4::ListBox,
  gtk4::ScrolledWindow)` in `src/ui/picker_nav.rs`. (A separate 4-line
  `ScrolledWindow::new(); set_vexpand; set_max_content_height(400);
  set_propagate_natural_height(true)` block recurs at 4 imperative-style pickers —
  a SECOND narrow helper, not unified with the builder one.)
- **Variants:** A = builder-style 600×400 card (extract the list_box+scrolled pair);
  C = imperative `ScrolledWindow::new()` + `max_content_height` (its own 4-site
  helper). Do NOT unify A and C — builder vs imperative + different CSS classes;
  merging would change literals = not behavior-preserving.
- **EXCLUDED:** the `picker_box` builder block itself (media_picker inserts a
  title; concordance uses 400×400/different class — too variant to share cleanly);
  scrim/header pickers (library/echo — those are the #21 family); authorship
  (hand-rolled). Extract ONLY the byte-identical list_box+scrolled pair.
- **Safe-scope:** yes for the narrowed pair — no per-site inputs.

## #21 — picker-header-scrim-helpers — DONE (merge e5bf8ec)

- **Status:** OPEN (clean but small — 3–4 sites each)
- **Signal:** the scrim block (`GtkBox::builder().hexpand.vexpand.build();
  add_css_class("library-picker-scrim"); set_visible(false)`) and the header_box +
  title block (`add_css_class("library-picker-header"/"-title")`) recur across the
  scrim-style pickers/overlays.
- **Identical part (extract):** `fn build_picker_scrim() -> gtk4::Box` (4 sites:
  echo_picker, concordance_works_picker, library_picker, settings_overlay) and
  `fn build_picker_header(title: &str) -> (gtk4::Box, gtk4::Label)` (3 sites: echo,
  echo_turns, concordance_works — byte-identical but the label string).
- **Variants:** header B — `library_picker` appends a second `header_crumb` after
  the title; it can call the helper then append (still behavior-preserving).
- **EXCLUDED:** any overlay whose scrim composes differently with an ask-card.
- **Safe-scope:** yes — two tiny widget-construction helpers; lower payoff than
  #15–#19, list last.

### Examined and EXCLUDED in Batch 2 (no clean cut — do NOT number)

- **`move_selection` preamble family** (picker): #6 took the `select_row_at` tail;
  the preamble splits into 3 byte-identical sub-variants (unwrap_or(-1) plain /
  +`.max(0)` / `if let Some` guard) of 2–5 sites each. Each is only 3–5 lines and
  the variance is real — if ever extracted, per-variant only; NOT one helper.
  Left unnumbered: payoff is marginal and forcing a single signature would be
  mis-scoped. (library/keybinds/settings move_selection genuinely differ — wrap +
  scroll, rem_euclid, skip-disabled — hard-excluded.)
- **seek-then-suppress sequence** (handler, ~7 sites): the bare
  `suppress_sync_until = Some(Instant::now() + SYNC_SUPPRESS_SEEK)` statement
  recurs, but the surrounding Seek ops reorder per site and `do_mpv_seek` already
  centralizes the reader binds. A `fn suppress_sync_for_seek(s)` would dedup only
  one statement; the `navigation.rs` "don't-shorten-existing" max-form variant
  differs. Marginal — note, don't number. (A `fn preroll_seek_time(start) -> f64`
  for `(start - SEEK_PREROLL).max(0.0)` ~7 sites is the better latent cut here if
  revisited.)
- **restore-return-position 4-liner** (handler): `s.current_line=line;
  s.page_top_line=top; resnap_page; update_highlight` after a `<field>.take()` —
  2 pure byte-identical (journal/gloss) + 2 variant (search else-arm, gloss
  jump-guard). Only 2 clean sites; borderline. Note, don't number.
- **wl-copy stdin-pipe block** (handler, 3 sites): `Command::new("wl-copy").stdin
  (piped).spawn()…write_all…wait` at word_copy.rs ×2 + visual.rs. A
  `fn copy_to_clipboard(text, log_tag)` would dedup 3 sites — but the 4
  fire-and-forget `wl-copy` arg-form sites (keymap/gloss/echoes) are a DIFFERENT,
  smaller pattern and can't merge without an arg→stdin behavior change. 3 sites is
  at the floor; note, don't number unless the clipboard path is touched anyway.
- **standalone picker j/k arms** (handler, 3 handlers): echo_picker /
  echo_turns_picker / library_picker still hand-roll `move_selection(±1)` instead
  of routing through the existing `picker_keys::resolve_picker_key`/`PickerAction`
  path that voice_picker uses. The clean fix is *routing through the shipped
  helper*, not a new extraction — a follow-on to the picker-dispatch project, not
  a Batch-2 numbered cut.
- **db families with no byte-identical unit:** `ensure_*_table` bodies (distinct
  columns/FKs; the one shared body is already the `GLOSS_AUDIO_COLUMNS` const),
  the `prepare/query_map/collect` skeleton (distinct SQL + row-closure every time),
  FK fragments (singletons or already in a const), the `div1,div2,line_in_div`
  column list (always a different leading set). Forcing any of these needs a
  generic/trait — explicitly out of scope. The `created_at strftime` SQL DEFAULT
  (2 sites) and `has_column` pragma-probe (4 sites, normalizing `=` spacing) are
  weak/borderline named-const/helper candidates — flag only, not numbered.

---

## Batch 3 (audited 2026-06-23, after the gloss API-error + error-card padding fix)

Fresh scan over the post-Batch-2 tree (triggered while fixing the gloss overlay
auth-error display). Ranked by (duplication × drift_risk) ÷ scope_size. Each
verified by direct grep, not agent word alone (the #11 lesson). All six are
byte-identical-modulo-one-token; none needs a trait/generic.

## #22 — select-first-row-helper — DONE (merge pending, commit bc5e612)

- **Status:** DONE — 15 sites collapsed to picker_nav::select_first_row; build + 413 tests green.
- **Signal:** the "select the first row after populate" block
  `if let Some(row) = self.list_box.row_at_index(0) { self.list_box.select_row(Some(&row)); }`
  — byte-identical except the binding name (`row` vs `first`) — at **15 sites**
  across 12 picker files: echo_picker:116, library_picker:393, voice_picker:154,
  echo_line_picker:77, journal_picker:125, concordance_works_picker:78,
  concordance_word_picker:102, gloss_picker:124, bookmark_picker:121,
  media_picker:118, echo_turns_picker:118, concordance_picker:86 & :131,
  authorship_picker:57 & :64.
- **Identical part (extract):** `pub fn select_first_row(list_box: &gtk4::ListBox)`
  in `src/ui/picker_nav.rs` (already owns `select_row_at`). Call sites become
  `select_first_row(&self.list_box);`.
- **Variants:** binding `row` vs `first` — cosmetic, disappears in the helper.
- **EXCLUDED on sight:** any `row_at_index(0)` followed by extra per-row logic
  (not just the bare select). The two doubled sites (authorship_picker,
  concordance_picker) each have two independent bare-select copies — both qualify,
  but verify the second copy is unconditional (concordance_picker:86 is inside an
  outer conditional — the inner select block is still byte-identical, extracts).
- **Safe-scope:** yes — a 3-line GTK block → one call; only input is the list_box.

## #23 — selected-index-helper — DONE (commit bc5e612)

- **Status:** DONE — 5 picker bodies delegate to picker_nav::selected_index.
- **Signal:** `pub fn selected_index(&self) -> Option<usize>` whole-body identical
  across 5 pickers: concordance_list_picker:102, echo_turns_picker:123,
  echo_picker:121, journal_picker:129, gloss_picker:129. Body is
  `self.list_box.selected_row().and_then(|row| row.widget_name()<parse>.ok())`.
- **Identical part (extract):** `pub fn selected_index(list_box: &gtk4::ListBox)
  -> Option<usize>` in `src/ui/picker_nav.rs`; each picker's method delegates.
- **Variants:** A `.parse::<usize>().ok()` (concordance_list/echo_turns/echo);
  B `.to_string().parse().ok()` (journal/gloss) — trivially equivalent
  (`widget_name()` derefs to `&str`); the helper picks one (`.parse::<usize>()`).
- **EXCLUDED:** pickers that derive the index from `selected_row().index()`
  (echo_line_picker:90, echo_picker:128) — that's the *row position*, not the
  widget-name-encoded `items` index; different value, do NOT merge.
- **Safe-scope:** yes — pure fn over the list_box; method becomes a one-line
  delegate.

## #24 — preroll-seek-time-helper — DONE (commit d06ddb8)

- **Status:** DONE — 9 sites use navigation::preroll_seek_time; A-B-loop prerolls excluded as planned.
- **Signal:** `(start - SEEK_PREROLL).max(0.0)` — the "seek this many seconds
  before the line's start, clamped at 0" computation — at **9 sites**:
  timestamps.rs:215, echoes.rs:1359 & :1504, concordance.rs:531 & :545 & :580,
  search.rs:100 & :228, navigation.rs:1759, gloss.rs:1770. `SEEK_PREROLL` is
  already `pub const SEEK_PREROLL: f64 = 0.2` (navigation.rs:57).
- **Identical part (extract):** `pub fn preroll_seek_time(start: f64) -> f64`
  beside the const in `src/input/navigation.rs`. Sites become
  `preroll_seek_time(ts.start)`.
- **EXCLUDED — different preroll consts, do NOT merge:** keymap.rs:2360
  `(a - CHUNK_PREROLL).max(0.0)` and echoes.rs:885 `(a - TURN_PREROLL).max(0.0)`
  — A-B-loop start prerolls with their own constants, a distinct concept.
- **Safe-scope:** yes — one-expression helper; #12-sibling (that named the const,
  this names the computation that uses it).

## #25 — mpv-set-property-cmd-helper — DONE (commit 681db30)

- **Status:** DONE — 6 sends use set_property_cmd; static pause strings excluded.
- **Signal:** `format!(r#"{{"command":["set_property","<PROP>",{}]}}"#, val)` —
  byte-identical envelope, varying only the property-name literal + value — at 6
  sites in `src/mpv/client.rs`: :44 (ab-loop-a), :45 (ab-loop-b), :50 (pause),
  :125 (speed), :158 (ab-loop-a), :159 (ab-loop-b).
- **Identical part (extract):** file-local `fn set_property_cmd(prop: &str, val:
  impl Display) -> String` in `src/mpv/client.rs`.
- **EXCLUDED:** the two static-string `pause` sends (:112 true, :120 false) — no
  format args; converting them adds a call where a `&'static str` literal works.
- **Safe-scope:** yes — file-local format-template helper; protects the JSON
  envelope from drifting between commands.

## #26 — mpv-seek-absolute-cmd-helper — DONE (commit 681db30)

- **Status:** DONE — 4 sends use seek_absolute_cmd; relative-exact seek excluded.
- **Signal:** `format!(r#"{{"command":["seek",{},"absolute"]}}"#, time)` —
  byte-identical except the time-var name — at 4 sites in `src/mpv/client.rs`:
  :47, :118, :132, :160.
- **Identical part (extract):** file-local `fn seek_absolute_cmd(time: f64) ->
  String`.
- **EXCLUDED — distinct second template:** client.rs:138
  `["seek",{},"relative","exact"]` is a different seek mode; give it its own
  `seek_relative_exact_cmd` only if it ever gains a second site (currently 1×).
- **Safe-scope:** yes — file-local template helper.

## #27 — card-side-margin-helper — DONE (commit f8e9459)

- **Status:** DONE — 9 sites (audit undercounted; layout.rs:189 included) use crate::ui::card_side_margin; the column_width/8 echo sites stayed untouched (the critical exclusion).
- **Signal:** `card_width / 4` — the gloss/synopsis/ask card "side margin = a
  quarter of the live card width" — at 8 computation sites: gloss_overlay.rs:582
  (`show`), :622 (`show_gloss_with_color`), :642 (`bar_left`), :704
  (`show_glossing`), :727 (`bar_left`), :888 (`show_synopsis`); ask_card.rs:101;
  layout.rs:109 (`card_w / 4 - text_margins`, the translation-view variant).
- **Identical part (extract):** `const CARD_SIDE_MARGIN_DIVISOR: i32 = 4` (or
  `fn card_side_margin(card_width: i32) -> i32 { card_width / 4 }`), shared by the
  overlay + ask_card; layout.rs calls it then subtracts `text_margins` (variant).
- **Variants:** the 6 gloss_overlay sites are plain `card_width / 4`; ask_card
  applies it to both start+end; layout.rs:109 subtracts `text_margins` (keeps that
  inline after the helper).
- **EXCLUDED — CRITICAL, different value & concept:** the echo view's
  `self.column_width / 8` (gloss_overlay.rs:770/775/791) and `right_margin =
  column_width / 8` (:175). These are anchored to the FIXED column_width (1050/8),
  NOT the live card_width, and the code comment (lines 773-774) pins the echo list
  to `column_width/8`. The past "tiny margin on a wide card" bug (commented at
  618-621) is exactly what conflating `/8` with `/4` would reintroduce — do NOT
  unify the two divisor families.
- **Safe-scope:** yes — literal → named const/helper, #8/#12-style. Lowest copy
  count of Batch 3 but the one with a documented drift-hazard, so worth naming.

### Examined and EXCLUDED in Batch 3 (no clean cut — do NOT number)

- **settings_overlay arrow-label template** (`format!("\u{25C0} {} \u{25B6}", v)`
  + the `{}px` variant) — ~14 sites but ALL in one file (settings_overlay.rs:272–
  426). A file-local `fn arrow_label(&str)` / `fn arrow_px_label(i32)` is a fine
  tidy, but it's a single-file cosmetic spinner-value format with no cross-file
  drift risk — low payoff. Flag only; do as a drive-by if settings_overlay is
  touched, not as its own numbered PR.
- **echo-picker row-construction block** (meta label + ellipsized first-line
  label, ~14 lines) — byte-identical at exactly 2 sites (echo_picker.rs:86–113,
  echo_turns_picker.rs:91–115), differing only by the field name (`passage_text`
  vs `turn_text`). Meets the 2-site/5-line floor but is borderline; the field-name
  difference would force a closure/getter param. Note, don't number unless a 3rd
  echo-style picker appears.
- **`"Error: {}"` prefix** (5 sites: visual.rs ×3, claude_bridge.rs, settings.rs)
  — the literal is shared but each routes to a DIFFERENT sink (gloss_overlay.show
  vs on_error callback vs voice_picker.set_status). No single helper fits; the
  only shared token is the 7-char prefix. Marginal; do not number.
- **`format!("%{}%", x)` LIKE-wildcard** (4 sites: queries.rs:2215,
  concordance.rs:24, gloss.rs:787, viewport.rs:2701) — same template, different
  modules + inputs (some lowercase first). A `fn like_contains(&str) -> String`
  would dedup 4 trivial sites across 4 modules — payoff below the floor. Note only.
- **`set_widget_name(&idx.to_string())` row tail** (4 sites) — overlaps the
  already-handled picker-list-scaffold / widget-name territory (#20); the other 6
  `ListBoxRow::builder().child` sites use non-index widget-names. Not a clean new
  family on its own.

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

- **AppState god-struct** (`src/app/mod.rs`) — ~217 fields, de-facto global.
  **STARTED (Phase A DONE, merge ddf20c2).** A blast-radius inventory scoped the
  project to the **contained single-file clusters only** — `nav_test`, `journal`,
  `page_image`, `word_cycle`, `echo_overlay`, `scansion`, `vocab_popup` — each its
  own sub-project sequenced lowest-risk-first. The **core fields stay flat
  deliberately** (`buffer` 291 hits/20 files, `current_line` 263/22,
  `current_work` 196/23, `config` 167/21, `text_view`, `input_mode`): grouping them
  is huge churn for ~no readability gain. Idiomatic per the existing
  `ab_repeat: AbRepeatState`; sub-structs init via a nested literal/`::default()`
  in `build_window` (the only build_window touch). Verification is per-cluster
  risk-tiered: pure-state clusters use 413+clippy (tier-a), render-touching ones
  (`vocab_popup`, `scansion`) add a user nav-fuzz before merge (tier-b).
  **Phase A — `nav_test` → `NavTestState`** (6 fields, 28 access sites in one file,
  pure-tier) — DONE: first behavior-changing slice (access shape only), exhaustive
  drift check confirmed zero behavioral mutations; 413+clippy 115 unchanged. Spec/
  plan under docs/superpowers/ (2026-06-23). **Phase B — `journal` → `JournalState`**
  (4 fields, 33 access sites in one file, pure-tier) — DONE: established the
  **non-default-init variant** — `build_window` uses an explicit nested literal
  (not `::default()`) to preserve `journal_prompt_mode: JournalPromptMode::Ask`,
  since the enum has no `Default`. Boundary fields (`journal_overlay`/`_picker`/
  `_band`) correctly untouched. Exhaustive drift check: zero mutations; 413+clippy
  115 unchanged. Spec/plan (2026-06-24). The two init variants are now both proven:
  all-`Default` cluster → `::default()` (nav_test); any non-default field →
  explicit nested literal (journal). **Phases C/D/E — the remaining pure-tier
  clusters — DONE (batched, merges c9039bf / 149a23b / 8451759):**
  `word_cycle` → `WordCycleState` (5 fields, 20 sites in word_copy.rs, merge
  c9039bf); `echo_overlay` → `EchoOverlayState` (6 fields, 91 sites across
  echoes.rs + keymap.rs, merge 149a23b); `page_image` → `PageImageState` (5
  fields, ~43 sites all internal to mod.rs's image/calibration fns, merge
  8451759). All three are the all-`Default` `::default()` variant. Drift-checked:
  zero behavioral mutations; substring boundaries held (`word_bold_tag`,
  `page_image_overlay`/`page_image_for_line_id`/`refresh_page_image`,
  echo_session/pickers). 413 + clippy 115 unchanged; verified on merged master.
  Spec/plans (2026-06-24). **All five contained PURE-TIER clusters are now done**
  (nav_test, journal, word_cycle, echo_overlay, page_image). **Remaining contained
  clusters:** `scansion`, `vocab_popup` — both **render-tier** (touch displayed
  scansion marks / the vocab Popover widget), so they need a **user nav-fuzz gate
  before merge** (the agent can't launch cage). vocab_popup is the hardest (8
  access files, holds a real widget, not `Default`-derivable) — do it last.
  **Phase F — `scansion` → `ScansionState`** (3 fields, 21 sites across mod.rs/
  keymap/navigation, merge ace9857) — DONE: the **first render-tier cluster**.
  Explicit-nested-literal init (`ScanLevel::Off`, no `Default`). Both boundary
  traps held — `scansion_label_tag` (TextTag) and `s.config.scansion_level`
  (Config) stay flat. Zero-drift. Verified by the **two-part user render gate**:
  nav-fuzz on `Son` (scansion-off nav) PLUS a manual scansion-ON eyeball on `TN`
  (`Alt+i` cycles Off→StressOnly→Full→Off) — marks render correctly post-grouping.
  413+clippy 115.
  **Phase G — `vocab_popup` → `VocabPopupState`** (7 fields incl. the VocabPopup
  widget, 45 sites / 5 files, merge — see git log) — DONE: the **final and hardest
  contained cluster**. It holds the Popover widget, so a widget/state name-collision
  forced a **two-way rewrite** — state fields `s.vocab_popup_x` → `s.vocab_popup.x`
  AND bare-widget calls `s.vocab_popup.m()` → `s.vocab_popup.popup.m()` (16 sites;
  the cargo-build-as-checklist strategy surfaced 2 beyond the planned 14).
  Explicit-nested-literal init captures the widget local + `VocabView::Definition`
  (non-default). The separate vocab-HIGHLIGHT fields (`vocab_words`/`vocab_matches`/
  `vocab_match_idx`/`vocab_tag`/`vocab_highlight_visible`) stayed flat. Zero-drift;
  verified by the user render gate (nav-fuzz + manual popup open/update/view-toggle/
  hide eyeball). 413+clippy 115.
  **✅ AppState god-struct grouping — COMMITTED SCOPE COMPLETE.** All seven contained
  clusters are grouped into sub-structs (nav_test, journal, word_cycle, echo_overlay,
  page_image, scansion, vocab_popup), across Phases A–G. Both init variants
  (`::default()` / explicit nested literal) and both verification tiers (pure /
  render) are proven.
  **Out of scope (unchanged):** grouping the core fields (stays flat, likely permanently);
  medium-spread clusters (search, mpv/sync, translations, gloss-state, toasts,
  gutter) — re-evaluate after the contained set ships.
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
  which is the prerequisite for any real `build_window` split. ~~Pre-existing
  flake: `db::queries::tests::test_bookmark_toggle`.~~ **FIXED (merge e172779).**
  Root cause: it and `test_load_bookmarks_with_details` toggled the same Ham row
  on the shared real lit.db in parallel with no serialization (the only 2 of 34
  tests using `open_db_rw`), reading each other's writes — the other test's
  INSERT landed between this test's toggle-off DELETE and its `!contains` assert.
  Fixed by isolating both in a fresh in-memory DB via a shared `bookmark_fixture()`
  (stub `works` + `line_mapping` + the real bookmarks schema), matching the
  32-test in-memory majority. Verified across 6 consecutive clean full-suite runs.

- **DECISION (2026-06-24): the two still-parked larger projects are NOT worth
  doing — leave them parked, do not re-litigate.** The "larger projects" section
  is otherwise complete (picker-dispatch accessor, the entire god-struct
  *contained-cluster* grouping Phases A–G, all three app.rs carve-up phases, and
  gloss_overlay — all merged). What remains is exactly two coupled items, and the
  blast-radius data gathered when scoping the god-struct project shows the cost
  exceeds the benefit:
  - **Grouping the AppState *core* fields** (`buffer` 291 hits/20 files,
    `current_line` 263/22, `current_work` 196/23, `config` 167/21, `text_view`,
    `input_mode`). Each is a 90–290-site rewrite, all **render-tier** (drives the
    reading view → needs a user nav-fuzz/eyeball per slice, not just
    `cargo test`), for **~zero readability gain** — `state.buffer` /
    `state.current_line` are already perfectly clear as flat fields. Worst
    churn:value ratio in the entire backlog. (Contained clusters like `nav_test_*`
    / `vocab_popup_*` were worth grouping because they're a genuine *cluster* that
    reads better named; the core fields are the irreducible reader state and are
    not.) This is why Phase A–G's scope note said the core fields "stay flat,
    likely permanently" — a judgment, not a deferral.
  - **The `build_window` body + `display_work` split** is **blocked on the core
    split** (build_window's body is the ~203-field `AppState` struct literal +
    closures capturing `state` built mid-fn; no *worthwhile* grouping unblocks
    it). Its only payoff is a smaller `build_window` — real, but `mod.rs` is
    already 6735 → ~3,950 and navigable, so the unblock isn't worth its
    prerequisite.
  - **When this changes:** only a *specific* concrete pain re-opens a slice — e.g.
    `build_window` becomes genuinely unworkable to edit, or a particular core
    field's flatness causes an actual bug. "Finish the section for completeness"
    is the wrong reason; it would be the most expensive, least rewarding work in
    the repo. The valuable structural work (everything with a good churn:value
    ratio) is done.

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

## #28 — parse-citation-reuse (collapse cite_tail closures) — DONE (commit 95c7343)

- **Status:** DONE (commit 95c7343) — 4 cite_tail closures deleted, repointed to app::parse_citation. Rank #1 of this batch — highest (4 dup sites × real drift) ÷
  (tiny cut: the helper already exists and is tested).
- **Signal:** the citation-tail parse `rsplitn(4,'.') -> (div1,div2,line)` is
  inlined as a `cite_tail` closure at **4 sites**, byte-identical except a `: i64`
  annotation and indentation:
  gloss.rs:29 (jump_to_gloss_source_start), gloss.rs:1972 (toggle_overlay),
  journal.rs:588 (action_gloss_from_journal_passage), keymap.rs:1076 (J handler).
- **Identical part (extract → ALREADY EXISTS):** `crate::app::parse_citation(cite)
  -> Option<(i64,i64,i64)>` (app/mod.rs:3583) is byte-identical to all four and is
  already `pub(crate)` and unit-tested (`parse_citation_extracts_div_and_line`).
  So this is NOT a new helper — each `cite_tail` closure is deleted and its call
  sites call `crate::app::parse_citation(...)`.
- **Variants:** `: i64` annotation present at 2 sites, absent at 2 — disappears
  when calling the typed free fn. journal.rs:588 parses BOTH start and end
  citations with the same closure — both calls map to parse_citation.
- **EXCLUDED on sight:** the `cite_tail` name is reused but any site whose closure
  body differs from the bare 3-field tail (none found — all 4 are the bare form).
  Do NOT touch `GlossContext`-building citation construction (`models::citation`,
  the forward direction, audit #14) — this is the parse direction only.
- **Safe-scope:** yes — delete 4 closures, repoint to an existing tested fn. Zero
  new code; pure dedup. The strongest cut in the ledger since #22.

## #29 — journal-page-row-mapper — DONE (commit 5900e79)

- **Status:** DONE (commit 5900e79) — 4 row builders -> map_journal_page_row + JOURNAL_PAGE_COLUMNS const. Rank #2 — 4 sites, single file, clean fn extraction.
- **Signal:** the `|row| Ok(JournalPage { id: row.get(0)?, … source_text:
  row.get(9)? })` 10-field row-builder closure is byte-identical at **4 sites** in
  src/db/journal.rs: find_journal_pages:90, find_work_pages:118,
  find_all_pages_ordered:150, find_passage_pages:206. The SELECT column list
  string `"id, div1, div2, question, answer, COALESCE(claude_model,''), timestamp,
  start_citation, end_citation, source_text"` is also byte-identical across the 4
  queries (a two-part family: column-list const + row mapper).
- **Identical part (extract):** a file-local `fn map_journal_page_row(row:
  &rusqlite::Row) -> Result<JournalPage, rusqlite::Error>`; each `query_map(params,
  |row| Ok(...))` becomes `query_map(params, map_journal_page_row)`. Optionally a
  `const JOURNAL_PAGE_COLUMNS: &str` for the SELECT list.
- **Variants:** only the WHERE/ORDER BY and the `params!` binding differ — left at
  each call site. find_passage_pages reformats the closure to one-per-line but is
  structurally identical.
- **EXCLUDED:** find_journal_scenes:218 maps to `(i64,i64)` tuples, not JournalPage
  — different return type, hard exclude. No JournalPage row-builder in queries.rs.
- **Safe-scope:** yes — pure data-mapping extraction; query bodies unchanged.

## #30 — overlay-attach-body — DONE (commit 694ac40)

- **Status:** DONE (commit 694ac40) — 3 attach() -> picker_attach::attach_overlay_panel. Rank #3 — 3 sites, cross-file, genuine drift risk.
- **Signal:** the 7-line overlay-attach wiring is byte-identical at **3 sites**:
  GlossOverlay::attach (gloss_overlay.rs:566), JournalOverlay::attach
  (journal_overlay.rs:117), TranslationOverlay::attach (translation_overlay.rs:131).
  Body: `set_child(Some(child)); add_overlay(&scrim); add_overlay(&container);
  set_measure_overlay(&scrim,false); set_measure_overlay(&container,false);
  set_clip_overlay(&scrim,true); set_clip_overlay(&container,true);` — all three
  types have identically-named `overlay`/`scrim`/`container` fields.
- **Identical part (extract):** `fn attach_overlay_panel(overlay: &gtk4::Overlay,
  child: &impl IsA<Widget>, scrim: &gtk4::Box, container: &gtk4::Box)` in
  src/ui/picker_attach.rs (the existing home for this class, audit #13). Each
  `attach` becomes one call.
- **EXCLUDED (named, why):** picker `attach_panel` (#13) — omits the
  measure/clip overlay calls, different contract. settings_overlay::attach — adds
  scrim+container separately via `panels()`. gamepad_overlay — adds a drawing_area
  not a scrim/container pair. echo_keybinds_overlay::attach_to / page_image_overlay
  — take an external `&Overlay`, no `set_child`, different signature.
- **Safe-scope:** yes — pure widget wiring; only inputs are the 4 widget refs.

## #31 — reassert-italic-tags — DONE (commit 43a5f71)

- **Status:** DONE (commit 43a5f71) — 2 italic loops -> ui::reassert_italic_tags; guard test passes. Rank #4 — only 2 sites, but a comment cross-reference proves
  the two are EXPECTED to stay in sync (drift risk is documented, not theoretical).
- **Signal:** the 7-line italic-priority re-assertion inside each overlay's
  `apply_font` per-view loop is byte-identical at **2 sites**:
  gloss_overlay.rs:436, journal_overlay.rs:360. Body: `let top = table.size(); for
  italic in ["gloss-stage","gloss-bracket"] { if let Some(t)=table.lookup(italic){
  if top>0 { t.set_priority(top-1); } } }`. journal_overlay.rs's comment literally
  says "Mirror the same fix used in gloss_overlay.rs" — so a third italic tag added
  to one must be added to the other.
- **Identical part (extract):** `fn reassert_italic_tags(table:
  &gtk4::TextTagTable)` (a new src/ui/overlay_util.rs or inline in ui/mod.rs). Each
  per-view loop body collapses to one call.
- **EXCLUDED:** the rest of each `apply_font` is NOT identical — gloss iterates 3
  views + uses tag name "gloss-font" + calls apply_synopsis_label_bold after;
  journal iterates 2 views + "journal-font" + has an early-return guard. Only the
  7-line italic loop extracts; the surrounding apply_font stays per-overlay.
- **Safe-scope:** yes, narrowly. At the 2-site floor — but the explicit "mirror
  this fix" comment is exactly the drift signal the house bar wants.

## #32 — gloss-overlay-clip-helpers-route-to-shared — DONE (commit b9c7d26)

- **Status:** DONE (commit b9c7d26) — gloss_overlay private display_rows + recompute_bottom_clip deleted, all callers route to ui::mod shared helpers; ~85 lines removed; 445 tests, clippy parity.
  (was: highest-value of the 2026-06-25 post-citation-work audit —
  removes ~85 lines AND kills a lockstep-fragile duplicate of tested clip math).
- **Signal:** the bottom-clip refactor extracted `display_rows`,
  `bottom_clip_height` (pure, tested), and `recompute_overlay_bottom_clip` into
  `src/ui/mod.rs:84-160`. **journal_overlay routes through them**
  (journal_overlay.rs:427,442). **gloss_overlay never got converted** — it still
  carries private copies: `gloss_overlay.rs:1091-1108` `fn display_rows` is
  **byte-identical** to `ui::mod::display_rows` (mod.rs:124-142, verified — only
  the helper's leading `use gtk4::prelude::*;` + indentation differ); and
  `gloss_overlay.rs:1021-1073` `fn recompute_bottom_clip` reimplements the tested
  `bottom_clip_height` + `recompute_overlay_bottom_clip` pair
  (last_full_bottom/any_full/effective_bottom/guard logic matches mod.rs:84-115).
- **Identical part (extract / route):** delete both private fns;
  `gloss_overlay.rs:1111-1113` `update_bottom_clip` becomes the same one-line
  shared call journal already uses:
  `crate::ui::recompute_overlay_bottom_clip(&self.gloss_view, &self.bottom_clip,
  &self.gloss_scrolled)`. Field names line up.
- **EXCLUDED (named, why):** gloss's `row_step` (gloss_overlay.rs:1513) and
  `snap_value_to_line` (:1537) — they intentionally use `display_rows`-based
  geometry and differ from journal's `line_yrange`-based versions; not part of
  this cut.
- **Safe-scope:** yes — routes to the already-running tested superset journal
  uses. The drift risk (two clip implementations that must stay in lockstep) is
  exactly the bug class the shared helper was created to kill.

## #33 — two-label-picker-row-builder — DONE (commit 66c844a)

- **Status:** DONE (commit 66c844a) — gloss/bookmark/journal row builders -> picker_nav::two_label_row + speaker_prefixed_first_line; 445 tests, clippy parity..
- **Signal:** inside `populate_list`, three card pickers build the identical row
  — an ellipsizing start-aligned `text_label` (hexpand) + an end-aligned
  secondary label with css `picker-item-detail` + an hbox (Horizontal, spacing 8)
  appending both: gloss_picker.rs:99-117, bookmark_picker.rs:96-114
  (character-identical to gloss's block), journal_picker.rs:100-119 (same shape,
  only local var names differ). The preceding `speaker.is_empty()` display
  computation is byte-identical between two: gloss_picker.rs:92-97 and
  bookmark_picker.rs:89-94.
- **Identical part (extract):** a helper `(primary: &str, detail: &str) ->
  gtk4::Box` returning the configured spacing-8 hbox with the two aligned labels.
  Caller wraps in `ListBoxRow` and stamps `widget_name` (varies: `idx` vs
  `item.line_mapping_id`) — that stays out of the helper.
- **EXCLUDED (named, why):** echo_picker.rs:86-113 / echo_turns_picker use a
  **Vertical** row_box (meta-over-text), a structurally different row.
  concordance_works_picker / concordance_list_picker use a single primary + count
  detail with explicit `set_margin_*`, a different builder style. The
  `speaker.is_empty()` display-compute is a separate optional 2-site sub-cut.
- **Safe-scope:** yes — pure widget construction, identical CSS/align/spacing.

## #34 — picker-move-selection-two-families — DONE (commit 6d7dc7f)

- **Status:** DONE (commit 6d7dc7f) — 9 sites -> picker_nav::move_selection_clamped (5) / move_selection_from (4); two helpers preserve the clamp-vs-no-clamp contract..
- **Signal:** `picker_nav` has `select_row_at`/`select_first_row`/`selected_index`
  but NOT `move_selection`; every picker hand-rolls it in exactly TWO shapes.
  **Family A (clamp-from-current):** `if let Some(current) =
  list_box.selected_row() { let idx = current.index(); let new_idx = (idx +
  delta).max(0); select_row_at(...) }` — **5 byte-identical bodies**:
  bookmark_picker.rs:130-136, gloss_picker.rs:131-137, journal_picker.rs:128-134,
  media_picker.rs:148-154, concordance_picker.rs:138-144.
  **Family B (unwrap_or(-1), no clamp):** `let current =
  ...selected_row().map(|r| r.index()).unwrap_or(-1); let next = current + delta;
  select_row_at(...)` — 4 identical: concordance_word_picker.rs:111-115,
  concordance_list_picker.rs:106-110, concordance_works_picker.rs:96-100,
  echo_line_picker.rs:80-84.
- **Identical part (extract):** TWO free fns in `picker_nav` —
  `move_selection_clamped(list_box, delta)` (Family A: requires a current
  selection, clamps ≥0) and `move_selection_from(list_box, delta)` (Family B:
  −1 start, no clamp). Each is the exact existing body with `self.list_box` →
  `list_box`.
- **EXCLUDED (named, why):** the `-1`-vs-`0` sentinel and the presence of
  `.max(0)` are what distinguish A from B — that is why this is two helpers, not
  one (folding them would change behavior). Near-variants with `.max(0)` added
  (echo_picker.rs:123-127, echo_turns_picker.rs:125-128,
  authorship_picker.rs:69-72) are a THIRD shape — flag only, don't fold.
  voice_picker.rs:172 and library_picker.rs:483 carry extra logic beyond the
  pattern — verify before routing, likely exclude.
- **Safe-scope:** yes per family — identical bodies, mechanical `self.` → param.

## #35 — picker-card-builder-600x400 — DONE (commit a2a7f54)

- **Status:** DONE (commit a2a7f54) — 4 card boxes -> picker_nav::build_picker_card; unused Orientation imports removed.
- **Signal:** the `picker_box = GtkBox::builder()...width_request(600)
  .height_request(400)...add_css_class("library-picker")` block is byte-identical
  at **4 sites**: gloss_picker.rs:20-28, journal_picker.rs:28-36,
  media_picker.rs:23-31, bookmark_picker.rs:20-28.
- **Identical part (extract):** `picker_nav::build_picker_card() -> gtk4::Box`
  returning the configured 600×400 `library-picker` box.
- **EXCLUDED (named, why):** concordance_picker.rs:18-26 (width 400, css
  `concordance-picker`), echo_picker.rs:24-32 (640×520, spacing 0),
  concordance_word_picker.rs:17-22 (uses `GtkBox::new` + setters, not the
  builder). Genuinely a 4-site family, not 12.
- **Safe-scope:** yes — fixed-config widget construction.

## #36 — gloss-normalize-abbrev-reuse — DONE (commit 8abc18a)

- **Status:** DONE (commit 8abc18a) — gloss.rs:786 + queries.rs:1898 route through gloss::normalize_abbrev; guard/superset sites excluded as planned..
- **Signal:** `gloss::normalize_abbrev` (gloss.rs:524) IS
  `abbrev.strip_suffix("-Amb").unwrap_or(abbrev)`. Two sites re-inline that exact
  expression instead of calling it: gloss.rs:786
  (`source_work.strip_suffix("-Amb").unwrap_or(source_work)`) and queries.rs:1898
  (`exclude_work.strip_suffix("-Amb").unwrap_or(exclude_work)`). Both return
  `&str` and feed a SQL bind / comparison.
- **Identical part (route):** replace the two inline expressions with
  `crate::gloss::normalize_abbrev(...)` (or bare `normalize_abbrev(...)` inside
  gloss.rs). Centralizes the `-Amb` literal behind the one helper that owns it.
- **EXCLUDED (named, why):** queries.rs:373 `if let Some(base) =
  abbrev.strip_suffix("-Amb")` is a control-flow GUARD (the fallback branch only
  runs when the suffix is present), NOT `unwrap_or` — routing it would change
  behavior. app/mod.rs:2227 `base_work_abbrev` strips at the FIRST `-`
  (`-Amb`/`-BBC`/`-Ep-N`), a SUPERSET with ~15 call sites — not interchangeable
  with `normalize_abbrev` (which keeps `-BBC`/`-Ep-N`). The SQL `NOT LIKE
  '%-Amb'` (gloss.rs:802) is a SQL pattern, not a Rust suffix — no shared const.
  Do NOT add a new `const AMB_SUFFIX` or `base_abbrev()` — the helper exists.
- **Safe-scope:** yes — identical semantics, both `&str`.

## #37 — column-exists-pragma-helper — DONE (commit 3b560fc)

- **Status:** DONE (commit 3b560fc) — 3 `.exists([])?` probes
  (ensure_claude_model_columns/characters/gloss_audio) -> `column_exists(conn,
  table, col)`; the error-swallowing `works.default_voice_id` probe was EXCLUDED
  (it deliberately doesn't propagate with `?`), keeping the cut behavior-preserving.
  The borderline concern resolved cleanly: only the 3 same-shape sites collapsed.
- **Signal:** the `pragma_table_info` column-exists migration guard recurs at **4
  sites**: queries.rs:670, :725, :800, :903, each running `SELECT 1 FROM
  pragma_table_info('<table>') WHERE name = '<col>'`. The comment at :657 already
  says it "mirrors `ensure_characters_table`'s `pragma_table_info` pattern."
- **Identical part (extract):** `fn column_exists(conn, table: &str, col: &str)
  -> bool` centralizing the SQL skeleton.
- **EXCLUDED / caveat:** :670 uses `format!` while others are static strings, and
  table/column vary — so this is "parameterize a SQL template," closer to an
  abstraction than a byte-identical block extraction. The house bar leans against
  it; included only because the drift count (4) and the existing "mirrors…"
  comment are real signals.
- **Safe-scope:** marginal — behavior-preserving, but it is a new helper with
  interpolated args, not a literal-naming or byte-identical cut.

## #38 — claude-bridge-async-render-tail — OPEN

- **Status:** OPEN (HIGHEST of the 2026-06-25 post-gloss-fixes audit — the
  deferred family audit #7 flagged; ~240 lines across 4 sites, and each NEW
  gloss-type path copies the whole tail, so drift is structural).
- **Signal:** the async Claude-call render tail — the
  `match result { Ok(Ok(gloss_text)) => { save_gloss → find_glosses_by_start
  reload → render overlay → reinstall gloss_context }, Ok(Err)/Err => log }` body
  — is byte-identical (verified: the `Ok(Ok)` `save_gloss` arm matches
  token-for-token between visual.rs and journal.rs) at **4 sites**:
  visual.rs:528-586 (`action_reader_gloss`), visual.rs:676-734
  (`action_gloss_with_claude`), visual.rs:941-1000
  (`run_pending_inner_monologue_blocking`), journal.rs:702-762 (`ask_claude`,
  gloss-from-journal). The arms differ in **3 tokens** — the gloss_type literal
  (the `save_gloss` arg + `record_last_gloss` arg) and the log-prefix strings.
- **Identical part (extract):** a NEW sibling helper (e.g.
  `persist_render_install_gloss(s, ctx, text, gloss_type, log_prefix)`) — it
  moves `ctx` in, persists, reloads, renders, AND reinstalls
  `s.gloss_context = Some(ctx)` + calls `record_last_gloss`. Each `match` arm
  collapses to one call.
- **EXCLUDED (named, why):** the existing `persist_and_render_gloss` (gloss.rs)
  does NOT cover these — it takes `&str`, does NOT reinstall gloss_context, and
  does NOT `record_last_gloss` (add/edit keep context live); the async sites move
  `ctx` in and must reinstall it. The inner-monologue site adds one line
  (`verify_echo_citations`) and renders the verified text — pass the to-persist
  text in so the body stays shared. The `spawn_future_local` + `call_claude`
  PROLOGUE differs per site (different prompt const / `call_claude` vs
  `_with_prompt`) — only the post-await `match result` body extracts.
- **Safe-scope:** yes — same concrete types, no trait/generic; a parameterized
  block move.

## #39 — overlay-close-position-restore-helpers — OPEN

- **Status:** OPEN (pairs with this session's `return_to_reader_mode`: that
  centralized mode+tint, this centralizes the still-duplicated position-restore;
  7 of 8 sites collapse).
- **Signal:** the take-and-restore tail
  `if let Some((line, top)) = <FIELD>.take() { s.current_line = line;
  s.page_top_line = top; [resnap_page; update_highlight] }` recurs in two
  byte-identical variants. **No-tail (TRANSITION) variant — 4 sites:**
  journal.rs:647, journal.rs:856, journal.rs:929, keymap.rs:1130 (each is exactly
  the 3-line restore, nothing else inside). **Resnap (return-to-CARD) variant —
  3 sites:** journal.rs:138, gloss.rs:1934, keymap.rs:1050 (inner) — the same
  body plus `resnap_page` + `update_highlight`.
- **Identical part (extract):** two free fns —
  `restore_saved_position(s, pos)` (the bare 3-line restore, collapses the 4
  transition sites) and `restore_saved_position_resnap(s, pos)` (restore +
  resnap + update_highlight, collapses the 3 card sites). Two fns, not one
  bool-flag fn, keeps each call site byte-identical. Caller still passes
  `s.journal.return_pos.take()` / `s.gloss_return_pos.take()`.
- **EXCLUDED (named, why):** keymap.rs:596 (search Escape) has an
  `else { page_top_line = current_line }` branch AND runs `resnap`/`update_highlight`
  UNCONDITIONALLY outside the `if` — folding it changes control flow. The
  `return_to_reader_mode`/`input_mode = Reader` line stays at the call site (mode
  ordering differs per site; already centralized).
- **Safe-scope:** yes — pure cut-and-lift, no abstraction.

## #40 — timestamps-line-id-extraction — OPEN

- **Status:** OPEN (5 byte-identical sites in one file; cleanest cut in
  timestamps.rs).
- **Signal:** the `let line_id = { let work = match &state.current_work { Some(w)
  => w, None => return false }; work.lines[line_idx].id };` block is byte-identical
  (incl. whitespace) at **5 sites**: timestamps.rs:144-150 (set_start_time),
  :304-310 (set_chapter), :394-400 (set_end_time), :456-462 (delete_timestamp),
  :528-534 (nudge_start_time).
- **Identical part (extract):** `fn work_line_id(state, line_idx) -> Option<i64>`;
  each caller writes `let Some(line_id) = work_line_id(state, line_idx) else {
  return false };` (the `return false` can't live in the helper).
- **EXCLUDED (named, why):** `undo_timestamp` resolves the line by id lookup, not
  `line_idx` — different shape.
- **Safe-scope:** yes — byte-identical, helper returns Option.

## #41 — timestamps-sign-column-setter — OPEN

- **Status:** OPEN (4+3 sites, ~40 lines collapse to one 4-arg setter).
- **Signal:** the sign-column borrow-and-set block
  `{ let mut ht = state.has_timestamp.borrow_mut(); if bl < ht.len() { ht[bl] = V; }
  let mut manual = state.is_manual.borrow_mut(); if bl < manual.len() { manual[bl]
  = V; } }` is byte-identical (given the value V) at **4 sites**:
  timestamps.rs:218-228 (set_start, V=true), :342-352 (set_chapter, true),
  :495-505 (delete, false), :683-700 (undo). The adjacent `is_chapter_line`
  sub-block is byte-identical at **3** of those.
- **Identical part (extract):** `fn set_sign_columns(state, buffer_line, has_ts:
  bool, is_manual: bool, is_chapter: bool)` covering all 4 (incl. undo, whose
  values are computed). Each site becomes one call.
- **EXCLUDED (named, why):** none material — undo's values come from computed
  `has_ts`/`is_man`/`is_ch` rather than constants, but the setter takes them as
  args so it still covers undo.
- **Safe-scope:** yes — self-contained block move.

## #42 — unspoken-stage-direction-refusal-block — OPEN

- **Status:** OPEN (only 2 sites, but FRESHLY TOUCHED — the two copies must stay
  in lockstep, the exact drift signal the house bar wants).
- **Signal:** the `u`/end-time spoken-line gate body is byte-identical at **2
  sites**: timestamps.rs:131-141 (set_start_time) and :381-391 (set_end_time) —
  `let l = &work.lines[line_idx]; if !timestamp_allowed(l.sub_line, l.is_spoken) {
  log("TS: refused start/end time …"); show_chapter_toast(state, "Not a spoken
  line — no timestamp set"); return false; }` — identical incl. the log string and
  the toast literal.
- **Identical part (extract):** a helper returning `bool` (the `return false`
  stays at the call site: `if !timestamp_writable(state, line_idx) { return false }`).
  The toast string `"Not a spoken line — no timestamp set"` is a load-bearing
  literal worth a `const`.
- **EXCLUDED (named, why):** nudge/delete intentionally ungated (operate on
  existing timestamps) — do not add the gate there.
- **Safe-scope:** yes — byte-identical block + named literal.

## #43 — word-prefix-boundary-predicate — OPEN

- **Status:** OPEN (best cut in text_file_map.rs — a subtle byte-boundary check
  that's exactly the kind that drifts).
- **Signal:** the "needle is a prefix of haystack at a WORD boundary" test —
  `X.starts_with(needle) && X.as_bytes().get(needle.len()) == Some(&b' ')` —
  recurs at **3 sites**: text_file_map.rs:466-467 (`candidate`), :512-513
  (`nf`), :649-650 (`find_skip_target`, `nf`). Same off-by-one-prone boundary
  semantics, receiver/needle vary.
- **Identical part (extract):** `fn is_word_prefix(haystack: &str, needle: &str)
  -> bool`. All 3 sites collapse byte-identically.
- **EXCLUDED (named, why):** none — all three are the exact same boundary check.
- **Safe-scope:** yes — pure predicate.

## #44 — gloss-render-current-row-block — OPEN

- **Status:** OPEN (low — 2 strict byte-identical sites, low drift).
- **Signal:** the 13-line "render the current gloss row" block (`gloss_start`/
  `gloss_end` clone, `ctx`, `cw`, `h`, `pairs`, `show_gloss_with_color`,
  `set_position`, `set_citation`, `recolor_cached_blocks`) is byte-identical
  (modulo indent) at **2 sites**: gloss.rs:184-196 (`cycle_gloss`) and
  gloss.rs:261-273 (`delete_current_gloss` re-render).
- **Identical part (extract):** `fn render_gloss_row(s, gloss, new_idx)`.
- **EXCLUDED (named, why):** `persist_and_render_gloss` tail (renders `full_gloss`
  over `all.len()`, mutates list after, citation from `ctx`), `apply_ipa_fix`
  (owned clones, citation from ctx), `open_gloss_overlay` (empty source_lines) —
  all near-but-not-byte-identical.
- **Safe-scope:** yes.

## #45 — gloss-row-map-closures — OPEN (low)

- **Status:** OPEN but LOW (2 sites per closure, a `|row|` lambda is already
  near-minimal; named for completeness).
- **Signal:** two byte-identical (modulo indent) row-map closures — the SavedGloss
  7-field map at queries.rs:1588-1599 (`find_all_glosses`) and 1637-1647
  (`find_glosses_by_start`); the GlossedPassage 8-field map at 1700-1709
  (`find_glossed_passages`) and 1750-1759 (`find_glossed_passage_by_start`),
  incl. the `row.get::<_, Option<String>>(6)?.unwrap_or_default()` speaker line.
- **Identical part (extract):** `fn row_to_saved_gloss(row) ->
  rusqlite::Result<SavedGloss>` and `fn row_to_glossed_passage(row)`, passed as
  `|row| row_to_*(row)`.
- **EXCLUDED (named, why):** `find_existing_gloss` SavedGloss map has shifted
  column indices + `gloss_type` from a captured `gt.clone()` — not foldable. The
  dynamic-IN `placeholders`/`params`/`param_refs` boilerplate differs by
  placeholder offset (+4/+3/+2/+3) — only the trailing line is identical, needs
  generics, skip.
- **Safe-scope:** yes, but lowest priority.

## Noted but NOT numbered (below the safe-scope floor or behavior-risky)

These came up in the post-journal-Q&A audit but do not qualify as numbered
safe-scope opportunities:

- **close-gloss-overlay + restore-pos block (2 sites: journal.rs:930, keymap.rs:1124)
  and close-journal-overlay-restore (2 pure sites: journal.rs:648, :859).** Each is
  a real 5-line byte-identical block (the gloss one even shares a verbatim comment),
  but each family is only 2 sites at the floor, and the close-and-restore tails have
  sibling variants (toggle_overlay adds resnap+update_highlight; view_gloss sets
  gloss_return_pos after) that complicate a clean cut. Flag; number if a 3rd site
  appears.
- **journal_overlay show_* reveal tail (2 sites: show_page:163, show_passage_page:230).**
  7 identical lines (apply_font; ask.close; scrim/container visible; scroll-to-top;
  update_bottom_clip), but only 2 sites in one file; show_loading/show_message are
  4-line variants. Single-file `fn reveal_page(&self)` — low payoff, flag only.
- **async reader-gloss spawn+save+render tail (2 sites: visual.rs:519,
  journal.rs:698) and the cache-hit show-gloss block (2 sites: visual.rs:484,
  journal.rs:661).** Byte-identical save/render bodies (log string aside), BUT
  extracting them cleanly needs ~6 params or folding into claude_bridge
  (audit #7) which changes the callback shape — a small API change, NOT pure
  behavior-preserving extraction. The `action_gloss_with_claude`/`inner_monologue`
  siblings differ by gloss_type literal. Larger near-identical family; not a
  safe-scope byte-identical cut. Note for a future broader pass, do not number.

### From the 2026-06-25 post-citation-work audit

- **citation/id → buffer-line resolution family (~12 sites).** Thematically real
  (`work.lines.position(|l| l.id == x or (div1,div2,line_in_div) == t)` →
  `lm.work_to_buffer[work_idx]`), but NOT a safe-scope cut: the only
  byte-identical pair is concordance.rs:495 ↔ db/concordance.rs:187, and the
  latter is a `#[test]`-module helper that DELIBERATELY duplicates prod to verify
  it (its own comment: "Replicate concordance_resolve_indices logic") — folding it
  defeats the test. Every other site differs in a load-bearing token: id-vs-tuple
  key, presence/absence of the `buffer_to_work.get(bi) == Some(&Some(work_idx))`
  round-trip check, panic-`[idx]` vs `.get().unwrap_or(state.current_line)` vs
  `.get()→None` access, and a site-specific failure path
  (`None`/`current_line`/`continue`). Unifying would CHANGE behavior. Not numbered.
- **Behavior-difference flag (not a refactor — for /code-review, not this audit):**
  the round-trip validity check (`buffer_to_work.get(bi) == Some(&Some(work_idx))`)
  is PRESENT in concordance.rs:495, app/mod.rs resume (~2944) + concordance-target
  (~3057), and main.rs sync (~175), but SILENTLY ABSENT in pickers.rs:215,
  pickers.rs:985, bookmarks.rs:84, keymap.rs:320, timestamps.rs:599. Without it, an
  unmatched/default-0 work line resolves to buffer 0 instead of being rejected.
  This is a latent correctness divergence, not duplication — route to /code-review
  if it ever manifests; out of scope for maintainability numbering.

### Clip-prevention pass (2026-06-25, see specs/2026-06-25-clip-prevention-design.md)

- **Free-scroll covering math is now unified.** `scrolloff_bottom_clip_widgets`
  (scroll.rs) was a verbatim copy of `bottom_clip_height`; it now feeds
  `ui::line_yrange_rows` into the pure `bottom_clip_height`, so scroll-mode shares
  the overlays' single tested covering algorithm. The translation overlay gained a
  bottom-clip guard (`recompute_overlay_bottom_clip_box`, a box-content variant —
  its scrolled child is a widget Box, not a TextView). An overlay clip-invariant
  test (`tests/overlay_clipping.rs`) now enforces no-clip on the synopsis overlay.
- **Deliberately NOT unified (do not re-propose as safe-scope dedup):** (1) the
  MAIN reading card's `update_bottom_clip` (scroll.rs) is a PAGINATED clip
  (boundary-line `line_yrange` sums from page_top + descender-guard/column-split/
  section logic), a fundamentally different strategy from the free-scroll
  partial-row mask — merging would change behavior. (2) The gloss vs journal
  overlay `snap_value_to_line` are DIFFERENT algorithms (per-`display_rows`-row
  snap vs uniform `row_step` rounding), not duplicates. Both are behavior-changing
  to "unify", so they are out of scope for maintainability dedup.

### From the 2026-06-25 post-gloss-fixes audit

- **`work_line_for_buffer -> None => return false` guard (timestamps.rs, 6 sites).**
  Real, but splits into 2 variants: 4 bare `None => return false` and 2 that LOG
  before returning. A helper can only return `Option<usize>` (the `return false`
  is the caller's), so the bare 4 collapse to `let-else` but the 2 logged sites
  keep a custom else — partial. Lower value than #40/#41; fold opportunistically
  when touching the file, don't number separately.
- **timestamp upsert family** (`upsert_start_time`/`upsert_spoken_status`/
  `upsert_chapter`/`update_end_time`/`restore_timestamp`, queries.rs): structurally
  similar but each has a DIFFERENT SQL string + param list — folding needs
  parameterizing SQL, not a byte-identical cut. Not numbered.
- **map-commit triple** (`buffer_to_work[..]=Some; work_to_buffer[..]=..;
  db_cursor=..; matched+=1`, text_file_map): too many micro-variants (range-fill
  fallback, ParagraphAccumulate `wi+=1`, trailing `continue`) — only 2 sites
  truly identical, would need a parameterized abstraction. Not numbered.
- **`window_end = (db_cursor+WINDOW).min(n_work)` (2 sites, 57 lines apart in one
  fn) and `is_stage_row = sub_line>0` (3 trivial comparisons):** both
  behavior-preserving but marginal (tiny, low drift). Name them only if touching
  the matcher anyway.
- **picker/overlay show()/hide() tails:** the remaining `set_visible` reveal/hide
  tails reach PRIVATE fields on different structs, so a clean free-function cut is
  impossible without a trait/inherent method (out of scope). `hide_pair(&a,&b)`
  for the 5 scrim+container hides saves ~1 line/site across 4 structs — marginal,
  skip.
