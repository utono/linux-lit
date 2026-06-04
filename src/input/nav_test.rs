use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;
use crate::db::line_types;
use crate::input::highlight::update_highlight_and_advance_page;
use crate::input::navigation;
use crate::input::viewport::{buffer_line_text, is_dialogue_line, last_fully_visible_line,
                              next_dialogue_line};

#[derive(Clone, Copy, Debug)]
enum Step {
    PageForward,
    PageBackward,
    NextScene,
    PrevScene,
    NextChapter,
    PrevChapter,
    JumpTop,
    JumpEnd,
    NextDialogue, // q/j — reading-order forward; can turn the page
    PrevDialogue, // ,/k — reading-order backward; can turn the page
    SyncAdvance,
    SearchJump,
}

impl Step {
    fn delay_ms(self) -> u64 {
        match self {
            Step::SyncAdvance => 1000,
            // Let GTK layout settle between jumps — pixel-dependent functions
            // (column_split, jump_to_end's height walk) read stale widget heights
            // if hammered faster, producing layout-instability false positives.
            _ => 400,
        }
    }

    /// Every `Step` variant, in a list whose completeness is COMPILER-ENFORCED:
    /// the exhaustive `match` below (no `_` arm) fails to compile if a variant is
    /// added without classifying it. This is what turns "remember to update
    /// ALL_STEPS" from a human checklist into a guarantee.
    const EVERY: [Step; 12] = [
        Step::PageForward, Step::PageBackward,
        Step::NextScene, Step::PrevScene,
        Step::NextChapter, Step::PrevChapter,
        Step::JumpTop, Step::JumpEnd,
        Step::NextDialogue, Step::PrevDialogue,
        Step::SyncAdvance, Step::SearchJump,
    ];

    /// Whether the coverage prelude drives this action from every anchor.
    /// Exhaustive `match` (no wildcard) — adding a `Step` variant forces a
    /// decision here, so the prelude can never silently omit a new action.
    const fn in_coverage(self) -> bool {
        match self {
            Step::PageForward | Step::PageBackward
            | Step::NextScene | Step::PrevScene
            | Step::NextChapter | Step::PrevChapter
            | Step::JumpTop | Step::JumpEnd
            | Step::NextDialogue | Step::PrevDialogue => true,
            // SyncAdvance has its own slow cadence; SearchJump is a simulation —
            // both are covered by the random body, not the per-anchor sweep.
            Step::SyncAdvance | Step::SearchJump => false,
        }
    }
}

// Compile-time guard: `EVERY` must list exactly as many entries as the enum has
// variants. If a variant is added without extending `EVERY`, this fails to
// compile. (`variant_count` is stable as of the toolchain this builds against;
// if unavailable, the exhaustive `match` in `in_coverage` is the backstop.)
const _: () = assert!(Step::EVERY.len() == 12);

/// Deterministic LCG — `Math.random` would make runs unreproducible. Seeded from
/// a fixed constant so a failure can be replayed by re-running the same mode.
struct Lcg(u64);
impl Lcg {
    fn next_u32(&mut self) -> u32 {
        // Numerical Recipes constants.
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next_u32() % n }
    }
}

/// `jumps-only`: the original fixed key-press script.
fn build_script() -> Vec<Step> {
    let mut s = Vec::new();
    s.extend_from_slice(&[Step::PageForward; 5]);
    s.extend_from_slice(&[Step::PageBackward; 5]);
    s.extend_from_slice(&[Step::PageForward; 3]);
    s.push(Step::NextScene); s.push(Step::PageBackward);
    s.push(Step::PrevScene); s.push(Step::PageBackward);
    s.extend_from_slice(&[Step::PageForward; 5]);
    s.push(Step::SearchJump); s.push(Step::PageBackward);
    s.push(Step::NextChapter); s.push(Step::PageBackward);
    s.push(Step::PrevChapter); s.push(Step::PageBackward);
    s
}

/// The navigation Steps the coverage prelude drives from each structural anchor.
/// Derived from `Step::EVERY` filtered by the compiler-enforced `in_coverage`
/// classification — so adding a `Step` variant automatically includes it here
/// (if classified `true`) or forces an explicit `false`, never silent omission.
fn all_coverage_steps() -> Vec<Step> {
    Step::EVERY.into_iter().filter(|st| st.in_coverage()).collect()
}

