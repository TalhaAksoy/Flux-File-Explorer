//! Data-safety regression tests for move/copy primitives.
//!
//! These pin the guarantees the audit flagged as data-loss paths:
//! - conflict policy is applied BEFORE any rename (no silent overwrite)
//! - partial cross-volume moves keep every untransferred source item
//! - a dir can never be copied/moved into itself or a descendant
//! - staging files are claimed with create_new, never truncated
//! - progress bytes are not double-counted

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use flux_operations::conflict::{ConflictAction, ConflictDecision, ConflictPolicy};
use flux_operations::copy::{
    CopyCtx, ItemOutcome, copy_file, copy_tree, move_item, set_force_copy_path,
};
use flux_operations::progress::OpProgress;
use flux_operations::rename::finish_staged;
use flux_operations::{OpEngine, OpEvent, OpKind};

fn tmp() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().to_path_buf();
    (d, p)
}

fn write_file(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::File::create(&p).unwrap().write_all(contents).unwrap();
    p
}

fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap()
}

fn ctx<'a>(
    progress: &'a Mutex<OpProgress>,
    cancel: &'a AtomicBool,
    resolve: &'a flux_operations::copy::ConflictResolver<'a>,
) -> CopyCtx<'a> {
    CopyCtx {
        progress,
        cancel,
        resolve,
        tag: 1,
        created_dirs: Mutex::new(Vec::new()),
        failed: Mutex::new(Default::default()),
    }
}

macro_rules! fresh {
    () => {{
        let progress = Mutex::new(OpProgress::default());
        let cancel = AtomicBool::new(false);
        (progress, cancel)
    }};
}

