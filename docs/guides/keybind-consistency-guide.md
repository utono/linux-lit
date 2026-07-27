# Keybind consistency guide (for agents)

**What this is.** A *methodology* for keeping linux-lit's keybinds mnemonic and
consistent from the user's point of view — so a reader learns "the `r` key is
vocab, the `g` key is gloss" once and it holds everywhere. It gives agents a
mental model of the app's key→concept families, the modifier conventions, the
known inconsistencies, and a **sweep procedure** for auditing all binds before
or after a change.

**How this differs from `keybind-surface-guide.md`.** That guide is a
*descriptive per-key reference* — "what does `\` do on each surface" — updated on
request only, expected to lag the source. THIS guide is a *prescriptive design
lens*: the reasoning an agent applies when adding, moving, or auditing a bind so
the change makes the app easier to remember, not harder. The Rust source
(`keymap_config.rs` + the `handle_*_key` handlers in `keymap.rs`) is always the
truth for what a bind currently IS; this guide is the truth for what a bind
SHOULD be, mnemonically.

**When to consult it.** Every time you touch a keybind. Before proposing a new
bind, check which key already owns that concept. After any bind change, run the
sweep in the last section to confirm you didn't create a new divergence. Propose
reorganizations when the sweep surfaces one of the ranked inconsistencies below.

---

## The core idea: one key per concept

The app already leans on a **one-key-per-concept** convention. A physical cap
carries a single idea, and modifiers select the *variant* of that idea. The
user's memory load is then one association per key, not one per chord.

Concept families that are established (hold these up as the model). This list
describes the **target** scheme, which includes the approved 2026-07-23 vocab
consolidation (see the change log); until that lands, verify the current chord
against the Rust source, which is always authoritative for what a bind IS today:

- **`g` = gloss** — `Ctrl+g` toggle overlay · `Alt+g` picker · `Ctrl+Shift+g`
  last gloss · `Ctrl+Alt+g` annotation tint.
