use serde_json::Value;
use std::path::PathBuf;

/// Minimum contrast ratio (vs the reading-surface bg) for the reader-gloss line
/// tint. The old 3.0 UI floor let a barely-distinct pastel (e.g. a 3.0 salmon on
/// cream) through, which was hard to read against near-black body text. 4.5 is
/// the WCAG AA body-text threshold — forces a DARKER variant of the same hue so
/// glossed lines read clearly while staying a distinct tint (body text is ~6.7).
const READER_GLOSS_MIN_CONTRAST: f64 = 4.5;

/// Number of root-color variants every theme has (index 0 = designed root).
/// Cycled by Ctrl+t; see docs/plans/2026-07-10-bg-variant-cycling-design.md.
pub const ROOT_VARIANT_COUNT: u8 = 5;

/// Root color for `variant` (0-4). Variant 0 = the designed root. The other
/// four are the alternates — two from dwl.rootcolor_candidates when present
/// (candidates equal to the designed root are skipped), else computed (25%
/// toward white + darkened), plus two brighter computed steps (50% and 70%
/// toward white) — presented in LIGHTEST-to-DARKEST order, so Ctrl+t walks a
/// predictable brightness ladder. See the design doc.
fn root_variant_color(val: &Value, designed_root: &str, variant: u8) -> String {
    if variant == 0 {
        return designed_root.to_string();
    }
    let candidates: Vec<String> = val
        .get("dwl")
        .and_then(|d| d.get("rootcolor_candidates"))
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.eq_ignore_ascii_case(designed_root))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let slot = |i: usize| -> String {
        match candidates.get(i) {
            Some(c) => c.clone(),
            None if i == 0 => blend_colors("#ffffff", designed_root, 0.25),
            None => darken_color(designed_root, 0.7),
        }
    };
    let mut pool = vec![
        slot(0),
        slot(1),
        blend_colors("#ffffff", designed_root, 0.50),
        blend_colors("#ffffff", designed_root, 0.70),
    ];
    pool.sort_by(|a, b| {
        relative_luminance(b)
            .partial_cmp(&relative_luminance(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    pool[(variant - 1) as usize].clone()
}

/// WCAG relative luminance of a hex color (0.0 = black, 1.0 = white).
fn relative_luminance(hex: &str) -> f64 {
    let (r, g, b) = hex_to_rgb(hex);
    let lin = |c: f64| if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// Resolved theme colors for linux-lit.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Theme {
    pub name: String,
    pub display_name: String,
    pub is_light: bool,
    pub root_color: String,       // outer wallpaper/padding color
    pub focus_color: String,      // dwl focused-window border color; tints reader-gloss source lines
    pub text_bg: String,          // text area background
    pub text_fg: String,          // text foreground
    pub cursor_line_bg: String,   // current line highlight
    pub phrase_highlight_bg: String, // spoken-phrase karaoke tint during narration sync
    pub dim_fg: String,           // dimmed text foreground (non-current lines)
    pub sign_fg: String,          // gutter sign color: text hue, gently dimmed (65% fg)
    pub cursor_bg: String,        // cursor indicator background
    pub cursor_fg: String,        // cursor indicator foreground
    pub vocab_fg: String,         // vocabulary word highlight foreground
    pub reader_gloss: String,        // off-cursor glossed-line tint (guarded)
    pub reader_gloss_cursor: String, // glossed line that is ALSO the cursor block
    pub overlay_panel_bg: String, // inset prose-overlay panel tint (barely-there)
    pub scrim_bg: String,         // full-bleed overlay matting (dimmed vs root_color)
    pub root_variant: u8,         // active root-color variant (0 = designed)
}

fn themes_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("utono/themes/.config/themes/themes-unified.json")
}

/// The blue selection-highlight background for a theme — the color used both for
/// the reading card's visual-selection tag AND the `<hi>` marker in the gloss/
/// synopsis/journal overlays, so a highlighted span reads the SAME in the editor
/// (visual selection) and on the page. Slightly stronger on dark themes for
/// contrast. Single source of truth so a theme change updates both at once.
pub fn selection_bg(theme: &Theme) -> &'static str {
    if theme.is_light {
        "rgba(38, 109, 211, 0.15)"
    } else {
        "rgba(68, 138, 255, 0.25)"
    }
}

/// Load all themes from themes-unified.json.
#[allow(dead_code)]
pub fn load_all_themes() -> Vec<Theme> {
    let path = themes_path();
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            crate::logging::log(&format!("Theme: failed to read {}: {}", path.display(), e));
            return vec![default_theme()];
        }
    };
    let data: Value = match serde_json::from_str(&contents) {
        Ok(d) => d,
        Err(e) => {
            crate::logging::log(&format!("Theme: failed to parse JSON: {}", e));
            return vec![default_theme()];
        }
    };
    let obj = match data.as_object() {
        Some(o) => o,
        None => return vec![default_theme()],
    };

    let mut themes = Vec::new();
    for (name, val) in obj {
        themes.push(resolve_theme(name, val));
    }
    if themes.is_empty() {
        themes.push(default_theme());
    }
    themes
}

/// Load a single theme by name.
pub fn load_theme(name: &str) -> Theme {
    let path = themes_path();
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return default_theme(),
    };
    let data: Value = match serde_json::from_str(&contents) {
        Ok(d) => d,
        Err(_) => return default_theme(),
    };
    match data.get(name) {
        Some(val) => resolve_theme(name, val),
        None => default_theme(),
    }
}

/// Load `name` from themes-unified.json; if absent fall back to the app
/// default theme name, then to the hardcoded default_theme(). linux-lit's
/// theme is independent of the system-wide .current_theme (see
/// docs/plans/2026-07-08-independent-theme-keybinds-design.md).
pub fn load_theme_with_fallback(name: &str, variant: u8) -> Theme {
    let path = themes_path();
    let data: Value = match std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
    {
        Some(d) => d,
        None => return default_theme(),
    };
    if let Some(val) = data.get(name) {
        return resolve_theme_variant(name, val, variant);
    }
    let fallback = crate::config::DEFAULT_THEME;
    match data.get(fallback) {
        Some(val) => resolve_theme_variant(fallback, val, variant),
        None => default_theme(),
    }
}

fn resolve_theme(name: &str, val: &Value) -> Theme {
    resolve_theme_variant(name, val, 0)
}

