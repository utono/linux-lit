# Split Echoes into Two Channels: BCP vs Shakespeare — Design

**Date:** 2026-06-15
**Status:** Design, pending implementation plan

## Problem

`linux-lit` will gain a second class of cross-work echo: **Book of Common Prayer
(BCP) inner-monologue** echoes for Shakespeare speaker-turns, produced by the
`ws-book-of-common-prayer-references` pipeline and written into the same
`echo_links` table the existing semantic-echo feature uses.

These two kinds of echo serve different purposes and **must not appear in the
same overlay list**:

- **Shakespeare→Shakespeare** — the existing `Alt+e` / `Ctrl+e` /
  `Shift+Ctrl+e` feature: cross-work dramatic echoes within Shakespeare's corpus
  (one character performing the same dramatic action as another, found by Voyage
  similarity). Documented in `docs/specs/2026-05-30-semantic-echo-search-design.md`.
- **BCP→Shakespeare** — new: liturgical inner-monologue an actor can play under
  a Shakespeare speech.

The user wants the BCP channel to **own** the `Alt+e` / `Ctrl+e` /
`Shift+Ctrl+e` bindings, and the existing Shakespeare→Shakespeare behavior moved
to a different key family.

## What distinguishes the two channels (no schema change)

A BCP echo row is exactly an `echo_links` row whose `echo_work_abbrev` matches
`'BCP%'` (the BCP editions are registered as works `BCP1549` / `BCP1559` /
`BCP1662`). A Shakespeare echo row is any `echo_links` row whose
`echo_work_abbrev` does **not** match `'BCP%'`.

So the channel split is a **filter on existing data** — no migration, no new
column. The data-side guarantee is provided by the BCP repo: every row it writes
carries `echo_work_abbrev LIKE 'BCP%'`.

## Goal

1. Two independent echo channels, each with its own overlay session and turns
   picker, never intermixing rows.
2. The **BCP channel** is bound to `Alt+e` (search/show), `Ctrl+e` (turns
   picker), `Shift+Ctrl+e` (reopen) — the current `e`-family.
3. The **Shakespeare→Shakespeare channel** keeps all of its current behavior but
   moves to a new, currently-unbound key family (chosen during implementation
   after auditing `keymap.rs` — see Open items).

## Current implementation (verified)

- **Actions** (`src/input/keymap.rs`): `ShowEchoes` →
  `echoes::show_echoes_for_cursor_line`, `ReopenEchoes` → `echoes::reopen_echoes`,
  `ShowEchoTurns` → `echoes::open_echo_turns_picker` (keymap.rs:1704–1706). These
  are the three entry points to rebind/duplicate.
- **Overlay modes** (`src/app.rs` `InputMode`): `EchoesOverlay`, `EchoPicker`,
  `EchoTurnsPicker`, `EchoLinePicker`, `EchoKeybindsOverlay`. Dispatched in
  `keymap.rs` (lines ~119–124).
- **Echo state** lives in `AppState` (`echo_session`, `echo_overlay_links`,
  `echo_overlay_index`, `echo_picker`, `echo_turns_picker`, …). The session
  survives work-switches and `alt+i` round-trips (per
  `docs/specs/2026-05-31-echo-jump-navigation-design.md`).
- **The single load query** is `load_echo_links(conn, turn_id)`
  (`src/db/queries.rs:1794`): `... FROM echo_links WHERE turn_id = ?1 ORDER BY
  curated DESC, rank ASC`. This is the one place a channel filter must be
  applied. The echo *search* path (`build_embeddings` / `find_similar_passages`)
  already targets only Shakespeare vectors; the BCP candidates are produced
  offline by the BCP repo, so the live search path is unaffected — the split is
  almost entirely about **which cached rows each overlay reads and renders**.
- The turns picker (`open_echo_turns_picker`) lists turns that *have* cached
  echoes; it must list per-channel (turns with BCP echoes vs. turns with
  Shakespeare echoes).

## Design

### Channel as a parameter, not a duplicated subsystem

Introduce a `EchoChannel { Bcp, Shakespeare }` enum and thread it through the
echo action/state/query path, rather than copy-pasting the overlay code:

- `load_echo_links(conn, turn_id, channel)` adds a `WHERE` clause:
  - `Bcp`: `AND echo_work_abbrev LIKE 'BCP%'`
  - `Shakespeare`: `AND echo_work_abbrev NOT LIKE 'BCP%'`
  Ordering (`curated DESC, rank ASC`) is unchanged. A NULL-safe `LIKE` is fine
  here because `echo_work_abbrev` is `NOT NULL`.
- The echo session in `AppState` records which channel it was opened for, so
  `reopen` and `alt+i` round-trips stay within the same channel.
- The turns picker query is filtered the same way (turns that have ≥1 row in the
  requested channel).
- Each overlay session is per-channel; opening the other channel replaces the
  session (same lifecycle as today, just keyed by channel).

This keeps one rendering/navigation implementation and differentiates only at
the data-load boundary and the keybind that opens it.

### Keybindings

- **BCP channel** ← `Alt+e` (show/search for cursor turn), `Ctrl+e` (turns
  picker), `Shift+Ctrl+e` (reopen). These reuse the existing `ShowEchoes` /
  `ShowEchoTurns` / `ReopenEchoes` actions, now invoked with
  `channel = Bcp`.
- **Shakespeare→Shakespeare channel** ← a new key family (the same three
  gestures: show, turns picker, reopen), invoked with `channel = Shakespeare`.
  The concrete keys are chosen during implementation after auditing free keys in
  `keymap.rs` — see Open items.
- The echo keybinds overlay (`EchoKeybindsOverlay`, the help card in the
  screenshot) is updated to show both families and which channel each drives.

### Behavior preserved on both channels

`Enter` opens the echo's work at its line (BCP edition rite, or the other
Shakespeare play), `alt+i` returns, `s`/reorder/`A`-add/refresh and audio
controls all work as today — the only differences are which rows load and which
key opens the channel.

## Out of scope

- Producing the BCP echo data — that is the `ws-book-of-common-prayer-references`
  pipeline (ingest → embed → judge into `echo_links`). This spec consumes that
  data.
- Any lit.db schema change — the channel is a data filter on `echo_work_abbrev`.
- Changing the Shakespeare→Shakespeare *search* algorithm or the BCP *discovery*
  algorithm.

## Open items for the implementation plan

- **New keys for the Shakespeare→Shakespeare channel.** Audit `keymap.rs` for an
  unbound family that mirrors the three `e` gestures (candidates discussed:
  `s`-family for "Shakespeare", `w`-family for "cross-work"). Pick one that does
  not collide with existing reader-mode or overlay bindings.
- **Whether the echo session needs to remember both channels simultaneously** or
  one-at-a-time replacement suffices (default: one active session, replaced on
  channel switch — simplest, matches today's single-session model).
- **Empty-channel UX** — what each overlay shows when a turn has echoes in the
  other channel but none in this one (default: the normal "no echoes" state).
