# linux-lit

GTK4 Rust literature reader with e-reader pagination, MPV audio sync, and vim-style navigation.

## Active Context (read first)

**At the start of every session, read `CLAUDE-activeContext.md` if it exists**
(project root). It records the current branch, in-progress/uncommitted work,
recent decisions, and next actions — none recoverable from code or git alone.

**`ac` is the alias for `CLAUDE-activeContext.md`** ("update ac" / "uac",
"read ac" / "rac"). Record only what is NOT recoverable from code or git;
convert relative dates to absolute (US Central). **Do NOT update `ac` after
every commit** — only when asked or before a likely context break. It is a
scoped continuity file, not a changelog: REPLACE stale state, keep it short.
(This project deliberately opts OUT of the global "After a Commit" rule.)

## Debug Log

- Dev build (`cargo run`): `~/utono/linux-lit/linux-lit-dev.log`; release:
  `linux-lit-release.log`. Instance slots n≥2 write `-{n}`-suffixed logs —
  check which file has fresh timestamps before trusting a tail.
- Cleared on every launch. Add lines with `log_fmt!()` (`src/logging.rs`).
- **When fixing bugs, read the log first.**
- **Read the log yourself with `rg` whenever you can** — the debug logs live in
  the repo dir and are readable from the agent's own shell, so run the `rg`
  command against them directly rather than asking the user to paste log output.
  Only ask the user to run it when the file genuinely isn't reachable (wrong
  slot/worktree) or the user must reproduce a live-only state first. NOTE: an
  ad-hoc `LIT_DEV=1 ./target/debug/linux-lit` run can still land on a
  release/`-{n}` slot — search ALL `*.log` in the repo dir by mtime
  (`rg -l PATTERN *.log`) instead of assuming `linux-lit-dev.log`.
- **Verify on-screen edges/margins by PIXEL-MEASURING the screenshot, never by
  eye.** A 24px margin at 1920px scaled into a chat reply is easy to misread as
  "flush." Sample the actual cream/teal boundary (e.g. a short Python/PIL scan
  of a row) before concluding a layout is wrong — and trust the logged
  allocation numbers over a visual impression.
- **Cage (software rendering) can disagree with the real GL renderer on layout.**
  A margin/sizing fix that "passes" headless in cage is NOT confirmed — verify
  the final result on the user's real renderer (or by pixel-measuring their
  screenshot) before claiming a layout bug is fixed.
- GLib/GTK criticals go to **stderr**, not the app log — capture with
  `cargo run 2>&1 | tee linux-lit-dev-stderr.log`; investigate with the
  `check-stderr-log` skill.

## Playback Sync Bugs — identify the PLAYING work FIRST

For ANY sync bug, first pin down **which edition is playing** (`Cym` vs
`Cym-Amb` vs `Cym-BBC`) — each has its own media file and `line_timestamps`;
diagnosing the wrong abbrev inspects the wrong data. Confirm via the title
bar/screenshot or the log's `SEEK:`/`CURSOR_LINE:` lines, then run the sync
queries (see the `debug-playback-sync` skill) against THAT abbrev. A common
root cause is one corrupt out-of-order timestamp in the playing edition —
a litdb/wizard defect fixed in lit.db, not in linux-lit code.

## Clipping Bugs — read clip-prevention.md FIRST

For ANY text-clipping or flush-to-the-edge bug, **read
`docs/troubleshooting/clip-prevention.md` before attempting a fix** — it has
the frequency-ordered failure checklist for every surface. When the visible
result contradicts the logged clip value, launch with
`LIT_DEBUG_CLIP_COLOR='#ff0000'` to paint the clip boxes. Clipping acceptance
is pixel-level: verify on the real display or the `line_clipping` /
`overlay_clipping` e2e invariants, never from logs alone.

**After addressing ANY clipping bug, UPDATE
`docs/troubleshooting/clip-prevention.md`** — add the new failure mode to the
frequency-ordered checklist (and a surface note if relevant) with its tell,
root cause, and fix, so the next occurrence is diagnosed from the doc, not
re-derived. This is required, not optional.

## Hidden Timestamp Glyphs — READ THE LOG FIRST

