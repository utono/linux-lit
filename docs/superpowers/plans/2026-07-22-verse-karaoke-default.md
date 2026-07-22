# Verse Karaoke Default + Alt+p Axis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Verse works karaoke-highlight by default with the cursor-line tint off; Alt+p swaps between karaoke display and cursor-line display (mutually exclusive, both classes).

**Architecture:** A session-only axis flag `AppState.cursor_line_mode` (false = karaoke at every launch). One predicate, `karaoke_marks_cursor`, generalizes prose's existing `prose_no_tint` suppression to both classes: the persistent cursor tint is suppressed exactly when karaoke is actually marking the cursor line (axis in karaoke mode, class mode on, media connected with phrase rows — memoized — and the cursor line timestamped). `active_mode` gains the mirror gate so the sweep, o/e steps, and painters all obey the axis. `Config.show_cursor_line` is retired; Alt+p (`Action::TogglePhraseHighlight`, name kept so keymap.json stays valid) flips the axis without saving config; stored `"off"` karaoke modes migrate to `"phrase"` at load.

**Tech Stack:** Rust, GTK4, existing linux-lit test layout (`#[cfg(test)]` modules in-file), cage headless harness.

Spec: `docs/superpowers/specs/2026-07-22-verse-karaoke-default-design.md`

## Global Constraints

- Repo: `~/utono/linux-lit`, branch master (session practice: commit scoped files only — the tree carries unrelated in-flight edits; NEVER `git add -A`).
- The axis is session-only: no `config::save` on Alt+p; every launch starts karaoke (cursor_line_mode = false).
- `Action::TogglePhraseHighlight` keeps its name (keymap.json compatibility).
- Compiled default `phrase_highlight_verse` becomes `Phrase`; `load()` migrates stored `"off"` → `"phrase"` for BOTH classes.
- `Config.show_cursor_line` is fully retired (field, default fn, Default impl, forced reset in load()); serde ignores stale JSON keys.
- DB failure while checking phrase capability ⇒ treat as incapable (cursor line shows).
- Fallback rule: the reader must NEVER be indicator-less — no media, phraseless media, class mode Off, or untimestamped cursor line ⇒ cursor tint shows.
- Verify with `cargo build` / `cargo test --bins`; do NOT `cargo run` (user runs the app).
- Commit messages end with the standard Claude trailer used in this repo.

---

### Task 1: Axis state + karaoke_marks_cursor predicate + active_mode gate

**Files:**
- Modify: `src/app/mod.rs` (~line 309 field block, ~line 2041 init block)
- Modify: `src/input/phrase_highlight.rs` (near `active_mode`, ~line 197; tests module at end)

**Interfaces:**
- Produces (later tasks call these exactly):
  - `AppState.cursor_line_mode: bool` and `AppState.phrase_capable_memo: Option<(i64, bool)>`
  - `pub fn media_karaoke_capable(s: &mut AppState) -> bool` (phrase_highlight.rs)
  - `pub fn karaoke_marks_cursor(s: &mut AppState) -> bool` (phrase_highlight.rs)
  - `pub(crate) fn karaoke_marks_cursor_for(cursor_line_mode: bool, class_mode_on: bool, media_present: bool, media_has_phrases: bool, cursor_has_timestamp: bool) -> bool` (pure, tested)

- [ ] **Step 1: Write the failing tests** (append inside the existing `#[cfg(test)] mod tests` at the bottom of `src/input/phrase_highlight.rs`)

```rust
    #[test]
    fn karaoke_marks_cursor_truth_table() {
        // Marks only when: axis karaoke + class mode on + media present with
        // phrase rows + cursor line timestamped.
        assert!(karaoke_marks_cursor_for(false, true, true, true, true));
        // Axis swapped to cursor-line mode: never marks.
        assert!(!karaoke_marks_cursor_for(true, true, true, true, true));
        // Class mode Off (manual config edit): cursor line is the indicator.
        assert!(!karaoke_marks_cursor_for(false, false, true, true, true));
        // No connected media: fallback to cursor line.
        assert!(!karaoke_marks_cursor_for(false, true, false, false, true));
        // Media without phrase rows (un-backfilled edition): fallback.
        assert!(!karaoke_marks_cursor_for(false, true, true, false, true));
        // Untimestamped cursor line (front matter): fallback per line.
        assert!(!karaoke_marks_cursor_for(false, true, true, true, false));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --bin linux-lit karaoke_marks_cursor 2>&1 | tail -5`
