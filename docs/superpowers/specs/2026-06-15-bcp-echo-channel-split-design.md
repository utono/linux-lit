# Split Echoes into Two Channels: BCP vs Shakespeare — Design

**Date:** 2026-06-15
**Status:** Design, pending implementation plan
**Revision:** channel covers BCP echoes in **both** reading directions
(Shakespeare→BCP and BCP→Shakespeare); the filter is two-sided ("either side is
BCP"), not link-side only.

## Problem

`linux-lit` will gain a second class of cross-work echo involving the **Book of
Common Prayer (BCP)**, produced by the `ws-book-of-common-prayer-references`
pipeline and written into the same `echo_turns` / `echo_links` tables the
existing semantic-echo feature uses. The BCP pairing exists in **both reading
directions**:

- **Shakespeare → BCP** — reading a Shakespeare speech, the actor sees BCP
  liturgical inner-monologue. (Turn = Shakespeare work; echo = a `BCP*` work.)
- **BCP → Shakespeare** — reading a BCP edition, the user highlights a passage
  and sees Shakespeare echoes. (Turn = a `BCP*` work; echo = a Shakespeare work.)

These BCP pairings serve a different purpose from the **Shakespeare→Shakespeare**
echoes (the existing `Alt+e` / `Ctrl+e` / `Shift+Ctrl+e` feature: cross-work
dramatic echoes within Shakespeare's corpus, found by Voyage similarity;
documented in `docs/specs/2026-05-30-semantic-echo-search-design.md`). They
**must not appear in the same overlay list**.

The user wants the BCP channel (both directions) to **own** the `Alt+e` /
`Ctrl+e` / `Shift+Ctrl+e` bindings, and the existing Shakespeare→Shakespeare
behavior moved to a different key family.

## What distinguishes the two channels (no schema change)

A pair belongs to the **BCP channel** when **either side is a BCP work** — the
turn's `work_abbrev LIKE 'BCP%'` (the BCP→Shakespeare direction) **or** the
link's `echo_work_abbrev LIKE 'BCP%'` (the Shakespeare→BCP direction). The BCP
editions are registered as works `BCP1549` / `BCP1559` / `BCP1662`. A pair
belongs to the **Shakespeare channel** when **neither** side is BCP.

> ⚠️ A link-side-only filter (`echo_work_abbrev LIKE 'BCP%'`) is **insufficient**:
> it would miss every BCP→Shakespeare row, whose `echo_work_abbrev` is a
> Shakespeare work. The filter must consider the turn's `work_abbrev` too — which
> means `load_echo_links` must JOIN `echo_turns` to see the turn's work.

So the channel split is a **filter on existing data** — no migration, no new
column. The data-side guarantee is provided by the BCP repo: every BCP pair has a
`BCP*` work on exactly one side.

## Goal

1. Two independent echo channels, each with its own overlay session and turns
   picker, never intermixing rows; the BCP channel covers both reading directions.
2. The **BCP channel** is bound to `Alt+e` (search/show), `Ctrl+e` (turns
   picker), `Shift+Ctrl+e` (reopen) — the current `e`-family.
3. The **Shakespeare→Shakespeare channel** keeps all of its current behavior but
   moves to a new, currently-unbound key family (chosen during implementation
   after auditing `keymap.rs` — see Open items).
4. **BCP works are openable in the reader** so the BCP→Shakespeare direction is
   usable: open a `BCP*` work, navigate/highlight a passage, and the BCP-channel
   overlay shows that passage's Shakespeare echoes. (See "BCP-reading
   interaction" below — this is the part beyond the channel filter.)

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

- The channel predicate references **both** the turn's `work_abbrev` (alias `t`)
  and the link's `echo_work_abbrev` (alias `l`):
  - `Bcp`: `(t.work_abbrev LIKE 'BCP%' OR l.echo_work_abbrev LIKE 'BCP%')`
  - `Shakespeare`: `(t.work_abbrev NOT LIKE 'BCP%' AND l.echo_work_abbrev NOT LIKE 'BCP%')`
  Both columns are `NOT NULL`, so `LIKE` is well-defined.
- `load_echo_links(conn, turn_id, channel)` must therefore **JOIN `echo_turns t`
  ON `t.id = l.turn_id`** (it currently selects from `echo_links` alone) so the
  turn's `work_abbrev` is available to the predicate. Ordering
  (`curated DESC, rank ASC`) is unchanged.
- `list_echo_turns_for_work(conn, work_abbrev, channel)` already JOINs
  `echo_turns t` + `echo_links l`, so it applies the same two-sided predicate
  directly.
- `EchoChannel::sql_predicate()` returns the two-sided fragment above (a fixed
  `&'static str` referencing `t.`/`l.`), so both queries share one definition.
- The echo session in `AppState` records which channel it was opened for, so
  `reopen` and `alt+i` round-trips stay within the same channel.
- Each overlay session is per-channel; opening the other channel replaces the
  session (same lifecycle as today, just keyed by channel).

This keeps one rendering/navigation implementation and differentiates only at
the data-load boundary and the keybind that opens it.

### BCP-reading interaction (the BCP→Shakespeare direction)

For Shakespeare→BCP, the actor is already reading a Shakespeare work, so the
existing cursor-turn / visual-selection flow works unchanged once the channel
filter is in place. The **inverse** direction needs the reader to be *in a BCP
work*:

- A `BCP*` work must be **openable in the reader** like any other work (it is a
  normal `works` + `line_mapping` entry produced by the BCP repo, so the existing
  work-open path should already handle it — verify, don't assume).
- With a BCP work open, the same `Alt+e` (cursor turn) / visual-selection echo
  trigger resolves the highlighted BCP passage to its `echo_turns` row and loads
  its BCP-channel echoes (Shakespeare works). Because the channel filter is
  two-sided, a BCP turn's Shakespeare echoes correctly count as BCP-channel.
- `Enter` on such an echo opens the Shakespeare work at the echoed line (the
  existing jump path, which already handles arbitrary `echo_work_abbrev`).

If the existing cursor-turn resolution assumes a `speaker` (BCP lines have
`speaker = NULL`), that path may need a small adjustment to resolve a BCP "turn"
by line/selection rather than by speaker block — flagged as an open item.

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

- Producing the BCP echo data (both directions) — that is the
  `ws-book-of-common-prayer-references` pipeline (ingest → embed → judge
  `--direction shx2bcp|bcp2shx` into `echo_turns`/`echo_links`). This spec
  consumes that data.
- Any lit.db schema change — the channel is a two-sided data filter on
  `work_abbrev` / `echo_work_abbrev`.
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
- **BCP cursor-turn resolution.** The existing cursor-turn logic groups lines by
  `speaker`; BCP lines have `speaker = NULL`. Resolving the highlighted BCP
  passage to its `echo_turns` row may need a by-line/by-selection path rather than
  a speaker-block path. Confirm against the actual turn-resolution code and adjust
  if needed (the BCP repo keys BCP `echo_turns` by `(work_abbrev, div1, div2,
  start_line, end_line)`).
- **Does the existing work-open path already open `BCP*` works?** They are normal
  `works`/`line_mapping` entries, so it likely does — verify, and handle any
  `bible_book`/`speaker NULL` assumptions in the reader.