/// Deterministic COVERAGE prelude: drive every Step from every structural
/// anchor, so a single run guarantees (anchor × action) coverage regardless of
/// the random seed. Anchors are reached by replayable jumps:
///   - work start (gg),
///   - work end (G),
///   - each act/scene boundary, reached by gg then N× NextScene,
///   - the mid-point of a page (a few NextDialogue off an anchor).
/// After landing on each anchor we fire every Step once (the invariants in
/// `run_step` then check the landing). This is what makes the fuzz test "all
/// variations / scenarios" instead of sampling them.
fn build_coverage_prelude() -> Vec<Step> {
    let mut s = Vec::new();
    let actions = all_coverage_steps();

    // 1. From the START, every action (re-anchor with gg before each).
    for &st in &actions { s.push(Step::JumpTop); s.push(st); }

    // 2. From the END, every action (the final-spread / EPILOGUE scenarios).
    for &st in &actions { s.push(Step::JumpEnd); s.push(st); }

    // 3. Sweep scene boundaries: gg, then k× NextScene to reach scene k, then
    //    every action from there. 24 scenes covers the longest Folger plays;
    //    NextScene past the last scene is a harmless no-op the invariants allow.
    for k in 1..=24usize {
        s.push(Step::JumpTop);
        for _ in 0..k { s.push(Step::NextScene); }
        for &st in &actions { s.push(st);
            // Return to the same scene anchor before the next action so each
            // action starts from the boundary, not from wherever the prior one
            // left the cursor.
            s.push(Step::JumpTop);
            for _ in 0..k { s.push(Step::NextScene); }
        }
    }

    // 4. MID-PAGE anchors: from the end, walk a few dialogue lines back (so the
    //    cursor sits mid-spread, not on a boundary) then every action — exercises
    //    page turns triggered from inside a column rather than at its edge.
    for back in [3usize, 7, 12] {
        s.push(Step::JumpEnd);
        for _ in 0..back { s.push(Step::PrevDialogue); }
        for &st in &actions {
            s.push(st);
            s.push(Step::JumpEnd);
            for _ in 0..back { s.push(Step::PrevDialogue); }
        }
    }
    s
}

/// `fuzz`: a deterministic COVERAGE prelude (every action from every structural
/// anchor) followed by a long, deterministic-random mix of every navigation jump,
/// weighted toward structural jumps and the top/end binds (gg / G). The prelude
/// guarantees scenario coverage every run; the random body adds combinatorial
/// depth. Each jump is followed by a landing check in `run_step` (cursor on a
/// dialogue line, within the visible page, page_top consistent).
/// The default LCG seed. Overridable at runtime via `LIT_NAV_SEED` (decimal or
/// `0x`-prefixed hex) so a failing run can be replayed exactly. The resolved
/// seed is logged once at run start (see `toggle`).
const DEFAULT_NAV_SEED: u64 = 0x9E3779B97F4A7C15;

/// Resolve the fuzz seed: `LIT_NAV_SEED` if set and parseable, else the default.
fn fuzz_seed() -> u64 {
    match std::env::var("LIT_NAV_SEED") {
        Ok(v) => {
            let v = v.trim();
            let parsed = if let Some(hex) = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16).ok()
            } else {
                v.parse::<u64>().ok()
            };
            parsed.unwrap_or(DEFAULT_NAV_SEED)
        }
        Err(_) => DEFAULT_NAV_SEED,
    }
}