fn wait_op(engine: &OpEngine, id: u64, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let _ = engine.drain_events().count();
        if let Ok(ops) = engine.ops.read()
            && let Some(o) = ops.iter().find(|o| o.id == id)
            && o.progress_snapshot().state.is_finished()
        {
            return;
        }
        assert!(Instant::now() <= deadline, "op {id} did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ---------- move conflict policy ----------

#[test]
fn move_over_existing_respects_overwrite() {
    let (_t, dir) = tmp();
    let from = dir.join("from");
    let to = dir.join("to");
    std::fs::create_dir_all(&from).unwrap();
    std::fs::create_dir_all(&to).unwrap();
    let src = write_file(&from, "same.txt", b"NEW");
    let dst = write_file(&to, "same.txt", b"OLD");

    let (p, c) = fresh!();
    let asked = Mutex::new(0u32);
    let resolve = |_: &Path, _: &Path| {
        *asked.lock().unwrap() += 1;
        ConflictAction::Overwrite
    };
    let cx = ctx(&p, &c, &resolve);
    let r = move_item(&src, &dst, &cx).unwrap();
    assert_eq!(r, ItemOutcome::Done);
    assert_eq!(*asked.lock().unwrap(), 1, "conflict policy never consulted");
    assert_eq!(read(&dst), b"NEW");
    assert!(!src.exists());
}

#[test]
fn move_skip_preserves_both_files() {
    let (_t, dir) = tmp();
    let src = write_file(&dir, "a.txt", b"NEW");
    let dst = dir.join("dest");
    std::fs::create_dir(&dst).unwrap();
    let target = write_file(&dst, "a.txt", b"OLD");

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::Skip;
    let cx = ctx(&p, &c, &resolve);
    let r = move_item(&src, &target, &cx).unwrap();
    assert_eq!(r, ItemOutcome::Skipped);
    assert_eq!(read(&target), b"OLD", "destination must not be touched");
    assert_eq!(read(&src), b"NEW", "source must survive a skip");
}

#[test]
fn move_keep_both_preserves_original() {
    let (_t, dir) = tmp();
    let src = write_file(&dir, "a.txt", b"NEW");
    let dst = dir.join("dest");
    std::fs::create_dir(&dst).unwrap();
    let target = write_file(&dst, "a.txt", b"OLD");

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::KeepBoth;
    let cx = ctx(&p, &c, &resolve);
    let r = move_item(&src, &target, &cx).unwrap();
    assert_eq!(r, ItemOutcome::Done);
    assert_eq!(read(&target), b"OLD");
    assert_eq!(read(&dst.join("a (2).txt")), b"NEW");
    assert!(!src.exists());
}

#[test]
fn move_cancel_preserves_source() {
    let (_t, dir) = tmp();
    let src = write_file(&dir, "a.txt", b"NEW");
    let target = write_file(&dir, "b.txt", b"OLD");

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::CancelOp;
    let cx = ctx(&p, &c, &resolve);
    let r = move_item(&src, &target, &cx).unwrap();
    assert_eq!(r, ItemOutcome::Cancelled);
    assert_eq!(read(&src), b"NEW");
    assert_eq!(read(&target), b"OLD");
}

#[test]
fn move_onto_itself_is_rejected_and_survives() {
    let (_t, dir) = tmp();
    let src = write_file(&dir, "a.txt", b"data");

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::Overwrite;
    let cx = ctx(&p, &c, &resolve);
    let r = move_item(&src, &src, &cx);
    assert!(r.is_err(), "moving a file onto itself must not proceed");
    assert_eq!(read(&src), b"data");
}

#[test]
fn move_case_only_name_change() {
    let (_t, dir) = tmp();
    let src = write_file(&dir, "Case.txt", b"data");
    let dst = dir.join("CASE.txt");

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| panic!("case-only rename must not be a conflict");
    let cx = ctx(&p, &c, &resolve);
    let r = move_item(&src, &dst, &cx).unwrap();
    // Case-sensitive fs: plain rename. Case-insensitive fs: same_file →
    // rename (name fix). Either way it must not prompt or lose data.
    assert_eq!(r, ItemOutcome::Done);
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 1);
    let kept = if dst.exists() { dst } else { src };
    assert_eq!(read(&kept), b"data");
}

// ---------- destination changing mid-operation ----------

#[test]
fn copy_keep_both_skips_taken_names() {
    let (_t, dir) = tmp();
    let src = write_file(&dir, "a.txt", b"NEW");
    write_file(&dir, "b.txt", b"OLD");
    write_file(&dir, "b (2).txt", b"ALSO-OLD");

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::KeepBoth;
    let cx = ctx(&p, &c, &resolve);
    let r = copy_file(&src, &dir.join("b.txt"), &cx).unwrap();
    assert_eq!(r, ItemOutcome::Done);
    assert_eq!(read(&dir.join("b.txt")), b"OLD");
    assert_eq!(read(&dir.join("b (2).txt")), b"ALSO-OLD");
    assert_eq!(read(&dir.join("b (3).txt")), b"NEW");
}

// ---------- staging ----------

#[test]
fn staging_never_truncates_existing_data() {
    let (_t, dir) = tmp();
    let src = write_file(&dir, "a.txt", b"NEW");
    // A leftover temp name from a crashed run sharing the same op tag.
    let stale = write_file(&dir, "b.txt.fluxpart-1-0", b"SENTINEL");

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::Overwrite;
    let cx = ctx(&p, &c, &resolve);
    copy_file(&src, &dir.join("b.txt"), &cx).unwrap();
    assert_eq!(read(&dir.join("b.txt")), b"NEW");
    assert_eq!(read(&stale), b"SENTINEL", "staging truncated a leftover file");
}

#[test]
fn cancelled_copy_leaves_no_complete_looking_dst() {
    let (_t, dir) = tmp();
    // >16MiB forces the chunked path so cancellation lands mid-file.
    let data = vec![7u8; 20 * 1024 * 1024];
    let src = write_file(&dir, "big.bin", &data);

    let progress = Mutex::new(OpProgress::default());
    let cancel = AtomicBool::new(true); // cancelled before we even start
    let resolve = |_: &Path, _: &Path| ConflictAction::Overwrite;
    let cx = ctx(&progress, &cancel, &resolve);
    let r = copy_file(&src, &dir.join("big2.bin"), &cx);
    assert!(
        matches!(r, Ok(ItemOutcome::Cancelled) | Err(_)),
        "cancel mid-copy must not report success: {r:?}"
    );
    assert!(!dir.join("big2.bin").exists());
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        !names.iter().any(|n| n.contains("fluxpart")),
        "staging leftover: {names:?}"
    );
    assert_eq!(read(&src), data, "source must be untouched");
}

// ---------- partial moves keep untransferred sources ----------

#[test]
fn partial_move_keeps_skipped_source_items() {
    let (_t, dir) = tmp();
    let src = dir.join("src");
    std::fs::create_dir(&src).unwrap();
    write_file(&src, "ok.txt", b"OK");
    write_file(&src, "skip.txt", b"KEEP-ME");
    let dst = dir.join("dst");
    std::fs::create_dir(&dst).unwrap();
    write_file(&dst, "skip.txt", b"OLD"); // conflicts only for skip.txt

    let (p, c) = fresh!();
    // Skip only the conflicting child; overwrite the dst dir itself.
    let resolve = |_: &Path, d: &Path| {
        if d.file_name().map(|n| n == "skip.txt").unwrap_or(false) {
            ConflictAction::Skip
        } else {
            ConflictAction::Overwrite
        }
    };
    let cx = ctx(&p, &c, &resolve);
    set_force_copy_path(true); // exercise the cross-volume procedure
    let r = move_item(&src, &dst, &cx).unwrap();
    set_force_copy_path(false);

    assert_eq!(r, ItemOutcome::Partial);
    assert_eq!(read(&dst.join("ok.txt")), b"OK");
    assert_eq!(read(&dst.join("skip.txt")), b"OLD");
    assert!(
        !src.join("ok.txt").exists(),
        "successfully moved item should be pruned"
    );
    assert_eq!(
        read(&src.join("skip.txt")),
        b"KEEP-ME",
        "skipped source item must survive the move"
    );
    assert!(src.is_dir(), "source dir kept because it still holds items");
}

