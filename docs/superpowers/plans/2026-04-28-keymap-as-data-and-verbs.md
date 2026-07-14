# F4 + F2: Verbs Out of Dispatch + Keymap as Data Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Two sequential phases (F4 → F2); each ends in commit + manual verification gate.

**Goal:** Shrink `src/input/keymap.rs` from a 1911-line monolith into a thin dispatcher backed by a feature-partitioned `actions/` verb module (F4) and a JSON-loadable keymap (F2). Future re-binding becomes editing a JSON file; future verb additions become a new file in `actions/` plus an enum variant.

**Architecture:**

*F4 (Phase 1).* Extract 12 verbs from `keymap.rs` match-arm bodies into `src/input/actions/{bookmarks,pickers,concordance,settings}.rs`. Verbs encapsulate the borrow/spawn/await dance internally. Each match arm collapses to `crate::input::actions::module::verb(state, tokio_handle)`. Pure code reorganization; no public API change.

*F2 (Phase 2).* Define `Action` enum (one variant per verb + per simple inline action) and `KeyCombo` struct in `src/input/actions/mod.rs` and `src/input/keymap_config.rs`. Add `Keymap` struct loaded from `~/.config/linux-lit/keymap.json` with compiled-in defaults; falls back to defaults on missing/malformed JSON. Replace base-key match block (lines ~1338-1741) with `keymap.lookup → dispatch_action` table. Ship default file as a stow package at `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`.

**Tech Stack:** Rust 2021, GTK4 0.9, libadwaita 0.7, tokio (existing), serde 1 + serde_json 1 (already in `Cargo.toml`). No new dependencies. Stow for the dotfile deployment.

**Source spec:** `docs/superpowers/specs/2026-04-28-keymap-as-data-and-verbs-design.md`.
**Source review:** `docs/reviews/2026-04-28-keymap-vs-references.md` F4 and F2.

**Plan-time discoveries (not in spec, fixed in this plan):**
- `apply_theme_to_state` (currently `pub(crate)` at `keymap.rs:1879`) is called from `revert_to_snapshot` (Escape) and `apply_settings_change`. Must move to `actions/settings.rs` along with those two; its `pub(crate)` becomes `pub` within actions::settings.
- `retry_gloss` (`keymap.rs:1833`) is called from the gloss overlay key handler at line 844 (`Char('r')`). Not in F4 scope (gloss overlay block stays inline), so the function stays in `keymap.rs`.

**Out of scope:**
- F1 (`OverlayMode` trait) — L-effort; spec defers.
- F3 (chord state via one-shot modes) — depends on F1.
- F5–F8 (smaller findings) — all depend on F1.
- Per-overlay keymaps in JSON.
- Multi-key chord COMPLETION (`g`-then-`g`/`g`-then-`;`) stays inline; only chord ENTRY (`Action::PendingG`) routes through dispatch.
- Modifier-conditional overloads (e.g., `Ctrl+p` open-vs-nav) stay inline because the disambiguation is overlay-state-dependent.

---

## File Map

**Phase 1 (F4) creates:**
- `src/input/actions/mod.rs` — module declarations + re-exports.
- `src/input/actions/pickers.rs` — 5 picker verbs.
- `src/input/actions/bookmarks.rs` — 2 bookmark verbs.
- `src/input/actions/concordance.rs` — 2 concordance verbs (`open_concordance_picker` + relocated `handle_word_selection`).
- `src/input/actions/settings.rs` — 4 settings helpers (`apply_settings_change`, `revert_to_snapshot`, `reset_to_defaults`, `apply_theme_to_state`).

**Phase 1 modifies:**
- `src/input/mod.rs` — declare `actions` submodule.
- `src/input/keymap.rs` — delete the relocated free fns and inline match-arm bodies; replace each with a one-line verb call.

**Phase 2 (F2) creates:**
- `src/input/keymap_config.rs` — `KeyCombo`, `Keymap`, `default_reader_bindings()`, `Keymap::load`.
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — canonical default bindings as JSON; deployed via stow.

**Phase 2 modifies:**
- `src/input/actions/mod.rs` — add `Action` enum.
- `src/input/mod.rs` — declare `keymap_config` submodule.
- `src/app.rs` — add `pub keymap: crate::input::keymap_config::Keymap` field to `AppState`; init in constructor.
- `src/input/keymap.rs` — collapse base-key match block; add `dispatch_action(state, action, key_state, tokio_handle) -> bool`.
- `~/utono/linux-lit/CLAUDE.md` — document keymap.json + stow workflow.

---

## Manual Verification Protocol (used after each phase)

```
1. cargo build (must succeed; warnings only).
2. cargo run.
3. Reader navigation:
   - Press x, y, j, k, q, comma — confirm normal nav.
   - Press gg, G — confirm jump-to-start/end.
   - Press [, ], 2, 3 — chapter / scene jumps.
4. Bookmarks:
   - Press m to toggle bookmark; ; / : to walk bookmarks.
   - Ctrl+m to open bookmark picker; navigate, jump, delete.
   - Press g; for jump-to-recent.
5. Pickers:
   - Ctrl+p library picker; Ctrl+Shift+M media picker.
   - Ctrl+\ concordance picker.
6. Settings:
   - Ctrl+, opens settings; j/k navigates; h/l adjusts; r resets;
     Escape reverts; Return saves.
7. MPV / playback:
   - Tab toggles playback; o/e/O/E seek; +/- speed.
8. Vocab / glossing:
   - h toggles vocab popup; \ / # cycle words.
9. Confirm: 'verified' or describe regression.
```

After Phase 2 only, also:

```
10. Edit ~/.config/linux-lit/keymap.json (or stow your own).
    Override one binding (e.g., remap "x" to "PageBackward").
    Restart linux-lit; confirm the override takes effect.
11. Introduce a syntax error in keymap.json.
    Restart; confirm linux-lit logs a warning and falls back to defaults.
```

After each phase commit, paste this protocol and stop.

---

# Phase 1 — F4: Verbs out of dispatch

## Task 1.1: Create `actions/` module skeleton + relocate pure free fns

**Files:**
- Create: `src/input/actions/mod.rs`
- Create: `src/input/actions/concordance.rs`
- Create: `src/input/actions/settings.rs`
- Modify: `src/input/mod.rs` — add `pub mod actions;`
- Modify: `src/input/keymap.rs` — delete the relocated free fns; update one call site.

This task relocates two functions that are already free fns in `keymap.rs` (`handle_concordance_word_selection` at line 77, `apply_settings_change` at line 1782), no body changes needed. It also relocates `apply_theme_to_state` (line 1879) because settings depends on it.

- [ ] **Step 1: Create `src/input/actions/mod.rs`**

```rust
//! Verbs invoked from keymap.rs match arms. Each submodule groups verbs by
//! feature area. Phase 2 (F2) adds the Action enum here and re-exports the
//! verbs as the dispatch target.

pub mod bookmarks;
pub mod concordance;
pub mod pickers;
pub mod settings;
```

(Bookmarks and pickers files don't exist yet — Tasks 1.2 and 1.3 create them. Add the `pub mod` lines now anyway; the build will fail until those files exist.)

Wait — that fails the build. Replace the `pub mod` lines with just the two we're creating in this task:

```rust
//! Verbs invoked from keymap.rs match arms. Each submodule groups verbs by
//! feature area. Phase 2 (F2) adds the Action enum here and re-exports the
//! verbs as the dispatch target.

pub mod concordance;
pub mod settings;
```

Tasks 1.2-1.4 will add the other `pub mod` lines.

- [ ] **Step 2: Add `pub mod actions;` to `src/input/mod.rs`**

Read current contents:

```bash
cd /home/mlj/utono/linux-lit && cat src/input/mod.rs
```

Expected current shape:
```rust
pub mod gamepad;
pub mod keymap;
pub mod navigation;
pub mod search;
pub mod timestamps;
pub mod visual;
```

Add `pub mod actions;` (alphabetical position):

```rust
pub mod actions;
pub mod gamepad;
pub mod keymap;
pub mod navigation;
pub mod search;
pub mod timestamps;
pub mod visual;
```

- [ ] **Step 3: Create `src/input/actions/concordance.rs`**

Find `handle_concordance_word_selection` in `keymap.rs` (line 77) and read its full body:

```bash
cd /home/mlj/utono/linux-lit && sed -n '77,157p' src/input/keymap.rs
```

Move the entire function body verbatim into a new `src/input/actions/concordance.rs`. Add necessary `use` lines at the top:

```rust
use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;
use crate::input::navigation;

/// Handle concordance word selection: partition hits by work, set up same-work
/// concordance state, and spawn new instances for other works.
pub(crate) fn handle_word_selection(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
    word: String,
) {
    // ... (paste the body verbatim from keymap.rs:77-157, replacing the function
    // signature line with the one above — note the rename from
    // handle_concordance_word_selection to handle_word_selection)
}
```

The function rename is intentional — namespacing `concordance::handle_word_selection` is cleaner than `concordance::handle_concordance_word_selection`. The full body (the `glib::spawn_future_local(async move { ... })` block plus the partitioning logic) moves verbatim.

- [ ] **Step 4: Update the call site in `keymap.rs`**

Find the two call sites (the existing function name was `handle_concordance_word_selection`):

```bash
cd /home/mlj/utono/linux-lit && grep -n "handle_concordance_word_selection" src/input/keymap.rs
```

Expected: 2 call sites at the concordance picker and concordance word picker `Return` arms (around lines 922 and 947 respectively).

Update each call site from:
```rust
handle_concordance_word_selection(state, tokio_handle, word);
```
to:
```rust
crate::input::actions::concordance::handle_word_selection(state, tokio_handle, word);
```

- [ ] **Step 5: Delete the original `handle_concordance_word_selection` function from `keymap.rs`**

Delete lines 77-157 (the entire function body) from `src/input/keymap.rs`. Confirm with grep that no references remain:

```bash
cd /home/mlj/utono/linux-lit && grep -n "fn handle_concordance_word_selection\|handle_concordance_word_selection" src/input/keymap.rs
```

Expected: no matches.

- [ ] **Step 6: Create `src/input/actions/settings.rs`**

This file gets THREE relocations: `apply_settings_change`, `apply_theme_to_state`, plus a stub for `revert_to_snapshot` and `reset_to_defaults` that Task 1.5 will fill in.

Find the source functions:

```bash
cd /home/mlj/utono/linux-lit && grep -n "^fn apply_settings_change\|^pub(crate) fn apply_theme_to_state" src/input/keymap.rs
```

Read both:

```bash
cd /home/mlj/utono/linux-lit && sed -n '1782,1831p' src/input/keymap.rs
cd /home/mlj/utono/linux-lit && sed -n '1879,1911p' src/input/keymap.rs
```

Create `src/input/actions/settings.rs` with both functions verbatim (renaming `apply_theme_to_state`'s visibility from `pub(crate)` to `pub(crate)` stays the same — same crate, just different module). Add `use` lines:

```rust
use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;

/// Apply a SettingsChange variant to AppState in-place. Called from the
/// settings overlay's h/l/j/k key handlers.
pub(crate) fn apply_settings_change(
    state: &Rc<RefCell<crate::app::AppState>>,
    change: crate::ui::settings_overlay::SettingsChange,
) {
    use crate::ui::settings_overlay::SettingsChange;
    let mut s = state.borrow_mut();
    // ... (paste body from keymap.rs:1782-1831 verbatim)
}

/// Apply a theme to AppState: load CSS, update tag colors, write
/// .current_theme. Called from settings overlay's theme cycling and from
/// revert_to_snapshot.
pub(crate) fn apply_theme_to_state(
    state: &mut crate::app::AppState,
    theme: &crate::theme::Theme,
) {
    // ... (paste body from keymap.rs:1879-1911 verbatim)
}
```

- [ ] **Step 7: Update the two call sites of `apply_theme_to_state` in `keymap.rs`**

Find them:

```bash
cd /home/mlj/utono/linux-lit && grep -n "apply_theme_to_state" src/input/keymap.rs
```

Expected: 3 matches — the function definition (at ~line 1879, will be deleted in Step 9) and 2 call sites (at ~lines 736 and 1817, both inside settings-overlay handlers).

For each CALL SITE (not the def), replace:
```rust
apply_theme_to_state(&mut s, &snap_theme);
```
with:
```rust
crate::input::actions::settings::apply_theme_to_state(&mut s, &snap_theme);
```

(Both call sites use the same form; just prefix with the module path.)

- [ ] **Step 8: Update `apply_settings_change` call sites in `keymap.rs`**

```bash
cd /home/mlj/utono/linux-lit && grep -n "apply_settings_change" src/input/keymap.rs
```

Expected: 3 matches — definition (deleted Step 9) and 2 call sites (inside `h`/`l` arms of settings overlay handler around lines 765 and 774).

Replace each call site from:
```rust
apply_settings_change(state, change);
```
to:
```rust
crate::input::actions::settings::apply_settings_change(state, change);
```

- [ ] **Step 9: Delete the original `apply_settings_change` and `apply_theme_to_state` functions from `keymap.rs`**

Delete the function body for each. Confirm:

```bash
cd /home/mlj/utono/linux-lit && grep -n "^fn apply_settings_change\|^pub(crate) fn apply_theme_to_state" src/input/keymap.rs
```

Expected: no matches.

- [ ] **Step 10: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: compiles. New `dead_code` warnings for `actions::concordance::handle_word_selection`, `actions::settings::apply_settings_change`, `actions::settings::apply_theme_to_state` — wait, these all have call sites. Should be no new warnings. If you see "unresolved import" or "module not found", you forgot Step 2 (`pub mod actions;` in `src/input/mod.rs`).

- [ ] **Step 11: Run tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -5
```

Expected: 118 pass / 1 pre-existing fail (`mpv::client::tests::test_find_line_for_time`). No new failures.

- [ ] **Step 12: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/mod.rs src/input/actions/mod.rs src/input/actions/concordance.rs src/input/actions/settings.rs src/input/keymap.rs && git commit -m "$(cat <<'EOF'
Create actions/ module; relocate concordance + settings free fns

Phase 1.1 of the F4+F2 keymap refactor. Moves three functions that are
already free fns in keymap.rs into the new src/input/actions/ module
without changing bodies:

- handle_concordance_word_selection -> actions::concordance::handle_word_selection
- apply_settings_change -> actions::settings::apply_settings_change
- apply_theme_to_state -> actions::settings::apply_theme_to_state

Pure relocation — no body changes, no behavior changes. Establishes the
actions/ module skeleton for Tasks 1.2-1.5 to extract verbs into.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 1.2: Extract picker verbs into `actions/pickers.rs`

**Files:**
- Create: `src/input/actions/pickers.rs`
- Modify: `src/input/actions/mod.rs` — add `pub mod pickers;`
- Modify: `src/input/keymap.rs` — replace 5 inlined match-arm bodies with verb calls.

This task extracts five picker-related verbs. Each is currently a long async closure inlined at its match arm.

- [ ] **Step 1: Add `pub mod pickers;` to `src/input/actions/mod.rs`**

Insert the line in alphabetical position:

```rust
pub mod concordance;
pub mod pickers;
pub mod settings;
```

- [ ] **Step 2: Create `src/input/actions/pickers.rs` with the 5 verbs**

Read each source location first to copy bodies verbatim:

```bash
cd /home/mlj/utono/linux-lit && grep -n "^fn load_selected_work" src/input/keymap.rs
cd /home/mlj/utono/linux-lit && grep -n "load_bookmarks_with_details\|list_media_for_work" src/input/keymap.rs
```

`load_selected_work` is already a free fn at line 17; pure relocation. The other four (`open_bookmark_picker`, `open_media_picker`, `confirm_media_selection`, `delete_bookmark`) are inlined match-arm bodies.

Write `src/input/actions/pickers.rs`:

```rust
use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use crate::app::AppState;

/// Load the selected work in the library picker, hide the picker, and
/// display the new work. Spawns an async task to query the DB.
pub(crate) fn load_selected_work(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    // ... (paste body from keymap.rs:17-73 verbatim, including the inner
    // glib::spawn_future_local block. Drop the leading/trailing fn signature
    // since it now has the new signature above.)
}

/// Open the bookmark picker, querying bookmarks for the current work.
/// Spawns an async task to query the DB.
pub(crate) fn open_bookmark_picker(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());
    if let Some(abbrev) = abbrev {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let items = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::queries::load_bookmarks_with_details(&conn, &abbrev)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            {
                let mut s = state_clone.borrow_mut();
                s.correction_overlay.hide();
                s.bookmark_picker.set_items(items);
            }
            state_clone.borrow().bookmark_picker.show();
        });
    }
}

