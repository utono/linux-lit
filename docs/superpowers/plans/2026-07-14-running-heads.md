# Running Heads Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the bottom-center act/scene toast with an always-visible, book-style running-head strip across the top of the reading card — work abbrev at top-left, act/scene at top-right, with a hairline rule beneath it.

**Architecture:** The card's existing `top_spacer` (a 40px full-width box already sitting above both columns, with the `card-top` background and rounded top corners) becomes a horizontal container holding two labels. It updates from a new `update_running_heads(state)` function called at the same two `update_highlight` exit points that already call `update_title_bar_scene`, plus on work load. The old bottom `chapter_toast` widget stays in place for transient system messages (Sync:, Rewriting…, copy toasts) but is no longer used for the persistent act/scene indicator on plays.

**Tech Stack:** Rust, GTK4 (gtk4-rs), existing `scene_synopsis::scene_label_for` / `compute_current_chapter_text` helpers, CSS in `theme.rs::generate_css`.

## Global Constraints

- Layout is Real Programmers Dvorak — irrelevant here (no new keybinds), but do not touch keymap files.
- Do NOT run `cargo run` — build only (`cargo build`); the user launches the app.
- Boundaries are authoritative metadata: derive act/scene from `current_scene_divs` / `scene_label_for`, never by parsing buffer text.
- Reader theme is independent of the system theme; all CSS lives in `src/theme.rs`.
- The running head shows on **plays/verse only** (works where `is_prose()` is false). Prose keeps its existing chapter-heading behavior and shows an empty strip (or the work + chapter label — see Task 6).
- Head text sources: left = `work.abbrev`; right = `scene_label_for(state, div1, div2)` (e.g. `Act 5, Scene 4`). Do NOT reuse the em-dash-joined single string.

---

## File Structure

- `src/app/mod.rs` — build the two head labels, restructure `top_spacer` into an hbox, store the labels on `AppState`. The single place the widget tree is assembled.
- `src/app/scene_synopsis.rs` — new `update_running_heads(state)` (right-label refresh) alongside the existing `update_title_bar_scene`; both read the same authoritative divs.
- `src/input/highlight.rs` — call `update_running_heads` at the two exit points that already call `update_title_bar_scene`.
- `src/theme.rs` — CSS for `.running-head` container + `.running-head-work` / `.running-head-scene` labels + the hairline.
- (Work-load left-label set) `src/app/mod.rs` display_work path — set the left (work) label once when a work loads.

Each task ends with `cargo build` succeeding; the visible result is verified with the headless cage harness at the end (Task 7).

---

### Task 1: Add head-label fields to AppState and build the strip

**Files:**
- Modify: `src/app/mod.rs:300` (AppState `top_spacer` field — add two sibling fields)
- Modify: `src/app/mod.rs:1393-1396` (top_spacer construction)
- Modify: `src/app/mod.rs:1808` (AppState struct literal — add the two new fields)

**Interfaces:**
- Produces: `AppState.running_head_work: gtk4::Label`, `AppState.running_head_scene: gtk4::Label` — later tasks set their text. `top_spacer` remains the `gtk4::Box` but is now horizontal with these two labels as children.

- [ ] **Step 1: Add the two label fields to the AppState struct**

In `src/app/mod.rs`, immediately after the `pub top_spacer: gtk4::Box,` field (line ~300), add:

```rust
    pub top_spacer: gtk4::Box,
    /// Running-head strip labels living inside `top_spacer` (the card's top
    /// band). `running_head_work` is the work abbrev (left, set on work load);
    /// `running_head_scene` is the act/scene label (right, refreshed on every
    /// cursor move via `scene_synopsis::update_running_heads`). Plays/verse
    /// only — blanked for prose. Replaces the bottom-center act/scene toast.
    pub running_head_work: gtk4::Label,
    pub running_head_scene: gtk4::Label,
```

- [ ] **Step 2: Build the labels and pack them into `top_spacer`**

In `src/app/mod.rs`, replace the `top_spacer` construction block (currently lines ~1393-1396):

```rust
    // Top spacer — one line height, rounded top corners only
    let top_spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    top_spacer.set_hexpand(true);
    top_spacer.set_height_request(TOP_SPACER_HEIGHT);
    top_spacer.add_css_class("card-top");
```

