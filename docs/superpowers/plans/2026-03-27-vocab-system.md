# Vocab System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add vocabulary word highlighting, a definition panel, vocab navigation (w/W), and a concordance picker (Ctrl+\) to linux-lit.

**Architecture:** Vocab words are loaded from lit.db on display_work, matched against buffer text to build a precomputed VocabMatch index. A color-only TextTag highlights matches. A right-side definition panel shows definition/etymology/gloss. Navigation keys w/W cycle through occurrences. A concordance picker lists all vocab words in the current work.

**Tech Stack:** Rust, GTK4, sourceview5, rusqlite, serde_json

---

### Task 1: Add vocab_fg to Theme

**Files:**
- Modify: `src/theme.rs:7-18` (Theme struct)
- Modify: `src/theme.rs:88-159` (resolve_theme)
- Modify: `src/theme.rs:162-175` (default_theme)

- [ ] **Step 1: Add vocab_fg field to Theme struct**

In `src/theme.rs`, add `vocab_fg` to the `Theme` struct after `cursor_fg`:

```rust
pub cursor_fg: String,        // cursor indicator foreground
pub vocab_fg: String,         // vocab word highlight foreground
```

- [ ] **Step 2: Resolve vocab_fg in resolve_theme**

In `src/theme.rs` `resolve_theme()`, after the `cursor_fg` resolution (around line 146), add:

```rust
let vocab_fg = highlights
    .get("VocabWord")
    .and_then(|c| str_field(c, "guifg"))
    .unwrap_or_else(|| {
        if is_light { "#8a6534".to_string() } else { "#d8a657".to_string() }
    });
```

Add `vocab_fg` to the `Theme` struct construction at the end of `resolve_theme`.

- [ ] **Step 3: Add vocab_fg to default_theme**

In `default_theme()`, add:

```rust
vocab_fg: "#d8a657".to_string(),
```

- [ ] **Step 4: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add src/theme.rs
git commit -m "feat: add vocab_fg color to Theme from VocabWord highlight"
```

---

### Task 2: Add vocab_highlight_visible to Config

**Files:**
- Modify: `src/config.rs:20-44` (Config struct)
- Modify: `src/config.rs:88-103` (Default impl)

- [ ] **Step 1: Add field to Config struct**

In `src/config.rs`, add after the `ollama_endpoint` field (line 43):

```rust
#[serde(default = "default_vocab_highlight_visible")]
pub vocab_highlight_visible: bool,
```

- [ ] **Step 2: Add default function**

After `default_ollama_endpoint()` (line 85), add:

```rust
fn default_vocab_highlight_visible() -> bool {
    true
}
```

- [ ] **Step 3: Add to Default impl**

In the `Default` impl, add after `ollama_endpoint`:

```rust
vocab_highlight_visible: default_vocab_highlight_visible(),
```

- [ ] **Step 4: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat: add vocab_highlight_visible config field (default true)"
```

---

### Task 3: Add vocab DB queries

**Files:**
- Modify: `src/db/queries.rs` (after `load_translations`, line 178)

- [ ] **Step 1: Write tests for vocab queries**

Add to the `tests` module in `src/db/queries.rs`:

```rust
#[test]
fn test_load_vocab_words() {
    let conn = open_db().unwrap();
    let words = load_vocab_words(&conn, "Ham").unwrap();
    // Hamlet should have some vocab words
    assert!(!words.is_empty(), "Should have vocab words for Hamlet");
}

#[test]
fn test_load_vocab_definition() {
    let conn = open_db().unwrap();
    let words = load_vocab_words(&conn, "Ham").unwrap();
    if let Some(word) = words.iter().next() {
        let def = load_vocab_definition(&conn, word);
        // Just verify no crash — definition may or may not exist
        let _ = def;
    }
}

#[test]
fn test_load_vocab_word_list() {
    let conn = open_db().unwrap();
    let list = load_vocab_word_list(&conn, "Ham").unwrap();
    // Should return sorted list with counts
    if list.len() > 1 {
        assert!(list[0].0 <= list[1].0, "Should be alphabetically sorted");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_load_vocab -- --nocapture`
Expected: FAIL — functions not defined

- [ ] **Step 3: Implement load_vocab_words**

Add after `load_translations` (line 178) in `src/db/queries.rs`:

```rust
/// Load all vocab words + variants for matching against buffer text.
/// Returns a HashSet of lowercase words (base words + variants).
pub fn load_vocab_words(
    conn: &Connection,
    _work_abbrev: &str,
) -> Result<std::collections::HashSet<String>, rusqlite::Error> {
    let mut words = std::collections::HashSet::new();

    // Base words
    let mut stmt = conn.prepare("SELECT LOWER(word) FROM vocab_words")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        words.insert(row?);
    }

    // Variants
    let mut stmt = conn.prepare(
        "SELECT LOWER(v.variant) FROM vocab_word_variants v"
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        words.insert(row?);
    }

    Ok(words)
}
```

