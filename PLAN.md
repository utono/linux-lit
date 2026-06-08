# Plan: ElevenLabs voice picker + free-voice (Alice) fallback

## Goal

1. Let the user pick a **preferred ElevenLabs voice** from a fuzzy picker that
   lists their account's voices (live from the API), flagging which are
   free-tier-safe vs paid.
2. When synthesis with the preferred voice returns **HTTP 402
   `paid_plan_required`**, **fall back to Alice** (a free `premade` voice),
   show a toast, **keep the preference**, and cache the audio under Alice's id.

## Codebase state at last review (commit 6ad9ce8, branch `master`)

The gloss feature branches merged to `master`; current HEAD is `6ad9ce8`.
Line numbers below were re-verified against HEAD on this review. Relevant
recent commits since the plan was first written:

- `a763556 fix(settings): give settings overlay a fixed height so all 7 rows
  show` — added `.height_request(360)` (`settings_overlay.rs:56`) so the
  vexpand scroll fills between header and footer. **Consequence for this plan:**
  adding an 8th "Voice" row means that 360 must grow (≈ +50 → 410) or the new
  row pushes the footer back into overlap — the exact bug that fix addressed.
  Also bump the hardcoded header count label `"7 items"` (`settings_overlay.rs:74`).
- `f14861d` / `c3770be` / `01798f9` — gloss block-stepping cursor + Space
  play/pause/seek. These reshaped `gloss.rs` (synth call site moved to ~678)
  but did NOT change the synth/cache/error structure this plan hooks into.
- `968b967` / `5ac7729` — `gloss_audio` table now has a `kind` column;
  `save_gloss_audio(conn, gloss_id, kind, index, path, voice_id, model_id)` is
  unchanged in shape from the plan's assumptions.

## Resolved facts (from investigation — do not re-derive)

- **Current default voice IS already Alice** — `Xb7hH8MSUJpSbSDYk0k2`,
  category `premade`, free-tier-safe. `src/config.rs:125-129`. This is the
  fallback constant.
- **Target voice "Will - Poetical & Measured"** = `KjWPwHJWLungxeiYigoM`,
  category **`professional`** (paid). Fine-tuned for `eleven_multilingual_v2`.
- **Account is `free` tier** (`professional_voice_limit: 0`) → professional/
  library voices 402 on this plan. The fallback is the whole point.
- **Ctrl+, settings overlay already exists and is fully wired:**
  `OpenSettingsOverlay` action → `settings::open_settings`. Overlay UI in
  `src/ui/settings_overlay.rs` (`NUM_SETTINGS` line 8; `SettingsSnapshot`
  10-19; `names` array 98-106; header count `"7 items"` line 74;
  `.height_request(360)` line 56), handlers in
  `src/input/actions/settings.rs`, input routed via `InputMode::Settings`
  (`keymap.rs` → `handle_settings_key`).
- **Ctrl+/ keybinds overlay already documents Ctrl+, as "settings"**
  (`keybinds_overlay.rs:53` + describe arm 304-305). **No keycap/describe
  change needed** as long as we don't add a brand-new keybind.
- **Config fields already exist:** `elevenlabs_voice_id` / `elevenlabs_model_id`
  (`config.rs:67-70`). No schema change for the preference itself.

## Design decisions (locked with user)

- **Voice selection UI:** a **fuzzy picker popup** (filterable list, like
  `concordance_word_picker.rs`), NOT an inline h/l cycle row.
- **Voice list source:** **live `GET /v1/voices`** from the ElevenLabs REST API
  using the existing `ELEVENLABS_API_KEY` (the running Rust app has the REST
  API, NOT the MCP server). Cached in memory for the session.
- **Free-tier flag:** **yes** — show each voice's `category`
  (`premade` = free-safe; `professional`/`cloned`/etc. = likely-paid badge).
- **Fallback UX:** **toast + use Alice, keep preference.** Audio plays in
  Alice's voice; the preferred voice stays saved so it works after an upgrade;
  cache row stores Alice's voice_id/model_id (the actually-used voice).

---

## Work item A — 402 fallback to Alice (do this first; smaller, self-contained)

### A1. `src/elevenlabs.rs` — distinguish 402

- Add enum variant `PaidPlanRequired` to `ElevenLabsError` (lines 4-9) + a
  `Display` arm (e.g. "Voice requires a paid plan").
