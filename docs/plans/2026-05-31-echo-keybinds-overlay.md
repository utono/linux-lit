# Echo Keybinds Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a static legend overlay (key — action rows) for the echoes-overlay keybinds, opened with Ctrl+/ from the echoes overlay and dismissed by Esc or Ctrl+/ back to the echoes overlay.

**Architecture:** A new `EchoKeybindsOverlay` widget (scrim + container of Label rows), modeled on `concordance_works_picker` and added via `add_overlay` onto `authorship_picker.overlay` — NOT inserted into the reader's size-bearing widget chain. Wired through a new `InputMode::EchoKeybindsOverlay`, a Ctrl+/ open arm in `handle_echoes_overlay_key`, and a small `handle_echo_keybinds_key` close handler.

**Tech Stack:** Rust, GTK4 (Box, Label, Overlay).

**Testing note:** GTK widget + wiring — not unit-testable. Compile-verified per task; behavior confirmed by the user in Task 5 (manual). Do NOT run `cargo run`. The 2 pre-existing `input::viewport::block_atom_tests` failures are known/unrelated.

**CRITICAL layout rule:** The picker/overlay MUST be added via `add_overlay`, NOT inserted into the reader's `attach` chain. Inserting into that chain orphans the reader content and collapses the layout (scrolled_window height stuck at 0, nothing renders). Follow the `concordance_works_picker` pattern exactly.

---

## Reference facts (verified in source)

- `concordance_works_picker` (`src/ui/concordance_works_picker.rs`) is the model: `pub container: GtkBox`, `pub scrim: GtkBox`, both `set_visible(false)` by default; added in app.rs via `authorship_picker.overlay.add_overlay(&concordance_works_picker.scrim)` + `...add_overlay(&concordance_works_picker.container)`.
- CSS classes that exist: `picker-box`, `picker-item-title` (used by echo_line_picker/concordance pickers), `gloss-scrim` (used by gloss_overlay scrim), `library-picker-scrim`/`library-picker`/`library-picker-title` (used by works picker).
- `handle_echoes_overlay_key` (`src/input/keymap.rs:764`): has an `if is_ctrl { match key_name { "Up"=>vol, "Down"=>vol, _=>{} } }` block (lines ~779-790). Add the Ctrl+/ open arm here. The main `match key_name` follows.
- Mode dispatch in `handle_key` (`keymap.rs:77-79`): `InputMode::EchoesOverlay => handle_echoes_overlay_key(...)`, then `GamepadOverlay`, `KeybindsOverlay`. Add the new `EchoKeybindsOverlay` arm here.
- `InputMode` enum (`src/app.rs:54` has `EchoLinePicker`). AppState field `echo_line_picker` at `app.rs:219`; constructed + attached at `app.rs:789-790`; constructor field list includes `echo_line_picker,` at `app.rs:1030`.
- `src/ui/mod.rs` registers `pub mod echo_line_picker;` etc.
- The echoes overlay's actual binds (from `handle_echoes_overlay_key`, verified): `a` play echo, `A` add echo, `n`/`p` next/prev (select+play), `Up`/`Down` reorder, `gg`/`G` first/last, `j`/`k` scroll, `Tab` play source turn, `Return` open work, `c` copy, `s` toggle curate, `R` refresh, `Ctrl+Up`/`Ctrl+Down` volume, `Esc` close.

---

## File Structure

- **Create** `src/ui/echo_keybinds_overlay.rs` — the `EchoKeybindsOverlay` widget.
- **Modify** `src/ui/mod.rs` — register the module.
- **Modify** `src/app.rs` — `InputMode::EchoKeybindsOverlay`, AppState field, construct + attach, constructor field.
- **Modify** `src/input/keymap.rs` — Ctrl+/ open arm, dispatch arm, `handle_echo_keybinds_key`.

---

## Task 1: `EchoKeybindsOverlay` widget

**Files:**
- Create: `src/ui/echo_keybinds_overlay.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create the widget file**

Create `src/ui/echo_keybinds_overlay.rs`:

```rust
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, Orientation};

/// Static legend of the echoes-overlay keybinds, shown over the echoes overlay.
pub struct EchoKeybindsOverlay {
    pub container: GtkBox,
    pub scrim: GtkBox,
}

/// (key, action) rows shown in the legend. Matches handle_echoes_overlay_key.
const BINDS: &[(&str, &str)] = &[
    ("a", "play echo"),
    ("A", "add echo"),
    ("n / p", "next / prev echo"),
    ("↑ / ↓", "reorder (curate)"),
    ("g g / G", "first / last echo"),
    ("j / k", "scroll list"),
    ("Tab", "play source turn"),
    ("Enter", "open echo's work"),
    ("c", "copy echo"),
    ("s", "toggle curate"),
    ("R", "refresh echoes"),
    ("Ctrl+↑ / Ctrl+↓", "volume"),
    ("Esc", "close overlay"),
];