fn resolve_theme_variant(name: &str, val: &Value, variant: u8) -> Theme {
    let meta = val.get("meta").unwrap_or(&Value::Null);
    let dwl = val.get("dwl").unwrap_or(&Value::Null);
    let kitty = val.get("kitty").unwrap_or(&Value::Null);
    let nvim = val.get("nvim").unwrap_or(&Value::Null);
    let highlights = nvim.get("highlights").unwrap_or(&Value::Null);

    let is_light = meta
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("dark")
        == "light";

    let display_name = meta
        .get("display")
        .and_then(|v| v.as_str())
        .unwrap_or(name)
        .to_string();

    let text_bg = str_field(kitty, "background").unwrap_or_else(|| {
        if is_light {
            "#ffffff".to_string()
        } else {
            "#282828".to_string()
        }
    });

    let text_fg = str_field(kitty, "active_tab_foreground").unwrap_or_else(|| {
        if is_light {
            "#000000".to_string()
        } else {
            "#d4be98".to_string()
        }
    });

    let designed_root = str_field(dwl, "rootcolor").unwrap_or_else(|| {
        // Dark themes: darken kitty.background by shifting toward black
        darken_color(&text_bg, 0.6)
    });

    // Focused-window border color (dwl `focuscolor`) — reused to tint source
    // lines covered by a reader-gloss passage. Falls back to the text fg.
    let focus_color = str_field(dwl, "focuscolor").unwrap_or_else(|| text_fg.clone());

    let variant = variant % ROOT_VARIANT_COUNT;
    let root_color = root_variant_color(val, &designed_root, variant);

    let lit = val.get("linux-lit").unwrap_or(&Value::Null);
    let cursor_line_bg = str_field(lit, "cursor_line_bg")
        .unwrap_or_else(|| "rgba(86, 148, 100, 0.25)".to_string());

    // Spoken-phrase karaoke tint: optional per-theme key, else the cursor-line
    // hue at a stronger alpha so it reads inside the full-strength paragraph.
    let phrase_highlight_bg = str_field(lit, "phrase_highlight_bg").unwrap_or_else(|| {
        let (r, g, b) = rgba_str_to_rgb(&cursor_line_bg);
        format!(
            "rgba({}, {}, {}, 0.28)",
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8
        )
    });

    // Reader-gloss tints, contrast-guaranteed (raw focuscolor is dim/indistinct
    // on ~13 themes). Off-cursor = guarded focuscolor; on-cursor = guarded
    // complement, also kept distinct from the off-cursor tint.
    // Both tints are run through the readable floor even when a theme sets them
    // explicitly, so an explicit-but-washed-out value (e.g. a 3.1-contrast teal)
    // is still darkened to a readable variant of its hue rather than shipping a
    // hard-to-read gloss. Explicit values only differ from the derived ones by
    // choosing the base hue.
    let reader_gloss = {
        let base = str_field(lit, "reader_gloss").unwrap_or_else(|| focus_color.clone());
        ensure_gloss_color_min(&base, &text_bg, &[&text_fg], READER_GLOSS_MIN_CONTRAST)
    };
    let reader_gloss_cursor = {
        let base = str_field(lit, "reader_gloss_cursor")
            .unwrap_or_else(|| complement_hex(&reader_gloss));
        ensure_gloss_color_min(&base, &text_bg, &[&text_fg, &reader_gloss], READER_GLOSS_MIN_CONTRAST)
    };

    // Dim foreground: 40% fg blended toward bg (matching lit's playback sync)
    let dim_fg = blend_colors(&text_fg, &text_bg, 0.40);

    // Gutter sign color: the text hue, dimmed (45% fg / 55% bg) so the signs
    // clearly read as belonging to the reading-text family while sitting quieter
    // than the text. dim_fg's heavier 40% blend washes the hue out to a
    // near-neutral grey, which reads as a different color from the text.
    let sign_fg = blend_colors(&text_fg, &text_bg, 0.45);

    let cursor_bg = highlights
        .get("Cursor")
        .and_then(|c| str_field(c, "guibg"))
        .unwrap_or_else(|| text_fg.clone());

    let cursor_fg = highlights
        .get("Cursor")
        .and_then(|c| str_field(c, "guifg"))
        .unwrap_or_else(|| text_bg.clone());

    let vocab_orig = highlights
        .get("VocabWord")
        .and_then(|c| str_field(c, "guifg"))
        .unwrap_or_else(|| {
            if is_light { "#8a6534".to_string() } else { "#d8a657".to_string() }
        });

    let vocab_fg = if is_light {
        choose_vocab_fg(&text_fg, &cursor_bg, &vocab_orig)
    } else {
        vocab_orig
    };

    let mut theme = Theme {
        name: name.to_string(),
        display_name,
        is_light,
        root_color,
        focus_color,
        text_bg,
        text_fg,
        cursor_line_bg,
        phrase_highlight_bg,
        dim_fg,
        sign_fg,
        cursor_bg,
        cursor_fg,
        vocab_fg,
        reader_gloss,
        reader_gloss_cursor,
        overlay_panel_bg: String::new(),
        scrim_bg: String::new(),
        root_variant: variant,
    };
    theme.overlay_panel_bg = overlay_panel_bg(&theme);
    theme.scrim_bg = scrim_bg(&theme);
    theme
}

fn default_theme() -> Theme {
    let mut theme = Theme {
        name: "default".to_string(),
        display_name: "Default".to_string(),
        is_light: false,
        root_color: "#1a1a2e".to_string(),
        focus_color: "#d4be98".to_string(),
        text_bg: "#282828".to_string(),
        text_fg: "#d4be98".to_string(),
        cursor_line_bg: "rgba(255, 255, 255, 0.08)".to_string(),
        phrase_highlight_bg: "rgba(255, 255, 255, 0.22)".to_string(),
        dim_fg: blend_colors("#d4be98", "#282828", 0.40),
        sign_fg: blend_colors("#d4be98", "#282828", 0.45),
        cursor_bg: "#d4be98".to_string(),
        cursor_fg: "#282828".to_string(),
        vocab_fg: "#d8a657".to_string(),
        reader_gloss: ensure_gloss_color_min("#d4be98", "#282828", &["#d4be98"], READER_GLOSS_MIN_CONTRAST),
        reader_gloss_cursor: ensure_gloss_color_min(
            &complement_hex("#d4be98"),
            "#282828",
            &["#d4be98"],
            READER_GLOSS_MIN_CONTRAST,
        ),
        overlay_panel_bg: String::new(),
        scrim_bg: String::new(),
        root_variant: 0,
    };
    theme.overlay_panel_bg = overlay_panel_bg(&theme);
    theme.scrim_bg = scrim_bg(&theme);
    theme
}

fn str_field(val: &Value, key: &str) -> Option<String> {
    val.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty() && *s != "NONE")
        .map(|s| s.to_string())
}

/// Darken a hex color by a factor (0.0 = black, 1.0 = unchanged).
fn darken_color(hex: &str, factor: f64) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 {
        return "#1a1a2e".to_string();
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    format!(
        "#{:02x}{:02x}{:02x}",
        (r as f64 * factor) as u8,
        (g as f64 * factor) as u8,
        (b as f64 * factor) as u8,
    )
}

/// Blend two hex colors: result = fg * alpha + bg * (1 - alpha).
fn blend_colors(fg_hex: &str, bg_hex: &str, alpha: f64) -> String {
    let fg = fg_hex.trim_start_matches('#');
    let bg = bg_hex.trim_start_matches('#');
    if fg.len() < 6 || bg.len() < 6 {
        return fg_hex.to_string();
    }
    let fr = u8::from_str_radix(&fg[0..2], 16).unwrap_or(0) as f64;
    let fg_g = u8::from_str_radix(&fg[2..4], 16).unwrap_or(0) as f64;
    let fb = u8::from_str_radix(&fg[4..6], 16).unwrap_or(0) as f64;
    let br = u8::from_str_radix(&bg[0..2], 16).unwrap_or(0) as f64;
    let bg_g = u8::from_str_radix(&bg[2..4], 16).unwrap_or(0) as f64;
    let bb = u8::from_str_radix(&bg[4..6], 16).unwrap_or(0) as f64;
    format!(
        "#{:02x}{:02x}{:02x}",
        (fr * alpha + br * (1.0 - alpha)) as u8,
        (fg_g * alpha + bg_g * (1.0 - alpha)) as u8,
        (fb * alpha + bb * (1.0 - alpha)) as u8,
    )
}

/// Parse a hex color to (r, g, b) floats in [0, 1].
fn hex_to_rgb(hex: &str) -> (f64, f64, f64) {
    let h = hex.trim_start_matches('#');
    if h.len() < 6 {
        return (0.0, 0.0, 0.0);
    }
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0) as f64 / 255.0;
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0) as f64 / 255.0;
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0) as f64 / 255.0;
    (r, g, b)
}

/// Pick a foreground (near-black or near-white) that contrasts with `bg_hex`,
/// using WCAG relative luminance. Used for toasts that float over the wallpaper
/// (the dwl root color), so their text stays legible on light *and* dark
/// wallpapers. Returns slightly off-pure colors so the text reads as a soft
/// label rather than harsh pure black/white.
fn contrast_on(bg_hex: &str) -> &'static str {
    let lum = relative_luminance(bg_hex);
    if lum > 0.4 {
        "#1a1a1a" // dark text on a light wallpaper
    } else {
        "#f0f0f0" // light text on a dark wallpaper
    }
}

