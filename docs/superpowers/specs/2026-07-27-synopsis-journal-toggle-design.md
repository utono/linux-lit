# `\` in the synopsis — toggle to the band's newest scene Q&A

## Purpose

The synopsis overlay's `\` is a consumed no-op (`keymap.rs:3219`, synopsis
dropped from the reader lap 2026-07-21). Give it a use that matches the surface
you are on: a synopsis shows a chapter or scene, and the journal entries filed
under that same band are its natural companion.

`\` in the synopsis opens the newest `scope='scene'` journal entry for the
band being displayed. `\` in that entry returns to the synopsis. Two stops,
one key.

## Why this pairing

The synopsis and a scene-scoped journal entry address the SAME unit — the
`(div1, div2)` band. `synopsis_overlay_scene` already holds exactly that pair,
for chapters and scenes alike, and `journal_entries.div1/div2` files entries
under the same key. No new addressing is needed; the two surfaces are already
talking about the same thing.

This is deliberately NOT the reader's `\` lap. That lap is segment-scoped —
every stop must cover the cursor's passage (see the 2026-07-27 change). The
synopsis is band-scoped by nature, so its `\` is a band-scoped toggle. Keeping
them distinct is the point: same key, same idea ("show me the companion
material here"), scoped to whatever the current surface addresses.

## Behavior

**From the synopsis.** Read `synopsis_overlay_scene` as `(div1, div2)`. Query
the newest scene-scoped entry in that band:

```sql
SELECT … FROM journal_entries
WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3 AND scope = 'scene'
ORDER BY timestamp DESC, id DESC
LIMIT 1
```

- **Hit** — close the synopsis, open the journal overlay landed on that entry,
  and record that the synopsis is the return stop.
- **Miss** — toast "No journal entry for this chapter" (scene works: "…for
  this scene") and leave the synopsis open, untouched. The key must never
  appear dead.

**From that journal entry.** `\` closes the journal and reopens the synopsis
for the band it came from, clearing the return marker. `\` in a journal entry
reached any other way keeps today's behavior (the reader's overlay cycle).

**"Newest" means newest CREATED.** `journal_entries.timestamp` is a creation
stamp; there is no last-viewed tracking (glosses have `config.last_gloss`,
journal entries have no equivalent). Real MRU would need a schema migration
plus write-on-view plumbing in every journal display path — deliberately not
built. `ORDER BY timestamp DESC` is the whole rule.

## Why scene-scoped only

Passage entries belong to a span inside the band, not to the band itself; the
reader's `\` already reaches those from the passage they cover. Pulling them in
here would reintroduce exactly the scope-blindness removed from the reader
cycle on 2026-07-27 — `\` showing material about a different part of the
chapter than the surface you are looking at.

Consequence: a band whose only entries are passage-scoped toasts a miss. That
is correct — those entries have a home, and it is not the synopsis.

## State

One new `AppState` field: the band the journal was entered FROM, e.g.
`journal_from_synopsis: Option<(i64, i64)>`. Set on the synopsis→journal hop,
taken on the return hop, and cleared wherever a journal session ends by any
other route, so a later unrelated journal open cannot inherit a stale marker.
The `\` cycle's `close_current` and `journal::close_overlay` are the clear
sites.

## Out of scope

- The reader's gloss → journal → syntax lap is untouched. The synopsis does NOT
  rejoin it; this is a separate two-stop toggle.
- `Ctrl+j`, the journal picker, `Ctrl+n/p`, and `Alt+n/p` are unchanged.
- No schema change, no keybind moves (`\` is already routed to the synopsis
  handler; only its body changes).

## Known drift found while specifying — fix in this change

`src/ui/journal_keybinds_overlay.rs:29` describes `Ctrl+n / Ctrl+p` as
"next / prev Q&A in band". The implementation (`nav_page`, `journal.rs:1536`)
walks the WHOLE WORK via `find_all_pages_ordered` — no band filter, no scope
filter — stepping across work, scene, and passage entries alike. Correct the
legend to say what it does.

## Testing

**Unit** (pure helpers, per the house no-AppState-in-tests rule):

- The band-target chooser picks the newest by `(timestamp, id)` from a list,
  and returns None from an empty list.
- The return marker round-trips: set on hop, taken on return, None after.

**On-screen** — the acceptance criterion:

Open a synopsis on BH-Barrett ch. 10 (three scene entries, so "newest" is a
real choice), press `\`, confirm the journal opens on the newest of the three.
Press `\` again, confirm the synopsis returns. Then a band with no scene entry:
confirm the toast and that the synopsis stays open. Verify headlessly, then on
the real renderer.

## Files

- `src/db/journal.rs` — the newest-scene-entry query
- `src/input/keymap.rs:3219` — synopsis `\` arm (currently a no-op)
- `src/input/actions/journal.rs` — the hop + return handlers, marker clearing
- `src/app/mod.rs` — the `journal_from_synopsis` field
- `src/ui/synopsis_keybinds_overlay.rs` — legend entry for `\`
- `src/ui/journal_keybinds_overlay.rs` — the `\` return entry, plus the
  `Ctrl+n/p` correction above
