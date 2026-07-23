# Keybind surface guide

How individual keybinds behave on each surface (reader main card, gloss
overlay, synopsis overlay, journal overlay, pickers). One section per bind;
add new binds as top-level `##` sections following the same shape:

- **Surfaces** — what the key does on each surface where it is live, and
  which surfaces deliberately consume it as a no-op.
- **Selection / targeting rules** — how the handler decides what to act on
  (cursor line, citation spans, bands, anchors).
- **Worked example** — a concrete lit.db-backed walk-through when the
  targeting is non-obvious.
- **Key files** — where the behavior lives in the Rust source.

The source of truth is always the Rust source (`keymap_config.rs` plus the
per-surface handlers in `keymap.rs`), never this document — update this
guide in the same change as a handler edit, like the Ctrl+/ legends.

## `\` — segment-overlay cycle

`\` walks a fixed three-press lap and does not wrap:

gloss, then journal Q&A, then back to the reading card.

The synopsis overlay is NOT a stop (dropped from the lap 2026-07-21);
inside the synopsis `\` is a consumed no-op. Open the synopsis directly
with Ctrl+h from the reader.

### Anchor semantics

The lap is anchored to the reader position where it started. Each `\`
advance closes the current overlay by restoring its saved pre-open reader
position (never the "jump to source" close), so both stops are resolved
against the same anchor line — even if you traversed elsewhere inside an
overlay with Ctrl+n / Ctrl+p before pressing `\`. Every advance also
silences any playing TTS and tears down the chat loop.

Empty stops keep their standalone fallbacks: with no gloss on the anchor
line the lap toasts "No gloss on this line" and never starts; an empty
journal stop toasts and lands back in the reader at the anchor.

### Surfaces

- **Reader** (`Action::CycleSegmentOverlays`): starts the lap at the
  gloss stop (`open_gloss_at_cursor`). The pre-open reader position is
  recorded as the lap's anchor.
- **Gloss overlay**: closes the gloss restoring the anchor, then opens
  the journal Q&A stop for the anchor line. Entering the gloss directly
  with Ctrl+g joins the same lap mid-way — the next `\` goes to the
  journal, and one more returns to the reader.
- **Journal overlay**: ends the lap — closes the journal restoring the
  anchor, back to the reading card. Opens nothing.
- **Synopsis overlay**: consumed no-op (not a stop).
- **Vocab popup open** (inside gloss/journal): `\` belongs to the popup's
  own key set and does not advance the lap.

### Which journal entry the journal stop lands on

`open_journal_scene` picks the entry in two steps:

1. **Exact passage hit.** If the anchor line falls inside some passage
   Q&A's citation span, land directly on that entry. Matching uses the
   citation's own parsed address (not the entry's stored band, which can
   drift after a litdb re-import). When several passages contain the line,
   the nearest start wins — so a narrow passage nested inside a wider one
   is preferred — with newest id as the tie-break.
2. **Scene band fallback.** Otherwise open the anchor scene's band: all
   scene-scope Q&As plus every passage Q&A filed in that chapter, ordered
   oldest first (`ORDER BY timestamp, id`), landed on the first page.

So the landing is only cursor-specific when the anchor line sits inside a
stored Q&A's cited passage; otherwise you get the chapter band starting at
its oldest entry.

### Worked example (BH chapter 10)

Anchor: cursor on "Peffer is never seen in Cook's Court now."
(BH.10.0.939), gloss opened with Ctrl+g, then `\` twice.

- Gloss stop: the Peffer/recumbent gloss (the passage covering 939).
- Journal stop: chapter 10 has three passage entries, cited at
  BH.10.0.948 (the "little out at elbows" Q&A and a "peerage" vocab Q&A)
  and BH.10.0.951 (the "subterranean" motif vocab Q&A). None covers 939,
  so step 1 misses and the scene band opens on its oldest entry: "Q:
  Explain 'a little out at elbows'" — Q&A 1 of 3. Ctrl+n steps to the
  peerage entry, then the motif entry.
- Second `\`: closes the journal and lands back in the reader on the
  Peffer line (the anchor).

### Key files

- `src/input/actions/overlay_cycle.rs` — the three advance functions and
  the anchor/restore discipline (module doc explains the lap).
- `src/input/actions/journal.rs` — `open_journal_scene` (two-step entry
  selection), `land_on_page`.
- `src/db/journal.rs` — `find_journal_page_for_line` (citation-span
  matching), `find_scene_band_pages` (band contents + order).
- `src/input/actions/gloss.rs` — `open_gloss_at_cursor`.

## `Alt+s` / `Alt+w` / `Alt+a` — journal band jumps

Journal-overlay-only direct jumps: `Alt+s` the cursor's scene band,
`Alt+w` the whole-work band, `Alt+a` the author corpus band. Jump
targets, not part of the sequential `Alt+n`/`Alt+p` scene walk. A jump
to the band already showing is a no-op.

### Surfaces

- **Journal overlay**: the three jumps above. Listed in the legend's
  MRU column (moved out of the Navigation group 2026-07-22).
- **Reader / other overlays**: not bound (Alt+a elsewhere belongs to
  other surfaces' own handlers; check `keymap.rs`).

### No rewrite-diff tint on landing

Landing on an entry normally paints the diff vs its last stored
revision (`refresh_entry_diff_highlight`, persists until Escape). Band
jumps deliberately suppress this: they browse Q&As fresh, so the three
handlers clear the tint right after `render_current`. The tint still
appears on rewrite/restore landings and revision browsing
(Ctrl+Shift+n/p).

### Key files

- `src/input/actions/journal.rs` — `nav_to_scene_band`,
  `nav_to_work_band`, `nav_to_author_band` (each clears the diff tint
  after rendering); `refresh_entry_diff_highlight`.
- `src/ui/journal_keybinds_overlay.rs` — legend rows (MRU column).

## `j` / `k` — reader bookmark steps

Reshuffled twice on 2026-07-22; final state: `j` =
`Action::NextBookmark`, `k` = `Action::PrevBookmark` (swapped with
`'`/`;`, which carry the seeking cursor steps). The speaker JUMPS that
historically sat on `j`/`k` (`JumpToNextSpeaker`/`JumpToPrevSpeaker`)
are gone from the lowercase caps; they remain on `q`/`,` and the
shifted `J`/`K`. `m` still toggles a bookmark; `.` is still the
bookmark tap.

