use std::path::{Path, PathBuf};

/// What the engine does when a destination already exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// Pause the op and ask the UI.
    Ask,
    OverwriteAll,
    SkipAll,
    /// Write next to the existing file with a " (n)" suffix.
    KeepBothAll,
}

/// A user's answer to one conflict (or all, when `apply_all`).
#[derive(Clone, Debug)]
pub struct ConflictDecision {
    pub action: ConflictAction,
    pub apply_all: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictAction {
    Overwrite,
    Skip,
    KeepBoth,
    CancelOp,
}

/// A question the worker raises to the UI.
#[derive(Clone, Debug)]
pub struct ConflictQuery {
    pub op_id: u64,
    pub source: PathBuf,
    pub dest: PathBuf,
    /// "already exists" conflicts only for now.
    pub reason: &'static str,
}

/// Find `dir/name (n)` — first free variant starting at n=2.
/// "file.txt" → "file (2).txt". Assumes `dir/name` is taken.
pub fn keep_both_path(dir: &Path, name: &std::ffi::OsStr) -> PathBuf {
    let p = Path::new(name);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = p.extension().map(|e| e.to_string_lossy().into_owned());
    for n in 2..10_000u32 {
        let candidate = match &ext {
            Some(e) if !e.is_empty() => format!("{stem} ({n}).{e}"),
            _ => format!("{stem} ({n})"),
        };
        let c = dir.join(candidate);
        if !c.exists() {
            return c;
        }
    }
    // Give up with a time-stamped name — better than failing outright.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    dir.join(match &ext {
        Some(e) if !e.is_empty() => format!("{stem}-{stamp}.{e}"),
        _ => format!("{stem}-{stamp}"),
    })
}

/// Filename validity for rename targets on the *current* platform.
/// Conservative shared rules (no `/`, NUL, empty, pure dots, >255 bytes);
/// Windows adds its reserved set.
pub fn validate_filename(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name is empty".into());
    }
    if name.len() > 255 {
        return Err("name exceeds 255 bytes".into());
    }
    if name == "." || name == ".." {
        return Err("reserved name".into());
    }
    if name.contains('/') || name.contains('\0') {
        return Err("contains '/' or NUL".into());
    }
    if cfg!(windows) {
        for c in ['<', '>', ':', '"', '\\', '|', '?', '*'] {
            if name.contains(c) {
                return Err(format!("contains '{c}'"));
            }
        }
        if name.ends_with(['.', ' ']) {
            return Err("ends with '.' or space".into());
        }
        const RESERVED: [&str; 22] = [
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5",
            "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4",
            "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ];
        let stem = name.split('.').next().unwrap_or_default();
        if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
            return Err(format!("'{stem}' is reserved on Windows"));
        }
    }
    Ok(())
}
