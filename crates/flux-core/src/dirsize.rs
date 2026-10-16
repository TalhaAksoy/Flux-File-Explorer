//! Opt-in recursive folder sizes — never implicit (a recursive walk is
//! exactly what "fast browsing" must not pay for). The user asks per
//! directory via the context menu; a single background worker walks the
//! tree, respecting symlinks (counted, never followed), and reports the
//! byte total which lands in the size cell like deferred metadata.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use crate::EntryIdx;
use crate::snapshot::SnapshotHandle;

/// One folder-size request: write the total into `snap`'s entry `idx`,
/// valid only while `snap.generation == gen_id`.
struct SizeJob {
    snap: SnapshotHandle,
    gen_id: u64,
    idx: EntryIdx,
    path: PathBuf,
}

pub enum SizeEvent {
    /// Total computed and applied to the entry.
    Done { gen_id: u64, idx: EntryIdx },
    /// Walk failed (permissions, vanished dir) — entry keeps UNKNOWN_SIZE.
    Failed { gen_id: u64, idx: EntryIdx },
}

/// Sequential worker: each request is a full recursive walk, so requests
/// queue FIFO. Stale-generation jobs are skipped before doing any IO.
pub struct SizeEngine {
    req_tx: mpsc::Sender<SizeJob>,
    cur_gen: Arc<AtomicU64>,
    event_rx: mpsc::Receiver<SizeEvent>,
    _worker: JoinHandle<()>,
}

impl Default for SizeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SizeEngine {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<SizeJob>();
        let (ev_tx, event_rx) = mpsc::sync_channel::<SizeEvent>(512);
        let cur_gen = Arc::new(AtomicU64::new(0));
        let gen_w = Arc::clone(&cur_gen);
        let worker = std::thread::Builder::new()
            .name("flux-dirsize".into())
            .spawn(move || worker_loop(req_rx, ev_tx, gen_w))
            .expect("spawn dirsize worker");
        Self {
            req_tx,
            cur_gen,
            event_rx,
            _worker: worker,
        }
    }

    /// Queue a size computation for snapshot entry `idx` (`path` on disk).
    pub fn request(&self, snap: &SnapshotHandle, gen_id: u64, idx: EntryIdx, path: PathBuf) {
        let _ = self.req_tx.send(SizeJob {
            snap: snap.clone(),
            gen_id,
            idx,
            path,
        });
    }

    /// Invalidate pending work — every queued job for older generations
    /// is skipped, and the walk in progress aborts at the next check.
    pub fn cancel_current(&self) {
        self.cur_gen.fetch_add(1, Ordering::SeqCst);
    }

    pub fn drain_events(&self) -> impl Iterator<Item = SizeEvent> + '_ {
        self.event_rx.try_iter()
    }
}

fn worker_loop(
    req_rx: mpsc::Receiver<SizeJob>,
    ev_tx: mpsc::SyncSender<SizeEvent>,
    cur_gen: Arc<AtomicU64>,
) {
    while let Ok(job) = req_rx.recv() {
        // Skip jobs enqueued before the last navigation — their target
        // entry doesn't exist anymore (indices are generation-scoped).
        let job_gen = cur_gen.load(Ordering::SeqCst);
        let live = |snap: &SnapshotHandle, gen_id: u64| {
            cur_gen.load(Ordering::SeqCst) == job_gen
                && snap.read().map(|s| s.generation == gen_id).unwrap_or(false)
        };
        if !live(&job.snap, job.gen_id) {
            continue;
        }
        match walk_size(&job.path, || live(&job.snap, job.gen_id)) {
            Some(bytes) => {
                if let Ok(mut s) = job.snap.write()
                    && s.generation == job.gen_id
                {
                    s.apply_size(job.idx, bytes);
                }
                let _ = ev_tx.try_send(SizeEvent::Done {
                    gen_id: job.gen_id,
                    idx: job.idx,
                });
            }
            None => {
                let _ = ev_tx.try_send(SizeEvent::Failed {
                    gen_id: job.gen_id,
                    idx: job.idx,
                });
            }
        }
    }
}

/// Recursive byte total for `root` — files only, counted via lstat;
/// symlinks contribute their own link size but are never descended.
/// `keep` is polled periodically; returning false aborts (`None`).
fn walk_size(root: &std::path::Path, keep: impl Fn() -> bool) -> Option<u64> {
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    let mut since_check = 0u32;
    while let Some(p) = stack.pop() {
        since_check += 1;
        if since_check >= 64 {
            since_check = 0;
            if !keep() {
                return None;
            }
        }
        let md = std::fs::symlink_metadata(&p).ok()?;
        if md.is_dir() && !md.file_type().is_symlink() {
            let rd = std::fs::read_dir(&p).ok()?;
            for e in rd.flatten() {
                stack.push(e.path());
            }
        } else if !md.file_type().is_symlink() {
            // Links contribute 0 — "size" means the sum of real file
            // contents; the link's own lstat length is noise.
            total = total.saturating_add(md.len());
        }
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn walk_size_sums_files_skips_link_targets() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::File::create(root.join("a.bin"))
            .unwrap()
            .write_all(&[0u8; 100])
            .unwrap();
        std::fs::File::create(root.join("sub").join("b.bin"))
            .unwrap()
            .write_all(&[0u8; 50])
            .unwrap();
        #[cfg(unix)]
        {
            // A link to a huge external file must not inflate the total.
            let big = tempfile::NamedTempFile::new().unwrap();
            big.as_file().write_all(&vec![0u8; 10_000]).unwrap();
            std::os::unix::fs::symlink(big.path(), root.join("link")).unwrap();
        }
        let total = walk_size(root, || true).unwrap();
        assert_eq!(total, 150, "total={total}");
    }
}
