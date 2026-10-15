//! The operation engine: one bounded worker + a shared op registry.
//!
//! The UI submits [`OpJob`]s, polls [`OpEngine::ops`] for progress, and
//! answers [`OpEvent::Conflict`] via [`OpEngine::answer_conflict`].
//! Serial execution is deliberate: for spinning disks and most SSDs,
//! concurrent bulk copies contend rather than scale. Parallelism can be
//! revisited with measurements.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rustc_hash::FxHashSet;

use crate::conflict::{ConflictAction, ConflictDecision, ConflictPolicy, ConflictQuery};
use crate::copy::{
    CopyCtx, cancelled, cleanup_created, copy_tree, discover, move_item, remove_tree,
};
use crate::progress::{OpProgress, OpState};
use crate::rename::{RenameAction, execute_renames};

pub type OpId = u64;

#[derive(Debug)]
pub enum OpKind {
    Copy { sources: Vec<PathBuf>, dest_dir: PathBuf },
    Move { sources: Vec<PathBuf>, dest_dir: PathBuf },
    Trash { items: Vec<PathBuf> },
    /// Permanent delete — never a fallback when trash is unavailable.
    Delete { items: Vec<PathBuf> },
    /// Copy each item next to itself as "name copy.ext" / "name copy N".
    Duplicate { items: Vec<PathBuf> },
    /// Compress items into a .zip at `dest`.
    Compress { items: Vec<PathBuf>, dest: PathBuf },
    Rename { from: PathBuf, to: PathBuf },
    BatchRename { actions: Vec<RenameAction> },
    Mkdir { path: PathBuf },
    CreateFile { path: PathBuf },
}

pub struct OpJob {
    pub id: OpId,
    pub kind: OpKind,
    pub policy: ConflictPolicy,
}

#[derive(Debug)]
pub enum OpEvent {
    /// Some op's progress changed; repaint.
    Dirty,
    /// Worker is parked awaiting an answer via answer_conflict().
    Conflict(ConflictQuery),
}

pub struct OpHandle {
    pub id: OpId,
    pub label: String,
    pub progress: Mutex<OpProgress>,
    cancel: AtomicBool,
    policy: Mutex<ConflictPolicy>,
    conflict_reply: Mutex<Option<mpsc::Sender<ConflictDecision>>>,
}

impl OpHandle {
    pub fn progress_snapshot(&self) -> OpProgress {
        self.progress.lock().map(|p| OpProgress {
            state: p.state.clone(),
            total_items: p.total_items,
            done_items: p.done_items,
            total_bytes: p.total_bytes,
            done_bytes: p.done_bytes,
            current_item: p.current_item.clone(),
            errors: p.errors.clone(),
            error_count: p.error_count,
            started: p.started,
            finished: p.finished,
            tracks_bytes: p.tracks_bytes,
        }).unwrap_or_default()
    }
}

pub struct OpEngine {
    job_tx: mpsc::Sender<OpJob>,
    pub ops: Arc<RwLock<Vec<Arc<OpHandle>>>>,
    event_rx: mpsc::Receiver<OpEvent>,
    next_id: AtomicU64,
    _worker: JoinHandle<()>,
}

impl Default for OpEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl OpEngine {
    pub fn new() -> Self {
        let (job_tx, job_rx) = mpsc::channel::<OpJob>();
        let (ev_tx, ev_rx) = mpsc::channel::<OpEvent>();
        let ops = Arc::new(RwLock::new(Vec::new()));
        let worker_ops = Arc::clone(&ops);
        let worker = std::thread::Builder::new()
            .name("flux-ops".into())
            .spawn(move || ops_worker(job_rx, ev_tx, worker_ops))
            .expect("spawn ops worker");
        Self {
            job_tx,
            ops,
            event_rx: ev_rx,
            next_id: AtomicU64::new(1),
            _worker: worker,
        }
    }

