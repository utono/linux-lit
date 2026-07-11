# Chat Panel Float Over 2-Col Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On two-column works the chat panel floats on top of one reading column (default: the one the cursor is not in), `Ctrl+l` flips sides; single-column works keep the pin-right layout.

**Architecture:** A `ChatPlacement` enum on `AppState` selects Pinned vs FloatLeft/FloatRight. The panel widget is already an `outer_overlay.add_overlay` child (halign Start, valign Center); float mode positions it with `margin_start` + `width_request` over a column's live `compute_bounds` rect and leaves the card margins alone (a new `chat_pinned()` predicate replaces `chat_layout_open` at every `apply_card_sizing` call site). The work-switch regate converts placements instead of closing on 2-col targets.

**Tech Stack:** Rust, gtk4-rs (`compute_bounds`, overlay margins), existing page-table/column-split boundary sources.

## Global Constraints

- Design doc: `docs/plans/2026-07-10-chat-panel-float-2col-design.md`.
- Column boundaries come from `active_page_table`/`column_split` — never re-inferred from buffer text.
- Panel stays an overlay child; nothing enters the size-bearing widget chain.
- Side is session-only; no config persistence.
- All keybind changes update: `keymap_config.rs`, stow `keymap.json`, Ctrl+/ overlay KeyDef + `describe()`.
- Known constants: `CHAT_MIN_PANEL_W=500` (chat.rs:11), `CARD_OUTER_MARGIN=24` (layout.rs:41), `MIN_TWO_COLUMN_COLUMN_WIDTH=760` (mod.rs:987).

---

### Task 1: ChatPlacement enum + chat_pinned() predicate

**Files:**
- Modify: `src/input/actions/chat.rs` (top, near `CHAT_MIN_PANEL_W`)
- Modify: `src/app/mod.rs:318` (field), `:1698` (init), `impl AppState`
- Modify: all `apply_card_sizing(..., chat_open)` call sites: `chat.rs:46` (reapply_card_margins), `mod.rs` tick `:2064`, `:2081`, display_work `~:3076`, `layout.rs:290` (apply_column_layout)

**Interfaces:**
- Produces: `pub(crate) enum ChatPlacement { Pinned, FloatLeft, FloatRight }` (Clone, Copy, PartialEq, Debug); `AppState.chat_placement: ChatPlacement`; `AppState::chat_pinned(&self) -> bool`.

- [ ] **Step 1: Add the enum in chat.rs**

```rust
/// Where the open chat panel sits. Pinned = single-column layout (card pinned
/// right, panel in the freed left space). Float* = two-column layout (panel
/// overlays one reading column; the card is untouched).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ChatPlacement {
    Pinned,
    FloatLeft,
    FloatRight,
}
```

- [ ] **Step 2: AppState field + init + predicate**

After `chat_regate_pending: bool` (mod.rs:318): `pub chat_placement: crate::input::actions::chat::ChatPlacement,`; init `chat_placement: crate::input::actions::chat::ChatPlacement::Pinned,` next to `chat_regate_pending: false` (:1698). In `impl AppState`:

```rust
    /// True only when the chat layout is open in its PINNED (single-column)
    /// form — the only placement where the card yields space to the panel.
    /// Float placements overlay a column and must NOT pin the card, so every
    /// apply_card_sizing site reads this, not chat_layout_open.
    pub fn chat_pinned(&self) -> bool {
        self.chat_layout_open
            && self.chat_placement == crate::input::actions::chat::ChatPlacement::Pinned
    }
```

- [ ] **Step 3: Swap all apply_card_sizing chat args**

Replace `s.chat_layout_open` / `state.chat_layout_open` with `s.chat_pinned()` / `state.chat_pinned()` at the five call sites listed above (grep `apply_card_sizing(` to confirm none is missed).

- [ ] **Step 4: Build + tests + commit**

Run: `cargo build` (no errors), `cargo test --bins 2>&1 | rg "test result"` (761 pass / 1 known fail). Behavior unchanged (placement is always Pinned so far).

```bash
git add -A src && git commit -m "refactor: ChatPlacement enum + chat_pinned() predicate (no behavior change)"
```

### Task 2: Cursor-column detection (pure fn + AppState helper)

**Files:**
- Modify: `src/input/actions/chat.rs` (+ `#[cfg(test)]` module)