### Surfaces

- **Reader**: as above. `h`/`t` stay the no-seek dialogue twins.
- **Overlays** (gloss/journal/synopsis/chat): `j`/`k` are each overlay's
  own block/row cursor — unchanged by this reshuffle.

### Key files

- `src/input/keymap_config.rs` — the reader table rows.
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — override
  kept in sync (it silently shadows the compiled defaults).

## `;` / `'` — reader seeking cursor steps

Final 2026-07-22 state: `;` (`semicolon`) = `Action::CursorPrevLine`,
`'` (`apostrophe`) = `Action::CursorNextDialogue` — swapped with
`k`/`j` (bookmarks). `}`/`]` (`braceright`/`bracketright`) are UNBOUND
in the reader. Shift+`;` (the `colon` glyph) still toggles playback
speed.

### Key files

- `src/input/keymap_config.rs`, keymap.json (same pair as above).
- `src/ui/keybinds_overlay.rs` — Ctrl+/ keycaps for `;`, `'`, `j`, `k`,
  and the blank `}`/`]`.

## `Ctrl+o` — dropped (was ToggleLastOverlay)

Dropped from the reader defaults 2026-07-22. `Action::ToggleLastOverlay`
(reopen the last-closed gloss/journal overlay) is no longer bound; it
survives only as an action name a keymap.json override could target.

## `+` — copy work + division (CopyWorkDivision)

Since 2026-07-22, `+` (the `plus` keysym, level 1 on the RPD `<AE01>`
cap) copies the cursor's work and division to the system clipboard via
`wl-copy`, with a confirming toast:

- plays / scened works: `ABBR d1.d2` (e.g. `MM-Amb 4.2`)
- prose chapters (`div2 == 0`): `ABBR d1`
- front matter (`div1 == 0`): bare `ABBR`

