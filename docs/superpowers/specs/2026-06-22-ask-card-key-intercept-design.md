# Shared ask-card key intercept — design

## Goal

Replace the three hand-duplicated ask-card key-intercept blocks in
`src/input/keymap.rs` with one shared helper, so the Tab / Ctrl+Enter / Escape /
fall-through routing can no longer drift between the journal, gloss, and synopsis
overlays.

This is the keymap-routing twin of the already-landed shared `AskCard` widget
(`src/ui/ask_card.rs`): that unified the *widget*; this unifies the *key routing*
that drives it.

## Background — the duplication today

Three handlers each open with a near-identical intercept block, run before their
own navigation/chord routing:

- `handle_journal_key` (`keymap.rs` ~658) — overlay `journal_overlay`; actions
  `journal::submit_prompt` / `journal::close_prompt`.
- `handle_gloss_key` (`keymap.rs` ~787) — overlay `gloss_overlay`; actions
  `gloss::submit_gloss_prompt` / `gloss::close_gloss_prompt`.
- `handle_synopsis_overlay_key` (`keymap.rs` ~1150) — overlay `gloss_overlay`
  (synopsis renders in the gloss overlay); actions
  `synopsis::submit_amend_prompt` / `synopsis::close_amend_prompt`.

The shared shape, when the ask card is open:

1. `Tab` / `ISO_Left_Tab` → `<overlay>.toggle_ask_focus()`, consume (`return true`).
2. `Ctrl+Return` → `<submit>(state)`, consume.
3. `Escape` → `<close>(state)`, consume.
4. If `ask_focus == AskFocus::Ask` → `return false` so GTK delivers the typed
   character to the editable input.

### Where the three diverge

- **journal & gloss**: the whole block sits inside `if ask_open { … }`. When the
  card is **closed**, `Escape` is *not* handled by this block — it falls through
  to the handler's own match arm, which closes the overlay. (Two-stage Esc via
  two separate code paths.)
- **synopsis**: structured differently. `Tab` is consumed **unconditionally**
  (always swallowed so it never reaches playback toggle), and `Escape` is handled
  **inline, two-stage, even when the card is closed**: if `ask_open` →
  `close_amend_prompt`; else hide the overlay and reset
  `InputMode` to `Reader`. Only the *ask-open* `Ctrl+Return` and the
  `ask_focus == Ask` fall-through mirror journal/gloss.

## Decision: helper owns ONLY the ask-card keys

The helper fires only while the ask card is **open** and only for the ask-card
chord keys (Tab / Ctrl+Enter / Esc) plus the focus-fall-through. **Escape when the
ask card is closed is NOT the helper's concern** — it returns `NotHandled` and the
caller's existing overlay-close logic runs exactly as today.

Rejected alternative: a fuller helper that also owns the closed-card overlay-close
(taking an extra `overlay_close` callback). It would consolidate more but force
journal/gloss to route their match-arm Esc through the helper and pull synopsis's
inline `InputMode = Reader` reset into a callback — a larger change with no
behavioral gain. We keep each overlay's two-stage Esc semantics verbatim.

## Component

A new **private free function** in `src/input/keymap.rs` (same module as the three
callers). It does **not** belong in `src/ui/ask_card.rs`: that module is pure GTK
widget state and knows nothing about `AppState`, key names, or the action fns.
Keeping the helper in `keymap.rs` respects that boundary.

### Types

```rust
/// Outcome of the shared ask-card key intercept.
enum AskIntercept {
    /// The helper consumed the key (Tab / Ctrl+Enter / Esc-while-open) —
    /// the calling handler must `return true`.
    Consumed,
    /// The ask card holds focus and the key is a plain character — the calling
    /// handler must `return false` so GTK delivers it to the editable input.
    FallThrough,
    /// Not an ask-card key, or the card is closed — the calling handler
    /// continues its own routing.
    NotHandled,
}
```

### Signature

```rust
/// Intercept the ask-card chord keys when `ask_open`.
///
/// `toggle` / `submit` / `close` are the calling overlay's own actions
/// (e.g. journal: toggle_ask_focus / submit_prompt / close_prompt). Esc-when-
/// closed is intentionally NOT handled here; the helper returns `NotHandled` so
/// the caller's existing overlay-close path runs unchanged.
fn ask_card_intercept(
    ask_open: bool,
    ask_focus: AskFocus,
    key_name: &str,
    is_ctrl: bool,
    state: &Rc<RefCell<AppState>>,
    toggle: impl Fn(&Rc<RefCell<AppState>>),
    submit: impl Fn(&Rc<RefCell<AppState>>),
    close: impl Fn(&Rc<RefCell<AppState>>),
) -> AskIntercept
```

`AskFocus` is `crate::ui::ask_card::AskFocus`.

### Body

```rust
fn ask_card_intercept(
    ask_open: bool,
    ask_focus: AskFocus,
    key_name: &str,
    is_ctrl: bool,
    state: &Rc<RefCell<AppState>>,
    toggle: impl Fn(&Rc<RefCell<AppState>>),
    submit: impl Fn(&Rc<RefCell<AppState>>),
    close: impl Fn(&Rc<RefCell<AppState>>),
) -> AskIntercept {
    if !ask_open {
        return AskIntercept::NotHandled;
    }
    if key_name == "Tab" || key_name == "ISO_Left_Tab" {
        toggle(state);
        return AskIntercept::Consumed;
    }
    if is_ctrl && key_name == "Return" {
        submit(state);
        return AskIntercept::Consumed;
    }
    if key_name == "Escape" {
        close(state);
        return AskIntercept::Consumed;
    }
    if ask_focus == AskFocus::Ask {
        return AskIntercept::FallThrough;
    }
    AskIntercept::NotHandled
}
```

