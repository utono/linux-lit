//! Pure page + row-cursor arithmetic for the paginated chat panel. No GTK.
use crate::ui::pagination::Page;

/// One rendered transcript widget: its text, CSS class, and whether it starts a
/// new indivisible pagination unit (a `GlossAnswer`/journal answer's first
/// widget is a group start; its continuation widgets are not).
///
/// `extra_class` carries the SECOND CSS class a row may render with (today only
/// `chat-a-src-lead`, the top gap on the first source row after a gloss — see
/// `gloss_answer_specs`). It is DATA, not just a render detail: the extra class
/// changes the row's rendered height, so pagination must account for it or it
/// undercounts and packs one row too many (the returning bottom clip). See
/// `src_lead_extra_pad` for how the effective extra is derived from the CSS
/// source-order collision.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChatWidget {
    pub text: String,
    pub class: String,
    pub extra_class: Option<String>,
    pub group_start: bool,
}

/// Total vertical CSS padding (top + bottom) the class adds around its text, so
/// the pagination height matches what GTK renders (`measure_text_height` sees
/// only the text). MIRRORS the `.chat-*` rules in `theme.rs` — keep in sync.
///
/// Every row also inherits `.chat-transcript label { padding-bottom: 3px }`
/// (theme.rs:1369) UNLESS a more specific `.chat-transcript label.chat-a-*`
/// selector overrides it to 0px (chat-a-speaker, chat-a-verse, chat-a-stage,
/// chat-a-verse-flush, chat-a-stage-flush all override; chat-q/chat-a/
/// chat-chip/chat-error/chat-saved/chat-a-src-lead/chat-a-gloss do not, so
/// they keep the blanket 3px). The 3px is folded into each total below so the
/// computed height matches what GTK actually renders — omitting it would
/// undercount every un-overridden row by 3px and drift pagination.
pub(crate) fn class_pad(class: &str) -> i32 {
    match class {
        // padding-top 10 (theme.rs:1392/1394/1398/1400/1401) + blanket
        // padding-bottom 3 (not overridden for these classes) = 13.
        "chat-q" => 13,
        "chat-a" => 13,
        "chat-chip" => 13,
        "chat-error" => 13,
        "chat-saved" => 13,
        // padding-top 14 (theme.rs:1424) + padding-bottom 0 override
        // (theme.rs:1434) = 14.
        "chat-a-speaker" => 14,
        // padding-top 0 (theme.rs:1441) + padding-bottom 0 override
        // (theme.rs:1442) = 0.
        "chat-a-verse" => 0,
        // padding-top 0 (theme.rs:1457) + padding-bottom 0 override
        // (theme.rs:1458) = 0.
        "chat-a-verse-flush" => 0,
        // padding-top 8 (theme.rs:1446) + padding-bottom 0 override
        // (theme.rs:1447) = 8.
        "chat-a-stage" => 8,
        // padding-top 8 (theme.rs:1460) + padding-bottom 0 override
        // (theme.rs:1461) = 8.
        "chat-a-stage-flush" => 8,
        // padding-top 18 (theme.rs:1462) + blanket padding-bottom 3
        // (no override for chat-a-gloss) = 21.
        "chat-a-gloss" => 21,
        // padding-top 44 (theme.rs:1428) + blanket padding-bottom 3
        // (no override for chat-a-src-lead) = 47. NOTE: chat-a-src-lead is only
        // ever an extra_class, never a base class, so this arm is unused by
        // pagination — src_lead_extra_pad handles the effective extra instead.
        "chat-a-src-lead" => 47,
        _ => 0,
    }
}

/// The EXTRA rendered `padding-top` a `chat-a-src-lead` second class adds ON TOP
/// of the base class's own `padding-top`, accounting for GTK CSS's non-additive
/// padding: two single-class rules that both set `padding-top` have EQUAL
/// specificity, so the one LATER in source order wins (it does not add).
///
/// Source order in `theme.rs` (see the `.chat-a-*` block, theme.rs ~1420-1461):
///   `.chat-a-speaker`      padding-top 14 (theme.rs:1424) — BEFORE src-lead
///   `.chat-a-src-lead`     padding-top 44 (theme.rs:1428)
///   `.chat-a-verse`        padding-top 0  (theme.rs:1441) — AFTER src-lead
///   `.chat-a-stage`        padding-top 8  (theme.rs:1446) — AFTER src-lead
///   `.chat-a-verse-flush`  padding-top 0  (theme.rs:1457) — AFTER src-lead
///   `.chat-a-stage-flush`  padding-top 8  (theme.rs:1460) — AFTER src-lead
///
/// So src-lead only WINS (raises rendered padding-top to 44) for a base class
/// whose rule comes BEFORE it — only `chat-a-speaker`. For every base whose rule
/// comes AFTER src-lead, the base's own `padding-top` wins and src-lead adds
/// nothing (net extra 0). The effective extra is therefore
/// `max(0, 44 - base_padding_top)` for the ONE class ordered before src-lead
/// (`chat-a-speaker`: 44-14 = 30), and 0 for all others. Do NOT use
/// `class_pad("chat-a-src-lead")` — that would double-count the base pad already
/// added by `class_pad(&w.class)`.
///
/// SYNC: `SRC_LEAD_PADDING_TOP` MUST equal `.chat-a-src-lead { padding-top }`
/// in theme.rs (~1428). Change both together or the height model undercounts
/// the src-lead row and the bottom clip returns.
pub(crate) fn src_lead_extra_pad(base_class: &str) -> i32 {
    const SRC_LEAD_PADDING_TOP: i32 = 44; // theme.rs:1428
    match base_class {
        // Only chat-a-speaker's rule (theme.rs:1424) precedes src-lead, so
        // src-lead wins: rendered padding-top 44, base pad 14 → +30 more.
        "chat-a-speaker" => SRC_LEAD_PADDING_TOP - 14,
        // Every other base class's padding-top rule comes AFTER src-lead in
        // source order, so the base wins and src-lead adds nothing.
        _ => 0,
    }
}

