# Branches and Worktrees — a Beginner's Guide

Why `git branch` in the main checkout lists a branch you created somewhere
else, what the `*` and `+` markers mean, and how to decide where new feature
work should live. Written around this repo's conventions (worktrees under
`~/utono/linux-lit-wt/`, merge back to `master` locally).

## Branch vs worktree — what kind of thing is each?

They are different kinds of things entirely: a branch is a *name*, a
worktree is a *place*.

A **branch** is a named pointer to a commit — pure bookkeeping data. On
disk it is one tiny file inside `.git` containing a 40-character commit id.
It answers "*what* line of work is this?" A branch has no files of its own;
it is a label on a point in history. Creating one is instant and free, and
a repository can have hundreds. A branch can exist without being checked
out anywhere — a dormant branch is just a saved bookmark waiting to be
picked up.

A **worktree** is a directory of real files on disk — an actual checkout
you can `cd` into, open in an editor, and build in. It answers "*where* am
I working?" Every git repository has at least one (the main checkout);
`git worktree add` creates extras. A worktree is not free: it holds a full
copy of the project's files (and here, its own `target/` build directory).

The relationship between them:

- A worktree always has exactly **one** branch checked out at a time (or a
  bare commit, "detached HEAD").
- A branch is checked out in **at most one** worktree at a time — git
  enforces this, which is what the `+` marker below is about.
- One worktree can switch between many branches *over time*
  (`git switch`); multiple worktrees let many branches be checked out
  *simultaneously*, each in its own directory.

Deleting them means different things, too. `git branch -d` removes the
label — the directory you were working in is untouched. `git worktree
remove` deletes the directory — the branch survives, back to being a
dormant bookmark, and its commits are safe in the shared repository.

If an analogy helps: the repository is a book's complete manuscript
archive, a branch is a labeled bookmark naming one version of the text, and
a worktree is a desk with that version physically laid out for editing.
Adding a second desk (worktree) lets you edit two versions side by side,
but both desks file every change back into the same one archive.

## Branches belong to the repository, not to a directory

A git *repository* is the hidden `.git` directory: all commits, history, and
the list of branches live there. The files you see and edit next to it are
just one *working directory* ("checkout") — a view of one branch at a time.

A *branch* is nothing more than a named pointer to a commit, stored inside
`.git`. It is repo-level state. Whichever directory you run `git branch`
from, you are reading the same single list.

That is why this happens:

```bash
cd ~/utono/linux-lit     # main checkout, sitting on master
git branch
```

```text
+ feat/backslash-overlay-cycle
* master
```

The branch `feat/backslash-overlay-cycle` was created for a worktree in a
different directory, yet it shows up here — because "here" and "there" share
one repository.

## How a branch gets "created somewhere else"

Creating a branch is not a special event — it happens as a side effect of a
few everyday commands:

```bash
git branch <name>            # create it, stay where you are
git switch -c <name>         # create it AND check it out here
git worktree add <dir> -b <name> <start>   # create it, checked out in <dir>
```

All three do the same tiny thing under the hood: write one small file,
`.git/refs/heads/<name>`, containing the commit the branch points at. That
file lives in the **single shared** `.git` — there is no per-directory copy.

"Somewhere else" therefore means: the command that wrote that file was run
in (or for) a different working directory. In this repo's case it was

```bash
git worktree add ~/utono/linux-lit-wt/feat-backslash-overlay-cycle \
  -b feat/backslash-overlay-cycle master
```

which created the branch and immediately checked it out in the new worktree
directory. Equally, another Claude Code session working *inside* an
existing worktree can run `git switch -c` there; the new ref lands in the
same shared `.git`, so the main checkout's `git branch` lists it the
instant it exists. There is no syncing step and no delay — "creating a
branch in a worktree" and "creating a branch in the repository" are the
same act, because the worktree has no branch storage of its own.

## Reading the markers: `*` and `+`

- `*` — the branch checked out in the directory you ran the command from
  (here, `master`).
- `+` — a branch checked out in **another worktree** (git has shown this
  since v2.23).

`+` also carries a protection: git refuses to check that branch out here
while the worktree holds it:

```bash
git checkout feat/backslash-overlay-cycle
# fatal: 'feat/backslash-overlay-cycle' is already used by worktree ...
```

Two directories can never sit on the same branch and silently overwrite each
other's files. The same rule protects `master` in the other direction — the
worktree cannot check out `master` while the main checkout has it.

## The same list from the other side

"Whichever directory you run `git branch` from, you are reading the same
single list" — concretely, run the identical command inside the worktree:

```bash
cd ~/utono/linux-lit-wt/feat-backslash-overlay-cycle
git branch
```

```text
* feat/backslash-overlay-cycle
+ master
```

Same two branches, same list — only the markers have swapped, because the
markers describe the *relationship between the branch and where you are
standing*, not a property of the branch itself. From here,
`feat/backslash-overlay-cycle` is "checked out here" (`*`) and `master` is
"checked out in another worktree" (`+`, the main checkout).