    /// Queue an operation. Returns its id.
    pub fn submit(&self, kind: OpKind, policy: ConflictPolicy) -> OpId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let label = describe(&kind);
        let handle = Arc::new(OpHandle {
            id,
            label,
            progress: Mutex::new(OpProgress::default()),
            cancel: AtomicBool::new(false),
            policy: Mutex::new(policy),
            conflict_reply: Mutex::new(None),
        });
        if let Ok(mut ops) = self.ops.write() {
            ops.push(handle);
        }
        let _ = self.job_tx.send(OpJob { id, kind, policy });
        id
    }

    /// Cancel a running or queued op (also answers any pending conflict).
    pub fn cancel(&self, id: OpId) {
        if let Ok(ops) = self.ops.read()
            && let Some(op) = ops.iter().find(|o| o.id == id) {
                op.cancel.store(true, Ordering::Relaxed);
                if let Some(tx) = op.conflict_reply.lock().unwrap().take() {
                    let _ = tx.send(ConflictDecision {
                        action: ConflictAction::CancelOp,
                        apply_all: false,
                    });
                }
            }
    }

    pub fn answer_conflict(&self, query_op: OpId, decision: ConflictDecision) {
        if let Ok(ops) = self.ops.read()
            && let Some(op) = ops.iter().find(|o| o.id == query_op)
                && let Some(tx) = op.conflict_reply.lock().unwrap().take() {
                    let _ = tx.send(decision);
                }
    }

    /// Active ops (not finished), for the status bar / progress panel.
    pub fn active_ops(&self) -> Vec<Arc<OpHandle>> {
        self.ops
            .read()
            .map(|ops| {
                ops.iter()
                    .filter(|o| {
                        o.progress
                            .lock()
                            .map(|p| p.state.is_active())
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// All ops including finished, newest last.
    pub fn all_ops(&self) -> Vec<Arc<OpHandle>> {
        self.ops.read().map(|o| o.clone()).unwrap_or_default()
    }

    /// GC finished ops older than `keep` seconds. Call occasionally.
    pub fn gc_finished(&self, keep: Duration) {
        if let Ok(mut ops) = self.ops.write() {
            ops.retain(|o| {
                let p = o.progress.lock().unwrap();
                match (p.state.is_finished(), p.finished) {
                    (true, Some(t)) => t.elapsed() < keep,
                    (true, None) => false,
                    _ => true,
                }
            });
        }
    }

    pub fn drain_events(&self) -> impl Iterator<Item = OpEvent> + '_ {
        self.event_rx.try_iter()
    }
}

fn describe(kind: &OpKind) -> String {
    match kind {
        OpKind::Copy { sources, dest_dir } => {
            format!("Copy {} item(s) to {}", sources.len(), dest_dir.display())
        }
        OpKind::Move { sources, dest_dir } => {
            format!("Move {} item(s) to {}", sources.len(), dest_dir.display())
        }
        OpKind::Trash { items } => format!("Trash {} item(s)", items.len()),
        OpKind::Delete { items } => format!("Delete {} item(s)", items.len()),
        OpKind::Duplicate { items } => format!("Duplicate {} item(s)", items.len()),
        OpKind::Compress { items, dest } => format!(
            "Compress {} item(s) → {}",
            items.len(),
            dest.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        ),
        OpKind::Rename { from, .. } => {
            format!("Rename {}", from.display())
        }
        OpKind::BatchRename { actions } => format!("Rename {} item(s)", actions.len()),
        OpKind::Mkdir { path } => format!("New folder {}", path.display()),
        OpKind::CreateFile { path } => format!("New file {}", path.display()),
    }
}

/// Conflict resolution inside the worker: policy first, else block on UI.
fn make_resolver<'a>(
    op: &'a Arc<OpHandle>,
    ev_tx: &'a mpsc::Sender<OpEvent>,
) -> impl Fn(&std::path::Path, &std::path::Path) -> ConflictAction + 'a {
    move |src, dst| {
        let policy = *op.policy.lock().unwrap();
        match policy {
            ConflictPolicy::OverwriteAll => return ConflictAction::Overwrite,
            ConflictPolicy::SkipAll => return ConflictAction::Skip,
            ConflictPolicy::KeepBothAll => return ConflictAction::KeepBoth,
            ConflictPolicy::Ask => {}
        }
        // Park the worker on the UI's answer.
        let (tx, rx) = mpsc::channel::<ConflictDecision>();
        *op.conflict_reply.lock().unwrap() = Some(tx);
        if let Ok(mut p) = op.progress.lock() {
            p.state = OpState::WaitingConflict;
        }
        let _ = ev_tx.send(OpEvent::Conflict(ConflictQuery {
            op_id: op.id,
            source: src.to_path_buf(),
            dest: dst.to_path_buf(),
            reason: "destination already exists",
        }));
        let decision = rx.recv().unwrap_or(ConflictDecision {
            action: ConflictAction::CancelOp,
            apply_all: false,
        });
        if decision.apply_all {
            let mapped = match decision.action {
                ConflictAction::Overwrite => Some(ConflictPolicy::OverwriteAll),
                ConflictAction::Skip => Some(ConflictPolicy::SkipAll),
                ConflictAction::KeepBoth => Some(ConflictPolicy::KeepBothAll),
                ConflictAction::CancelOp => None,
            };
            if let Some(m) = mapped {
                *op.policy.lock().unwrap() = m;
            }
        }
        if let Ok(mut p) = op.progress.lock() {
            p.state = OpState::Running;
        }
        decision.action
    }
}

fn set_state(op: &OpHandle, s: OpState) {
    if let Ok(mut p) = op.progress.lock() {
        p.state = s;
    }
}

fn ops_worker(
    job_rx: mpsc::Receiver<OpJob>,
    ev_tx: mpsc::Sender<OpEvent>,
    ops: Arc<RwLock<Vec<Arc<OpHandle>>>>,
) {
    // Throttle Dirty events so a fast copy doesn't spam repaints.
    let mut last_dirty = Instant::now() - Duration::from_millis(100);
    let mut dirty = |ev_tx: &mpsc::Sender<OpEvent>| {
        if last_dirty.elapsed() > Duration::from_millis(50) {
            last_dirty = Instant::now();
            let _ = ev_tx.send(OpEvent::Dirty);
        }
    };
    while let Ok(job) = job_rx.recv() {
        let op = match ops.read() {
            Ok(ops) => ops.iter().find(|o| o.id == job.id).cloned(),
            Err(_) => None,
        };
        let Some(op) = op else { continue };
        *op.policy.lock().unwrap() = job.policy;
        {
            let mut p = op.progress.lock().unwrap();
            p.state = OpState::Running;
            p.started = Some(Instant::now());
            p.tracks_bytes = matches!(
                job.kind,
                OpKind::Copy { .. }
                    | OpKind::Move { .. }
                    | OpKind::Duplicate { .. }
                    | OpKind::Compress { .. }
            );
        }
        run_job(&job, &op, &ev_tx, &mut dirty);
        let cancelled_flag = op.cancel.load(Ordering::Relaxed);
        {
            let mut p = op.progress.lock().unwrap();
            p.finished = Some(Instant::now());
            if cancelled_flag {
                p.state = OpState::Cancelled;
            } else if p.error_count > 0 {
                p.state = OpState::DoneWithErrors;
            } else {
                p.state = OpState::Done;
            }
        }
        let _ = ev_tx.send(OpEvent::Dirty);
    }
}

fn run_job(
    job: &OpJob,
    op: &Arc<OpHandle>,
    ev_tx: &mpsc::Sender<OpEvent>,
    dirty: &mut impl FnMut(&mpsc::Sender<OpEvent>),
) {
    match &job.kind {
        OpKind::Copy { sources, dest_dir } | OpKind::Move { sources, dest_dir } => {
            let is_move = matches!(job.kind, OpKind::Move { .. });
            let resolver = make_resolver(op, ev_tx);
            let ctx = CopyCtx {
                progress: &op.progress,
                cancel: &op.cancel,
                resolve: &resolver,
                tag: job.id,
                created_dirs: Mutex::new(Vec::new()),
                failed: Mutex::new(FxHashSet::default()),
            };
            discover(&ctx, sources);
            if cancelled(&ctx) {
                return;
            }
            set_state(op, OpState::Running);
            for src in sources {
                if cancelled(&ctx) {
                    break;
                }
                let Some(name) = src.file_name() else {
                    continue;
                };
                let dst = dest_dir.join(name);
                if is_move {
                    // Conflict policy is applied inside move_item BEFORE any
                    // rename — nothing is silently overwritten. Same-volume
                    // moves stay atomic; cross-volume moves copy then remove
                    // only the parts of the source that actually transferred.
                    match move_item(src, &dst, &ctx) {
                        Ok(crate::copy::ItemOutcome::Done)
                        | Ok(crate::copy::ItemOutcome::Partial) => {}
                        Ok(crate::copy::ItemOutcome::Skipped)
                        | Ok(crate::copy::ItemOutcome::Cancelled) => {}
                        Err(e) => {
                            crate::copy::note_error(&ctx, format!("{}: {e}", src.display()))
                        }
                    }
                    dirty(ev_tx);
                    continue;
                }
                match copy_tree(src, &dst, &ctx) {
                    Ok(crate::copy::ItemOutcome::Done)
                    | Ok(crate::copy::ItemOutcome::Partial)
                    | Ok(crate::copy::ItemOutcome::Skipped)
                    | Ok(crate::copy::ItemOutcome::Cancelled) => {}
                    Err(e) => crate::copy::note_error(&ctx, format!("{}: {e}", src.display())),
                }
                dirty(ev_tx);
            }
            if cancelled(&ctx) {
                cleanup_created(&ctx);
            }
        }
        OpKind::Duplicate { items } => {
            let resolver = make_resolver(op, ev_tx);
            let ctx = CopyCtx {
                progress: &op.progress,
                cancel: &op.cancel,
                resolve: &resolver,
                tag: job.id,
                created_dirs: Mutex::new(Vec::new()),
                failed: Mutex::new(FxHashSet::default()),
            };
            discover(&ctx, items);
            if cancelled(&ctx) {
                return;
            }
            set_state(op, OpState::Running);
            for src in items {
                if cancelled(&ctx) {
                    break;
                }
                let dst = crate::copy::duplicate_dst(src);
                match copy_tree(src, &dst, &ctx) {
                    Ok(crate::copy::ItemOutcome::Done)
                    | Ok(crate::copy::ItemOutcome::Partial)
                    | Ok(crate::copy::ItemOutcome::Skipped)
                    | Ok(crate::copy::ItemOutcome::Cancelled) => {}
                    Err(e) => crate::copy::note_error(&ctx, format!("{}: {e}", src.display())),
                }
                dirty(ev_tx);
            }
        }
        OpKind::Compress { items, dest } => {
            let resolver = make_resolver(op, ev_tx);
            let ctx = CopyCtx {
                progress: &op.progress,
                cancel: &op.cancel,
                resolve: &resolver,
                tag: job.id,
                created_dirs: Mutex::new(Vec::new()),
                failed: Mutex::new(FxHashSet::default()),
            };
            discover(&ctx, items);
            if cancelled(&ctx) {
                return;
            }
            let mut dest = dest.clone();
            if dest.exists() {
                match resolver(&items[0], &dest) {
                    ConflictAction::Overwrite => {
                        let _ = std::fs::remove_file(&dest);
                    }
                    ConflictAction::KeepBoth => {
                        dest = crate::copy::duplicate_dst(&dest);
                    }
                    ConflictAction::Skip | ConflictAction::CancelOp => return,
                }
            }
            set_state(op, OpState::Running);
            if let Err(e) = crate::archive::compress_items(items, &dest, &ctx) {
                set_state(op, OpState::Failed(format!("compress: {e}")));
            }
            if cancelled(&ctx) {
                let _ = std::fs::remove_file(&dest);
            }
        }
        OpKind::Trash { items } => {
            if let Ok(mut p) = op.progress.lock() {
                p.total_items = items.len() as u64;
                p.tracks_bytes = false;
            }
            for it in items {
                if op.cancel.load(Ordering::Relaxed) {
                    return;
                }
                let (ok, errs) = flux_platform::trash_items(std::slice::from_ref(it));
                {
                    let mut p = op.progress.lock().unwrap();
                    p.done_items += ok as u64;
                    p.current_item = it.clone();
                    for (_, te) in errs {
                        p.record_error(format!("trash {}: {te}", it.display()));
                    }
                }
                dirty(ev_tx);
            }
        }
        OpKind::Delete { items } => {
            let resolver = make_resolver(op, ev_tx);
            let ctx = CopyCtx {
                progress: &op.progress,
                cancel: &op.cancel,
                resolve: &resolver,
                tag: job.id,
                created_dirs: Mutex::new(Vec::new()),
                failed: Mutex::new(FxHashSet::default()),
            };
            discover(&ctx, items);
            if cancelled(&ctx) {
                return;
            }
            set_state(op, OpState::Running);
            for it in items {
                if cancelled(&ctx) {
                    return;
                }
                if let Err(e) = remove_tree(it, &ctx) {
                    crate::copy::note_error(&ctx, format!("delete {}: {e}", it.display()));
                }
                dirty(ev_tx);
            }
        }
        OpKind::Rename { from, to } => {
            if to.exists() || to.symlink_metadata().is_ok() {
                let resolver = make_resolver(op, ev_tx);
                match resolver(from, to) {
                    ConflictAction::Overwrite => {}
                    ConflictAction::Skip | ConflictAction::CancelOp => {
                        set_state(op, OpState::Cancelled);
                        return;
                    }
                    ConflictAction::KeepBoth => {
                        set_state(op, OpState::Failed("rename keep-both unsupported".into()));
                        return;
                    }
                }
            }
            // If we got here with `to` still existing the user chose
            // overwrite — swap-protect it so a failed rename can't lose
            // the destination (and so Windows can replace at all).
            match if to.symlink_metadata().is_ok() {
                crate::copy::rename_replace(from, to)
            } else {
                std::fs::rename(from, to)
            } {
                Ok(()) => {
                    let mut p = op.progress.lock().unwrap();
                    p.total_items = 1;
                    p.done_items = 1;
                }
                Err(e) => set_state(op, OpState::Failed(format!("rename: {e}"))),
            }
        }
        OpKind::BatchRename { actions } => {
            let resolver = make_resolver(op, ev_tx);
            let ctx = CopyCtx {
                progress: &op.progress,
                cancel: &op.cancel,
                resolve: &resolver,
                tag: job.id,
                created_dirs: Mutex::new(Vec::new()),
                failed: Mutex::new(FxHashSet::default()),
            };
            execute_renames(actions, &ctx);
        }
        OpKind::Mkdir { path } => match std::fs::create_dir(path) {
            Ok(()) => {
                let mut p = op.progress.lock().unwrap();
                p.total_items = 1;
                p.done_items = 1;
            }
            Err(e) => set_state(op, OpState::Failed(format!("mkdir: {e}"))),
        },
        OpKind::CreateFile { path } => match std::fs::File::create_new(path) {
            Ok(_) => {
                let mut p = op.progress.lock().unwrap();
                p.total_items = 1;
                p.done_items = 1;
            }
            Err(e) => set_state(op, OpState::Failed(format!("create file: {e}"))),
        },
    }
}