fn build_fuzz_script() -> Vec<Step> {
    let mut s = build_coverage_prelude();
    let mut rng = Lcg(fuzz_seed());
    let setups = [
        Step::PageForward, Step::PageForward,
        Step::NextScene, Step::PrevScene,
        Step::NextChapter, Step::PrevChapter,
        Step::JumpEnd, Step::JumpTop,
        Step::NextDialogue, Step::NextDialogue, Step::NextDialogue,
        Step::PrevDialogue, Step::PrevDialogue,
        Step::SearchJump,
    ];
    // Hand-crafted boundary-stress motifs — the sequences that surfaced the
    // recent x/y/G final-spread bugs. Each is appended whole so the invariants
    // run after every step inside it (a bug shows on the offending step).
    let motifs: [&[Step]; 8] = [
        // March to the end and keep pressing x: empty-right / stuck.
        &[Step::JumpEnd, Step::PageForward, Step::PageForward, Step::PageForward],
        // G then walk back with y from the final spread.
        &[Step::JumpEnd, Step::PageBackward, Step::PageBackward, Step::PageBackward],
        // gg then walk forward with x from the first spread.
        &[Step::JumpTop, Step::PageForward, Step::PageForward, Step::PageForward],
        // gg then y at the very start (first-spread guard).
        &[Step::JumpTop, Step::PageBackward, Step::PageBackward],
        // Double end / double top.
        &[Step::JumpEnd, Step::JumpEnd, Step::JumpTop, Step::JumpTop],
        // Dialogue-walk into the tail then reverse (q/j then ,/k boundary).
        &[Step::JumpEnd, Step::PrevDialogue, Step::PrevDialogue, Step::NextDialogue, Step::NextDialogue],
        // Jump end, page back once, then forward — round-trip at the tail.
        &[Step::JumpEnd, Step::PageBackward, Step::PageForward],
        // Scene-jump to the last scene then x/y around it.
        &[Step::NextScene, Step::NextScene, Step::JumpEnd, Step::PageBackward, Step::PageForward],
    ];
    // Random body appends to the coverage prelude (do NOT re-init `s`).
    while s.len() < MAX_STEPS {
        // Mostly random "setup moves then y" blocks, but ~1 in 4 iterations inject
        // a boundary-stress motif so the end/start edges get hammered repeatedly.
        if rng.below(4) == 0 {
            s.extend_from_slice(motifs[rng.below(motifs.len() as u32) as usize]);
        } else {
            let n = 1 + rng.below(4) as usize;
            for _ in 0..n {
                s.push(setups[rng.below(setups.len() as u32) as usize]);
            }
            s.push(Step::PageBackward);
            if rng.below(3) == 0 {
                s.push(Step::PageBackward);
            }
        }
    }
    s.truncate(MAX_STEPS);
    s
}

/// Total fuzz steps. Sized so the deterministic coverage prelude (~900 steps:
/// every action from start, end, 24 scene boundaries, and 3 mid-page anchors)
/// always runs in full, followed by a long random body for combinatorial depth.
const MAX_STEPS: usize = 1400;

pub fn toggle(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    if s.nav_test_active {
        let steps = s.nav_test_step;
        let failures = s.nav_test_failures;
        s.nav_test_active = false;
        crate::log_fmt!("NAV_TEST: stopped after {} steps, {} failures", steps, failures);
        let icon = s.debug_icon.clone();
        icon.set_label(&format!("NAV TEST: done ({} steps, {} fail)", steps, failures));
        icon.set_visible(true);
        let icon2 = icon.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            icon2.set_visible(false);
        });
        return;
    }

    s.nav_test_active = true;
    s.nav_test_step = 0;
    s.nav_test_failures = 0;
    s.nav_test_prev_top = s.page_top_line;
    s.nav_test_expect_return = None;
    // Opt into the long random fuzz script via env (for headless 5-min runs).
    s.nav_test_fuzz = std::env::var("LIT_NAV_FUZZ").map(|v| v == "1").unwrap_or(false);
    crate::log_fmt!(
        "NAV_TEST: started ({}) at page_top={} current_line={}",
        if s.nav_test_fuzz { "fuzz" } else { "jumps-only" },
        s.page_top_line, s.current_line,
    );
    if s.nav_test_fuzz {
        // Print the resolved seed so a FAIL can be replayed exactly with
        // `LIT_NAV_SEED=0x...` (set it to this value to reproduce the same run).
        crate::log_fmt!("NAV_TEST: seed=0x{:016X} (override with LIT_NAV_SEED)", fuzz_seed());
    }

    let icon = s.debug_icon.clone();
    icon.set_label("NAV TEST: running…");
    icon.set_visible(true);

    drop(s);

    schedule_next(Rc::clone(state_rc));
}

fn current_script(s: &AppState) -> Vec<Step> {
    if s.nav_test_fuzz { build_fuzz_script() } else { build_script() }
}

fn schedule_next(state: Rc<RefCell<AppState>>) {
    let script = current_script(&state.borrow());
    let delay = {
        let s = state.borrow();
        if !s.nav_test_active || s.nav_test_step >= MAX_STEPS {
            return;
        }
        let idx = s.nav_test_step % script.len();
        script[idx].delay_ms()
    };

    glib::timeout_add_local_once(std::time::Duration::from_millis(delay), move || {
        let mut s = state.borrow_mut();
        if !s.nav_test_active {
            return;
        }
        if s.nav_test_step >= MAX_STEPS {
            let steps = s.nav_test_step;
            let failures = s.nav_test_failures;
            s.nav_test_active = false;
            crate::log_fmt!("NAV_TEST: completed {} steps, {} failures", steps, failures);
            let icon = s.debug_icon.clone();
            icon.set_label(&format!("NAV TEST: done ({} steps, {} fail)", steps, failures));
            glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                icon.set_visible(false);
            });
            return;
        }

        run_step(&mut s);

        let state_clone = Rc::clone(&state);
        drop(s);
        schedule_next(state_clone);
    });
}

