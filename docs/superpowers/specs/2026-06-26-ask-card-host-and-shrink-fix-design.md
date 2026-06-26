# Ask-card host: uniform lifecycle + fix the viewport-occlusion bug

**Date:** 2026-06-26
**Status:** Design approved

## Problem

Two coupled problems, one root.

**The bug (runtime-proven).** Opening the journal Q&A "ask a question" card draws
it **over** the reading text — the lower answer rows render *behind* the ask card.
A runtime diagnostic proved the cause: opening the ask card does **NOT** shrink
the scrolled viewport (`page_size` stays constant, both synchronously and on the
next idle). The ask card overflows the overlay's container and overlaps the
bottom of the `vexpand` scroll area; the occluded rows are fully laid out, just
covered. **No bottom-clip recompute can fix this** — there is no partial edge row
to mask and no viewport resize to react to (the just-merged BottomClipGuard
refactor is correct but does not address this; see
`docs/troubleshooting/clip-prevention.md` "occlusion is not clipping").

**Why it overflows.** The overlay container is sized
`set_size_request(card_width, card_height)` where `card_height =
content_hbox.height()` — the full reading-pane height, a *minimum*. The container
is `valign=Center`. Inside it (vertical box): `title → vexpand scroll → footer →
ask card`. GTK satisfies the scroll's `vexpand` by keeping it at full height;
when the ask card becomes visible and needs its natural ~200px, the box grows
*beyond* its minimum and, being `valign=Center`, the extra extends off-pane
rather than displacing the scroll. The scroll never yields its space.

**The duplication (the maintainability half).** The ask card is already a shared
`AskCard` component (`src/ui/ask_card.rs`), but the **hosting lifecycle** is
hand-wired and duplicated in BOTH overlays: each appends `ask.container()`, and
each defines its own `open_ask_card`(`_with`) / `close_ask_card` /
`toggle_ask_focus` / `take_ask_text` / `ask_is_open` / ask-clip-recompute, with
journal additionally toggling the footer. The wiring is near-identical and drifts
(the bug — and its fix — has to be applied per overlay).

**The two are structurally identical, so gloss has the same latent bug.** Gloss
and journal use the same `card_height = content_hbox.height()`, the same
`valign=Center` container, the same `vexpand` scroll, the same appended-last
shared `AskCard`. The gloss synopsis ask card will occlude its text the same way
once the synopsis is long enough. Fixing one without the other leaves the bug
half-fixed. So: fix it ONCE in a shared host, and route both overlays through it.

## Goal

1. **Fix the occlusion:** when the ask card opens, the scrolled viewport must
   shrink so the reading text ends *above* the ask card (no occlusion). Apply via
   a shared host so journal AND gloss are both fixed.
2. **Unify the ask-card hosting** into one reusable unit so the lifecycle (insert,
   open/close, focus toggle, take-text, is-open, clip-recompute, footer-hide) is
   defined once, not duplicated per overlay — making the ask card easier to
   maintain and enhance.

## Design

### The layout fix: bound the container height so the scroll yields

Make the overlay container a **bounded** height rather than a minimum it can
exceed, so the `vexpand` scroll is forced to give up height to the ask card.

Approach (to validate against both widget trees in the plan; the spec commits to
the *outcome*, the plan picks the exact call):

- The container currently relies on a min-height (`size_request` height) +
  `valign=Center`. Change it so the container's height is CONSTRAINED to the pane
  (it cannot grow past `card_height`): set the container to fill a height-bounded
  parent (`valign=Fill` within the scrim sized to the pane) OR cap its height so
  the box must shrink the `vexpand` scroll when the ask card claims space.
