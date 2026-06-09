# Gloss-overlay "pick voice & play source verse" (`r` key) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an `r` keybind in the gloss overlay that, **only when the accent-bar cursor is on a Source block**, opens the voice picker; on confirm it sets the picked voice as the gloss's **active voice** and **plays the source verse** in it — via the ElevenLabs-synthesized MP3 through the Rust `TtsPlayer` (rodio). If the TTS MP3 is already playing when `r` is pressed, instead **stop the `TtsPlayer` sink** (rewound to the MP3's start, ready to replay) — no picker.

**CRITICAL — two separate audio worlds. Do not conflate them:**
- On a Source block, **`space` (`read_current_block`) and `a` (`begin_current_block`) drive the MPV instance** playing the work's media file (the real human recording): `ResumeAndSeek` / `Pause` on `cmd_tx`. They only fall through to TTS when the block has NO media.
- The new **`r` ALWAYS drives the ElevenLabs-synthesized MP3 via `TtsPlayer` (rodio)** — `play_block_tts` → `tts.play_file` / `tts.stop`. It does this **regardless of whether MPV media exists** for the verse, and it **NEVER** touches MPV (no `cmd_tx`, no `Pause`, no `ResumeAndSeek`, no seek). `space` = recorded human voice; `r` = synthesized AI voice; the same verse can have both, and `r` always means the AI one.

**Architecture:** Reuse the existing voice picker (`open_voice_picker` / `confirm_voice_picker` in `src/input/actions/settings.rs`) via a NEW `VoicePickerOrigin::GlossPlay` variant so confirm sets the active voice + plays rather than toggling association. The `r` key is a new arm in `handle_gloss_key` (`src/input/keymap.rs`) gated on `gloss_overlay.current_block()` being `BlockKind::Source`. The "already playing → stop" path is `s.tts.is_playing()` → `s.tts.stop()` (the rodio sink ONLY — MPV is never consulted or commanded by `r`); "ready to replay from start" is satisfied because the next `r`→pick→play re-invokes `play_block_tts`, which plays the cached MP3 from its beginning. No new TTS machinery — `play_block_tts` already does cache-or-synthesize-then-play.

**Tech Stack:** Rust + GTK4 (gtk4 crate) + rusqlite + MPV IPC. Binary-only crate: `cargo build`; tests `cargo test --bins -- --test-threads=1` (rare parallel flake). DO NOT run the GUI (`cargo run`) — the user runs it; visual/runtime behavior is a user check.

