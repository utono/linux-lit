#!/usr/bin/env python3
"""Assert that an app exposes an expected widget over AT-SPI.

Compositor-independent: it queries the accessibility bus, not pixels, so it
answers "did the right view load / is this control present" rather than "are
the pixels right". GTK4 ships the AT-SPI backend built in (no GTK_MODULES /
atk-bridge needed like GTK3) — it just needs an a11y bus, which
scripts/e2e-env.sh provides.

Requires python-dogtail (which pulls in pyatspi). Exit 0 if found, 1 if not.

Example:
    python3 scripts/atspi_assert.py --app litreader --name Library --role frame
    python3 scripts/atspi_assert.py --app litreader --role "text"  # any text widget
"""

import argparse
import sys
import time

try:
    from dogtail.tree import root, SearchError
    from dogtail import config
except ImportError:
    sys.stderr.write(
        "python-dogtail not installed (provides dogtail + pyatspi)\n"
    )
    sys.exit(2)

# Keep dogtail quiet and snappy; we do our own outer retry loop.
config.config.logDebugToFile = False
config.config.searchCutoffCount = 3
config.config.searchBackoffDuration = 0.5


def find_app(app_name, timeout):
    """Wait for the app to register on the a11y bus."""
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            return root.application(app_name)
        except SearchError as e:
            last = e
            time.sleep(0.25)
    raise SystemExit(
        f"app '{app_name}' not found on the a11y bus within {timeout}s "
        f"(check it registered its name; last: {last})"
    )


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--app", required=True,
                   help="accessible app name (g_set_prgname / GApplication id tail)")
    p.add_argument("--name", default=None, help="widget accessible name to require")
    p.add_argument("--role", default=None,
                   help="widget role name, e.g. frame, push button, text")
    p.add_argument("--timeout", type=float, default=10.0)
    args = p.parse_args()

    app = find_app(args.app, args.timeout)

    # If only --app was given, presence on the bus is the assertion.
    if args.name is None and args.role is None:
        print(f"OK: '{args.app}' is present on the a11y bus")
        return

    try:
        widget = app.child(name=args.name, roleName=args.role)
    except SearchError:
        sys.stderr.write(
            f"FAIL: no widget name={args.name!r} role={args.role!r} under '{args.app}'\n"
        )
        # Dump the tree to aid debugging.
        try:
            app.dump()
        except Exception:
            pass
        sys.exit(1)

    print(f"OK: found {widget} under '{args.app}'")


if __name__ == "__main__":
    main()