When prompted that the **left-gutter timestamp glyphs are hidden/missing**
(or "timestamp glyphs hidden", "signs disappeared", "gutter glyphs gone",
or any similar wording), the bug is **intermittent and does not reproduce
on demand** — a fresh load of the exact work/chapter and repeated `\`
overlay cycling both keep the glyphs (verified headlessly 2026-07-24). Do
NOT try to reproduce it first, and do NOT blind-fix. **Read the debug log
and grep for the breadcrumb lines** committed for exactly this bug:

- `GUTTER_TS: has_timestamp rebuilt len=… true=… line_map=… current_work=… visible=…`
  — the key line. Healthy is `true=7287`-ish and `visible=true`; a
  disappearance shows `true=0` / tiny (the `has_timestamp` vec rebuilt
  empty — dropped `current_work` or a `line_map` length mismatch) or
  `visible=false` (the sign flag got flipped off).
- `SIGN: … via show_translations` / `… via strip_translation_lines` /
  `… had no saved prev` — the translations save/restore path (only runs
  on works that HAVE translations; the reported BH-Barrett has none).
- `SIGN: signs … via toggle_sign_column (l key)` — the `l` toggle.
- `RETURN_TO_READER: from … sign_column_visible=…` — timeline anchor at
  each overlay close (the `\` event users attribute it to).

The log is cleared on every launch, so if the user still has the affected
instance running, have them copy it BEFORE relaunching:
`cp linux-lit-dev.log vanished-glyphs-$(date +%H%M%S).log`. Only these
lines pin the culprit; fix the ROOT CAUSE they reveal, not the symptom.
Known non-causes (already eliminated): the `\` gloss/journal cycle alone
(overlays render into their own views, never the reader gutter), and the
translations path on a work with zero translations. Diagnostic source:
`src/app/mod.rs` `setup_gutter`/`return_to_reader_mode`,
`src/app/translations.rs`; instrumentation commit `015dc67c`.

## Troubleshooting Ledgers

The clip-prevention pattern generalizes: each recurring-bug domain
keeps a frequency-ordered ledger in `docs/troubleshooting/` (clipping
and page-turning exist; playback-sync is the next candidate). Trigger:
if diagnosis took more than one session OR the root cause contradicted
the first hypothesis, append the failure mode — tell, root cause, fix —
in the same change. This is required, not optional.

## Build & Run

Verify with `cargo build`; do NOT run the app — the user runs `cargo run`
themselves. Multi-instance is supported (per-process slots: own log suffix,
`i{n}-` MPV sockets, config merge-on-save), but a running instance predates
any rebuild.

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
  **"No review gates" waives the code-review subagents ONLY.** Build,
  clippy, tests, AND the on-screen headless/real-renderer check are
  correctness, not review, and remain mandatory whatever the user says
  about review. Do not re-derive this each session.
- **Verify against the VISIBLE surface, not the branch the spec names.**
  For any UI change, the acceptance check must exercise the exact
  widget/mode the user actually interacts with. A spec that names an
  internal branch (e.g. "the non-float open path") is NOT proof the
  change affects what the user sees — confirm which branch actually
  renders for the real interaction before calling it verified. (Backlog
  #13 was fully green — build + clippy + tests + a spec-matching
  implementation — and still a silent no-op, because it sized a pinned
  ask card the user never opens instead of the float column they do.)
- **Batch-day playbook.** When a queue of small independent polish
  items is planned: (1) one plan via `superpowers:writing-plans` with
  explicitly independent tasks; (2) execute via
  `superpowers:subagent-driven-development`, one worktree per task;
  (3) merge serially from the main checkout; (4) run the e2e suite
  once at the end, not per branch. **If reviews are waived, the
  end-of-batch e2e/on-screen run is MANDATORY (not optional) and must
  exercise each item's own on-screen criterion — it is the only
  remaining gate.** **If verification or user feedback invalidates an
  already-merged item, treat the correction as a fresh
  spec→plan→implement cycle (including a revert of the superseded
  change), not an in-place patch** — as the #13 → overlay-width pivot
  did.
- **Effort-level retrospective.** **PAUSED (2026-07-23) — do NOT append
  the retrospective note for now.** The full rule is kept below so it can
  be turned back on later (delete this PAUSED line to re-enable). When
  paused, finish a plan without the effort-level trade-off note.
  After finishing a plan (branch merged
  or ready), append a short **effort-level trade-off** note. The axis is
  EFFORT LEVEL (how much verification/review/adversarial-checking/headless
  re-running was done), NOT model choice — the user runs opus (or fable),
  so hold the model constant and compare effort levels. Report: (1) what
  effort this run actually spent — number of implement + review + fix
  round-trips, whether per-task reviews AND an adversarial whole-branch
  review ran, how many headless re-verifications, whether a root-cause
  investigation was needed; (2) a ROUGH, explicitly-labeled estimate of
  the time/token savings a LOWER effort level would have given (e.g.
  "dropping the per-task reviews and the adversarial final review, keeping
  only a light final check ≈ 30–45% fewer subagent turns"); (3) what that
  lower level would have RISKED here — tie it to concrete outcomes from
  this run: a bug that the review/headless pass actually caught (e.g. the
  loading-spinner "spins forever on error paths" leak, or the blank-
  journal-body regression) is evidence the higher effort paid off; a clean
  pass with zero findings is evidence a lower level would have sufficed.
  Estimates are directional, not measured — say so. Keep it to a few
  sentences; the goal is to calibrate the EFFORT LEVEL on the NEXT similar
  task, not to instrument this one precisely.

## Parallel Claude Code Sessions (git worktrees)

Two or more concurrent sessions must NEVER share this checkout — each
session gets its own git worktree on its own branch. Worktrees HOST
feature branches, they don't replace them: all branching and
finishing-a-branch conventions still apply. Worktrees live under
`~/utono/linux-lit-wt/<branch>` (`~/utono` is not a repo, so siblings
are safe):

```bash
git worktree add ~/utono/linux-lit-wt/<branch> -b <branch>
```

Worktrees are not only for concurrent sessions: any branch **expected
to span sessions** — or likely to leave the tree dirty at session end —
starts in a worktree via `superpowers:using-git-worktrees`. EXEMPT:
quick fixes branched, committed, and merged within one session. The
invariant bought: the main checkout ends every session clean on master.

Session pre-flight: run `git worktree list` and `git status` in this
main checkout at session start; a dirty main checkout is the first
thing to resolve, not work around. Branch hygiene: when abandoning a
branch, delete it — never leave a third state on origin.

- Each worktree builds its own `target/` (first build is from scratch).
  Never share `CARGO_TARGET_DIR` across worktrees — parallel builds on
  different branches lock and thrash each other.
- `CLAUDE-activeContext.md` is gitignored, so a fresh worktree has no
  `ac`. The canonical `ac` stays in this main checkout; don't create
  per-worktree copies.
- Shared mutable state stays shared across worktrees:
  `~/.config/linux-lit/config-dev.json` and `lit.db` are absolute
  paths. The instance-slot merge-on-save covers the config; avoid two
  sessions writing lit.db at the same time.
- Merge back to master from THIS main checkout (git refuses to check
  master out in two worktrees), then `git worktree remove` the finished
  worktree before deleting its branch.
- Debug logs live in the repo dir, so each worktree logs separately;
  the instance slots already keep concurrent dev runs apart. The cage
  cleanup `pkill -f "cage -- ./target/debug/linux-lit"` matches EVERY
  worktree's debug build — fine (all are throwaway test instances),
  but it is not scoped to one worktree.

## Testing

```bash
cargo test
cargo clippy
```

**TDD default for sync/pagination/clipping (seek included):** bug fixes
in these subsystems start with a FAILING headless repro (extend
`test-playback-sync`, the nav-fuzz, or the clipping e2e) per
`superpowers:test-driven-development`; then fix; then green. Strong
default, not a hard gate: when the state is genuinely live-only and
cannot be automated, say so explicitly in the commit message and
proceed.

## Headless Verification (agent self-check)

Agents verify GUI changes WITHOUT touching the live session by running the
reader inside a throwaway `cage` compositor and screenshotting with `grim`.
**This works from the agent's own shell — first resort for any on-screen
acceptance criterion.** Fall back to asking the user only when a launch
genuinely fails after a retry (`/tmp/cage.log` dead, or repeated empty PNGs).

**Run cage-backed test binaries ONE AT A TIME, single-threaded, with a short
pause between them.** They contend over the compositor stack, and the contention
is not merely a parallelism problem: `--test-threads=1` is necessary but not
sufficient, because separate `cargo test --test <name>` invocations run back to
back still collide. Observed 2026-08-01: `smoke` failed with "screenshot 4322
bytes — reader likely did not render (blank output)" immediately after another
suite, then passed unchanged after `sleep 3`. Chasing that as a real failure
wastes a debugging cycle, so:

```bash
for t in niri_smoke overlay_clipping journal_clipping; do
  cargo test --test $t -- --ignored --test-threads=1; sleep 3
