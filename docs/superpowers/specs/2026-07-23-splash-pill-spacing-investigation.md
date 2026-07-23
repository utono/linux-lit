# Launch-splash pill spacing: root-cause investigation

Date: 2026-07-23
Scope: root-cause only. No fix applied. A separate task applies the fix.
Branch: `worktree-agent-ac789ad4217d71566` (isolated worktree).

## The bug

At launch, for a brief window a small centered rounded "pill" shows the current
work's abbrev jammed against the chapter/position with no separator — the user's
screenshot captured `TTChapter 6` (work abbrev `TT` + `Chapter 6`, no gap). It
should read with a gap, e.g. `TT   Chapter 6`. It appears on the bare
background before the reading card is fully revealed, then disappears once the
card maps at full width.

## Confirmed root cause

**The pill is the running-head strip (`top_spacer`), not the `chapter_toast`.**
The `TT` text is `running_head_work` and the `Chapter 6` text is
`running_head_scene` — two separate labels appended to the same horizontal
`top_spacer` box. They are set as two independent strings by
`scene_synopsis::update_running_heads` (never concatenated):

- `src/app/scene_synopsis.rs:655` `state.running_head_work.set_text(&abbrev);`
- `src/app/scene_synopsis.rs:656` `state.running_head_scene.set_text(position);`

Widget setup (`src/app/mod.rs`):

- `1553-1557` `running_head_work`: `halign=Start`, `hexpand=true`, class
  `running-head-work`.
- `1559-1563` `running_head_scene`: `halign=End`, `hexpand=true`, class
  `running-head-scene`.
- `1565-1566` both appended to `top_spacer`.
- `top_spacer` (`1547-1551`) has classes `card-top` + `running-head`.

CSS (`src/theme.rs`):

- `1193` `.card-top { background-color: {bg}; border-radius: 12px 12px 0 0; }`
- `1194` `.running-head { padding: 0 40px; }`

So the strip is a rounded-top, background-filled box. **The ONLY thing that
pushes the two labels apart is `hexpand` spreading them across the card width.**
There is no minimum gap, margin, or spacer between them. Whenever the strip is
rendered at (or near) its natural content width — i.e. before the reading card
has been laid out to full width — `hexpand` has no slack to distribute, the two
labels sit immediately adjacent, and the whole `content_hbox` (which is
`halign=Center`) collapses to that natural width and sits centered on the bare
background. The `card-top` background + rounded corners make the collapsed strip
look like a small centered pill: exactly `TTChapter 6`.

### Mechanism — why it touches in the splash but not in the laid-out card

`content_hbox` is `halign=Center` with a `width_request` that is normally the
full card width (`src/app/mod.rs:1602` sets the build-time
`config.column_width`; `apply_card_sizing` in `src/app/layout.rs:392` later sets
`card_w`). At full width the two labels are ~900px apart (see evidence below).
In any window where the vbox becomes visible while `content_hbox` is at its
natural/collapsed width, the centered card shrinks to the strip's content width
and the labels touch.

`chapter_toast` is ruled out: it is a single centered `Label`
(`src/app/mod.rs:1886-1891`), and its `.chapter-toast` CSS currently has **no
background and no border-radius** (`src/theme.rs:1296-1297` — the pill styling is
commented out at `1291-1295`). It therefore cannot render as a rounded pill, and
nothing sets it to a combined abbrev+chapter string at startup (grepped:
`update_running_heads` / running-head labels are the only place `TT` and
`Chapter 6` are produced as adjacent text).

## Headless reproduction — NOT reproduced (code-traced instead)

Status on the collapsed-pill visual: **PARTIAL — not reproduced headless; root
cause established by code tracing + the laid-out-card screenshot evidence.**

I drove the real app under `cage` (software/pixman) with rapid `grim` capture
across the whole launch-to-reveal window, for both a play (`2H6-Arkangel`, the
current dev `last_work`) and a prose-with-position work (`TT`, temporarily set as
`last_work`, then restored). Frame stride ~24-30ms, spanning launch through
reveal. In every run the reveal was **atomic and correct** — the strip appeared
already spread full-width; no intermediate collapsed-pill frame was captured.

Evidence frames (scratchpad):
- `.../scratchpad/ttr-016.png` — last pre-reveal frame (TT): title bar + bare
  teal bg, no strip.
- `.../scratchpad/ttr-017.png` — first revealed frame (TT): full card, running
  head `TT` at far LEFT and `Front matter` at far RIGHT — correctly spread.
- `.../scratchpad/rev-073.png` — first revealed frame (2H6): running head
  `2H6-Arkangel` far LEFT, `Act 4, Scene 10` far RIGHT — correctly spread.
- `.../scratchpad/tt-loaded.png` — settled TT card, same correct spread.

Why headless did not catch the transient (reproduction caveats, not a
contradiction of the root cause):

1. The primary reveal path always sizes first. The deferred-layout-refresh
   reveal (`src/app/mod.rs:2512-2599`, `do_reveal` set at `2599`, applied at
   `2739-2741`) runs `apply_card_sizing` at `2524` BEFORE `set_opacity(1.0)`, so
   at reveal `content_hbox` already has the full `card_w`. Logged TT sequence
   (from `/home/mlj/utono/linux-lit/linux-lit-dev.log`):
   `CARD_SIZING ... card_w=1050 margin=24` at ~186/248/399ms, then
   `STARTUP: revealing vbox (sw_h=582)` at ~387-401ms.
