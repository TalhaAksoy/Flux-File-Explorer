//! Streaming directory enumeration on a dedicated worker thread.
//!
//! One worker owns a request queue; only the newest request matters — queued
//! requests are superseded by later navigation and drained. Cancellation is
//! cooperative via a monotonically increasing generation counter: a scan
//! checks every `CANCEL_CHECK_EVERY` entries whether its generation is still
//! current and aborts promptly when superseded.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use crate::entry::{DirEntry, EntryKind, is_hidden_name};
use crate::snapshot::SnapshotHandle;

/// Entries buffered per snapshot append / UI wakeup. Tuned by benchmarks;
/// large enough to keep message volume low, small enough that
/// time-to-first-visible stays well under 250ms on local disks.
pub const BATCH_SIZE: usize = 512;

const CANCEL_CHECK_EVERY: usize = 64;

/// Progress notifications from the enumeration worker to the UI.
/// Payload data is already inside the snapshot; events are cheap signals.
#[derive(Debug)]
pub enum EnumEvent {
    Started { gen_id: u64, path: PathBuf },
    /// `total` entries are now in the snapshot (cumulative).
    Batch { gen_id: u64, total: usize },
    Finished { gen_id: u64, total: usize },
    /// Scan aborted because a newer request superseded it.
    Superseded { gen_id: u64 },
    Error { gen_id: u64, message: String },
}

struct EnumRequest {
    snap: SnapshotHandle,
    gen_id: u64,
}

pub struct EnumEngine {
    req_tx: mpsc::Sender<EnumRequest>,
    cur_gen: Arc<AtomicU64>,
    event_rx: mpsc::Receiver<EnumEvent>,
    _worker: JoinHandle<()>,
}

impl Default for EnumEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl EnumEngine {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<EnumRequest>();
        // Bounded so a fast producer cannot pile up obsolete events.
        let (ev_tx, event_rx) = mpsc::sync_channel::<EnumEvent>(8192);
        let cur_gen = Arc::new(AtomicU64::new(0));
        let gen_in_worker = Arc::clone(&cur_gen);
        let worker = std::thread::Builder::new()
            .name("flux-enum".into())
            .spawn(move || worker_loop(req_rx, ev_tx, gen_in_worker))
            .expect("spawn enum worker");
        Self {
            req_tx,
            cur_gen,
            event_rx,
            _worker: worker,
        }
    }

    /// Begin streaming enumeration of `path` into `snap`. Returns the new
    /// generation; events for older generations must be ignored by callers.
    pub fn navigate(&self, snap: &SnapshotHandle, path: PathBuf) -> u64 {
        let gen_id = self.cur_gen.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut s) = snap.write() {
            s.begin_load(path, gen_id);
        }
        // If the worker died, navigation degrades to "Failed" visibly rather
        // than hanging: mark the snapshot so the UI shows an error state.
        if self.req_tx.send(EnumRequest { snap: snap.clone(), gen_id }).is_err() {
            if let Ok(mut s) = snap.write() {
                s.fail("enumeration worker unavailable".into());
            }
        }
        gen_id
    }

    /// Invalidate whatever scan is in flight without starting a new one.
    pub fn cancel_current(&self) {
        self.cur_gen.fetch_add(1, Ordering::SeqCst);
    }

    pub fn current_generation(&self) -> u64 {
        self.cur_gen.load(Ordering::SeqCst)
    }

    /// Drain pending events without blocking.
    pub fn drain_events(&self) -> impl Iterator<Item = EnumEvent> + '_ {
        self.event_rx.try_iter()
    }
}

fn worker_loop(
    req_rx: mpsc::Receiver<EnumRequest>,
    ev_tx: mpsc::SyncSender<EnumEvent>,
    cur_gen: Arc<AtomicU64>,
) {
    while let Ok(mut req) = req_rx.recv() {
        // Take the newest queued request; older ones are obsolete.
        while let Ok(newer) = req_rx.try_recv() {
            // Mark the superseded snapshot so a UI still looking at it knows.
            if let Ok(mut s) = req.snap.write() {
                s.cancel();
            }
            let _ = ev_tx.try_send(EnumEvent::Superseded { gen_id: req.gen_id });
            req = newer;
        }
        run_scan(&req, &ev_tx, &cur_gen);
    }
}

