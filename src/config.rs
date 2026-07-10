use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

/// Works whose per-work config entries (position, position id, last gloss)
/// THIS instance changed. `save()` only overwrites these keys in the
/// on-disk file, so a concurrently running instance's newer entries for
/// OTHER works survive this instance's exit.
static DIRTY_WORKS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Works opened this session, most recent first — merged to the front of
/// `recent_works` on save.
static SESSION_RECENT: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Mark a work's per-work config entries as changed by this instance.
/// Call at every site that writes `work_positions`, `work_position_ids`,
/// or `last_gloss` for a work.
pub fn mark_work_dirty(abbrev: &str) {
    let mut d = DIRTY_WORKS.lock().unwrap();
    if !d.iter().any(|a| a == abbrev) {
        d.push(abbrev.to_string());
    }
}

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

/// Karaoke narration highlight granularity per work class (see
/// src/input/phrase_highlight.rs). Serialized as a lowercase string; legacy
/// boolean configs deserialize as true=Phrase, false=Off (Phrase — the
/// spoken-phrase span — is the default "on" state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PhraseHighlightMode {
    Off,
    Phrase,
    Line,
}

impl PhraseHighlightMode {
    pub fn is_on(self) -> bool {
        self != PhraseHighlightMode::Off
    }

    /// Alt+p cycle order: Off -> Phrase -> Line -> Off.
    pub fn cycle(self) -> Self {
        match self {
            PhraseHighlightMode::Off => PhraseHighlightMode::Phrase,
            PhraseHighlightMode::Phrase => PhraseHighlightMode::Line,
            PhraseHighlightMode::Line => PhraseHighlightMode::Off,
        }
    }

    /// Toast/log label.
    pub fn label(self) -> &'static str {
        match self {
            PhraseHighlightMode::Off => "OFF",
            PhraseHighlightMode::Phrase => "PHRASE",
            PhraseHighlightMode::Line => "LINE",
        }
    }
}

impl<'de> Deserialize<'de> for PhraseHighlightMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bool(bool),
            Str(String),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Bool(true) => Ok(PhraseHighlightMode::Phrase),
            Raw::Bool(false) => Ok(PhraseHighlightMode::Off),
            Raw::Str(s) => match s.as_str() {
                "off" => Ok(PhraseHighlightMode::Off),
                "phrase" => Ok(PhraseHighlightMode::Phrase),
                "line" => Ok(PhraseHighlightMode::Line),
                other => Err(D::Error::custom(format!(
                    "unknown phrase highlight mode: {other:?}"
                ))),
            },
        }
    }
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
    /// Karaoke narration highlight granularity, per work class: `off`,
    /// `phrase` (spoken phrase span; the default for prose), or `line`
    /// (whole verse line / prose sentence — still requires phrase data).
    /// Legacy boolean configs load as true=phrase, false=off. Alt+p cycles
    /// the current class.
    #[serde(default = "default_phrase_highlight_prose")]
    pub phrase_highlight_prose: PhraseHighlightMode,
    #[serde(default = "default_phrase_highlight_verse")]
    pub phrase_highlight_verse: PhraseHighlightMode,
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
    /// Per-app theme (independent of the system-wide theme). None means
    /// DEFAULT_THEME. Set by the settings overlay and Alt+t / Alt+Shift+T.
    #[serde(default)]
    pub theme: Option<String>,
    /// Ordered list the theme-cycle keybinds walk through. None means the
    /// compiled default reading list. Edit config.json to customize.
    #[serde(default)]
    pub theme_cycle: Option<Vec<String>>,
    /// Per-theme ROOT-color-variant index (0-2) chosen with Ctrl+t. Keyed
    /// by theme name; absent = 0 (the designed root). Merged ours-wins on
    /// save, like `theme` itself (see merge_configs).
    #[serde(default)]
    pub root_variants: HashMap<String, u8>,
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

/// linux-lit's theme is independent of the system-wide theme system.
/// This is the effective theme when config.json has no `theme` field.
pub const DEFAULT_THEME: &str = "kindle-sepia";

fn default_theme_cycle() -> Vec<String> {
    ["kindle-sepia", "kindle-green", "zenbones-light", "zenwritten-light"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

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

fn default_phrase_highlight_prose() -> PhraseHighlightMode {
    PhraseHighlightMode::Phrase
}

fn default_phrase_highlight_verse() -> PhraseHighlightMode {
    PhraseHighlightMode::Off
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
            phrase_highlight_prose: PhraseHighlightMode::Phrase,
            phrase_highlight_verse: PhraseHighlightMode::Off,
            title_bar_visible: default_title_bar_visible(),
            echo_affect_weight: default_echo_affect_weight(),
            system_volume: default_system_volume(),
            mpv_volume: default_mpv_volume(),
            theme: None,
            theme_cycle: None,
            root_variants: HashMap::new(),
        }
    }
}

const MAX_RECENT_WORKS: usize = 10;

