# Multi-page overlay paragraph retention + glossed-cursor color — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a distinct per-theme color for a glossed line that is the cursor block (Feature B), and stop multi-page synopsis/gloss overlays from dropping label/echo paragraphs the single-page path keeps (Feature A).

**Architecture:** B adds a second reader-gloss TextTag whose color comes from a new optional per-theme key (falling back to a hue-complement of the existing reddish gloss tint), and flips `repaint_reader_gloss_visible` to apply it on the cursor line. A adds a display-only `attached: Vec<Attachment>` field to `GlossBlock`; the block builders attach label/echo paragraphs to a block instead of dropping them, and the multi-page render arms emit them.

**Tech Stack:** Rust, GTK4 (gtk4-rs), serde_json, SQLite (rusqlite). Pure-logic tests via `cargo test --bins`.

**Design doc:** `docs/plans/2026-06-29-overlay-paragraph-retention-and-gloss-cursor-color-design.md`

## Global Constraints

- Build with `cargo build`; never run the app (`cargo run`) — the user runs it. (CLAUDE.md)
- Pure-logic verification: `cargo build` clean + `cargo test --bins` green + `cargo clippy` parity (no NEW warnings; baseline is 122).
- Visual acceptance (color on screen, multi-page layout) is pixel-level and CANNOT be self-verified on the live dwl seat — ASK the user to run `./scripts/e2e-env.sh` or a manual launch, per CLAUDE.md "When to ASK THE USER to run e2e-env.sh".
- Timestamps US Central (`TZ='America/Chicago'`).
- US English spelling in code/comments (existing file convention: "color", not "colour").
- Commit messages end with the Co-Authored-By + Claude-Session trailer (see CLAUDE.md / existing commits).
- `~/.config/linux-lit/config-dev.json` is the dev config; do not touch it.

---

# FEATURE B — glossed-cursor 3rd color (implement first)

## Task B1: hue-complement helper in theme.rs

**Files:**
- Modify: `src/theme.rs` (add `complement_hex` fn near the other color helpers, after `rgb_to_hsl`/`hsl_to_rgb`, ~line 338; add a unit test in the existing `#[cfg(test)] mod tests` if present, else a new one at end of file)

**Interfaces:**
- Consumes: existing `hex_to_rgb(&str) -> (f64,f64,f64)`, `rgb_to_hsl(f64,f64,f64) -> (f64,f64,f64)`, `hsl_to_rgb(f64,f64,f64) -> (f64,f64,f64)`, `rgb_to_hex(f64,f64,f64) -> String` (all already in `src/theme.rs`).
- Produces: `fn complement_hex(hex: &str) -> String` — returns the hex color with hue rotated 180° (0.5 in [0,1] hue space), same S and L. Used by `Theme::load` as the fallback for `reader_gloss_cursor`.

- [ ] **Step 1: Write the failing test**

Add to `src/theme.rs` test module:

```rust
#[test]
fn complement_rotates_hue_180() {
    // rose-pine-dawn focuscolor #c4788a (a rose-red) -> a teal/green complement.
    let c = complement_hex("#c4788a");
    let (h_in, _, _) = rgb_to_hsl(hex_to_rgb("#c4788a").0, hex_to_rgb("#c4788a").1, hex_to_rgb("#c4788a").2);
    let (h_out, _, _) = rgb_to_hsl(hex_to_rgb(&c).0, hex_to_rgb(&c).1, hex_to_rgb(&c).2);
    // hue moved ~0.5 (180°), wrapping mod 1.0
    let diff = ((h_out - h_in).abs() - 0.5).abs();
    assert!(diff < 0.02, "expected ~0.5 hue rotation, got in={h_in} out={h_out} ({c})");
    // and it is a green-ish hue (teal), not red: hue in [0.33, 0.66]
    assert!((0.33..=0.70).contains(&h_out), "complement of a red should be green/teal, got hue {h_out} ({c})");
}

#[test]
fn complement_malformed_is_safe() {
    // hex_to_rgb returns (0,0,0) for malformed; complement must still return a hex.
    let c = complement_hex("not-a-color");
    assert!(c.starts_with('#') && c.len() == 7, "got {c}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins theme::tests::complement -- --nocapture`
Expected: FAIL — `complement_hex` not found (cannot find function).

- [ ] **Step 3: Write minimal implementation**

Add after `hsl_to_rgb` (around line 338) in `src/theme.rs`:

