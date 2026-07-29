# Journal ask — blank card during the LLM wait

Frequency-ordered ledger for what the journal overlay shows between submitting
a question and the answer arriving.

## 1. Vocab Q&A (Ctrl+Shift+r) showed an empty card (fixed 2026-07-29)

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
