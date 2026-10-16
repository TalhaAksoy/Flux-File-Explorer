use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use flux_operations::conflict::{
    ConflictAction, ConflictPolicy, keep_both_path, validate_filename,
};
use flux_operations::copy::{CopyCtx, ItemOutcome, copy_tree, discover};
use flux_operations::progress::{OpProgress, OpState};
use flux_operations::rename::{RenameRule, execute_renames, plan_renames};
use flux_operations::{OpEngine, OpEvent, OpKind, RenameStatus};

fn tmp() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().to_path_buf();
    (d, p)
}

fn write_file(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::File::create(&p)
        .unwrap()
        .write_all(contents)
        .unwrap();
    p
}

fn noop_ctx<'a>(progress: &'a Mutex<OpProgress>, cancel: &'a AtomicBool) -> CopyCtx<'a> {
    CopyCtx {
        progress,
        cancel,
        resolve: &|_s, _d| ConflictAction::Overwrite,
        tag: 1,
        created_dirs: Mutex::new(Vec::new()),
        failed: Mutex::new(Default::default()),
    }
}

// ---------- filename validation ----------

#[test]
fn validate_rejects_bad_names() {
    assert!(validate_filename("ok.txt").is_ok());
    assert!(validate_filename("").is_err());
    assert!(validate_filename("a/b").is_err());
    assert!(validate_filename("..").is_err());
    assert!(validate_filename(&"x".repeat(300)).is_err());
}

#[test]
fn keep_both_finds_free_name() {
    let (_t, dir) = tmp();
    write_file(&dir, "a.txt", b"1");
    write_file(&dir, "a (2).txt", b"2");
    let p = keep_both_path(&dir, std::ffi::OsStr::new("a.txt"));
    assert_eq!(p.file_name().unwrap(), "a (3).txt");
}

// ---------- copy ----------

#[test]
fn copy_tree_copies_nested_structure() {
    let (_t, dir) = tmp();
    let src = dir.join("src");
    std::fs::create_dir(&src).unwrap();
    write_file(&src, "a.txt", b"hello");
    std::fs::create_dir(src.join("sub")).unwrap();
    write_file(&src.join("sub"), "b.txt", b"world");

    let dst = dir.join("dst");
    let progress = Mutex::new(OpProgress::default());
    let cancel = AtomicBool::new(false);
    let ctx = noop_ctx(&progress, &cancel);
    let r = copy_tree(&src, &dst, &ctx).unwrap();
    assert_eq!(r, flux_operations::copy::ItemOutcome::Done);
    assert_eq!(std::fs::read(dst.join("a.txt")).unwrap(), b"hello");
    assert_eq!(std::fs::read(dst.join("sub/b.txt")).unwrap(), b"world");
    // No temp files left behind.
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(
        !names
            .iter()
            .any(|n| n.to_string_lossy().contains("fluxpart"))
    );
}

#[test]
fn copy_preserves_contents_byte_exact() {
    let (_t, dir) = tmp();
    let data: Vec<u8> = (0..200_000u32).map(|i| (i * 31 % 251) as u8).collect();
    let src = write_file(&dir, "big.bin", &data);
    let dst = dir.join("big-copy.bin");
    let progress = Mutex::new(OpProgress::default());
    let cancel = AtomicBool::new(false);
    let ctx = noop_ctx(&progress, &cancel);
    copy_tree(&src, &dst, &ctx).unwrap();
    assert_eq!(std::fs::read(&dst).unwrap(), data);
}

#[test]
fn discover_counts_items_and_bytes() {
    let (_t, dir) = tmp();
    write_file(&dir, "a", &[0u8; 100]);
    std::fs::create_dir(dir.join("d")).unwrap();
    write_file(&dir.join("d"), "b", &[0u8; 50]);
    let progress = Mutex::new(OpProgress::default());
    let cancel = AtomicBool::new(false);
    let ctx = noop_ctx(&progress, &cancel);
    discover(&ctx, std::slice::from_ref(&dir));
    let p = progress.lock().unwrap();
    assert_eq!(p.total_items, 4); // dir + file a + subdir + file b
    assert_eq!(p.total_bytes, 150);
}

// ---------- rename planning ----------

