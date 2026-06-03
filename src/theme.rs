use serde_json::Value;
use std::path::PathBuf;

/// Resolved theme colors for linux-lit.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Theme {
    pub name: String,
    pub display_name: String,
    pub is_light: bool,
    pub root_color: String,       // outer wallpaper/padding color
    pub text_bg: String,          // text area background
    pub text_fg: String,          // text foreground
    pub cursor_line_bg: String,   // current line highlight
    pub dim_fg: String,           // dimmed text foreground (non-current lines)
    pub cursor_bg: String,        // cursor indicator background
    pub cursor_fg: String,        // cursor indicator foreground
    pub vocab_fg: String,         // vocabulary word highlight foreground
}

fn themes_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("utono/themes/.config/themes/themes-unified.json")
}

fn current_theme_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("utono/themes/.config/themes/.current_theme")
}

/// Read the current theme name from .current_theme file.
pub fn current_theme_name() -> String {
    std::fs::read_to_string(current_theme_path())
        .unwrap_or_default()
        .trim()
        .to_string()
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

fn resolve_theme(name: &str, val: &Value) -> Theme {
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

    let root_color = str_field(dwl, "rootcolor").unwrap_or_else(|| {
        // Dark themes: darken kitty.background by shifting toward black
        darken_color(&text_bg, 0.6)
    });

    let lit = val.get("linux-lit").unwrap_or(&Value::Null);
    let cursor_line_bg = str_field(&lit, "cursor_line_bg")
        .unwrap_or_else(|| "rgba(86, 148, 100, 0.25)".to_string());

    // Dim foreground: 40% fg blended toward bg (matching lit's playback sync)
    let dim_fg = blend_colors(&text_fg, &text_bg, 0.40);

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

    Theme {
        name: name.to_string(),
        display_name,
        is_light,
        root_color,
        text_bg,
        text_fg,
        cursor_line_bg,
        dim_fg,
        cursor_bg,
        cursor_fg,
        vocab_fg,
    }
}

fn default_theme() -> Theme {
    Theme {
        name: "default".to_string(),
        display_name: "Default".to_string(),
        is_light: false,
        root_color: "#1a1a2e".to_string(),
        text_bg: "#282828".to_string(),
        text_fg: "#d4be98".to_string(),
        cursor_line_bg: "rgba(255, 255, 255, 0.08)".to_string(),
        dim_fg: blend_colors("#d4be98", "#282828", 0.40),
        cursor_bg: "#d4be98".to_string(),
        cursor_fg: "#282828".to_string(),
        vocab_fg: "#d8a657".to_string(),
    }
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

/// Parse a hex color string to (r, g, b) as f32 for GDK RGBA.
pub fn root_color_rgb(hex: &str) -> (f32, f32, f32) {
    let (r, g, b) = hex_to_rgb(hex);
    (r as f32, g as f32, b as f32)
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

/// Generate GTK CSS for a theme.
pub fn generate_css(theme: &Theme, font_family: &str, font_size: u32) -> String {
    format!(
        "window {{ background-color: {root}; }} \
         .tiled {{ background-color: {bg}; }} \
         .page-turn-overlay {{ background-color: {bg}; border-radius: 12px; }} \
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
         .chapter-toast {{ font-size: 13px; color: {dim}; opacity: 0.85; }} \
         .gloss-scrim {{ background-color: {root}; }} \
         .gloss-overlay {{ background-color: {bg}; color: {fg}; border-radius: 12px; \
           box-shadow: 0 8px 32px rgba(0, 0, 0, 0.45); }} \
         .gloss-title {{ font-size: {size}pt; font-weight: bold; \
           margin-bottom: 12px; padding-bottom: 12px; \
           border-bottom: 1px solid {dim}; }} \
         .gloss-header {{ font-size: 11px; font-weight: bold; \
           color: {dim}; letter-spacing: 2px; margin-top: 8px; margin-bottom: 4px; }} \
         .gloss-text {{ font-family: {font}; font-size: {size}pt; }} \
         .gloss-hint {{ font-size: 14px; \
           color: {dim}; padding-top: 8px; \
           border-top: 1px solid {dim}; }} \
         .gloss-position {{ font-size: 14px; color: {dim}; }} \
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
         .picker-box {{ background-color: rgba(40, 40, 40, 0.95); color: white; \
           padding: 16px; border-radius: 8px; }} \
         .picker-entry {{ margin-bottom: 8px; }} \
         .picker-list row:selected {{ background-color: rgba(100, 140, 200, 0.8); }} \
         .picker-item-title {{ }} \
         .picker-item-detail {{ opacity: 0.6; }} \
         .picker-header {{ font-size: 14px; font-weight: bold; }} \
",
        root = theme.root_color,
        bg = theme.text_bg,
        fg = theme.text_fg,
        dim = theme.dim_fg,
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
    )
}
