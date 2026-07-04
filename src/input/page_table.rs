//! Pure page-table types + the invariant suite shared by the in-app generator
//! and (structurally) the validate-play-pages skill. Everything here is
//! GTK-free so it is unit-testable. Buffer-line space; `end` is inclusive.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spread {
    pub left_start: usize,
    /// First line of the right column; None = empty right (watermark spread).
    pub split: Option<usize>,
    pub end: usize,
}

pub struct ValidateCtx<'a> {
    pub line_count: usize,
    pub is_dialogue: &'a [bool],
    pub section_starts: Option<&'a [bool]>,
    /// Per-buffer-line pixel heights (line_yrange), measured at the layout
    /// the table is generated for.
    pub heights: &'a [i32],
    /// widget_height - descender_guard - BASE_BOTTOM_MARGIN at that layout.
    pub usable_height: i32,
}

/// The invariant suite (design doc §Invariant suite, items 1-4). Returns the
/// FIRST violated invariant as "<name>: <details>".
pub fn validate_spreads(spreads: &[Spread], ctx: &ValidateCtx) -> Result<(), String> {
    if spreads.is_empty() {
        return Err("coverage: no spreads".into());
    }
    // sanity + monotone, contiguous coverage
    let mut expect_start = spreads[0].left_start;
    if expect_start != 0 {
        return Err(format!("coverage: first page starts at {expect_start}, not 0"));
    }
    for (i, s) in spreads.iter().enumerate() {
        if s.left_start != expect_start {
            return Err(format!(
                "coverage: page {} starts at {} but previous page ended at {}",
                i + 1, s.left_start, expect_start.saturating_sub(1)
            ));
        }
        if let Some(sp) = s.split {
            if !(s.left_start <= sp && sp <= s.end + 1) {
                return Err(format!(
                    "sanity: page {} split {} outside [{}, {}]",
                    i + 1, sp, s.left_start, s.end + 1
                ));
            }
        }
        if s.end < s.left_start || s.end >= ctx.line_count {
            return Err(format!(
                "sanity: page {} end {} outside [{}, {})",
                i + 1, s.end, s.left_start, ctx.line_count
            ));
        }
        // watermark: an empty right column is only sanctioned when the NEXT
        // page opens a (div1,div2) section (authoritative bitmap, never text).
        if s.split.is_none() && i + 1 < spreads.len() {
            let next_top = spreads[i + 1].left_start;
            let opens_section = ctx
                .section_starts
                .and_then(|ss| ss.get(next_top).copied())
                .unwrap_or(false);
            if !opens_section {
                return Err(format!(
                    "watermark: page {} has an empty right column but page {} does not open a section",
                    i + 1, i + 2
                ));
            }
        }
        // fit: each column's summed heights must fit usable_height.
        let col_sum = |a: usize, b_incl: usize| -> i32 {
            ctx.heights[a..=b_incl.min(ctx.heights.len() - 1)].iter().sum()
        };
        let (left_end, right_range) = match s.split {
            Some(sp) if sp > s.left_start => (sp - 1, (sp <= s.end).then_some((sp, s.end))),
            Some(sp) => (s.left_start, (sp <= s.end).then_some((sp, s.end))), // empty left
            None => (s.end, None),
        };
        if left_end >= s.left_start && s.split != Some(s.left_start) {
            let sum = col_sum(s.left_start, left_end);
            if sum > ctx.usable_height {
                return Err(format!(
                    "fit: page {} left column {}..={} sums to {} > usable {}",
                    i + 1, s.left_start, left_end, sum, ctx.usable_height
                ));
            }
        }
        if let Some((a, b)) = right_range {
            let sum = col_sum(a, b);
            if sum > ctx.usable_height {
                return Err(format!(
                    "fit: page {} right column {}..={} sums to {} > usable {}",
                    i + 1, a, b, sum, ctx.usable_height
                ));
            }
        }
        expect_start = s.end + 1;
    }
    // tail: every dialogue line at/after the last page's end must be ON a page.
    let last_end = spreads.last().unwrap().end;
    if let Some(missed) = (last_end + 1..ctx.line_count)
        .find(|&i| ctx.is_dialogue.get(i).copied().unwrap_or(false))
    {
        return Err(format!(
            "tail: dialogue line {} lies past the last page (end {})",
            missed, last_end
        ));
    }
    Ok(())
}