- Net effect, verifiable by the same runtime check that proved the bug: with the
  ask card open, the scrolled `page_size` is SMALLER than with it closed (by ~the
  ask card's height). Today it is unchanged — that is the regression signal.

The fix lives in the shared host (below) so it is written once.

### The shared host: `AskCardHost` (extend `AskCard`'s role, or a thin host)

Unify the hand-wired lifecycle. Two viable shapes — the plan picks one; both put
the layout-correct insertion + lifecycle in ONE place:

- **Option A — `AskCard` owns its hosting.** Add to `AskCard`:
  - `attach(container: &gtk4::Box)` — append the card with the layout properties
    that let the box shrink the sibling scroll (the fix), instead of each overlay
    doing `container.append(ask.container())` raw.
  - `open(title, hint, card_width)` / `close()` already exist; add the shared
    side effects an overlay needs as injected callbacks or return signals:
    on-open and on-close hooks so the overlay can run its own extras (clip
    recompute; journal's footer hide/show) without re-implementing open/close.
- **Option B — a small `AskCardHost` struct** holding the `AskCard` + a reference
  to the host container + an optional footer widget + the clip guard, exposing
  `open(title, hint)` / `close()` / `toggle_focus()` / `take_text()` /
  `is_open()` that do the uniform sequence: open/close the card, recompute the
  clip (via the existing `BottomClipGuard`), and hide/show the footer if present.
  Each overlay holds one `AskCardHost` and calls these instead of its own copies.

**Recommendation: Option B** (a host struct) — it composes the existing
`BottomClipGuard` and the footer toggle into the same unit, so an overlay's ask
lifecycle becomes "hold an `AskCardHost`, call `open`/`close`," with the layout
fix and the clip recompute guaranteed. `AskCard` stays the pure widget; the host
owns the overlay-integration. Confirm in the plan.

The host's `open`/`close` subsume today's per-overlay:
- `open`: `ask.open(...)` → apply the height-bounding so the scroll shrinks →
  recompute clip → (if a footer was registered) hide it.
- `close`: `ask.close()` → restore the scroll height → recompute clip → show the
  footer.

### Migration

- gloss: replace its `open_ask_card`/`open_ask_card_with`/`close_ask_card`/
  `toggle_ask_focus`/`take_ask_text`/`ask_is_open`/`schedule_ask_clip_recompute`
  with calls into the host. (gloss has no footer to toggle — the host's footer is
  optional.)
- journal: same, plus register its `footer_container` with the host so open/close
  hides/shows it (replacing the manual `footer_container.set_visible`).
- Both keep their public method names if other code calls them (e.g.
  `toggle_ask_focus` from keymap) — those become thin delegations to the host.

## Verification

- **The bug, gone (the acceptance criterion):** journal — open Cromwell, open the
  journal Q&A, press **A**; the answer text ends cleanly ABOVE the ask card (no
  row behind it). Escape — text fills back down. Runtime check: the
  ask-open `page_size` is now < the closed `page_size` (was equal — that
  equality WAS the bug).
- **Gloss fixed too:** open a long synopsis (`h`), open its ask card; the synopsis
  text ends above the ask card, no occlusion.
- **Uniformity:** a grep shows neither overlay hand-wires
  `ask.open`/`ask.close`/footer-toggle/ask-clip directly — both go through the
  host.
- The e2e `tests/journal_clipping.rs` (already in tree, `#[ignore]`d) asserts no
  occluded row with the ask card open — it should PASS after this fix (it is the
  on-screen guard for exactly this).
- `cargo test --bins` green; clippy not increased (118).
- Agent cannot run the GUI — the on-screen criteria are user-verified (and the
  e2e via `./scripts/e2e-env.sh` when the seat is free).

## Out of scope

- The BottomClipGuard refactor (merged) — the host COMPOSES it; this does not
  change the guard.
- The main reading card's paginated clip and the translation overlay (the
  translation overlay has no ask card).
- Any change to what the ask card SENDS or the Q&A flow — purely layout + hosting.

## Files

- `src/ui/ask_card.rs` — the host (Option B: a new `AskCardHost`, or extend
  `AskCard`); the layout-bounding insertion (the fix).
- `src/ui/journal_overlay.rs` — hold an `AskCardHost` (register footer); delegate
  the ask lifecycle to it; remove the duplicated methods' bodies.
- `src/ui/gloss_overlay.rs` — same, no footer.
- `tests/journal_clipping.rs` — already present; becomes a passing guard.
- `docs/troubleshooting/clip-prevention.md` — update the "occlusion is not
  clipping" note to point at the host's height-bounding as the fix.
