# Overlay `\` cycle — failure modes

Frequency-ordered ledger for the plain-`\` segment-overlay rotation
(`src/input/actions/overlay_cycle.rs`). Read this BEFORE debugging a
"`\` doesn't go to X" report.

## 1. A stop opens nothing and the lap silently ends (fixed 2026-07-27)

**Tell.** `\` in an overlay puts you back in the READER instead of the next
overlay. The log shows `KEY: name=backslash … mode=GlossOverlay` immediately
followed by `KEY: name=backslash … mode=Reader`, with a
`CHAPTER_TOAST: … "No journal entry for this segment"` in between.

**Root cause.** `open_journal_scene` returned `()`. `cycle_from_gloss` called
it as its last statement, having ALREADY hidden the gloss overlay and restored
the reader position, so when the journal stop turned out to be empty it had no
way to know and no way to go back. Any scene without a journal entry made the
syntax stop unreachable by cycling — even when a syntax gloss existed.

**Fix.** `open_journal_scene` returns `bool`. `advance()` PROBES the candidate
stops (`gloss_covers_cursor` / `journal_has_content_at_cursor`) BEFORE any
teardown, and opens the first with content. When none has content the current
overlay is left untouched and a toast says
"Nothing else to cycle to for this passage".

## 2. Probing the live cursor instead of the lap anchor

**Tell.** The probe reports the wrong line — e.g. `cur=(5, 2, 437)` when the
lap started at 424 — so a narrow stop (a syntax gloss over one sentence) never
matches while a wide one (a reader-gloss over a whole speech) does.

**Root cause.** Opening the gloss stop moves `current_line` to the END of the
glossed passage. Probing `s.current_line` therefore asks about the wrong line
entirely.

**Fix.** Resolve from the lap anchor —
`gloss_return_pos.or(journal.return_pos)` — falling back to `current_line`
only when no overlay is open.

## 3. The reader-gloss and syntax-gloss sit on DIFFERENT passages

Not a bug — expected, and the reason #2 bites. A syntax gloss is created from
an explicit narrower selection (visual-mode "Syntax", or `-`/`_` + `Return`),
so it gets its OWN `passages` row with its own span. Example (Ant 5.2):

- passage 17588 `Ant.5.2.424`–`437` → reader-gloss (whole speech)
- passage 17589 `Ant.5.2.424`–`425` → syntax-gloss (one sentence)

A cursor at 437 is inside the first and outside the second. `\` correctly
reports no syntax stop there; move to 424 and it appears. **Confirm the
cursor's `line_in_div` before concluding the cycle is broken** — the reader's
glossed-passage TINT covers the whole 424–437 span, so a screenshot showing a
highlighted "Most probable" does NOT mean the cursor is on line 424.

## 4. Verifying headlessly

The whole rotation drives under cage. Land on the work, step the cursor onto a
line covered by BOTH gloss types, then send `\` and watch the mode transitions
in the log. Screenshot byte-size alone distinguishes the stops (the syntax card
renders 1 page, the reader gloss 2). See
`docs/troubleshooting/headless-testing.md` for the harness rules — in
particular, use a FRESH `XDG_RUNTIME_DIR` so `grim` cannot capture the user's
own desktop.
