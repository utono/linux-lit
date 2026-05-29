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
    SyncAdvance,
}

impl Step {
    fn delay_ms(self) -> u64 {
        match self {
            Step::SyncAdvance => 1000,
            _ => 300,
        }
    }
}

fn build_script() -> Vec<Step> {
    // jumps-only: test key-press navigation
    let mut s = Vec::new();
    s.extend_from_slice(&[Step::PageForward; 5]);
    s.extend_from_slice(&[Step::PageBackward; 5]);
    s.extend_from_slice(&[Step::PageForward; 3]);
    s.push(Step::NextScene); s.push(Step::PageBackward);
    s.push(Step::PrevScene); s.push(Step::PageBackward);
    s.extend_from_slice(&[Step::PageForward; 5]);
    s.push(Step::NextChapter); s.push(Step::PageBackward);
    s.push(Step::PrevChapter); s.push(Step::PageBackward);
    s
}

const MAX_STEPS: usize = 500;

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
    crate::log_fmt!("NAV_TEST: started at page_top={} current_line={}", s.page_top_line, s.current_line);

    let icon = s.debug_icon.clone();
    icon.set_label("NAV TEST: running…");
    icon.set_visible(true);

    drop(s);

    schedule_next(Rc::clone(state_rc));
}

fn schedule_next(state: Rc<RefCell<AppState>>) {
    let script = build_script();
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
    let script = build_script();
    let script_idx = s.nav_test_step % script.len();
    let step = script[script_idx];
    let step_num = s.nav_test_step;
    let pre_top = s.page_top_line;
    let pre_line = s.current_line;
    s.nav_test_prev_top = pre_top;

    // Record expected return for structural jumps
    match step {
        Step::NextScene | Step::PrevScene | Step::NextChapter | Step::PrevChapter => {
            s.nav_test_expect_return = Some(pre_top);
        }
        _ => {}
    }
    // Record expected return for x followed by y
    if matches!(step, Step::PageForward) {
        let next_idx = (step_num + 1) % script.len();
        if matches!(script[next_idx], Step::PageBackward) {
            s.nav_test_expect_return = Some(pre_top);
        }
    }

    // Execute
    match step {
        Step::PageForward => navigation::page_forward(s),
        Step::PageBackward => navigation::page_backward(s),
        Step::NextScene => navigation::jump_to_next_scene(s),
        Step::PrevScene => navigation::jump_to_prev_scene(s),
        Step::NextChapter => navigation::jump_to_next_chapter(s),
        Step::PrevChapter => navigation::jump_to_prev_chapter(s),
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
                fail(s, step_num, step, &format!(
                    "return mismatch: expected top={} got top={}",
                    expected, post_top
                ));
            }
        }
    }

    // 3. No scene break mid-page
    if s.current_work.is_some() {
        let last_vis = last_fully_visible_line(s, post_top);
        let mut scan = post_top + 1;
        while scan <= last_vis {
            let text = buffer_line_text(&s.buffer, scan);
            let t = text.trim();
            if line_types::is_act_scene_marker(&t)
                || line_types::is_separator(&t)
                || t.is_empty()
                || line_types::is_stage_direction(&t)
            {
                scan += 1;
            } else {
                break;
            }
        }
        for i in scan..=last_vis {
            let text = buffer_line_text(&s.buffer, i);
            let t = text.trim();
            if line_types::is_act_scene_marker(&t) || line_types::is_separator(&t) {
                fail(s, step_num, step, &format!(
                    "scene break at line {} ('{}') mid-page (top={} last={})",
                    i, t.chars().take(40).collect::<String>(), post_top, last_vis
                ));
                break;
            }
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

    // 5. current_line is dialogue (plays only)
    if s.current_work.as_ref().map(|w| w.work_type == "play").unwrap_or(false) {
        if post_line < line_count && !is_dialogue_line(&s.buffer, post_line) {
            let text = buffer_line_text(&s.buffer, post_line);
            fail(s, step_num, step, &format!(
                "current_line {} is not dialogue: '{}'",
                post_line, text.chars().take(60).collect::<String>()
            ));
        }
    }

    s.nav_test_step += 1;
}

fn fail(state: &mut AppState, step: usize, action: Step, msg: &str) {
    state.nav_test_failures += 1;
    crate::log_fmt!("NAV_TEST: FAIL step={} {:?} {}", step, action, msg);
}