done
```

Corollary: a cage failure seen in a batch run is not evidence until it
reproduces in isolation. Re-run the single binary before believing it — and
before believing it is YOURS, re-run it against a stash of your changes.

```bash
cd ~/utono/linux-lit && cargo build
LIT_NO_MPV=1 GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
```

- `GSK_RENDERER=cairo` is **mandatory** (default renderer aborts headless);
  `LIT_NO_MPV=1` fully isolates from live MPV (`LIT_HEADLESS_TEST=1` alone
  still CONNECTS to a running live socket).
- **Ad-hoc cage runs need `LIT_DEV=1`** or the app loads `config.json`
  (release theme/positions) instead of `config-dev.json`; `run-fuzz.sh` and
  `e2e-env.sh` already set it, a bare `cage -- ./target/debug/...` does not.
  Without it the run takes a release instance slot (e.g. wrote
  `linux-lit-release-2.log`) — find the fresh log by mtime, not by name.
- Cage opens a fresh socket (`ls /run/user/1000/wayland-*`); export
  `WAYLAND_DISPLAY` to it. Default output is 1280×720 — resize to production
  geometry when pagination matters:
  `wlr-randr --output HEADLESS-1 --custom-mode 1920x1236`.
  **1236, not 1200** — pagination keys on the TEXT VIEW height, and only 1236
  reproduces production's `text_view.height = 1098` (1200 gives 1062, a 36px
  miss that changes the page grid and can hide the bug entirely). Verify with
  `RESIZE_TICK: text_view.height changed … -> 1098` in the log. The resize
  lands after the app maps, so the first page table is built at 720p and
  dropped — wait for the settled-layout hook to regenerate before driving, or
  the run has no table and table-mode bugs cannot reproduce.
- Give the window ~3s to map before `wtype`. An empty ~2-byte PNG from `grim`
  means not-mapped-yet, NOT failure — sleep 3 and retry; check `stat -c%s`
  before Read-ing. Modifier chords: `wtype -M ctrl -k j -m ctrl`.
- Key names drift — confirm current binds in `src/input/keymap_config.rs`
  before scripting a drive. Stable gotchas: front matter (before Chapter 1)
  has no synopsis; overlay-open line-scroll keys scroll the overlay; Escape
  closes it.
- A cage run takes instance slot 2+ (own `-{n}` log, `i{n}-` MPV sockets).
- **A direct cage launch reusing `XDG_RUNTIME_DIR=/run/user/1000` collides
  with the user's OWN compositor** — `grim` on the ambiguous `wayland-0` then
  screenshots the user's live desktop, not the test. Prefer `land-on.sh` /
  `e2e-env.sh` (they mint a fresh temp `XDG_RUNTIME_DIR`); for a hand-rolled
  cage run, set a fresh runtime dir and screenshot THAT socket.
- **`land-on.sh` and hand-rolled cage runs must stay foreground-alive** — a
  `nohup … &`, `setsid … &`, or `timeout N ./land-on.sh` kills the instance
  the moment the wrapper returns. For agent self-check, launch cage with the
  harness `run_in_background` (it owns the lifecycle), not a detached shell
  backgrounding.
- **Never poll for a launch with a bare `until … done` — always bound it with
  `timeout`.** Waiting on `land-on.sh` output with
  `until rg -q "XDG_RUNTIME_DIR=" "$OUT"; do sleep 2; done` spins FOREVER when
  the launch fails, when the run is abandoned, or when the output file was
  already consumed — leaving orphaned shells in the harness's Background panel
  that the user has to notice and kill by hand. (This happened repeatedly in
  one session; the loops outlived every cage they were waiting on.) Bound the
  wait and also match the failure line the script actually emits:

  ```bash
  timeout 90 bash -c 'until rg -q "XDG_RUNTIME_DIR=|ERROR" "$0"; do sleep 2; done' "$OUT" \
    || echo "launch never reported — check $OUT"
  ```

  The `timeout` here wraps the POLLING shell, not the cage: it is a different
  thing from `timeout N ./land-on.sh`, which the bullet above rightly forbids
  because it kills the instance itself.
- **After a `wlr-randr` resize the first `wtype` chord is dropped** (focus
  lost) — re-send it and confirm the `KEY:` log line landed before trusting
  the screenshot. The vim ask card also eats Escapes one modal layer at a
  time, so to test a reader-mode bind, LAND in reader mode directly
  (`land-on.sh WORK d1.d2` with no overlay arg) rather than escaping into it.
- **LSP/rust-analyzer diagnostics injected right after a merge or revert can
  be PHANTOM** (stale editor index) — a real `cargo build` is the only
  authority. Twice in one session a phantom `E0107`/`E0004` on freshly-merged
  `journal.rs` vanished under a real compile. Never edit code to satisfy a
  post-merge LSP diagnostic without confirming with `cargo build` first.
- **Cleanup: ONLY `pkill -f "cage -- ./target/debug/linux-lit"`.** A bare
  `pkill -f target/debug/linux-lit` kills the user's live instance.

## Automated UI tests (cargo)

`tests/harness/mod.rs` + `tests/smoke.rs` + `tests/line_clipping.rs` wrap the
same cage/grim/wtype flow in `cargo test` with a fail-closed line-clipping
assertion. Tests are `#[ignore]`d so bare `cargo test` stays green.