- [ ] **Step 4: Implement load_vocab_definition**

```rust
/// Load definition and sources for a vocab word.
pub fn load_vocab_definition(
    conn: &Connection,
    word: &str,
) -> Option<(String, Vec<String>)> {
    let result: Result<(String, Option<String>), _> = conn.query_row(
        "SELECT w.definition, GROUP_CONCAT(s.source) \
         FROM vocab_words w \
         LEFT JOIN vocab_word_sources s ON s.word_id = w.id \
         WHERE LOWER(w.word) = ?1 \
         GROUP BY w.id",
        [word.to_lowercase()],
        |row| Ok((
            row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            row.get::<_, Option<String>>(1)?,
        )),
    );
    match result {
        Ok((def, sources_str)) => {
            let sources: Vec<String> = sources_str
                .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
                .unwrap_or_default();
            if def.is_empty() { None } else { Some((def, sources)) }
        }
        Err(_) => None,
    }
}
```

- [ ] **Step 5: Implement load_vocab_etymology**

```rust
/// Load etymology breakdown from vocab_rhetoric.
pub fn load_vocab_etymology(
    conn: &Connection,
    word: &str,
) -> Option<VocabEtymology> {
    conn.query_row(
        "SELECT vr.prefix, vr.prefix_gloss, vr.root, vr.root_base, \
         vr.root_gloss, vr.suffix, vr.suffix_gloss \
         FROM vocab_rhetoric vr \
         JOIN vocab_words vw ON vr.word_id = vw.id \
         WHERE LOWER(vw.word) = ?1",
        [word.to_lowercase()],
        |row| Ok(VocabEtymology {
            prefix: row.get::<_, Option<String>>(0)?,
            prefix_gloss: row.get::<_, Option<String>>(1)?,
            root: row.get::<_, Option<String>>(2)?,
            root_base: row.get::<_, Option<String>>(3)?,
            root_gloss: row.get::<_, Option<String>>(4)?,
            suffix: row.get::<_, Option<String>>(5)?,
            suffix_gloss: row.get::<_, Option<String>>(6)?,
        }),
    ).ok()
}

pub struct VocabEtymology {
    pub prefix: Option<String>,
    pub prefix_gloss: Option<String>,
    pub root: Option<String>,
    pub root_base: Option<String>,
    pub root_gloss: Option<String>,
    pub suffix: Option<String>,
    pub suffix_gloss: Option<String>,
}
```

- [ ] **Step 6: Implement load_vocab_gloss**

```rust
/// Load a vocab-word gloss for a word near a given line.
pub fn load_vocab_gloss(
    conn: &Connection,
    word: &str,
    work_abbrev: &str,
    line_citation: &str,
) -> Option<String> {
    // Find the word_id
    let word_id: i64 = conn.query_row(
        "SELECT id FROM vocab_words WHERE LOWER(word) = ?1",
        [word.to_lowercase()],
        |row| row.get(0),
    ).ok()?;

    // Find a gloss for this word in a passage containing this citation
    conn.query_row(
        "SELECT g.gloss_text FROM glosses g \
         JOIN passages p ON g.passage_id = p.id \
         WHERE g.gloss_type = 'vocab-word' \
         AND g.word_id = ?1 \
         AND p.work_abbrev = ?2 \
         AND p.start_citation <= ?3 \
         AND p.end_citation >= ?3",
        rusqlite::params![word_id, work_abbrev, line_citation],
        |row| row.get::<_, String>(0),
    ).ok()
}
```

- [ ] **Step 7: Implement load_vocab_word_list**

```rust
/// List all vocab words found in a work's text, with occurrence counts.
/// Returns alphabetically sorted (word, count) pairs.
pub fn load_vocab_word_list(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<(String, usize)>, rusqlite::Error> {
    // Load all work lines
    let mut stmt = conn.prepare(
        "SELECT canonical_text FROM line_mapping WHERE work_abbrev = ?1 \
         ORDER BY div1, div2, line_in_div"
    )?;
    let lines: Vec<String> = stmt.query_map([work_abbrev], |row| {
        row.get::<_, String>(0)
    })?.collect::<Result<_, _>>()?;

    // Load vocab word set
    let vocab = load_vocab_words(conn, work_abbrev)?;

    // Count occurrences
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for line in &lines {
        for token in line.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}') {
            let lower = token.to_lowercase();
            if vocab.contains(&lower) {
                *counts.entry(lower).or_insert(0) += 1;
            }
        }
    }

    let mut result: Vec<(String, usize)> = counts.into_iter().collect();
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}
```

- [ ] **Step 8: Run tests**