Expected: compile error — `karaoke_marks_cursor_for` not found.

- [ ] **Step 3: Add the AppState fields**

In `src/app/mod.rs`, directly after the `phrase_paint_hold` field (~line 309):

```rust
    /// Session-only display axis: false = karaoke (default every launch),
    /// true = classic cursor-line display. Alt+p flips it; never persisted.
    pub cursor_line_mode: bool,
    /// Memo for "does this media have ANY phrase_timestamps rows" keyed by
    /// media id; cleared on MPV connection changes. None = not yet checked.
    pub phrase_capable_memo: Option<(i64, bool)>,
```

In the AppState construction block (directly after `phrase_paint_hold: None,` ~line 2041):

```rust
        cursor_line_mode: false,
        phrase_capable_memo: None,
```

- [ ] **Step 4: Add the predicates in `src/input/phrase_highlight.rs`** (directly below `active_mode`, ~line 206)

```rust
/// Pure core of `karaoke_marks_cursor` — see that fn for the semantics.
pub(crate) fn karaoke_marks_cursor_for(
    cursor_line_mode: bool,
    class_mode_on: bool,
    media_present: bool,
    media_has_phrases: bool,
    cursor_has_timestamp: bool,
) -> bool {
    !cursor_line_mode
        && class_mode_on
        && media_present
        && media_has_phrases
        && cursor_has_timestamp
}

/// Karaoke can paint AT ALL for the current media: axis in karaoke mode is
/// NOT part of this — it is the media/class capability half (used by the
/// Alt+p toast). Memoized per media id; DB failure counts as incapable so
/// the cursor line falls back in (never indicator-less).
pub fn media_karaoke_capable(s: &mut AppState) -> bool {
    let class_mode_on = if s.is_prose() {
        s.config.phrase_highlight_prose.is_on()
    } else {
        s.config.phrase_highlight_verse.is_on()
    };
    if !class_mode_on {
        return false;
    }
    let Some(media) = s.media_id else { return false };
    let has = match s.phrase_capable_memo {
        Some((id, v)) if id == media => v,
        _ => {
            let v = crate::db::queries::open_db()
                .map(|conn| crate::db::queries::media_has_phrase_data(&conn, media))
                .unwrap_or(false);
            s.phrase_capable_memo = Some((media, v));
            v
        }
    };
    has
}

/// Karaoke is actually marking the CURSOR line right now, so the persistent
/// cursor-line tint must stay off (the sweep is the position indicator).
/// Generalizes the prose `prose_no_tint` rule to both classes. Reads the
/// CLASS CONFIG mode, not `active_mode`: during the vocab drill the sweep is
/// suppressed but the drill's sentence tint marks position — the cursor tint
/// must not reappear there.
pub fn karaoke_marks_cursor(s: &mut AppState) -> bool {
    if s.cursor_line_mode {
        return false;
    }
    // media_karaoke_capable folds the class-mode and media checks; the pure
    // karaoke_marks_cursor_for spells out the full five-way rule for tests.
    let capable = media_karaoke_capable(s);
    let cursor_has_timestamp = s
        .work_line_for_buffer(s.current_line)
        .and_then(|wi| s.current_work.as_ref()?.lines.get(wi))
        .is_some_and(|l| l.timestamp.is_some());
    capable && cursor_has_timestamp
}
```

- [ ] **Step 5: Gate `active_mode` on the axis** — in the same file (~line 197):

Old:

```rust
fn active_mode(s: &AppState) -> PhraseHighlightMode {
    if s.vocab_loop.is_some() {
        return PhraseHighlightMode::Off;
    }
```

New:

```rust
fn active_mode(s: &AppState) -> PhraseHighlightMode {
    if s.vocab_loop.is_some() {
        return PhraseHighlightMode::Off;
    }
    // Session axis: cursor-line display suppresses the karaoke sweep (and
    // the o/e phrase steps, which fall back to raw seeks).
    if s.cursor_line_mode {
        return PhraseHighlightMode::Off;
    }
```

- [ ] **Step 6: Run tests**

Run: `cargo test --bin linux-lit karaoke_marks_cursor 2>&1 | tail -3`
Expected: `1 passed`. Then `cargo build 2>&1 | tail -3` — success (new pub fns may warn dead_code until Task 2; the repo tolerates dead-code warnings).

- [ ] **Step 7: Commit**

```bash
cd ~/utono/linux-lit && git add src/app/mod.rs src/input/phrase_highlight.rs && git commit -m "karaoke axis: session flag + karaoke_marks_cursor predicate

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011cbBsQuKjBq5NRHzCAKwTD"
```

