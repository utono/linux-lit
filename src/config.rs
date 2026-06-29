use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NavigationMode {
    Scroll,
    #[default]
    #[serde(rename = "ereader")]
    EReader,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransitionStyle {
    #[default]
    Crossfade,
    Slide,
    Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualModeCommand {
    pub name: String,
    pub command: String,
}

/// The most-recently-viewed gloss for one work: which passage (by its
/// start citation) and which gloss type was on screen. Reopened by Ctrl+g.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastGloss {
    pub start_citation: String,
    pub gloss_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: u32,
    #[serde(default = "default_line_spacing")]
    pub line_spacing: u32,
    #[serde(default = "default_column_width")]
    pub column_width: u32,
    #[serde(default = "default_text_margins")]
    pub text_margins: u32,
    #[serde(default)]
    pub navigation_mode: NavigationMode,
    #[serde(default)]
    pub transition_style: TransitionStyle,
    #[serde(default)]
    pub last_work: Option<String>,
    #[serde(default = "default_previous_work")]
    pub previous_work: Option<String>,
    #[serde(default)]
    pub recent_works: Vec<String>,
    #[serde(default)]
    pub work_positions: HashMap<String, usize>,
    /// Resume position keyed by line_mapping_id (citation-stable across
    /// re-imports). Preferred over `work_positions` (legacy raw buffer index).
    #[serde(default)]
    pub work_position_ids: HashMap<String, i64>,
    /// Per-work most-recently-viewed gloss, keyed by work_abbrev. Mirrors
    /// `work_positions`. Written at every gloss-display site; read by Ctrl+g.
    #[serde(default)]
    pub last_gloss: HashMap<String, LastGloss>,
    #[serde(default)]
    pub column_overrides: HashMap<String, u8>,
    /// Column count (1 or 2) the LAST session resolved for `last_work`. Used as
    /// the initial layout guess at build time — before `current_work` loads —
    /// so the first card-sizing/formatting pass already matches the target
    /// layout and there's no visible 1→2-column reflow on startup. Corrected
    /// after the work loads if the real count differs.
    #[serde(default)]
    pub last_column_count: Option<u8>,
    #[serde(default)]
    pub visual_mode_commands: Vec<VisualModeCommand>,
    #[serde(default = "default_claude_model")]
    pub claude_model: String,
    #[serde(default = "default_elevenlabs_voice_id")]
    pub elevenlabs_voice_id: String,
    #[serde(default = "default_elevenlabs_model_id")]
    pub elevenlabs_model_id: String,
    #[serde(default = "default_dim_enabled")]
    pub dim_enabled: bool,
    #[serde(default = "default_scansion_level")]
    pub scansion_level: String,
    #[serde(default = "default_show_cursor_line")]
    pub show_cursor_line: bool,
    #[serde(default = "default_title_bar_visible")]
    pub title_bar_visible: bool,
    /// Weight of the sentiment/affect (NRC-VAD) axis in echo re-ranking, in
    /// [0, 1]. Final score = (1 - w) * semantic_cosine + w * affect_cosine.
    /// 0.0 = pure semantic ranking (default; the affect axis is inert).
    /// See docs/plans/2026-05-30-semantic-echo-search-design.md.
    #[serde(default = "default_echo_affect_weight")]
    pub echo_affect_weight: f32,
    /// System output sink volume (percent) applied once on startup via
    /// `pactl set-sink-volume @DEFAULT_SINK@`. Default 70 (matches the dwl
    /// session default in ~/utono/dwl-mlj/start-dwl). Change with the
    /// `set-startup-volume` skill.
    #[serde(default = "default_system_volume")]
    pub system_volume: u32,
    /// MPV playback volume (percent) passed as `--volume=` when launching mpv.
    /// Default 100 (i.e. 100% of the system sink). Change with the
    /// `set-startup-volume` skill.
    #[serde(default = "default_mpv_volume")]
    pub mpv_volume: u32,
}

fn default_font_family() -> String {
    "Charter".to_string()
}

pub const FONT_CYCLE: &[&str] = &[
    "Charter",
    "Crimson Pro",
    "Noto Serif",
    "Source Serif 4",
    "IBM Plex Serif",
    "Cormorant Garamond",
];

pub fn default_font_size() -> u32 {
    16
}

pub const DEFAULT_LINE_SPACING: u32 = 5;
pub const DEFAULT_COLUMN_WIDTH: u32 = 1050;
pub const DEFAULT_TEXT_MARGINS: u32 = 40;
pub const EXTRA_RIGHT_MARGIN: i32 = 48;

fn default_line_spacing() -> u32 {
    DEFAULT_LINE_SPACING
}

fn default_column_width() -> u32 {
    DEFAULT_COLUMN_WIDTH
}

fn default_text_margins() -> u32 {
    DEFAULT_TEXT_MARGINS
}

fn default_claude_model() -> String {
    "claude-opus-4-8".to_string()
}

fn default_elevenlabs_voice_id() -> String {
    // Alice — "Clear, Engaging Educator". A premade voice (free-tier API
    // usable); library voices return HTTP 402 paid_plan_required on free plans.
    // Also the 402 fallback voice (see elevenlabs.rs). Id lives in one place.
    crate::elevenlabs::ALICE_VOICE_ID.to_string()
}

fn default_elevenlabs_model_id() -> String {
    crate::elevenlabs::ALICE_MODEL_ID.to_string()
}

fn default_previous_work() -> Option<String> {
    Some("Dominion".to_string())
}

fn default_dim_enabled() -> bool {
    false
}

fn default_scansion_level() -> String {
    "off".to_string()
}

fn default_show_cursor_line() -> bool {
    true
}

fn default_title_bar_visible() -> bool {
    false
}

fn default_echo_affect_weight() -> f32 {
    0.0
}

pub fn default_system_volume() -> u32 {
    70
}

pub fn default_mpv_volume() -> u32 {
    100
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            line_spacing: default_line_spacing(),
            column_width: default_column_width(),
            text_margins: default_text_margins(),
            navigation_mode: NavigationMode::default(),
            transition_style: TransitionStyle::default(),
            last_work: None,
            previous_work: default_previous_work(),
            recent_works: Vec::new(),
            work_positions: HashMap::new(),
            work_position_ids: HashMap::new(),
            last_gloss: HashMap::new(),
            column_overrides: HashMap::new(),
            last_column_count: None,
            visual_mode_commands: Vec::new(),
            claude_model: default_claude_model(),
            elevenlabs_voice_id: default_elevenlabs_voice_id(),
            elevenlabs_model_id: default_elevenlabs_model_id(),
            dim_enabled: default_dim_enabled(),
            scansion_level: default_scansion_level(),
            show_cursor_line: true,
            title_bar_visible: default_title_bar_visible(),
            echo_affect_weight: default_echo_affect_weight(),
            system_volume: default_system_volume(),
            mpv_volume: default_mpv_volume(),
        }
    }
}

