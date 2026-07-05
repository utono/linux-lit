# Launching linux-lit — Dev vs Release Mode

## Goal

Run the app during development without corrupting your real reading state
(config + position) and while producing the log file the debug workflow expects.

## The one switch that matters: `LIT_DEV`

Whether the app runs in "dev" or "release" mode is decided at runtime by a single
environment variable, **not** by the Cargo build profile:

```rust
// src/mode.rs
pub fn is_dev_mode() -> bool {
    std::env::var("LIT_DEV").is_ok()
}
```

Consequence: a **debug** binary (`./target/debug/linux-lit`) launched **without**
`LIT_DEV` set runs in *release* mode — it reads/writes your production config and
logs to the release log. The build being unoptimized does not make it "dev".

## What `LIT_DEV` gates

`is_dev_mode()` is checked in exactly three places, and each one isolates dev runs
from real reading:

- **App-id** (`src/main.rs`) — `com.utono.linux-lit.dev` vs `com.utono.linux-lit`.
  GTK/libadwaita treats the app-id as the single-instance identity, so a dev
  instance and a release instance are *different apps* to the desktop and will not
  collide or be treated as the same window.
- **Config file** (`src/config.rs`) — `~/.config/linux-lit/config-dev.json` vs
  `~/.config/linux-lit/config.json`. Dev experiments (last work, positions, font
  size, per-work flags) stay out of the real config. This matters because the app
  **rewrites its config on exit** — see
  `../../CLAUDE.md` (Dev vs release config) and the memory note
  "Config clobbered on exit".
- **Log file** (`src/main.rs`) — `linux-lit-dev.log` vs `linux-lit-release.log`.
  (`LIT_LOG_PATH` overrides this entirely when set — used by the headless test
  harness so it never clobbers a live session's dev log.)

## Recommended (and current default): run in dev mode

Set `LIT_DEV=1` so dev runs are fully isolated. This is how the app is launched
during development — the `crll` alias sets it:

```bash
LIT_DEV=1 ./target/debug/linux-lit
```

This is the live `crll` alias (in the `shell-config` stow package at
`~/utono/shell-config/.config/shell/alias-mlj`, symlinked to
`~/.config/shell/`). It builds, runs in dev mode, and captures stderr — GLib/GTK
criticals go to stderr, not the app log (see the `check-stderr-log` skill):

```bash
alias crll='cd ~/utono/linux-lit && cargo build && LIT_DEV=1 ./target/debug/linux-lit 2>&1 | tee ~/utono/linux-lit/linux-lit-dev-stderr.log'
```

After editing the alias, reload it in an open shell (new shells pick it up
automatically):

```bash
source ~/.config/shell/alias-mlj
```

### Pros

- **Isolated config.** Dev churn writes `config-dev.json`; your real reading
  config (`config.json`) is never touched or clobbered on exit.
- **Isolated log.** Live log is `linux-lit-dev.log` — the file the project docs
  and debug skills expect to read first.
- **Separate app identity.** The `.dev` app-id means a dev instance won't collide
  with a release copy you run for actual reading.
- **Matches the docs.** `CLAUDE.md` documents "Dev build → `linux-lit-dev.log`";
  that only holds when `LIT_DEV` is set.

### Cons / caveats

- **First launch may open a different work.** Dev mode reads `config-dev.json`,
  which can hold a different `last_work`/position than the `config.json` you were
  using. One-time reconciliation, not a fault.
- **Still an unoptimized debug binary** — fine for development, slower than a
  release build. `LIT_DEV` does not change build optimization.
- **Set the config value while no dev instance runs.** A running instance
  re-clobbers its config on exit, so edit `config-dev.json` only when the dev app
  is closed.

## Not recommended for development: release mode

Running the binary with `LIT_DEV` **unset** (the default):

```bash
./target/debug/linux-lit          # runs in RELEASE mode despite the debug build
```

### Pros

- Exercises the exact config/log/app-id a real user's release build uses — useful
  for a final eyeball on production behavior.

### Cons

- **Development churn lands in your real `config.json`** and is clobbered on exit,
  so your reading position/settings drift with every dev run.
- **Logs to `linux-lit-release.log`**, so `linux-lit-dev.log` goes stale and any
  "read the dev log" step inspects the wrong (old) file. This is a recurring
  source of debugging-against-the-wrong-log confusion.
- **Shares the release app-id**, so a dev run and a real reading run can be
  treated as the same single-instance app.

## Two invocation forms: `cargo run` vs `build && run`

Independent of `LIT_DEV`, there are two shapes the launch command takes. Both
build the same debug binary; they differ in how compile and launch relate and in
what happens to output.

### `cargo run 2>&1 | tee …-dev-stderr.log`

- `cargo run` **compiles and launches in one step**, using the *current
  directory* — with no `cd` you must already be in `~/utono/linux-lit` or it
  builds the wrong crate.
- `2>&1 | tee …` merges stderr into stdout, echoes it to the terminal, **and
  saves it** to `linux-lit-dev-stderr.log`. This is the only way the GTK/GLib
  runtime diagnostics (`Gtk-WARNING`, `GLib-GObject-CRITICAL`, abort backtraces)
  get captured — they go to **stderr**, not the app log — and it is the file the
  `check-stderr-log` skill reads.
- The app runs **under a pipe**, so the shell's `$?` is `tee`'s exit status, not
  the app's, and on a hard crash the last unflushed stderr line can be lost.

### `cd … && cargo build && ./target/debug/linux-lit`

- `cd` guarantees the correct crate regardless of where you started.
- `cargo build && …` **separates compile from launch**. The `&&` means the app
  only starts *if the build succeeded* — a compile error stops with the error on
  screen and never runs a stale binary.
- With no redirection, stderr/stdout go to the terminal and are **not saved** —
  nothing to grep afterward.

### Which to use

- Want **GTK/GLib stderr saved for debugging** (or to feed `check-stderr-log`) →
  the `tee` form.
- Want a **foolproof dev launch** where a broken build can't run a stale binary,
  and where "still compiling" is visibly distinct from "actually running" → the
  `build && run` form. (This distinction matters: a long `cargo run` compile
  sitting on the dead-code-warnings screen looks like a hung app — it is still
  building; the window appears only after `Finished` and the link complete.)

The live `crll` alias combines both — `cd` + gated `cargo build` + dev-mode run
under `tee` — which is why it is the recommended default (see above).

## Best practice, in one line

**Develop with `LIT_DEV=1` set** (isolated config + dev log + `.dev` app-id);
reserve unset/release mode for a deliberate final check of production behavior.

## Gotcha: which log is actually live?

Because the log path follows `LIT_DEV`, not the build, always confirm which log
the running instance writes before trusting a tail:

```bash
# newest linux-lit log wins
fd -I -e log 'linux-lit' ~/utono/linux-lit -x stat -c '%y  %n' {} | sort
# when did the running instance start?
ps -o pid,lstart,args -p "$(pgrep -f target/debug/linux-lit | head -1)"
```

If `linux-lit-dev.log` is stale but the app is clearly running, it is almost
certainly running in release mode and writing `linux-lit-release.log`. See the
memory notes "Stale instance, stale log" and "crll launch alias".