**Interfaces:**
- Consumes: `page_table::active_page_table(&AppState) -> Option<Rc<Vec<Spread>>>` (page_table.rs:579), `page_table::spread_for_top(&[Spread], top) -> Option<&Spread>` (:593), `Spread { left_start, split: Option<usize>, end }` (:6-11), `viewport::column_split(state, top) -> ColumnSplit` (viewport.rs:1211, fields `split`, `page_end` — confirm exact types at edit time against viewport.rs:1147-1152; the synthesis at scroll.rs:609-627 is the authoritative pattern).
- Produces: `pub(crate) fn line_in_right_column(line: usize, split: Option<usize>, end: usize) -> bool`; `fn cursor_in_right_column(s: &AppState) -> bool`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod placement_tests {
    use super::*;
    #[test]
    fn line_in_right_column_respects_split_and_end() {
        assert!(!line_in_right_column(5, None, 40));          // no right column
        assert!(!line_in_right_column(5, Some(20), 40));      // left side
        assert!(line_in_right_column(20, Some(20), 40));      // first right line
        assert!(line_in_right_column(40, Some(20), 40));      // last line
        assert!(!line_in_right_column(41, Some(20), 40));     // off-page
    }
}
```

- [ ] **Step 2: Run to verify it fails** — `cargo test --bins line_in_right_column` → compile error (function not defined).

- [ ] **Step 3: Implement**

```rust
/// Pure boundary test: is `line` rendered in the RIGHT column of a spread
/// whose right column starts at `split` and whose last line is `end`?
pub(crate) fn line_in_right_column(line: usize, split: Option<usize>, end: usize) -> bool {
    split.is_some_and(|sp| line >= sp && line <= end)
}

/// Which column holds the cursor on the CURRENT spread. Table mode reads the
/// stored spread (authoritative); live mode falls back to column_split. Both
/// are (div1,div2)-derived boundaries — never text inference.
fn cursor_in_right_column(s: &AppState) -> bool {
    let line = s.current_line;
    if let Some(table) = crate::input::page_table::active_page_table(s) {
        if let Some(sp) = crate::input::page_table::spread_for_top(&table, s.page_top_line) {
            return line_in_right_column(line, sp.split, sp.end);
        }
    }
    let cs = crate::input::viewport::column_split(s, s.page_top_line);
    // Live ColumnSplit encodes "no right column" as split > page_end
    // (see the synthesis at scroll.rs:609-627); normalize to Option.
    let split = (cs.split <= cs.page_end).then_some(cs.split);
    line_in_right_column(line, split, cs.page_end)
}
```

(If `spread_for_top`/`column_split` visibility is `pub(crate)`-gated differently, widen to `pub(crate)` rather than duplicating logic.)

- [ ] **Step 4: Test passes** — `cargo test --bins line_in_right_column` → PASS.
- [ ] **Step 5: Commit** — `git add src/input/actions/chat.rs && git commit -m "feat: cursor-column detection for chat panel placement"`

### Task 3: Float geometry + opaque CSS

**Files:**
- Modify: `src/input/actions/chat.rs` (`size_panel`)
- Modify: `src/theme.rs` (`generate_css`, `.chat-panel` block at ~:881)

**Interfaces:**
- Consumes: `main_card_rect(s) -> (card_w, card_h)` (layout.rs:431); `s.scrolled_overlay` / `s.right_scrolled_overlay` (the fixed-width 760px column overlays); gtk4 `WidgetExt::compute_bounds(&target) -> Option<graphene::Rect>`.
- Produces: `size_panel` handling all three placements; CSS class `chat-panel-float`.

- [ ] **Step 1: Rewrite size_panel (chat.rs:412-419)**

```rust
pub(crate) fn size_panel(s: &AppState) {
    use gtk4::prelude::WidgetExt;
    let (card_w, card_h) = crate::app::layout::main_card_rect(s);
    match s.chat_placement {
        ChatPlacement::Pinned => {
            let ww = s.window.width().max(0);
            let end = crate::app::layout::CARD_OUTER_MARGIN;
            // left outer margin (24) + gap to the card (16)
            let w = ww - card_w - end - 24 - 16;
            s.chat_panel.container.set_margin_start(24);
            s.chat_panel.container.remove_css_class("chat-panel-float");
            s.chat_panel.size_to(w, card_h);
        }
        ChatPlacement::FloatLeft | ChatPlacement::FloatRight => {
            let col = if s.chat_placement == ChatPlacement::FloatLeft {
                &s.scrolled_overlay
            } else {
                &s.right_scrolled_overlay
            };
            // Live column rect in window coords: the overlay child's
            // margin_start is relative to the window-filling outer overlay,
            // so bounds.x() maps directly onto it.
            let (x, w) = col
                .compute_bounds(&s.window)
                .map(|b| (b.x() as i32, b.width() as i32))
                .unwrap_or((24, crate::app::MIN_TWO_COLUMN_COLUMN_WIDTH));
            s.chat_panel.container.set_margin_start(x.max(0));
            s.chat_panel.container.add_css_class("chat-panel-float");
            s.chat_panel.size_to(w, card_h);
        }
    }
}
```

(`MIN_TWO_COLUMN_COLUMN_WIDTH` may need `pub(crate)` visibility from mod.rs:987; `compute_bounds` comes from `gtk4::prelude::WidgetExt` — confirm the import name at edit time, it may be `graphene::Rect`-returning `compute_bounds(&impl IsA<Widget>)`.)

- [ ] **Step 2: CSS** — in `generate_css` next to `.chat-panel` (theme.rs:881), using the same `{bg}`/`{fg}` tokens that block's `format!` already interpolates:

```css
.chat-panel-float {{ background-color: {bg}; border: 1px solid alpha({fg}, 0.25); }}
```

(Match the surrounding format-string style exactly; the card background token used by the main card CSS is the one to use — grep the format args in that function.)

- [ ] **Step 3: Build + commit** — `cargo build` clean; `git add src/input/actions/chat.rs src/theme.rs && git commit -m "feat: float geometry + opaque float CSS for chat panel"`

### Task 4: Open/close/regate placement logic

**Files:**
- Modify: `src/input/actions/chat.rs` (`toggle_chat_layout`, `close_chat_layout`, `regate_panel`)

**Interfaces:**
- Consumes: Tasks 1-3 (`ChatPlacement`, `cursor_in_right_column`, three-way `size_panel`).
- Produces: float-aware open, close, regate; the "No room" toast becomes unreachable for 2-col works.

- [ ] **Step 1: toggle_chat_layout** — replace the gate section (chat.rs:124-134):

```rust
    if s.column_count() == 2 {
        // Two-column: float over the column the cursor is NOT in.
        s.chat_placement = if cursor_in_right_column(&s) {
            ChatPlacement::FloatLeft
        } else {
            ChatPlacement::FloatRight
        };
        s.chat_layout_open = true;
        reapply_card_margins(&s); // chat_pinned()==false → card untouched
        size_panel(&s);
        set_panel_header(&s);
        s.chat_panel.show();
        crate::logging::log(&format!("CHAT: layout opened floating ({:?})", s.chat_placement));
        focus_prompt(&mut s);
        return;
    }
    s.chat_placement = ChatPlacement::Pinned;
    // ... existing free-space gate + pinned open path unchanged ...
