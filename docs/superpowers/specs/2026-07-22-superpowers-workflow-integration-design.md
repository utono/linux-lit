# Superpowers Workflow Integration — Design

**Date:** 2026-07-22
**Amended:** 2026-07-22 — scope extended from linux-lit alone to the full
pipeline (whisper-transcript → litdb → linux-lit) after a fresh two-week
commit review of all three repos.
**Scope:** process rules — CLAUDE.md, memory-bank, and docs edits in each
repo. Three small code deliverables are *named* here (the Component 9
sweep, the Component 10 automation, the Component 11 default flip) but go
through their host repo's own plan before any code is written; everything
else is documentation-only.

## Context

The three repos form one pipeline: whisper-transcript produces word/line
timestamps, litdb imports and aligns them into lit.db, linux-lit reads
lit.db. Defects flow downstream (a bad timestamp becomes a reader sync
bug); diagnosis flows upstream. Review window: 2026-07-08..2026-07-22.

### linux-lit (672 non-merge commits, 122 merges)

The superpowers plugin already carries medium features well
(spec→plan→implement chains for vocab-surfaces, chat space loop, karaoke
axis) and branch finishing is habitual. The original three failure
signatures, sharpened by the fresh review:

1. **Mixed dirty trees.** The main checkout accumulated uncommitted hunks
   from three separate sessions, with ownership no longer derivable. Not
   witnessable in git after the fact; the generating condition — dozens of
   same-day branch-create-and-merge cycles — is.
2. **Conversation-only decision states.** Sharper than first recorded: the
   2026-07-16 keybind reshuffle was a five-commit oscillation in ~2.5
   hours (`1afb046f` → `f0d09f59` → `cc2101e8` → `f89e1318` → `8f12db33`)
   with no spec commit anywhere in the sequence.
3. **Drift-repair trickle.** Sharper: `docs/guides/keybind-surface-guide.md`
   received ONE commit all window while `keymap_config.rs` changed 33
   times and `keybinds_overlay.rs` 53 times. A fourth mirror — the stowed
   `keymap.json` — lives in tty-dotfiles, invisible to this repo's log.

New signatures from the fresh review:

4. **Test-fix asymmetry.** 171 `fix(...)` commits; roughly 3% touched
   `tests/`. Sync/seek fixes in the historically fragile subsystems
   (`1d31dc25`, `cee3af6c`) shipped with no test.
5. **Same-file thrash.** `actions/chat.rs` touched 116 times in two weeks;
   ~35 commits in one 21-hour chat-panel run, several correcting the
   immediately preceding commit; two reverts (`6a693bf1`, `5ddae1fd`)
   un-merged work that review would have caught pre-merge.
6. **Abandoned branches.** Three stale `origin/*` branches
   (`fix/ask-card-host`, `fix/inline-translation-clip`,
   `refactor/passages-act-scene-to-div`) neither merged nor deleted.

### litdb (110 non-merge commits, direct to master)

Feature discipline is genuinely strong and this design does NOT change it:
~6 spec→plan→implement chains this window, skills as the packaging unit
for new capability, pytest landing with the feature commit, plans amended
in the open. The failures are all on the data-defect side:

1. **Guard built mid-chain.** Five `fix(translations)` re-align commits
   across six works in ~24 hours (`6f01d52`, `36f72a2`, `a1b4eea`,
   `eac851f`, `e156c56`); the `troubleshoot-translations` skill landed in
   the middle of the chain (`5c0eccd`) and caught only the last two.
2. **Lessons instead of gates.** 14 `docs(wizard-*)` per-work "lesson"
   commits this window; the alignment gates (6.7/6.7b/6.8) grew advisory
   carve-outs only after live imports broke them.
3. **No regression corpus for the reader-blamed defect classes.** Nothing
   checks timestamp ordering, collapsed line_mapping rows, or dropped
   stage directions — the three classes linux-lit's docs blame on litdb.
   No fixes for them this window could mean no defects, or no checking.

### whisper-transcript (124 commits lifetime; 10 this window)

Already the model citizen: populated `docs/superpowers/{specs,plans}/`,
full memory bank, a frozen-reference fidelity-gate test built as a
regression oracle, and this window's 10 commits form a textbook
design→plan→test→implement→revert-with-documented-root-cause cycle.
Remaining gaps:

1. **The audit is manual.** `align-forced` writes `line_timestamps` with
   no automatic post-write check; `alignment-status` has monotonicity,
   gap, and speech-rate checks but runs only when remembered.
2. **Tribal rules unwritten.** "Prefer large-v3" and "never mix another
   recording's JSON" exist only in the reader project's memory; this
   repo's own default is `medium.en`, inconsistent with the preference.
