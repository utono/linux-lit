# Spacebar: play from cursor line start (replicate the `a` bind)

Date: 2026-06-09

## Problem

Today the spacebar is a global MPV **play/pause toggle** (`TogglePause`),
handled in an early block in `handle_key` (`src/input/keymap.rs:64-81`) that
fires for *every* input mode before mode dispatch. The user wants space to
instead **replicate each surface's `a` bind** — begin playback from the cursor
line's start time — in the main card and every overlay that has an `a`/play
binding. The gloss overlay keeps its current space behavior.

## Goal

On every surface, `Space` should do exactly what that surface's `a` key does:

- **Main card (Reader)** — `PlayCurrentLine`: seek MPV to the cursor line's
  start (minus `SEEK_PREROLL`) and resume. Same as `a`.
- **Translation overlay** — `play_current_line` (its `a` arm,
  `keymap.rs:881`). Same as the main card.
- **Echoes overlay** — `play_selected_echo` (its `a` arm, `keymap.rs:1189`):
  play the currently selected echo. This is the echoes surface's own notion of
  "play", so space mirrors it.
- **Gloss overlay** — UNCHANGED: space stays `read_current_block`
  (`keymap.rs:800`), TTS-reading the cursor block. The user explicitly asked to
  keep gloss space as-is.
- **Synopsis overlay, pickers, settings, visual, search** — no `a`/play
  binding, so space stays as it is today on those surfaces (swallowed /
  falls through to editable input as appropriate).

When the play attempt is a no-op because the cursor line has no timestamp
(`play_current_line` returns `false`), show a brief bottom-center toast
("No timestamp on this line") instead of silently doing nothing.

## Design

### 1. Narrow the global space block to Reader mode

The current early block (`keymap.rs:64-81`) runs for all modes and sends
`TogglePause`. Because each overlay now needs its *own* notion of "play", this
global behavior is wrong for overlays. Change it to:

- Keep the existing guards: editable-widget focus (Entry, editable TextView,
  or `InputMode::Search`) means space types a literal space — return `false`
  so GTK routes it.
- Keep the gloss-open guard (gloss handles space itself).
- Only act when `input_mode == Reader` (and not editable). In that case call
  the same path as the `a` bind: `play_current_line`, with the
  no-timestamp toast fallback.

Rationale: space-in-Reader and space-in-overlay diverge (Reader/translation →
play cursor line; echoes → play selected echo), so a single global send can't
serve all of them. Handling Reader here and each overlay in its own arm keeps
"space == that surface's `a`" exact, and leaves the editable/gloss guards in
the one place that already owns them.

Note: this block currently runs before mode dispatch, so the editable + gloss
guards must remain here (they protect Search and Gloss before their handlers
run). Only the *action* changes (TogglePause → play_current_line) and the
*scope* narrows (act only in Reader mode; for other non-editable modes, fall
through to mode dispatch so the overlay's own space arm runs).

### 2. Per-overlay space arms

- `handle_translation_overlay_key`: add `"space"` arm identical to its `"a"`
  arm (`play_current_line`), with the same no-timestamp toast fallback.
- `handle_echoes_overlay_key`: add `"space"` arm identical to its `"a"` arm
  (`play_selected_echo`).
- `handle_gloss_key`: no change.

### 3. No-timestamp toast helper

`play_current_line` already returns `bool` (`false` when the line has no
timestamp). Add a small helper to show a 3-second bottom-center toast reusing
the existing `chapter_toast` + `glib::timeout_add_local_once` pattern already
duplicated several times in `keymap.rs` (e.g. lines 1757-1762). The helper
takes `&AppState` and a `&str` label. Use it from the Reader space path and the
translation-overlay space arm when `play_current_line` returns `false`.

(The echoes `play_selected_echo` path has its own existing feedback and is out
of scope for the toast.)

### 4. keymap.json

No change. Space is handled directly in `handle_key`, not via the keymap
lookup table, so there is no JSON binding to update.

### 5. Ctrl+/ keybinds overlay

`src/ui/keybinds_overlay.rs` documents space. Update the space `KeyDef` and its
`describe()` arm to read "play from cursor line start" (matching `a`) instead of
"play/pause toggle". Use the `update-cairo-keybinds-overlay` skill to do the
exhaustive cross-reference pass (no blank slot, no wrong label, every label has
a describe() arm).

## Out of scope

- Changing what `a` does on any surface.
- Adding space to surfaces with no play binding (synopsis, pickers, visual).
- Any change to `Tab` (still toggles play/pause where bound).
- Toast for the echoes no-selection case.

## Testing

- `cargo build` — compiles.
- `cargo test --bins` — pure-logic suite stays green (no GTK measurement
  involved in this change).
- Runtime verification ("space plays from the cursor line on screen") requires
  the e2e harness, which the agent generally cannot launch (the live dwl owns
  the seat). Ask the user to:
  - press `a`, note playback position; press `Space` on the same line, confirm
    identical seek;
  - in the translation overlay, confirm `Space` matches `a`;
  - in the echoes overlay, confirm `Space` plays the selected echo;
  - in the gloss overlay, confirm `Space` still reads the block aloud;
  - on a line with no timestamp, confirm the toast appears.

## Affected files

- `src/input/keymap.rs` — global space block (narrow + retarget), translation
  overlay space arm, echoes overlay space arm, toast helper.
- `src/ui/keybinds_overlay.rs` — space KeyDef + describe() arm.
