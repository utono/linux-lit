# Gloss-overlay "pick voice & play source verse" (`r` key) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an `r` keybind in the gloss overlay that, **only when the accent-bar cursor is on a Source block**, opens the voice picker; on confirm it sets the picked voice as the gloss's **active voice** and **plays the source verse** in it. If audio is already playing when `r` is pressed, instead **stop and seek the synced MPV media to the verse's first-line start time** (rewound, ready to replay) — no picker.

**Architecture:** Reuse the existing voice picker (`open_voice_picker` / `confirm_voice_picker` in `src/input/actions/settings.rs`) via a NEW `VoicePickerOrigin::GlossPlay` variant so confirm sets the active voice + plays rather than toggling association. The `r` key is a new arm in `handle_gloss_key` (`src/input/keymap.rs`) gated on `gloss_overlay.current_block()` being `BlockKind::Source`. The "already playing → stop+seek-to-start" path reuses the stop + MPV-seek pattern from `read_current_block` (`src/input/actions/gloss.rs`). No new TTS machinery — `play_block_tts` already does cache-or-synthesize-then-play.

**Tech Stack:** Rust + GTK4 (gtk4 crate) + rusqlite + MPV IPC. Binary-only crate: `cargo build`; tests `cargo test --bins -- --test-threads=1` (rare parallel flake). DO NOT run the GUI (`cargo run`) — the user runs it; visual/runtime behavior is a user check.