3. **Stale root `PLAN.md`** still references the removed
   `bin/align-timestamps`.
4. **Known trouble spot.** `bin/align-forced` (31 touches lifetime) plus
   `word_matching.py` are where fix-chains happen (drift recovery, anchor
   confirmation, stale column names).

Each rule below states its exemption inline so it does not rot into
ceremony.

## Part I — linux-lit (Components 1–6)

### Component 1 — Spec threshold

Invoke `superpowers:brainstorming` and write a spec (a few sentences is
enough) before any change that:

- reshuffles **two or more** reader-surface keybinds in one change, or
- changes a mode, axis, or per-class default, or
- spans two or more surfaces (main card + overlay + chat), or
- alters config schema.

**Exempt:** single keybind moves, single-file bug fixes, cosmetic tweaks.

**Retrospective signal:** more than ~3 commits to the same file within 24
hours means the change was above this threshold and should have started
with a spec — note it and spec the follow-up work.

### Component 2 — Worktrees for session-spanning work

Worktrees are not only for concurrent sessions. Any branch **expected to
span sessions** — or likely to leave the tree dirty at session end —
starts in a worktree via `superpowers:using-git-worktrees`
(`~/utono/linux-lit-wt/<branch>`).

**Session pre-flight:** at session start, run `git worktree list` and
`git status` in the main checkout; a dirty main checkout is the first
thing to resolve, not work around.

**Branch hygiene:** when abandoning a branch, delete it or record it in
`docs/to-do/to-do.md` — never leave a third state on origin.

**Exempt:** quick fixes branched, committed, and merged within one
session.

**Invariant bought:** the main checkout ends every session clean on
master; the mixed-hunk state cannot recur.

### Component 3 — Pre-merge review gate

