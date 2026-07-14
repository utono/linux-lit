# Persistent live "you are here" scene/chapter toast

**Date:** 2026-07-14
**Status:** Design — approved, ready for implementation plan

## Problem

Pressing `+` (keysym `plus`, unshifted on RPD `<AE01>`) dispatches
`Action::ShowCurrentChapter` → `show_current_chapter`
(`src/input/navigation.rs:2347`), which shows a bottom-center toast via
`show_chapter_toast` (`navigation.rs:2468`) — `"{abbrev} — Act N, Scene M"`
for a play, `"{abbrev} — Chapter N of M — {title}"` for prose with chapters.
That toast auto-hides after 3 seconds (`navigation.rs:2478-2486`,
generation-guarded by `chapter_toast_gen`).

The user wants that toast to **persist** — for a **play** and for **prose with
chapters** — becoming a live "you are here" indicator that follows the cursor,
toggled off by pressing `+` again, and temporarily yielding only to the
same-slot search toast. Front-matter-only prose (no chapter markers) does NOT
persist.

## Current mechanics (what we build on)

- Binding: `keymap_config.rs:278` `(KeyCombo::plain("plus"), ShowCurrentChapter)`;
  also on `C` (`keymap_config.rs:238`). User `keymap.json` confirms `plus`.
- Dispatch: `keymap.rs:3539` → `navigation::show_current_chapter`.
- Toast widget: `chapter_toast` `gtk4::Label`, bottom-center
  (`valign End`, `halign Center`, `margin_bottom 32`, css `chapter-toast`),
  created `app/mod.rs:1594-1599`, overlaid `app/mod.rs:1678`.
- Auto-dismiss: 3s `timeout_add_local_once` in `show_chapter_toast`
  (`navigation.rs:2478-2486`), guarded by `chapter_toast_gen` (`app/mod.rs:669`).
- Bottom toasts do NOT overlap: `speed_toast`/copy sit bottom-LEFT
  (`app/mod.rs:1601-1607`), `search_toast` sits in a lower center strip
  (`margin_bottom 5`, `app/mod.rs:1613-1618`). Only `search_toast` shares the
  center column with `chapter_toast`.
- Work type is a plain `String` (`db/models.rs`); predicates are inverse —
  `is_prose()` (`app/mod.rs:726`), `is_anthology()` (`:751`). **No `is_play`.**
- Per-navigation scene refresh already exists: `update_title_bar_scene`
  (`scene_synopsis.rs:566`) is called from `update_highlight`
  (`highlight.rs:446, 486`) and `keymap.rs:3534` on every cursor move, and
  already computes the current scene.

## Design

### 1. Persistence predicate — play OR prose-with-chapters

`show_current_chapter` already branches three ways (`navigation.rs:2363-2445`):

- `!is_prose()` → act/scene label (plays and other verse).
- prose with non-empty `chapter_lines` (`lm.chapter_breaks` / `l.is_chapter`,
  `navigation.rs:2376-2385`) → `"Chapter N of M — {title}"`.
- prose with **empty** `chapter_lines` → scene-label fallback
  (`navigation.rs:2389-2394`).

Persistence is enabled for the first two, NOT the third. Add two helpers on
`AppState` (next to `is_prose`/`is_anthology`, `src/app/mod.rs`):

- `is_play()` → `current_work.map(|w| w.work_type == "play").unwrap_or(false)`.
- `chapter_toast_persists()` → true when `is_play()`, OR (`is_prose()` AND the
  work has ≥1 chapter marker). Compute "has chapters" the same way
  `show_current_chapter` does — `line_map.chapter_breaks` non-empty, else any
  `work.lines[*].is_chapter`.

Front-matter-only prose and non-play verse fall through to the unchanged
transient 3-second toast.

**Decision (settled):** persistence is plays + prose-with-chapters ONLY.
Non-play verse (`poem`, `sonnet_sequence`) takes the `!is_prose()` act/scene
branch for its toast text but does NOT persist — `is_play()` is exact. Do not
widen it to the whole `!is_prose()` branch.

### 2. Persistent state + `+` becomes a toggle (plays only)

Add `chapter_toast_persistent: Cell<bool>` to `AppState` (default false).
`show_current_chapter` branches on `chapter_toast_persists()`:

- **Non-persisting work (unchanged):** front-matter-only prose and non-play
  verse (`poem` / `sonnet_sequence`) — build text, call `show_chapter_toast`
  (3s transient, generation-guarded) exactly as today.