/// Per-widget heights + group-start flags for pagination. `measure(text)` is the
/// pango text-height measurement (injected so this is unit-testable without GTK).
///
/// A row's height is `measure(text) + class_pad(primary) + src_lead extra`. The
/// src-lead extra is folded in via `src_lead_extra_pad` (NOT
/// `class_pad("chat-a-src-lead")`) because GTK CSS padding-top is non-additive
/// under a source-order collision — see `src_lead_extra_pad`.
pub(crate) fn widget_heights(
    widgets: &[ChatWidget],
    measure: impl Fn(&str) -> i32,
) -> (Vec<i32>, Vec<bool>) {
    let heights = widgets
        .iter()
        .map(|w| {
            let extra = if w.extra_class.as_deref() == Some("chat-a-src-lead") {
                src_lead_extra_pad(&w.class)
            } else {
                0
            };
            measure(&w.text) + class_pad(&w.class) + extra
        })
        .collect();
    let group_start = widgets.iter().map(|w| w.group_start).collect();
    (heights, group_start)
}

/// Index of the page whose `[start,end)` contains `widget`; clamps to the last
/// page (and returns 0 when there are no pages).
pub(crate) fn page_of_widget(pages: &[Page], widget: usize) -> usize {
    for (i, p) in pages.iter().enumerate() {
        if widget >= p.start && widget < p.end {
            return i;
        }
    }
    pages.len().saturating_sub(1)
}

/// First widget index in the page with `landable[i]` true, if any.
pub(crate) fn first_landable_in_page(page: Page, landable: &[bool]) -> Option<usize> {
    (page.start..page.end).find(|&i| landable.get(i).copied().unwrap_or(false))
}

/// Last widget index in the page with `landable[i]` true, if any.
pub(crate) fn last_landable_in_page(page: Page, landable: &[bool]) -> Option<usize> {
    (page.start..page.end).rev().find(|&i| landable.get(i).copied().unwrap_or(false))
}