```bash
./scripts/e2e-env.sh cargo test -- --ignored --nocapture
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
```

Design notes: cage (not bare dwl/sway) because the app needs a configured,
focused, fullscreen surface; MPV is skipped under `LIT_HEADLESS_TEST=1`; the
app logs `TEST_VIEWPORT_RECT` for the pixel detector's `--region` (no AT-SPI
text interface). Scope is the main reading card only.

### niri harness (the real WM)

**niri is the current window manager** (`~/utono/niri-mlj`); dwl is the
predecessor. `tests/harness/niri.rs` + `tests/niri_smoke.rs` +
`tests/harness/niri-test.kdl` run the app under REAL niri, for anything
where the WM's own behavior matters — decorations, tiling geometry —
which cage's kiosk force-fullscreen hides. Cage stays the DEFAULT for the
rest of the suite.

```bash
./scripts/e2e-env.sh cargo test --test niri_smoke -- --ignored --nocapture
```

**niri has NO headless backend** (it is Smithay, not wlroots): `WLR_BACKENDS`
is ignored, and with no parent display it picks the TTY backend and panics.
So the harness NESTS it: `cage (headless) → niri (winit) → linux-lit`.
Load-bearing consequences, each verified by measurement, not assumption:

- **Output size comes from the OUTER cage window.** A `mode` in the niri
  config is inert; `set_output_size` resizes cage's output and niri follows.
