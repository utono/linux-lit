# Named sentinel-key constants — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the whole-work synopsis sentinel `(-2,0)` and the journal-work `(-1,-1)` magic-number scene keys with two named `pub(crate) const`s, with zero behavior change.

**Architecture:** Add `SYNOPSIS_WHOLE_WORK` and `JOURNAL_WORK_DIV` consts to `src/app.rs`; swap the 8 synopsis literal sites (all in `app.rs`) and the 1 journal literal site (`journal.rs:271`). Pure literal→const substitution in one task.

**Tech Stack:** Rust.

**Spec:** `docs/superpowers/specs/2026-06-22-sentinel-key-constants-design.md`

## Global Constraints

- **No behavior change.** Literal→const swap only. A `const (i64,i64)` is bit-identical to the literal.
- **Do NOT touch any `(0, 0)` or `(-1, *)` literal** — the `(0,0)` Prologue value is overloaded with coincidental zeros (default/not-found/unrelated counter), and Induction `(-1,*)` has no live key site. Aliasing them is out of scope and a correctness hazard.
- **No keybind change** → do NOT touch `src/ui/keybinds_overlay.rs`, `src/input/keymap_config.rs`, `keymap.json`.
- `cargo build` + `cargo clippy` clean; `cargo test --bins` green.
- Bash/CLI rules (CLAUDE.md): use `rg`/`fd`, not `grep`/`find`; bypass `mv`/`cp`/`rm` aliases with `\mv -f`/`\cp -f`/`command rm -f`.

---

### Task 1: Add the two constants and swap all 9 literal sites

**Files:**
- Modify: `src/app.rs` (add 2 consts; swap 8 sites: 5599, 6251, 6263, 6292, 6714, 6725, 6737; reword 3 doc comments: 5596, 6246, 6289–6291)
- Modify: `src/input/actions/journal.rs` (swap 1 site: 271)

**Interfaces:**
- Produces: `pub(crate) const SYNOPSIS_WHOLE_WORK: (i64, i64) = (-2, 0);` and `pub(crate) const JOURNAL_WORK_DIV: (i64, i64) = (-1, -1);` in `crate::app`.

- [ ] **Step 1: Re-read the live sites before editing**

Line numbers below are approximate — confirm against the live file. Run:
`rg -n "\(-2, *0\)" src/app.rs` and `rg -n "JournalBand::Work => \(" src/input/actions/journal.rs`
Treat the live code as source of truth. Confirm there are exactly 8 `(-2, 0)` occurrences in `app.rs` (4 functional + 1 in a doc comment line + 3 in tests, plus the 2 extra doc-comment mentions at ~6289–6291) and exactly one functional `(-1_i64, -1_i64)` in `journal.rs`.

- [ ] **Step 2: Add the two constants to `app.rs`**

Place just above `fn whole_work_label` (~line 5596), or with the file's other module-level items if that grouping is clearer:

```rust
/// Scene-key sentinel for the whole-work synopsis (not a real (div1,div2)
/// scene). Sorts before all real scenes in the synopsis picker; `whole_work_label`
/// maps it to "Whole work". Distinct from the journal whole-work key, which lives
/// in a separate table and disambiguates by its `scope` column.
pub(crate) const SYNOPSIS_WHOLE_WORK: (i64, i64) = (-2, 0);

/// (div1, div2) stored for a journal page scoped to the whole work (vs a scene).
/// The journal_entries table ALSO carries a `scope` TEXT column ('work'/'scene'),
/// so this pair is not unique on its own — it is always paired with scope='work'.
pub(crate) const JOURNAL_WORK_DIV: (i64, i64) = (-1, -1);
```

- [ ] **Step 3: Swap the 4 functional `(-2,0)` sites in `app.rs`**

