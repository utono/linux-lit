# #16 — block-visual-key-twin: unify the synopsis/gloss visual-key handlers

**Date:** 2026-06-24
**Status:** Design approved, pending spec review
**Scope class:** Behavior-preserving function unification — the audit's #16
(`docs/superpowers/audit-opportunities.md`). Two near-identical whole functions
collapse into one parameterized by a config of plain `fn` pointers. NOT a
trait/generic (house style rejects those). **Render-tier:** the handlers drive
the visual-block selection display in the synopsis/gloss overlays, so the unit
suite proves it compiles but a **user manual check** gates the merge.

## Problem

`handle_synopsis_visual_key` (keymap.rs:1328–1386) and `handle_gloss_visual_key`
(keymap.rs:1392–1450) are byte-identical except for the `y` (yank) and
`Escape|V` (exit) arms. The gloss handler's own doc comment says it "Mirrors
`handle_synopsis_visual_key`". This is the one near-whole-function clone the
Batch-2 audit found — ~58 lines duplicated, the highest drift risk in the batch
(a future change to the gg-chord or j/k/G logic must be made in both, or they
silently diverge).

Both dispatch from `route_key` (keymap.rs:117/120): `InputMode::GlossVisual` →
gloss handler, `InputMode::SynopsisVisual` → synopsis handler. Both operate on
the same `state.gloss_overlay` widget (the synopsis overlay reuses `GlossOverlay`).

## The byte-identical part (extracts)

Everything except the `y` and `Escape|V` arms is identical between the two:
- the `gg`-chord preamble (`PendingG` → `visual_to_end(false)`),
- the `"j"` / `"k"` / `"G"` / `"g"` match arms,
- the `_ => true` catch-all,
- the `y` arm's *structure* (borrow → `(text, n)` → if non-empty wl-copy + log →
  borrow_mut → exit + set mode + set hint + "Copied" toast),
- the `Escape|V` arm's *structure* (borrow_mut → exit + set mode + set hint).

## The 5 variant points (parameterize via config)

| variant | synopsis | gloss |
|---|---|---|
| yank text getter | `visual_selection_text()` | `visual_selection_buffer_text()` |
| log tag | `"SYNOPSIS:"` | `"GLOSS:"` |
| **yank exit fn** | `exit_visual()` | `exit_visual_to_start()` |
| **escape exit fn** | `exit_visual()` | `exit_visual()` |
| return mode | `InputMode::SynopsisOverlay` | `InputMode::GlossOverlay` |
| set-hint fn | `set_synopsis_hint()` | `set_gloss_hint()` |

**The asymmetry that must be preserved:** the gloss handler uses
`exit_visual_to_start()` on **yank** but `exit_visual()` on **Escape**; the
synopsis handler uses `exit_visual()` on **both**. So the config needs TWO exit
slots — `yank_exit` and `escape_exit` — not one. Collapsing them to one would
change behavior (the gloss Escape would start-exit, or the gloss yank would
plain-exit). This is the load-bearing detail of this unification.

## The config struct

In `src/input/keymap.rs`, beside the unified handler:

```rust
/// Per-mode variance for the unified visual-block key handler. Plain `fn`
/// pointers over `&GlossOverlay` (both the synopsis and gloss overlays use the
/// GlossOverlay widget) — no trait, no generic.
struct BlockVisualCfg {
    /// Yank text source: synopsis reads the rendered selection text; gloss reads
    /// the raw buffer block text (source verse + gloss as displayed).
    yank_text: fn(&crate::ui::gloss_overlay::GlossOverlay) -> String,
    /// Log prefix ("SYNOPSIS" / "GLOSS").
    log_tag: &'static str,
    /// Exit on yank (synopsis: exit_visual; gloss: exit_visual_to_start).
    yank_exit: fn(&crate::ui::gloss_overlay::GlossOverlay),
    /// Exit on Escape/V (both: exit_visual — kept separate to preserve the
    /// gloss yank/escape asymmetry).
    escape_exit: fn(&crate::ui::gloss_overlay::GlossOverlay),
    /// InputMode to return to on exit.
    return_mode: crate::app::InputMode,
    /// Hint setter for the returned-to overlay.
    set_hint: fn(&crate::ui::gloss_overlay::GlossOverlay),
}

const SYNOPSIS_VISUAL_CFG: BlockVisualCfg = BlockVisualCfg {
    yank_text: crate::ui::gloss_overlay::GlossOverlay::visual_selection_text,
    log_tag: "SYNOPSIS",
    yank_exit: crate::ui::gloss_overlay::GlossOverlay::exit_visual,
    escape_exit: crate::ui::gloss_overlay::GlossOverlay::exit_visual,
    return_mode: crate::app::InputMode::SynopsisOverlay,
    set_hint: crate::ui::gloss_overlay::GlossOverlay::set_synopsis_hint,
};

const GLOSS_VISUAL_CFG: BlockVisualCfg = BlockVisualCfg {
    yank_text: crate::ui::gloss_overlay::GlossOverlay::visual_selection_buffer_text,
    log_tag: "GLOSS",
    yank_exit: crate::ui::gloss_overlay::GlossOverlay::exit_visual_to_start,
    escape_exit: crate::ui::gloss_overlay::GlossOverlay::exit_visual,
    return_mode: crate::app::InputMode::GlossOverlay,
    set_hint: crate::ui::gloss_overlay::GlossOverlay::set_gloss_hint,
};
```

