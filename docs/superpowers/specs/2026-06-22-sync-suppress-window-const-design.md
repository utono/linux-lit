# Sync-suppress-window constant — design

## Goal

Replace the repeated `Duration::from_millis(500)` sync-suppression window literal
(8 sites) with one named constant, and name the distinct 24h "suppress
indefinitely" sentinel too — with **zero behavior change**. Audit opportunity #12,
the #8-style "name the sentinel" pattern.

## The duplication

`state.suppress_sync_until = Some(Instant::now() + Duration::from_millis(500))`
appears byte-identical at 8 sites:

- `search.rs:104`, `search.rs:247`
- `timestamps.rs:210`
- `keymap.rs:2343`
- `gamepad.rs:181`
- `actions/echoes.rs:896`, `actions/echoes.rs:1364`
- `actions/concordance.rs:543`

All set the same "brief suppression while MPV processes a seek" window. The 500ms
value is currently an unexplained magic literal at each site.

A 9th site, `navigation.rs:1736`, uses `from_secs(86400)` — a *different*
sentinel meaning "suppress indefinitely until the next real sync event". It must
NOT be merged with the 500ms window; it gets its own name.

## Component

Two consts beside the existing sync constants in `src/input/navigation.rs`
(which already holds `SEEK_PREROLL`, `SYNC_PREROLL`, `SYNC_GAP_*`, consumed
cross-module as `crate::input::navigation::NAME`):

```rust
/// Brief window during which playback-sync is suppressed while MPV processes a
/// seek, so the highlight doesn't fight the in-flight seek. Used at every
/// manual-seek site (search, timestamp set, gamepad, echo/concordance jumps).
pub const SYNC_SUPPRESS_SEEK: std::time::Duration = std::time::Duration::from_millis(500);

/// "Suppress sync indefinitely" sentinel (24h) — set when there is no active
/// timestamp to sync against, cleared by the next real sync event.
pub const SYNC_SUPPRESS_INDEFINITE: std::time::Duration = std::time::Duration::from_secs(86400);
```

## Call-site changes

Each 500ms site:
```rust
state.suppress_sync_until = Some(std::time::Instant::now() + crate::input::navigation::SYNC_SUPPRESS_SEEK);
```
`navigation.rs:1736` (same module, no path prefix needed):
```rust
state.suppress_sync_until = Some(std::time::Instant::now() + SYNC_SUPPRESS_INDEFINITE);
```
`navigation.rs:1725`'s max-guard (`new_until`) likewise uses `SYNC_SUPPRESS_SEEK`
for its `from_millis(500)` — the guard logic itself stays inline.

## Explicitly EXCLUDED (leave as raw literals — different meaning)

- `app.rs:1972` and `keymap.rs:28` — `timeout_add_local_once(from_millis(500))`:
  a GTK UI timer (startup re-tick / chord reset), not a sync window. Same number,
  unrelated concept; naming them `SYNC_*` would be wrong.
- The max-guard control flow at `navigation.rs:1725` — stays inline. Folding all
  sites into a `suppress_sync_for(state, dur)` helper that applies the guard
  would ADD a guard at the 8 plain sites (which currently overwrite
  unconditionally) — a behavior change. Out of scope.

## Verification

Pure literal → const; `cargo build` + `cargo test --bins`. Grep confirms no
`suppress_sync_until = Some(... from_millis(500) ...)` literal remains.