- **The niri IPC socket path is subject to `SUN_LEN`.** The runtime dir must
  be short (`/tmp/lit-niri-*`), never a deep scratchpad path, or every
  `niri msg` fails with "path must be shorter than SUN_LEN".
- **niri 26.04 reports no fullscreen flag** in `niri msg --json windows`, and
  with `gaps 0` a tiled window measures the SAME as a fullscreen one
  (1272x688 in a 1272x688 output). Neither JSON nor geometry can tell the
  states apart — only pixels can. `fullscreen-window` is a TOGGLE, so use
  the idempotent `ensure_fullscreen` / `unfullscreen_window` wrappers.
- **The test config deliberately omits `prefer-no-csd`.** Setting it
  suppresses the titlebar regardless of what the app requests, which makes
  any decoration test pass vacuously.

**Decoration tests must run TILED, never fullscreen** — a fullscreen window
is undecorated by definition. GTK's titlebar is CLIENT-side (painted inside
the window's own surface), so it is invisible to niri's IPC and must be
detected in pixels: a ~37px bright-neutral band at the top of the capture.
When adding such a test, prove it fails by temporarily building with
`.decorated(true)`; three successive versions of this check passed against a
decorated build before the pixel-based one caught it.

**When the change is pagination/spread/page-turn**, the workhorse is the
nav-fuzz (drives every nav action; asserts on-page landing, balanced columns,
G-idempotency). It lives in the `test-headless-navigation` skill and MUST run
through the env wrapper, always with `--start-work`:

```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work <ABBR>
```

