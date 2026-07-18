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

