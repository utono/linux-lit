# MP3-Cached Block Coloring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Color gloss/synopsis overlay blocks (source verse, explication, synopsis paragraphs) in the theme accent color when that block's TTS mp3 is cached, applied on overlay open and immediately after a block's synthesis completes.

**Architecture:** Add one UI-only method `GlossOverlay::color_audio_blocks(is_cached)` that applies an accent-color `TextTag` over the buffer line-span of each block the injected closure marks cached. The DB/voice existence check lives in the action layer as `recolor_cached_blocks(&AppState)`, which detects gloss-vs-synopsis mode and builds the closure. It is called at the user-facing display entry points and at the four synth-completion sites. Voice resolution for gloss blocks is factored into a shared helper so the recolor check and `play_block_tts` agree on which voice's mp3 to look for.

**Tech Stack:** Rust, GTK4 (gtk4-rs), sourceview5/TextView+TextBuffer+TextTag, rusqlite (via `crate::db::queries`).

---

## File structure

- `src/ui/gloss_overlay.rs` — add `pub fn color_audio_blocks`; make `BlockKind` and a read-only view of block spans available to the closure (it already iterates `self.blocks` internally, so the closure only needs `(&BlockKind, i32)`). No new struct fields.
- `src/db/queries.rs` — (Task 1) factor `resolve_gloss_block_voice` so existence checks and playback agree. *Only if not already trivially reusable.*
- `src/input/actions/gloss.rs` — add `gloss_block_voice` shared helper (or reuse), `recolor_cached_blocks(&AppState)`, and `recolor_cached_blocks_rc(&Rc<RefCell<AppState>>)` wrapper; call at gloss display entry points and the 2 gloss synth-completion sites.
- `src/input/actions/synopsis.rs` — call `recolor_cached_blocks` at synopsis display entry points.
- `src/app.rs` — call `recolor_cached_blocks` after the synopsis open/cycle `show_synopsis` sites.

---

## Task 1: Add `color_audio_blocks` to GlossOverlay (UI-only)

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (add method in the `impl GlossOverlay` block near `apply_synopsis_label_bold`, ~line 518; the method reads `self.blocks` and `self.bar_color`)

- [ ] **Step 1: Read the existing tag-priority pattern**

Re-read `apply_synopsis_label_bold` at `src/ui/gloss_overlay.rs:488-518` — it is the template: look up/add a named tag, bump priority to `table.size()-1`, apply over iters. Reuse this shape.

- [ ] **Step 2: Add the method**

Add to the `impl GlossOverlay` block (after `apply_synopsis_label_bold`):

```rust
/// Color every block whose `is_cached(kind, index)` returns true with the
/// stored accent color (`bar_color`, = theme root_color). Idempotent;
/// re-tagging an already-colored block is harmless. Call AFTER `apply_font`
/// with `self.blocks` already populated (every `show_*` path does both).
pub fn color_audio_blocks(&self, is_cached: impl Fn(&BlockKind, i32) -> bool) {
    let buffer = self.gloss_view.buffer();
    let table = buffer.tag_table();
    let (r, g, b) = *self.bar_color.borrow();
    let rgba = format!(
        "#{:02x}{:02x}{:02x}",
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    );
    let tag = match table.lookup("gloss-audio-cached") {
        Some(t) => {
            t.set_foreground(Some(&rgba));
            t
        }
        None => {
            let t = gtk4::TextTag::builder()
                .name("gloss-audio-cached")
                .foreground(&rgba)
                .build();
            table.add(&t);
            t
        }
    };
    // Outrank the buffer-wide `gloss-font` tag (added last on first show).
    let size = table.size();
    if size > 0 {
        tag.set_priority(size - 1);
    }
    let line_count = buffer.line_count();
    for blk in self.blocks.borrow().iter() {
        if !is_cached(&blk.kind, blk.index) {
            continue;
        }
        let start = buffer.iter_at_line(blk.start_line).unwrap_or_else(|| buffer.start_iter());
        let end_line = (blk.end_line + 1).min(line_count);
        let end = buffer
            .iter_at_line(end_line)
            .unwrap_or_else(|| buffer.end_iter());
        buffer.apply_tag(&tag, &start, &end);
    }
}
```

