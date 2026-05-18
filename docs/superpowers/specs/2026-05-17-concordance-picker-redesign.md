# Concordance Picker Redesign

Date: 2026-05-17

## Summary

Redesign the concordance system to navigate across all works by the same author
in a single instance. Replace spawn-new-instance behavior with in-place work
loading. Change navigation keys to Ctrl+n/Ctrl+p. Populate the word picker with
all content words (minus stopwords) from the author's corpus, sorted
alphabetically.

## Current State

- Concordance picker (`Ctrl+\`) shows vocab_words for the current work only
- Selecting a word searches all works in the database
- Cross-work hits spawn new linux-lit instances via `std::process::Command`
- `r`/`R` dual-purposes as concordance next/prev (when state active) or vocab
  jump (when no state)
- Navigation within a work only (`advance_within_work` filters by current
  `work_abbrev`)

## Design

### Word List Source

New query `load_concordance_words(conn, author) -> Vec<String>`:

1. Fetch all `normalized_text` from `line_mapping` for works by the given author
2. Tokenize in Rust: split on whitespace, strip leading/trailing punctuation
3. Lowercase, deduplicate
4. Filter out English stopwords (~150 function words)
5. Sort alphabetically
6. Return as `Vec<String>`

Stopwords live as `const STOPWORDS: &[&str]` in a new file `src/db/stopwords.rs`.

Cache the word list in `AppState` keyed by author string. Invalidate only when
the author changes (i.e., user opens a work by a different author via library
picker).

### Keybinds

| Key | Action | Context |
|-----|--------|---------|
| `Ctrl+\` | `OpenConcordancePicker` | Reader mode, unchanged |
| `Ctrl+n` | `ConcordanceNext` | Reader mode, new |
| `Ctrl+p` | `ConcordancePrev` | Reader mode, new |
| `r` | `JumpToNextVocab` | Reader mode, always plain vocab jump (ignores concordance state) |
| `R` | `JumpToPrevVocab` | Reader mode, always plain vocab jump (ignores concordance state) |

`ConcordanceNext`/`ConcordancePrev` are no-ops when no concordance state is
active.

### Cross-Work Navigation

When `Ctrl+n`/`Ctrl+p` advances to a hit in a different work:

1. Save current position (`save_position`)
2. Send `MpvCommand::Quit` to disconnect from current media
3. Call `display_work(target_abbrev)` with `target_line_id` pointing to the
   hit's `line_mapping_id`
4. `display_work` handles: buffer rebuild, media discovery/connection, cursor
   positioning, MPV seek to target line's timestamp

`ConcordanceState` is preserved across work switches — it is not cleared or
modified by `display_work`.

Same-work navigation (hit is in current work): move cursor to target line,
center on screen, seek MPV to sentence start. Unchanged from current behavior.

### Hit Query

Modify `find_word_occurrences` to accept an `author` parameter:

```sql
SELECT lm.id, lm.work_abbrev, w.title, w.author,
       lm.div1, lm.div2, lm.line_in_div, lm.canonical_text,
       EXISTS(SELECT 1 FROM line_timestamps lt WHERE lt.line_mapping_id = lm.id) as has_audio
FROM line_mapping lm
JOIN works w ON w.abbrev = lm.work_abbrev
WHERE w.author = ?
  AND lm.normalized_text LIKE '%' || ? || '%'
ORDER BY w.abbrev, lm.div1, lm.div2, lm.line_in_div
```

### ConcordanceState Changes

Replace `advance_within_work(work_abbrev)` / `retreat_within_work(work_abbrev)`
with `advance()` / `retreat()` that navigate the full hit list without filtering
by work:

```rust
pub fn advance(&mut self) -> bool {
    if self.occurrences.is_empty() { return false; }
    self.current_index = (self.current_index + 1) % self.occurrences.len();
    true
}

pub fn retreat(&mut self) -> bool {
    if self.occurrences.is_empty() { return false; }
    let len = self.occurrences.len();
    self.current_index = (self.current_index + len - 1) % len;
    true
}
```

### Concordance State Lifecycle

- **Activated**: User selects a word in the concordance picker. System queries
  all occurrences for that word across the author's works, creates
  `ConcordanceState`, shows concordance bar, jumps to first hit.
- **Persists through**: Ctrl+n/p navigation (including cross-work), normal
  reading, manual work switches via library picker.
- **Cleared when**: User selects a new word in the concordance picker (replaces
  state).
- **No spawning**: All `std::process::Command::new(exe).spawn()` calls in
  concordance code are removed.

### Status Bar

Concordance bar shows: `word [N/total] — Author, Work Title`

Updated after every Ctrl+n/Ctrl+p navigation.

### New Actions

Add to `src/input/actions/mod.rs`:

```rust
ConcordanceNext,
ConcordancePrev,
```

Category: `Vocab`. Wire in `dispatch_action` to call new handler functions.

## Files Modified

- `src/db/concordance.rs` — add author param to `find_word_occurrences`, add
  `load_concordance_words`
- `src/db/stopwords.rs` — new file, `const STOPWORDS`
- `src/db/mod.rs` — add `pub mod stopwords;`
- `src/concordance.rs` — replace `advance_within_work`/`retreat_within_work`
  with `advance`/`retreat`
- `src/input/actions/concordance.rs` — remove spawn logic, add
  `concordance_next`/`concordance_prev` with cross-work `display_work` jump
- `src/input/actions/mod.rs` — add `ConcordanceNext`, `ConcordancePrev`
- `src/input/keymap_config.rs` — add Ctrl+n/Ctrl+p bindings, simplify r/R
- `src/input/keymap.rs` — wire new actions in `dispatch_action`
- `src/app.rs` — add `concordance_word_cache: Option<(String, Vec<String>)>`

## Performance

For Shakespeare (~37 works, ~100k lines), fetching all normalized_text and
tokenizing is estimated at <100ms. The word list is cached per author in
AppState, so repeated picker opens are instant.

Cross-work navigation triggers a full `display_work` reload (buffer rebuild,
media reconnect). This matches the existing cost of opening a work via library
picker — acceptable latency.

## Testing

- Unit test: stopword filtering produces expected word list
- Unit test: `advance()`/`retreat()` wrap correctly across full hit list
- Integration test: `find_word_occurrences` with author filter returns only
  same-author hits
- Manual test: Ctrl+n across works loads new work, seeks MPV, updates bar
