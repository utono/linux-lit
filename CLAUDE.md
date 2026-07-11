# linux-lit

GTK4 Rust literature reader with e-reader pagination, MPV audio sync, and vim-style navigation.

## Active Context (read first)

**At the start of every session, read `CLAUDE-activeContext.md` if it exists**
(in this project root). It records the current branch, in-progress/uncommitted
work, recent decisions, and the ordered next actions — none of which are
recoverable from the code or git history alone. Resume from its "Next Actions".

**`ac` is the alias for `CLAUDE-activeContext.md`.** When the user says "update
ac", "read ac", "check ac", etc., it means `CLAUDE-activeContext.md`. Keep it
current as work progresses: record only what is NOT recoverable from the code or
git (uncommitted work, why a decision was made, what to do next); convert
relative dates to absolute (US Central). Update it before a likely context break
(reboot, compaction, end of a work block).

**Do NOT update `ac` automatically after every commit.** Update it only when the
user asks ("update ac" / "uac"), or before a likely context break. `ac` is a
SCOPED continuity file, not a changelog — a per-commit summary is recoverable
from `git log`, so do not accumulate one here. Keep `ac` short: the current state
+ any uncommitted work + next actions + genuinely non-recoverable decisions/
gotchas. When you refresh it, REPLACE stale state rather than prepending a new
block on top of the old ones. (This project deliberately opts OUT of the global
`~/CLAUDE.md` "After a Commit" rule.)

## To-Do List

`docs/to-do/to-do.md` is the running list of reader bugs and feature requests.
**Update it as to-dos are completed:** when an item is done, put a `[X]` to
the left of the item (at the start of its first line). Do not delete
completed items — the `[X]` marks them done.

## Debug Log

The app writes debug logs to:

- **Dev build** (`cargo run`): `~/utono/linux-lit/linux-lit-dev.log`
- **Release build**: `~/utono/linux-lit/linux-lit-release.log`

The log is cleared on every app launch. Use `log_fmt!()` macro (from `src/logging.rs`) to add log lines.

When fixing bugs, **always read the log first** before proposing changes:

```bash
cat ~/utono/linux-lit/linux-lit-dev.log
```

### GTK/GLib runtime warnings go to stderr, not the app log

The app's `linux-lit-dev.log` only holds `log_fmt!()` lines. **GLib/GTK
diagnostics** — `GLib-GObject-CRITICAL`, `Gtk-WARNING`, `g_object_unref`
assertions, GTK abort backtraces — are printed to the process **stderr**.
Capture them with `cargo run 2>&1 | tee ~/utono/linux-lit/linux-lit-dev-stderr.log`.
To investigate them (separating GLib criticals from cargo dead-code noise and
proposing fixes), use the **`check-stderr-log`** skill.

## Playback Sync Bugs — identify the PLAYING work FIRST

For ANY playback-sync bug (highlight jumps to the wrong line, seeks to the wrong
place, page turns early/late, cursor lands on a stray line during MPV playback)
the FIRST step is to identify **which work is currently playing** — not the work
the user names casually, and not the base work. A play often has several editions
sharing the same text (`Cym`, `Cym-Amb`, `Cym-BBC`), each with its OWN media file
and its OWN `line_timestamps` rows. Sync is driven entirely by the playing
edition's timestamps, so diagnosing against the wrong abbrev inspects the wrong
data and leads nowhere.

Confirm the abbrev before touching the log or DB:

- Ask the user, or read the title bar / library picker in the screenshot
  (e.g. "Cymbeline (BBC Radio)" → `Cym-BBC`, not `Cym`).
- The debug log's `SEEK:`/`CURSOR_LINE:` lines and the loaded media path pin it
  down; the media file lives at `media_files.path` for that abbrev.