```

- [ ] **Step 2: close_chat_layout** — after `s.chat_layout_open = false;` add `s.chat_placement = ChatPlacement::Pinned;` and `s.chat_panel.container.remove_css_class("chat-panel-float"); s.chat_panel.container.set_margin_start(24);` (before `reapply_card_margins`).

- [ ] **Step 3: regate_panel** — replace the body's gate with placement conversion:

```rust
pub(crate) fn regate_panel(s: &mut AppState) {
    if !s.chat_layout_open {
        return;
    }
    if s.column_count() == 2 {
        s.chat_placement = if cursor_in_right_column(s) {
            ChatPlacement::FloatLeft
        } else {
            ChatPlacement::FloatRight
        };
        reapply_card_margins(s); // un-pin the card if we arrived from Pinned
        size_panel(s);
        set_panel_header(s);
        crate::logging::log(&format!("CHAT: regate floated panel ({:?})", s.chat_placement));
        return;
    }
    s.chat_placement = ChatPlacement::Pinned;
    s.chat_panel.container.remove_css_class("chat-panel-float");
    reapply_card_margins(s); // pin the card right again
    let ww = s.window.width().max(0);
    let (card_w, _) = crate::app::layout::main_card_rect(s);
    let free = ww - card_w - 2 * crate::app::layout::CARD_OUTER_MARGIN;
    if free < CHAT_MIN_PANEL_W {
        close_chat_layout(s);
        crate::ui::toast::show_transient(&s.chapter_toast, "No room for chat panel at this layout", 3);
        return;
    }
    size_panel(s);
    set_panel_header(s);
    crate::logging::log(&format!("CHAT: regate kept panel (free={}px)", free));
}
```

- [ ] **Step 4: Build + tests + commit** — `cargo build`, `cargo test --bins` (761/1 known). `git add src/input/actions/chat.rs && git commit -m "feat: chat panel floats on 2-col works; regate converts placements"`

### Task 5: Ctrl+l flip action (all four keybind surfaces)

**Files:**
- Modify: `src/input/actions/mod.rs` (enum + `category()` + `name()`)
- Modify: `src/input/keymap_config.rs` (binding + test)
- Modify: `src/input/keymap.rs` (dispatch arm; `handle_chat_prompt_key` :1051, `handle_chat_transcript_key` :1083)
- Modify: `src/input/actions/chat.rs` (`flip_panel_side`)
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`
- Modify: `src/ui/keybinds_overlay.rs` (`l` KeyDef :61, `describe()` :217, short-label match)

