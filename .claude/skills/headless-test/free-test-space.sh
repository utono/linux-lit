#!/usr/bin/env bash
# free-test-space.sh — reclaim the tmpfs that the headless cage tests fill.
#
# Each fuzz/headless run copies the 628M lit.db into a `mktemp -d` under /tmp and
# spawns dbus / xdg-desktop-portal / at-spi daemons whose stdio (cage.log) stays
# open on that dir. A plain `rm -rf` then unlinks the dir but the kernel keeps the
# space allocated until those FDs close — so /tmp (commonly a 32G tmpfs) creeps to
# 0% free and every later run dies with ENOSPC after ~2 NAV_TEST steps.
#
# This script: (1) kills any process holding an open FD into a leftover test temp
# dir, (2) deletes the leftover `tmp.*` dirs and stray test artifacts, (3) reports
# before/after free space. Safe: it only touches test scratch and the daemons that
# the tests spawned (matched by an open FD into a /tmp test dir) — never a live
# `cargo run` session.
#
# Usage:
#   .claude/skills/headless-test/free-test-space.sh
#
# Run it when a headless/fuzz run reports only ~2 steps, or `df -h /tmp` shows
# little free, or the preflight in run-fuzz-all-works.sh refuses to start.

set -uo pipefail

# Glob roots where mktemp -d lands (TMPDIR may point under the Claude harness dir).
TMP_GLOBS=(
  "${TMPDIR:-/tmp}"/tmp.*
  /tmp/tmp.*
  /tmp/claude-*/tmp.*
)

free_mb() { df -Pk /tmp | awk 'NR==2{printf "%d", $4/1024}'; }

echo "[free-space] /tmp free before: $(free_mb)M" >&2

# 1. Kill processes holding an FD into any leftover test temp dir. We match by the
#    FD target starting with a known temp-dir prefix, so orphaned dbus/xdg/at-spi
#    daemons (which don't carry an identifying cmdline) are caught.
killed=0
for fd in /proc/[0-9]*/fd; do
  tgt=$(readlink "$fd"/* 2>/dev/null) || continue
  case "$tgt" in
    */tmp.*/* | */claude-*/tmp.*/* )
      pid=$(basename "$(dirname "$fd")")
      kill -9 "$pid" 2>/dev/null && killed=$((killed+1))
      ;;
  esac
done
[ "$killed" -gt 0 ] && { echo "[free-space] killed $killed FD-holding process(es); waiting for FDs to close…" >&2; sleep 1; }

# 2. Delete leftover test temp dirs + stray artifacts. Use find/-print0 so an
#    aliased `ls` (eza) can't poison a captured path, and so a huge glob can't
#    overflow argv.
for root in /tmp /tmp/claude-* "${TMPDIR:-/tmp}"; do
  [ -d "$root" ] || continue
  find "$root" -maxdepth 1 -name 'tmp.*' -type d -print0 2>/dev/null \
    | xargs -0r rm -rf 2>/dev/null
done
rm -f /tmp/fuzz-*.log /tmp/branch-*.log /tmp/fuzz-nav.log \
      /tmp/fuzz-all-works-summary.txt /tmp/fuzz_pid.txt 2>/dev/null

sync
echo "[free-space] /tmp free after:  $(free_mb)M" >&2
