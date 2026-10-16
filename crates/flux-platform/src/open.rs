use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Open a path with the OS default handler, detached.
pub fn open_path(path: &Path) -> io::Result<()> {
    spawn_open(path).map(|_| ())
}

/// Open several paths; errors are collected but don't stop the rest.
pub fn open_paths(paths: &[PathBuf]) -> Vec<(PathBuf, io::Error)> {
    let mut errs = Vec::new();
    for p in paths {
        if let Err(e) = open_path(p) {
            errs.push((p.clone(), e));
        }
    }
    errs
}

/// Reveal a file in the OS file manager (selects the item when supported).
pub fn reveal_path(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg("-R").arg(path).spawn()?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer").arg("/select,").arg(path).spawn()?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // FreeDesktop has no universal "select"; open the parent directory.
        let dir = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or_default()
        };
        return open_path(&dir);
    }
    #[allow(unreachable_code)]
    Ok(())
}

/// Open a terminal emulator rooted at `dir`. Configurable via
/// `FLUX_TERMINAL` (command name/path); otherwise platform defaults.
pub fn open_in_terminal(dir: &Path) -> io::Result<()> {
    if let Ok(custom) = std::env::var("FLUX_TERMINAL")
        && !custom.is_empty()
    {
        // Custom terminal: split "cmd args" cheaply on first space.
        let (bin, args) = match custom.split_once(' ') {
            Some((b, rest)) => (b.to_string(), vec![rest.to_string()]),
            None => (custom, Vec::new()),
        };
        return Command::new(bin).args(args).arg(dir).spawn().map(|_| ());
    }
    platform_terminal(dir)
}

#[cfg(target_os = "macos")]
fn platform_terminal(dir: &Path) -> io::Result<()> {
    Command::new("open")
        .arg("-a")
        .arg("Terminal")
        .arg(dir)
        .spawn()
        .map(|_| ())
}

#[cfg(target_os = "windows")]
fn platform_terminal(dir: &Path) -> io::Result<()> {
    // Prefer Windows Terminal, fall back to cmd via start.
    if Command::new("wt").arg("-d").arg(dir).spawn().is_ok() {
        return Ok(());
    }
    Command::new("cmd")
        .args(["/c", "start", "/d"])
        .arg(dir)
        .arg("cmd")
        .spawn()
        .map(|_| ())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_terminal(dir: &Path) -> io::Result<()> {
    // DE-native terminals are spawn clients of a daemon — `current_dir`
    // does not reliably reach the new shell, so they need an explicit
    // working-directory flag. Direct-process terminals inherit cwd fine.
    for term in [
        "kgx",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "x-terminal-emulator",
        "alacritty",
        "kitty",
        "xterm",
    ] {
        let spawned = match term {
            "kgx" | "gnome-terminal" | "xfce4-terminal" => Command::new(term)
                .arg("--working-directory")
                .arg(dir)
                .spawn(),
            "konsole" => Command::new(term).arg("--workdir").arg(dir).spawn(),
            _ => Command::new(term).current_dir(dir).spawn(),
        };
        if spawned.is_ok() {
            return Ok(());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "no terminal emulator found",
    ))
}

#[cfg(target_os = "macos")]
fn spawn_open(path: &Path) -> io::Result<std::process::Child> {
    Command::new("open").arg(path).spawn()
}

#[cfg(target_os = "windows")]
fn spawn_open(path: &Path) -> io::Result<std::process::Child> {
    // explorer exits nonzero even on success; we only care about spawn errors.
    Command::new("explorer").arg(path).spawn()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn spawn_open(path: &Path) -> io::Result<std::process::Child> {
    Command::new("xdg-open").arg(path).spawn()
}

/// One "Open With" candidate — a `.desktop` file on Linux, an app bundle
/// name on macOS. Windows has no list here: it uses the system dialog.
#[derive(Clone, Debug)]
pub struct OpenWithApp {
    pub name: String,
    /// Linux: desktop-file id (path). macOS: app name for `open -a`.
    pub id: String,
    /// Default handler for this file type — listed first, marked in UI.
    pub is_default: bool,
}

/// Launch `path` with the app identified by `app.id` (see
/// [`open_with_apps`]).
pub fn open_with(path: &Path, app: &OpenWithApp) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-a", &app.id])
            .arg(path)
            .spawn()
            .map(|_| ())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let Some(exec) = desktop_exec(&app.id) else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "desktop file gone"));
        };
        let mut argv = expand_exec(&exec);
        if argv.is_empty() {
            return Err(io::Error::other("empty Exec line"));
        }
        // No %f/%u field code ⇒ append the file ourselves.
        if !exec_contains_field(&exec) {
            argv.push(path.to_string_lossy().into_owned());
        } else {
            for a in &mut argv {
                if *a == "{FIELD}" {
                    *a = path.to_string_lossy().into_owned();
                }
            }
        }
        let prog = argv.remove(0);
        Command::new(prog).args(&argv).spawn().map(|_| ())
    }
    #[cfg(windows)]
    {
        let _ = (path, app);
        unreachable!("Windows uses open_with_dialog instead of an app list");
    }
}

