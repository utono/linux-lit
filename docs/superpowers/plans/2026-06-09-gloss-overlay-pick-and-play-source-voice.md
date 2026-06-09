# Gloss-overlay synthesized-voice playback: `r` (play/stop) + `R` (pick voice) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add TWO keybinds in the gloss overlay, **both active only when the accent-bar cursor is on a Source block**, both driving the ElevenLabs-synthesized MP3 of the verse via the Rust `TtsPlayer` (rodio), and **both pausing the MPV recording first** so the AI voice never overlaps the human recording:

- **`r` (lowercase)** — play/stop the source block's synthesized MP3 in the gloss's **active voice** (or the age-aware default voice when the gloss has no associated voices). If not playing: pause MPV, then play (cache hit → play the stored MP3; miss → synthesize via ElevenLabs, then play). If already playing: stop the TTS sink (MPV stays paused). **No picker.**
- **`R` (shift+r)** — pause MPV, then open the **voice picker**; confirming sets the picked voice as the gloss's **active voice** and plays the verse in it. If the TTS sink is already playing when `R` is pressed, stop it (MPV stays paused) — same stop semantics as `r`. **`R` is the only key that opens the picker.**

**CRITICAL — two separate audio worlds; `r`/`R` control ONLY the synthesized one, and pause (never resume) the recording:**
- On a Source block, **`space` (`read_current_block`) and `a` (`begin_current_block`) drive the MPV recording** (`ResumeAndSeek`/`Pause` on `cmd_tx`); they fall through to TTS only when the block has NO media.
- **`r`/`R` ALWAYS drive the ElevenLabs MP3 via `TtsPlayer`** (`play_block_tts` → `tts.play_file`/`tts.stop`), regardless of whether MPV media exists. Their ONLY MPV interaction is sending `MpvCommand::Pause` **once, immediately before starting TTS playback** (to avoid the two streams overlapping). They never `ResumeAndSeek`, never resume, never seek MPV. Stopping the TTS sink does NOT resume MPV — the user resumes the recording explicitly with `space`. `space` = recorded human voice; `r`/`R` = synthesized AI voice.

**Architecture:** Reuse the existing voice picker (`open_voice_picker`/`confirm_voice_picker` in `src/input/actions/settings.rs`) via a NEW `VoicePickerOrigin::GlossPlay` variant so confirm sets the active voice + plays. Add to `src/input/actions/gloss.rs`: a shared helper `play_source_tts_pausing_mpv` (pause MPV, then `play_block_tts`), an `r` entry `toggle_source_tts` (Source-gated: stop-if-playing else play active/default), and an `R` entry `pick_source_voice` (Source-gated: stop-if-playing else open `GlossPlay` picker). Bind `r` and `R` in `handle_gloss_key`. No new TTS machinery — `play_block_tts` already does cache-or-synthesize-then-play; the only new MPV touch is a single `Pause` before playback.

**Tech Stack:** Rust + GTK4 (gtk4 crate) + rusqlite + MPV IPC. Binary-only crate: `cargo build`; tests `cargo test --bins -- --test-threads=1` (rare parallel flake). DO NOT run the GUI (`cargo run`) — the user runs it; visual/runtime behavior is a user check.