/// Parse the RGB channels (0.0–1.0) from an `rgba(r, g, b, a)` CSS string where
/// r/g/b are 0–255. The alpha is ignored — the caller drives alpha via the fade
/// animation. Falls back to mid-gray on a malformed string. Used so the
/// cursor-line FADE matches `cursor_line_bg`'s hue instead of the window root
/// color.
pub fn rgba_str_to_rgb(s: &str) -> (f32, f32, f32) {
    let inner = s.trim().trim_start_matches("rgba").trim_start_matches("rgb")
        .trim().trim_start_matches('(').trim_end_matches(')');
    let parts: Vec<f32> = inner
        .split(',')
        .take(3)
        .filter_map(|p| p.trim().parse::<f32>().ok())
        .collect();
    if parts.len() == 3 {
        (parts[0] / 255.0, parts[1] / 255.0, parts[2] / 255.0)
    } else {
        (0.5, 0.5, 0.5)
    }
}

/// Convert (r, g, b) floats to hex string.
fn rgb_to_hex(r: f64, g: f64, b: f64) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    )
}

/// Convert RGB to HSL. Returns (h, s, l) with h in [0,1], s in [0,1], l in [0,1].
fn rgb_to_hsl(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < 1e-10 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if (max - r).abs() < 1e-10 {
        let mut h = (g - b) / d;
        if g < b { h += 6.0; }
        h / 6.0
    } else if (max - g).abs() < 1e-10 {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, s, l)
}

/// Convert HSL to RGB.
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    if s.abs() < 1e-10 {
        return (l, l, l);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue_to_rgb = |t: f64| -> f64 {
        let t = ((t % 1.0) + 1.0) % 1.0;
        if t < 1.0 / 6.0 { p + (q - p) * 6.0 * t }
        else if t < 1.0 / 2.0 { q }
        else if t < 2.0 / 3.0 { p + (q - p) * (2.0 / 3.0 - t) * 6.0 }
        else { p }
    };
    (hue_to_rgb(h + 1.0 / 3.0), hue_to_rgb(h), hue_to_rgb(h - 1.0 / 3.0))
}

/// Hue distance in degrees [0, 180] between two hex colors.
fn hue_distance(c1: &str, c2: &str) -> f64 {
    let (h1, _, _) = rgb_to_hsl(hex_to_rgb(c1).0, hex_to_rgb(c1).1, hex_to_rgb(c1).2);
    let (h2, _, _) = rgb_to_hsl(hex_to_rgb(c2).0, hex_to_rgb(c2).1, hex_to_rgb(c2).2);
    let d = (h1 - h2).abs();
    d.min(1.0 - d) * 360.0
}

/// Return `hex` with its hue rotated 180° (the color-wheel complement), keeping
/// saturation and lightness. Malformed input degrades to the complement of black
/// (still a valid `#rrggbb`); never panics.
fn complement_hex(hex: &str) -> String {
    let (r, g, b) = hex_to_rgb(hex);
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let (nr, ng, nb) = hsl_to_rgb((h + 0.5) % 1.0, s, l);
    rgb_to_hex(nr, ng, nb)
}

/// WCAG relative-luminance contrast ratio between two hex colors, 1.0 (identical)
/// to 21.0 (black on white). Used to keep the gloss tints legible on the reading
/// background and distinct from body text.
fn contrast_ratio(a_hex: &str, b_hex: &str) -> f64 {
    let lin = |c: f64| if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    let lum = |hex: &str| {
        let (r, g, b) = hex_to_rgb(hex);
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    };
    let (la, lb) = (lum(a_hex) + 0.05, lum(b_hex) + 0.05);
    if la > lb { la / lb } else { lb / la }
}

/// Choose a vocab foreground color that is visually distinct from text_fg.
/// Picks the best candidate from vocab_orig and cursor_bg, or derives one
/// by rotating text_fg hue by 150 degrees.
fn choose_vocab_fg(text_fg: &str, cursor_bg: &str, vocab_orig: &str) -> String {
    let min_distance = 50.0;
    let vocab_dist = hue_distance(text_fg, vocab_orig);
    let cursor_dist = hue_distance(text_fg, cursor_bg);

    // Pick whichever candidate has more hue distance
    if vocab_dist >= cursor_dist && vocab_dist > min_distance {
        return vocab_orig.to_string();
    }
    if cursor_dist > min_distance {
        return cursor_bg.to_string();
    }

    // Neither is distinct enough — derive by rotating text_fg hue
    let (r, g, b) = hex_to_rgb(text_fg);
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let new_h = (h + 150.0 / 360.0) % 1.0;
    let new_s = s.max(0.45);
    let new_l = l.clamp(0.30, 0.45);
    let (r2, g2, b2) = hsl_to_rgb(new_h, new_s, new_l);
    rgb_to_hex(r2, g2, b2)
}

/// Return a color at `base_hex`'s hue that is legible on `bg_hex` (WCAG contrast
/// ≥ 3.0) and visually distinct (hue distance ≥ 40° OR contrast ≥ 1.4) from each
/// color in `avoid`. If `base_hex` already qualifies it is returned unchanged, so
/// themes that already look right do not move. Otherwise lightness is pushed away
/// from the background and saturation raised at the same hue; as a last resort the
/// hue is rotated 150° (the `choose_vocab_fg` strategy) and S/L clamped. Used to
/// derive both reader-gloss tints so they never wash out or blend into body text.
/// Resolve a gloss-tint color with a caller-chosen minimum contrast floor vs the
/// background. The reader-gloss tint uses a higher floor (READER_GLOSS_MIN_
/// CONTRAST) so it renders as a DARKER, clearly readable variant of the hue
/// rather than a washed-out near-3.0 pastel that's hard to read on the cream
/// reading surface. `min_contrast = 3.0` is the plain UI floor (used by tests /
/// any future non-body-text caller).
fn ensure_gloss_color_min(base_hex: &str, bg_hex: &str, avoid: &[&str], min_contrast: f64) -> String {
    let ok = |c: &str| {
        contrast_ratio(c, bg_hex) >= min_contrast
            && avoid.iter().all(|a| hue_distance(c, a) >= 40.0 || contrast_ratio(c, a) >= 1.4)
    };
    if ok(base_hex) {
        return base_hex.to_string();
    }
    let (br, bg_, bb) = hex_to_rgb(base_hex);
    let (h, s, _l) = rgb_to_hsl(br, bg_, bb);
    let bg_is_light = contrast_ratio(bg_hex, "#000000") > contrast_ratio(bg_hex, "#ffffff");
    // Push lightness toward the side with headroom against the bg; raise S.
    let s2 = s.max(0.50);
    for &l in if bg_is_light {
        &[0.42_f64, 0.36, 0.30, 0.24][..]   // darker, for a light bg
    } else {
        &[0.62_f64, 0.68, 0.74, 0.80][..]   // lighter, for a dark bg
    } {
        let (r, g, b) = hsl_to_rgb(h, s2, l);
        let cand = rgb_to_hex(r, g, b);
        if ok(&cand) {
            return cand;
        }
    }
    // Last resort: rotate the hue away from the base, sweeping lightness rungs
    // at each rotation for one that satisfies `ok`. A single fixed rotation is
    // not enough: the cursor tint's base is the COMPLEMENT of reader_gloss, so
    // +150° from it lands only 30° from reader_gloss's hue — on an all-warm
    // palette (kindle-sepia) that shipped a non-distinct pair. +150° stays the
    // first try (matches choose_vocab_fg; existing themes keep their colors),
    // then widening alternates. Falls back to the highest-contrast-vs-bg
    // candidate if (theoretically) no rotation satisfies the full guard.
    let s2 = s.max(0.50);
    let rungs: &[f64] = if bg_is_light {
        &[0.36, 0.30, 0.24, 0.42, 0.18]
    } else {
        &[0.70, 0.76, 0.64, 0.82, 0.58]
    };
    let mut best = rgb_to_hex(0.0, 0.0, 0.0);
    let mut best_c = -1.0;
    for rot in [150.0_f64, 210.0, 120.0, 240.0, 90.0, 270.0] {
        let new_h = (h + rot / 360.0) % 1.0;
        for &l in rungs {
            let (r, g, b) = hsl_to_rgb(new_h, s2, l);
            let cand = rgb_to_hex(r, g, b);
            if ok(&cand) {
                return cand;
            }
            let c = contrast_ratio(&cand, bg_hex);
            if c > best_c {
                best_c = c;
                best = cand;
            }
        }
    }
    best
}

