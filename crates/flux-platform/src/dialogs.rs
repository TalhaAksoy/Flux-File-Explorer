//! Native OS dialogs — folder picking for "Add Bookmark…" and friends.
//!
//! macOS/Windows go through `rfd` (NSOpenPanel / IFileOpenDialog).
//! Linux spawns `zenity` (GNOME/GTK) or `kdialog` (KDE/Qt) so we pull no
//! GTK/portal dependency chain; both return `None` when absent.

use std::path::PathBuf;

/// Outcome of [`pick_folder`]. `Unavailable` (no picker backend exists —
/// e.g. a minimal Linux without zenity/kdialog) is surfaced separately so
/// the UI can say *something* instead of failing silently like a cancel.
#[derive(Debug)]
pub enum PickResult {
    Picked(PathBuf),
    Cancelled,
    Unavailable,
}

/// Ask the user for a directory with the platform-native picker.
pub fn pick_folder() -> PickResult {
    imp::pick_folder()
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod imp {
    use super::PickResult;

    pub fn pick_folder() -> PickResult {
        match rfd::FileDialog::new()
            .set_title("Add to Sidebar")
            .pick_folder()
        {
            Some(p) => PickResult::Picked(p),
            None => PickResult::Cancelled,
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::PickResult;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    pub fn pick_folder() -> PickResult {
        let home = directories::UserDirs::new()
            .map(|u| u.home_dir().to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".to_string());
        let candidates: &[(&str, Vec<&str>)] = &[
            (
                "zenity",
                vec!["--file-selection", "--directory", "--title=Add to Sidebar"],
            ),
            ("kdialog", vec!["--getexistingdirectory", home.as_str()]),
        ];
        for (cmd, args) in candidates {
            match Command::new(cmd)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
            {
                // Tool isn't installed — try the next candidate. A *spawned*
                // picker that exits non-zero was cancelled by the user, so
                // we must not pop a second dialog in its place.
                Err(_) => continue,
                Ok(out) => {
                    if !out.status.success() {
                        return PickResult::Cancelled;
                    }
                    // Strip exactly the trailing newline the tool appends —
                    // a folder name may legitimately end in a space, so a
                    // blanket whitespace trim would corrupt real paths.
                    let mut bytes = out.stdout;
                    if bytes.last() == Some(&b'\n') {
                        bytes.pop();
                        if bytes.last() == Some(&b'\r') {
                            bytes.pop();
                        }
                    }
                    if bytes.is_empty() {
                        return PickResult::Cancelled;
                    }
                    use std::os::unix::ffi::OsStringExt;
                    return PickResult::Picked(PathBuf::from(std::ffi::OsString::from_vec(bytes)));
                }
            }
        }
        PickResult::Unavailable
    }
}

#[cfg(not(any(unix, target_os = "windows")))]
mod imp {
    use super::PickResult;
    pub fn pick_folder() -> PickResult {
        PickResult::Unavailable
    }
}
