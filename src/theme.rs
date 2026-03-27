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

    // Use semi-transparent overlay to preserve background warmth
    let cursor_line_bg = if is_light {
        "rgba(0, 80, 220, 0.10)".to_string()
    } else {
        "rgba(255, 255, 255, 0.08)".to_string()
    };

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

/// Generate GTK CSS for a theme.
pub fn generate_css(theme: &Theme, font_family: &str, font_size: u32) -> String {
    format!(
        "window {{ background-color: {root}; }} \
         .text-card {{ border-radius: 12px; }} \
         textview {{ background-color: {bg}; color: {fg}; }} \
         textview border {{ background-color: {bg}; }} \
         textview border.left {{ background-color: {bg}; }} \
         textview border * {{ background-color: {bg}; background: {bg}; }} \
         textview text {{ background-color: {bg}; color: {fg}; \
           font-family: {font}; font-size: {size}pt; }} \
         .library-picker {{ background-color: rgba(40, 40, 40, 0.95); color: white; \
           padding: 16px; border-radius: 8px; }} \
         .library-picker entry {{ margin-bottom: 8px; }} \
         .library-picker row:selected {{ background-color: rgba(100, 140, 200, 0.8); }} \
         .search-bar {{ background-color: {bg}; color: {fg}; padding: 4px 12px; }} \
         .search-entry {{ background: transparent; border: none; color: {fg}; }} \
         .search-slash {{ color: {fg}; opacity: 0.6; }} \
         .search-counter {{ color: {fg}; opacity: 0.6; }} \
         .settings-overlay {{ background-color: rgba(40, 40, 40, 0.95); color: white; \
           padding: 16px; border-radius: 8px; }} \
         .settings-title {{ font-size: 18px; font-weight: bold; \
           margin-bottom: 12px; padding-bottom: 12px; \
           border-bottom: 1px solid rgba(255,255,255,0.2); }} \
         .settings-row {{ padding: 8px 12px; margin: 2px 0; border-radius: 4px; }} \
         .settings-row-selected {{ background-color: rgba(100, 140, 200, 0.8); \
           border-left: 3px solid rgba(100, 180, 255, 0.9); }} \
         .settings-footer {{ font-size: 11px; opacity: 0.6; margin-top: 12px; \
           text-align: center; }} \
         .action-popup {{ background-color: {bg}; color: {fg}; \
           padding: 16px; border-radius: 12px; border: 1px solid {dim}; }} \
         .action-popup .settings-title {{ border-bottom: 1px solid {dim}; }} \
         .action-popup .settings-row-selected {{ background-color: {cursor_bg}; \
           color: {cursor_fg}; border-left: 3px solid {cursor_bg}; }} \
         .action-popup .settings-footer {{ color: {dim}; }} \
         .action-separator {{ color: {dim}; opacity: 0.3; margin: 4px 12px; }} \
         .keybinds-overlay {{ background-color: rgba(40, 40, 40, 0.95); color: white; \
           padding: 16px; border-radius: 8px; }} \
         .keybind-key {{ background-color: rgba(255, 255, 255, 0.08); \
           border-radius: 3px; min-width: 68px; padding: 3px 6px; }} \
         .keybind-key-unbound {{ opacity: 0.25; }} \
         .keybind-label-bare {{ color: #7db8f0; font-size: 44px; font-weight: bold; }} \
         .keybind-label-ctrl {{ color: #d4a052; font-size: 44px; font-weight: bold; }} \
         .keybind-label-alt {{ color: #c47dd4; font-size: 44px; font-weight: bold; }} \
         .keybind-label-ctrlalt {{ color: #d45050; font-size: 44px; font-weight: bold; }} \
         .keybind-action {{ color: rgba(255, 255, 255, 0.5); font-size: 36px; }} \
         .keybind-row-header {{ font-size: 36px; letter-spacing: 2px; \
           color: rgba(255, 255, 255, 0.35); margin-bottom: 4px; }} \
         .keybind-legend {{ border-top: 1px solid rgba(255, 255, 255, 0.1); \
           margin-top: 12px; padding-top: 8px; }} \
         .correction-scrim {{ background-color: rgba(0, 0, 0, 0.3); }} \
         .correction-overlay {{ background-color: {bg}; color: {fg}; \
           padding: 24px; border-radius: 12px; \
           border: 1px solid {dim}; }} \
         .correction-title {{ font-size: {size}pt; font-weight: bold; \
           margin-bottom: 12px; padding-bottom: 12px; \
           border-bottom: 1px solid {dim}; }} \
         .correction-header {{ font-size: 11px; font-weight: bold; \
           color: {dim}; letter-spacing: 2px; margin-top: 8px; margin-bottom: 4px; }} \
         .correction-text {{ font-family: {font}; font-size: {size}pt; }} \
         .correction-hint {{ font-size: 11px; \
           color: {dim}; margin-top: 12px; padding-top: 8px; \
           border-top: 1px solid {dim}; }} \
",
        root = theme.root_color,
        bg = theme.text_bg,
        fg = theme.text_fg,
        dim = theme.dim_fg,
        cursor_bg = theme.cursor_bg,
        cursor_fg = theme.cursor_fg,
        font = font_family,
        size = font_size,
    )
}
