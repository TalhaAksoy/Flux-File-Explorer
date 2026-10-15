use std::path::PathBuf;

/// The current user's home directory.
pub fn home_dir() -> PathBuf {
    directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .unwrap_or_else(|| std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from))
}

/// Standard well-known directories for the sidebar, in display order.
/// Entries whose directory doesn't exist are skipped.
pub fn known_dirs() -> Vec<(&'static str, PathBuf)> {
    let mut out: Vec<(&'static str, PathBuf)> = Vec::new();
    if let Some(u) = directories::UserDirs::new() {
        let candidates: [(&'static str, Option<PathBuf>); 7] = [
            ("Desktop", u.desktop_dir().map(PathBuf::from)),
            ("Documents", u.document_dir().map(PathBuf::from)),
            ("Downloads", u.download_dir().map(PathBuf::from)),
            ("Pictures", u.picture_dir().map(PathBuf::from)),
            ("Music", u.audio_dir().map(PathBuf::from)),
            ("Videos", u.video_dir().map(PathBuf::from)),
            ("Home", Some(u.home_dir().to_path_buf())),
        ];
        for (label, p) in candidates {
            if let Some(p) = p
                && p.exists() && !out.iter().any(|(_, q)| q == &p) {
                    out.push((label, p));
                }
        }
    }
    if out.is_empty() {
        let h = home_dir();
        for label in ["Desktop", "Documents", "Downloads"] {
            let p = h.join(label);
            if p.exists() {
                out.push((match label {
                    "Desktop" => "Desktop",
                    "Documents" => "Documents",
                    _ => "Downloads",
                }, p));
            }
        }
        out.push(("Home", h));
    }
    out
}

#[derive(Clone, Copy, Debug)]
pub struct FreeSpace {
    pub total: u64,
    pub available: u64,
}

/// Total/available bytes for the filesystem containing `path`.
pub fn free_space(path: &std::path::Path) -> Option<FreeSpace> {
    let total = fs2::total_space(path).ok()?;
    let available = fs2::available_space(path).ok()?;
    Some(FreeSpace { total, available })
}
