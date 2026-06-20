# Synopsis `E` edit key — design

**Date:** 2026-06-19
**Status:** Approved, ready for implementation plan

## Summary

Add an `E` keybind to the synopsis overlay that prompts the Claude API to
**edit** the currently displayed scene synopsis according to a free-text
instruction the reader types — e.g. *"split the first paragraph into two after
the first sentence"*, *"tighten the second paragraph"*, *"reorder so the arrest
comes first"*. The rewritten synopsis replaces the current one, is persisted to
`lit.db` (`scene_synopses`), is displayed in the overlay, and is revertible with
the existing `U` undo key.

This reuses nearly all of the existing `A` ("ask") machinery; the only material
difference is the **system prompt** sent to Claude (a structural-editor prompt
that follows the instruction literally, rather than the augment prompt that
weaves in an answer to a reader's question).

## Motivation

The `A` ("ask about this scene") flow already opens a stacked input card, sends
the current synopsis + a user prompt to Claude, receives `<p>`-tagged
paragraphs, persists them, displays them, and supports `U` undo. But its system
prompt (`SYNOPSIS_AMEND_PROMPT`) is purpose-built to *augment and explain*:
"KEEP all of the existing content and wording as much as possible... weave in a
clear, concise explanation that answers the reader's question. Do not drop any
plot points." That framing resists a *structural edit* instruction such as
"split the first paragraph into two", which is an edit command, not a question.

`E` provides the editor framing while sharing the same UI and async/save/undo
plumbing.

## Non-goals

- No new input-card UI. `E` reuses the existing stacked ask card.
- No separate undo key or multi-level undo. `E` shares the single-level
  `synopsis_undo` slot with `A`; `U` reverts whichever (A or E) ran last.
- No change to `Space` / `Shift+Space` behavior. (A separate one-line footer
  label fix — `⇧Space` → `Shift+Space` — is already applied and is not part of
  this feature.)

## Architecture

### Reused as-is

- **Ask card UI** — `GlossOverlay::open_ask_card_with(title, hint)`,
  `take_ask_text()`, `close_ask_card()`, `ask_is_open()`. Already generic; only
  collects text.
- **Async send → save → display → recolor** — the body of `amend_synopsis` in
  `src/input/actions/synopsis.rs`.
- **Undo** — `synopsis_undo: Option<((i64,i64), String)>` and `undo_amend`
  (`U`). `E` writes the pre-edit text into the same slot before applying its
  result.
- **Scene targeting** — `synopsis_amend_scene` (the scene currently shown in the
  overlay, which `n`/`p` may have moved away from the cursor's scene).
- **DB prompt override** — `crate::db::prompts::active_prompt(key)`, mirroring
  `synopsis.amend`.
- **Claude call** — `crate::claude::send_message(system, user, model)`.
- **Persist** — `crate::db::queries::save_synopsis(conn, abbrev, div1, div2,
  text, model)`; undo restore via `restore_synopsis_text`.

### New (small, isolated)

1. **Prompt-kind flag on `AppState`.** The ask card's Ctrl+Enter submit must
   know whether the open card is an *ask* or an *edit*. Add:

   ```rust
   #[derive(Clone, Copy, PartialEq)]
   pub enum SynopsisPromptKind { Ask, Edit }
   ```

   and a field `pub synopsis_prompt_kind: SynopsisPromptKind` (default `Ask`),
   set when the card opens.

2. **`SYNOPSIS_EDIT_PROMPT`** in `synopsis.rs` — an editor system prompt. It
   instructs the model to:
   - apply the reader's edit instruction literally (split/merge paragraphs,
     reword, tighten, reorder, etc.);
   - preserve plot accuracy and not invent events;
   - return the **full** revised synopsis (not a diff);
   - output ONLY `<p>...</p>`-tagged paragraphs, no heading/preamble/commentary
     — identical output contract to the amend prompt so the renderer is
     unchanged.

   Registered under DB prompt key `synopsis.edit` via `active_prompt`
   (compiled-in constant is the fallback), matching `synopsis.amend`.

3. **Shared async helper.** Refactor the body of the existing `amend_synopsis`
   into one private helper, e.g.

   ```rust
   fn run_synopsis_revision(
       state_rc: &Rc<RefCell<AppState>>,
       instruction: &str,
       system_prompt: String,
       log_verb: &str, // "amended" / "edited"
   )
   ```

   `amend_synopsis` and the new `edit_synopsis` become thin callers that pass the
   right system prompt and log verb. This keeps A and E from drifting (a
   targeted "improve the code you're working in" refactor; no behavior change to
   A).

4. **`show_edit_prompt(state)`** — opens the card via
   `open_ask_card_with("EDIT THIS SCENE", "<edit hint> · Tab switch · Ctrl+Enter
   submit · Esc cancel")` and sets `synopsis_prompt_kind = Edit` (and
   `synopsis_amend_scene = synopsis_overlay_scene`, as `show_amend_prompt`
   does). `show_amend_prompt` is updated to set `synopsis_prompt_kind = Ask`.

5. **Submit dispatch.** `submit_amend_prompt` reads `synopsis_prompt_kind` and
   calls `amend_synopsis` or `edit_synopsis` with the taken text. (Renaming is
   optional; the simplest change is to branch inside the existing function.)

6. **Key routing.** Add `"E" => { show_edit_prompt(state); true }` to the
   synopsis-overlay match in `src/input/keymap.rs` (alongside `"A"` and `"U"`).
   `E` is currently unbound there (falls to `_ => true`).

## Data flow

1. Reader is in `SynopsisOverlay` mode viewing scene `(div1, div2)`.
2. `E` → `show_edit_prompt`: card opens (kind = `Edit`), `synopsis_amend_scene`
   captured.
3. Reader types the edit instruction; `Ctrl+Enter` → `submit_amend_prompt`.
4. Card closes; if instruction non-empty, `edit_synopsis(state, instruction)`.
5. `run_synopsis_revision`: build user message (play, label, current synopsis,
   instruction), `show_loading()`, spawn the Claude call on the Tokio handle
   with `SYNOPSIS_EDIT_PROMPT` (or DB override).
6. On success: save to `lit.db` (stamp model), record pre-edit text in
   `synopsis_undo`, update `synopsis_cache`, `show_synopsis(...)`,
   `recolor_cached_blocks`, mode back to `SynopsisOverlay`, log
   `SYNOPSIS: edited <abbrev> (div1,div2)`.
7. `U` → `undo_amend` restores the pre-edit text (cache + lit.db + display).

## Error handling / edge cases

Identical to `A`:
- Empty instruction → no-op (`submit_amend_prompt` early-returns on blank).
- No cached synopsis for the scene → early return (no Claude call).
- Claude error → card shows `Error: <e>`; mode returns to `SynopsisOverlay`.
- Tokio join error → logged, no UI change.
- Undo is single-level: `U` reverts the last A *or* E only.

## UI strings

- **Footer hint** (`src/ui/gloss_overlay.rs`, the synopsis footer set in
  `show_synopsis`): insert `· E edit` after `A ask`, before `U undo`:

  `Esc close · j/k block · Space play · n/p scene · Shift+Space synth · Ctrl+g
  glosses · A ask · E edit · U undo`

- **Edit card title:** `EDIT THIS SCENE`.
- **Edit card hint:** describes typing an edit instruction, e.g.
  `Describe the edit (split/merge paragraphs, reword, reorder)  ·  Tab switch  ·
  Ctrl+Enter submit  ·  Esc cancel`.

## Mandatory cross-references (project rules)

- **`keymap.json` stow source** — `A` and `U` are synopsis-overlay binds handled
  directly in `keymap.rs` (not reader binds in `keymap.json`). `E` follows the
  same in-`keymap.rs` pattern and needs **no** `keymap.json` entry. (Confirm
  during implementation that `A`/`U` are absent from `keymap.json`.)
- **Ctrl+/ keybinds overlay** (`src/ui/keybinds_overlay.rs`) — add/extend the
  `E` key's `describe()` arm and `KeyDef` for the synopsis context, via the
  `update-cairo-keybinds-overlay` skill (its three-pass cross-check).

## Testing

- Logic is pure-prompt selection + async plumbing that mirrors an existing,
  working path (`A`). No GTK measurement is involved.
- `cargo build` + `cargo test --bins` (pure-logic suite) must stay green.
- The on-screen result — `E` opens the card, the edited `<p>` paragraphs render,
  `U` reverts — is a **visual** acceptance criterion. Per the project's headless
  rules, the agent cannot reliably launch cage from the live session, so the
  user verifies on screen (open synopsis with `h`, press `E`, type an
  instruction, `Ctrl+Enter`, confirm the rewrite, then `U`).

## Files touched

- `src/app.rs` — `SynopsisPromptKind` enum + `synopsis_prompt_kind` field + init.
- `src/input/actions/synopsis.rs` — `SYNOPSIS_EDIT_PROMPT`,
  `run_synopsis_revision` helper, `edit_synopsis`, `show_edit_prompt`,
  `show_amend_prompt`/`submit_amend_prompt` kind wiring.
- `src/input/keymap.rs` — `"E"` arm in the synopsis match.
- `src/ui/gloss_overlay.rs` — footer hint string (`· E edit`).
- `src/ui/keybinds_overlay.rs` — `E` cap/detail for synopsis context.
- (Optional) DB prompt seed for `synopsis.edit` if prompts are seeded in code.