```rust
/// Return `hex` with its hue rotated 180° (the color-wheel complement), keeping
/// saturation and lightness. Used as the per-theme fallback for the
/// glossed-cursor tint (the "opposite of the reddish" gloss color) when a theme
/// does not define `linux-lit.reader_gloss_cursor`. Malformed input degrades to
/// the complement of black (still a valid `#rrggbb`), never panics.
fn complement_hex(hex: &str) -> String {
    let (r, g, b) = hex_to_rgb(hex);
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let (nr, ng, nb) = hsl_to_rgb((h + 0.5) % 1.0, s, l);
    rgb_to_hex(nr, ng, nb)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins theme::tests::complement -- --nocapture`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add src/theme.rs
git commit -m "$(cat <<'EOF'
feat(theme): complement_hex helper (180° hue rotation)

Composes the existing hex/hsl helpers to produce a color-wheel complement.
Used next as the per-theme fallback for the glossed-cursor tint.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task B2: `reader_gloss_cursor` field on Theme + load/default

**Files:**
- Modify: `src/theme.rs` — `Theme` struct (~line 19, after `vocab_fg`); `load`/builder return (the `Theme { ... }` at ~164); `default_theme` (~180).

**Interfaces:**
- Consumes: `complement_hex` (Task B1); existing `str_field`, `focus_color`, `lit` (`val.get("linux-lit")`).
- Produces: `Theme.reader_gloss_cursor: String` — the glossed-cursor foreground color. Read from `linux-lit.reader_gloss_cursor` when present, else `complement_hex(&focus_color)`.

- [ ] **Step 1: Write the failing test**

Add to `src/theme.rs` test module:

```rust
#[test]
fn reader_gloss_cursor_explicit_wins() {
    let json: serde_json::Value = serde_json::from_str(
        r#"{ "dwl": {"focuscolor": "#c4788a"},
             "linux-lit": {"reader_gloss_cursor": "#56949f"},
             "kitty": {"background": "#faf4ed"} }"#,
    ).unwrap();
    let t = resolve_theme("rose-pine-dawn", &json);
    assert_eq!(t.reader_gloss_cursor, "#56949f");
}

#[test]
fn reader_gloss_cursor_falls_back_to_complement() {
    let json: serde_json::Value = serde_json::from_str(
        r#"{ "dwl": {"focuscolor": "#c4788a"}, "kitty": {"background": "#faf4ed"} }"#,
    ).unwrap();
    let t = resolve_theme("x", &json);
    assert_eq!(t.reader_gloss_cursor, complement_hex("#c4788a"));
}
```

NOTE: the per-value constructor is the existing private `fn resolve_theme(name: &str, val: &Value) -> Theme` (src/theme.rs:90), callable from the in-file test module. `load_theme`/`load_all_themes` are thin file-reading wrappers around it — do NOT test through them (they read the real themes-unified.json). No `from_value` exists; do not invent one.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins theme::tests::reader_gloss_cursor -- --nocapture`
Expected: FAIL — no `reader_gloss_cursor` field (or no `from_value`).

- [ ] **Step 3: Write minimal implementation**

In `src/theme.rs`:

1. Add the field to `struct Theme` after `vocab_fg: String,` (~line 19):

```rust
    pub reader_gloss_cursor: String, // glossed line that is ALSO the cursor block
```

2. In `resolve_theme` (the per-value builder), after the `focus_color` line (~132) and after `let lit = ...` (~134), add:

```rust
    let reader_gloss_cursor = str_field(&lit, "reader_gloss_cursor")
        .unwrap_or_else(|| complement_hex(&focus_color));
```

3. Add `reader_gloss_cursor,` to the returned `Theme { ... }` (after `vocab_fg,` ~176).

4. In `default_theme` (~193, after `vocab_fg`), add:

```rust
        reader_gloss_cursor: complement_hex("#d4be98"),
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins theme::tests::reader_gloss_cursor -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Build + full bins + commit**

```bash
cargo build 2>&1 | tail -3
cargo test --bins 2>&1 | tail -3
git add src/theme.rs
git commit -m "$(cat <<'EOF'
feat(theme): reader_gloss_cursor color (explicit key or hue complement)

New optional per-theme linux-lit.reader_gloss_cursor; falls back to the
180° complement of the reddish focuscolor so all themes get a sensible
glossed-cursor color without per-theme entries.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task B3: rose-pine-dawn explicit value in themes-unified.json

**Files:**
- Modify: `~/utono/themes/.config/themes/themes-unified.json` — the `rose-pine-dawn` object's `linux-lit` block (currently `{"cursor_line_bg": "rgba(196, 120, 138, 0.2)"}`).

**Interfaces:**
- Consumes: nothing (data file).
- Produces: `rose-pine-dawn.linux-lit.reader_gloss_cursor == "#56949f"` (rosé-pine "foam").

NOTE: this file is in a SEPARATE repo (`~/utono/themes`), not linux-lit. Edit + commit it there separately; it is not part of any linux-lit commit.

- [ ] **Step 1: Add the key**

Edit the `rose-pine-dawn` → `linux-lit` object to:

```json
"linux-lit": { "cursor_line_bg": "rgba(196, 120, 138, 0.2)", "reader_gloss_cursor": "#56949f" }
```

- [ ] **Step 2: Verify JSON is valid**

Run: `jq '."rose-pine-dawn"."linux-lit".reader_gloss_cursor' ~/utono/themes/.config/themes/themes-unified.json`
Expected: `"#56949f"`

- [ ] **Step 3: Commit in the themes repo**

