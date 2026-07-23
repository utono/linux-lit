# Animated loading state: Braille spinner + staged journal-ask labels

## Problem

The Q&A / gloss / synopsis loading state is a static string. Two issues:

1. **No motion.** `show_loading` renders a literal `"Asking…"` / `"Glossing…"`
   / `"Synthesizing…"` into the overlay's TextView buffer. During a
   multi-second generation the card looks frozen — an ellipsis, not a spinner.

2. **The journal ask reads as a double-ask.** The journal ask is THREE
   sequential Claude round-trips: `extract_scene_terms` → `improve_question`
   → `ask_claude`. Today the card shows `"Q: <raw>\n\nAsking…"` during the
   refine phase, then re-renders `"Q: <improved>\n\nAsking…"` for the answer
   phase — so the same "Asking…" appears twice while the question visibly
   rewrites itself mid-wait. A working pipeline reads like a stall or a
   repeated ask.

## Goal

- Animate the loading indicator with a **Braille spinner**
  (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`, ~120 ms/frame) on all three surfaces (journal ask,
  gloss, synopsis).
- Stage the journal ask as readable progress: **"Refining question…"** during
  the extract-terms + improve-question phase, **"Answering…"** during the
  answer phase.
- **Hold the reader's raw typed question on screen** through the refine phase;
  swap to the improved question only when the "Answering…" phase begins. No
  mid-wait question rewrite.

## Design

### The spinner animator

A small reusable `LoadingAnimator` owns:
- a `RefCell<Option<glib::SourceId>>` for the active ~120 ms `timeout_add_local`
  tick,
- a `Cell<usize>` frame index over the 10 Braille frames,
- the current `body` (question text to hold, may be empty) and `label`
  (e.g. "Answering…") it is animating.

On each tick it rewrites ONLY the overlay's buffer text to
`format!("{body}\n\n{spinner} {label}")` (or just `"{spinner} {label}"` when
`body` is empty), where `spinner` is the current frame glyph. It does not touch
blocks, footer, or sizing — those are set once by `show_loading`.

`start(view, body, label)` installs the tick (removing any prior source first);
`stop()` removes the source and clears the frame state. `stop()` is idempotent.

Placement: a new `src/ui/loading_animator.rs` (small, self-contained), owned as
a field on both `JournalOverlay` and `GlossOverlay`. (Synopsis shares the gloss
overlay's animator, since it renders through `gloss_overlay`.)

### Journal: staged labels + hold-then-swap

`JournalOverlay::show_loading` gains a `label: &str` parameter. It sets the
static frame immediately (so the first paint is correct even before the first
tick), starts the animator with `(body = the passed question, label)`, and
otherwise keeps its current behavior (hide footer, clear blocks, size the card,
show scrim/container).

`src/input/actions/journal.rs` call sites:
- `submit_passage_question` (the entry that runs extract-terms → improve):
  `show_loading(raw_question, "Refining question…")` — passes the **raw typed
  question** as the held body.
- `ask_claude` (answer phase, currently calls `show_loading(question)` at its
  top): `show_loading(improved_question, "Answering…")` — the swap to the
  improved question happens HERE, at the answer phase, not during refine.

The result renders (`show_page`, `show_message`) call `animator.stop()` before
painting the answer, so no late tick repaints over it.

### Gloss / synopsis: animate the fixed label

`GlossOverlay::show_loading()` (fixed "Glossing…") and
`show_loading_message(msg)` start the animator with an empty body and the
respective label, so the spinner animates. No label-staging needed
(single-phase). The gloss result-render path (`show_page` / the gloss render)
calls `animator.stop()`.

### Signature / caller compatibility

`JournalOverlay::show_loading(&self, question: &str)` →
`show_loading(&self, question: &str, label: &str)`. There are TWO callers in
`journal.rs` (both updated to pass a stage label). No other caller.
`GlossOverlay::show_loading` / `show_loading_message` keep their signatures;
only their bodies change to start the animator.

## Files

- Create: `src/ui/loading_animator.rs` (the `LoadingAnimator`).
- Modify: `src/ui/mod.rs` (register the module).
- Modify: `src/ui/journal_overlay.rs` (animator field; `show_loading` gains
  `label`; start on load, stop on result render).
- Modify: `src/ui/gloss_overlay.rs` (animator field; animate
  `show_loading`/`show_loading_message`; stop on result render).
- Modify: `src/input/actions/journal.rs` (two call sites: staged labels +
  raw-vs-improved body).
- No `synopsis.rs` change required (it calls `gloss_overlay.show_loading()`,
  which now animates); confirm during implementation.
- No keybind / legend / keymap surfaces touched.

## Risks

1. **Late-tick repaint over the answer (primary).** If the animator keeps
   ticking after the result renders, it paints the spinner over the answer.
   Every result-render path MUST call `animator.stop()` before/at painting the
   answer, and `stop()` must remove the `glib` source so no queued tick fires.
   Guard: `start()` removes any existing source first; `stop()` is idempotent.
2. **Buffer contention.** The tick sets the buffer text; it must run only while
   loading. `stop()` on the result path prevents overlap. The tick also must
   not clobber blocks/footer/sizing — it touches only `buffer().set_text`.
3. **Borrow discipline.** The animator holds no `AppState` borrow — it captures
   the `gtk4::TextView` (a widget clone) in the tick closure. No `RefCell<
   AppState>` access inside the tick.
4. **`prefers-reduced-motion`** does not apply (native GTK). Keep the tick
   gentle (~120 ms) and it stops the instant the result lands.

## Testing

- **Headless (cage):** submit a journal ask; capture during the refine phase
  (raw Q held + a spinner glyph + "Refining question…"), during the answer
  phase (improved Q + spinner + "Answering…"), and after (answer rendered, no
  spinner glyph or stray "Answering…" left behind). Capture a gloss "Glossing…"
  frame to confirm it animates. A single screenshot catches one spinner frame —
  assert the glyph is one of the 10 Braille frames and the label is correct for
  the phase.
- `cargo test --bins` stays green. If the spinner frame-advance is factored
  into a pure helper (`fn frame(i: usize) -> char`), unit-test the cycle.
- `cargo clippy --bin linux-lit` — no new errors.

## Acceptance

- All three surfaces show a moving Braille spinner during generation.
- The journal ask shows "Refining question…" (raw question held) then
  "Answering…" (improved question), never a repeated "Asking…".
- The spinner stops cleanly when the answer/gloss/synopsis renders — no leftover
  spinner or label over the result.
- No regression to the loading card's sizing, footer-hide, or block-clearing.