The abbrev is the LOADED edition's `work.abbrev` (not
`canonical_abbrev`). The chapter toast `+` used to trigger
(`ShowCurrentChapter`) stays on `C`; Shift+`+` (the `1` glyph) remains
`CopyWorkInfo` (abbrev + media path + whisperX JSON).

### Surfaces

- **Reader**: `Action::CopyWorkDivision` via the keymap table.
- **Gloss / journal / synopsis overlays**: their modal handlers bind
  `plus` to the same `copy_work_division` helper (same clipboard
  payload, resolved from the source cursor position). In the same
  change the gloss/synopsis `;` scene-toast arms were DROPPED (the
  running head already shows the position); `;` remains only in the
  echo overlay.

### Key files

- `src/input/keymap.rs` — `copy_work_division` (shared helper), the
  `CopyWorkDivision` dispatch arm, and the three overlay `plus` arms.
- `src/app/scene_synopsis.rs` — `current_scene_divs` (division source).

## `Ctrl+Alt+\` — add vocab word (vocab_add_card)

Opens the floating add-vocab input card (`vocab_add::open`, vim input,
starts in INSERT; `:w` adds the word, Esc cancels). The card sits above
the whole overlay chain and restores the surface it opened from
(`vocab_add_return_mode`).

### Surfaces

- **Reader**: `Action::AddVocabWord` via the keymap table.
- **Gloss / journal / synopsis overlays + chat transcript**: their modal
  handlers route the chord to the same `vocab_add::open` (the synopsis
  arm was the last one added, 2026-07-22 — the other three predate it).
  Listed in each overlay legend's Editing group.

### Key files

- `src/input/actions/vocab_add.rs` — open/close, mode save/restore.
- `src/input/keymap.rs` — `InputMode::AddVocab` owns all keys before
  mode dispatch; the per-handler chord arms.

## `Alt+b` — set end time (was Alt+i)

`Action::SetEndTime` moved from `Alt+i` to `Alt+b` 2026-07-22, pairing
with plain `b` = `Action::SetStartTime` on the same cap. `Alt+i` is now
unbound; the other `i` chords (Ctrl+i image, Ctrl+Alt+i translations,
plain `i` translation overlay) are untouched.

## `R` (vim edit mode) — dropped in the gloss/synopsis editor

Dropped 2026-07-22. Inside the gloss/synopsis in-place vim editor
(`InputMode::GlossEdit`, after `e`), `R` used to leave the editor and
open the ask-Claude rewrite prompt; it is now a consumed no-op (the
engine still emits `EditorAction::OpenRewrite`, which the handler
swallows). The rewrite stays reachable without entering the editor:

- **Gloss overlay read view**: `Ctrl+r` → `gloss::begin_rewrite`.
- **Synopsis overlay read view**: `R` → `synopsis::begin_rewrite`.
- **Journal vim editor**: unchanged — its `R`
  (`journal::vim_open_rewrite`) still opens the rewrite card, and its
  legend keeps the row.

### Key files

- `src/input/keymap.rs` — `handle_gloss_edit_key`'s `OpenRewrite` no-op
  arm; `handle_journal_edit_key` keeps the live arm.
- `src/ui/keybinds_legend.rs` — `VIM_EDIT_GROUP` (shared by the gloss
  and synopsis legends) no longer lists `R`.

## `H` (vim edit mode, visual) — toggle `<hi>` highlight

In any of the vim editors (gloss/synopsis `e`, journal `e`), `H` on a
visual selection wraps it in `<hi>..</hi>` tags — the persistent
highlight markup — and unwraps an already-highlighted span (toggle).
The engine mutates its buffer (`visual_toggle_highlight` →
`highlight::toggle`), drops back to normal mode, and the change
persists on `:w` like any other edit. Outside the editors `H` is not a
gloss/synopsis/journal overlay bind.

### Key files

- `src/input/vim/engine.rs` — `visual_toggle_highlight`;
  `src/input/vim/highlight.rs` — the tag toggle.