(without `--start-work` the run loads — and rewrites — the dev config's
`last_work`, so the bug silently moves works). `--secs N` shortens the run;
full log at `/tmp/fuzz-nav.log`.

`cargo test --bins` alone suffices for pure helpers/parsing/DB/state machines.
Hand the user the exact e2e command for a final eyeball on the real GL
renderer when the criterion is "renders correctly on screen" (cage is
software rendering).

## UI review protocol

E2e screenshots land in `target/ui/` (auto-cleaned per run). **Open every PNG
— and any `_clip.png` overlay — and report what you see inline**: quote the
on-screen text, call out clipping/layout problems by eye. A passing exit code
is not enough.

## Key Files

- `src/main.rs` — entry point, Tokio runtime, channel bridge, MPV event loop
- `src/app/mod.rs` — GTK4 window, AppState, display_work, the resize tick
- `src/app/text_prep.rs` — text-file read/clean/prepare for display
- `src/app/layout.rs` — card sizing/margins (`apply_card_sizing`, `main_card_rect`)
- `src/config.rs` — config persistence (see dev-vs-release gotcha below)
- `src/input/keymap.rs` — key event routing, chord state machines, dispatch_action
- `src/input/keymap_config.rs` — compiled-in default keybinds, keymap.json loader
- `src/input/navigation.rs` — cursor movement, page turns, scroll logic
- `src/input/actions/mod.rs` — Action enum with all reader-mode actions
- `src/input/actions/concordance.rs` — concordance + vocab jump handlers
- `src/input/actions/chat.rs` — chat layout (panel toggle, send/save/revision)
- `src/input/segments.rs` — cursor-segment context for chat prompts
- `src/input/vocab_loop.rs` — vocab-sentence drill loop mode
- `src/input/highlight.rs` — update_highlight, update_highlight_and_center
- `src/input/scroll.rs` — set_page, set_page_instant, center_cursor
- `src/input/page_table.rs` — pinned play_pages engine
- `src/db/queries.rs` — SQLite queries (list_works, load_work)
- `src/db/line_types.rs` — line classification predicates
- `src/mpv/` — client.rs (IPC), commands.rs (enums), discovery.rs (sockets/launch)
- `src/ui/keybinds_overlay.rs` — Ctrl+/ reader overlay (main-card binds ONLY)
- `src/ui/chat_panel.rs` — left chat panel (chat layout)
- `src/theme.rs` — Theme struct + generate_css (all theme CSS lives here)
- `tests/harness/mod.rs`, `tests/line_clipping.rs`, `scripts/e2e-env.sh`,
  `scripts/check_line_clipping.py` — headless test harness
- `src/logging.rs` — file-based debug logging

## Keybinds

- **ALWAYS update every relevant keybind overlay and legend in the SAME
  change as the keybind itself — this is required, not optional.** After
  adding, removing, or moving ANY bind, update all surfaces it touches:
  the main-card Ctrl+/ overlay (`src/ui/keybinds_overlay.rs` — keycap strip
  AND describe() arm) for main-card binds, and the per-overlay legend
  (`src/ui/{gloss,synopsis,journal,chat,echo}_keybinds_overlay.rs` GROUPS +
  MRU consts) for any overlay bind. A change that shifts a chord's meaning
  on multiple surfaces must touch EACH affected legend — including
  reserved-key comments and consumed-no-op notes that the change
  invalidates. The per-surface mechanics are spelled out in the bullets
  below; run the `update-cairo-keybinds-overlay` three-pass cross-reference
  to confirm nothing drifted. (`docs/guides/keybind-surface-guide.md`
  remains the one exception — on-request only.)