```bash
cd ~/utono/themes && git add .config/themes/themes-unified.json && git commit -m "$(cat <<'EOF'
feat(rose-pine-dawn): reader_gloss_cursor #56949f (foam) for linux-lit

The glossed-cursor color for linux-lit's reading card: rosé-pine "foam",
the complement of the #c4788a focuscolor gloss tint.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
cd ~/utono/linux-lit
```

---

## Task B4: second TextTag + apply/remove helpers + state field

**Files:**
- Modify: `src/app/mod.rs` — `AppState` field (~392, after `pub reader_gloss_tag`); tag creation (~914, after `reader_gloss_tag` add); state construction (~1550, after `reader_gloss_tag,`); add helpers next to `apply_reader_gloss_tag_to_line`/`remove_reader_gloss_tag_from_line` (~3818–3838).

**Interfaces:**
- Consumes: `theme.reader_gloss_cursor` (Task B2).
- Produces:
  - `AppState.reader_gloss_cursor_tag: gtk4::TextTag`
  - `pub(crate) fn apply_reader_gloss_cursor_tag_to_line(state: &AppState, buf_idx: usize)`
  - `pub(crate) fn remove_reader_gloss_cursor_tag_from_line(state: &AppState, buf_idx: usize)`

This task has no standalone unit test (it is GTK widget wiring); it is verified by `cargo build` and consumed by Task B5's behavior. Fold it into B5's review gate.

- [ ] **Step 1: Add the AppState field**

After `pub reader_gloss_tag: gtk4::TextTag,` (~392):

```rust
    /// Foreground tag for a glossed line that is ALSO the cursor block — a
    /// distinct color (theme.reader_gloss_cursor) so it reads differently from
    /// both body text and the off-cursor reddish gloss tint. Applied by
    /// `repaint_reader_gloss_visible` on the cursor line.
    pub reader_gloss_cursor_tag: gtk4::TextTag,
```

- [ ] **Step 2: Create the tag**

After the `reader_gloss_tag` block (after `buffer.tag_table().add(&reader_gloss_tag);` ~918):

```rust
    // The glossed-cursor tint: same role as reader-gloss-line but a distinct
    // color, applied to a glossed line WHILE it is the cursor block. Added after
    // reader-gloss-line so it outranks it on the cursor's own line.
    let reader_gloss_cursor_tag = gtk4::TextTag::builder()
        .name("reader-gloss-cursor-line")
        .foreground(&theme.reader_gloss_cursor)
        .build();
    buffer.tag_table().add(&reader_gloss_cursor_tag);
```

- [ ] **Step 3: Store it in AppState**

After `reader_gloss_tag,` in the construction (~1550):

```rust
        reader_gloss_cursor_tag,
```

- [ ] **Step 4: Add the apply/remove helpers**

After `remove_reader_gloss_tag_from_line` (~3838):

```rust
/// Apply the glossed-cursor tint to a single buffer line (used when the cursor
/// is on a glossed line, so it reads in the distinct color rather than the
/// off-cursor reddish tint).
pub(crate) fn apply_reader_gloss_cursor_tag_to_line(state: &AppState, buf_idx: usize) {
    if let Some(start) = state.buffer.iter_at_line(buf_idx as i32) {
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        state.buffer.apply_tag(&state.reader_gloss_cursor_tag, &start, &end);
    }
}

/// Remove the glossed-cursor tint from a single buffer line (used when the cursor
/// leaves a glossed line, so it reverts to the off-cursor reddish tint).
pub(crate) fn remove_reader_gloss_cursor_tag_from_line(state: &AppState, buf_idx: usize) {
    if let Some(start) = state.buffer.iter_at_line(buf_idx as i32) {
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        state.buffer.remove_tag(&state.reader_gloss_cursor_tag, &start, &end);
    }
}
```

- [ ] **Step 5: Build**

Run: `cargo build 2>&1 | tail -3`
Expected: Finished (clean). No commit yet — commit with B5.

---

## Task B5: flip `repaint_reader_gloss_visible` + theme-change refresh

**Files:**
- Modify: `src/input/highlight.rs` — `repaint_reader_gloss_visible` (~346–357).
- Modify: `src/input/actions/settings.rs` — theme-change refresh (~285, after the `reader_gloss_tag` foreground set).

**Interfaces:**
- Consumes: `apply_reader_gloss_cursor_tag_to_line` / `remove_reader_gloss_cursor_tag_from_line` (B4); `state.reader_gloss_cursor_tag` (B4); `theme.reader_gloss_cursor` (B2).
- Produces: three-state behavior (normal / reddish-off-cursor / new-color-on-cursor).

No new unit test (behavior is GTK-tag application on a live buffer, covered by the e2e visual check). Verified by `cargo build` + `cargo test --bins` parity + the user's screenshot.

- [ ] **Step 1: Flip the cursor-line case in repaint_reader_gloss_visible**

Replace the body of the loop in `src/input/highlight.rs` (~350–356):

```rust
    for &buf_idx in &state.reader_gloss_lines {
        if buf_idx == state.current_line {
            crate::app::remove_reader_gloss_tag_from_line(state, buf_idx);
        } else {
            crate::app::apply_reader_gloss_tag_to_line(state, buf_idx);
        }
    }
```

with:

```rust
    for &buf_idx in &state.reader_gloss_lines {
        if buf_idx == state.current_line {
            // Cursor is on a glossed line: show the distinct glossed-cursor color
            // (not the off-cursor reddish tint).
            crate::app::remove_reader_gloss_tag_from_line(state, buf_idx);
            crate::app::apply_reader_gloss_cursor_tag_to_line(state, buf_idx);
        } else {
            // Off-cursor glossed line: reddish tint; clear any stale cursor color.
            crate::app::remove_reader_gloss_cursor_tag_from_line(state, buf_idx);
            crate::app::apply_reader_gloss_tag_to_line(state, buf_idx);
        }
    }
```

Also update the doc comment above the fn (~339–345): it currently says the cursor line is left un-tinted ("the cursor-line highlight wins on its own line") — change it to say the cursor line now gets the distinct glossed-cursor color.

- [ ] **Step 2: Refresh the new tag's color on theme change**

In `src/input/actions/settings.rs`, after the existing line (~285):

```rust
    state.reader_gloss_tag.set_property("foreground", &theme.focus_color);
```

add:

```rust
    // Glossed-cursor tint tracks the theme too (the "opposite of the reddish").
    state.reader_gloss_cursor_tag.set_property("foreground", &theme.reader_gloss_cursor);
```

- [ ] **Step 3: Build + bins + clippy**

```bash
cargo build 2>&1 | tail -3
cargo test --bins 2>&1 | tail -3
cargo clippy 2>&1 | rg -c '^warning'   # expect 122 (baseline; no new)
```
Expected: build Finished; bins 519+ pass (517 baseline + new theme tests), 0 fail; clippy count 122.

- [ ] **Step 4: Commit B4 + B5 together**

```bash
git add src/app/mod.rs src/input/highlight.rs src/input/actions/settings.rs
git commit -m "$(cat <<'EOF'
feat(reader): distinct color for a glossed line that is the cursor block

A glossed line now has three states on the reading card: normal body text
(not glossed); the reddish focuscolor tint (glossed, off-cursor); and a
distinct theme.reader_gloss_cursor color (glossed AND the cursor block).
Adds the reader-gloss-cursor-line TextTag + apply/remove helpers, flips
repaint_reader_gloss_visible to apply it on the cursor line instead of
stripping the tint, and refreshes its color on theme change.

Logic-verified (cargo test --bins); visual acceptance needs an e2e
screenshot on rose-pine-dawn — see ac.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

- [ ] **Step 5: ASK THE USER to verify on screen**

Per CLAUDE.md, the agent cannot launch on the live dwl seat. Ask the user to launch on rose-pine-dawn, open Bleak House → "In Chancery", and confirm:
- cursor ON the glossed first paragraph → it renders teal `#56949f`;
- cursor OFF it (on para 2) → the glossed para renders reddish `#c4788a`;
- a non-glossed cursor line renders normal slate body text.

Provide the manual single-work launch from CLAUDE.md "Headless Verification" + `grim`, or `./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture` for a smoke screenshot.

---

# FEATURE A — multi-page label/echo retention (implement second)

## Task A1: `Attachment` enum + `attached` field on GlossBlock

