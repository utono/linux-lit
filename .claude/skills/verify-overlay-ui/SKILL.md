---
name: verify-overlay-ui
description: Use when a change touches overlay rendering, spacing, pagination, or block navigation (journal notes/Q&A, gloss, synopsis) and needs on-screen verification without the user's manual review — runs the headless cage e2e, asserts ink-band/fill/cursor invariants, and visually reviews the captures
argument-hint: [--test NAME] [--skip-run]
---

# Verify overlay UI headlessly

Verify overlay rendering + navigation end-to-end with NO user eyeballing:
run the cage e2e test, let the assertions catch invariant violations, then
visually review every capture in `target/ui/` and report what you see.
Full background + gotchas: `docs/troubleshooting/headless-overlay-ui-verification.md`.

## Steps

1. **Build + pure suite first.**

```bash
cd ~/utono/linux-lit && cargo build && cargo test --bins
```

2. **Run the overlay e2e** (default: the journal corpus-note test; pass a
   different `--test` for other surfaces as they gain coverage). Run it
   yourself — the harness's tempdir cage does not touch the live seat:

```bash
./scripts/e2e-env.sh cargo test --test journal_markdown -- --ignored --nocapture
```

   It asserts, per page walked with `j`:
   - **Ink band** — no text column outside `TEST_JOURNAL_CONTENT_BAND`
     (`scripts/check_ink_outside.py`); catches tag-margin escapes.
   - **Fill** — page 1 ink reaches ≥ 50% of the viewport (underfill guard).
   - **Cursor** — every `j` press logs a NEW `JOURNAL-CURSOR:` line with a
     CHANGED `full#` index (phantom-press guard). Compare `full#`, never the
     page-local `cursor#` (it legitimately repeats across page turns).

3. **If the run fails or hangs**: read the tail of the output and
   `~/utono/linux-lit/linux-lit-dev.log` (the `JOURNAL-PAGINATE:` heights +
   `KEY:`/`JOURNAL-CURSOR:` timeline decode most failures without pixels).
   Premature-wtype and stale-live-instance gotchas are in the doc above. If
   cage cannot run in this sandbox (SIGTERM/exit 144), hand the user the
   exact command instead — that is the only manual step ever needed.

4. **Visual review (mandatory, even on a green run).** Open EVERY
   `target/ui/journal_md_*.png` with the Read tool and report inline:
   quote on-screen text, judge heading sizes/spacing, accent-bar placement,
   list indents, page fill, marker position. Assertions catch invariants;
   spacing/size judgment calls are caught only by looking.

5. **Verdict.** Claim "verified" only when the e2e passed AND the visual
   pass found nothing. Otherwise report the defect with the capture name
   and the log lines that pin it.

## Adding coverage for a new overlay surface

Follow `docs/troubleshooting/headless-overlay-ui-verification.md` →
"Adding coverage": emit a `TEST_<SURFACE>_VIEWPORT_RECT` (+ content band)
under `LIT_HEADLESS_TEST`, log whole-model cursor indices, clone the
`tests/journal_markdown.rs` pattern.
