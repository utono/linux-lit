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
    /// Per-work reading face, keyed by work_abbrev. Mirrors `work_positions`.
    /// Written by `cycle_font` (both directions of `F`); read at work load,
    /// where it is resolved INTO `font_family` so every downstream consumer —
    /// `reapply_font`, the prose layout fingerprint, the overlay — sees one
    /// value and needs no per-work awareness.
    ///
    /// A work with no entry opens on the global `font_family` default
    /// (Charis). Changing the font in one work never affects another, and
    /// never moves the global default.
    ///
    /// Seeded with `LoJ -> Charter` (see `default_work_fonts`).
    #[serde(default = "default_work_fonts")]
    pub work_fonts: HashMap<String, String>,
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
    /// Whether main-card lines covered by a reader-gloss or journal passage
    /// Q&A are tinted (theme.reader_gloss). Ctrl+Alt+g toggles.
    #[serde(default = "default_annotation_tint")]
    pub annotation_tint: bool,
    #[serde(default = "default_scansion_level")]
    pub scansion_level: String,
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
    /// Whether the persistent `+` scene/chapter toast is shown by default for
    /// plays and prose-with-chapters. Shown by default; `+` toggles it and the
    /// off state is remembered here across launches and work switches.
    #[serde(default = "default_chapter_toast_shown")]
    pub chapter_toast_shown: bool,
    /// Weight of the sentiment/affect (NRC-VAD) axis in echo re-ranking, in
    /// [0, 1]. Final score = (1 - w) * semantic_cosine + w * affect_cosine.
    /// 0.0 = pure semantic ranking (default; the affect axis is inert).
    /// See docs/superpowers/specs/2026-05-30-semantic-echo-search-design.md.
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
    /// How many percent points BELOW `system_volume` the in-process TTS clip
    /// player (rodio; synopsis/gloss overlays) seeds at on startup — the
    /// ElevenLabs clips run hotter than the audiobook masters. Ctrl+Alt+
    /// Up/Down nudges the level AND rewrites this offset in config.
    #[serde(default = "default_tts_volume_offset")]
    pub tts_volume_offset: u32,
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
    /// Whether newly-saved journal Q&A entries are automatically tagged by
    /// the background tagger (Task 4). Default on.
    #[serde(default = "default_auto_tag_journal")]
    pub auto_tag_journal: bool,
    /// Claude model id used for the background journal-tag extraction call.
    /// Defaults to the same model as the main gloss/journal chat
    /// (DEFAULT_CLAUDE_MODEL); override here to use a cheaper model for this
    /// background call.
    #[serde(default = "default_tag_extract_model")]
    pub tag_extract_model: String,
}

/// The reading family a config with no `font_family` starts on, and therefore
/// `FONT_CYCLE`'s first entry (`font_cycle_starts_at_the_default_family` keeps
/// the two in step). Charis over the long-standing Charter because it covers the
/// archaic sorts and the IPA that Charter lacks — see the FONT_CYCLE comment.
fn default_font_family() -> String {
    "Charis".to_string()
}

/// Per-work reading faces a fresh config starts with.
///
/// These four are the ONLY works whose glosses carry IPA — the OP
/// pronunciations — measured against lit.db on 2026-08-11: `TT` 13 glosses,
/// `H8` 6, `Jn` 1, `Ant` 1. **Charter has no IPA glyphs at all** (verified:
/// U+0283 ʃ, U+0259 ə, U+026A ɪ, U+02D0 ː are all absent from
/// `Charter Regular.otf` and all present in `Charis-Regular.ttf`), so under
/// a Charter reading font those 21 glosses render their pronunciations with
/// a mid-word Noto Sans fallback. Pinning them to Charis fixes that without
/// touching the global face.
///
/// Everything else — LoJ included — inherits the global `font_family`,
/// whatever the reader has it set to.
///
/// Before seeding another work, check whether it needs Charis:
/// ```sql
/// SELECT p.work_abbrev, COUNT(*) FROM glosses g
///   JOIN passages p ON p.id = g.passage_id
///  WHERE g.gloss_text GLOB '*[ɪɛəʊʃʒθðŋæɑɔʌː]*'
///  GROUP BY 1;
/// ```
fn default_work_fonts() -> HashMap<String, String> {
    ["TT", "H8", "Jn", "Ant"]
        .iter()
        .map(|w| (w.to_string(), "Charis".to_string()))
        .collect()
}