(Method `fn`-pointer paths like `GlossOverlay::visual_selection_text` coerce to
`fn(&GlossOverlay) -> String` because the methods take `&self` — **verified** with
a throwaway `rustc` compile that a `&self` method path assigns to a
`fn(&T) -> R`-typed `const` field. No closure wrapper needed.)

## The unified handler

```rust
fn handle_block_visual_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    cfg: &BlockVisualCfg,
) -> bool {
    // gg: extend to the first block.
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            state.borrow().gloss_overlay.visual_to_end(false);
        }
        return true;
    }

    match key_name {
        "j" => { state.borrow().gloss_overlay.visual_step(1); true }
        "k" => { state.borrow().gloss_overlay.visual_step(-1); true }
        "G" => { state.borrow().gloss_overlay.visual_to_end(true); true }
        "g" => { KeyState::start_chord(key_state, ChordState::PendingG); true }
        "y" => {
            let (text, n) = {
                let s = state.borrow();
                ((cfg.yank_text)(&s.gloss_overlay), s.gloss_overlay.visual_selection_len())
            };
            if !text.is_empty() {
                let _ = std::process::Command::new("wl-copy").arg(&text).spawn();
                crate::logging::log(&format!("{}: copied {} blocks", cfg.log_tag, n));
            }
            {
                let mut s = state.borrow_mut();
                (cfg.yank_exit)(&s.gloss_overlay);
                s.input_mode = cfg.return_mode;
                (cfg.set_hint)(&s.gloss_overlay);
                crate::ui::toast::show_transient(&s.chapter_toast, "Copied", 2);
            }
            true
        }
        "Escape" | "V" => {
            let mut s = state.borrow_mut();
            (cfg.escape_exit)(&s.gloss_overlay);
            s.input_mode = cfg.return_mode;
            (cfg.set_hint)(&s.gloss_overlay);
            true
        }
        _ => true,
    }
}
```

Note the log line is now `"{}: copied {} blocks"` with `cfg.log_tag` (`"SYNOPSIS"`
/`"GLOSS"`) — byte-equivalent output to the originals (`"SYNOPSIS: copied …"` /
`"GLOSS: copied …"`).

## Dispatch sites

`route_key` (keymap.rs:117, 120) — replace the two direct calls:

```rust
// before:
crate::app::InputMode::GlossVisual => handle_gloss_visual_key(state, key_state, key_name),
crate::app::InputMode::SynopsisVisual => handle_synopsis_visual_key(state, key_state, key_name),
// after:
crate::app::InputMode::GlossVisual =>
    handle_block_visual_key(state, key_state, key_name, &GLOSS_VISUAL_CFG),
crate::app::InputMode::SynopsisVisual =>
    handle_block_visual_key(state, key_state, key_name, &SYNOPSIS_VISUAL_CFG),
```

Then DELETE `handle_synopsis_visual_key` and `handle_gloss_visual_key`.

## Keybinds-overlay reference

`src/ui/keybinds_overlay.rs:465` names both handlers in a `describe()` text
(`handle_synopsis_visual_key / handle_gloss_visual_key / ...`). Update that
reference to name `handle_block_visual_key` (per the project's
`update-cairo-keybinds-overlay` rule — the overlay's describe text is a hand-
maintained mirror). This is a doc-string change, not a behavior change.

## Verification — RENDER-TIER

The unification is access-shape/structure-only — no logic change — so the unit
gates prove it compiles and the rest of the suite is unaffected:
- `cargo build` — clean
- `cargo test --bins` — **413** (no test covers these handlers; this proves no
  collateral breakage)
- `cargo clippy` — **115**

But the handlers drive the *visual-block selection display* and the yank/exit
behavior, which the unit suite cannot exercise (no test references them). So a
**user manual check gates the merge** (the agent cannot launch cage):

In BOTH a synopsis overlay (`h` then Shift+V to enter SynopsisVisual) AND a gloss
overlay (open a gloss, Shift+V to enter GlossVisual), exercise the full key set
and confirm identical behavior to before:
- `j` / `k` — selection extends down / up one block,
- `G` — extends to the last block; `gg` — extends to the first,
- `y` — yanks the selection (paste to confirm: synopsis yanks rendered text,
  gloss yanks the buffer block text), shows the "Copied" toast, and returns to
  the SYNOPSIS / GLOSS overlay respectively,
- `Escape` (and `V`) — exits without copying, returns to the right overlay.

The critical regression to watch is the **yank/escape exit asymmetry** in the
gloss overlay: gloss `y` must `exit_visual_to_start` (cursor at block start) while
gloss `Escape` must `exit_visual` — if the config collapsed them, this is where it
shows. If anything differs from current behavior → systematic debugging, do NOT
merge.

## Risks & mitigations

- **Collapsing the yank/escape exit asymmetry.** The load-bearing risk; mitigated
  by the two separate config slots (`yank_exit` / `escape_exit`) and the manual
  check's explicit asymmetry test.
- **`fn`-pointer coercion of `&self` methods.** Mitigated by `cargo build`; fall
  back to `|o| o.method()` closures only if a bare method path won't coerce
  (it should).
- **Log output drift.** The `"{}: copied {} blocks"` format with `log_tag` is
  byte-equivalent to the two originals — confirm in the diff.
- **Missed keybinds-overlay reference.** Named explicitly above.

## Out of scope

The other Batch-2 opportunities (#15, #17–#21) are their own PRs. No change to the
`GlossOverlay` widget methods, the visual-selection logic, or the dispatch beyond
the two arms.