with:

```rust
    // Top spacer — one line height, rounded top corners only. Doubles as the
    // running-head strip: work abbrev at the start, act/scene at the end, with
    // a hairline rule (CSS border-bottom) separating it from the reading text.
    let top_spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    top_spacer.set_hexpand(true);
    top_spacer.set_height_request(TOP_SPACER_HEIGHT);
    top_spacer.add_css_class("card-top");
    top_spacer.add_css_class("running-head");

    let running_head_work = gtk4::Label::new(None);
    running_head_work.set_halign(gtk4::Align::Start);
    running_head_work.set_valign(gtk4::Align::Center);
    running_head_work.set_hexpand(true);
    running_head_work.add_css_class("running-head-work");

    let running_head_scene = gtk4::Label::new(None);
    running_head_scene.set_halign(gtk4::Align::End);
    running_head_scene.set_valign(gtk4::Align::Center);
    running_head_scene.set_hexpand(true);
    running_head_scene.add_css_class("running-head-scene");

    top_spacer.append(&running_head_work);
    top_spacer.append(&running_head_scene);
```

- [ ] **Step 3: Add the two fields to the AppState struct literal**

In `src/app/mod.rs`, find the struct literal line `        top_spacer,` (~line 1808) and add the two new fields right after it:

```rust
        top_spacer,
        running_head_work,
        running_head_scene,
```

- [ ] **Step 4: Build to verify it compiles**

Run: `cargo build`
Expected: compiles clean (labels are unused-but-stored — no dead-code error because they are public struct fields).

- [ ] **Step 5: Commit**

```bash
git add src/app/mod.rs
git commit -m "feat(running-heads): add head-strip labels to the card top spacer"
```

---

### Task 2: Style the running-head strip

**Files:**
- Modify: `src/theme.rs:938` (near `.card-top`) — add `.running-head*` rules to the `generate_css` format string.

**Interfaces:**
- Consumes: the `{bg}`, `{dim}`, `{divider}` (or equivalent) format args already bound in `generate_css`. Verify the exact arg names bound at the `format!` call before referencing them (search `generate_css` for `dim =` / `divider =` / `bg =`).

- [ ] **Step 1: Confirm available CSS format args**

Run: `rg -n "bg = |dim = |divider = |root = " src/theme.rs`
Expected: shows which identifiers are bound in the `generate_css` format call (e.g. `dim`, `bg`). Use `{dim}` for the faint head text (it is what `.title-bar-hint` uses). If a hairline color arg like a divider/border color is not already bound, use a literal `rgba(0,0,0,0.12)` for the border so no new format arg is needed.

- [ ] **Step 2: Add the CSS rules**

In `src/theme.rs`, immediately after the `.card-top` rule (line ~938):

```rust
         .card-top {{ background-color: {bg}; border-radius: 12px 12px 0 0; }} \
```

add:

```rust
         .running-head {{ border-bottom: 1px solid rgba(0, 0, 0, 0.12); \
           padding: 0 30px; }} \
         .running-head-work {{ color: {dim}; font-size: 11px; \
           font-variant: small-caps; letter-spacing: 1px; opacity: 0.7; }} \
         .running-head-scene {{ color: {dim}; font-size: 11px; \
           font-variant: small-caps; letter-spacing: 1px; opacity: 0.7; }} \
```

Note: `padding: 0 30px` aligns the head labels with the text columns' left margin (columns use `text_margins`, default 30; the head sits above them). If the text starts noticeably out of alignment on screen, adjust this padding in Task 7 after visual review — do NOT guess a different value now.

- [ ] **Step 3: Build to verify it compiles**

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 4: Commit**

```bash
git add src/theme.rs
git commit -m "feat(running-heads): style the head strip (small-caps, hairline rule)"
```

---

### Task 3: Add `update_running_heads` (right-label refresh)

**Files:**
- Modify: `src/app/scene_synopsis.rs:566-577` (add a sibling function to `update_title_bar_scene`)

