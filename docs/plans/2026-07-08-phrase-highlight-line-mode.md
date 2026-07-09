# Phrase Highlight Line Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a LINE mode to the karaoke narration highlight — Alt+p cycles Off → Phrase → Line per work class; Line tints the whole verse line (plays/poetry) or the current sentence (prose) instead of the spoken phrase span.

**Architecture:** A `PhraseHighlightMode` enum replaces the two config booleans in-place with legacy-bool-compatible deserialization. The driver (`update_phrase_highlight`) keeps all of today's gating, spoken-line resolution, and phrase-span caching; only the applied character range widens in Line mode, via a pure `tint_range()` + `sentence_bounds()` pair. Spec: `docs/plans/2026-07-08-phrase-highlight-line-mode-design.md`.

**Tech Stack:** Rust, GTK4 (`gtk4::TextBuffer` tags), serde/serde_json.

## Global Constraints

- **Bin-only crate:** run tests with `cargo test --bins` — `cargo test --lib` finds nothing.
- **Two PRE-EXISTING failing tests** (unrelated, do not fix, do not be blocked by them): `db::queries::tests::test_load_work_hamlet` and `theme::tests::reader_gloss_colors_are_legible_and_distinct_for_all_themes`. "Tests pass" means: no failures OTHER than these two.
- **Never run the app** (`cargo run` is the user's job). Build with `cargo build`.
- **`keymap.json` / `keymap_config.rs` are NOT touched** — the action name `TogglePhraseHighlight` is unchanged.
- **All character offsets are unicode-char indices** (matching GTK iter line offsets and the phrase spans' Python-backfill indices), never byte indices.
- **Serialization compat:** legacy JSON booleans in `config.json` / `config-dev.json` must keep loading (`true` → `phrase`, `false` → `off`); new form serializes as lowercase strings.
- Commit messages end with the standard Claude co-author trailer used in this repo.

---

### Task 0: Preflight — resolve the dirty working tree

**Files:** none (git state only)

The working tree has ~10 pre-existing modified `src/` files (including `src/input/keymap.rs`, which Task 1 modifies). Staging `src/input/keymap.rs` for this feature would silently sweep that unrelated work into a feature commit.

- [ ] **Step 1: Check the tree**

Run: `git status --porcelain`

- [ ] **Step 2: Gate**

If ANY `src/` file is already modified (expected: `src/app/font.rs`, `src/input/actions/gloss.rs`, `src/input/actions/synopsis.rs`, `src/input/keymap.rs`, `src/theme.rs`, `src/ui/gloss_keybinds_overlay.rs`, `src/ui/gloss_overlay.rs`, `src/ui/gloss_render.rs`, `src/ui/journal_overlay.rs`, `src/ui/synopsis_keybinds_overlay.rs`): **STOP and ask the user** whether to commit or stash that work first. Do NOT stash or commit it yourself, and do NOT proceed to Task 1 until the tree is clean of unrelated `src/` changes.

---

### Task 1: `PhraseHighlightMode` enum + Alt+p 3-state cycle

**Files:**
- Modify: `src/config.rs` (fields ~93–99, default fn ~209–211, `Default` impl ~250–251, tests ~374–382)
- Modify: `src/input/phrase_highlight.rs:70-84` (gating only — keep the applied range phrase-width in this task)
- Modify: `src/input/keymap.rs:2946-2967` (`TogglePhraseHighlight` arm)
- Test: `src/config.rs` tests module (same file, `mod tests` uses `use super::*;`)

**Interfaces:**
- Produces: `crate::config::PhraseHighlightMode` — `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]` enum with variants `Off | Phrase | Line`; methods `is_on(self) -> bool`, `cycle(self) -> Self` (Off→Phrase→Line→Off), `label(self) -> &'static str` ("OFF"/"PHRASE"/"LINE"). `Config.phrase_highlight_prose` and `Config.phrase_highlight_verse` are now this type. Task 3 consumes all of this.

- [ ] **Step 1: Write the failing tests**

In `src/config.rs`'s `mod tests`, REPLACE the existing `phrase_highlight_defaults_prose_on_verse_off` test and ADD three new tests:

```rust
    #[test]
    fn phrase_highlight_defaults_prose_on_verse_off() {
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.phrase_highlight_prose, PhraseHighlightMode::Phrase);
        assert_eq!(cfg.phrase_highlight_verse, PhraseHighlightMode::Off);
        let dflt = Config::default();
        assert_eq!(dflt.phrase_highlight_prose, PhraseHighlightMode::Phrase);
        assert_eq!(dflt.phrase_highlight_verse, PhraseHighlightMode::Off);
    }

    #[test]
    fn phrase_highlight_mode_legacy_bools_deserialize() {
        let cfg: Config = serde_json::from_str(
            r#"{"phrase_highlight_prose": true, "phrase_highlight_verse": false}"#,
        )
        .unwrap();
        assert_eq!(cfg.phrase_highlight_prose, PhraseHighlightMode::Phrase);
        assert_eq!(cfg.phrase_highlight_verse, PhraseHighlightMode::Off);
    }

    #[test]
    fn phrase_highlight_mode_string_round_trip() {
        let mut cfg = Config::default();
        cfg.phrase_highlight_prose = PhraseHighlightMode::Line;
        cfg.phrase_highlight_verse = PhraseHighlightMode::Phrase;
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains(r#""phrase_highlight_prose":"line""#));
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.phrase_highlight_prose, PhraseHighlightMode::Line);
        assert_eq!(back.phrase_highlight_verse, PhraseHighlightMode::Phrase);
    }

    #[test]
    fn phrase_highlight_mode_cycle_and_is_on() {
        use PhraseHighlightMode::*;
        assert_eq!(Off.cycle(), Phrase);
        assert_eq!(Phrase.cycle(), Line);
        assert_eq!(Line.cycle(), Off);
        assert!(!Off.is_on());
        assert!(Phrase.is_on());
        assert!(Line.is_on());
    }
```

- [ ] **Step 2: Verify they fail**

Run: `cargo test --bins phrase_highlight_mode 2>&1 | tail -20`
Expected: compile error — `PhraseHighlightMode` not found.

- [ ] **Step 3: Implement the enum and switch the config fields**

In `src/config.rs`, add near the other type definitions (before the `Config` struct):

```rust
/// Karaoke narration highlight granularity per work class (see
/// src/input/phrase_highlight.rs). Serialized as a lowercase string; legacy
/// boolean configs deserialize as true=Phrase, false=Off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PhraseHighlightMode {
    Off,
    Phrase,
    Line,
}

impl PhraseHighlightMode {
    pub fn is_on(self) -> bool {
        self != PhraseHighlightMode::Off
    }

    /// Alt+p cycle order: Off -> Phrase -> Line -> Off.
    pub fn cycle(self) -> Self {
        match self {
            PhraseHighlightMode::Off => PhraseHighlightMode::Phrase,
            PhraseHighlightMode::Phrase => PhraseHighlightMode::Line,
            PhraseHighlightMode::Line => PhraseHighlightMode::Off,
        }
    }

    /// Toast/log label.
    pub fn label(self) -> &'static str {
        match self {
            PhraseHighlightMode::Off => "OFF",
            PhraseHighlightMode::Phrase => "PHRASE",
            PhraseHighlightMode::Line => "LINE",
        }
    }
}

impl<'de> Deserialize<'de> for PhraseHighlightMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bool(bool),
            Str(String),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Bool(true) => Ok(PhraseHighlightMode::Phrase),
            Raw::Bool(false) => Ok(PhraseHighlightMode::Off),
            Raw::Str(s) => match s.as_str() {
                "off" => Ok(PhraseHighlightMode::Off),
                "phrase" => Ok(PhraseHighlightMode::Phrase),
                "line" => Ok(PhraseHighlightMode::Line),
                other => Err(D::Error::custom(format!(
                    "unknown phrase highlight mode: {other:?}"
                ))),
            },
        }
    }
}
```

(A malformed mode string makes the whole config fall back to compiled-in defaults — same behavior as any other malformed config JSON today. Acceptable.)

Change the two fields (currently `bool` at ~lines 96–99), updating the doc comment:

```rust
    /// Karaoke narration highlight granularity, per work class: `off`,
    /// `phrase` (spoken phrase span), or `line` (whole verse line / prose
    /// sentence — still requires phrase data). Legacy boolean configs load
    /// as true=phrase, false=off. Alt+p cycles the current class.
    #[serde(default = "default_phrase_highlight_prose")]
    pub phrase_highlight_prose: PhraseHighlightMode,
    #[serde(default = "default_phrase_highlight_verse")]
    pub phrase_highlight_verse: PhraseHighlightMode,
```

Replace the default fn (~line 209) and add the verse one:

```rust
fn default_phrase_highlight_prose() -> PhraseHighlightMode {
    PhraseHighlightMode::Phrase
}

fn default_phrase_highlight_verse() -> PhraseHighlightMode {
    PhraseHighlightMode::Off
}
```

In `impl Default for Config` (~lines 250–251):

```rust
            phrase_highlight_prose: PhraseHighlightMode::Phrase,
            phrase_highlight_verse: PhraseHighlightMode::Off,
```

- [ ] **Step 4: Update the driver gating (compile fix — behavior identical)**

In `src/input/phrase_highlight.rs`, the top of `update_phrase_highlight` currently reads:

```rust
    let enabled = if s.is_prose() {
        s.config.phrase_highlight_prose
    } else {
        s.config.phrase_highlight_verse
    };
    let suppressed = s
        .suppress_sync_until
        .map(|until| std::time::Instant::now() < until)
        .unwrap_or(false);
    if !enabled || !s.sync_enabled || s.loading_work.get() || s.translations_visible || suppressed
```

Change to (only `enabled` → `mode` + `is_on()`; everything else identical):

```rust
    let mode = if s.is_prose() {
        s.config.phrase_highlight_prose
    } else {
        s.config.phrase_highlight_verse
    };
    let suppressed = s
        .suppress_sync_until
        .map(|until| std::time::Instant::now() < until)
        .unwrap_or(false);
    if !mode.is_on() || !s.sync_enabled || s.loading_work.get() || s.translations_visible || suppressed
```

`mode` is otherwise unused in this task (Task 3 uses it); if clippy complains it won't — it is read in the gate.

- [ ] **Step 5: Replace the `TogglePhraseHighlight` arm with the 3-state cycle**

In `src/input/keymap.rs` (~lines 2946–2967), replace the whole arm body:

```rust
        TogglePhraseHighlight => {
            let mut s = state.borrow_mut();
            let is_prose = s.is_prose();
            let mode = if is_prose {
                s.config.phrase_highlight_prose = s.config.phrase_highlight_prose.cycle();
                s.config.phrase_highlight_prose
            } else {
                s.config.phrase_highlight_verse = s.config.phrase_highlight_verse.cycle();
                s.config.phrase_highlight_verse
            };
            crate::config::save(&s.config);
            // Clear on EVERY transition (not just Off) so a stale phrase-width
            // tint never lingers when entering LINE mode; the next TimePos
            // tick repaints at the new mode's width.
            crate::input::phrase_highlight::clear_phrase_highlight(&mut s);
            let text = format!(
                "Phrase highlight {} ({})",
                mode.label(),
                if is_prose { "prose" } else { "plays/poetry" },
            );
            crate::input::navigation::show_chapter_toast(&s, &text);
            crate::logging::log(&format!("PHRASE_HL: toggled {}", text));
        }
```

- [ ] **Step 6: Run the tests**

Run: `cargo test --bins config:: 2>&1 | tail -20`
Expected: all `config::tests::*` PASS (including the four phrase-highlight tests).

Run: `cargo build 2>&1 | tail -5`
Expected: clean build (warnings ok, no errors).

- [ ] **Step 7: Commit**

```bash
git add src/config.rs src/input/phrase_highlight.rs src/input/keymap.rs
git commit -m "feat: PhraseHighlightMode enum, Alt+p cycles off/phrase/line"
```

---

### Task 2: `sentence_bounds` prose sentence scanner

**Files:**
- Modify: `src/input/phrase_highlight.rs` (new pure functions + tests in the existing `mod tests`)

**Interfaces:**
- Produces: `pub fn sentence_bounds(text: &str, start_char: usize, end_char: usize) -> (usize, usize)` in `crate::input::phrase_highlight` — unicode-char offsets in, `[start, end)` char range of the sentence(s) containing the span, clamped to the text. Task 3 consumes it.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `src/input/phrase_highlight.rs`:

```rust
    #[test]
    fn sentence_bounds_first_mid_last_sentence() {
        let text = "First sentence here. Second one is longer. Third ends.";
        // Span inside the middle sentence ("one").
        assert_eq!(sentence_bounds(text, 28, 31), (21, 42));
        // Span in the first sentence.
        assert_eq!(sentence_bounds(text, 0, 5), (0, 20));
        // Span in the last sentence (no trailing terminator scan overrun).
        assert_eq!(sentence_bounds(text, 43, 48), (43, 54));
    }

    #[test]
    fn sentence_bounds_span_crossing_boundary_covers_both() {
        let text = "First sentence here. Second one is longer. Third ends.";
        // Span from inside sentence 1 into sentence 2 -> both tinted.
        assert_eq!(sentence_bounds(text, 15, 25), (0, 42));
    }

    #[test]
    fn sentence_bounds_abbreviation_and_initial_guard() {
        let text = "Mr. Tulkinghorn arrives. He waits.";
        // "Mr." must not end the sentence; "arrives." does.
        assert_eq!(sentence_bounds(text, 16, 23), (0, 24));
        assert_eq!(sentence_bounds(text, 25, 27), (25, 34));
        // Single-letter initials ("J.") must not end the sentence either.
        let text2 = "Mr. J. Smith spoke. So did I.";
        assert_eq!(sentence_bounds(text2, 7, 12), (0, 19));
    }

    #[test]
    fn sentence_bounds_closing_quotes_and_decimals() {
        // Terminator followed by a closing quote: quote belongs to the sentence.
        let text = "\u{201C}Stop!\u{201D} Then left.";
        assert_eq!(sentence_bounds(text, 1, 5), (0, 7));
        assert_eq!(sentence_bounds(text, 8, 12), (8, 18));
        // A decimal point is not a sentence end.
        let text2 = "It cost 3.5 pounds. Yes.";
        assert_eq!(sentence_bounds(text2, 12, 18), (0, 19));
    }

    #[test]
    fn sentence_bounds_clamps_out_of_range_span() {
        // Offsets beyond the text clamp instead of panicking (data drift guard).
        assert_eq!(sentence_bounds("Hi.", 10, 20), (3, 3));
    }
```

- [ ] **Step 2: Verify they fail**

Run: `cargo test --bins sentence_bounds 2>&1 | tail -10`
Expected: compile error — `sentence_bounds` not found.

- [ ] **Step 3: Implement**

Add to `src/input/phrase_highlight.rs` (below `resolve_spoken_idx`, above the `use crate::app::AppState;` line):

```rust
/// Words whose trailing '.' does not end a sentence (titles/abbreviations
/// common in 19th-century prose — Bleak House is full of them). Matched
/// case-sensitively against the word immediately before the '.'.
const ABBREVIATIONS: &[&str] = &[
    "Mr", "Mrs", "Ms", "Dr", "St", "Prof", "Rev", "Hon", "Capt", "Col",
    "Gen", "Lieut", "Sgt", "Esq", "Jr", "Sr", "vol", "chap", "etc", "viz",
    "cf", "vs",
];

/// Closing punctuation that may trail a sentence terminator and still belong
/// to the sentence (curly + straight quotes, brackets).
const CLOSERS: &[char] = &['\u{2019}', '\u{201D}', '\'', '"', ')', ']'];

/// True when `chars[k]` ends a sentence: it is `.`/`!`/`?`, the next char
/// (skipping CLOSERS) is whitespace or end-of-text (rejects decimals like
/// "3.5" and mid-word dots), and for '.' the word before it is neither a
/// known abbreviation nor a single uppercase initial ("Mr. J. Smith").
fn is_sentence_end(chars: &[char], k: usize) -> bool {
    let c = chars[k];
    if c != '.' && c != '!' && c != '?' {
        return false;
    }
    let mut j = k + 1;
    while j < chars.len() && CLOSERS.contains(&chars[j]) {
        j += 1;
    }
    if j < chars.len() && !chars[j].is_whitespace() {
        return false;
    }
    if c == '.' {
        let mut w = k;
        while w > 0 && chars[w - 1].is_alphabetic() {
            w -= 1;
        }
        let word: String = chars[w..k].iter().collect();
        let mut cs = word.chars();
        if let (Some(first), None) = (cs.next(), cs.next()) {
            if first.is_uppercase() {
                return false; // single initial, e.g. "J."
            }
        }
        if ABBREVIATIONS.contains(&word.as_str()) {
            return false;
        }
    }
    true
}

/// `[start, end)` unicode-char range of the sentence(s) containing the span
/// `[start_char, end_char)`. A span crossing a sentence boundary extends over
/// BOTH sentences (backward from the span start, forward from the span end).
/// Out-of-range offsets clamp; mis-detection yields a wrong-width tint, never
/// a panic (apply_phrase_tag clamps again downstream).
pub fn sentence_bounds(text: &str, start_char: usize, end_char: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let sc = start_char.min(n);
    let ec = end_char.min(n).max(sc);
    // Backward: the sentence starts after the previous sentence's terminator
    // (+ trailing closers + whitespace); at 0 when there is none.
    let mut start = 0;
    for k in (0..sc).rev() {
        if is_sentence_end(&chars, k) {
            let mut j = k + 1;
            while j < n && CLOSERS.contains(&chars[j]) {
                j += 1;
            }
            while j < n && chars[j].is_whitespace() {
                j += 1;
            }
            start = j.min(sc);
            break;
        }
    }
    // Forward: through the next terminator (+ trailing closers); n when there
    // is none. Starts at ec-1 so a span already ending ON the terminator
    // doesn't extend into the following sentence.
    let mut end = n;
    for k in ec.saturating_sub(1)..n {
        if is_sentence_end(&chars, k) {
            let mut j = k + 1;
            while j < n && CLOSERS.contains(&chars[j]) {
                j += 1;
            }
            end = j;
            break;
        }
    }
    (start, end.max(ec))
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --bins sentence_bounds 2>&1 | tail -10`
Expected: 4 tests... (all `sentence_bounds_*` tests) PASS.

- [ ] **Step 5: Commit**

```bash
git add src/input/phrase_highlight.rs
git commit -m "feat: sentence_bounds prose sentence scanner for LINE mode"
```

---

### Task 3: Driver widens the tint range in LINE mode

**Files:**
- Modify: `src/input/phrase_highlight.rs` (`update_phrase_highlight` apply site; new `tint_range` + `buffer_line_text`; tests)

**Interfaces:**
- Consumes: `crate::config::PhraseHighlightMode` (Task 1), `sentence_bounds` (Task 2), `crate::db::queries::PhraseSpan` (existing; fields `start_time: f64, end_time: f64, start_char: usize, end_char: usize`; it is `Copy`).
- Produces: `pub fn tint_range(mode: PhraseHighlightMode, is_prose: bool, line_text: &str, span: PhraseSpan) -> (usize, usize)` — the char range to tag for the active span under the given mode/class.

- [ ] **Step 1: Write the failing test**

Append to `mod tests` in `src/input/phrase_highlight.rs`:

```rust
    #[test]
    fn tint_range_by_mode_and_class() {
        use crate::config::PhraseHighlightMode::{Line, Phrase};
        let sp = span(10.0, 11.0, 4, 7); // "two" in the text below
        let text = "One two. Three four.";
        // Phrase mode: exactly the span, both classes.
        assert_eq!(tint_range(Phrase, true, text, sp), (4, 7));
        assert_eq!(tint_range(Phrase, false, text, sp), (4, 7));
        // Line mode, verse: the whole line.
        assert_eq!(tint_range(Line, false, text, sp), (0, 20));
        // Line mode, prose: the sentence containing the span.
        assert_eq!(tint_range(Line, true, text, sp), (0, 8));
    }
```

- [ ] **Step 2: Verify it fails**

Run: `cargo test --bins tint_range 2>&1 | tail -10`
Expected: compile error — `tint_range` not found.

- [ ] **Step 3: Implement `tint_range` and wire it into the driver**

Add below `sentence_bounds` in `src/input/phrase_highlight.rs`:

```rust
use crate::config::PhraseHighlightMode;

/// Char range to tag for the active span: the span itself in Phrase mode;
/// in Line mode the whole buffer line (verse) or the containing sentence
/// (prose). Off never reaches here (gated in update_phrase_highlight).
pub fn tint_range(
    mode: PhraseHighlightMode,
    is_prose: bool,
    line_text: &str,
    span: PhraseSpan,
) -> (usize, usize) {
    match mode {
        PhraseHighlightMode::Line if is_prose => {
            sentence_bounds(line_text, span.start_char, span.end_char)
        }
        PhraseHighlightMode::Line => (0, line_text.chars().count()),
        _ => (span.start_char, span.end_char),
    }
}

/// Text of buffer line `bl` (no trailing newline). Empty when out of range.
fn buffer_line_text(s: &AppState, bl: usize) -> String {
    let buffer = &s.buffer;
    let Some(start) = buffer.iter_at_line(bl as i32) else {
        return String::new();
    };
    let mut end = start;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    buffer.text(&start, &end, false).to_string()
}
```

In `update_phrase_highlight`, the apply site currently reads:

```rust
    if s.active_phrase == Some((bl, span_idx)) {
        return;
    }
    apply_phrase_tag(s, bl, span.start_char, span.end_char);
    s.active_phrase = Some((bl, span_idx));
```

Replace with (dedup check stays FIRST so line text isn't extracted every tick):

```rust
    if s.active_phrase == Some((bl, span_idx)) {
        return;
    }
    let line_text = buffer_line_text(s, bl);
    let (sc, ec) = tint_range(mode, s.is_prose(), &line_text, span);
    apply_phrase_tag(s, bl, sc, ec);
    s.active_phrase = Some((bl, span_idx));
```

Also update the module doc comment (top of the file, lines 1–7) to mention modes — append one sentence to the paragraph:

```rust
//! In LINE mode the tint widens to the whole buffer line (verse) or the
//! containing sentence (prose) via tint_range/sentence_bounds; the span
//! resolution and caching are identical in every mode.
```

The dedup key `(bl, span_idx)` is deliberately unchanged: in Line mode,
consecutive spans within one sentence re-apply an identical range — a
harmless remove+apply, no state-machine change.

- [ ] **Step 4: Run all module tests + build**

Run: `cargo test --bins phrase_highlight 2>&1 | tail -15`
Expected: all `phrase_highlight::tests::*` PASS (phrase_at_time, resolve_spoken_idx, sentence_bounds x4, tint_range).

Run: `cargo build 2>&1 | tail -5`
Expected: clean build.

- [ ] **Step 5: Commit**

```bash
git add src/input/phrase_highlight.rs
git commit -m "feat: LINE-mode tint range (verse whole line, prose sentence)"
```

---

### Task 4: Keybinds-overlay description + full verification

**Files:**
- Modify: `src/ui/keybinds_overlay.rs:284-286` (the `"phrase hl"` describe arm)

**Interfaces:**
- Consumes: the Alt+p cycle wording from Task 1 (Off → Phrase → Line).

- [ ] **Step 1: Update the describe() arm**

Replace lines 284–286:

```rust
        "phrase hl" => "Cycle the karaoke narration highlight for this work's \
class: OFF -> PHRASE (spoken phrase) -> LINE (whole verse line / prose \
sentence). Saved to config. \
-> TogglePhraseHighlight arm — src/input/keymap.rs \
(driver: src/input/phrase_highlight.rs)",
```

- [ ] **Step 2: Overlay cross-check**

Per the repo rule, run the `update-cairo-keybinds-overlay` skill's exhaustive cross-reference for this change. Scope here is narrow — the key, label (`phrase hl`), action (`TogglePhraseHighlight`), and modifiers are all unchanged; only the description text changed. Verify: (a) the `KeyDef` for the Alt+p key still names `phrase hl`; (b) `describe("phrase hl")` returns the new text; (c) no other arm references the old "Toggle the karaoke" wording (`rg -n "Toggle the karaoke" src/`→ no hits).

- [ ] **Step 3: Full test + lint pass**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: only the two known pre-existing failures (`test_load_work_hamlet`, `reader_gloss_colors_are_legible_and_distinct_for_all_themes`); everything else PASS.

Run: `cargo clippy 2>&1 | tail -10`
Expected: no NEW warnings in the files this plan touched (`config.rs`, `phrase_highlight.rs`, `keymap.rs`, `keybinds_overlay.rs`).

- [ ] **Step 4: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(overlay): Alt+p describe() reflects off/phrase/line cycle"
```

- [ ] **Step 5: Hand off visual acceptance to the user**

Do NOT run the app. Tell the user the feature is ready to eyeball via `crll`:
- A verse work with phrase data (e.g. Cym-Amb): Alt+p to LINE → the whole spoken verse line tints during narration; Alt+p again → OFF; again → PHRASE.
- Bleak House (prose): Alt+p to LINE → sentence-width tint (check a paragraph with "Mr." — the tint must not stop at the abbreviation).
- Legacy config check happens implicitly: their existing `config-dev.json` still holds booleans and must load with today's behavior (prose PHRASE, verse OFF).
