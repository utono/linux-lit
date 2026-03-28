use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NavigationMode {
    #[default]
    Scroll,
    #[serde(rename = "ereader")]
    EReader,
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
    pub last_work: Option<String>,
    #[serde(default)]
    pub last_line: usize,
    #[serde(default)]
    pub work_positions: HashMap<String, usize>,
    #[serde(default)]
    pub visual_mode_commands: Vec<VisualModeCommand>,
    #[serde(default = "default_ollama_model")]
    pub ollama_model: String,
    #[serde(default = "default_ollama_endpoint")]
    pub ollama_endpoint: String,
    #[serde(default = "default_vocab_highlight_visible")]
    pub vocab_highlight_visible: bool,
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
pub const DEFAULT_COLUMN_WIDTH: u32 = 750;
pub const DEFAULT_TEXT_MARGINS: u32 = 48;
pub const EXTRA_RIGHT_MARGIN: i32 = 24;

fn default_line_spacing() -> u32 {
    DEFAULT_LINE_SPACING
}

fn default_column_width() -> u32 {
    DEFAULT_COLUMN_WIDTH
}

fn default_text_margins() -> u32 {
    DEFAULT_TEXT_MARGINS
}

fn default_ollama_model() -> String {
    "qwen2.5:7b".to_string()
}

fn default_ollama_endpoint() -> String {
    "http://localhost:11434".to_string()
}

fn default_vocab_highlight_visible() -> bool {
    true
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
            last_work: None,
            last_line: 0,
            work_positions: HashMap::new(),
            visual_mode_commands: Vec::new(),
            ollama_model: default_ollama_model(),
            ollama_endpoint: default_ollama_endpoint(),
            vocab_highlight_visible: default_vocab_highlight_visible(),
        }
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
    // Always start at the default font size regardless of saved value
    config.font_size = default_font_size();
    config
}

pub fn save(config: &Config) {
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