- In `synthesize`, alongside the existing 429 special-case (lines 63-64), add:
  `if status == reqwest::StatusCode::PAYMENT_REQUIRED { return
  Err(ElevenLabsError::PaidPlanRequired); }`. (Belt-and-suspenders: also treat a
  body containing `paid_plan_required` as this variant, since some plans return
  it with a different status.)
- Add public consts for the free fallback so callers don't reach into config:
  `pub const ALICE_VOICE_ID: &str = "Xb7hH8MSUJpSbSDYk0k2";`
  `pub const ALICE_MODEL_ID: &str = "eleven_turbo_v2_5";` (premade voice works
  with turbo; keep it free-safe). Reuse these in `config.rs::default_*` so the
  Alice id lives in ONE place.
- Unit test: a small test asserting the new variant's Display string (the file
  already has `error_display_messages`).

### A2. `src/input/actions/gloss.rs` — retry on fallback

In the synth closure (`gloss.rs:674-719`; synth call at line 678), the success
path writes the file + `save_gloss_audio(... &voice_id, &model_id ...)` + plays
(682-709, the `save_gloss_audio` call at 696-704); the `Ok(Err(e))` arm
(711-714) currently just toasts.

- Refactor the write+cache+play block (682-709) into a small local helper/closure
  parameterized by `(bytes, used_voice_id, used_model_id)` so it can be called
  for either the primary or fallback synth, and **caches under the voice that
  actually produced the bytes**.
- Change the `Ok(Err(e))` arm: if `matches!(e, ElevenLabsError::PaidPlanRequired)`
  AND the requested voice != `ALICE_VOICE_ID`, then:
  - `show_tts_toast(&state_for_result, "<voicename> needs a paid plan — using Alice")`
    (use the configured voice id; a friendly name is optional — id is fine for v1),
  - re-run the same `tokio_handle.spawn(synthesize(&text, ALICE_VOICE_ID,
    ALICE_MODEL_ID))` shape (re-clone `text`; note `text` was moved into the first
    spawn, so capture an extra clone up front),
  - on `Ok(Ok(bytes))` call the write+cache+play helper with Alice's id/model,
  - on a second failure, toast that error and stop (no infinite loop).
  - Otherwise (already Alice, or a non-402 error) keep the existing toast.
- **Do NOT** mutate `s.config.elevenlabs_voice_id` here (decision: keep
  preference).

### A3. Verify A

- `cargo build`, `cargo test --bins` (elevenlabs unit tests).
- Runtime is user-run (`cargo run`): with Will preferred on a free plan,
  triggering TTS should toast "...using Alice" and play in Alice's voice; the
  cached `gloss_audio` row should carry Alice's voice_id.

---

## Work item B — fuzzy voice picker in the settings flow

### B1. `src/elevenlabs.rs` — `list_voices()`

- Add `pub struct VoiceInfo { voice_id, name, category }` and
  `pub async fn list_voices() -> Result<Vec<VoiceInfo>, ElevenLabsError>` that
  `GET https://api.elevenlabs.io/v1/voices` with the `xi-api-key` header and
  parses `voices[] → { voice_id, name, category }`. Same client/timeout/error
  mapping as `synthesize`. (`premade` category = free-safe badge.)

### B2. New UI: `src/ui/voice_picker.rs`

- Copy the structure of `src/ui/concordance_word_picker.rs` verbatim where
  possible: `Overlay` + `picker_box` (`picker-box` CSS) + search `Entry`
  (`picker-entry`) + `ScrolledWindow` + `ListBox` (`picker-list`); `attach` /
  `show` / `hide` / `is_visible` / `filter_changed` / `move_selection` /
  `selected_*` / `entry()`.
- Each row: name label (`picker-item-title`) + a small category badge label
  (e.g. "free" for `premade`, "paid" for others) styled like the count column
  in the concordance picker. `row.set_widget_name(voice_id)` so
  `selected_voice_id()` returns the id.
- Hold `voices: Vec<VoiceInfo>`; `set_voices(...)`; filter on name
  (case-insensitive), same as `populate_list`.
- Register the picker in `src/ui/mod.rs` and add an instance to whatever struct
  owns the other pickers (mirror how `concordance_word_picker` is attached/owned
  — follow the memory-bank rule: **add via `add_overlay`, never into the
  size-bearing widget chain**).

### B3. Reaching the picker — settings-overlay row (CHOSEN)

