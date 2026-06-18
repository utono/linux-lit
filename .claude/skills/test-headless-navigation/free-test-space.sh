#!/usr/bin/env bash
# free-test-space.sh — reclaim the tmpfs that the headless cage tests fill.
#
# Two distinct causes fill /tmp (commonly a 32G tmpfs), and this script handles
# both:
#
#  (A) The XDG TRASH on the same tmpfs. The user's interactive `rm` is aliased to
#      `trash-put`, which MOVES files to /tmp/.Trash-1000/ instead of unlinking
#      them — and that trash lives on the very tmpfs you're trying to free, so a
#      "delete" reclaims NOTHING. This is the #1 recurring cause: a sweep's worth
#      of 628M lit.db copies and 1.4G target trees pile up in the trash. (Aliases
#      don't apply to non-interactive scripts, but a human running `rm` in a
#      terminal trips this every time.) We empty the trash with a REAL unlink.
#
#  (B) Deleted-but-open files. Each cage run copies the 628M lit.db into a
#      `mktemp -d` and spawns dbus / xdg-desktop-portal / at-spi daemons whose
#      stdio (cage.log) stays open on that dir; unlinking the dir then frees
#      nothing until those FDs close. We kill the FD-holders first.
#
# Order: kill FD-holders → real-unlink leftover test dirs/artifacts → empty the
# tmpfs trash → report before/after. Every removal uses `command rm` (and a
# guard against an `rm`→trash alias) so files are truly unlinked, never trashed
# back onto the same fs.
#
# Safe: it only touches test scratch, the XDG trash, and daemons holding an FD
# into a /tmp test dir — never a live `cargo run` session.
#
# Usage:
#   .claude/skills/test-headless-navigation/free-test-space.sh
#
# Run it when a headless/fuzz run reports only ~2 steps, `df -h /tmp` shows little
# free, the preflight in run-fuzz-all-works.sh refuses to start, or Bash tool
# calls start failing with "the temp filesystem … is full".

set -uo pipefail

# REAL unlink, bypassing any `rm`→trash-put alias/function. `command` skips
# shell functions and aliases; we still call /usr/bin/rm style semantics. Never
# use a bare `rm` in this script — it would trash onto the same tmpfs.
RM() { command rm "$@"; }

free_mb() { df -Pk /tmp | awk 'NR==2{printf "%d", $4/1024}'; }

before=$(free_mb)
echo "[free-space] /tmp free before: ${before}M" >&2

# 1. Kill processes holding an FD into any leftover test temp dir, so the
#    deleted-but-open space (B) can actually be reclaimed. Match by FD target
#    prefix — orphaned dbus/xdg/at-spi daemons carry no identifying cmdline.
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

# 2. Real-unlink leftover test temp dirs + stray artifacts. find/-print0 so an
#    aliased `ls` (eza) can't poison a captured path and a huge glob can't
#    overflow argv.
for root in /tmp /tmp/claude-* "${TMPDIR:-/tmp}"; do
  [ -d "$root" ] || continue
  find "$root" -maxdepth 1 -name 'tmp.*' -type d -print0 2>/dev/null \
    | xargs -0r command rm -rf 2>/dev/null
done
RM -f /tmp/fuzz-*.log /tmp/branch-*.log /tmp/fuzz-nav.log \
      /tmp/fuzz-all-works-summary.txt /tmp/fuzz_pid.txt 2>/dev/null

# 3. Empty the XDG trash that lives ON the tmpfs (cause A). This is what a human
#    `rm` actually filled. Only trash dirs under /tmp (and the runtime dir) — the
#    trash on the real home fs is the user's and is left alone.
for trash in /tmp/.Trash-* "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"/.Trash-*; do
  [ -d "$trash" ] || continue
  echo "[free-space] emptying tmpfs trash: $trash ($(du -sh "$trash" 2>/dev/null | cut -f1))" >&2
  command rm -rf "$trash" 2>/dev/null
done

sync
after=$(free_mb)
echo "[free-space] /tmp free after:  ${after}M  (reclaimed $((after-before))M)" >&2
