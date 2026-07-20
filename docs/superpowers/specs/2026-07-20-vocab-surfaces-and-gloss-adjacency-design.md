# Vocab on every surface + gloss neighbor-context — design

Date: 2026-07-20 (US Central). Status: approved by user (conversation),
pending spec review.

## Goals

1. Reader-gloss generation stops recycling metaphors/images/rhetorical
   devices already used by glosses of neighboring source text (observed:
   gloss 21807 and 21810 both build on "fishes"/"angling" for adjacent
   TGV 1.2 passages).
2. In two-column layout, the `rr` vocab popup becomes a compact float
   instead of a full-column overlay; Escape closes it everywhere.
3. The gloss overlay, journal overlay, and chat panel highlight vocab
   words (lit.db `vocab_words`) and support the `rr` popup toggle and
   the `Ctrl+Alt+\` add-vocab card, like the main card.
4. While the chat panel is open, a small horizontal rule marks whichever
   surface (chat panel or main card) has key focus.

Approved decisions: compact float over the non-cursor column; displaced
binds keep their mnemonic under Ctrl (with `Ctrl+w` where `Ctrl+Shift+r`
is taken); focus rule shows above whichever surface has focus; the
previously uncommitted journal/prompt/test work was committed first
(`fc9b4be0`, `aecfd68e`, `72533206`).

## Background (verified against source)

- `r` = `Action::VocabPopupTap` (`keymap_config.rs:308`); the `rr` chord
  is recognized in `handle_key_inner` (`keymap.rs:308-341`) via
  `ChordState::PendingR`. Main-card `R` is unbound; `Ctrl+r` =
  `VocabJournalAsk`.
- Two-column popup placement: `position_vocab_popup`
  (`src/app/vocab_popup.rs:99-122`) calls `layout::column_float_rect`
  and `place_float` (`src/ui/vocab_popup.rs:122-131`), which sizes the
  popup to the full column (width_request/height_request = column ×
  card height, CSS class `vocab-popup-float`). This is the "definitions
  fill a column" behavior being replaced. Escape does NOT close the
  popup today (`escape_reader_mode` has no popup branch).
- Highlighting is main-card only: `vocab_tag` on the main buffer
  (`app/mod.rs:614`), matches built by `build_vocab_matches`
  (`app/mod.rs:4436`), applied by `apply_vocab_highlighting`
  (`app/mod.rs:4535`), per-work flag `works.vocab_highlight`.
- Add-vocab (`Ctrl+Alt+\` = `Action::AddVocabWord`,
  `keymap_config.rs:317`) is Reader-mode only and REUSES the gloss
  overlay widget as its input card (`src/input/actions/vocab_add.rs`),
  so it cannot open over the gloss/journal overlays today. Definition
  ladder: `vocab_lookup::lookup_local` (wn, then dict/gcide), then
  Claude fallback; insert via `insert_vocab_word` (`queries.rs:1129`).
- Overlay key handling: `handle_gloss_key` (`keymap.rs:2088`; `r` is a
  consumed no-op, `R` = rewrite gloss), `handle_journal_key`
  (`keymap.rs:1703`; `r` = ask, `R` = rewrite chooser),
  `handle_chat_transcript_key` (`keymap.rs:1501`; `r`/`R` = re-gloss in
  Gloss view, ask/rewrite in Journal view; `a` already means ask).
  `Ctrl+r` is free in all three handlers; `Ctrl+Shift+r` is TAKEN in
  gloss and journal (revision browse/restore).
- Both overlays render through a `GtkTextView`+`TextBuffer` with
  existing TextTags (search/diff/font/audio) — vocab tags fit there.
  Chat panel rows are plain `gtk4::Label`s built from widget-specs
  (`chat_panel.rs:425`), so highlighting there must use Pango markup.
- Vocab popup attaches to `corpus_search_popup.overlay`
  (`app/mod.rs:1746`), which sits above the gloss/journal overlay chain
  (`gloss_overlay.attach(&gamepad_overlay.overlay)` at 1573,
  `journal_overlay.attach(&gloss_overlay.overlay)` at 1578) — the
  popup can already paint over the overlays; verify at implementation.
- Gloss generation: system prompt `gloss.reader-gloss` v7 (active row in
  `api_prompts`; master in `~/utono/claude-api-prompts/prompts/`), user
  message from `build_user_message` (`src/gloss.rs:699`), request paths
  `request_reader_gloss` (`chat.rs:1708`), `add_gloss` question path
  (`gloss.rs:1436`), and the edit path. v7's lede-verb examples
  literally include "fishes for, angles for" — a direct cause of the
  recycled conceit. Citations encode adjacency:
  `abbrev.div1.div2.line_in_div`; `find_glossed_passages`
  (`queries.rs:2015`) already returns a work's glossed passages in
  reading order with the trailing-line-number CAST idiom.

## Workstream A — gloss neighbor-context (no recycled devices)

Prompt (v8 of `gloss.reader-gloss`, plus matching guidance in
`gloss.reader-gloss-question` and `gloss.reader-gloss-edit`):

- Remove "fishes for, angles for" from the lede-verb example list.
- Add a rule: when a "Neighboring glosses" block is present in the user
  message, do not reuse its characterizing verbs, governing metaphors,
  images, or other rhetorical devices; choose fresh, equally precise
  language.
- Flow: edit masters in `~/utono/claude-api-prompts/prompts/`, insert
  new `api_prompts` rows (active flips to the new version), update the
  Rust `FALLBACK` consts minimally (add the neighbor rule; the fallback
  deliberately stays terse). No hot reload — restart picks it up.

Code (linux-lit):

- New query `find_neighbor_glosses(conn, canonical_abbrev, div1, div2,
  start_line, end_line, gloss_type, n)` in `src/db/queries.rs`: the `n`
  nearest preceding and `n` nearest following glossed passages in the
  same scene, ordered by trailing line number of `start_citation`
  (reuse the CAST idiom). Default n = 2 per side.
- `build_user_message` gains an optional neighbors block appended as:
  `--- Neighboring glosses (adjacent passages; do NOT recycle their
  metaphors, images, verbs, or rhetorical devices):` followed by each
  neighbor's citation span + gloss text.
- All three reader-gloss request paths pass neighbors (base, question,
  edit). Other gloss types unchanged.
- Practice note (outside the app): when glosses are written or edited
  manually in a Claude session, consult adjacent glosses first — saved
  as an agent memory, not code.

## Workstream B — compact two-column popup + Escape everywhere

- `place_float` becomes a compact card: keep `column_float_rect` only
  for the target column's x/width; the popup takes its natural height
  (cap: card height minus margins, scrollable inside if over), width =
  min(natural, column width − 2×12px inset), halign centered in the
  column rect, valign Center. Drop the full-size width/height requests;
  restyle `vocab-popup-float` to match the single-column strip's
  readable colors (theme ink on card background — fixes the
  white-on-gray contrast in the screenshot).
- Escape precedence: a visible vocab popup closes FIRST. Add a branch
  at the top of `escape_reader_mode` and in each overlay/chat Escape
  arm (before rewrite-diff/search/close handling). Closing sets
  `auto = false` like the `rr` off-toggle.

## Workstream C — vocab on gloss overlay, journal overlay, chat panel

Highlighting:

- Extract the tokenizer core of `build_vocab_matches` into a
  buffer-agnostic helper (`vocab_matches_in_text(lines, words) ->
  Vec<VocabMatch>`-shaped) reused by the main card and both overlays.
- Gloss + journal overlays: register a `vocab_tag` on each overlay
  buffer at construction; after every populate/font/recolor pass, when
  the current work's `vocab_highlight` flag is on, apply the tag at
  char offsets (same pattern as `apply_vocab_highlighting`). Skip
  `<speaker>` header lines and citation/label rows.
- Chat panel: highlight by wrapping matches in a Pango `<span>` during
  the spec→Label step (`append_spec_label` path). Escape existing
  markup first; GlossAnswer rows already carry markup — apply matching
  to text segments only.

Popup (`rr`) in the three surfaces:

- `r` mirrors the main card: when the popup is visible, tap = next
  word; every tap arms `ChordState::PendingR`; `rr` toggles the popup.
  `R` becomes unbound (matches main card). Chord recognition moves to
  a small shared helper reachable from the gloss/journal/chat handlers
  (the current chord check lives ahead of Reader-mode dispatch only).
- Word scope: the current block (gloss/journal overlays — same block
  the TTS/copy-id actions target) or the selected transcript row
  (chat). No matches → same "no vocab words here" toast as the main
  card.
- Placement: overlays and chat are single-column surfaces — the popup
  is the workstream-B compact card anchored to the lower-right inside
  the surface's card, 12px inset, natural height. The popup already
  attaches above the overlay chain; verify z-order over the journal
  overlay and chat panel during implementation.

Add-vocab everywhere:

- New dedicated compact card widget (`src/ui/vocab_add_card.rs`,
  ≤560×140, no scrim, same vim engine wiring as today) attached to
  `corpus_search_popup.overlay`, replacing the gloss-overlay reuse in
  `vocab_add.rs`. This removes the conflict that blocks opening it
  from the gloss overlay.
- `Ctrl+Alt+\` handled in Reader (both layouts — already works in
  2-col), gloss overlay, journal overlay, and chat (transcript)
  handlers. `InputMode::AddVocab` remembers the prior mode and
  restores it on close (today it hard-returns to Reader).
- Submit flow unchanged: normalize, local wn/dict lookup, Claude
  fallback, `insert_vocab_word` (word + definition into lit.db —
  neither may pre-exist; the existing `VocabInsertOutcome` dedup
  stands). After insert, refresh matches on ALL visible surfaces
  (main card + any open overlay + chat panel).

## Workstream D — keybind moves (displaced by `r`/`R`)

| Surface         | Old                    | New      |
|-----------------|------------------------|----------|
| Gloss overlay   | `R` rewrite gloss      | `Ctrl+r` |
| Journal overlay | `r` ask new question   | `Ctrl+r` |
| Journal overlay | `R` rewrite chooser    | `Ctrl+w` |
| Chat transcript | `r` re-gloss / ask     | `Ctrl+r` |
| Chat transcript | `R` re-gloss / rewrite | `Ctrl+w` |

- `Ctrl+Shift+r` (revision browse/restore) keeps its meaning in both
  overlays — hence `Ctrl+w` for the two rewrite-target slots. Chat's
  ask also stays on `a`.
- Journal term-filter intercept (`r`/`space`/`a`/`backslash` →
  clear-filter toast) keeps `r` in its list; the vocab popup is
  reachable after clearing the filter.
- Same-change legend updates (required): `gloss_keybinds_overlay.rs`,
  `journal_keybinds_overlay.rs`, `chat_keybinds_overlay.rs` GROUPS;
  main Ctrl+/ overlay additionally gains the missing `Ctrl+Alt+\`
  add-vocab entry (run the update-cairo-keybinds-overlay three-pass
  cross-reference). `keymap.json` (tty-dotfiles) is unaffected — these
  are modal-handler keys, not keymap_config bindings — but verify no
  user JSON binding shadows `r` behavior.

## Workstream E — chat-panel focus rule

- A ~24×2px horizontal rule (own CSS class, ink-colored, alpha set in
  `theme.rs generate_css`; also wired at startup — remember overlay
  colors set at startup are NOT applied by `apply_theme_to_state`).
- Two instances: one in a new header slot at the top of
  `ChatPanel.container` (the panel has no header today), one centered
  in the main card's `top_spacer` (between the running-head labels).
- Visibility: only while the chat panel is open; the panel's rule shows
  when `input_mode` is `ChatTranscript`/`ChatPrompt`, the card's rule
  when `Reader`. Updated from `focus_reader`/`focus_transcript`/
  `focus_prompt` and panel open/close/regate (tick-deferred regate rule
  applies).

## Testing & acceptance

- Unit: tokenizer extraction (same matches as before on the main card),
  `find_neighbor_glosses` ordering/limits (same-scene only, trailing
  line-number order), user-message neighbor block formatting.
- Headless e2e (cage/grim, `./scripts/e2e-env.sh`): two-column `rr`
  shows a compact float (geometry: popup rect strictly inside the
  non-cursor column, height < column height), Escape closes it in
  reader and in each overlay; overlays show vocab tags when the flag is
  on; chat focus rule flips with Tab. Pixel-verify contrast on
  screenshots (no by-eye margin calls); clipping acceptance per
  clip-prevention.md.
- Prompt change: generate a fresh gloss adjacent to an existing one on
  a test passage and confirm the neighbor block is present in the
  logged/sent user message (log line), not by eyeballing model output
  alone.
- Final GL-renderer eyeball handed to the user (cage is software
  rendering; contrast/margins need the real renderer).

## Risks / notes

- Chat Pango markup: escaping regressions are the main risk — matches
  must be applied to text runs only, never inside existing spans.
- Popup z-order above the journal overlay is expected from the attach
  chain but must be verified; if wrong, re-attach the popup higher.
- The gloss/journal buffers are repopulated on nav/edit — vocab tag
  application must hook every populate path or highlights silently
  vanish (mirror how search/font tags are reapplied).
- api_prompts v8 rollout needs an app restart; fallbacks stay terse.
- `docs/troubleshooting/clip-prevention.md` must gain any new failure
  mode found while sizing the compact float (standing project rule).
