//! Pure geometry, color, and citation helpers extracted from `gloss_overlay`.
//! No GTK dependencies; all functions are pure and `pub(crate)` for the
//! overlay's impl to call.

pub(crate) fn split_echo(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    let open = trimmed.find('[')?;
    let close = trimmed.rfind(']')?;
    if close <= open {
        return None;
    }
    let inner = &trimmed[open + 1..close];
    let suffix = trimmed[close + 1..].trim();

    // Split the bracket interior at the last em-dash separator.
    let sep = inner.rfind(" — ").or_else(|| inner.rfind(" - "))?;
    let quote = inner[..sep].trim().to_string();
    let mut citation = inner[sep..].trim().to_string();
    if !suffix.is_empty() {
        citation.push(' ');
        citation.push_str(suffix);
    }
    if quote.is_empty() || citation.is_empty() {
        return None;
    }
    Some((quote, citation))
}

/// Inputs to `cursor_scroll_target`: the cursor block's vertical span and the
/// current viewport/scroll geometry, all in vadjustment coordinate space.
pub(crate) struct CursorScrollGeom {
    pub(crate) block_top: f64,
    pub(crate) block_bottom: f64,
    pub(crate) view_top: f64,
    pub(crate) view_bottom: f64,
    pub(crate) page_size: f64,
    pub(crate) lower: f64,
    pub(crate) max_value: f64,
    pub(crate) pad: f64,
}

/// Decide the scroll value (viewport top) that brings the cursor block into
/// view, or `None` if it is already fully visible. Pure arithmetic so it can be
/// unit-tested without GTK.
///
/// Three cases:
/// - block starts above the viewport → reveal its top (clamped).
/// - block ends below the viewport → reveal its bottom, BUT keep the block's
///   top in view *only when the block actually fits* in the viewport. A block
///   TALLER than the viewport cannot show both edges; for it we reveal the
///   bottom unconditionally (the final explication is often taller than the
///   card, and capping at its top stranded the last line below the fold — the
///   bottom-clip box only masks a sub-row sliver, not a whole clipped line).
/// - otherwise already visible → `None`.
pub(crate) fn cursor_scroll_target(g: &CursorScrollGeom) -> Option<f64> {
    let CursorScrollGeom {
        block_top,
        block_bottom,
        view_top,
        view_bottom,
        page_size,
        lower,
        max_value,
        pad,
    } = *g;
    // Does the block (plus its top pad) fit inside one viewport height? An
    // over-tall block (e.g. the final explication, often taller than the card)
    // cannot show both edges, so it gets special handling below.
    let fits = (block_bottom - block_top) + pad <= page_size;
    let bottom_hidden = block_bottom > view_bottom - pad;
    let top_hidden = block_top < view_top + pad;

    if !fits && bottom_hidden {
        // Over-tall block whose bottom is below the fold: reveal the bottom even
        // if that scrolls the block's top off the top edge. This MUST take
        // priority over the "reveal top" branch — otherwise, once the cursor is
        // on the last (over-tall) block and the top is already in view, the
        // top-reveal branch wins forever and the final rows stay clipped below
        // the fold (the bottom-clip box only masks a sub-row sliver, not a whole
        // line). by_bottom brings `block_bottom + pad` to the viewport bottom.
        Some((block_bottom + pad - page_size).clamp(lower, max_value))
    } else if top_hidden {
        // Block starts above the viewport: bring its top into view.
        Some((block_top - pad).clamp(lower, max_value))
    } else if bottom_hidden {
        // Fitting block ending below the viewport: bring its bottom into view,
        // but never scroll its own top above the viewport top.
        let by_bottom = (block_bottom + pad - page_size).clamp(lower, max_value);
        Some(by_bottom.min((block_top - pad).max(lower)))
    } else {
        None // already fully visible
    }
}

/// Snap `target_y` to the least row top at/above it (clamped to
/// `[lower, max_value]`). If no row top is >= target, use `max_value`. Pure so
/// the snap-UP direction (used for bottom-reveal) can be unit-tested. `row_tops`
/// must be ascending.
pub(crate) fn snap_up_to_row(target_y: f64, row_tops: &[f64], lower: f64, max_value: f64) -> f64 {
    let target = target_y.clamp(lower, max_value);
    row_tops
        .iter()
        .copied()
        .find(|t| *t + 0.5 >= target)
        .unwrap_or(max_value)
        .clamp(lower, max_value)
}

pub(crate) fn parse_hex_color(hex: &str) -> Option<(f64, f64, f64)> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()? as f64 / 255.0;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()? as f64 / 255.0;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()? as f64 / 255.0;
    Some((r, g, b))
}

