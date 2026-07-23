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

## To-Do List

`docs/to-do/to-do.md` is the running list of reader bugs and feature requests.
Mark completed items with a leading `[X]`; never delete them.

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

## Build & Run

Verify with `cargo build`; do NOT run the app — the user runs `cargo run`
themselves. Multi-instance is supported (per-process slots: own log suffix,
`i{n}-` MPV sockets, config merge-on-save), but a running instance predates
any rebuild.

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

## Headless Verification (agent self-check)

Agents verify GUI changes WITHOUT touching the live session by running the
reader inside a throwaway `cage` compositor and screenshotting with `grim`.
**This works from the agent's own shell — first resort for any on-screen
acceptance criterion.** Fall back to asking the user only when a launch
genuinely fails after a retry (`/tmp/cage.log` dead, or repeated empty PNGs).

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
  `wlr-randr --output HEADLESS-1 --custom-mode 1920x1200`.
- Give the window ~3s to map before `wtype`. An empty ~2-byte PNG from `grim`
  means not-mapped-yet, NOT failure — sleep 3 and retry; check `stat -c%s`
  before Read-ing. Modifier chords: `wtype -M ctrl -k j -m ctrl`.
- Key names drift — confirm current binds in `src/input/keymap_config.rs`
  before scripting a drive. Stable gotchas: front matter (before Chapter 1)
  has no synopsis; overlay-open line-scroll keys scroll the overlay; Escape
  closes it.
- A cage run takes instance slot 2+ (own `-{n}` log, `i{n}-` MPV sockets).
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
- **The gloss/synopsis/journal overlays have their OWN Ctrl+/ legends**
  (`src/ui/{gloss,synopsis,journal}_keybinds_overlay.rs`, `GROUPS` consts);
  their binds live in the overlay modal handlers, not keymap_config. Update
  the legend in the same change as the handler.
- **When updating keybinds, also update
  `docs/guides/keybind-surface-guide.md`** — it documents per-surface bind
  behavior (one `##` section per bind, per the template in its intro). Edit
  the bind's section if it has one, add a section for newly documented
  binds, and keep it in the same change as the handler edit.

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
- **Reader theme is INDEPENDENT of the system-wide theme system**: stored in
  the app's own config (`theme` + `theme_cycle`; current defaults in
  `src/config.rs`). SIGUSR1 re-reads the app's own config. linux-lit never
  touches the theme system's `.current_theme`.

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