2. The very first tick has `ww=0` ("vbox.width changed -1 -> 0"), but the guard
   `if ww <= 100 { return }` (`src/app/mod.rs:2495`) returns BEFORE
   `apply_card_sizing`, so `content_hbox` never gets `width_request=1` from
   `card_w = target.min(ww.max(1))` (`src/app/layout.rs:403`) at ww=0. It keeps
   its build-time 1050. Good — this is why the collapse did not occur in my runs.
3. `TT.text_file` is NULL (DB-join load), so TT never writes/reads a snapshot;
   the snapshot cache-hit path is not involved in the user's TT case.

The user does see the collapsed pill on their real GL renderer. The reachable
code path that produces it is the **500ms grace reveal**
(`src/app/mod.rs:2380-2400`): it calls `reveal_snap` (scroll/highlight only —
**does NOT call `apply_card_sizing`**) then `vbox.set_opacity(1.0)` when
`!loading && !refresh_pending`. On timing where this grace wins the race against
the resize-tick reveal — or where the strip is momentarily at natural width when
opacity flips to 1.0 — the labels render adjacent. Because the underlying defect
(no guaranteed gap between the labels) is width-dependent and renderer/timing
sensitive, it is intermittent and did not surface under cage's pixman timing.

## Recommended minimal fix (candidate (a))

**Give the two running-head labels a guaranteed horizontal gap that does not
depend on `hexpand` having slack** — add a right margin to `running_head_work`
(and/or a left margin to `running_head_scene`). Even when the strip collapses to
natural width, the labels then keep a visible gap and read `TT  Chapter 6`
instead of `TTChapter 6`.

Exact change (one or two lines), in `src/app/mod.rs`, in the widget setup block
`1553-1563`:

```rust
running_head_work.set_margin_end(16);   // add after line 1557
// optionally, symmetric:
running_head_scene.set_margin_start(16); // add after line 1563
```

A single `set_margin_end(16)` on `running_head_work` is sufficient to guarantee
the gap; adding the symmetric `margin_start` on `running_head_scene` keeps the
collapsed pill visually balanced but is optional.

### Why this is low-risk and does not harm the laid-out card

In the normal full-width card the two labels are ~900px apart (measured from the
reveal screenshots: `TT`/`2H6-Arkangel` sits at the far left inside the 40px
`.running-head` padding, the position label at the far right). A 16px margin is
absorbed entirely by the `hexpand` slack — it shifts the spread by at most 16px
out of ~900px and is not perceptible. `hexpand` distributes the remaining space,
so the labels stay pinned to their Start/End alignments; the only effect at full
width is a negligible reduction in the gap. The strip's `.running-head`
`padding: 0 40px` is unaffected (margin is between the two labels, inside the
padding).

### Fix location summary

- File: `src/app/mod.rs`
- Line 1557 (after `running_head_work.add_css_class("running-head-work");`):
  add `running_head_work.set_margin_end(16);`
- Optional, line 1563 (after
  `running_head_scene.add_css_class("running-head-scene");`): add
  `running_head_scene.set_margin_start(16);`

No overlay/legend/keymap surfaces are touched (this is a layout gap, not a
keybind), so no keybind-overlay mirroring applies.

### Alternatives considered (and why not preferred)

- **(b) Guarantee `content_hbox` has full `width_request` before any reveal path
  paints/reveals.** Would require making the 500ms grace path
  (`src/app/mod.rs:2380-2400`) call `apply_card_sizing` before `set_opacity(1.0)`
  (as the resize-tick path already does). More correct structurally but higher
  risk: it touches the startup reveal race that the surrounding comments warn is
  delicate (premature reveal shows a pre-layout spread that then re-flows). Fixes
  only the sizing race, not the fundamental "labels have no minimum gap"
  fragility.
- **(c) Don't paint the running head until after layout.** The strip lives inside
  `vbox` (opacity-gated), so it is already not painted until reveal; the problem
  is the width AT reveal, not that it paints too early. Would not address the
  collapsed-width case.
- **(d) Add an empty `hexpand` spacer box between the labels.** Equivalent effect
  to (a) but heavier (an extra widget) and still zero-width when there is no
  slack, so it would NOT guarantee a gap in the collapsed case. (a) with a fixed
  margin is strictly better.

(a) is the smallest change, fixes the actual defect (no minimum gap), and is
provably invisible in the laid-out card.

## Verification plan for the applier

- After adding the margin, re-run the cage splash-capture flow used here
  (launch under cage, rapid `grim`), but the collapsed pill is timing-dependent
  and may still not surface headless — so the stronger check is the code
  argument above plus a laid-out-card screenshot confirming the full-width spread
  is unchanged (`ttr-017.png`-style frame: `TT` far left, position far right).
- Definitive confirmation is on the user's real GL renderer at next launch: the
  brief pill, if it appears, now reads `TT  Chapter 6` with a gap.

## Tree state

Working tree is CLEAN. No source edits, no diagnostic logging added, no commits.
The only change outside the repo was a temporary edit to
`~/.config/linux-lit/config-dev.json` (`last_work` → `TT` for the prose repro),
which was restored to its original value (`2H6-Arkangel`) after the captures. No
cage instances left running (only the user's own `wayland-0` session remains).
Temporary screenshots live in the session scratchpad, not the repo.