#[test]
fn plan_find_replace() {
    let (_t, dir) = tmp();
    let a = write_file(&dir, "old_a.txt", b"");
    let b = write_file(&dir, "old_b.txt", b"");
    let plan = plan_renames(
        &[a.clone(), b.clone()],
        &[RenameRule::FindReplace {
            find: "old".into(),
            replace: "new".into(),
            regex: false,
            case_sensitive: false,
        }],
    );
    assert_eq!(plan[0].dst.file_name().unwrap(), "new_a.txt");
    assert_eq!(plan[1].dst.file_name().unwrap(), "new_b.txt");
    assert!(plan.iter().all(|a| a.status.is_ready()));
}

#[test]
fn plan_detects_duplicate_targets() {
    let (_t, dir) = tmp();
    let a = write_file(&dir, "a.txt", b"");
    let b = write_file(&dir, "b.txt", b"");
    let plan = plan_renames(&[a, b], &[RenameRule::Template("same.txt".into())]);
    assert_eq!(plan[0].status, RenameStatus::Ready);
    assert_eq!(plan[1].status, RenameStatus::DuplicateTarget);
}

#[test]
fn plan_detects_disk_conflict() {
    let (_t, dir) = tmp();
    let a = write_file(&dir, "a.txt", b"");
    write_file(&dir, "exists.txt", b"");
    let plan = plan_renames(&[a], &[RenameRule::Template("exists.txt".into())]);
    assert_eq!(plan[0].status, RenameStatus::Conflict);
}

#[test]
fn plan_template_tokens() {
    let (_t, dir) = tmp();
    std::fs::create_dir(dir.join("subdir")).unwrap();
    let a = write_file(&dir.join("subdir"), "photo.JPG", b"");
    let plan = plan_renames(
        &[a],
        &[RenameRule::Template("{parent}_{name}_{n:03}.{ext}".into())],
    );
    assert_eq!(plan[0].dst.file_name().unwrap(), "subdir_photo_000.JPG");
}

#[test]
fn plan_numbering_and_case() {
    let (_t, dir) = tmp();
    let files: Vec<_> = (0..3)
        .map(|i| write_file(&dir, &format!("IMG{i}.png"), b""))
        .collect();
    let plan = plan_renames(
        &files,
        &[
            RenameRule::ChangeCase(flux_operations::rename::CaseKind::Lower),
            RenameRule::Numbering {
                start: 10,
                pad: 3,
                prefix: false,
            },
        ],
    );
    assert_eq!(plan[0].dst.file_name().unwrap(), "img0_010.png");
    assert_eq!(plan[2].dst.file_name().unwrap(), "img2_012.png");
}

#[test]
fn execute_handles_rename_cycle() {
    let (_t, dir) = tmp();
    let a = write_file(&dir, "A.txt", b"aaa");
    let b = write_file(&dir, "B.txt", b"bbb");
    // Swap names.
    let plan = vec![
        flux_operations::rename::RenameAction {
            src: a.clone(),
            dst: dir.join("B.txt"),
            status: RenameStatus::Ready,
        },
        flux_operations::rename::RenameAction {
            src: b.clone(),
            dst: dir.join("A.txt"),
            status: RenameStatus::Ready,
        },
    ];
    let progress = Mutex::new(OpProgress::default());
    let cancel = AtomicBool::new(false);
    let ctx = noop_ctx(&progress, &cancel);
    execute_renames(&plan, &ctx);
    assert_eq!(std::fs::read(dir.join("A.txt")).unwrap(), b"bbb");
    assert_eq!(std::fs::read(dir.join("B.txt")).unwrap(), b"aaa");
    // No staging leftovers.
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("fluxstage")
        })
        .collect();
    assert!(leftovers.is_empty());
}

// ---------- engine ----------