#[cfg(unix)]
#[test]
fn cross_volume_move_preserves_untransferred_source() {
    let (_t, dir) = tmp();
    let src = dir.join("src");
    std::fs::create_dir(&src).unwrap();
    write_file(&src, "ok.txt", b"OK");
    // A fifo can never be copied (it isn't a file/dir/symlink) — the move
    // must keep it rather than delete the source tree.
    let fifo = src.join("stuck");
    std::process::Command::new("mkfifo").arg(&fifo).status().unwrap();
    let dst = dir.join("dst");
    std::fs::create_dir(&dst).unwrap();

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::Overwrite;
    let cx = ctx(&p, &c, &resolve);
    set_force_copy_path(true);
    let r = move_item(&src, &dst, &cx).unwrap();
    set_force_copy_path(false);

    assert_eq!(r, ItemOutcome::Partial);
    assert_eq!(read(&dst.join("ok.txt")), b"OK");
    assert!(!src.join("ok.txt").exists());
    assert!(
        src.join("stuck").exists(),
        "untransferred source entry was deleted"
    );
}

#[test]
fn move_overwrite_replaces_dir_with_file() {
    let (_t, dir) = tmp();
    let src = dir.join("src");
    std::fs::create_dir(&src).unwrap();
    write_file(&src, "bad.txt", b"ORIGINAL");
    let dst = dir.join("dst");
    std::fs::create_dir(&dst).unwrap();
    // dst/bad.txt is a non-empty DIRECTORY; user chose Replace.
    std::fs::create_dir(dst.join("bad.txt")).unwrap();
    write_file(&dst.join("bad.txt"), "inner.txt", b"i");

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::Overwrite;
    let cx = ctx(&p, &c, &resolve);
    set_force_copy_path(true);
    let r = move_item(&src, &dst, &cx).unwrap();
    set_force_copy_path(false);

    assert_eq!(r, ItemOutcome::Done);
    assert!(dst.join("bad.txt").is_file());
    assert_eq!(read(&dst.join("bad.txt")), b"ORIGINAL");
    assert!(!src.join("bad.txt").exists());
}

// ---------- self-copy / descendant ----------

#[test]
fn directory_copy_into_descendant_is_rejected() {
    let (_t, dir) = tmp();
    let src = dir.join("project");
    std::fs::create_dir(&src).unwrap();
    write_file(&src, "a.txt", b"a");

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::Overwrite;
    let cx = ctx(&p, &c, &resolve);
    let r = copy_tree(&src, &src.join("backup"), &cx);
    assert!(r.is_err(), "copying a dir into itself must be rejected");
    let names: Vec<_> = std::fs::read_dir(&src).unwrap().collect();
    assert_eq!(names.len(), 1, "something was created inside src");
}

#[test]
fn directory_move_into_descendant_is_rejected() {
    let (_t, dir) = tmp();
    let src = dir.join("project");
    std::fs::create_dir(&src).unwrap();
    write_file(&src, "a.txt", b"a");

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::Overwrite;
    let cx = ctx(&p, &c, &resolve);
    let r = move_item(&src, &src.join("nested"), &cx);
    assert!(r.is_err());
    assert_eq!(read(&src.join("a.txt")), b"a");
}

#[cfg(unix)]
#[test]
fn symlinked_dst_descendant_is_rejected() {
    let (_t, dir) = tmp();
    let src = dir.join("real");
    std::fs::create_dir(&src).unwrap();
    write_file(&src, "a.txt", b"a");
    let link = dir.join("link");
    std::os::unix::fs::symlink(&src, &link).unwrap();

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::Overwrite;
    let cx = ctx(&p, &c, &resolve);
    // dst reaches into src through a symlink — naive prefix checks miss it.
    let r = move_item(&src, &link.join("inside"), &cx);
    assert!(r.is_err(), "symlinked descendant must be rejected");
    assert_eq!(read(&src.join("a.txt")), b"a");
}

// ---------- progress ----------

#[test]
fn copy_progress_does_not_double_count() {
    let (_t, dir) = tmp();
    let data = vec![3u8; 100_000];
    let src = write_file(&dir, "f.bin", &data);

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::Overwrite;
    let cx = ctx(&p, &c, &resolve);
    copy_tree(&src, &dir.join("g.bin"), &cx).unwrap();
    let pr = p.lock().unwrap();
    assert_eq!(pr.done_bytes, 100_000, "bytes counted twice");
    assert_eq!(pr.done_items, 1);
}

