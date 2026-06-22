# Shared shorten_author helper — design

## Goal

Remove the byte-identical `shorten_author` copied into `concordance.rs` and
`ui/concordance_list_picker.rs`, by promoting the core copy to `pub(crate)` and
deleting the UI copy — with **zero behavior change**. This is audit opportunity
#11, **narrowed from the original ledger entry** after verification (see below).

## Verification narrowed the scope

The ledger flagged #11 as "shorten_author AND shorten_title doubled — confirm
bodies match before merging." On inspection:

- **`shorten_author`** — byte-identical in both modules. Safe to share.
- **`shorten_title`** — **NOT identical.** `concordance.rs` truncates titles >25
  chars at a word boundary (`if t.len() > 25 { &t[..t[..25].rfind(' ')...] }`);
  `concordance_list_picker.rs` does the prefix strip only (no length truncation,
  because it has its own `truncate_around_center` downstream). Unifying them would
  change one site's output — a behavior change, so **out of scope**.

So #11 ships ONLY the `shorten_author` extraction. `shorten_title` stays as two
deliberately-different functions.

## The duplication (in scope)

```rust
fn shorten_author(author: &str) -> &str {
    if let Some(idx) = author.find(',') {
        &author[..idx]
    } else {
        author.rsplit_once(' ').map(|(_, last)| last).unwrap_or(author)
    }
}
```

Identical at `concordance.rs:118` and `concordance_list_picker.rs:119`; one call
site each (`status_work` building a label; the picker building a row label).

## Change

- In `src/concordance.rs`: change `fn shorten_author` to `pub(crate) fn
  shorten_author`. (Core module; the UI picker already depends on `concordance`.)
- In `src/ui/concordance_list_picker.rs`: delete the local `fn shorten_author`;
  change its call site to `crate::concordance::shorten_author(&hit.author)`.

## Explicitly EXCLUDED (stay as-is)

- **Both `shorten_title` functions** — behaviorally different (truncating vs
  non-truncating). Not merged.
- **`truncate_around_center`** (concordance_list_picker) — single copy, nothing to
  dedup.

## Verification

Pure fn move; `cargo build` + `cargo test --bins`. No render change (label text
for the author segment is produced by identical code before and after).