**Load-bearing facts (verified at branch HEAD):**
- Current block at keypress: `state.borrow().gloss_overlay.current_block() -> Option<(BlockKind, i32)>` (`src/ui/gloss_overlay.rs:1084`). Condition = `Some((BlockKind::Source, idx))`. `BlockKind` is `{ Source, Explication }` (`gloss_overlay.rs:1546`).
- `VoicePickerOrigin { Settings, GlossOverlay }` (`src/app.rs:36`); field `voice_picker_origin` (`app.rs:260`).
- `open_voice_picker(state, origin)` (`settings.rs:67`); `confirm_voice_picker` (`settings.rs:130`) branches on origin; `cancel_voice_picker` (`settings.rs:182`).
- Picker selection: `voice_picker.selected_voice() -> Option<(String /*voice_id*/, String /*name*/, bool /*free*/)>` (`src/ui/voice_picker.rs:164`).
- `play_block_tts(state_rc, kind, index)` — private fn (`src/input/actions/gloss.rs:654`): resolves voice (active voice if the gloss has associated voices via `gloss_active_voice` index, else `resolve_default_voice`), cache-or-synthesize, `tts.play_file`.
- Active-voice state: `AppState.gloss_active_voice: usize` (index into the gloss's associated voices). `cycle_active_voice` (key `V`) advances it. Associated voices come from `get_gloss_voices`; toggled by `toggle_gloss_voice`.
- `read_current_block` (`gloss.rs:529`, key `space`) and `begin_current_block` (`gloss.rs:569`, key `a`): the existing Source-block audio keys. **These are the MPV-media path** — `space` checks `source_media_state` (`mpv_connected`/`mpv_playing`) and sends `MpvCommand::Pause` / `MpvCommand::ResumeAndSeek(start)` via `cmd_tx`; both fall through to `play_block_tts` ONLY when the block has no media. **The new `r` key does NOT use this path** — it is TTS-only (see below). `resolve_cursor_block` (`gloss.rs:599`) wraps `current_block()` with a "Nothing to read" toast.
- `stop_all_gloss_audio` (`gloss.rs:519`) stops BOTH: `s.tts.stop()` + MPV `Pause`. The `r` key must NOT use this (it would pause MPV) — `r` stops the TTS sink ONLY via `s.tts.stop()`.
- TtsPlayer (`src/tts.rs`): `play_file(path)` (line 41, stops current + plays the MP3 from its start), `stop()` (72), `is_playing()` (82) — NO true pause. This is the ONLY audio system `r` touches. Replay-from-start = `play_file` re-invoked on the cached MP3 (it always starts the file at 0).
- Gloss-overlay key dispatch: `handle_gloss_key` (`src/input/keymap.rs:645`); plain-key `match` at line 752; `_ => true` swallows unbound keys. GTK key name for the letter is `"r"` (matches the established `"v"`/`"a"`/`"j"` pattern). `r` is currently UNBOUND in the overlay.

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

- [ ] **Step 2: Add the `r`-key entry fn.** Add to `src/input/actions/gloss.rs`. This is **TTS-only** — it must NEVER touch MPV (`cmd_tx`, `Pause`, `ResumeAndSeek`) even on a Source block that has synced media:

```rust
/// Gloss-overlay `r`: on a Source block, drive the ELEVENLABS-SYNTHESIZED MP3
/// via `TtsPlayer` (rodio) — independent of MPV. (`space`/`a` drive the MPV
/// recording; `r` is always the AI voice, even when the verse has real media.)
/// - If the TTS sink is already playing -> stop it (rewound; next `r`→pick→play
///   replays the MP3 from its start). MPV is NOT touched.
/// - Else -> open the voice picker in `GlossPlay` mode; confirming sets the
///   active voice and plays the verse's MP3.
/// No-op (toast) when the accent-bar cursor is not on a Source block.
pub(crate) fn pick_or_replay_source(state_rc: &Rc<RefCell<AppState>>) {
    // 1. Gate: only act on a Source block.
    let on_source = {
        let s = state_rc.borrow();
        matches!(s.gloss_overlay.current_block(), Some((BlockKind::Source, _)))
    };
    if !on_source {
        show_tts_toast(state_rc, "Source verse only"); // see note below
        return;
    }

    // 2. If the TTS MP3 is already playing, stop the rodio sink (TTS ONLY —
    //    do NOT consult or command MPV here; mpv_playing is irrelevant to `r`).
    let tts_playing = { state_rc.borrow().tts.is_playing() };
    if tts_playing {
        state_rc.borrow().tts.stop();
        return;
    }

    // 3. Otherwise open the picker in GlossPlay mode (Task 1: confirm sets the
    //    active voice + plays the verse's MP3 via play_block_tts).
    crate::input::actions::settings::open_voice_picker(
        state_rc,
        crate::app::VoicePickerOrigin::GlossPlay,
    );
}
```

NOTE on the toast: use the project's ACTUAL toast helper — `show_tts_toast(state_rc, "…")` already exists in this module and is what `resolve_cursor_block` (`gloss.rs:599`) uses for "Nothing to read" (it handles dropping the borrow before showing). Confirm its signature with `rg -n "fn show_tts_toast" src/input/actions/gloss.rs` and call it exactly; do NOT invent `s.show_toast`.

IMPORTANT — only the TTS sink matters here, NOT MPV. Deliberately use `s.tts.is_playing()` and `s.tts.stop()` only. Do NOT add an `|| s.mpv_playing` check and do NOT call `stop_all_gloss_audio` (which would also `Pause` MPV). The whole point of `r` is that it is a second, independent audio channel (synthesized AI) that leaves the MPV recording controlled by `space`/`a` completely alone — pressing `r` while MPV media is playing must NOT stop the recording; it should layer/replace only the TTS sink. (If overlapping the AI MP3 over a playing MPV recording is undesirable in practice, that is a UX refinement for the user to flag after seeing it — the spec here is "r controls only the TTS sink"; do not pre-emptively add MPV stopping.)

- [ ] **Step 3: (removed — no MPV seek).** The earlier draft had a `stop_and_seek_source_to_start` that seeked MPV; that was WRONG. `r` is TTS-only, so "ready to replay from start" is achieved entirely by `tts.stop()` in Step 2 plus the fact that `play_block_tts`→`tts.play_file` always starts the MP3 at 0. No MPV seek, no extra function. Skip directly to Step 4.

- [ ] **Step 4: Build.**

Run: `cargo build 2>&1 | rg "^error" || echo OK` — expect `OK`.

- [ ] **Step 5: Commit.**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(gloss): pick_or_replay_source — r-key (Source-only, TTS-only): stop sink or open GlossPlay picker"
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
             and plays the source verse as a synthesized MP3 (ElevenLabs voice — \
             separate from the recorded media on space/a). If that synthesized \
             audio is already playing, stop it (ready to replay from the start). \
             Does not affect the MPV recording.",
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
  - Press `r` → voice picker opens. Pick a voice, Enter → the verse plays as a **synthesized ElevenLabs MP3** in that voice; the picked voice becomes the gloss's active voice (`V` now cycles from it; the TTS-fallback path uses it).
  - While that synthesized audio is playing, press `r` again → **the TTS audio stops** (next `r`→pick→Enter replays the MP3 from its start). Confirm pressing `r` does **NOT** stop or seek the MPV recording.
  - Confirm the audio worlds stay separate: with the MPV recording playing (via `space`), pressing `r` controls only the synthesized voice — `space` still governs the recording.
  - Move the accent bar onto an **Explication** block, press `r` → nothing plays; a "Source verse only" toast appears.
  - Press Ctrl+/ → the overlay shows `r` with the "voice: pick & play verse" detail.

---

## Self-review notes

- **Decision coverage:** "set active voice, then play" → Task 1 Step 4 (associate-if-needed + set `gloss_active_voice` + play via `play_block_tts`). "pause = stop, ready to replay from start" → Task 2 Step 2 (`tts.stop()`; replay is `play_file`-from-0). Key `r` → Task 3. Source-only gate ("accent bar to left of source text") → Task 2 Step 2 (`current_block()` == `Source`).
- **The two audio worlds are kept strictly separate (the load-bearing clarification):** `space`/`a` = MPV recording; `r` = ElevenLabs synthesized MP3 via `TtsPlayer`. `r` reads/writes ONLY the rodio sink (`tts.is_playing()`/`tts.stop()`/`play_block_tts`) and NEVER issues an MPV command. This is the single most important correctness property; Task 2 Step 2 forbids `|| s.mpv_playing` and `stop_all_gloss_audio`.
- **Reuse over invention:** the picker (open/confirm/cancel), `play_block_tts`, the toast helper `show_tts_toast`, and the active-voice indexing are all EXISTING — the plan reuses each and flags where to read the real shape (no invented APIs). The only genuinely new surface is `VoicePickerOrigin::GlossPlay` + two small gloss-module fns. (No MPV seek function is added — the original draft's `stop_and_seek_source_to_start` was removed as wrong.)
- **Borrow discipline** is called out explicitly (Task 1 Step 4): mutate under the borrow, drop it, then call `play_block_*` with `state_rc` — the recurring GTK/Rc<RefCell> hazard in this codebase.
- **No keymap.json change** is the deliberate, verified conclusion (Task 5 Step 1), consistent with how the gloss overlay's other internal keys work.
- **Active-voice index validity:** Task 1 Step 4 sets `gloss_active_voice` to the picked voice's index in the freshly re-read associated list, so `play_block_tts`'s `voices[i]` access (clamped via `.min(len-1)`) stays valid even if the list changed.
- **Open UX question (flag, don't pre-solve):** pressing `r` while the MPV recording plays will layer the synthesized MP3 over it (two audio streams). The plan deliberately does NOT stop MPV on `r` (per the clarification). If the user finds the overlap undesirable in practice, stopping/pausing MPV on `r`-play is a one-line follow-up — left to a post-demo decision.