Then run the sync queries (see the `debug-playback-sync` skill) against **that**
abbrev's `line_mapping` + `line_timestamps` + `media_files.id`. A common root
cause is a single corrupt/out-of-order timestamp in the playing edition whose
`start_time` falls inside an earlier line's audio window — that stray line gets
highlighted while the earlier line is actually being spoken. Production editions
(`-Amb`, `-BBC`) are aligned by the `wizard-ambrose` skill in litdb; a
backwards-in-time timestamp there is a wizard/import defect, fixed in lit.db (and
in that skill's monotonicity gate), not in linux-lit code.

## Clipping Bugs — read clip-prevention.md FIRST

For ANY text-clipping or flush-to-the-edge bug — a half-cut line at the top or
bottom of a card, text touching a card edge with no gap, a partial row poking
under a footer, "not enough padding at the bottom", or text rendering behind an
overlay — **always read `docs/troubleshooting/clip-prevention.md` before
proposing or attempting a fix.** It is the consolidated reference for every
free-scroll surface AND the main reading card's paginated clip, with a
frequency-ordered failure checklist that names the usual culprits (missing
`value_changed` path, uniform row-step cutting descenders, occlusion-not-clipping,
the over-tall single prose paragraph). Skipping it leads to guess-and-check
cycles and fixes that re-cut glyphs (e.g. a fixed-pixel reserve instead of a
clean visual-row boundary). When the visible result contradicts the logged clip
value, launch with `LIT_DEBUG_CLIP_COLOR='#ff0000'` to paint every bottom-clip
box for that run (no theme.rs edit needed) — that single screenshot
distinguishes "clip is 0" from "clip is mis-sized."
Clipping acceptance is pixel-level: verify on the real display (or the
`line_clipping` / `overlay_clipping` e2e invariants), never from logs alone.

## Build & Run

Verify changes compile with `cargo build` but do not run the app — the user will run `cargo run` themselves.

**Important:** `cargo run` is for development only. Only run one instance at a time — multiple instances share the same log file and database, and restarting one won't update the other.

```bash
cargo build
```

## Testing

```bash
cargo test
cargo clippy
```

## Headless Verification (agent self-check)

The standing rule is "do not run the app — the user runs `cargo run`." The
exception below lets an agent verify GUI changes **without touching the user's
live session**, by running the reader inside a throwaway headless compositor
(`cage`) on its own Wayland socket and screenshotting it with `grim`.

**This DOES work from the agent's own shell — prefer it over asking the user.**
It has been run successfully mid-session (nested `cage` on `wayland-1` while the
user's dwl held `wayland-0`, app rendered, `wtype` drove it, `grim` captured a
real screenshot). Earlier notes saying "the agent can't launch cage / gets
SIGTERM'd" are **stale/overcautious** — treat headless verify as a first resort
for any on-screen acceptance criterion, and only fall back to asking the user
(see *When to ASK THE USER* below) if a launch genuinely fails after a retry.
The two gotchas that make it *look* broken are both handled below: the first
`grim` firing before the surface maps (empty ~2-byte PNG — retry after a few
seconds), and the cleanup `pkill` matching the user's live instance (scope it).

**Why this and not the live dwl:** on the user's dwl session a new reader window
opens on a non-visible tag, so `grim` (which captures the *active* output) can't
see it, and the user's seat is already owned — so `ydotool` and
`WLR_BACKENDS=headless,libinput` both fail with "seat busy". A nested headless
`cage` sidesteps all of that.

**Required tools** (already installed): `cage`, `grim`, `wtype`. Documented in
`~/utono/ccinstall/paclists/`.

### Launch the reader headless

```bash
cd ~/utono/linux-lit && cargo build
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
```

- `GSK_RENDERER=cairo` is **mandatory**: the default GTK renderer tries Vulkan,
  loses its surface on the headless backend, and the reader aborts with a Rust
  stack overflow. Cairo (software) renders cleanly.
- `WLR_RENDERER=pixman` keeps wlroots on software rendering too.
- Cage opens a fresh Wayland socket, normally `wayland-1` (it does **not** honor
  a `WAYLAND_DISPLAY` you pass in for its own server socket — check
  `ls /run/user/1000/wayland-*` for the new one).
- **The headless output defaults to 1280×720** (wlroots headless backend
  default; cage has no size flag) — but cage implements wlr-output-management,
  so it can be resized live to the real display's geometry (verified
  2026-07-04; the app re-lays-out and `grim` captures at the new size):

```bash
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
```

  Use this whenever the acceptance criterion depends on the production
  geometry (pagination boundaries, page tables, spread balance) — a 720p run
  paginates differently than the user's 1920×1200 session.

### Capture and drive

Wait for the socket, then give the window ~3s to map and gain focus before
sending keys (premature `wtype` is dropped — this caused early false negatives):

```bash
export WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000
grim /tmp/shot.png                 # screenshot the reader
wtype "3"                          # send keystrokes to the focused reader
```

- **Empty ~2-byte PNG = the surface hadn't mapped yet, NOT a failure.** `grim`
  returns exit 0 with a near-empty file if it fires before the reader paints.
  `sleep 3` and re-`grim`; a real capture is tens-to-hundreds of KB. Always
  check `stat -c%s` before `Read`-ing the PNG.
- `wtype` works (virtual-keyboard protocol, no seat needed) **once the window is
  focused**; `ydotool`/libinput do not (seat owned by dwl). Modifier chords use
  `-M`/`-m`, e.g. `wtype -M ctrl -k j -m ctrl` for Ctrl+j.
- **Log location depends on the instance slot.** With the user's live instance
  running (it holds slot 1), a manual cage run on the same
  `XDG_RUNTIME_DIR=/run/user/1000` takes slot 2 and writes
  `linux-lit-dev-2.log` — a separate file, so tails no longer interleave. The
  cage run's MPV sockets are likewise `i2-`-prefixed, so it can no longer
  touch the live session's players. Prefer `LIT_LOG_PATH` anyway for an
  explicit per-run log; trust the screenshot over any log when in doubt.
- Then `Read` the PNG to inspect the result.

### Useful key sequences for verification

- `5` / `4` — next / previous chapter (jumps the cursor onto the `CHAPTER N`
  heading). `3`/`2` step scenes/sections instead (`2` = first line of the
  current scene/chapter, thereafter previous; `3` = next); bookmarks are on
  `&`/`(`. Front matter (before Chapter 1) has chapter number 0 and no
  synopsis, so `h` shows nothing there — advance into a chapter first.