/// Open the media picker, querying media files for the current work.
/// Spawns an async task to query the DB.
pub(crate) fn open_media_picker(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());
    if let Some(abbrev) = abbrev {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let items = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::queries::list_media_for_work(&conn, &abbrev)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            {
                let mut s = state_clone.borrow_mut();
                s.correction_overlay.hide();
                s.media_picker.set_items(items);
            }
            state_clone.borrow().media_picker.show();
        });
    }
}

/// Confirm the selected media file: discover or launch the MPV socket,
/// re-send filtered timestamps, and connect MPV. Called from the media
/// picker's Return key.
pub(crate) fn confirm_media_selection(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let selected_path = state.borrow().media_picker.selected_media_path();
    let selected_id = state.borrow().media_picker.selected_media_id();
    if let (Some(path), Some(media_id)) = (selected_path, selected_id) {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let socket_path = handle
                .spawn_blocking(move || {
                    if let Some((sock, _)) =
                        crate::mpv::discovery::find_socket_for_work(&[path.clone()])
                    {
                        return sock.to_string_lossy().to_string();
                    }
                    let launched = crate::mpv::discovery::launch_mpv(&path);
                    for _ in 0..60 {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        if std::path::Path::new(&launched).exists() {
                            return launched;
                        }
                    }
                    launched
                })
                .await
                .unwrap_or_default();

            if !socket_path.is_empty() {
                let mut s = state_clone.borrow_mut();
                s.media_id = Some(media_id);
                if let Some(ref work) = s.current_work {
                    let mut ts_data: Vec<(i64, f64, f64)> = work
                        .timestamps
                        .iter()
                        .filter(|t| t.media_id == media_id)
                        .map(|t| (t.line_id, t.start, t.end))
                        .collect();
                    ts_data.sort_by(|a, b| {
                        a.1.partial_cmp(&b.1)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    let mut id_to_idx: std::collections::HashMap<i64, usize> =
                        std::collections::HashMap::new();
                    for (i, line) in work.lines.iter().enumerate() {
                        id_to_idx.insert(line.id, i);
                    }
                    let _ = s.cmd_tx.try_send(
                        crate::mpv::MpvCommand::SetTimestamps {
                            timestamps: ts_data,
                            line_id_to_index: id_to_idx,
                        },
                    );
                }
                let _ = s
                    .cmd_tx
                    .try_send(crate::mpv::MpvCommand::Connect(socket_path));
                s.media_picker.hide();
                crate::logging::log(&format!(
                    "MEDIA: switched to media_id={}",
                    media_id
                ));
            }
        });
    }
}

/// Delete the selected bookmark from DB and update AppState's is_bookmarked
/// vec + gutter renderer. Called from the bookmark picker's Delete/d key.
pub(crate) fn delete_bookmark(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let selected_id = state.borrow().bookmark_picker.selected_line_mapping_id();
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());
    if let (Some(lm_id), Some(abbrev)) = (selected_id, abbrev) {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let result = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db_rw()
                        .expect("Failed to open lit.db rw");
                    crate::db::queries::delete_bookmark(&conn, &abbrev, lm_id)
                })
                .await;
            if let Ok(Ok(())) = result {
                let mut s = state_clone.borrow_mut();
                let buffer_line = if let Some(ref lm) = s.line_map {
                    s.current_work.as_ref().and_then(|w| {
                        let work_idx = w.lines.iter().position(|l| l.id == lm_id)?;
                        Some(lm.work_to_buffer[work_idx])
                    })
                } else {
                    s.current_work.as_ref().and_then(|w| {
                        w.lines.iter().position(|l| l.id == lm_id)
                    })
                };
                if let Some(bl) = buffer_line {
                    let mut bm = s.is_bookmarked.borrow_mut();
                    if bl < bm.len() {
                        bm[bl] = false;
                    }
                }
                if let Some(ref renderer) = s.gutter_renderer {
                    renderer.queue_draw();
                }
                s.bookmark_picker.remove_selected();
                if !s.bookmark_picker.has_items() {
                    s.bookmark_picker.hide();
                }
            }
        });
    }
}
```

The verb bodies are byte-for-byte identical to what's currently inlined in `keymap.rs` at the listed line ranges. Only the wrapping (function signature, imports) is new.

- [ ] **Step 3: Update `keymap.rs` call sites — `load_selected_work`**

```bash
cd /home/mlj/utono/linux-lit && grep -n "load_selected_work" src/input/keymap.rs
```

Expected: 3 matches — definition (line 17, will be deleted Step 8) and 2 call sites in the picker `Return` handler (around lines 260 and 266).

Replace each call site:
```rust
load_selected_work(state, tokio_handle);
```
with:
```rust
crate::input::actions::pickers::load_selected_work(state, tokio_handle);
```

- [ ] **Step 4: Update `keymap.rs` `Ctrl+m` arm**

Find it:

```bash
cd /home/mlj/utono/linux-lit && grep -n "Ctrl+m: open bookmark picker" src/input/keymap.rs
```

Expected: ~line 650, followed by ~28 lines of inline bookmark picker open logic.

Replace the entire arm body (lines ~651-679) with:

```rust
    // Ctrl+m: open bookmark picker
    if is_ctrl && !is_shift && key_name == "m" {
        crate::input::actions::pickers::open_bookmark_picker(state, tokio_handle);
        return true;
    }
```

- [ ] **Step 5: Update `keymap.rs` `Ctrl+Shift+M` arm**

Find it:

```bash
cd /home/mlj/utono/linux-lit && grep -n "Ctrl+Shift+M: open media picker" src/input/keymap.rs
```

Expected: ~line 619.

Replace the entire arm (lines ~620-648) with:

```rust
    // Ctrl+Shift+M: open media picker
    if is_ctrl && is_shift && key_name == "M" {
        crate::input::actions::pickers::open_media_picker(state, tokio_handle);
        return true;
    }
```

- [ ] **Step 6: Update `keymap.rs` media picker `Return` arm**

Find the match arm:

```bash
cd /home/mlj/utono/linux-lit && grep -n "selected_media_path\|selected_media_id" src/input/keymap.rs
```

Expected: the Return arm body around lines 438-503 inside the `if media_picker_visible { match key_name { "Return" => {...} } }` block.

Replace the body with:

```rust
            "Return" => {
                crate::input::actions::pickers::confirm_media_selection(state, tokio_handle);
                return true;
            }
```

- [ ] **Step 7: Update `keymap.rs` bookmark picker `Delete | d` arm**

Find it:

```bash
cd /home/mlj/utono/linux-lit && grep -n "selected_line_mapping_id" src/input/keymap.rs
```

Expected: `Delete | d` arm at ~line 343.

This arm has a special check — it only calls delete when the search entry isn't focused (so typing `d` in the search box doesn't delete a bookmark). Preserve that wrapper:

```rust
            "Delete" | "d" => {
                let is_search_focused = state.borrow().bookmark_picker.search_entry().has_focus();
                if key_name == "Delete" || !is_search_focused {
                    crate::input::actions::pickers::delete_bookmark(state, tokio_handle);
                    return true;
                }
            }
```

- [ ] **Step 8: Delete `load_selected_work` definition from `keymap.rs`**

Delete lines 17-73 (the original `fn load_selected_work` body). Confirm:

```bash
cd /home/mlj/utono/linux-lit && grep -n "^fn load_selected_work" src/input/keymap.rs
```

Expected: no matches.

- [ ] **Step 9: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: compiles, no new warnings (the verbs all have call sites).

If "use of unresolved module": you forgot Step 1's `pub mod pickers;` in `actions/mod.rs`.

- [ ] **Step 10: Run tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -5
```

Expected: 118 pass / 1 pre-existing fail.

- [ ] **Step 11: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/actions/mod.rs src/input/actions/pickers.rs src/input/keymap.rs && git commit -m "$(cat <<'EOF'
Extract picker verbs into actions::pickers

Phase 1.2 of F4+F2 keymap refactor. Moves five picker-related verbs
(load_selected_work, open_bookmark_picker, open_media_picker,
confirm_media_selection, delete_bookmark) out of inlined match-arm
bodies in keymap.rs into actions/pickers.rs.

Each verb encapsulates its own glib::spawn_future_local + Rc<RefCell>
clone + tokio::runtime::Handle clone dance. Match arms collapse to
one-line dispatch. ~150 lines removed from keymap.rs net.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 1.3: Extract bookmark verbs into `actions/bookmarks.rs`

**Files:**
- Create: `src/input/actions/bookmarks.rs`
- Modify: `src/input/actions/mod.rs` — add `pub mod bookmarks;`
- Modify: `src/input/keymap.rs` — replace 2 inlined match-arm bodies.

- [ ] **Step 1: Add `pub mod bookmarks;` to `src/input/actions/mod.rs`**

Insert at top (alphabetical):

```rust
pub mod bookmarks;
pub mod concordance;
pub mod pickers;
pub mod settings;
```

- [ ] **Step 2: Create `src/input/actions/bookmarks.rs`**

