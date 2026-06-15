# Split Echoes into BCP vs Shakespeare Channels — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the existing echoes feature channel-aware so BCP→Shakespeare echoes and Shakespeare→Shakespeare echoes never appear in the same overlay. Bind the BCP channel to `Alt+e` / `Ctrl+e` / `Ctrl+Shift+E` / `'`; move the Shakespeare→Shakespeare channel to a new key family.

**Architecture:** Introduce an `EchoChannel { Bcp, Shakespeare }` enum threaded through the echo actions, `EchoSession`, and the two DB queries that read cached echoes. The channel is a pure data filter on `echo_work_abbrev` (`LIKE 'BCP%'` vs `NOT LIKE 'BCP%'`) — no schema change. One rendering/navigation implementation; only the data-load boundary and the opening keybind differ.

**Tech Stack:** Rust (linux-lit), `rusqlite`, GTK. Tests are inline `#[test]` fns with in-memory SQLite, matching the existing pattern in `src/db/queries.rs`.

---

## Reference facts (verified against the codebase)

- **Channel definition:** a BCP echo row is an `echo_links` row with
  `echo_work_abbrev LIKE 'BCP%'`; a Shakespeare echo row is `NOT LIKE 'BCP%'`.
  `echo_work_abbrev` is `NOT NULL`, so the `LIKE` is NULL-safe. The BCP repo
  guarantees every BCP row carries that prefix; **no schema change here.**
- **Action enum** (`src/input/actions/mod.rs`): variants `ShowEchoes` (100),
  `ReopenEchoes` (101), `ShowEchoTurns` (102); also referenced at 207–209 (some
  predicate group) and 325–327 (`as_str`/name match).
- **Key bindings** (`src/input/keymap_config.rs`):
  - `(KeyCombo::ctrl("e"), Action::ShowEchoTurns)` — line 236
  - `(KeyCombo::plain("apostrophe"), Action::ReopenEchoes)` — line 270
  - `(KeyCombo::alt("e"), Action::ShowEchoes)` — line 296
  - `(KeyCombo::ctrl_shift("E"), Action::ReopenEchoes)` — line 329
  These currently drive the (only) echo channel. They become the **BCP** channel.
- **Dispatch** (`src/input/keymap.rs:1704–1706`):
  `ShowEchoes => echoes::show_echoes_for_cursor_line(state, tokio_handle)`,
  `ReopenEchoes => echoes::reopen_echoes(state, tokio_handle)`,
  `ShowEchoTurns => echoes::open_echo_turns_picker(state)`.
- **Echo actions** (`src/input/actions/echoes.rs`):
  - `show_echoes_for_cursor_line` — cache lookup: `find_echo_turn(&conn, &key)`
    then `load_echo_links(&conn, turn_id)` (~line 163). (The Voyage *search*
    fallback is a separate path; the BCP channel is **cache-only** — it never
    triggers a live search, because BCP echoes are precomputed by the BCP repo.)
  - `reopen_echoes` (~line 604), `open_echo_turns_picker` (~line 1395),
    `confirm_echo_turns_pick` (~line 1442).
  - `EchoSession` struct at line 18: `{ turn_key, turn_id, links, selected,
    titles, source_doc, origin_work, origin_line_id }`. Constructed at lines 182,
    288, 366, 465.
- **DB queries** (`src/db/queries.rs`):
  - `load_echo_links(conn, turn_id) -> Vec<StoredEchoLink>` — line 1794:
    `... FROM echo_links WHERE turn_id = ?1 ORDER BY curated DESC, rank ASC`.
    **8 call sites** in `echoes.rs` (lines ~163, 349, 604, 791, 826, 911, 1017,
    1112).
  - `list_echo_turns_for_work(conn, work_abbrev) -> Vec<EchoTurnSummary>` —
    line 1705: `FROM echo_turns t JOIN echo_links l ON l.turn_id = t.id WHERE
    t.work_abbrev = ?1 GROUP BY t.id ...`.
- **Tests:** inline `#[test]` in `src/db/queries.rs` using in-memory SQLite
  (`Connection::open_in_memory`), `ensure_echo_tables`, then `insert_echo_links`.

---

## File structure

