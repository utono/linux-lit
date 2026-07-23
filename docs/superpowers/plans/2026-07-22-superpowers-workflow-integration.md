# Superpowers Workflow Integration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the approved workflow rules from
`docs/superpowers/specs/2026-07-22-superpowers-workflow-integration-design.md`
as CLAUDE.md/docs edits in linux-lit, litdb, and whisper-transcript, plus
the one-line whisper model-default flip.

**Architecture:** Documentation-only edits per repo (Components 1–13),
committed directly on each repo's master (matching each repo's practice
for docs). The two larger code deliverables — the litdb defect sweep
(Component 9) and the align-forced post-write automation (Component 10) —
are NOT built here; the rules reference them as pending their own
spec/plan. The Component 11 default flip IS included (spec sizes it as a
one-line change belonging to this plan).

**Tech Stack:** Markdown, git, one Python argparse default.

## Global Constraints

- Preserve the deliberate scope cuts verbatim: single keybind moves are
  EXEMPT from spec/review; there is exactly ONE review gate (spec
  threshold); `docs/guides/keybind-surface-guide.md` is on-request only.
- Markdown rules: no box-drawing diagrams; no NEW tables (editing rows
  of whisper-transcript's existing model table is fine); commands in
  fenced code blocks; wrap near 80 columns.
- Every commit message body references the spec path:
  `docs/superpowers/specs/2026-07-22-superpowers-workflow-integration-design.md`
  (for litdb/whisper commits, prefix with `~/utono/linux-lit/`).
- Do NOT update any repo's `CLAUDE-activeContext.md` after commits.
- Each task's edits happen inside that task's repo; `cd` there before
  git commands. All three trees must be clean before starting a task
  (`git status --short` → empty).

---

### Task 1: linux-lit CLAUDE.md — Components 1–6 and 13

**Files:**
- Modify: `~/utono/linux-lit/CLAUDE.md` (five edit sites, listed below)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: the linux-lit rule text later tasks' commit messages may cite;
  no code interfaces.

- [ ] **Step 1: Insert the new Workflow Rules section**

In `~/utono/linux-lit/CLAUDE.md`, insert the following new section
immediately BEFORE the line `## Parallel Claude Code Sessions (git
worktrees)`:

```markdown
## Workflow Rules (superpowers)

Spec: `docs/superpowers/specs/2026-07-22-superpowers-workflow-integration-design.md`.

- **Spec threshold.** Invoke `superpowers:brainstorming` and write a
  spec (a few sentences is enough) BEFORE any change that: reshuffles
  two or more reader-surface keybinds in one change; changes a mode,
  axis, or per-class default; spans two or more surfaces (main card +
  overlay + chat); or alters config schema. EXEMPT: single keybind
  moves, single-file bug fixes, cosmetic tweaks. Retrospective signal:
  more than ~3 commits to the same file within 24 hours means the
  change was above this threshold — note it and spec the follow-up.
- **Pre-merge review gate (one trigger only).** A branch whose change
  met the spec threshold gets `superpowers:requesting-code-review`
  before merge. Small fix branches merge as today, unreviewed. When
  binds changed, the `update-cairo-keybinds-overlay` three-pass
  cross-reference runs inside that review and enumerates all three
  lockstep mirrors: `keymap_config.rs`, the `ui/*_keybinds_overlay.rs`
  legends, and the stowed `keymap.json` (in tty-dotfiles).
  `keybind-surface-guide.md` is NOT in the set (on-request only).
- **Batch-day playbook.** When a queue of small independent polish
  items is planned: (1) one plan via `superpowers:writing-plans` with
  explicitly independent tasks; (2) execute via
  `superpowers:subagent-driven-development`, one worktree per task;
  (3) merge serially from the main checkout; (4) run the e2e suite
  once at the end, not per branch.
```

- [ ] **Step 2: Extend the worktree section (Component 2)**

In the section `## Parallel Claude Code Sessions (git worktrees)`, insert
the following two paragraphs immediately AFTER the fenced
`git worktree add` code block (before the bullet list that starts
`- Each worktree builds its own`):

```markdown
Worktrees are not only for concurrent sessions: any branch **expected
to span sessions** — or likely to leave the tree dirty at session end —
starts in a worktree via `superpowers:using-git-worktrees`. EXEMPT:
quick fixes branched, committed, and merged within one session. The
invariant bought: the main checkout ends every session clean on master.

Session pre-flight: run `git worktree list` and `git status` in this
main checkout at session start; a dirty main checkout is the first
thing to resolve, not work around. Branch hygiene: when abandoning a
branch, delete it or record it in `docs/to-do/to-do.md` — never leave a
third state on origin.
```

- [ ] **Step 3: Add the TDD default to the Testing section (Component 4)**

In the `## Testing` section, append this paragraph at the end of the
section (after the existing `cargo test` / `cargo clippy` code block),
before the `## Headless Verification` heading:

```markdown
**TDD default for sync/pagination/clipping (seek included):** bug fixes
in these subsystems start with a FAILING headless repro (extend
`test-playback-sync`, the nav-fuzz, or the clipping e2e) per
`superpowers:test-driven-development`; then fix; then green. Strong
default, not a hard gate: when the state is genuinely live-only and
cannot be automated, say so explicitly in the commit message and
proceed.
```

- [ ] **Step 4: Add the Troubleshooting Ledgers section (Component 5)**

Insert the following new section immediately AFTER the end of the
`## Clipping Bugs — read clip-prevention.md FIRST` section (before
`## Build & Run`):

```markdown
## Troubleshooting Ledgers

The clip-prevention pattern generalizes: each recurring-bug domain
keeps a frequency-ordered ledger in `docs/troubleshooting/` (clipping
and page-turning exist; playback-sync is the next candidate). Trigger:
if diagnosis took more than one session OR the root cause contradicted
the first hypothesis, append the failure mode — tell, root cause, fix —
in the same change. This is required, not optional.
```

- [ ] **Step 5: Add upstream routing to External Data & Config (Component 13)**

In the `## External Data & Config` section, append this bullet at the
end of the bullet list:

```markdown
- **Upstream root-cause routing**: when a reader bug root-causes to
  lit.db data (litdb) or timestamp output (whisper-transcript), the fix
  and its regression guard land in the UPSTREAM repo; this repo gets
  only a troubleshooting-ledger entry linking to the upstream commit.
  Never patch around an upstream defect in reader code.
```

- [ ] **Step 6: Verify all five edits landed**

```bash
cd ~/utono/linux-lit
rg -c 'Workflow Rules \(superpowers\)|Troubleshooting Ledgers|TDD default for sync/pagination|Upstream root-cause routing|Session pre-flight' CLAUDE.md
```

Expected: 5 total matches across those patterns (one each).

- [ ] **Step 7: Commit**

```bash
cd ~/utono/linux-lit
git add CLAUDE.md
git commit -m "docs: superpowers workflow rules in CLAUDE.md (spec components 1-6, 13)

Per docs/superpowers/specs/2026-07-22-superpowers-workflow-integration-design.md"
```

---

### Task 2: litdb CLAUDE.md — Components 7–9 and 13

**Files:**
- Modify: `~/utono/litdb/CLAUDE.md` (one insertion)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: nothing later tasks depend on.

- [ ] **Step 1: Insert the Data-Defect Workflow section**

In `~/utono/litdb/CLAUDE.md`, insert the following new section
immediately BEFORE the line `## Audible Filenames`:

```markdown
## Data-Defect Workflow

Spec: `~/utono/linux-lit/docs/superpowers/specs/2026-07-22-superpowers-workflow-integration-design.md`
(Components 7–9, 13). The feature workflow (spec→plan→skill, tests with
the feature) is unchanged — these rules cover data defects only.

- **Guard before the second fix.** When a data-defect fix
  (`fix(translations)`, `fix(alignment)`, timestamp or line_mapping
  repair) plausibly belongs to a CLASS — the same root cause could sit
  in other works — the FIRST fix ships with the detector (SQL
  invariant, pytest, or gate extension) that finds the other
  instances, and the detector runs corpus-wide before work-by-work
  fixing continues. Never build the detector mid-chain. EXEMPT:
  genuinely single-work one-offs.
- **Lessons promote to gates.** A wizard "lesson" entry is a record,
  not a fix. The THIRD lesson sharing a root-cause category converts
  that category into an automated check in `check_alignment_gates.py`
  (or a `validate.sh` step) in the same change — advisory mode first is
  fine, but the gate must exist.
- **Reader-blamed defect sweep.** At the end of every import-wizard
  session, run the validation sweep for the three defect classes the
  reader repeatedly root-causes to lit.db: per-edition timestamp
  monotonicity, collapsed/duplicate line_mapping rows, and dropped
  stage directions vs source. The dedicated sweep script is pending its
  own spec/plan; until it exists, run the closest current checks:

  ```bash
  ~/utono/litdb/scripts/validate-db.sh
  python ~/utono/litdb/scripts/check_alignment_gates.py --work <ABBREV>
  ```

- **Upstream root-cause routing.** When a defect diagnosed here
  root-causes to whisper-transcript output, the fix and its regression
  guard land THERE; this repo gets the ledger entry linking to it. The
  same applies downstream: reader bugs root-caused to lit.db data are
  fixed here (wizard/scripts), never patched around in linux-lit.
```

Note: before committing, verify the `check_alignment_gates.py` flag
spelling — run
`python ~/utono/litdb/scripts/check_alignment_gates.py --help` and, if
its per-work flag differs from `--work`, correct the fenced example to
match the real interface.

- [ ] **Step 2: Verify**

```bash
cd ~/utono/litdb
rg -c 'Data-Defect Workflow|Guard before the second fix|Lessons promote to gates|Reader-blamed defect sweep' CLAUDE.md
```

Expected: 4 total matches (one each). Also confirm the section sits
between `## Database` and `## Audible Filenames`:

```bash
rg -n '^## (Database|Data-Defect Workflow|Audible Filenames)' ~/utono/litdb/CLAUDE.md
```

Expected: the three headings in that order.

- [ ] **Step 3: Commit**

```bash
cd ~/utono/litdb
git add CLAUDE.md
git commit -m "docs: data-defect workflow rules (spec components 7-9, 13)

Per ~/utono/linux-lit/docs/superpowers/specs/2026-07-22-superpowers-workflow-integration-design.md"
```

---

### Task 3: whisper-transcript CLAUDE.md + troubleshooting — Components 10, 12, 13 and the Component 11 rules text

**Files:**
- Modify: `~/utono/whisper-transcript/CLAUDE.md` (one insertion)
- Modify: `~/utono/whisper-transcript/CLAUDE-troubleshooting.md` (append)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: the "Recording rules" troubleshooting section that Task 4's
  model-table text points to ("see CLAUDE-troubleshooting.md").

- [ ] **Step 1: Insert the Workflow Rules section into CLAUDE.md**

In `~/utono/whisper-transcript/CLAUDE.md`, insert the following new
section immediately BEFORE the line `## File Operations`:

```markdown
## Workflow Rules

Spec: `~/utono/linux-lit/docs/superpowers/specs/2026-07-22-superpowers-workflow-integration-design.md`
(Components 10–13).

- **Alignment gate after every write.** Every `align-forced` run that
  writes `line_timestamps` is followed by
  `./bin/alignment-status --work ABBREV --status` (monotonicity +
  coverage) BEFORE the recording is used downstream; a FAIL blocks the
  litdb import until resolved. (Candidate automation — align-forced
  invoking the check itself post-write — goes through its own
  spec/plan.)
- **Fidelity gate is the TDD pattern.** Any alignment-algorithm change
  (`word_matching.py`, `aligner.py`, the matching logic in
  `align-forced`) starts from the frozen-reference fidelity test
  (`tests/test_match_fidelity.py`): extend the oracle first, watch it
  fail, then change the algorithm.
- **Fix-chains escalate.** A third fix to the same alignment file
  within a week triggers `superpowers:systematic-debugging` before
  further patches.
- **Upstream root-cause routing.** When a litdb or linux-lit bug
  root-causes to output produced here, the fix and its regression
  guard land HERE; downstream repos get only a ledger entry linking to
  the commit.
```

- [ ] **Step 2: Append the Recording rules to CLAUDE-troubleshooting.md**

Append the following section at the END of
`~/utono/whisper-transcript/CLAUDE-troubleshooting.md`:

```markdown
## Recording rules (codified 2026-07-22)

- **Never mix timestamps from another recording's JSON.** Every
  alignment consumes the transcript generated from the exact media
  file being aligned; a same-title different-recording JSON produces
  plausible-looking but corrupt timestamps (the classic downstream
  linux-lit sync-jump signature).
- **The model is pinned per work.** `large-v3` is the production
  standard; if a work was aligned with a different model, use that
  same model for incremental passes on that work, or redo the whole
  alignment at `large-v3` — never blend models within one work's
  timestamps.
```

- [ ] **Step 3: Verify**

```bash
cd ~/utono/whisper-transcript
rg -c 'Workflow Rules|Alignment gate after every write' CLAUDE.md
rg -c 'Recording rules \(codified 2026-07-22\)' CLAUDE-troubleshooting.md
```

Expected: 2 matches in CLAUDE.md (heading + bullet), 1 in
CLAUDE-troubleshooting.md. Also sanity-check the fidelity test path
referenced by the new rule exists:

```bash
ls ~/utono/whisper-transcript/tests/test_match_fidelity.py
```

Expected: the file exists (if the filename differs, fix the rule text
to the real filename before committing).

- [ ] **Step 4: Commit**

```bash
cd ~/utono/whisper-transcript
git add CLAUDE.md CLAUDE-troubleshooting.md
git commit -m "docs: workflow rules — alignment gate, fidelity-gate TDD, recording rules (spec components 10-13)

Per ~/utono/linux-lit/docs/superpowers/specs/2026-07-22-superpowers-workflow-integration-design.md"
```

---

### Task 4: whisper-transcript — flip the transcription default to large-v3 (Component 11)

**Files:**
- Modify: `~/utono/whisper-transcript/bin/transcribe` (argparse default)
- Modify: `~/utono/whisper-transcript/CLAUDE.md` (Whisper Models section)
- Modify: any `.claude/skills/*/SKILL.md` claiming a `medium.en` default

**Interfaces:**
- Consumes: Task 3's "Recording rules" troubleshooting section (the new
  model paragraph points readers at it).
- Produces: nothing later tasks depend on.

- [ ] **Step 1: Find the current default and any model-name validation**

```bash
cd ~/utono/whisper-transcript
rg -n "medium\.en" bin/transcribe .claude/skills/ CLAUDE.md
rg -n "choices" bin/transcribe
```

Note every hit. If `bin/transcribe` validates `--model` against a
`choices=[...]` list that lacks `large-v3`, add `large-v3` to that list
in Step 2.

- [ ] **Step 2: Flip the argparse default**

In `bin/transcribe`, change the `--model` argument's
`default="medium.en"` to `default="large-v3"` (and add `large-v3` to
the choices list if one exists, per Step 1). Do not change anything
else.

- [ ] **Step 3: Update the Whisper Models section in CLAUDE.md**

In `~/utono/whisper-transcript/CLAUDE.md`, in the `## Whisper Models`
table: change the `medium.en` row's use-case text from
`Default — English, best timestamps` to `English, fast full-length
passes`, and add this row after it:

```markdown
| large-v3 | Slowest | Default — production alignment    |
```

Then replace the paragraph beginning `**Default**: \`medium.en\` model.`
with:

```markdown
**Default**: `large-v3` — the standard for any transcript whose
timestamps will enter lit.db (2026-07-22 decision; the standing
"prefer the larger model" rule, now encoded). `medium.en` remains
acceptable for previews and experiments; any deliberate production use
of a smaller model is documented per work. The model is pinned per
work — see the Recording rules in `CLAUDE-troubleshooting.md`. The
`.en` variants are English-only; all models except `large`/`large-v3`
have `.en` variants.
```

- [ ] **Step 4: Update skill docs that claim the old default**

For each `.claude/skills/` hit from Step 1 that states the default is
`medium.en` (e.g. "All three alignment skills use `medium.en` by
default"), update it to name `large-v3` as the default. Change only
default-claims, not examples that explicitly pass `--model`.

- [ ] **Step 5: Verify**

```bash
cd ~/utono/whisper-transcript
./bin/transcribe --help | rg -i 'model'
rg -n "medium\.en" CLAUDE.md .claude/skills/ bin/transcribe
```

Expected: `--help` shows `large-v3` as the default; remaining
`medium.en` mentions are only non-default contexts (model table row,
"acceptable for previews" sentence, explicit `--model` examples).

- [ ] **Step 6: Commit**

```bash
cd ~/utono/whisper-transcript
git add bin/transcribe CLAUDE.md .claude/skills/
git commit -m "feat: default transcription model is large-v3 (spec component 11)

Per ~/utono/linux-lit/docs/superpowers/specs/2026-07-22-superpowers-workflow-integration-design.md"
```

---

### Task 5: whisper-transcript — retire root PLAN.md

**Files:**
- Move: `~/utono/whisper-transcript/PLAN.md` →
  `~/utono/whisper-transcript/docs/superpowers/plans/2026-02-21-medium-model-sentence-times.md`

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: nothing later tasks depend on.

- [ ] **Step 1: Check for references to PLAN.md**

```bash
cd ~/utono/whisper-transcript
rg -n 'PLAN\.md' --hidden -g '!.git' .
```

Expected: no hits outside PLAN.md itself. If any file references
`PLAN.md`, rewrite that reference to the new path in the same commit.

- [ ] **Step 2: Move with history**

```bash
cd ~/utono/whisper-transcript
git mv PLAN.md docs/superpowers/plans/2026-02-21-medium-model-sentence-times.md
```

(The date matches its sibling design doc
`docs/superpowers/specs/2026-02-21-medium-model-sentence-times-design.md`.)

- [ ] **Step 3: Prepend a status note**

At the very top of the moved file (above the `# Medium vs Small…` title),
insert:

```markdown
> **Status (2026-07-22):** completed historical plan, moved from root
> `PLAN.md`. `bin/align-timestamps` has since been consolidated into
> `bin/align-forced`; command examples below are period-accurate but no
> longer runnable as written.

```

- [ ] **Step 4: Verify and commit**

```bash
cd ~/utono/whisper-transcript
ls PLAN.md 2>&1
git status --short
```

Expected: `ls` reports "No such file or directory"; status shows the
rename plus the edit. Then:

```bash
cd ~/utono/whisper-transcript
git add -A
git commit -m "docs: retire root PLAN.md into docs/superpowers/plans/ (spec component 11)

Per ~/utono/linux-lit/docs/superpowers/specs/2026-07-22-superpowers-workflow-integration-design.md"
```

---

### Task 6: Cross-repo verification sweep

**Files:** none modified — verification only.

**Interfaces:**
- Consumes: all prior tasks' committed edits.
- Produces: the final completion report to the user.

- [ ] **Step 1: Confirm every component landed**

```bash
rg -c 'Workflow Rules \(superpowers\)' ~/utono/linux-lit/CLAUDE.md
rg -c 'Data-Defect Workflow' ~/utono/litdb/CLAUDE.md
rg -c 'Workflow Rules' ~/utono/whisper-transcript/CLAUDE.md
rg -c 'Recording rules' ~/utono/whisper-transcript/CLAUDE-troubleshooting.md
ls ~/utono/whisper-transcript/PLAN.md 2>&1
```

Expected: first four commands each report ≥1; the `ls` fails.

- [ ] **Step 2: Confirm all three trees are clean and count the commits**

```bash
for r in linux-lit litdb whisper-transcript; do
  echo "== $r"; git -C ~/utono/$r status --short
  git -C ~/utono/$r log --oneline -3
done
```

Expected: empty status for all three; the new docs/feat commits at the
top of each log.

- [ ] **Step 3: Report**

Report per-repo commit hashes and note the two deferred code
deliverables (litdb defect sweep, align-forced post-write automation)
each awaiting its own spec/plan in its host repo. Per the linux-lit
testing convention, this change has no runnable behavior beyond
`./bin/transcribe --help` (verified in Task 4) — no e2e run needed;
offer the user a quick read-through of the three CLAUDE.md diffs as the
manual check.