**Load-bearing facts (verified at branch HEAD):**
- Current block at keypress: `state.borrow().gloss_overlay.current_block() -> Option<(BlockKind, i32)>` (`src/ui/gloss_overlay.rs:1084`). Condition = `Some((BlockKind::Source, idx))`. `BlockKind` is `{ Source, Explication }` (`gloss_overlay.rs:1546`).
- `VoicePickerOrigin { Settings, GlossOverlay }` (`src/app.rs:36`); field `voice_picker_origin` (`app.rs:260`).
- `open_voice_picker(state, origin)` (`settings.rs:67`); `confirm_voice_picker` (`settings.rs:130`) branches on origin; `cancel_voice_picker` (`settings.rs:182`).
- Picker selection: `voice_picker.selected_voice() -> Option<(String /*voice_id*/, String /*name*/, bool /*free*/)>` (`src/ui/voice_picker.rs:164`).
- `play_block_tts(state_rc, kind, index)` — private fn (`src/input/actions/gloss.rs:654`): resolves voice (active voice if the gloss has associated voices via `gloss_active_voice` index, else `resolve_default_voice`), cache-or-synthesize, `tts.play_file`.
- Active-voice state: `AppState.gloss_active_voice: usize` (index into the gloss's associated voices). `cycle_active_voice` (key `V`) advances it. Associated voices come from `get_gloss_voices`; toggled by `toggle_gloss_voice`.
- `read_current_block` (`gloss.rs:529`): the existing space-toggle — `if s.tts.is_playing() { s.tts.stop(); return; }`; for Source blocks with media it pause/seeks MPV. `resolve_cursor_block` (`gloss.rs:599`) wraps `current_block()` with a "Nothing to read" toast.
- TtsPlayer: `play_file`/`stop`/`is_playing` only — NO true pause (`src/tts.rs:41/72/82`).
- Gloss-overlay key dispatch: `handle_gloss_key` (`src/input/keymap.rs:645`); plain-key `match` at line 752; `_ => true` swallows unbound keys. GTK key name for the letter is `"r"` (matches the established `"v"`/`"a"`/`"j"` pattern). `r` is currently UNBOUND in the overlay.
- MPV seek-to-line-start: the per-line start time + the MPV `Seek`/`ResumeAndSeek`/`Pause` commands are how `read_current_block` seeks media for a Source block — reuse that exact path for the "stop and seek to first-line start" behavior. (Implementer: read `read_current_block`'s Source-block branch in full and mirror its seek; do not invent a new seek.)

**Per CLAUDE.md, this keybind change ALSO requires (Tasks 4–5):** updating the Ctrl+/ keybinds overlay (`src/ui/keybinds_overlay.rs` — keycap + `describe()` arm) via the `update-cairo-keybinds-overlay` skill. It does **NOT** require a `keymap.json` change: gloss-overlay internal keys (space/a/v/V/j/k) are hardcoded in `handle_gloss_key`, not in keymap.json (which only maps reader-mode `Action` variants). Confirm this during Task 5.

---

## Task 1: Add `VoicePickerOrigin::GlossPlay` + confirm behavior (set active voice + play)

**Files:**
- Modify: `src/app.rs` (add enum variant)
- Modify: `src/input/actions/settings.rs` (`open_voice_picker` seeding; `confirm_voice_picker` new branch; `cancel_voice_picker` return mode)

This task wires the picker's confirm path for the new origin. It is the behavioral core. There is no pure unit test for the GTK picker flow (it needs a mapped surface), so verification is build + a focused logic check; the rendered behavior is a user check.

- [ ] **Step 1: Add the enum variant.** In `src/app.rs`, the `VoicePickerOrigin` enum (line 36):

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VoicePickerOrigin {
    Settings,
    GlossOverlay,
    /// Opened by the gloss-overlay `r` key on a Source block: confirming sets
    /// the picked voice as the gloss's active voice and plays the source verse.
    GlossPlay,
}
```

- [ ] **Step 2: Build to confirm the new variant forces exhaustive-match updates.**

Run: `cargo build 2>&1 | rg "non-exhaustive|match.*VoicePickerOrigin|^error" | head` — expect errors at each `match origin` site that doesn't yet handle `GlossPlay`. List them; they are the sites Step 3 must update. (If the matches use a catch-all `_`, there will be NO error — in that case grep `rg -n "VoicePickerOrigin::" src/` and inspect each `match` by hand to ensure `GlossPlay` is handled, not silently swallowed by `_`.)

- [ ] **Step 3: Handle `GlossPlay` in `open_voice_picker`.** In `src/input/actions/settings.rs`, `open_voice_picker` (line 67) currently seeds the ✓ badges from the gloss's associated voices when origin is `GlossOverlay`. `GlossPlay` should seed the SAME badges (so the user sees which voices are associated / which is active) — change the badge-seeding condition from `== GlossOverlay` to `!= Settings` (i.e. either gloss origin seeds badges). Read the function first; the minimal change is broadening that origin check. Set `s.voice_picker_origin = origin` as it already does. Keep `input_mode = InputMode::VoicePicker`.

- [ ] **Step 4: Add the `GlossPlay` branch to `confirm_voice_picker`.** In `src/input/actions/settings.rs`, `confirm_voice_picker` (line 130) branches on `s.voice_picker_origin`. Add a `GlossPlay` arm that:
  1. Reads the picked voice: `let picked = s.gloss_overlay_voice_picker.selected_voice();` (use the actual picker field name — find it via `rg -n "selected_voice" src/input/actions/settings.rs` to see how the `GlossOverlay` arm reads it). If `None`, just return to `InputMode::GlossOverlay` (nothing picked).
  2. Ensures the picked voice is **associated AND active**: the active-voice index (`gloss_active_voice`) indexes into the gloss's associated-voice list (`get_gloss_voices`). To make the picked voice the active one, it must be in that list. So: if the picked `voice_id` is not already associated, associate it (`toggle_gloss_voice` adds it — confirm `toggle_gloss_voice` ADDS when absent by reading its impl); then set `s.gloss_active_voice` to the index of the picked voice within the (re-read) `get_gloss_voices` list. (Read `cycle_active_voice` in `gloss.rs` to match exactly how `gloss_active_voice` is bounded/used, so the index you set is valid for `play_block_tts`'s `voices[i]` access.)
  3. Returns `input_mode = InputMode::GlossOverlay`.
  4. Triggers play of the current Source block. Because `play_block_tts` is a private fn in `gloss.rs`, expose a small `pub(crate)` entry point there (Task 2) and call it here AFTER restoring `input_mode` and dropping the `AppState` borrow (play_block_tts borrows `state_rc`). Order matters: do all the DB/state mutation inside the borrow, drop it, then call the play helper with `state_rc`.

  IMPORTANT borrow discipline: `confirm_voice_picker` likely holds `let mut s = state_rc.borrow_mut();`. You CANNOT call `play_block_tts(state_rc, ...)` while `s` is alive. Structure it: compute `(kind, index)` of the current block and do all mutations under the borrow, capture what you need into locals, `drop(s)` (or end the block), then call the Task-2 play helper.

- [ ] **Step 5: `cancel_voice_picker` returns to GlossOverlay for `GlossPlay`.** In `cancel_voice_picker` (line 182), ensure the `GlossPlay` origin returns `input_mode = InputMode::GlossOverlay` (same as `GlossOverlay`). If it maps origin→mode via a match, add the `GlossPlay` arm; if it's an `if origin == GlossOverlay`, broaden to `!= Settings`.

- [ ] **Step 6: Build.**

Run: `cargo build 2>&1 | rg "^error" || echo OK` — expect `OK`. Resolve any borrow/exhaustiveness errors per the discipline in Step 4.

- [ ] **Step 7: Commit.**

```bash
git add src/app.rs src/input/actions/settings.rs
git commit -m "feat(gloss): VoicePickerOrigin::GlossPlay — confirm sets active voice + plays"
```

---

## Task 2: `pub(crate)` play entry point + the `r` key handler logic

**Files:**
- Modify: `src/input/actions/gloss.rs` (expose play; add the `r`-key entry fn `pick_or_replay_source`)

This adds the function the `r` key calls and the function Task 1's confirm calls.

- [ ] **Step 1: Expose a `pub(crate)` play helper.** In `src/input/actions/gloss.rs`, `play_block_tts` (line 654) is private. Add a thin `pub(crate)` wrapper (do NOT change `play_block_tts`'s visibility unless simpler — a wrapper keeps the private fn private):

```rust
/// Play the given gloss block's TTS (used by the gloss-overlay `r` flow after a
/// voice is picked). Thin `pub(crate)` entry to the private `play_block_tts`.
pub(crate) fn play_block_now(state_rc: &Rc<RefCell<AppState>>, kind: BlockKind, index: i32) {
    play_block_tts(state_rc, kind, index);
}
```

(If `play_block_tts` is already reachable from `settings.rs` via its module path, you may instead make it `pub(crate)` directly and skip the wrapper — pick whichever is the smaller diff. Confirm the call site in Task 1 Step 4 uses whatever you expose.)

- [ ] **Step 2: Add the `r`-key entry fn.** Add to `src/input/actions/gloss.rs`:

```rust
/// Gloss-overlay `r`: on a Source block, either (a) if audio is currently
/// playing, stop and seek the synced media back to the verse's first-line start
/// (rewound, ready to replay); or (b) open the voice picker in `GlossPlay` mode
/// so confirming sets the active voice and plays the verse. No-op (toast) when
/// the accent-bar cursor is not on a Source block.
pub(crate) fn pick_or_replay_source(state_rc: &Rc<RefCell<AppState>>) {
    // 1. Gate: only act on a Source block.
    let on_source = {
        let s = state_rc.borrow();
        matches!(s.gloss_overlay.current_block(), Some((BlockKind::Source, _)))
    };
    if !on_source {
        // Match the existing "wrong block" UX: a brief toast, no action.
        let s = state_rc.borrow();
        s.show_toast("Source verse only"); // use the actual toast API — see below
        return;
    }

    // 2. If audio is playing, stop + seek media to the verse's first-line start.
    let playing = { state_rc.borrow().tts.is_playing() };
    if playing {
        stop_and_seek_source_to_start(state_rc); // Step 3
        return;
    }

    // 3. Otherwise open the picker in GlossPlay mode.
    crate::input::actions::settings::open_voice_picker(
        state_rc,
        crate::app::VoicePickerOrigin::GlossPlay,
    );
}
```

IMPORTANT: Replace `s.show_toast("Source verse only")` with the project's ACTUAL toast call. Find how `resolve_cursor_block` (`gloss.rs:599`) emits its "Nothing to read" toast and copy that exact mechanism (it may be `crate::ui::...` or a method on state / a channel send). Do NOT invent an API.

Also: the MPV "is playing" notion for a Source block may be tracked separately from `tts.is_playing()` (MPV vs rodio). Read `read_current_block`'s Source branch: if it checks an MPV-playing flag (e.g. `s.mpv_playing`) rather than `tts.is_playing()` for media blocks, the `playing` check here must consider BOTH (TTS sink playing OR MPV playing). Mirror exactly what `read_current_block` treats as "currently playing" for a Source block.

- [ ] **Step 3: Implement `stop_and_seek_source_to_start`.** Mirror `read_current_block`'s Source-block media handling, but the requested behavior is **stop + seek to the FIRST LINE's start time** (rewound), not pause-in-place:

```rust
/// Stop any gloss TTS and seek the synced MPV media to the start time of the
/// current Source block's first line, leaving it paused/ready to replay.
fn stop_and_seek_source_to_start(state_rc: &Rc<RefCell<AppState>>) {
    // a. Stop the TTS sink.
    { state_rc.borrow().tts.stop(); }
    // b. Resolve the current Source block's first-line start time and send the
    //    MPV seek (paused). Reuse the SAME seek/pause command read_current_block
    //    uses for a Source block — read that branch and replicate it, but target
    //    the block's FIRST line start time and issue a pause-after-seek (so it is
    //    rewound and not playing).
    // ...(implementer fills from read_current_block + the per-line start-time
    //    lookup the overlay/AppState already exposes for the cursor block)...
}
```

Implementer: the per-line start time for a block's first line is already obtainable on the Source path (read_current_block seeks media to a line time). Find that lookup (likely via the block's `start_line` → `line_mapping` start time, or an AppState helper). If `read_current_block` pauses MPV in place rather than seeking to a specific time, the new requirement differs: you must seek to the first-line start time THEN pause. Use the MPV command enum (`src/mpv/commands.rs` — `Seek` / `ResumeAndSeek` / `Pause`) the rest of the code already uses; do not add a new command. If there is genuinely no synced media for the block (TTS-only), `stop` alone (step a) satisfies "ready to replay from start" since the next `r`→pick→play re-synthesizes from the block start.

- [ ] **Step 4: Build.**

Run: `cargo build 2>&1 | rg "^error" || echo OK` — expect `OK`.

- [ ] **Step 5: Commit.**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(gloss): pick_or_replay_source — r-key entry (Source-only): replay-seek or open GlossPlay picker"
```

---

## Task 3: Bind `r` in `handle_gloss_key`

**Files:**
- Modify: `src/input/keymap.rs` (`handle_gloss_key` plain-key match)

- [ ] **Step 1: Add the `"r"` arm.** In `src/input/keymap.rs`, `handle_gloss_key` plain-key `match key_name` (around line 752, alongside `"a"`, `"v"`, `"space"`), add:

```rust
            "r" => {
                crate::input::actions::gloss::pick_or_replay_source(state);
                true
            }
```

Match the surrounding arms' exact call style (the others pass `state` — confirm the local variable name in this fn is `state`, an `&Rc<RefCell<AppState>>`). Place it near the other audio keys (`a`, `space`, `V`, `v`) for readability.

- [ ] **Step 2: Build + tests.**

Run: `cargo build 2>&1 | rg "^error" || echo OK` (expect `OK`), then `cargo test --bins -- --test-threads=1 2>&1 | rg "test result"` (expect all pass — this change adds no tests; it must not break existing ones).

- [ ] **Step 3: Commit.**

```bash
git add src/input/keymap.rs
git commit -m "feat(gloss): bind r in the gloss overlay to pick_or_replay_source"
```

---

## Task 4: Update the Ctrl+/ keybinds overlay

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (keycap + `describe()` arm)

REQUIRED SUB-SKILL: Use the `update-cairo-keybinds-overlay` skill — it carries the mandatory three-pass cross-reference (no blank slot hides a real binding; no label names the wrong action; every label has a `describe()` arm). Run it for this single new `r` bind.

- [ ] **Step 1: Add `r` to the gloss-overlay key rows.** The gloss-overlay binds live near the `v`/`V` keycap (line 92) and `Space` (line 100). Add a keycap/descriptor for `r` in the correct row table (find `r`'s row — it's a home/letter-row key) with a short label, e.g. `"r"` → `"voice: pick & play verse"`. Follow the existing `key(...)`/`bare(...)` pattern used for `v`/`Space`.

- [ ] **Step 2: Add the `describe()` arm.** Add a `describe()` arm for the new label so the detail panel renders a full description + the handler reference, mirroring the existing voice arms (lines ~353–356):

```rust
        "voice: pick & play verse" => (
            "Gloss overlay, on a Source block (accent bar on the verse): open the \
             voice picker; confirming sets that voice as the gloss's active voice \
             and plays the source verse in it. If audio is already playing, stop \
             and rewind the media to the verse's first line, ready to replay.",
            "-> pick_or_replay_source — src/input/actions/gloss.rs",
        ),
```

(Match the exact tuple shape/format the other gloss arms use — read one before writing.)

- [ ] **Step 3: Run the skill's three-pass cross-reference** (no blank slot, no wrong label, every label described). Confirm `r` renders both a keycap and a non-blank detail row.

- [ ] **Step 4: Build.**

Run: `cargo build 2>&1 | rg "^error" || echo OK` — expect `OK`.

- [ ] **Step 5: Commit.**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(keybinds): document gloss-overlay r (pick & play source verse) in Ctrl+/ overlay"
```

---

## Task 5: Verify keymap.json is correctly NOT involved + final checks

**Files:** none modified (verification only)

- [ ] **Step 1: Confirm keymap.json does not need the gloss-overlay `r`.** Check that the gloss overlay's internal keys are hardcoded, not keymap.json-driven:

Run: `rg -n "\"key\"\s*:\s*\"v\"|\"key\"\s*:\s*\"space\"|GlossOverlay" ~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — expect the gloss-overlay internal keys (v/space/a/V) to be ABSENT (keymap.json only maps reader-mode `Action` variants like `ToggleGlossOverlay`). If absent → no keymap.json change is needed (correct). Document this in the commit message of Task 4 or here. If, unexpectedly, gloss-overlay keys ARE present in keymap.json, STOP and reconsider — the `r` bind would then also need a JSON entry.

- [ ] **Step 2: Full build + test.**

Run: `cargo build && cargo test --bins -- --test-threads=1 2>&1 | rg "test result"` — expect clean build, all tests pass.

- [ ] **Step 3: User runtime verification (cannot be done by an agent).** Provide the user this checklist (the acceptance criterion is on-screen behavior):
  - Open a gloss with a source verse; move the accent bar (`j`/`k`) onto the **Source** block.
  - Press `r` → voice picker opens. Pick a voice, Enter → the verse plays in that voice; the picked voice is now the active voice (subsequent `space`/`a` use it; `V` cycles from it).
  - While it's playing, press `r` again → playback stops and the media rewinds to the verse's first line (next `r`→pick→Enter replays from the start).
  - Move the accent bar onto an **Explication** block, press `r` → nothing plays; a "Source verse only" toast appears.
  - Press Ctrl+/ → the overlay shows `r` with the "voice: pick & play verse" detail.

---

## Self-review notes

- **Decision coverage:** "set active voice, then play" → Task 1 Step 4 (associate-if-needed + set `gloss_active_voice` + play). "pause = stop and seek to first-line start, ready to replay" → Task 2 Step 3 (`stop_and_seek_source_to_start`). Key `r` → Task 3. Source-only gate ("accent bar to left of source text") → Task 2 Step 2 (`current_block()` == `Source`).
- **Reuse over invention:** the picker (open/confirm/cancel), `play_block_tts`, the MPV seek/pause commands, the toast API, and the active-voice indexing are all EXISTING — the plan reuses each and flags exactly where to read the real shape (no invented APIs). The only genuinely new surface is `VoicePickerOrigin::GlossPlay` + two small gloss-module fns.
- **Borrow discipline** is called out explicitly (Task 1 Step 4, Task 2): mutate under the borrow, drop it, then call `play_block_*` with `state_rc` — the recurring GTK/Rc<RefCell> hazard in this codebase.
- **No keymap.json change** is the deliberate, verified conclusion (Task 5 Step 1), consistent with how the gloss overlay's other internal keys work.
- **Risk — the "currently playing" notion for a Source block** (TTS sink vs MPV) is the subtlest point; Task 2 Step 2 directs the implementer to mirror exactly what `read_current_block` treats as playing, rather than assume `tts.is_playing()` alone. If the implementer finds `read_current_block` uses an MPV flag, the `r` handler must consider both.
- **Active-voice index validity:** Task 1 Step 4 sets `gloss_active_voice` to the picked voice's index in the freshly re-read associated list, so `play_block_tts`'s `voices[i]` access (clamped via `.min(len-1)`) stays valid even if the list changed.
