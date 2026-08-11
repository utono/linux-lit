# Plan — vocab-word gloss bind (Ctrl+Shift+g)

Spec: `docs/superpowers/specs/2026-08-10-vocab-word-gloss-bind-design.md`
Branch: `feat/vocab-gloss-bind` (worktree `~/utono/linux-lit-wt/feat-vocab-gloss-bind`)
Base: `d5b0eaa1`

Tasks are sequential (each touches the same few files); reviews are
waived by the user, so the end-of-branch build + clippy + tests + the
headless on-screen check are the ONLY gates and are mandatory.

## Task 1 — the opener

`src/input/actions/gloss.rs`, beside `try_open_syntax_gloss_at_cursor`:

- `try_open_vocab_gloss_at_cursor(state) -> bool` with
  `const GLOSS_TYPES: &[&str] = &["vocab-word"]`.
- Clone the syntax variant's shape EXCEPT the displayed-passage overlap
  fallback (lines 3772-3797). That fallback exists only so the `\` cycle
  can reach a narrower syntax passage from a wider reader-gloss overlay.
  This bind opens from the reader cursor, where a strict cursor-line
  match is the correct and predictable rule.
- `open_vocab_gloss_at_cursor(state)` wrapper: toast
  `"No vocab gloss on this line"` on false, via `show_tts_toast`.

## Task 2 — action + dispatch

- `src/input/actions/mod.rs`: add `ShowVocabGloss` to the `Action` enum.
- `src/input/keymap.rs`: dispatch arm ->
  `crate::input::actions::gloss::open_vocab_gloss_at_cursor(state)`.

## Task 3 — bind + mirrors (lockstep, one change)

- `src/input/keymap_config.rs`: `(KeyCombo::ctrl_shift("g"),
  Action::ShowVocabGloss)` in the gloss group; extend the `g`-hub comment.
- `src/ui/keybinds_overlay.rs`: keycap strip entry AND `describe()` arm.
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`: add
  `{"key": "g", "ctrl": true, "shift": true, "action": "ShowVocabGloss"}`
  or the JSON shadows the compiled default. Restow if needed.

## Task 4 — tests

`keymap_config.rs` unit test: `ctrl_shift("g")` -> `ShowVocabGloss` and
`ctrl("g")` -> `ToggleGlossOverlay` (guards level-2 distinctness on
`<AD07>`).

## Task 5 — verification (mandatory, reviews waived)

1. `cargo build`, `cargo clippy`, `cargo test` in the worktree.
2. Headless cage: land on a LoJ `solicitude` passage (e.g. `LoJ.1.2207`),
   press Ctrl+Shift+g, screenshot, and CONFIRM THE OVERLAY PAINTS with
   the gloss text. A green build is not acceptance — verify the visible
   surface.
3. Also confirm the miss path toasts rather than opening a stale overlay.

## Task 6 — finish

Merge to master from the MAIN checkout (`--no-ff`), re-verify build,
push, remove the worktree, delete the branch.

## Hazards

- `keymap.json` shadowing is silent — easy to "verify" a bind that the
  JSON overrides.
- Glosses key by `canonical_abbrev`, never `Work.abbrev`.
- Do NOT copy the overlap fallback from the syntax variant.
- Cage needs `LIT_DEV=1`, `GSK_RENDERER=cairo`, `LIT_NO_MPV=1`; find the
  fresh log by mtime, and clean up with the SCOPED pkill only.