Run: `cargo test test_load_vocab -- --nocapture`
Expected: all 3 tests PASS

- [ ] **Step 9: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat: add vocab word DB queries (words, definition, etymology, gloss, word list)"
```

---

### Task 4: Add VocabMatch struct and vocab highlighting in AppState

**Files:**
- Modify: `src/app.rs:18-84` (AppState struct)
- Modify: `src/app.rs:105-234` (build_window — tag registration and widget tree)
- Modify: `src/app.rs:429-595` (display_work — load vocab, build matches, apply highlighting)

- [ ] **Step 1: Add VocabMatch struct and new fields to AppState**

At the top of `src/app.rs`, after the `SearchMatch` struct (line 23), add:

```rust
#[derive(Debug, Clone)]
pub struct VocabMatch {
    pub word: String,
    pub line_index: usize,
    pub char_start: usize,
    pub char_end: usize,
}
```

Add these fields to `AppState` after `gloss_original_text` (line 83):

```rust
pub vocab_words: std::collections::HashSet<String>,
pub vocab_matches: Vec<VocabMatch>,
pub vocab_match_idx: Option<usize>,
pub vocab_tag: gtk4::TextTag,
pub vocab_highlight_visible: bool,
pub definition_panel_visible: bool,
```

- [ ] **Step 2: Register vocab-word TextTag in build_window**

After the `selection_tag` creation (line 176-184), add:

```rust
let vocab_tag = gtk4::TextTag::builder()
    .name("vocab-word")
    .foreground(&theme.vocab_fg)
    .build();
buffer.tag_table().add(&vocab_tag);
```

- [ ] **Step 3: Change layout to hbox with definition panel placeholder**

In `build_window`, replace the single `scrolled` window setup. After creating `scrolled` (line 234), wrap it in an hbox:

```rust
let content_hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
content_hbox.set_halign(gtk4::Align::Center);
content_hbox.set_valign(gtk4::Align::Fill);
content_hbox.set_vexpand(true);
content_hbox.set_margin_top(24);
content_hbox.set_margin_bottom(24);
content_hbox.set_margin_start(24);
content_hbox.set_margin_end(24);
content_hbox.append(&scrolled);
```

Remove the margin settings from `scrolled` itself (margin_top/bottom/start/end become 0 since the hbox handles them). Keep the `width_request` and `css_classes` on scrolled.

Then pass `&content_hbox` instead of `&scrolled` to `picker.attach()` (line 239).

- [ ] **Step 4: Initialize new fields in AppState construction**

In the `AppState` construction (line 283-333), add after `gloss_original_text: None`:

```rust
vocab_words: std::collections::HashSet::new(),
vocab_matches: Vec::new(),
vocab_match_idx: None,
vocab_tag,
vocab_highlight_visible: config.vocab_highlight_visible,
definition_panel_visible: false,
```

- [ ] **Step 5: Add vocab loading and matching in display_work**

In `display_work`, after `apply_dialogue_formatting(state);` (line 522), add:

```rust
// Load vocab words and apply highlighting
if let Some(ref work) = state.current_work {
    if let Ok(conn) = crate::db::queries::open_db() {
        state.vocab_words = crate::db::queries::load_vocab_words(&conn, &work.abbrev)
            .unwrap_or_default();
        crate::logging::log(&format!(
            "VOCAB: loaded {} vocab words",
            state.vocab_words.len(),
        ));
    }
}
build_vocab_matches(state);
if state.vocab_highlight_visible {
    apply_vocab_highlighting(state);
}
```

- [ ] **Step 6: Implement build_vocab_matches and apply_vocab_highlighting**

Add these functions to `src/app.rs`:

```rust
/// Tokenize buffer lines and find vocab word matches.
fn build_vocab_matches(state: &mut AppState) {
    state.vocab_matches.clear();
    state.vocab_match_idx = None;

    if state.vocab_words.is_empty() {
        return;
    }

    let line_count = state.effective_line_count();
    let buffer_text = state.buffer.text(
        &state.buffer.start_iter(),
        &state.buffer.end_iter(),
        false,
    );

    for (line_idx, line_text) in buffer_text.lines().enumerate() {
        if line_idx >= line_count {
            break;
        }
        // Track char offset (not byte offset) for GTK TextIter compatibility
        let mut char_offset = 0usize;
        let mut in_word = false;
        let mut word_start = 0usize;
        let mut word_buf = String::new();

        for ch in line_text.chars() {
            let is_word_char = ch.is_alphanumeric() || ch == '\'' || ch == '\u{2019}';
            if is_word_char {
                if !in_word {
                    word_start = char_offset;
                    word_buf.clear();
                    in_word = true;
                }
                word_buf.push(ch);
            } else if in_word {
                let lower = word_buf.to_lowercase();
                if state.vocab_words.contains(&lower) {
                    state.vocab_matches.push(VocabMatch {
                        word: lower,
                        line_index: line_idx,
                        char_start: word_start,
                        char_end: char_offset,
                    });
                }
                in_word = false;
            }
            char_offset += 1;
        }
        // Handle word at end of line
        if in_word {
            let lower = word_buf.to_lowercase();
            if state.vocab_words.contains(&lower) {
                state.vocab_matches.push(VocabMatch {
                    word: lower,
                    line_index: line_idx,
                    char_start: word_start,
                    char_end: char_offset,
                });
            }
        }
    }
}