pub(crate) fn build_diff_markup(original: &str, corrected: &str, is_original: bool) -> String {
    let orig_words: Vec<&str> = original.split_whitespace().collect();
    let corr_words: Vec<&str> = corrected.split_whitespace().collect();

    let (source_words, other_words) = if is_original {
        (&orig_words, &corr_words)
    } else {
        (&corr_words, &orig_words)
    };

    let mut changed = vec![false; source_words.len()];
    for (i, word) in source_words.iter().enumerate() {
        changed[i] = other_words.get(i) != Some(word);
    }

    let source_text = if is_original { original } else { corrected };
    let mut result = String::new();
    let mut word_idx = 0;

    for (line_num, line) in source_text.lines().enumerate() {
        if line_num > 0 {
            result.push('\n');
        }
        for (j, word) in line.split_whitespace().enumerate() {
            if j > 0 {
                result.push(' ');
            }
            let escaped = glib::markup_escape_text(word);
            if word_idx < changed.len() && changed[word_idx] {
                let color = if is_original { "#cc3333" } else { "#228833" };
                result.push_str(&format!("<span foreground=\"{}\" weight=\"bold\">{}</span>", color, escaped));
            } else {
                result.push_str(&escaped);
            }
            word_idx += 1;
        }
    }
    result
}

/// Parse a citation "ABBR.div1.div2.line" into (abbrev, div1, div2, line).
/// Returns None unless it has the full 4-part shape with numeric tail segments.
pub(crate) fn parse_citation(c: &str) -> Option<(&str, &str, &str, &str)> {
    // Split off the trailing three numeric segments; the abbrev may itself
    // contain dots in principle, so split from the right.
    let mut it = c.rsplitn(4, '.');
    let line = it.next()?;
    let div2 = it.next()?;
    let div1 = it.next()?;
    let abbrev = it.next()?;
    if abbrev.is_empty() || line.is_empty() || div1.is_empty() || div2.is_empty() {
        return None;
    }
    Some((abbrev, div1, div2, line))
}

/// Format a passage citation range for the footer, collapsing the shared
/// prefix:
/// - single line (start == end):            "2H6 1.4.7"
/// - same act/scene, different line:         "2H6 1.4.7–14"
/// - spans a scene/act boundary:             "2H6 1.4.7–2.1.3"
/// Falls back to "start–end" (or "start") when a citation can't be parsed.
/// Returns None only when there is no usable start citation.
pub(crate) fn format_citation_range(start: &str, end: &str) -> Option<String> {
    if start.is_empty() {
        return None;
    }
    let s = parse_citation(start);
    let e = parse_citation(end);
    match (s, e) {
        (Some((sa, s1, s2, sl)), Some((_ea, e1, e2, el))) => {
            if start == end {
                Some(format!("{} {}.{}.{}", sa, s1, s2, sl))
            } else if s1 == e1 && s2 == e2 {
                // Same act/scene: collapse the end to just its line number.
                Some(format!("{} {}.{}.{}–{}", sa, s1, s2, sl, el))
            } else {
                // Spans a boundary: show the end's act.scene.line (no abbrev).
                Some(format!("{} {}.{}.{}–{}.{}.{}", sa, s1, s2, sl, e1, e2, el))
            }
        }
        // Unparseable: degrade gracefully to the raw strings.
        _ => {
            if end.is_empty() || start == end {
                Some(start.to_string())
            } else {
                Some(format!("{}–{}", start, end))
            }
        }
    }
}

#[cfg(test)]
mod snap_up_tests {
    use super::snap_up_to_row;

    // Live geometry from the dev log for the clipped Cranmer gloss: revealing
    // the last block computed target=514, and the visual row tops near it were
    // [450, 520, 547, 582] with the scroll ceiling max_value=570. The OLD
    // floor-snap pulled 514 down to 450 (re-hiding the bottom). Snapping UP must
    // pick 520 — the least row top >= 514 — keeping the bottom in view.
    #[test]
    fn snaps_up_to_next_row_not_down() {
        let rows = [450.0, 520.0, 547.0, 582.0];
        let v = snap_up_to_row(514.0, &rows, 0.0, 570.0);
        assert!(
            (v - 520.0).abs() < 0.5,
            "514 must snap UP to 520 (next row), not down to 450; got {v}"
        );
    }

    #[test]
    fn target_on_a_row_top_stays_put() {
        let rows = [450.0, 520.0, 547.0];
        let v = snap_up_to_row(520.0, &rows, 0.0, 570.0);
        assert!((v - 520.0).abs() < 0.5, "exact row top stays; got {v}");
    }

    #[test]
    fn target_past_last_row_uses_max_value() {
        // No row top >= target but target <= ceiling: use max_value so the
        // document end is reachable.
        let rows = [450.0, 520.0];
        let v = snap_up_to_row(560.0, &rows, 0.0, 570.0);
        assert!(
            (v - 570.0).abs() < 0.5,
            "target past last row top should use max_value (570); got {v}"
        );
    }

    #[test]
    fn clamps_to_max_value() {
        let rows = [450.0, 520.0, 600.0];
        // target above ceiling clamps to max_value first; next row 600 > 570
        // would exceed ceiling, so result clamps to 570.
        let v = snap_up_to_row(900.0, &rows, 0.0, 570.0);
        assert!((v - 570.0).abs() < 0.5, "must clamp to max_value; got {v}");
    }
}

#[cfg(test)]
mod cursor_scroll_tests {
    use super::{cursor_scroll_target, CursorScrollGeom};

