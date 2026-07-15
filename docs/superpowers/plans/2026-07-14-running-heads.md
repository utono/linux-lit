# Running Heads Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the bottom-center act/scene toast with an always-visible, book-style running-head strip across the top of the reading card, on ALL works — work abbrev at top-left, position (act/scene for plays, chapter for prose) at top-right, with a hairline rule beneath it.

**Architecture:** The card's existing `top_spacer` (a 40px full-width box already sitting above both columns, with the `card-top` background and rounded top corners) becomes a horizontal container holding two labels. Both labels are fed by a new `update_running_heads(state)` function called at the same two `update_highlight` exit points that already call `update_title_bar_scene`, plus on work load. The position label reuses the existing `compute_current_chapter_text` (which already produces `Act N, Scene M` for plays and `Chapter N of M — title` / `Front matter — title` for prose) by splitting off its leading `"{abbrev} — "` prefix — one source of truth, no re-derivation. The old bottom `chapter_toast` widget stays in place for transient system messages (Sync:, Rewriting…, copy toasts) but is no longer used as the persistent act/scene or chapter indicator.

**Tech Stack:** Rust, GTK4 (gtk4-rs), existing `scene_synopsis::scene_label_for` / `navigation::compute_current_chapter_text` helpers, CSS in `theme.rs::generate_css`.

## Global Constraints

- Layout is Real Programmers Dvorak — irrelevant here (no new keybinds), but do not touch keymap files.
- Do NOT run `cargo run` — build only (`cargo build`); the user launches the app.
- Boundaries are authoritative metadata: derive act/scene from `current_scene_divs` / `scene_label_for`, never by parsing buffer text.
- Reader theme is independent of the system theme; all CSS lives in `src/theme.rs`.
- The running head shows on **ALL works** — plays, verse, AND prose. It is blank only when no work is loaded.
- Head text sources: left = `work.abbrev`; right = the position string from `compute_current_chapter_text` with its leading `"{abbrev} — "` prefix stripped (plays → `Act 5, Scene 4`; prose → `Chapter 3 of 67 — In Chancery` or `Front matter — <title>`). Reuse that function; do NOT re-derive act/scene or chapter numbering independently.

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
    /// band). `running_head_work` is the work abbrev (left); `running_head_scene`
    /// is the position label (right) — act/scene for plays, chapter for prose.
    /// Both are refreshed on every cursor move and on work load via
    /// `scene_synopsis::update_running_heads`, on ALL works. Blank only when no
    /// work is loaded. Replaces the persistent bottom-center position toast.
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
    // running-head strip: work abbrev at the start, position (act/scene or
    // chapter) at the end, with a hairline rule (CSS border-bottom) separating
    // it from the reading text.
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

### Task 3: Add `update_running_heads` (feeds both labels, all works)

**Files:**
- Modify: `src/app/scene_synopsis.rs:566-577` (add a sibling function to `update_title_bar_scene`)

**Interfaces:**
- Consumes: `AppState`, `state.current_work`, `navigation::compute_current_chapter_text(state) -> String` (already exists — returns `"{abbrev} — <position>"`, e.g. `Cym-Arkangel — Act 5, Scene 4` or `Bleak-House — Chapter 3 of 67 — In Chancery`; returns `""` when no work).
- Produces: `pub fn update_running_heads(state: &AppState)` — sets BOTH `running_head_work` (abbrev, left) and `running_head_scene` (position, right) for ALL works; blanks both when no work is loaded. Called by highlight.rs (Task 4) and display_work (Task 5).

- [ ] **Step 1: Write the function**

In `src/app/scene_synopsis.rs`, immediately after `update_title_bar_scene` (after line ~577), add:

```rust
/// Refresh the running-head strip from the cursor's authoritative position.
/// Left label = work abbrev; right label = the position string (act/scene for
/// plays, `Chapter N of M — title` / `Front matter — title` for prose). Both
/// come from `navigation::compute_current_chapter_text`, which already encodes
/// the play-vs-prose distinction from authoritative `(div1, div2)` metadata —
/// we split off its leading `"{abbrev} — "` so the work and position sit on
/// opposite ends of the strip. Blanks both labels when no work is loaded.
/// Runs on every cursor move (see highlight.rs) AND on work load.
pub fn update_running_heads(state: &AppState) {
    let abbrev = match state.current_work.as_ref() {
        Some(w) => w.abbrev.clone(),
        None => {
            state.running_head_work.set_text("");
            state.running_head_scene.set_text("");
            return;
        }
    };
    // `compute_current_chapter_text` returns "{abbrev} — <position>". Strip the
    // "{abbrev} — " prefix to get just the position for the right label; the
    // separator is " — " (space, em-dash U+2014, space).
    let full = crate::input::navigation::compute_current_chapter_text(state);
    let prefix = format!("{} — ", abbrev);
    let position = full.strip_prefix(&prefix).unwrap_or(full.as_str());
    state.running_head_work.set_text(&abbrev);
    state.running_head_scene.set_text(position);
}
```

- [ ] **Step 2: Build to verify it compiles**