- `h` — open the synopsis overlay for the current chapter; `Ctrl+g` glosses.
- `j` / `k` — scroll. While an overlay is open these scroll the overlay; with no
  overlay they scroll the reading buffer. To stress overlay top/bottom clipping,
  open the overlay then `j` repeatedly to reach the last line.
- `Escape` — close the overlay.

### Clean up

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

**Kill ONLY the cage-scoped pattern.** Do NOT run a bare
`pkill -f target/debug/linux-lit` — that also matches the user's **live**
`cargo run` instance (same binary path) and kills it out from under them. Killing
the `cage --` parent takes its child reader down with it, so the narrow pattern
is sufficient. If a headless reader somehow outlives its cage, target it by the
specific PID (`pgrep -f "cage -- ./target/debug/linux-lit"` first), never by the
shared binary-path pattern. (`ydotoold` is not needed for this flow; only `cage`
+ `wtype` + `grim`.)

For the **automated** equivalent of this manual self-check, see *Automated UI
tests* below — it wraps the same cage + grim + wtype flow in `cargo test` and
adds a fail-closed line-clipping assertion.

## Automated UI tests (cargo)

`tests/harness/mod.rs` + `tests/smoke.rs` + `tests/line_clipping.rs` are a
headless UI test harness: each test runs the app inside its **own isolated
`cage`** (a temp `XDG_RUNTIME_DIR`, never the live session), screenshots with
`grim`, drives input with `wtype`, and asserts the main reading card never clips
its first/last line.

```bash
# everything (provides the a11y bus + software GL the artifacts want):
./scripts/e2e-env.sh cargo test -- --ignored --nocapture

# just the clipping invariant:
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
```

Tests are `#[ignore]`d so a bare `cargo test` stays green without cage/grim/wtype.
Deps (pacman/AUR): `cage`, `grim`, `wtype`, `python-pillow`, `python-numpy`,
`at-spi2-core`, `dbus` (the AT-SPI bits are only needed by `annotate_ui.py`'s
best-effort overlay; the clipping detector itself is pure-pixel).

