# Sync-suppress-window constant — implementation plan

Audit opportunity #12. See
`docs/superpowers/specs/2026-06-22-sync-suppress-window-const-design.md`.

## Task 1 — add the consts

In `src/input/navigation.rs`, beside `SEEK_PREROLL`/`SYNC_*`:
- `pub const SYNC_SUPPRESS_SEEK: Duration = from_millis(500);`
- `pub const SYNC_SUPPRESS_INDEFINITE: Duration = from_secs(86400);`

## Task 2 — repoint the 8 500ms sites

search.rs:104, search.rs:247, timestamps.rs:210, keymap.rs:2343, gamepad.rs:181,
echoes.rs:896, echoes.rs:1364, concordance.rs:543 →
`Some(Instant::now() + crate::input::navigation::SYNC_SUPPRESS_SEEK)`.
Also navigation.rs:1725 `new_until` (same module → `SYNC_SUPPRESS_SEEK`).

## Task 3 — name the 86400 sentinel

navigation.rs:1736 → `Some(Instant::now() + SYNC_SUPPRESS_INDEFINITE)`.

## Guard — do NOT touch (EXCLUDED)

app.rs:1972 and keymap.rs:28 `timeout_add_local_once(from_millis(500))` (UI
timers). The navigation.rs:1725 max-guard control flow stays inline.

## Verify

`cargo build` + `cargo test --bins`. Grep: no remaining
`suppress_sync_until = Some(... from_millis(500))` or `... from_secs(86400)`
literal.