**Interfaces:**
- Produces: `Action::ChatPanelFlipSide` (serde name `"ChatPanelFlipSide"`), `chat::flip_panel_side(&mut AppState)`.

- [ ] **Step 1: Action variant** — add `ChatPanelFlipSide,` to the enum near `ToggleChatLayout`; add it to the same `category()` group as `ToggleChatLayout`; add `Action::ChatPanelFlipSide => "ChatPanelFlipSide",` to `name()`.

- [ ] **Step 2: flip handler in chat.rs**

```rust
/// Ctrl+l: flip a floating panel to the other column. No-op when closed or
/// pinned (single-column has no "other side").
pub(crate) fn flip_panel_side(s: &mut AppState) {
    if !s.chat_layout_open {
        return;
    }
    s.chat_placement = match s.chat_placement {
        ChatPlacement::FloatLeft => ChatPlacement::FloatRight,
        ChatPlacement::FloatRight => ChatPlacement::FloatLeft,
        ChatPlacement::Pinned => return,
    };
    size_panel(s);
    crate::logging::log(&format!("CHAT: panel flipped ({:?})", s.chat_placement));
}
```

- [ ] **Step 3: Wire dispatch + modal handlers** — dispatch arm: `ChatPanelFlipSide => crate::input::actions::chat::flip_panel_side(&mut state.borrow_mut()),`. In `handle_chat_prompt_key` add, after the Tab guards and BEFORE `ask_vim_intercept`: `if key_name == "l" && is_ctrl { crate::input::actions::chat::flip_panel_side(&mut state.borrow_mut()); return true; }`. In `handle_chat_transcript_key`'s match add `"l" if is_ctrl => { crate::input::actions::chat::flip_panel_side(&mut s); true }` (match the surrounding borrow pattern).

- [ ] **Step 4: Bindings** — keymap_config.rs: `(KeyCombo::ctrl("l"), Action::ChatPanelFlipSide),` (near the other ctrl binds; confirm ctrl+l is truly unbound with `rg 'ctrl\("l"\)'`). keymap.json (stow source): add `{"key": "l", "ctrl": true, "action": "ChatPanelFlipSide"},`. Add a keymap_config test assertion: `assert_eq!(m.get(&KeyCombo::ctrl("l")), Some(&Action::ChatPanelFlipSide));`.

- [ ] **Step 5: Ctrl+/ overlay** — `l` KeyDef (keybinds_overlay.rs:61) modifiers gain `("C-l", "chat side")`; `describe()` gains `"chat side" => "Flip the floating chat panel to the other reading column (two-column works). -> chat::flip_panel_side — src/input/actions/chat.rs",`; short-label match gains `"chat side" => "flip chat panel column",`. Run the update-cairo-keybinds-overlay skill's three-pass check.

- [ ] **Step 6: Build + tests + commit** — `cargo build`, `cargo test --bins keymap`. `git add -A src ~/tty-dotfiles/linux-lit && git commit -m "feat: Ctrl+l flips floating chat panel side"` (tty-dotfiles is a separate repo — commit there separately).

### Task 6: Headless e2e verification

- [ ] **Step 1: Drive the four scenarios** (per CLAUDE.md Headless Verification; `LIT_DEV=1 dbus-run-session`, 1920x1200):
  1. Open on Hamlet (2-col): `wtype -k Tab` → grim → panel floats over ONE column, other column fully readable, no toast.
  2. `wtype -M ctrl -k l -m ctrl` → grim → panel on the other column.
  3. BH (pinned open) → library-pick Hamlet → grim → panel converted to float (log `CHAT: regate floated panel`), card two-column and uncovered on one side.
  4. Hamlet (float open) → library-pick BH → panel converts to pin (log `CHAT: regate kept panel` or close+toast if narrow).
- [ ] **Step 2: Review every PNG inline** (UI review protocol) and confirm the log lines. Clean up with the cage-scoped pkill only.
