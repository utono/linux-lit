# Superpowers Workflow Integration — Design

**Date:** 2026-07-22
**Scope:** linux-lit process rules (CLAUDE.md edits only; no code changes).
litdb is unaffected — its spec/plan/skill discipline is the pattern being
borrowed, not the target.

## Context

A review of the two weeks ending 2026-07-22 (671 non-merge commits, 122
`--no-ff` merges in linux-lit) found the superpowers plugin already carrying
medium features well (spec→plan→implement chains for vocab-surfaces, chat
space loop, karaoke axis) and branch finishing done habitually. Three
recurring failure signatures were not covered by any rule:

1. **Mixed dirty trees.** The main checkout accumulated uncommitted hunks
   from three separate sessions, with ownership no longer derivable.
2. **Conversation-only decision states.** A reader keybind reshuffle went
   through two swaps in one day; the intermediate states exist only in a
   chat transcript.
3. **Drift-repair trickle.** Hand-maintained mirrors (Ctrl+/ overlay,
   keymap.json, per-overlay legends, surface guide) generate a steady
   stream of `fix stale comment` / legend-sync commits; overlay audits are
   run as ad-hoc subagent sweeps rather than a named review step.

This design adds six small rules to linux-lit's CLAUDE.md that route the
existing superpowers skills at these failures. Each rule states its
exemption inline so it does not rot into ceremony.

## Component 1 — Spec threshold

Invoke `superpowers:brainstorming` and write a spec (a few sentences is
enough) before any change that:

- reshuffles **two or more** reader-surface keybinds in one change, or
- changes a mode, axis, or per-class default, or
- spans two or more surfaces (main card + overlay + chat), or
- alters config schema.

**Exempt:** single keybind moves, single-file bug fixes, cosmetic tweaks.

## Component 2 — Worktrees for session-spanning work

Extend the existing worktree section: worktrees are not only for concurrent
sessions. Any branch **expected to span sessions** — or likely to leave the
tree dirty at session end — starts in a worktree via
`superpowers:using-git-worktrees` (`~/utono/linux-lit-wt/<branch>`).

**Exempt:** quick fixes branched, committed, and merged within one session.

**Invariant bought:** the main checkout ends every session clean on master;
the mixed-hunk state cannot recur.

## Component 3 — Pre-merge review gate

One trigger only: **a branch whose change met the Component 1 spec
threshold gets `superpowers:requesting-code-review` before merge.** Small
fix branches merge as today, unreviewed. The
`update-cairo-keybinds-overlay` three-pass cross-reference remains the
keybind-specific instrument inside that review when binds changed.

## Component 4 — TDD default for sync/pagination/clipping

Bug fixes in playback sync, pagination, or clipping start with a
**failing** headless repro (extend `test-playback-sync`, the nav-fuzz, or
the clipping e2e) per `superpowers:test-driven-development`; then fix; then
green. This is a strong default, not a hard gate: when the state is
genuinely live-only and cannot be automated, say so explicitly in the
commit message and proceed.

## Component 5 — Troubleshooting ledgers beyond clipping

Promote the clip-prevention.md mandate to a pattern: each recurring-bug
domain keeps a frequency-ordered ledger in `docs/troubleshooting/`
(clipping and page-turning exist; playback-sync is the next candidate).
Trigger, borrowed from litdb's wizard lessons: if diagnosis took more than
one session **or** the root cause contradicted the first hypothesis, append
the failure mode — tell, root cause, fix — in the same change.

## Component 6 — Batch-day playbook

A short CLAUDE.md subsection (not a skill; nothing to automate yet). When a
queue of small independent polish items is planned (cf. the ~40-branch
chat-panel day of 2026-07-20):

1. Write one plan via `superpowers:writing-plans` with explicitly
   independent tasks.
2. Execute via `superpowers:subagent-driven-development`, one worktree per
   task.
3. Merge serially from the main checkout.
4. Run the e2e suite once at the end, not per branch.

May graduate to a skill if it ever earns scripts.

## Implementation

All six components are additions/edits to `~/utono/linux-lit/CLAUDE.md`
(sections: AI-guidance area for 1 and 3, the worktree section for 2, the
Testing section for 4, the Clipping/troubleshooting area for 5, a new
subsection for 6). No code, no new files beyond this spec and the plan.

## Success criteria

Over the following two weeks of history: no multi-session mixed dirty tree
in the main checkout; keybind reshuffles of 2+ binds have a spec commit
preceding them; drift-repair commits on spec'd branches drop (caught in the
pre-merge review instead); sync/pagination fix commits either follow a red
test or carry the explicit live-only note.

**Tripwire:** single keybind moves rely on the existing lockstep rule +
skills (no spec, no review). If legend/keymap drift-repair commits keep
appearing for single-bind changes, re-add keybind changes to the
Component 3 review gate.
