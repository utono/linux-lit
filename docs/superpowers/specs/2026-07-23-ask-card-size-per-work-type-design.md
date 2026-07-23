# Per-work-type ask card size — design

**Backlog items #13 + #14 (merged).** Remember the ask card's fill fraction per
work-type, so play vs prose vs verse asks each reopen at the proportion that fit
last. Subsumes #14 (retire the dead `set_input_fill_fraction`) by giving that
method a caller again rather than deleting it.

## Problem

The 2-column ask float now serves gloss, journal, and synopsis across every
work-type. Play, prose, and verse asks want slightly different float proportions,
but the card opens at a single hardcoded feel. Separately, `set_input_fill_fraction`
(`src/ui/ask_card.rs:569`) is dead code: nothing calls it, so the field it sets
(`input_fill_fraction`, line 519) is always `None`, and the read block that
consumes it (lines 652–664) is unreachable. #14 flagged the method for deletion.

## Key facts (from codebase survey)

- `AskCardHost` (`src/ui/ask_card.rs`) is shared by the gloss and journal
  overlays (`ask_host`). Its `set_input_fill_fraction(frac)` pins the input to
  `frac` of the overlay card height on `open`; the read block at 652–664
  measures chrome and sets the input height. All present, all currently dead.
- Config already uses per-key `HashMap<String, _>` maps for per-work overrides
  (`work_positions`, `column_overrides`, `root_variants`). A per-work-type
  fraction map fits that idiom exactly.
- `work_type` for the current work is reachable from `AppState` (`is_prose` /
  `is_play` and `current_work.work_type`).

## Decision: repurpose, don't delete

Rather than delete the fraction mechanism (#14) and rebuild it later (#13), keep
the field + read block and **wire per-work-type persistence into the existing
`set_input_fill_fraction`**. #14's dead-code concern is resolved because the
method regains a caller and the read block comes alive.

## Components

1. **Config** — add
   `pub ask_fill_fraction_by_type: HashMap<String, f32>` to `Config`
   (`src/config.rs`), `#[serde(default)]`. Key is a small work-class token
   (`"play"`, `"prose"`, `"verse"`), NOT the raw `work_type` string, so the
   handful of `work_type` values collapse to three buckets. A helper
   `work_class(work_type: &str) -> &'static str` maps each `work_type` to its
   bucket (prose types → `"prose"`, `play` → `"play"`, everything else verse-like
   → `"verse"`). Default (missing key) → a per-bucket compiled default
   (e.g. play 0.60, prose 0.75, verse 0.70 — tune during headless verify).
2. **Read on open** — where each overlay opens its ask card, look up the
   remembered fraction for the current work's class and call
   `ask_host.set_input_fill_fraction(frac)` before `open`. This is the single
   line that revives the dead read block.
3. **Persist on resize/close** — when the user has adjusted the card (or on
   close), write the current fill fraction back into the map under the work's
   class key, and mark config dirty (`mark_work_dirty` / the instance-slot
   merge-on-save path, per the multi-instance rule). Persistence granularity:
   store the fraction that was in effect for that class this session, so the next
   ask of the same class reopens at it.

## Data flow

open ask (class = work_class(work_type)) → `config.ask_fill_fraction_by_type`
lookup (or compiled default) → `set_input_fill_fraction(frac)` →
`AskCardHost::open` applies it (lines 652–664 now live) → on adjust/close, write
the effective fraction back under `class` → merge-on-save.

## Error handling / edge cases

- Missing or corrupt fraction (NaN, ≤0, >1): clamp to a sane range
  (e.g. 0.4..=0.9); fall back to the compiled default if out of range.
- Float vs non-float: the fraction path only applies in the non-float open branch
  (the float branch returns early at line 640–645). Per-work-type sizing here
  governs the pinned single-column ask; leave float width alone (its own field).
- Multi-instance: two instances writing config — the instance-slot merge-on-save
  already covers `HashMap` config fields; add the new map to whatever the merge
  logic iterates, or confirm it merges by-key automatically.

## Testing

- `cargo test --bins` for `work_class` mapping (every known `work_type` →
  correct bucket) and fraction clamping.
- Headless (cage): open an ask on a play, a prose work, and a verse work;
  confirm each reopens at its class's proportion after adjustment. Pixel-measure
  the card proportion, don't eyeball (cage vs GL caveat — final eyeball on the
  real renderer).

## Out of scope

- No per-*work* memory (only per-class: play/prose/verse). No float-width
  persistence. No UI to set the fraction explicitly — it's remembered from use.