/// Reading families cycled by `F` (forward) and `Ctrl+Shift+f` (backward) —
/// plain `f` is `OpenJournalTermInput`, not a font key. The FIRST entry must
/// equal `default_font_family` (currently Charis), so cycling forward from a
/// fresh config starts where it is already sitting;
/// `font_cycle_starts_at_the_default_family` asserts the pair stays in step. To
/// reorder the head of this list, change that function too.
///
/// Every entry must be an INSTALLED family that Pango resolves, because a name
/// it cannot resolve does not warn — it silently renders the fontconfig fallback
/// (see `ui::font_string`). "Junicode SemiExp" resolves to family `Junicode` at
/// `stretch=semi-expanded`: that is the intended cut (445px on the reference
/// line vs the regular cut's 407px), and the comma form `ui::font_string`
/// builds preserves the stretch. Do not shorten it to "Junicode".
///
/// Use the exact PANGO family name, which is not always the name the foundry
/// or the package uses: "Charis" (not "Charis SIL" — that resolves to Noto
/// Sans). `font_cycle_entries_resolve_to_themselves` catches these, but only
/// on a machine where the font is installed.
pub const FONT_CYCLE: &[&str] = &[
    // Charis SIL — the default (`default_font_family`), so it MUST stay first.
    // The Pango family is "Charis"; asking for "Charis SIL" resolves to Noto
    // Sans, silently. A legibility-first publishing serif with FULL coverage of
    // both the archaic sorts (yogh, long-s) and the IPA, so it renders the
    // OP-IPA gloss pronunciations without the mid-word fallback Charter takes.
    // 463px on the reference line against Charter's 457 — near enough that the
    // swap needs no size compensation.
    "Charis",
    "Charter",
    // Parked (2026-07-28) — kept here rather than deleted so the set can be
    // restored by uncommenting. All five are installed and resolve; they were
    // taken out of rotation to make `f` a short cycle over the faces below.
    // NOTE: of these, only Noto Serif covers the IPA — the other four fall back
    // to Noto Sans mid-word on the OP-IPA gloss pronunciations.
    // "Crimson Pro",
    // "Noto Serif",
    // "Source Serif 4",
    // "IBM Plex Serif",
    // "Cormorant Garamond",
    // Parked (2026-07-28). Junicode is drawn for medieval/Early Modern text and
    // is the only face here covering BOTH the archaic sorts (yogh, long-s) and
    // the IPA, so cycling to it also renders the OP-IPA gloss pronunciations
    // without the mid-word fallback every other family takes. Uncomment to
    // restore; the entry resolves (family `Junicode`, stretch semi-expanded).
    // "Junicode SemiExp",
    // The journal Q&A's own face. Cycling the reader ONTO it is safe: the
    // journal detects the collision and swaps to Charter for as long as the
    // reader holds Gentium (`journal_overlay::journal_family_for_reader`), so
    // the Q&A never renders in the same family as the card behind it.
    "Gentium Book",
    // TeX Gyre: metric-compatible clones of the classic book types. Parked
    // (2026-07-28) alongside the five above — all installed and resolving,
    // uncomment to restore.
    // "TeX Gyre Pagella", // Palatino — 459px, closest match to Charter's 457
    // "TeX Gyre Schola",  // Century Schoolbook — 493px, large x-height
    // "TeX Gyre Termes",  // Times — 419px, narrowest and tightest
];

/// linux-lit's theme is independent of the system-wide theme system.
/// This is the effective theme when config.json has no `theme` field.
pub const DEFAULT_THEME: &str = "kindle-sepia";