impl EchoKeybindsOverlay {
    pub fn new() -> Self {
        let scrim = GtkBox::builder().hexpand(true).vexpand(true).build();
        scrim.add_css_class("gloss-scrim");
        scrim.set_visible(false);

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .halign(Align::Center)
            .valign(Align::Center)
            .width_request(420)
            .build();
        container.add_css_class("picker-box");
        container.set_visible(false);

        let title = Label::builder()
            .label("Echo keybinds")
            .halign(Align::Start)
            .build();
        title.add_css_class("picker-item-title");
        container.append(&title);

        for (key, action) in BINDS {
            let row = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(12)
                .build();
            let key_label = Label::builder()
                .label(*key)
                .halign(Align::Start)
                .width_chars(16)
                .xalign(0.0)
                .build();
            key_label.add_css_class("picker-item-title");
            let action_label = Label::builder()
                .label(*action)
                .halign(Align::Start)
                .hexpand(true)
                .xalign(0.0)
                .build();
            row.append(&key_label);
            row.append(&action_label);
            container.append(&row);
        }

        Self { container, scrim }
    }

    /// Add the legend onto an outer overlay (NOT a chain link).
    pub fn attach_to(&self, overlay: &gtk4::Overlay) {
        overlay.add_overlay(&self.scrim);
        overlay.add_overlay(&self.container);
    }

    pub fn show(&self) {
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.scrim.set_visible(false);
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }
}
```

- [ ] **Step 2: Register the module**

In `src/ui/mod.rs`, add (near the other `echo_*` entries):

```rust
pub mod echo_keybinds_overlay;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | tail -8`
Expected: builds clean. `never used`/`never constructed` warnings on `EchoKeybindsOverlay` are expected until Task 2/3. Do NOT add `#[allow(...)]`.

- [ ] **Step 4: Commit**

```bash
git add src/ui/echo_keybinds_overlay.rs src/ui/mod.rs
git commit -m "Add EchoKeybindsOverlay legend widget

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: AppState wiring (field, InputMode, attach)

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add the InputMode variant**

In `src/app.rs`, in `enum InputMode`, add immediately after `EchoLinePicker,` (line ~54):

```rust
    EchoLinePicker,
    EchoKeybindsOverlay,
```

- [ ] **Step 2: Add the AppState field**

In `pub struct AppState`, immediately after `pub echo_line_picker: …` (line ~219), add:

```rust
    pub echo_keybinds_overlay: crate::ui::echo_keybinds_overlay::EchoKeybindsOverlay,
```

- [ ] **Step 3: Construct + attach**

In `build_window`, immediately after the echo_line_picker construct+attach (lines ~789-790, `let echo_line_picker = …::new(); authorship_picker.overlay.add_overlay(&echo_line_picker.picker_box);`), add:

```rust
    // Echo keybinds legend (Ctrl+/ in the echoes overlay). add_overlay panel,
    // NOT a chain link (chain insertion collapses the reader layout).
    let echo_keybinds_overlay = crate::ui::echo_keybinds_overlay::EchoKeybindsOverlay::new();
    echo_keybinds_overlay.attach_to(&authorship_picker.overlay);
```

- [ ] **Step 4: Add the constructor field**

In the `AppState { … }` construction, immediately after `echo_line_picker,` (line ~1030), add:

```rust
        echo_line_picker,
        echo_keybinds_overlay,
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build 2>&1 | tail -10`
Expected: builds clean. The `EchoKeybindsOverlay` "never constructed" warning is now gone; `show`/`hide`/`is_visible` are still unused (until Task 3). If a `non-exhaustive match` error appears for `InputMode::EchoKeybindsOverlay`, the compiler will name the match — add an arm there mirroring the simplest sibling (most likely the keymap dispatch, handled in Task 3; if any OTHER match breaks, add an arm and report it).

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "Wire EchoKeybindsOverlay into AppState and the overlay stack

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Keymap wiring (open arm, dispatch, close handler)

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Add the Ctrl+/ open arm**

In `src/input/keymap.rs`, in `handle_echoes_overlay_key`, the `if is_ctrl { match key_name { … } }` block (lines ~779-790) currently has `"Up"`/`"Down"` arms then `_ => {}`. Add a `"slash"` arm immediately before the `_ => {}`:

```rust
            "slash" => {
                let mut s = state.borrow_mut();
                s.echo_keybinds_overlay.show();
                s.input_mode = crate::app::InputMode::EchoKeybindsOverlay;
                return true;
            }
            _ => {}
