---
name: assess-maintainability
description: Use when auditing linux-lit for accumulated maintainability drift — duplicated picker/overlay code, near-identical handler families, oversized files like app.rs, or repeated literals — and you need a ranked, numbered list of safe-scope refactoring opportunities in the house spec→plan→refactor→merge style. Run on demand (e.g. after a batch of features), not on a schedule.
argument-hint: (no args) | <module path to focus, e.g. src/ui>
---

# assess-maintainability

## Overview

Produce a **ranked, numbered list of behavior-preserving refactoring
opportunities**, continuing the ledger at `docs/audit-opportunities.md`. Not a
generic code-smell report: each entry is sized to one **safe-scope** PR that
ships through `docs(spec) → docs(plan) → refactor → merge --no-ff` with zero
behavior change. If a cut can't meet that bar, it is out of scope.

## When NOT to use

- **Per-change review of a diff** → `/code-review` (it sees only what you
  changed; it can't see cross-commit drift). Run it before every merge; run
  *this* skill periodically over the whole tree.
- **Bug hunting / crash safety** (`.unwrap()` panics, wrong logic) →
  `/code-review`. This audit is duplication/structure only.
- **Correctness vs. external readers** → `review-against-references`.

## What "safe-scope" means (the house bar)

A generic assessment defaults to large, behavior-*changing* rewrites ("split
AppState", "extract 6 modules from app.rs"). Those are real, but they are
multi-PR projects with behavior risk — note them as a "larger project" aside,
never as a numbered opportunity.

A safe-scope opportunity:

- **Is behavior-preserving and verifiable.** Widget-construction extraction,
  byte-identical tail extraction, literal → named constant. No control-flow or
  observable API change.
- **Decomposes the duplication into variant FAMILIES.** Near-identical sites are
  almost never uniform. Classify them (A/B/C…) and extract ONLY the part
  identical across all. See #6 picker-nav: 13 ListBox pickers share one identical
  `select_row_at` tail but compute their index four different ways — the helper
  owns only the tail. "Dedup the toast blocks" is not an opportunity; "variants A
  (2 sites) / B (1 site), extract identical A, exclude B" is.
- **Names what it EXCLUDES and why** (picker-nav excludes `library_picker` for
  its scroll-into-view, `settings`/`action_popup` for `rem_euclid` over a `Vec`).
  An opportunity with no exclusions is mis-scoped — if you can't name what stays
  untouched, you haven't scoped it.
- **Rejects speculative generality.** No `Picker` trait, no generic abstraction,
  no "future-proof" layer. One concrete, minimal cut.

## Procedure

1. **Read only the ledger's index, never the whole file.** The ledger
   (`docs/audit-opportunities.md`; create from `ledger-template.md` if missing)
   keeps shipped opportunities as one-liners and OPEN ones as full analyses.
   Pull just what numbering + de-dup require:

   ```bash
   rg -n '^## #|^\*\*#|^- \*\*#|Standing exclusions|Below the floor|^## Lessons|^- \*\*The #' \
     docs/audit-opportunities.md
   ```

   That pattern depends on the heading format pinned in `ledger-template.md` — if
   it returns an implausibly short index, fix the pattern rather than proceeding
   on a partial one (a missed `#N` means reusing a merged number).

   From it: the highest `#N` (new work continues the numbering), which are
   shipped, the Lessons block, the Standing exclusions. Open a full OPEN entry
   only when a new signal looks like it might collide with it. Cross-check DONE
   against `git log --grep refactor`. Never open
   `docs/audit-opportunities-archive.md` during an audit.

2. **Prune shipped entries FIRST, before adding any new ones.** For every `#N`
   merged since the last audit (confirm via `git log --grep refactor` + the
   hash), collapse its analysis to the one-line form (`**#N** slug (hash) —
   one-sentence what-and-where`). A completed action item never stays as a full
   analysis — that is what keeps the always-read ledger flat as batches
   accumulate.

   Before deleting the prose, append it **verbatim** (newest batch first) to
   `docs/audit-opportunities-archive.md`, so the Signal / Identical-part /
   Variants / EXCLUDED reasoning survives outside git history. Promote any
   still-load-bearing exclusion into the ledger's **Standing exclusions** rather
   than losing it in the archive.

3. **Gather raw signals** — dispatch parallel `Explore`/`general-purpose` agents
   for breadth (read-only, and it keeps the dumps out of this context):

   - **Duplication families** — sibling files/handlers with near-identical
     bodies. Recurring hot spots: `src/ui/*_picker.rs` (~12 files),
     `src/ui/*_overlay.rs`, `handle_*_key` in `keymap.rs`, the
     gloss/synopsis/journal trio in `src/input/actions/`. Find them with
     `rg -n 'fn move_selection|fn handle_.*_key|fn build_footer|fn build_.*_row' src`
     — an opportunity only if there's an identical tail or identical widget
     construction under the varying parts.
   - **Repeated literals** — magic strings/numbers reused as sentinels:
     `rg -n '"whole_work"|"__.*__"|= -1|-1\b' src/input src/ui`. Counts if the
     same literal appears at ≥2 sites.
   - **Oversized files** — `fd -e rs . src -I | xargs wc -l | sort -rn | head`.
     Size is a *pointer to look*, never itself an opportunity; the opportunity is
     a specific self-contained block that moves with no coupling (e.g.
     gloss_overlay.rs's ~1100 lines of pure buffer helpers).

4. **Convert each signal into a candidate.** What is the **byte-identical** part
   across all sites (that is what extracts)? What are the **variants**? What is
   **EXCLUDED** and why? Is it behavior-preserving — if not, drop it or note it
   as a larger project.

5. **Rank** by `(duplication_count × drift_risk) ÷ scope_size`. Highest = many
   identical copies that can drift apart, extracted by a tiny safe cut. A
   6000-line file with no clean seam ranks LOW despite its size.

6. **Write the ledger.** Append each new opportunity as a numbered entry (format
   in `ledger-template.md`), report the ranked list inline, then STOP. This skill
   produces the audit; it does not write specs or refactor. The user picks one;
   the spec→plan→refactor→merge pipeline takes it from there.