- `src/db/echo_channel.rs` — new: `EchoChannel` enum + its SQL `WHERE` fragment. Task 1.
- `src/db/queries.rs` — `load_echo_links` and `list_echo_turns_for_work` gain a `channel` arg; new tests. Tasks 1–3.
- `src/db/mod.rs` — register the new module. Task 1.
- `src/input/actions/echoes.rs` — `EchoSession.channel` field; the 3 entry actions become channel-aware; the 8 `load_echo_links` calls pass a channel. Tasks 4–5.
- `src/input/actions/mod.rs` — add channel-suffixed Action variants. Task 6.
- `src/input/keymap.rs` — dispatch the new variants. Task 6.
- `src/input/keymap_config.rs` — BCP keeps `e`-family; Shakespeare gets the new family. Task 7.
- `src/ui/keybinds_overlay.rs` — help card shows both families. Task 8.

---

## Task 1: `EchoChannel` enum + SQL fragment

**Files:**
- Create: `src/db/echo_channel.rs`
- Modify: `src/db/mod.rs`

- [ ] **Step 1: Create `src/db/echo_channel.rs`**

```rust
//! Which class of cross-work echo an overlay shows.
//!
//! The channel is a pure data filter on `echo_links.echo_work_abbrev`:
//! BCP editions are registered as works `BCP1549` / `BCP1559` / `BCP1662`, so a
//! BCP echo row matches `echo_work_abbrev LIKE 'BCP%'`. There is no schema
//! column — the BCP pipeline guarantees the prefix on every row it writes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EchoChannel {
    /// Book of Common Prayer inner-monologue echoes (echo_work_abbrev LIKE 'BCP%').
    Bcp,
    /// Shakespeare-to-Shakespeare dramatic echoes (everything else).
    Shakespeare,
}

impl EchoChannel {
    /// SQL predicate (no leading AND) selecting this channel's echo_links rows.
    /// `echo_work_abbrev` is NOT NULL, so LIKE is well-defined.
    pub fn sql_predicate(self) -> &'static str {
        match self {
            EchoChannel::Bcp => "echo_work_abbrev LIKE 'BCP%'",
            EchoChannel::Shakespeare => "echo_work_abbrev NOT LIKE 'BCP%'",
        }
    }
}
```

- [ ] **Step 2: Register the module in `src/db/mod.rs`**

Add alongside the other `pub mod` lines:

```rust
pub mod echo_channel;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: builds (warnings about unused `EchoChannel` are fine until later tasks).

- [ ] **Step 4: Commit**

```bash
git add src/db/echo_channel.rs src/db/mod.rs
git commit -m "Add EchoChannel enum (BCP vs Shakespeare echo filter)"
```

---

## Task 2: Channel-filter `load_echo_links` (test first)

**Files:**
- Modify: `src/db/queries.rs` (signature + query + new test)

- [ ] **Step 1: Write the failing test** — append to the `#[cfg(test)] mod tests` block in `src/db/queries.rs`

```rust
#[test]
fn load_echo_links_filters_by_channel() {
    use crate::db::echo_channel::EchoChannel;
    let conn = Connection::open_in_memory().unwrap();
    ensure_echo_tables(&conn).unwrap();
    conn.execute(
        "INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) \
         VALUES (1, 'Ham', 5, 1, 1, 4, 'Clown', 'Is she to be buried')",
        [],
    ).unwrap();
    // One BCP echo, one Shakespeare echo on the same turn.
    conn.execute(
        "INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) \
         VALUES (1, 'BCP1559', 11, NULL, 1, 'I am the resurrection', 0.9, 1, 0)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) \
         VALUES (1, 'Mac', 1, 2, 5, 'Tomorrow and tomorrow', 0.8, 0, 0)",
        [],
    ).unwrap();

    let bcp = load_echo_links(&conn, 1, EchoChannel::Bcp).unwrap();
    assert_eq!(bcp.len(), 1);
    assert_eq!(bcp[0].echo_work_abbrev, "BCP1559");

    let shx = load_echo_links(&conn, 1, EchoChannel::Shakespeare).unwrap();
    assert_eq!(shx.len(), 1);
    assert_eq!(shx[0].echo_work_abbrev, "Mac");
}
```

- [ ] **Step 2: Run it to verify it fails (wrong arity)**

Run: `cargo test load_echo_links_filters_by_channel 2>&1 | tail -15`
Expected: compile error — `load_echo_links` takes 2 args, test passes 3.

- [ ] **Step 3: Update `load_echo_links`** (`src/db/queries.rs:1794`)

