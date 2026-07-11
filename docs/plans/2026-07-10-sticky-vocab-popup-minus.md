# Sticky Vocab Popup on Minus Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `-` cycles vocab words with the popup staying up (no 3s auto-hide anywhere), `Ctrl+-` fades it out via a new `HideVocabPopup` action; `z` is freed, `TogglePause` leaves `-` (still on `a`/Space), `OpenRecentPicker` becomes unbound.

**Architecture:** Delete the auto-hide timer from `handle_vocab_popup_key` (keymap.rs:3485-3527) and move its fade animation into an immediately-executed `fade_out_vocab_popup` helper backing the new action. Rebind in compiled defaults + stow keymap.json + Ctrl+/ overlay, all in lockstep.

**Tech Stack:** Rust, gtk4/adw (`TimedAnimation`), serde-derived action names (keymap.json parses enum variant names directly — `parse_action`, keymap_config.rs:167-171).

## Global Constraints

- Design doc: `docs/plans/2026-07-10-sticky-vocab-popup-minus-design.md`.
- `#` keeps `VocabPopupPrev`; `H` keeps `ToggleVocabPopup`; both become sticky too (auto-hide is removed popup-wide).
- Keybind changes must land in all four surfaces in the SAME change: `keymap_config.rs`, `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (silently overrides compiled defaults otherwise), `keybinds_overlay.rs` KeyDefs + `describe()`, RPD sanity (key name `minus` already proven in the current map).
- `HideVocabPopup` must be idempotent (no-op when popup hidden).

---

### Task 1: HideVocabPopup action + sticky popup (remove auto-hide)

**Files:**
- Modify: `src/input/actions/mod.rs` (enum after `VocabPopupPrev` :110; `category()` Vocab group :250-253; `name()` :388-392)
- Modify: `src/input/keymap.rs` (`handle_vocab_popup_key` :3485-3527; dispatch arms :3110-3126)

**Interfaces:**
- Consumes: `state.vocab_popup.popup` (`VocabPopup`: `.is_visible()`, `.widget()`, `.hide()`), `state.vocab_popup.fade_gen: Rc<Cell<u64>>` (vocab_popup.rs:11-19).
- Produces: `Action::HideVocabPopup` (serde name `"HideVocabPopup"` — parse_action picks it up automatically); `fn fade_out_vocab_popup(state: &Rc<RefCell<AppState>>)` in keymap.rs.

- [ ] **Step 1: Enum + category + name arms**

```rust
    VocabPopupPrev,
    HideVocabPopup,