// ---------- rename staging recovery ----------

#[test]
fn cancelled_staged_rename_reports_recovery_mapping() {
    let (_t, dir) = tmp();
    let tmpfile = write_file(&dir, ".fluxstage-9-0-0", b"content");
    let intended = dir.join("final.txt");

    let (p, c) = fresh!();
    c.store(true, std::sync::atomic::Ordering::Relaxed);
    let resolve = |_: &Path, _: &Path| ConflictAction::Overwrite;
    let cx = ctx(&p, &c, &resolve);
    finish_staged(&[(tmpfile.clone(), intended.clone())], &cx);

    let pr = p.lock().unwrap();
    assert!(
        pr.errors.iter().any(|e| e.contains("left staged")),
        "no recovery info recorded: {:?}",
        pr.errors
    );
    assert!(tmpfile.exists(), "staged file must be left findable");
    assert!(!intended.exists());
}

// ---------- engine-level: the original silent-overwrite bug ----------

#[test]
fn engine_move_asks_before_overwriting() {
    let (_t, dir) = tmp();
    let from = dir.join("from");
    let to = dir.join("to");
    std::fs::create_dir(&from).unwrap();
    std::fs::create_dir(&to).unwrap();
    write_file(&from, "same.txt", b"NEW");
    write_file(&to, "same.txt", b"OLD");

    let engine = OpEngine::new();
    let id = engine.submit(
        OpKind::Move {
            sources: vec![from.join("same.txt")],
            dest_dir: to.clone(),
        },
        ConflictPolicy::Ask,
    );
    // The old code renamed first — dst would already be clobbered and no
    // conflict ever emitted. Assert the query arrives and answer it.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut answered = false;
    while Instant::now() < deadline {
        for ev in engine.drain_events() {
            if let OpEvent::Conflict(q) = ev {
                assert_eq!(q.dest, to.join("same.txt"));
                engine.answer_conflict(
                    q.op_id,
                    ConflictDecision {
                        action: ConflictAction::Skip,
                        apply_all: false,
                    },
                );
                answered = true;
            }
        }
        if answered {
            break;
        }
        assert_eq!(
            read(&to.join("same.txt")),
            b"OLD",
            "destination was overwritten before the user answered"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(answered, "conflict was never raised — silent overwrite");
    wait_op(&engine, id, Duration::from_secs(10));
    assert_eq!(read(&to.join("same.txt")), b"OLD");
    assert_eq!(read(&from.join("same.txt")), b"NEW");
}

#[test]
fn engine_move_overwrite_all_replaces() {
    let (_t, dir) = tmp();
    let from = dir.join("from");
    let to = dir.join("to");
    std::fs::create_dir(&from).unwrap();
    std::fs::create_dir(&to).unwrap();
    write_file(&from, "same.txt", b"NEW");
    write_file(&to, "same.txt", b"OLD");

    let engine = OpEngine::new();
    let id = engine.submit(
        OpKind::Move {
            sources: vec![from.join("same.txt")],
            dest_dir: to.clone(),
        },
        ConflictPolicy::OverwriteAll,
    );
    wait_op(&engine, id, Duration::from_secs(10));
    assert_eq!(read(&to.join("same.txt")), b"NEW");
    assert!(!from.join("same.txt").exists());
}

// ---------- non-UTF-8 ----------

#[cfg(unix)]
#[test]
fn non_utf8_filename_survives_copy() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let (_t, dir) = tmp();
    let name = OsStr::from_bytes(b"weird\xffname.bin");
    let src = dir.join(name);
    std::fs::write(&src, b"bytes").unwrap();
    let dst_dir = dir.join("out");
    std::fs::create_dir(&dst_dir).unwrap();

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::Overwrite;
    let cx = ctx(&p, &c, &resolve);
    copy_tree(&src, &dst_dir.join(name), &cx).unwrap();
    assert_eq!(std::fs::read(dst_dir.join(name)).unwrap(), b"bytes");
}

// ---------- special files ----------

#[cfg(unix)]
#[test]
fn fifo_is_reported_not_opened() {
    let (_t, dir) = tmp();
    let src = dir.join("pipe");
    std::process::Command::new("mkfifo").arg(&src).status().unwrap();

    let (p, c) = fresh!();
    let resolve = |_: &Path, _: &Path| ConflictAction::Overwrite;
    let cx = ctx(&p, &c, &resolve);
    // Must not hang opening the fifo.
    let r = copy_tree(&src, &dir.join("pipe2"), &cx).unwrap();
    assert_eq!(r, ItemOutcome::Partial);
    let pr = p.lock().unwrap();
    assert!(pr.errors.iter().any(|e| e.contains("special")));
    assert!(src.exists());
}