/// Apply the vocab-word TextTag to all matches in the buffer.
pub fn apply_vocab_highlighting(state: &AppState) {
    for m in &state.vocab_matches {
        let mut line_iter = state.buffer.iter_at_line(m.line_index as i32);
        if let Some(ref mut iter) = line_iter {
            let mut start = iter.clone();
            start.forward_chars(m.char_start as i32);
            let mut end = iter.clone();
            end.forward_chars(m.char_end as i32);
            state.buffer.apply_tag(&state.vocab_tag, &start, &end);
        }
    }
}

/// Remove all vocab-word tags from the buffer.
pub fn remove_vocab_highlighting(state: &AppState) {
    let start = state.buffer.start_iter();
    let end = state.buffer.end_iter();
    state.buffer.remove_tag(&state.vocab_tag, &start, &end);
}
```

- [ ] **Step 7: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 8: Commit**

```bash
git add src/app.rs
git commit -m "feat: add VocabMatch, vocab TextTag, hbox layout, and vocab highlighting"
```

---

### Task 5: Create definition panel widget

**Files:**
- Create: `src/ui/definition_panel.rs`
- Modify: `src/ui/mod.rs` (add module)

- [ ] **Step 1: Register the module**

Add to `src/ui/mod.rs`:

```rust
pub mod definition_panel;
```

- [ ] **Step 2: Create definition_panel.rs**

Create `src/ui/definition_panel.rs`:

```rust
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, ScrolledWindow};

pub struct DefinitionPanel {
    pub container: GtkBox,
    scrolled: ScrolledWindow,
    content_box: GtkBox,
    header_label: Label,
    word_label: Label,
    definition_label: Label,
    etymology_header: Label,
    etymology_label: Label,
    gloss_header: Label,
    gloss_label: Label,
    hint_label: Label,
}

impl DefinitionPanel {
    pub fn new() -> Self {
        let content_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .build();

        // DEFINITION header
        let header_label = Label::builder()
            .label("DEFINITION")
            .halign(gtk4::Align::Start)
            .margin_bottom(8)
            .build();
        header_label.add_css_class("definition-header");
        content_box.append(&header_label);

        // Word name
        let word_label = Label::builder()
            .halign(gtk4::Align::Start)
            .margin_bottom(12)
            .build();
        word_label.add_css_class("definition-word");
        content_box.append(&word_label);

        // Definition text
        let definition_label = Label::builder()
            .halign(gtk4::Align::Start)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::Word)
            .margin_bottom(16)
            .build();
        definition_label.add_css_class("definition-text");
        content_box.append(&definition_label);

        // ETYMOLOGY header
        let etymology_header = Label::builder()
            .label("ETYMOLOGY")
            .halign(gtk4::Align::Start)
            .margin_bottom(8)
            .build();
        etymology_header.add_css_class("definition-header");
        content_box.append(&etymology_header);

