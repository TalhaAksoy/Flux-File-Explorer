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
        Command::new("explorer")
            .arg("/select,")
            .arg(path)
            .spawn()?;
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
        && !custom.is_empty() {
            // Custom terminal: split "cmd args" cheaply on first space.
            let (bin, args) = match custom.split_once(' ') {
                Some((b, rest)) => (b.to_string(), vec![rest.to_string()]),
                None => (custom, Vec::new()),
            };
            return Command::new(bin)
                .args(args)
                .arg(dir)
                .spawn()
                .map(|_| ());
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
