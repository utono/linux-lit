# Transient-toast helper — implementation plan

Audit opportunity #9. Behavior-preserving extraction of the auto-hide toast tail.
See `docs/superpowers/specs/2026-06-22-transient-toast-helper-design.md`.

## Task 1 — add the helper

- New `src/ui/toast.rs` with `pub(crate) fn show_transient(&Label, &str, u64)`.
- Register `pub mod toast;` in `src/ui/mod.rs` (alongside the other `pub mod`).
- `cargo build` — unused-until-adopted is fine.

## Task 2 — delegate the named wrappers

Replace ONLY the 5-line tail in each, keeping message construction:
- `show_no_timestamp_toast` (`keymap.rs`)
- `edge_toast` (`search.rs`) — keeps its `format!`
- `show_no_concordance_toast` (`concordance.rs`)
- `show_no_echo_turns_toast` (`echoes.rs`)
- `show_tts_toast`, `voice_picker_toast` (`gloss.rs`)

## Task 3 — delegate the inline sites

`keymap.rs` (speed/copy/scansion ~1151–2382), `app.rs` (calibration/page-image/
synopsis ~5809–6095). Each becomes one `show_transient(&s.<toast>, <msg>, <secs>)`.
Preserve each site's exact `<secs>` (2 vs 3).

## Guard — do NOT touch (EXCLUDED)

`show_chapter_toast` (navigation.rs, gen-guarded), `show_persistent_tts_toast`/
`hide_tts_toast` (gloss.rs, no auto-hide), the 5s/6s startup/fuzz timers, the
500ms chord-reset. Verify none of these change.

## Verify

`cargo build` + `cargo test --bins`. Confirm grep: every remaining
`toast.set_visible(false)` closure is either the helper or an EXCLUDED site.
