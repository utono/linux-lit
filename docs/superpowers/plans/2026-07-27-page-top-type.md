# `PageTop` — implementation plan

Spec: `docs/superpowers/specs/2026-07-27-page-top-type-design.md`
Branch: `refactor/page-top-type` (worktree `~/utono/linux-lit-wt/refactor/page-top-type`)

Behaviour-preserving EXCEPT where Task 5 finds a real latent bug. The nav-fuzz
is the oracle: any unexplained behavioural diff means the refactor is wrong.

## Task 0 — baseline capture (BEFORE touching code)

The fuzz is only an oracle if we know what "unchanged" looks like. Capture on
the CURRENT tree, both engines, and keep the logs:

```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh \
  --start-work BH-Barrett --secs 90
cp /tmp/fuzz-nav.log /tmp/fuzz-baseline-prose.log
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh \
  --start-work Ham --secs 90
cp /tmp/fuzz-nav.log /tmp/fuzz-baseline-play.log
```

Record step count + any FAILs. `cargo test --bins` count too (expect 1209).

## Task 1 — add the type

New `src/input/page_top.rs` (or in `scroll.rs` if wiring is simpler):

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PageTop { line: usize, offset: i32 }
```

`at_line_start(line)`, `new(line, offset)`, `line()`, `offset()`. Fields private.

Unit tests: accessors round-trip; `at_line_start` yields offset 0; `Default` is
`(0, 0)`; `Ord`/comparison NOT derived unless a call site needs it (add only if
the compiler asks).

No behaviour change. Build + test must stay green.

## Task 2 — swap the AppState fields

Replace `page_top_line` / `page_top_offset` with `pub page_top: PageTop`.

The build breaks. **That error list is the work list** — save it:

```bash
cargo build 2>&1 | rg "^error" -A3 > /tmp/pagetop-worklist.txt
```

## Task 3 — mechanical conversion, smallest file first

Order chosen so the fiddly files come last, when the pattern is established:

1. `src/app/translations.rs` (3 assignments)
2. `src/input/highlight.rs` (2)
3. `src/input/navigation.rs` (3)
4. `src/input/prose_pages.rs` (4)
5. `src/app/mod.rs` (12)
6. `src/input/scroll.rs` (12)

Commit per file. Build green at each commit — never leave a half-converted file.

Readers of the pair (`main.rs` 382/430/578, `phrase_highlight.rs` 879/910,
`prose_pages.rs` 265/271/555, `app/mod.rs` 2483/2854/2868/2879) become
`s.page_top` directly, which is a simplification.

## Task 4 — `page_back_stack` → `Vec<PageTop>`

Currently `Vec<(usize, i32)>` — already the pair, so this is a rename-level
change. Do it AFTER Task 3 so the type is settled.

## Task 5 — the actual point: audit every `at_line_start`

```bash
rg -n "at_line_start" src/
```

For EACH site ask: **can a pinned table be active here?**

- No (plays-only path, pre-load, genuine line-start) → leave, add a one-line
  comment saying why offset 0 is correct.
- Yes → this is a latent instance of the 2026-07-27 landing bug. Fix it to read
  `canonical_page_top_offset_for` / `prose_table_boundary_for_line`, and A/B it
  (`BOTTOM_CLIP_EXACT` vs `ROWFILL`) exactly like the landing fixes.

**Every Task-5 finding is called out individually in the commit message and the
ledger — never folded silently into the refactor.**

## Task 6 — verification (mandatory; review gates waived does NOT waive these)

1. `cargo build`
2. `cargo clippy` — no NEW warnings on touched lines
3. `cargo test --bins` — 1209+ green
4. Nav-fuzz BOTH engines, diffed against Task 0 baselines:
   - prose: `--start-work BH-Barrett`
   - play: `--start-work Ham`
   Same step counts, zero FAILs. A behavioural diff that is not a Task-5
   finding is an ABORT signal.
5. Headless on-screen at production geometry (`wlr-randr 1920x1236` →
   `text_view.height = 1098`); confirm `BOTTOM_CLIP_EXACT` still governs on
   BH-Barrett and the page renders correctly.
6. Hand the user the exact command for a real-renderer eyeball (cage is
   software rendering and can disagree on layout).

## Task 7 — ledger

Append to `docs/troubleshooting/page-turning-mechanics.md`: the type now makes
the unpaired-position bug unrepresentable, `at_line_start` is the audit grep,
and any Task-5 findings with their tells.

## Task 8 — finish

Merge to master locally, re-verify build, push, remove worktree, delete branch.

**ABORT CRITERION (from the spec):** an unexplained behavioural diff not
resolved within one session → revert the branch. The current code is correct
today; a half-migrated state is not worth shipping.