---

### Task 2: highlight.rs consumers + MPV-connect repaint

**Files:**
- Modify: `src/input/highlight.rs:315-336, 371, 421, 432, 440-443, 518` (the six `state.config.show_cursor_line` reads)
- Modify: `src/main.rs:492-494` (`MpvEvent::ConnectionStatus` arm)

**Interfaces:**
- Consumes: `karaoke_marks_cursor(&mut AppState) -> bool`, `AppState.phrase_capable_memo` (Task 1).
- Produces: `highlight.rs` no longer reads `config.show_cursor_line` anywhere; local `karaoke_no_tint` replaces `prose_no_tint`.

- [ ] **Step 1: Rewrite the condition block in `update_highlight`** (`src/input/highlight.rs` ~315-336)

Old:

```rust
    let prose_dim = PROSE_DIM_OTHER_PARAGRAPHS
        && state.is_prose()
        && state.config.show_cursor_line;
```

New:

```rust
    let prose_dim = PROSE_DIM_OTHER_PARAGRAPHS && state.is_prose();
```

Old (the `cursor_has_timestamp` + `prose_no_tint` computation, ~328-336 —
the multi-line comment above it stays, but reword its prose-specific
sentences per the New block's comment):

```rust
    let cursor_has_timestamp = state
        .work_line_for_buffer(state.current_line)
        .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
        .is_some_and(|l| l.timestamp.is_some());
    let prose_no_tint = !PROSE_DIM_OTHER_PARAGRAPHS
        && state.is_prose()
        && state.config.show_cursor_line
        && cursor_has_timestamp;
```

New:

```rust
    // BOTH classes: no persistent cursor tint while karaoke marks the cursor
    // line (the sweep is the marking) — the prose-only rule generalized now
    // that verse karaoke is on by default. karaoke_marks_cursor folds in the
    // untimestamped-line fallback described above, plus media capability and
    // the Alt+p axis.
    let karaoke_no_tint =
        !PROSE_DIM_OTHER_PARAGRAPHS && crate::input::phrase_highlight::karaoke_marks_cursor(state);
```

(The `cursor_has_timestamp` local disappears — the predicate owns that
check. If a later line in the function still references
`cursor_has_timestamp`, rg first: `rg -n cursor_has_timestamp src/input/highlight.rs`
— as of this plan only the deleted binding uses it.)

- [ ] **Step 2: The fade gate** (~371) and its closing comment (~421)

Old:

```rust
        if state.config.show_cursor_line && !prose_dim && !prose_no_tint {
```

New:

```rust
        if !prose_dim && !karaoke_no_tint {
```

Old (~421):

```rust
        } // show_cursor_line
```

New:

```rust
        } // cursor fade (skipped while karaoke marks the cursor line)
```

- [ ] **Step 3: The tint-apply gate** (~432-443)

Old:

```rust
        if state.config.show_cursor_line {
            if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
                let mut line_end = line_start;
                if !line_end.ends_line() {
                    line_end.forward_to_line_end();
                }
                if prose_dim {
                    buffer.remove_tag(tag, &line_start, &line_end);
                } else if !prose_no_tint {
                    // Non-dim prose has no persistent cursor tint — the
                    // karaoke tint is the only marking.
                    buffer.apply_tag(cl_tag, &line_start, &line_end);
                }
```

New (drop the outer conditional — the master switch is gone; suppression
lives in `karaoke_no_tint`):

```rust
        {
            if let Some(line_start) = buffer.iter_at_line(state.current_line as i32) {
                let mut line_end = line_start;
                if !line_end.ends_line() {
                    line_end.forward_to_line_end();
                }
                if prose_dim {
                    buffer.remove_tag(tag, &line_start, &line_end);
                } else if !karaoke_no_tint {
                    // No persistent cursor tint while karaoke marks the
                    // cursor line — the sweep is the only marking.
                    buffer.apply_tag(cl_tag, &line_start, &line_end);
                }
```

(Keep the block's closing braces exactly as they are; only the opening
line and the two identifier/comment changes shown here.)

Then sweep the rest of the function for any remaining `prose_no_tint`
mention: `rg -n prose_no_tint src/input/highlight.rs` must return nothing
(rename every hit to `karaoke_no_tint`).

- [ ] **Step 4: `flush_pending_prose_flash`** (~518)

Old:

```rust
    if PROSE_DIM_OTHER_PARAGRAPHS || !state.is_prose() || !state.config.show_cursor_line {
        return;
    }
```

New:

```rust
    // Flash only where karaoke is the marking (no persistent tint to look
    // at); with the axis on cursor-line mode — or any karaoke fallback — the
    // persistent tint already re-orients the eye.
    if PROSE_DIM_OTHER_PARAGRAPHS
        || !state.is_prose()
        || !crate::input::phrase_highlight::karaoke_marks_cursor(state)
    {
        return;
    }
```

- [ ] **Step 5: Repaint on MPV connection changes** — `src/main.rs`, in the `MpvEvent::ConnectionStatus(connected)` arm, directly after `s.mpv_connected = connected;` (~line 493):

```rust
                        // Axis fallback: capability may have just changed
                        // (connect/disconnect), so re-evaluate the cursor
                        // tint now, not on the next nav key.
                        s.phrase_capable_memo = None;
                        crate::input::navigation::update_highlight_only(&mut s);
```

- [ ] **Step 6: Verify no reads remain + build + tests**

Run: `rg -n "config.show_cursor_line" src/input/highlight.rs` — expected: no matches.
Run: `cargo test --bins 2>&1 | tail -3` — expected: all pass (1045+).

- [ ] **Step 7: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/highlight.rs src/main.rs && git commit -m "highlight: cursor tint yields to karaoke via karaoke_marks_cursor

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011cbBsQuKjBq5NRHzCAKwTD"
```

---

### Task 3: Alt+p swap handler + settings redirect + retire Config.show_cursor_line

**Files:**
- Modify: `src/input/keymap.rs:4243-4266` (TogglePhraseHighlight arm)
- Modify: `src/input/actions/settings.rs:61-64, 408, 442, 474, 532`
- Modify: `src/config.rs:178-179, 328-330, 397, 474` (retire field) and the `cycle()` impl (~63-67) + its test (~725-730)

**Interfaces:**
- Consumes: `AppState.cursor_line_mode`, `media_karaoke_capable`, `karaoke_marks_cursor` (Tasks 1–2).
- Produces: `Config` has NO `show_cursor_line`; `PhraseHighlightMode` has no `cycle()`; Alt+p flips the axis with no config save.

- [ ] **Step 1: Rewrite the Alt+p arm** (`src/input/keymap.rs`, the whole `TogglePhraseHighlight => { ... }` block, ~4243-4266)

```rust
        TogglePhraseHighlight => {
            // Two-state axis swap: karaoke display <-> cursor-line display.
            // Session-only (never saved); every launch starts in karaoke.
            // The per-class phrase/line WIDTH stays config-only.
            let mut s = state.borrow_mut();
            s.cursor_line_mode = !s.cursor_line_mode;
            crate::input::phrase_highlight::clear_phrase_highlight(&mut s);
            let text = if s.cursor_line_mode {
                "Cursor line"
            } else if !crate::input::phrase_highlight::media_karaoke_capable(&mut s) {
                "Karaoke (no phrase audio — cursor line kept)"
            } else {
                "Karaoke"
            };
            crate::input::navigation::update_highlight_only(&mut s);
            crate::input::navigation::show_chapter_toast(&s, text);
            crate::logging::log(&format!("PHRASE_HL: axis -> {}", text));
        }
```

- [ ] **Step 2: Settings redirect** (`src/input/actions/settings.rs`)

Line ~62, old: `s.config.show_cursor_line = val;` → new: `s.cursor_line_mode = val;`
Line ~408, old: `s.config.show_cursor_line = snap_cl;` → new: `s.cursor_line_mode = snap_cl;`
Line ~442, old: `let cl = s.config.show_cursor_line;` → new: `let cl = s.cursor_line_mode;`
Line ~474, old: `let cl = s.config.show_cursor_line;` → new: `let cl = s.cursor_line_mode;`
Line ~532, old: `s.config.show_cursor_line = false;` → new: `s.cursor_line_mode = false;`

(The settings overlay's "cursor line On/Off" row now displays and drives
the same axis as Alt+p — they can never disagree. No changes needed in
`src/ui/settings_overlay.rs`: its `show_cursor_line` params are plain
bools fed from these call sites.)

- [ ] **Step 3: Retire the config field** (`src/config.rs`)

Delete these four spots:

```rust
    #[serde(default = "default_show_cursor_line")]
    pub show_cursor_line: bool,
```

```rust
fn default_show_cursor_line() -> bool {
    true
}
```

(check the exact body with `sed -n 328,331p src/config.rs` before deleting)

In `impl Default for Config` (~397): the `show_cursor_line: true,` line.

In `load()` (~474): the `config.show_cursor_line = true;` line.

- [ ] **Step 4: Remove `cycle()`** (`src/config.rs` ~62-67) and its unit test (the `assert_eq!(Off.cycle(), Phrase); ...` test fn at ~725-730). Keep `label()` — the Alt+p arm no longer calls it, so check other callers first: `rg -n '\.label\(\)' src/` — if the ONLY hit was the old keymap.rs toast, delete `label()` too; if settings/other UI uses it, keep it.

- [ ] **Step 5: Build + full tests**

Run: `rg -n "show_cursor_line" src/ --type rust | rg -v settings_overlay` — expected: no matches (settings_overlay.rs keeps its local param names; they're plain bools).
Run: `cargo test --bins 2>&1 | tail -3` — expected: all pass.

- [ ] **Step 6: Commit**

```bash
cd ~/utono/linux-lit && git add src/input/keymap.rs src/input/actions/settings.rs src/config.rs && git commit -m "Alt+p: karaoke/cursor-line axis swap; retire config.show_cursor_line

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011cbBsQuKjBq5NRHzCAKwTD"
```

---

### Task 4: Verse default + off→phrase migration + docs/overlay text

**Files:**
- Modify: `src/config.rs:344-346` (default), `load()` (~466-476), tests module
- Modify: `src/ui/keybinds_overlay.rs:55, 274`
- Modify: `docs/guides/keybind-surface-guide.md` (new/updated Alt+p section)

**Interfaces:**
- Consumes: nothing new. Produces: `migrate_phrase_modes(&mut Config)` (pub(crate), called by `load()`).

- [ ] **Step 1: Write the failing migration test** (in the existing `#[cfg(test)]` module of `src/config.rs`)

```rust
    #[test]
    fn migrate_phrase_modes_maps_off_to_phrase() {
        use PhraseHighlightMode::{Line, Off, Phrase};
        let mut c = Config::default();
        c.phrase_highlight_prose = Off;
        c.phrase_highlight_verse = Off;
        migrate_phrase_modes(&mut c);
        assert_eq!(c.phrase_highlight_prose, Phrase);
        assert_eq!(c.phrase_highlight_verse, Phrase);
        // phrase / line survive untouched.
        c.phrase_highlight_prose = Phrase;
        c.phrase_highlight_verse = Line;
        migrate_phrase_modes(&mut c);
        assert_eq!(c.phrase_highlight_prose, Phrase);
        assert_eq!(c.phrase_highlight_verse, Line);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --bin linux-lit migrate_phrase_modes 2>&1 | tail -4`
Expected: compile error — `migrate_phrase_modes` not found.

- [ ] **Step 3: Implement**

`src/config.rs` ~344, old:

```rust
fn default_phrase_highlight_verse() -> PhraseHighlightMode {
    PhraseHighlightMode::Off
}
```

New:

```rust
fn default_phrase_highlight_verse() -> PhraseHighlightMode {
    PhraseHighlightMode::Phrase
}
```

Add (near `load()`):

```rust
/// `off` is no longer a persisted karaoke mode — the session axis (Alt+p)
/// expresses it at runtime. Stored `off` values predate the axis; treat
/// them as `phrase` so verse karaoke actually defaults on (a stored value
/// always beats a compiled default).
pub(crate) fn migrate_phrase_modes(config: &mut Config) {
    if config.phrase_highlight_prose == PhraseHighlightMode::Off {
        config.phrase_highlight_prose = PhraseHighlightMode::Phrase;
    }
    if config.phrase_highlight_verse == PhraseHighlightMode::Off {
        config.phrase_highlight_verse = PhraseHighlightMode::Phrase;
    }
}
```

In `load()`, next to the other post-load fixups (where the retired
`show_cursor_line` reset used to sit, after `config.text_margins = ...`):

```rust
    migrate_phrase_modes(&mut config);
```

- [ ] **Step 4: Run tests**

Run: `cargo test --bin linux-lit migrate_phrase_modes 2>&1 | tail -3` — expected: `1 passed`.

- [ ] **Step 5: Overlay + guide text**

`src/ui/keybinds_overlay.rs:55`, old:

```rust
    key("p", "P", "nudge \u{2212}0.2", "P: +0.2", &[("M-p", "phrase hl")]),
```

New:

```rust
    key("p", "P", "nudge \u{2212}0.2", "P: +0.2", &[("M-p", "karaoke")]),
```

`src/ui/keybinds_overlay.rs:274`, old:

```rust
        "phrase hl" => "Action::TogglePhraseHighlight — src/input/phrase_highlight.rs",
```

New:

```rust
        "karaoke" => "Action::TogglePhraseHighlight — src/input/keymap.rs (karaoke ↔ cursor-line swap)",
```

(If the describe() key string must match the keycap label — check how
other entries pair strip labels to describe arms in this file — keep them
consistent: the match arm string equals the strip label.)

`docs/guides/keybind-surface-guide.md`: read the intro template, then add
(or replace any existing phrase-highlight section for Alt+p) one `##`
section in the template's shape documenting: main card, Alt+p, swaps
karaoke display (phrase sweep, no cursor tint) ↔ cursor-line display
(persistent tint, no sweep); session-only (launches in karaoke); cursor
line auto-shows when karaoke can't paint (no media / no phrase rows /
class mode off / untimestamped cursor line); width (phrase vs line) is
config-only (`phrase_highlight_prose` / `phrase_highlight_verse`).

