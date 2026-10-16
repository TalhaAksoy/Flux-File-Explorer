//! OS-level file clipboard interop.
//!
//! - macOS: `NSPasteboard` file URLs (Finder Cmd+C / Cmd+V).
//! - Windows: `CF_HDROP` + `Preferred DropEffect` (Explorer copy/cut).
//! - Linux: `wl-copy`/`wl-paste` or `xclip` offering
//!   `x-special/gnome-copied-files` (the GNOME/KDE copy-or-cut list) and
//!   `text/uri-list`.
//!
//! Every entry point degrades silently — a missing tool or unavailable
//! pasteboard yields `None`/`false` and the UI keeps using its internal
//! clipboard. Nothing here ever panics on platform API failure.

use std::path::PathBuf;

/// What the OS clipboard offered on read: file paths plus whether they
/// were cut (move semantics) or copied.
#[derive(Debug, Clone)]
pub struct ClipPayload {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
}

/// Put `paths` on the OS file clipboard. `cut` marks a move where the
/// platform supports it (Windows DropEffect, Linux x-special verb);
/// Finder has no cut verb — it is always a copy.
pub fn write_files(paths: &[PathBuf], cut: bool) {
    if paths.is_empty() {
        return;
    }
    imp::write_files(paths, cut);
}

/// File paths currently on the OS clipboard, or `None` when the
/// clipboard holds no file list (or the platform tool is unavailable).
pub fn read_files() -> Option<ClipPayload> {
    imp::read_files()
}

// ---------------------------------------------------------------------------
// URI helpers (shared by the Linux backend)
// ---------------------------------------------------------------------------

/// `file://` URI for a path: UTF-8 bytes, percent-encoding everything
/// except RFC 3986 unreserved characters and '/'.
#[cfg(all(unix, not(target_os = "macos")))]
fn file_uri(p: &std::path::Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut s = String::with_capacity(p.as_os_str().len() + 8);
    s.push_str("file://");
    for &b in p.as_os_str().as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                s.push(b as char)
            }
            _ => s.push_str(&format!("%{b:02X}")),
        }
    }
    s
}