- `src/ui/keybinds_legend.rs` — `VIM_EDIT_GROUP` row (gloss/synopsis);
  `src/ui/journal_keybinds_overlay.rs` — the journal legend's own copy.

## `Alt+p` — karaoke / cursor-line display swap (TogglePhraseHighlight)

Two-state session axis on the reader main card only: `Alt+p` swaps
between **karaoke display** (the phrase/word sweep tracks MPV's
position; no persistent cursor-line tint) and **cursor-line display**
(a persistent tint on the cursor line; no sweep, and the `o`/`e` phrase
steps fall back to raw seeks). Session-only — never persisted — and
per work class, with different launch defaults: **prose launches in
karaoke, verse launches in cursor-line** (the sweep stays off on plays
and poetry until Alt+p opts in for the session). Flipping the axis on a
verse work leaves the prose axis untouched, and vice versa; a settings
"reset" restores each class's default.

### Surfaces

- **Reader**: `Action::TogglePhraseHighlight` via the keymap table.
  Toggling clears any in-flight phrase highlight, updates the tint
  immediately, and toasts the new state.
- **Overlays**: not bound; the axis is a main-card-only concept.

### Auto-fallback to cursor line

Karaoke can only paint when ALL of the following hold — otherwise the
cursor-line tint shows regardless of the axis's session state (never
indicator-less):

- media is loaded for the current work
- that media has phrase-level timestamp rows (`media_has_phrase_data`)
- the per-class config mode is on (`phrase_highlight_prose` /
  `phrase_highlight_verse`, both default `Phrase`)
- the cursor line itself has a timestamp

The Alt+p toast distinguishes this: switching to karaoke on media that
lacks phrase data toasts "Karaoke (no phrase audio — cursor line
kept)" rather than plain "Karaoke".

### Width is config-only

Which axis is active (karaoke vs cursor-line) is the session toggle
above; the WIDTH karaoke sweeps at (whole phrase vs whole line) is a
separate, config-only, per-class setting — `phrase_highlight_prose` /
`phrase_highlight_verse` in `config.rs` (`Phrase` or `Line`; `Off` is a
legacy stored value migrated to `Phrase` on load, see
`migrate_phrase_modes`). Alt+p does not touch these.

### Key files

- `src/input/keymap.rs` — `TogglePhraseHighlight` dispatch arm
  (`cursor_line_mode` flip, toast text).
- `src/input/phrase_highlight.rs` — `karaoke_marks_cursor`,
  `media_karaoke_capable`, `active_mode` (per-class width lookup).
- `src/config.rs` — `PhraseHighlightMode`, `migrate_phrase_modes`.
- `src/ui/keybinds_overlay.rs` — the `p` keycap's `M-p` entry and its
  describe() arm (`"karaoke"`).

## `Ctrl+↑` / `Ctrl+↓` — coupled volume nudge (mpv + rodio TTS)

One nudge moves BOTH audio channels: the shared `adjust_volume` helper
sends the relative `add volume` IPC to MPV and applies the SAME ±5 step
to the in-process rodio TTS player, so the two channels move together
and their startup offset is preserved. Contrast `Ctrl+Alt+↑/↓`
(`adjust_tts_volume`): rodio TTS alone, toasted and PERSISTED to config
(`tts_volume_offset`); MPV untouched.

### Surfaces

- **Reader**: `Action::VolumeUp` / `Action::VolumeDown` via the keymap
  table (also the `↑`/`↓` caps' `C-` entries in the Ctrl+/ overlay).
- **Gloss / synopsis / echoes overlays**: their modal handlers call the
  same `adjust_volume`. Every legend row and the Ctrl+/ describe()
  blurbs say "mpv + TTS (rodio) volume" (updated 2026-07-22).

### Key files

- `src/input/keymap.rs` — `adjust_volume`, `adjust_tts_volume`, the
  overlay `Up`/`Down` arms, and the `VolumeUp`/`VolumeDown` dispatch.
- `src/ui/{gloss,synopsis,echo}_keybinds_overlay.rs`,
  `src/ui/keybinds_overlay.rs` — the legend rows / describe() arms.
