# Echo Jump Navigation: Enter Opens Echo's Work, alt+i Returns

**Date:** 2026-05-31

## Problem

In the echoes overlay (press `I`), Enter currently copies the selected echo to
the clipboard. The reader cannot navigate from an echo to the work it belongs
to, nor jump back-and-forth between the turn's work and each echo's work.

## Solution

- **Enter** on a selected echo opens that echo's work and places the cursor on
  the echoed line.
- **alt+i** jumps back to the original turn's work and line, then
  reopens the echoes overlay with the same links — so the reader can pick
  another echo and jump again. Hub-and-spoke navigation between the turn's work
  and each echo's work.
- The echo session (turn + links + origin) is held in `AppState` and survives
  work-switches, Esc, and repeated alt+i round-trips. It is replaced only when
  the reader presses `I` on a new line.

## Schema / Data Changes

The echo's line identity is needed to land on the exact echoed line.

- **`find_similar_passages`**: project `start_line` from `passage_embeddings`
  into `EchoCandidate` (add `start_line: i64`). The column already exists; only
  the SELECT list and the struct change.
- **`echo_links` table**: add column `echo_start_line INTEGER`. Migration in
  `ensure_echo_tables`: `ALTER TABLE echo_links ADD COLUMN echo_start_line
  INTEGER` wrapped to ignore the "duplicate column" error if already present.
- **`StoredEchoLink`**: add `echo_start_line: i64`.
- **`insert_echo_links`** / `load_echo_links`: carry `echo_start_line`.

To resolve the echoed line at jump time:
`line_id_for_location(conn, work_abbrev, div1, div2, line_in_div) -> Option<i64>`
queries `line_mapping.id`. (`echo_start_line` is the line_in_div within the
echo's scene.)

## Enter Behavior (changed)

On Enter with a selected echo:

1. Build/refresh the `EchoSession` (see State) and store it in `AppState`,
   capturing the origin work and the turn's first-line `line_mapping.id`.
2. Resolve the echo's `line_mapping.id` via `line_id_for_location`.
3. If unresolved, fall back to a toast and do nothing.
4. Hide the overlay; load the echo's work and place the cursor on the echoed
   line, reusing the concordance cross-work load path
   (`save_position`, `skip_mpv_discovery`, `display_work_at_with_prepared`).
5. Enter Reader mode.

Copy moves off Enter to **`c`** in the overlay (copy still available).

## alt+i Behavior (new `Action::ReopenEchoes`)

`alt+i` in Reader mode:

1. If `AppState.echo_session` is `None`: no-op (optional toast "no echo
   session").
2. Otherwise: jump back to `origin_work` at `origin_line_id` (same cross-work
   load path), then reopen the echoes overlay restored from the session
   (`links`, `selected`, `titles`, `source_doc`), entering `EchoesOverlay` mode.

If already on the origin work, skip the reload and just reopen the overlay.

## Session Lifetime

`AppState.echo_session: Option<EchoSession>`:

- **Set / replaced** when the reader presses `I` (a fresh search or cache hit).
- **Mutated in place** by `s` (curate) and `R` (refresh) — the session's `links`
  and `selected` update.
- **Read** by Enter (to record origin before jumping) and alt+i (to restore).
- **Kept** on Esc and across work-switches. Only the next `I` replaces it.

## State

```rust
pub struct EchoSession {
    pub turn_key: EchoTurnKey,
    pub turn_id: Option<i64>,
    pub links: Vec<StoredEchoLink>,
    pub selected: usize,
    pub titles: HashMap<String, String>,
    pub source_doc: String,
    pub origin_work: String,    // work where I was pressed
    pub origin_line_id: i64,    // line_mapping.id of the turn's first line
}
```

**Field strategy (to minimize churn):** keep the existing loose
`echo_overlay_*` fields as the *active* overlay state that `render_echoes` and
the key handlers already read/write. Add `echo_session: Option<EchoSession>` as
a sticky snapshot used only for restore. The session is written whenever the
active overlay is (re)built or mutated (`I`, `s`, `R`), capturing the origin on
the first build. alt+i restores the loose fields from the session, then renders.
This avoids rewriting every handler to read through `EchoSession`.

## Keys in the Echoes Overlay (updated)

- **Ctrl+n / Ctrl+p** — select echo
- **Enter** — jump to the selected echo's work, cursor on the echoed line
- **c** — copy selected echo to clipboard (was Enter)
- **s** — toggle curated
- **R** — refresh
- **j / k / g / G** — scroll
- **Esc** — close overlay, keep session (alt+i still works)

## Reuse

- Concordance cross-work load: `save_position`, `skip_mpv_discovery`,
  `display_work_at_with_prepared`, the `glib::spawn_future_local` +
  `spawn_blocking(load_work + prepare_text_for_display)` pattern from
  `concordance_jump_to_current`.
- Existing echo render (`render_echoes`, `show_echoes`) and overlay.
- `voyage::embed_query`, `find_similar_passages`, `load_work_titles`.

## New Code

- `src/db/queries.rs`: `start_line` in `EchoCandidate` + SELECT; `echo_start_line`
  column + migration; `StoredEchoLink.echo_start_line`; `line_id_for_location`.
- `src/input/actions/echoes.rs`: introduce `EchoSession`; rework
  `show_echoes_for_cursor_line` to populate the session and record origin;
  `jump_to_selected_echo` (Enter); `reopen_echoes` (alt+i); update
  `toggle_curated`, `refresh_echoes`, `copy_selected_echo`, `move_echo_selection`
  to read/write the session.
- `src/app.rs`: add `echo_session: Option<EchoSession>` (init `None`); keep the
  existing loose `echo_overlay_*` fields as the active overlay state.
- `src/input/keymap.rs`: in `handle_echoes_overlay_key`, Enter → jump,
  add `c` → copy; reader-mode dispatch for `ReopenEchoes`.
- `src/input/actions/mod.rs`: `Action::ReopenEchoes` (+ category + name).
- `src/input/keymap_config.rs`: bind `(KeyCombo::alt("i"), Action::ReopenEchoes)`
  (replacing the old `alt+i` = SetEndTime), and move SetEndTime to
  `(KeyCombo::alt("u"), Action::SetEndTime)`.
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`: change the
  `{"key":"i","alt":true}` entry from SetEndTime to ReopenEchoes, and add
  `{"key":"u","alt":true,"action":"SetEndTime"}`.

## Migration Note

`alt+i` was bound to SetEndTime. This spec reassigns `alt+i` → ReopenEchoes and
moves SetEndTime to `alt+u` (which is free; plain `u` = SetStartTime,
`ctrl+u` = PageForward). Both the compiled defaults and the user's keymap.json
are updated.

## Out of Scope

- No back-stack of multiple echo sessions (one sticky session).
- No MPV sync into the echo's work beyond the existing cross-work behavior.
- No change to how `I` finds/searches echoes.

## Risks

- **Unresolvable echo line:** if `line_id_for_location` returns `None` (e.g. the
  echo's line numbers changed after a corpus rebuild), Enter shows a toast and
  stays in the overlay. The echo is still copyable via `c`.
- **Origin line drift:** `origin_line_id` is a `line_mapping.id` captured at `I`
  time; it is stable. If the origin work was edited between sessions, the id may
  point to shifted content — acceptable, same risk as bookmarks.
- **Session + curate/refresh consistency:** `s` and `R` must update the
  session's `links`/`selected` in place so a later alt+i restores the current
  state, not a stale copy.