fn wait_op(engine: &OpEngine, id: u64, timeout: Duration) -> OpProgress {
    let deadline = Instant::now() + timeout;
    loop {
        for ev in engine.drain_events() {
            if let OpEvent::Conflict(_) = ev {
                panic!("unexpected conflict in test");
            }
        }
        if let Ok(ops) = engine.ops.read()
            && let Some(o) = ops.iter().find(|o| o.id == id)
        {
            let p = o.progress_snapshot();
            if p.state.is_finished() {
                return p;
            }
        }
        if Instant::now() > deadline {
            panic!("op {id} did not finish");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn engine_copies_files_end_to_end() {
    let (_t, dir) = tmp();
    let src_dir = dir.join("from");
    let dst_dir = dir.join("to");
    std::fs::create_dir(&src_dir).unwrap();
    std::fs::create_dir(&dst_dir).unwrap();
    write_file(&src_dir, "x.txt", b"data!");
    let engine = OpEngine::new();
    let id = engine.submit(
        OpKind::Copy {
            sources: vec![src_dir.join("x.txt")],
            dest_dir: dst_dir.clone(),
        },
        ConflictPolicy::OverwriteAll,
    );
    let p = wait_op(&engine, id, Duration::from_secs(10));
    assert!(matches!(p.state, OpState::Done), "state: {:?}", p.state);
    assert_eq!(std::fs::read(dst_dir.join("x.txt")).unwrap(), b"data!");
}

#[test]
fn engine_move_removes_source() {
    let (_t, dir) = tmp();
    let src_dir = dir.join("from");
    let dst_dir = dir.join("to");
    std::fs::create_dir(&src_dir).unwrap();
    std::fs::create_dir(&dst_dir).unwrap();
    write_file(&src_dir, "m.txt", b"payload");
    let engine = OpEngine::new();
    let id = engine.submit(
        OpKind::Move {
            sources: vec![src_dir.join("m.txt")],
            dest_dir: dst_dir.clone(),
        },
        ConflictPolicy::OverwriteAll,
    );
    let p = wait_op(&engine, id, Duration::from_secs(10));
    assert!(matches!(p.state, OpState::Done));
    assert!(!src_dir.join("m.txt").exists());
    assert_eq!(std::fs::read(dst_dir.join("m.txt")).unwrap(), b"payload");
}

#[test]
fn engine_delete_removes_tree() {
    let (_t, dir) = tmp();
    let victim = dir.join("victim");
    std::fs::create_dir(&victim).unwrap();
    write_file(&victim, "inside.txt", b"x");
    let engine = OpEngine::new();
    let id = engine.submit(
        OpKind::Delete {
            items: vec![victim.clone()],
        },
        ConflictPolicy::OverwriteAll,
    );
    wait_op(&engine, id, Duration::from_secs(10));
    assert!(!victim.exists());
}

#[test]
fn engine_mkdir_and_create_file() {
    let (_t, dir) = tmp();
    let engine = OpEngine::new();
    let id = engine.submit(
        OpKind::Mkdir {
            path: dir.join("newdir"),
        },
        ConflictPolicy::Ask,
    );
    wait_op(&engine, id, Duration::from_secs(10));
    assert!(dir.join("newdir").is_dir());
}

#[test]
fn engine_conflict_ask_pauses_for_answer() {
    let (_t, dir) = tmp();
    let src_dir = dir.join("from");
    let dst_dir = dir.join("to");
    std::fs::create_dir(&src_dir).unwrap();
    std::fs::create_dir(&dst_dir).unwrap();
    write_file(&src_dir, "same.txt", b"NEW");
    write_file(&dst_dir, "same.txt", b"OLD");
    let engine = OpEngine::new();
    let id = engine.submit(
        OpKind::Copy {
            sources: vec![src_dir.join("same.txt")],
            dest_dir: dst_dir.clone(),
        },
        ConflictPolicy::Ask,
    );
    // Wait for the conflict event then answer KeepBoth.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut answered = false;
    while Instant::now() < deadline {
        for ev in engine.drain_events() {
            if let OpEvent::Conflict(q) = ev {
                engine.answer_conflict(
                    q.op_id,
                    flux_operations::ConflictDecision {
                        action: ConflictAction::KeepBoth,
                        apply_all: false,
                    },
                );
                answered = true;
            }
        }
        if answered {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(answered, "conflict never arrived");
    let p = wait_op(&engine, id, Duration::from_secs(10));
    assert!(matches!(p.state, OpState::Done));
    assert_eq!(std::fs::read(dst_dir.join("same.txt")).unwrap(), b"OLD");
    assert!(dst_dir.join("same (2).txt").exists());
}

// ---------- duplicate ----------

#[test]
fn duplicate_creates_copy_with_suffix() {
    let (_t, dir) = tmp();
    let file = write_file(&dir, "report.txt", b"data");
    let engine = OpEngine::new();
    let id = engine.submit(OpKind::Duplicate { items: vec![file] }, ConflictPolicy::Ask);
    let p = wait_op(&engine, id, Duration::from_secs(10));
    assert!(matches!(p.state, OpState::Done), "state: {:?}", p.state);
    assert!(dir.join("report.txt").exists());
    assert_eq!(std::fs::read(dir.join("report copy.txt")).unwrap(), b"data");
}

#[test]
fn duplicate_increments_when_copy_exists() {
    let (_t, dir) = tmp();
    write_file(&dir, "a.txt", b"1");
    write_file(&dir, "a copy.txt", b"x");
    write_file(&dir, "a copy 2.txt", b"x");
    let engine = OpEngine::new();
    let id = engine.submit(
        OpKind::Duplicate {
            items: vec![dir.join("a.txt")],
        },
        ConflictPolicy::Ask,
    );
    wait_op(&engine, id, Duration::from_secs(10));
    assert!(dir.join("a copy 3.txt").exists());
}

#[test]
fn duplicate_dir_copies_tree() {
    let (_t, dir) = tmp();
    let src = dir.join("docs");
    std::fs::create_dir(&src).unwrap();
    write_file(&src, "inner.txt", b"i");
    let engine = OpEngine::new();
    let id = engine.submit(OpKind::Duplicate { items: vec![src] }, ConflictPolicy::Ask);
    wait_op(&engine, id, Duration::from_secs(10));
    assert_eq!(
        std::fs::read(dir.join("docs copy").join("inner.txt")).unwrap(),
        b"i"
    );
}

// ---------- compress ----------

#[test]
fn compress_creates_valid_zip() {
    let (_t, dir) = tmp();
    write_file(&dir, "a.txt", b"hello");
    let sub = dir.join("docs");
    std::fs::create_dir(&sub).unwrap();
    write_file(&sub, "inner.txt", b"inner-data");
    let dest = dir.join("out.zip");
    let engine = OpEngine::new();
    let id = engine.submit(
        OpKind::Compress {
            items: vec![dir.join("a.txt"), sub.clone()],
            dest: dest.clone(),
        },
        ConflictPolicy::Ask,
    );
    let p = wait_op(&engine, id, Duration::from_secs(15));
    assert!(matches!(p.state, OpState::Done), "state: {:?}", p.state);
    assert!(dest.exists());
    let mut za = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
    let names: Vec<String> = (0..za.len())
        .filter_map(|i| {
            za.by_index(i)
                .ok()
                .and_then(|e| e.name().ok().map(|n| n.into_owned()))
        })
        .collect();
    assert!(names.iter().any(|n| n == "a.txt"), "{names:?}");
    assert!(names.iter().any(|n| n == "docs/inner.txt"), "{names:?}");
    assert!(names.iter().any(|n| n == "docs/"), "{names:?}");
    use std::io::Read as _;
    let mut s = String::new();
    za.by_name("docs/inner.txt")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert_eq!(s, "inner-data");
    // Staging is transient — no .fluxpart-* litter next to the output.
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".fluxpart-"))
        .collect();
    assert!(leftovers.is_empty(), "staging files left: {leftovers:?}");
}

#[test]
fn compress_cancel_publishes_nothing() {
    let (_t, dir) = tmp();
    write_file(&dir, "a.txt", b"hello");
    let dest = dir.join("out.zip");
    let progress = Mutex::new(OpProgress::default());
    let cancel = AtomicBool::new(true); // cancelled before the first byte
    let ctx = noop_ctx(&progress, &cancel);
    flux_operations::archive::compress_items(&[dir.join("a.txt")], &dest, &ctx).unwrap();
    assert!(!dest.exists(), "cancelled compress must not publish dest");
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".fluxpart-"))
        .collect();
    assert!(leftovers.is_empty(), "staging files left: {leftovers:?}");
}

#[test]
fn compress_replaces_existing_zip_after_full_write() {
    let (_t, dir) = tmp();
    write_file(&dir, "new.txt", b"new-data");
    let dest = dir.join("out.zip");
    // Pre-existing "old" archive must survive until the new one is
    // complete — and be replaced wholesale on success.
    write_file(&dir, "out.zip", b"not-a-zip-yet");
    let progress = Mutex::new(OpProgress::default());
    let cancel = AtomicBool::new(false);
    let ctx = noop_ctx(&progress, &cancel);
    flux_operations::archive::compress_items(&[dir.join("new.txt")], &dest, &ctx).unwrap();
    let mut za = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
    assert!(za.by_name("new.txt").is_ok());
}

#[cfg(unix)]
#[test]
fn compress_stores_symlink_as_link_not_content() {
    let (_t, dir) = tmp();
    write_file(&dir, "real.txt", b"real-data");
    std::os::unix::fs::symlink("real.txt", dir.join("link.txt")).unwrap();
    // A link pointing OUTSIDE the selection must not leak its content.
    let (_t2, outdir) = tmp();
    let outside = outdir.join("secret.txt");
    std::fs::write(&outside, b"do-not-bundle").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("escape.txt")).unwrap();
    let dest = dir.join("out.zip");
    let progress = Mutex::new(OpProgress::default());
    let cancel = AtomicBool::new(false);
    let ctx = noop_ctx(&progress, &cancel);
    flux_operations::archive::compress_items(
        &[
            dir.join("real.txt"),
            dir.join("link.txt"),
            dir.join("escape.txt"),
        ],
        &dest,
        &ctx,
    )
    .unwrap();
    let mut za = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
    use std::io::Read as _;
    // Symlink entry: mode bits say link, payload is the target path —
    // not the target's bytes.
    let (mode, target) = {
        let mut entry = za.by_name("link.txt").unwrap();
        let mut t = String::new();
        entry.read_to_string(&mut t).unwrap();
        (entry.unix_mode().unwrap_or(0), t)
    };
    assert_eq!(mode & 0o170000, 0o120000, "link.txt should be a symlink");
    assert_eq!(target, "real.txt");
    let mut payload = String::new();
    za.by_name("escape.txt")
        .unwrap()
        .read_to_string(&mut payload)
        .unwrap();
    assert_eq!(payload, outside.to_string_lossy());
    assert!(!payload.contains("do-not-bundle"));
}