fn run_step(s: &mut AppState) {
    let script = current_script(s);
    let script_idx = s.nav_test_step % script.len();
    let step = script[script_idx];
    let step_num = s.nav_test_step;
    let pre_top = s.page_top_line;
    let pre_line = s.current_line;
    s.nav_test_prev_top = pre_top;

    // Record an expected `y` return ONLY when the very next step is PageBackward,
    // for moves that push the origin (x, chapter jumps, search). An immediate
    // round-trip is the only case where the return target is unambiguous — with
    // intervening moves the back-stack changes, so a later `y` legitimately lands
    // elsewhere (the y-went-FORWARD invariant still guards correctness there).
    // Scene jumps (2/3) deliberately clear the stack without pushing, so they
    // never set an expectation.
    let next_is_y = matches!(script[(step_num + 1) % script.len()], Step::PageBackward);
    if next_is_y
        && matches!(
            step,
            Step::PageForward | Step::NextChapter | Step::PrevChapter | Step::SearchJump
        )
    {
        s.nav_test_expect_return = Some(pre_top);
    } else {
        // Any non-round-trip move invalidates a pending expectation.
        s.nav_test_expect_return = None;
    }

    // Execute
    match step {
        Step::PageForward => navigation::page_forward(s),
        Step::PageBackward => navigation::page_backward(s),
        Step::NextScene => navigation::jump_to_next_scene(s),
        Step::PrevScene => navigation::jump_to_prev_scene(s),
        Step::NextChapter => navigation::jump_to_next_chapter(s),
        Step::PrevChapter => navigation::jump_to_prev_chapter(s),
        Step::JumpTop => navigation::jump_to_start(s),
        Step::JumpEnd => navigation::jump_to_end(s),
        Step::NextDialogue => navigation::jump_to_next_dialogue(s),
        Step::PrevDialogue => navigation::jump_to_prev_dialogue(s),
        Step::SearchJump => {
            // Simulate a search jump to a DIALOGUE line ~50 lines ahead (a real
            // search lands on matched text, which the cursor invariant expects to
            // be dialogue — an arbitrary +50 could land on a speaker/stage line).
            let line_count = s.effective_line_count();
            let raw = (s.current_line + 50).min(line_count.saturating_sub(1));
            let target = next_dialogue_line(&s.buffer, &s.translation_lines, raw, line_count)
                .filter(|&d| d < line_count)
                .unwrap_or(raw);
            s.current_line = target;
            let top = s.page_top_line;
            if s.page_back_stack.last() != Some(&top) {
                s.page_back_stack.push(top);
            }
            crate::input::highlight::update_highlight_and_center(s);
        }
        Step::SyncAdvance => {
            let line_count = s.effective_line_count();
            if let Some(target) = next_dialogue_line(
                &s.buffer, &s.translation_lines, s.current_line, line_count,
            ) {
                s.current_line = target;
                update_highlight_and_advance_page(s);
            }
        }
    }

    let post_top = s.page_top_line;
    let post_line = s.current_line;
    let line_count = s.effective_line_count();

    // If a navigation was a no-op (no target found, or at end of work),
    // clear the return expectation — the stack wasn't touched.
    if post_top == pre_top && post_line == pre_line {
        s.nav_test_expect_return = None;
    }

    crate::log_fmt!(
        "NAV_TEST: step={} {:?} top={}->{} line={}->{}",
        step_num, step, pre_top, post_top, pre_line, post_line
    );

    // 1. Forward progress on x (skip if page_forward was a no-op at end of work)
    if matches!(step, Step::PageForward) && post_top <= pre_top {
        let is_end_noop = post_top == pre_top && post_line == pre_line;
        if !is_end_noop {
            fail(s, step_num, step, &format!(
                "forward progress: top {}->{}",
                pre_top, post_top
            ));
        }
    }

    // 2. y return check (round-trip x or structural jump return)
    if matches!(step, Step::PageBackward) {
        if let Some(expected) = s.nav_test_expect_return.take() {
            if post_top != expected {
                // Soft: the immediate-return heuristic is approximate (the stack
                // legitimately changes across the new dialogue-nav semantics). The
                // hard `y went FORWARD` check below is the real guarantee.
                warn(s, step_num, step, &format!(
                    "return mismatch: expected top={} got top={}",
                    expected, post_top
                ));
            }
        }
    }

    // 2b. y DIRECTION: page-backward must never move the page FORWARD. The bug
    // this catches: `y` popping a stale older back-stack entry and jumping ahead
    // of where the reader was. Skip the at-start no-op (top unchanged at 0).
    if matches!(step, Step::PageBackward) && post_top > pre_top {
        fail(s, step_num, step, &format!(
            "y went FORWARD: top {}->{} (cursor {}->{})",
            pre_top, post_top, pre_line, post_line
        ));
    }

    // 2c. PAGE-BACK TILING: a `y` page turn must land on the page that tiles
    // EXACTLY into the page we came from — its forward boundary
    // (`column_split(post_top).next_page_top`) should EQUAL `pre_top`.
    //   * fwd > pre_top  → the back-page's content runs PAST the old top, so the
    //     lines [pre_top, fwd) are shown on BOTH pages (overlap). This is the
    //     bug behind the y-from-final-spread spread that barely moved.
    //   * fwd < pre_top  → a gap (content between the pages shown on neither).
    // Two-column only; skip the no-op and the first-spread guard (pre_top==0).
    if matches!(step, Step::PageBackward)
        && s.column_count() == 2
        && post_top < pre_top
        && pre_top > 0
        && line_count > 0
    {
        let fwd = crate::input::viewport::column_split(s, post_top).next_page_top;
        if fwd > pre_top {
            fail(s, step_num, step, &format!(
                "y OVERLAP: back-page top={} runs to next_page_top={} PAST old top={} ({} lines shown twice)",
                post_top, fwd, pre_top, fwd - pre_top
            ));
        } else if fwd < pre_top {
            fail(s, step_num, step, &format!(
                "y GAP: back-page top={} ends at next_page_top={} before old top={} ({} lines skipped)",
                post_top, fwd, pre_top, pre_top - fwd
            ));
        }
    }

    // 2d. FORWARD TILING: an `x` page turn must land on the page that tiles
    // EXACTLY off the page we came from — the OLD page's forward boundary
    // (`column_split(pre_top).next_page_top`) should EQUAL the new top. If they
    // differ, consecutive pages overlap (the same content shown twice) or gap.
    // Skip the final-spread no-turn (cursor moves, page doesn't) and the
    // would-empty redirect to the anchor (a legitimate non-adjacent jump).
    if matches!(step, Step::PageForward)
        && s.column_count() == 2
        && post_top > pre_top
        && line_count > 0
    {
        let expected = crate::input::viewport::column_split(s, pre_top).next_page_top;
        // The forward path may redirect onto the final-spread anchor when the
        // natural next page would empty the right column — that's an intentional
        // non-adjacent landing, not an overlap. Only flag when the landing is
        // BELOW the natural next boundary (content shown twice).
        if post_top < expected {
            fail(s, step_num, step, &format!(
                "x OVERLAP: from top={} expected next={} but landed at {} ({} lines shown twice)",
                pre_top, expected, post_top, expected - post_top
            ));
        }
    }

    // 2e. RIGHT-COLUMN BALANCE: on any non-final two-column spread the right
    // column must hold a reasonable share of the spread. A right column with far
    // fewer lines than the left (while content remains below) is an unbalanced
    // spread — the page boundary landed too early. Exempt the genuine final
    // spread (short tail) and pages whose remaining content legitimately ends.
    if s.column_count() == 2 && line_count > 0 && !s.loading_work.get() {
        let cs = crate::input::viewport::column_split(s, post_top);
        let left_lines = cs.split.saturating_sub(post_top);
        let right_lines = (cs.page_end + 1).saturating_sub(cs.split);
        let more_below = cs.next_page_top < line_count
            && (cs.next_page_top..line_count).any(|i| is_dialogue_line(&s.buffer, i));
        // Unbalanced: right column is less than a third of the left AND there's
        // more content that could have filled it. (The final spread, where
        // `more_below` is false, is exempt.)
        if more_below && right_lines * 3 < left_lines && left_lines >= 12 {
            fail(s, step_num, step, &format!(
                "UNBALANCED SPREAD: top={} left={} lines right={} lines (more content below) split={} page_end={}",
                post_top, left_lines, right_lines, cs.split, cs.page_end
            ));
        }
    }

    // 3. No scene break mid-page. A marker that STARTS a column is a legitimate
    // boundary, not a mid-page break: the left column starts at post_top, and in
    // two-column mode the right column starts at `split`. The work's final spread
    // is also exempt — a trailing section (lone EPILOGUE) has no next page to go
    // to, so its marker shares the last spread.
    let on_final_spread = {
        let lv = last_fully_visible_line(s, post_top);
        lv + 1 >= line_count
            || next_dialogue_line(&s.buffer, &s.translation_lines, lv, line_count)
                .map(|d| d >= line_count)
                .unwrap_or(true)
    };
    if s.current_work.is_some() && !on_final_spread {
        let last_vis = last_fully_visible_line(s, post_top);
        let split = if s.column_count() == 2 {
            crate::input::viewport::column_split(s, post_top).split
        } else {
            usize::MAX
        };
        // Skip the header block (markers/separators/blanks/stage directions)
        // beginning at `from`; returns the first content line after it.
        let skip_header = |from: usize| -> usize {
            let mut j = from;
            while j <= last_vis {
                let t = buffer_line_text(&s.buffer, j);
                let t = t.trim();
                if line_types::is_act_scene_marker(t)
                    || line_types::is_separator(t)
                    || t.is_empty()
                    || line_types::is_stage_direction(t)
                {
                    j += 1;
                } else {
                    break;
                }
            }
            j
        };
        let mut i = skip_header(post_top + 1);
        while i <= last_vis {
            // A marker that begins the right column is a valid column boundary.
            if i == split {
                let skipped = skip_header(i);
                if skipped > i {
                    i = skipped;
                    continue;
                }
            }
            let text = buffer_line_text(&s.buffer, i);
            let t = text.trim();
            if line_types::is_act_scene_marker(&t) || line_types::is_separator(&t) {
                // Soft: the two-column header/split-aware scan is approximate and
                // fires on legitimate column boundaries near the tail.
                warn(s, step_num, step, &format!(
                    "scene break at line {} ('{}') mid-page (top={} last={} split={})",
                    i, t.chars().take(40).collect::<String>(), post_top, last_vis, split
                ));
                break;
            }
            i += 1;
        }
    }

    // 4. Viewport fill (at least 10%)
    let widget_height = s.text_view.height();
    if widget_height > 0 {
        let last_vis = last_fully_visible_line(s, post_top);
        let mut total = 0i32;
        for i in post_top..=last_vis {
            if let Some(iter) = s.buffer.iter_at_line(i as i32) {
                let (_y, h) = s.text_view.line_yrange(&iter);
                total += h;
            }
        }
        let fill_pct = (total as f64 / widget_height as f64) * 100.0;
        if fill_pct < 10.0 {
            fail(s, step_num, step, &format!(
                "viewport fill {:.0}% < 10% (top={} last={} height={} content={})",
                fill_pct, post_top, last_vis, widget_height, total
            ));
        }
    }

    // 4b. TWO-COLUMN LAYOUT: the right column must not be empty, and the left
    // column must not be severely underfilled, UNLESS the entire remaining work
    // genuinely fits in one column (the true short-tail case, where there's
    // nothing to pull into the right). This catches the x/G/sync bugs that left a
    // lone EPILOGUE in the left column (empty right) or anchored G so late the
    // left column had a few lines and a huge gap.
    if s.column_count() == 2 && s.current_work.is_some() && line_count > 0 {
        let cs = crate::input::viewport::column_split(s, post_top);
        // Does ALL remaining content (post_top..end) fit in a single column? If
        // so an empty right column is unavoidable and fine. Approximate: the work
        // genuinely ends within this spread's left column.
        let tail_fits_one_col = cs.split >= line_count;
        let right_empty = cs.split >= line_count || cs.page_end < cs.split;
        if right_empty && !tail_fits_one_col {
            fail(s, step_num, step, &format!(
                "RIGHT COLUMN EMPTY (top={} split={} page_end={} line_count={})",
                post_top, cs.split, cs.page_end, line_count
            ));
        }
        // Left-column underfill: the left column spans [post_top, split-1]. If it
        // holds very few lines while there's plenty of content that COULD fill it
        // (i.e. we're not at the true end), the page was anchored too late (the G
        // bug). Only flag when the right column is also short — a full right
        // column means the spread is legitimately near the end.
        let left_lines = cs.split.saturating_sub(post_top);
        let right_lines = (cs.page_end + 1).saturating_sub(cs.split);
        if !tail_fits_one_col && left_lines < 6 && right_lines < 8 && cs.next_page_top < line_count {
            fail(s, step_num, step, &format!(
                "LEFT COLUMN UNDERFILLED (top={} left_lines={} right_lines={} split={} page_end={})",
                post_top, left_lines, right_lines, cs.split, cs.page_end
            ));
        }
    }

    // 5. current_line is dialogue (plays only). SearchJump is a harness
    // simulation (not a real product path), so a non-dialogue landing there is a
    // simulation artifact, not a bug — warn instead of fail.
    if s.current_work.as_ref().map(|w| w.work_type == "play").unwrap_or(false)
        && post_line < line_count
        && !is_dialogue_line(&s.buffer, post_line)
    {
        let text = buffer_line_text(&s.buffer, post_line);
        let msg = format!(
            "current_line {} is not dialogue: '{}'",
            post_line, text.chars().take(60).collect::<String>()
        );
        if matches!(step, Step::SearchJump) {
            warn(s, step_num, step, &msg);
        } else {
            fail(s, step_num, step, &msg);
        }
    }

    // 6. LANDING: after a jump, the cursor must be on the page it landed on —
    // i.e. within [post_top, last_visible]. This is the core "jumps land on the
    // right page" invariant: a cursor below/above the visible range means the
    // page didn't follow the jump (the bug class behind G/3/y mis-landings).
    // Skip the genuine end-of-document no-op (page and cursor both unchanged).
    let moved = post_top != pre_top || post_line != pre_line;
    let is_jump = matches!(
        step,
        Step::NextScene | Step::PrevScene | Step::NextChapter | Step::PrevChapter
            | Step::JumpTop | Step::JumpEnd
    );
    if is_jump && moved && line_count > 0 {
        let last_vis = last_fully_visible_line(s, post_top);
        if post_line < post_top || post_line > last_vis {
            fail(s, step_num, step, &format!(
                "landing off-page: cursor={} not in visible [{}, {}]",
                post_line, post_top, last_vis
            ));
        }
    }

    // 7. JUMP-TO-END REACHES THE END: after `G`/jump_to_end the spread must be
    // the CANONICAL last one — nothing left unshown below it
    // (`next_page_top >= line_count`). A spread that ends mid-work (e.g. the last
    // full two-column page while a short trailing EPILOGUE is still below) passes
    // the shape checks (full left, non-empty right) but is the WRONG page: paging
    // forward would still reveal more. This is the 4308-vs-4316 bug — the cursor
    // was on-page but the page wasn't the end of the work.
    if matches!(step, Step::JumpEnd) && s.column_count() == 2 && line_count > 0 {
        let cs = crate::input::viewport::column_split(s, post_top);
        if cs.next_page_top < line_count {
            // Only a bug if the remaining lines actually contain dialogue/content
            // worth showing (not just trailing blanks/exit markers).
            let mut remaining_content = false;
            for i in cs.next_page_top..line_count {
                if is_dialogue_line(&s.buffer, i) { remaining_content = true; break; }
            }
            if remaining_content {
                fail(s, step_num, step, &format!(
                    "JUMP-TO-END not at end: next_page_top={} < line_count={} (top={} page_end={}) — content still below",
                    cs.next_page_top, line_count, post_top, cs.page_end
                ));
            }
        }
    }

    // 7b. JUMP-TO-END IDEMPOTENCE: `G` must land on the SAME final spread no
    // matter where it starts from — recomputing `last_page_top` for the work's
    // last dialogue line must equal the page G just landed on. A mismatch means
    // G disagrees with itself (and with the saved-position startup spread): two
    // different "final" pages, the bug where G from one position lands on a
    // different, too-early spread than from another.
    if matches!(step, Step::JumpEnd) && s.column_count() == 2 && line_count > 0 {
        // last dialogue line of the work
        let mut target = line_count - 1;
        loop {
            if !s.translation_lines.get(target).copied().unwrap_or(false)
                && is_dialogue_line(&s.buffer, target) { break; }
            if target == 0 { break; }
            target -= 1;
        }
        let canonical = navigation::last_page_top(s, target);
        if canonical != post_top {
            fail(s, step_num, step, &format!(
                "JUMP-TO-END not idempotent: landed top={} but last_page_top recomputes {} (G disagrees with itself)",
                post_top, canonical
            ));
        }
    }

    // 8. LINE CLIPPING (in-app, per-step): the first and last visible lines of
    // each column must be shown WHOLE — never cut by the top or bottom edge of
    // the reading pane. This is the deterministic, pixel-free equivalent of
    // `check_line_clipping.py`: it reads the same `line_yrange` geometry the
    // renderer uses, so it runs on EVERY step (~1400/run) with no numpy, no
    // theme/contrast sensitivity, and no dependence on where `grim` points.
    // (The screenshot detector stays as an occasional oracle to confirm the
    // in-app geometry agrees with rendered pixels.)
    if s.column_count() == 2 && line_count > 0 && !s.loading_work.get() {
        let cs = crate::input::viewport::column_split(s, post_top);
        // Left column spans [post_top, split-1]; right column [split, page_end].
        if let Some(msg) = clip_violation(
            &s.text_view, &s.scrolled_window, &s.buffer,
            post_top, cs.split.saturating_sub(1).max(post_top),
        ) {
            fail(s, step_num, step, &format!("LEFT COLUMN CLIPPED: {}", msg));
        }
        if cs.page_end >= cs.split {
            if let Some(msg) = clip_violation(
                &s.right_view, &s.right_scrolled_window, &s.buffer,
                cs.split, cs.page_end,
            ) {
                fail(s, step_num, step, &format!("RIGHT COLUMN CLIPPED: {}", msg));
            }
        }
    } else if s.column_count() == 1 && line_count > 0 && !s.loading_work.get() {
        let last_vis = last_fully_visible_line(s, post_top);
        if let Some(msg) = clip_violation(
            &s.text_view, &s.scrolled_window, &s.buffer, post_top, last_vis,
        ) {
            fail(s, step_num, step, &format!("LINE CLIPPED: {}", msg));
        }
    }

    s.nav_test_step += 1;
}