Note: `BlockRange` (private) stores `kind`, `index`, `start_line`, `end_line` (`src/ui/gloss_overlay.rs:17-22`); `BlockKind` is already `pub` (`:1611`). `iter_at_line` returns `Option<TextIter>` in gtk4-rs — the `unwrap_or_else` guards out-of-range lines.

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles. If `set_foreground` / `iter_at_line` signatures differ, adjust per the compiler (gtk4-rs `TextTag::set_foreground(Option<&str>)`, `TextBuffer::iter_at_line(i32) -> Option<TextIter>`).

- [ ] **Step 4: Clippy**

Run: `cargo clippy 2>&1 | rg "gloss_overlay|color_audio" || echo "clean"`
Expected: no warnings for the new code.

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(overlay): color_audio_blocks tags cached blocks with accent color

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Factor shared gloss-block voice resolution

**Why:** `play_block_tts` resolves a per-block voice (per-gloss override list indexed by `gloss_active_voice`, else age-aware default by kind) at `src/input/actions/gloss.rs:1006-1025`. The recolor existence check must look for the SAME voice's mp3 (then Alice fallback). Duplicating the resolution would drift. Extract it once.

**Files:**
- Modify: `src/input/actions/gloss.rs` (add a free fn near `play_block_tts`)

- [ ] **Step 1: Add the helper**

Add to `src/input/actions/gloss.rs`:

```rust
/// Resolve the (voice_id, model_id) a gloss block plays in: the active per-gloss
/// override voice if the gloss has associated voices (clamped to
/// `active_voice`), else the age-aware default by kind (verse->OP, prose->plain).
/// Shared by `play_block_tts` and the cached-audio recolor check so both look at
/// the same mp3. Mirrors the inline logic at the former call site.
pub(crate) fn gloss_block_voice(
    conn: &rusqlite::Connection,
    gloss_id: i64,
    work_abbrev: &str,
    speaker: &str,
    kind: BlockKind,
    active_voice: usize,
) -> (String, String) {
    let is_verse = kind == BlockKind::Source;
    let voices = crate::db::queries::get_gloss_voices(conn, gloss_id);
    if !voices.is_empty() {
        let i = active_voice.min(voices.len() - 1);
        (voices[i].0.clone(), voices[i].1.clone())
    } else {
        crate::db::queries::resolve_default_voice(conn, work_abbrev, speaker, is_verse)
    }
}
```

- [ ] **Step 2: Use it in `play_block_tts`**

Replace the inline block at `src/input/actions/gloss.rs:1006-1025` (the `let (vid, mid): (String, String) = match crate::db::queries::open_db() { ... }`) with:

```rust
        let (vid, mid): (String, String) = match crate::db::queries::open_db() {
            Ok(conn) => gloss_block_voice(
                &conn, gloss_id, &work_abbrev, &speaker, kind, s.gloss_active_voice,
            ),
            Err(_) => {
                let (v, m) =
                    crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, is_verse);
                (v.to_string(), m.to_string())
            }
        };
```

(`is_verse` is already bound at `:1002`; keep it.)

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles; `play_block_tts` behaves identically (same voice resolution, now via the shared fn).

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "refactor(gloss): extract gloss_block_voice shared by play + recolor

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: `recolor_cached_blocks` action helper

**Files:**
- Modify: `src/input/actions/gloss.rs` (add `recolor_cached_blocks(&AppState)` + `recolor_cached_blocks_rc` wrapper)

- [ ] **Step 1: Add the helpers**

Add to `src/input/actions/gloss.rs`:

```rust
/// Re-apply accent coloring to every block of the currently-open gloss OR
/// synopsis overlay whose mp3 is cached. Detects mode from which context is set:
/// a live `gloss_context` + non-empty `gloss_list` means gloss mode; otherwise
/// fall through to synopsis mode keyed by `synopsis_overlay_scene`. UI-only side
/// effect; DB errors degrade to "uncached" (no color). Call with `s` already
/// borrowed (the display sites) — see `recolor_cached_blocks_rc` for the
/// borrow-and-call wrapper used by async synth completions.
pub(crate) fn recolor_cached_blocks(s: &AppState) {
    // Gloss mode.
    if let (Some(ctx), Some(gloss)) =
        (s.gloss_context.as_ref(), s.gloss_list.get(s.gloss_index))
    {
        let gloss_id = gloss.gloss_id;
        let work_abbrev = ctx.work_abbrev.clone();
        let speaker = ctx.speaker.clone();
        let active = s.gloss_active_voice;
        let conn = match crate::db::queries::open_db() {
            Ok(c) => c,
            Err(_) => return,
        };
        s.gloss_overlay.color_audio_blocks(|kind, index| {
            let kind_str = match kind {
                BlockKind::Source => "source",
                BlockKind::Explication => "explication",
            };
            let (vid, _mid) =
                gloss_block_voice(&conn, gloss_id, &work_abbrev, &speaker, *kind, active);
            for vid_try in [vid.as_str(), crate::elevenlabs::ALICE_VOICE_ID] {
                if let Ok(Some(path)) = crate::db::queries::find_gloss_audio(
                    &conn, gloss_id, kind_str, index as i64, vid_try,
                ) {
                    if std::path::Path::new(&path).exists() {
                        return true;
                    }
                }
            }
            false
        });
        return;
    }

    // Synopsis mode.
    let (div1, div2) = s.synopsis_overlay_scene;
    let work_abbrev = match s.current_work.as_ref() {
        Some(w) => crate::app::base_work_abbrev(&w.abbrev).to_string(),
        None => return,
    };
    let (voice_id, _mid) =
        crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, false);
    let voice_id = voice_id.to_string();
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };
    s.gloss_overlay.color_audio_blocks(|_kind, index| {
        for vid_try in [voice_id.as_str(), crate::elevenlabs::ALICE_VOICE_ID] {
            if let Ok(Some(path)) = crate::db::queries::find_synopsis_audio(
                &conn, &work_abbrev, div1, div2, index as i64, vid_try,
            ) {
                if std::path::Path::new(&path).exists() {
                    return true;
                }
            }
        }
        false
    });
}

/// Borrow `state` and recolor. For async synth-completion sites that hold an
/// `Rc<RefCell<AppState>>` and must not already hold a borrow.
pub(crate) fn recolor_cached_blocks_rc(state: &Rc<RefCell<AppState>>) {
    recolor_cached_blocks(&state.borrow());
}
```

Verify against source while writing: `gloss_context` field type and `.work_abbrev`/`.speaker` (`src/input/actions/gloss.rs:993-994`); `synopsis_overlay_scene: (i64,i64)` and `base_work_abbrev` (`src/input/actions/synopsis.rs:171`, `src/app.rs`); `find_synopsis_audio` signature (`src/input/actions/gloss.rs:1268`). The synopsis-mode `current_work` may be `None` when a gloss overlay is open — that's why gloss mode is checked first and returns.

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles. Fix any borrow/lifetime issue by cloning the closure's captured `String`s (already cloned above) — the `conn` is moved into the closure by reference capture; if the borrow checker complains, capture `conn` by `move` and the `Strings` are owned, so `move` closures work.

- [ ] **Step 3: Clippy**

Run: `cargo clippy 2>&1 | rg "recolor_cached|gloss.rs" || echo "clean"`
Expected: no new warnings.

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(gloss): recolor_cached_blocks resolves cached mp3s per block

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Call recolor at gloss display entry points

