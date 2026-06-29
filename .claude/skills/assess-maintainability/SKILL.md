---
name: assess-maintainability
description: Use when auditing linux-lit for accumulated maintainability drift — duplicated picker/overlay code, near-identical handler families, oversized files like app.rs, or repeated literals — and you need a ranked, numbered list of safe-scope refactoring opportunities in the house spec→plan→refactor→merge style. Run on demand (e.g. after a batch of features), not on a schedule.
argument-hint: (no args) | <module path to focus, e.g. src/ui>
---

# assess-maintainability

## Overview

Produce a **ranked, numbered list of behavior-preserving refactoring
opportunities** for linux-lit, in the same house style as audit opportunities
#5–#8 (footer-row-builder, picker-nav, claude-bridge, sentinel-keys). The
deliverable is NOT a generic code-smell report — it is a continuation of the
numbered ledger at `docs/audit-opportunities.md`, where each entry is
sized to one **safe-scope** PR: extract the byte-identical part of a duplication
family, name a magic literal, or move a self-contained block — with the
structurally-different cases **explicitly EXCLUDED**.

**Core principle:** every opportunity must be a *behavior-preserving* cut a
developer could ship through `docs(spec) → docs(plan) → refactor → merge --no-ff`
with zero behavior change. If it can't, it is out of scope for this audit.

## When to use

- After merging a batch of features, to catch drift no single diff revealed.
- When a file crosses a size threshold (the picker/overlay family grew again).
- The user asks to "assess maintainability" or "find refactoring opportunities."

## When NOT to use

- **Per-change review of a diff** → use `/code-review` (it sees only what you
  changed; it can't see cross-commit drift). Run `/code-review` before every
  merge; run *this* skill periodically over the whole tree.
- **Bug hunting / crash safety** (e.g. `.unwrap()` panics, wrong logic) → that is
  `/code-review`, not this. This audit is duplication/structure only.
- **Correctness vs. external readers** → `review-against-references`.

## What "safe-scope" means (the house bar)

This is the distinction the baseline generic assessment gets wrong. It defaults
to recommending large, behavior-*changing* rewrites ("split AppState into
sub-structs", "extract 6 modules from app.rs"). Those are real but they are NOT
safe-scope opportunities — they are multi-PR projects with behavior risk.

A safe-scope opportunity:

- **Is behavior-preserving and verifiable.** Pure widget-construction extraction,
  byte-identical tail extraction, literal → named constant. No control-flow
  change, no API change a caller can observe.
- **Decomposes the duplication into variant FAMILIES.** Near-identical sites are
  almost never uniform. Classify them (variant A/B/C/D…) and extract ONLY the
  part that is identical across all of them. See #6 picker-nav: 13 ListBox
  pickers share one identical `select_row_at` tail but compute their index four
  different ways — the helper owns only the tail.
- **Names what it EXCLUDES and why.** Every spec lists the structurally-different
  cases left untouched (e.g. picker-nav excludes `library_picker` for its
  scroll-into-view, `settings`/`action_popup` for `rem_euclid` over a `Vec`).
  An opportunity with no exclusions is usually mis-scoped.
- **Rejects speculative generality.** Do NOT propose a `Picker` trait, a generic
  abstraction, or a "future-proof" layer. The house style explicitly rejects
  these ("Deliberately NOT a full Picker trait"). One concrete, minimal cut.

## Procedure

1. **Read the ledger.** Open `docs/audit-opportunities.md` (create it
   from `ledger-template.md` if missing). Note the highest `#N` and which entries
   are already DONE (cross-check against `git log --grep refactor`). New
   opportunities continue the numbering; never reuse a merged number.

2. **Gather raw signals** (dispatch parallel `Explore`/`general-purpose` agents
   for breadth; this is read-only). Look for:
   - **Duplication families** — sibling files/handlers with near-identical bodies.
     The recurring hot spots: `src/ui/*_picker.rs` (~12 files),
     `src/ui/*_overlay.rs`, `handle_*_key` in `keymap.rs`, the
     gloss/synopsis/journal trio in `src/input/actions/`.
     Find them: `rg -n 'fn move_selection|fn handle_.*_key|fn build_footer' src`
   - **Repeated literals** — magic strings/numbers reused as sentinels.
     `rg -n '"whole_work"|"__.*__"|-1\b' src/input src/ui`
   - **Oversized files** — `fd -e rs . src -I | xargs wc -l | sort -rn | head`.
     Size is a *pointer to look*, not itself an opportunity — the opportunity is
     a specific self-contained block inside it that can move with no coupling
     (e.g. gloss_overlay.rs's ~1100 lines of pure buffer helpers).

3. **Convert each signal into a candidate opportunity.** For each, answer:
   - What is the **byte-identical** part across all sites? (That is what extracts.)
   - What are the **variants** (how does each site differ)?
   - What must be **EXCLUDED** and why?
   - Is it behavior-preserving? If not, drop it or note it separately as a
     "larger project, not safe-scope" — do NOT number it as an opportunity.

4. **Rank** by `(duplication_count × drift_risk) ÷ scope_size`. Highest = many
   identical copies that can drift apart, extracted by a tiny safe cut. A 6000-line
   file with no clean seam ranks LOW despite its size.

5. **Write the ledger.** Append each new opportunity as a numbered entry (format
   in `ledger-template.md`). Report the ranked list inline to the user, then STOP
   — this skill produces the audit, it does not write specs or refactor. The user
   picks an opportunity; the spec→plan→refactor→merge pipeline takes it from there.

## Quick reference

| Signal | Command | Becomes opportunity if… |
|--------|---------|-------------------------|
| Picker duplication | `rg -n 'fn move_selection' src/ui` | identical tail, varying index calc |
| Overlay duplication | `rg -n 'fn build_.*_row\|footer' src/ui` | identical widget construction |
| Handler duplication | `rg -n 'fn handle_.*_key' src/input` | shared hide/reset block |
| Magic sentinel | `rg -n '"whole_work"\|= -1' src` | same literal reused ≥2 sites |
| Oversized file | `wc -l` sort | a self-contained block moves with no coupling |

## Common mistakes

- **Proposing the god-struct rewrite as a P0 opportunity.** Splitting `AppState`
  or carving 6 modules out of `app.rs` is behavior-changing and multi-PR — note
  it as a "larger project" aside, never as a numbered safe-scope opportunity.
- **Listing a smell without the variant analysis.** "Dedup the toast blocks" is
  not an opportunity; "toast blocks split into variants A (2 sites) / B (1 site),
  extract the identical A part, exclude B" is.
- **No EXCLUSIONS.** If you can't name what stays untouched, you haven't scoped it.
- **Mixing in bug fixes.** `.unwrap()`→`?` is crash safety, not this audit.
- **Reusing a merged `#N`** or restarting numbering — always continue the ledger.
