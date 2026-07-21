use serde_json::Value;
use std::path::PathBuf;

/// Minimum contrast ratio (vs the reading-surface bg) for the reader-gloss line
/// tint. The old 3.0 UI floor let a barely-distinct pastel (e.g. a 3.0 salmon on
/// cream) through, which was hard to read against near-black body text. 4.5 is
/// the WCAG AA body-text threshold — forces a DARKER variant of the same hue so
/// glossed lines read clearly while staying a distinct tint (body text is ~6.7).
const READER_GLOSS_MIN_CONTRAST: f64 = 4.5;

/// Minimum contrast of the reader-gloss/journal-Q&A tint against the CURRENT
/// ROOT color. The tint's base hue is the dwl focuscolor — usually the root
/// accent — so a value that merely clears the card-bg floor can sit exactly AT
/// the root hue and lightness, reading as "the root color leaked onto the
/// card" (washy next to every root-colored surface). This floor forces a
/// clearly darker/lighter cousin of the hue and, because it is checked
/// against the LIVE root, the tint re-derives on every Ctrl+t root-variant
/// step like vocab_fg does. The rule is hue-OR-contrast: the root is a mid-
/// luminance saturated surface, so a hue 40°+ away reads as a different
/// color even at equal luminance; only a same-hue tint must buy its
/// distinctness with this luminance ratio.
const READER_GLOSS_ROOT_MIN_CONTRAST: f64 = 1.8;

/// Minimum luminance contrast of the tint against the BODY INK (text_fg)
/// on LIGHT themes. There the ink is usually near-black, where hue is
/// imperceptible — a hue escape would let a "distinct" tint render as just
/// more body text (the failure mode: the root floor pushed the tint deep
/// enough to sit next to the ink). Only a luminance gap separates a tint
/// from near-black ink. Dark themes keep the hue-OR-contrast rule instead:
/// their ink is a mid-light cream and their accents live in the mid range
/// where hue still reads, and a hard luminance floor there squeezes every
/// tint to an undifferentiable near-white (everforest-dark-soft).
///
/// When the light theme's ink is instead a HUE-PERCEPTIBLE color (a
/// mid-luminance saturated ink — zenbones/zenwritten teal #286486,
/// tokyonight-day blue, papercolor-light blue), this floor alone is not
/// enough: a same-hue tint that clears it on the dark side just reads as
/// bolder body text (a #1b4150 vocab word next to #286486 dialogue). For
/// those inks the tint must ALSO sit ≥ 40° away in hue — which is exactly
/// what makes the derived wine/maroon pop against teal ink while the
/// luminance floor still guarantees it is not the ink's own weight.
const READER_GLOSS_INK_MIN_CONTRAST: f64 = 1.5;

/// Hue distance (degrees) at which two colors read as DIFFERENT colors on
/// screen; the "visually distinct" half of the hue-OR-contrast guard in
/// `ensure_gloss_color_min`.
const HUE_DISTINCT_MIN_DEG: f64 = 40.0;

/// The paired fallback: when two colors sit closer than
/// `HUE_DISTINCT_MIN_DEG` in hue, they still read as distinct if their
/// contrast ratio clears this floor.
const TINT_DISTINCT_MIN_CONTRAST: f64 = 1.4;

/// Saturation floor for derived tints — below this a pushed tint washes out
/// into gray instead of reading as the base hue.
const TINT_MIN_SATURATION: f64 = 0.50;

/// Thresholds above which an ink's HUE reads on screen (vs a near-black or
/// muted ink where only luminance registers). Tuned on the theme corpus:
/// zenbones/zenwritten #286486 (lum .11, sat .54), tokyonight-day #3760bf
/// and papercolor-light #005f87 qualify; near-black inks (modus, github),
/// muted browns (gruvbox-material #654735, sat .31) and dark plums (dayfox,
/// lum .035) stay on the pure-luminance rule.
const INK_HUE_PERCEPTIBLE_MIN_LUM: f64 = 0.05;
const INK_HUE_PERCEPTIBLE_MIN_SAT: f64 = 0.40;

/// True when `hex` is bright and saturated enough that its hue is readable
/// as a COLOR (see INK_HUE_PERCEPTIBLE_*). Used to pick the ink-distinctness
/// rule for tints on light reading surfaces.
fn hue_perceptible(hex: &str) -> bool {
    let (r, g, b) = hex_to_rgb(hex);
    let (_, s, _) = rgb_to_hsl(r, g, b);
    relative_luminance(hex) >= INK_HUE_PERCEPTIBLE_MIN_LUM && s >= INK_HUE_PERCEPTIBLE_MIN_SAT
}