**Interfaces:**
- Consumes: `AppState`, `state.current_work`, `state.is_prose()`, `current_scene_divs`, `scene_label_for` — all already in scope in this module (`update_title_bar_scene` uses them).
- Produces: `pub fn update_running_heads(state: &AppState)` — sets `running_head_scene` text (act/scene) for plays; blanks it for prose or when no work is loaded. Called by highlight.rs (Task 4) and display_work (Task 5).

- [ ] **Step 1: Write the function**

In `src/app/scene_synopsis.rs`, immediately after `update_title_bar_scene` (after line ~577), add:

```rust
/// Refresh the running-head strip's act/scene label (right side) from the
/// cursor's authoritative `(div1, div2)`. Plays/verse only; prose and the
/// no-work state blank it. The left (work) label is set once on work load
/// (see display_work), so this only touches the scene side. Rides the same
/// per-navigation sites as `update_title_bar_scene`.
pub fn update_running_heads(state: &AppState) {
    let is_play = state
        .current_work
        .as_ref()
        .map(|_| !state.is_prose())
        .unwrap_or(false);
    if !is_play {
        state.running_head_scene.set_text("");
        return;
    }
    let (div1, div2) = current_scene_divs(state);
    let label = scene_label_for(state, div1, div2);
    state.running_head_scene.set_text(&label);
}
```

- [ ] **Step 2: Build to verify it compiles**

Run: `cargo build`
Expected: compiles, with a `function is never used` dead-code warning for `update_running_heads` (removed in Task 4 when call sites are added). The warning is acceptable at this task boundary.

- [ ] **Step 3: Commit**

```bash
git add src/app/scene_synopsis.rs
git commit -m "feat(running-heads): add update_running_heads scene-label refresh"
```

---

### Task 4: Wire the refresh into the cursor-move path

**Files:**
- Modify: `src/input/highlight.rs:446` and `src/input/highlight.rs:487` (both `update_highlight` exit points)

**Interfaces:**
- Consumes: `scene_synopsis::update_running_heads` (Task 3).

- [ ] **Step 1: Add the call at the early-return exit (visual-selection path)**

In `src/input/highlight.rs`, at the block ending around line 446-448:

```rust
        crate::app::scene_synopsis::update_title_bar_scene(state);
        crate::input::navigation::refresh_persistent_chapter_toast(state);
        return;
```

insert the running-heads call right after `update_title_bar_scene`:

```rust
        crate::app::scene_synopsis::update_title_bar_scene(state);
        crate::app::scene_synopsis::update_running_heads(state);
        crate::input::navigation::refresh_persistent_chapter_toast(state);
        return;
```

- [ ] **Step 2: Add the call at the normal exit**

At the end of `update_highlight` (around line 487-488):

```rust
    crate::app::scene_synopsis::update_title_bar_scene(state);
    crate::input::navigation::refresh_persistent_chapter_toast(state);
```

insert:

```rust
    crate::app::scene_synopsis::update_title_bar_scene(state);
    crate::app::scene_synopsis::update_running_heads(state);
    crate::input::navigation::refresh_persistent_chapter_toast(state);
```

- [ ] **Step 3: Build to verify it compiles (no dead-code warning now)**

Run: `cargo build`
Expected: compiles clean; the Task 3 dead-code warning is gone.

- [ ] **Step 4: Commit**

```bash
git add src/input/highlight.rs
git commit -m "feat(running-heads): refresh the scene head on every cursor move"
```

---

### Task 5: Set the work (left) label on work load

**Files:**
- Modify: `src/app/mod.rs` (display_work path — locate where the current work becomes active)

**Interfaces:**
- Consumes: `AppState.running_head_work`, `state.current_work`, `state.is_prose()`.

- [ ] **Step 1: Find the display_work assignment point**

Run: `rg -n "fn display_work\b|current_work = Some|self.current_work = |state.current_work = " src/app/mod.rs`
Expected: identifies the function and the line where `current_work` is set to the loaded work. The left label must be set AFTER `current_work` is assigned (so `is_prose()` is accurate).

- [ ] **Step 2: Set the work label after the work becomes current**

Immediately after `current_work` is assigned in `display_work` (exact line from Step 1), add:

```rust
    // Running-head left label: work abbrev for plays/verse; blank for prose
    // (prose keeps its in-text chapter heading).
    if let Some(w) = self.current_work.as_ref() {
        if !self.is_prose() {
            self.running_head_work.set_text(&w.abbrev);
        } else {
            self.running_head_work.set_text("");
        }
    } else {
        self.running_head_work.set_text("");
    }
```

Adjust `self.` vs `state.` to match the receiver name in `display_work` (check the function signature from Step 1 — it may be a free function taking `state: &mut AppState`, in which case use `state.`).

- [ ] **Step 3: Build to verify it compiles**

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 4: Commit**

```bash
git add src/app/mod.rs
git commit -m "feat(running-heads): set the work-abbrev head label on work load"
```

---

### Task 6: Stop the bottom toast from showing the persistent act/scene on plays

**Files:**
- Modify: `src/input/navigation.rs` (`surface_current_scene_toast`, `show_current_chapter`, `refresh_persistent_chapter_toast`)

**Rationale:** With the running head always visible for plays, the bottom-center persistent act/scene toast is now redundant on plays. The bottom `chapter_toast` widget must remain for transient system messages (Sync:, copy toasts, Rewriting…) via the borrow mechanism — do NOT delete the widget. Only stop it from being used as the *persistent act/scene indicator on plays*. Prose (chapter toasts) is unaffected.

**Interfaces:**
- Consumes: `state.is_prose()`, existing toast functions.

- [ ] **Step 1: Confirm the play-vs-prose split in the toast path**

Run: `rg -n "fn surface_current_scene_toast|fn show_current_chapter|fn refresh_persistent_chapter_toast|chapter_toast_persists" src/input/navigation.rs`
Expected: shows the three functions and the `chapter_toast_persists()` predicate. Read `chapter_toast_persists` and `refresh_persistent_chapter_toast` to confirm plays currently drive the persistent toast.

- [ ] **Step 2: Make `surface_current_scene_toast` a no-op for plays**

In `src/input/navigation.rs`, in `surface_current_scene_toast` (line ~2384), change the early prose guard so plays ALSO return early (the running head now covers plays), leaving only prose-without-chapters to fall through if it ever used this path:

```rust
pub(crate) fn surface_current_scene_toast(state: &mut AppState) {
    // Plays/verse: the running-head strip is the always-visible act/scene
    // indicator, so the bottom toast no longer surfaces act/scene here.
    // Prose keeps its chapter-heading behavior; nothing to surface.
    if state.is_prose() || !state.is_prose() {
        return;
    }
    // (unreachable) retained shape for future non-play, non-prose works
    let text = compute_current_chapter_text(state);
    show_chapter_toast(state, &text);
}
```

NOTE: `state.is_prose() || !state.is_prose()` is always true — this is a deliberate "return unconditionally" written so the intent (both branches covered) is explicit. If clippy flags it (`logic_bug`/`nonminimal_bool`), replace the whole body with a single `return;` and a comment. Run `cargo clippy` in Step 4 and apply whichever form clippy accepts.

- [ ] **Step 3: Neutralize the persistent-toast toggle for plays in `show_current_chapter`**

In `show_current_chapter` (line ~2495), the `+` key toggles the persistent toast. For plays this is now redundant. Wrap the persistent branch so plays skip it (prose-with-chapters keeps it). Change the `if state.chapter_toast_persists() {` block to first check prose:

```rust
    // Plays/verse now show act/scene in the always-on running head, so `+`
    // has nothing to toggle for them. Prose-with-chapters keeps the toggle.
    if state.chapter_toast_persists() && state.is_prose() {
```

Verify `chapter_toast_persists()` returns true for both plays and prose-with-chapters before this change (Step 1) — if it is play-only, this `&& state.is_prose()` would disable it entirely, which is wrong; in that case instead make the whole `show_current_chapter` return early for plays. Decide based on the Step 1 reading and note which you did in the commit message.

- [ ] **Step 4: Verify the persistent-refresh path also skips plays**

Read `refresh_persistent_chapter_toast` (line ~2693). If it recomputes and re-shows the act/scene toast for plays, add a guard at its top so plays are a no-op (the running head handles them):

