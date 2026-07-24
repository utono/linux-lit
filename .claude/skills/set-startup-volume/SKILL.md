---
name: set-startup-volume
description: Use when changing linux-lit's startup audio volume — the system output sink percent applied via pactl on launch, MPV's playback launch volume, or the rodio/TTS clip-player level
argument-hint: system <pct> | mpv <pct> | rodio <pct> | both <sys> <mpv>
---

# Set Startup Volume

linux-lit applies THREE volumes on launch, all read from config (no hardcoded
literals, no rebuild needed to change them):

- **system** — `config.system_volume` (percent). Applied once at startup via
  `pactl set-sink-volume @DEFAULT_SINK@ <pct>%` (`src/main.rs`). This is the OS
  output sink. Historical default: **70**.
- **mpv** — `config.mpv_volume` (percent). Passed as `--volume=<pct>` when
  launching MPV (`src/mpv/discovery.rs`). Relative to the system sink, so the
  effective level is `system% × mpv%`. Historical default: **100**.
- **rodio / TTS** — the in-process gloss/synopsis TTS clip player. There is NO
  standalone `rodio_volume` key; its startup level is DERIVED as
  `system_volume − tts_volume_offset` (`src/app/mod.rs` ~2109,
  `tts.set_volume_percent`). So "rodio" is expressed as an OFFSET below the
  system volume, not an absolute percent. Historical offset default: **50**
  (ElevenLabs clips run hotter than audiobook masters). A `rodio_volume` key
  may exist in the JSON but is unused/null — ignore it; the offset governs.

This skill edits the config JSON; it does NOT touch source or rebuild.

## rodio is coupled to system_volume (the trap)

Because rodio's startup level is `system_volume − tts_volume_offset`, **any
change to `system_volume` also moves rodio/TTS** by the same amount unless you
adjust the offset to compensate. When the user changes `system_volume`, ALWAYS
tell them the resulting TTS effective level and ASK whether to keep TTS at its
prior effective % (recompute `tts_volume_offset = new_system − desired_tts`) or
let it ride down with the sink. Never change `system_volume` silently.

Likewise, "lower rodio" / "give rodio N offset" means edit `tts_volume_offset`,
not any absolute key. Larger offset = quieter TTS. Report the effective
`system_volume − tts_volume_offset` back, not just the offset.

## Two config files, both written by default

linux-lit reads a DIFFERENT config per build mode (`src/config.rs config_path`):

- `~/.config/linux-lit/config-dev.json` — used by `cargo run` (dev)
- `~/.config/linux-lit/config.json` — used by the release build

Write **both** by default so the value takes effect either way.

**A running instance clobbers its config on exit.** Before editing, check for a
running app and warn the user if one is up — its exit will overwrite your change:

```bash
pgrep -af 'target/debug/linux-lit|target/release/linux-lit' | rg -v 'rg |claude' || echo "no instance running"
```

(Match the built binary paths, not a bare `linux-lit` — a bare pattern also
catches `cargo build`/`rg`/this agent's own shell and gives false positives.)

If an instance is running, ask the user to quit it and pick "Quit it, then
edit"; do NOT tell them to re-run the skill from scratch. Then **re-run the
`pgrep` check and confirm "no instance running" before writing** — verify the
quit actually happened, don't assume. The instance may reappear between
successive edits in one session, so re-check on EVERY write, not just the first.

## Arguments

- `system <pct>` — set only `system_volume` (see the rodio-coupling trap above)
- `mpv <pct>` — set only `mpv_volume`
- `rodio <pct>` — set the TTS EFFECTIVE percent; compute
  `tts_volume_offset = system_volume − <pct>` (clamp offset to `0..=150`)
- `both <sys> <mpv>` — set both system and mpv
- `"lower both"` / `"further lower"` — no explicit numbers: lower `mpv_volume`
  AND rodio a sensible step (mpv ~10–15 pts; rodio via +8–10 offset). This is a
  recurring interactive request — take the step, report the new effective
  levels, and offer to go lower again rather than asking for exact numbers each
  time. Once TTS effective drops near ~7% it's almost inaudible; at that point
  say so and suggest lowering `system_volume` instead.
- `"reset both to same"` — mpv is a % of the sink while rodio is an offset, so
  "same" is ambiguous. Default to equal EFFECTIVE %: keep `mpv_volume` and set
  `tts_volume_offset = system_volume − mpv_volume`. Confirm the target % if
  unclear.
- bare `<pct>` (single number) — ambiguous; ask whether it's system, mpv, or
  rodio.

Validate each percent is an integer in `0..=150` (pactl and mpv both accept
>100; reject negatives and non-integers). Report old → new for each value
changed.

## Steps

1. Parse the argument(s) into `system_volume`, `mpv_volume`, and/or
   `tts_volume_offset` integers; validate range `0..=150`. For a `rodio <pct>`
   or "keep TTS at N%" request, derive `tts_volume_offset = system_volume −
   <pct>`. If the request changes `system_volume`, resolve the rodio-coupling
   question first (keep effective TTS vs let it drop — see the trap section).
2. Check whether a linux-lit instance is running (see check above); if so, ask
   the user to quit it, then RE-RUN the check and confirm "no instance running"
   before writing. Re-check on every write in a multi-edit session.
3. For each of `config.json` and `config-dev.json`, set the chosen key(s) with
   `jq` (creating the key if absent). Write atomically via a temp file and
   `\mv -f` (the user's `mv` is aliased; the backslash bypasses it):

   ```bash
   for f in config.json config-dev.json; do
     p="$HOME/.config/linux-lit/$f"
     tmp="$(mktemp)"
     jq --argjson sv "$SYS" --argjson mv "$MPV" --argjson off "$OFF" \
        '.system_volume = $sv | .mpv_volume = $mv | .tts_volume_offset = $off' "$p" > "$tmp" \
       && \mv -f "$tmp" "$p"
   done
   ```

   (Only include the assignments for the keys actually being changed.)
4. Read both files back and report the new `system_volume` / `mpv_volume` /
   `tts_volume_offset`, plus the prior values. ALWAYS report the rodio/TTS
   EFFECTIVE level (`system_volume − tts_volume_offset`), not just the raw
   offset — the offset alone is meaningless to the user. Note the change takes
   effect on the next launch. Handy readback with the effective level computed:

   ```bash
   for f in config.json config-dev.json; do
     echo "=== $f ==="
     jq '{system_volume, mpv_volume, tts_volume_offset, tts_effective: (.system_volume - .tts_volume_offset)}' "$HOME/.config/linux-lit/$f"
   done
   ```

## Notes

- Defaults live in `src/config.rs` (`default_system_volume` = 70,
  `default_mpv_volume` = 100, `default_tts_volume_offset` = 50). A stored config
  value overrides the compiled default, so editing the JSON is the right layer
  (see the dev/release config gotcha in the project CLAUDE.md).
- **MPV** runtime nudging (`Ctrl+Up`/`Ctrl+Down` → `VolumeAdjust(±5)`) is a
  separate, unsaved relative `add volume` IPC to MPV — it does not change these
  startup values and is not persisted.
- **TTS/rodio** runtime nudging (`Ctrl+Alt+Up`/`Ctrl+Alt+Down`) DOES persist:
  it rewrites `tts_volume_offset` below `system_volume` on the fly
  (`src/input/keymap.rs` ~4730), so the next launch seeds at the tuned level.
  This is the one runtime nudge that touches a startup config value.
- The system-volume `pactl` call is skipped under `LIT_HEADLESS_TEST` so UI
  test runs never touch the live session's sink.