        // Etymology text
        let etymology_label = Label::builder()
            .halign(gtk4::Align::Start)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::Word)
            .margin_bottom(16)
            .build();
        etymology_label.add_css_class("definition-etymology");
        content_box.append(&etymology_label);

        // GLOSS header
        let gloss_header = Label::builder()
            .label("GLOSS")
            .halign(gtk4::Align::Start)
            .margin_bottom(8)
            .build();
        gloss_header.add_css_class("definition-header");
        content_box.append(&gloss_header);

        // Gloss text
        let gloss_label = Label::builder()
            .halign(gtk4::Align::Start)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::Word)
            .build();
        gloss_label.add_css_class("definition-gloss");
        content_box.append(&gloss_label);

        // Scrolled wrapper for content
        let scrolled = ScrolledWindow::builder()
            .child(&content_box)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .vexpand(true)
            .build();

        // Hint bar at bottom
        let hint_label = Label::builder()
            .label("w next \u{00B7} W prev \u{00B7} \\ hide \u{00B7} Alt+\\ highlights")
            .halign(gtk4::Align::Center)
            .build();
        hint_label.add_css_class("definition-hint");

        // Outer container
        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .width_request(320)
            .build();
        container.add_css_class("definition-panel");
        container.append(&scrolled);
        container.append(&hint_label);
        container.set_visible(false);

        DefinitionPanel {
            container,
            scrolled,
            content_box,
            header_label,
            word_label,
            definition_label,
            etymology_header,
            etymology_label,
            gloss_header,
            gloss_label,
            hint_label,
        }
    }

    pub fn show(&self) {
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    pub fn toggle(&self) {
        self.container.set_visible(!self.container.is_visible());
    }

    /// Update the panel content with vocab word data.
    pub fn update(
        &self,
        word: &str,
        definition: Option<&str>,
        etymology: Option<&str>,
        gloss: Option<&str>,
        vocab_fg: &str,
    ) {
        // Word label — use vocab color via Pango markup
        self.word_label.set_markup(&format!(
            "<span foreground=\"{}\">{}</span>",
            glib::markup_escape_text(vocab_fg),
            glib::markup_escape_text(word),
        ));

        // Definition
        if let Some(def) = definition {
            self.definition_label.set_text(def);
            self.definition_label.set_visible(true);
            self.header_label.set_visible(true);
        } else {
            self.definition_label.set_visible(false);
            self.header_label.set_visible(false);
        }

        // Etymology
        if let Some(etym) = etymology {
            self.etymology_label.set_markup(etym);
            self.etymology_label.set_visible(true);
            self.etymology_header.set_visible(true);
        } else {
            self.etymology_label.set_visible(false);
            self.etymology_header.set_visible(false);
        }

        // Gloss
        if let Some(g) = gloss {
            self.gloss_label.set_text(g);
            self.gloss_label.set_visible(true);
            self.gloss_header.set_visible(true);
        } else {
            self.gloss_label.set_visible(false);
            self.gloss_header.set_visible(false);
        }

        // Scroll to top
        if let Some(adj) = self.scrolled.vadjustment() {
            adj.set_value(0.0);
        }
    }
}
```

- [ ] **Step 3: Build to verify**

Run: `cargo build`
Expected: compiles (with dead_code warnings for unused fields, which is fine)

- [ ] **Step 4: Commit**

```bash
git add src/ui/definition_panel.rs src/ui/mod.rs
git commit -m "feat: add DefinitionPanel widget"
```

---

### Task 6: Wire definition panel into AppState and layout

**Files:**
- Modify: `src/app.rs` (AppState, build_window, CSS)
- Modify: `src/theme.rs:222-294` (generate_css)

- [ ] **Step 1: Add definition_panel field to AppState**

Add to `AppState` after `definition_panel_visible`:

```rust
pub definition_panel: crate::ui::definition_panel::DefinitionPanel,
```

- [ ] **Step 2: Create and attach definition panel in build_window**

After creating `content_hbox` and appending `scrolled`, add:

```rust
let definition_panel = crate::ui::definition_panel::DefinitionPanel::new();
content_hbox.append(&definition_panel.container);
```

- [ ] **Step 3: Initialize definition_panel in AppState construction**

Add to the `AppState` construction:

```rust
definition_panel,
```

- [ ] **Step 4: Add definition panel CSS to generate_css**

In `src/theme.rs` `generate_css()`, add before the closing quote:

```rust
.definition-panel {{ background-color: {bg}; color: {fg}; \
  border-radius: 12px; padding: 20px 24px; }} \
.definition-header {{ font-size: 11px; color: {dim}; \
  letter-spacing: 2px; font-weight: bold; }} \
.definition-word {{ font-size: 16px; }} \
.definition-text {{ opacity: 0.85; font-size: {size}pt; font-family: {font}; }} \
.definition-etymology {{ opacity: 0.7; font-size: 12px; }} \
.definition-gloss {{ opacity: 0.7; font-size: 12px; }} \
.definition-hint {{ font-size: 11px; color: {dim}; \
  border-top: 1px solid {dim}; padding-top: 8px; margin-top: 12px; }} \
```

- [ ] **Step 5: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/theme.rs
git commit -m "feat: wire definition panel into layout and add CSS"
```

---

### Task 7: Add update_definition_panel helper

**Files:**
- Modify: `src/app.rs` (new function)

- [ ] **Step 1: Implement update_definition_panel**

Add to `src/app.rs`:

```rust
/// Update the definition panel with data for the given vocab word.
/// Runs DB queries to fetch definition, etymology, and gloss.
pub fn update_definition_panel(state: &AppState, word: &str) {
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };

    let definition = crate::db::queries::load_vocab_definition(&conn, word);
    let etymology = crate::db::queries::load_vocab_etymology(&conn, word);

    // Get citation for current line to find contextual gloss
    let gloss = state.current_work.as_ref().and_then(|work| {
        let work_line = state.work_line_for_buffer(state.current_line)?;
        let line = work.lines.get(work_line)?;
        crate::db::queries::load_vocab_gloss(&conn, word, &work.abbrev, &line.citation)
    });

    // Format etymology as Pango markup
    let etym_markup = etymology.map(|e| {
        let mut parts = Vec::new();
        if let Some(ref prefix) = e.prefix {
            let gloss = e.prefix_gloss.as_deref().unwrap_or("");
            parts.push(format!(
                "<span foreground=\"{}\">{}</span> \"{}\"",
                state.theme.vocab_fg, glib::markup_escape_text(prefix), glib::markup_escape_text(gloss)
            ));
        }
        if let Some(ref root) = e.root {
            let gloss = e.root_gloss.as_deref().unwrap_or("");
            if !parts.is_empty() { parts.push(" + ".to_string()); }
            parts.push(format!(
                "<span foreground=\"{}\">{}</span> \"{}\"",
                state.theme.vocab_fg, glib::markup_escape_text(root), glib::markup_escape_text(gloss)
            ));
        }
        if let Some(ref suffix) = e.suffix {
            let gloss = e.suffix_gloss.as_deref().unwrap_or("");
            if !parts.is_empty() { parts.push(" + ".to_string()); }
            parts.push(format!(
                "<span foreground=\"{}\">{}</span> \"{}\"",
                state.theme.vocab_fg, glib::markup_escape_text(suffix), glib::markup_escape_text(gloss)
            ));
        }
        parts.join("")
    });

    state.definition_panel.update(
        word,
        definition.as_ref().map(|(d, _)| d.as_str()),
        etym_markup.as_deref(),
        gloss.as_deref(),
        &state.theme.vocab_fg,
    );
}
```

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: add update_definition_panel helper for vocab lookups"
```

---

### Task 8: Add w/W keybindings for vocab navigation

**Files:**
- Modify: `src/input/keymap.rs` (add w/W key handlers)
- Modify: `src/input/navigation.rs` (add vocab jump functions)

- [ ] **Step 1: Add vocab navigation functions to navigation.rs**

Add to `src/input/navigation.rs`:

```rust
/// Jump to the next vocab word occurrence after current position.
pub fn jump_to_next_vocab(state: &mut AppState) {
    if state.vocab_matches.is_empty() {
        return;
    }

    let next_idx = match state.vocab_match_idx {
        Some(idx) => {
            if idx + 1 < state.vocab_matches.len() {
                idx + 1
            } else {
                0 // wrap
            }
        }
        None => {
            // Find first match after current line
            state.vocab_matches
                .iter()
                .position(|m| m.line_index > state.current_line)
                .unwrap_or(0)
        }
    };

    state.vocab_match_idx = Some(next_idx);
    let target_line = state.vocab_matches[next_idx].line_index;
    state.current_line = target_line;
    update_highlight(state);
    update_highlight_and_center(state);
    seek_to_current_line(state);

    // Auto-show definition panel
    state.definition_panel_visible = true;
    state.definition_panel.show();
    let word = state.vocab_matches[next_idx].word.clone();
    crate::app::update_definition_panel(state, &word);
}

/// Jump to a specific vocab match index. Used by concordance picker.
pub fn jump_to_vocab_at(state: &mut AppState, match_idx: usize) {
    if match_idx >= state.vocab_matches.len() {
        return;
    }
    state.vocab_match_idx = Some(match_idx);
    let target_line = state.vocab_matches[match_idx].line_index;
    state.current_line = target_line;
    update_highlight(state);
    update_highlight_and_center(state);
    seek_to_current_line(state);

    state.definition_panel_visible = true;
    state.definition_panel.show();
    let word = state.vocab_matches[match_idx].word.clone();
    crate::app::update_definition_panel(state, &word);
}

/// Jump to the previous vocab word occurrence before current position.
pub fn jump_to_prev_vocab(state: &mut AppState) {
    if state.vocab_matches.is_empty() {
        return;
    }

    let prev_idx = match state.vocab_match_idx {
        Some(idx) => {
            if idx > 0 {
                idx - 1
            } else {
                state.vocab_matches.len() - 1 // wrap
            }
        }
        None => {
            // Find last match before current line
            state.vocab_matches
                .iter()
                .rposition(|m| m.line_index < state.current_line)
                .unwrap_or(state.vocab_matches.len() - 1)
        }
    };

    state.vocab_match_idx = Some(prev_idx);
    let target_line = state.vocab_matches[prev_idx].line_index;
    state.current_line = target_line;
    update_highlight(state);
    update_highlight_and_center(state);
    seek_to_current_line(state);

    // Auto-show definition panel
    state.definition_panel_visible = true;
    state.definition_panel.show();
    let word = state.vocab_matches[prev_idx].word.clone();
    crate::app::update_definition_panel(state, &word);
}
```

- [ ] **Step 2: Add w/W key handlers to keymap.rs**

In `src/input/keymap.rs`, in the single-keys `match` block (around line 549), add before the `"Escape"` arm:

```rust
"w" => {
    navigation::jump_to_next_vocab(&mut state.borrow_mut());
    true
}
"W" => {
    navigation::jump_to_prev_vocab(&mut state.borrow_mut());
    true
}
```

- [ ] **Step 3: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap.rs src/input/navigation.rs
git commit -m "feat: add w/W keybindings for vocab word navigation"
```

