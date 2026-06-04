#!/usr/bin/env bash
# e2e-env.sh — provide a headless Wayland + AT-SPI environment, then run $@.
#
# Sets every WLR/GTK env the harness relies on, forces software rendering so
# it works with no GPU (container/CI/SSH), and wraps the command in a private
# D-Bus session bus. at-spi2-core auto-activates the a11y bus (org.a11y.Bus)
# on that session bus, which is what lets GTK4 expose its widget tree and lets
# scripts/atspi_assert.py introspect it.
#
# Usage:
#   ./scripts/e2e-env.sh cargo test --test smoke -- --ignored --nocapture
#   ./scripts/e2e-env.sh python3 scripts/atspi_assert.py --app litreader ...
#
# The harness spawns its own compositor per test, so this script intentionally
# does NOT start one — it only supplies the environment + buses.

set -euo pipefail

# wlroots headless backend + software renderer (no GPU/DRM needed).
export WLR_BACKENDS=headless
export WLR_LIBINPUT_NO_DEVICES=1
export WLR_RENDERER=pixman
export WLR_RENDERER_ALLOW_SOFTWARE=1
export WLR_HEADLESS_OUTPUTS=1

# Make every GTK/GL client behave headlessly and accessibly.
export GDK_BACKEND=wayland       # never fall back to X11
export GTK_A11Y=atspi            # GTK4: force the AT-SPI accessibility backend
export LIBGL_ALWAYS_SOFTWARE=1   # llvmpipe for any client that still wants GL
export GALLIUM_DRIVER=llvmpipe

# Re-exec ourselves under a fresh session bus exactly once. dbus-run-session
# exports DBUS_SESSION_BUS_ADDRESS for everything below it; at-spi2-core
# registers org.a11y.Bus on demand against it.
if [[ "${E2E_DBUS_READY:-}" != 1 ]]; then
  export E2E_DBUS_READY=1
  exec dbus-run-session -- "$0" "$@"
fi

if [[ $# -eq 0 ]]; then
  echo "usage: $0 <command> [args...]" >&2
  exit 64
fi

# Bring up the AT-SPI registry on the a11y bus ourselves. at-spi2-core would
# normally autostart it, but on Arch the bus launcher embeds dbus-broker, which
# routes service activation through systemd — absent in a nested
# dbus-run-session — so org.a11y.atspi.Registry fails with "unit failed" and
# every dogtail/pyatspi assertion dies. Forcing the classic dbus-daemon
# implementation and starting the launcher MANUALLY (so it inherits the var;
# autostart strips the environment) makes the registry activatable headlessly.
export ATSPI_DBUS_IMPLEMENTATION=dbus-daemon
/usr/lib/at-spi-bus-launcher --launch-immediately &
_atspi_launcher_pid=$!
cleanup() { kill "$_atspi_launcher_pid" 2>/dev/null || true; }
trap cleanup EXIT INT TERM

# Wait for the a11y bus name, then export its address for dogtail/pyatspi.
for _ in $(seq 1 20); do
  _atspi_raw=$(gdbus call --session --dest org.a11y.Bus \
    --object-path /org/a11y/bus \
    --method org.a11y.Bus.GetAddress 2>/dev/null) && [[ -n "$_atspi_raw" ]] && break
  sleep 0.2
done
if [[ -n "${_atspi_raw:-}" ]]; then
  export AT_SPI_BUS=$(python3 -c \
    "import sys,re; m=re.search(r\"unix:[^')]*\", sys.argv[1]); print(m.group(0) if m else '')" \
    "$_atspi_raw")
fi

# Run the command as a child (not exec) so the trap above can reap the launcher.
"$@"
