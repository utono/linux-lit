# Prose-aware canonical page top — implementation plan

Spec: `docs/superpowers/specs/2026-07-27-prose-canonical-page-top-design.md`

Branch: `fix/prose-canonical-page-top` (worktree at
`~/utono/linux-lit-wt/fix/prose-canonical-page-top`)

TDD per CLAUDE.md — pagination subsystem, so a failing repro comes first.

## Task 1 — failing unit repro for the helper

Add `#[cfg(test)]` coverage in `src/input/navigation.rs` exercising the
boundary rule against a synthetic prose table (pure, no GTK).

The existing `prose_pages` tests already build synthetic `ProsePage` vectors —
follow that shape. Assert: a line whose first row sits mid-page maps to that
page's `(start_line, start_off)`, NOT to the line itself.

Must FAIL before Task 2 (no such function yet — a compile failure counts as
the red state here, since the helper does not exist).

## Task 2 — add `canonical_page_top_offset_for`

`src/input/navigation.rs`, beside `canonical_page_top_for`.

```rust
pub(crate) fn canonical_page_top_offset_for(state: &AppState, target: usize) -> (usize, i32)
```

Order of authority: prose table → play table → live walk.

- prose: `crate::input::prose_pages::prose_table_boundary_for_line(state, target)`
  returns `Option<(usize, i32)>` — return it directly when `Some`.
- play + live walk: reuse the EXISTING body of `canonical_page_top_for`,
  returning `(top, 0)`.

Then rewrite `canonical_page_top_for` as a wrapper:

```rust
pub(crate) fn canonical_page_top_for(state: &AppState, target: usize) -> usize {
    canonical_page_top_offset_for(state, target).0
}
```

Green Task 1. All five existing callers keep compiling untouched.

## Task 3 — fix `jump_to_line` (Bug B)

`src/input/navigation.rs:3030`. In the `EReader` arm:

```rust
let (top, off) = canonical_page_top_offset_for(state, buffer_line);
set_page_instant_offset(state, top, off);
```

(`set_page_instant_offset` is `scroll.rs:489`, already `pub(crate)`.)

Leave the `Scroll` arm and the `is_line_fully_visible` early return alone.

## Task 4 — fix `display_work` (Bug A)

`src/app/mod.rs:4321-4332`. The `near_end` branches keep their current
behaviour. Replace ONLY the final `else` (`current_line.saturating_sub(1)`)
with the new helper, and set `page_top_offset` alongside `page_top_line`.

Note the scene-heading adjustment immediately below (`is_first_line_of_scene`)
may lower `page_top_line`; when it does, `page_top_offset` must be reset to 0
or the offset would be applied to a DIFFERENT line. Handle that explicitly.

## Task 5 — collapse `chapter_jump_land_ereader`

`src/input/navigation.rs:1882`. Delete the hand-rolled
`prose_table_boundary_for_line` branch; call the new helper instead. Keep the
`is_line_fully_visible` / `update_highlight_only` / `ensure_cursor_visible_ereader`
structure intact.

Behaviour-preserving — it already used the same first-row rule.

## Task 6 — verification (all mandatory; review gates waived, these are not)

1. `cargo build`
2. `cargo clippy`
3. `cargo test` (unit + the new helper test)
4. Headless e2e reproducing the exact user drive: BH-Barrett ch2, `Ctrl+j` →
   `Return` → `Escape`. Assert the log shows `BOTTOM_CLIP_EXACT ...
   page_top=42 top_off=603` and NOT `BOTTOM_CLIP_ROWFILL page_top=47`.
5. Startup check: no `PAGES_PROSE: resnap off-grid` on a clean launch, and
   `PAINT: first frame for page_top=42` lands promptly (not +23s).
6. Nav-fuzz: `./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work BH-Barrett`

Headless gotchas that apply (from CLAUDE.md + ac): `LIT_DEV=1`, fresh
`XDG_RUNTIME_DIR`, `GSK_RENDERER=cairo`, `wlr-randr` to 1920x1236, re-send the
first chord after resize, `run_in_background` for lifecycle, scoped pkill only.

## Task 7 — ledger + docs

Append the failure mode to `docs/troubleshooting/page-turning-mechanics.md`:
tell, root cause, fix. Include the `is_line_fully_visible` latent issue from
the spec's "Known latent issue" section.

Required by CLAUDE.md (diagnosis took more than one session's hypothesis and
the root cause contradicted the first read).

## Task 8 — finish the branch

Per CLAUDE.md: merge back to master locally, then push. Tests pass + clean
tree → `git checkout master` → `git merge --no-ff` → re-verify build → `git
push origin master` → `git worktree remove` → `git branch -d`.
