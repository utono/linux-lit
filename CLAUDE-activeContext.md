# Active Context — linux-lit

> Memory bank file. Read this first to understand current state and maintain
> session continuity. Update as work progresses; keep only what is NOT
> recoverable from the code or git history.

_Last updated: 2026-06-27T22:00 CDT (before a reboot)._

## Current Focus

**Branch `fix/inline-translation-clip`** — fixing the 2-col translation overlay
(the `i` key → `ShowTranslationOverlay` side-by-side original/translation view).
It was rewritten from a scrolled layout to **pagination** (whole speaker blocks
per page, like the main card) to eliminate a class of bottom-row clipping that no
scroll-based mask could fix reliably.

Spec: `docs/superpowers/specs/2026-06-27-paginated-translation-overlay-design.md`
Plan: `docs/superpowers/plans/2026-06-27-paginated-translation-overlay.md`

## Status

The pagination rewrite is **committed** (`067e90b`) and working: no clipping,
highlight visible on open, `j`/`k`/`q`/`,` turn pages as the reader cursor
crosses block boundaries.

**ONE FOLLOW-UP FIX IS UNCOMMITTED (working tree):** `,` sometimes grew the
overlay card TALLER than the main-card size (card bottom ran off-screen). Root
cause (found via a `TRANS_PAGE` diagnostic, since removed): standalone
`pango::Layout` block-height measurement UNDER-shoots the rendered TextView
height, so a page measured under-budget renders over-budget, and the card grew
because `set_height_request` is only a MINIMUM.

The uncommitted fix in `src/ui/translation_overlay.rs` is **defense-in-depth**:
1. Wrapped `content_vbox` in a non-scrolling `ScrolledWindow` with
   `propagate_natural_height(false)` + scrollbars off — HARD-CAPS the card to its
   `height_request` so it can never grow (same trick the gloss overlay uses).
2. Conservative page budget `page_height = raw_budget * 9 / 10` so pagination
   under-packs and the under-measured content stays within the cap (no block
   hidden behind it).

`cargo build` clean, `cargo test --bins` = 489 pass. **NOT yet user-verified on a
rendered spread** — that is the immediate next action.

Also modified (uncommitted): `CLAUDE.md` (+14 lines: the "Clipping Bugs — read
clip-prevention.md FIRST" section added earlier this session) — commit it too.

## Next Actions (in order)

1. **User verifies the card-cap fix**: `cargo run`, open `i` on TN, press `,`
   through the whole scene. Confirm (a) the card stays the SAME size as the main
   card on every page (no growing off-screen, rounded bottom corner always
   visible), (b) no block's bottom is cut. If the card still grows OR a block is
   cut, adjust the `9/10` safety factor (or improve the Pango measurement).
2. **Commit the fix** (translation_overlay.rs + the CLAUDE.md clip-rule edit).
3. **Finish the branch** per CLAUDE.md "Finishing a Branch": squash the now
   SUPERSEDED scroll-mask commit `0b5fdc9` (its machinery was deleted in
   `067e90b`), then `git checkout master` → `git merge --no-ff` → re-verify
   build+tests → `git push origin master` → `git branch -d`.

## Recent Decisions (this session, not in git messages)

- The 2-col translation overlay PAGINATES rather than scrolls — chosen by the
  user after multiple scroll-based bottom-clip fixes failed. Pagination removes
  the clip bug class by construction (no partial row ever rendered). The
  scroll/clip machinery (`recompute_translation_bottom_clip`,
  `ClipKind::Custom`/`attach_custom`/`ClipFn`, `scroll_to_highlight`) was DELETED.
- Block heights are measured with a standalone `pango::Layout` (synchronous) to
  avoid the GTK widget-allocation settle races that plagued the scroll version.
  Caveat discovered: Pango under-measures vs the rendered TextView — hence the
  card cap + conservative budget above.
- New general rule added to `docs/troubleshooting/clip-prevention.md`: a Box of
  WRAPPING TextViews should PAGINATE, not be masked (checklist #8 + "Pagination
  instead of a mask"). And: the paged main-card clip is page_top-relative — never
  call it on a cursor-scrolled view (checklist #7).

## Earlier this session (already merged to master, for context)

- `feat/prose-nyt-column` (merged + pushed): prose works render a centered
  `card_width/5` NYTimes-style column (main card + synopsis/gloss/journal
  overlays); over-tall-paragraph bottom-clip fix; `clip-prevention.md` updates.

## Pointers

- Clipping bugs → ALWAYS read `docs/troubleshooting/clip-prevention.md` first
  (now also enforced by a rule in `CLAUDE.md`).
- Translation overlay code: `src/ui/translation_overlay.rs` (pagination,
  `paginate`/`page_containing_block`/`block_for_work_idx`, all unit-tested);
  driver/sync: `src/app/translations.rs` (`rebuild_translation_overlay`,
  `sync_translation_overlay` → `show_for_cursor`); keys:
  `src/input/keymap.rs::handle_translation_overlay_key`.
- Do NOT run `cargo run` as the agent (user launches it; see
  `feedback_no_cargo_run` memory). Pagination/clip changes need a RENDERED spread
  to verify — logic tests aren't enough.
