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
    Started {
        gen_id: u64,
        path: PathBuf,
    },
    /// `total` entries are now in the snapshot (cumulative).
    Batch {
        gen_id: u64,
        total: usize,
    },
    Finished {
        gen_id: u64,
        total: usize,
    },
    /// Scan aborted because a newer request superseded it.
    Superseded {
        gen_id: u64,
    },
    Error {
        gen_id: u64,
        message: String,
    },
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
        if self
            .req_tx
            .send(EnumRequest {
                snap: snap.clone(),
                gen_id,
            })
            .is_err()
            && let Ok(mut s) = snap.write()
        {
            s.fail("enumeration worker unavailable".into());
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

fn run_scan(req: &EnumRequest, ev_tx: &mpsc::SyncSender<EnumEvent>, cur_gen: &Arc<AtomicU64>) {
    let gen_id = req.gen_id;
    let is_current = |g: u64| cur_gen.load(Ordering::SeqCst) == g;
    if !is_current(gen_id) {
        return;
    }
    let path = req.snap.read().map(|s| s.path.clone()).unwrap_or_default();
    let _ = ev_tx.try_send(EnumEvent::Started {
        gen_id,
        path: path.clone(),
    });

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
        // Lazy metadata: `file_type()` is free on virtually every filesystem
        // (it rides along in the dirent). size/mtime/readonly are deferred
        // to the MetaEngine so names reach the UI as fast as the filesystem
        // can list them; `Other` (undetermined type) stats once eagerly
        // because a reliable kind is needed for sorting and icons.
        let (kind, eager_meta) = match ent.file_type() {
            Ok(ft) if ft.is_dir() => (EntryKind::Dir, None),
            Ok(ft) if ft.is_symlink() => (EntryKind::Symlink, None),
            Ok(ft) if ft.is_file() => (EntryKind::File, None),
            _ => (EntryKind::Other, ent.metadata().ok()),
        };
        let (size, mtime, readonly) = match eager_meta {
            Some(md) => (
                crate::UNKNOWN_SIZE,
                md.modified().ok(),
                md.permissions().readonly(),
            ),
            None => (crate::UNKNOWN_SIZE, None, false),
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

// ---------------------------------------------------------------------------
// Deferred metadata fill
// ---------------------------------------------------------------------------

/// Entries statted per apply/signal step. Same reasoning as `BATCH_SIZE`:
/// large enough to amortize locking, small enough to stay responsive.
const META_BATCH: usize = 1024;

/// Progress notifications from the metadata worker.
#[derive(Debug)]
pub enum MetaEvent {
    /// `done` of `total` entries now have real size/mtime/readonly.
    Batch {
        gen_id: u64,
        done: usize,
        total: usize,
    },
    /// All entries processed (or no stat needed). Triggers the final sort.
    Finished { gen_id: u64 },
    /// Job aborted — a newer job or navigation superseded it.
    Superseded { gen_id: u64 },
}

struct MetaRequest {
    snap: SnapshotHandle,
    gen_id: u64,
}

/// Background `lstat` filler for entries enumerated with unknown metadata.
/// One bounded worker; latest-request-wins like [`EnumEngine`]. The UI can
/// steer work with [`MetaEngine::set_priority`] so the visible range is
/// statted before the rest.
pub struct MetaEngine {
    req_tx: mpsc::Sender<MetaRequest>,
    cur_gen: Arc<AtomicU64>,
    /// Packed `(start << 32) | end` index range the UI wants first; 0 means
    /// none. Written by the UI every scroll, read by the worker per chunk.
    priority: Arc<AtomicU64>,
    event_rx: mpsc::Receiver<MetaEvent>,
    _worker: JoinHandle<()>,
}

impl Default for MetaEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl MetaEngine {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<MetaRequest>();
        let (ev_tx, event_rx) = mpsc::sync_channel::<MetaEvent>(8192);
        let cur_gen = Arc::new(AtomicU64::new(0));
        let priority = Arc::new(AtomicU64::new(0));
        let gen_w = Arc::clone(&cur_gen);
        let pri_w = Arc::clone(&priority);
        let worker = std::thread::Builder::new()
            .name("flux-meta".into())
            .spawn(move || meta_worker_loop(req_rx, ev_tx, gen_w, pri_w))
            .expect("spawn meta worker");
        Self {
            req_tx,
            cur_gen,
            priority,
            event_rx,
            _worker: worker,
        }
    }

    /// Fill metadata for every entry of `snap` (generation `gen_id`).
    /// Called by the UI when enumeration finishes. Resets the priority
    /// range — it belongs to the previous directory's viewport until the
    /// new view reports one.
    pub fn fill(&self, snap: &SnapshotHandle, gen_id: u64) {
        let job = self.cur_gen.fetch_add(1, Ordering::SeqCst) + 1;
        self.priority.store(0, Ordering::Relaxed);
        let _ = self.req_tx.send(MetaRequest {
            snap: snap.clone(),
            gen_id,
        });
        debug_assert!(job > 0);
    }

    /// Stat the snapshot index range `start..end` before the rest. Called on
    /// every scroll; costs one atomic store.
    pub fn set_priority(&self, start: u32, end: u32) {
        self.priority
            .store((u64::from(start) << 32) | u64::from(end), Ordering::Relaxed);
    }

    pub fn cancel_current(&self) {
        self.cur_gen.fetch_add(1, Ordering::SeqCst);
    }

    pub fn drain_events(&self) -> impl Iterator<Item = MetaEvent> + '_ {
        self.event_rx.try_iter()
    }
}

fn meta_worker_loop(
    req_rx: mpsc::Receiver<MetaRequest>,
    ev_tx: mpsc::SyncSender<MetaEvent>,
    cur_gen: Arc<AtomicU64>,
    priority: Arc<AtomicU64>,
) {
    // Latest request wins; superseded jobs are discarded, not reported —
    // the stale snapshot's generation check would ignore their data anyway.
    while let Ok(mut req) = req_rx.recv() {
        while let Ok(newer) = req_rx.try_recv() {
            req = newer;
        }
        run_fill(&req, &ev_tx, &cur_gen, &priority);
    }
}

fn run_fill(
    req: &MetaRequest,
    ev_tx: &mpsc::SyncSender<MetaEvent>,
    cur_gen: &Arc<AtomicU64>,
    priority: &Arc<AtomicU64>,
) {
    let gen_id = req.gen_id;
    let job_gen = cur_gen.load(Ordering::SeqCst);
    let is_current = || {
        cur_gen.load(Ordering::SeqCst) == job_gen
            && req
                .snap
                .read()
                .map(|s| s.generation == gen_id)
                .unwrap_or(false)
    };
    if !is_current() {
        let _ = ev_tx.try_send(MetaEvent::Superseded { gen_id });
        return;
    }
    let (path, count) = match req.snap.read() {
        Ok(s) => (s.path.clone(), s.entry_count()),
        Err(_) => return,
    };
    if count == 0 {
        let _ = ev_tx.try_send(MetaEvent::Finished { gen_id });
        return;
    }
    let mut done_marks = vec![false; count];
    let mut done = 0usize;
    let mut cursor = 0usize;
    let mut batch: Vec<(u32, u64, Option<std::time::SystemTime>, bool)> =
        Vec::with_capacity(META_BATCH);

    while done < count {
        if !is_current() {
            let _ = ev_tx.try_send(MetaEvent::Superseded { gen_id });
            return;
        }
        batch.clear();
        // Pick this chunk's indices: priority range first, then sequential.
        let mut idxs: Vec<u32> = Vec::with_capacity(META_BATCH);
        let pri = priority.load(Ordering::Relaxed);
        let (ps, pe) = ((pri >> 32) as usize, (pri & 0xFFFF_FFFF) as usize);
        for (i, mark) in done_marks.iter().enumerate().take(pe.min(count)).skip(ps) {
            if !mark && idxs.len() < META_BATCH {
                idxs.push(i as u32);
            }
        }
        while idxs.len() < META_BATCH && cursor < count {
            if !done_marks[cursor] {
                idxs.push(cursor as u32);
            }
            cursor += 1;
        }
        if idxs.is_empty() {
            break;
        }
        // Grab (name, kind) under one short read lock; stat outside all locks.
        let infos: Vec<(std::ffi::OsString, EntryKind)> = match req.snap.read() {
            Ok(s) if s.generation == gen_id => idxs
                .iter()
                .filter_map(|&i| {
                    s.entries()
                        .get(i as usize)
                        .map(|e| (e.name.clone(), e.kind))
                })
                .collect(),
            _ => return,
        };
        for (slot, (name, kind)) in infos.iter().enumerate() {
            let i = idxs[slot];
            if let Ok(md) = std::fs::symlink_metadata(path.join(name)) {
                batch.push((
                    i,
                    if *kind == EntryKind::File {
                        md.len()
                    } else {
                        crate::UNKNOWN_SIZE
                    },
                    md.modified().ok(),
                    md.permissions().readonly(),
                ));
            }
            done_marks[i as usize] = true;
            done += 1;
        }
        if !batch.is_empty() {
            match req.snap.write() {
                Ok(mut s) if s.generation == gen_id => s.apply_meta(&batch),
                _ => {
                    let _ = ev_tx.try_send(MetaEvent::Superseded { gen_id });
                    return;
                }
            }
        }
        let _ = ev_tx.try_send(MetaEvent::Batch {
            gen_id,
            done,
            total: count,
        });
    }
    let _ = ev_tx.try_send(MetaEvent::Finished { gen_id });
}