```rust
use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;
use crate::input::navigation;

/// Toggle a bookmark on the current cursor line. Updates DB, AppState's
/// is_bookmarked vec, and gutter renderer. Called from `m` in reader.
pub(crate) fn toggle_bookmark(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let (abbrev, line_mapping_id, buffer_line) = {
        let s = state.borrow();
        let abbrev = s.current_work.as_ref().map(|w| w.abbrev.clone());
        let lm_id = s.current_work.as_ref().and_then(|w| {
            let work_idx = if let Some(ref lm) = s.line_map {
                lm.buffer_to_work.get(s.current_line)?.as_ref().copied()
            } else {
                Some(s.current_line)
            };
            work_idx.and_then(|wi| w.lines.get(wi).map(|l| l.id))
        });
        (abbrev, lm_id, s.current_line)
    };
    if let (Some(abbrev), Some(lm_id)) = (abbrev, line_mapping_id) {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let result = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db_rw()
                        .expect("Failed to open lit.db rw");
                    crate::db::queries::toggle_bookmark(&conn, &abbrev, lm_id)
                })
                .await;
            if let Ok(Ok(added)) = result {
                let s = state_clone.borrow();
                {
                    let mut bm = s.is_bookmarked.borrow_mut();
                    if buffer_line < bm.len() {
                        bm[buffer_line] = added;
                    }
                }
                if let Some(ref renderer) = s.gutter_renderer {
                    renderer.queue_draw();
                }
            }
        });
    }
}

/// Jump to the most recently created bookmark in the current work.
/// Called from `g;` chord.
pub(crate) fn jump_to_recent_bookmark(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());
    if let Some(abbrev) = abbrev {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let result = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db()
                        .expect("Failed to open lit.db");
                    crate::db::queries::most_recent_bookmark(&conn, &abbrev)
                })
                .await;
            if let Ok(Ok(Some(lm_id))) = result {
                let mut s = state_clone.borrow_mut();
                let buffer_line = if let Some(ref lm) = s.line_map {
                    s.current_work.as_ref().and_then(|w| {
                        let work_idx = w.lines.iter().position(|l| l.id == lm_id)?;
                        Some(lm.work_to_buffer[work_idx])
                    })
                } else {
                    s.current_work.as_ref().and_then(|w| {
                        w.lines.iter().position(|l| l.id == lm_id)
                    })
                };
                if let Some(bl) = buffer_line {
                    navigation::jump_to_line(&mut s, bl);
                }
            }
        });
    }
}
```

Bodies copied verbatim from `keymap.rs` (`m` arm at ~lines 1591-1631; `g;` chord at ~lines 1110-1147).

- [ ] **Step 3: Replace `m` (toggle bookmark) arm in `keymap.rs`**

Find it:

```bash
cd /home/mlj/utono/linux-lit && grep -n '^        "m" =>' src/input/keymap.rs
```

The arm currently spans ~40 lines (the destructuring let, the if-let pair, the spawn_future_local, etc.). Replace the entire arm body with:

```rust
        "m" => {
            crate::input::actions::bookmarks::toggle_bookmark(state, tokio_handle);
            true
        }
```

- [ ] **Step 4: Replace `g;` chord handler in `keymap.rs`**

Find it:

```bash
cd /home/mlj/utono/linux-lit && grep -n '"semicolon"' src/input/keymap.rs
```

Multiple matches expected. The g; chord is in the `if key_state.borrow().pending_g` block — find:

```bash
cd /home/mlj/utono/linux-lit && grep -n 'g; — jump to most recently' src/input/keymap.rs
```

Expected: ~line 1111. The arm currently spans ~36 lines. Replace the entire `else if key_name == "semicolon" {` branch body (the whole block inside that `else if`) with:

```rust
        } else if key_name == "semicolon" {
            // g; — jump to most recently created bookmark
            crate::input::actions::bookmarks::jump_to_recent_bookmark(state, tokio_handle);
            return true;
        }
```

- [ ] **Step 5: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 6: Run tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -3
```

Expected: 118 / 1 pre-existing fail.

- [ ] **Step 7: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/actions/mod.rs src/input/actions/bookmarks.rs src/input/keymap.rs && git commit -m "$(cat <<'EOF'
Extract bookmark verbs into actions::bookmarks

Phase 1.3 of F4+F2 keymap refactor. Moves toggle_bookmark (m key) and
jump_to_recent_bookmark (g; chord) out of inlined match-arm bodies in
keymap.rs into actions/bookmarks.rs. Each verb encapsulates its own
async DB query + state mutation + gutter redraw.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 1.4: Extract concordance picker open verb

**Files:**
- Modify: `src/input/actions/concordance.rs` — add `open_picker` verb.
- Modify: `src/input/keymap.rs` — replace `Ctrl+\` arm.

- [ ] **Step 1: Append `open_picker` to `src/input/actions/concordance.rs`**

Read the current `Ctrl+\` arm body in `keymap.rs`:

```bash
cd /home/mlj/utono/linux-lit && grep -n '"backslash"' src/input/keymap.rs | head -5
```

Multiple matches; the `Ctrl+\` arm is inside the `if is_ctrl { match key_name { "backslash" => {...} } }` block at ~lines 1225-1253.

Append to `src/input/actions/concordance.rs`:

```rust
/// Open the concordance picker, populating it with the current work's vocab
/// words. Called from `Ctrl+\`.
pub(crate) fn open_picker(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());
    if let Some(abbrev) = abbrev {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let words = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::queries::load_vocab_word_list(&conn, &abbrev)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            {
                let mut s = state_clone.borrow_mut();
                s.concordance_picker.set_words(words);
                s.concordance_picker.show();
            }
            // set_text triggers connect_changed which borrows state, so the
            // mutable borrow must be dropped first.
            state_clone.borrow().concordance_picker.search_entry().set_text("");
        });
    }
}
```

(Body verbatim from `keymap.rs:1225-1253`. The trailing `set_text("")` after the spawn block stays inside the spawn closure — it's the same pattern.)

- [ ] **Step 2: Replace `Ctrl+\` arm in `keymap.rs`**

```bash
cd /home/mlj/utono/linux-lit && sed -n '1224,1255p' src/input/keymap.rs
```

(Confirm you're looking at the right block.)

Replace the entire `"backslash" => { ... }` arm body inside the `if is_ctrl { match key_name { ... } }` block with:

```rust
            "backslash" => {
                crate::input::actions::concordance::open_picker(state, tokio_handle);
                return true;
            }
```

- [ ] **Step 3: Build + test**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -5 && cargo test 2>&1 | grep -E "^test result" | tail -2
```

Expected: clean build, 118 / 1 fail.

- [ ] **Step 4: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/actions/concordance.rs src/input/keymap.rs && git commit -m "$(cat <<'EOF'
Extract concordance::open_picker verb

Phase 1.4 of F4+F2 keymap refactor. Moves the Ctrl+\ concordance picker
open logic out of an inlined match-arm body into actions/concordance.rs.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 1.5: Extract settings revert/reset verbs

**Files:**
- Modify: `src/input/actions/settings.rs` — add `revert_to_snapshot` and `reset_to_defaults`.
- Modify: `src/input/keymap.rs` — replace settings overlay's Escape and `r` arms.

- [ ] **Step 1: Append `revert_to_snapshot` and `reset_to_defaults` to `actions/settings.rs`**

Read the current Escape and `r` arm bodies (~lines 702-741 and 777-812 respectively):

```bash
cd /home/mlj/utono/linux-lit && sed -n '702,741p' src/input/keymap.rs
cd /home/mlj/utono/linux-lit && sed -n '777,812p' src/input/keymap.rs
```

Append to `src/input/actions/settings.rs`:

```rust
/// Revert AppState to the snapshot taken when the settings overlay opened,
/// then hide the overlay. Called from Escape in settings overlay.
pub(crate) fn revert_to_snapshot(state: &Rc<RefCell<AppState>>) {
    let (snap_ls, snap_cw, snap_tm, snap_ti, snap_nm, snap_ts, snap_cl) =
        state.borrow().settings_overlay.snapshot();
    let mut s = state.borrow_mut();
    if s.dialogue_formatting_active {
        let tag_table = s.buffer.tag_table();
        if let Some(tag) = tag_table.lookup("speaker-gap") {
            tag.set_property("pixels-above-lines", snap_ls.max(1) as i32 * 5);
        }
    } else {
        s.text_view.set_pixels_above_lines((snap_ls as i32).max(0));
        s.text_view.set_pixels_below_lines((snap_ls as i32).max(0));
    }
    crate::app::apply_card_sizing(&s.content_hbox, s.window.width(), snap_cw);
    let work_type = s.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or("");
    let is_verse = !crate::db::line_types::is_prose_work(work_type);
    let verse_bump = if is_verse {
        crate::app::verse_left_offset(s.window.width(), snap_cw)
    } else {
        0
    };
    s.text_view.set_left_margin(snap_tm as i32 + verse_bump);
    s.text_view.set_right_margin(snap_tm as i32 + crate::config::EXTRA_RIGHT_MARGIN);
    s.config.line_spacing = snap_ls;
    s.config.column_width = snap_cw;
    s.config.text_margins = snap_tm;
    s.config.navigation_mode = snap_nm;
    s.config.transition_style = snap_ts;
    s.config.show_cursor_line = snap_cl;
    if s.dialogue_formatting_active {
        crate::app::apply_dialogue_formatting(&mut s);
    }
    crate::input::navigation::update_highlight_only(&mut s);
    if let Some(snap_theme) = s.settings_overlay.themes().get(snap_ti) {
        let snap_theme = snap_theme.clone();
        s.settings_overlay.set_theme_index(snap_ti);
        apply_theme_to_state(&mut s, &snap_theme);
    }
    s.settings_overlay.hide();
}

/// Reset AppState to default settings. Called from `r` in settings overlay.
pub(crate) fn reset_to_defaults(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    let ls = crate::config::DEFAULT_LINE_SPACING;
    let cw = crate::config::DEFAULT_COLUMN_WIDTH;
    let tm = crate::config::DEFAULT_TEXT_MARGINS;
    let nm = crate::config::NavigationMode::default();
    let ts = crate::config::TransitionStyle::default();
    if s.dialogue_formatting_active {
        let tag_table = s.buffer.tag_table();
        if let Some(tag) = tag_table.lookup("speaker-gap") {
            tag.set_property("pixels-above-lines", ls.max(1) as i32 * 5);
        }
    } else {
        s.text_view.set_pixels_above_lines((ls as i32).max(0));
        s.text_view.set_pixels_below_lines((ls as i32).max(0));
    }
    crate::app::apply_card_sizing(&s.content_hbox, s.window.width(), cw);
    let work_type = s.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or("");
    let is_verse = !crate::db::line_types::is_prose_work(work_type);
    let verse_bump = if is_verse {
        crate::app::verse_left_offset(s.window.width(), cw)
    } else {
        0
    };
    s.text_view.set_left_margin(tm as i32 + verse_bump);
    s.text_view.set_right_margin(tm as i32 + crate::config::EXTRA_RIGHT_MARGIN);
    s.config.line_spacing = ls;
    s.config.column_width = cw;
    s.config.text_margins = tm;
    s.config.navigation_mode = nm;
    s.config.transition_style = ts;
    s.config.show_cursor_line = false;
    if s.dialogue_formatting_active {
        crate::app::apply_dialogue_formatting(&mut s);
    }
    crate::input::navigation::update_highlight_only(&mut s);
    s.settings_overlay.update_displayed_values(ls, cw, tm, nm, ts, false);
}
```

(Both bodies are byte-for-byte identical to the current keymap arm bodies, just wrapped in a function and with `state.borrow_mut()` extracted from `state.borrow_mut(); { ... }` pattern.)

- [ ] **Step 2: Replace settings overlay Escape arm in `keymap.rs`**

Find it:

```bash
cd /home/mlj/utono/linux-lit && grep -n "Revert to snapshot values" src/input/keymap.rs
```

Expected: ~line 703. The arm body spans ~38 lines. Replace it (keep the outer arm wrapping `"Escape" => { ... }`):

```rust
            "Escape" => {
                crate::input::actions::settings::revert_to_snapshot(state);
                return true;
            }
```

- [ ] **Step 3: Replace settings overlay `r` arm in `keymap.rs`**

Find it:

```bash
cd /home/mlj/utono/linux-lit && grep -n "Reset to defaults" src/input/keymap.rs
```

Expected: ~line 778. Replace the arm:

```rust
            "r" => {
                crate::input::actions::settings::reset_to_defaults(state);
                return true;
            }
```

