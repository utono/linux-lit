pub fn to_roman(n: u32) -> Option<String> {
    if n == 0 || n > 3999 {
        return None;
    }
    const PAIRS: &[(u32, &str)] = &[
        (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"),
        (100, "C"), (90, "XC"), (50, "L"), (40, "XL"),
        (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
    ];
    let mut out = String::new();
    let mut remaining = n;
    for &(value, numeral) in PAIRS {
        while remaining >= value {
            out.push_str(numeral);
            remaining -= value;
        }
    }
    Some(out)
}

pub fn format_play_citation(
    act: i64,
    scene: i64,
    line: i64,
    speaker: Option<&str>,
) -> Option<String> {
    if line < 1 {
        return None;
    }
    // Prologue / Epilogue cases: div1/div2 may be 0 or out of normal act range.
    // Detect via speaker label when act/scene don't look like a normal citation.
    if act < 1 || scene < 1 {
        return match speaker {
            Some("PROLOGUE") => Some(format!("Prologue {}", line)),
            Some("EPILOGUE") => Some(format!("Epilogue {}", line)),
            _ => None,
        };
    }
    if let Some("EPILOGUE") = speaker {
        return Some(format!("Epilogue {}", line));
    }
    let act_r = to_roman(act as u32)?;
    let scene_r = to_roman(scene as u32)?.to_lowercase();
    Some(format!("{}.{}.{}", act_r, scene_r, line))
}

/// Format the bottom-overlay label for non-play works as
/// `"{page} - {line_id}"`, where `page` is the 1-indexed viewport page
/// containing the highlighted line.
pub fn format_prose_label(page: usize, line_id: i64) -> String {
    format!("{} - {}", page, line_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roman_basic() {
        assert_eq!(to_roman(1).as_deref(), Some("I"));
        assert_eq!(to_roman(4).as_deref(), Some("IV"));
        assert_eq!(to_roman(5).as_deref(), Some("V"));
        assert_eq!(to_roman(9).as_deref(), Some("IX"));
        assert_eq!(to_roman(40).as_deref(), Some("XL"));
        assert_eq!(to_roman(99).as_deref(), Some("XCIX"));
        assert_eq!(to_roman(3999).as_deref(), Some("MMMCMXCIX"));
    }

    #[test]
    fn roman_out_of_range() {
        assert_eq!(to_roman(0), None);
        assert_eq!(to_roman(4000), None);
    }

    #[test]
    fn play_citation_typical() {
        assert_eq!(format_play_citation(1, 1, 15, None).as_deref(), Some("I.i.15"));
        assert_eq!(format_play_citation(3, 2, 187, None).as_deref(), Some("III.ii.187"));
        assert_eq!(format_play_citation(5, 1, 1, None).as_deref(), Some("V.i.1"));
        assert_eq!(format_play_citation(4, 4, 42, None).as_deref(), Some("IV.iv.42"));
    }

    #[test]
    fn play_citation_prologue() {
        assert_eq!(
            format_play_citation(0, 0, 1, Some("PROLOGUE")).as_deref(),
            Some("Prologue 1")
        );
        assert_eq!(
            format_play_citation(0, 0, 31, Some("PROLOGUE")).as_deref(),
            Some("Prologue 31")
        );
    }

    #[test]
    fn play_citation_epilogue() {
        assert_eq!(
            format_play_citation(6, 0, 14, Some("EPILOGUE")).as_deref(),
            Some("Epilogue 14")
        );
        assert_eq!(
            format_play_citation(6, 0, 1, Some("EPILOGUE")).as_deref(),
            Some("Epilogue 1")
        );
    }

    #[test]
    fn play_citation_nested_prologue_uses_normal_format() {
        // Play-within-play prologue (e.g. Ham 3.2, MND 5.1) keeps normal citation.
        assert_eq!(
            format_play_citation(3, 2, 1, Some("PROLOGUE")).as_deref(),
            Some("III.ii.1")
        );
        assert_eq!(
            format_play_citation(5, 1, 5, Some("PROLOGUE")).as_deref(),
            Some("V.i.5")
        );
    }

    #[test]
    fn play_citation_invalid() {
        assert_eq!(format_play_citation(1, 1, 0, None), None);
        assert_eq!(format_play_citation(0, 0, 1, None), None);
        assert_eq!(format_play_citation(-1, 1, 1, None), None);
    }

    #[test]
    fn prose_label_basic() {
        assert_eq!(format_prose_label(1, 1234), "1 - 1234");
        assert_eq!(format_prose_label(34, 5678), "34 - 5678");
    }

    #[test]
    fn prose_label_page_one_line_one() {
        assert_eq!(format_prose_label(1, 1), "1 - 1");
    }

    #[test]
    fn prose_label_large_numbers() {
        assert_eq!(format_prose_label(999, 123456), "999 - 123456");
    }
}
