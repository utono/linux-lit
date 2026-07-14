# Transient-toast helper — design

## Goal

Remove the duplicated "show a label, then auto-hide it after N seconds" tail that
~9 named toast helpers and a batch of inline call sites each re-implement, via one
pure-GTK free helper — with **zero behavior change**, preserving each site's own
toast field, message, and duration. This is audit opportunity #9, deliberately
scoped to the byte-identical auto-hide tail (NOT a unified toast system).

## The duplication

Across `src/app.rs`, `src/input/keymap.rs`, `src/input/search.rs`,
`src/input/navigation.rs`, and the `src/input/actions/*` handlers there are 30
`timeout_add_local_once` closures whose body is exactly `toast.set_visible(false)`.
Every one is preceded by the identical tail:

```rust
s.<TOAST>.set_text(<msg>);
s.<TOAST>.set_visible(true);
let toast = s.<TOAST>.clone();
glib::timeout_add_local_once(std::time::Duration::from_secs(<N>), move || {
    toast.set_visible(false);
});
```

The sites differ only on two axes, both of which must stay at the call site:

- **Which `gtk4::Label`** — `chapter_toast` | `speed_toast` | `search_toast`.
- **Duration** — `from_secs(2)` (8 sites, the "Copied"/calibration confirmations)
  and `from_secs(3)` (the rest).

So the byte-identical part is: set text, show, clone, schedule a `set_visible(false)`
after `secs`. That is the whole helper.

These already exist as ~9 ad-hoc named wrappers re-implementing the same tail —
`show_no_timestamp_toast` (keymap), `edge_toast` (search), `show_no_concordance_toast`
(concordance), `show_no_echo_turns_toast` (echoes), `show_tts_toast`/`voice_picker_toast`
(gloss), plus inline sites in `keymap.rs` (speed/copy/scansion) and `app.rs`
(calibration/page-image/synopsis). Each named wrapper keeps its own message/field;
only its tail collapses to the helper.

## Component

A `pub mod toast;` module at `src/ui/toast.rs` (registered in `src/ui/mod.rs`).
Pure GTK, no `AppState` — takes the `Label` by reference, so it works from both
`&AppState` and `&Rc<RefCell<AppState>>` call sites without borrowing concerns.

```rust
use gtk4::prelude::*;
use gtk4::Label;

/// Show `label` with `text`, then auto-hide it after `secs` seconds.
/// The shared tail of every transient toast: callers pass their own label,
/// message, and duration (each preserved exactly at the call site).
/// Does NOT cover generation-guarded or persistent toasts — see EXCLUDED.
pub(crate) fn show_transient(label: &Label, text: &str, secs: u64) {
    label.set_text(text);
    label.set_visible(true);
    let label = label.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(secs), move || {
        label.set_visible(false);
    });
}
```

## Call-site changes

Each named wrapper and inline site replaces its 5-line tail with one call:

```rust
crate::ui::toast::show_transient(&s.chapter_toast, "No timestamp on this line", 3);
```

The named wrappers (`show_no_timestamp_toast`, `edge_toast`, …) stay — they own
the message construction (e.g. `edge_toast`'s `format!` of the side/query); only
their tail delegates. Inline sites in `keymap.rs`/`app.rs` call the helper directly.

## Explicitly EXCLUDED (structurally different — leave untouched)

- **`show_chapter_toast` (`navigation.rs:1612`)** — generation-guarded: its closure
  checks `chapter_toast_gen` and *keeps the toast visible* if a newer toast
  superseded it. Not a plain `set_visible(false)`; merging would drop the guard.
- **`show_persistent_tts_toast` / `hide_tts_toast` (`gloss.rs`)** — no auto-hide
  timeout at all (shown until explicitly hidden). Not a transient toast.
- **The 5s startup-reveal fallback and 6s nav-fuzz auto-start (`app.rs:1997`,
  `:2016`)** — `timeout_add_local_once` but not toasts; their closures reveal the
  vbox / start the fuzz harness.
- **The 500ms chord-reset timer (`keymap.rs:28`)** — conditional chord-state reset,
  not a toast.
- **`start_chord` and any `timeout_add_local_once` whose closure does more than
  `set_visible(false)`** — out of scope.

## Why not a richer toast system

Rejected: a `Toast` struct / `AppState::toast(field, msg, secs)` method. It would
either need to know all three label fields (coupling the helper to `AppState`) or
still not unify the guarded/persistent variants. The pure `&Label` tail is the
clean, fully behavior-preserving cut; the generation-guard and persistent-toast
behaviors are deliberately left as their own functions.

## Verification

Pure widget-construction + scheduling extraction; no control-flow change. Build
with `cargo build`; `cargo test --bins` for the pure suite. No rendered-spread
check needed (toast visibility is not pagination), but a headless smoke run that
triggers a toast (e.g. `u` on an unmapped line → "No timestamp on this line")
confirms the auto-hide still fires.