- [ ] **Step 4: Build + test**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -5 && cargo test 2>&1 | grep -E "^test result" | tail -2
```

Expected: clean, 118 / 1.

- [ ] **Step 5: Manual verification (FIRST GATE)**

Paste the Manual Verification Protocol into chat. Stop and wait for the user.

Critical things to test specifically for Phase 1:
- All keymap functions still work (page nav, bookmarks, pickers, settings, MPV).
- Settings overlay Escape correctly reverts AND closes.
- Settings overlay `r` correctly resets AND keeps the overlay open.
- Settings overlay theme cycling still works (uses apply_theme_to_state via apply_settings_change).

If user reports a regression: revert with `git checkout src/input/`, diagnose. Most likely cause: the verb's `state.borrow_mut()` pattern conflicts with a caller that already holds a borrow. Compare the verb body to the original keymap arm body line-by-line.

- [ ] **Step 6: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/actions/settings.rs src/input/keymap.rs && git commit -m "$(cat <<'EOF'
Extract settings revert + reset verbs into actions::settings

Phase 1.5 of F4+F2 keymap refactor. Moves the settings overlay's Escape
revert (40 lines) and r reset (36 lines) handlers out of inlined
match-arm bodies into actions/settings.rs. Both call apply_theme_to_state
which moved to actions::settings in Phase 1.1.

Phase 1 (F4) complete: 12 verbs relocated across 4 actions/ files;
keymap.rs reduced by ~250 lines net. No public API change. Manual
verification confirms no behavior regression.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Phase 2 — F2: Keymap as data

## Task 2.1: Define `Action` enum and `KeyCombo` struct

**Files:**
- Modify: `src/input/actions/mod.rs` — add `Action` enum.
- Create: `src/input/keymap_config.rs` — add `KeyCombo` struct.
- Modify: `src/input/mod.rs` — declare `keymap_config`.

- [ ] **Step 1: Add `Action` enum to `src/input/actions/mod.rs`**

Append after the existing `pub mod` declarations:

```rust
//! Action enum identifying every reader-mode behavior. F2 maps KeyCombo →
//! Action via Keymap; dispatch_action in keymap.rs translates Action into
//! the corresponding verb call.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Action {
    // Page navigation
    PageForward,
    PageBackward,
    PageBackwardBottom,
    JumpToStart,
    JumpToEnd,

    // Cursor / dialogue navigation
    CursorNextDialogue,
    CursorPrevLine,
    CursorToPageBottom,
    JumpToNextDialogue,
    JumpToPrevDialogue,
    JumpToNextChapter,
    JumpToPrevChapter,
    JumpToNextScene,
    JumpToPrevScene,

    // Bookmarks
    ToggleBookmark,
    NextBookmark,
    PrevBookmark,
    JumpToRecentBookmark,
    OpenBookmarkPicker,

    // Pickers / overlays
    OpenLibraryPicker,
    OpenMediaPicker,
    OpenConcordancePicker,
    OpenConcordanceWordPicker,
    OpenConcordanceListPicker,
    OpenSettingsOverlay,
    OpenKeybindsOverlay,
    OpenSearch,

    // MPV / media
    TogglePlaybackSync,
    TogglePlayback,
    SeekShortBackward,
    SeekShortForward,
    SeekLongBackward,
    SeekLongForward,
    SeekBackward30,
    VolumeUp,
    VolumeDown,
    TogglePlaybackSpeed,

    // Vocab / glossing
    ToggleVocabPopup,
    VocabPopupNext,
    VocabPopupPrev,
    JumpToNextVocab,
    JumpToPrevVocab,
    ToggleVocabHighlight,

    // Visual / selection
    EnterVisualMode,
    WordCycleCopy,
    WordCollectCopy,

    // Translations
    ToggleTranslations,

    // Settings (in reader)
    AdjustFontSizeUp,
    AdjustFontSizeDown,
    ResetFontSize,
    CycleFontForward,
    CycleFontBackward,
    ToggleSignColumn,
    ToggleCursorLine,
    ToggleDim,
    ShowFontInfo,

    // Timestamps
    SetStartTime,
    SetEndTime,
    SetChapter,
    DeleteTimestamp,
    NudgeStartBackward,
    NudgeStartForward,
    UndoTimestamp,
    PlayCurrentLine,

    // App
    SaveAndQuit,
    ToggleDebugLogging,
    CopyLineMappingId,

    // Multi-key chords (entry — completion handled by KeyState)
    PendingG,

    // Search (in reader, when matches present)
    SearchNextMatch,
    SearchPrevMatch,
}
```

- [ ] **Step 2: Create `src/input/keymap_config.rs` with `KeyCombo`**

```rust
//! Keymap configuration: KeyCombo struct + Keymap loader.
//!
//! Loaded from ~/.config/linux-lit/keymap.json with compiled-in defaults.
//! Falls back to defaults on missing or malformed JSON. Mirrors lue's
//! load_keyboard_shortcuts pattern (lue/lue/input_handler.py:48-64).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::input::actions::Action;

/// One key combination. `key` is the GDK key name as logged by handle_key
/// (e.g., "x", "Return", "BackSpace", "comma"). Modifiers default to false
/// when omitted from JSON.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct KeyCombo {
    pub key: String,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
}

impl KeyCombo {
    pub fn plain(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: false, shift: false, alt: false }
    }
    pub fn ctrl(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: true, shift: false, alt: false }
    }
    pub fn shift(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: false, shift: true, alt: false }
    }
    pub fn alt(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: false, shift: false, alt: true }
    }
    pub fn ctrl_shift(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: true, shift: true, alt: false }
    }
    pub fn ctrl_alt(key: &str) -> Self {
        Self { key: key.to_string(), ctrl: true, shift: false, alt: true }
    }
}
```

- [ ] **Step 3: Add `pub mod keymap_config;` to `src/input/mod.rs`**

```rust
pub mod actions;
pub mod gamepad;
pub mod keymap;
pub mod keymap_config;
pub mod navigation;
pub mod search;
pub mod timestamps;
pub mod visual;
```

- [ ] **Step 4: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: compiles. New `dead_code` warnings on `Action`, `KeyCombo`, and the `KeyCombo` constructors — clears in Tasks 2.2-2.5 when callers wire up.

- [ ] **Step 5: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/actions/mod.rs src/input/keymap_config.rs src/input/mod.rs && git commit -m "$(cat <<'EOF'
Define Action enum + KeyCombo struct (no callers yet)

Phase 2.1 of F4+F2 keymap refactor. Adds the Action enum (one variant
per reader behavior, ~70 entries) and the KeyCombo struct in a new
src/input/keymap_config.rs module. Both derive Serialize/Deserialize
for JSON parsing.

Currently unused; Tasks 2.2-2.5 wire up the Keymap loader, dispatcher,
and migration of base-key match arms.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2.2: Implement `Keymap` struct with `default_reader_bindings()` + tests

**Files:**
- Modify: `src/input/keymap_config.rs` — add `Keymap`, `default_reader_bindings`, `Keymap::load`, tests.

- [ ] **Step 1: Write the failing tests**

Append to `src/input/keymap_config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::actions::Action;

    #[test]
    fn default_reader_bindings_returns_nonempty_map() {
        let m = default_reader_bindings();
        assert!(m.len() > 50, "expected ~70 default bindings, got {}", m.len());
    }

    #[test]
    fn default_reader_bindings_contains_known_bindings() {
        let m = default_reader_bindings();
        assert_eq!(m.get(&KeyCombo::plain("x")), Some(&Action::PageForward));
        assert_eq!(m.get(&KeyCombo::plain("y")), Some(&Action::PageBackward));
        assert_eq!(m.get(&KeyCombo::plain("j")), Some(&Action::CursorNextDialogue));
        assert_eq!(m.get(&KeyCombo::plain("k")), Some(&Action::CursorPrevLine));
        assert_eq!(m.get(&KeyCombo::ctrl("f")), Some(&Action::PageForward));
        assert_eq!(m.get(&KeyCombo::ctrl_shift("M")), Some(&Action::OpenMediaPicker));
        assert_eq!(m.get(&KeyCombo::ctrl_alt("l")), Some(&Action::SaveAndQuit));
    }

    #[test]
    fn keymap_lookup_returns_action_for_bound_key() {
        let km = Keymap::default();
        assert_eq!(km.lookup("x", false, false, false), Some(Action::PageForward));
        assert_eq!(km.lookup("f", true, false, false), Some(Action::PageForward));
    }

    #[test]
    fn keymap_lookup_returns_none_for_unbound_key() {
        let km = Keymap::default();
        assert_eq!(km.lookup("zzz", false, false, false), None);
    }

    #[test]
    fn keymap_lookup_distinguishes_modifiers() {
        let km = Keymap::default();
        // "f" is bound to CycleFontForward; Ctrl+f to PageForward.
        let f_plain = km.lookup("f", false, false, false);
        let f_ctrl = km.lookup("f", true, false, false);
        assert_ne!(f_plain, f_ctrl);
        assert_eq!(f_plain, Some(Action::CycleFontForward));
        assert_eq!(f_ctrl, Some(Action::PageForward));
    }

    #[test]
    fn keymap_load_from_json_overrides_defaults() {
        let json = r#"{
            "reader": [
                {"key": "x", "action": "PageBackward"}
            ]
        }"#;
        let km = Keymap::from_json_str(json);
        // Override took effect:
        assert_eq!(km.lookup("x", false, false, false), Some(Action::PageBackward));
        // Other defaults preserved:
        assert_eq!(km.lookup("y", false, false, false), Some(Action::PageBackward));
        assert_eq!(km.lookup("j", false, false, false), Some(Action::CursorNextDialogue));
    }

    #[test]
    fn keymap_load_from_malformed_json_returns_defaults() {
        let bad_json = "not valid json {{{ ";
        let km = Keymap::from_json_str(bad_json);
        // Falls back to defaults entirely:
        assert_eq!(km.lookup("x", false, false, false), Some(Action::PageForward));
        assert_eq!(km.lookup("y", false, false, false), Some(Action::PageBackward));
    }

    #[test]
    fn keymap_load_skips_unknown_action() {
        let json = r#"{
            "reader": [
                {"key": "x", "action": "PageBackward"},
                {"key": "z", "action": "ThisActionDoesNotExist"}
            ]
        }"#;
        let km = Keymap::from_json_str(json);
        // Override succeeded for known action:
        assert_eq!(km.lookup("x", false, false, false), Some(Action::PageBackward));
        // Unknown action skipped silently:
        assert_eq!(km.lookup("z", false, false, false), None);
    }
}
```

- [ ] **Step 2: Run tests, verify they fail**

```bash
cd /home/mlj/utono/linux-lit && cargo test --lib input::keymap_config::tests 2>&1 | tail -10
```

Expected: compilation errors for missing `default_reader_bindings`, `Keymap`, `Keymap::default`, `Keymap::lookup`, `Keymap::from_json_str`.

- [ ] **Step 3: Implement `Keymap` and `default_reader_bindings`**

Append to `src/input/keymap_config.rs` (BEFORE the `#[cfg(test)] mod tests` block):