- **Persisting work (play or prose-with-chapters) — toggle:**
  - persistent flag ON → turn OFF: set flag false, hide `chapter_toast`.
  - persistent flag OFF → turn ON: set flag true, build current text (the SAME
    text the existing branch already builds — scene label for a play, "Chapter
    N of M" for prose), show `chapter_toast` in **persistent (timer-less)** mode.

`show_chapter_toast` gains a persistent mode (a `persistent: bool` param or a
sibling `show_chapter_toast_persistent`): when persistent, it sets the text +
visibility and bumps `chapter_toast_gen` (so any in-flight transient timer
becomes a no-op) but installs **no** `timeout_add_local_once`. Mirrors
`ui/toast.rs:show_persistent` (`toast.rs:29`, no timer).

`C` shares `ShowCurrentChapter`, so it gets the same toggle — one action, one
behavior.

### 3. Live "you are here" updates

While the flag is ON, the toast text follows the cursor across scene/chapter
boundaries. Add a small helper `refresh_persistent_chapter_toast(state)`:

```
if state.chapter_toast_persistent.get() {
    let text = compute_current_chapter_text(state); // same builder the toggle uses
    state.chapter_toast.set_text(&text);
    state.chapter_toast.set_visible(true);
}
```

To avoid divergence, factor the text-building out of `show_current_chapter`
into a shared `compute_current_chapter_text(state) -> String` that both the
`+` toggle and this refresh call — so the live-updating toast always matches
what a fresh `+` would show (scene label for plays, "Chapter N of M" for
prose). Call the refresh from the same per-navigation sites as
`update_title_bar_scene`
(`highlight.rs:446, 486`; `keymap.rs:3534`) — riding the existing update path
rather than a new boundary-crossing detector. It is a SEPARATE call, NOT nested
inside `update_title_bar_scene` (which early-returns when the title bar is
hidden, `scene_synopsis.rs:567`); the persistent toast must refresh
independently of title-bar visibility. Setting identical text is a visual
no-op, so refreshing every move is cheap and correct.

### 4. Yield to the search toast only

The persistent toast yields ONLY to `search_toast` (same center slot). At the
search-toast show/hide sites:

- On `search_toast` show: if the persistent flag is ON, hide `chapter_toast`
  (leave the flag ON).
- On `search_toast` dismiss/clear: if the flag is still ON, re-show
  `chapter_toast` via `refresh_persistent_chapter_toast`.

The flag is the single source of truth: "hidden by search" is just
widget-hidden while the flag stays true. The per-navigation refresh (§3) is a
natural backstop — the next cursor move re-shows it even if the explicit
restore is missed.

Speed/copy toasts (bottom-left) and any center overlay cards do NOT affect it.

### 5. Interactions & edge cases

- **Work switch:** reset `chapter_toast_persistent` to false in `display_work`
  so a persistent indicator never leaks across works.
- **Persisting → non-persisting work:** the new work never re-arms the flag;
  the reset in `display_work` clears it.
- **Front-matter-only prose / non-play verse / anthology:** transient 3s
  path fully unchanged.
- **Rapid `+` presses:** generation bump keeps behavior sane; the toggle reads
  the flag, so two presses = on then off.

## Files touched

- `src/app/mod.rs` — add `is_play()` and `chapter_toast_persists()`; add
  `chapter_toast_persistent: Cell<bool>`; reset it in `display_work`.
- `src/input/navigation.rs` — factor out `compute_current_chapter_text`;
  `show_current_chapter` becomes a toggle for persisting works;
  `show_chapter_toast` gains a persistent (timer-less) mode; add
  `refresh_persistent_chapter_toast`.
- `src/input/highlight.rs` / `src/input/keymap.rs` — call
  `refresh_persistent_chapter_toast` alongside `update_title_bar_scene` on
  cursor move.
- Search-toast show/hide sites — yield/restore the persistent toast.

## Testing

- **Headless e2e** (`test-headless-navigation` skill, via `scripts/e2e-env.sh`):
  - Play: press `+` → toast shows; navigate across a scene boundary → text
    updates to the new scene; press `+` again → hides.
  - Prose-with-chapters: press `+` → toast shows; navigate into the next
    chapter → text updates to "Chapter N+1 of M"; press `+` again → hides.
  - Screenshot each step.
- **Regression:** on front-matter-only prose (no chapter markers), `+` still
  auto-dismisses after 3s (generation-guarded), text unchanged.
- **Search yield:** persistent toast up, trigger a search boundary toast →
  chapter toast hides, then reappears when the search toast clears.
- **Unit:**
  - `is_play()` true only for `work_type == "play"`.
  - `chapter_toast_persists()` true for a play and for prose with ≥1 chapter
    marker; false for front-matter-only prose, `poem`, `sonnet_sequence`,
    and `anthology`.
- **Work-switch:** flag resets — after showing the persistent toast, loading
  another work leaves it hidden.

## Non-goals (YAGNI)

- A general float z-order / suppression system — only the single search-toast
  yield is built.
- Persistence for front-matter-only prose (no chapter markers), non-play verse
  (`poem`, `sonnet_sequence`), and anthology.
- A config option for the dismiss timeout or persistence — `+` toggle is the
  chosen control.