Run: `cargo build`
Expected: compiles, with a `function is never used` dead-code warning for `update_running_heads` (removed in Task 4 when call sites are added). The warning is acceptable at this task boundary. If `compute_current_chapter_text` is not `pub(crate)`-visible from this module, widen its visibility to `pub(crate)` (it is currently `pub(crate)` per navigation.rs — confirm and, if narrower, widen it in this same commit).

- [ ] **Step 3: Commit**

```bash
git add src/app/scene_synopsis.rs
git commit -m "feat(running-heads): add update_running_heads (both labels, all works)"
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

### Task 5: Populate the running head on work load

**Files:**
- Modify: `src/app/mod.rs` (display_work path — locate where the current work becomes active)

**Interfaces:**
- Consumes: `scene_synopsis::update_running_heads(state)` (Task 3) — sets both labels for the now-current work.

**Why:** `update_running_heads` already fills BOTH labels for all works (Task 3). The cursor-move path (Task 4) keeps them fresh thereafter, but the FIRST paint after a work loads happens before any cursor move, so call it once at load so the strip is correct immediately (and so a work with no subsequent nav still shows its head).

- [ ] **Step 1: Find the display_work render/finish point**

Run: `rg -n "fn display_work\b|current_work = Some|self.current_work = |state.current_work = |update_title_bar_scene" src/app/mod.rs`
Expected: identifies `display_work` and where `current_work` becomes the loaded work. The call must go AFTER `current_work` is assigned (so `is_prose()` inside `update_running_heads` is accurate) and after the buffer/line state is set (so `compute_current_chapter_text` reads the right cursor line). If `display_work` already calls `update_title_bar_scene` or `update_highlight` near its end, place the new call right beside that — same lifecycle point.

- [ ] **Step 2: Call update_running_heads once at load**

Immediately after the point identified in Step 1 (after `current_work` is set and the initial cursor/line state is established), add:

```rust
    // Populate the running-head strip for the freshly-loaded work (both labels).
    // Cursor-move updates keep it fresh afterward; this covers the first paint.
    crate::app::scene_synopsis::update_running_heads(self);
```

Adjust `self` vs `state` to match the receiver in `display_work` (check the signature from Step 1 — a free function takes `state: &mut AppState`, so use `state`). If the surrounding code holds a `&AppState` (not `&mut`), `update_running_heads` takes `&AppState` so either works.

- [ ] **Step 3: Build to verify it compiles**

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 4: Commit**

```bash
git add src/app/mod.rs
git commit -m "feat(running-heads): populate the head strip on work load"
```

---

### Task 6: Retire the persistent bottom toast (the running head replaces it)

**Files:**
- Modify: `src/input/navigation.rs` (`surface_current_scene_toast`, `show_current_chapter`, `refresh_persistent_chapter_toast`)

**Rationale:** The running head is now the always-visible position indicator for exactly the works that previously drove the persistent bottom toast — `chapter_toast_persists()` is true for plays AND prose-with-chapters (confirmed: `AppState::chapter_toast_persists` at `src/app/mod.rs:799` returns true for `is_play()` and for prose with chapter markers). So the persistent bottom toast is now fully redundant and must be retired for all those works. The bottom `chapter_toast` **widget must remain** — transient system messages (Sync:, copy toasts, Rewriting…) still borrow it via `show_transient_over_chapter_toast` / the borrow mechanism. Only its use as the *persistent position indicator* goes away. Works that never persisted the toast (front-matter-only prose, non-play verse, anthology — `chapter_toast_persists()` false) are unchanged: they still get their transient one-off toast, which is harmless alongside the head.

**Interfaces:**
- Consumes: `state.chapter_toast_persists()`, `state.chapter_toast_persistent` (Cell<bool>), existing toast functions.

- [ ] **Step 1: Read the three functions and confirm the flow**

Run: `rg -n "fn surface_current_scene_toast|fn show_current_chapter|fn refresh_persistent_chapter_toast" src/input/navigation.rs`
Then read all three plus `AppState::chapter_toast_persists` (`src/app/mod.rs:799`) and the `chapter_toast_persistent` field docs. Confirm: the persistent indicator is gated on `chapter_toast_persistent.get()`, set true only in `show_current_chapter`'s persist branch, and refreshed by `refresh_persistent_chapter_toast` on cursor move. Retiring it = never setting that flag true + making the refresh a no-op.

- [ ] **Step 2: Make `show_current_chapter`'s persist branch a no-op (never arm the flag)**

In `show_current_chapter` (line ~2495), the persist branch (`if state.chapter_toast_persists() { ... }`) toggles `chapter_toast_persistent`. Replace that whole branch so persisting works do nothing on `+` (the running head already shows position), while non-persisting works keep falling through to the transient toast below. Change:

```rust
    if state.chapter_toast_persists() {
        // ... existing toggle-on / toggle-off body ...
        return;
    }

    let text = compute_current_chapter_text(state);
    show_chapter_toast(state, &text);
