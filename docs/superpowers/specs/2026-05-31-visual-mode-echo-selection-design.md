# Visual-mode echo selection (`i` in Visual mode)

**Date:** 2026-05-31
**Status:** Approved design, pending implementation plan

## Problem

The `i` key is bound to `Action::ShowEchoes` (echoes overlay) and works in Reader
mode. In Visual mode, `i` falls through `handle_visual_key`'s `_ => true` catch-all
(`src/input/keymap.rs:998`) and is silently consumed — no action, no log line. The
user expects `i` to work in **both** Reader mode and Visual mode.

In Visual mode, `i` should echo the **selected range** (one or more speaker turns)
rather than only the cursor's turn.

## Root cause (debugging, confirmed)

- The failing `i` in `linux-lit-dev.log` was pressed in Visual mode (after
  `EnterVisualMode`, between two `j` moves). `handle_visual_key` consumes it via
  `_ => true` with no dispatch.
- Reader-mode `i` → `ShowEchoes` → `show_echoes_for_cursor_line` is correct and
  treated as working (per user). **Out of scope.**

## What already exists (reuse, do not rebuild)

- `show_echoes_for_cursor_line` (`src/input/actions/echoes.rs`): expands the
  cursor's contiguous same-speaker turn, tries the **cached-link path**
  (`find_echo_turn` → `load_echo_links`), and otherwise **live-embeds** the turn
  via `voyage::embed_query` → `find_similar_passages`, rendering the **echoes
  overlay** (sticky session keyed on `turn_id`/`EchoTurnKey`; `alt+i` returns to
  the turn's work).
- `action_inner_monologue` (`src/input/visual.rs:515`): already maps a Visual
  selection (`start..=end`) to work lines via `work_line_for_buffer`, live-embeds
  the selection, and runs `find_similar_passages`. It feeds the **gloss echo
  picker**, not the echoes overlay. We reuse its selection→lines pattern, not its
  destination.
- `voyage::embed_query` (`src/voyage.rs:28`) — live embedding, already used by both
  paths above.
- `EchoTurnKey` + `save_echo_turn` + `ensure_echo_tables`
  (`src/db/queries.rs:1041`+): `echo_turns` is keyed `UNIQUE(work_abbrev, div1,
  div2, start_line, end_line)`; `save_echo_turn` is idempotent (returns existing id
  or inserts). A multi-turn selection's first/last lines form a valid unique key.

## Design

Add a new function `show_echoes_for_selection` in `src/input/actions/echoes.rs`,
mirroring `show_echoes_for_cursor_line` but sourcing text from the Visual
selection. Bind it to `i` in Visual mode.

### Selection resolution

1. Read `visual_selection.range()` → `(start, end)` buffer lines.
2. Map to work lines (reuse the `work_line_for_buffer` + `work.lines.get` pattern
   from `action_inner_monologue:527`).
3. Group the selected work lines into contiguous speaker turns (same grouping rule
   as `cursor_turn`: same `div1`/`div2`/`speaker`).

### Routing by turn count

- **Exactly 1 turn, or a 2-turn exchange** → build an `EchoTurnKey` from the
  selection's first/last line and try `find_echo_turn` → `load_echo_links` (the
  fast, no-API cached path), exactly like Reader `i`'s cache hit. On a cache miss,
  fall through to live-embed (below).
- **More than a 2-turn exchange, or no cached match** → live-embed the selection
  text via `voyage::embed_query` → `find_similar_passages`.

### Persisting ad-hoc selection turns (user decision)

For the live-embed path, **synthesize and persist a turn** so `alt+i` return works
and results are cached:

1. Build `EchoTurnKey` from the selection (`work_abbrev`, `div1`/`div2` of the
   first selected line, `start_line` = first line, `end_line` = last line,
   `speaker` = first turn's speaker or a synthesized label for multi-speaker
   selections, `turn_text` = joined selected text).
2. `save_echo_turn` → `turn_id` (idempotent).
3. After `find_similar_passages`, persist results by **reusing
   `persist_and_load`** (`src/input/actions/echoes.rs:342`), the same helper
   Reader `i`'s live-embed path uses — it calls `save_echo_turn` +
   `insert_echo_links` and returns `(turn_id, links)`. No new persistence code.
4. Render the **echoes overlay** with `echo_overlay_turn_id = Some(turn_id)` and
   the matching `EchoTurnKey`, so the sticky session and `alt+i` behave identically
   to Reader `i`.

### End state

`show_echoes_for_selection` exits Visual mode first (matching
`action_inner_monologue:555`), then lands in Reader + EchoesOverlay state — the
same end state as Reader `i`.

### Wiring

Add an `"i"` arm to `handle_visual_key` (`src/input/keymap.rs`, before `_ =>
true`):

```rust
"i" => {
    crate::input::actions::echoes::show_echoes_for_selection(state, tokio_handle);
    true
}
```

`handle_visual_key`'s current signature does not take `tokio_handle`; the call site
(`keymap.rs:80`) must thread it through (the echo path needs it for
`voyage::embed_query`, as the Reader path does).

## Out of scope

- Reader-mode `i` (unchanged, treated as working).
- The Return → action-popup → inner-monologue gloss flow (unchanged).
- Affect-axis behavior (reuse existing `echo_affect_weight` as both echo paths do).

## Testing

- Unit/logic: selection → turn grouping (1 turn, 2-turn exchange, >2 turns,
  multi-speaker) produces the expected `EchoTurnKey` and routes to the correct
  path. No network needed for the routing/grouping logic.
- Manual (user runs): in Visual mode, select 1 turn → echoes overlay matches Reader
  `i`; select 3+ turns → live-embed, overlay shows, `alt+i` returns; verify
  `ACTION`/`ECHO` log lines now appear for Visual `i` (previously absent).
- Verify `cargo build` and `cargo clippy` clean.

## Open items for the implementation plan

- `speaker` value for multi-speaker selections in the synthesized `EchoTurnKey`
  (e.g. first turn's speaker, or a joined label). Affects only the cache key and
  overlay header, not the echo search.