```

- [ ] **Step 2: Add the dispatch arm**

In `handle_key`'s mode-dispatch match (the `match mode` returning per-mode handlers, line ~77-79), add after the `KeybindsOverlay` arm:

```rust
            crate::app::InputMode::KeybindsOverlay => handle_keybinds_key(state, key_name),
            crate::app::InputMode::EchoKeybindsOverlay => handle_echo_keybinds_key(state, key_name, is_ctrl),
```

(Place it adjacent to the other overlay handler arms; the exact neighbor doesn't matter as long as it's inside the same `match mode`.)

- [ ] **Step 3: Add the close handler**

Add a new function in `src/input/keymap.rs` (place it near `handle_keybinds_key`):

```rust
fn handle_echo_keybinds_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
) -> bool {
    // Esc or Ctrl+/ closes the legend, returning to the echoes overlay.
    if key_name == "Escape" || (is_ctrl && key_name == "slash") {
        let mut s = state.borrow_mut();
        s.echo_keybinds_overlay.hide();
        s.input_mode = crate::app::InputMode::EchoesOverlay;
    }
    true // consume all keys while the legend is up (modal)
}
```

- [ ] **Step 4: Verify it compiles + clippy + tests**

Run: `cargo build 2>&1 | tail -6 && cargo clippy 2>&1 | rg 'keymap.rs|echo_keybinds_overlay.rs|app.rs' | rg -v 'CHUNK_PREROLL|activate_chunk|display_work_at|build_line_map|apply_ab_dim|is_visible' | head && cargo test 2>&1 | tail -5`
Expected: builds clean — the `EchoKeybindsOverlay::show`/`hide` dead_code warnings are now gone (used by the open arm + handler). `is_visible` may remain unused (harmless, matches sibling pickers). No new clippy warnings; tests show only the 2 known pre-existing `block_atom_tests` failures.

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Bind Ctrl+/ to the echo keybinds legend overlay

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Manual verification (user runs the app)

GTK overlay behavior cannot be exercised in `cargo test`. Hand these to the user. IMPORTANT: a stale running instance holds the shared log/DB — the user must ensure no old `linux-lit` is running before launching (check `pgrep -af target/debug/linux-lit`), or the new build won't take over.

- [ ] **Step 1: Build for the user**

Run: `cargo build 2>&1 | tail -3`
Expected: clean build.

- [ ] **Step 2: User reproduction script**

Ask the user to `cargo run` (after killing any stale instance), open a work, open the echoes overlay (`i` or Visual-mode `i`), then:
1. Press `Ctrl+/` → a legend panel appears over a dimmed echoes overlay, listing the echo keybinds.
2. Press `Esc` → legend closes, echoes overlay is back with the same selected echo.
3. Press `Ctrl+/` again to reopen, then `Ctrl+/` once more → it toggles closed (back to echoes overlay).
4. While the legend is up, other echo keys (a, n, Up) do nothing.
5. From normal reading (no echoes overlay), `Ctrl+/` still shows the READER keybinds overlay (unchanged).

---

## Self-Review

**Spec coverage:**
- `EchoKeybindsOverlay` widget (scrim + container, static legend rows, attach_to/show/hide/is_visible) → Task 1. ✓
- Bind list matches handle_echoes_overlay_key (incl. gg/G, j/k) → Task 1 `BINDS`. ✓
- AppState field + InputMode + add_overlay attach (not chain) → Task 2. ✓
- Ctrl+/ open in echoes handler; dispatch arm; Esc/Ctrl+/ close → echoes → Task 3. ✓
- Reader Ctrl+/ unchanged (only the echoes-mode path adds a slash arm; reader uses the keymap lookup) → no task touches the reader path. ✓
- Module registration → Task 1 Step 2. ✓
- Manual verification → Task 4. ✓

**Placeholder scan:** No TBD/TODO; all code blocks complete; commands have expected output. The Task 2 Step 5 note ("if any OTHER match breaks, add an arm") is a contingency for an unlikely non-exhaustive-match error, with the rule stated — not a vague TODO.

**Type consistency:** `EchoKeybindsOverlay` with `container`/`scrim` fields and `new`/`attach_to`/`show`/`hide`/`is_visible` methods — defined in Task 1, used in Tasks 2-3. `InputMode::EchoKeybindsOverlay` (Task 2) used in Task 3. `handle_echo_keybinds_key(state, key_name, is_ctrl)` signature matches its Task-3 call site. Field `echo_keybinds_overlay` (Task 2) used as `s.echo_keybinds_overlay.show()/hide()` (Task 3). ✓