/// The page whose [left_start, end] interval contains `line`.
pub fn page_for_line(spreads: &[Spread], line: usize) -> Option<usize> {
    let idx = spreads.partition_point(|s| s.left_start <= line);
    if idx == 0 {
        return None;
    }
    let i = idx - 1;
    (line <= spreads[i].end).then_some(i)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 10 lines, all dialogue, uniform height 10, viewport fits 3+3 per spread.
    fn ctx<'a>(heights: &'a [i32], dlg: &'a [bool]) -> ValidateCtx<'a> {
        ValidateCtx {
            line_count: heights.len(),
            is_dialogue: dlg,
            section_starts: None,
            heights,
            usable_height: 30,
        }
    }

    fn ok_spreads() -> Vec<Spread> {
        vec![
            Spread { left_start: 0, split: Some(3), end: 5 },
            Spread { left_start: 6, split: Some(9), end: 9 },
        ]
    }

    #[test]
    fn valid_table_passes() {
        let h = vec![10; 10];
        let d = vec![true; 10];
        assert!(validate_spreads(&ok_spreads(), &ctx(&h, &d)).is_ok());
    }

    #[test]
    fn gap_between_pages_fails_coverage() {
        let h = vec![10; 10];
        let d = vec![true; 10];
        let s = vec![
            Spread { left_start: 0, split: Some(3), end: 5 },
            Spread { left_start: 7, split: Some(9), end: 9 }, // line 6 dropped
        ];
        let err = validate_spreads(&s, &ctx(&h, &d)).unwrap_err();
        assert!(err.contains("coverage"), "got: {err}");
    }

    #[test]
    fn tail_not_reached_fails() {
        let h = vec![10; 10];
        let d = vec![true; 10];
        let s = vec![Spread { left_start: 0, split: Some(3), end: 5 }]; // 6..9 missing
        let err = validate_spreads(&s, &ctx(&h, &d)).unwrap_err();
        assert!(err.contains("tail"), "got: {err}");
    }

    #[test]
    fn overfull_column_fails_fit() {
        let mut h = vec![10; 10];
        h[1] = 25; // left col 0..=2 sums to 45 > usable 30
        let d = vec![true; 10];
        let err = validate_spreads(&ok_spreads(), &ctx(&h, &d)).unwrap_err();
        assert!(err.contains("fit"), "got: {err}");
    }

    #[test]
    fn disordered_split_fails_sanity() {
        let h = vec![10; 10];
        let d = vec![true; 10];
        let s = vec![
            Spread { left_start: 0, split: Some(7), end: 5 }, // split > end
            Spread { left_start: 6, split: Some(9), end: 9 },
        ];
        let err = validate_spreads(&s, &ctx(&h, &d)).unwrap_err();
        assert!(err.contains("sanity"), "got: {err}");
    }

    #[test]
    fn empty_right_requires_section_start_next() {
        let h = vec![10; 10];
        let d = vec![true; 10];
        let mut ss = vec![false; 10];
        let s = vec![
            Spread { left_start: 0, split: None, end: 2 },
            Spread { left_start: 3, split: Some(5), end: 6 },
            Spread { left_start: 7, split: Some(9), end: 9 },
        ];
        // Without a section start at the next page top: fail.
        let c1 = ValidateCtx {
            line_count: h.len(),
            is_dialogue: &d,
            section_starts: Some(&ss),
            heights: &h,
            usable_height: 30,
        };
        assert!(validate_spreads(&s, &c1).unwrap_err().contains("watermark"));
        // With it: pass.
        ss[3] = true;
        let c2 = ValidateCtx {
            line_count: h.len(),
            is_dialogue: &d,
            section_starts: Some(&ss),
            heights: &h,
            usable_height: 30,
        };
        assert!(validate_spreads(&s, &c2).is_ok());
    }

    #[test]
    fn page_for_line_finds_containing_page() {
        let s = ok_spreads();
        assert_eq!(page_for_line(&s, 0), Some(0));
        assert_eq!(page_for_line(&s, 5), Some(0));
        assert_eq!(page_for_line(&s, 6), Some(1));
        assert_eq!(page_for_line(&s, 9), Some(1));
        assert_eq!(page_for_line(&s, 10), None);
    }
}
