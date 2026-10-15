//! Filesystem watching, debounced. Watchers are advisory: they can drop
//! events on network filesystems or under load, so consumers must treat a
//! burst as "refresh cheaply", never as authoritative state.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

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