**Files:**
- Modify: `src/input/actions/gloss.rs` (after the user-facing `show_gloss_with_color` calls at the open/navigate/delete sites and the add/edit/work-open completion sites)

- [ ] **Step 1: Add recolor after each gloss display**

After each of these `show_gloss_with_color(...)` calls, while `s` is still borrowed, insert `recolor_cached_blocks(&s);` (the call uses `&AppState`; if the local is `mut s`, `&s` is fine):

- `:145-148` (main passage open) → after `set_position` at `:149`, add `recolor_cached_blocks(&s);`
- `:173-176` (navigate_gloss) → after `:177`, add `recolor_cached_blocks(&s);`
- `:245-249` (delete redisplay) → after `:249`, add `recolor_cached_blocks(&s);`
- `:569-577` region → after its `set_position`, add `recolor_cached_blocks(&s);`
- `:719-727` (add/edit completion under `state_for_result.borrow_mut()`) → after `set_position`, add `recolor_cached_blocks(&s);`
- `:816-823` (second completion arm) → same, add `recolor_cached_blocks(&s);`
- `:1772-1779` (work-glosses open) → after `set_position`, add `recolor_cached_blocks(&s);`

For each, the borrow holding `s` is already a `borrow()`/`borrow_mut()`; `recolor_cached_blocks(&s)` re-borrows nothing (takes `&AppState`). Place the call AFTER `set_position` so block ranges are settled.

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(gloss): color cached blocks on every gloss display

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Call recolor at synopsis display entry points

**Files:**
- Modify: `src/app.rs` (`show_synopsis_overlay` ~:4949, `cycle_synopsis` ~:5156)
- Modify: `src/input/actions/synopsis.rs` (amend :124, error :137, undo :190)

- [ ] **Step 1: app.rs sites**

After `s.gloss_overlay.show_synopsis(...)` at `src/app.rs:4949` and `:5156`, add:

```rust
    crate::input::actions::gloss::recolor_cached_blocks(&s);
```

(Confirm the local binding name at each site — both use `s`/`mut s`. `show_synopsis` does NOT call `apply_font` last in the same way; verify `self.blocks` are populated by checking `show_synopsis` builds `synopsis_blocks` ranges — it does, via `rebuild_block_ranges_from` at `gloss_overlay.rs:872`.)

- [ ] **Step 2: synopsis.rs sites**

After `s.gloss_overlay.show_synopsis(...)` at `src/input/actions/synopsis.rs:124`, `:137`, and `:190`, add:

```rust
                crate::input::actions::gloss::recolor_cached_blocks(&s);
```

(Match the indentation of each site; the binding is `s` from `state_for_result.borrow_mut()` / `state_rc.borrow_mut()`.)

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles. If `recolor_cached_blocks` isn't visible, confirm the module path (`crate::input::actions::gloss::recolor_cached_blocks`) and that it's `pub(crate)`.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs src/input/actions/synopsis.rs
git commit -m "feat(synopsis): color cached paragraphs on synopsis display

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Re-color after synthesis completes (4 sites)

**Files:**
- Modify: `src/input/actions/gloss.rs` (gloss single :1133, gloss batch :1225, synopsis batch :1307, synopsis single — the `play_synopsis_block` completion arm)

- [ ] **Step 1: Gloss single**

In `play_block_tts`'s async block, after the `save_gloss_audio` block ends (`src/input/actions/gloss.rs:1133`, before `hide_tts_toast` at :1135), add:

```rust
        recolor_cached_blocks_rc(&state_for_result);
```

- [ ] **Step 2: Gloss batch**

In `synth_all_prose_blocks`'s loop, after the `save_gloss_audio` block (`:1220-1225`, still inside the `for` loop, after the closing `}` of the `if let Ok(conn)`), add:

```rust
            recolor_cached_blocks_rc(&state_for_result);
```

