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
/// `src_lead_extra_pad` for how the effective extra (44 − base padding-top per
/// source class) is derived from src-lead's compound CSS selector.
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
/// UNLESS a more specific `.chat-transcript label.chat-a-*` selector overrides
/// it to 0px (chat-a-speaker, chat-a-verse, chat-a-stage, chat-a-verse-flush,
/// chat-a-stage-flush all override; chat-q/chat-a/chat-chip/chat-error/
/// chat-saved/chat-a-src-lead/chat-a-gloss do not, so they keep the blanket
/// 3px). The 3px is folded into each total below so the computed height
/// matches what GTK actually renders — omitting it would undercount every
/// un-overridden row by 3px and drift pagination.
pub(crate) fn class_pad(class: &str) -> i32 {
    match class {
        // padding-top 10 + blanket padding-bottom 3 (not overridden for
        // these classes) = 13.
        "chat-q" => 13,
        "chat-a" => 13,
        "chat-chip" => 13,
        "chat-error" => 13,
        "chat-saved" => 13,
        // Source base classes: padding-top per `base_padding_top` + a
        // padding-bottom 0 override, so the total IS the base padding-top.
        "chat-a-speaker" | "chat-a-verse" | "chat-a-verse-flush" | "chat-a-stage"
        | "chat-a-stage-flush" => base_padding_top(class),
        // padding-top 26 + blanket padding-bottom 3
        // (no override for chat-a-gloss) = 29.
        "chat-a-gloss" => 29,
        // padding-top 26 + blanket padding-bottom 3
        // (no override for chat-a-src-lead) = 29. NOTE: chat-a-src-lead is only
        // ever an extra_class, never a base class, so this arm is unused by
        // pagination — src_lead_extra_pad handles the effective extra instead.
        "chat-a-src-lead" => 29,
        _ => 0,
    }
}

/// The `padding-top` of each SOURCE base class's `.chat-a-*` rule in theme.rs
/// — the one table `class_pad` and `src_lead_extra_pad` must both agree on
/// (they hand-copied it until audit #89). MIRRORS theme.rs; keep in sync.
fn base_padding_top(class: &str) -> i32 {
    match class {
        "chat-a-speaker" => 14,
        "chat-a-verse" | "chat-a-verse-flush" => 0,
        "chat-a-stage" | "chat-a-stage-flush" => 8,
        _ => 0,
    }
}

/// The EXTRA rendered `padding-top` a `chat-a-src-lead` second class adds ON TOP
/// of the base class's own `padding-top`.
///
/// The src-lead rule in theme.rs is a COMPOUND selector
/// `.chat-transcript label.chat-a-src-lead { padding-top: 26px }` (specificity
/// 0,0,2,1). Every base source rule (`.chat-a-speaker`, `.chat-a-verse`,
/// `.chat-a-stage`, `.chat-a-*-flush`) is a single-class selector (0,0,1,0), so
/// src-lead WINS for ALL of them regardless of stylesheet order: the rendered
/// `padding-top` of a src-lead row is 26 for EVERY source base class. (This
/// replaced the old source-order collision, where only `chat-a-speaker` — the
/// one base rule ordered before src-lead — won and everything else got 0.)
///
/// padding-top is non-additive (26 REPLACES the base, it does not stack), and
/// `class_pad(&w.class)` has already added the base's own padding-top. So the
/// EXTRA src-lead contributes over the base is `26 - base_padding_top`, clamped
/// at ≥0, per base class:
///   `chat-a-speaker`      base pt 14 → +12
///   `chat-a-verse`        base pt 0  → +26
///   `chat-a-verse-flush`  base pt 0  → +26
///   `chat-a-stage`        base pt 8  → +18
///   `chat-a-stage-flush`  base pt 8  → +18
///
/// Do NOT use `class_pad("chat-a-src-lead")` — that would double-count the base
/// pad already added by `class_pad(&w.class)`.
///
/// SYNC: three things must stay in lockstep or the src-lead row is mis-measured
/// (undercount → bottom clip; overcount → underfill): (1) `SRC_LEAD_PADDING_TOP`
/// = `.chat-a-src-lead { padding-top }` in theme.rs; (2) the CSS selector
/// stays a COMPOUND (`.chat-transcript label.chat-a-src-lead`) so it wins for all
/// source classes; (3) `base_padding_top`'s table matches theme.rs. Change any
/// one and update the others.
pub(crate) fn src_lead_extra_pad(base_class: &str) -> i32 {
    const SRC_LEAD_PADDING_TOP: i32 = 26; // theme.rs .chat-a-src-lead padding-top
    // src-lead's 26 wins via its compound selector, so the extra it adds over
    // the base is 26 - base_padding_top.
    (SRC_LEAD_PADDING_TOP - base_padding_top(base_class)).max(0)
}

/// Per-widget heights + group-start flags for pagination. `measure(text)` is the
/// pango text-height measurement (injected so this is unit-testable without GTK).
///
/// A row's height is `measure(text) + class_pad(primary) + src_lead extra`. The
/// src-lead extra is folded in via `src_lead_extra_pad` (NOT
/// `class_pad("chat-a-src-lead")`) because GTK CSS padding-top is non-additive:
/// src-lead's compound selector pins the row's rendered top to 44 for every
/// source class, so the extra over the base is 44 − base padding-top — see
/// `src_lead_extra_pad`.
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
    fn src_lead_extra_pad_raises_every_source_class() {
        // .chat-a-src-lead is a COMPOUND selector, so its 44px padding-top wins
        // for EVERY source base class regardless of stylesheet order. The extra
        // it adds over each base is 26 - base padding-top (clamped ≥0).
        assert_eq!(src_lead_extra_pad("chat-a-speaker"), 12); // 26 - 14
        assert_eq!(src_lead_extra_pad("chat-a-verse"), 26); // 26 - 0
        assert_eq!(src_lead_extra_pad("chat-a-verse-flush"), 26); // 26 - 0
        assert_eq!(src_lead_extra_pad("chat-a-stage"), 18); // 26 - 8
        assert_eq!(src_lead_extra_pad("chat-a-stage-flush"), 18); // 26 - 8
    }

    #[test]
    fn src_lead_height_includes_effective_extra_for_every_source_class() {
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
        assert_eq!(h[0], 20 + class_pad("chat-a-speaker") + 12);

        // A speakerless (verse-flush) source-after-gloss row also carries
        // chat-a-src-lead. Under the compound selector src-lead now WINS here
        // too, so the height includes the +26 extra (26 - 0 base padding-top).
        let verse_lead = ChatWidget {
            text: "prose source".into(),
            class: "chat-a-verse-flush".into(),
            extra_class: Some("chat-a-src-lead".into()),
            group_start: true,
        };
        let (h2, _) = widget_heights(std::slice::from_ref(&verse_lead), |_t| 20);
        assert_eq!(h2[0], 20 + class_pad("chat-a-verse-flush") + 26);
    }
}