- **`j` = journal** — `Ctrl+j` work-wide journal Q&A picker · `Alt+j`
  cross-work recent-Q&A jump-back picker. (Reshuffled 2026-07-23: both journal
  pickers now live on the `j` cap; the overlay TOGGLE was dropped — the `\`
  overlay cycle opens the journal — and recent-Q&A moved here off `Ctrl+a`.)
- **`r` = vocab** — plain `r` tap the popup word · `Ctrl+r` add vocab word ·
  `Ctrl+Shift+r` vocab journal Q&A · `Alt+r` toggle per-work vocab highlight.
- **`a` = ask** — `Ctrl+a` opens an ask-a-question card on every surface that
  has one (reader-visual, gloss, journal). The most deliberately-unified
  cross-surface family in the app. (In plain READER mode `Ctrl+a` is now
  UNBOUND as of 2026-07-23 — its recent-Q&A jump-back picker moved to `Alt+j`;
  the ask-card family on the visual/gloss/journal surfaces is unaffected.)
- **`w` = rewrite** — `Ctrl+w` starts a Claude rewrite of the current gloss /
  journal Q&A / chat item, on every overlay.
- **`/` = search + legend** — plain `/` searches the current surface; `Ctrl+/`
  opens *this surface's own* keybind legend. Uniform everywhere — the
  best-behaved key in the app.
- **`\` = add-vocab (with modifiers)** — `Ctrl+Alt+\` opened the add-vocab card
  uniformly across all five surfaces (the cleanest 4-tier family). NOTE: the
  2026-07-23 vocab consolidation moves add-vocab onto `Ctrl+r` and frees the
  `\` vocab chords — see the change note below.

The user should be able to complete this sentence for any concept: **"To do
anything with X, I press the X key."** Vocab → `r`. Gloss → `g`. Journal → `j`.
Ask a question → `a`. Rewrite → `w`. Search → `/`.

---

## Modifier conventions (a strong tendency, not a law)

Inside the `g`/`j` families the modifier carries a consistent *meaning*:

- **plain** = act at / navigate the cursor (no overlay).
- **Ctrl** = toggle / open the concept's overlay.
- **Alt** = open the concept's picker.
- **Ctrl+Shift** = an alternate or history variant (last-gloss, revision browse).
- **Ctrl+Alt** = a further specialization (annotation tint, add-vocab).

Treat this as a **tendency to reach for first**, not an invariant. It is
contradicted in several places (plain `z`/`\`/`/`/`-`/`v`/`V` all open things
unmodified; `Ctrl+c` is a nav jump, not a toggle). When you add a chord, prefer
the slot the convention predicts — but do not retrofit the whole app to it.

**RPD case gotcha (applies to every `Ctrl+Shift+<letter>` bind).** On Real
Programmers Dvorak, `Ctrl+Shift+<letter>` arrives as **lowercase key name +
shift=true**, NOT the uppercase glyph (verified by the `OpenLastGloss` test at
`keymap_config.rs:607-616`). Register such a bind as `ctrl_shift("x")` AND
`ctrl_shift("X")` for robustness. Always confirm the physical cap and its shift
levels in `~/utono/rpd/xkb/.../real_prog_dvorak` before assuming a chord fits.

---

## The plain-key-nav vs. modified-key-concept split

Two of the flagship families (`g`, `j`) have their *plain* key doing navigation,
not the concept:

- plain `g` / `gg` / `G` = jump to start / end (navigation), NOT gloss.
- plain `j` = next bookmark (reader) or next block (overlays), NOT journal.

This is fine and learnable — the rule is **"plain key = navigate, modified key =
concept"** — but state it explicitly whenever you touch these keys, because it is
the single most confusing thing about the scheme for a new user. Do NOT try to
"fix" it by moving navigation off `g`/`j`; the vim-idiom `gg`/`G` and bookmark
`j` are load-bearing.

---

## Known inconsistencies (ranked — candidates for reorganization)

When a sweep or a change lands near one of these, propose a fix to the user
rather than adding to the mess. Ordered by how much they hurt memorability:

1. **`e` — four unrelated concepts, no rescuing modifier.** SeekShortForward
   (plain reader), ShowEchoTurnsBcp (Alt), ShowEchoesBcp (Ctrl), begin_edit
   (overlays). Highest priority.
2. **`c` — bare key, no modifier to disambiguate.** ToggleChapterStart (reader)
   vs. copy-id (every overlay). Two unrelated meanings on the naked cap.
3. **`s` — sync vs. save vs. curated-flag.** sync (reader/translation), save
   (chat), curated-flag (echoes); dropped in journal/gloss.
4. **`w` — three-way split on one cap.** plain `w` word-copy (reader),
   reader-table `Ctrl+w` echoes, overlay `Ctrl+w` rewrite. Input-mode keeps a
   user from hitting the wrong one, but the cap means three unrelated things.
   The rewrite meaning is the family we want; the echoes meaning is the intruder.
5. **Echoes concept is scattered across five caps** — `Ctrl+e`, `Alt+e`,
   `Alt+w`, `Ctrl+w`, and `i` (visual mode) — with no unifying key. A prime
   candidate to consolidate onto one key the way vocab consolidated onto `r`.
6. **`i` — translation overlay vs. echoes-for-selection.** Two unrelated opens.
7. **`n` / `p` — required modifier varies by surface** (reader plain, most
   overlays Ctrl, vocab-loop/echoes/pickers plain again). Real but low-stakes.
8. **`comma` — four meanings** depending on mode/modifier.
9. **Nested minor collisions** — lowercase `v` (segment-vim viewer vs. voice
   picker) inside the otherwise-clean `V` = visual-select family; `u` undo vs.
   `Ctrl+u` half-page-up.

The `a` and `-` (minus) keys are consistent *once split by modifier* (plain =
audio-transport / chat-toggle; modified = ask / vocab-jump). The one outlier
worth noting: chat's plain `a` opens the ask input instead of audio-transport.

---

## The sweep procedure (audit all binds for consistency)

Run this when explicitly asked to reorganize binds, and as a self-check after
any multi-surface keybind change.

1. **Build the key→concept map from source.** For the key(s) in scope, read
   `keymap_config.rs` (reader defaults) and every `handle_*_key` in `keymap.rs`
   (each overlay). List every chord on that cap and what it does, per surface.
   Do NOT trust legends or `keybinds.db` — they drift.
2. **Ask the memorability question per key:** can the user complete "to do
   anything with X, I press the X key"? If the cap carries two *unrelated*
   concepts (see the ranked list), flag it.
3. **Check the concept isn't scattered.** For each concept (vocab, gloss,
   journal, rewrite, ask, echoes, search), confirm all its actions live on ONE
   key. A concept spread across multiple caps (echoes today) is a scatter defect.
4. **Check modifier meaning within a family** is consistent
   (plain/Ctrl/Alt/Ctrl+Shift as above). A family that uses Alt for "open
   overlay" on one key and Ctrl on another is an inconsistency.
5. **Check cross-surface uniformity.** A chord that means the same concept on
   the reader should mean it (or be a deliberate no-op) on every overlay, never
   an unrelated action. Deliberate no-ops are fine and should stay commented.
6. **Propose, don't silently rebind.** Present the divergences you found and a
   proposed consolidation (which key each concept should own, which chords move),
   then let the user choose. Reorganizations that move muscle-memory binds are
   the user's call.
7. **When a change IS approved, update every mirror in the same change** — the
   compiled defaults, the modal handlers, `keymap.json` (stowed), and ALL
   affected `Ctrl+/` legends. Run the `update-cairo-keybinds-overlay` three-pass
   cross-reference. (This guide's companion rule is in CLAUDE.md.)

---

## Change log of consistency decisions

Record each deliberate consistency move here so future sweeps know the intent.

- **2026-07-23 — vocab consolidated onto `r`.** `Ctrl+r` = add vocab word
  (reader + gloss/journal/synopsis/chat, uniform); `Ctrl+Shift+r` = vocab
  journal Q&A; `Alt+r` = toggle vocab highlight (moved off `Alt+\`). Gloss
  rewrite moved off `Ctrl+r` to `Ctrl+w`, joining the journal/chat rewrite
  family so `Ctrl+w` = rewrite everywhere. `Ctrl+Alt+\` and `Alt+\` vocab chords
  removed. Spec:
  `docs/superpowers/specs/2026-07-23-vocab-r-key-consolidation-design.md`.

- **2026-07-23 — journal pickers consolidated onto `j`.** Both journal pickers
  now live on the `j` cap: `Ctrl+j` = work-wide journal Q&A picker (moved off
  `Alt+j`), `Alt+j` = cross-work recent-Q&A jump-back (moved off `Ctrl+a`). The
  journal-overlay TOGGLE (`ToggleJournalOverlay`, formerly `Ctrl+j`) was dropped
  from the reader binds — the `\` overlay cycle is the way in — and reader
  `Ctrl+a` is now unbound. Strengthens "j = journal"; recent-Q&A is a journal
  jump-back, so it reads more naturally on `j` than on `a`. Spec:
  `docs/superpowers/specs/2026-07-23-journal-bind-reshuffle-design.md`.

- **2026-07-26 — `-` / `_` gain a second meaning.** They still copy to the
  clipboard, unchanged. They now ALSO leave a persistent underline that
  `Return` turns into a syntax diagram of the containing sentence. This keeps
  "`-`/`_` = word selection" as one concept with two consumers (clipboard and
  diagram) rather than spending a fresh cap on the diagram.

  Decision: no new `InputMode`. The state distinguishing "words are underlined"
  already exists (`WordCycleState.collect_ranges`), so the reader stays in
  `InputMode::Reader` and `Return` is a guarded arm. A mode would have forced
  every unrelated reader bind to stop working or grow a passthrough arm.

  `Return` was unbound in reader mode, so nothing was displaced. `Escape` is
  NOT a new bind — reader `Escape` is already `EscapeReaderMode`, and the
  underline clear is a new rung at the BOTTOM of that existing priority ladder
  (below search), because an underline is the least modal of the states the
  ladder arbitrates. Spec:
  `docs/superpowers/specs/2026-07-26-word-underline-diagram-design.md`.

  **Update, same day — the target is now a syntax gloss, not a diagram.**
  `-`/`_` still underline; `Return` still opens the analysis for the
  underlined sentence — the binds are unchanged. What it opens changed: the
  full-screen Cairo diagram was retired in favor of `syntax-gloss`, a
  grammatical-analysis gloss type rendered as prose by the existing gloss
  overlay (spec: `docs/superpowers/specs/2026-07-26-syntax-gloss-design.md`).
  Overlay visual-mode `s` (`Shift+V, s`, "syntax diagram of the selected
  blocks") is NOT carried over to the syntax-gloss surface and is removed
  outright — a gloss must key to a `passage_id`, and overlay prose has no
  `line_mapping` rows to derive one from.

  **The `\` overlay cycle grows a fourth stop: gloss → journal Q&A → syntax →
  reader.** This reverses part of the 2026-07-21 decision (documented in
  `src/input/actions/overlay_cycle.rs`'s module doc) that dropped synopsis
  from the lap for being one stop too many — the count goes back up to four,
  but with syntax replacing synopsis, not restoring it. **The synopsis
  exclusion still stands**; synopsis remains a consumed no-op inside its own
  overlay's `\` key, unchanged.

  **Known inconsistency (documented, not fixed):** the three stops now handle
  an empty result differently. The gloss stop toasts "No gloss on this line"
  when nothing covers the cursor and the lap doesn't start; the journal stop
  toasts and continues to the syntax stop regardless; the syntax stop misses
  SILENTLY — no toast — when no syntax-gloss exists for the segment, and the
  lap just ends in the reader at the anchor. This is a real gap (a user
  advancing the lap can't tell "no syntax gloss here" from "the lap ended"),
  left as-is deliberately — fixing it is a separate decision, not part of
  this change. See `src/input/actions/overlay_cycle.rs` module doc.

- **2026-07-26 — the `-` cap becomes the underline family; vocab drill moves to
  `=`.** All four levels of the minus cap now feed ONE underline set
  (`WordCycleState`), which `Return` turns into a syntax gloss:

  - `-` — next word in the line, wrapping to the first after the last
  - `Shift+-` — previous word in the line, wrapping to the LAST from the first
  - `Alt+-` — collect the whole line (moved off `Shift+-` to free it)
  - `Ctrl+-` — first word of the NEXT sentence (`UnderlineNextSentence`)

  This completes the "`-` cap = text selection for the syntax gloss" concept
  started in the entry above, rather than spending fresh caps on the extra
  selection verbs. `-` and `Shift+-` are exact mirrors sharing one
  `cycle_index`, so the directions compose.

  **Scope note (a reversal worth recording).** `-` was briefly made
  SENTENCE-scoped the same day — walking only the current sentence and wrapping
  there — on the reasoning that a prose buffer line is a whole paragraph. The
  user reverted it: `-`/`Shift+-` step words within the LINE, and `Ctrl+-` is
  the only sentence-level bind on the cap. `Alt+-` stays line-scoped too, since
  collecting a full line is its whole purpose. Don't re-derive the sentence
  version.

  Displaced: the vocab-sentence drill (`JumpToNextVocab` /`JumpToPrevVocab`),
  which had been on `Ctrl+-` / `Ctrl+Shift+-`. It moved WHOLE to the `=` cap —
  `Ctrl+=` forward, `Ctrl+Shift+=` back — chosen because `=` was completely
  unbound and sits next to `-`, so the drill keeps an adjacent home. The drill's
  in-mode EXIT key followed its entry key (`handle_vocab_loop_key` now exits on
  `Ctrl+=`, not `Ctrl+-`); leaving the old exit in place would have made a
  reader bind behave like an exit inside the drill.

  RPD note: `=` and `+` are DIFFERENT physical keys here, not two levels of one
  cap (xkb `<AE06> { [ equal, 6 ] }` vs `<AE01> { [ plus, 1 ] }`). `equal` is
  therefore level-1/unshifted, which is what makes `Ctrl+=` and `Ctrl+Shift+=`
  distinct chords — the same property the `$` cap relies on. No `plus`
  alternate belongs on the drill; `plus` is a separate key already bound to
  `CopyWorkDivision`.

- **2026-07-26 — the `i` cap becomes a full dupe of `-` on every work type;
  the translation family moves to `(`.** First shipped as a prose-only mirror
  (dispatch-time remap + placeholder actions); superseded the same day when
  the verse features vacated the cap: `(` = two-column translation, `Ctrl+(` =
  page image, `Ctrl+Alt+(` = inline translation (the `(` cap was entirely
  unbound; RPD `<AE04>` puts `parenleft` on level 1, so all three chords are
  distinct). With `i` freed, the mirror became four plain table entries —
  `i`/`Shift+i`/`Alt+i`/`Ctrl+i` = `-`/`Shift+-`/`Alt+-`/`Ctrl+-` — and the
  whole `prose_i_mirror` apparatus was deleted. `Ctrl+Shift+I` (page
  calibration) stayed put: not part of the moved pair, and `-` has no fifth
  level to twin it.

- **2026-07-26 — `u` becomes a full dupe of `\`.** Plain `u` = cycle segment
  overlays, `Ctrl+u` = library picker. Home-row twin for the far-corner `\`
  cap; plain `u`/Ctrl+u were unbound (Alt+u scansion and Shift+u undo-
  timestamp keep their meanings).

- **2026-07-26 — `CursorPrevLine` renamed `CursorPrevDialogue`.** The name
  said "line" but the body always called `prev_dialogue_line`, the exact
  mirror of `CursorNextDialogue` — same rename applied to the backing fn and
  the keymap.json wire name. `;`/Up = prev, `'`/Down = next.

- **2026-07-26 — stale-name sweep.** keymap.json's echo actions predated the
  BCP/Shx split (`ShowEchoes`/`ReopenEchoes`/`ShowEchoTurns` were silently
  skipped by the loader; the compiled `*Bcp` defaults were what actually ran)
  — renamed to the `*Bcp` forms, so the JSON says what happens. Doc comments
  brought back in line with the table: `VocabJournalAsk` (Ctrl+Shift+r, not
  Ctrl+r), `AddVocabWord` (Ctrl+r, not Ctrl+Alt+\), `OpenJournalPicker`
  (Ctrl+j, not Alt+j), `OpenRecentQaPicker` (Alt+j, not Ctrl+a),
  `CycleSegmentOverlays` (lap is gloss → journal → syntax → reader, no wrap),
  `AskPassage`/`ReaderGlossChatAtCursor` (marked NO DEFAULT BIND),
  `cursor_next/prev_dialogue` (`'`/`;`, not `j`/`k`), the vocab-loop exit
  funnel (Ctrl+=), and the word-copy family comments (family-wide wording
  instead of `-`/`_`).

**Open candidates** (flagged, not yet scheduled): consolidate the echoes
concept onto one key (#5 above); disambiguate bare `e` (#1) and bare `c` (#2).