```rust
/// Reader-mode keybinds. Per-overlay keymaps are deferred to F1.
pub struct Keymap {
    pub reader: HashMap<KeyCombo, Action>,
}

#[derive(Deserialize)]
struct KeymapJson {
    #[serde(default)]
    reader: Vec<BindingJson>,
}

#[derive(Deserialize)]
struct BindingJson {
    key: String,
    #[serde(default)]
    ctrl: bool,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    alt: bool,
    action: String,
}

impl Keymap {
    /// Load keymap from `~/.config/linux-lit/keymap.json` if present, else
    /// return defaults. Malformed JSON logs a warning and falls back.
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(text) = std::fs::read_to_string(&path) {
            Self::from_json_str(&text)
        } else {
            Self::default()
        }
    }

    /// Parse keymap from a JSON string. Used by tests and load(). Malformed
    /// JSON returns defaults entirely; unknown action names are skipped with
    /// a logged warning.
    pub fn from_json_str(json: &str) -> Self {
        let parsed: KeymapJson = match serde_json::from_str(json) {
            Ok(p) => p,
            Err(e) => {
                crate::logging::log(&format!("keymap.json parse error: {}; using defaults", e));
                return Self::default();
            }
        };
        let mut km = Self::default();
        for b in parsed.reader {
            let action = match parse_action(&b.action) {
                Some(a) => a,
                None => {
                    crate::logging::log(&format!("keymap.json: unknown action '{}', skipping", b.action));
                    continue;
                }
            };
            let combo = KeyCombo {
                key: b.key,
                ctrl: b.ctrl,
                shift: b.shift,
                alt: b.alt,
            };
            km.reader.insert(combo, action);
        }
        km
    }

    pub fn lookup(&self, key: &str, ctrl: bool, shift: bool, alt: bool) -> Option<Action> {
        let combo = KeyCombo {
            key: key.to_string(),
            ctrl, shift, alt,
        };
        self.reader.get(&combo).copied()
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self { reader: default_reader_bindings() }
    }
}

fn config_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    std::path::PathBuf::from(home).join(".config/linux-lit/keymap.json")
}

fn parse_action(name: &str) -> Option<Action> {
    // serde_json round-trip via a single-element JSON value.
    let json = format!("\"{}\"", name);
    serde_json::from_str(&json).ok()
}

/// Compiled-in default reader bindings. Mirrors the inline match arms
/// currently in keymap.rs:1338-1741 (base-keys block) and the Ctrl+ /
/// Shift+ / Alt+ combo blocks.
pub fn default_reader_bindings() -> HashMap<KeyCombo, Action> {
    let mut m = HashMap::new();

    // Page navigation
    m.insert(KeyCombo::plain("x"), Action::PageForward);
    m.insert(KeyCombo::plain("y"), Action::PageBackward);
    m.insert(KeyCombo::plain("less"), Action::PageBackward);
    m.insert(KeyCombo::plain("space"), Action::PageForward);
    m.insert(KeyCombo::shift("space"), Action::PageBackward);
    m.insert(KeyCombo::ctrl("d"), Action::ToggleDebugLogging);
    m.insert(KeyCombo::ctrl("f"), Action::PageForward);
    m.insert(KeyCombo::ctrl("u"), Action::PageForward);
    m.insert(KeyCombo::ctrl("b"), Action::PageBackward);

    // Cursor / dialogue
    m.insert(KeyCombo::plain("j"), Action::CursorNextDialogue);
    m.insert(KeyCombo::plain("k"), Action::CursorPrevLine);
    m.insert(KeyCombo::plain("Q"), Action::CursorToPageBottom);
    m.insert(KeyCombo::plain("Up"), Action::JumpToPrevDialogue);
    m.insert(KeyCombo::shift("Up"), Action::PageBackwardBottom);
    m.insert(KeyCombo::plain("Down"), Action::JumpToNextDialogue);
    m.insert(KeyCombo::plain("comma"), Action::JumpToPrevDialogue);
    m.insert(KeyCombo::shift("comma"), Action::PageBackwardBottom);
    m.insert(KeyCombo::plain("q"), Action::JumpToNextDialogue);

    // Multi-key chord entry
    m.insert(KeyCombo::plain("g"), Action::PendingG);
    m.insert(KeyCombo::plain("G"), Action::JumpToEnd);

    // Chapter / scene
    m.insert(KeyCombo::plain("bracketleft"), Action::JumpToPrevChapter);
    m.insert(KeyCombo::plain("braceleft"), Action::JumpToNextChapter);
    m.insert(KeyCombo::plain("2"), Action::JumpToPrevScene);
    m.insert(KeyCombo::plain("3"), Action::JumpToNextScene);

    // Bookmarks
    m.insert(KeyCombo::plain("m"), Action::ToggleBookmark);
    m.insert(KeyCombo::plain("semicolon"), Action::NextBookmark);
    m.insert(KeyCombo::shift("semicolon"), Action::PrevBookmark);
    m.insert(KeyCombo::plain("colon"), Action::PrevBookmark);
    m.insert(KeyCombo::ctrl("m"), Action::OpenBookmarkPicker);

    // Pickers
    m.insert(KeyCombo::ctrl("p"), Action::OpenLibraryPicker);
    m.insert(KeyCombo::ctrl_shift("M"), Action::OpenMediaPicker);
    m.insert(KeyCombo::ctrl("backslash"), Action::OpenConcordancePicker);
    m.insert(KeyCombo::ctrl_shift("P"), Action::OpenConcordanceWordPicker);
    m.insert(KeyCombo::ctrl_alt("p"), Action::OpenConcordanceListPicker);
    m.insert(KeyCombo::ctrl("comma"), Action::OpenSettingsOverlay);
    m.insert(KeyCombo::ctrl("slash"), Action::OpenKeybindsOverlay);
    m.insert(KeyCombo::plain("slash"), Action::OpenSearch);

    // MPV / media
    m.insert(KeyCombo::plain("s"), Action::TogglePlaybackSync);
    m.insert(KeyCombo::plain("Tab"), Action::TogglePlayback);
    m.insert(KeyCombo::plain("o"), Action::SeekShortBackward);
    m.insert(KeyCombo::plain("e"), Action::SeekShortForward);
    m.insert(KeyCombo::plain("O"), Action::SeekLongBackward);
    m.insert(KeyCombo::plain("E"), Action::SeekLongForward);
    m.insert(KeyCombo::plain("Left"), Action::SeekBackward30);
    m.insert(KeyCombo::ctrl("Up"), Action::VolumeUp);
    m.insert(KeyCombo::ctrl("Down"), Action::VolumeDown);
    m.insert(KeyCombo::plain("plus"), Action::TogglePlaybackSpeed);

    // Vocab / glossing
    m.insert(KeyCombo::plain("h"), Action::ToggleVocabPopup);
    m.insert(KeyCombo::plain("backslash"), Action::VocabPopupNext);
    m.insert(KeyCombo::plain("numbersign"), Action::VocabPopupPrev);
    m.insert(KeyCombo::plain("r"), Action::JumpToNextVocab);
    m.insert(KeyCombo::plain("R"), Action::JumpToPrevVocab);
    m.insert(KeyCombo::alt("backslash"), Action::ToggleVocabHighlight);

    // Visual / selection
    m.insert(KeyCombo::plain("V"), Action::EnterVisualMode);
    m.insert(KeyCombo::plain("w"), Action::WordCycleCopy);
    m.insert(KeyCombo::plain("W"), Action::WordCollectCopy);

    // Translations
    m.insert(KeyCombo::plain("i"), Action::ToggleTranslations);

    // Settings (in reader)
    m.insert(KeyCombo::plain("exclam"), Action::AdjustFontSizeDown);
    m.insert(KeyCombo::plain("bar"), Action::AdjustFontSizeUp);
    m.insert(KeyCombo::plain("0"), Action::ResetFontSize);
    m.insert(KeyCombo::plain("f"), Action::CycleFontForward);
    m.insert(KeyCombo::plain("F"), Action::CycleFontBackward);
    m.insert(KeyCombo::plain("l"), Action::ToggleSignColumn);
    m.insert(KeyCombo::plain("minus"), Action::ToggleCursorLine);
    m.insert(KeyCombo::alt("d"), Action::ToggleDim);
    m.insert(KeyCombo::alt("f"), Action::ShowFontInfo);

    // Timestamps
    m.insert(KeyCombo::plain("u"), Action::SetStartTime);
    m.insert(KeyCombo::plain("Right"), Action::SetStartTime);
    m.insert(KeyCombo::alt("i"), Action::SetEndTime);
    m.insert(KeyCombo::plain("period"), Action::SetChapter);
    m.insert(KeyCombo::plain("BackSpace"), Action::DeleteTimestamp);
    m.insert(KeyCombo::plain("p"), Action::NudgeStartBackward);
    m.insert(KeyCombo::plain("P"), Action::NudgeStartForward);
    m.insert(KeyCombo::plain("U"), Action::UndoTimestamp);
    m.insert(KeyCombo::plain("a"), Action::PlayCurrentLine);

    // App
    m.insert(KeyCombo::ctrl_alt("l"), Action::SaveAndQuit);
    m.insert(KeyCombo::ctrl("y"), Action::CopyLineMappingId);

    // Search (in reader)
    m.insert(KeyCombo::plain("n"), Action::SearchNextMatch);
    m.insert(KeyCombo::plain("N"), Action::SearchPrevMatch);

    m
}
```

- [ ] **Step 4: Run tests, verify they pass**

```bash
cd /home/mlj/utono/linux-lit && cargo test --lib input::keymap_config 2>&1 | tail -10
```

Expected: 8 tests pass.

If a test fails: most likely cause is a missing entry in `default_reader_bindings()` for the key the test asserts. Check the test's assertion against the binding list.

- [ ] **Step 5: Run all tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -3
```

Expected: 126 pass / 1 pre-existing fail (118 before + 8 new).

- [ ] **Step 6: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/keymap_config.rs && git commit -m "$(cat <<'EOF'
Implement Keymap loader + default_reader_bindings (no callers yet)

Phase 2.2 of F4+F2 keymap refactor. Implements Keymap struct with:
- default_reader_bindings(): ~70 hard-coded reader bindings matching
  current keymap.rs match-arm semantics.
- Keymap::load(): reads ~/.config/linux-lit/keymap.json, falls back to
  defaults on missing or malformed JSON.
- Keymap::from_json_str(): pure parsing for tests.
- Keymap::lookup(): (key, ctrl, shift, alt) -> Option<Action>.

Unknown action names in JSON are skipped with a logged warning.
Malformed JSON falls back entirely to defaults.

8 unit tests cover defaults, lookup, modifier disambiguation, JSON
override, malformed JSON, and unknown action handling. 126 pass / 1
pre-existing fail.

Currently unused; Tasks 2.3-2.5 wire up AppState integration and
dispatch.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2.3: Add `keymap` field to AppState

**Files:**
- Modify: `src/app.rs` — add field, init in constructor.

- [ ] **Step 1: Add field to AppState struct**

Find the AppState struct:

```bash
cd /home/mlj/utono/linux-lit && grep -n "^pub struct AppState\|page_tops:" src/app.rs | head -5
```

Add the field near `page_tops` (any spot in the struct works; near other input-related fields is cleanest):

```rust
    /// Loaded keybinds. Compiled-in defaults overridden by
    /// ~/.config/linux-lit/keymap.json if present.
    pub keymap: crate::input::keymap_config::Keymap,
```

- [ ] **Step 2: Init in AppState constructor**

Find the AppState constructor (search for `keymap: Keymap` won't match; search for `page_tops: std::cell::RefCell::new`):

```bash
cd /home/mlj/utono/linux-lit && grep -n "page_tops: std::cell::RefCell::new" src/app.rs
```

Add immediately after that line:

```rust
        keymap: crate::input::keymap_config::Keymap::load(),
```

- [ ] **Step 3: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -5
```

Expected: clean. The `Keymap` struct is Send + Sync, public, and Default — should compose cleanly.

If "field not initialized": the constructor has multiple sites or you missed one — find with `grep -n "AppState {" src/app.rs`.