- **ALWAYS consult `docs/guides/keybind-consistency-guide.md` when changing
  keybinds.** It holds the app's key→concept map (`r`=vocab, `g`=gloss,
  `j`=journal, `w`=rewrite, `a`=ask, `/`=search+legend), the modifier
  conventions, and the ranked list of known inconsistencies. Before adding a
  bind, check which key already owns that concept and put the new bind on it.
  **Proactively propose reorganizing binds** to help the user remember them —
  when a change lands near a known inconsistency, or when you notice a concept
  scattered across keys or a cap carrying unrelated meanings, surface a
  consolidation proposal (don't silently rebind — reorganizations are the
  user's call). Record each approved consistency decision in that guide's
  change log, and run its sweep procedure as a self-check after multi-surface
  bind changes.
- **The spacebar is `"space"`.** The Rust code binds it by that keysym name,
  so refer to it as `"space"` — never `"Space"` or `"spacebar"`. When the user
  asks to change the space bind, respond using `"space"`.
- **Layout is Real Programmers Dvorak (RPD)**, defined in `~/utono/rpd`.
  Always check it when adding/changing binds — the GTK key name a physical
  key emits is not obvious (`(` → `parenleft`, `'` → `apostrophe`).
  - **Authoritative source: the xkb symbols file**
    `~/utono/rpd/xkb/usr/share/X11/xkb/symbols/real_prog_dvorak`. Each
    `key <CODE> { [ level1, level2, ... ] }` row lists the keysyms a physical
    cap emits at each shift level. `<CODE>` is the QWERTY position of that key
    (`<TLDE>` = the QWERTY `` `/~ `` cap, `<AE04>` = QWERTY `4`, etc.), so a
    glyph does NOT sit where QWERTY puts it — e.g. `<TLDE>` on RPD emits
    `dollar` (level 1) / `asciitilde` (level 2).
  - **A symbol on level 1 is UNSHIFTED** — it needs no Shift, so its Ctrl and
    Ctrl+Shift chords are distinct (that is why `$` supports both
    `KeyCombo::ctrl("dollar")` and `KeyCombo::ctrl_shift("dollar")`). A symbol
    that only exists on level 2 (e.g. shift+minus → `underscore`) can only be a
    shifted chord. Confirm the level before assuming two directions fit on one
    cap.
- **Source of truth is the Rust source** (`keymap_config.rs` + the handlers
  keymap.rs dispatches to). Never the keybinds.db / `keybinds-search` skill,
  and never key names written in prose docs — they drift.
- **`~/.config/linux-lit/keymap.json` overrides compiled defaults** (stowed
  from `~/tty-dotfiles/linux-lit/`; deploy: `cd ~/tty-dotfiles && stow
  linux-lit`). When changing binds, ALWAYS update both, or the JSON silently
  shadows the compiled change. Bindings are
  `{"key": "x", "action": "PageForward"}` + optional `"ctrl"/"shift"/"alt"`;
  action names = `Action` variants; unknown names are skipped with a warning.
- **Every keybind change also updates the Ctrl+/ overlay**
  (`src/ui/keybinds_overlay.rs` — keycap strip AND describe() detail arm).
  It shows MAIN-CARD binds only (no overlay-context entries). It is a
  hand-maintained mirror; use the `update-cairo-keybinds-overlay` skill and
  run its three-pass cross-reference.
- **The gloss/synopsis/journal/chat/echo overlays have their OWN Ctrl+/
  legends** (`src/ui/{gloss,synopsis,journal,chat,echo}_keybinds_overlay.rs`,
  `GROUPS` + MRU consts); their binds live in the overlay modal handlers,
  not keymap_config. Update the legend in the same change as the handler.
- **`docs/guides/keybind-surface-guide.md` is updated on request ONLY** —
  never automatically as part of a keybind change (rule flipped
  2026-07-22). When explicitly asked to refresh it, follow the template
  in its intro (one `##` section per bind). It is expected to lag the
  source; the Rust source stays the truth.

## Concordance & vocab

- The concordance picker opens a stopword-filtered word list for the current
  author; hit stepping is cross-work (loads the work in-place, seeks MPV to
  the hit line's own start time). Word list cached per author.
- Vocab word jumps enter the **vocab-sentence loop mode** on works whose
  playing media has `phrase_timestamps` (gapless MPV ab-loop per sentence,
  fully modal). See `src/input/vocab_loop.rs`.
- Cross-work jumps open the media picker (single-media works auto-select);
  `concordance_state` persists until a new word is selected.

Key files: `src/input/actions/concordance.rs`, `src/concordance.rs`,
`src/db/concordance.rs`, `src/ui/concordance_picker.rs`

## Pagination & Scene Boundaries

**Boundaries are authoritative metadata, never inferred from text.** A
boundary is where a line's `(div1, div2)` changes; `build_line_map`
precomputes `LineMap.section_starts`, read via `AppState::is_section_start` /
the `section_break_fn` closures in `viewport.rs`. Text classifiers
(`is_act_scene_marker`/`is_separator`) are for building the bitmap, display,
and mid-load fallback only. General rule: if lit.db encodes a per-line fact,
surface it through `LineMap`/`Line` — never reconstruct it from buffer text.
This applies to TEST ASSERTIONS too: a nav-fuzz UNBALANCED/short-column FAIL
at a scene edge usually means the assertion (not production) re-inferred the
boundary. See `docs/troubleshooting/page-turning-mechanics.md`; bump
`snapshot.rs SNAPSHOT_VERSION` when LineMap's serialized shape changes.

**Pinned pagination:** two-column plays (and row-fill prose) read spreads
from lit.db `play_pages`/`prose_pages` (`src/input/page_table.rs`;
`PAGES: table hit/fallback/generated` log lines say which engine is active).
Flags: `LIT_NO_PAGE_TABLE=1` forces the live engine, `LIT_GEN_PAGE_TABLE=1`
forces generation at current geometry. Audit with `validate-play-pages`.

## MPV Integration

- MPV is reused across work switches via `loadfile replace`; state on
  `AppState.mpv_connected` / `mpv_playing`; `pending_loadfile_seek` fires on
  the first TimePos after loadfile (event-driven).
- Sockets: `/tmp/mpvsocket-{author}-{basename}`; instance slots ≥2 prefix
  `i{n}-` so parallel instances never share a player.
- After jumping the cursor far, use `update_highlight_and_center` (NOT
  `center_cursor` alone — it doesn't update `page_top_line` and desyncs
  pagination state).

## External Data & Config

- Database: `~/utono/litdb/data/lit.db` (read-write). Theme palettes:
  `~/utono/themes/.config/themes/themes-unified.json` (read-only).
- **Never rename a work's abbrev with raw SQL** — `works.abbrev` is the
  de-facto FK for ~15 tables plus the snapshot cache and config; use the
  `rename-work-abbrev` skill in litdb (close linux-lit first).
- **Dev and release use SEPARATE config files** (`config-dev.json` for
  `cargo run`, `config.json` for release), each rewritten on exit. A stored
  value always beats a compiled default — to change a dev setting, edit
  `config-dev.json` while NO dev instance runs. When a session uses an
  unexpected value, check `config-dev.json` first.
- **ALWAYS keep `config.json` in sync with `config-dev.json`.** Any change to
  a shared setting in one file gets mirrored into the other IN THE SAME
  CHANGE — a dev-only tweak that never reaches release is drift, not a
  setting. This covers every key EXCEPT the per-session ones below, which are
  meant to differ per instance and must be left alone:
  - `theme` (active theme — the LIST `theme_cycle` still syncs)
  - `last_work`, `previous_work`, `recent_works`, `last_gloss`
  - `work_positions`, `work_position_ids`, `last_column_count`
  - `chapter_toast_shown`
  Procedure: no instance of the TARGET build may be running (it rewrites its
  config on exit — release binary for `config.json`, `cargo run`/
  `target/debug` for `config-dev.json`); back the file up; edit with `jq`;
  re-read to confirm. A key present in only one file is drift — add it to the
  other rather than assuming it is build-specific.
- **Reader theme is INDEPENDENT of the system-wide theme system**: stored in
  the app's own config (`theme` + `theme_cycle`; current defaults in
  `src/config.rs`). SIGUSR1 re-reads the app's own config. linux-lit never
  touches the theme system's `.current_theme`.
- **Upstream root-cause routing**: when a reader bug root-causes to
  lit.db data (litdb) or timestamp output (whisper-transcript), the fix
  and its regression guard land in the UPSTREAM repo; this repo gets
  only a troubleshooting-ledger entry linking to the upstream commit.
  Never patch around an upstream defect in reader code.

## Reference Codebases

`docs/reference-codebases.md` catalogs the read-only reference readers at
`~/Documents/repos/linux-lit/` (foliate/foliate-js, lue, bk, openreader,
html5-audio-read-along, transcript-tracer-js) and which file to read per
problem area. Patterns only — never import code. See also the
`review-against-references` skill.

## Memory Bank System

Check these context files before starting work and keep them updated:
**CLAUDE-activeContext.md** (read FIRST — session continuity),
CLAUDE-patterns.md, CLAUDE-decisions.md, CLAUDE-troubleshooting.md,
CLAUDE-config-variables.md.
