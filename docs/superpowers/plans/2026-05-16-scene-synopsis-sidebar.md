# Scene Synopsis Sidebar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show scene synopses in the right sidebar (reusing the vocab popup widget) when the cursor enters a Shakespeare scene, toggled with H.

**Architecture:** Extend the existing VocabPopup with a `update_synopsis()` method. Add a `SidebarMode` enum to AppState that controls whether vocab or synopsis is rendered. The H keybind toggles between modes. Synopses are bulk-loaded from a new `scene_synopses` table on work open and cached in a HashMap keyed by (div1, div2).

**Tech Stack:** Rust, GTK4, rusqlite, SQLite

---

### Task 1: Database Schema and Query Function

**Files:**
- Modify: `src/db/queries.rs` (add `load_synopses` function after line ~427)

- [ ] **Step 1: Create the scene_synopses table in lit.db**

Run this against the live database:

```bash
sqlite3 ~/utono/litdb/data/lit.db "CREATE TABLE IF NOT EXISTS scene_synopses (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    work_abbrev TEXT NOT NULL,
    div1 INTEGER NOT NULL,
    div2 INTEGER NOT NULL,
    synopsis TEXT NOT NULL,
    UNIQUE(work_abbrev, div1, div2)
);"
```

- [ ] **Step 2: Add load_synopses to queries.rs**

Add after the existing `load_translations` function:

```rust
pub fn load_synopses(conn: &Connection, work_abbrev: &str) -> HashMap<(i64, i64), String> {
    let mut map = HashMap::new();
    let mut stmt = match conn.prepare(
        "SELECT div1, div2, synopsis FROM scene_synopses WHERE work_abbrev = ?1"
    ) {
        Ok(s) => s,
        Err(_) => return map,
    };
    let rows = stmt.query_map([work_abbrev], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
    });
    if let Ok(rows) = rows {
        for row in rows.flatten() {
            map.insert((row.0, row.1), row.2);
        }
    }
    map
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: success (function is unused for now — no warning since it's pub)

- [ ] **Step 4: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat: add scene_synopses table and load_synopses query"
```

---

### Task 2: Action Enum and Keybind Registration

**Files:**
- Modify: `src/input/actions/mod.rs`
- Modify: `src/input/keymap_config.rs`
- Modify: `src/ui/keybinds_overlay.rs`

- [ ] **Step 1: Add ToggleSynopsis variant to Action enum**

In `src/input/actions/mod.rs`, add after `ToggleTranslations`:

```rust
    // Synopsis
    ToggleSynopsis,
```

- [ ] **Step 2: Add ToggleSynopsis to category() match**

In the `Display` arm of `category()`, add `Action::ToggleSynopsis` to the list (after `Action::ToggleTranslations`):

```rust
            | Action::ToggleSynopsis
```

- [ ] **Step 3: Add ToggleSynopsis to name() match**

Add after the `ToggleTranslations` entry:

```rust
            Action::ToggleSynopsis => "ToggleSynopsis",
```

- [ ] **Step 4: Add default keybind in keymap_config.rs**

In `vocab_bindings()` function, add at the end of the vec before the closing `]`:

```rust
        (KeyCombo::plain("H"), Action::ToggleSynopsis),
```

- [ ] **Step 5: Update keybinds overlay**

In `src/ui/keybinds_overlay.rs`, change line 76 from:

```rust
    bare("h", "H", "auto vocab"),
```

to:

```rust
    key("h", "H", "auto vocab", "H: synopsis", &[]),
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo build`
Expected: warning about unused variant in dispatch_action (we'll add the handler in Task 4)

- [ ] **Step 7: Commit**

```bash
git add src/input/actions/mod.rs src/input/keymap_config.rs src/ui/keybinds_overlay.rs
git commit -m "feat: add ToggleSynopsis action and H keybind"
```

---

### Task 3: AppState Fields and VocabPopup Extension

**Files:**
- Modify: `src/app.rs` (AppState struct + display_work + new helper functions)
- Modify: `src/ui/vocab_popup.rs`

- [ ] **Step 1: Add SidebarMode enum to app.rs**

Add before the `AppState` struct definition (around line 55):

```rust
#[derive(Clone, Copy, PartialEq)]
pub enum SidebarMode {
    Vocab,
    Synopsis,
}
```

- [ ] **Step 2: Add synopsis fields to AppState**

Add after `vocab_popup_fade_gen` field (line ~171):

```rust
    pub sidebar_mode: SidebarMode,
    pub synopsis_cache: HashMap<(i64, i64), String>,
    pub synopsis_visible: bool,
```

- [ ] **Step 3: Initialize new fields in AppState construction**

Find where AppState is constructed (look for `AppState {` in app.rs) and add:

```rust
            sidebar_mode: SidebarMode::Vocab,
            synopsis_cache: HashMap::new(),
            synopsis_visible: false,
```

- [ ] **Step 4: Load synopses in display_work_at_with_prepared**

In `display_work_at_with_prepared`, after the translations loading block (around line 1633), add:

```rust
    // Load scene synopses for this work
    if let Some(ref work) = state.current_work {
        if let Ok(conn) = crate::db::queries::open_db() {
            let base_abbrev = base_work_abbrev(&work.abbrev);
            state.synopsis_cache = crate::db::queries::load_synopses(&conn, base_abbrev);
            crate::logging::log(&format!(
                "SYNOPSIS: loaded {} scene synopses for {}",
                state.synopsis_cache.len(),
                base_abbrev,
            ));
        }
    }
    state.sidebar_mode = SidebarMode::Vocab;
    state.synopsis_visible = false;
```

- [ ] **Step 5: Add base_work_abbrev helper function**

Add as a free function in `src/app.rs` (near the other helper functions):

```rust
/// Strip variant suffixes (-Amb, -BBC, -Ep-N) to get base work abbreviation
/// for shared data like synopses.
pub fn base_work_abbrev(abbrev: &str) -> &str {
    if let Some(pos) = abbrev.find('-') {
        &abbrev[..pos]
    } else {
        abbrev
    }
}
```

- [ ] **Step 6: Add update_synopsis method to VocabPopup**

In `src/ui/vocab_popup.rs`, add after the existing `update()` method (before the closing `}` of `impl VocabPopup`):

```rust
    /// Render a scene synopsis in the popup.
    pub fn update_synopsis(&self, scene_label: &str, synopsis: &str) {
        while let Some(child) = self.content_box.first_child() {
            self.content_box.remove(&child);
        }

        self.header_label.set_text("SYNOPSIS");
        self.header_label.set_visible(true);
        self.counter_label.set_visible(false);

        let scene_label_widget = Label::builder()
            .halign(gtk4::Align::Start)
            .margin_bottom(12)
            .build();
        scene_label_widget.add_css_class("definition-word");
        scene_label_widget.set_text(scene_label);
        self.content_box.append(&scene_label_widget);

        let synopsis_label = Label::builder()
            .halign(gtk4::Align::Start)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::Word)
            .build();
        synopsis_label.add_css_class("definition-text");
        synopsis_label.set_text(synopsis);
        self.content_box.append(&synopsis_label);

        self.footer_label.set_visible(false);
    }
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo build`
Expected: success (may have warning about unused `synopsis_cache` — that's fine until Task 4)

- [ ] **Step 8: Commit**

```bash
git add src/app.rs src/ui/vocab_popup.rs
git commit -m "feat: add SidebarMode, synopsis_cache to AppState; add VocabPopup::update_synopsis"
```

---

### Task 4: Synopsis Display and Toggle Logic

**Files:**
- Modify: `src/app.rs` (add show_synopsis helper)
- Modify: `src/input/keymap.rs` (dispatch ToggleSynopsis)
- Modify: `src/input/highlight.rs` (auto-show on scene boundary)

- [ ] **Step 1: Add show_synopsis helper in app.rs**

Add near `open_vocab_popup`:

```rust
/// Show the synopsis for the current scene in the sidebar popup.
pub fn show_synopsis(state: &mut AppState) {
    let (div1, div2) = current_scene_divs(state);
    if let Some(synopsis) = state.synopsis_cache.get(&(div1, div2)) {
        let scene_label = format!("Act {}, Scene {}", div1, div2);
        state.vocab_popup.update_synopsis(&scene_label, synopsis);
        state.vocab_popup.show();
        update_vocab_popup_margin(state);
        state.sidebar_mode = SidebarMode::Synopsis;
        state.synopsis_visible = true;
    }
}

/// Get the (div1, div2) of the current line.
fn current_scene_divs(state: &AppState) -> (i64, i64) {
    if let Some(ref work) = state.current_work {
        let work_idx = if let Some(ref lm) = state.line_map {
            lm.buffer_to_work.get(state.current_line).copied().flatten()
        } else {
            Some(state.current_line)
        };
        if let Some(idx) = work_idx {
            if let Some(line) = work.lines.get(idx) {
                return (line.div1, line.div2);
            }
        }
    }
    (0, 0)
}

/// Check if the current line is the first line of a new scene.
fn is_first_line_of_scene(state: &AppState) -> bool {
    if state.current_line == 0 {
        return true;
    }
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return false,
    };
    let cur_idx = if let Some(ref lm) = state.line_map {
        lm.buffer_to_work.get(state.current_line).copied().flatten()
    } else {
        Some(state.current_line)
    };
    let prev_idx = if let Some(ref lm) = state.line_map {
        lm.buffer_to_work.get(state.current_line - 1).copied().flatten()
    } else {
        Some(state.current_line - 1)
    };
    match (cur_idx, prev_idx) {
        (Some(ci), Some(pi)) => {
            let cur = &work.lines[ci];
            let prev = &work.lines[pi];
            cur.div1 != prev.div1 || cur.div2 != prev.div2
        }
        _ => false,
    }
}
```

- [ ] **Step 2: Add toggle_synopsis handler in app.rs**

Add after `show_synopsis`:

```rust
/// Toggle between synopsis and vocab sidebar modes.
pub fn toggle_synopsis(state: &mut AppState) {
    if state.synopsis_cache.is_empty() {
        return;
    }
    if state.sidebar_mode == SidebarMode::Synopsis && state.synopsis_visible {
        // Switch to vocab
        state.sidebar_mode = SidebarMode::Vocab;
        state.synopsis_visible = false;
        if state.vocab_popup_auto {
            open_vocab_popup(state);
        } else {
            close_vocab_popup(state);
        }
    } else {
        // Switch to synopsis
        let (div1, div2) = current_scene_divs(state);
        if state.synopsis_cache.contains_key(&(div1, div2)) {
            show_synopsis(state);
        }
    }
}
```

- [ ] **Step 3: Wire ToggleSynopsis in dispatch_action**

In `src/input/keymap.rs`, in the `dispatch_action` function, add after the `ToggleTranslations` arm:

```rust
        // Synopsis
        ToggleSynopsis => crate::app::toggle_synopsis(&mut state.borrow_mut()),
```

- [ ] **Step 4: Add auto-show on scene boundary in highlight.rs**

In `src/input/highlight.rs`, modify `auto_show_vocab_popup` to add scene detection. Replace the function with:

```rust
/// If vocab auto-popup is enabled, show/update the popup when the paragraph changes.
/// Also auto-shows synopsis when cursor enters a new scene (if synopses are available).
pub(crate) fn auto_show_vocab_popup(state: &mut AppState) {
    // Auto-show synopsis on scene boundary
    if !state.synopsis_cache.is_empty() && crate::app::is_first_line_of_scene(state) {
        crate::app::show_synopsis(state);
        state.vocab_popup_line = Some(state.current_line);
        return;
    }

    // If synopsis is currently showing, leave it alone (user toggled it on)
    if state.sidebar_mode == crate::app::SidebarMode::Synopsis && state.synopsis_visible {
        return;
    }

    if !state.vocab_popup_auto {
        return;
    }
    // Refresh whenever the current line changes; the refresh function
    // decides whether to show (line has vocab words) or hide (line has none).
    if state.vocab_popup_line != Some(state.current_line) {
        if state.vocab_popup.is_visible() {
            crate::app::refresh_vocab_popup(state);
        } else {
            crate::app::open_vocab_popup(state);
        }
    }
}
```

- [ ] **Step 5: Make is_first_line_of_scene public**

In `src/app.rs`, change `fn is_first_line_of_scene` to `pub fn is_first_line_of_scene` so `highlight.rs` can call it.

- [ ] **Step 6: Verify it compiles**

Run: `cargo build`
Expected: success, no errors

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/input/keymap.rs src/input/highlight.rs
git commit -m "feat: wire ToggleSynopsis dispatch and auto-show on scene boundary"
```

---

### Task 5: Insert Hamlet Synopses (Verification Data)

**Files:**
- Create: `scripts/insert_synopses_hamlet.sql`

- [ ] **Step 1: Create a test SQL file with Hamlet synopses**

Create `scripts/insert_synopses_hamlet.sql` with INSERT statements for all 20 scenes of Hamlet. This allows testing before generating all ~756 scenes.

```sql
INSERT OR REPLACE INTO scene_synopses (work_abbrev, div1, div2, synopsis) VALUES
('Ham', 1, 1, 'On the battlements of Elsinore Castle, sentinels Bernardo and Francisco stand watch on a bitterly cold night. Horatio and Marcellus arrive, and the guards reveal they have twice seen the ghost of the recently deceased King Hamlet. The ghost appears again but refuses to speak, vanishing at the cock''s crow. Horatio, shaken, resolves to tell Prince Hamlet what they have witnessed. The scene establishes the atmosphere of dread and political tension with Norway.'),
('Ham', 1, 2, 'King Claudius addresses his court, acknowledging his hasty marriage to Queen Gertrude (his dead brother''s wife) while dispatching ambassadors to Norway over young Fortinbras''s territorial threats. Laertes receives permission to return to France. Hamlet, dressed in mourning black, reveals his disgust at his mother''s rapid remarriage in his first soliloquy ("O, that this too, too solid flesh would melt"). Horatio arrives and tells Hamlet about the ghost; Hamlet resolves to watch that night.'),
('Ham', 1, 3, 'Laertes warns his sister Ophelia that Hamlet''s attention is merely youthful infatuation and cannot lead to marriage, given Hamlet''s royal obligations. Polonius delivers his famous advice to Laertes ("Neither a borrower nor a lender be," "To thine own self be true") before sending him to France. Then Polonius turns to Ophelia and, more harshly than Laertes, forbids her from seeing Hamlet, dismissing the prince''s vows as traps. Ophelia obediently agrees.'),
('Ham', 1, 4, 'Hamlet, Horatio, and Marcellus wait on the battlements as Claudius carouses below. Hamlet reflects on how one fault can corrupt a person''s entire reputation. The ghost appears and beckons Hamlet to follow it alone. Despite his friends'' desperate warnings that it may be a demon, Hamlet breaks free and follows.'),
('Ham', 1, 5, 'The ghost reveals itself as Hamlet''s father and discloses that Claudius murdered him by pouring poison in his ear while he slept in the garden. The ghost demands revenge but instructs Hamlet to leave Gertrude to heaven and her own conscience. Hamlet is devastated and swears to remember and act. He makes Horatio and Marcellus swear secrecy on his sword and hints that he may adopt an "antic disposition" — pretending madness.'),
('Ham', 2, 1, 'Polonius sends his servant Reynaldo to Paris to spy on Laertes''s behavior through indirect questioning. Ophelia then rushes in, frightened, describing how Hamlet appeared in her room with disheveled clothes, grabbed her wrist, stared at her silently, then departed with a heavy sigh. Polonius concludes Hamlet has gone mad from love and decides to inform the king.'),
('Ham', 2, 2, 'Claudius and Gertrude welcome Rosencrantz and Guildenstern, Hamlet''s old school friends, asking them to spy on the prince. The ambassadors return with news that Norway has redirected Fortinbras toward Poland. Polonius announces his theory that Hamlet is mad for love of Ophelia and proposes eavesdropping on them. Hamlet toys with Polonius ("You are a fishmonger") and then with Rosencrantz and Guildenstern, quickly seeing through their mission. A troupe of players arrives; Hamlet requests "The Murder of Gonzago" with a speech inserted. In his soliloquy ("O, what a rogue and peasant slave am I"), Hamlet plans to use the play to test the ghost''s honesty.'),
('Ham', 3, 1, 'Rosencrantz and Guildenstern report failure. Claudius and Polonius set up the "nunnery" encounter, placing Ophelia where Hamlet will find her while they hide. Hamlet delivers the "To be, or not to be" soliloquy, weighing life against death. When Ophelia appears to return his gifts, Hamlet denies ever loving her and launches into a furious tirade against women and marriage ("Get thee to a nunnery"). Claudius, having watched, concludes Hamlet is not mad from love but is dangerously purposeful, and resolves to send him to England.'),
('Ham', 3, 2, 'Hamlet instructs the players on naturalistic acting ("Suit the action to the word"). He confides in Horatio and asks him to observe Claudius during the play. The court assembles for "The Mousetrap." During the poisoning scene, which mirrors King Hamlet''s murder, Claudius rises in alarm and storms out, calling for lights. Hamlet and Horatio confirm the ghost''s truth. Rosencrantz and Guildenstern summon Hamlet to his mother; Hamlet resolves to "speak daggers to her, but use none."'),
('Ham', 3, 3, 'Claudius, alone, tries to pray, acknowledging his guilt for fratricide ("O, my offence is rank"). Hamlet discovers him kneeling and draws his sword but hesitates — killing Claudius at prayer would send him to heaven, not hell. Hamlet decides to wait for a moment when Claudius is sinning. He leaves Claudius, who reveals his prayer was futile: "My words fly up, my thoughts remain below."'),
('Ham', 3, 4, 'In Gertrude''s closet, Polonius hides behind the arras. Hamlet confronts his mother so aggressively she cries for help. Polonius calls out; Hamlet stabs through the curtain, killing him, hoping it was the king. Hamlet forces Gertrude to compare portraits of his father and Claudius, shaming her marriage. The ghost reappears (visible only to Hamlet) and reminds him of his purpose. Gertrude thinks Hamlet truly mad. Hamlet tells her to avoid Claudius''s bed and reveals he''s being sent to England. He drags Polonius''s body away.'),
('Ham', 4, 1, 'Gertrude reports to Claudius that Hamlet has killed Polonius in a fit of madness. Claudius decides Hamlet must be sent away immediately for England and sends Rosencrantz and Guildenstern to find the body.'),
('Ham', 4, 2, 'Rosencrantz and Guildenstern question Hamlet about Polonius''s body. Hamlet evades them with riddling answers ("The body is with the king, but the king is not with the body") and lets himself be brought to Claudius.'),
('Ham', 4, 3, 'Claudius interrogates Hamlet about Polonius''s body; Hamlet mockingly directs him to look under the stairs. Claudius announces Hamlet will sail for England "for thine especial safety." Alone, Claudius reveals that sealed letters command England to execute Hamlet immediately.'),
('Ham', 4, 4, 'Hamlet encounters Fortinbras''s captain marching to fight over a worthless patch of Polish ground. In his final soliloquy ("How all occasions do inform against me"), Hamlet contrasts Fortinbras''s decisive action over nothing with his own paralysis over everything, and resolves that from now on his thoughts shall be "bloody, or be nothing worth."'),
('Ham', 4, 5, 'Ophelia appears at court, singing disconnected songs about death and betrayal, clearly insane from grief over her father''s murder. Claudius laments the accumulating disasters. Laertes storms in with a mob, demanding to know who killed Polonius. Claudius calms him and promises a full explanation. Ophelia returns in deeper madness, distributing symbolic flowers. Claudius begins to manipulate Laertes toward vengeance against Hamlet.'),
('Ham', 4, 6, 'Horatio receives a letter from Hamlet revealing that pirates attacked the ship to England; Hamlet boarded the pirate vessel during the fight and was taken captive, then released. He is back in Denmark. Rosencrantz and Guildenstern continue to England (where Hamlet''s rewritten letter condemns them). Hamlet asks Horatio to come to him immediately.'),
('Ham', 4, 7, 'Claudius confirms to Laertes that Hamlet killed Polonius and explains why he hasn''t punished him (Gertrude loves Hamlet; the people love him). A messenger brings Hamlet''s letter announcing his return. Claudius and Laertes plot: a fencing match where Laertes will use an unbated (sharp) sword tipped with poison; as backup, Claudius will prepare a poisoned cup. Gertrude enters with news that Ophelia has drowned, garlands falling from a willow over a brook. Laertes grieves.'),
('Ham', 5, 1, 'Two gravediggers joke while digging Ophelia''s grave. Hamlet and Horatio arrive; Hamlet meditates on mortality, handling skulls and discovering one belongs to Yorick, the king''s jester he knew as a child ("Alas, poor Yorick"). Ophelia''s funeral procession arrives with reduced rites (the priest suspects suicide). Laertes leaps into the grave in grief; Hamlet reveals himself, and they grapple before being separated. Hamlet declares his love for Ophelia surpassed Laertes''.'),
('Ham', 5, 2, 'Hamlet tells Horatio how he discovered and rewrote Claudius''s death warrant on the ship, sending Rosencrantz and Guildenstern to their deaths. Osric brings the fencing challenge from Claudius. Despite misgivings ("the readiness is all"), Hamlet accepts. During the match, Hamlet scores hits; Gertrude drinks the poisoned cup despite Claudius''s attempt to stop her; Laertes wounds Hamlet with the poisoned blade; in the scuffle they exchange rapiers and Hamlet wounds Laertes. Gertrude dies, naming the poison. Laertes confesses the plot and blames Claudius. Hamlet stabs Claudius and forces the poisoned drink on him. Claudius dies. Laertes and Hamlet exchange forgiveness. Hamlet, dying, names Fortinbras as his successor and asks Horatio to tell his story. Fortinbras arrives, claims the throne, and orders a soldier''s funeral for Hamlet.');
```

- [ ] **Step 2: Run the SQL**

```bash
sqlite3 ~/utono/litdb/data/lit.db < scripts/insert_synopses_hamlet.sql
```

- [ ] **Step 3: Verify data loaded**

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT COUNT(*) FROM scene_synopses WHERE work_abbrev='Ham'"
```

Expected: `20`

- [ ] **Step 4: Commit**

```bash
git add scripts/insert_synopses_hamlet.sql
git commit -m "data: add Hamlet scene synopses for testing"
```

---

### Task 6: Build Verification and Manual Test

**Files:** (none — integration test)

- [ ] **Step 1: Full build**

Run: `cargo build`
Expected: clean compilation, no errors

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: no errors (warnings acceptable if pre-existing)

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 4: Manual test instructions for user**

The user should:
1. Run `cargo run`
2. Open Hamlet (Ctrl+p, select Ham)
3. Navigate to first line — synopsis should auto-appear in right sidebar
4. Press `j` to advance lines — synopsis stays visible
5. Press `H` — synopsis hides, vocab shows (if line has vocab words)
6. Press `H` again — synopsis re-appears for current scene
7. Navigate to Act 2 (`3` to jump to next scene) — synopsis auto-updates
8. Open a non-Shakespeare work — `H` should do nothing (no synopses loaded)

- [ ] **Step 5: Commit any fixes**

If manual testing reveals issues, fix and commit.

---

### Task 7: Bulk Synopsis SQL for All Shakespeare Plays

**Files:**
- Create: `scripts/insert_synopses.sql`

- [ ] **Step 1: Generate synopses for all 37 Shakespeare plays**

Create `scripts/insert_synopses.sql` covering all ~756 scenes across:
1H4, 1H6, 2H4, 2H6, 3H6, AWW, AYL, Ado, Ant, Cor, Cym, Err, H5, H8, Ham, JC, Jn, LLL, Lr, MM, MND, MV, Mac, Oth, Per, R2, R3, Rom, Shr, TGV, TN, TNK, Tim, Tit, Tmp, Tro, WT, Wiv

Each synopsis should be:
- Beginner-friendly and complete (someone unfamiliar with the scene can understand what happens)
- 3-6 sentences depending on scene complexity
- Written in present tense
- Factual plot summary, not literary analysis
- SQL-escaped (single quotes doubled)

Format:
```sql
INSERT OR REPLACE INTO scene_synopses (work_abbrev, div1, div2, synopsis) VALUES
('abbrev', div1, div2, 'synopsis text'),
...
;
```

Note: The Hamlet entries from Task 5 can be included here (INSERT OR REPLACE handles duplicates).

- [ ] **Step 2: Run the bulk SQL**

```bash
sqlite3 ~/utono/litdb/data/lit.db < scripts/insert_synopses.sql
```

- [ ] **Step 3: Verify counts**

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT work_abbrev, COUNT(*) FROM scene_synopses GROUP BY work_abbrev ORDER BY work_abbrev"
```

Expected: 37 rows with scene counts matching the div1/div2 structure in line_mapping.

- [ ] **Step 4: Commit**

```bash
git add scripts/insert_synopses.sql
git commit -m "data: add scene synopses for all 37 Shakespeare plays"
```