- [ ] **Step 4: Run tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result" | tail -2
```

Expected: 126 / 1 pre-existing fail.

- [ ] **Step 5: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/app.rs && git commit -m "$(cat <<'EOF'
Add Keymap field to AppState; load from ~/.config/linux-lit/keymap.json

Phase 2.3 of F4+F2 keymap refactor. AppState now carries a Keymap loaded
at construction. Falls back to compiled-in defaults if the JSON file is
absent or malformed.

Currently the field is unused (no dispatcher reads from it yet); Tasks
2.4-2.5 wire up dispatch_action and migrate the base-key match block.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2.4: Write `dispatch_action` table in keymap.rs

**Files:**
- Modify: `src/input/keymap.rs` — add `dispatch_action` function.

- [ ] **Step 1: Find the right spot to add `dispatch_action`**

It goes at the end of `keymap.rs` after `handle_key`. Find the end of `handle_key`:

```bash
cd /home/mlj/utono/linux-lit && grep -n "^pub fn handle_key\|^const CHUNK_PREROLL" src/input/keymap.rs
```

`handle_key` ends just before `CHUNK_PREROLL`. Add `dispatch_action` between them.

- [ ] **Step 2: Implement `dispatch_action`**

Append before `const CHUNK_PREROLL`:

```rust
/// Execute an Action by calling its corresponding verb. Returns true if the
/// action was dispatched (currently always true for known actions; false
/// branch reserved for future "action exists but precondition failed" cases).
fn dispatch_action(
    state: &Rc<RefCell<AppState>>,
    action: crate::input::actions::Action,
    key_state: &Rc<RefCell<KeyState>>,
    tokio_handle: &tokio::runtime::Handle,
) -> bool {
    use crate::input::actions::Action::*;
    match action {
        // Page navigation
        PageForward => { navigation::page_forward(&mut state.borrow_mut()); true }
        PageBackward => { navigation::page_backward(&mut state.borrow_mut()); true }
        PageBackwardBottom => { navigation::page_backward_bottom(&mut state.borrow_mut()); true }
        JumpToStart => { navigation::jump_to_start(&mut state.borrow_mut()); true }
        JumpToEnd => { navigation::jump_to_end(&mut state.borrow_mut()); true }

        // Cursor / dialogue
        CursorNextDialogue => { navigation::cursor_next_dialogue(&mut state.borrow_mut()); true }
        CursorPrevLine => { navigation::cursor_prev_line(&mut state.borrow_mut()); true }
        CursorToPageBottom => { navigation::cursor_to_page_bottom(&mut state.borrow_mut()); true }
        JumpToNextDialogue => { navigation::jump_to_next_dialogue(&mut state.borrow_mut()); true }
        JumpToPrevDialogue => { navigation::jump_to_prev_dialogue(&mut state.borrow_mut()); true }
        JumpToNextChapter => {
            let mut s = state.borrow_mut();
            if s.translations_visible {
                crate::app::toggle_translations(&mut s);
            }
            navigation::jump_to_next_chapter(&mut s);
            true
        }
        JumpToPrevChapter => {
            let mut s = state.borrow_mut();
            if s.translations_visible {
                crate::app::toggle_translations(&mut s);
            }
            navigation::jump_to_prev_chapter(&mut s);
            true
        }
        JumpToNextScene => {
            let mut s = state.borrow_mut();
            let is_play = s.current_work.as_ref().map(|w| w.work_type == "play").unwrap_or(false);
            if is_play {
                navigation::jump_to_next_scene(&mut s);
            } else {
                navigation::jump_to_next_chapter(&mut s);
            }
            true
        }
        JumpToPrevScene => {
            let mut s = state.borrow_mut();
            let is_play = s.current_work.as_ref().map(|w| w.work_type == "play").unwrap_or(false);
            if is_play {
                navigation::jump_to_prev_scene(&mut s);
            } else {
                navigation::jump_to_prev_chapter(&mut s);
            }
            true
        }

        // Bookmarks
        ToggleBookmark => { crate::input::actions::bookmarks::toggle_bookmark(state, tokio_handle); true }
        NextBookmark => { navigation::next_bookmark(&mut state.borrow_mut()); true }
        PrevBookmark => { navigation::prev_bookmark(&mut state.borrow_mut()); true }
        JumpToRecentBookmark => { crate::input::actions::bookmarks::jump_to_recent_bookmark(state, tokio_handle); true }
        OpenBookmarkPicker => { crate::input::actions::pickers::open_bookmark_picker(state, tokio_handle); true }

        // Pickers / overlays
        OpenLibraryPicker => {
            let s = state.borrow();
            if !s.bookmark_picker.is_visible() && !s.media_picker.is_visible() {
                drop(s);
                let mut sm = state.borrow_mut();
                sm.concordance_state = None;
                sm.concordance_bar.hide();
                drop(sm);
                state.borrow().correction_overlay.hide();
                state.borrow_mut().picker.show_prepare();
                state.borrow().picker.show_finish();
            }
            true
        }
        OpenMediaPicker => { crate::input::actions::pickers::open_media_picker(state, tokio_handle); true }
        OpenConcordancePicker => { crate::input::actions::concordance::open_picker(state, tokio_handle); true }
        OpenConcordanceWordPicker => {
            let words: Vec<(String, usize)> = {
                let s = state.borrow();
                let mut seen = std::collections::BTreeSet::new();
                for m in &s.vocab_matches {
                    seen.insert(m.word.clone());
                }
                seen.into_iter().map(|w| (w, 0)).collect()
            };
            state.borrow_mut().concordance_word_picker.set_words(words);
            state.borrow().concordance_word_picker.show();
            true
        }
        OpenConcordanceListPicker => {
            let s = state.borrow();
            if let Some(conc) = &s.concordance_state {
                s.concordance_list_picker.show(&conc.occurrences, conc.current_index);
            }
            true
        }
        OpenSettingsOverlay => {
            let s = state.borrow();
            if !s.settings_overlay.is_visible() && !s.picker.is_visible() {
                s.correction_overlay.hide();
                let ls = s.config.line_spacing;
                let cw = s.config.column_width;
                let tm = s.config.text_margins;
                let nm = s.config.navigation_mode;
                let ts = s.config.transition_style;
                let cl = s.config.show_cursor_line;
                drop(s);
                state.borrow_mut().settings_overlay.show(ls, cw, tm, nm, ts, cl);
            }
            true
        }
        OpenKeybindsOverlay => {
            let s = state.borrow();
            if s.keybinds_overlay.is_visible() || s.gamepad_overlay.is_visible() {
                s.keybinds_overlay.hide();
                s.gamepad_overlay.hide();
            } else {
                s.picker.hide();
                s.media_picker.hide();
                s.settings_overlay.hide();
                s.search_bar.hide();
                s.correction_overlay.hide();
                s.keybinds_overlay.show();
            }
            key_state.borrow_mut().pending_ctrl_slash = true;
            let ks = Rc::clone(key_state);
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                ks.borrow_mut().pending_ctrl_slash = false;
            });
            true
        }
        OpenSearch => {
            let mut s = state.borrow_mut();
            crate::input::search::clear_search(&mut s);
            s.search_bar.show();
            true
        }

        // MPV / media
        TogglePlaybackSync => {
            let mut s = state.borrow_mut();
            s.sync_enabled = !s.sync_enabled;
            s.sync_icon.set_visible(!s.sync_enabled);
            crate::logging::log(&format!("SYNC: {}", if s.sync_enabled { "enabled" } else { "disabled" }));
            true
        }
        TogglePlayback => { crate::input::search::toggle_playback(&mut state.borrow_mut()); true }
        SeekShortBackward => { do_mpv_seek(state, -3.5); true }
        SeekShortForward => { do_mpv_seek(state, 3.5); true }
        SeekLongBackward => { do_mpv_seek(state, -60.0); true }
        SeekLongForward => { do_mpv_seek(state, 60.0); true }
        SeekBackward30 => { do_mpv_seek(state, -30.0); true }
        VolumeUp => { let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(5.0)); true }
        VolumeDown => { let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::VolumeAdjust(-5.0)); true }
        TogglePlaybackSpeed => {
            let mut s = state.borrow_mut();
            let new_speed = if s.playback_speed == 1.0 { 1.3 } else { 1.0 };
            s.playback_speed = new_speed;
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SetSpeed(new_speed));
            crate::logging::log(&format!("SPEED: toggled to {}x", new_speed));
            true
        }

        // Vocab / glossing — these have complex preconditions; keep using
        // the existing handlers. dispatch only triggers the entry point;
        // fall-through to existing inline logic where needed.
        ToggleVocabPopup => {
            let mut s = state.borrow_mut();
            s.vocab_popup_auto = !s.vocab_popup_auto;
            if s.vocab_popup_auto {
                crate::app::open_vocab_popup(&mut s);
            } else {
                crate::app::close_vocab_popup(&mut s);
            }
            true
        }
        VocabPopupNext => {
            // Same inline logic as the original "backslash" / "numbersign"
            // arms; the auto-hide timer handling is preserved.
            handle_vocab_popup_key(state, true);
            true
        }
        VocabPopupPrev => {
            handle_vocab_popup_key(state, false);
            true
        }
        JumpToNextVocab => {
            let has_concordance = state.borrow().concordance_state.is_some();
            if has_concordance {
                let current_abbrev = state.borrow().current_work.as_ref().map(|w| w.abbrev.clone());
                let advanced = {
                    let mut s = state.borrow_mut();
                    if let (Some(conc), Some(ref abbrev)) = (s.concordance_state.as_mut(), &current_abbrev) {
                        conc.advance_within_work(abbrev)
                    } else { false }
                };
                if advanced {
                    navigation::concordance_jump_to_current(state, tokio_handle);
                }
            } else {
                navigation::jump_to_next_vocab(&mut state.borrow_mut());
            }
            true
        }
        JumpToPrevVocab => {
            let has_concordance = state.borrow().concordance_state.is_some();
            if has_concordance {
                let current_abbrev = state.borrow().current_work.as_ref().map(|w| w.abbrev.clone());
                let retreated = {
                    let mut s = state.borrow_mut();
                    if let (Some(conc), Some(ref abbrev)) = (s.concordance_state.as_mut(), &current_abbrev) {
                        conc.retreat_within_work(abbrev)
                    } else { false }
                };
                if retreated {
                    navigation::concordance_jump_to_current(state, tokio_handle);
                }
            } else {
                navigation::jump_to_prev_vocab(&mut state.borrow_mut());
            }
            true
        }
        ToggleVocabHighlight => {
            let mut s = state.borrow_mut();
            s.vocab_highlight_visible = !s.vocab_highlight_visible;
            if s.vocab_highlight_visible {
                crate::app::apply_vocab_highlighting(&s);
            } else {
                crate::app::remove_vocab_highlighting(&s);
            }
            s.config.vocab_highlight_visible = s.vocab_highlight_visible;
            crate::config::save(&s.config);
            crate::logging::log(&format!("VOCAB: highlighting {}", if s.vocab_highlight_visible { "on" } else { "off" }));
            true
        }

        // Visual / selection
        EnterVisualMode => { crate::input::visual::enter_visual_mode(&mut state.borrow_mut()); true }
        WordCycleCopy => { navigation::word_cycle_copy(&mut state.borrow_mut()); true }
        WordCollectCopy => { navigation::word_collect_copy(&mut state.borrow_mut()); true }

        // Translations
        ToggleTranslations => {
            let s = state.borrow();
            let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
            drop(s);
            crate::app::toggle_translations(&mut state.borrow_mut());
            true
        }

        // Settings (in reader)
        AdjustFontSizeUp => { crate::app::adjust_font_size(&mut state.borrow_mut(), 1); crate::app::show_font_info(&state.borrow()); true }
        AdjustFontSizeDown => { crate::app::adjust_font_size(&mut state.borrow_mut(), -1); crate::app::show_font_info(&state.borrow()); true }
        ResetFontSize => { crate::app::reset_font_size(&mut state.borrow_mut()); true }
        CycleFontForward => { crate::app::cycle_font(&mut state.borrow_mut(), true); true }
        CycleFontBackward => { crate::app::cycle_font(&mut state.borrow_mut(), false); true }
        ToggleSignColumn => { crate::app::toggle_sign_column(&mut state.borrow_mut()); true }
        ToggleCursorLine => {
            let mut s = state.borrow_mut();
            s.config.show_cursor_line = !s.config.show_cursor_line;
            crate::input::navigation::update_highlight_only(&mut s);
            crate::config::save(&s.config);
            true
        }
        ToggleDim => {
            let mut s = state.borrow_mut();
            s.dim_enabled = !s.dim_enabled;
            if !s.dim_enabled {
                let (start, end) = s.buffer.bounds();
                s.buffer.remove_tag(&s.dim_tag, &start, &end);
            }
            navigation::update_highlight_only(&mut s);
            s.config.dim_enabled = s.dim_enabled;
            crate::config::save(&s.config);
            crate::logging::log(&format!("DIM: {}", if s.dim_enabled { "on" } else { "off" }));
            true
        }
        ShowFontInfo => { crate::app::show_font_info(&state.borrow()); true }

        // Timestamps
        SetStartTime => {
            let ok = crate::input::timestamps::set_start_time(&mut state.borrow_mut());
            if ok {
                navigation::cursor_next_dialogue(&mut state.borrow_mut());
            }
            ok
        }
        SetEndTime => crate::input::timestamps::set_end_time(&mut state.borrow_mut()),
        SetChapter => crate::input::timestamps::set_chapter(&mut state.borrow_mut()),
        DeleteTimestamp => crate::input::timestamps::delete_timestamp(&mut state.borrow_mut()),
        NudgeStartBackward => crate::input::timestamps::nudge_start_backward(&mut state.borrow_mut()),
        NudgeStartForward => crate::input::timestamps::nudge_start_forward(&mut state.borrow_mut()),
        UndoTimestamp => crate::input::timestamps::undo_timestamp(&mut state.borrow_mut()),
        PlayCurrentLine => { crate::input::timestamps::play_current_line(&mut state.borrow_mut()); true }

        // App
        SaveAndQuit => {
            crate::app::save_position(&mut state.borrow_mut());
            let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::Quit);
            state.borrow().window.close();
            true
        }
        ToggleDebugLogging => {
            let enabled = !crate::logging::debug_mode();
            crate::logging::set_debug_mode(enabled);
            crate::logging::log_always(&format!("DEBUG_MODE: {}", if enabled { "on" } else { "off" }));
            state.borrow().debug_icon.set_visible(enabled);
            true
        }
        CopyLineMappingId => {
            let s = state.borrow();
            let lm_id = s.line_mapping_id_for_buffer(s.current_line);
            let media_id = s.media_id;
            drop(s);
            let clip = match (lm_id, media_id) {
                (Some(l), Some(m)) => format!("{} {}", l, m),
                (Some(l), None) => format!("{}", l),
                (None, Some(m)) => format!("- {}", m),
                (None, None) => return true,
            };
            if let Ok(mut child) = std::process::Command::new("wl-copy")
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    let _ = stdin.write_all(clip.as_bytes());
                }
                let _ = child.wait();
            }
            crate::logging::log(&format!("CLIPBOARD: copied {}", clip));
            true
        }

        // Multi-key chord entry
        PendingG => {
            key_state.borrow_mut().pending_g = true;
            let ks = Rc::clone(key_state);
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                ks.borrow_mut().pending_g = false;
            });
            true
        }

        // Search (in reader, when matches present)
        SearchNextMatch => {
            if !state.borrow().search_matches.is_empty() {
                crate::input::search::next_match(&mut state.borrow_mut());
                true
            } else {
                false
            }
        }
        SearchPrevMatch => {
            if !state.borrow().search_matches.is_empty() {
                crate::input::search::prev_match(&mut state.borrow_mut());
                true
            } else {
                false
            }
        }
    }
}

/// MPV seek with brief sync suppression. Common pattern for o/e/O/E/Left.
fn do_mpv_seek(state: &Rc<RefCell<AppState>>, offset: f64) {
    let mut s = state.borrow_mut();
    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::SeekRelative(offset));
    s.suppress_sync_until = Some(
        std::time::Instant::now() + std::time::Duration::from_millis(500),
    );
}

