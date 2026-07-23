# Verse Karaoke by Default + Karaoke/Cursor-Line Axis (Alt+p) — Design

Date: 2026-07-22
Status: approved
Scope: linux-lit only (phase A of the verse phrase-timestamps roadmap;
phase 0 backfilled phrase_timestamps for all 38 Arkangel plays)

## Goal

Verse works karaoke-highlight by default, exactly like prose: the phrase
sweep is the position indicator and the cursor-line tint stays off. Alt+p
becomes a two-state swap between that karaoke display and the classic
cursor-line display. The two are mutually exclusive on every work class —
karaoke sweep OR cursor line, never both.

## Current behavior (what changes)

- `phrase_highlight_verse` defaults to `off` (compiled and in both stored
  configs), so verse never karaoke-highlights; verse shows the cursor line.
- Prose already shows karaoke-only in practice (phrase tint, no cursor
  line) — that look is the target for verse.
- Alt+p (`Action::TogglePhraseHighlight`) cycles the CURRENT class's mode
  off → phrase → line and persists it to config.
- `config::load()` force-resets `show_cursor_line = true` on every launch
  (`src/config.rs:474`); `highlight.rs` reads `config.show_cursor_line`
  directly in ~6 places; the settings overlay toggles that config field.

## Design

### State model: session-only axis flag

**AMENDED 2026-07-22 (post-live-test):** the axis is per work class with
different launch defaults — `cursor_line_mode_prose: bool` launches
`false` (karaoke) and `cursor_line_mode_verse: bool` launches `true`
(cursor line; the sweep is OFF on verse until Alt+p opts in for the
session). `AppState::cursor_line_mode()` / `set_cursor_line_mode()`
select the flag by the current work's class; Alt+p and the settings
overlay flip only the displayed class's flag, and settings "reset"
restores the class default (`cursor_line_mode_default()`). Still NOT
persisted. The original single-flag design (everything launching in
karaoke) is superseded by this amendment; the rest of the spec's
references to "the axis flag" read as "the current class's flag".

The persisted per-class karaoke modes (`phrase_highlight_prose`,
`phrase_highlight_verse`) are untouched by the axis — they keep expressing
the WIDTH of the karaoke tint (`phrase` vs `line`, still editable in
config). Alt+p no longer writes them.

### Defaults and config migration

- Compiled default `phrase_highlight_verse` → `Phrase`
  (`src/config.rs:344`, currently `Off`).
- `config::load()` migrates a stored `"off"` to `"phrase"` for BOTH class
  fields. `off` ceases to exist as a persisted mode — the axis expresses
  "karaoke off" at runtime. Without this migration the stored
  `"phrase_highlight_verse": "off"` in config-dev.json/config.json would
  pin verse karaoke off forever (stored values beat compiled defaults).
- The `load()` line forcing `show_cursor_line = true` is removed along
  with all direct consumption of `config.show_cursor_line` (field retired
  from Config; serde ignores unknown JSON keys, so stale entries in
  existing configs are harmless).

### Effective visibility: one function each way

- `cursor_line_visible(s) -> bool` (new, replaces every
  `s.config.show_cursor_line` read in `highlight.rs`): true when
  `s.cursor_line_mode` is set, OR when karaoke is INCAPABLE of painting:
  - no connected media (`s.media_id.is_none()`), or
  - the playing media has no `phrase_timestamps` rows (cached per media
    id — see below), or
  - the current class's configured mode is `Off` (post-migration this
    only happens via manual config edit).
  The user always has a position indicator: works with no media, verse
  editions without backfilled phrase data, and disconnected-MPV sessions
  all fall back to the cursor line automatically.
- `active_mode(s)` (`src/input/phrase_highlight.rs:197`) gains the mirror
  gate: returns `Off` while `s.cursor_line_mode` is set (alongside the
  existing vocab-loop suppression). This keeps the phrase sweep, the
  o/e phrase step, and every other karaoke consumer consistent with the
  axis for free.

### Phrase-capability cache

`media_has_phrase_data` currently queries per call (vocab loop). Add a
small memo on AppState: `phrase_capable: Option<(i64, bool)>` keyed by
media id, consulted by `cursor_line_visible`, refreshed on loadfile /
work switch / MPV connect, cleared on disconnect. Repaint the highlight
on MPV connect/disconnect so the fallback swap is visible immediately,
not on the next nav key.

### Alt+p handler (same action name)

`Action::TogglePhraseHighlight` keeps its name — keymap.json entries and
the stowed override file remain valid. New behavior:

1. Flip `s.cursor_line_mode`.
2. Entering cursor-line mode: `clear_phrase_highlight`, repaint the
   cursor line (`update_highlight`).
3. Entering karaoke mode: clear the cursor-line tint, repaint; the next
   TimePos tick (or pending-phrase paint) restores the sweep.
4. Toast: "Karaoke" / "Cursor line"; when entering karaoke in an
   incapable context, "Karaoke (no phrase audio — cursor line kept)".
5. No `config::save` call.

`PhraseHighlightMode::cycle()` loses its caller; remove it (and its
Off-arm label use) if nothing else consumes it.

### Settings overlay

The overlay's "show cursor line" toggle drives the same axis flag
(`cursor_line_mode`) instead of the retired config field, so the overlay
and Alt+p can never disagree.

### Docs and overlays (same change)

- Ctrl+/ keybinds overlay: Alt+p describe() arm and keycap-strip text
  ("swap karaoke / cursor line highlight").
- `docs/guides/keybind-surface-guide.md`: update the Alt+p section.
- `update-cairo-keybinds-overlay` skill's three-pass cross-reference run.

## Error handling

- DB open failure while checking phrase capability → treat as incapable
  (cursor line shows; never leave the reader indicator-less).
- Vocab-sentence loop continues to force the sweep off exactly as today
  (its gate runs before the axis gate in `active_mode`).

## Testing

- Unit: truth table for `cursor_line_visible` (axis flag × media present
  × phrase rows × class mode); `load()` migration test (`"off"` →
  `Phrase` both classes, `phrase`/`line` untouched).
- Headless (cage, LIT_NO_MPV=1): launch a verse work — cursor line must
  SHOW (fallback: no MPV connected) despite karaoke default; Alt+p flips
  the toast/axis; screenshot review per UI protocol.
- Live (user, real MPV on an Arkangel play): karaoke sweep with no
  cursor line by default; Alt+p swaps to cursor line + no sweep; Alt+p
  back resumes the sweep on the next TimePos.

## Out of scope

- Any change to `phrase`-vs-`line` width semantics or their config keys.
- Persisting the axis across restarts (deliberately session-only).
- The vocab drill's prose sentence logic on verse (phase B).