## Call-site changes

Each handler still reads `(ask_open, ask_focus)` from its overlay (this stays at
the call site — the overlay accessor differs per handler), then calls the helper.

### journal (`keymap.rs` ~658)

Replace the `if ask_open { … }` block with:

```rust
let (ask_open, ask_focus) = {
    let s = state.borrow();
    (s.journal_overlay.ask_is_open(), s.journal_overlay.ask_focus())
};
match ask_card_intercept(
    ask_open, ask_focus, key_name, is_ctrl, state,
    |st| st.borrow().journal_overlay.toggle_ask_focus(),
    crate::input::actions::journal::submit_prompt,
    crate::input::actions::journal::close_prompt,
) {
    AskIntercept::Consumed => return true,
    AskIntercept::FallThrough => return false,
    AskIntercept::NotHandled => {}
}
```

The match arm `"Escape" => journal::close_overlay` stays untouched — when the card
is closed the helper returns `NotHandled` and Esc reaches the arm as today.

### gloss (`keymap.rs` ~787)

Same shape, with `gloss_overlay`, `gloss::submit_gloss_prompt`,
`gloss::close_gloss_prompt`. The gloss handler's closed-card Esc lives in its
`"Escape" | "n" | "bracketright"` match arm (hide overlay + `InputMode::Reader`,
`keymap.rs` ~982). That arm stays untouched — the helper only fires when
`ask_open`, so closed-card Esc reaches the arm exactly as today.

### synopsis (`keymap.rs` ~1150)

Synopsis keeps its **closed-card** Tab/Esc semantics verbatim and routes only the
**open-card** keys through the helper. The cleanest equivalent ordering:

```rust
let (ask_open, ask_focus) = {
    let s = state.borrow();
    (s.gloss_overlay.ask_is_open(), s.gloss_overlay.ask_focus())
};

// Open-card chord keys go through the shared helper.
match ask_card_intercept(
    ask_open, ask_focus, key_name, is_ctrl, state,
    |st| st.borrow().gloss_overlay.toggle_ask_focus(),
    crate::input::actions::synopsis::submit_amend_prompt,
    crate::input::actions::synopsis::close_amend_prompt,
) {
    AskIntercept::Consumed => return true,
    AskIntercept::FallThrough => return false,
    AskIntercept::NotHandled => {}
}

// Closed-card overlay-level semantics (preserved verbatim):
// Tab is always consumed so it never reaches playback toggle.
if key_name == "Tab" || key_name == "ISO_Left_Tab" {
    return true;
}
// Escape with the card closed hides the overlay and returns to Reader.
if key_name == "Escape" {
    let mut s = state.borrow_mut();
    s.gloss_overlay.hide();
    s.input_mode = crate::app::InputMode::Reader;
    return true;
}
```

This is behaviorally identical to the current synopsis block:

- **Card open**: helper handles Tab→toggle, Ctrl+Return→submit, Esc→close,
  Ask-focus→fall-through (`Consumed`/`FallThrough` short-circuit before the
  closed-card lines). ✅
- **Card closed**: helper returns `NotHandled`; the explicit Tab arm consumes Tab,
  the explicit Esc arm hides + resets InputMode. ✅

The implementer must confirm the original synopsis ordering (Tab/Esc handled
before the `ask_focus == Ask` fall-through) is preserved — see the original block
at `keymap.rs:1150–1191`.

## Global Constraints

- **No behavior change.** Every key in every overlay state (card open vs closed,
  Doc-focus vs Ask-focus) must do exactly what it does today. This is a pure
  extraction. The reviewer verifies behavioral equivalence line-by-line against
  the three original blocks (`keymap.rs` ~658, ~787, ~1150).
- **Keybinds overlay**: no keybind is added, removed, or changed, so the Ctrl+/
  keybinds overlay (`src/ui/keybinds_overlay.rs`), `keymap_config.rs`, and
  `keymap.json` are **not** touched. (Per CLAUDE.md, the overlay must be updated
  only when a binding changes — it does not here.)
- **Helper stays in `keymap.rs`**, not `ask_card.rs` (boundary: `ask_card.rs`
  knows nothing about `AppState` / key names / action fns).
- `cargo build` and `cargo clippy` must be clean; `cargo test --bins` must stay
  green (no new tests — see Testing).

## Testing

The helper is pure routing over GTK `AppState`. Constructing `AppState` requires
GTK init and is not unit-testable in this harness, and the helper measures no
geometry. Therefore:

- **No new unit test** (a fake one would assert nothing — forbidden by the review
  rubric).
- Verification = `cargo build` clean + `cargo clippy` clean + reviewer confirming
  behavioral equivalence against the three original blocks.
- **Runtime verification is the user's cage pass** (acceptance criterion is "the
  three overlays still route Tab/Esc/Ctrl+Enter and typing correctly"): in each of
  the journal, gloss, and synopsis overlays — open the ask card, confirm Tab
  toggles focus, typing reaches the field, Ctrl+Enter submits, Esc closes the
  card; then with the card closed confirm Esc closes the overlay (and, for
  synopsis, Tab is still swallowed). Per the project's headless-verification
  protocol, the agent cannot reliably drive cage on the live dwl session, so this
  is handed to the user.

## Out of scope

- The other audit refactors (#5 footer/hint builder, #6 Picker trait, #7
  `run_claude_request` bridge helper, #8 sentinel-key constants) — each is its own
  spec.
- Any change to `AskCard` itself or the overlays' widget structure.
