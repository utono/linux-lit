//! Pure page + row-cursor arithmetic for the paginated chat panel. No GTK.
use crate::ui::pagination::Page;

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
}
