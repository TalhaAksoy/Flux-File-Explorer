//! Filesystem watching, debounced. Watchers are advisory: they can drop
//! events on network filesystems or under load, so consumers must treat a
//! burst as "refresh cheaply", never as authoritative state.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::event::{AccessKind, AccessMode};
use notify::{EventKind, RecursiveMode, Watcher};

/// One debounced "something changed under this path" signal.
#[derive(Debug)]
pub struct WatchEvent {
    pub path: PathBuf,
}

pub struct FsWatcher {
    watcher: Option<notify::RecommendedWatcher>,
    rx: mpsc::Receiver<PathBuf>,
    watching: Option<PathBuf>,
    /// First undelivered change; delivered once the debounce window passes.
    pending_since: Option<Instant>,
}

const DEBOUNCE: Duration = Duration::from_millis(250);

/// True when `kind` reflects a change the UI can see. Linux's inotify
/// backend reports `Open`/`Close(Read)` for the watched directory itself —
/// including our own `read_dir` scans — so forwarding access events makes
/// every scan retrigger the debounce and refresh forever. `Close(Write)`
/// is kept: it marks a finished writer (covers mmap writes that emit no
/// `Modify`). macOS/Windows backends never emit access events.
fn is_content_change(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

impl Default for FsWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl FsWatcher {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<PathBuf>();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res
                && is_content_change(&ev.kind)
                && let Some(p) = ev.paths.first() {
                    let _ = tx.send(p.clone());
                }
        });
        Self {
            watcher: watcher.ok(),
            rx,
            watching: None,
            pending_since: None,
        }
    }

    /// True when the platform watcher could not be created.
    pub fn available(&self) -> bool {
        self.watcher.is_some()
    }

    /// Watch `dir` non-recursively. Replaces any previous watch.
    pub fn watch(&mut self, dir: &std::path::Path) {
        if self.watching.as_deref() == Some(dir) {
            return;
        }
        self.unwatch();
        if let Some(w) = self.watcher.as_mut()
            && w.watch(dir, RecursiveMode::NonRecursive).is_ok() {
                self.watching = Some(dir.to_path_buf());
            }
    }

    pub fn unwatch(&mut self) {
        if let (Some(w), Some(p)) = (self.watcher.as_mut(), self.watching.take()) {
            let _ = w.unwatch(&p);
        }
    }

    /// Returns true once per burst: drains raw events, then reports after the
    /// debounce window has elapsed since the first event of the burst.
    pub fn poll_changed(&mut self) -> bool {
        let mut got = false;
        while self.rx.try_recv().is_ok() {
            got = true;
        }
        if got && self.pending_since.is_none() {
            self.pending_since = Some(Instant::now());
        }
        match self.pending_since {
            Some(t) if t.elapsed() >= DEBOUNCE => {
                self.pending_since = None;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_change(w: &mut FsWatcher) -> bool {
        for _ in 0..30 {
            if w.poll_changed() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// A directory scan (`read_dir` opens the watched dir) must not feed
    /// back into the debounce — on Linux's inotify backend it emits `Open`
    /// access events that would otherwise refresh-loop the UI forever.
    #[test]
    fn scanning_watched_dir_does_not_self_trigger() {
        let dir = std::env::temp_dir().join(format!("flux-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut w = FsWatcher::new();
        if !w.available() {
            let _ = std::fs::remove_dir_all(&dir);
            return; // platform watcher unavailable (e.g. sandboxed CI)
        }
        w.watch(&dir);
        std::thread::sleep(Duration::from_millis(100));

        // Simulate an enumeration pass.
        for e in std::fs::read_dir(&dir).unwrap() {
            let _ = e;
        }
        assert!(!wait_for_change(&mut w));

        // A real content change still reports.
        std::fs::write(dir.join("new-file.txt"), b"x").unwrap();
        assert!(wait_for_change(&mut w));

        drop(w);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