- [ ] **Step 6: Full suite + clippy**

Run: `cargo test --bins 2>&1 | tail -3` — all pass.
Run: `cargo clippy --bin linux-lit 2>&1 | rg "^error" | head` — no errors (warnings tolerated).

- [ ] **Step 7: Commit**

```bash
cd ~/utono/linux-lit && git add src/config.rs src/ui/keybinds_overlay.rs docs/guides/keybind-surface-guide.md && git commit -m "verse karaoke default: Phrase + off->phrase migration; overlay/guide text

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011cbBsQuKjBq5NRHzCAKwTD"
```

---

### Task 5: Headless verification (operational, no commit)

**Files:** none (screenshots under `/tmp/claude-1000/-home-mlj-utono-linux-lit/b031ce0e-6cd5-41b6-883b-4a7f7e0a0a56/scratchpad/`)

**Interfaces:** consumes the built binary from Task 4.

- [ ] **Step 1: Launch headless** (per CLAUDE.md: `LIT_DEV=1` mandatory for ad-hoc cage runs, `GSK_RENDERER=cairo` mandatory, cleanup ONLY via the scoped pkill)

```bash
cd ~/utono/linux-lit && cargo build
LIT_DEV=1 LIT_NO_MPV=1 GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
sleep 3
```

Export `WAYLAND_DISPLAY` to the fresh socket (`ls /run/user/1000/wayland-*`), find the fresh `-{n}` log by mtime.