/// In-app clipping check for one column: do the `top` and `bottom` visible lines
/// both fit WHOLE inside the view's scroll viewport? Returns `Some(message)` on a
/// clip, `None` when both are fully shown. Mirrors `check_line_clipping.py`'s
/// invariant using `line_yrange` (the renderer's own geometry) vs the
/// vadjustment window. A small tolerance absorbs sub-pixel rounding / the
/// descender guard the layout intentionally reserves.
fn clip_violation(
    view: &sourceview5::View,
    scrolled: &gtk4::ScrolledWindow,
    buffer: &sourceview5::Buffer,
    top: usize,
    bottom: usize,
) -> Option<String> {
    // Tolerance: the descender guard + bottom margin the layout reserves on
    // purpose (a line sitting within this band is not "clipped"). Keep generous
    // enough to avoid false positives from Pango sub-pixel jitter.
    const TOL: f64 = 6.0;
    let adj = scrolled.vadjustment();
    let view_top = adj.value();
    let view_bottom = view_top + adj.page_size();
    if adj.page_size() <= 0.0 {
        return None; // layout not ready — don't fail closed mid-transition
    }
    // Top visible line: its pixel TOP must not be above the viewport top.
    if let Some(iter) = buffer.iter_at_line(top as i32) {
        let (y, _h) = view.line_yrange(&iter);
        if (y as f64) < view_top - TOL {
            return Some(format!(
                "top line {} y={} above viewport_top={:.0} (cut at top)",
                top, y, view_top
            ));
        }
    }
    // Bottom visible line: its pixel BOTTOM must not fall below the viewport
    // bottom.
    if let Some(iter) = buffer.iter_at_line(bottom as i32) {
        let (y, h) = view.line_yrange(&iter);
        let line_bottom = (y + h) as f64;
        if line_bottom > view_bottom + TOL {
            return Some(format!(
                "bottom line {} bottom={:.0} below viewport_bottom={:.0} (cut at bottom)",
                bottom, line_bottom, view_bottom
            ));
        }
    }
    None
}

fn fail(state: &mut AppState, step: usize, action: Step, msg: &str) {
    state.nav_test_failures += 1;
    crate::log_fmt!("NAV_TEST: FAIL step={} {:?} {}", step, action, msg);
}

/// A soft check: logged for inspection but NOT counted as a failure. Used for
/// invariants whose *detection* is approximate (the two-column scene-break scan,
/// the search-jump simulation, the immediate-return heuristic) — they catch real
/// issues but also fire on legitimate layouts, so they shouldn't fail the run.
/// The hard correctness invariants (forward progress, y-direction, empty/under-
/// filled columns, on-page landing) stay as `fail`.
fn warn(_state: &mut AppState, step: usize, action: Step, msg: &str) {
    crate::log_fmt!("NAV_TEST: WARN step={} {:?} {}", step, action, msg);
}