/// The "Open With" candidates for `path`. Linux parses FreeDesktop
/// `.desktop` files whose MimeType covers the file's type; macOS lists
/// installed app bundles; Windows returns empty (system dialog used).
pub fn open_with_apps(path: &Path) -> Vec<OpenWithApp> {
    #[cfg(target_os = "macos")]
    {
        let _ = path;
        macos_apps()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux_apps(path)
    }
    #[cfg(windows)]
    {
        let _ = path;
        Vec::new()
    }
}

/// Windows' own "Open with" chooser — the same dialog Explorer shows,
/// including "always use this app" and Store lookup.
#[cfg(windows)]
pub fn open_with_dialog(path: &Path) -> io::Result<()> {
    // rundll32 arg must be the single comma-joined token.
    Command::new("rundll32")
        .arg(format!("shell32.dll,OpenAs_RunDLL {}", path.display()))
        .spawn()
        .map(|_| ())
}

#[cfg(target_os = "macos")]
fn macos_apps() -> Vec<OpenWithApp> {
    let mut names: Vec<String> = Vec::new();
    for dir in [
        "/Applications",
        "/System/Applications",
        "/System/Applications/Utilities",
    ] {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "app")
                    && let Some(stem) = p.file_stem()
                {
                    names.push(stem.to_string_lossy().into_owned());
                }
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let user_apps = PathBuf::from(home).join("Applications");
        if let Ok(rd) = std::fs::read_dir(&user_apps) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "app")
                    && let Some(stem) = p.file_stem()
                {
                    names.push(stem.to_string_lossy().into_owned());
                }
            }
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
        .into_iter()
        .map(|n| OpenWithApp {
            id: n.clone(),
            name: n,
            is_default: false,
        })
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_apps(path: &Path) -> Vec<OpenWithApp> {
    let mime = Command::new("xdg-mime")
        .args(["query", "filetype"])
        .arg(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if mime.is_empty() {
        return Vec::new();
    }
    let default_id = Command::new("xdg-mime")
        .args(["query", "default", &mime])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    // Supertype for wildcard matches: "image/png" also accepts "image/*".
    let supertype = mime.split('/').next().map(|s| format!("{s}/*"));

    let mut dirs: Vec<PathBuf> = vec![];
    let data_home = std::env::var_os("XDG_DATA_HOME").or_else(|| {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share").into_os_string())
    });
    if let Some(h) = &data_home {
        dirs.push(PathBuf::from(h).join("applications"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/flatpak/exports/share/applications"));
    }
    let xdg_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for d in xdg_dirs.split(':').filter(|d| !d.is_empty()) {
        dirs.push(PathBuf::from(d).join("applications"));
    }
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));

    let mut out: Vec<OpenWithApp> = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "desktop") {
                continue;
            }
            let Some(de) = parse_desktop(&p) else {
                continue;
            };
            if de.hidden || de.nodisplay || de.terminal {
                continue;
            }
            let serves = de
                .mimetypes
                .iter()
                .any(|m| m == &mime || Some(m) == supertype.as_ref());
            if !serves {
                continue;
            }
            let id = p.to_string_lossy().into_owned();
            out.push(OpenWithApp {
                is_default: de.id.as_deref() == Some(default_id.as_str()),
                name: de.name,
                id,
            });
        }
    }
    out.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