This makes each block colorize as its synth lands (progressive fill).

- [ ] **Step 3: Synopsis batch**

In `synth_all_synopsis_blocks`'s loop, after the `save_synopsis_audio` block (`:1301-1307`, inside the `for` loop), add:

```rust
            recolor_cached_blocks_rc(&state_for_result);
```

- [ ] **Step 4: Synopsis single**

Find the synth-completion arm in `play_synopsis_block` (mirrors `play_block_tts`: after its `save_synopsis_audio`, before `hide_tts_toast`/`play_file`). Add:

```rust
        recolor_cached_blocks_rc(&state_for_result);
```

(Locate it by `rg -n "save_synopsis_audio" src/input/actions/gloss.rs` — the single-block one is inside `play_synopsis_block`, distinct from the batch site already handled in Step 3.)

- [ ] **Step 5: Build + clippy**

Run: `cargo build && cargo clippy 2>&1 | rg "recolor|gloss.rs" || echo clean`
Expected: compiles, no new warnings. Watch for double-borrow panics at runtime: `recolor_cached_blocks_rc` calls `state.borrow()`; ensure no enclosing `borrow_mut()` is live at these 4 points. In `play_block_tts`/`play_synopsis_block` the prior `state_for_result.borrow()` for `play_file` happens AFTER our call, so we are clear; in the batch loops nothing holds a borrow across the await boundary.

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(tts): recolor block accent the moment its mp3 is cached

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Full verification

- [ ] **Step 1: Build, clippy, unit tests**

Run:
```bash
cargo build && cargo clippy && cargo test --bins
```
Expected: all green. (No pure unit test covers tag color — this is a rendered concern.)

- [ ] **Step 2: Visual acceptance (ask the user)**

Per CLAUDE.md, "renders correctly on screen" criteria require the user to launch. Ask the user to run `cargo run`, then:
- Open a gloss (`Ctrl+g` on a glossed line) where some blocks were previously synthesized — confirm cached source/explication blocks render in the theme accent color, uncached ones in default ink.
- Press `Space` on an uncached block — when the "Synthesizing…" pill clears, that block should turn the accent color.
- Press `Shift+Space` (batch) — blocks should colorize progressively as each completes.
- Open a synopsis (`h`), repeat: cached paragraphs accent-colored; `Space`/`Shift+Space` colorize on completion.

Alternatively the headless e2e (won't exercise live TTS, only on-open coloring of pre-cached blocks):
```bash
./scripts/e2e-env.sh cargo test --test smoke -- --ignored --nocapture
```
then inspect `target/ui/` PNGs of an `h`-opened overlay.

- [ ] **Step 3: Finish the branch**

Once the user confirms the visual result, follow the project's finish-a-branch rule (merge to master, push) — or stay on the branch if the user wants more changes.

---

## Self-review notes

- **Spec coverage:** all-block scope (Task 1 colors any block kind), accent = `bar_color` (Task 1), on-open trigger (Tasks 4-5), after-synth trigger × 4 sites (Task 6), idempotent re-tag (Task 1 reuses one tag). ✓
- **Per-block voice** (gloss): handled by Task 2's shared `gloss_block_voice` so the existence check matches playback's voice + Alice fallback. ✓
- **DB-as-authority + Path::exists:** every check is `find_*_audio` then `Path::exists()`. ✓
- **Type consistency:** `recolor_cached_blocks(&AppState)` and `recolor_cached_blocks_rc(&Rc<RefCell<AppState>>)` names used identically across Tasks 3-6; `color_audio_blocks(is_cached: impl Fn(&BlockKind,i32)->bool)` signature consistent between Task 1 and its callers in Task 3.
- **Risk flagged:** line-number anchors (`:145`, `:1133`, etc.) are from a snapshot; the executor must confirm each site by content (the surrounding `show_gloss_with_color`/`save_gloss_audio` call) rather than trusting the line number.