/// Vocab popup key handler with auto-hide timer reset.
fn handle_vocab_popup_key(state: &Rc<RefCell<AppState>>, forward: bool) {
    use libadwaita::prelude::AnimationExt;
    let popup_visible = state.borrow().vocab_popup.is_visible();
    if popup_visible {
        if forward {
            crate::app::vocab_popup_next(&mut state.borrow_mut());
        } else {
            crate::app::vocab_popup_prev(&mut state.borrow_mut());
        }
    } else {
        crate::app::open_vocab_popup(&mut state.borrow_mut());
    }
    let gen = {
        let s = state.borrow();
        let next = s.vocab_popup_fade_gen.get() + 1;
        s.vocab_popup_fade_gen.set(next);
        next
    };
    let state_clone = Rc::clone(state);
    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
        let s = state_clone.borrow();
        if s.vocab_popup_fade_gen.get() != gen {
            return;
        }
        if !s.vocab_popup.is_visible() {
            return;
        }
        let widget = s.vocab_popup.widget().clone();
        let target = adw::CallbackAnimationTarget::new(move |value| {
            widget.set_opacity(value as f64);
            if value <= 0.0 {
                widget.set_visible(false);
                widget.set_opacity(1.0);
            }
        });
        let anim = adw::TimedAnimation::new(
            s.vocab_popup.widget(),
            1.0, 0.0, 500, target,
        );
        anim.set_easing(adw::Easing::EaseOutQuad);
        anim.play();
    });
}
```

- [ ] **Step 3: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: clean. `dispatch_action`, `do_mpv_seek`, `handle_vocab_popup_key` warn dead_code (no callers yet — Task 2.5 wires them up).

If "no method `vocab_popup_fade_gen`": check the field name in AppState — it might be slightly different. Grep `vocab_popup_fade` to find the actual field.

If "trait `WidgetExt` not in scope": already handled by `gtk4::prelude::*` at top of file; if not, add `use gtk4::prelude::*;` near `dispatch_action`.

- [ ] **Step 4: Run tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result" | tail -2
```

Expected: 126 / 1.

- [ ] **Step 5: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/keymap.rs && git commit -m "$(cat <<'EOF'
Add dispatch_action table mapping Action enum to verb calls

Phase 2.4 of F4+F2 keymap refactor. Adds dispatch_action(state, action,
key_state, tokio_handle) -> bool — one canonical table mapping each
Action variant to its corresponding verb. Includes do_mpv_seek and
handle_vocab_popup_key helper fns to dedupe common patterns previously
inlined per match arm.