```

`category()`: extend the Vocab group: `| Action::HideVocabPopup` alongside `VocabPopupNext`/`VocabPopupPrev`. `name()`: `Action::HideVocabPopup => "HideVocabPopup",`.

- [ ] **Step 2: Strip the timer; add the fade helper**

`handle_vocab_popup_key` becomes ONLY the open/advance logic — delete everything from `let gen = {` through the end of the `timeout_add_local_once` closure, but KEEP a `fade_gen` bump so any in-flight legacy timer from before the change is invalidated:

```rust
/// Vocab popup key handler. The popup is STICKY: it stays visible until
/// HideVocabPopup (Ctrl+-) or the H toggle dismisses it — no auto-hide.
fn handle_vocab_popup_key(state: &Rc<RefCell<AppState>>, forward: bool) {
    let popup_visible = state.borrow().vocab_popup.popup.is_visible();
    if popup_visible {
        if forward {
            crate::app::vocab_popup::vocab_popup_next(&mut state.borrow_mut());
        } else {
            crate::app::vocab_popup::vocab_popup_prev(&mut state.borrow_mut());
        }
    } else {
        crate::app::vocab_popup::open_vocab_popup(&mut state.borrow_mut());
    }
    // Invalidate any pending fade (defensive; nothing arms one anymore).
    let s = state.borrow();
    s.vocab_popup.fade_gen.set(s.vocab_popup.fade_gen.get() + 1);
}

/// Ctrl+-: fade the vocab popup out (500ms, EaseOutQuad — the same animation
/// the old auto-hide used). Idempotent: no-op when the popup isn't visible.
fn fade_out_vocab_popup(state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    s.vocab_popup.fade_gen.set(s.vocab_popup.fade_gen.get() + 1);
    if !s.vocab_popup.popup.is_visible() {
        return;
    }
    let widget = s.vocab_popup.popup.widget().clone();
    let target = adw::CallbackAnimationTarget::new(move |value| {
        widget.set_opacity(value as f64);
        if value <= 0.0 {
            widget.set_visible(false);
            widget.set_opacity(1.0);
        }
    });
    let anim = adw::TimedAnimation::new(s.vocab_popup.popup.widget(), 1.0, 0.0, 500, target);
    anim.set_easing(adw::Easing::EaseOutQuad);
    anim.play();
}
```

- [ ] **Step 3: Dispatch arm** — next to the `VocabPopupNext`/`VocabPopupPrev` arms (keymap.rs:3119-3126):

```rust
        HideVocabPopup => {
            fade_out_vocab_popup(state);
        }
```

- [ ] **Step 4: Build** — `cargo build` → no errors (`name()`/`category()` matches are exhaustive; a missed arm fails the build here).
- [ ] **Step 5: Commit** — `git add src/input && git commit -m "feat: HideVocabPopup action; vocab popup no longer auto-hides"`

### Task 2: Rebind defaults + stow keymap.json + tests

**Files:**
- Modify: `src/input/keymap_config.rs` (:227-228 z, :281-283 minus, :389-391 ctrl+minus; test `tab_toggles_chat_layout_and_minus_toggles_pause` :444-450)
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (:67-68 minus entries, :126 z entry)

**Interfaces:**
- Consumes: `Action::HideVocabPopup` from Task 1.

- [ ] **Step 1: Update the failing test FIRST**

```rust
    #[test]
    fn minus_cycles_vocab_and_ctrl_minus_hides_popup() {
        let m = default_reader_bindings();
        assert_eq!(m.get(&KeyCombo::plain("Tab")), Some(&Action::ToggleChatLayout));
        assert_eq!(m.get(&KeyCombo::plain("minus")), Some(&Action::VocabPopupNext));
        assert_eq!(m.get(&KeyCombo::ctrl("minus")), Some(&Action::HideVocabPopup));
        // z freed; # keeps prev; pause still reachable on a.
        assert_eq!(m.get(&KeyCombo::plain("z")), None);
        assert_eq!(m.get(&KeyCombo::plain("numbersign")), Some(&Action::VocabPopupPrev));
        assert_eq!(m.get(&KeyCombo::plain("a")), Some(&Action::TogglePause));
    }
```

(replaces `tab_toggles_chat_layout_and_minus_toggles_pause`). Run: `cargo test --bins minus_cycles` → FAIL (old bindings).

- [ ] **Step 2: Flip the compiled defaults**

- Delete `:227-228` (`// z steps forward...` + `(KeyCombo::plain("z"), Action::VocabPopupNext),`).
- `:281-283` becomes:

```rust
        // '-' cycles the vocab popup (sticky — no auto-hide; Ctrl+- hides).
        (KeyCombo::plain("minus"), Action::VocabPopupNext),
```

- `:390` becomes `(KeyCombo::ctrl("minus"), Action::HideVocabPopup),` (OpenRecentPicker left unbound by design).

- [ ] **Step 3: Test passes** — `cargo test --bins minus_cycles` → PASS; full `cargo test --bins` → 761-equivalent pass / 1 known fail (adjust count for the renamed test).

- [ ] **Step 4: Stow keymap.json (same change!)** — in `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`: line 67 → `{"key": "minus", "action": "VocabPopupNext"},`; line 68 → `{"key": "minus", "ctrl": true, "action": "HideVocabPopup"},`; DELETE line 126 (`z` entry — leaving it would re-bind z on top of the removed default).

- [ ] **Step 5: Commits (two repos)**

```bash
cd ~/utono/linux-lit && git add src/input/keymap_config.rs && git commit -m "feat: minus cycles vocab (sticky), Ctrl+minus hides; z and recent-picker unbound"
cd ~/tty-dotfiles && git add linux-lit/.config/linux-lit/keymap.json && git commit -m "linux-lit: minus vocab cycle + Ctrl+minus hide; drop z binding"
```

### Task 3: Ctrl+/ keybinds overlay

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (HOME_ROW `-` KeyDef :79, BOTTOM_ROW `z` KeyDef :93, `describe()` arms :398-404/:422-423/:299-300, short-label match :601-638)

- [ ] **Step 1: KeyDefs**

- `:79` → `key("-", "_", "vocab ▶", "", &[("C--", "hide vocab")]),`
- `:93` → `bare("z", "Z", ""),` (freed key renders a blank slot — verify another freed key uses this convention; follow it).

- [ ] **Step 2: describe() arms**

- Add: `"hide vocab" => "Fade the vocabulary popup out (it no longer auto-hides). -> fade_out_vocab_popup — src/input/keymap.rs",`
- Update `"play/pause"` (:422-423): drop the "a and - both bind this" claim → `"Play/pause without seeking (unlike Space; bound on a). -> TogglePause arm -> MpvCommand::TogglePause — src/input/keymap.rs",`
- Update `"vocab ▶"` text to mention stickiness: `"Next word in the vocabulary popup (stays up; Ctrl+- hides). -> handle_vocab_popup_key(.., true) — src/input/keymap.rs",`
- Delete the now-orphaned `"recent"` arm (:299-300) — no KeyDef references it anymore.
- Short-label match: add `"hide vocab" => "hide vocab popup",`.

- [ ] **Step 3: Three-pass cross-check (update-cairo-keybinds-overlay discipline)** — (1) no blank slot hides a real binding (z is genuinely unbound now — blank is CORRECT; `-` slot shows the real new binding); (2) no label names a wrong action; (3) every label has a describe() arm (`rg '"hide vocab"|"vocab ▶"|"recent"' src/ui/keybinds_overlay.rs` — "recent" must have zero hits).

- [ ] **Step 4: Build + commit** — `cargo build`; `git add src/ui/keybinds_overlay.rs && git commit -m "docs(overlay): minus=vocab cycle, Ctrl+minus=hide, z freed"`

### Task 4: Headless e2e + to-do checkoff

**Files:**
- Modify: `docs/to-do/to-do.md` (mark the `-`/vocab item `[X]`)

- [ ] **Step 1: Drive it** (CLAUDE.md Headless Verification, `LIT_DEV=1 dbus-run-session`, work with vocab data e.g. a Shakespeare play):
  1. `wtype -k minus` ×2 → popup visible, word advanced (log `ACTION: VocabPopupNext`, no `TogglePause`).
  2. Wait 5s → `grim` → popup STILL visible (auto-hide gone).
  3. `wtype -M ctrl -k minus -m ctrl` → sleep 1 → `grim` → popup gone (log `ACTION: HideVocabPopup`).
  4. `wtype "z"` → no `ACTION:` line for z in the log.
- [ ] **Step 2: Review PNGs inline; mark to-do; commit**

```bash
git add docs/to-do/to-do.md && git commit -m "docs: mark sticky vocab popup to-do done"
```