```rust
pub(crate) fn refresh_persistent_chapter_toast(state: &AppState) {
    // Plays/verse use the running head; only prose-with-chapters refreshes here.
    if !state.is_prose() {
        return;
    }
    // ... existing body ...
```

Only add this guard if the existing body would otherwise show the play act/scene. If it already gates on `chapter_toast_persistent.get()` and that flag is never set for plays after Step 3, this guard is belt-and-suspenders but harmless.

- [ ] **Step 5: Build and clippy**

Run: `cargo build && cargo clippy 2>&1 | rg "running|chapter_toast|nonminimal" || echo "clippy clean of relevant warnings"`
Expected: compiles; no new clippy warnings tied to these functions (fix per Step 2 note if flagged).

- [ ] **Step 6: Commit**

```bash
git add src/input/navigation.rs
git commit -m "feat(running-heads): retire the bottom act/scene toast for plays"
```

---

### Task 7: Headless visual verification and alignment tuning

**Files:**
- Possibly modify: `src/theme.rs` (`.running-head` padding) if the head labels don't align with the text columns.

**Interfaces:** none (verification task).

- [ ] **Step 1: Build and drive the reader headless on a play**

Run (from the project CLAUDE.md headless recipe, resized to production geometry):

```bash
cd ~/utono/linux-lit && cargo build
LIT_DEV=1 LIT_NO_MPV=1 GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 LIT_START_WORK=Cym-Arkangel \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
```

Then (per the headless-drive gotchas): find the fresh wayland socket, `export WAYLAND_DISPLAY`, `wlr-randr --output HEADLESS-1 --custom-mode 1920x1200`, give it ~3s, and screenshot with `grim`. Navigate into a two-column play spread and capture.

- [ ] **Step 2: Open the PNG and verify by eye (UI review protocol)**

Confirm on screen:
- Top-left shows the work abbrev (`CYM-ARKANGEL`), small-caps, faint.
- Top-right shows the act/scene (`ACT 5, SCENE 4`), small-caps, faint.
- A hairline rule sits under the strip, above both columns.
- The reading text starts below the strip with no overlap and no clipping.
- The bottom-center area no longer shows the act/scene toast (transient Sync:/copy toasts still work).

Quote the on-screen text in the report.

- [ ] **Step 3: Tune padding only if misaligned**

If the head labels don't align with the text columns' left/right edges, adjust `.running-head` `padding` in `src/theme.rs` to match the text-column margins observed on screen, rebuild, re-screenshot. Do not change anything else.

- [ ] **Step 4: Confirm prose shows an empty strip (no stray label)**

Repeat Step 1 with a prose work (e.g. `LIT_START_WORK=<a prose abbrev>`), screenshot, confirm the head strip is blank (no abbrev, no scene) and the in-text chapter heading is unaffected.

- [ ] **Step 5: Cleanup and commit any tuning**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
git add src/theme.rs
git commit -m "fix(running-heads): align head labels with text columns" # only if Step 3 changed anything
```

- [ ] **Step 6: Update the clip-prevention doc if any clipping was found/fixed**

If Step 2/3 surfaced any clipping between the new strip and the text, append the failure mode to `docs/troubleshooting/clip-prevention.md` per the project CLAUDE.md requirement.

---

## Self-Review Notes

- **Spec coverage:** always-visible (Task 1 packs into the always-present `top_spacer`; Tasks 3-5 keep it fed) ✓; reserved strip above both columns (Task 1 uses `top_spacer`, already above `columns_hbox`) ✓; work top-left + act/scene top-right (Task 1 halign Start/End) ✓; hairline rule (Task 2 border-bottom) ✓; plays only, prose blank (Tasks 3, 5) ✓; retire bottom toast for plays (Task 6) ✓; visual verification (Task 7) ✓.
- **No new keybinds**, so overlay legends and keymap.json are untouched — correct per scope.
- **Open decision for the implementer (Task 6, Step 3):** confirm from the code reading whether `chapter_toast_persists()` is play+prose or play-only; the guard differs. The task tells them how to decide and what to write either way.
- **`top_spacer` height:** stays `TOP_SPACER_HEIGHT` (40px) — already reserved, so no reading-height regression beyond what exists today; the "cost ~24px" from the design discussion is already spent by the current empty spacer.
