---
name: check-stderr-log
description: Use when investigating GTK/GLib runtime warnings or crashes in linux-lit — reads linux-lit-dev-stderr.log (the tee'd stderr capture), separates GLib/GTK criticals from cargo build noise, and proposes debugging steps
---

# Check stderr Log

linux-lit's own debug log (`linux-lit-dev.log`) only holds the app's `log_fmt!()`
lines. **GLib/GTK diagnostics** — `GLib-GObject-CRITICAL`, `Gtk-WARNING`,
`g_object_unref` assertions, GTK abort backtraces — go to the process **stderr**,
not that file. Capture them with:

```bash
cargo run 2>&1 | tee ~/utono/linux-lit/linux-lit-dev-stderr.log
```

This skill reads that stderr capture and proposes what to fix.

## Log File

```bash
cat ~/utono/linux-lit/linux-lit-dev-stderr.log
```

Not cleared on launch (unlike the app's own log) — it accumulates across runs
unless `tee` is used without `-a`. Old runs may be present; anchor on the last
`Running \`target/debug/linux-lit\`` line to find the current session.

## Two kinds of lines — separate them FIRST

The file interleaves two unrelated streams. Classify before proposing anything:

1. **`cargo`/`rustc` build output** — `warning: ... is never used`, `-->`, `|`,
   `^^^^`, `generated N warnings`. This is **compile-time dead-code noise**, NOT
   a runtime problem. Do not propose "fixes" for these unless the user asked to
   clean up warnings. They are the bulk of the file by line count and are the
   least interesting.
2. **GLib/GTK runtime diagnostics** — the `(linux-lit:PID): ...` and
   `(process:PID): ...` lines. **These are the signal.** They are emitted while
   the app runs and at teardown.

Extract just the runtime lines:

```bash
rg '\((linux-lit|process):[0-9]+\):' ~/utono/linux-lit/linux-lit-dev-stderr.log
```

Collapse to unique shapes (strip PID + wall-clock) to see distinct problems and
their counts:

```bash
sed -E 's/[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]+//; s/:[0-9]+\)/:PID)/' \
  ~/utono/linux-lit/linux-lit-dev-stderr.log \
  | rg '\((linux-lit|process):PID\):' | sort | uniq -c | sort -rn
```

## Known-benign lines (report, don't chase)

- **`Gtk-WARNING ... Unknown key gtk-button-images` / `gtk-menu-images` /
  `gtk-modules`** — stale keys in `~/.config/gtk-4.0/settings.ini`, unrelated to
  linux-lit code. Mention once; do not touch app source for these.

## Actionable runtime diagnostics

- **`GLib-GObject-CRITICAL ... g_object_unref: assertion 'G_IS_OBJECT (object)'
  failed`** — a GObject is being unref'd after it was already finalized (or a
  NULL is passed to unref). Note **when** it fires (its wall-clock vs. the
  `Running` line and the app's own log): startup vs. teardown points at
  different owners.
  - Group by count. In practice these arrive in a startup pair and a
    teardown triple — a repeated count usually means one widget/object dropped
    on a path that already dropped it.
  - Hunt for the owner: explicit `unref`/`g_object_unref` are rare in this Rust
    codebase (gtk-rs refcounts via `Drop`), so the cause is usually a widget
    **removed from a parent AND separately held+dropped**, or an `add_overlay`
    panel torn down twice. Cross-reference the timing with overlay/picker
    teardown and `clear_display`.
  - This is a *diagnostic*, not a crash — the app keeps running. Fix only if the
    user wants it silenced or it precedes a real teardown crash.
- **`Gtk-CRITICAL` / `Gtk-ERROR` / `assertion failed` with a `SIGTRAP`/backtrace**
  — a real fault. Read the frames; map the top GTK call to the linux-lit call
  site that triggered it (usually a buffer-iter or widget op after teardown).

## Correlate with the app's own log

The stderr criticals carry a wall-clock timestamp; `linux-lit-dev.log` lines
carry `[NNNms]` relative to launch. To place a critical in the app's timeline,
find the launch wall-clock (file mtime / the terminal), subtract, and read the
`linux-lit-dev.log` lines around that offset for what the app was doing
(`clear_display`, overlay close, work switch).

```bash
cat ~/utono/linux-lit/linux-lit-dev.log
```

## Debugging workflow

1. `cat` the stderr log; anchor on the last `Running \`target/debug/linux-lit\``.
2. Split the two streams (build warnings vs. runtime `(linux-lit:PID)` lines).
   Set the build warnings aside unless asked to clean them.
3. Collapse runtime lines to unique shapes + counts.
4. Drop known-benign (`settings.ini`) lines.
5. For each remaining critical: note firing time (startup / teardown / mid-run),
   correlate with `linux-lit-dev.log`, and name the suspected owner + call site.
6. **Propose** the fix and its verification (usually a headless run —
   `./scripts/e2e-env.sh ...` or the cage launch from the project CLAUDE.md — so
   the same critical can be re-observed on stderr). Do not edit source unless the
   user asks; this skill's job is to *check and propose*.

## Getting a fresh capture

If the log is stale or empty, ask the user to run (they launch the app, per the
project's no-`cargo run` rule):

```bash
cargo run 2>&1 | tee ~/utono/linux-lit/linux-lit-dev-stderr.log
```

Then reproduce the issue and quit, so teardown-time criticals are captured too.