**Files:**
- Modify: `src/ui/gloss_block.rs` — add `Attachment` enum near `BlockKind` (~71); add `attached: Vec<Attachment>` to `GlossBlock` (~79); update ALL `GlossBlock { ... }` literals in the file to add `attached: Vec::new()` (in `synopsis_blocks` ~159 & ~172, `gloss_blocks`' `flush_source` ~198 and explication push ~222).

**Interfaces:**
- Produces:
  - `pub enum Attachment { LeadLabel(String), TrailEcho(String) }` (derive `Clone`, `Debug`, `PartialEq`, `Eq`).
  - `GlossBlock.attached: Vec<Attachment>` — display-only paragraphs riding with the block.

This is a pure data change; it compiles and existing tests still pass (every block just gets an empty `attached`). Its own test asserts the default is empty.

- [ ] **Step 1: Write the failing test**

Add to the `block_tests` module in `src/ui/gloss_block.rs`:

```rust
#[test]
fn blocks_default_to_no_attachments() {
    let g = "<speaker>X</speaker>\n<verse>a line</verse>\n<gloss>note</gloss>";
    for b in gloss_blocks(g) {
        assert!(b.attached.is_empty(), "fresh block must have no attachments");
    }
    for b in synopsis_blocks("<p>One.</p><p>Two.</p>") {
        assert!(b.attached.is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins gloss_block -- --nocapture`
Expected: FAIL — no field `attached` on `GlossBlock`.

- [ ] **Step 3: Implement the enum + field**

In `src/ui/gloss_block.rs`, after `BlockKind` (~75):

```rust
/// A non-cursor-stop paragraph that rides WITH a block on a paginated page so it
/// is not dropped at a page boundary (the single-page render keeps it; without
/// this the multi-page render would lose it). Display-only — never a cursor stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Attachment {
    /// A bold label paragraph that HEADS the block (synopsis, e.g.
    /// "Shakespearean parallels:"). Rendered above the block body.
    LeadLabel(String),
    /// An echo-bracket markup ("<gloss>[...]</gloss>") that TRAILS the block
    /// (gloss). Rendered below the block body via the echo render path.
    TrailEcho(String),
}
```

Add to `GlossBlock` after `pub display: String,` (~90):

```rust
    /// Non-cursor-stop paragraphs that ride with this block on a paginated page.
    /// Empty in the common case. See `Attachment`.
    pub attached: Vec<Attachment>,
```

Add `attached: Vec::new(),` to EVERY `GlossBlock { ... }` literal in the file:
- `synopsis_blocks` legacy single block (~159) and the per-`<p>` push (~172),
- `gloss_blocks`'s `flush_source` closure source push (~198) and the explication push (~222).

(Find them all: `rg -n "GlossBlock \{" src/ui/gloss_block.rs` — there should be exactly 4. Add the field to each.)

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test --bins gloss_block -- --nocapture`
Expected: PASS (new test + all existing block/synopsis tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_block.rs
git commit -m "$(cat <<'EOF'
feat(gloss-block): Attachment enum + GlossBlock.attached (empty default)

Display-only paragraphs that ride with a block across a page turn. No
behavior change yet — every block gets an empty attached; the builders
populate it in the next tasks.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task A2: `synopsis_blocks` attaches labels as LeadLabel

**Files:**
- Modify: `src/ui/gloss_block.rs` — `synopsis_blocks` (~140–181).

**Interfaces:**
- Consumes: `Attachment::LeadLabel` (A1); existing `is_label_paragraph`.
- Produces: `synopsis_blocks` returns the SAME blocks/indices as before, but each block's `attached` carries any label paragraph(s) immediately preceding it as `LeadLabel`. A trailing label (after the last block) attaches to the LAST block.

- [ ] **Step 1: Write the failing test**

Add to `synopsis_blocks_tests` in `src/ui/gloss_block.rs`:

```rust
#[test]
fn label_attaches_as_lead_to_following_block() {
    let syn = "<p>First paragraph of action.</p>\
               <p>Shakespearean parallels:</p>\
               <p>Second paragraph continues.</p>";
    let blocks = synopsis_blocks(syn);
    // Still 2 cursor-stop blocks, indices unchanged.
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].index, 0);
    assert_eq!(blocks[1].index, 1);
    // Block 0 has no lead; the label heads block 1.
    assert!(blocks[0].attached.is_empty());
    assert_eq!(
        blocks[1].attached,
        vec![super::Attachment::LeadLabel("Shakespearean parallels:".to_string())]
    );
}

#[test]
fn trailing_label_attaches_to_last_block() {
    let syn = "<p>Only paragraph.</p><p>Afterword:</p>";
    let blocks = synopsis_blocks(syn);
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0].attached,
        vec![super::Attachment::LeadLabel("Afterword:".to_string())]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins synopsis_blocks_tests -- --nocapture`
Expected: FAIL — `attached` is empty (labels still skipped).

- [ ] **Step 3: Implement label buffering**

Rewrite the block-emitting loop in `synopsis_blocks` (~166–180) so a label is buffered and attached to the next emitted block; a trailing label attaches to the last:

```rust
    let mut blocks: Vec<GlossBlock> = Vec::new();
    let mut index = 0i32;
    let mut pending_labels: Vec<Attachment> = Vec::new();
    for p in &paras {
        if is_label_paragraph(p) {
            pending_labels.push(Attachment::LeadLabel(p.clone()));
            continue;
        }
        blocks.push(GlossBlock {
            kind: BlockKind::Explication,
            index,
            text: p.clone(),
            display: p.clone(),
            attached: std::mem::take(&mut pending_labels),
        });
        index += 1;
    }
    // A label after the last block: attach to the last block (so it is not lost).
    if !pending_labels.is_empty() {
        if let Some(last) = blocks.last_mut() {
            last.attached.append(&mut pending_labels);
        }
    }
    blocks
```

(Import `Attachment` is already in-module — same file. No `use` needed.)

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test --bins gloss_block -- --nocapture`
Expected: PASS (new tests + existing `each_p_becomes_one_explication_block_skipping_labels`, which still sees 2 blocks).

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_block.rs
git commit -m "$(cat <<'EOF'
feat(synopsis): attach label paragraphs as LeadLabel to the next block

synopsis_blocks no longer drops a label paragraph; it buffers it and
attaches it to the block it heads (trailing label -> last block). Block
count and indices are unchanged; the multi-page render reads attached.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task A3: `gloss_block_markups` attaches echoes as TrailEcho

**Files:**
- Modify: `src/ui/gloss_block.rs` — `gloss_block_markups` (~261–301). NOTE: `gloss_block_markups` returns `Vec<String>` (markup per block), NOT `Vec<GlossBlock>`. The echo must be appended to the PRECEDING block's markup STRING (so the existing `markups[start..end].join` carries it). `gloss_blocks` (the `Vec<GlossBlock>`) stays unchanged — echoes are still not cursor stops there.

