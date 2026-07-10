# Ask-Passage Keybind (`A` `A`) — Design

_2026-07-10. Status: approved for planning._

## Problem

Asking a Journal Q&A question about a passage currently takes four-plus steps:
`V` to enter visual mode, `j`/`k` to extend the selection, `Return` to open the
Action menu, `Return` again on "Journal Q&A". The user wants a direct keybind
from the main reading card to the "Ask a question about this passage" card,
making the Action menu unnecessary for this (most common) action.

The downstream pipeline is untouched and already correct: the visual-selection
path calls `action_journal_qa` (`src/input/visual.rs`), which builds the
`<speaker>/<verse>` passage markup and hands off to `begin_passage_ask`
(`src/input/actions/journal.rs`). On Ctrl+Enter, `ask_claude` assembles the user
message (work type/title/author, scene label, windowed chapter text via
`scene_text_windowed`, the passage markup, the reader's question) under the
`journal.qa` system prompt from lit.db `api_prompts` (compiled fallback in
`src/gloss.rs::journal_qa_prompt`), and saves the answer via
`save_passage_page`.

## Behavior

- **Reader mode `A`** — auto-select the blank-line-delimited block around the
  cursor (prose: the paragraph; plays: the speech including its speaker label)
  and enter the existing visual mode with that selection highlighted, flagged
  as *entered-via-A* (pending ask).
- **Second `A` in visual mode** — confirm: exit visual mode and open the ask
  card via `begin_passage_ask`, exactly as the Action-menu "Journal Q&A" path
  does. Works in ANY visual mode session (also after a manual `V` selection),
  so the Action menu is never required for Journal Q&A.
- **`Return` in visual mode** — when the session was *entered-via-A*, Return is
  a direct confirm (same as the second `A`). When visual mode was entered via
  `V`, Return keeps its existing meaning: open the Action menu.
- `j`/`k`/`G`/`gg` still extend or shrink the selection before confirming;
  extending an `A`-entered selection does not clear the pending-ask flag.
- `Escape` / `V` cancels as today.
- Inside the ask card nothing changes: `Ctrl+Enter` submits, plain Return
  inserts a newline in the vim editor buffer.

## Block expansion rule

From the cursor's buffer line, walk up and down over contiguous lines whose
text is non-blank and not a separator (`line_types::is_separator`), stopping at
blank lines. Anchor = block start, cursor = block end (so `j` extends downward
naturally). If the cursor sits on a blank or separator line, fall back to plain
single-line visual mode, same as `V` (still flagged entered-via-A). The
expansion is a small pure helper over buffer-line texts, unit-testable without
GTK.

## Wiring

- New `Action::AskPassage` variant (`src/input/actions/mod.rs`), bound to
  `plain("A")` in `keymap_config.rs`; dispatch arm in `keymap.rs` calls a new
  `visual::enter_visual_block_mode` (block expansion + the internals of
  `enter_visual_mode` + the pending-ask flag).
- Pending-ask flag lives on `SelectionState` (e.g. `pending_ask: bool`),
  cleared implicitly when the selection is dropped.
- New `"A"` arm in `handle_visual_key` calling `action_journal_qa` (made
  `pub(crate)`); the `"Return"` arm branches on the pending-ask flag
  (confirm vs. open Action menu).
- **Both keymap files**: compiled defaults in `keymap_config.rs` AND the stow
  source `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — a stale
  JSON silently shadows compiled changes.
- Keybinds overlay (`src/ui/keybinds_overlay.rs`): `shift_action` on the `a`
  keycap + a `describe()` arm; run the `update-cairo-keybinds-overlay` skill's
  three-pass cross-reference after the change.

## Edge cases

- No work loaded / empty buffer: no-op (existing visual-mode guards).
- Cursor on blank/separator line: single-line visual fallback (above).
- `A`-entered selection then Return: direct confirm; `V`-entered then Return:
  Action menu (unchanged).
- Downstream guards (stale `pending_passage` band check, windowed scene text,
  passage DB key) are reused untouched.

## Testing

- Unit tests for the block-expansion helper: prose paragraph mid-buffer, speech
  with speaker label, cursor on blank line, block at buffer start/end.
- `cargo build` + `cargo test --bins`.
- Headless cage verify (per CLAUDE.md Headless Verification): on a Bleak House
  paragraph, `A` shows the highlighted paragraph selection; second `A` (and,
  separately, Return) opens the ask card; `V`-entered Return still opens the
  Action menu.

## Logistics

The four files touched (`keymap.rs`, `keymap_config.rs`, `actions/mod.rs`,
`keybinds_overlay.rs`) currently carry an unrelated uncommitted WIP (Shift+1
`CopyWorkInfo`). This feature goes on its own branch with its own commits,
keeping the two changes separate.