#[cfg(all(unix, not(target_os = "macos")))]
struct DesktopEntry {
    id: Option<String>,
    name: String,
    mimetypes: Vec<String>,
    nodisplay: bool,
    hidden: bool,
    terminal: bool,
}

/// Parse the [Desktop Entry] group fields we need; `id` is the file's
/// desktop-id (stem or Name is unreliable for `xdg-mime` results, so we
/// take the filename stem).
#[cfg(all(unix, not(target_os = "macos")))]
fn parse_desktop(path: &Path) -> Option<DesktopEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_group = false;
    let mut name = None;
    let mut mimetypes = Vec::new();
    let (mut nodisplay, mut hidden, mut terminal) = (false, false, false);
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_group = l == "[Desktop Entry]";
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((k, v)) = l.split_once('=') else {
            continue;
        };
        match k.trim() {
            "Name" => name = Some(v.trim().to_string()),
            "MimeType" => {
                mimetypes = v
                    .split(';')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "NoDisplay" => nodisplay = v.trim() == "true",
            "Hidden" => hidden = v.trim() == "true",
            "Terminal" => terminal = v.trim() == "true",
            _ => {}
        }
    }
    let id = path.file_name().map(|n| n.to_string_lossy().into_owned());
    Some(DesktopEntry {
        id,
        name: name?,
        mimetypes,
        nodisplay,
        hidden,
        terminal,
    })
}

/// Read the `Exec=` line of a desktop file (path stored in `OpenWithApp.id`).
#[cfg(all(unix, not(target_os = "macos")))]
fn desktop_exec(desktop_path: &str) -> Option<String> {
    let text = std::fs::read_to_string(desktop_path).ok()?;
    let mut in_group = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_group = l == "[Desktop Entry]";
            continue;
        }
        if in_group && let Some(v) = l.strip_prefix("Exec=") {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// Does the Exec line carry a file field code (%f/%F/%u/%U)? When absent
/// the file path is appended to the argv.
#[cfg(all(unix, not(target_os = "macos")))]
fn exec_contains_field(exec: &str) -> bool {
    exec.contains("%f") || exec.contains("%F") || exec.contains("%u") || exec.contains("%U")
}

/// Split a desktop `Exec=` line into argv, honoring double quotes and
/// resolving field codes. File placeholders become the literal marker
/// `{FIELD}` (the caller substitutes real paths); all other %-codes and
/// %% literals are resolved per the spec enough for launch purposes.
#[cfg(all(unix, not(target_os = "macos")))]
fn expand_exec(exec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut chars = exec.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => in_q = !in_q,
            c if c.is_whitespace() && !in_q => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            '%' => match chars.next() {
                Some('f') | Some('F') | Some('u') | Some('U') => cur.push_str("{FIELD}"),
                Some('%') => cur.push('%'),
                // %i/%c/%k/%d/… — drop the whole placeholder.
                Some(_) => {}
                None => cur.push('%'),
            },
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.retain(|a| !a.is_empty());
    out
}

#[cfg(test)]
mod tests {
    #[cfg(all(unix, not(target_os = "macos")))]
    use super::*;

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn expand_exec_strips_field_codes() {
        assert_eq!(expand_exec("gedit %f"), ["gedit", "{FIELD}"]);
        assert_eq!(
            expand_exec("/usr/bin/foo --flag %U"),
            ["/usr/bin/foo", "--flag", "{FIELD}"]
        );
        assert_eq!(expand_exec("foo"), ["foo"]);
        assert_eq!(expand_exec("foo %i %c"), ["foo"]);
        assert_eq!(
            expand_exec("foo \"--arg with space\" %f"),
            ["foo", "--arg with space", "{FIELD}"]
        );
        assert_eq!(expand_exec("foo 100%%"), ["foo", "100%"]);
        assert!(exec_contains_field("foo %u"));
        assert!(!exec_contains_field("foo --bar"));
    }
}