---

### Task 9: Add \ and Alt+\ keybindings

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Add \ key handler (toggle definition panel)**

In the single-keys `match` block, add before `"Escape"`:

```rust
"backslash" => {
    let mut s = state.borrow_mut();
    s.definition_panel_visible = !s.definition_panel_visible;
    if s.definition_panel_visible {
        s.definition_panel.show();
        // Update panel with nearest vocab word on cursor line
        if let Some(m) = s.vocab_matches.iter().find(|m| m.line_index == s.current_line) {
            let word = m.word.clone();
            crate::app::update_definition_panel(&s, &word);
        }
    } else {
        s.definition_panel.hide();
    }
    true
}
```

- [ ] **Step 2: Add Alt+\ handler (toggle vocab highlighting)**

In the Alt key section of keymap.rs (where `Alt+f` and `Alt+i` are handled), add:

```rust
if is_alt && key_name == "backslash" {
    let mut s = state.borrow_mut();
    s.vocab_highlight_visible = !s.vocab_highlight_visible;
    if s.vocab_highlight_visible {
        crate::app::apply_vocab_highlighting(&s);
    } else {
        crate::app::remove_vocab_highlighting(&s);
    }
    s.config.vocab_highlight_visible = s.vocab_highlight_visible;
    crate::config::save(&s.config);
    crate::logging::log(&format!("VOCAB: highlighting {}", if s.vocab_highlight_visible { "on" } else { "off" }));
    return true;
}
```

- [ ] **Step 3: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat: add \\ toggle panel and Alt+\\ toggle highlighting keybindings"
```

---

### Task 10: Create concordance picker widget

**Files:**
- Create: `src/ui/concordance_picker.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Register the module**

Add to `src/ui/mod.rs`:

```rust
pub mod concordance_picker;
```

- [ ] **Step 2: Create concordance_picker.rs**

Create `src/ui/concordance_picker.rs`:

```rust
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Entry, Label, ListBox, ListBoxRow, Orientation, Overlay, ScrolledWindow,
};

pub struct ConcordancePicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    words: Vec<(String, usize)>,
}

impl ConcordancePicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let picker_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(400)
            .height_request(400)
            .build();
        picker_box.add_css_class("concordance-picker");

        // Title
        let title = Label::builder()
            .label("Vocab Words")
            .halign(gtk4::Align::Start)
            .build();
        title.add_css_class("settings-title");
        picker_box.append(&title);

        let search_entry = Entry::builder()
            .placeholder_text("Filter...")
            .build();
        picker_box.append(&search_entry);

        let list_box = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .build();

        let scrolled = ScrolledWindow::builder()
            .child(&list_box)
            .vexpand(true)
            .build();
        picker_box.append(&scrolled);

        // Footer
        let footer = Label::builder()
            .label("j/k navigate \u{00B7} Enter jump \u{00B7} Esc close")
            .build();
        footer.add_css_class("settings-footer");
        picker_box.append(&footer);

        ConcordancePicker {
            overlay,
            picker_box,
            search_entry,
            list_box,
            words: Vec::new(),
        }
    }

    pub fn set_words(&mut self, words: Vec<(String, usize)>) {
        self.words = words;
        self.populate_list("");
    }

    pub fn show(&mut self) {
        self.picker_box.set_visible(true);
        self.search_entry.set_text("");
        self.search_entry.grab_focus();
        self.populate_list("");
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.picker_box.is_visible()
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.picker_box);
        self.picker_box.set_visible(false);
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    pub fn populate_list(&self, filter: &str) {
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        let filter_lower = filter.to_lowercase();

        for (word, count) in &self.words {
            if !filter.is_empty() && !word.contains(&filter_lower) {
                continue;
            }

            let row_box = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(0)
                .build();
            row_box.add_css_class("settings-row");

            let word_label = Label::builder()
                .label(word)
                .halign(gtk4::Align::Start)
                .hexpand(true)
                .build();

            let count_label = Label::builder()
                .label(&format!("{} occurrence{}", count, if *count == 1 { "" } else { "s" }))
                .halign(gtk4::Align::End)
                .opacity(0.5)
                .build();

            row_box.append(&word_label);
            row_box.append(&count_label);

            let row = ListBoxRow::builder().child(&row_box).build();
            row.set_widget_name(word);
            self.list_box.append(&row);
        }

        if let Some(first) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&first));
        }
    }

    pub fn selected_word(&self) -> Option<String> {
        self.list_box
            .selected_row()
            .map(|row| row.widget_name().to_string())
    }

    pub fn move_selection(&self, delta: i32) {
        if let Some(current) = self.list_box.selected_row() {
            let idx = current.index();
            let new_idx = (idx + delta).max(0);
            if let Some(row) = self.list_box.row_at_index(new_idx) {
                self.list_box.select_row(Some(&row));
            }
        }
    }
}
```