/// Background color for the gloss/synopsis overlay card. A pure-white reading
/// surface is harsh under a long gloss, so on light themes we warm `text_bg`
/// toward a paper cream. Dark themes already have a soft, non-garish `text_bg`,
/// so they are left untouched.
fn gloss_background(theme: &Theme) -> String {
    // The overlays (gloss/synopsis/journal/translation + their ask cards) paint
    // the SAME paper as the main reading card: the theme's `text_bg`. Light
    // themes used to substitute a fixed Rosé Pine Dawn off-white (#faf4ed),
    // which made every overlay visibly mismatch the card on any other light
    // theme (user request: overlay theme must match the main card's theme).
    theme.text_bg.clone()
}

/// The inset-panel fill for the prose overlays. It now paints the card's own
/// `gloss_bg` cream — i.e. NO tint — so the prose column reads as one flat
/// surface. Framing is done entirely by the dimmed `scrim_bg` matting around the
/// card (see `scrim_bg`); an additional panel tint here produced a competing
/// second box inside the frame. The `panel_drawing` plumbing is retained (the
/// overlay's measured main child, z-order below the accent bar, bottom-clip
/// guard) — it simply fills invisibly. To reintroduce a subtle inset later,
/// shift this a few percent off `gloss_bg` again (light darkens, dark lightens).
fn overlay_panel_bg(theme: &Theme) -> String {
    gloss_background(theme)
}

/// Matting color for the full-bleed overlay scrim (gloss/journal/synopsis/
/// translation/echo-keybinds). A subtle (~20%) darkening of `root_color` so the
/// border area around an overlay card reads as an intentional dimmed frame
/// rather than the identical, un-dimmed window background. Stays opaque (the
/// main reading card is fully hidden behind the overlay), same hue as root, and
/// tracks every theme automatically.
fn scrim_bg(theme: &Theme) -> String {
    darken_color(&theme.root_color, 0.80)
}

/// Scale the alpha of an `rgba(r, g, b, a)` string by `factor`. Any other
/// color form (hex, named) passes through unchanged — the caller then reuses
/// the undimmed color, which is a safe worst case (tint too strong, never
/// wrong-colored or invalid CSS).
pub fn dim_rgba_alpha(color: &str, factor: f64) -> String {
    let inner = color
        .trim()
        .strip_prefix("rgba(")
        .and_then(|r| r.strip_suffix(')'));
    if let Some(inner) = inner {
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
        if parts.len() == 4 {
            if let Ok(a) = parts[3].parse::<f64>() {
                return format!(
                    "rgba({}, {}, {}, {:.3})",
                    parts[0],
                    parts[1],
                    parts[2],
                    a * factor
                );
            }
        }
    }
    color.to_string()
}

impl Theme {
    /// Sentence-extent tint for the vocab-sentence loop: the karaoke sweep
    /// color at ~45% of its alpha, so the moving sweep stays readable inside
    /// the static sentence marker.
    pub fn vocab_sentence_bg(&self) -> String {
        dim_rgba_alpha(&self.phrase_highlight_bg, 0.45)
    }
}

