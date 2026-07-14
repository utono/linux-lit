# MND Final-Spread Dialogue-Tail Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `last_page_top` recognize a work's true end when the trailing
content past the final full spread is DIALOGUE (MND: the rest of Robin's
spoken epilogue), so `x`, `G`, and the startup snap can reach the last ~9
buffer lines of every MND-* edition instead of stranding one spread early.

**Architecture:** One-condition change to the case (a)/(b) discriminator
inside `last_page_top`'s forward walk (`src/input/navigation.rs`). The walk
already has a correct "pull forward to the canonical final spread" search
(case a); it just never runs for MND because the current discriminator —
"is there any dialogue at/below `next`?" — assumes a work's tail past the
last full spread is always non-dialogue (H8's lone EPILOGUE header). The fix
adds a second true-end signal: the forward page chain ENDS at `next`
(`next_page_top(next)` cannot advance past it). No downstream changes:
`redirect_to_final_spread`, `page_forward`, `jump_to_end`, and the startup
snap all consume `last_page_top` and start working once the anchor is right.

**Tech Stack:** Rust, GTK4 (layout-measured pagination — no pure unit-test
seam for `last_page_top`), headless verification via cage + grim + wtype,
nav-fuzz harness.

## Global Constraints

- **Never `cargo run`** — the user launches the app; agents verify headlessly
  (cage) only.
- **Scoped cleanup only:** kill headless instances with
  `pkill -f "cage -- ./target/debug/linux-lit"` — never a bare
  `pkill -f target/debug/linux-lit` (it kills the user's live instance).
- **Log/config gotcha:** dev mode is selected by the `LIT_DEV` env var, NOT
  build profile. A direct `./target/debug/linux-lit` launch (as under cage)
  is release-mode: it writes **`linux-lit-release.log`** and rewrites
  `~/.config/linux-lit/config.json` (`last_work`) on exit. Read the RELEASE
  log during headless verification.
- Nav-fuzz must be launched through `./scripts/e2e-env.sh` and always with
  `--start-work <ABBR>`.
- Pagination reads authoritative `(div1,div2)` metadata via
  `section_starts` / `is_dialogue_line` — do not add buffer-text
  re-inference.
- Branch off `master` (current branch is master). Finishing convention:
  merge back to master locally with `--no-ff`, re-verify, push, delete
  branch.
- Another Claude session may be building in this repo concurrently — if
  `cargo` blocks on the package lock, wait it out rather than killing
  anything.

## Diagnosis (evidence — do NOT re-investigate)

Reproduced 2026-07-04 headlessly (cage, `LIT_START_WORK=MND
LIT_START_POS=3450`, 1280×720). MND buffer `line_count=3035`. Log
(`linux-lit-release.log`):

```
STARTUP: snap near-end page_top 3032 -> canonical 2986 (cursor 3025)
KEY: name=x ... ACTION: PageForward
PAGE_FWD: final-region candidate=3026 -> anchor=2986
PAGE_FWD: page_top=2986 new_top=3026 next_dialogue=3026 candidate_top=2986 effective_top=2986 prose=false
PAGE_FWD: ceiling hit, jumping to end
```

- The forward walk in `last_page_top` reaches the boundary `next=3026`;
  that page's right column would be empty (only the 9-line tail
  3026..=3034 remains — txt lines 3553–3561: "Now to 'scape the serpent's
  tongue" … "And Robin shall restore amends." + `[He exits.]`).
- The discriminator `dialogue_at_or_below_next` is TRUE (the tail is
  Robin's spoken epilogue, plain `5.1` dialogue rows in lit.db — verified:
  every row through `line_in_div=455` is `div1=5, div2=1`, no section
  break). So the walk classifies the true end as case (b) "mid-work
  scene-opening boundary", skips the case (a) pull-forward, advances to
  `top=3026` (empty right column → not recorded as `last_full`), then the
  chain ends and it returns `last_full=2986`.
- Spread 2986's forward boundary is 3026 < 3035, so the final spread never
  reaches the end. `x`'s correct candidate 3026 is then bounced back by
  `redirect_to_final_spread` (empty right column → redirect to anchor
  2986 = current page) → "ceiling hit" → cursor parked on the last visible
  line. Lines 3026..=3034 are unreachable by `x`, `G`, and startup resume.
- All MND-* editions share this text/layout, hence "all MND-* works".
  H8's epilogue does NOT hit this because its tail starts with a
  non-dialogue `EPILOGUE` header region, making
  `dialogue_at_or_below_next=false` — that is exactly the case the current
  discriminator was built for.

---

### Task 1: Branch + failing repro script

**Files:**
- Create: branch `fix/mnd-final-spread-dialogue-tail` (off `master`)
- Create: `/tmp/claude-1000/-home-mlj-utono-linux-lit/*/scratchpad/mnd-repro.sh`
  (scratchpad; session-temporary, not committed)

**Interfaces:**
- Produces: a repeatable headless repro whose PASS/FAIL criterion Task 2
  flips. FAIL (current behavior): release log shows
  `PAGE_FWD: final-region candidate=3026 -> anchor=2986` followed by
  `ceiling hit`, and the final screenshot does NOT contain the work's last
  dialogue lines. PASS (after fix): `x` from the old anchor advances
  `page_top` forward, and the final spread's right column shows
  "Give me your hands, if we be friends," and
  "And Robin shall restore amends."

- [ ] **Step 1: Create the branch**

```bash
cd ~/utono/linux-lit && git checkout -b fix/mnd-final-spread-dialogue-tail master
```

- [ ] **Step 2: Write the repro script** (adjust `$SCRATCH` to the session
  scratchpad dir; any writable temp dir works)

```bash
SCRATCH=${SCRATCH:-/tmp}
cat > "$SCRATCH/mnd-repro.sh" <<'EOF'
#!/usr/bin/env bash
# Headless MND end-of-work pagination repro. Run from repo root.
set -e
cargo build 2>&1 | tail -1
pkill -f "cage -- ./target/debug/linux-lit" 2>/dev/null; sleep 1
LIT_START_WORK=MND LIT_START_POS=3450 \
  GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 LIT_HEADLESS_TEST=1 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage-mnd.log &
sleep 10
export WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000
grim /tmp/mnd-before-x.png
wtype "x"; sleep 2; grim /tmp/mnd-after-x1.png
wtype "x"; sleep 2; grim /tmp/mnd-after-x2.png
pkill -f "cage -- ./target/debug/linux-lit" || true
echo "--- PAGE_FWD / snap lines (RELEASE log — direct binary launch):"
rg -n "PAGE_FWD|snap near-end" linux-lit-release.log || true
EOF
chmod +x "$SCRATCH/mnd-repro.sh"
```

- [ ] **Step 3: Run it and confirm it FAILS (bug present)**

Run: `cd ~/utono/linux-lit && $SCRATCH/mnd-repro.sh`
Expected (current buggy behavior):
- Log shows `snap near-end page_top ... -> canonical 2986`,
  `PAGE_FWD: final-region candidate=3026 -> anchor=2986`, and
  `PAGE_FWD: ceiling hit, jumping to end`.
- Read `/tmp/mnd-after-x2.png`: the right column ends around "And, as I am
  an honest Puck," — the last lines ("Give me your hands…", "And Robin
  shall restore amends.") are NOT visible anywhere.

Note: an empty ~2-byte PNG means the surface hadn't mapped — sleep and
re-`grim`, don't diagnose. Check `stat -c%s` first.

- [ ] **Step 4: No commit** (nothing repo-tracked changed; the repro
  script is scratch tooling)

---

### Task 2: The discriminator fix

**Files:**
- Modify: `src/input/navigation.rs:456-482` (inside `last_page_top`'s
  forward-walk loop)

**Interfaces:**
- Consumes: `super::viewport::next_page_top(state, top) -> NextPage { new_top, .. }`,
  `would_empty_right_column(state, top) -> bool`,
  `super::viewport::column_split(state, t) -> ColumnSplit { next_page_top, .. }`,
  `is_dialogue_line(&state.buffer, i, is_prose, &stage_lookup) -> bool` —
  all already in scope in this function.
- Produces: `last_page_top(state)` returns a top whose spread's forward
  boundary reaches past the work's last dialogue line for works with a
  dialogue tail (MND). Signature unchanged — no caller edits.

- [ ] **Step 1: Apply the change**

The current code (navigation.rs, inside the `loop`; comment block above it
describes cases (a)/(b)):

```rust
            let dialogue_at_or_below_next =
                (next..line_count).any(|i| is_dialogue_line(&state.buffer, i, state.is_prose(), &stage_lookup));
            if we_next && !dialogue_at_or_below_next {
```

Replace with:

```rust
            let dialogue_at_or_below_next =
                (next..line_count).any(|i| is_dialogue_line(&state.buffer, i, state.is_prose(), &stage_lookup));
            // Second true-end signal: the forward chain ENDS at `next` — no page
            // exists past it. A dialogue tail (MND: the remainder of Robin's
            // spoken epilogue, plain 5.1 lines, no trailing section) makes
            // `dialogue_at_or_below_next` true even at the work's real end, which
            // used to misclassify it as case (b) and strand the anchor one spread
            // short (the tail was unreachable by x/G/startup). A mid-work
            // scene-opening boundary (case b, H8) always has further pages, so
            // its chain continues and this stays false. Short-circuit on
            // `we_next` so the extra layout walk only runs at empty-right
            // boundaries, not every page.
            let chain_ends_at_next = we_next && {
                let nn = super::viewport::next_page_top(state, next).new_top;
                nn >= line_count || nn <= next
            };
            if we_next && (!dialogue_at_or_below_next || chain_ends_at_next) {
```

Also extend the case-discriminator comment above (the block ending "…the
next full-right-column spread the walk reaches will.)") by appending one
line so the prose matches the code:

```rust
            // A dialogue TAIL (case a with spoken lines, e.g. MND's epilogue) is
            // caught by the chain-end check below even though dialogue remains.
```

- [ ] **Step 2: Build**

Run: `cd ~/utono/linux-lit && cargo build 2>&1 | tail -1`
Expected: `Finished \`dev\` profile ...` (warnings OK, no errors)

- [ ] **Step 3: Run the Task 1 repro and confirm it PASSES**

Run: `$SCRATCH/mnd-repro.sh`
Expected (fixed behavior):
- Startup snap and the `x` anchor land on a top LATER than 2986 (some t in
  2987..3025 — exact value depends on viewport height; at 1280×720 expect
  roughly 2990–3000).
- The log shows `x` producing forward progress (a `PAGE_FWD:` line whose
  new top > previous page_top, or the final-spread-guard cursor move) —
  NOT `final-region candidate=... -> anchor=<same as page_top>` +
  `ceiling hit`.
- Read `/tmp/mnd-after-x2.png` (and `mnd-after-x1.png`): the right column
  now ends with "Give me your hands, if we be friends," / "And Robin shall
  restore amends." Also note whether the final stage direction
  `[He exits.]` is visible; if it is not, report it in the task summary
  (the case (a) pull only guarantees no DIALOGUE remains below the
  boundary — a trailing stage direction may legitimately sit below; do not
  silently widen the fix to chase it).
- Check both screenshots for clipping: no half-cut glyph row at either
  column's bottom edge.

- [ ] **Step 4: Verify G-idempotency by hand in the same launch pattern**
  (catches "G disagrees with itself", the historical failure mode of this
  function)

```bash
pkill -f "cage -- ./target/debug/linux-lit" 2>/dev/null; sleep 1
LIT_START_WORK=MND LIT_START_POS=100 \
  GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 LIT_HEADLESS_TEST=1 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage-mnd-g.log &
sleep 10
export WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000
wtype -M shift -k g -m shift; sleep 2; grim /tmp/mnd-G1.png
wtype -M shift -k g -m shift; sleep 2; grim /tmp/mnd-G2.png
pkill -f "cage -- ./target/debug/linux-lit"
```

Expected: `/tmp/mnd-G1.png` and `/tmp/mnd-G2.png` show the SAME spread
(pressing G twice must not move), and it is the spread whose right column
ends at "And Robin shall restore amends."

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit
git add src/input/navigation.rs
git commit -m "fix(pagination): reach MND's dialogue tail — treat chain-end as true end in last_page_top

The case (a)/(b) discriminator in last_page_top assumed a work's trailing
content past the last full spread is non-dialogue (H8's lone EPILOGUE
header). MND's tail is the remainder of Robin's spoken epilogue (plain 5.1
dialogue), so the walk classified the true end as a mid-work scene break,
skipped the pull-forward, and returned an anchor one spread short — the
last 9 buffer lines were unreachable by x/G/startup on every MND-*
edition. Add a second true-end signal: the forward page chain ends at
\`next\`."
```

---

### Task 3: Regression verification (nav-fuzz) + docs

**Files:**
- Modify: `docs/troubleshooting/page-turning-mechanics.md` (the section
  documenting `last_page_top`'s case (a)/(b) discriminator — search for
  `EPILOGUE` / `last_page_top`)
- Test: nav-fuzz runs (no repo files; logs land at `/tmp/fuzz-nav.log`)

**Interfaces:**
- Consumes: the Task 2 code change (committed).
- Produces: evidence that MND is fixed and H8 (the work the current
  discriminator was built for) plus one dialogue-heavy control work did
  not regress; doc paragraph so the next reader knows the discriminator
  has TWO true-end signals.

- [ ] **Step 1: nav-fuzz MND** (the workhorse for pagination changes; runs
  ~330s by default — use `--secs 120` for iteration, full length for the
  final pass)

Run:
```bash
cd ~/utono/linux-lit && ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work MND
```
Expected: FAIL summary reports 0 failures (in particular no
`G`-idempotency, UNBALANCED-SPREAD, or stuck-`x` findings). On any FAIL,
read `/tmp/fuzz-nav.log` around the failing action before touching code.

- [ ] **Step 2: nav-fuzz H8 (regression guard for the case the old
  discriminator served)**

Run:
```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work H8
```
Expected: no NEW failures versus master. H8 has one KNOWN historical
open-bug note (`project_navfuzz_sweep_2026-06-05`); if a failure appears,
check out `master`, rerun the same command, and compare — only a
delta introduced by this change blocks the merge.

- [ ] **Step 3: nav-fuzz one control work with a normal (non-dialogue-tail)
  ending, e.g. Ham**

Run:
```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work Ham
```
Expected: 0 failures.

- [ ] **Step 4: pure-logic suite**

Run: `cargo test --bins 2>&1 | tail -3`
Expected: all tests pass (this change has no pure-helper seam, so this is
a no-regression check only).

- [ ] **Step 5: Review screenshots per UI review protocol**

Open every PNG under `target/ui/` from the fuzz runs plus the manual
`/tmp/mnd-*.png` captures; quote the on-screen text of the MND final
spread in the reply and call out any clipping by eye.

- [ ] **Step 6: Document the second true-end signal**

In `docs/troubleshooting/page-turning-mechanics.md`, find the paragraph
describing `last_page_top`'s empty-right-column discriminator (the H8
EPILOGUE case) and append:

```markdown
A dialogue tail defeats the dialogue-below test: MND ends with the
remainder of Robin's spoken epilogue (plain 5.1 dialogue, no trailing
section), so "dialogue remains below `next`" is true even at the work's
real end. `last_page_top` therefore has a SECOND true-end signal: the
forward page chain ENDS at `next` (`next_page_top(next)` cannot advance).
Either signal triggers the case (a) pull-forward. A mid-work
scene-opening boundary always has further pages, so the chain-end signal
never fires mid-work (fixed 2026-07-04; before this, all MND-* editions
stranded one spread early and the last ~9 lines were unreachable).
```

- [ ] **Step 7: Commit**

```bash
git add docs/troubleshooting/page-turning-mechanics.md
git commit -m "docs(page-turning): document chain-end as second true-end signal in last_page_top"
```

---

### Task 4: Finish the branch (house convention)

**Files:**
- Modify: git state only.

- [ ] **Step 1: Verify clean tree + build on the branch**

Run: `git status --short && cargo build 2>&1 | tail -1`
Expected: no unexpected modified files (the 6 pre-existing dirty files
from before this work — app/mod.rs, concordance.rs, keymap.rs,
keymap_config.rs, journal_keybinds_overlay.rs, keybinds_overlay.rs — are
UNRELATED and must NOT be committed or reverted by this plan); build OK.

- [ ] **Step 2: Merge to master, re-verify, push, delete branch**

```bash
git checkout master
git merge --no-ff fix/mnd-final-spread-dialogue-tail
cargo build 2>&1 | tail -1 && cargo test --bins 2>&1 | tail -3
git push origin master   # if SSH fails in the agent shell, ask the user to push
git branch -d fix/mnd-final-spread-dialogue-tail
```

Expected: merge commit on master, build + tests green, pushed. (Note:
master is already ahead of origin by `46941ce`, unpushed — this push
carries that too.)

- [ ] **Step 3: Hand the user the final eyeball command** (cage renders
  via cairo software; the real GL renderer deserves one look)

```bash
cargo run
```
Then in-app: open any MND-* edition, press `G` — the spread should end at
"And Robin shall restore amends."; press `x` twice from a few pages back
and confirm it walks onto that spread and stops there.