**Interfaces:**
- Consumes: existing `split_echo`, `parse_gloss_tags`.
- Produces: `gloss_block_markups` returns one markup per cursor-stop block (count == `gloss_blocks().len()`, unchanged), but a markup now also contains any echo `<gloss>[...]</gloss>` that trailed it (appended after the block's own markup, separated by `\n`).

- [ ] **Step 1: Write the failing test**

Replace/extend the existing `block_markups_match_blocks_count_and_order` test's echo assertion. Add a NEW test to `block_tests`:

```rust
#[test]
fn echo_attaches_to_preceding_block_markup() {
    let gloss = "<speaker>CRANMER</speaker>\n\
                 <verse>Ah, my good Lord of Winchester, I thank you.</verse>\n\
                 <gloss>Cranmer opens with cutting irony.</gloss>\n\
                 <gloss>[\"a quote\" — Macbeth 1.1]</gloss>";
    let blocks = gloss_blocks(gloss);
    let markups = gloss_block_markups(gloss);
    // Count still 1:1 with cursor-stop blocks (echo is NOT a new entry).
    assert_eq!(markups.len(), blocks.len());
    // The echo rides in the LAST block's markup (the explication it trails).
    assert!(
        markups.last().unwrap().contains("a quote"),
        "echo must be appended to the preceding block markup, got {:?}",
        markups.last()
    );
}
```

ALSO update `block_markups_match_blocks_count_and_order` (~643): its final assertion is `assert!(!markups.iter().any(|m| m.contains("a quote")));` — that is now WRONG (the echo is retained). Change it to assert the echo IS present in some markup:

```rust
        // The echo bracket now RIDES in the preceding block's markup (retained
        // across page turns) rather than being dropped.
        assert!(markups.iter().any(|m| m.contains("a quote")),
            "echo should be retained in a block markup");
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins gloss_block -- --nocapture`
Expected: FAIL — echo absent (`block_markups_match_blocks_count_and_order` now fails its flipped assert; the new test fails).

- [ ] **Step 3: Implement echo attachment**

In `gloss_block_markups`, change the echo arm so the echo is appended to the last pushed markup instead of `continue`-dropping. Replace the `Gloss` arm (~289–295):

```rust
            GlossElement::Gloss(text) => {
                if split_echo(&text).is_some() {
                    // Echo bracket: not a cursor stop, but RETAIN it by appending
                    // to the preceding block's markup so a page turn keeps it.
                    // (An echo before any block — not observed — flushes the
                    // pending source run first, then attaches to it.)
                    flush_source(&mut markups, &mut pending, &mut pending_has_body);
                    let echo = format!("<gloss>{}</gloss>", text);
                    if let Some(last) = markups.last_mut() {
                        last.push('\n');
                        last.push_str(&echo);
                    } else {
                        markups.push(echo);
                    }
                    continue;
                }
                flush_source(&mut markups, &mut pending, &mut pending_has_body);
                markups.push(format!("<gloss>{}</gloss>", text));
            }
```

WARNING — count invariant: when an echo trails a SOURCE block with no explication between (the all-echo gloss case, e.g. the `["..." — ...]`-only glosses in lit.db), `flush_source` first pushes the source markup, then the echo appends to it — so count stays 1:1 with `gloss_blocks` (which also makes one Source block there). The "echo before any block" `else` branch pushes a lone markup; this would break 1:1, but no such data exists (every observed echo follows a source). Keep the `else` as a safety net but it should never fire for real data.

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test --bins gloss_block -- --nocapture`
Expected: PASS — including `all_echo_gloss_has_only_source_block` (gloss_blocks unchanged), `block_markups_match_blocks_count_and_order` (flipped assert), and the new test. If `block_markups_lone_pron_yields_nothing` or the all-echo count tests fail, the `else` branch fired — re-check.

VERIFY the all-echo case count explicitly by adding:

```rust
#[test]
fn all_echo_gloss_markups_count_matches_blocks() {
    // The lit.db echoes-only style: every gloss is an echo trailing its source.
    let g = "<speaker>PARIS</speaker>\n<verse>Come you to make confession?</verse>\n\
             <gloss>[\"q1\" — Ado 4.1]</gloss>\n\
             <speaker>JULIET</speaker>\n<verse>To answer that.</verse>\n\
             <gloss>[\"q2\" — Oth 1.1]</gloss>";
    assert_eq!(gloss_block_markups(g).len(), gloss_blocks(g).len());
    // Each echo rides with its own source block.
    let m = gloss_block_markups(g);
    assert!(m[0].contains("q1") && m[1].contains("q2"), "got {m:?}");
}
```

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_block.rs
git commit -m "$(cat <<'EOF'
feat(gloss): retain echo brackets in the preceding block's markup

gloss_block_markups no longer drops echo <gloss>[...] brackets; it appends
each to the markup of the block it trails, so a paginated gloss keeps its
echoes (markups stay 1:1 with gloss_blocks). gloss_blocks (cursor stops)
is unchanged — echoes are still not cursor stops.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task A4: measure attachments in `gloss_block_height`

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — `gloss_block_height` (find it: `rg -n "fn gloss_block_height" src/ui/gloss_overlay.rs`).

**Interfaces:**
- Consumes: `GlossBlock.attached` (A1); existing `measure_text_height` / pango context already used in `gloss_block_height`.
- Produces: `gloss_block_height` returns a height that INCLUDES the block's attachments, so pagination reserves room for them and nothing clips.

No standalone unit test (it needs a live pango context, like the rest of `gloss_block_height` — there are deliberately no pure tests for the measurement path per CLAUDE.md). Verified by `cargo build` + the e2e spread.

- [ ] **Step 1: The current function (already read)**

`gloss_block_height` (src/ui/gloss_overlay.rs:2188) is exactly:

```rust
fn gloss_block_height(block: &GlossBlock, pctx: &pango::Context, family: &str, size_pt: i32, wrap_w: i32) -> i32 {
    let text_h = crate::ui::pagination::measure_text_height(pctx, &block.display, size_pt, family, wrap_w);
    block_height_overhead(block.kind == BlockKind::Source, text_h)
}
```

There is NO line-height local. Use `measure_text_height` (already imported via the `crate::ui::pagination` path) for each attachment and a conservative per-attachment line allowance of `size_pt + size_pt/2` (over-measure is the safe direction per `repaginate`'s comment).

- [ ] **Step 2: Add attachment height**

Rewrite `gloss_block_height` to add the attachments' measured height before returning:

```rust
fn gloss_block_height(block: &GlossBlock, pctx: &pango::Context, family: &str, size_pt: i32, wrap_w: i32) -> i32 {
    let text_h = crate::ui::pagination::measure_text_height(pctx, &block.display, size_pt, family, wrap_w);
    let mut h = block_height_overhead(block.kind == BlockKind::Source, text_h);
    // Reserve room for attachments so a paginated page never clips them
    // (over-measure: one extra line allowance per attachment as a gap/citation).
    let line = size_pt + size_pt / 2;
    for a in &block.attached {
        match a {
            crate::ui::gloss_block::Attachment::LeadLabel(s) => {
                h += crate::ui::pagination::measure_text_height(pctx, s, size_pt, family, wrap_w) + line;
            }
            crate::ui::gloss_block::Attachment::TrailEcho(markup) => {
                let inner = markup.trim_start_matches("<gloss>").trim_end_matches("</gloss>");
                h += crate::ui::pagination::measure_text_height(pctx, inner, size_pt, family, wrap_w) + line * 2;
            }
        }
    }
    h
}
```

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | tail -3`
Expected: Finished clean.

- [ ] **Step 4: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "$(cat <<'EOF'
feat(overlay): measure block attachments in gloss_block_height

Pagination now reserves room for a block's lead labels / trailing echoes
so a paginated page never clips them. Over-measures (the safe direction).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task A5: emit LeadLabels in `render_synopsis_page` multi-page branch

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — `render_synopsis_page` multi-page branch (~1462–1476).

**Interfaces:**
- Consumes: `GlossBlock.attached` / `Attachment::LeadLabel` (A1); existing `synopsis_label_ranges`, `apply_synopsis_label_bold`, `rebuild_block_ranges_from`.
- Produces: the multi-page synopsis page body includes each block's `LeadLabel`(s) bolded above its body, and `synopsis_label_ranges` is set to those labels' char offsets (instead of cleared).

No unit test (GTK buffer render); verified by `cargo build` + the e2e screenshot of a multi-page synopsis.

- [ ] **Step 1: Rewrite the multi-page body build**

Replace the `else` (paginated) branch of `render_synopsis_page` (~1462–1476):

```rust
        } else {
            // Paginated: render this page's blocks, each preceded by its lead
            // label(s) (bolded) so labels survive the page turn. Track label
            // char-offset ranges in the page text so apply_synopsis_label_bold
            // can bold them.
            let Some(page) = page else { return };
            let all = self.all_blocks.borrow();
            let slice: Vec<GlossBlock> = all[page.start..page.end.min(all.len())].to_vec();
            drop(all);
            let mut body = String::new();
            let mut label_ranges: Vec<(usize, usize)> = Vec::new();
            let mut char_off = 0usize; // char offset into `body`
            for b in &slice {
                for a in &b.attached {
                    if let crate::ui::gloss_block::Attachment::LeadLabel(lbl) = a {
                        if !body.is_empty() {
                            body.push_str("\n\n");
                            char_off += 2;
                        }
                        let len = lbl.chars().count();
                        label_ranges.push((char_off, char_off + len));
                        body.push_str(lbl);
                        char_off += len;
                    }
                }
                if !body.is_empty() {
                    body.push_str("\n\n");
                    char_off += 2;
                }
                let len = b.display.chars().count();
                body.push_str(&b.display);
                char_off += len;
            }
            buffer.set_text(&body);
            *self.synopsis_label_ranges.borrow_mut() = label_ranges;
            self.apply_synopsis_label_bold();
            self.rebuild_block_ranges_from(slice);
        }
```

CONFIRMED (no change needed): `rebuild_block_ranges_from` (src/ui/gloss_overlay.rs:1173) finds each block by SEARCHING the buffer text for the block's first `display` line (`find_line` → `line_text.trim().starts_with(needle)`, advancing `search_from`). Inserted lead-label lines are simply skipped by the search, so block ranges stay correct with labels present. No edit to `rebuild_block_ranges_from`. (Soft edge: a label whose text `starts_with` a following block's first-line needle could match one line early — bounded and harmless, since labels are short `:`-terminated headings unlikely to prefix a body line.)

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | tail -3`
Expected: Finished clean.

- [ ] **Step 3: bins**

Run: `cargo test --bins 2>&1 | tail -3`
Expected: all pass (no pure test covers this; just ensure nothing else broke).

- [ ] **Step 4: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "$(cat <<'EOF'
feat(synopsis): render lead labels on paginated pages

The multi-page synopsis render now emits each block's lead label(s) bolded
above its body (was: dropped), tracking label char ranges so
apply_synopsis_label_bold bolds them. Single-page path unchanged.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

---

## Task A6: confirm gloss echoes render on paginated pages + e2e

**Files:**
- Inspect: `src/ui/gloss_overlay.rs` — `render_gloss_page` multi-page branch (~1526–1535). It already does `markups[start..end].join("\n")` → `populate_gloss_buffer`; since A3 put the echo INTO the block markup, echoes now render with NO code change here.

**Interfaces:**
- Consumes: A3's echo-bearing markups; existing `populate_gloss_buffer` echo render path.
- Produces: paginated gloss pages render their echoes. (Verification task — no new production code expected; if `rebuild_block_ranges_from` in this arm needs the same care as A5, fix it.)

- [ ] **Step 1: Read render_gloss_page multi-page branch**

Confirm `body = markups[start..end].join("\n")` and that `populate_gloss_buffer(&markup, ...)` is called on it. Since the echo is now inside `markups[i]`, it is in `body`, so `populate_verse_buffer`'s `GlossElement::Gloss` + `split_echo` arm renders it. No change expected.

- [ ] **Step 2: Build + full bins + clippy**

```bash
cargo build 2>&1 | tail -3
cargo test --bins 2>&1 | tail -3
cargo clippy 2>&1 | rg -c '^warning'   # expect 122 baseline
```
Expected: build clean; bins green; clippy 122 (no new).

- [ ] **Step 3: Commit (if any fix was needed; else skip)**

Only if `render_gloss_page`/`rebuild_block_ranges_from` needed a tweak:

```bash
git add src/ui/gloss_overlay.rs
git commit -m "$(cat <<'EOF'
fix(gloss): keep paginated block ranges correct with retained echoes

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01QBmkjF6UmwopCrhALaGQgj
EOF
)"
```

- [ ] **Step 4: ASK THE USER to verify both A cases on screen**

Per CLAUDE.md, ask the user to run the e2e (or manual launch) and confirm on a MULTI-PAGE overlay:
- a synopsis whose label paragraph ("…parallels:") falls on a later page shows the bold label on that page (not dropped);
- a gloss with an echo bracket spanning >1 page shows the echo bracket on its page.

Command:

```bash
./scripts/e2e-env.sh cargo test -- --ignored --nocapture
```

and the manual single-work synopsis/gloss launch from CLAUDE.md "Headless Verification" + `grim` so they can eyeball the exact paginated spread. Open every PNG in `target/ui/` and report what you see (UI review protocol).

---

## Self-Review (completed)

**Spec coverage:**
- B color source (new theme key + complement fallback) → B1, B2, B3. ✓
- B three states + repaint flip + theme refresh → B4, B5. ✓
- A scope (labels + echoes; pron excluded) → A2 (labels), A3 (echoes); pron untouched by design. ✓
- A attach rule (lead→following, trail→preceding) → A2, A3. ✓
- A pagination measures attachments → A4. ✓
- A multi-page render emits attachments → A5 (synopsis), A6 (gloss verify). ✓

**Placeholder scan:** No "TBD/TODO". All three previously-speculative spots are now resolved against the real source: B2 uses the existing `resolve_theme(name, &Value)` (theme.rs:90), A4 uses the real `gloss_block_height`/`block_height_overhead` shape (theme.rs:2188), A5's `rebuild_block_ranges_from` is confirmed text-search-based (gloss_overlay.rs:1173) so it needs no edit.

**Type consistency:** `Attachment::{LeadLabel, TrailEcho}` used identically across A1/A2/A3/A4/A5. `reader_gloss_cursor` (field) / `reader-gloss-cursor-line` (tag name) / `reader_gloss_cursor_tag` (state field) / `apply_reader_gloss_cursor_tag_to_line` consistent across B2/B4/B5. `complement_hex` consistent B1/B2. `resolve_theme` is the real per-value builder (not `from_value`).

**Known soft spot flagged inline (not a gap):** the echo count invariant in `gloss_block_markups` (A3 step 3 warning + the explicit `all_echo_gloss_markups_count_matches_blocks` test) — the only place the 1:1 markup/block count could drift. Covered by a test and the never-fires `else` safety net.