/// Step the row cursor by `delta` (±1) over landable widgets. Within the current
/// page the cursor moves to the next/previous landable widget. When it would run
/// off the page edge, turn to the adjacent page and land on that page's first
/// (delta>0) / last (delta<0) landable widget. Clamps (no-op) at the document
/// ends. Returns `(new_cursor, new_page)`.
pub(crate) fn step_cursor_paged(
    cursor: usize,
    delta: i32,
    page_idx: usize,
    pages: &[Page],
    landable: &[bool],
) -> (usize, usize) {
    let Some(page) = pages.get(page_idx).copied() else {
        return (cursor, page_idx);
    };
    // Next landable within this page in the step direction.
    let within = if delta > 0 {
        (cursor + 1..page.end).find(|&i| landable.get(i).copied().unwrap_or(false))
    } else {
        (page.start..cursor)
            .rev()
            .find(|&i| landable.get(i).copied().unwrap_or(false))
    };
    if let Some(w) = within {
        return (w, page_idx);
    }
    // Off the page edge — turn the page.
    if delta > 0 {
        if let Some(next) = pages.get(page_idx + 1).copied() {
            if let Some(w) = first_landable_in_page(next, landable) {
                return (w, page_idx + 1);
            }
        }
    } else if page_idx > 0 {
        let prev = pages[page_idx - 1];
        if let Some(w) = last_landable_in_page(prev, landable) {
            return (w, page_idx - 1);
        }
    }
    (cursor, page_idx) // clamp at document ends
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::pagination::Page;

    fn pages() -> Vec<Page> {
        // 3 pages over widget indices 0..9
        vec![Page { start: 0, end: 3 }, Page { start: 3, end: 6 }, Page { start: 6, end: 9 }]
    }

    #[test]
    fn page_of_widget_locates_and_clamps() {
        let p = pages();
        assert_eq!(page_of_widget(&p, 0), 0);
        assert_eq!(page_of_widget(&p, 4), 1);
        assert_eq!(page_of_widget(&p, 8), 2);
        assert_eq!(page_of_widget(&p, 99), 2); // clamp past end
        assert_eq!(page_of_widget(&[], 0), 0); // no pages
    }

    #[test]
    fn first_last_landable_skip_unlandable() {
        // page [3,6): widget 3 unlandable (a speaker), 4 & 5 landable
        let landable = vec![true, true, true, false, true, true, true, true, true];
        assert_eq!(first_landable_in_page(Page { start: 3, end: 6 }, &landable), Some(4));
        assert_eq!(last_landable_in_page(Page { start: 3, end: 6 }, &landable), Some(5));
        // a page with no landable widget
        assert_eq!(first_landable_in_page(Page { start: 3, end: 4 }, &landable), None);
    }

    #[test]
    fn step_within_page_moves_cursor_only() {
        let p = pages();
        let landable = vec![true; 9];
        // cursor 0 on page 0, +1 → cursor 1, same page
        assert_eq!(step_cursor_paged(0, 1, 0, &p, &landable), (1, 0));
        // cursor 1, -1 → cursor 0, same page
        assert_eq!(step_cursor_paged(1, -1, 0, &p, &landable), (0, 0));
    }

    #[test]
    fn step_off_page_end_turns_to_next_first_landable() {
        let p = pages();
        let landable = vec![true; 9];
        // cursor 2 is the last widget of page 0; +1 → page 1, its first landable (3)
        assert_eq!(step_cursor_paged(2, 1, 0, &p, &landable), (3, 1));
    }

    #[test]
    fn step_off_page_top_turns_to_prev_last_landable() {
        let p = pages();
        let landable = vec![true; 9];
        // cursor 3 is the first widget of page 1; -1 → page 0, its last landable (2)
        assert_eq!(step_cursor_paged(3, -1, 1, &p, &landable), (2, 0));
    }

    #[test]
    fn step_clamps_at_document_ends() {
        let p = pages();
        let landable = vec![true; 9];
        // cursor 0 on page 0, -1 → no prev page, stay
        assert_eq!(step_cursor_paged(0, -1, 0, &p, &landable), (0, 0));
        // cursor 8 (last) on page 2, +1 → no next page, stay
        assert_eq!(step_cursor_paged(8, 1, 2, &p, &landable), (8, 2));
    }

    #[test]
    fn class_pad_reads_known_classes() {
        // Values MUST match theme.rs at implementation time; these assert the
        // shape (a src-lead row carries a big top gap; a plain answer carries little).
        assert!(class_pad("chat-a-src-lead") >= class_pad("chat-a"));
        assert!(class_pad("chat-a-gloss") > 0);
        // An unknown class contributes 0 (defensive).
        assert_eq!(class_pad("nonexistent-class"), 0);
    }

    #[test]
    fn widget_heights_add_padding_and_carry_group_start() {
        let widgets = vec![
            ChatWidget { text: "Q".into(), class: "chat-q".into(), extra_class: None, group_start: true },
            ChatWidget { text: "verse".into(), class: "chat-a-verse".into(), extra_class: None, group_start: false },
        ];
        // measure returns a fixed 20px for any text
        let (h, gs) = widget_heights(&widgets, |_t| 20);
        assert_eq!(h[0], 20 + class_pad("chat-q"));
        assert_eq!(h[1], 20 + class_pad("chat-a-verse"));
        assert_eq!(gs, vec![true, false]);
    }

    #[test]
    fn src_lead_extra_pad_only_raises_speaker() {
        // Only chat-a-speaker's rule precedes .chat-a-src-lead in theme.rs, so
        // src-lead wins there (44 - 14 = 30). Every other base class's rule
        // comes AFTER src-lead, so the base wins and src-lead adds nothing.
        assert_eq!(src_lead_extra_pad("chat-a-speaker"), 30);
        assert_eq!(src_lead_extra_pad("chat-a-verse"), 0);
        assert_eq!(src_lead_extra_pad("chat-a-stage"), 0);
        assert_eq!(src_lead_extra_pad("chat-a-verse-flush"), 0);
        assert_eq!(src_lead_extra_pad("chat-a-stage-flush"), 0);
    }

    #[test]
    fn src_lead_speaker_height_includes_effective_extra_but_verse_does_not() {
        // A source-after-gloss block whose leading source row is a SPEAKER
        // carries chat-a-src-lead: its measured height must include the +30
        // effective extra on top of the base chat-a-speaker pad.
        let speaker_lead = ChatWidget {
            text: "CYMBELINE".into(),
            class: "chat-a-speaker".into(),
            extra_class: Some("chat-a-src-lead".into()),
            group_start: true,
        };
        let (h, _) = widget_heights(std::slice::from_ref(&speaker_lead), |_t| 20);
        assert_eq!(h[0], 20 + class_pad("chat-a-speaker") + 30);

        // A speakerless (verse-flush) source-after-gloss row also carries
        // chat-a-src-lead, but the verse-flush rule comes AFTER src-lead, so the
        // extra is 0 — no double-count.
        let verse_lead = ChatWidget {
            text: "prose source".into(),
            class: "chat-a-verse-flush".into(),
            extra_class: Some("chat-a-src-lead".into()),
            group_start: true,
        };
        let (h2, _) = widget_heights(std::slice::from_ref(&verse_lead), |_t| 20);
        assert_eq!(h2[0], 20 + class_pad("chat-a-verse-flush"));
    }
}
