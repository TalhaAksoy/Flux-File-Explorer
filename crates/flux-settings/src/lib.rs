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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub theme: Theme,
    pub density: Density,
    pub show_hidden: bool,
    pub confirm_permanent_delete: bool,
    /// Saved tab paths for session restore.
    pub tabs: Vec<PathBuf>,
    pub bookmarks: Vec<PathBuf>,
    pub recents: Vec<PathBuf>,
    pub sidebar_width: f32,
    /// Column widths [name, size, modified, type] in points.
    pub column_widths: [f32; 4],
    /// Sort spec mirrored as plain data to keep flux-settings GUI-free.
    pub sort_key: String,
    pub sort_desc: bool,
    pub dirs_first: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            density: Density::Standard,
            show_hidden: false,
            confirm_permanent_delete: true,
            tabs: Vec::new(),
            bookmarks: Vec::new(),
            recents: Vec::new(),
            sidebar_width: 190.0,
            column_widths: [340.0, 90.0, 160.0, 90.0],
            sort_key: "name".into(),
            sort_desc: false,
            dirs_first: true,
        }
    }
}

fn settings_path() -> PathBuf {
    let base = directories::ProjectDirs::from("dev", "flux", "flux-files")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("settings.json")
}

pub fn load() -> Settings {
    let path = settings_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("settings parse failed ({e}); using defaults");
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

/// Atomic write: temp file + rename within the same directory.
pub fn save(s: &Settings) -> io::Result<()> {
    let path = settings_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(s)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

pub fn push_recent(s: &mut Settings, path: PathBuf) {
    s.recents.retain(|p| p != &path);
    s.recents.insert(0, path);
    s.recents.truncate(20);
}
