# Navigation Test Harness

In-app deterministic test mode toggled with Ctrl+Shift+T that exercises
page-turn and structural navigation (x, y, 2, 3, [, {) against the
currently loaded work with real GTK layout, checking 6 invariants after
every step.

## Activation

Ctrl+Shift+T toggles on/off. Toast shows "NAV TEST: started" or
"NAV TEST: stopped (N steps, M failures)". Runs on a
`glib::timeout_add_local` timer firing every 300ms (one key press per tick).

## Test sequence

Deterministic script, not random. Exercises all navigation patterns:

1. x x x x x — 5 forward pages
2. y — back one (round-trip check against step 5's origin)
3. y y y y — back 4 more (each checks round-trip)
4. x x x — forward 3 to reach mid-work
5. 3 — next scene jump
6. y — return to pre-jump page
7. 2 — prev scene jump
8. y — return to pre-jump page
9. x x x x x — forward 5
10. { — next chapter jump
11. y — return to pre-jump page
12. [ — prev chapter jump
13. y — return to pre-jump page

Repeats from the new position until end-of-work or step limit (200).

## Invariants

Checked after every simulated key press:

- **No scene break mid-page** — scan visible lines from page_top_line to
  last_fully_visible_line. Skip the opening header block at the top (scene
  header at page start is fine). Any act/scene marker or separator in the
  interior is a failure
- **Viewport fill** — pixel height of lines from page_top_line to
  last_fully_visible_line must fill at least 50% of the viewport. Short
  pages from unconditional clamp are valid; mostly-empty pages are not
- **Forward progress on x** — after PageForward, page_top_line must be
  strictly greater than before
- **y round-trips x** — after x then y, page_top_line must equal the
  pre-x value
- **y after structural jump returns** — after 2/3/[/{, pressing y must
  return page_top_line to its pre-jump value
- **current_line is dialogue** — cursor must be on a dialogue line (plays)

## State

Add to AppState:

- `nav_test_active: bool`
- `nav_test_step: usize` — position in the script
- `nav_test_failures: usize`
- `nav_test_prev_top: usize` — page_top before current step
- `nav_test_expect_return: Option<usize>` — when set, next y must return
  to this value

## Integration

The timer callback calls navigation functions directly on AppState
(page_forward, page_backward, jump_to_next_scene, etc.) — same functions
that key dispatch calls. No synthetic key events. Tests real navigation
logic with real GTK layout.

Failures logged with `NAV_TEST: FAIL` prefix including step number, key,
invariant violated, and relevant values. Harness continues after failures.

## Files changed

- `src/input/actions/mod.rs` — add ToggleNavTest action
- `src/input/keymap_config.rs` — bind Ctrl+Shift+T to ToggleNavTest
- `src/input/keymap.rs` — dispatch ToggleNavTest
- `src/input/nav_test.rs` — new file: start/stop, step function, invariant
  checks, test sequence script
- `src/app.rs` — add nav_test fields to AppState