- [ ] **Step 2: Fallback check** — the loaded work under LIT_NO_MPV has no connected media ⇒ despite the karaoke default, the CURSOR LINE TINT MUST BE VISIBLE. `grim` a screenshot, open it, confirm by eye a tinted cursor line exists (per UI review protocol: describe what you see).

- [ ] **Step 3: Alt+p axis check** — `wtype -M alt -k p -m alt`, screenshot within 3s: toast reads "Cursor line". Again: toast reads "Karaoke (no phrase audio — cursor line kept)" (no media headlessly). Log has the two `PHRASE_HL: axis ->` lines.

- [ ] **Step 4: Cleanup**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

- [ ] **Step 5: Report** — screenshots reviewed inline + the live-test handoff for the user (real MPV on an Arkangel play: sweep with no cursor line; Alt+p ↔ swaps; o/e raw-seek in cursor-line mode).

---

## Self-Review Notes

- Spec coverage: axis flag (T1), visibility predicate incl. per-line
  timestamp fallback (T1/T2), active_mode gate (T1), highlight consumers +
  connect repaint (T2), Alt+p + toast + no-save (T3), settings redirect
  (T3), retire show_cursor_line (T3), verse default + migration (T4),
  overlay/guide docs (T4), headless + live verification (T5). Error
  handling (DB fail ⇒ incapable) in T1's `unwrap_or(false)`.
- Type consistency: `cursor_line_mode: bool`, `phrase_capable_memo:
  Option<(i64, bool)>`, `media_karaoke_capable(&mut AppState) -> bool`,
  `karaoke_marks_cursor(&mut AppState) -> bool` used identically in T1–T3.
- Known judgment point for the implementer (T2 Step 1): `update_highlight`
  may borrow-conflict computing `karaoke_marks_cursor(state)` mid-function
  if immutable borrows are live — compute it at the top of the function
  beside `prose_dim`, before any buffer borrows.