fn run_scan(
    req: &EnumRequest,
    ev_tx: &mpsc::SyncSender<EnumEvent>,
    cur_gen: &Arc<AtomicU64>,
) {
    let gen_id = req.gen_id;
    let is_current = |g: u64| cur_gen.load(Ordering::SeqCst) == g;
    if !is_current(gen_id) {
        return;
    }
    let path = req.snap.read().map(|s| s.path.clone()).unwrap_or_default();
    let _ = ev_tx.try_send(EnumEvent::Started { gen_id, path: path.clone() });

    let rd = match std::fs::read_dir(&path) {
        Ok(rd) => rd,
        Err(e) => {
            if is_current(gen_id) {
                if let Ok(mut s) = req.snap.write() {
                    s.fail(format!("cannot read directory: {e}"));
                }
                let _ = ev_tx.try_send(EnumEvent::Error {
                    gen_id,
                    message: e.to_string(),
                });
            }
            return;
        }
    };

    let mut batch: Vec<DirEntry> = Vec::with_capacity(BATCH_SIZE);
    let mut since_check = 0usize;
    for ent in rd {
        since_check += 1;
        if since_check >= CANCEL_CHECK_EVERY {
            since_check = 0;
            if !is_current(gen_id) {
                let _ = ev_tx.try_send(EnumEvent::Superseded { gen_id });
                return;
            }
        }
        let ent = match ent {
            Ok(e) => e,
            Err(_) => continue, // entry vanished mid-scan; skip
        };
        let name = ent.file_name();
        let kind = match ent.file_type() {
            Ok(ft) if ft.is_dir() => EntryKind::Dir,
            Ok(ft) if ft.is_symlink() => EntryKind::Symlink,
            Ok(ft) if ft.is_file() => EntryKind::File,
            _ => EntryKind::Other,
        };
        // One stat per entry: gives size/mtime/readonly without following
        // symlinks. Failures degrade to unknown fields, never abort the scan.
        let (size, mtime, readonly) = match ent.metadata() {
            Ok(md) => {
                let size = if kind == EntryKind::Dir || kind == EntryKind::Symlink {
                    crate::UNKNOWN_SIZE
                } else {
                    md.len()
                };
                (size, md.modified().ok(), md.permissions().readonly())
            }
            Err(_) => (crate::UNKNOWN_SIZE, None, false),
        };
        batch.push(DirEntry {
            hidden: is_hidden_name(&name),
            name,
            kind,
            size,
            mtime,
            readonly,
        });
        if batch.len() >= BATCH_SIZE {
            flush_batch(req, &mut batch, gen_id, ev_tx);
        }
    }
    if !batch.is_empty() {
        flush_batch(req, &mut batch, gen_id, ev_tx);
    }
    if !is_current(gen_id) {
        let _ = ev_tx.try_send(EnumEvent::Superseded { gen_id });
        return;
    }
    let total = req.snap.read().map(|s| s.entry_count()).unwrap_or(0);
    let _ = ev_tx.try_send(EnumEvent::Finished { gen_id, total });
}

fn flush_batch(
    req: &EnumRequest,
    batch: &mut Vec<DirEntry>,
    gen_id: u64,
    ev_tx: &mpsc::SyncSender<EnumEvent>,
) {
    let total = match req.snap.write() {
        Ok(mut s) if s.generation == gen_id => {
            s.push_batch(batch);
            s.entry_count()
        }
        // Snapshot moved on (e.g. tab reused for a different dir): drop data.
        _ => {
            batch.clear();
            0
        }
    };
    if total > 0 {
        let _ = ev_tx.try_send(EnumEvent::Batch { gen_id, total });
    }
}
