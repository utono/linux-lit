use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
    pub last_work: Option<String>,
    #[serde(default)]
    pub last_line: usize,
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

fn default_font_size() -> u32 {
    20
}

fn default_line_spacing() -> u32 {
    4
}

fn default_column_width() -> u32 {
    950
}

fn default_text_margins() -> u32 {
    48
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            line_spacing: default_line_spacing(),
            column_width: default_column_width(),
            text_margins: default_text_margins(),
            last_work: None,
            last_line: 0,
        }
    }
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/linux-lit/config.json")
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