    // Geometry captured from the live dev log for the Cranmer (H8) gloss whose
    // final explication clipped: viewport page_size=1055, scroll ceiling
    // max_value=570 (upper 1625 - page 1055), last block spans 450..1539 — a
    // block 1089px tall, i.e. TALLER than the 1055px viewport. The cursor sits
    // on this last block.
    const PAGE: f64 = 1055.0;
    const MAX_VALUE: f64 = 570.0;
    const LOWER: f64 = 0.0;
    const PAD: f64 = 24.0;

    #[test]
    fn over_tall_last_block_reveals_bottom_not_top() {
        // Cursor on the last block; viewport currently at top=450 (the buggy
        // plateau). The block's bottom (1539) is below the fold (450+1055=1505).
        let target = cursor_scroll_target(&CursorScrollGeom {
            block_top: 450.0,
            block_bottom: 1539.0,
            view_top: 450.0,
            view_bottom: 450.0 + PAGE,
            page_size: PAGE,
            lower: LOWER,
            max_value: MAX_VALUE,
            pad: PAD,
        })
        .expect("over-tall block below fold must scroll, not report already-visible");

        // The fix: reveal the bottom. by_bottom = 1539+24-1055 = 508, clamped to
        // max_value 570 => 508. The OLD code did .min(block_top-pad=426) => 426,
        // which left the last line clipped. Assert we do NOT cap at the top.
        assert!(
            target > 450.0,
            "must scroll past the plateau top (450) to reveal the last row; got {target}"
        );
        assert!(
            (target - 508.0).abs() < 0.5,
            "should target by_bottom (508) to bring the block bottom to the fold; got {target}"
        );
    }

    #[test]
    fn fitting_block_below_fold_keeps_top_in_view() {
        // A SHORT block that fits in the viewport, sitting just below the fold:
        // reveal its bottom but never scroll its own top off-screen.
        let target = cursor_scroll_target(&CursorScrollGeom {
            block_top: 1400.0,
            block_bottom: 1500.0, // 100px tall, fits easily
            view_top: 0.0,
            view_bottom: PAGE,
            page_size: PAGE,
            lower: LOWER,
            max_value: 5000.0, // big ceiling so clamping doesn't mask the cap
            pad: PAD,
        })
        .expect("block below fold must scroll");

        // by_bottom = 1500+24-1055 = 469; block_top-pad = 1376. min => 469.
        // The cap (1376) does not bind here, so we land on by_bottom and the
        // block's top stays comfortably in view.
        assert!(
            (target - 469.0).abs() < 0.5,
            "fitting block should reveal bottom via by_bottom (469); got {target}"
        );
    }

    #[test]
    fn fully_visible_block_does_not_scroll() {
        // Block already inside the viewport (with pad): no scroll.
        let target = cursor_scroll_target(&CursorScrollGeom {
            block_top: 200.0,
            block_bottom: 400.0,
            view_top: 100.0,
            view_bottom: 100.0 + PAGE,
            page_size: PAGE,
            lower: LOWER,
            max_value: MAX_VALUE,
            pad: PAD,
        });
        assert!(target.is_none(), "fully visible block must not scroll");
    }

    #[test]
    fn block_above_viewport_reveals_top() {
        // Block starts above the current viewport top: bring its top into view.
        let target = cursor_scroll_target(&CursorScrollGeom {
            block_top: 300.0,
            block_bottom: 500.0,
            view_top: 800.0,
            view_bottom: 800.0 + PAGE,
            page_size: PAGE,
            lower: LOWER,
            max_value: 5000.0,
            pad: PAD,
        })
        .expect("block above viewport must scroll up");
        // block_top - pad = 276.
        assert!(
            (target - 276.0).abs() < 0.5,
            "should reveal block top (276); got {target}"
        );
    }
}

#[cfg(test)]
mod citation_range_tests {
    use super::format_citation_range;

    #[test]
    fn single_line_no_dash() {
        assert_eq!(format_citation_range("2H6.1.4.7", "2H6.1.4.7").unwrap(), "2H6 1.4.7");
    }

    #[test]
    fn same_scene_collapses_end_to_line() {
        assert_eq!(format_citation_range("2H6.1.4.7", "2H6.1.4.14").unwrap(), "2H6 1.4.7–14");
    }

    #[test]
    fn cross_scene_shows_full_end_without_abbrev() {
        assert_eq!(
            format_citation_range("2H6.1.4.7", "2H6.2.1.3").unwrap(),
            "2H6 1.4.7–2.1.3"
        );
    }

    #[test]
    fn empty_start_is_none() {
        assert_eq!(format_citation_range("", "2H6.1.4.7"), None);
    }

    #[test]
    fn unparseable_degrades_to_raw_range() {
        assert_eq!(format_citation_range("weird", "alsoweird").unwrap(), "weird–alsoweird");
        assert_eq!(format_citation_range("weird", "weird").unwrap(), "weird");
    }

    #[test]
    fn empty_end_uses_raw_start_only() {
        // Empty end can't be parsed, so we degrade to the raw start string.
        assert_eq!(format_citation_range("2H6.1.4.7", "").unwrap(), "2H6.1.4.7");
    }
}
