//! flux-settings: user preferences persisted atomically as JSON.
//! Writes go to a sibling temp file then rename — a crash mid-write never
//! leaves a truncated settings file.

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
    System,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Density {
    Compact,
    Standard,
    Comfortable,
}

/// File-pane presentation. `List` is the classic details table; `Compact`
/// a names-only dense list; `Grid`/`Gallery` icon cells (Gallery = large).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    List,
    Compact,
    Grid,
    Gallery,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum IconSource {
    Flux,
    System,
    #[default]
    Automatic,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub theme: Theme,
    #[serde(default)]
    pub icon_source: IconSource,
    pub density: Density,
    pub show_hidden: bool,
    /// Reveal hidden+system "protected operating system files" (Windows).
    /// Stronger gate than `show_hidden`, mirroring Explorer's option.
    #[serde(default)]
    pub show_system_files: bool,
    /// Right-side preview panel (Alt+P / status-bar toggle).
    #[serde(default)]
    pub show_preview: bool,
    /// File-pane view mode (details list / compact / grid / gallery).
    #[serde(default)]
    pub view_mode: ViewMode,
    pub confirm_permanent_delete: bool,
    /// Saved tab paths for session restore.
    pub tabs: Vec<PathBuf>,
    pub bookmarks: Vec<PathBuf>,
    /// User-defined ordering for the Places section (paths in display
    /// order). Entries missing from this list keep their default position.
    #[serde(default)]
    pub places_order: Vec<PathBuf>,
    pub recents: Vec<PathBuf>,
    pub sidebar_width: f32,
    /// Column widths [name, size, modified, type] in points.
    pub column_widths: [f32; 4],
    /// Sort spec mirrored as plain data to keep flux-settings GUI-free.
    pub sort_key: String,
    pub sort_desc: bool,
    pub dirs_first: bool,
    /// Key into the UI font-choice table ("" = system default).
    #[serde(default)]
    pub ui_font: String,
    /// Key into the mono font-choice table ("" = default).
    #[serde(default)]
    pub mono_font: String,
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: f32,
    /// Whole-app interface scale (egui zoom factor).
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
}

fn default_terminal_font_size() -> f32 {
    12.5
}
fn default_ui_scale() -> f32 {
    1.0
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            icon_source: IconSource::Automatic,
            density: Density::Standard,
            show_hidden: false,
            show_system_files: false,
            show_preview: false,
            view_mode: ViewMode::List,
            confirm_permanent_delete: true,
            tabs: Vec::new(),
            bookmarks: Vec::new(),
            places_order: Vec::new(),
            recents: Vec::new(),
            sidebar_width: 190.0,
            column_widths: [340.0, 90.0, 160.0, 90.0],
            sort_key: "name".into(),
            sort_desc: false,
            dirs_first: true,
            ui_font: String::new(),
            mono_font: String::new(),
            terminal_font_size: default_terminal_font_size(),
            ui_scale: default_ui_scale(),
        }
    }
}

fn settings_path() -> PathBuf {
    // Overridable for tests and isolated screenshot runs.
    if let Some(p) = std::env::var_os("FLUX_SETTINGS_PATH") {
        return PathBuf::from(p);
    }
    let base = directories::ProjectDirs::from("dev", "flux", "flux-files")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("settings.json")
}

pub fn load() -> Settings {
    load_from(&settings_path())
}

pub fn load_from(path: &std::path::Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("settings parse failed ({e}); using defaults");
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

/// Atomic write: temp file + rename within the same directory.
pub fn save(s: &Settings) -> io::Result<()> {
    save_to(s, &settings_path())
}

/// Atomic write to an explicit path: temp file + rename within the same
/// directory.
pub fn save_to(s: &Settings, path: &std::path::Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(s)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

pub fn push_recent(s: &mut Settings, path: PathBuf) {
    s.recents.retain(|p| p != &path);
    s.recents.insert(0, path);
    s.recents.truncate(20);
}