**Decision: open the voice picker from a new "Voice" row inside the existing
Ctrl+, settings overlay. NO new keybind.** This deliberately avoids the Ctrl+/
keybind-overlay maintenance burden (no keycap/`describe()` change, no editing
`keymap.json` in both `~/.config/linux-lit/` and the stow source). Ctrl+, is
already documented as "settings" in `keybinds_overlay.rs:53` + describe arm
304-305 — that stays accurate.

Implementation:
- Add a "Voice" row to `src/ui/settings_overlay.rs`: bump `NUM_SETTINGS` 7→8
  (line 8), extend the `names` array (lines 98-106), `SettingsSnapshot`
  (lines 10-19), update the hardcoded header count `"7 items"` → `"8 items"`
  (line 74), and the value-display/update path so the row shows the current
  voice (name if known, else id).
- **Resize the overlay to fit all 8 rows** (per user: the settings overlay can
  be made larger). The current `.height_request(360)` (line 56, sized for 7
  rows by commit `a763556`) must grow — bump to ~410 (≈ +50 per added row) so
  the new row fits without the footer overlapping. Update the line-54-55
  comment's "7 rows" wording to match. This is the same collapse-bug class that
  `a763556` fixed, so the height MUST track the row count.
- This row is **action-on-Enter**, not a value-cycle. In
  `handle_settings_key` (`keymap.rs:510-564`): when the selected row is the
  Voice row and the user presses Enter (or `l`/Right), DON'T cycle a value —
  instead open the voice picker (transition `InputMode::Settings` →
  `InputMode::VoicePicker`, keeping the settings overlay underneath so it
  reappears on picker close). Up/Down still move between settings rows as
  normal.

### B4. Voice picker action, fetch, input routing, confirm

**AS BUILT:** The standalone `Action::OpenVoicePicker` variant was *skipped* —
the picker opens only from the settings Voice row (`apply_settings_change` →
`open_voice_picker`), so a bindable action with no binding would be dead
scaffolding (extra `category()`/`Display`/keymap-parser arms for nothing). If a
dedicated hotkey is ever wanted, add the variant + a dispatch arm then.
- `pickers::open_voice_picker(state)`: spawn async `list_voices()`; show
  "Loading voices…" meanwhile; populate on completion; set
  `InputMode::VoicePicker`; show the picker.
- Input handling for `InputMode::VoicePicker` in `keymap.rs`, mirroring the
  concordance-word-picker: type-to-filter via the Entry, Up/Down =
  `move_selection`, Enter = confirm, Escape = cancel (Escape returns to the
  settings overlay, i.e. `InputMode::Settings`, not straight to Reader).
- **Confirm handler:** set `s.config.elevenlabs_voice_id = selected_id`; keep
  current `elevenlabs_model_id` unless the picked voice is professional, in
  which case optionally set `eleven_multilingual_v2`; then
  `crate::config::save(&s.config)` (runtime-persist pattern, `app.rs:4429`).
  Return to `InputMode::Settings` and refresh the Voice row's displayed value.
- **No Ctrl+/ overlay change, no `keymap.json` edit, no
  `update-cairo-keybinds-overlay` run** — that's the whole point of the
  settings-row approach.

### B5. Verify B

- `cargo build`, `cargo test --bins`.
- This is overlay/visual work → **user must run the e2e/headless check**
  (CLAUDE.md "When to ASK THE USER to run e2e-env.sh" → overlay layout). Agent
  cannot launch cage from the live session. Provide:
  `./scripts/e2e-env.sh cargo test -- --ignored --nocapture`
  and ask the user to open Ctrl+, → Voice row → picker + screenshot.

---

## Out of scope (confirmed)

- No `voice_settings` (stability/similarity/style/speed/speaker-boost). The
  request body stays `{ text, model_id }`. Those screenshot sliders are a
  separate future feature.
- No re-synth of already-cached gloss audio (`gloss_audio` rows keep their
  original voice; only new glosses use the new preference).
- No change to the separate `elevenlabs-spoken-word` MCP project.

## Suggested order

1. **A (402 fallback)** — small, independently shippable, the safety net.
2. **B1 `list_voices()`** + **B2 picker UI**.
3. **B3 settings-overlay "Voice" row** + **B4 picker action/fetch/confirm
   wiring** (settings-row approach, decided — no keybind/overlay work).
4. Build + `cargo test --bins`; hand off visual verification to user.