- `whole_work_label` (~5599): `if (div1, div2) == SYNOPSIS_WHOLE_WORK {`
- `prepend_whole_work` (~6251): `out.push(SYNOPSIS_WHOLE_WORK);`
- `has_whole_work` (~6263): `let has_whole_work = s.synopsis_cache.contains_key(&SYNOPSIS_WHOLE_WORK);`
- scene-loop guard (~6292): `if k != SYNOPSIS_WHOLE_WORK && seen.insert(k) && s.synopsis_cache.contains_key(&k) {`

- [ ] **Step 4: Swap the 3 test `(-2,0)` sites in `app.rs`**

Mind the arg-vs-tuple distinction:
- ~6714 (separate args): `assert_eq!(super::whole_work_label(SYNOPSIS_WHOLE_WORK.0, SYNOPSIS_WHOLE_WORK.1), Some("Whole work"));`
- ~6725 (tuple in a vec): replace the `(-2, 0)` entry → `vec![SYNOPSIS_WHOLE_WORK, (1, 1), (1, 2), (2, 1)]`
- ~6737 (tuple in a vec): `vec![SYNOPSIS_WHOLE_WORK]`

If the test module does not already have `SYNOPSIS_WHOLE_WORK` in scope, reference it as `super::SYNOPSIS_WHOLE_WORK` (consistent with the existing `super::whole_work_label` / `super::prepend_whole_work` calls in those tests).

- [ ] **Step 5: Reword the 3 doc comments in `app.rs`**

These are prose mentions of `(-2,0)` — update so the comment names the const:
- ~5596: `/// The fixed label for the whole-work synopsis position (SYNOPSIS_WHOLE_WORK), or None for`
- ~6246: `/// Put the whole-work synopsis key (SYNOPSIS_WHOLE_WORK) first when it exists, otherwise`
- ~6289–6291: update both `(-2,0)` mentions in that comment block to name `SYNOPSIS_WHOLE_WORK` (preserve the surrounding wording; only the `(-2,0)` tokens change).

- [ ] **Step 6: Swap the journal site in `journal.rs`**

~271, the band-match arm:
```rust
JournalBand::Work => ("work", crate::app::JOURNAL_WORK_DIV.0, crate::app::JOURNAL_WORK_DIV.1),
```
Leave every other `JournalBand::Work` arm in the file untouched — only this DB-write coordinate literal changes.

- [ ] **Step 7: Confirm no excluded site was touched**

Run: `git diff` and verify ZERO `(0, 0)` / `(-1, *)` literals changed, and that only the 9 intended sites + 2 const definitions + 3 doc comments differ. Run `rg -n "\(0, *0\)" src/app.rs` and confirm those lines are unchanged in the diff.

- [ ] **Step 8: Build**

Run: `cargo build`
Expected: `Finished`, no errors, no new warnings (the consts are used immediately, so no dead_code).

- [ ] **Step 9: Clippy**

Run: `cargo clippy`
Expected: no new warnings.

- [ ] **Step 10: Tests**

Run: `cargo test --bins`
Expected: same pass count as before (413 at last check), 0 failed. The `whole_work_label`/`prepend_whole_work` tests now exercise the const and are the regression guard.

- [ ] **Step 11: Commit**

```bash
git add src/app.rs src/input/actions/journal.rs
git commit -m "refactor(keys): name the whole-work/journal-work scene sentinels

Add SYNOPSIS_WHOLE_WORK=(-2,0) and JOURNAL_WORK_DIV=(-1,-1) pub(crate) consts
in app.rs; swap the 8 synopsis + 1 journal literal sites. Pure literal->const,
no behavior change. The overloaded (0,0) Prologue and Induction (-1,*) are
intentionally left as literals.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

## Verification (after the task)

- `cargo build` + `cargo clippy` clean, `cargo test --bins` green.
- Reviewer confirms: each swapped site is a genuine scene-key sentinel use; NO `(0,0)`/`(-1,*)` literal was changed; the const values match the prior literals exactly; the test arg-vs-tuple substitutions are correct.
- No cage pass needed — keys are numerically identical on disk and in memory; no runtime/visual surface change.
