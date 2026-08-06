# Journal ask — blank card during the LLM wait

Frequency-ordered ledger for what the journal overlay shows between submitting
a question and the answer arriving.

**There are THREE ask paths, and each has now shipped this same bug once.**
Before diagnosing, establish WHICH path the report came from — the two already
fixed do not reproduce it, so a static trace of the wrong one concludes "not a
bug." The tell is the log line right after `KEY: name=Return ctrl=true`:

- **Passage ask** — reader Ctrl+a; submits in `JournalOverlay`; no
  `RETURN_TO_READER` line. Fixed from the start.
- **Vocab Q&A** — popup Ctrl+Shift+r; submits in `Reader`; marked by
  `VOCAB QA: asking about '<word>'`. Fixed 2026-07-29 (item 2).
- **Gloss passage ask** — gloss overlay Ctrl+a; submits in `GlossOverlay`;
  marked by `RETURN_TO_READER: from GlossOverlay`. Fixed 2026-08-05 (item 1).

## 1. Gloss-overlay passage ask showed an empty card (fixed 2026-08-05)

**Tell.** Ask a question from inside the GLOSS overlay (Ctrl+g, then Ctrl+a,
type, Ctrl+Return). A collapsed cream strip — running head only ("BH-Barrett …
Chapter 9"), no question, no indicator — sits on screen for the whole round
trip. Measured: submit at 281711ms, `IMPROVE-Q` at 287321ms, answer at
309835ms, so the blank strip was up ~28s. The answer itself lands correctly;
only the wait is broken.

**Root cause.** The SAME `last_card_size == (0,0)` mechanism as item 2 below,
reached by a third route. `submit_passage_question` (`journal.rs`) does call
`show_loading`, so the path looks correct — but the card was never SIZED:

- `open_passage_qa_float` (`gloss.rs`) floats its input card inside the GLOSS
  overlay and **deliberately never opens the journal overlay** (its own comment
  says so). So `JournalOverlay::size_card` — the ONLY writer of
  `last_card_size` (`journal_overlay.rs`), reachable only via `show_page` and
  `show_passage_source` — had never run for this session's journal overlay.
- `show_loading` then hit its `if w > 0` guard FALSE and skipped
  `set_size_request` entirely, while `set_visible(true)` ran unconditionally.
  Hence: visible, but at the natural (collapsed) size, with no body.

The reader-side ask escapes this only because `begin_passage_ask` sets
`input_mode = JournalOverlay` and calls `render_current` BEFORE opening its ask
card, priming `last_card_size` as a side effect.

**Fix.** `submit_passage_question` now claims the overlay and renders before
showing the loading state, so every caller is primed regardless of entry point:

```rust
let mut s = state.borrow_mut();
s.input_mode = crate::app::InputMode::JournalOverlay;
render_current(&mut s);
let head = crate::app::division_synopsis::cursor_head(&s);
s.journal_overlay.set_running_head(&head.0, &head.1);
s.journal_overlay.show_loading(text, "Refining question…");
```

`render_current` takes its `pending_passage`-matches-band early return here and
calls `show_passage_source(…, cw, h)` → `size_card`, which is what primes the
size. Claiming `JournalOverlay` at submit is safe: `ask_claude`'s arrival branch
sets `input_mode = JournalOverlay` **unconditionally** (no guard to break — it
was made unconditional on 2026-07-23 for exactly this gloss-side path).

**Lesson that generalizes.** `show_loading`'s silent-skip guard has now caused
this bug on two of three paths. The guard makes a MISSING PRIME look like a
working call: the code reads as if it sizes, and fails only at runtime, only on
the path that never rendered first. Any NEW ask entry point must either render
before `show_loading` or be added to `submit_passage_question`'s shared prime.

**Verification.** `scripts/land-on.sh BH-Barrett 9.0`, then Ctrl+g, Ctrl+a,
type, Ctrl+Return. Expect a FULL-HEIGHT card in three states: typed question +
"Refining question…", improved question + "Answering…", then the rendered Q&A
with the footer back. Verified headlessly 2026-08-05 (spends a real paid API
call). Note the first chord after launch is dropped — re-send it.

## 2. Vocab Q&A (Ctrl+Shift+r) showed an empty card (fixed 2026-07-29)

**Tell.** After asking a vocab Q&A from the popup, a collapsed journal card
sits on screen — running head only ("BH-Barrett … Chapter 4"), no body — for
the whole round trip. The wait is not brief: a real session logged
`IMPROVE-Q` at 483s and the answer at 507s, so the blank card was up ~24s.

**Root cause.** Two ask paths, only one of them wired for loading:

- The PASSAGE ask (`journal.rs` `submit_passage_question` → `ask_claude`)
  calls `journal_overlay.show_loading(question, …)` at BOTH stages, which is
  why it never shows a blank card.
- The VOCAB Q&A path (`vocab_journal.rs` `vocab_journal_ask`) never called
  `show_loading` at all. It only raised a held bottom-strip toast and stayed
  in `InputMode::Reader`, entering `JournalOverlay` only once the answer
  arrived.

A static trace of the passage path alone will conclude "this bug does not
reproduce" — it doesn't, on that path. Confirm WHICH ask path the report came
from before diagnosing; the log line `VOCAB QA: asking about '<word>'` marks
the vocab one.

**Fix.** `vocab_journal_ask` now mirrors the passage flow's ordering exactly.

**The ordering is load-bearing** — `show_loading` reads `last_card_size` and
**silently skips sizing while that width is 0**, and only a real render ever
writes it. So the sequence must be:

1. set band/`page_index`/`input_mode = JournalOverlay`
2. `render_current` — this is what primes `last_card_size`
3. `set_running_head`
4. `show_loading(seed_question, "Refining question…")`

then, in `improve_question`'s `on_done`, re-`show_loading(improved,
"Answering…")` so the body shows the phrasing actually being answered.

**Two consequences that must be handled together:**

- **The reveal guard.** The answer-arrival branch used to be
  `Some(id) if input_mode == Reader`. Claiming `JournalOverlay` at submit
  time breaks that guard — the entry would never be revealed. It now accepts
  `Reader | JournalOverlay`, keeping the "user navigated away mid-wait →
  don't hijack" behavior for every other mode.
- **Failure paths must stop the spinner.** A save failure or request failure
  that only toasts leaves the loading card spinning forever. Both arms now
  call `journal_overlay.show_message(…)` when still in `JournalOverlay`;
  `show_message` stops the animator and restores the footer.

**Animator exit paths (verified).** `loading_animator.stop()` is called by
`show_page` (`journal_overlay.rs:960`), `show_passage_source` (:1156),
`show_message` (:1178) and `hide` (:1218) — so every render and every close
funnel stops it. Nothing in the action layer stops it directly.

**Verification.** Headless `scripts/land-on.sh BH-Barrett 4.0`, `rr` to open
the vocab popup on a vocab word, then Ctrl+Shift+r. Expect three states:
question + "Refining question…", improved question + "Answering…", then the
rendered Q&A with the footer back ("Q&A n of m"). This spends a real paid API
call.