fn default_theme_cycle() -> Vec<String> {
    [
        "lauds",
        "zenbones-light",
        "zenwritten-light",
        "papercolor-light",
        "lauds-blais",
        "rose-pine-dawn",
        "blush-rose",
        "sepia-lightest",
        "sepia-light",
        "green-lightest",
        "green-light",
        "kindle-sepia",
        "kindle-green",
    ]
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

/// The current default Claude model, referenced by every config default.
/// Change this one line to swap models repo-wide.
const DEFAULT_CLAUDE_MODEL: &str = "claude-opus-5";

fn default_claude_model() -> String {
    DEFAULT_CLAUDE_MODEL.to_string()
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

fn default_annotation_tint() -> bool {
    true
}

fn default_scansion_level() -> String {
    "off".to_string()
}

fn default_title_bar_visible() -> bool {
    false
}

fn default_chapter_toast_shown() -> bool {
    true
}

fn default_phrase_highlight_prose() -> PhraseHighlightMode {
    PhraseHighlightMode::Phrase
}

fn default_phrase_highlight_verse() -> PhraseHighlightMode {
    PhraseHighlightMode::Phrase
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

pub fn default_tts_volume_offset() -> u32 {
    50
}

fn default_auto_tag_journal() -> bool {
    true
}

fn default_tag_extract_model() -> String {
    DEFAULT_CLAUDE_MODEL.to_string()
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
            // Not HashMap::new(): a `Default` config must agree with what
            // serde builds for a file that omits the key, or LoJ's face would
            // depend on whether the key happened to be present.
            work_fonts: default_work_fonts(),
            last_gloss: HashMap::new(),
            column_overrides: HashMap::new(),
            last_column_count: None,
            visual_mode_commands: Vec::new(),
            claude_model: default_claude_model(),
            elevenlabs_voice_id: default_elevenlabs_voice_id(),
            elevenlabs_model_id: default_elevenlabs_model_id(),
            dim_enabled: default_dim_enabled(),
            annotation_tint: default_annotation_tint(),
            scansion_level: default_scansion_level(),
            phrase_highlight_prose: PhraseHighlightMode::Phrase,
            phrase_highlight_verse: PhraseHighlightMode::Phrase,
            title_bar_visible: default_title_bar_visible(),
            chapter_toast_shown: default_chapter_toast_shown(),
            echo_affect_weight: default_echo_affect_weight(),
            system_volume: default_system_volume(),
            mpv_volume: default_mpv_volume(),
            tts_volume_offset: default_tts_volume_offset(),
            theme: None,
            theme_cycle: None,
            root_variants: HashMap::new(),
            auto_tag_journal: default_auto_tag_journal(),
            tag_extract_model: default_tag_extract_model(),
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

    /// Saved root-color-variant index for `theme_name` (0 if none). Returned
    /// verbatim — `resolve_theme_variant` clamps an out-of-range index to 0
    /// against the theme's own candidate count.
    pub fn root_variant_for(&self, theme_name: &str) -> u8 {
        self.root_variants.get(theme_name).copied().unwrap_or(0)
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

/// `off` is no longer a persisted karaoke mode — the session axis (Alt+p)
/// expresses it at runtime. Stored `off` values predate the axis; treat
/// them as `phrase` so verse karaoke actually defaults on (a stored value
/// always beats a compiled default).
pub(crate) fn migrate_phrase_modes(config: &mut Config) {
    if config.phrase_highlight_prose == PhraseHighlightMode::Off {
        config.phrase_highlight_prose = PhraseHighlightMode::Phrase;
    }
    if config.phrase_highlight_verse == PhraseHighlightMode::Off {
        config.phrase_highlight_verse = PhraseHighlightMode::Phrase;
    }
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
    config.title_bar_visible = false;
    migrate_phrase_modes(&mut config);
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
    // LIT_THEME picks this instance's startup theme, so parallel instances are
    // visually distinguishable (crll walks theme_cycle for one not already in
    // use). Applied at LOAD only: an in-app Alt+t change still saves normally,
    // and an unset/blank value leaves the stored theme untouched. An unknown
    // name is left as-is — load_theme_with_fallback resolves it.
    apply_theme_override(&mut config, std::env::var("LIT_THEME").ok().as_deref());
    config
}

/// Apply a `LIT_THEME` startup override to `config`. Blank/absent leaves the
/// stored theme untouched; an unknown name is passed through for
/// `load_theme_with_fallback` to resolve.
pub(crate) fn apply_theme_override(config: &mut Config, requested: Option<&str>) {
    if let Some(theme) = requested.map(str::trim).filter(|t| !t.is_empty()) {
        config.theme = Some(theme.to_string());
    }
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
    fn default_font_is_charis_16() {
        // A config with no font fields falls back to the compiled defaults.
        let c = cfg();
        assert_eq!(c.font_family, "Charis");
        assert_eq!(c.font_size, 16);
        // The default helpers agree (they feed both the serde default and the
        // Default impl).
        assert_eq!(default_font_family(), "Charis");
        assert_eq!(default_font_size(), 16);
    }

    /// The four IPA-bearing works pin to Charis; everything else (LoJ
    /// included) inherits the global face. The seed must survive BOTH
    /// construction paths — `Default` and a deserialized config that omits
    /// the key — or a work's face would depend on whether the key happened to
    /// have been written.
    #[test]
    fn ipa_works_pin_to_charis_others_inherit_the_global_family() {
        let c = Config::default();
        for w in ["TT", "H8", "Jn", "Ant"] {
            assert_eq!(
                c.work_fonts.get(w).map(String::as_str),
                Some("Charis"),
                "{w} carries IPA glosses and must not fall back to Charter"
            );
        }
        assert_eq!(c.work_fonts.get("LoJ"), None, "LoJ inherits the global face");
        assert_eq!(c.work_fonts.get("BH"), None);

        let from_empty: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(
            from_empty.work_fonts.get("TT").map(String::as_str),
            Some("Charis"),
            "a config omitting work_fonts must still seed the IPA works"
        );
    }

    /// Every seeded per-work face must be a real cycle entry, or `F` would
    /// jump to index 0 (`position(...).unwrap_or(0)`) and the work would
    /// silently lose its face on the first press.
    #[test]
    fn seeded_work_fonts_are_all_cycle_entries() {
        for (work, fam) in default_work_fonts() {
            assert!(
                FONT_CYCLE.contains(&fam.as_str()),
                "work_fonts[{work}] = {fam:?} is not in FONT_CYCLE — `F` would reset it"
            );
        }
    }

    #[test]
    fn font_cycle_starts_at_the_default_family() {
        // Cycling fonts forward from the default should begin at the default
        // family, so the cycle's first entry must equal it.
        assert_eq!(FONT_CYCLE.first().copied(), Some(default_font_family().as_str()));
    }

    #[test]
    fn font_cycle_has_no_duplicates() {
        // A repeated family makes `f` appear to "stick" for one press.
        let mut seen = std::collections::HashSet::new();
        for f in FONT_CYCLE {
            assert!(seen.insert(*f), "{f} appears twice in FONT_CYCLE");
        }
    }

    /// A cycle entry Pango cannot resolve does NOT warn — it silently renders
    /// the fontconfig fallback, so cycling would land on a face that is not the
    /// one named. A misspelling is the live risk here (verified: "Tex Gyre
    /// Pagela" resolves to "Noto Sans" and this test fails on it).
    ///
    /// This asserts unconditionally. An earlier version gated on
    /// `gdk::Display::default()`, which is None under `cargo test`, so it
    /// returned early and asserted NOTHING on every run — the pangocairo
    /// fontmap works headless, which is the whole point. If the fonts are ever
    /// absent from a CI image, skip by name rather than reintroducing a guard
    /// that silently disables the check.
    #[test]
    fn font_cycle_entries_resolve_to_themselves() {
        // NOT gated on a gdk::Display: there is none under `cargo test`, and
        // gating on it made this assert nothing at all on every run. The
        // pangocairo fontmap works headless, which is the whole point.
        use gtk4::prelude::*;
        let fontmap = pangocairo::FontMap::default();
        let ctx = fontmap.create_context();
        for entry in FONT_CYCLE {
            let desc = pango::FontDescription::from_string(&crate::ui::font_string(entry, 16));
            let Some(font) = fontmap.load_font(&ctx, &desc) else {
                continue; // fonts unavailable in this environment
            };
            let got = font.describe().family().unwrap_or_default().to_string();
            // A width-axis entry ("Junicode SemiExp") legitimately resolves to
            // the BASE family plus a non-normal stretch, so accept the name with
            // its trailing width token removed — but nothing looser, or a typo
            // ("Tex Gyre Pagela") slips through on a prefix match.
            let base = entry
                .strip_suffix(" SemiExp")
                .or_else(|| entry.strip_suffix(" SemiCond"))
                .or_else(|| entry.strip_suffix(" Cond"))
                .or_else(|| entry.strip_suffix(" Exp"))
                .unwrap_or(entry);
            assert!(
                got.eq_ignore_ascii_case(entry) || got.eq_ignore_ascii_case(base),
                "FONT_CYCLE entry {entry:?} resolved to {got:?} — not installed, \
                 or misspelled; either way `f` would silently render a fallback",
            );
        }
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
    fn phrase_highlight_defaults_prose_phrase_verse_phrase() {
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.phrase_highlight_prose, PhraseHighlightMode::Phrase);
        assert_eq!(cfg.phrase_highlight_verse, PhraseHighlightMode::Phrase);
        let dflt = Config::default();
        assert_eq!(dflt.phrase_highlight_prose, PhraseHighlightMode::Phrase);
        assert_eq!(dflt.phrase_highlight_verse, PhraseHighlightMode::Phrase);
    }

    #[test]
    fn phrase_highlight_mode_legacy_bools_deserialize() {
        // Legacy ON means the default "on" state — Phrase — so a stored
        // `true` from the boolean era keeps phrase-width highlighting
        // (stored values override compiled defaults). Legacy `false` still
        // deserializes to Off here; migrate_phrase_modes (run in load())
        // is what maps Off -> Phrase, not deserialization itself.
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
    fn phrase_highlight_mode_is_on() {
        use PhraseHighlightMode::*;
        assert!(!Off.is_on());
        assert!(Phrase.is_on());
        assert!(Line.is_on());
    }

    #[test]
    fn migrate_phrase_modes_maps_off_to_phrase() {
        use PhraseHighlightMode::{Line, Off, Phrase};
        let mut c = Config::default();
        c.phrase_highlight_prose = Off;
        c.phrase_highlight_verse = Off;
        migrate_phrase_modes(&mut c);
        assert_eq!(c.phrase_highlight_prose, Phrase);
        assert_eq!(c.phrase_highlight_verse, Phrase);
        // phrase / line survive untouched.
        c.phrase_highlight_prose = Phrase;
        c.phrase_highlight_verse = Line;
        migrate_phrase_modes(&mut c);
        assert_eq!(c.phrase_highlight_prose, Phrase);
        assert_eq!(c.phrase_highlight_verse, Line);
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
    fn theme_override_replaces_stored_theme() {
        let mut config = Config::default();
        config.theme = Some("zenbones-light".to_string());
        apply_theme_override(&mut config, Some("rose-pine-dawn"));
        assert_eq!(config.theme_name(), "rose-pine-dawn");
    }

    #[test]
    fn theme_override_ignores_absent_or_blank() {
        let mut config = Config::default();
        config.theme = Some("zenbones-light".to_string());
        apply_theme_override(&mut config, None);
        assert_eq!(config.theme_name(), "zenbones-light");
        apply_theme_override(&mut config, Some("   "));
        assert_eq!(config.theme_name(), "zenbones-light");
    }

    #[test]
    fn theme_override_trims_whitespace() {
        let mut config = Config::default();
        apply_theme_override(&mut config, Some("  sepia-light\n"));
        assert_eq!(config.theme_name(), "sepia-light");
    }

    #[test]
    fn theme_cycle_defaults_to_reading_themes() {
        let config = Config::default();
        assert_eq!(
            config.theme_cycle(),
            vec![
                "lauds",
                "zenbones-light",
                "zenwritten-light",
                "papercolor-light",
                "lauds-blais",
                "rose-pine-dawn",
                "blush-rose",
                "sepia-lightest",
                "sepia-light",
                "green-lightest",
                "green-light",
                "kindle-sepia",
                "kindle-green"
            ]
        );
    }

    #[test]
    fn theme_cycle_uses_configured_list() {
        let mut config = Config::default();
        config.theme_cycle = Some(vec!["melange-light".to_string()]);
        assert_eq!(config.theme_cycle(), vec!["melange-light"]);
    }

    #[test]
    fn root_variant_for_returns_saved_index_unclamped() {
        let mut c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(c.root_variant_for("kindle-sepia"), 0);
        c.root_variants.insert("kindle-sepia".into(), 2);
        assert_eq!(c.root_variant_for("kindle-sepia"), 2);
        // A large saved index is returned verbatim — resolve_theme_variant
        // clamps out-of-range indices to 0 against the theme's candidate count.
        c.root_variants.insert("kindle-sepia".into(), 7);
        assert_eq!(c.root_variant_for("kindle-sepia"), 7);
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