```

to:

```rust
    // The running-head strip is now the always-visible position indicator for
    // every work that used to persist the bottom toast (plays + prose-with-
    // chapters). `+` therefore has nothing to toggle for them — no-op. Works
    // that never persisted (front matter, bare verse, anthology) still get the
    // one-off transient toast below.
    if state.chapter_toast_persists() {
        log_fmt!("SHOW_CHAPTER (+): no-op — running head shows position");
        return;
    }

    let text = compute_current_chapter_text(state);
    show_chapter_toast(state, &text);
```

(Delete the old toggle-on/off body entirely — `chapter_toast_persistent` is never armed now. Keep the `log_fmt!` import already used in this file.)

- [ ] **Step 3: Make `refresh_persistent_chapter_toast` a no-op**

In `refresh_persistent_chapter_toast` (line ~2693), the body re-shows the persistent toast when `chapter_toast_persistent.get()` is true. Since Step 2 means that flag is never armed, this is already dead in practice — but make it explicit and cheap by returning early:

```rust
pub(crate) fn refresh_persistent_chapter_toast(state: &AppState) {
    // Retired: the running-head strip replaced the persistent bottom toast.
    // The transient-toast borrow mechanism no longer needs this refresh.
    let _ = state;
}
```

Keep the function (it is called from `highlight.rs` and the toast-restore path); just neuter its body. If removing the body causes an "unused import"/"unused variable" warning elsewhere in the file, resolve it minimally.

- [ ] **Step 4: Make `surface_current_scene_toast` a no-op**

In `surface_current_scene_toast` (line ~2384) — called from scene/chapter jumps — the persistent works now show position in the head and the non-persistent transient toast is not wanted here either (jumps already move the head). Replace the body:

```rust
pub(crate) fn surface_current_scene_toast(state: &mut AppState) {
    // Retired: the running-head strip shows the current position for every
    // work. Scene/chapter jumps update the head via the cursor-move path, so
    // there is nothing to surface as a bottom toast.
    let _ = state;
}
```

- [ ] **Step 5: Build, tests, clippy**

Run: `cargo build && cargo test --bins 2>&1 | tail -5 && cargo clippy 2>&1 | rg "chapter_toast|surface_current|refresh_persistent|unused" || echo "clippy clean of relevant warnings"`
Expected: compiles; tests pass (a pre-existing `db::queries` hamlet failure, if present, is not this branch's fault — note it, don't fix it); no new clippy warnings on the touched functions. If `chapter_toast_shown` in config / the `chapter_toast_gen`/`chapter_toast_saved`/`chapter_toast_persistent` fields become dead as a result, do NOT delete them in this task (transient-toast borrow still uses gen/saved/borrowed; `persistent`/`shown` going unused is acceptable and flagged for the final review).

- [ ] **Step 6: Commit**

```bash
git add src/input/navigation.rs
git commit -m "feat(running-heads): retire the persistent bottom toast (head replaces it)"
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
- The bottom-center area no longer shows the persistent position toast (transient Sync:/copy toasts still work).

Quote the on-screen text in the report.

- [ ] **Step 3: Tune padding only if misaligned**

If the head labels don't align with the text columns' left/right edges, adjust `.running-head` `padding` in `src/theme.rs` to match the text-column margins observed on screen, rebuild, re-screenshot. Do not change anything else.

- [ ] **Step 4: Confirm prose shows its own running head (chapter position)**

Repeat Step 1 with a prose work (e.g. `LIT_START_WORK=Bleak-House`, or another prose abbrev — list with `rg -n` on a works dump if unsure), screenshot, and confirm the head strip shows the work abbrev top-left and the chapter position top-right (e.g. `CHAPTER 3 OF 67 — IN CHANCERY`, or `FRONT MATTER — <title>` in the opening). Confirm the in-text chapter heading is unaffected and there is no bottom position toast. Quote the on-screen text.

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

- **Spec coverage:** always-visible on ALL works (Task 1 packs into the always-present `top_spacer`; Tasks 3-5 keep both labels fed on plays AND prose) ✓; reserved strip above both columns (Task 1 uses `top_spacer`, already above `columns_hbox`) ✓; work top-left + position top-right (Task 1 halign Start/End) ✓; hairline rule (Task 2 border-bottom) ✓; single source of truth for the position string via `compute_current_chapter_text` (Task 3) ✓; retire the persistent bottom toast that the head replaces (Task 6) ✓; visual verification on both a play and a prose work (Task 7) ✓.
- **Scope change (user, 2026-07-14):** running head applies to ALL works including prose (prose right label = chapter position), NOT plays-only. Tasks 3, 5, 6, 7 reflect this.
- **No new keybinds**, so overlay legends and keymap.json are untouched — correct per scope.
- **`chapter_toast_persists()` confirmed** (src/app/mod.rs:799) to be true for plays + prose-with-chapters — exactly the works the head now covers, so Task 6 retires the persistent toast for all of them without a play/prose branch.
- **Field naming:** `running_head_scene` holds the position for all works (act/scene OR chapter); the `scene` name is historical/cosmetic and not worth a rename churn. Doc comment says "position".
- **`top_spacer` height:** stays `TOP_SPACER_HEIGHT` (40px) — already reserved, so no reading-height regression beyond what exists today; that space is already spent by the current empty spacer.
