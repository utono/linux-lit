# Headless e2e environment (`scripts/e2e-env.sh`)

`scripts/e2e-env.sh` provides a headless Wayland + accessibility environment
and then runs whatever command you pass it. It is the wrapper the headless UI
tests (`tests/line_clipping.rs`, `tests/smoke.rs`) and the AT-SPI assertion
scripts run inside, so they work with no GPU, no monitor, and no login session —
over SSH, in a container, or in CI.

It does **not** start a compositor. Each e2e test spawns its own `cage`
compositor per test (`tests/harness/mod.rs`); this script only supplies the
environment variables and D-Bus buses those tests need.

## What is `cage`?

A Wayland application cannot draw to a screen by itself — it needs a Wayland
**compositor** (the server that owns the display, hands each app a surface, and
composites them). On a normal desktop that role is filled by your window manager
(here, `dwl`). Headless tests have no desktop, so they bring their own throwaway
compositor: **`cage`**.

`cage` is a minimal kiosk compositor: it runs exactly **one** application,
**fullscreen**, with no window chrome, no title bar, and no way to switch away —
when that app exits, `cage` exits. That single-focused-fullscreen behavior is
exactly what the reader needs under test (it assumes a configured, focused,
fullscreen surface), and being minimal it starts in milliseconds and leaves
nothing behind.

Combined with the `WLR_BACKENDS=headless` / `WLR_RENDERER=pixman` environment
this script exports, `cage` runs against an in-memory virtual output with a
software renderer — no monitor and no GPU. The harness then drives the app with
`wtype` (synthetic keystrokes) and captures the virtual output with `grim`
(screenshots), which the pixel assertions inspect. `cage` is invoked as, e.g.,
`cage -- ./target/debug/linux-lit`; everything after `--` is the one app it
hosts.

Cleanup note (from `CLAUDE.md`): kill a stuck test compositor with the scoped
`pkill -f "cage -- ./target/debug/linux-lit"` — never a bare
`pkill -f target/debug/linux-lit`, which would also kill a live dev instance.

## Usage

```bash
./scripts/e2e-env.sh <command> [args...]
```

Everything after the script name is the command it runs. Common invocations:

```bash
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
```

```bash
./scripts/e2e-env.sh cargo test --test smoke -- --ignored --nocapture
```

```bash
./scripts/e2e-env.sh python3 scripts/atspi_assert.py --app litreader ...
```

With no command it prints a usage line and exits 64.

## Anatomy of the canonical command

```bash
cd ~/utono/linux-lit && ./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
```

- `./scripts/e2e-env.sh` — set up the headless env, then exec the rest.
- `cargo test --test line_clipping` — build and run only the
  `tests/line_clipping.rs` integration binary.
- `--` — everything after this goes to the test binary, not to cargo.
- `--ignored` — run the `#[ignore]`-marked e2e tests. They are ignored by
  default so a plain `cargo test` stays green without a compositor; this flag
  opts them in.
- `--nocapture` — let the test's stdout/stderr through live (the clip
  detector's `PASS`/`FAIL` lines) instead of buffering it.

The wrapper takes no flags of its own — it passes its whole argument list
straight through to the command.

## What the wrapper sets up

In order:

- **Headless wlroots + software rendering.** Exports `WLR_BACKENDS=headless`,
  `WLR_RENDERER=pixman`, `WLR_RENDERER_ALLOW_SOFTWARE=1`,
  `WLR_LIBINPUT_NO_DEVICES=1`, `WLR_HEADLESS_OUTPUTS=1`, plus
  `LIBGL_ALWAYS_SOFTWARE=1` / `GALLIUM_DRIVER=llvmpipe` for any client that
  still reaches for GL. No GPU or DRM device is required.
- **Wayland + AT-SPI GTK backends.** `GDK_BACKEND=wayland` (never fall back to
  X11) and `GTK_A11Y=atspi` (force GTK4's AT-SPI accessibility backend) so the
  harness and `check_line_clipping.py` / `atspi_assert.py` can introspect the
  widget tree.
- **A private session bus.** Re-execs itself once under `dbus-run-session` so
  the run gets a fresh `DBUS_SESSION_BUS_ADDRESS` and never touches your real
  session bus. The `E2E_DBUS_READY` guard makes the re-exec happen exactly once.
- **The AT-SPI registry, started manually.** Sets
  `ATSPI_DBUS_IMPLEMENTATION=dbus-daemon` and launches
  `/usr/lib/at-spi-bus-launcher --launch-immediately` itself, then waits for the
  `org.a11y.Bus` name and exports its address as `AT_SPI_BUS`. This is the
  Arch/CachyOS workaround: the default `dbus-broker` routes service activation
  through systemd, which is absent inside the nested `dbus-run-session`, so
  `org.a11y.atspi.Registry` would fail to activate and every pyatspi/dogtail
  assertion would die. Forcing the classic `dbus-daemon` and starting the
  launcher by hand (so it inherits the environment) makes the registry
  activatable headlessly. The launcher is reaped on exit via a trap.

The command runs as a child process (not `exec`) so that exit trap can clean up
the launcher.

## Related

- `tests/harness/mod.rs` — spawns the per-test `cage` compositor and drives it
  with `wtype`/`grim`.
- `scripts/check_line_clipping.py` — the pixel-level top/bottom clip detector
  the `line_clipping` test asserts with.
- `docs/troubleshooting/clip-prevention.md` — how the reader prevents clipped
  partial rows, and the failure checklist for clip bugs.
- Project `CLAUDE.md` (Headless Verification / Automated UI tests) — the ad-hoc
  `cage` launch recipe and the `LIT_DEV=1` / `LIT_NO_MPV=1` gotchas.
