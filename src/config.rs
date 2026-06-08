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
    #[serde(default = "default_vocab_highlight_visible")]
    pub vocab_highlight_visible: bool,
    #[serde(default = "default_dim_enabled")]
    pub dim_enabled: bool,
    #[serde(default = "default_show_cursor_line")]
    pub show_cursor_line: bool,
    #[serde(default = "default_title_bar_visible")]
    pub title_bar_visible: bool,
    /// Weight of the sentiment/affect (NRC-VAD) axis in echo re-ranking, in
    /// [0, 1]. Final score = (1 - w) * semantic_cosine + w * affect_cosine.
    /// 0.0 = pure semantic ranking (default; the affect axis is inert).
    /// See docs/specs/2026-05-30-semantic-echo-search-design.md.
    #[serde(default = "default_echo_affect_weight")]
    pub echo_affect_weight: f32,
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
    "claude-opus-4-7".to_string()
}

fn default_elevenlabs_voice_id() -> String {
    // Rachel — ElevenLabs' stock default voice.
    "21m00Tcm4TlvDq8ikWAM".to_string()
}

fn default_elevenlabs_model_id() -> String {
    "eleven_turbo_v2_5".to_string()
}

fn default_previous_work() -> Option<String> {
    Some("Dominion".to_string())
}

fn default_vocab_highlight_visible() -> bool {
    true
}

fn default_dim_enabled() -> bool {
    false
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
            column_overrides: HashMap::new(),
            last_column_count: None,
            visual_mode_commands: Vec::new(),
            claude_model: default_claude_model(),
            elevenlabs_voice_id: default_elevenlabs_voice_id(),
            elevenlabs_model_id: default_elevenlabs_model_id(),
            vocab_highlight_visible: default_vocab_highlight_visible(),
            dim_enabled: default_dim_enabled(),
            show_cursor_line: true,
            title_bar_visible: default_title_bar_visible(),
            echo_affect_weight: default_echo_affect_weight(),
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
