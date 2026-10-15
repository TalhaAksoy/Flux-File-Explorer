use std::path::PathBuf;

/// A mounted volume / drive the user can navigate to.
#[derive(Clone, Debug)]
pub struct Volume {
    pub label: String,
    pub mount: PathBuf,
    /// Best-effort "kind" for icon choice.
    pub removable: bool,
}

/// Mounted volumes, platform-adapted. Excludes pseudo/system mounts that
/// would only clutter the sidebar.
pub fn mounted_volumes() -> Vec<Volume> {
    platform_volumes()
}

#[cfg(target_os = "macos")]
fn platform_volumes() -> Vec<Volume> {
    let mut out = vec![Volume {
        label: "Macintosh HD".into(),
        mount: PathBuf::from("/"),
        removable: false,
    }];
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name == "Macintosh HD" {
                continue;
            }
            // Follow symlinks: /Volumes often links the boot volume.
            let removable = std::fs::metadata(&p)
                .map(|m| m.is_dir())
                .unwrap_or(false);
            if removable {
                out.push(Volume {
                    label: name,
                    mount: p,
                    removable: true,
                });
            }
        }
    }
    out
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_volumes() -> Vec<Volume> {
    let mut out = vec![Volume {
        label: "Root".into(),
        mount: PathBuf::from("/"),
        removable: false,
    }];
    let mut seen = std::collections::HashSet::from([PathBuf::from("/")]);
    // Well-known user mounts first.
    let user = std::env::var("USER").unwrap_or_default();
    for base in [
        format!("/media/{user}"),
        format!("/run/media/{user}"),
        "/mnt".to_string(),
    ] {
        if let Ok(rd) = std::fs::read_dir(&base) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() && seen.insert(p.clone()) {
                    out.push(Volume {
                        label: e.file_name().to_string_lossy().into_owned(),
                        mount: p,
                        removable: true,
                    });
                }
            }
        }
    }
    // /proc/mounts for anything else user-visible (nfs, fuse, cifs...).
    if let Ok(m) = std::fs::read_to_string("/proc/mounts") {
        for line in m.lines() {
            let mut it = line.split_whitespace();
            let (_dev, mount, fstype) = match (it.next(), it.next(), it.next()) {
                (Some(d), Some(m), Some(f)) => (d, m, f),
                _ => continue,
            };
            let interesting = matches!(
                fstype,
                "fuse" | "nfs" | "nfs4" | "cifs" | "smb3" | "sshfs" | "fuseblk"
                    | "exfat" | "ntfs" | "ntfs3" | "vfat" | "ext4" | "btrfs" | "xfs"
            );
            if !interesting {
                continue;
            }
            let p = PathBuf::from(mount.replace("\\040", " "));
            if seen.insert(p.clone()) && p.exists() {
                out.push(Volume {
                    label: p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| mount.to_string()),
                    mount: p,
                    removable: true,
                });
            }
        }
    }
    out
}

#[cfg(target_os = "windows")]
fn platform_volumes() -> Vec<Volume> {
    // Drive letters A:..Z: — probe existence cheaply.
    let mut out = Vec::new();
    for letter in b'A'..=b'Z' {
        let mount = PathBuf::from(format!("{}:\\", letter as char));
        if mount.exists() {
            out.push(Volume {
                label: format!("{}:", letter as char),
                mount,
                removable: letter != b'C',
            });
        }
    }
    out
}