/// Generate GTK CSS for a theme.
pub fn generate_css(theme: &Theme, font_family: &str, font_size: u32) -> String {
    let css = format!(
        "window {{ background-color: {root}; }} \
         .tiled {{ background-color: {bg}; }} \
         .page-turn-overlay {{ background-color: {bg}; border-radius: 12px; }} \
         .page-image-overlay {{ background-color: {bg}; border-radius: 12px; }} \
         .page-image-caption {{ color: {fg}; font-size: 12px; \
           background-color: {bg}; padding: 4px 14px; border-radius: 10px; \
           border: 1px solid {dim}; }} \
         .card-top {{ background-color: {bg}; border-radius: 12px 12px 0 0; }} \
         .card-middle {{ background-color: {bg}; border-radius: 0; }} \
         .card-bottom {{ background-color: {bg}; border-radius: 0 0 12px 12px; }} \
         .column-divider {{ background-color: {dim}; min-width: 1px; \
           margin: 24px 8px; opacity: 0.28; }} \
         textview {{ background-color: {bg}; color: {fg}; }} \
         textview border {{ background-color: {bg}; }} \
         textview border.left {{ background-color: {bg}; }} \
         textview border * {{ background-color: {bg}; background: {bg}; }} \
         textview text {{ background-color: {bg}; color: {fg}; \
           font-family: {font}; font-size: {size}pt; }} \
         .library-picker {{ background-color: {bg}; color: {fg}; \
           padding: 0; border-radius: 12px; border: 1px solid {dim}; \
           box-shadow: 0 18px 48px rgba(0, 0, 0, 0.22), \
                       0 2px 6px rgba(0, 0, 0, 0.08); }} \
         .library-picker-header {{ padding: 14px 22px 10px; \
           border-bottom: 1px solid {header_border}; }} \
         .library-picker-title {{ font-size: 14px; font-weight: 700; \
           letter-spacing: 2px; color: {fg}; opacity: 0.75; }} \
         .library-picker-crumb {{ font-size: 13px; color: {fg}; \
           opacity: 0.65; }} \
         .library-picker entry {{ margin: 12px 18px 8px; \
           padding: 8px 12px; border: 1px solid {dim}; \
           border-radius: 8px; background-color: {bg}; color: {fg}; }} \
         .library-picker entry:focus {{ \
           box-shadow: 0 0 0 3px {focus_ring}; }} \
         .library-picker scrolledwindow {{ padding: 4px 8px 10px; }} \
         .library-picker row {{ padding: 8px 14px; \
           border-radius: 6px; }} \
         .library-picker row label.picker-item-detail {{ \
           font-variant-numeric: tabular-nums; min-width: 32px; \
           font-size: 15px; color: {fg}; opacity: 0.7; }} \
         .library-picker row:selected {{ \
           background-color: {picker_selection_bg}; color: {cursor_fg}; }} \
         .library-picker row:selected label.picker-item-detail {{ \
           color: {cursor_fg}; opacity: 1.0; }} \
         .library-picker-footer {{ padding: 8px 22px 12px; \
           border-top: 1px solid {header_border}; \
           font-size: 12px; letter-spacing: 1.2px; \
           color: {fg}; opacity: 0.65; }} \
         .library-picker-scrim {{ background-color: rgba(0, 0, 0, 0.3); }} \
         .search-bar {{ background-color: {bg}; color: {fg}; padding: 4px 12px; }} \
         .search-entry {{ background: transparent; border: none; color: {fg}; }} \
         .search-slash {{ color: {fg}; opacity: 0.6; }} \
         .search-counter {{ color: {fg}; opacity: 0.6; }} \
         .settings-title {{ font-size: 18px; font-weight: bold; \
           margin-bottom: 12px; padding-bottom: 12px; \
           border-bottom: 1px solid rgba(255,255,255,0.2); }} \
         .settings-row {{ padding: 8px 12px; margin: 2px 0; border-radius: 4px; }} \
         .settings-row-selected {{ background-color: rgba(100, 140, 200, 0.8); \
           border-left: 3px solid rgba(100, 180, 255, 0.9); }} \
         .settings-row-disabled {{ opacity: 0.35; }} \
         .settings-footer {{ font-size: 11px; opacity: 0.6; margin-top: 12px; }} \
         .action-popup {{ background-color: {bg}; color: {fg}; \
           padding: 16px; border-radius: 12px; border: 1px solid {dim}; }} \
         .action-popup .settings-title {{ border-bottom: 1px solid {dim}; }} \
         .action-popup .settings-row-selected {{ background-color: {cursor_bg}; \
           color: {cursor_fg}; border-left: 3px solid {cursor_bg}; }} \
         .action-popup .settings-footer {{ color: {dim}; }} \
         .action-separator {{ color: {dim}; opacity: 0.3; margin: 4px 12px; }} \
         .amend-dialog {{ background-color: {bg}; color: {fg}; \
           padding: 0; border-radius: 12px; border: 1px solid {dim}; \
           box-shadow: 0 18px 48px rgba(0, 0, 0, 0.22), \
                       0 2px 6px rgba(0, 0, 0, 0.08); }} \
         .amend-title {{ font-size: 14px; font-weight: 700; \
           letter-spacing: 2px; color: {fg}; opacity: 0.75; \
           padding: 14px 22px 10px; \
           border-bottom: 1px solid {header_border}; }} \
         .amend-text {{ font-family: {font}; font-size: {size}pt; \
           color: {fg}; background-color: {bg}; }} \
         .amend-hint {{ font-size: 12px; letter-spacing: 1.2px; \
           color: {fg}; opacity: 0.65; \
           padding: 8px 22px 12px; \
           border-top: 1px solid {header_border}; }} \
         .keybinds-overlay {{ background-color: rgba(26, 26, 26, 0.95); color: white; \
           padding: 20px; border-radius: 10px; }} \
         .kb-row {{ }} \
         .kb-key {{ background-color: #2a2a2a; border: 1px solid #444444; \
           border-radius: 5px; padding: 3px 5px; min-height: 42px; }} \
         .kb-key-bound {{ background-color: #1a3a2a; border-color: #3a6a4a; }} \
         .kb-key-bound-shift {{ background-color: #1a2a3a; border-color: #3a4a6a; }} \
         .kb-key-bound-both {{ background-color: #1a3a2a; border-color: #3a6a4a; }} \
         .kb-key-unbound {{ opacity: 0.5; }} \
         .kb-char {{ font-size: 22px; font-weight: bold; color: #888888; }} \
         .kb-char-bound {{ color: #88ff88; }} \
         .kb-char-shift {{ color: #88aaff; }} \
         .kb-char-both {{ color: #88ff88; }} \
         .kb-shifted {{ font-size: 14px; color: #666666; }} \
         .kb-shifted-active {{ color: #6688cc; }} \
         .kb-action {{ font-size: 12px; color: #66cc66; }} \
         .kb-shift-action {{ font-size: 11px; color: #6688cc; }} \
         .kb-arrow {{ background-color: #2a2a2a; border: 1px solid #444444; \
           border-radius: 4px; padding: 2px 4px; min-width: 38px; min-height: 36px; }} \
         .kb-arrow-bound {{ background-color: #1a3a2a; border-color: #3a6a4a; }} \
         .kb-arrow-bound-both {{ background-color: #1a3a2a; border-color: #3a6a4a; }} \
         .kb-arrow-char {{ font-size: 18px; color: #88ff88; }} \
         .kb-arrow-action {{ font-size: 10px; color: #66cc66; }} \
         .kb-legend {{ border-top: 1px solid rgba(255, 255, 255, 0.1); \
           margin-top: 12px; padding-top: 8px; }} \
         .kb-legend-swatch {{ min-width: 14px; min-height: 14px; \
           border-radius: 3px; border: 1px solid #555555; }} \
         .kb-legend-bound {{ background-color: #1a3a2a; border-color: #3a6a4a; }} \
         .kb-legend-shift {{ background-color: #1a2a3a; border-color: #3a4a6a; }} \
         .kb-legend-both {{ background-color: #1a3a2a; border-color: #3a6a4a; }} \
         .kb-legend-unbound {{ background-color: #2a2a2a; border-color: #444444; }} \
         .debug-icon {{ font-size: 18px; color: {bg}; opacity: 0.85; }} \
         .word-status {{ font-size: 16px; color: {fg}; opacity: 0.85; }} \
         .chapter-toast {{ font-size: 13px; color: {toast_fg}; \
           background-color: {root}; padding: 6px 14px; border-radius: 10px; \
           box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4); opacity: 0.97; }} \
         .search-toast {{ font-size: 10px; color: {toast_fg}; \
           background-color: {root}; padding: 3px 10px; border-radius: 8px; \
           box-shadow: 0 3px 12px rgba(0, 0, 0, 0.35); opacity: 0.95; }} \
         .gloss-scrim {{ background-color: {scrim}; }} \
         .legend-scrim {{ background-color: rgba(0, 0, 0, 0.3); }} \
         .gloss-overlay {{ background-color: {gloss_bg}; color: {fg}; border-radius: 12px; \
           box-shadow: 0 8px 32px rgba(0, 0, 0, 0.45); }} \
         .gloss-bottom-clip {{ background-color: {gloss_bg}; }} \
         textview.gloss-text {{ background-color: {gloss_bg}; }} \
         textview.gloss-text text {{ background-color: {gloss_bg}; }} \
         textview.overlay-prose {{ background-color: transparent; color: {fg}; }} \
         textview.overlay-prose text {{ background-color: transparent; color: {fg}; }} \
         .gloss-title {{ font-size: {size}pt; font-weight: bold; \
           margin-bottom: 12px; padding-bottom: 12px; \
           border-bottom: 1px solid {dim}; }} \
         .journal-title {{ font-size: 13px; font-weight: normal; \
           color: {dim}; \
           margin-bottom: 12px; padding-bottom: 10px; \
           border-bottom: 1px solid {dim}; }} \
         .synopsis-header {{ font-size: 16px; font-weight: normal; \
           color: {dim}; \
           margin-bottom: 12px; padding-bottom: 10px; \
           border-bottom: 1px solid {dim}; }} \
         .gloss-header {{ font-size: 11px; font-weight: bold; \
           color: {dim}; letter-spacing: 2px; margin-top: 8px; margin-bottom: 4px; }} \
         .gloss-text {{ font-family: {font}; font-size: {size}pt; }} \
         textview.translation-col text {{ color: {dim}; font-style: italic; }} \
         textview.translation-col {{ background-color: {gloss_bg}; }} \
         .gloss-hint {{ font-size: 14px; \
           color: {dim}; padding-top: 8px; \
           border-top: 1px solid {dim}; }} \
         .gloss-position {{ font-size: 14px; color: {dim}; }} \
         .ask-card {{ background-color: alpha({gloss_bg}, 0.82); border-radius: 10px; \
           border: 1px solid {header_border}; transition: opacity 120ms ease; }} \
         .ask-card .gloss-header {{ margin-top: 0; }} \
         textview.ask-input {{ background-color: transparent; color: {fg}; }} \
         textview.ask-input text {{ background-color: transparent; }} \
         .ask-hint {{ font-size: 13px; color: {dim}; }} \
         .ask-card.card-focused {{ border-color: {cursor_bg}; \
           border-left: 4px solid {cursor_bg}; \
           box-shadow: 0 8px 24px rgba(0, 0, 0, 0.30); }} \
         .ask-card.card-dimmed {{ opacity: 0.55; }} \
         .definition-panel {{ background-color: {bg}; color: {fg}; \
           border-radius: 12px; padding: 20px 24px; }} \
         .vocab-popup {{ background-color: {root}; color: {bg}; \
           padding: 16px 20px; border-radius: 12px; }} \
         .vocab-popup .definition-header {{ font-size: 11px; color: {vocab_popup_dim}; \
           letter-spacing: 2px; font-weight: bold; }} \
         .vocab-popup .definition-word {{ font-size: 16px; color: {vocab_popup_fg}; }} \
         .vocab-popup .definition-text {{ font-size: 16px; color: {vocab_popup_fg}; }} \
         .vocab-popup .definition-etymology {{ opacity: 0.7; font-size: 12px; color: {vocab_popup_fg}; }} \
         .vocab-popup .definition-gloss {{ opacity: 0.7; font-size: 12px; color: {vocab_popup_fg}; }} \
         .vocab-popup .definition-hint {{ font-size: 11px; color: {vocab_popup_dim}; \
           border-top: 1px solid {vocab_popup_border}; padding-top: 8px; margin-top: 12px; }} \
         .concordance-picker {{ background-color: {bg}; color: {fg}; \
           padding: 16px; border-radius: 12px; border: 1px solid {dim}; }} \
         .concordance-picker entry {{ margin-bottom: 8px; }} \
         .concordance-picker row:selected {{ background-color: {cursor_bg}; color: {cursor_fg}; }} \
         .concordance-picker .settings-title {{ border-bottom: 1px solid {dim}; }} \
         .concordance-picker .settings-footer {{ color: {dim}; }} \
         .concordance-bar {{ background-color: {root}; padding: 4px 12px; }} \
         .concordance-bar-word {{ color: {dim}; font-size: 12px; }} \
         .concordance-bar-position {{ color: {dim}; font-size: 14px; }} \
         .concordance-bar-hint {{ color: {dim}; font-size: 12px; opacity: 0.6; }} \
         .title-bar {{ background-color: {root}; padding: 4px 12px; }} \
         .title-bar-label {{ color: {dim}; font-size: 14px; }} \
         .title-bar-hint {{ color: {dim}; font-size: 12px; opacity: 0.6; }} \
         .chat-panel {{ background-color: transparent; }} \
         .chat-panel-header {{ color: {dim}; font-variant: small-caps; font-size: 13px; }} \
         .chat-panel-rule {{ background-color: alpha({fg}, 0.25); }} \
         .chat-q {{ color: {fg}; }} \
         .chat-a {{ color: alpha({fg}, 0.72); }} \
         .chat-chip {{ color: {dim}; font-style: italic; \
           border-left: 2px solid alpha({fg}, 0.35); padding-left: 8px; }} \
         .chat-error {{ color: alpha({fg}, 0.55); font-style: italic; }} \
         .chat-saved {{ color: {dim}; }} \
         .chat-input {{ border: 1px solid alpha({fg}, 0.30); border-radius: 6px; }} \
         .picker-box {{ background-color: rgba(40, 40, 40, 0.95); color: white; \
           padding: 16px; border-radius: 8px; }} \
         .legend-box {{ background-color: {bg}; color: {fg}; \
           padding: 32px 40px; border-radius: 12px; border: 1px solid {dim}; \
           box-shadow: 0 18px 48px rgba(0, 0, 0, 0.22), \
                       0 2px 6px rgba(0, 0, 0, 0.08); }} \
         .legend-title {{ font-family: {font}; font-size: 20px; font-weight: 700; \
           letter-spacing: 2px; color: {fg}; opacity: 0.78; }} \
         .legend-group {{ font-family: {font}; font-size: 12px; font-weight: 700; \
           letter-spacing: 2px; text-transform: uppercase; color: {dim}; }} \
         .legend-rule {{ background-color: {dim}; opacity: 0.25; min-height: 1px; \
           margin: 2px 0; }} \
         .legend-key {{ font-family: monospace; font-size: 15px; \
           color: {cursor_bg}; padding: 2px 0; }} \
         .legend-action {{ font-family: {font}; font-size: 16px; color: {fg}; \
           opacity: 0.72; padding: 2px 0; }} \
         .picker-entry {{ margin-bottom: 8px; }} \
         .picker-list row:selected {{ background-color: rgba(100, 140, 200, 0.8); }} \
         .picker-item-title {{ }} \
         .picker-item-detail {{ opacity: 0.6; }} \
         .picker-header {{ font-size: 14px; font-weight: bold; }} \
",
        root = theme.root_color,
        scrim = theme.scrim_bg,
        bg = theme.text_bg,
        gloss_bg = gloss_background(theme),
        fg = theme.text_fg,
        dim = theme.dim_fg,
        toast_fg = contrast_on(&theme.root_color),
        cursor_bg = theme.cursor_bg,
        cursor_fg = theme.cursor_fg,
        vocab_popup_fg = blend_colors(&theme.text_bg, &theme.root_color, 0.60),
        vocab_popup_dim = blend_colors(&theme.text_bg, &theme.root_color, 0.45),
        vocab_popup_border = blend_colors(&theme.text_bg, &theme.root_color, 0.25),
        focus_ring = blend_colors(&theme.cursor_bg, &theme.text_bg, 0.4),
        picker_selection_bg = blend_colors(&theme.cursor_bg, &theme.text_bg, 0.5),
        header_border = blend_colors(&theme.dim_fg, &theme.text_bg, 0.5),
        font = font_family,
        size = font_size,
    );
    // Diagnostic knob: LIT_DEBUG_CLIP_COLOR=<css color> paints every bottom-clip
    // box (main card + overlays) that color for the run, so a clip edge that is
    // normally invisible (card bg on card bg) can be seen and pixel-measured in a
    // screenshot. Replaces the recurring hand-edit of `.card-bottom` to #ff0000
    // that clip-prevention.md prescribed. Debug-only: unset means zero change.
    match std::env::var("LIT_DEBUG_CLIP_COLOR") {
        Ok(color) if !color.trim().is_empty() => format!(
            "{css} .card-bottom {{ background-color: {color}; }} \
             .gloss-bottom-clip {{ background-color: {color}; }}"
        ),
        _ => css,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complement_rotates_hue_180() {
        let c = complement_hex("#c4788a");
        let (h_in, _, _) = rgb_to_hsl(hex_to_rgb("#c4788a").0, hex_to_rgb("#c4788a").1, hex_to_rgb("#c4788a").2);
        let (h_out, _, _) = rgb_to_hsl(hex_to_rgb(&c).0, hex_to_rgb(&c).1, hex_to_rgb(&c).2);
        let diff = ((h_out - h_in).abs() - 0.5).abs();
        assert!(diff < 0.02, "expected ~0.5 hue rotation, in={h_in} out={h_out} ({c})");
        assert!((0.33..=0.70).contains(&h_out), "complement of a red should be teal/green, got {h_out} ({c})");
    }

    #[test]
    fn complement_malformed_is_safe() {
        let c = complement_hex("nope");
        assert!(c.starts_with('#') && c.len() == 7, "got {c}");
    }

    #[test]
    fn contrast_ratio_known_pairs() {
        assert!((contrast_ratio("#ffffff", "#000000") - 21.0).abs() < 0.1);
        assert!((contrast_ratio("#888888", "#888888") - 1.0).abs() < 0.01);
        // a mid case is between
        let c = contrast_ratio("#c4788a", "#faf4ed");
        assert!(c > 2.5 && c < 3.5, "rose on cream ~3.0, got {c}");
    }

    #[test]
    fn ensure_keeps_already_good_color() {
        // rose-pine-dawn focuscolor on cream, avoiding slate body text: passes
        // the plain 3.0 UI floor, so returned unchanged there.
        let c = ensure_gloss_color_min("#c4788a", "#faf4ed", &["#575279"], 3.0);
        assert_eq!(c, "#c4788a", "a color that already passes the 3.0 floor must be returned unchanged");
    }

    #[test]
    fn reader_gloss_floor_darkens_a_washed_out_pastel() {
        // The user's case: #c4788a on cream is exactly ~3.0 — readable as UI but
        // washed out as body text. The reader-gloss floor must push it DARKER
        // (higher contrast) while keeping it distinct from the slate body text.
        let bg = "#faf4ed";
        let before = contrast_ratio("#c4788a", bg);
        let c = ensure_gloss_color_min("#c4788a", bg, &["#575279"], READER_GLOSS_MIN_CONTRAST);
        let after = contrast_ratio(&c, bg);
        assert!(after >= READER_GLOSS_MIN_CONTRAST,
            "reader-gloss tint {c} must clear the readable floor, got {after:.2}");
        assert!(after > before,
            "reader-gloss tint must be DARKER than the raw pastel ({after:.2} > {before:.2})");
        assert_ne!(c, "#c4788a", "the washed-out pastel must not survive the readable floor");
    }

    #[test]
    fn ensure_fixes_dim_color_on_light_bg() {
        // dayfox: a muted purple focuscolor on a near-white bg is too dim.
        let c = ensure_gloss_color_min("#7b6b99", "#f6f2ee", &["#3d2b5a"], 3.0);
        assert!(contrast_ratio(&c, "#f6f2ee") >= 3.0,
            "fixed color must contrast with bg, got {} ({c})", contrast_ratio(&c, "#f6f2ee"));
    }

    #[test]
    fn ensure_result_is_distinct_from_avoid() {
        let c = ensure_gloss_color_min("#7b6b99", "#f6f2ee", &["#3d2b5a"], 3.0);
        let distinct = hue_distance(&c, "#3d2b5a") >= 40.0 || contrast_ratio(&c, "#3d2b5a") >= 1.4;
        assert!(distinct, "result {c} must be distinct from the avoid color");
    }

    #[test]
    fn reader_gloss_tints_enforce_readable_floor_even_when_explicit() {
        // An explicit reader_gloss_cursor (#56949f, ~3.1 contrast on cream) sets
        // the HUE but is still darkened to the readable floor; the off-cursor tint
        // derives from the focuscolor and is likewise darkened. Neither ships the
        // washed-out raw value.
        let json: serde_json::Value = serde_json::from_str(
            r##"{ "dwl": {"focuscolor": "#c4788a"},
                 "linux-lit": {"reader_gloss_cursor": "#56949f"},
                 "kitty": {"background": "#faf4ed", "active_tab_foreground": "#575279"} }"##,
        ).unwrap();
        let t = resolve_theme("rose-pine-dawn", &json);
        let bg = "#faf4ed";
        assert!(contrast_ratio(&t.reader_gloss_cursor, bg) >= READER_GLOSS_MIN_CONTRAST,
            "explicit cursor tint must be darkened to the readable floor, got {} ({:.2})",
            t.reader_gloss_cursor, contrast_ratio(&t.reader_gloss_cursor, bg));
        assert!(contrast_ratio(&t.reader_gloss, bg) >= READER_GLOSS_MIN_CONTRAST,
            "off-cursor tint must clear the readable floor, got {} ({:.2})",
            t.reader_gloss, contrast_ratio(&t.reader_gloss, bg));
        // The raw washed-out inputs must NOT survive.
        assert_ne!(t.reader_gloss, "#c4788a");
        assert_ne!(t.reader_gloss_cursor, "#56949f");
    }

    #[test]
    fn gloss_tints_distinct_on_all_warm_palette() {
        // kindle-sepia's palette (brown focuscolor, brown text, sepia bg)
        // excludes the whole warm range, so BOTH tints reach the last-resort
        // hue rotation. The cursor tint's base is the complement of
        // reader_gloss, so a single fixed +150° rotation lands ~30° from
        // reader_gloss's hue and used to ship a non-distinct fallback pair.
        let json: serde_json::Value = serde_json::from_str(
            r##"{ "meta": {"type": "light"},
                 "dwl": {"focuscolor": "#8a6a45"},
                 "kitty": {"background": "#e7dec7", "active_tab_foreground": "#5d4232"} }"##,
        ).unwrap();
        let t = resolve_theme("kindle-sepia", &json);
        assert!(contrast_ratio(&t.reader_gloss, "#e7dec7") >= READER_GLOSS_MIN_CONTRAST,
            "off tint {} dim on sepia bg", t.reader_gloss);
        assert!(contrast_ratio(&t.reader_gloss_cursor, "#e7dec7") >= READER_GLOSS_MIN_CONTRAST,
            "cursor tint {} dim on sepia bg", t.reader_gloss_cursor);
        let distinct = hue_distance(&t.reader_gloss, &t.reader_gloss_cursor) >= 40.0
            || contrast_ratio(&t.reader_gloss, &t.reader_gloss_cursor) >= 1.4;
        assert!(distinct, "off {} and cursor {} not distinct",
            t.reader_gloss, t.reader_gloss_cursor);
    }

    #[test]
    fn reader_gloss_colors_are_legible_and_distinct_for_all_themes() {
        // The audit invariant: every shipped theme yields a legible, mutually
        // distinct pair. Guards against a future theme regressing.
        for t in load_all_themes() {
            let cvb_off = contrast_ratio(&t.reader_gloss, &t.text_bg);
            let cvb_cur = contrast_ratio(&t.reader_gloss_cursor, &t.text_bg);
            // Reader-gloss tints now use the higher readable floor (a darker
            // variant of the hue), not the old 3.0 UI floor.
            assert!(cvb_off >= READER_GLOSS_MIN_CONTRAST, "{}: off-cursor tint {} dim on bg {} ({cvb_off:.2})", t.name, t.reader_gloss, t.text_bg);
            assert!(cvb_cur >= READER_GLOSS_MIN_CONTRAST, "{}: on-cursor color {} dim on bg {} ({cvb_cur:.2})", t.name, t.reader_gloss_cursor, t.text_bg);
            let distinct = hue_distance(&t.reader_gloss, &t.reader_gloss_cursor) >= 40.0
                || contrast_ratio(&t.reader_gloss, &t.reader_gloss_cursor) >= 1.4;
            assert!(distinct, "{}: off {} and on {} not distinct", t.name, t.reader_gloss, t.reader_gloss_cursor);
        }
    }

    #[test]
    fn overlay_panel_bg_matches_the_card_background() {
        // The inset panel now paints the card's own cream (no tint) so it reads
        // as one flat surface — the dimmed scrim matting is the only frame.
        let light = Theme {
            is_light: true,
            text_bg: "#fbf1c7".to_string(),
            ..default_theme()
        };
        let dark = Theme {
            is_light: false,
            text_bg: "#282828".to_string(),
            ..default_theme()
        };

        for theme in [&light, &dark] {
            let gloss_bg = gloss_background(theme);
            let panel = overlay_panel_bg(theme);
            assert_eq!(
                panel, gloss_bg,
                "panel must equal gloss_bg (no competing inner box), is_light={}",
                theme.is_light
            );
        }
    }

    #[test]
    fn dim_rgba_alpha_scales_only_the_alpha() {
        assert_eq!(
            dim_rgba_alpha("rgba(255, 255, 255, 0.22)", 0.45),
            "rgba(255, 255, 255, 0.099)"
        );
        // Non-rgba strings pass through untouched.
        assert_eq!(dim_rgba_alpha("#ffcc00", 0.45), "#ffcc00");
    }

    const CANDIDATES_JSON: &str = r##"{ "meta": {"display": "S", "type": "light"},
        "dwl": {"rootcolor": "#08526b", "focuscolor": "#8a6a45",
                "rootcolor_candidates": ["#41819b", "#286983", "#08526b"]},
        "kitty": {"background": "#e7dec7", "active_tab_foreground": "#5d4232"},
        "linux-lit": {"cursor_line_bg": "rgba(93, 66, 50, 0.14)"} }"##;

    #[test]
    fn variant_zero_is_identical_to_base_resolution() {
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let base = resolve_theme("s", &json);
        let v0 = resolve_theme_variant("s", &json, 0);
        assert_eq!(base.root_color, v0.root_color);
        assert_eq!(base.text_bg, v0.text_bg);
        assert_eq!(base.cursor_line_bg, v0.cursor_line_bg);
        assert_eq!(base.phrase_highlight_bg, v0.phrase_highlight_bg);
        assert_eq!(base.scrim_bg, v0.scrim_bg);
        assert_eq!(base.dim_fg, v0.dim_fg);
        assert_eq!(v0.root_variant, 0);
    }

    #[test]
    fn candidates_and_brighter_sorted_lightest_to_darkest() {
        // Pool: candidates #41819b/#286983 (designed #08526b skipped) + the
        // 50%/70%-toward-white blends (#83a8b5/#b4cbd2), sorted light->dark.
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let v0 = resolve_theme_variant("s", &json, 0);
        assert_eq!(v0.root_color, "#08526b"); // designed stays slot 0
        let roots: Vec<String> = (1..=4)
            .map(|i| resolve_theme_variant("s", &json, i).root_color)
            .collect();
        assert_eq!(roots[0], blend_colors("#ffffff", "#08526b", 0.70));
        assert_eq!(roots[1], blend_colors("#ffffff", "#08526b", 0.50));
        assert_eq!(roots[2], "#41819b");
        assert_eq!(roots[3], "#286983");
        for w in roots.windows(2) {
            assert!(relative_luminance(&w[0]) >= relative_luminance(&w[1]));
        }
    }

    #[test]
    fn computed_fallback_without_candidates() {
        let json: serde_json::Value = serde_json::from_str(
            r##"{ "meta": {"type": "light"},
                  "dwl": {"rootcolor": "#08526b"},
                  "kitty": {"background": "#e7dec7", "active_tab_foreground": "#5d4232"} }"##)
            .unwrap();
        let v0 = resolve_theme_variant("s", &json, 0);
        let roots: Vec<String> = (1..=4)
            .map(|i| resolve_theme_variant("s", &json, i).root_color)
            .collect();
        // Pool = 25%-lighter, darkened, 50%, 70% — sorted lightest->darkest.
        assert_eq!(roots[0], blend_colors("#ffffff", "#08526b", 0.70));
        assert_eq!(roots[1], blend_colors("#ffffff", "#08526b", 0.50));
        assert_eq!(roots[2], blend_colors("#ffffff", "#08526b", 0.25));
        assert_eq!(roots[3], darken_color("#08526b", 0.7));
        for r in &roots {
            assert_ne!(*r, v0.root_color);
        }
    }

    #[test]
    fn short_candidate_list_fills_remaining_computed() {
        let json: serde_json::Value = serde_json::from_str(
            r##"{ "meta": {"type": "light"},
                  "dwl": {"rootcolor": "#08526b",
                          "rootcolor_candidates": ["#41819b", "#08526b"]},
                  "kitty": {"background": "#e7dec7", "active_tab_foreground": "#5d4232"} }"##)
            .unwrap();
        // One usable candidate (#41819b) + darkened fill + the two brighter
        // blends, sorted lightest->darkest.
        let roots: Vec<String> = (1..=4)
            .map(|i| resolve_theme_variant("s", &json, i).root_color)
            .collect();
        assert_eq!(roots[0], blend_colors("#ffffff", "#08526b", 0.70));
        assert_eq!(roots[1], blend_colors("#ffffff", "#08526b", 0.50));
        assert_eq!(roots[2], "#41819b");
        assert_eq!(roots[3], darken_color("#08526b", 0.7));
    }

    #[test]
    fn card_surface_never_varies() {
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let v0 = resolve_theme_variant("s", &json, 0);
        let v1 = resolve_theme_variant("s", &json, 1);
        let v2 = resolve_theme_variant("s", &json, 2);
        assert_eq!(v0.text_bg, v1.text_bg);
        assert_eq!(v0.text_bg, v2.text_bg);
        assert_eq!(v0.text_fg, v1.text_fg);
        assert_eq!(v0.text_fg, v2.text_fg);
        assert_eq!(v0.cursor_line_bg, v1.cursor_line_bg);
        assert_eq!(v0.cursor_line_bg, v2.cursor_line_bg);
        assert_eq!(v0.phrase_highlight_bg, v1.phrase_highlight_bg);
        assert_eq!(v0.phrase_highlight_bg, v2.phrase_highlight_bg);
        assert_eq!(v0.dim_fg, v1.dim_fg);
        assert_eq!(v0.dim_fg, v2.dim_fg);
        assert_eq!(v0.sign_fg, v1.sign_fg);
        assert_eq!(v0.sign_fg, v2.sign_fg);
        assert_eq!(v0.cursor_bg, v1.cursor_bg);
        assert_eq!(v0.cursor_bg, v2.cursor_bg);
        assert_eq!(v0.cursor_fg, v1.cursor_fg);
        assert_eq!(v0.cursor_fg, v2.cursor_fg);
        assert_eq!(v0.vocab_fg, v1.vocab_fg);
        assert_eq!(v0.vocab_fg, v2.vocab_fg);
        assert_eq!(v0.reader_gloss, v1.reader_gloss);
        assert_eq!(v0.reader_gloss, v2.reader_gloss);
        assert_eq!(v0.overlay_panel_bg, v1.overlay_panel_bg);
        assert_eq!(v0.overlay_panel_bg, v2.overlay_panel_bg);
        // scrim_bg DIFFERS between v0 and v1 — it follows root.
        assert_ne!(v0.scrim_bg, v1.scrim_bg);
    }

    #[test]
    fn card_surface_pinned_on_extended_slots() {
        // The two extra slots (3-4) pin the card surface like slots 1-2 do.
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let v0 = resolve_theme_variant("s", &json, 0);
        for i in 3..=4 {
            let v = resolve_theme_variant("s", &json, i);
            assert_eq!(v0.text_bg, v.text_bg);
            assert_eq!(v0.cursor_line_bg, v.cursor_line_bg);
            assert_eq!(v0.phrase_highlight_bg, v.phrase_highlight_bg);
        }
    }

    #[test]
    fn variant_index_wraps_modulo_count() {
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let v5 = resolve_theme_variant("s", &json, 5);
        let v0 = resolve_theme_variant("s", &json, 0);
        assert_eq!(v5.root_color, v0.root_color); // 5 % 5 = 0
    }

    #[test]
    fn scrim_bg_is_a_small_darkening_of_root() {
        // Sample themes with distinct root colors (light-ish and dark).
        let light = Theme {
            is_light: true,
            root_color: "#c8c0a8".to_string(),
            ..default_theme()
        };
        let dark = Theme {
            is_light: false,
            root_color: "#1a1a2e".to_string(),
            ..default_theme()
        };

        for theme in [&light, &dark] {
            let scrim = scrim_bg(theme);
            let root = &theme.root_color;
            // Distinct from the un-dimmed window background...
            assert_ne!(&scrim, root, "scrim must differ from root_color");
            // ...but strictly darker on every channel (a dim, never a brighten)...
            let (sr, sg, sb) = hex_to_rgb(&scrim);
            let (rr, rg, rb) = hex_to_rgb(root);
            assert!(
                sr <= rr && sg <= rg && sb <= rb && (sr < rr || sg < rg || sb < rb),
                "scrim {scrim} must be no brighter than root {root} on any channel"
            );
            // ...and a subtle (~20%) delta, not a heavy blackout.
            let ratio = contrast_ratio(root, &scrim);
            assert!(
                ratio > 1.0 && ratio < 1.6,
                "scrim darkening out of the subtle band: ratio={ratio} \
                 (scrim={scrim}, root={root}, is_light={})",
                theme.is_light
            );
        }
    }

    #[test]
    fn cursor_colors_do_not_vary_with_root_variant() {
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let v0 = resolve_theme_variant("s", &json, 0);
        let v1 = resolve_theme_variant("s", &json, 1);
        let v2 = resolve_theme_variant("s", &json, 2);
        assert_eq!(v0.cursor_fg, v1.cursor_fg);
        assert_eq!(v0.cursor_fg, v2.cursor_fg);
        assert_eq!(v0.cursor_bg, v1.cursor_bg);
        assert_eq!(v0.cursor_bg, v2.cursor_bg);
        assert_eq!(v0.vocab_fg, v1.vocab_fg);
        assert_eq!(v0.vocab_fg, v2.vocab_fg);
    }
}

