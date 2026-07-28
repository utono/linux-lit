//! `PageTop` — a page position as ONE value.
//!
//! A page top is `(buffer line, pixel offset into that line's first display
//! row)`. The offset is 0 in the normal line-aligned case and non-zero only on
//! a pinned prose grid, where a page may begin mid-paragraph — the viewport top
//! is then `line_yrange(line).y + offset`.
//!
//! # Why this is a type and not two fields
//!
//! It used to be two independent public fields on `AppState`, six lines apart.
//! They must always change together, and nothing enforced that: 29 sites
//! assigned the line, only 12 set the offset. The gap was the standing bug
//! surface, and on 2026-07-27 it produced five shipped bugs in one day — a
//! journal-overlay Escape that re-framed the page, a startup resume that opened
//! off-grid, a cross-work jump landing, a centering landing, and the
//! translations hide. Every one was "set the line, forget the offset" or
//! "compute the page geometrically instead of reading the pinned table".
//!
//! Making the fields private turns that whole class into a compile error: you
//! cannot hand a bare line to `set_page_instant` any more.
//!
//! # `at_line_start` is deliberately verbose
//!
//! There is no `From<usize>` and there must not be — an implicit conversion
//! would silently restore exactly the bug this type removes. Every site that
//! legitimately has no offset says so by name, which makes the audit a grep:
//!
//! ```text
//! rg "at_line_start" src/
//! ```
//!
//! Each hit is a claim that no pinned table is active there. When that claim is
//! wrong, it is a latent instance of the 2026-07-27 landing bug.

/// The top of a rendered page: a buffer line plus the pixel offset into that
/// line's first display row. Construct with [`PageTop::at_line_start`] (offset
/// 0, deliberate) or [`PageTop::new`] (offset read from a pinned table).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PageTop {
    line: usize,
    offset: i32,
}

impl PageTop {
    /// A page starting at the very top of `line`.
    ///
    /// The ONLY way to build a `PageTop` without an offset. Use it where no
    /// pinned prose grid can be active (plays, pre-table-load, a genuine
    /// line-aligned boundary). If a prose table COULD be active at the call
    /// site, read the boundary instead —
    /// `navigation::canonical_page_top_offset_for` or
    /// `prose_pages::prose_table_boundary_for_line` — or the page renders a
    /// window the pagination never chose.
    pub fn at_line_start(line: usize) -> Self {
        Self { line, offset: 0 }
    }

    /// A page position with a known offset — typically read from a pinned
    /// `prose_pages` row, where `offset` is the row's `start_off`.
    pub fn new(line: usize, offset: i32) -> Self {
        Self { line, offset }
    }

    /// The buffer line this page starts at.
    pub fn line(self) -> usize {
        self.line
    }

    /// Pixels scrolled PAST `line`'s pixel top. 0 unless a pinned prose page
    /// begins mid-paragraph.
    pub fn offset(self) -> i32 {
        self.offset
    }

    /// True when this position sits at a line's top — i.e. carries no
    /// sub-line offset. Not the same as "on the grid": a stored prose page can
    /// legitimately start at offset 0.
    #[allow(dead_code)] // read by tests; kept as part of the type's vocabulary
    pub fn is_line_aligned(self) -> bool {
        self.offset == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_line_start_carries_no_offset() {
        let t = PageTop::at_line_start(42);
        assert_eq!(t.line(), 42);
        assert_eq!(t.offset(), 0);
        assert!(t.is_line_aligned());
    }

    /// The real BH-Barrett page from the 2026-07-27 bug: the reader belonged at
    /// `(42, 603)` and landed on line 47 instead. The offset is the half a bare
    /// `usize` could not carry — 603px, most of a paragraph.
    #[test]
    fn new_round_trips_a_pinned_boundary() {
        let t = PageTop::new(42, 603);
        assert_eq!((t.line(), t.offset()), (42, 603));
        assert!(!t.is_line_aligned(), "a mid-paragraph page is not line-aligned");
        // Dropping the offset is a DIFFERENT position, and the type keeps them
        // distinguishable — the old two-field form compared equal on the line
        // alone at every site that only looked at the line.
        assert_ne!(t, PageTop::at_line_start(42));
    }

    #[test]
    fn default_is_the_document_start() {
        assert_eq!(PageTop::default(), PageTop::at_line_start(0));
    }

    /// Equality is by BOTH halves — `resnap`-style "am I already on the grid?"
    /// checks depend on this, and comparing lines alone is how an off-grid
    /// position passes for a canonical one.
    #[test]
    fn equality_uses_both_halves() {
        assert_eq!(PageTop::new(42, 603), PageTop::new(42, 603));
        assert_ne!(PageTop::new(42, 603), PageTop::new(42, 0));
        assert_ne!(PageTop::new(42, 603), PageTop::new(43, 603));
    }
}