One trigger only: **a branch whose change met the Component 1 spec
threshold gets `superpowers:requesting-code-review` before merge.** Small
fix branches merge as today, unreviewed. When binds changed, the
`update-cairo-keybinds-overlay` three-pass cross-reference remains the
keybind-specific instrument inside that review, and it enumerates all
**four** mirrors mechanically: `keymap_config.rs`, the
`ui/*_keybinds_overlay.rs` legends, the stowed `keymap.json` (lives in
tty-dotfiles — this repo's log cannot witness its drift), and
`docs/guides/keybind-surface-guide.md`.

### Component 4 — TDD default for sync/pagination/clipping

Bug fixes in playback sync (including seek), pagination, or clipping
start with a **failing** headless repro (extend `test-playback-sync`, the
nav-fuzz, or the clipping e2e) per
`superpowers:test-driven-development`; then fix; then green. Evidence
this window: 171 fixes, ~3% with tests. This is a strong default, not a
hard gate: when the state is genuinely live-only and cannot be automated,
say so explicitly in the commit message and proceed.

### Component 5 — Troubleshooting ledgers beyond clipping

Promote the clip-prevention.md mandate to a pattern: each recurring-bug
domain keeps a frequency-ordered ledger in `docs/troubleshooting/`
(clipping and page-turning exist; playback-sync is the next candidate).
Trigger, borrowed from litdb's wizard lessons: if diagnosis took more
than one session **or** the root cause contradicted the first hypothesis,
append the failure mode — tell, root cause, fix — in the same change.

### Component 6 — Batch-day playbook

A short CLAUDE.md subsection (not a skill; nothing to automate yet).
When a queue of small independent polish items is planned (cf. the
~40-branch chat-panel day of 2026-07-20):

1. Write one plan via `superpowers:writing-plans` with explicitly
   independent tasks.
2. Execute via `superpowers:subagent-driven-development`, one worktree
   per task.
3. Merge serially from the main checkout.
4. Run the e2e suite once at the end, not per branch.

May graduate to a skill if it ever earns scripts.

## Part II — litdb (Components 7–9)

Feature workflow (spec→plan→skill, tests with the feature, direct small
commits to master) is unchanged — it is the pattern the other repos
borrow. These rules target only the data-defect side.

### Component 7 — Guard before the second fix

When a data-defect fix (`fix(translations)`, `fix(alignment)`,
timestamp or line_mapping repair) plausibly belongs to a **class** —
the same root cause could sit in other works — the FIRST fix ships with
the detector (SQL invariant, pytest, or gate extension) that finds the
other instances, and the detector runs corpus-wide before work-by-work
fixing continues. Never build the detector mid-chain: turned-under lines
were foreseeably going to hit more than one work.

**Exempt:** genuinely single-work one-offs.

### Component 8 — Lessons promote to gates

A wizard "lesson" entry is a record, not a fix. The **third** lesson
sharing a root-cause category converts that category into an automated
check in `check_alignment_gates.py` (or a `validate.sh` step) in the same
change — advisory mode first is fine, but the gate must exist. The
6.7/6.7b/6.8 gate evolution shows this works when actually done.

### Component 9 — Reader-blamed defect sweep

A validation sweep for the three defect classes the reader repeatedly
root-causes to lit.db: per-edition timestamp monotonicity, collapsed or
duplicate line_mapping rows, and dropped stage directions against source.
Process rule now: run it at the end of every import-wizard session, not
only when a reader bug points here. The sweep script itself is an
automation deliverable that goes through litdb's own plan.

## Part III — whisper-transcript (Components 10–12)

The repo already follows the spec→plan→test discipline; nothing is
re-scaffolded. These rules close the remaining gaps.

### Component 10 — Alignment gate after every write

Process rule now: every `align-forced` run that writes `line_timestamps`
is followed by the `alignment-status` monotonicity and coverage checks
**before** the recording is used downstream; a FAIL blocks the litdb
import. Candidate automation — `align-forced` invoking the check itself
post-write — goes through this repo's own spec/plan.

### Component 11 — Codify the tribal recording rules

Write into this repo's CLAUDE.md and `CLAUDE-troubleshooting.md`:

- the model is pinned per work, and `large-v3` is the standard — the
  current `medium.en` default in `bin/transcribe` contradicts the
  standing preference and gets flipped to `large-v3` (a one-line change,
  included in this repo's plan), with any deliberate `medium.en` use
  documented per work;
- never mix timestamps from another recording's JSON.

Also retire the stale root `PLAN.md` (fold anything still live into
`docs/superpowers/plans/`, then delete it).

### Component 12 — Fidelity gate is the TDD pattern; fix-chains escalate

Any alignment-algorithm change (`word_matching.py`, `aligner.py`, the
matching logic in `align-forced`) starts from the frozen-reference
fidelity test: extend the oracle first, watch it fail, then change the
algorithm — the pattern already proved itself once and becomes the
documented default. A **third** fix to the same alignment file within a
week triggers `superpowers:systematic-debugging` before further patches.

## Part IV — Cross-project (Component 13)

### Component 13 — Upstream root-cause routing

When a bug observed in one repo root-causes upstream (reader sync bug →
lit.db data → whisper-transcript output), the fix **and its regression
guard** land in the upstream repo; the downstream repo gets only a
ledger entry linking to the upstream commit. Never patch around an
upstream defect downstream. This codifies existing practice ("fixed in
lit.db via the wizard, not in linux-lit code") as a rule binding all
three repos.

## Implementation

Documentation edits only, per repo; each edit commit references this
spec's path:

- **linux-lit `CLAUDE.md`** — Components 1, 3 in the AI-guidance area;
  Component 2 in the worktree section; Component 4 in Testing;
  Component 5 in the clipping/troubleshooting area; Component 6 as a new
  subsection; Component 13 in the External Data section.
- **litdb `CLAUDE.md`** — Components 7, 8, 9 (process rule) in its
  wizard/validation guidance; Component 13 alongside. The Component 9
  sweep script goes through litdb's own plan.
- **whisper-transcript `CLAUDE.md` + `CLAUDE-troubleshooting.md`** —
  Components 10, 11, 12, 13; retire root `PLAN.md`. The Component 10
  automation goes through this repo's own plan.

## Success criteria

Over the following two weeks of history:

- **linux-lit:** no multi-session mixed dirty tree in the main checkout;
  keybind reshuffles of 2+ binds have a spec commit preceding them;
  drift-repair commits on spec'd branches drop; sync/pagination/seek fix
  commits either follow a red test or carry the explicit live-only note;
  no new abandoned `origin/*` branches.
- **litdb:** the next data-defect class produces detector-first history
  (detector commit precedes or accompanies the first fix, no mid-chain
  skill); no root-cause category reaches a fourth narrative lesson
  without a gate; the Component 9 sweep exists and runs per import
  session.
- **whisper-transcript:** no `line_timestamps` write reaches a litdb
  import without a recorded `alignment-status` pass; the model/JSON
  rules appear in the repo's own docs; root `PLAN.md` is gone.

## Tripwires

- **linux-lit:** single keybind moves rely on the existing lockstep rule
  + skills (no spec, no review). If legend/keymap drift-repair commits
  keep appearing for single-bind changes, re-add keybind changes to the
  Component 3 review gate.
- **litdb:** if a 3+-commit same-class fix chain recurs without a
  preceding detector, harden Component 7 from a default into a blocking
  rule in the wizard skills themselves.
- **whisper-transcript:** if a bad write still reaches litdb despite the
  manual gate, promote the Component 10 automation from candidate to
  scheduled work.