impl Config {
    pub fn push_recent_work(&mut self, abbrev: &str) {
        self.recent_works.retain(|a| a != abbrev);
        self.recent_works.insert(0, abbrev.to_string());
        self.recent_works.truncate(MAX_RECENT_WORKS);
        let mut s = SESSION_RECENT.lock().unwrap();
        s.retain(|a| a != abbrev);
        s.insert(0, abbrev.to_string());
        s.truncate(MAX_RECENT_WORKS);
    }

    /// Effective theme name: configured, else DEFAULT_THEME.
    pub fn theme_name(&self) -> &str {
        self.theme.as_deref().unwrap_or(DEFAULT_THEME)
    }

    /// Effective theme-cycle list: configured, else the compiled default.
    pub fn theme_cycle(&self) -> Vec<String> {
        match &self.theme_cycle {
            Some(list) if !list.is_empty() => list.clone(),
            _ => default_theme_cycle(),
        }
    }

    /// Saved root-color-variant index for `theme_name`, wrapped into range.
    pub fn root_variant_for(&self, theme_name: &str) -> u8 {
        self.root_variants.get(theme_name).copied().unwrap_or(0)
            % crate::theme::ROOT_VARIANT_COUNT
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

/// Merge this instance's snapshot over the freshly-read on-disk config.
/// Per-work maps take the DISK value except for works this instance touched
/// (dirty) or keys the disk doesn't have; `recent_works` puts this
/// session's opens first. All scalar fields (font, theme, last_work, ...)
/// come from `ours` — last-writer-wins, the natural MRU semantic.
fn merge_configs(
    ours: &Config,
    disk: Config,
    dirty: &[String],
    session_recent: &[String],
) -> Config {
    fn overlay<V: Clone>(
        ours: &HashMap<String, V>,
        disk: HashMap<String, V>,
        dirty: &[String],
    ) -> HashMap<String, V> {
        let mut out = disk;
        for (k, v) in ours {
            if dirty.iter().any(|d| d == k) || !out.contains_key(k) {
                out.insert(k.clone(), v.clone());
            }
        }
        out
    }

    let mut merged = ours.clone();
    merged.work_positions = overlay(&ours.work_positions, disk.work_positions, dirty);
    merged.work_position_ids =
        overlay(&ours.work_position_ids, disk.work_position_ids, dirty);
    merged.last_gloss = overlay(&ours.last_gloss, disk.last_gloss, dirty);

    let mut recent: Vec<String> = session_recent.to_vec();
    for a in disk.recent_works {
        if !recent.iter().any(|r| r == &a) {
            recent.push(a);
        }
    }
    recent.truncate(MAX_RECENT_WORKS);
    merged.recent_works = recent;
    merged
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
    // Merge over the freshly-read file so a concurrently running instance's
    // per-work entries survive this instance's saves (see merge_configs).
    // Read/parse failure (missing or corrupt file) falls back to writing the
    // full snapshot, exactly the pre-merge behavior.
    let to_write = match fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Config>(&s).ok())
    {
        Some(disk) => {
            let dirty = DIRTY_WORKS.lock().unwrap().clone();
            let session = SESSION_RECENT.lock().unwrap().clone();
            merge_configs(config, disk, &dirty, &session)
        }
        None => config.clone(),
    };
    // Atomic write: write to temp, then rename. The temp name is per-process:
    // with multiple instances saving concurrently, a shared name would let one
    // instance truncate the file another is about to rename — installing
    // partial JSON whose parse failure downgrades the next save to a full
    // snapshot (the clobber this merge exists to prevent). A pid-tmp leaked by
    // a crash is harmless.
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    if let Ok(json) = serde_json::to_string_pretty(&to_write) {
        if fs::write(&tmp, &json).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    fn cfg() -> Config {
        serde_json::from_str("{}").unwrap()
    }

    #[test]
    fn other_instances_positions_survive() {
        // Ours loaded Ham=5 at startup (stale) and only touched BH.
        // Disk meanwhile has Ham=99 (the other instance's newer save).
        let mut ours = cfg();
        ours.work_positions.insert("Ham".into(), 5);
        ours.work_positions.insert("BH".into(), 10);
        let mut disk = cfg();
        disk.work_positions.insert("Ham".into(), 99);
        disk.work_positions.insert("BH".into(), 1);
        let merged = merge_configs(&ours, disk, &["BH".into()], &[]);
        assert_eq!(merged.work_positions["Ham"], 99); // disk wins: not dirty
        assert_eq!(merged.work_positions["BH"], 10); // ours wins: dirty
    }

    #[test]
    fn ours_only_undirty_key_is_kept() {
        // A key the disk lost entirely (e.g. truncated file) is restored
        // from our snapshot even when not dirty.
        let mut ours = cfg();
        ours.work_position_ids.insert("Oth".into(), 42);
        let merged = merge_configs(&ours, cfg(), &[], &[]);
        assert_eq!(merged.work_position_ids["Oth"], 42);
    }

    #[test]
    fn last_gloss_merges_by_dirty_key() {
        let mut ours = cfg();
        ours.last_gloss.insert(
            "Ham".into(),
            LastGloss { start_citation: "Ham.1.1.1".into(), gloss_type: "reader-gloss".into() },
        );
        let mut disk = cfg();
        disk.last_gloss.insert(
            "Ham".into(),
            LastGloss { start_citation: "Ham.5.2.300".into(), gloss_type: "reader-gloss".into() },
        );
        let not_dirty = merge_configs(&ours, disk.clone(), &[], &[]);
        assert_eq!(not_dirty.last_gloss["Ham"].start_citation, "Ham.5.2.300");
        let dirty = merge_configs(&ours, disk, &["Ham".into()], &[]);
        assert_eq!(dirty.last_gloss["Ham"].start_citation, "Ham.1.1.1");
    }

    #[test]
    fn recent_works_session_opens_go_first() {
        let mut disk = cfg();
        disk.recent_works = vec!["Ham".into(), "Oth".into()];
        let merged = merge_configs(&cfg(), disk, &[], &["BH".into()]);
        assert_eq!(merged.recent_works, vec!["BH", "Ham", "Oth"]);
    }

    #[test]
    fn scalars_are_last_writer_wins() {
        let mut ours = cfg();
        ours.last_work = Some("BH".into());
        let mut disk = cfg();
        disk.last_work = Some("Ham".into());
        let merged = merge_configs(&ours, disk, &[], &[]);
        assert_eq!(merged.last_work.as_deref(), Some("BH"));
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

    #[test]
    fn phrase_highlight_defaults_prose_phrase_verse_off() {
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.phrase_highlight_prose, PhraseHighlightMode::Phrase);
        assert_eq!(cfg.phrase_highlight_verse, PhraseHighlightMode::Off);
        let dflt = Config::default();
        assert_eq!(dflt.phrase_highlight_prose, PhraseHighlightMode::Phrase);
        assert_eq!(dflt.phrase_highlight_verse, PhraseHighlightMode::Off);
    }

    #[test]
    fn phrase_highlight_mode_legacy_bools_deserialize() {
        // Legacy ON means the default "on" state — Phrase — so a stored
        // `true` from the boolean era keeps phrase-width highlighting
        // (stored values override compiled defaults).
        let cfg: Config = serde_json::from_str(
            r#"{"phrase_highlight_prose": true, "phrase_highlight_verse": false}"#,
        )
        .unwrap();
        assert_eq!(cfg.phrase_highlight_prose, PhraseHighlightMode::Phrase);
        assert_eq!(cfg.phrase_highlight_verse, PhraseHighlightMode::Off);
    }

    #[test]
    fn phrase_highlight_mode_string_round_trip() {
        let mut cfg = Config::default();
        cfg.phrase_highlight_prose = PhraseHighlightMode::Line;
        cfg.phrase_highlight_verse = PhraseHighlightMode::Phrase;
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains(r#""phrase_highlight_prose":"line""#));
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.phrase_highlight_prose, PhraseHighlightMode::Line);
        assert_eq!(back.phrase_highlight_verse, PhraseHighlightMode::Phrase);
    }

    #[test]
    fn phrase_highlight_mode_cycle_and_is_on() {
        use PhraseHighlightMode::*;
        assert_eq!(Off.cycle(), Phrase);
        assert_eq!(Phrase.cycle(), Line);
        assert_eq!(Line.cycle(), Off);
        assert!(!Off.is_on());
        assert!(Phrase.is_on());
        assert!(Line.is_on());
    }

    #[test]
    fn theme_name_defaults_to_kindle_sepia() {
        let config = Config::default();
        assert_eq!(config.theme_name(), "kindle-sepia");
    }

    #[test]
    fn theme_name_uses_configured_value() {
        let mut config = Config::default();
        config.theme = Some("zenbones-light".to_string());
        assert_eq!(config.theme_name(), "zenbones-light");
    }

    #[test]
    fn theme_cycle_defaults_to_reading_themes() {
        let config = Config::default();
        assert_eq!(
            config.theme_cycle(),
            vec!["kindle-sepia", "kindle-green", "zenbones-light", "zenwritten-light"]
        );
    }

    #[test]
    fn theme_cycle_uses_configured_list() {
        let mut config = Config::default();
        config.theme_cycle = Some(vec!["melange-light".to_string()]);
        assert_eq!(config.theme_cycle(), vec!["melange-light"]);
    }

    #[test]
    fn root_variant_for_defaults_to_zero_and_wraps() {
        let mut c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(c.root_variant_for("kindle-sepia"), 0);
        c.root_variants.insert("kindle-sepia".into(), 2);
        assert_eq!(c.root_variant_for("kindle-sepia"), 2);
        c.root_variants.insert("kindle-sepia".into(), 7); // malformed config
        assert_eq!(c.root_variant_for("kindle-sepia"), 2); // 7 % 5
    }

    #[test]
    fn root_variants_roundtrip_serde() {
        let mut c: Config = serde_json::from_str("{}").unwrap();
        c.root_variants.insert("sepia-lightest".into(), 1);
        let json = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.root_variant_for("sepia-lightest"), 1);
    }
}