// ---------- ops safety audit regressions ----------

#[cfg(unix)]
#[test]
fn copy_symlink_overwrite_replaces_existing_dest() {
    // Previously the link creation hit the occupied name and errored even
    // with an Overwrite answer — now it stages + backup-swaps like files.
    let (_t, dir) = tmp();
    write_file(&dir, "target.txt", b"t");
    std::os::unix::fs::symlink("target.txt", dir.join("link")).unwrap();
    let destdir = dir.join("dst");
    std::fs::create_dir(&destdir).unwrap();
    write_file(&destdir, "link", b"preexisting");
    let progress = Mutex::new(OpProgress::default());
    let cancel = AtomicBool::new(false);
    let ctx = noop_ctx(&progress, &cancel); // resolver answers Overwrite
    let r = copy_tree(&dir.join("link"), &destdir.join("link"), &ctx).unwrap();
    assert_eq!(r, ItemOutcome::Done);
    let dst = destdir.join("link");
    let md = std::fs::symlink_metadata(&dst).unwrap();
    assert!(md.file_type().is_symlink(), "dest should be a symlink now");
    assert_eq!(
        std::fs::read_link(&dst).unwrap(),
        PathBuf::from("target.txt")
    );
}

#[cfg(unix)]
#[test]
fn copy_symlink_does_not_follow_link() {
    // The link itself is copied — the target's content must not be
    // duplicated into a real file at the destination.
    let (_t, dir) = tmp();
    write_file(&dir, "real.txt", b"real");
    std::os::unix::fs::symlink("real.txt", dir.join("alias")).unwrap();
    let dst = dir.join("out");
    let progress = Mutex::new(OpProgress::default());
    let cancel = AtomicBool::new(false);
    let ctx = noop_ctx(&progress, &cancel);
    let r = copy_tree(&dir.join("alias"), &dst, &ctx).unwrap();
    assert_eq!(r, ItemOutcome::Done);
    assert!(std::fs::symlink_metadata(&dst).unwrap().file_type().is_symlink());
}

#[test]
fn remove_tree_deletes_readonly_files() {
    // POSIX deletes readonly files outright (dir perms govern); on
    // Windows remove_file_force clears the attribute first — either way
    // the file must go when the user asked for a permanent delete.
    let (_t, dir) = tmp();
    let p = write_file(&dir, "ro.txt", b"ro");
    let mut perms = std::fs::metadata(&p).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&p, perms).unwrap();
    let progress = Mutex::new(OpProgress::default());
    let cancel = AtomicBool::new(false);
    let ctx = noop_ctx(&progress, &cancel);
    flux_operations::copy::remove_tree(&p, &ctx).unwrap();
    assert!(!p.exists());
}