OpenLibraryPicker and OpenSettingsOverlay preserve their visibility-
guard preconditions (won't open when overlapping pickers are visible).
SearchNextMatch/SearchPrevMatch return false when no matches exist
(matches existing behavior).

dispatch_action is currently unused; Task 2.5 wires it into handle_key
after migrating the base-key match block to keymap.lookup().

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2.5: Migrate base-key match block to keymap lookup

**Files:**
- Modify: `src/input/keymap.rs` — wire dispatch_action; delete migrated match arms.

This is the load-bearing task: replace ~400 lines of inlined match arms with a 5-line lookup + dispatch.

- [ ] **Step 1: Identify the base-key match block**

Find it:

```bash
cd /home/mlj/utono/linux-lit && grep -n "// Single keys\|fn dispatch_action" src/input/keymap.rs
```

Expected: `// Single keys` comment around line 1338, marking the start of the `match key_name { ... }` block. The block ends at the closing `}` before `dispatch_action`. Also covers the `if is_alt { match key_name { ... } }` block (~line 1173) and the `if is_ctrl { match key_name { ... } }` block (~line 1201).

The migration replaces all THREE blocks (`is_ctrl`, `is_alt`, base) with a single keymap lookup at the bottom of `handle_key`.

Caveats — these blocks contain THREE behaviors that don't map cleanly to dispatch_action:
1. **Settings open guard** (the `!settings_visible && !picker_visible` check at line 685 for Ctrl+,). Already absorbed into `OpenSettingsOverlay`'s arm in dispatch_action.
2. **Library picker open guard** (the `!picker_visible && !state.borrow().bookmark_picker.is_visible() && !state.borrow().media_picker.is_visible()` check at line 213 for Ctrl+p). Already absorbed into `OpenLibraryPicker`'s arm.
3. **`Escape` handler** (concordance + AB loop + search-clear logic, ~30 lines starting at line 1694). Stays inline — not in F2 scope.

- [ ] **Step 2: Replace the three match blocks with a keymap lookup**

The end of `handle_key` currently has (in order):
1. `if is_ctrl && is_alt && key_name == "l"` — Ctrl+Alt+l save and quit (line 1151).
2. `if is_alt && key_name == "backslash"` — Alt+\ vocab highlight toggle (line 1159).
3. `if is_alt { match key_name { "d" | "f" | "i" => ... } }` — Alt combos (line 1173).
4. `if is_ctrl { match key_name { ... } }` — Ctrl combos including page nav, slash, backslash, d/f/u/b, Up/Down (line 1201).
5. Vocab popup keys (`backslash`, `numbersign`) — line 1276.
6. Vocab popup other keys (when popup visible) — line 1323.
7. `// Single keys` block — line 1338, the big match.

**The migration strategy:** Delete blocks 1-4 entirely (their bindings are in `default_reader_bindings`). Keep blocks 5-7 mostly intact since blocks 5-6 have complex precondition logic (vocab popup fade-timer, popup-visible context-sensitive `g`/`Tab`) that doesn't map to dispatch_action's "static action" model. But blocks 5-6 ARE handled by dispatch_action's `VocabPopupNext`/`VocabPopupPrev` arms; the lookup can subsume them.

Actually, simpler: delete ALL match blocks (1-4 and the base block 7). Keep only:
- The `Escape` arm (special multi-state logic).
- The `Ctrl+p` library picker open (already absorbed into dispatch but its existing arm has more guard logic).

**Concrete surgery:**

Wait — I'm going to keep this surgically simple. Replace the entire block from line 1151 (start of Ctrl+Alt+l) through the `_ => false` at the end of the base match (~line 1740) with a single keymap lookup + dispatch + Escape fallback:

Find the exact start with:

```bash
cd /home/mlj/utono/linux-lit && sed -n '1145,1155p' src/input/keymap.rs
```

Find the exact end with:

```bash
cd /home/mlj/utono/linux-lit && grep -n "^        _ => false," src/input/keymap.rs | tail -5
```

The base-keys match block ends with `_ => false,\n    }\n}` — the final `}` of `handle_key`. Replace EVERYTHING between the comment `// Ctrl+Alt+l: save position, quit MPV, and close window` and the final `_ => false,` with:

```rust
    // Escape: special multi-state handler (concordance, AB loop, search clear).
    // Stays inline because the precondition logic depends on multiple AppState
    // fields that don't fit the static Action model.
    if key_name == "Escape" {
        // ... (paste the existing Escape handler body here, lines ~1694-1727)
    }

    // Vocab popup `g` (toggle definition view) and `Tab` (toggle playback)
    // are popup-visible-context-sensitive — handled inline.
    if state.borrow().vocab_popup.is_visible() {
        match key_name {
            "g" => {
                crate::app::vocab_popup_toggle_view(&mut state.borrow_mut());
                return true;
            }
            "Tab" => {
                crate::input::search::toggle_playback(&mut state.borrow_mut());
                return true;
            }
            _ => {}
        }
    }

    // Keymap-driven dispatch for everything else.
    if let Some(action) = state.borrow().keymap.lookup(key_name, is_ctrl, is_shift, is_alt) {
        return dispatch_action(state, action, key_state, tokio_handle);
    }

    false
}
```

The Escape handler body needs to be copied verbatim from the existing block:

```bash
cd /home/mlj/utono/linux-lit && sed -n '1694,1727p' src/input/keymap.rs
```

Paste that body inside the new `if key_name == "Escape" { ... }` block.

**This is a large, mechanically careful edit.** Take it slow. Verify after each sub-edit that `cargo build` still succeeds.

- [ ] **Step 3: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -15
```

Expected: clean. The `dispatch_action`, `do_mpv_seek`, `handle_vocab_popup_key` dead_code warnings should clear (now called from the keymap lookup path).

If "use of moved value" or "borrow of moved value": probably the Escape handler's `state.borrow_mut()` is conflicting with something. Compare line-by-line with the original.

- [ ] **Step 4: Run tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -3
```

Expected: 126 / 1 pre-existing fail.

- [ ] **Step 5: Manual verification (SECOND GATE)**

Paste the Manual Verification Protocol (top of plan, including steps 10-11 about JSON override and malformed JSON). Stop and wait for the user.

Critical things to test specifically for Phase 2:
- Every key in the protocol should still work (page nav, bookmarks, pickers, settings, MPV, vocab).
- `Escape` still clears concordance / AB loop / search.
- Vocab popup `g` (definition toggle) still works.
- Step 10: edit `~/.config/linux-lit/keymap.json` to remap `x` to `PageBackward`; restart; confirm `x` now pages backward.
- Step 11: introduce a syntax error in `keymap.json`; restart; confirm linux-lit logs the warning and falls back to defaults.

If user reports a regression: most likely one of three things:
- A keybind missing from `default_reader_bindings()` — grep the protocol's keys against the defaults.
- A precondition guard not preserved in dispatch_action (e.g., the `Ctrl+p` open guard).
- The Escape inline block's behavior diverged from the original.

- [ ] **Step 6: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/keymap.rs && git commit -m "$(cat <<'EOF'
Migrate base-key match arms to keymap.lookup() + dispatch_action

Phase 2.5 of F4+F2 keymap refactor. Replaces ~400 lines of inlined
match arms (the // Single keys block, the if is_ctrl { match }, the
if is_alt { match }, plus Ctrl+Alt+l) with a single keymap lookup
at the bottom of handle_key. Each key event consults
state.keymap.lookup(...) for an Action, then dispatches via
dispatch_action.

Escape stays inline — its multi-state precondition logic (concordance
mode, AB loop, search matches) doesn't fit the static Action model.

Vocab popup `g` (definition toggle) and `Tab` (playback toggle) stay
inline — popup-visible-context-sensitive overrides.

keymap.rs net: ~400 lines removed. Phase 2 (F2) complete.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2.6: Create stow package + document workflow

**Files:**
- Create: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`
- Modify: `~/utono/linux-lit/CLAUDE.md` — document workflow.

- [ ] **Step 1: Generate the canonical default JSON**

Use the new `default_reader_bindings()` to produce the JSON. Write a tiny binary or use cargo to dump it; simplest is to write it by hand from the bindings list in Task 2.2. Or add a temporary debug command:

```bash
cd /home/mlj/utono/linux-lit && cat > /tmp/dump_keymap.rs <<'EOF'
fn main() {
    let bindings = linux_lit::input::keymap_config::default_reader_bindings();
    let mut entries: Vec<_> = bindings.iter().collect();
    entries.sort_by_key(|(k, _)| (k.key.clone(), k.ctrl, k.shift, k.alt));
    let json_entries: Vec<serde_json::Value> = entries.iter().map(|(k, v)| {
        let mut obj = serde_json::json!({"key": k.key, "action": format!("{:?}", v)});
        if k.ctrl { obj["ctrl"] = serde_json::json!(true); }
        if k.shift { obj["shift"] = serde_json::json!(true); }
        if k.alt { obj["alt"] = serde_json::json!(true); }
        obj
    }).collect();
    let out = serde_json::json!({"reader": json_entries});
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
EOF
```

Adding a binary target is overkill. Instead, just write the JSON file by hand from the keymap_config.rs source.

Create the directory:

```bash
mkdir -p ~/tty-dotfiles/linux-lit/.config/linux-lit
```

Write `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` with one entry per binding from `default_reader_bindings()`. Format as a JSON object with a `reader` array. Each entry: `{"key": "...", "action": "..."}` plus optional `"ctrl": true`, `"shift": true`, `"alt": true`.

The complete file (matches `default_reader_bindings()` exactly — alphabetize by key for ease of editing):

```json
{
  "reader": [
    {"key": "0", "action": "ResetFontSize"},
    {"key": "2", "action": "JumpToPrevScene"},
    {"key": "3", "action": "JumpToNextScene"},
    {"key": "BackSpace", "action": "DeleteTimestamp"},
    {"key": "Down", "action": "JumpToNextDialogue"},
    {"key": "Down", "ctrl": true, "action": "VolumeDown"},
    {"key": "E", "action": "SeekLongForward"},
    {"key": "F", "action": "CycleFontBackward"},
    {"key": "G", "action": "JumpToEnd"},
    {"key": "Left", "action": "SeekBackward30"},
    {"key": "M", "ctrl": true, "shift": true, "action": "OpenMediaPicker"},
    {"key": "N", "action": "SearchPrevMatch"},
    {"key": "O", "action": "SeekLongBackward"},
    {"key": "P", "action": "NudgeStartForward"},
    {"key": "P", "ctrl": true, "shift": true, "action": "OpenConcordanceWordPicker"},
    {"key": "Q", "action": "CursorToPageBottom"},
    {"key": "R", "action": "JumpToPrevVocab"},
    {"key": "Right", "action": "SetStartTime"},
    {"key": "Tab", "action": "TogglePlayback"},
    {"key": "U", "action": "UndoTimestamp"},
    {"key": "Up", "action": "JumpToPrevDialogue"},
    {"key": "Up", "ctrl": true, "action": "VolumeUp"},
    {"key": "Up", "shift": true, "action": "PageBackwardBottom"},
    {"key": "V", "action": "EnterVisualMode"},
    {"key": "W", "action": "WordCollectCopy"},
    {"key": "a", "action": "PlayCurrentLine"},
    {"key": "b", "ctrl": true, "action": "PageBackward"},
    {"key": "backslash", "action": "VocabPopupNext"},
    {"key": "backslash", "alt": true, "action": "ToggleVocabHighlight"},
    {"key": "backslash", "ctrl": true, "action": "OpenConcordancePicker"},
    {"key": "bar", "action": "AdjustFontSizeUp"},
    {"key": "bracketleft", "action": "JumpToPrevChapter"},
    {"key": "braceleft", "action": "JumpToNextChapter"},
    {"key": "colon", "action": "PrevBookmark"},
    {"key": "comma", "action": "JumpToPrevDialogue"},
    {"key": "comma", "ctrl": true, "action": "OpenSettingsOverlay"},
    {"key": "comma", "shift": true, "action": "PageBackwardBottom"},
    {"key": "d", "alt": true, "action": "ToggleDim"},
    {"key": "d", "ctrl": true, "action": "ToggleDebugLogging"},
    {"key": "e", "action": "SeekShortForward"},
    {"key": "exclam", "action": "AdjustFontSizeDown"},
    {"key": "f", "action": "CycleFontForward"},
    {"key": "f", "alt": true, "action": "ShowFontInfo"},
    {"key": "f", "ctrl": true, "action": "PageForward"},
    {"key": "g", "action": "PendingG"},
    {"key": "h", "action": "ToggleVocabPopup"},
    {"key": "i", "action": "ToggleTranslations"},
    {"key": "i", "alt": true, "action": "SetEndTime"},
    {"key": "j", "action": "CursorNextDialogue"},
    {"key": "k", "action": "CursorPrevLine"},
    {"key": "l", "action": "ToggleSignColumn"},
    {"key": "l", "ctrl": true, "alt": true, "action": "SaveAndQuit"},
    {"key": "less", "action": "PageBackward"},
    {"key": "m", "action": "ToggleBookmark"},
    {"key": "m", "ctrl": true, "action": "OpenBookmarkPicker"},
    {"key": "minus", "action": "ToggleCursorLine"},
    {"key": "n", "action": "SearchNextMatch"},
    {"key": "numbersign", "action": "VocabPopupPrev"},
    {"key": "o", "action": "SeekShortBackward"},
    {"key": "p", "action": "NudgeStartBackward"},
    {"key": "p", "ctrl": true, "action": "OpenLibraryPicker"},
    {"key": "p", "ctrl": true, "alt": true, "action": "OpenConcordanceListPicker"},
    {"key": "period", "action": "SetChapter"},
    {"key": "plus", "action": "TogglePlaybackSpeed"},
    {"key": "q", "action": "JumpToNextDialogue"},
    {"key": "r", "action": "JumpToNextVocab"},
    {"key": "s", "action": "TogglePlaybackSync"},
    {"key": "semicolon", "action": "NextBookmark"},
    {"key": "semicolon", "shift": true, "action": "PrevBookmark"},
    {"key": "slash", "action": "OpenSearch"},
    {"key": "slash", "ctrl": true, "action": "OpenKeybindsOverlay"},
    {"key": "space", "action": "PageForward"},
    {"key": "space", "shift": true, "action": "PageBackward"},
    {"key": "u", "action": "SetStartTime"},
    {"key": "u", "ctrl": true, "action": "PageForward"},
    {"key": "w", "action": "WordCycleCopy"},
    {"key": "x", "action": "PageForward"},
    {"key": "y", "action": "PageBackward"},
    {"key": "y", "ctrl": true, "action": "CopyLineMappingId"}
  ]
}
```

(Cross-check this list against `default_reader_bindings()` in keymap_config.rs. Every entry there should be present here; every entry here should be in there.)

- [ ] **Step 2: Deploy via stow**

```bash
cd ~/tty-dotfiles && stow -n -v linux-lit  # dry run first
cd ~/tty-dotfiles && stow linux-lit
ls -la ~/.config/linux-lit/keymap.json
```

Expected: `~/.config/linux-lit/keymap.json -> ../../tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`

If stow reports a conflict (file already exists at that path), back it up first:

```bash
mv ~/.config/linux-lit/keymap.json ~/.config/linux-lit/keymap.json.backup-pre-stow
cd ~/tty-dotfiles && stow linux-lit
```

- [ ] **Step 3: Add stow workflow to CLAUDE.md**

Read current CLAUDE.md:

```bash
cd /home/mlj/utono/linux-lit && grep -n "## Conventions\|## External Data\|## Reference Codebases" CLAUDE.md | head -5
```

Add a new section before "## Reference Codebases":

```markdown
## Keymap Configuration

Reader keybindings are loaded from `~/.config/linux-lit/keymap.json` at
startup. If the file is missing or malformed, linux-lit falls back to
compiled-in defaults (see `src/input/keymap_config.rs:default_reader_bindings`).

### Stow workflow

The canonical default keymap is shipped as a stow package at
`~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`. Deploy with:

```bash
cd ~/tty-dotfiles && stow linux-lit
```

Restart linux-lit; the new bindings take effect on next launch.

### Customizing bindings

Edit `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (the stow
source). Each binding is an object: `{"key": "x", "action": "PageForward"}`.
Optional modifier flags: `"ctrl": true`, `"shift": true`, `"alt": true`.

Available actions are the variants of `crate::input::actions::Action` —
see `src/input/actions/mod.rs`. Unknown action names are skipped at load
with a logged warning; malformed JSON falls back to compiled-in defaults
entirely.

User overrides take precedence over defaults; bindings not present in
the JSON keep their compiled-in default.
```

- [ ] **Step 4: Manual verification (FINAL)**

```
1. cargo run.
2. Press x — confirm PageForward.
3. Edit ~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json.
   Change "x" entry's action from "PageForward" to "PageBackward".
4. Restart linux-lit.
5. Press x — confirm now pages backward.
6. Revert the JSON change. Restart. Press x — confirm pages forward again.
7. Introduce a syntax error in the JSON (delete a closing brace).
   Restart. Check logs:

   tail -20 ~/utono/linux-lit/linux-lit-dev.log

   Expected: a "keymap.json parse error: ...; using defaults" warning.
8. Press x — confirm pages forward (defaults active).
9. Fix the JSON. Restart. Confirm normal operation.
```

- [ ] **Step 5: Commit (linux-lit side)**

```bash
cd /home/mlj/utono/linux-lit && git add CLAUDE.md && git commit -m "$(cat <<'EOF'
docs: document keymap.json + stow workflow

Phase 2.6 of F4+F2 keymap refactor. Documents how to deploy and
customize the reader keymap via the new tty-dotfiles stow package.

The linux-lit binary doesn't ship keymap.json — that's the dotfiles
repo's job. linux-lit falls back to compiled-in defaults if the JSON
file is absent.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 6: Commit (tty-dotfiles side)**

```bash
cd ~/tty-dotfiles && git add linux-lit && git commit -m "$(cat <<'EOF'
Add linux-lit keymap.json stow package

Canonical default keybindings for ~/utono/linux-lit's reader. Mirrors
default_reader_bindings() in src/input/keymap_config.rs. Deploy with:
  cd ~/tty-dotfiles && stow linux-lit

linux-lit falls back to compiled-in defaults if this file is absent
or malformed; the stow package is for users who want to customize
without forking linux-lit source.
EOF
)"
```

---

# Phase 3 — Final verification

- [ ] **Step 1: Confirm clean tree**

```bash
cd /home/mlj/utono/linux-lit && git status
cd ~/tty-dotfiles && git status
```

Expected: both `nothing to commit, working tree clean`.

- [ ] **Step 2: Confirm test suite**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | tail -10
```

Expected: 126 pass / 1 pre-existing fail.

- [ ] **Step 3: Confirm commit log**

```bash
cd /home/mlj/utono/linux-lit && git log --oneline -10
```

Expected order (most recent first):
1. `docs: document keymap.json + stow workflow`
2. `Migrate base-key match arms to keymap.lookup() + dispatch_action`
3. `Add dispatch_action table mapping Action enum to verb calls`
4. `Add Keymap field to AppState; load from ~/.config/linux-lit/keymap.json`
5. `Implement Keymap loader + default_reader_bindings (no callers yet)`
6. `Define Action enum + KeyCombo struct (no callers yet)`
7. `Extract settings revert + reset verbs into actions::settings`
8. `Extract concordance::open_picker verb`
9. `Extract bookmark verbs into actions::bookmarks`
10. `Extract picker verbs into actions::pickers`
(plus `Create actions/ module; relocate concordance + settings free fns` and prior session commits)

- [ ] **Step 4: User signoff**

Output to chat:

> "F4 + F2 implementation complete. 11 commits on master (linux-lit) + 1 commit on tty-dotfiles. Both manual verification gates passed. Ready to push to origin, or continue with another finding (F1 OverlayMode trait — L-effort, deferred per the review) — your call."

Do not push. Wait for the user.

---

## Self-Review

**Spec coverage (F4):**
- 12 verbs to extract — Tasks 1.1 (3 free fn relocations) + 1.2 (5 picker verbs) + 1.3 (2 bookmark verbs) + 1.4 (1 concordance verb) + 1.5 (2 settings verbs) = 13 verb relocations. Wait, that's 13. Let me recount: pure relocations are 3 (handle_concordance_word_selection, apply_settings_change, apply_theme_to_state). Five picker verbs = 5. Two bookmark verbs = 2. One concordance open verb = 1. Two settings verbs (revert, reset) = 2. Total: 3 + 5 + 2 + 1 + 2 = 13. Spec said 12 because `apply_theme_to_state` was a plan-time discovery added during planning. Updated.
- Module layout (`actions/{bookmarks,pickers,concordance,settings}.rs`) — ✓ all four files created.
- Verb signature shapes — ✓ Each verb takes `&Rc<RefCell<AppState>>` (async) or `&mut AppState` (sync) per the spec.
- No public API change — ✓ All verbs are `pub(crate)`.

**Spec coverage (F2):**
- Action enum — ✓ Task 2.1, ~70 variants matching the spec list.
- KeyCombo + load/lookup — ✓ Task 2.2.
- Default bindings — ✓ Task 2.2's `default_reader_bindings()`.
- AppState integration — ✓ Task 2.3.
- dispatch_action table — ✓ Task 2.4.
- Base-key match block migration — ✓ Task 2.5.
- Stow package — ✓ Task 2.6.
- Tests (~5-8 in spec; plan delivers 8) — ✓ Task 2.2 mod tests.
- JSON schema — ✓ shown in Task 2.6's keymap.json.
- Validation (unknown action, malformed JSON, conflicts) — ✓ Task 2.2 covers all three.

**Placeholder scan:** No "TBD" / "TODO" / "fill in later". Code blocks contain actual code. Manual verification protocol reproduced inline. ✓

**Type / API consistency:**
- `Keymap::lookup(&str, bool, bool, bool) -> Option<Action>` — used consistently in Task 2.2 (definition), Task 2.4 (no direct call), Task 2.5 (call site). ✓
- `KeyCombo::plain/ctrl/shift/alt/ctrl_shift/ctrl_alt` constructors — used in Task 2.2 tests + `default_reader_bindings`. ✓
- `dispatch_action(state, action, key_state, tokio_handle) -> bool` — Task 2.4 definition matches Task 2.5 call site. ✓
- Verb signatures — `pickers::open_bookmark_picker(state: &Rc<RefCell<AppState>>, tokio_handle: &tokio::runtime::Handle)` matches between Task 1.2 (definition) and Task 2.4's `OpenBookmarkPicker` arm (call). ✓

**Notes for the executor:**
- Task 2.5 is the highest-risk task — it deletes ~400 lines of working code in one commit. Take it slow; cargo build between sub-edits if you can.
- The `Escape` handler stays inline. Don't try to migrate it to dispatch_action — the multi-state precondition logic doesn't fit the static-action model.
- Vocab popup `g` and `Tab` (when popup visible) stay inline — they're popup-visible-context-sensitive overrides, not bindings.
- The chord COMPLETION handlers (the `g`-then-`g` and `g`-then-`;` checks at line 1101) stay inline. Only `Action::PendingG` (chord ENTRY) goes through dispatch.
- If a manual verification regression points at a missing binding, grep `default_reader_bindings()` to confirm; add the missing entry there AND in `keymap.json`.