/// The ordered root-color variants for a theme: `dwl.rootcolor_candidates`
/// used VERBATIM (authored order — the dwl-mlj tooling already builds a
/// hue-locked, sorted native-accent family). When a theme defines no
/// candidates, the sole variant is the designed `dwl.rootcolor`. Never
/// synthesizes colors and never re-sorts. See
/// docs/superpowers/specs/2026-07-14-root-native-accent-design.md.
fn root_variants(val: &Value, designed_root: &str) -> Vec<String> {
    let candidates: Vec<String> = val
        .get("dwl")
        .and_then(|d| d.get("rootcolor_candidates"))
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if candidates.is_empty() {
        vec![designed_root.to_string()]
    } else {
        candidates
    }
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
    pub root_variant: u8,         // active root-color variant (index into candidates)
    pub root_variant_count: u8,   // number of root variants (candidate count, min 1)
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
/// docs/superpowers/specs/2026-07-08-independent-theme-keybinds-design.md).
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

    // Reader body ink = the kitty theme's font color (`kitty.foreground`), so
    // the reading card matches the terminal theme. Falls back to
    // `active_tab_foreground` (the accent some linux-lit-only themes carry
    // instead of a foreground), then to the light/dark default.
    let text_fg = str_field(kitty, "foreground")
        .or_else(|| str_field(kitty, "active_tab_foreground"))
        .unwrap_or_else(|| {
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

    let variants = root_variants(val, &designed_root);
    let count = variants.len().max(1) as u8;
    // Out-of-range saved index resets to 0 (first candidate).
    let variant = if (variant as usize) < variants.len() { variant } else { 0 };
    let root_color = variants[variant as usize].clone();

    let lit = val.get("linux-lit").unwrap_or(&Value::Null);
    let cursor_line_bg = str_field(lit, "cursor_line_bg")
        .unwrap_or_else(|| "rgba(86, 148, 100, 0.25)".to_string());

    // Spoken-phrase karaoke tint: the ACTIVE root (wallpaper) color, re-derived
    // on every root-variant step so the sweep always reads as "the wallpaper
    // color washing through the line" (apply_theme_to_state pushes it into
    // phrase_tag live). An explicit per-theme phrase_highlight_bg contributes
    // only its hand-tuned ALPHA now (the sepia/green ladder runs 0.18/0.14);
    // its rgb is superseded by the live root. Themes without the key use the
    // historical 0.28 default strength. Search highlights derive from this
    // (search_highlight_colors), so they follow the root too.
    let phrase_highlight_bg = {
        let alpha = str_field(lit, "phrase_highlight_bg")
            .map(|s| rgba_str_alpha(&s))
            .unwrap_or(0.28);
        let (r, g, b) = hex_to_rgb(&root_color);
        format!(
            "rgba({}, {}, {}, {})",
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            alpha
        )
    };

    // Reader-gloss tints, contrast-guaranteed (raw focuscolor is dim/indistinct
    // on ~13 themes). Off-cursor = guarded focuscolor; on-cursor = guarded
    // complement, also kept distinct from the off-cursor tint.
    // Both tints are run through the readable floor even when a theme sets them
    // explicitly, so an explicit-but-washed-out value (e.g. a 3.1-contrast teal)
    // is still darkened to a readable variant of its hue rather than shipping a
    // hard-to-read gloss. Explicit values only differ from the derived ones by
    // choosing the base hue.
    // Both tints also clear READER_GLOSS_ROOT_MIN_CONTRAST against the LIVE
    // root color (this variant's), so a focuscolor that IS the root accent
    // can't ship as-is — the tint becomes a clearly darker/lighter cousin of
    // the hue and re-derives on every root-variant step.
    // text_fg rides the dedicated ink floor (contrast-only), NOT the avoid
    // list — the avoid rule's hue escape let a near-ink-dark tint pass as
    // "distinct" when it read as plain body text.
    // Per-root explicit gloss overrides: a hue chosen FOR one wallpaper root
    // (e.g. the dusty-rose roots pair with a complementary sage green). Keys
    // are candidate root hexes; looked up by the ACTIVE variant's root
    // (case-insensitive), falling back to the theme-wide explicit key, then
    // the derived default. Treated exactly like an explicit value by the
    // guards below — the hue is deliberate.
    let by_root = |key: &str| -> Option<String> {
        lit.get(key)?
            .as_object()?
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(&root_color))
            .and_then(|(_, v)| v.as_str().map(str::to_string))
    };
    let reader_gloss = {
        // An EXPLICIT reader_gloss is a deliberate hue choice and must stay that
        // hue across ALL root variants. The root (wallpaper) is CYCLEABLE and
        // independent of the text; guarding an explicit gloss against the active
        // variant's root rotated a blue gloss to red on the darker root rungs
        // (the wallpaper cycled, the text flipped color). So skip the root
        // distinctness check for an explicit value (bg + ink legibility still
        // apply); only the DERIVED focus_color default guards against root.
        // A `reader_gloss_by_root` entry for the active root outranks the
        // theme-wide key — it was picked for exactly this wallpaper.
        match by_root("reader_gloss_by_root").or_else(|| str_field(lit, "reader_gloss")) {
            Some(base) => ensure_gloss_color_min(
                &base, &text_bg, None, Some(&text_fg), &[],
                READER_GLOSS_MIN_CONTRAST,
            ),
            None => ensure_gloss_color_min(
                &focus_color, &text_bg, Some(&root_color), Some(&text_fg), &[],
                READER_GLOSS_MIN_CONTRAST,
            ),
        }
    };
    let reader_gloss_cursor = {
        // When a theme EXPLICITLY sets reader_gloss_cursor, honor it as-is —
        // including deliberately setting it EQUAL to reader_gloss (one gloss
        // color for both states). The `avoid: [reader_gloss]` list below forces
        // distinctness, which is only wanted for the DERIVED default; applying
        // it to an explicit same-as-gloss value rotated the cursor color to a
        // jarring complement (teal→pink, navy→red). So: explicit → no avoid;
        // derived (complement) → keep the avoid so the fallback pair stays
        // distinct. A `reader_gloss_cursor_by_root` entry for the active root
        // outranks the theme-wide key (same rule as reader_gloss_by_root).
        match by_root("reader_gloss_cursor_by_root")
            .or_else(|| str_field(lit, "reader_gloss_cursor"))
        {
            Some(base) => ensure_gloss_color_min(
                &base, &text_bg, Some(&root_color), Some(&text_fg), &[],
                READER_GLOSS_MIN_CONTRAST,
            ),
            None => ensure_gloss_color_min(
                &complement_hex(&reader_gloss), &text_bg, Some(&root_color), Some(&text_fg),
                &[&reader_gloss],
                READER_GLOSS_MIN_CONTRAST,
            ),
        }
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

    // Vocab words in the reading card start from the CURRENT root hue (so
    // the tint pairs with the field around the card and follows Ctrl+t's
    // root variants) but ride the SAME guard rails as the reader-gloss /
    // journal-Q&A tint: readable on the card, visibly apart from the live
    // root (hue OR contrast), and holding a hard luminance gap from the
    // body ink on light themes. The old derivation had only the card floor
    // plus the lenient avoid rule (1.4 with a hue escape), which on themes
    // whose ink shares the root's hue shipped vocab words that read as
    // plain body text.
    // vocab_fg is root-hued by default (derives from the active root, guarded
    // against it so it stays legible + root-distinct). But an EXPLICIT vocab_fg
    // in the theme's linux-lit block is used VERBATIM — the author picked an
    // exact color (and validated it), and it must render identically on every
    // theme (so the SAME blue can be shared across themes) and survive root-
    // variant cycling. Running it through the guard adapted it per-theme
    // (darkened it to meet each ink floor), breaking cross-theme consistency.
    let vocab_fg = match str_field(lit, "vocab_fg") {
        Some(base) => base,
        None => ensure_gloss_color_min(
            &root_color, &text_bg, Some(&root_color), Some(&text_fg), &[],
            VOCAB_WORD_MIN_CONTRAST,
        ),
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
        root_variant_count: count,
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
        // Root-hued like loaded themes (guarded for contrast on the card).
        vocab_fg: ensure_gloss_color_min("#1a1a2e", "#282828", None, None, &["#d4be98"], VOCAB_WORD_MIN_CONTRAST),
        reader_gloss: ensure_gloss_color_min("#d4be98", "#282828", Some("#1a1a2e"), Some("#d4be98"), &[], READER_GLOSS_MIN_CONTRAST),
        reader_gloss_cursor: ensure_gloss_color_min(
            &complement_hex("#d4be98"),
            "#282828",
            Some("#1a1a2e"),
            Some("#d4be98"),
            &[],
            READER_GLOSS_MIN_CONTRAST,
        ),
        overlay_panel_bg: String::new(),
        scrim_bg: String::new(),
        root_variant: 0,
        root_variant_count: 1,
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

/// Minimum WCAG contrast for the vocab popup's main text against the root
/// it sits on (AAA); the dim tier (header/counter/hint) gets the AA floor.
const VOCAB_POPUP_MIN_CONTRAST: f64 = 7.0;
const VOCAB_POPUP_DIM_MIN_CONTRAST: f64 = 4.5;
/// Minimum contrast for the root-hued vocab-word tint on the reading card.
const VOCAB_WORD_MIN_CONTRAST: f64 = 4.5;

/// Ink for the vocab popup, which floats directly on the window root: pick
/// whichever of the theme's text/card colors reads better against the CURRENT
/// root color. Root variants (Ctrl+t) regenerate the CSS with the variant as
/// `root_color`, so the popup stays legible on light, mid, and dark variants
/// alike. Mid-tone roots can defeat BOTH theme colors (e.g. #6f9fb3 holds the
/// dark sepia ink to ~4:1 and cream to ~2.7:1) — below the floor, fall back
/// to whichever of pure black/white maximizes the ratio (softer near-black
/// tops out below the AAA floor on mid roots).
fn vocab_popup_ink(theme: &Theme) -> String {
    let root = &theme.root_color;
    let best = if contrast_ratio(&theme.text_fg, root) >= contrast_ratio(&theme.text_bg, root) {
        &theme.text_fg
    } else {
        &theme.text_bg
    };
    if contrast_ratio(best, root) >= VOCAB_POPUP_MIN_CONTRAST {
        return best.clone();
    }
    // Perceptual pick, not ratio-max: on darker-than-mid roots white reads
    // better even where black scores the higher WCAG ratio (tuned on real
    // variants: #5c8da1, lum .24 → white; #6f9fb3, lum .31 → black).
    if relative_luminance(root) < 0.28 {
        "#ffffff".to_string()
    } else {
        "#000000".to_string()
    }
}

/// Accent for the vocab popup's etymology morphemes: the vocab tint
/// re-guarded against the ROOT (the popup's background) — the in-card
/// `vocab_fg` is root-hued and guarded only against the card, so used
/// directly it can sit invisibly close to the root itself.
pub(crate) fn vocab_popup_accent(theme: &Theme) -> String {
    ensure_gloss_color_min(
        &theme.vocab_fg,
        &theme.root_color,
        None,
        None,
        &[],
        VOCAB_WORD_MIN_CONTRAST,
    )
}

/// The vocab popup's rendered main text color for `theme`'s CURRENT root —
/// the same value `generate_css` writes for `.vocab-popup .definition-*`.
/// Also used by Ctrl+t's clipboard copy of the (root, popup fg) pair.
pub(crate) fn vocab_popup_fg(theme: &Theme) -> String {
    vocab_popup_tier(
        &vocab_popup_ink(theme),
        &theme.root_color,
        0.85,
        VOCAB_POPUP_MIN_CONTRAST,
    )
}

/// One tint tier of the popup ink: soften toward the root by `alpha`, but
/// never below `min` contrast — step the blend back toward the pure ink
/// until the floor is met (pure ink at worst).
fn vocab_popup_tier(ink: &str, root: &str, alpha: f64, min: f64) -> String {
    let mut a = alpha;
    loop {
        let candidate = blend_colors(ink, root, a);
        if contrast_ratio(&candidate, root) >= min || a >= 1.0 {
            return candidate;
        }
        a = (a + 0.15).min(1.0);
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
    let (h1, _, _) = { let (r, g, b) = hex_to_rgb(c1); rgb_to_hsl(r, g, b) };
    let (h2, _, _) = { let (r, g, b) = hex_to_rgb(c2); rgb_to_hsl(r, g, b) };
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
    let (la, lb) = (relative_luminance(a_hex) + 0.05, relative_luminance(b_hex) + 0.05);
    if la > lb { la / lb } else { lb / la }
}

/// Return a color at `base_hex`'s hue that is legible on `bg_hex` (WCAG contrast
/// ≥ 3.0) and visually distinct (hue distance ≥ 40° OR contrast ≥ 1.4) from each
/// color in `avoid`. If `base_hex` already qualifies it is returned unchanged, so
/// themes that already look right do not move. Otherwise lightness is pushed away
/// from the background and saturation raised at the same hue; as a last resort the
/// hue is rotated 150° and S/L clamped. Used to
/// derive both reader-gloss tints so they never wash out or blend into body text.
/// Resolve a gloss-tint color with a caller-chosen minimum contrast floor vs the
/// background. The reader-gloss tint uses a higher floor (READER_GLOSS_MIN_
/// CONTRAST) so it renders as a DARKER, clearly readable variant of the hue
/// rather than a washed-out near-3.0 pastel that's hard to read on the cream
/// reading surface. `min_contrast = 3.0` is the plain UI floor (used by tests /
/// any future non-body-text caller).
fn ensure_gloss_color_min(
    base_hex: &str,
    bg_hex: &str,
    root_hex: Option<&str>,
    ink_hex: Option<&str>,
    avoid: &[&str],
    min_contrast: f64,
) -> String {
    let bg_is_light = contrast_ratio(bg_hex, "#000000") > contrast_ratio(bg_hex, "#ffffff");
    let ok = |c: &str| {
        contrast_ratio(c, bg_hex) >= min_contrast
            // Root: hue-OR-contrast — the root is a mid-luminance saturated
            // surface, so a far-away hue reads as a different color.
            && root_hex.map_or(true, |r| {
                hue_distance(c, r) >= HUE_DISTINCT_MIN_DEG
                    || contrast_ratio(c, r) >= READER_GLOSS_ROOT_MIN_CONTRAST
            })
            // Ink: on a light bg the ink is usually near-black (hue-blind),
            // so a luminance gap is required; when the ink is itself a
            // hue-perceptible color (zenbones teal), a same-hue darker tint
            // still reads as bold body text, so a 40° hue gap is required
            // too. On a dark bg the ink is mid-light and hue still reads
            // (see READER_GLOSS_INK_MIN_CONTRAST).
            && ink_hex.map_or(true, |i| {
                if bg_is_light {
                    contrast_ratio(c, i) >= READER_GLOSS_INK_MIN_CONTRAST
                        && (!hue_perceptible(i) || hue_distance(c, i) >= HUE_DISTINCT_MIN_DEG)
                } else {
                    hue_distance(c, i) >= HUE_DISTINCT_MIN_DEG
                        || contrast_ratio(c, i) >= TINT_DISTINCT_MIN_CONTRAST
                }
            })
            && avoid.iter().all(|a| {
                hue_distance(c, a) >= HUE_DISTINCT_MIN_DEG
                    || contrast_ratio(c, a) >= TINT_DISTINCT_MIN_CONTRAST
            })
    };
    if ok(base_hex) {
        return base_hex.to_string();
    }
    let (br, bg_, bb) = hex_to_rgb(base_hex);
    let (h, s, _l) = rgb_to_hsl(br, bg_, bb);
    // Push lightness toward the side with headroom against the bg; raise S.
    // The deeper rungs exist for the root floor: a mid-lightness root needs
    // the tint pushed well past the card-bg floor before it clears both.
    let s2 = s.max(TINT_MIN_SATURATION);
    for &l in if bg_is_light {
        // darker, for a light bg — fine-grained so the narrow window between
        // the card floor and the root floor (dark root + light card) is hit
        &[0.42_f64, 0.39, 0.36, 0.33, 0.30, 0.27, 0.24, 0.21, 0.18, 0.15, 0.12][..]
    } else {
        // lighter, for a dark bg
        &[0.62_f64, 0.65, 0.68, 0.71, 0.74, 0.77, 0.80, 0.83, 0.86, 0.89, 0.92][..]
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
    // first try (existing themes keep their colors),
    // then widening alternates. Falls back to the highest-contrast-vs-bg
    // candidate if (theoretically) no rotation satisfies the full guard.
    let s2 = s.max(TINT_MIN_SATURATION);
    let rungs: &[f64] = if bg_is_light {
        &[0.36, 0.30, 0.24, 0.42, 0.18, 0.12]
    } else {
        &[0.70, 0.76, 0.64, 0.82, 0.58, 0.88]
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

/// Fraction of the root/wallpaper color kept in the toast background; the rest
/// is the card's own `text_bg`. Below 1.0 the toast reads as a MUTED variant of
/// the root hue rather than the full-strength wallpaper chip. Applies to DARK
/// themes only (see `chapter_toast_bg`).
const CHAPTER_TOAST_ROOT_MIX: f64 = 0.30;
/// After the root/card blend, lift the toast this fraction toward white so the
/// chip reads lighter/airier than the card while keeping its faint root tint.
/// Dark themes only.
const CHAPTER_TOAST_WHITE_LIFT: f64 = 0.35;

/// Background for the persistent act/scene/chapter toast (and the search toast).
///
/// The chip has to read as a quiet location label sitting ON the reading card,
/// so it must share the card's tone — NOT fight it. How we get there depends on
/// whether the card is light or dark:
///
/// - **Light themes** reuse the theme's OWN cursor-line highlight: the
///   `cursor_line_bg` tint composited over the card `text_bg` (its rgba alpha
///   applied). This is the exact warm band the reader already sees under the
///   current line, so the pill matches the card's family by construction. It
///   also fixes the sepia family, where blending the saturated, COMPLEMENTARY
///   teal root (e.g. sepia-lightest's `#437c8f`) into a warm cream card could
///   never yield a warm chip — teal's low red channel dragged any mix cool/gray
///   and clashed with the cream page.
/// - **Dark themes** keep a SOFTER variant of the root/wallpaper hue: root
///   blended toward the card's `text_bg` (`CHAPTER_TOAST_ROOT_MIX`) then lifted
///   toward white (`CHAPTER_TOAST_WHITE_LIFT`). On dark cards the root usually
///   shares the card's cool family, so its identity reads as an intentional echo
///   of the wallpaper.
///
/// `contrast_on` picks the label ink against whichever background results.
///
/// `pub(crate)` so the theme/root keybinds (`copy_pairing_and_screenshot` in
/// `input::actions::settings`) can copy the exact pill color to the clipboard.
pub(crate) fn chapter_toast_bg(theme: &Theme) -> String {
    if theme.is_light {
        // The cursor-line band, made opaque over the card: same warm tint the
        // reader sees under the current line.
        let (r, g, b) = rgba_str_to_rgb(&theme.cursor_line_bg);
        let ink = rgb_to_hex(r as f64, g as f64, b as f64);
        let alpha = rgba_str_alpha(&theme.cursor_line_bg);
        blend_colors(&ink, &theme.text_bg, alpha)
    } else {
        let tinted = blend_colors(&theme.root_color, &theme.text_bg, CHAPTER_TOAST_ROOT_MIX);
        blend_colors("#ffffff", &tinted, CHAPTER_TOAST_WHITE_LIFT)
    }
}

/// Parse the alpha channel from an `rgba(r, g, b, a)` string, defaulting to 1.0
/// (opaque) for a plain `rgb(...)`/hex or on any parse failure — so a solid
/// `cursor_line_bg` composites as itself.
fn rgba_str_alpha(s: &str) -> f64 {
    let inner = s.trim().trim_start_matches("rgba").trim_start_matches("rgb")
        .trim().trim_start_matches('(').trim_end_matches(')');
    inner
        .split(',')
        .nth(3)
        .and_then(|p| p.trim().parse::<f64>().ok())
        .unwrap_or(1.0)
}

/// Extra white-lift for the transient CENTER toast (`.center-toast`,
/// the `speed_toast` widget: sync/speed/volume messages) on top of the
/// chapter-toast background. The center toast pops over the reading text, so it
/// reads lighter/airier than the persistent location chip to be less
/// distracting.
const CENTER_TOAST_WHITE_LIFT: f64 = 0.45;

/// Background for the transient center toast: the chapter-toast background
/// lifted further toward white so it reads as a quiet, non-distracting flash.
fn center_toast_bg(theme: &Theme) -> String {
    blend_colors("#ffffff", &chapter_toast_bg(theme), CENTER_TOAST_WHITE_LIFT)
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

    /// Overlay search-highlight background colors as `(all_matches, current_match)`
    /// — VARIANTS of the main card's cursor-segment tint (`phrase_highlight_bg`,
    /// the karaoke highlight), so overlay search reads as the reader's own
    /// highlight. `all` = the base phrase tint; `current` = the same hue boosted
    /// (higher alpha) so the active match stands out. `phrase_highlight_bg`'s
    /// alpha is low (~0.18), so `all` at 1× is a subtle-but-visible tint and
    /// `current` at ~2.4× is a clearly stronger one (alpha caps at 1.0 in CSS).
    /// One source of truth for both the startup wiring and `apply_theme_to_state`.
    pub fn search_highlight_colors(&self) -> (String, String) {
        let all = self.phrase_highlight_bg.clone();
        let current = dim_rgba_alpha(&self.phrase_highlight_bg, 2.4);
        (all, current)
    }
}

/// Visual skin for the overlay card pickers (`.library-picker`). Flip
/// `PICKER_SKIN` to switch every card picker between directions; the rest of the
/// theme CSS is unaffected. This is the one-line revert knob.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PickerSkin {
    /// Direction A: the whole card is the root/wallpaper color, rows sit
    /// transparently on it with light on-root ink, selection is a translucent
    /// wash. No titled header band.
    RootFill,
    /// Direction F: a titled root header band over a cream body. Root earns a
    /// job (naming the picker); the list keeps the reading-card surface and the
    /// selection is a soft tint with a root rail.
    HeaderBand,
}

/// The active picker skin. Change this single line to switch directions
/// (`PickerSkin::RootFill` = A, `PickerSkin::HeaderBand` = F).
const PICKER_SKIN: PickerSkin = PickerSkin::HeaderBand;

/// The `.library-picker*` CSS block for the active [`PickerSkin`]. Returns a
/// fully-resolved CSS string (real color values, no `{}` placeholders) so it can
/// be spliced into `generate_css`'s outer `format!` as a plain `{picker_css}`
/// argument. `.library-picker-scrim` is emitted by the caller (skin-independent).
fn picker_css(theme: &Theme, font_family: &str, font_size: u32) -> String {
    let font = font_family;
    let size = font_size;
    let root = &theme.root_color;
    let bg = &theme.text_bg;
    let fg = &theme.text_fg;
    let dim = &theme.dim_fg;
    let on_root = vocab_popup_fg(theme);
    let header_border = blend_colors(&theme.dim_fg, &theme.text_bg, 0.5);
    let focus_ring = blend_colors(&theme.cursor_bg, &theme.text_bg, 0.4);
    // HeaderBand selection: reuse the reading card's own current-line highlight
    // so the row about to open matches the cursor-line band in the main card
    // exactly (same rgba tint over the same cream ground). Plus a root rail.
    let sel_tint = &theme.cursor_line_bg;
    let card_border = blend_colors(dim, bg, 0.3);

    match PICKER_SKIN {
        // ── Direction A: root fill, transparent light rows ──────────────
        PickerSkin::RootFill => format!(
            ".library-picker {{ background-color: {root}; color: {fg}; \
               padding: 0; border-radius: 12px; border: 1px solid {dim}; \
               box-shadow: 0 18px 48px rgba(0, 0, 0, 0.22), \
                           0 2px 6px rgba(0, 0, 0, 0.08); }} \
             .library-picker-header {{ padding: 14px 22px 10px; \
               border-bottom: 1px solid {header_border}; }} \
             .library-picker-title {{ font-size: 14px; font-weight: 700; \
               letter-spacing: 2px; color: {on_root}; opacity: 0.85; }} \
             .library-picker-crumb {{ font-size: 13px; color: {on_root}; \
               opacity: 0.75; }} \
             .library-picker entry {{ margin: 12px 18px 8px; \
               padding: 8px 12px; border: 1px solid {dim}; \
               border-radius: 8px; background-color: {bg}; color: {fg}; \
               font-family: {font}; font-size: {size}pt; }} \
             .library-picker entry:focus {{ \
               box-shadow: 0 0 0 3px {focus_ring}; }} \
             .library-picker scrolledwindow {{ padding: 4px 8px 10px; \
               background-color: transparent; }} \
             .library-picker list {{ background-color: transparent; }} \
             .library-picker row {{ padding: 8px 14px; \
               border-radius: 6px; background-color: transparent; \
               color: {on_root}; }} \
             .library-picker row label {{ color: {on_root}; \
               font-family: {font}; font-size: {size}pt; }} \
             .library-picker row label.picker-item-detail {{ \
               font-variant-numeric: tabular-nums; min-width: 32px; \
               font-family: {font}; font-size: {size}pt; color: {on_root}; opacity: 0.7; }} \
             .library-picker row:selected {{ \
               background-color: alpha({on_root}, 0.18); \
               color: {on_root}; }} \
             .library-picker row:selected label {{ color: {on_root}; }} \
             .library-picker row:selected label.picker-item-detail {{ \
               color: {on_root}; opacity: 0.85; }} \
             .library-picker-footer {{ padding: 8px 22px 12px; \
               border-top: 1px solid {header_border}; \
               font-size: 12px; letter-spacing: 1.2px; \
               color: {on_root}; opacity: 0.75; }} \
             "
        ),
        // ── Direction F: root header band over a cream body ─────────────
        PickerSkin::HeaderBand => format!(
            ".library-picker {{ background-color: {bg}; color: {fg}; \
               padding: 0; border-radius: 12px; border: 1px solid {card_border}; \
               box-shadow: 0 18px 48px rgba(0, 0, 0, 0.22), \
                           0 2px 6px rgba(0, 0, 0, 0.08); }} \
             .library-picker-header {{ padding: 11px 20px; \
               background-color: {root}; \
               border-radius: 11px 11px 0 0; }} \
             .library-picker-title {{ font-size: 12px; font-weight: 700; \
               letter-spacing: 2px; color: {on_root}; opacity: 0.95; }} \
             .library-picker-crumb {{ font-size: 13px; color: {on_root}; \
               opacity: 0.8; }} \
             .library-picker entry {{ margin: 12px 18px 8px; \
               padding: 8px 12px; border: 1px solid {dim}; \
               border-radius: 8px; background-color: {bg}; color: {fg}; \
               font-family: {font}; font-size: {size}pt; }} \
             .library-picker entry:focus {{ \
               box-shadow: 0 0 0 3px {focus_ring}; }} \
             .library-picker scrolledwindow {{ padding: 4px 8px 10px; \
               background-color: {bg}; }} \
             .library-picker list {{ background-color: {bg}; }} \
             .library-picker row {{ padding: 8px 14px; \
               border-radius: 6px; background-color: transparent; \
               color: {fg}; }} \
             .library-picker row label {{ color: {fg}; \
               font-family: {font}; font-size: {size}pt; }} \
             .library-picker row label.picker-item-detail {{ \
               font-variant-numeric: tabular-nums; min-width: 32px; \
               font-family: {font}; font-size: {size}pt; color: {fg}; opacity: 0.7; }} \
             .library-picker row:selected {{ \
               background-color: {sel_tint}; color: {fg}; \
               box-shadow: inset 3px 0 0 {root}; }} \
             .library-picker row:selected label {{ color: {fg}; }} \
             .library-picker row:selected label.picker-item-detail {{ \
               color: {fg}; opacity: 0.85; }} \
             .library-picker-footer {{ padding: 8px 22px 12px; \
               border-top: 1px solid {header_border}; \
               font-size: 12px; letter-spacing: 1.2px; \
               color: {fg}; opacity: 0.65; }} \
             "
        ),
    }
}

/// Chat-panel block-quote indents, derived from the gloss OVERLAY's own
/// `gloss_render::{QUOTE_BODY_INDENT, QUOTE_SPEAKER_INDENT, QUOTE_VERSE_INDENT}`
/// (20 / 48 / 108) at a 2/3 scale, rounded, THEN the source rows (speaker +
/// verse) bumped by ~2 characters. The overlay's card is typically ~1050px
/// wide (`config.column_width`) with its own `bar_left` inset on top of these
/// values; the chat panel's minimum width is `CHAT_MIN_PANEL_W` (500px, see
/// `input/actions/chat.rs`) — roughly half — so the full 108px verse hang
/// would leave under 400px of wrap width and crowd the text. 2/3 keeps the
/// graduated relationship (body < speaker < verse) the overlay establishes.
///
/// `CHAT_SOURCE_INDENT_BUMP` (16px ≈ 2 chars of Charter at the 16pt body size,
/// serif advance ~0.5em) deepens the SOURCE lines only — the block quote reads
/// as more clearly set off from the flush explication. The body/explication
/// indent is not a source line and is left at the 2/3 value. The bump lands
/// speaker back at the overlay's own 48.
const CHAT_SOURCE_INDENT_BUMP: i32 = 16;
const CHAT_QUOTE_BODY_INDENT: i32 = 14; // gloss_render::QUOTE_BODY_INDENT * 2/3 (explication; no bump)
const CHAT_QUOTE_SPEAKER_INDENT: i32 = 32 + CHAT_SOURCE_INDENT_BUMP; // 2/3 (=32) + 2 chars
const CHAT_QUOTE_VERSE_INDENT: i32 = 68 + CHAT_SOURCE_INDENT_BUMP; // 2/3 (=68, rounded) + 2 chars

/// Generate GTK CSS for a theme.
///
/// `.chat-a-speaker`/`.chat-a-verse` (the chat panel's typed reader-gloss
/// rows, see `gloss_render::chat_gloss_rows`) mirror the MAIN READING CARD's
/// own dialogue typography: `src/app/formatting.rs`'s `speaker-name` TextTag
/// is small-caps, weight 400, scale 0.75 — subordinate to the dialogue line
/// purely via size/weight, not color-dimming. `.chat-a-speaker` therefore
/// carries `font-variant: small-caps; font-weight: normal; font-size: 0.75em`
/// with NO `color`/`alpha` override (full ink, `{chat_ink}`/`{fg}` — CSS
/// `currentColor` inheritance would also work but an explicit value matches
/// every sibling rule's style) — `0.75em` is the CSS analogue of the card's
/// Pango `scale(0.75)`, both relative multipliers on the inherited/base font
/// size (see the `.chat-a-speaker` rule below for the full Fix 4 rationale).
/// The verse is the star (quoted Shakespeare), so `.chat-a-verse` renders at
/// FULL ink too, not a dimmed tint — that made the quoted source read
/// lighter than the reader's own verse on the same screen. Verse is also
/// upright, never italic (Fix 3) — the card italicizes ONLY stage
/// directions/BCP rubrics, never dialogue.
/// Vertical rhythm: a gloss block is `[speaker?; verse/stage lines; gloss]`.
/// `padding-top: 14px` on the speaker (mirrors the main card's own 14px
/// `speaker-gap` tag) is the only large gap — it opens each new block.
/// Verse/stage lines that follow sit tight (2px) so a multi-line quotation
/// reads as one grouped unit; the gloss gets a clearer break (10px) before
/// the model's own commentary begins. The tightened `padding-bottom` rules
/// need the `.chat-transcript label.chat-a-*` compound selector (class +
/// type + class) to out-specificity the blanket `.chat-transcript label {
/// padding-bottom: 3px }` rule — a bare `.chat-a-verse { padding-bottom }`
/// alone loses that cascade and silently keeps the old 3px gap.
pub fn generate_css(
    theme: &Theme,
    font_family: &str,
    font_size: u32,
    divider_bottom_px: i32,
) -> String {
    let picker_css = picker_css(theme, font_family, font_size);
    let css = format!(
        "window {{ background-color: {root}; \
           font-family: {font}; font-size: 10pt; }} \
         .tiled {{ background-color: {bg}; }} \
         .page-turn-overlay {{ background-color: {bg}; border-radius: 12px; }} \
         /* Pinned chat layout: the card's RIGHT edge is the hairline seam the \
            panel abuts, so square its right corners (keep the left ones rounded) \
            — the card + panel then read as one surface with no root sliver at the \
            seam. Toggled on page_turn_overlay only while a Pinned panel shows. */ \
         .page-turn-overlay.card-chat-seam {{ border-radius: 12px 0 0 12px; }} \
         .page-image-overlay {{ background-color: {bg}; border-radius: 12px; }} \
         .page-image-caption {{ color: {fg}; font-size: 12px; \
           background-color: {bg}; padding: 4px 14px; border-radius: 10px; \
           border: 1px solid {dim}; }} \
         .card-top {{ background-color: {bg}; border-radius: 12px 12px 0 0; }} \
         .running-head {{ padding: 0 40px; }} \
         .running-head-work {{ color: {fg}; font-size: 14px; \
           font-variant: small-caps; letter-spacing: 1px; opacity: 0.72; }} \
         .running-head-scene {{ color: {fg}; font-size: 14px; \
           font-variant: small-caps; letter-spacing: 1px; opacity: 0.72; }} \
         .card-middle {{ background-color: {bg}; border-radius: 0; }} \
         .card-bottom {{ background-color: {bg}; border-radius: 0 0 12px 12px; }} \
         /* Divider spans the full text band: top = the column top where the
            first line can appear (text_view top_margin = 0); bottom margin =
            `descender_guard + TWO_COLUMN_BOTTOM_MARGIN` = the exact reserve the
            two-column FILL leaves below the last possible line
            (`usable = card_h - guard - TWO_COLUMN_BOTTOM_MARGIN`). So the divider
            ends at the SAME baseline as the LAST POSSIBLE line, identically on
            every page (the reserve is fixed, independent of how much text a page
            holds). The caller passes this as `divider_bottom_px` from the live
            `descender_guard_px` so it stays correct across font changes. (Was a
            static 40px matching the view's bottom_margin, then 16px = the fill
            reserve alone, which overshot the last line by the descender guard.)
            Horizontal 8px keeps the gutter gap. */ \
         .column-divider {{ background-color: {dim}; min-width: 1px; \
           margin: 0px 8px {divider_bottom}px 8px; opacity: 0.28; }} \
         textview {{ background-color: {bg}; color: {fg}; }} \
         textview border {{ background-color: {bg}; }} \
         textview border.left {{ background-color: {bg}; }} \
         textview border * {{ background-color: {bg}; background: {bg}; }} \
         textview text {{ background-color: {bg}; color: {fg}; \
           font-family: {font}; font-size: {size}pt; }} \
         {picker_css}\
         .library-picker-scrim {{ background-color: rgba(0, 0, 0, 0.3); }} \
         .corpus-search-detail {{ font-variant: small-caps; \
           font-feature-settings: \"smcp\" 1; letter-spacing: 0.4px; }} \
         .journal-term-input row:selected {{ box-shadow: none; }} \
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
         .amend-title {{ font-size: 18px; font-weight: 700; \
           letter-spacing: 2px; color: {fg}; opacity: 0.75; \
           padding: 14px 22px 10px; }} \
         .amend-text {{ font-family: {font}; font-size: {size}pt; \
           color: {fg}; background-color: {bg}; }} \
         .amend-hint {{ font-size: 15px; letter-spacing: 1.2px; \
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
         /* act/scene chapter-toast: no background, sits on the card's own bg. \
            To restore the pill look, swap the line below for: \
            .chapter-toast {{ font-size: 13px; color: {toast_fg}; \
              background-color: {toast_bg}; padding: 6px 14px; border-radius: 10px; \
              box-shadow: 0 2px 8px rgba(0, 0, 0, 0.18); opacity: 0.97; }} */ \
         .chapter-toast {{ font-size: 13px; color: {fg}; \
           padding: 6px 14px; opacity: 0.97; }} \
         .center-toast {{ font-size: 13px; color: {fg}; \
           background-color: {center_toast_bg}; padding: 6px 14px; border-radius: 10px; \
           box-shadow: 0 2px 8px rgba(0, 0, 0, 0.14); opacity: 0.97; }} \
         .search-toast {{ font-size: 10px; color: {fg}; \
           background-color: {toast_bg}; padding: 3px 10px; border-radius: 8px; \
           box-shadow: 0 2px 6px rgba(0, 0, 0, 0.15); opacity: 0.95; }} \
         .gloss-scrim {{ background-color: {scrim}; }} \
         .legend-scrim {{ background-color: rgba(0, 0, 0, 0.3); }} \
         .gloss-overlay {{ background-color: {gloss_bg}; color: {fg}; border-radius: 12px; \
           box-shadow: 0 8px 32px rgba(0, 0, 0, 0.45); }} \
         .gloss-bottom-clip {{ background-color: {gloss_bg}; }} \
         textview.gloss-text {{ background-color: {gloss_bg}; }} \
         textview.gloss-text text {{ background-color: {gloss_bg}; }} \
         textview.overlay-prose {{ background-color: transparent; color: {fg}; }} \
         textview.overlay-prose text {{ background-color: transparent; color: {fg}; }} \
         .gloss-title {{ font-family: {font}; font-size: {size}pt; font-weight: bold; \
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
         .ask-card .gloss-header {{ margin-top: 0; font-size: 15px; }} \
         textview.ask-input {{ background-color: transparent; color: {fg}; }} \
         textview.ask-input text {{ background-color: transparent; }} \
         .ask-hint {{ font-size: 17px; color: {dim}; }} \
         .ask-legend {{ font-size: 18px; color: {dim}; }} \
         .ask-card.card-focused {{ border-color: {cursor_bg}; \
           border-left: 4px solid {cursor_bg}; \
           box-shadow: 0 8px 24px rgba(0, 0, 0, 0.30); }} \
         .ask-card.card-dimmed {{ opacity: 0.55; }} \
         .definition-panel {{ background-color: {bg}; color: {fg}; \
           border-radius: 12px; padding: 20px 24px; }} \
         .vocab-popup {{ background-color: {root}; color: {vocab_popup_fg}; \
           padding: 16px 20px; border-radius: 12px; }} \
         .vocab-popup.vocab-popup-float {{ background-color: {root}; \
           border: 1px solid alpha({fg}, 0.35); border-radius: 12px; \
           padding: 14px 18px; }} \
         /* Add-vocab card renders on the MAIN CARD's background (not root): \
            its children (gloss-header title, ask-input text, ask-hint mode \
            line) all carry card-ink colors ({fg}/{dim}) that were illegible \
            over the teal root. */ \
         .vocab-add-card {{ background-color: {bg}; \
           border: 1px solid alpha({fg}, 0.35); border-radius: 12px; \
           padding: 6px 10px; box-shadow: 0 8px 24px rgba(0, 0, 0, 0.30); }} \
         .vocab-popup .definition-header {{ font-size: 11px; color: {vocab_popup_dim}; \
           letter-spacing: 2px; font-weight: bold; }} \
         .vocab-popup .definition-word {{ font-size: 16px; color: {vocab_popup_fg}; }} \
         .vocab-popup .definition-text {{ font-size: 16px; color: {vocab_popup_fg}; }} \
         .vocab-popup .definition-etymology {{ opacity: 0.7; font-size: 12px; color: {vocab_popup_fg}; }} \
         .vocab-popup .definition-gloss {{ opacity: 0.7; font-size: 12px; color: {vocab_popup_fg}; }} \
         .vocab-popup .definition-hint {{ font-size: 11px; color: {vocab_popup_dim}; \
           border-top: 1px solid {vocab_popup_border}; padding-top: 8px; margin-top: 12px; }} \
         .vocab-popup .journal-pin {{ border-top: 1px solid {vocab_popup_border}; \
           padding-top: 10px; margin-top: 12px; }} \
         .concordance-picker {{ background-color: {root}; color: {vocab_popup_fg}; \
           padding: 16px; border-radius: 12px; border: 1px solid {dim}; }} \
         .concordance-picker entry {{ margin-bottom: 8px; \
           background-color: {bg}; color: {fg}; }} \
         .concordance-picker list {{ background-color: transparent; }} \
         .concordance-picker row {{ background-color: transparent; \
           color: {vocab_popup_fg}; }} \
         .concordance-picker row label {{ color: {vocab_popup_fg}; }} \
         .concordance-picker row:selected {{ \
           background-color: alpha({vocab_popup_fg}, 0.18); color: {vocab_popup_fg}; }} \
         .concordance-picker row:selected label {{ color: {vocab_popup_fg}; }} \
         .concordance-picker .settings-title {{ color: {vocab_popup_fg}; \
           border-bottom: 1px solid alpha({vocab_popup_fg}, 0.3); }} \
         .concordance-picker .settings-footer {{ color: {vocab_popup_fg}; opacity: 0.75; }} \
         .concordance-bar {{ background-color: {root}; padding: 4px 12px; }} \
         .concordance-bar-word {{ color: {dim}; font-size: 12px; }} \
         .concordance-bar-position {{ color: {dim}; font-size: 14px; }} \
         .concordance-bar-hint {{ color: {dim}; font-size: 12px; opacity: 0.6; }} \
         .title-bar {{ background-color: {root}; padding: 4px 12px; }} \
         .title-bar-label {{ color: {dim}; font-size: 14px; }} \
         .title-bar-hint {{ color: {dim}; font-size: 12px; opacity: 0.6; }} \
         /* Pinned (single-column) panel is a card surface too: it shares the \
            main card's background and ink, so it reads like an extension of the \
            card rather than pale text floating on the bare root. (The float \
            placement gets the same treatment via .chat-panel-float below; the \
            Pinned panel stands alone beside the card with its own outer margin, \
            so it needs no border to separate it from a reading column.) */ \
         /* Top padding is 0 (sides/bottom stay 12px): the panel is positioned \
            (size_panel) so the transcript's FIRST line lands level with the \
            main card's first reading line, and any top padding here would push \
            that first line down below it. `chat_first_line_top_margin` assumes \
            this zero — keep the two in sync (it also assumes .chat-transcript's \
            zero padding-top below). */ \
         .chat-panel {{ background-color: {bg}; padding: 0 12px 12px 12px; }} \
         /* Focus cue: a short rule (~ three hyphens) marking whichever surface \
            (panel or card) has focus while the chat layout is open. Shared by \
            the panel's own copy (ChatPanel::new) and the main card's twin \
            (page_turn_overlay overlay child); exactly one shows at a time \
            (see chat::update_focus_rules). */ \
         .focus-rule {{ background-color: alpha({fg}, 0.55); \
           border-radius: 1px; }} \
         /* Pinned placement: the panel abuts the card's right edge, separated \
            only by a 1px hairline seam (its border-left, painting over the 1px \
            gap size_panel leaves). Rounded on the RIGHT (outer) corners only — \
            the left/seam corners stay square so card+panel read as one cream \
            block split by the hairline. 12px matches the card's own \
            .page-turn-overlay radius. */ \
         .chat-panel-pinned {{ border-left: 1px solid alpha({fg}, 0.25); \
           border-radius: 0 12px 12px 0; }} \
         /* No TOP and no BOTTOM border. The float panel sits directly below the \
            card's header band (Fix 6's margin_top clears it) and its bottom now \
            stops at the two-column divider's end (size_panel), so neither a top \
            rule (nothing above it to separate FROM) nor a bottom rule (the panel \
            edge is already the divider's end) reads as a boundary. Only \
            LEFT/RIGHT stay — they separate the floating panel from the reading \
            column it overlaps. No border-radius: the bottom-right curve only \
            made sense as a stroked corner nesting in the card, and there is no \
            stroked bottom edge now. */ \
         .chat-panel-float {{ background-color: {bg}; \
           border-left: 1px solid alpha({fg}, 0.25); \
           border-right: 1px solid alpha({fg}, 0.25); \
           border-radius: 0; \
           padding: 12px; }} \
         /* Fix 5: the transcript renders at the CARD's own font_size, not a \
            shrunk one — source lines AND the explication/gloss paragraphs \
            must match the main card's body text (see generate_css's doc \
            comment). Was `font_size.saturating_sub(2).max(8)` (a local \
            `chat_size`, removed); the narrower panel still wraps fine at \
            full size (labels wrap via WordChar, see chat_panel::append_spec_label), so \
            nothing else in this stylesheet depended on the smaller size. */ \
         /* padding-right keeps the wrapped text clear of the scrollbar — it \
            is on the SCROLLED content Box (.chat-transcript), inside the \
            viewport, so the gutter sits between the text's right edge and the \
            bar rather than being eaten by it. */ \
         /* padding-top is 0 so the transcript's first line sits at the panel's \
            content top — the panel is positioned to align that first line with \
            the card's first reading line (see .chat-panel's note and \
            `chat_first_line_top_margin`). Bottom keeps its 16px breathing room. */ \
         .chat-transcript {{ font-family: {font}; font-size: {size}pt; \
           padding-right: 14px; padding-top: 0px; padding-bottom: 16px; }} \
         /* padding-right keeps the row text clear of the cursor accent bar \
            (.chat-cursor-row's inset -3px box-shadow paints at the label's \
            right edge): 6px pad − 3px bar = a 3px gap. Applied to EVERY row \
            (not just the cursor row) so a cursor move never rewraps text. \
            MIRRORED by chat_panel::transcript_wrap_width — keep in sync. */ \
         .chat-transcript label {{ padding-bottom: 3px; padding-right: 6px; }} \
         .chat-transcript-scroll {{ background-color: transparent; \
           border-radius: 8px; \
           transition: background-color 320ms ease-out; }} \
         /* Pinned panel breathing room: a top MARGIN on the scroll viewport, \
            NOT padding. The scroll node clips its content at its own top edge; \
            padding is INSIDE that clip box, so scrolled content renders under \
            it and the top line shows severed (the clipping bug). Margin sits \
            OUTSIDE the clip box, so the viewport starts BELOW the gap: the \
            panel now PAGINATES rather than scrolling to the cursor \
            (`chat_panel::render_page` renders a whole-widget page slice that \
            fits the transcript budget by construction), so this margin is \
            simply the top breathing gap above that page's first line — no \
            scroll/vadjustment involved. Float keeps 0 (it \
            first-line-aligns via `chat_first_line_top_margin`). */ \
         .chat-panel-pinned .chat-transcript-scroll {{ margin-top: 40px; }} \
         .chat-transcript-scroll.chat-flash-wash {{ \
           background-color: alpha({chat_ink}, 0.10); transition: none; }} \
         .chat-panel-float .chat-transcript-scroll.chat-flash-wash {{ \
           background-color: alpha({fg}, 0.10); }} \
         /* Prose rows keep the airy gap the transcript Box used to supply for \
            every row (spacing 10). It is padding now: the Box went to spacing \
            0 so SOURCE lines could sit at the main card's pure line-height, \
            and these rows would otherwise jam together. */ \
         .chat-q {{ color: {chat_ink}; padding-top: 10px; \
           padding-left: {q_body}px; }} \
         .chat-a {{ color: {chat_ink}; padding-top: 10px; \
           padding-left: {q_body}px; }} \
         .chat-chip {{ color: alpha({chat_ink}, 0.55); font-style: italic; \
           border-left: 2px solid alpha({chat_ink}, 0.35); padding-left: 8px; \
           padding-top: 10px; }} \
         .chat-error {{ color: alpha({chat_ink}, 0.55); font-style: italic; \
           padding-top: 10px; }} \
         .chat-saved {{ color: alpha({chat_ink}, 0.65); padding-top: 10px; }} \
         /* Page-marker footer under the transcript (chat_panel::set_page_marker):
            the overlays' ⌄/• glyphs at their ~20px size, in the panel's dim
            ink tier. The row itself is always present (fixed height) so the
            transcript budget never churns with marker visibility. */ \
         .chat-page-marker {{ color: alpha({chat_ink}, 0.55); font-size: 20px; }} \
         /* Source lines mirror the MAIN CARD's spacing model (formatting.rs): \
            the card sets pixels_above/below_lines(0) globally, so consecutive \
            verse lines sit at pure line-height with NO gap, and space appears \
            only BETWEEN blocks — 14px above a speaker (the card's speaker-gap \
            tag), 8px above a stage direction (stage-direction-gap). The Box \
            therefore uses spacing 0: every gap here is padding, or the Box's \
            own spacing would add itself to all of them. */ \
         /* Fix 4: speaker labels must match the CARD's own speaker-name \
            TextTag (formatting.rs:191-195) — variant: small-caps, weight: \
            400 (normal), scale: 0.75 (relative to the card's body font) — \
            and carry NO color override, so (like the card) it reads at FULL \
            ink, not dimmed. `font-size: 0.75em` is the CSS analogue of the \
            card's Pango `scale(0.75)`: `em` is relative to the element's \
            inherited font-size exactly as Pango scale is relative to the \
            view's base font, and `.chat-transcript`'s base IS the reader \
            font (Fix 5 sets it to the card's own font_size, not a shrunk \
            one), so 0.75em here lands the speaker at the same PROPORTION of \
            body text the card uses. Dropped `letter-spacing` (the card's tag \
            has none — small-caps + scale alone is what makes it read \
            quiet, per the card's own doc comment) and the 0.82em size. */ \
         .chat-a-speaker {{ color: {chat_ink}; font-weight: normal; \
           font-variant: small-caps; font-size: 0.75em; \
           padding-top: 14px; padding-left: {q_speaker}px; }} \
         /* The FIRST source row of a gloss block (tagged chat-a-src-lead by \
            chat_panel::gloss_answer_specs) carries the gap separating this block source \
            from the PRECEDING block gloss commentary. GTK CSS has no sibling \
            selector for source-after-gloss, so the row builder tags the block \
            leading source line and this rule adds the gap — only ONCE per \
            block, not per source line (multi-line prose source would otherwise \
            get 18px between every line). Matched to the gloss 18px top so the \
            rhythm reads evenly. \
            COMPOUND selector (class + type + class → specificity 0,0,2,1) so \
            it out-specificities the single-class base source rules \
            (.chat-a-verse/.chat-a-stage/.chat-a-*-flush, all 0,0,1,0) REGARDLESS \
            of their order in the stylesheet. padding-top is non-additive \
            (replaces, not adds), so this pins the rendered top to 44px for the \
            src-lead row whatever its base class is. Mirrors the padding-BOTTOM \
            compound rules below (.chat-transcript label.chat-a-X). */ \
         .chat-transcript label.chat-a-src-lead {{ padding-top: 26px; }} \
         .chat-transcript label.chat-a-speaker {{ padding-bottom: 0px; }} \
         /* Fix 3: the main card never italicizes dialogue/verse — \
            formatting.rs's Style::Italic appears ONLY on stage directions \
            (stage-direction-style) and BCP rubrics, never on speaker-name or \
            plain dialogue lines. Quoted source verse in the panel must match: \
            upright, not italic. */ \
         .chat-a-verse {{ color: {chat_ink}; \
           padding-left: {q_verse}px; padding-top: 0px; }} \
         .chat-transcript label.chat-a-verse {{ padding-bottom: 0px; }} \
         /* Stage directions KEEP italic — this matches the card's \
            stage-direction-style tag (Style::Italic), unlike verse above. */ \
         .chat-a-stage {{ color: alpha({chat_ink}, 0.55); font-style: italic; \
           padding-left: {q_verse}px; padding-top: 8px; }} \
         .chat-transcript label.chat-a-stage {{ padding-bottom: 0px; }} \
         /* Speakerless (prose) source: no label to hang past, so verse/stage \
            sit at the shallower speaker indent instead of the deep verse one \
            (mirrors gloss_render::populate_verse_buffer's has_speaker branch). */ \
         /* Prose source has no speaker heading or deep verse indent to set it \
            apart from the model's commentary, so italicize it — quoted prose \
            then reads as clearly distinct from the upright gloss. Play/verse \
            source (.chat-a-verse) keeps its upright weight (it has a speaker \
            + deep indent already). \
            Flush rows sit at the panel's OWN text margin (padding-left 0, \
            not the old speaker indent) and render at 0.9em, so a source \
            line as wrapped by the MAIN CARD fits on one panel row — the \
            user's rule: the panel must fully accommodate a card-rendered \
            source line. (Pagination measures at full size/width, so the \
            smaller render only over-counts — clip-safe.) */ \
         .chat-a-verse-flush {{ color: {chat_ink}; font-style: italic; \
           font-size: 0.9em; \
           padding-left: 0px; padding-top: 0px; }} \
         .chat-transcript label.chat-a-verse-flush {{ padding-bottom: 0px; }} \
         .chat-a-stage-flush {{ color: alpha({chat_ink}, 0.55); font-style: italic; \
           font-size: 0.9em; \
           padding-left: 0px; padding-top: 8px; }} \
         .chat-transcript label.chat-a-stage-flush {{ padding-bottom: 0px; }} \
         .chat-a-gloss {{ color: {chat_ink}; padding-top: 26px; padding-left: {q_body}px; }} \
         .chat-panel-float .chat-q {{ color: {fg}; }} \
         .chat-panel-float .chat-a {{ color: {fg}; }} \
         .chat-panel-float .chat-chip {{ color: {dim}; \
           border-left: 2px solid alpha({fg}, 0.35); }} \
         .chat-panel-float .chat-error {{ color: alpha({fg}, 0.55); }} \
         .chat-panel-float .chat-saved {{ color: {dim}; }} \
         /* Float variant: {fg} is the float body colour (see the doc comment \
            on .chat-panel-float's other rows below) — full ink, matching the \
            pinned rule's {chat_ink} above. Was {dim}; that dimmed the label \
            against Fix 4's brief. */ \
         .chat-panel-float .chat-a-speaker {{ color: {fg}; }} \
         .chat-panel-float .chat-a-verse {{ color: {fg}; }} \
         .chat-panel-float .chat-a-verse-flush {{ color: {fg}; }} \
         .chat-panel-float .chat-a-stage {{ color: alpha({fg}, 0.55); }} \
         .chat-panel-float .chat-a-stage-flush {{ color: alpha({fg}, 0.55); }} \
         .chat-panel-float .chat-a-gloss {{ color: {fg}; }} \
         /* j/k row cursor (CHANGE 1): accent bar to the left of the cursor \
            row, mirroring the reading card's cursorline treatment and the \
            gloss overlay's accent bar — reusing {cursor_bg}, the same accent \
            `.ask-card.card-focused`'s border-left already uses. An INSET \
            box-shadow (like `.library-picker row:selected`'s left accent, \
            theme.rs:1040) rather than `border-left`/`padding-left`: those two \
            properties are already spoken for by the row's own indent class \
            (chat-a-verse etc. sets padding-left directly), and CSS can't sum \
            two padding-left declarations — a shadow paints the bar without \
            consuming layout space, so it composes with ANY indent class. The \
            background wash makes a bare-on-root Answer/Question row (no \
            indent of its own) still read as the current line, not just a \
            thin rule. */ \
         /* Cursor row: the inset accent bar alone marks it — NO background \
            wash (it read as a distracting highlight over the cursor segment; \
            the bar suffices, matching the reading card's own cursor cue). \
            The bar insets from the RIGHT edge (negative x offset) — the \
            user wants it beside the segment's right side, clear of the \
            panel's left border. */ \
         .chat-cursor-row {{ box-shadow: inset -3px 0 0 {cursor_bg}; }} \
         /* V panel-local visual selection (chat panel's own y yank -
            distinct from the reader's AppState.visual_selection, a
            different space entirely). Reuses selection_bg, the SAME blue
            wash the reading card's visual-selection tag and the gloss/
            synopsis/journal hi marker already use, so a selected row reads
            as selected in the same color everywhere in the app.
            chat-cursor-row may paint on the SAME widget as the anchor/
            single-row-selection case; the two backgrounds simply layer
            (cursor's box-shadow accent bar stays on top), so no override
            is needed. */ \
         .chat-visual-row {{ background-color: {selection_bg}; }} \
         /* `y` copy-confirmation flash (replaces the old \"Copied N lines\" \
            toast): a brief wash on the ROW(S) actually copied, reusing \
            cursor_bg so it reads as the same accent color as the cursor bar \
            / flash-active input border - one \"this is the active accent\" \
            color across the panel. Asymmetric transition, mirroring \
            .chat-transcript-scroll.chat-flash-wash: `transition: none` here \
            so the wash appears INSTANTLY on add (a confirmation should not \
            fade in, that reads as sluggish), while the base \
            .chat-transcript label rule (implicit, inherited) has no \
            transition of its own - so add scripted `remove_css_class` after \
            160ms is a hard cut, not a fade. Kept quiet (low alpha, no \
            border/shadow) so it does not fight `.chat-cursor-row`'s inset \
            accent bar or `.chat-visual-row`'s selection wash when a flashed \
            row is ALSO the cursor row or in the just-cleared selection - the \
            three backgrounds simply layer like chat-cursor-row/chat-visual-row \
            already do (see that comment above). */ \
         .chat-flash-row {{ background-color: alpha({cursor_bg}, 0.22); \
           transition: none; }} \
         .chat-input {{ background-color: {bg}; \
           border: 1px solid alpha({fg}, 0.30); border-radius: 6px; \
           transition: border-color 320ms ease-out, \
                       background-color 320ms ease-out, \
                       box-shadow 320ms ease-out; }} \
         .chat-input.chat-flash-active {{ \
           border: 2px solid {cursor_bg}; \
           background-color: alpha({cursor_bg}, 0.12); \
           box-shadow: 0 0 0 2px alpha({cursor_bg}, 0.30); \
           transition: none; }} \
         .picker-box {{ background-color: {root}; color: {vocab_popup_fg}; \
           padding: 16px; border-radius: 8px; }} \
         .picker-box list {{ background-color: transparent; }} \
         .picker-box row {{ background-color: transparent; color: {vocab_popup_fg}; }} \
         .picker-box row label {{ color: {vocab_popup_fg}; }} \
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
         .picker-entry {{ margin-bottom: 8px; background-color: {bg}; color: {fg}; }} \
         .picker-list row:selected {{ background-color: alpha({vocab_popup_fg}, 0.18); \
           color: {vocab_popup_fg}; }} \
         .picker-list row:selected label {{ color: {vocab_popup_fg}; }} \
         .picker-item-title {{ }} \
         .picker-item-detail {{ opacity: 0.6; }} \
         .picker-header {{ font-size: 14px; font-weight: bold; color: {vocab_popup_fg}; }} \
",
        root = theme.root_color,
        scrim = theme.scrim_bg,
        bg = theme.text_bg,
        gloss_bg = gloss_background(theme),
        fg = theme.text_fg,
        dim = theme.dim_fg,
        toast_bg = chapter_toast_bg(theme),
        toast_fg = contrast_on(&chapter_toast_bg(theme)),
        center_toast_bg = center_toast_bg(theme),
        cursor_bg = theme.cursor_bg,
        cursor_fg = theme.cursor_fg,
        selection_bg = crate::theme::selection_bg(theme),
        vocab_popup_fg = vocab_popup_fg(theme),
        vocab_popup_dim = vocab_popup_tier(
            &vocab_popup_ink(theme), &theme.root_color, 0.55, VOCAB_POPUP_DIM_MIN_CONTRAST),
        vocab_popup_border = blend_colors(&vocab_popup_ink(theme), &theme.root_color, 0.30),
        header_border = blend_colors(&theme.dim_fg, &theme.text_bg, 0.5),
        font = font_family,
        size = font_size,
        // The panel (Pinned and float alike) now sits on the card background
        // {bg}, so its ink must contrast the CARD, not the root — use the card
        // foreground directly, matching the float overrides' {fg}. (Was
        // contrast_on(root_color), correct only while the Pinned panel was
        // transparent-over-root.)
        chat_ink = theme.text_fg,
        divider_bottom = divider_bottom_px,
        q_body = CHAT_QUOTE_BODY_INDENT,
        q_speaker = CHAT_QUOTE_SPEAKER_INDENT,
        q_verse = CHAT_QUOTE_VERSE_INDENT,
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

    // Env-driven contrast report for the `colorscheme` skill. NOT a real test
    // (always passes); it is a harness the skill's script invokes via
    // `cargo test contrast_report_harness -- --nocapture` to compute the app's
    // OWN contrast for candidate colors, so the skill's verdict matches
    // production. Reuses the private contrast_ratio (same module) — zero
    // production surface (#[cfg(test)] only). Silent no-op when the env var is
    // unset, so a normal `cargo test` run prints nothing.
    //
    // Input:  LIT_CONTRAST_THEME=<name>   (resolves card bg / ink / root)
    //         LIT_CONTRAST_COLORS=key=#hex,key=#hex,...
    //           keys: gloss gloss-cursor vocab-fg search root phrase cursor-line
    // Output (stdout, one line per foreground color × surface):
    //   CONTRAST <key> vs <surface> <ratio> <floor> PASS|FAIL
    //   plus a final `CONTRAST_SUMMARY PASS|FAIL <nfail>` line.
    #[test]
    fn contrast_report_harness() {
        let Ok(colors_spec) = std::env::var("LIT_CONTRAST_COLORS") else {
            return; // not invoked by the skill; no-op
        };
        let theme_name =
            std::env::var("LIT_CONTRAST_THEME").unwrap_or_else(|_| "default".to_string());
        let theme = super::load_theme(&theme_name);
        let bg = theme.text_bg.clone();
        let ink = theme.text_fg.clone();
        let root = theme.root_color.clone();

        // Foreground colors (must be READABLE) get the full bg/root/ink matrix
        // at their app floor; tints (phrase/cursor-line) are backgrounds, so we
        // only report their contrast vs the card bg for visibility of overlaid
        // text and skip the fg-only surfaces.
        let fg_floor = READER_GLOSS_MIN_CONTRAST; // 4.5
        let nfail = std::cell::Cell::new(0u32);
        let check = |key: &str, color: &str, surface: &str, surf_hex: &str, floor: f64| {
            // rgba tints: composite over the card bg before measuring.
            let solid = if color.trim_start().starts_with("rgba(") {
                let (r, g, b) = super::rgba_str_to_rgb(color);
                format!("#{:02x}{:02x}{:02x}", r as u8, g as u8, b as u8)
            } else {
                color.to_string()
            };
            let ratio = contrast_ratio(&solid, surf_hex);
            let verdict = if ratio >= floor { "PASS" } else { "FAIL" };
            if ratio < floor {
                nfail.set(nfail.get() + 1);
            }
            println!("CONTRAST {key} vs {surface} {ratio:.2} {floor:.2} {verdict}");
        };

        for pair in colors_spec.split(',') {
            let Some((key, hex)) = pair.split_once('=') else { continue };
            let (key, hex) = (key.trim(), hex.trim());
            match key {
                // Foreground colors: full matrix (bg 4.5, root 1.8, ink 1.5).
                "gloss" | "gloss-cursor" | "vocab-fg" | "search" => {
                    check(key, hex, "bg", &bg, fg_floor);
                    check(key, hex, "root", &root, READER_GLOSS_ROOT_MIN_CONTRAST);
                    check(key, hex, "ink", &ink, READER_GLOSS_INK_MIN_CONTRAST);
                }
                // Tint backgrounds: report vs bg only (visibility of the tint).
                "phrase" | "cursor-line" => {
                    // A tint is "visible" if it differs enough from bg; we use a
                    // low informational floor (not an app-enforced gate).
                    check(key, hex, "bg", &bg, 1.05);
                }
                // Root itself: report vs bg + ink so the wallpaper isn't a
                // near-match of the card or the body text.
                "root" => {
                    check(key, hex, "bg", &bg, 1.10);
                    check(key, hex, "ink", &ink, 1.10);
                }
                _ => {
                    println!("CONTRAST_ERROR unknown key {key}");
                    nfail.set(nfail.get() + 1);
                }
            }
        }
        let n = nfail.get();
        let summary = if n == 0 { "PASS" } else { "FAIL" };
        println!("CONTRAST_SUMMARY {summary} {n}");
    }

    #[test]
    fn vocab_popup_ink_contrasts_with_root_variants() {
        // Dark root: the default theme's light text_fg wins over its dark
        // text_bg, and stays legible.
        let mut theme = default_theme();
        assert_eq!(vocab_popup_ink(&theme), theme.text_fg);
        assert!(contrast_ratio(&vocab_popup_ink(&theme), &theme.root_color) >= 4.5);
        // Mid-tone light root (the blue-gray variant that washed the popup
        // out): the theme's dark ink only reaches ~5.9:1, below the AAA
        // floor, so the ink falls back to pure black.
        theme.root_color = "#9fbecd".to_string();
        theme.text_bg = "#fdf6e3".to_string();
        theme.text_fg = "#3a3532".to_string();
        assert_eq!(vocab_popup_ink(&theme), "#000000");
        assert!(contrast_ratio(&vocab_popup_ink(&theme), &theme.root_color) >= 7.0);
        // Near-black root variant of the same theme: cream card color wins.
        theme.root_color = "#14141c".to_string();
        assert_eq!(vocab_popup_ink(&theme), theme.text_bg);
        assert!(contrast_ratio(&vocab_popup_ink(&theme), &theme.root_color) >= 4.5);
        // Mid-tone root (screenshot 2026-07-11): BOTH theme colors are weak
        // (dark ink ~4:1, cream ~2.7:1) — the ink must fall back to the
        // maximizing pure black, and every tier must clear its floor.
        theme.root_color = "#6f9fb3".to_string();
        assert_eq!(vocab_popup_ink(&theme), "#000000");
        let ink = vocab_popup_ink(&theme);
        let fg = vocab_popup_tier(&ink, &theme.root_color, 0.85, VOCAB_POPUP_MIN_CONTRAST);
        let dim = vocab_popup_tier(&ink, &theme.root_color, 0.55, VOCAB_POPUP_DIM_MIN_CONTRAST);
        assert!(contrast_ratio(&fg, &theme.root_color) >= VOCAB_POPUP_MIN_CONTRAST);
        assert!(contrast_ratio(&dim, &theme.root_color) >= VOCAB_POPUP_DIM_MIN_CONTRAST);
        // Darker-than-mid root: the perceptual pick prefers white even
        // though black scores the higher WCAG ratio there.
        theme.root_color = "#5c8da1".to_string();
        assert_eq!(vocab_popup_ink(&theme), "#ffffff");
    }

    #[test]
    fn ensure_keeps_already_good_color() {
        // rose-pine-dawn focuscolor on cream, avoiding slate body text: passes
        // the plain 3.0 UI floor, so returned unchanged there.
        let c = ensure_gloss_color_min("#c4788a", "#faf4ed", None, None, &["#575279"], 3.0);
        assert_eq!(c, "#c4788a", "a color that already passes the 3.0 floor must be returned unchanged");
    }

    #[test]
    fn reader_gloss_floor_darkens_a_washed_out_pastel() {
        // The user's case: #c4788a on cream is exactly ~3.0 — readable as UI but
        // washed out as body text. The reader-gloss floor must push it DARKER
        // (higher contrast) while keeping it distinct from the slate body text.
        let bg = "#faf4ed";
        let before = contrast_ratio("#c4788a", bg);
        let c = ensure_gloss_color_min("#c4788a", bg, None, None, &["#575279"], READER_GLOSS_MIN_CONTRAST);
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
        let c = ensure_gloss_color_min("#7b6b99", "#f6f2ee", None, None, &["#3d2b5a"], 3.0);
        assert!(contrast_ratio(&c, "#f6f2ee") >= 3.0,
            "fixed color must contrast with bg, got {} ({c})", contrast_ratio(&c, "#f6f2ee"));
    }

    #[test]
    fn ensure_result_is_distinct_from_avoid() {
        let c = ensure_gloss_color_min("#7b6b99", "#f6f2ee", None, None, &["#3d2b5a"], 3.0);
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
    fn text_fg_prefers_kitty_foreground_over_active_tab() {
        // The reader ink matches the kitty theme's font color (kitty.foreground),
        // not the active_tab_foreground accent. zenbones-light shape: charcoal
        // foreground, steel-blue active_tab_foreground.
        let json: serde_json::Value = serde_json::from_str(
            r##"{ "meta": {"type": "light"},
                 "kitty": {"background": "#f0edec", "foreground": "#2c363c",
                           "active_tab_foreground": "#286486"} }"##,
        ).unwrap();
        let t = resolve_theme("z", &json);
        assert_eq!(t.text_fg, "#2c363c");
    }

    #[test]
    fn text_fg_falls_back_to_active_tab_when_no_foreground() {
        // Themes without kitty.foreground keep using active_tab_foreground.
        let json: serde_json::Value = serde_json::from_str(
            r##"{ "meta": {"type": "light"},
                 "kitty": {"background": "#fdfbf2", "active_tab_foreground": "#5d4232"} }"##,
        ).unwrap();
        let t = resolve_theme("s", &json);
        assert_eq!(t.text_fg, "#5d4232");
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
            // A theme may deliberately set the two gloss states to the SAME
            // color (one gloss color for both — the cursor line still stands
            // out via its own background tint). Only the DERIVED-complement pair
            // must be distinct; identical is an allowed explicit choice.
            let same = t.reader_gloss.eq_ignore_ascii_case(&t.reader_gloss_cursor);
            let distinct = hue_distance(&t.reader_gloss, &t.reader_gloss_cursor) >= 40.0
                || contrast_ratio(&t.reader_gloss, &t.reader_gloss_cursor) >= 1.4;
            assert!(same || distinct, "{}: off {} and on {} neither identical nor distinct", t.name, t.reader_gloss, t.reader_gloss_cursor);
            // Root distinctness (hue OR contrast) + ink rule (light themes:
            // luminance gap, near-black ink is hue-blind; dark themes:
            // hue-OR-contrast, their ink is mid-light).
            for (label, tint) in [("off", &t.reader_gloss), ("cursor", &t.reader_gloss_cursor)] {
                let root_ok = hue_distance(tint, &t.root_color) >= 40.0
                    || contrast_ratio(tint, &t.root_color) >= READER_GLOSS_ROOT_MIN_CONTRAST;
                assert!(root_ok, "{}: {label} tint {tint} too close to root {}", t.name, t.root_color);
                let cvi = contrast_ratio(tint, &t.text_fg);
                let ink_ok = if t.is_light {
                    cvi >= READER_GLOSS_INK_MIN_CONTRAST
                } else {
                    hue_distance(tint, &t.text_fg) >= 40.0 || cvi >= 1.4
                };
                assert!(ink_ok,
                    "{}: {label} tint {tint} reads as body ink {} ({cvi:.2})", t.name, t.text_fg);
            }
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

    #[test]
    fn search_highlight_colors_are_same_hue_current_stronger() {
        let mut t = default_theme();
        t.phrase_highlight_bg = "rgba(93, 66, 50, 0.18)".to_string();
        let (all, current) = t.search_highlight_colors();
        // `all` = the base phrase tint verbatim.
        assert_eq!(all, "rgba(93, 66, 50, 0.18)");
        // `current` = same RGB, higher alpha (0.18 * 2.4 = 0.432).
        assert_eq!(current, "rgba(93, 66, 50, 0.432)");
        // Same hue: both start with the same RGB triple.
        assert!(current.starts_with("rgba(93, 66, 50,"));
    }

    const CANDIDATES_JSON: &str = r##"{ "meta": {"display": "S", "type": "light"},
        "dwl": {"rootcolor": "#08526b", "focuscolor": "#8a6a45",
                "rootcolor_candidates": ["#41819b", "#286983", "#08526b"]},
        "kitty": {"background": "#e7dec7", "active_tab_foreground": "#5d4232"},
        "linux-lit": {"cursor_line_bg": "rgba(93, 66, 50, 0.14)"} }"##;

    #[test]
    fn card_surface_is_identical_across_root_variants() {
        // Only root_color (and its derivative scrim_bg) vary with the root
        // variant; the card surface stays byte-identical to the base.
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let base = resolve_theme("s", &json);
        let count = resolve_theme_variant("s", &json, 0).root_variant_count;
        for variant in 0..count {
            let v = resolve_theme_variant("s", &json, variant);
            assert_eq!(base.text_bg, v.text_bg);
            assert_eq!(base.cursor_line_bg, v.cursor_line_bg);
            // phrase_highlight_bg deliberately EXCLUDED: the karaoke tint
            // follows the root, so it varies per variant (see
            // phrase_tint_tracks_the_root_variant).
            assert_eq!(base.dim_fg, v.dim_fg);
            assert_eq!(v.root_variant, variant);
        }
    }

    #[test]
    fn phrase_tint_tracks_the_root_variant() {
        // The karaoke tint IS the active root color (per-variant rgb) at the
        // theme's tuned alpha: no explicit key → 0.28 default; an explicit
        // phrase_highlight_bg contributes only its alpha.
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let v0 = resolve_theme_variant("s", &json, 0); // #41819b
        let v2 = resolve_theme_variant("s", &json, 2); // #08526b
        assert_eq!(v0.phrase_highlight_bg, "rgba(65, 129, 155, 0.28)");
        assert_eq!(v2.phrase_highlight_bg, "rgba(8, 82, 107, 0.28)");

        let tuned: serde_json::Value = serde_json::from_str(
            r##"{ "meta": {"display": "S", "type": "light"},
                "dwl": {"rootcolor": "#08526b", "focuscolor": "#8a6a45",
                        "rootcolor_candidates": ["#41819b", "#286983"]},
                "kitty": {"background": "#e7dec7", "active_tab_foreground": "#5d4232"},
                "linux-lit": {"phrase_highlight_bg": "rgba(93, 66, 50, 0.14)"} }"##,
        )
        .unwrap();
        let v = resolve_theme_variant("s", &tuned, 0);
        assert_eq!(v.phrase_highlight_bg, "rgba(65, 129, 155, 0.14)");
    }

    #[test]
    fn root_variants_are_the_candidates_verbatim() {
        // Root variants ARE dwl.rootcolor_candidates, in authored order — no
        // re-sort, no dedup, no skipping the entry equal to the designed root.
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let want = ["#41819b", "#286983", "#08526b"];
        for (i, w) in want.iter().enumerate() {
            let v = resolve_theme_variant("s", &json, i as u8);
            assert_eq!(v.root_color, *w, "variant {i} root_color");
            assert_eq!(v.root_variant, i as u8, "variant {i} index");
        }
    }

    #[test]
    fn per_root_gloss_override_applies_only_on_its_root() {
        // A reader_gloss_by_root entry keyed by a candidate root wins on THAT
        // variant (verbatim when it clears the floors); other variants fall
        // back to the theme-wide explicit reader_gloss. Same for the cursor
        // map. Case-insensitive key match.
        let json: serde_json::Value = serde_json::from_str(
            r##"{ "meta": {"display": "S", "type": "light"},
                "dwl": {"rootcolor": "#08526b", "focuscolor": "#8a6a45",
                        "rootcolor_candidates": ["#41819b", "#B5637A"]},
                "kitty": {"background": "#f0ede4", "active_tab_foreground": "#2c2c2c"},
                "linux-lit": {"reader_gloss": "#8c5a3c",
                              "reader_gloss_cursor": "#8c5a3c",
                              "reader_gloss_by_root": {"#b5637a": "#286b49"},
                              "reader_gloss_cursor_by_root": {"#b5637a": "#286b49"}} }"##,
        )
        .unwrap();
        let teal = resolve_theme_variant("s", &json, 0);
        assert_eq!(teal.reader_gloss, "#8c5a3c", "non-matching root keeps the theme-wide gloss");
        assert_eq!(teal.reader_gloss_cursor, "#8c5a3c");
        let rose = resolve_theme_variant("s", &json, 1);
        assert_eq!(rose.reader_gloss, "#286b49", "rose root gets its per-root gloss verbatim");
        assert_eq!(rose.reader_gloss_cursor, "#286b49");
    }

    #[test]
    fn root_variant_count_matches_candidate_list() {
        // count = candidate list length (3 here).
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let v = resolve_theme_variant("s", &json, 0);
        assert_eq!(v.root_variant_count, 3);
    }

    #[test]
    fn out_of_range_variant_clamps_to_zero() {
        // A saved index past the candidate count resets to 0 (first candidate).
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let v = resolve_theme_variant("s", &json, 5);
        assert_eq!(v.root_color, "#41819b"); // candidates[0]
        assert_eq!(v.root_variant, 0);
    }

    #[test]
    fn light_theme_toast_matches_the_cursor_line_band() {
        // CANDIDATES_JSON is kindle-sepia-shaped (light card #e7dec7, cursor
        // line rgba(93,66,50,0.14)). The toast chip must equal that cursor-line
        // tint composited over the card — the exact band the reader sees under
        // the current line — NOT a root-tinted (teal) chip.
        let json: serde_json::Value = serde_json::from_str(CANDIDATES_JSON).unwrap();
        let t = resolve_theme("s", &json);
        assert!(t.is_light);
        let want = blend_colors("#5d4232", &t.text_bg, 0.14);
        assert_eq!(chapter_toast_bg(&t), want);
    }

    #[test]
    fn dark_theme_toast_still_uses_the_root_hue() {
        // Dark themes keep the softened-root chip (root mixed toward the card,
        // then lifted toward white) — the light-theme cursor-line path must not
        // capture them.
        let t = default_theme();
        assert!(!t.is_light);
        let tinted = blend_colors(&t.root_color, &t.text_bg, CHAPTER_TOAST_ROOT_MIX);
        let want = blend_colors("#ffffff", &tinted, CHAPTER_TOAST_WHITE_LIFT);
        assert_eq!(chapter_toast_bg(&t), want);
    }

    #[test]
    fn no_candidates_falls_back_to_designed_root() {
        // No rootcolor_candidates → single variant = dwl.rootcolor, count 1.
        let json: serde_json::Value = serde_json::from_str(
            r##"{ "meta": {"type": "light"},
                  "dwl": {"rootcolor": "#08526b"},
                  "kitty": {"background": "#e7dec7", "active_tab_foreground": "#5d4232"} }"##)
            .unwrap();
        let v0 = resolve_theme_variant("s", &json, 0);
        assert_eq!(v0.root_color, "#08526b");
        assert_eq!(v0.root_variant_count, 1);
        // Any index clamps to the single root.
        let v3 = resolve_theme_variant("s", &json, 3);
        assert_eq!(v3.root_color, "#08526b");
        assert_eq!(v3.root_variant, 0);
    }

    // zenbones-light shape: the ink (#286486) is a mid-luminance saturated
    // teal — hue-perceptible — and every root variant shares its hue. The
    // pure-luminance ink floor let variants 1-4 ship a same-hue darker teal
    // vocab tint that read as bold body text; with the hue requirement all
    // five variants must rotate to a clearly different hue (the wine/maroon
    // family the designed root already derived to).
    const COLORFUL_INK_JSON: &str = r##"{ "meta": {"display": "Z", "type": "light"},
        "dwl": {"rootcolor": "#08526b", "focuscolor": "#944927",
                "rootcolor_candidates": ["#41819b", "#286983", "#08526b"]},
        "kitty": {"background": "#f0edec", "active_tab_foreground": "#286486"} }"##;

    #[test]
    fn colorful_ink_vocab_tint_is_hue_distinct_on_every_variant() {
        let json: serde_json::Value = serde_json::from_str(COLORFUL_INK_JSON).unwrap();
        assert!(hue_perceptible("#286486"), "fixture ink must be hue-perceptible");
        let count = resolve_theme_variant("z", &json, 0).root_variant_count;
        for variant in 0..count {
            let v = resolve_theme_variant("z", &json, variant);
            assert!(
                hue_distance(&v.vocab_fg, &v.text_fg) >= 40.0,
                "variant {variant}: vocab {} shares the teal ink hue ({})",
                v.vocab_fg, v.text_fg
            );
            assert!(contrast_ratio(&v.vocab_fg, &v.text_bg) >= VOCAB_WORD_MIN_CONTRAST);
            assert!(
                contrast_ratio(&v.vocab_fg, &v.text_fg) >= READER_GLOSS_INK_MIN_CONTRAST,
                "variant {variant}: vocab {} sits at the ink's own weight", v.vocab_fg
            );
        }
    }

    #[test]
    fn muted_ink_keeps_luminance_only_rule() {
        // CANDIDATES_JSON's sepia ink (#5d4232, sat ~.30) must stay below the
        // hue-perceptible gate so the corpus of muted/near-black light themes
        // keeps its shipped tints.
        assert!(!hue_perceptible("#5d4232"));
        assert!(!hue_perceptible("#000000"));
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
        // phrase_highlight_bg intentionally varies with the root — see
        // phrase_tint_tracks_the_root_variant.
        assert_eq!(v0.dim_fg, v1.dim_fg);
        assert_eq!(v0.dim_fg, v2.dim_fg);
        assert_eq!(v0.sign_fg, v1.sign_fg);
        assert_eq!(v0.sign_fg, v2.sign_fg);
        assert_eq!(v0.cursor_bg, v1.cursor_bg);
        assert_eq!(v0.cursor_bg, v2.cursor_bg);
        assert_eq!(v0.cursor_fg, v1.cursor_fg);
        assert_eq!(v0.cursor_fg, v2.cursor_fg);
        // vocab_fg starts from the root hue (follows the variant like
        // scrim_bg) but rides the reader-gloss guard rails: readable on the
        // pinned card, visibly apart from the live root, and holding a
        // luminance gap from the body ink (hard floor on light themes).
        for v in [&v0, &v1, &v2] {
            assert!(contrast_ratio(&v.vocab_fg, &v.text_bg) >= VOCAB_WORD_MIN_CONTRAST);
            let root_ok = hue_distance(&v.vocab_fg, &v.root_color) >= 40.0
                || contrast_ratio(&v.vocab_fg, &v.root_color) >= READER_GLOSS_ROOT_MIN_CONTRAST;
            assert!(root_ok,
                "variant {}: vocab {} too close to root {}", v.root_variant, v.vocab_fg, v.root_color);
            let cvi = contrast_ratio(&v.vocab_fg, &v.text_fg);
            let ink_ok = if v.is_light {
                cvi >= READER_GLOSS_INK_MIN_CONTRAST
                    && (!hue_perceptible(&v.text_fg)
                        || hue_distance(&v.vocab_fg, &v.text_fg) >= 40.0)
            } else {
                hue_distance(&v.vocab_fg, &v.text_fg) >= 40.0 || cvi >= 1.4
            };
            assert!(ink_ok,
                "variant {}: vocab {} reads as body ink {}", v.root_variant, v.vocab_fg, v.text_fg);
        }
        // reader_gloss likewise FOLLOWS the root now: per variant it must
        // clear the card readable floor, sit visibly apart from the root
        // (hue OR contrast), and hold a luminance gap from the body ink.
        for v in [&v0, &v1, &v2] {
            assert!(
                contrast_ratio(&v.reader_gloss, &v.text_bg) >= READER_GLOSS_MIN_CONTRAST,
                "variant {}: tint {} dim on card {}", v.root_variant, v.reader_gloss, v.text_bg
            );
            let root_ok = hue_distance(&v.reader_gloss, &v.root_color) >= 40.0
                || contrast_ratio(&v.reader_gloss, &v.root_color) >= READER_GLOSS_ROOT_MIN_CONTRAST;
            assert!(root_ok,
                "variant {}: tint {} too close to root {}", v.root_variant, v.reader_gloss, v.root_color);
            let cvi = contrast_ratio(&v.reader_gloss, &v.text_fg);
            let ink_ok = if v.is_light {
                cvi >= READER_GLOSS_INK_MIN_CONTRAST
            } else {
                hue_distance(&v.reader_gloss, &v.text_fg) >= 40.0 || cvi >= 1.4
            };
            assert!(ink_ok,
                "variant {}: tint {} reads as body ink {}", v.root_variant, v.reader_gloss, v.text_fg);
        }
        assert_eq!(v0.overlay_panel_bg, v1.overlay_panel_bg);
        assert_eq!(v0.overlay_panel_bg, v2.overlay_panel_bg);
        // scrim_bg DIFFERS between v0 and v1 — it follows root.
        assert_ne!(v0.scrim_bg, v1.scrim_bg);
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
        // vocab_fg follows the root variant; only its card contrast is pinned.
        for v in [&v0, &v1, &v2] {
            assert!(contrast_ratio(&v.vocab_fg, &v.text_bg) >= 4.5);
        }
    }
}