```rust
pub fn load_echo_links(
    conn: &Connection,
    turn_id: i64,
    channel: crate::db::echo_channel::EchoChannel,
) -> Result<Vec<StoredEchoLink>, rusqlite::Error> {
    let sql = format!(
        "SELECT id, echo_work_abbrev, echo_div1, echo_div2, \
                echo_start_line, echo_text, similarity, curated, rank \
         FROM echo_links WHERE turn_id = ?1 AND {} \
         ORDER BY curated DESC, rank ASC",
        channel.sql_predicate(),
    );
    let mut stmt = conn.prepare(&sql)?;
    // ... keep the existing row-mapping closure body unchanged ...
}
```

(Keep the existing `query_map`/closure that builds `StoredEchoLink`; only the SQL string and the signature change. The predicate is a fixed `&'static str`, so `format!` introduces no injection risk.)

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test load_echo_links_filters_by_channel 2>&1 | tail -5`
Expected: PASS. (The 8 call sites won't compile yet — fixed in Task 5. To run just this test, the crate must build; if call sites block it, do Step 3 of Task 5 in the same change set, or temporarily expect a broken build and rely on Task 5's full-build gate. Prefer: land Tasks 2+5 together so the crate always builds.)

- [ ] **Step 5: Commit** (with Task 5 if needed for a clean build)

```bash
git add src/db/queries.rs
git commit -m "load_echo_links: filter echo_links by EchoChannel"
```

---

## Task 3: Channel-filter `list_echo_turns_for_work` (test first)

**Files:**
- Modify: `src/db/queries.rs` (signature + query + new test)

- [ ] **Step 1: Write the failing test** — append to the tests block

```rust
#[test]
fn list_echo_turns_for_work_filters_by_channel() {
    use crate::db::echo_channel::EchoChannel;
    let conn = Connection::open_in_memory().unwrap();
    ensure_echo_tables(&conn).unwrap();
    // Turn 1 has only a BCP echo; turn 2 has only a Shakespeare echo.
    conn.execute("INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) VALUES (1,'Ham',5,1,1,4,'Clown','a')", []).unwrap();
    conn.execute("INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text) VALUES (2,'Ham',1,2,10,12,'Hamlet','b')", []).unwrap();
    conn.execute("INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) VALUES (1,'BCP1559',11,NULL,1,'x',0.9,1,0)", []).unwrap();
    conn.execute("INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_start_line, echo_text, similarity, curated, rank) VALUES (2,'Mac',1,2,5,'y',0.8,0,0)", []).unwrap();

    let bcp = list_echo_turns_for_work(&conn, "Ham", EchoChannel::Bcp).unwrap();
    assert_eq!(bcp.len(), 1);
    assert_eq!(bcp[0].start_line, 1);

    let shx = list_echo_turns_for_work(&conn, "Ham", EchoChannel::Shakespeare).unwrap();
    assert_eq!(shx.len(), 1);
    assert_eq!(shx[0].start_line, 10);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test list_echo_turns_for_work_filters_by_channel 2>&1 | tail -15`
Expected: compile error (arity mismatch).

- [ ] **Step 3: Update `list_echo_turns_for_work`** (`src/db/queries.rs:1705`)

```rust
pub fn list_echo_turns_for_work(
    conn: &Connection,
    work_abbrev: &str,
    channel: crate::db::echo_channel::EchoChannel,
) -> Result<Vec<EchoTurnSummary>, rusqlite::Error> {
    let sql = format!(
        "SELECT t.div1, t.div2, t.start_line, t.speaker, t.turn_text \
         FROM echo_turns t \
         JOIN echo_links l ON l.turn_id = t.id \
         WHERE t.work_abbrev = ?1 AND l.{} \
         GROUP BY t.id \
         ORDER BY t.div1, t.div2, t.start_line",
        channel.sql_predicate(),
    );
    let mut stmt = conn.prepare(&sql)?;
    // ... keep the existing query_map closure unchanged ...
}
```

(Note the `l.` qualifier — `sql_predicate()` names a bare `echo_work_abbrev`
column, and in this JOIN it lives on `echo_links` aliased `l`.)

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test list_echo_turns_for_work_filters_by_channel 2>&1 | tail -5`
Expected: PASS (subject to the build-together note in Task 2 Step 4).

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "list_echo_turns_for_work: filter turns by EchoChannel"
```

---

## Task 4: Add `channel` to `EchoSession`

**Files:**
- Modify: `src/input/actions/echoes.rs` (struct + 4 construction sites)

- [ ] **Step 1: Add the field to `EchoSession`** (line 18)

```rust
pub struct EchoSession {
    pub channel: crate::db::echo_channel::EchoChannel,
    pub turn_key: EchoTurnKey,
    pub turn_id: Option<i64>,
    pub links: Vec<StoredEchoLink>,
    pub selected: usize,
    pub titles: std::collections::HashMap<String, String>,
    pub source_doc: String,
    pub origin_work: String,
    pub origin_line_id: i64,
}
```

- [ ] **Step 2: Set `channel` at each of the 4 construction sites** (lines ~182, 288, 366, 465)

Each `EchoSession { ... }` literal gains `channel,` where `channel` is the
channel the action was invoked with (threaded in via Task 5). For the
`reopen`/`alt+i` sites that rebuild from an existing session, carry the prior
session's channel forward (`channel: prev.channel`). Concretely, add `channel,`
as the first field in each literal and ensure the enclosing function has a
`channel: EchoChannel` binding in scope (Task 5 adds the parameter).

- [ ] **Step 3: Build gate** — deferred to Task 5 (the functions don't have the
  `channel` binding until their signatures change). No standalone commit; land
  with Task 5.

---

## Task 5: Thread `channel` through the three entry actions + 8 load calls

**Files:**
- Modify: `src/input/actions/echoes.rs`

- [ ] **Step 1: Add a `channel` parameter to the three entry points**

```rust
pub(crate) fn show_echoes_for_cursor_line(
    state: &Rc<RefCell<AppState>>,
    channel: crate::db::echo_channel::EchoChannel,
    tokio_handle: &tokio::runtime::Handle,
) { /* ... */ }

pub(crate) fn open_echo_turns_picker(
    state_rc: &Rc<RefCell<AppState>>,
    channel: crate::db::echo_channel::EchoChannel,
) { /* ... */ }

pub(crate) fn reopen_echoes(
    state: &Rc<RefCell<AppState>>,
    channel: crate::db::echo_channel::EchoChannel,
    tokio_handle: &tokio::runtime::Handle,
) { /* ... */ }
```

- [ ] **Step 2: Pass `channel` to every `load_echo_links` call** (8 sites: ~163, 349, 604, 791, 826, 911, 1017, 1112)

Each becomes `load_echo_links(&conn, turn_id, channel)`. For the helper/reorder
sites (791, 826, 911, 1017, 1112) that already operate within an open session,
use that session's channel: `let channel = s.echo_session.as_ref().map(|x|
x.channel).unwrap_or(EchoChannel::Shakespeare);` (or thread it from the caller).
Prefer reading `echo_session.channel` in those mutation helpers so they always
match the live overlay.

- [ ] **Step 3: Pass `channel` to `list_echo_turns_for_work`** in `open_echo_turns_picker` (~line 1410)

`list_echo_turns_for_work(&conn, &work_abbrev, channel)`.

- [ ] **Step 4: BCP channel is cache-only.** In `show_echoes_for_cursor_line`,
  when `channel == EchoChannel::Bcp`, do **not** fall through to the Voyage
  search path on a cache miss — show the "no echoes" state instead. (BCP echoes
  are precomputed by `ws-book-of-common-prayer-references`; there is no live BCP
  search.) The Shakespeare channel keeps its existing search-on-miss behavior.

- [ ] **Step 5: Set `channel` in the EchoSession literals** (the Task 4 field) from the in-scope `channel` (or `prev.channel` on reopen).

- [ ] **Step 6: Full build**

Run: `cargo build 2>&1 | tail -10`
Expected: only the keymap dispatch (Task 6) is now broken (callers pass too few
args). If you are landing Tasks 2–6 as one buildable change, also do Task 6
before building. Otherwise expect dispatch-site errors here and resolve in Task 6.

- [ ] **Step 7: Commit (with Tasks 2,3,6 for a clean build)**

```bash
git add src/db/queries.rs src/input/actions/echoes.rs
git commit -m "Thread EchoChannel through echo actions and cached-link loads; BCP is cache-only"
```

---

## Task 6: Channel-suffixed Action variants + dispatch

**Files:**
- Modify: `src/input/actions/mod.rs` (enum + the 207–209 group + 325–327 names)
- Modify: `src/input/keymap.rs` (dispatch at 1704–1706)

- [ ] **Step 1: Replace the three echo Action variants with six** (`src/input/actions/mod.rs`, ~line 100)

```rust
    ShowEchoesBcp,
    ReopenEchoesBcp,
    ShowEchoTurnsBcp,
    ShowEchoesShx,
    ReopenEchoesShx,
    ShowEchoTurnsShx,
```

Update the predicate group at ~207–209 and the name match at ~325–327 to list all
six (e.g. `Action::ShowEchoesBcp => "ShowEchoesBcp"`, etc.). Grep to be sure no
other arm references the old names: `rg -n "ShowEchoes|ReopenEchoes|ShowEchoTurns" src`.

- [ ] **Step 2: Dispatch the six variants** (`src/input/keymap.rs`, replace lines 1704–1706)

```rust
        ShowEchoesBcp => crate::input::actions::echoes::show_echoes_for_cursor_line(
            state, crate::db::echo_channel::EchoChannel::Bcp, tokio_handle),
        ReopenEchoesBcp => crate::input::actions::echoes::reopen_echoes(
            state, crate::db::echo_channel::EchoChannel::Bcp, tokio_handle),
        ShowEchoTurnsBcp => crate::input::actions::echoes::open_echo_turns_picker(
            state, crate::db::echo_channel::EchoChannel::Bcp),
        ShowEchoesShx => crate::input::actions::echoes::show_echoes_for_cursor_line(
            state, crate::db::echo_channel::EchoChannel::Shakespeare, tokio_handle),
        ReopenEchoesShx => crate::input::actions::echoes::reopen_echoes(
            state, crate::db::echo_channel::EchoChannel::Shakespeare, tokio_handle),
        ShowEchoTurnsShx => crate::input::actions::echoes::open_echo_turns_picker(
            state, crate::db::echo_channel::EchoChannel::Shakespeare),
```

- [ ] **Step 3: Full build**

Run: `cargo build 2>&1 | tail -10`
Expected: builds (keymap_config still references the old variant names — fix in
Task 7; if landing together, do Task 7 first). Land Tasks 2–7 as one buildable
change set so `cargo build` and `cargo test` pass at the commit boundary.

- [ ] **Step 4: Commit (with Task 7)**

```bash
git add src/input/actions/mod.rs src/input/keymap.rs
git commit -m "Split echo Actions into per-channel Bcp/Shx variants"
```

---

## Task 7: Rebind keys — BCP keeps the `e`-family; Shakespeare gets a new family

**Decision needed first:** pick the Shakespeare→Shakespeare key family. Audit
free keys:

```bash
rg -n "KeyCombo::(alt|ctrl|ctrl_shift)\(\"(s|S|w|W)\"\)" src/input/keymap_config.rs
```

Note: plain `s` and plain `e` are already used in `media_bindings`
(`TogglePlaybackSync`, `SeekShortForward`), so the new family should use a
modifier. **Default choice (adjust if the audit shows a conflict):** the `w`
family (w = cross-work) — `Alt+w` show, `Ctrl+w` turns, `Ctrl+Shift+W` reopen.
Record the final choice in the commit message and the keybinds overlay.

**Files:**
- Modify: `src/input/keymap_config.rs` (lines 236, 270, 296, 329)

- [ ] **Step 1: Repoint the existing `e`-family bindings at the BCP variants**

```rust
(KeyCombo::ctrl("e"), Action::ShowEchoTurnsBcp),       // was ShowEchoTurns (line 236)
(KeyCombo::plain("apostrophe"), Action::ReopenEchoesBcp), // was ReopenEchoes (line 270)
(KeyCombo::alt("e"), Action::ShowEchoesBcp),           // was ShowEchoes (line 296)
(KeyCombo::ctrl_shift("E"), Action::ReopenEchoesBcp),  // was ReopenEchoes (line 329)
```

- [ ] **Step 2: Add the Shakespeare family** (alongside the BCP bindings, same mode list — the reader keymap that holds line 296)

```rust
(KeyCombo::alt("w"), Action::ShowEchoesShx),
(KeyCombo::ctrl("w"), Action::ShowEchoTurnsShx),
(KeyCombo::ctrl_shift("W"), Action::ReopenEchoesShx),
```

(Use the family chosen in the audit; keep the three gestures parallel to the BCP
ones. If `apostrophe` should also have a Shakespeare twin, add one — but the
default keeps `'` BCP-only since it was the single legacy reopen shortcut.)

- [ ] **Step 3: Build + run the full test suite**

Run: `cargo build 2>&1 | tail -5 && cargo test echo 2>&1 | tail -15`
Expected: builds; the channel-filter tests (Tasks 2–3) pass.

- [ ] **Step 4: Commit**

```bash
git add src/input/keymap_config.rs
git commit -m "Bind BCP echoes to e-family; Shakespeare echoes to w-family"
```

---

## Task 8: Update the echo keybinds help overlay

**Files:**
- Modify: `src/ui/keybinds_overlay.rs`

- [ ] **Step 1: Find the echo rows in the overlay**

Run: `rg -n "echo|Echo|Alt\+e|Ctrl\+e|seek \+3.5|echoes" src/ui/keybinds_overlay.rs | head`

- [ ] **Step 2: Update the rows** so the overlay documents both families:
  - `Alt+e` / `Ctrl+e` / `Ctrl+Shift+E` / `'` → "BCP inner-monologue echoes"
    (search / turns picker / reopen).
  - the new `w`-family (or chosen keys) → "Shakespeare cross-work echoes".

  Keep the existing descriptive sentences but retarget which channel each drives,
  matching the actual bindings from Task 7. (This card is the one shown in the
  reference screenshot; both channels must be discoverable here.)

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | tail -5`
Expected: builds.

- [ ] **Step 4: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "Keybinds overlay: document BCP vs Shakespeare echo channels"
```

---

## Task 9: Manual verification

**Files:** none.

- [ ] **Step 1: Build release/run**

Run: `cargo build 2>&1 | tail -3`

- [ ] **Step 2: Seed a quick mixed fixture (optional, if no BCP data yet)**

If the BCP repo hasn't populated echoes, insert one BCP and one Shakespeare echo
on a known turn directly in a scratch copy of `lit.db` to eyeball channel
separation. Otherwise use real data.

- [ ] **Step 3: In the app, on a Shakespeare turn with both kinds of echo:**
  - `Ctrl+e` (BCP turns picker) lists only turns with BCP echoes; opening one
    shows only `BCP*` rows.
  - the `w`-family turns picker lists only Shakespeare-echo turns; opening one
    shows only non-`BCP` rows.
  - `Alt+e` on the BCP channel shows cached BCP echoes (no Voyage search) or the
    no-echoes state; `Alt+w` runs/loads the Shakespeare echoes as before.
  - `Enter` jumps correctly in each channel; `'` / reopen stays within its
    channel.

- [ ] **Step 4: Confirm no intermixing** — neither overlay ever shows a row from
  the other channel.

---

## Final verification

- [ ] `cargo build` clean.
- [ ] `cargo test echo` — channel-filter tests green.
- [ ] `git log --oneline` shows: enum → load filter → turns filter → session field → action threading → action split → rebind → overlay.
- [ ] Manual check (Task 9): channels never intermix; BCP on `e`-family, Shakespeare on the new family.
- [ ] When done, finish the branch per `~/CLAUDE.md` (merge `bcp-echo-channel-split-spec` back to master with `--no-ff`, push, delete) — only after the BCP repo's data pipeline can actually populate BCP echoes, so the feature is testable end-to-end.

---

## Self-review notes (addressed)

- **Spec coverage:** `EchoChannel` enum threaded through actions/session/queries
  (T1,4,5,6), channel = `echo_work_abbrev LIKE 'BCP%'` filter with no schema
  change (T1–T3), BCP on `Alt+e`/`Ctrl+e`/`Ctrl+Shift+E`/`'` + Shakespeare on a
  new family (T6,T7), one rendering impl differentiated only at data-load +
  keybind (no overlay duplication), BCP cache-only / no live search (T5 Step 4),
  keybinds overlay updated (T8). Open item (the new key family) is decided in T7
  Step 0 via a concrete audit command, not left vague.
- **Build-ordering hazard called out:** the signature changes (T2,T3,T5) and the
  Action split (T6,T7) must land together for `cargo build`/`cargo test` to pass
  at a commit boundary; each task notes this and the final commit groups them.
- **All 8 `load_echo_links` call sites enumerated** with line numbers and a rule
  for the mutation-helper sites (read `echo_session.channel`).
- **`l.` qualifier** noted for the JOIN query so the bare-column predicate
  resolves to `echo_links`.
- **Tests** match the existing inline `#[test]` + in-memory SQLite pattern in
  `queries.rs`; they assert separation in both directions.