Design notes (so you don't re-derive them):

- **cage, not bare dwl/sway.** linux-lit only lays out + paints once it gets a
  configured, focused, fullscreen surface. cage gives the single client exactly
  that; bare dwl/sway on the headless backend leave the window unsized so the
  reveal hits its 5s "load may be stuck" fallback and renders blank.
- **`GSK_RENDERER=cairo` is mandatory** (set by the harness): the default
  Vulkan/ngl renderer loses its surface on the headless backend and the app
  aborts with a stack overflow.
- **MPV is skipped in tests.** The harness sets `LIT_HEADLESS_TEST=1`;
  `launch_mpv` then does not spawn MPV at all — otherwise its window covers the
  reader in the test compositor and the process leaks across runs.
- **Region via the app, not AT-SPI.** linux-lit's `sourceview5::View` exposes no
  AT-SPI Text interface, so the clipping detector can't auto-find the pane. On
  reveal (under `LIT_HEADLESS_TEST`) the app logs `TEST_VIEWPORT_RECT x y w h`
  (window == screenshot coords); the harness reads it and passes `--region`.
- **Keys** are RPD: top `gg` (two presses), page `x`/`y`, end `shift+G`, line
  `j`/`k`. They land on the window's global capture-phase controller — no
  Tab-focus step.
- Scope: the tests cover the **main reading card**. The synopsis/gloss overlay
  has its own scroll/clip path and would need an `h`-open step + its own region.

### When to ASK THE USER to run e2e-env.sh

**Default: TRY the headless launch yourself first** (manual cage per *Headless
Verification* above, or `e2e-env.sh` for the cargo harness). It generally works
from the agent shell — a nested `cage` does not need the seat. Only fall back to
asking the user when a launch **actually fails after a retry**: the `cage --`
process dies immediately (check `/tmp/cage.log`), or `grim` keeps returning an
empty PNG after a few `sleep`+retry cycles (a real map failure, not the
first-shot timing issue). If it truly won't launch, do **not** claim the change
is verified — build, run `cargo test --bins` (the pure-logic suite), state
plainly that the launch failed, and **ask the user to run the e2e command** and
paste the result / screenshot.

The old blanket claim "an agent cannot launch cage — the seat is owned / it gets
SIGTERM'd" is **stale**; don't skip straight to asking. The SIGTERM/exit-144 case
was usually the cleanup `pkill` (or a prior instance) killing the run, not the
seat — launch first, and scope the eventual cleanup to `cage -- ...` only.

Even when you CAN verify headlessly, the manual single-work launch is still worth
handing to the user for a final eyeball on the real GL renderer (cage uses cairo
software rendering). Reach for the user whenever the change's acceptance criterion
is "it renders correctly on screen" rather than "the logic is right":

- **pagination / spread / column-split / page-turn** changes — boundaries are
  computed from live Pango pixel heights against `text_view`/`right_view`; there
  are deliberately no pure unit tests for `column_split`/`last_page_top`, so the
  only real check is a rendered spread. For these, the **nav-fuzz** is the
  workhorse — it drives every nav action and asserts on-page landing, balanced
  columns, and `G`/jump-to-end idempotency. It lives in the
  test-headless-navigation skill and **must** be launched through the env
  wrapper:

```bash
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work <ABBR>
```

  **Always pass `--start-work <ABBR>`** when reproducing a work-specific failure
  — without it the run loads the dev config's `last_work`, which a headless run
  rewrites on exit, so the bug silently moves works and can look "fixed". Add
  `--secs N` to shorten the run (default ~330s). The FAIL summary prints to the
  terminal and the full log lands at `/tmp/fuzz-nav.log`.
- **clipping / bottom-clip / descender-guard** changes — pixel-level, only
  visible in a screenshot.
- **overlay layout** (synopsis, gloss, pickers, keybinds overlay) — geometry
  only settles in a mapped, focused, fullscreen surface.
- **startup / resume / reveal-timing** changes — the correction paths
  (`snap_near_end_to_canonical`, the resize tick) only run at settled geometry
  after reveal; their effect can't be observed without a launch.

You do **not** need the user when `cargo test --bins` already covers the change
(pure helpers, parsing, DB queries, state machines with no GTK measurement).

Give the user the exact command, e.g.:

```bash
./scripts/e2e-env.sh cargo test --test <name> -- --ignored --nocapture
```

and, when the criterion is visual, the manual single-work launch from
*Headless Verification* above (`LINUX_LIT_WORK=… LIT_START_POS=… cage -- …` +
`grim`) so they can eyeball the exact spread.

## UI review protocol

After any e2e run, screenshots land in `target/ui/` (auto-cleaned at the start
of each run, so the directory only holds the current run's captures). **Open
every PNG — and any `_clip.png` overlay — and report what you see inline** in
your reply: quote the on-screen text and call out any clipping or layout problem
by eye. A passing exit code is not enough; clipping/layout bugs are caught by
looking. No written review file is required (there is no longer a `Stop` hook
gating this).

## Key Files

- `src/main.rs` — entry point, Tokio runtime, channel bridge, MPV event loop (TimePos, PlaybackState, ConnectionStatus)
- `src/app.rs` — GTK4 window, AppState, display_work, clear_display, prepare_text_for_display
- `src/config.rs` — ~/.config/linux-lit/config.json persistence
- `src/input/keymap.rs` — key event routing, gg state machine, dispatch_action
- `src/input/keymap_config.rs` — compiled-in default keybinds, keymap.json loader
- `src/input/navigation.rs` — cursor movement, page turns, scroll logic
- `src/input/actions/mod.rs` — Action enum with all reader-mode actions
- `src/input/actions/concordance.rs` — concordance picker, cross-work navigation, r/R handlers
- `src/input/actions/pickers.rs` — library/media/bookmark picker open/confirm handlers
- `src/input/highlight.rs` — update_highlight, update_highlight_and_center
- `src/input/scroll.rs` — set_page, set_page_instant, center_cursor
- `src/concordance.rs` — ConcordanceState, ConcordanceHit, advance/retreat
- `src/db/queries.rs` — SQLite queries (list_works, load_work)
- `src/db/concordance.rs` — find_word_occurrences, load_concordance_words
- `src/db/stopwords.rs` — English stopword list for concordance filtering
- `src/db/line_types.rs` — dialogue classification
- `src/mpv/client.rs` — MPV IPC command handler (Seek, LoadFile, ResumeAndSeek, Connect, Quit)
- `src/mpv/commands.rs` — MpvCommand and MpvEvent enums
- `src/mpv/discovery.rs` — derive_socket_path, find_socket_for_work, launch_mpv (skips MPV under `LIT_HEADLESS_TEST`)
- `tests/harness/mod.rs` — headless cage harness: screenshot/input/clipping helpers
- `tests/line_clipping.rs` — the core no-clip invariant (top/mid/end)
- `scripts/check_line_clipping.py` — fail-closed pixel line-clipping detector (`--region`)
- `scripts/e2e-env.sh` — headless WLR/GTK env + dbus + AT-SPI registry wrapper
- `src/input/scroll.rs::emit_test_viewport_rect` — logs `TEST_VIEWPORT_RECT` for the harness
- `src/ui/library_picker.rs` — Ctrl+p work picker with fuzzy filter
- `src/ui/concordance_picker.rs` — Ctrl+\ concordance word picker
- `src/ui/media_picker.rs` — Ctrl+Shift+M media file picker
- `src/logging.rs` — file-based debug logging

## Keyboard Layout

The user's keyboard layout is Real Programmers Dvorak (RPD), defined in
`~/utono/rpd`. **Always check `~/utono/rpd` when adding or changing keybinds** —
on RPD, characters like `[`, `{`, `(`, and `4` may sit on separate physical
keys (not shift-related), and the GTK key name a physical key emits is not
always obvious from the character. Consult the layout there to map a character
to its physical key and the GTK key name to use in `keymap_config.rs` /
`keymap.json` (e.g. `(` → `parenleft`, `'` → `apostrophe`).

## Searching for Keybinds

When searching for a keybind in linux-lit, **always check source** — primarily `src/input/keymap.rs` and the handlers in `src/input/` it dispatches to. **Do not use the `keybinds-search` skill or query `~/utono/keybinds/keybinds.db`** for this project; that database is not the source of truth for linux-lit binds and may be stale or incomplete. The Rust source is authoritative.

## Concordance System

Cross-work concordance navigation for searching word occurrences across an author's works.

- **Ctrl+\\** — opens concordance picker with stopword-filtered word list for the current author
- **R** — previous concordance hit (cross-work, loads new work in-place). Falls back to "no concordance active" toast if no word selected. Seeks MPV to the hit line's own start time (not sentence start). ConcordanceNext is deliberately unbound (plain `r` now cycles the vocab popup; rebind via keymap.json if needed).
- **Ctrl+- / Ctrl+Shift+R** — next/prev vocab word jump (always, ignores concordance state). On works whose playing media has `phrase_timestamps`, these instead enter the **vocab-sentence loop mode**: the sentence containing the vocab word repeats gaplessly (MPV ab-loop) with a static sentence tint (no karaoke sweep); `n`/`p` step between vocab sentences, `a`/Space pauses, Escape/Ctrl+- exits (Ctrl+r kept as a legacy exit; fully modal). See `src/input/vocab_loop.rs`.
- **r / Ctrl+r** — vocab popup: `r` cycles the popup's words (sticky, no auto-hide; follows the cursor/playback line), `Ctrl+r` fades it out.
- Word list is cached per author in `AppState.concordance_word_cache`
- Cross-work jumps open the media picker so the user chooses the audio file
- Single-media works auto-select without showing the picker
- `concordance_state` persists until a new word is selected

Key files: `src/input/actions/concordance.rs`, `src/concordance.rs`, `src/db/concordance.rs`, `src/ui/concordance_picker.rs`

### Keybind override: keymap.json takes precedence

Compiled-in defaults in `keymap_config.rs` are overridden by `~/.config/linux-lit/keymap.json` (stowed from `~/tty-dotfiles/linux-lit/`). When changing keybinds, **always update both files** or the JSON will silently override your compiled changes.

### Always update the Ctrl+/ overlay too

Adding, removing, or changing ANY keybind also requires updating the
Ctrl+/ keybinds overlay in `src/ui/keybinds_overlay.rs` — both the keycap
strip and the per-key **detail panel**:

- Update the key's `KeyDef` (`action` / `shift_action` / `modifiers`) in the
  right row table (`NUMBER_ROW`, `UPPER_ROW`, `HOME_ROW`, `BOTTOM_ROW`,
  `MOD_SEQ_ROW`, or a row-leader const) so the cap and detail rows render the
  change.
- Add or update the `describe()` arm for every label you introduce, so the
  detail panel shows a full description (and the `-> handler — src/path`
  reference) for that key. A label with no arm renders blank in the detail
  panel; a real binding with an empty slot renders a blank detail row.

The overlay is a hand-maintained mirror with no compile-time enforcement, so it
drifts silently. **Use the `update-cairo-keybinds-overlay` skill** — it carries
the mandatory exhaustive cross-reference (three passes: no blank slot hides a
real binding; no label names the wrong action; every label has a `describe()`
arm) that catches missing/wrong descriptions. Run it after any keybind change so
every bind — and each modifier variant on each key — is represented and
described.

### Overlay keybind legends drift too (gloss / synopsis / journal)

The gloss, synopsis, and journal overlays each have their OWN per-overlay Ctrl+/
keybind legend, separate from the reader card's `keybinds_overlay.rs`:

- `src/ui/gloss_keybinds_overlay.rs`
- `src/ui/synopsis_keybinds_overlay.rs`
- `src/ui/journal_keybinds_overlay.rs`

Each defines a `GROUPS` const (grouped `(key, action)` rows) rendered by the
shared `ui::keybinds_legend::build_legend`. Their binds are handled directly in
the overlay's modal key handler (e.g. `handle_gloss_key` in
`src/input/keymap.rs`), NOT in `keymap_config.rs` / `keymap.json`.

**When adding, removing, or changing ANY keybind for the gloss, synopsis, or
journal overlay, update that overlay's legend `GROUPS` in the same change.** Like
the reader overlay these are hand-maintained mirrors with no compile-time
enforcement, so they drift silently. Verify the legend's `(key, action)` text
against the actual handler arm (especially the direction/order in paired binds
like `Alt+n / Alt+p`).

## Pagination & Scene Boundaries

**Scene/section boundaries are authoritative metadata, not inferred from text.**
A boundary is exactly where a line's `(div1, div2)` changes (act, scene). At load,
`build_line_map` precomputes `LineMap.section_starts: Vec<bool>` (the FIRST buffer
line of each `(div1,div2)` run); all pagination reads it via
`AppState::is_section_start` / the `section_break_fn` closure threaded into the
pure helpers in `viewport.rs` (`clamp_at_section_break`, `back_up_for_speaker`,
the right-column "begins a new scene" check, `scene_header_top`, `scene_snap_top`).

**Do NOT re-infer a boundary from buffer text** (`line_types::is_act_scene_marker`
/ `is_separator`) in any pagination path. Those text classifiers are for BUILDING
the bitmap, for *display* (title bar, synopsis), and as a mid-load fallback only.
Re-inferring structure that the DB already encodes is what caused the long
`y GAP` / wrong-spread bug class; the fix was to read `(div1,div2)`. General rule:
if `lit.db` already encodes a per-line fact (boundary, chapter, dialogue,
spoken-status), surface it through `LineMap`/`Line` and read it — never
reconstruct it by classifying buffer text. See
`docs/troubleshooting/page-turning-mechanics.md` → "The authoritative-boundary
principle" and the snapshot version (`snapshot.rs SNAPSHOT_VERSION`) which must be
bumped when `LineMap`'s serialized shape changes.

**The rule applies to test assertions too, not just pagination.** The nav-fuzz
UNBALANCED-SPREAD check in `nav_test.rs` exempted scene-clamped spreads by
classifying buffer text (`is_act_scene_marker`/`is_separator`), so it flagged a
short right column at any boundary whose new scene opens on a stage direction +
speaker with no `ACT`/`SCENE` chrome line. **2H6, Cor, and Ham were all the same
false-positive class** — real `(div1,div2)` boundaries (e.g. 2H6 4.7→4.8) that
production's `clamp_at_section_break` clamps correctly but the text-classifying
exemption missed. The fix was to make the test read the authoritative
`section_starts` bitmap via `s.is_section_start` (the same source production
clamps on). Lesson: when a per-work nav-fuzz FAIL is an `UNBALANCED`/short-column
at a scene edge, first ask whether the *assertion* (not production) is
re-inferring the boundary from text.

**Pinned play pagination:** two-column plays at the pinned layout read their
spreads from lit.db `play_pages` (generated in-app, invariant-gated — see
`src/input/page_table.rs` and `docs/plans/2026-07-04-pinned-play-pagination-design.md`).
`PAGES: table hit/fallback/generated` log lines say which engine is active.
Test flags: `LIT_NO_PAGE_TABLE=1` forces the live engine;
`LIT_GEN_PAGE_TABLE=1` forces generation at the current (e.g. headless)
geometry. Audit with the `validate-play-pages` skill.

## MPV Integration

- MPV is reused across work switches via `loadfile replace` (no new process)
- `AppState.mpv_connected` tracks whether an IPC connection is active
- `AppState.mpv_playing` tracks playback state
- `AppState.pending_loadfile_seek` stores a deferred seek that fires on the first `TimePos` event after `loadfile` (event-driven, not timer-based)
- Socket paths are derived from media file paths: `/tmp/mpvsocket-{author}-{basename}`
- `display_work` skips MPV discovery when `skip_mpv_discovery` is set (used by concordance cross-work jumps that open the media picker instead)

Key files: `src/mpv/client.rs`, `src/mpv/commands.rs`, `src/mpv/discovery.rs`

### Scrolling after jumps

Use `update_highlight_and_center` (not `center_cursor` alone) when jumping the cursor to a distant line. `center_cursor` only sets the GTK vadjustment but doesn't update `page_top_line`, so the e-reader pagination state gets out of sync. `update_highlight_and_center` calls `set_page_instant` which updates both.

## External Data

- Database: `~/utono/litdb/data/lit.db` (read-write)
- Themes: theme color palettes are read (read-only) from
  `~/utono/themes/.config/themes/themes-unified.json`, but which theme is
  *active* is independent of the system-wide theme — see Reader theme below.
  linux-lit never reads or writes that system's `.current_theme` file.
- Config: `~/.config/linux-lit/config.json` (release) **or**
  `~/.config/linux-lit/config-dev.json` (dev / `cargo run`) — see gotcha below

### Renaming a work's abbrev is a multi-table migration — use the skill

`works.abbrev` is the de-facto foreign key for ~15 lit.db tables
(`line_mapping`, `media_files`, `work_media_associations`, `bookmarks`,
`characters`, `passages`, `chunks`, `echo_turns`, `scene_synopses`,
`passage_embeddings`, `attribution_sets`, ...). A bare
`UPDATE works SET abbrev=...` orphans every dependent row, the work loses its
`(div1,div2)` boundary metadata, and **pagination breaks** (anthology stops
putting one excerpt per column, plays show wrong spreads). The snapshot cache
(`~/.cache/linux-lit/snapshots/<abbrev>.text.bin`) and config (`last_work`,
`recent_works`, `work_positions`) are also abbrev-keyed and must move too.

**Never rename an abbrev with a raw `UPDATE works`.** Use the
`rename-work-abbrev` skill at
`~/utono/litdb/.claude/skills/rename-work-abbrev/` (it also optionally renames
the title). It migrates all dependent tables in one transaction, renames the
snapshot, and fixes the config — close linux-lit first so the app doesn't
clobber the config on exit:

```bash
~/utono/litdb/.claude/skills/rename-work-abbrev/rename-work-abbrev.sh --dry-run OLD NEW
~/utono/litdb/.claude/skills/rename-work-abbrev/rename-work-abbrev.sh OLD NEW ["New Title"]
```

### Dev vs release use SEPARATE config files

`config_path()` in `src/config.rs` selects the filename by build mode
(`crate::mode::is_dev_mode()`):

- **`cargo run` (dev)** reads/writes `~/.config/linux-lit/config-dev.json`
- **release build** reads/writes `~/.config/linux-lit/config.json`

Both files are independent, and **each rewrites its own file on exit** (the
"config clobbered on exit" behavior). Consequences when debugging:

- A **stored** config value always overrides the compiled-in `default_*` fn.
  Changing a default in `src/config.rs` only affects a config file that does
  NOT already have that key — once the app has run and persisted the key, the
  stored value wins. To change a setting for `cargo run`, edit
  **`config-dev.json`** (not `config.json`), and do it while **no dev instance
  is running** (a running instance re-clobbers the file on exit).
- When a `cargo run` session uses an unexpected value, check
  `config-dev.json` first — `config.json` is the wrong file in dev mode and
  will look "clean" while `config-dev.json` holds the real value. (This caused
  a long false hunt: a TTS default-voice change in `src/config.rs` had no
  effect because `config-dev.json` still pinned the old voice id.)

## Keymap Configuration

Reader keybindings are loaded from `~/.config/linux-lit/keymap.json` at
startup. If the file is missing or malformed, linux-lit falls back to
compiled-in defaults (see `src/input/keymap_config.rs:default_reader_bindings`).

### Stow workflow

The canonical default keymap is shipped as a stow package at
`~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`. Deploy with:

```bash
cd ~/tty-dotfiles && stow linux-lit
```

Restart linux-lit; the new bindings take effect on next launch.

### Customizing bindings

Edit `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (the stow
source). Each binding is an object: `{"key": "x", "action": "PageForward"}`.
Optional modifier flags: `"ctrl": true`, `"shift": true`, `"alt": true`.

Available actions are the variants of `crate::input::actions::Action` —
see `src/input/actions/mod.rs`. Unknown action names are skipped at load
with a logged warning; malformed JSON falls back to compiled-in defaults
entirely.

User overrides take precedence over defaults; bindings not present in
the JSON keep their compiled-in default.

### Reader theme

- linux-lit's theme is INDEPENDENT of the system-wide theme system. It is
  stored in `config.json` (`theme`, default `kindle-sepia`); `Alt+t` /
  `Alt+Shift+T` cycle `theme_cycle` (default: kindle-sepia, kindle-green,
  zenbones-light, zenwritten-light). SIGUSR1 re-reads the app's OWN config
  (external control: edit config.json, then `kill -USR1`). linux-lit never
  reads or writes `~/utono/themes/.config/themes/.current_theme`.

## Reference Codebases

When debugging or designing features that overlap with other ebook readers, consult these read-only checkouts at `~/Documents/repos/linux-lit/`. They are reference material, not dependencies — **never import code, only patterns**. Re-clone with `git clone <url>` into that directory if missing.

Pick the reference by problem area, not by language:

- **Pagination / clipping / page-turn math** → `foliate-js/`
- **Audio-text sync (the closest analog to linux-lit's MPV workflow)** → `lue/` first, then `html5-audio-read-along/`, then `transcript-tracer-js/`
- **Vim-style EPUB reading in Rust** → `bk/`
- **Whisper-driven word timestamps & per-document audio storage model** → `openreader/`
- **Annotations / highlights / location addressing / selection-tools UX** → `foliate/`

### foliate — `~/Documents/repos/linux-lit/foliate/` + `foliate-js/`

GNOME ebook reader, JavaScript/GJS + WebKitGTK + libadwaita (~8-10k LOC shell + ~9-11k LOC vendored renderer). Different rendering stack (CSS multi-column inside a WebView), but solves many of the same problems linux-lit faces.

- **Pagination edge cases** (clipped descenders, last-fully-visible-line, partial bottom lines, scroll-vs-page mode) — `foliate-js/paginator.js` (~44 KB). Different engine, transferable algorithm.
- **Location addressing** (portable bookmarks, sub-line precision, cross-device sync) — `foliate-js/epubcfi.js` (~13 KB) is the standard EPUB CFI implementation. Reference design if linux-lit ever needs more than `line_mapping.id`.
- **Annotations / highlights data model** — `foliate/src/annotations.js` (~25 KB): bookmark + named-color highlight + note schema, CFI-anchored, with export.
- **Selection-tools pattern** (Wiktionary, Wikipedia, translate as isolated modules with a uniform interface) — `foliate/src/selection-tools.js` and `foliate/src/selection-tools/*.html`.
- **EPUB Media Overlays / SMIL audio sync** — `foliate-js` SMIL modules. Reference only if importing timestamps from EPUB3 audiobooks.
- **Theme JSON schema** — `foliate/src/themes.js` and the user-themes-as-JSON pattern.
- **Not useful for:** library management (per-book JSON, no SQLite), library picker UI (WebView-based), vim navigation, MPV-driven sync, settings overlay (GSettings).

Quick map: app entry `foliate/src/main.js`, `app.js`. Reader: `foliate/src/reader/reader.html` + `reader.js`. Largest file: `foliate/src/book-viewer.js` (~47 KB).

### lue — `~/Documents/repos/linux-lit/lue/`

Terminal ebook reader (Python, ~1.5k LOC) with **word-level TTS sync** — the closest in-language analog to linux-lit's audio/text sync workflow. Modular by responsibility, easy to read in one sitting.

- `lue/audio.py` — playback control (mirrors what linux-lit/mpv-linux-lit does)
- `lue/tts_manager.py` — TTS engine integration; reference for sync state machine
- `lue/timing_calculator.py` — **highest-value file**: how to map text positions to audio time and back. Read this when debugging linux-lit's deferred page-turn or stall-on-seek issues.
- `lue/content_parser.py` — EPUB/PDF/DOCX/HTML/RTF/TXT/MD ingestion. Reference if linux-lit ever ingests anything beyond `lit.db`.
- `lue/progress_manager.py` — bookmark/last-position persistence. Compare to linux-lit's `page_history` and bookmark schema.
- `lue/input_handler.py` — keybind dispatch in a TUI. Different from GTK but the dispatch shape is similar.

### bk — `~/Documents/repos/linux-lit/bk/`

Terminal EPUB reader in Rust (~1163 LOC across 3 files). Closest Rust-language analog. Tiny enough to read end-to-end.

- `src/main.rs` (426 lines) — argv handling, key event loop, vim-style keymap dispatch. Compare to `src/input/keymap.rs`.
- `src/view.rs` (444 lines) — viewport/scroll/page state. Compare to `src/input/navigation.rs` and `src/app.rs`'s display logic.
- `src/epub.rs` (~9.8 KB) — EPUB unzip + chapter splitting. Reference if linux-lit ever ingests EPUB.

### openreader — `~/Documents/repos/linux-lit/openreader/`

Next.js/TypeScript web app (~30k LOC) with **whisper.cpp word timestamps** and per-document audio. Most of it is unrelated to linux-lit (auth, S3 uploads, Drizzle ORM), but the audio-sync pieces are the most direct reference for linux-lit's manual-timestamp + sync workflow.

- `src/hooks/audio/` — audio playback hooks, time-update handling, seek behavior. Read when debugging playback sync stalls.
- `src/components/player/` — the read-along UI: word/line highlight driven by audio time. Compare to linux-lit's cursor advancement under MPV sync.
- `src/hooks/epub/` and `src/hooks/html/` — content-to-timestamp mapping, chunked. Useful pattern even though linux-lit's chunks come from `lit.db`, not whisper.
- **Skip:** auth, billing, S3, Drizzle, Tailwind, anything outside `hooks/audio`, `hooks/epub`, `components/player`.

### html5-audio-read-along — `~/Documents/repos/linux-lit/html5-audio-read-along/`

Tiny (~11 KB JS total) read-along demo: word-level highlight synced to `<audio>` with click-to-seek.

- `read-along.js` (8.6 KB) — the entire algorithm: word spans with `data-begin`/`data-end`, audio `timeupdate` → highlight current word, click span → seek audio. Read this when designing click-to-seek or rewriting linux-lit's per-word highlight loop.
- `index.html` — example markup format (XML-ish word spans).

### transcript-tracer-js — `~/Documents/repos/linux-lit/transcript-tracer-js/`

Single-file (`transcript-tracer.js`, 20 KB) library for syncing audio/video with text using **WebVTT timestamps**.

- Reference for: WebVTT parsing as a sync data format (an alternative to linux-lit's per-line SQLite timestamps if linux-lit ever needs to import/export sync data), and the active-cue → highlight loop.
- See `examples/` for usage patterns.

### How to use these references

1. Identify the problem (pagination edge case, sync stall, bookmark schema, etc.).
2. Pick the reference from the bullets at the top of this section.
3. Read the named file end-to-end before grepping — these are small enough.
4. Translate the **algorithm or schema**, never the code. linux-lit is Rust + GTK4 + SQLite + MPV — not JS, not curses, not WebView.
5. If the reference disagrees with linux-lit's current approach, that's a design question — don't silently change linux-lit to match. Surface the tradeoff.

## Memory Bank System

This project uses a structured memory bank system. Always check these context
files before starting work, and keep them updated as the project evolves:

- **CLAUDE-activeContext.md** — current session state, goals, and progress
- **CLAUDE-patterns.md** — established code patterns and conventions
- **CLAUDE-decisions.md** — architecture decisions and rationale
- **CLAUDE-troubleshooting.md** — common issues and proven solutions
- **CLAUDE-config-variables.md** — configuration variables reference

Always read **CLAUDE-activeContext.md** first to maintain session continuity.
When you change core context, update the relevant memory bank file.