**Load-bearing facts (verified at branch HEAD):**
- Current block at keypress: `state.borrow().gloss_overlay.current_block() -> Option<(BlockKind, i32)>` (`src/ui/gloss_overlay.rs:1084`). Condition = `Some((BlockKind::Source, idx))`. `BlockKind` is `{ Source, Explication }` (`gloss_overlay.rs:1546`).
- `VoicePickerOrigin { Settings, GlossOverlay }` (`src/app.rs:36`); field `voice_picker_origin` (`app.rs:260`).
- `open_voice_picker(state, origin)` (`settings.rs:67`); `confirm_voice_picker` (`settings.rs:130`) branches on origin; `cancel_voice_picker` (`settings.rs:182`).
- Picker selection: `voice_picker.selected_voice() -> Option<(String /*voice_id*/, String /*name*/, bool /*free*/)>` (`src/ui/voice_picker.rs:164`).
- `play_block_tts(state_rc, kind, index)` — private fn (`src/input/actions/gloss.rs:654`): resolves voice (active voice if the gloss has associated voices via `gloss_active_voice` index, else `resolve_default_voice`), cache-or-synthesize, `tts.play_file`.
- Active-voice state: `AppState.gloss_active_voice: usize` (index into the gloss's associated voices). `cycle_active_voice` (key `V`) advances it. Associated voices come from `get_gloss_voices`; toggled by `toggle_gloss_voice`.
- `read_current_block` (`gloss.rs:529`, key `space`) and `begin_current_block` (`gloss.rs:569`, key `a`): the existing Source-block audio keys. **These are the MPV-media path** — `space` checks `source_media_state` (`mpv_connected`/`mpv_playing`) and sends `MpvCommand::Pause` / `MpvCommand::ResumeAndSeek(start)` via `cmd_tx`; both fall through to `play_block_tts` ONLY when the block has no media. **The new `r` key does NOT use this path** — it is TTS-only (see below). `resolve_cursor_block` (`gloss.rs:599`) wraps `current_block()` with a "Nothing to read" toast.
- `stop_all_gloss_audio` (`gloss.rs:519`) stops BOTH: `s.tts.stop()` + MPV `Pause`. `r`/`R` must NOT use this on the stop path (it sends a redundant Pause, but more importantly the intent is "stop only the TTS sink") — on stop, use `s.tts.stop()` ONLY.
- MPV pause command: `MpvCommand::Pause` (`src/mpv/commands.rs:11`), sent via `s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause)` — the exact idiom used at `gloss.rs:522` and `gloss.rs:548`. This is the ONE MPV command `r`/`R` send, and only immediately before starting TTS playback.
- TtsPlayer (`src/tts.rs`): `play_file(path)` (line 41, stops current + plays the MP3 from its start), `stop()` (72), `is_playing()` (82) — NO true pause. This is the ONLY audio system `r`/`R` read/play. Replay-from-start = `play_file` re-invoked on the cached MP3 (it always starts the file at 0).
- `show_tts_toast(state_rc, msg)` (`gloss.rs:869`) — the toast helper this module already uses (drops the borrow before showing). Use it for the "Source verse only" no-op toast.
- Gloss-overlay key dispatch: `handle_gloss_key` (`src/input/keymap.rs:645`); plain-key `match key_name` (line 752); `_ => true` swallows unbound keys. GTK key names are `"r"` and `"R"` (shift+r), matching the established `"v"`/`"V"`/`"a"` pattern. **Both `"r"` and `"R"` are currently UNBOUND in the gloss overlay** (the `"R"` at `keymap.rs:1193` is in `handle_echoes_overlay_key`, a different handler — no collision).

**Per CLAUDE.md, this keybind change ALSO requires (Task 4):** updating the Ctrl+/ keybinds overlay (`src/ui/keybinds_overlay.rs` — keycap + `describe()` arms for BOTH `r` and `R`) via the `update-cairo-keybinds-overlay` skill. It does **NOT** require a `keymap.json` change: gloss-overlay internal keys (space/a/v/V/j/k) are hardcoded in `handle_gloss_key`, not in keymap.json (which only maps reader-mode `Action` variants). Confirm this during Task 5.

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
    /// Opened by the gloss-overlay `R` key on a Source block: confirming sets
    /// the picked voice as the gloss's active voice and plays the source verse
    /// (synthesized MP3, pausing MPV first).
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
  4. Triggers **pause-MPV-then-play** of the current Source block by calling the Task-2 helper `crate::input::actions::gloss::play_source_tts_pausing_mpv(state_rc, index)` (NOT bare `play_block_tts` — the picker-confirm path must also pause MPV so the synthesized voice doesn't overlap the recording). Call it AFTER restoring `input_mode` and dropping the `AppState` borrow. Order: do all DB/state mutation inside the borrow, capture the current Source block's `index` into a local, drop the borrow, then call the helper.

  IMPORTANT borrow discipline: `confirm_voice_picker` likely holds `let mut s = state_rc.borrow_mut();`. You CANNOT call the play helper while `s` is alive. Structure it: compute the current block's `index` and do all mutations under the borrow, capture locals, `drop(s)` (or end the block), then call `play_source_tts_pausing_mpv(state_rc, index)`. (Confirm via `current_block()` that it is a Source block; if somehow not, just return to GlossOverlay without playing — `R` is only reachable on a Source block per Task 3's gate, so this is a belt-and-suspenders check.)

- [ ] **Step 5: `cancel_voice_picker` returns to GlossOverlay for `GlossPlay`.** In `cancel_voice_picker` (line 182), ensure the `GlossPlay` origin returns `input_mode = InputMode::GlossOverlay` (same as `GlossOverlay`). If it maps origin→mode via a match, add the `GlossPlay` arm; if it's an `if origin == GlossOverlay`, broaden to `!= Settings`.

- [ ] **Step 6: Build.**

Run: `cargo build 2>&1 | rg "^error" || echo OK` — expect `OK`. Resolve any borrow/exhaustiveness errors per the discipline in Step 4.

- [ ] **Step 7: Commit.**

```bash
git add src/app.rs src/input/actions/settings.rs
git commit -m "feat(gloss): VoicePickerOrigin::GlossPlay — confirm sets active voice + plays"
```

---

## Task 2: gloss.rs entry fns — shared play-pausing-MPV helper, `r` toggle, `R` pick

**Files:**
- Modify: `src/input/actions/gloss.rs` (add `play_source_tts_pausing_mpv`, `toggle_source_tts`, `pick_source_voice`)

This adds the three functions: the shared helper Task-1's confirm and the `r` key both use, the `r`-key entry, and the `R`-key entry.

- [ ] **Step 1: Add the shared play helper `play_source_tts_pausing_mpv`.** It pauses MPV (so the synthesized voice doesn't overlap the recording), then plays the Source block's TTS MP3 via the existing private `play_block_tts` (which does cache-hit-play OR synthesize-then-play). Add to `src/input/actions/gloss.rs`:

```rust
/// Play a Source block's synthesized (ElevenLabs) MP3 in the gloss's active /
/// default voice, FIRST pausing the MPV recording so the two audio streams do
/// not overlap. Cache hit -> play the stored MP3; miss -> synthesize then play
/// (both handled by `play_block_tts`). Used by the gloss-overlay `r` key and by
/// the `R` picker-confirm path. MPV is paused exactly once here, immediately
/// before playback; it is never resumed by this path (the user resumes the
/// recording with `space`).
pub(crate) fn play_source_tts_pausing_mpv(state_rc: &Rc<RefCell<AppState>>, index: i32) {
    // Pause the MPV recording first (idempotent if already paused).
    {
        let s = state_rc.borrow();
        let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
    }
    // Then play the synthesized MP3 for this Source block.
    play_block_tts(state_rc, BlockKind::Source, index);
}
```

(`play_block_tts` stays private; this `pub(crate)` helper is the single entry both `r` and `R`-confirm use. Confirm `cmd_tx`/`MpvCommand::Pause` are reachable exactly as written by reading `gloss.rs:522`.)

- [ ] **Step 2: Add the `r`-key entry `toggle_source_tts`.** On a Source block: if the TTS sink is playing -> stop it (MPV stays paused; no resume); else -> play the active/default-voice MP3 via the Step-1 helper. NO picker. Add:

```rust
/// Gloss-overlay `r`: play/stop the Source block's synthesized MP3 in the
/// gloss's ACTIVE voice (or the age-aware default voice when the gloss has no
/// associated voices) — the ElevenLabs/`TtsPlayer` channel, NOT the MPV
/// recording (`space`/`a`). Toggle: if the TTS sink is playing, stop it (MPV
/// stays paused; the user resumes the recording with `space`); else pause MPV
/// and play (cache hit -> play; miss -> synthesize then play). No picker. No-op
/// (toast) off a Source block.
pub(crate) fn toggle_source_tts(state_rc: &Rc<RefCell<AppState>>) {
    let index = match source_block_index(state_rc) {
        Some(i) => i,
        None => return, // not a Source block — toast already shown
    };
    // Already playing the synthesized audio -> stop only the TTS sink.
    let tts_playing = { state_rc.borrow().tts.is_playing() };
    if tts_playing {
        state_rc.borrow().tts.stop();
        return;
    }
    play_source_tts_pausing_mpv(state_rc, index);
}
```

- [ ] **Step 3: Add the `R`-key entry `pick_source_voice`.** On a Source block: if the TTS sink is playing -> stop it (same stop semantics as `r`); else -> open the `GlossPlay` voice picker (Task 1's confirm then sets the active voice and plays via the Step-1 helper). Add:

```rust
/// Gloss-overlay `R` (shift+r): open the voice picker for the Source block's
/// synthesized reading. If the TTS sink is already playing, stop it (MPV stays
/// paused) — same stop semantics as `r`. Otherwise open the picker in
/// `GlossPlay` mode; confirming sets the picked voice as the gloss's active
/// voice and plays the verse (pausing MPV first, via the GlossPlay confirm
/// path). `R` is the ONLY key that opens the picker. No-op (toast) off a Source
/// block.
pub(crate) fn pick_source_voice(state_rc: &Rc<RefCell<AppState>>) {
    if source_block_index(state_rc).is_none() {
        return; // not a Source block — toast already shown
    }
    let tts_playing = { state_rc.borrow().tts.is_playing() };
    if tts_playing {
        state_rc.borrow().tts.stop();
        return;
    }
    crate::input::actions::settings::open_voice_picker(
        state_rc,
        crate::app::VoicePickerOrigin::GlossPlay,
    );
}
```

- [ ] **Step 4: Add the shared Source-gate helper `source_block_index`.** Both entries gate on "accent bar on a Source block" and want the block index. Add a small private helper that returns the current Source block's index or toasts + returns None:

```rust
/// The current cursor block's index if it is a Source block; otherwise toast
/// "Source verse only" and return None. (The `r`/`R` synthesized-voice keys act
/// only on the source verse, where the accent bar sits to the left of the
/// source text.)
fn source_block_index(state_rc: &Rc<RefCell<AppState>>) -> Option<i32> {
    let block = state_rc.borrow().gloss_overlay.current_block();
    match block {
        Some((BlockKind::Source, index)) => Some(index),
        _ => {
            show_tts_toast(state_rc, "Source verse only");
            None
        }
    }
}
```

NOTES:
- Use the EXISTING toast helper `show_tts_toast(state_rc, msg)` (`gloss.rs:869`) — confirm with `rg -n "fn show_tts_toast" src/input/actions/gloss.rs`; do NOT invent `s.show_toast`. It drops the borrow before showing (so calling it after `state_rc.borrow()` has ended is fine — `source_block_index` ends its borrow before calling it).
- Stop path is TTS-ONLY: `s.tts.is_playing()` / `s.tts.stop()`. Do NOT add `|| s.mpv_playing`; do NOT call `stop_all_gloss_audio` on the stop path (it sends an extra MPV Pause — harmless but not the intent). MPV is paused ONLY on the play path, inside `play_source_tts_pausing_mpv`.
- `source_block_index` borrows `state_rc`, reads `current_block()`, then ends the borrow before the toast — match the borrow discipline of `resolve_cursor_block` (`gloss.rs:599`), which it mirrors.

- [ ] **Step 5: Build.**

Run: `cargo build 2>&1 | rg "^error" || echo OK` — expect `OK`. (A `dead_code` warning on any of the three `pub(crate)` fns is expected until Task 3 binds the keys / Task 1 calls the helper — that's fine; do not silence.)

- [ ] **Step 6: Commit.**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(gloss): source-verse TTS keys — play_source_tts_pausing_mpv + r toggle + R pick"
```

---

## Task 3: Bind `r` and `R` in `handle_gloss_key`

**Files:**
- Modify: `src/input/keymap.rs` (`handle_gloss_key` plain-key match)

- [ ] **Step 1: Add the `"r"` and `"R"` arms.** In `src/input/keymap.rs`, `handle_gloss_key` plain-key `match key_name` (around line 752, alongside `"a"`, `"v"`, `"V"`, `"space"`), add:

```rust
            "r" => {
                crate::input::actions::gloss::toggle_source_tts(state);
                true
            }
            "R" => {
                crate::input::actions::gloss::pick_source_voice(state);
                true
            }
```

Match the surrounding arms' exact call style (the others pass `state` — confirm the local variable name in this fn is `state`, an `&Rc<RefCell<AppState>>`). Place them near the other audio/voice keys (`a`, `space`, `V`, `v`) for readability. (`"R"` here does not collide with the `"R"` in `handle_echoes_overlay_key` — different handler.)

- [ ] **Step 2: Build + tests.**

Run: `cargo build 2>&1 | rg "^error" || echo OK` (expect `OK` — the Task-2 dead_code warnings should now clear since the keys reach the fns), then `cargo test --bins -- --test-threads=1 2>&1 | rg "test result"` (expect all pass — this change adds no tests; it must not break existing ones).

- [ ] **Step 3: Commit.**

```bash
git add src/input/keymap.rs
git commit -m "feat(gloss): bind r (play/stop source TTS) and R (pick voice) in the gloss overlay"
```

---

## Task 4: Update the Ctrl+/ keybinds overlay

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (keycap + `describe()` arm)

REQUIRED SUB-SKILL: Use the `update-cairo-keybinds-overlay` skill — it carries the mandatory three-pass cross-reference (no blank slot hides a real binding; no label names the wrong action; every label has a `describe()` arm). Run it for BOTH the new `r` and `R` binds.

- [ ] **Step 1: Add `r`/`R` to the gloss-overlay key rows.** The gloss-overlay binds live near the `v`/`V` keycap (line 92) and `Space` (line 100). The `v`/`V` keycap already carries both a plain and a shift label (it's the model for a single key with two actions). Add the `r` key's keycap with its plain + shift labels — e.g. plain `"r"` → `"verse audio: play/stop"`, shift `"R"` → `"verse audio: pick voice"` — following the same `key("v", "V", ...)` two-label pattern used at line 92. (Find `r`'s row; it's a home/letter-row key.)

- [ ] **Step 2: Add BOTH `describe()` arms.** Add a `describe()` arm for EACH new label so the detail panel renders a full description + handler reference, mirroring the existing voice arms (lines ~353–356):

```rust
        "verse audio: play/stop" => (
            "Gloss overlay, on a Source block (accent bar on the verse): play or \
             stop the source verse as a SYNTHESIZED MP3 (ElevenLabs) in the \
             gloss's active voice — or the age-aware default voice if no voice is \
             associated. This is the AI voice, separate from the recorded media \
             that space/a play; starting it pauses the MPV recording so they do \
             not overlap. Press again to stop (the recording stays paused; resume \
             it with space). Cache miss synthesizes on first play.",
            "-> toggle_source_tts — src/input/actions/gloss.rs",
        ),
        "verse audio: pick voice" => (
            "Gloss overlay, on a Source block: open the voice picker for the \
             source verse's synthesized reading. Confirming sets that voice as the \
             gloss's active voice and plays the verse (pausing the MPV recording \
             first). If the synthesized audio is already playing, this stops it \
             instead. R is the only key that opens the picker; r alone replays in \
             the current active voice.",
            "-> pick_source_voice — src/input/actions/gloss.rs",
        ),
```

(Match the exact tuple shape/format the other gloss arms use — read one before writing.)

- [ ] **Step 3: Run the skill's three-pass cross-reference** (no blank slot, no wrong label, every label described). Confirm the `r` keycap renders with BOTH its plain and shift labels, and BOTH detail rows are non-blank.

- [ ] **Step 4: Build.**

Run: `cargo build 2>&1 | rg "^error" || echo OK` — expect `OK`.

- [ ] **Step 5: Commit.**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(keybinds): document gloss-overlay r (verse audio play/stop) + R (pick voice)"
```

---

## Task 5: Verify keymap.json is correctly NOT involved + final checks

**Files:** none modified (verification only)

- [ ] **Step 1: Confirm keymap.json does not need the gloss-overlay `r`/`R`.** Check that the gloss overlay's internal keys are hardcoded, not keymap.json-driven:

Run: `rg -n "\"key\"\s*:\s*\"v\"|\"key\"\s*:\s*\"space\"|GlossOverlay" ~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — expect the gloss-overlay internal keys (v/space/a/V) to be ABSENT (keymap.json only maps reader-mode `Action` variants like `ToggleGlossOverlay`). If absent → no keymap.json change is needed (correct). If, unexpectedly, gloss-overlay keys ARE present in keymap.json, STOP and reconsider — the `r`/`R` binds would then also need JSON entries.

- [ ] **Step 2: Full build + test.**

Run: `cargo build && cargo test --bins -- --test-threads=1 2>&1 | rg "test result"` — expect clean build, all tests pass.

- [ ] **Step 3: User runtime verification (cannot be done by an agent).** Provide the user this checklist (the acceptance criterion is on-screen behavior):
  - Open a gloss with a source verse; move the accent bar (`j`/`k`) onto the **Source** block.
  - Press **`R`** → voice picker opens. Pick a voice, Enter → the verse plays as a **synthesized ElevenLabs MP3** in that voice; the picked voice becomes the gloss's active voice. Confirm the **MPV recording pauses** when the synthesized audio starts (no overlap).
  - Press **`r`** (no picker) → it plays the source verse's synthesized MP3 in the **current active voice** (cache hit replays instantly; first time it synthesizes). MPV pauses on play.
  - While the synthesized audio is playing, press **`r`** (or `R`) again → **the TTS audio stops**; the MPV recording **stays paused** (resume the recording with `space`). Next `r` replays the MP3 from its start.
  - Confirm separation: `space`/`a` still drive the MPV recording; `r`/`R` drive only the synthesized voice; the two never play at once (TTS-play pauses MPV first).
  - Move the accent bar onto an **Explication** block, press `r` or `R` → nothing plays; a "Source verse only" toast appears.
  - Press Ctrl+/ → the overlay shows the `r` keycap with both labels ("verse audio: play/stop" for `r`, "verse audio: pick voice" for `R`), each with a non-blank detail.

---

## Self-review notes

- **Decision coverage (the two-key spec):**
  - `r` = play/stop synthesized MP3 in the **active/default** voice, **synthesize-if-missing**, **no picker** → Task 2 Step 2 (`toggle_source_tts`) + the shared helper (Step 1).
  - `R` = open picker (the ONLY picker key); confirm sets active voice + plays → Task 2 Step 3 (`pick_source_voice`) + Task 1's `GlossPlay` confirm.
  - "stop = TTS sink only; leave MPV paused (no auto-resume)" → Task 2 Steps 2/3 (`tts.stop()`, no MPV command on stop).
  - "starting TTS pauses MPV so they don't overlap" → Task 2 Step 1 (`play_source_tts_pausing_mpv` sends `MpvCommand::Pause` before `play_block_tts`); used by BOTH `r` and the `R` confirm path.
  - Source-only gate ("accent bar to left of source text") → Task 2 Step 4 (`source_block_index` → `current_block()` == `Source`, else toast).
  - keys `r`/`R` bound → Task 3.
- **Two audio worlds, one-way coupling:** `space`/`a` = MPV recording; `r`/`R` = ElevenLabs synthesized MP3 via `TtsPlayer`. The ONLY cross-coupling is: starting TTS sends a single `MpvCommand::Pause` first (no overlap). `r`/`R` never resume/seek MPV; stopping TTS leaves MPV paused (user resumes with `space`). Task 2 forbids `|| s.mpv_playing` on the stop path and confines the MPV `Pause` to the play helper only.
- **Reuse over invention:** the picker (open/confirm/cancel), `play_block_tts`, the toast helper `show_tts_toast`, `MpvCommand::Pause`+`cmd_tx`, and the active-voice indexing are all EXISTING — the plan reuses each and flags where to read the real shape (no invented APIs). New surface: `VoicePickerOrigin::GlossPlay` + four small gloss-module fns (`play_source_tts_pausing_mpv`, `toggle_source_tts`, `pick_source_voice`, `source_block_index`). (No MPV seek function — the original single-`r` draft's `stop_and_seek_source_to_start` was removed as wrong.)
- **Borrow discipline** is called out explicitly (Task 1 Step 4, Task 2): mutate/read under a scoped borrow, end it, then call the play helper with `state_rc` — the recurring GTK/Rc<RefCell> hazard.
- **No keymap.json change** is the deliberate, verified conclusion (Task 5 Step 1), consistent with how the gloss overlay's other internal keys work.
- **Active-voice index validity:** Task 1 Step 4 sets `gloss_active_voice` to the picked voice's index in the freshly re-read associated list, so `play_block_tts`'s `voices[i]` access (clamped via `.min(len-1)`) stays valid even if the list changed.
- **MPV-overlap resolved (was an open question):** the user confirmed `r`/`R` should pause MPV before playing the synthesized voice — implemented in `play_source_tts_pausing_mpv`. No layering of the two streams.
