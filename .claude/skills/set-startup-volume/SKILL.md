---
name: set-startup-volume
description: Use when changing linux-lit's startup audio volume — the system output sink percent applied via pactl on launch, or MPV's playback launch volume
argument-hint: system <pct> | mpv <pct> | both <sys> <mpv>
---

# Set Startup Volume

linux-lit applies two volumes on launch, both read from config (no hardcoded
literals, no rebuild needed to change them):

- **system** — `config.system_volume` (percent). Applied once at startup via
  `pactl set-sink-volume @DEFAULT_SINK@ <pct>%` (`src/main.rs`). This is the OS
  output sink. Historical default: **70**.
- **mpv** — `config.mpv_volume` (percent). Passed as `--volume=<pct>` when
  launching MPV (`src/mpv/discovery.rs`). Relative to the system sink, so the
  effective level is `system% × mpv%`. Historical default: **100**.

This skill edits the config JSON; it does NOT touch source or rebuild.

## Two config files, both written by default

linux-lit reads a DIFFERENT config per build mode (`src/config.rs config_path`):

- `~/.config/linux-lit/config-dev.json` — used by `cargo run` (dev)
- `~/.config/linux-lit/config.json` — used by the release build

Write **both** by default so the value takes effect either way.

**A running instance clobbers its config on exit.** Before editing, check for a
running app and warn the user if one is up — its exit will overwrite your change:

```bash
pgrep -af 'linux-lit' | rg -v 'cargo build|rg ' || echo "no instance running"
```

If an instance is running, tell the user to quit it first, then re-run the skill.

## Arguments

- `system <pct>` — set only `system_volume`
- `mpv <pct>` — set only `mpv_volume`
- `both <sys> <mpv>` — set both
- bare `<pct>` (single number) — ambiguous; ask whether it's system or mpv

Validate each percent is an integer in `0..=150` (pactl and mpv both accept
>100; reject negatives and non-integers). Report old → new for each value
changed.

## Steps

1. Parse the argument(s) into `system_volume` and/or `mpv_volume` integers;
   validate range `0..=150`.
2. Warn if a linux-lit instance is running (see check above); if so, stop and
   ask the user to quit it first.
3. For each of `config.json` and `config-dev.json`, set the chosen key(s) with
   `jq` (creating the key if absent). Write atomically via a temp file and
   `\mv -f` (the user's `mv` is aliased; the backslash bypasses it):

   ```bash
   for f in config.json config-dev.json; do
     p="$HOME/.config/linux-lit/$f"
     tmp="$(mktemp)"
     jq --argjson sv "$SYS" --argjson mv "$MPV" \
        '.system_volume = $sv | .mpv_volume = $mv' "$p" > "$tmp" \
       && \mv -f "$tmp" "$p"
   done
   ```

   (Only include the `.system_volume`/`.mpv_volume` assignments for the keys
   actually being changed.)
4. Read both files back and report the new `system_volume` / `mpv_volume`, plus
   the prior values, and note the change takes effect on the next launch.

## Notes

- Defaults live in `src/config.rs` (`default_system_volume` = 70,
  `default_mpv_volume` = 100). A stored config value overrides the compiled
  default, so editing the JSON is the right layer (see the dev/release config
  gotcha in the project CLAUDE.md).
- Runtime nudging (`Ctrl+Up`/`Ctrl+Down` → `VolumeAdjust(±5)`) is a separate,
  unsaved relative `add volume` IPC to MPV — it does not change these startup
  values and is not persisted.
- The system-volume `pactl` call is skipped under `LIT_HEADLESS_TEST` so UI
  test runs never touch the live session's sink.