const MAX_RECENT_WORKS: usize = 10;

impl Config {
    pub fn push_recent_work(&mut self, abbrev: &str) {
        self.recent_works.retain(|a| a != abbrev);
        self.recent_works.insert(0, abbrev.to_string());
        self.recent_works.truncate(MAX_RECENT_WORKS);
    }
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    let filename = if crate::mode::is_dev_mode() {
        "config-dev.json"
    } else {
        "config.json"
    };
    PathBuf::from(home).join(".config/linux-lit").join(filename)
}

pub fn load() -> Config {
    let path = config_path();
    if !path.exists() {
        return Config::default();
    }
    let mut config = match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => Config::default(),
    };
    // Honor the saved font size across restarts (in-app !/| adjustments
    // persist). Clamp to a sane range so a malformed config can't render the
    // text unreadable or zero-height.
    config.font_size = config.font_size.clamp(8, 48);
    config.column_width = default_column_width();
    config.text_margins = default_text_margins();
    config.show_cursor_line = true;
    config.title_bar_visible = false;
    if config.claude_model.contains("-20") {
        config.claude_model = default_claude_model();
    }
    if config.previous_work == config.last_work {
        config.previous_work = match config.last_work.as_deref() {
            Some("Dominion") => Some("TGV".to_string()),
            _ => Some("Dominion".to_string()),
        };
    }
    // Clamp the affect weight so a malformed config can never distort ranking.
    config.echo_affect_weight = config.echo_affect_weight.clamp(0.0, 1.0);
    config
}

pub fn save(config: &Config) {
    // Hermetic test runs: under LIT_HEADLESS_TEST the app must NEVER write config
    // back. A headless/fuzz run starts from LIT_START_WORK/LIT_START_POS (or the
    // dev config) and would otherwise rewrite last_work/work_positions on exit —
    // the documented footgun where the next run inherits the prior run's end
    // position. Suppressing writeback makes a run fully reproducible from env
    // alone and stops it mutating state a later run depends on.
    if std::env::var_os("LIT_HEADLESS_TEST").is_some() {
        return;
    }
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // Atomic write: write to temp, then rename
    let tmp = path.with_extension("tmp");
    if let Ok(json) = serde_json::to_string_pretty(config) {
        if fs::write(&tmp, &json).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

#[cfg(test)]
mod last_gloss_tests {
    use super::*;

    #[test]
    fn last_gloss_round_trips_through_json() {
        let mut cfg: Config = serde_json::from_str("{}").unwrap();
        cfg.last_gloss.insert(
            "Ham".to_string(),
            LastGloss { start_citation: "Ham.1.2.93".to_string(),
                        gloss_type: "reader-gloss".to_string() },
        );
        let json = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        let lg = back.last_gloss.get("Ham").unwrap();
        assert_eq!(lg.start_citation, "Ham.1.2.93");
        assert_eq!(lg.gloss_type, "reader-gloss");
    }

    #[test]
    fn config_without_last_gloss_key_loads_empty() {
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.last_gloss.is_empty());
    }
}