/// Decode a `file://` URI back to a path; returns None for non-file
/// schemes or malformed input.
#[cfg(all(unix, not(target_os = "macos")))]
fn uri_to_path(s: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let rest = s.strip_prefix("file://")?;
    // The path starts at the first '/': covers both "file:///p" (empty
    // authority) and "file://host/p" (authority we treat as local).
    let i = rest.find('/')?;
    let rest = &rest[i..];
    let mut bytes = Vec::with_capacity(rest.len());
    let bs = rest.as_bytes();
    let mut i = 0;
    while i < bs.len() {
        if bs[i] == b'%' && i + 2 < bs.len() {
            let h = (bs[i + 1] as char).to_digit(16)?;
            let l = (bs[i + 2] as char).to_digit(16)?;
            bytes.push(((h << 4) | l) as u8);
            i += 3;
        } else {
            bytes.push(bs[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

// ---------------------------------------------------------------------------
// macOS — NSPasteboard
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod imp {
    use super::{ClipPayload, PathBuf};

    pub fn write_files(paths: &[PathBuf], _cut: bool) {
        use objc2::runtime::ProtocolObject;
        use objc2_app_kit::{NSPasteboard, NSPasteboardWriting};
        use objc2_foundation::{NSArray, NSString, NSURL};

        // Build URLs first: if nothing converts, leave the user's existing
        // pasteboard untouched rather than clearing it.
        let urls: Vec<objc2::rc::Retained<ProtocolObject<dyn NSPasteboardWriting>>> = paths
            .iter()
            .filter_map(|p| p.to_str())
            .map(|s| ProtocolObject::from_retained(NSURL::fileURLWithPath(&NSString::from_str(s))))
            .collect();
        if urls.is_empty() {
            return;
        }
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        let arr = NSArray::from_retained_slice(&urls);
        pb.writeObjects(&arr);
    }

    #[allow(unsafe_code)]
    pub fn read_files() -> Option<ClipPayload> {
        use objc2_app_kit::NSPasteboard;
        use objc2_foundation::{NSArray, NSURL};

        let pb = NSPasteboard::generalPasteboard();
        let classes = NSArray::from_slice(&[objc2::class!(NSURL)]);
        let objs = unsafe { pb.readObjectsForClasses_options(&classes, None) }?;
        let mut paths = Vec::new();
        for o in objs.iter() {
            let Some(u) = o.downcast_ref::<NSURL>() else {
                continue;
            };
            if u.isFileURL()
                && let Some(p) = u.path()
            {
                paths.push(PathBuf::from(p.to_string()));
            }
        }
        if paths.is_empty() {
            return None;
        }
        Some(ClipPayload { paths, cut: false })
    }
}

// ---------------------------------------------------------------------------
// Windows — CF_HDROP + Preferred DropEffect
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod imp {
    use super::{ClipPayload, PathBuf};
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GlobalFree, HWND};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows_sys::Win32::UI::Shell::{DROPFILES, DragQueryFileW};

    const CF_HDROP: u32 = 15;
    const GMEM_MOVEABLE_ZEROINIT: u32 = 0x0042; // GMEM_MOVEABLE | GMEM_ZEROINIT
    const DROPEFFECT_COPY: u32 = 1;
    const DROPEFFECT_MOVE: u32 = 2;

    #[allow(unsafe_code)]
    pub fn write_files(paths: &[PathBuf], cut: bool) {
        // DROPFILES header + NUL-separated UTF-16 paths + final NUL.
        let mut w: Vec<u16> = Vec::new();
        for p in paths {
            w.extend(p.as_os_str().encode_wide());
            w.push(0);
        }
        w.push(0);
        let hdr = DROPFILES {
            pFiles: std::mem::size_of::<DROPFILES>() as u32,
            pt: windows_sys::Win32::Foundation::POINT { x: 0, y: 0 },
            fNC: 0,
            fWide: 1,
        };
        let total = std::mem::size_of::<DROPFILES>() + w.len() * 2;
        unsafe {
            let hglob = GlobalAlloc(GMEM_MOVEABLE_ZEROINIT, total);
            if hglob.is_null() {
                return;
            }
            let ptr = GlobalLock(hglob);
            if ptr.is_null() {
                GlobalFree(hglob);
                return;
            }
            std::ptr::write(ptr as *mut DROPFILES, hdr);
            std::ptr::copy_nonoverlapping(
                w.as_ptr(),
                (ptr as *mut u16).add(std::mem::size_of::<DROPFILES>() / 2),
                w.len(),
            );
            GlobalUnlock(hglob);
            if OpenClipboard(0 as HWND) == 0 {
                GlobalFree(hglob);
                return;
            }
            EmptyClipboard();
            // Ownership of hglob transfers to the clipboard on success.
            SetClipboardData(CF_HDROP, hglob);
            // Preferred DropEffect: lets Explorer distinguish cut vs copy.
            let fmt_name: Vec<u16> = "Preferred DropEffect\0".encode_utf16().collect();
            let fmt = RegisterClipboardFormatW(fmt_name.as_ptr());
            if fmt != 0 {
                let hfx = GlobalAlloc(GMEM_MOVEABLE_ZEROINIT, 4);
                if !hfx.is_null() {
                    let pfx = GlobalLock(hfx);
                    if pfx.is_null() {
                        GlobalFree(hfx);
                    } else {
                        *(pfx as *mut u32) = if cut {
                            DROPEFFECT_MOVE
                        } else {
                            DROPEFFECT_COPY
                        };
                        GlobalUnlock(hfx);
                        if SetClipboardData(fmt, hfx).is_null() {
                            GlobalFree(hfx);
                        }
                    }
                }
            }
            CloseClipboard();
        }
    }

    #[allow(unsafe_code)]
    pub fn read_files() -> Option<ClipPayload> {
        unsafe {
            if OpenClipboard(0 as HWND) == 0 {
                return None;
            }
            let out = (|| {
                if IsClipboardFormatAvailable(CF_HDROP) == 0 {
                    return None;
                }
                let h = GetClipboardData(CF_HDROP);
                if h.is_null() {
                    return None;
                }
                let count = DragQueryFileW(h, u32::MAX, std::ptr::null_mut(), 0);
                let mut paths = Vec::with_capacity(count as usize);
                let mut buf = vec![0u16; 4096];
                for i in 0..count {
                    let n = DragQueryFileW(h as _, i, std::ptr::null_mut(), 0) as usize;
                    if n + 1 > buf.len() {
                        buf.resize(n + 1, 0);
                    }
                    let got = DragQueryFileW(h, i, buf.as_mut_ptr(), buf.len() as u32);
                    if got == 0 {
                        continue;
                    }
                    let s = String::from_utf16_lossy(&buf[..got as usize]);
                    paths.push(PathBuf::from(s));
                }
                if paths.is_empty() {
                    return None;
                }
                // Preferred DropEffect == 2 → Explorer cut.
                let fmt_name: Vec<u16> = "Preferred DropEffect\0".encode_utf16().collect();
                let fmt = RegisterClipboardFormatW(fmt_name.as_ptr());
                let mut cut = false;
                if fmt != 0 && IsClipboardFormatAvailable(fmt) != 0 {
                    let hfx = GetClipboardData(fmt);
                    if !hfx.is_null() {
                        let p = GlobalLock(hfx);
                        if !p.is_null() {
                            cut = *(p as *const u32) == DROPEFFECT_MOVE;
                            GlobalUnlock(hfx);
                        }
                    }
                }
                Some(ClipPayload { paths, cut })
            })();
            CloseClipboard();
            out
        }
    }
}

// ---------------------------------------------------------------------------
// Linux / other unix — wl-copy / wl-paste / xclip subprocesses
// ---------------------------------------------------------------------------

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::{ClipPayload, PathBuf, file_uri, uri_to_path};
    use std::io::Write;
    use std::process::{Command, Stdio};

    /// GNOME/KDE file managers honor this type for copy *and* cut verbs.
    const GNOME_FILES_MIME: &str = "x-special/gnome-copied-files";

    fn wayland_session() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
    }

    /// Bound on a child process. Selection reads go through whichever app
    /// owns the clipboard — a hung owner must not freeze the UI thread.
    const CLIP_IO_DEADLINE: std::time::Duration = std::time::Duration::from_millis(400);

    fn wait_bounded(child: &mut std::process::Child) -> bool {
        use std::time::{Duration, Instant};
        let deadline = Instant::now() + CLIP_IO_DEADLINE;
        loop {
            match child.try_wait() {
                Ok(Some(st)) => return st.success(),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
            }
        }
    }

    fn run_stdin(cmd: &str, args: &[&str], data: &str) -> bool {
        let Ok(mut child) = Command::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        let ok = child
            .stdin
            .as_mut()
            .map(|s| s.write_all(data.as_bytes()).is_ok())
            .unwrap_or(false);
        drop(child.stdin.take());
        // wl-copy/xclip daemonize to own the selection; the wait mainly
        // catches immediate failure (bad args, no display).
        wait_bounded(&mut child) && ok
    }

    fn run_capture(cmd: &str, args: &[&str]) -> Option<String> {
        use std::io::Read;
        let mut child = Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        if !wait_bounded(&mut child) {
            return None;
        }
        let mut s = String::new();
        child.stdout.take()?.read_to_string(&mut s).ok()?;
        if s.is_empty() { None } else { Some(s) }
    }

    pub fn write_files(paths: &[PathBuf], cut: bool) {
        let mut data = String::from(if cut { "cut\n" } else { "copy\n" });
        for p in paths {
            data.push_str(&file_uri(p));
            data.push('\n');
        }
        // Prefer the native tool for the running session; fall back to the
        // other (xclip works under XWayland, wl-copy under Wayland-only).
        let writers: &[(&str, &[&str])] = if wayland_session() {
            &[
                ("wl-copy", &["-t", GNOME_FILES_MIME]),
                (
                    "xclip",
                    &["-selection", "clipboard", "-t", GNOME_FILES_MIME],
                ),
            ]
        } else {
            &[
                (
                    "xclip",
                    &["-selection", "clipboard", "-t", GNOME_FILES_MIME],
                ),
                ("wl-copy", &["-t", GNOME_FILES_MIME]),
            ]
        };
        for (cmd, args) in writers {
            if run_stdin(cmd, args, &data) {
                return;
            }
        }
    }

    pub fn read_files() -> Option<ClipPayload> {
        let readers: &[(&str, &[&str], bool)] = if wayland_session() {
            &[
                ("wl-paste", &["-t", GNOME_FILES_MIME], true),
                (
                    "xclip",
                    &["-o", "-selection", "clipboard", "-t", GNOME_FILES_MIME],
                    true,
                ),
                ("wl-paste", &["-t", "text/uri-list"], false),
                (
                    "xclip",
                    &["-o", "-selection", "clipboard", "-t", "text/uri-list"],
                    false,
                ),
            ]
        } else {
            &[
                (
                    "xclip",
                    &["-o", "-selection", "clipboard", "-t", GNOME_FILES_MIME],
                    true,
                ),
                ("wl-paste", &["-t", GNOME_FILES_MIME], true),
                (
                    "xclip",
                    &["-o", "-selection", "clipboard", "-t", "text/uri-list"],
                    false,
                ),
                ("wl-paste", &["-t", "text/uri-list"], false),
            ]
        };
        for (cmd, args, has_verb) in readers {
            let Some(text) = run_capture(cmd, args) else {
                continue;
            };
            let mut lines = text.lines();
            let mut cut = false;
            if *has_verb {
                match lines.next() {
                    Some("cut") => cut = true,
                    Some("copy") => {}
                    _ => continue, // not a gnome-copied-files payload
                }
            }
            let paths: Vec<PathBuf> = lines
                .map(str::trim)
                .filter(|l| l.starts_with("file://"))
                .filter_map(uri_to_path)
                .collect();
            if !paths.is_empty() {
                return Some(ClipPayload { paths, cut });
            }
        }
        None
    }
}

#[cfg(not(any(unix, target_os = "macos", target_os = "windows")))]
mod imp {
    use super::{ClipPayload, PathBuf};
    pub fn write_files(_paths: &[PathBuf], _cut: bool) {}
    pub fn read_files() -> Option<ClipPayload> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip on the real OS clipboard. Skips silently in headless
    /// environments where no pasteboard service / clipboard tool exists.
    #[test]
    #[cfg(any(target_os = "macos", all(unix, not(target_os = "macos"))))]
    fn clipboard_round_trip_when_available() {
        let dir = std::env::temp_dir();
        let a = dir.join("flux-clip-a.tmp");
        let b = dir.join("flux-clip-b.tmp");
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"y").unwrap();
        write_files(&[a.clone(), b.clone()], false);
        match read_files() {
            Some(pl) => {
                assert!(pl.paths.contains(&a), "{:?}", pl.paths);
                assert!(pl.paths.contains(&b), "{:?}", pl.paths);
            }
            None => eprintln!("no OS clipboard available — skipped"),
        }
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn file_uri_round_trip() {
        let p = PathBuf::from("/tmp/with space/türkçe/dosya.txt");
        let uri = file_uri(&p);
        assert!(uri.starts_with("file:///tmp/"));
        assert!(!uri.contains(' '));
        assert_eq!(uri_to_path(&uri).unwrap(), p);
    }
}
