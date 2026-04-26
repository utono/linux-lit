# Re-adding `PAGE_FWD:` Debug Logging

## When This Document Applies

Pagination feels off — j/k turns to the wrong page, lands mid-paragraph, skips
a line of dialogue, or stops advancing. You suspect the issue is in
`page_forward` itself (descender-guard math, dialogue detection,
speaker-backup, last-visible-line calculation) and you want a per-keypress
trace of the page-boundary computation.

The detailed `PAGE_FWD:` log block was removed once pagination stabilised.
This doc explains how to put it back exactly as it was.

## Where the Log Block Lived

`src/input/navigation.rs`, inside `pub fn page_forward`, between the
"compute the next page" step and the "commit to state" step. It logged:

- `page_top`, `last_visible`, `last_dialogue`, `next` line indices
- `widget_height`, `descender_guard`, computed usable height
- The text content at `last_visible`, `last`, and `next` (60 chars)
- Per-line heights for the 5 lines around the page boundary

These five log lines per j-press show up in `~/utono/linux-lit/linux-lit-dev.log`.

## Re-adding the Block

`page_forward` currently looks like this (post-cleanup):

```rust
pub fn page_forward(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }

    let NextPage { new_top, next_dialogue } = next_page_top(state, state.page_top_line);
    if next_dialogue >= line_count {
        return; // already at end
    }

    // Remember current page so page_backward can return to it exactly
    state.page_history.push(state.page_top_line);

    state.current_line = next_dialogue;
    update_highlight(state);
    seek_to_current_line(state);
    set_page(state, new_top, PageDirection::Forward);
    auto_show_vocab_popup(state);
}
```

To restore the diagnostic, replace the body above with:

```rust
pub fn page_forward(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }

    // Recompute the inline boundary values used only by the debug-log block.
    // These match what `next_page_top` produces internally — keep them in
    // sync if you edit either path.
    let last_visible = last_fully_visible_line(state, state.page_top_line);
    let last = last_dialogue_in_page(
        &state.buffer,
        state.page_top_line,
        last_visible.saturating_sub(state.page_top_line) + 1,
        line_count,
    );
    let next = next_dialogue_from(&state.buffer, last + 1, line_count);

    let NextPage { new_top, next_dialogue } = next_page_top(state, state.page_top_line);

    {
        let lv_text = buffer_line_text(&state.buffer, last_visible);
        let ld_text = buffer_line_text(&state.buffer, last);
        let nx_text = if next < line_count {
            buffer_line_text(&state.buffer, next)
        } else {
            "(end)".into()
        };
        let widget_h = state.text_view.height();
        let desc_guard = descender_guard_px(&state.text_view, state.page_top_line);
        log_fmt!("PAGE_FWD: page_top={} last_visible={} last_dialogue={} next={}", state.page_top_line, last_visible, last, next);
        log_fmt!("PAGE_FWD: widget_h={} desc_guard={} usable_h={}", widget_h, desc_guard, widget_h - desc_guard);
        log_fmt!("PAGE_FWD: last_visible_text='{}'", lv_text.chars().take(60).collect::<String>());
        log_fmt!("PAGE_FWD: last_dialogue_text='{}'", ld_text.chars().take(60).collect::<String>());
        log_fmt!("PAGE_FWD: next_text='{}'", nx_text.chars().take(60).collect::<String>());
        for i in last_visible.saturating_sub(2)..=(last_visible + 2).min(line_count - 1) {
            if let Some(iter) = state.buffer.iter_at_line(i as i32) {
                let (_y, h) = state.text_view.line_yrange(&iter);
                let t = buffer_line_text(&state.buffer, i);
                log_fmt!("PAGE_FWD: line {} h={} '{}'", i, h, t.chars().take(50).collect::<String>());
            }
        }
    }

    if next_dialogue >= line_count {
        return;
    }

    state.page_history.push(state.page_top_line);

    state.current_line = next_dialogue;
    update_highlight(state);
    seek_to_current_line(state);
    set_page(state, new_top, PageDirection::Forward);
    auto_show_vocab_popup(state);
}
```

## Why Re-adding Costs Something

The inline `last_visible` / `last` / `next` computation duplicates work
`next_page_top` already does. If you restore the log, `page_forward`
computes the page boundary twice per j-press. Negligible cost on prose
(~30 lines/page → microseconds), but it creates two code paths that must
stay in sync. If you change descender-guard math, dialogue detection, or
speaker-backup logic, update **both** the inline values (used only for the
log) and `next_page_top` (used for actual navigation), or the log will lie
about what the user is seeing.

After diagnosing, remove the block again to restore the single source of
truth.

## How to Verify the Log Works

1. Re-add the block, `cargo build`, restart the app.
2. Open a prose work (e.g. *Bleak House*) and press `j` once.
3. `tail -n 20 ~/utono/linux-lit/linux-lit-dev.log` — you should see five
   `PAGE_FWD:` lines plus per-line height entries.
4. The `last_visible` line in the log should match the bottom of what you
   actually saw on screen before the page turn.

## Historical Context

The block was added during the e-reader pagination work (commit
`eb4f8f96` and around) when descender-guard sizing was being tuned. It
stayed for several months while dialogue-trim and speaker-backup edge
cases got hammered out. Removed in the page-prefix cleanup (April 2026)
once pagination had been stable and `next_page_top` had become the single
source of truth shared with the bottom-overlay page label.