- [ ] **Step 3: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add src/ui/concordance_picker.rs src/ui/mod.rs
git commit -m "feat: add ConcordancePicker widget"
```

---

### Task 11: Wire concordance picker into AppState and keybindings

**Files:**
- Modify: `src/app.rs` (AppState, build_window)
- Modify: `src/input/keymap.rs` (Ctrl+\ and overlay key routing)
- Modify: `src/theme.rs` (CSS)

- [ ] **Step 1: Add concordance_picker to AppState**

Add field to `AppState`:

```rust
pub concordance_picker: crate::ui::concordance_picker::ConcordancePicker,
```

- [ ] **Step 2: Create and attach in build_window**

In `build_window`, after the correction overlay setup and before the action popup (around line 267), add:

```rust
let concordance_picker = crate::ui::concordance_picker::ConcordancePicker::new();
concordance_picker.attach(&correction_overlay.overlay);
concordance_picker.overlay.set_vexpand(true);
```

Update the action popup to attach to `concordance_picker.overlay` instead of `correction_overlay.overlay`:

```rust
concordance_picker.overlay.add_overlay(&action_popup_widget.container);
```

Update the vbox to use `concordance_picker.overlay`:

```rust
vbox.append(&concordance_picker.overlay);
```

- [ ] **Step 3: Connect search entry filter**

After the media picker filter connection, add:

```rust
let state_for_concordance_filter = Rc::clone(&state);
{
    let s = state.borrow();
    s.concordance_picker.search_entry().connect_changed(move |entry| {
        let text = entry.text();
        state_for_concordance_filter.borrow().concordance_picker.populate_list(&text);
    });
}
```

- [ ] **Step 4: Initialize in AppState construction**

Add to `AppState`:

```rust
concordance_picker,
```

- [ ] **Step 5: Add Ctrl+\ key handler**

In `src/input/keymap.rs`, in the Ctrl key section (around line 220), add:

```rust
if is_ctrl && key_name == "backslash" {
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());
    if let Some(abbrev) = abbrev {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let words = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                    crate::db::queries::load_vocab_word_list(&conn, &abbrev)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
            let mut s = state_clone.borrow_mut();
            s.concordance_picker.set_words(words);
            s.concordance_picker.show();
        });
    }
    return true;
}
```

- [ ] **Step 6: Add concordance picker overlay key routing**

In `src/input/keymap.rs`, add a new overlay guard block after the keybinds_overlay block (around line 390):

```rust
// Concordance picker overlay
if state.borrow().concordance_picker.is_visible() {
    match key_name {
        "j" => {
            state.borrow().concordance_picker.move_selection(1);
            return true;
        }
        "k" => {
            state.borrow().concordance_picker.move_selection(-1);
            return true;
        }
        "Return" => {
            let selected = state.borrow().concordance_picker.selected_word();
            if let Some(word) = selected {
                {
                    state.borrow().concordance_picker.hide();
                }
                let mut s = state.borrow_mut();
                if let Some(idx) = s.vocab_matches.iter().position(|m| m.word == word) {
                    navigation::jump_to_vocab_at(&mut s, idx);
                }
            }
            return true;
        }
        "Escape" => {
            state.borrow().concordance_picker.hide();
            return true;
        }
        _ => return true,
    }
}
```

- [ ] **Step 7: Add concordance picker CSS**

In `src/theme.rs` `generate_css()`, add:

```rust
.concordance-picker {{ background-color: rgba(40, 40, 40, 0.95); color: white; \
  padding: 16px; border-radius: 8px; }} \
```

- [ ] **Step 8: Build to verify**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 9: Commit**

```bash
git add src/app.rs src/input/keymap.rs src/theme.rs
git commit -m "feat: wire concordance picker with Ctrl+\\ keybinding"
```

---

### Task 12: Final integration and testing

**Files:**
- All modified files

- [ ] **Step 1: Run clippy**

Run: `cargo clippy`
Expected: no errors (warnings OK for dead_code on unused struct fields)

- [ ] **Step 2: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 3: Build release**

Run: `cargo build`
Expected: compiles successfully

- [ ] **Step 4: Commit any remaining fixes**

If clippy or tests required changes:

```bash
git add -u
git commit -m "fix: address clippy warnings and test issues"
```