One trap: the *container* directory between the worktrees is not a
repository at all —

```bash
cd ~/utono/linux-lit-wt
git branch
# fatal: not a git repository (or any parent up to mount point /)
```

`~/utono/linux-lit-wt/` is just a plain folder that holds worktree
directories side by side (and `~/utono/` above it is not a repo either, so
git's upward search finds nothing). You must be *inside* a worktree — e.g.
`linux-lit-wt/feat-backslash-overlay-cycle/` — for git commands to work.

How does git know, from inside that directory, to read the main checkout's
branch list? The worktree's `.git` is a one-line file, not a directory:

```bash
cat ~/utono/linux-lit-wt/feat-backslash-overlay-cycle/.git
# gitdir: /home/mlj/utono/linux-lit/.git/worktrees/feat-backslash-overlay-cycle
```

Every git command run in the worktree follows that pointer back to the one
shared `.git`, which is why there is only ever one branch list to read.

## What a worktree is

`git worktree add` gives one repository a second (third, …) working
directory, each on its own branch:

```bash
git worktree add ~/utono/linux-lit-wt/feat/backslash-overlay-cycle \
  -b feat/backslash-overlay-cycle master
```

This does three things at once:

1. Creates the branch `feat/backslash-overlay-cycle`, starting from
   `master`, in the shared repository.
2. Creates the directory `~/utono/linux-lit-wt/feat/backslash-overlay-cycle`
   with a full checkout of that branch.
3. Links the two: the worktree's `.git` is not a directory but a small
   *file* containing a path back to the main checkout's `.git`, where all
   the real data stays.

There is still exactly one repository. Commits made in the worktree are
instantly visible from the main checkout (`git log feat/...`) and vice
versa — no push/pull between them, nothing to sync.

To see the mapping of directories to branches:

```bash
git worktree list
```

## Why worktrees instead of switching branches

Suppose you are on a feature branch with uncommitted changes and a new,
unrelated feature comes up. You have three options, in this repo's
recommended order:

### Option 1 — worktree off master (recommended)

The new work gets a fresh directory and a fresh branch; the current
checkout — including its **uncommitted** changes — is left completely
untouched. Neither feature can interfere with the other, and each merges to
`master` independently.

Costs: the worktree builds its own `target/`, so the first `cargo build` is
from scratch, and you must remove the worktree when done (below). This is
also *required* whenever two Claude Code sessions might touch the repo at
the same time — concurrent sessions must never share one checkout.

### Option 2 — new branch off master, in this checkout

Clean history, but you must deal with the dirty tree first: commit the
in-progress work or `git stash` it. A stash is a hidden pile of changes
with no branch and no commit message; one that sits around for days is how
work gets lost or reapplied onto the wrong branch. Fine when the current
work is at a natural stopping point you are happy to commit.

### Option 3 — stack on the current branch

Build the new feature on top of the unfinished one. Almost always the wrong
default for *unrelated* work: the two features become entangled — you
cannot merge one without the other, and a bug in either blocks both. Stack
only when the new work genuinely depends on unmerged changes in the current
branch (e.g. it edits code that branch introduced).

## Uncommitted changes are per-directory

Unlike branches, *uncommitted* edits exist only in the working directory
where you made them. They are invisible to other worktrees until committed.
This is exactly why option 1 is safe: creating a worktree never touches
another checkout's half-finished files.

## Finishing a worktree branch

Merging happens from the **main checkout** — git refuses to check `master`
out in two worktrees, so the main checkout is where `master` lives:

```bash
cd ~/utono/linux-lit
git checkout master          # usually already there
git merge --no-ff feat/backslash-overlay-cycle
git push origin master
```

`--no-ff` forces a merge commit even when git could "fast-forward" (just
slide the `master` pointer ahead). The merge commit keeps the feature's
commits visibly grouped in history.

Then clean up, in this order:

```bash
git worktree remove ~/utono/linux-lit-wt/feat/backslash-overlay-cycle
git branch -d feat/backslash-overlay-cycle
```

The order matters: git will not delete a branch while a worktree still has
it checked out. Once both are gone, the `+` line disappears from
`git branch`. `git branch -d` (lowercase) is the safe form — it refuses to
delete a branch whose commits are not already merged.

## Repo-specific notes

- Worktrees live at `~/utono/linux-lit-wt/<branch>` (`~/utono` is not a
  repo, so siblings are safe).
- Never share `CARGO_TARGET_DIR` across worktrees — parallel builds on
  different branches lock and thrash each other.
- `CLAUDE-activeContext.md` is gitignored; the canonical copy stays in the
  main checkout. Don't create per-worktree copies.
- Absolute-path state (`~/.config/linux-lit/config-dev.json`, `lit.db`) is
  shared across all worktrees — avoid two sessions writing lit.db at once.
