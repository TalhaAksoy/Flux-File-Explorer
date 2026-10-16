//! Safe copy/move primitives.
//!
//! Invariants:
//! - file copies go to a sibling `*.fluxpart-*` temp claimed with
//!   `create_new` (atomic, never truncates a leftover) then renamed into
//!   place — a cancelled/failed copy never leaves a complete-looking
//!   destination.
//! - `Move` resolves the destination conflict policy BEFORE any rename;
//!   nothing is ever silently overwritten. Same-volume moves stay atomic;
//!   cross-volume moves copy, then remove only the parts of the source
//!   tree that actually transferred (see [`prune_copied`]).
//! - replacing an existing destination moves it aside first and restores
//!   it if the swap fails — the old file is never unprotected.
//! - a directory can never be copied/moved into itself or a descendant.
//! - symlink sources are recreated as symlinks, never followed into;
//!   non-file/non-dir/non-symlink entries (fifos, sockets, devices) are
//!   reported, not opened — opening a fifo would block forever.
//! - cancelling removes in-flight temp files and dirs this op created.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use rustc_hash::FxHashSet;

use crate::conflict::{ConflictAction, keep_both_path};
use crate::progress::OpProgress;

/// Files smaller than this use `fs::copy`, which on all three platforms maps
/// to a kernel-side copy (copy_file_range / fcopyfile-clone / CopyFileExW):
/// faster and cancellation between files is still prompt. Larger files use
/// the chunked loop for progress + mid-file cancel.
const SMALL_FILE_FAST_PATH: u64 = 16 * 1024 * 1024;
const COPY_BUF: usize = 1024 * 1024;

pub type ConflictResolver<'a> = dyn Fn(&Path, &Path) -> ConflictAction + 'a;

pub struct CopyCtx<'a> {
    pub progress: &'a Mutex<OpProgress>,
    pub cancel: &'a AtomicBool,
    pub resolve: &'a ConflictResolver<'a>,
    /// Unique-ish tag for temp names (op id).
    pub tag: u64,
    /// Dirs this operation created; removed on cancel (deepest first).
    pub created_dirs: Mutex<Vec<PathBuf>>,
    /// Source paths whose transfer did NOT fully succeed (skipped or
    /// errored). On move these must survive — `prune_copied` consults this.
    pub failed: Mutex<FxHashSet<PathBuf>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemOutcome {
    /// Fully transferred, no errors.
    Done,
    /// Finished but at least one child was skipped or failed (details are
    /// in `progress.errors` / `ctx.failed`). A move source in this state
    /// keeps its untransferred items.
    Partial,
    /// The item itself was skipped by conflict policy.
    Skipped,
    Cancelled,
}

pub fn cancelled(c: &CopyCtx<'_>) -> bool {
    c.cancel.load(Ordering::Relaxed)
}

fn set_state(c: &CopyCtx<'_>, f: impl FnOnce(&mut OpProgress)) {
    if let Ok(mut p) = c.progress.lock() {
        f(&mut p);
    }
}

pub fn add_items(c: &CopyCtx<'_>, n: u64, bytes: u64) {
    set_state(c, |p| {
        p.total_items += n;
        p.total_bytes += bytes;
    });
}

/// Count one finished item. Byte accounting is done by the copy loops
/// themselves; mixing the two double-counts progress.
pub fn tick_item(c: &CopyCtx<'_>, path: &Path) {
    set_state(c, |p| {
        p.done_items += 1;
        p.current_item = path.to_path_buf();
    });
}

pub fn note_error(c: &CopyCtx<'_>, msg: String) {
    set_state(c, |p| p.record_error(msg));
}

/// Mark a source path as not fully transferred — it must survive a move.
fn mark_failed(c: &CopyCtx<'_>, src: &Path) {
    c.failed.lock().unwrap().insert(src.to_path_buf());
}

/// Existence check that also sees broken symlinks (`Path::exists` follows
/// links and reports false for them — that would happily overwrite one).
fn dst_exists(dst: &Path) -> bool {
    fs::symlink_metadata(dst).is_ok()
}

/// Same-filesystem-object test: dev+inode on unix, volume+file_index on
/// Windows, canonical-path equality elsewhere. Catches "move file onto
/// itself" and case-insensitive name aliases that must not be treated as
/// conflicts (or worse, copied over and then deleted).
pub fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        // stat-based identity — must NOT open the path: opening a FIFO
        // blocks forever and unreadable files can't be opened at all,
        // while dev+ino comparison needs neither.
        use std::os::unix::fs::MetadataExt;
        if let (Ok(x), Ok(y)) = (fs::metadata(a), fs::metadata(b)) {
            return x.dev() == y.dev() && x.ino() == y.ino();
        }
    }
    #[cfg(windows)]
    {
        // `std::os::windows::fs::MetadataExt::{volume_serial_number,
        // file_index}` is still unstable (`windows_by_handle`); same-file's
        // Handle compares exactly those fields via a proper CreateFile.
        if let (Ok(x), Ok(y)) = (
            same_file::Handle::from_path(a),
            same_file::Handle::from_path(b),
        ) {
            return x == y;
        }
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Canonical form of `p`, tolerating a not-yet-existing tail: canonicalize
/// the deepest existing ancestor and re-attach the missing components.
fn normalize(p: &Path) -> PathBuf {
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = p.to_path_buf();
    loop {
        if let Ok(canon) = fs::canonicalize(&cur) {
            let mut out = canon;
            for m in missing.iter().rev() {
                out.push(m);
            }
            return out;
        }
        match cur.file_name() {
            Some(n) => {
                missing.push(n.to_os_string());
                cur.pop();
            }
            // Hit the root without finding an existing ancestor.
            None => return cur,
        }
    }
}

/// True when `dst` is `src` itself or lives inside it — the operation that
/// must be rejected before it can recurse into the dir it just created.
/// Compares canonicalized prefixes so symlinks/junctions can't evade it
/// (a plain string prefix check is easy to fool and misses `..`-free but
/// symlinked ancestors).
pub fn is_descendant_or_same(src: &Path, dst: &Path) -> bool {
    // Cheap lexical test first; if dst isn't even textually under src it
    // can't be a descendant of the *lexical* src. We still do the full
    // canonical check — dest_dir may itself traverse a symlink.
    let s = normalize(src);
    let d = normalize(dst);
    paths_equal(&d, &s) || path_starts_with(&d, &s)
}

#[cfg(windows)]
fn path_starts_with(p: &Path, prefix: &Path) -> bool {
    // Windows filesystems are overwhelmingly case-insensitive — but the
    // comparison must stay component-wise: a raw string prefix treats
    // the sibling "docs copy" as inside "docs" and wrongly rejects
    // duplicates/moves between prefix-sharing names.
    let mut pc = p.components();
    for pre in prefix.components() {
        let Some(c) = pc.next() else { return false };
        if c.as_os_str().to_string_lossy().to_lowercase()
            != pre.as_os_str().to_string_lossy().to_lowercase()
        {
            return false;
        }
    }
    true
}

#[cfg(not(windows))]
fn path_starts_with(p: &Path, prefix: &Path) -> bool {
    p.starts_with(prefix)
}

#[cfg(windows)]
fn paths_equal(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

#[cfg(not(windows))]
fn paths_equal(a: &Path, b: &Path) -> bool {
    a == b
}

/// Walk sources, counting items and bytes. Cancel-aware.
pub fn discover(c: &CopyCtx<'_>, sources: &[PathBuf]) {
    set_state(c, |p| p.state = crate::OpState::Discovering);
    let mut stack: Vec<PathBuf> = sources.to_vec();
    while let Some(p) = stack.pop() {
        if cancelled(c) {
            return;
        }
        let md = match fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(e) => {
                note_error(c, format!("{}: {e}", p.display()));
                continue;
            }
        };
        if md.is_dir() && !md.file_type().is_symlink() {
            add_items(c, 1, 0);
            if let Ok(rd) = fs::read_dir(&p) {
                for e in rd.flatten() {
                    stack.push(e.path());
                }
            }
        } else {
            add_items(c, 1, md.len());
        }
    }
}

/// Claim a unique staging path next to `dst` via `create_new` — atomic,
/// and it can never truncate a leftover `*.fluxpart-*` or a user file.
fn create_staging(dst: &Path, tag: u64) -> std::io::Result<(PathBuf, fs::File)> {
    let base = dst
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    for n in 0..100u32 {
        let mut name = base.clone();
        name.push(format!(".fluxpart-{tag}-{n}"));
        let tmp = dst.with_file_name(name);
        match fs::File::create_new(&tmp) {
            Ok(f) => return Ok((tmp, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("no free staging name"))
}

/// A free sibling path used to park the existing destination during a
/// replace. Existence-checked; the window before the rename is tiny and
/// the name is ours to recycle anyway.
fn unique_sibling(dst: &Path, marker: &str) -> PathBuf {
    let base = dst
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    for n in 0..1000u32 {
        let mut name = base.clone();
        name.push(format!("{marker}-{n}"));
        let p = dst.with_file_name(name);
        if !dst_exists(&p) {
            return p;
        }
    }
    let mut name = base;
    name.push(format!("{marker}-{}", std::process::id()));
    dst.with_file_name(name)
}

/// Place `src` at `dst`, protecting any existing destination: the old
/// entry is first moved aside and restored if the swap fails, so the
/// user's previous data is never left unprotected. On unix `rename`
/// would replace atomically anyway; the dance mainly serves Windows,
/// where `rename` refuses to overwrite — and it keeps semantics uniform.
/// Best-effort atomic: not a transactional replace on any platform.
pub fn rename_replace(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst_exists(dst) {
        return fs::rename(src, dst);
    }
    let backup = unique_sibling(dst, ".fluxbak");
    fs::rename(dst, &backup)?;
    match fs::rename(src, dst) {
        Ok(()) => {
            match fs::symlink_metadata(&backup) {
                Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {
                    let _ = fs::remove_dir_all(&backup);
                }
                _ => {
                    let _ = fs::remove_file(&backup);
                }
            }
            Ok(())
        }
        Err(e) => {
            // Restore the previous destination as if nothing happened.
            let _ = fs::rename(&backup, dst);
            Err(e)
        }
    }
}

/// Copy one file's contents into `out` (already-opened staging file),
/// chunk-loop with cancel + byte progress. On failure/cancel the caller
/// removes the temp.
fn copy_file_chunks(src: &Path, mut out: fs::File, c: &CopyCtx<'_>) -> std::io::Result<u64> {
    let mut input = fs::File::open(src)?;
    let mut buf = vec![0u8; COPY_BUF];
    let mut written = 0u64;
    loop {
        if cancelled(c) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "cancelled",
            ));
        }
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        written += n as u64;
        set_state(c, |p| p.done_bytes += n as u64);
    }
    out.sync_all()?;
    Ok(written)
}

/// Best-effort metadata preservation (mtime + permissions).
fn preserve_metadata(src: &Path, dst: &Path) {
    if let Ok(md) = fs::metadata(src) {
        let _ = fs::set_permissions(dst, md.permissions());
        if let Ok(m) = md.modified() {
            let _ = filetime::set_file_mtime(dst, filetime::FileTime::from_system_time(m));
        }
    }
}

/// Resolve where `dst` should actually be written given the conflict
/// policy. Returns `(final_path, existed_at_resolve)`; `None` for
/// skip/cancel (caller distinguishes via `cancelled`).
fn resolve_dest(src: &Path, dst: &Path, c: &CopyCtx<'_>) -> Option<(PathBuf, bool)> {
    if !dst_exists(dst) {
        return Some((dst.to_path_buf(), false));
    }
    let action = (c.resolve)(src, dst);
    match action {
        ConflictAction::Overwrite => Some((dst.to_path_buf(), true)),
        ConflictAction::Skip => None,
        ConflictAction::KeepBoth => {
            let name = dst.file_name()?;
            Some((keep_both_path(dst.parent()?, name), false))
        }
        ConflictAction::CancelOp => {
            c.cancel.store(true, Ordering::Relaxed);
            None
        }
    }
}

/// `clonefile(2)` — APFS copy-on-write clone. The destination must not
/// exist, so the clone itself claims a `.fluxpart-*` name (same scheme as
/// `create_staging`). Returns the claimed path; `None` means cloning is
/// unavailable here and the caller should stage a real byte copy.
// Raw FFI is unavoidable: neither std nor libc wraps clonefile.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn clone_file(src: &Path, dst: &Path, tag: u64) -> Option<PathBuf> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn clonefile(src: *const std::ffi::c_char, dst: *const std::ffi::c_char, flags: i32)
        -> i32;
    }
    let csrc = CString::new(src.as_os_str().as_bytes()).ok()?;
    let base = dst.file_name()?.to_os_string();
    for n in 0..100u32 {
        let mut name = base.clone();
        name.push(format!(".fluxpart-{tag}-c{n}"));
        let tmp = dst.with_file_name(name);
        let Ok(cdst) = CString::new(tmp.as_os_str().as_bytes()) else {
            continue;
        };
        // SAFETY: both args are valid NUL-terminated path strings.
        if unsafe { clonefile(csrc.as_ptr(), cdst.as_ptr(), 0) } == 0 {
            return Some(tmp);
        }
        // Name collision → try the next suffix; anything else (ENOTSUP on
        // non-APFS, EXDEV, EPERM, ENOENT) means no clone is possible.
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists {
            return None;
        }
    }
    None
}

/// Copy a single file `src` to `dst` (final path). Handles conflict
/// resolution, staging, verification metadata.
pub fn copy_file(src: &Path, dst: &Path, c: &CopyCtx<'_>) -> std::io::Result<ItemOutcome> {
    let Some((mut final_dst, mut existed)) = resolve_dest(src, dst, c) else {
        return Ok(if cancelled(c) {
            ItemOutcome::Cancelled
        } else {
            ItemOutcome::Skipped
        });
    };
    let src_len = fs::symlink_metadata(src).map(|m| m.len()).unwrap_or(0);
    // APFS clonefile(2): a copy-on-write, metadata-only clone — near-instant
    // for any size. It clones into a claimed `.fluxpart-*` name and shares
    // the normal commit path, so atomicity is unchanged. Any failure
    // (non-APFS volume, cross-device, EPERM) falls back to a byte copy.
    #[cfg(target_os = "macos")]
    let cloned = clone_file(src, &final_dst, c.tag);
    #[cfg(not(target_os = "macos"))]
    let cloned: Option<PathBuf> = None;
    let tmp = match cloned {
        Some(tmp) => {
            // The clone is atomic — but honor a cancel that landed before
            // we commit, same contract as the byte-copy path.
            if cancelled(c) {
                let _ = fs::remove_file(&tmp);
                return Ok(ItemOutcome::Cancelled);
            }
            set_state(c, |p| p.done_bytes += src_len);
            tmp
        }
        None => {
            let (tmp, tmp_file) = create_staging(&final_dst, c.tag)?;
            let res = if src_len <= SMALL_FILE_FAST_PATH {
                // We exclusively claimed `tmp`, so overwriting our own
                // empty file with the kernel-side copy is safe.
                drop(tmp_file);
                fs::copy(src, &tmp).inspect(|&n| {
                    set_state(c, |p| p.done_bytes += n);
                })
            } else {
                copy_file_chunks(src, tmp_file, c)
            };
            let n = match res {
                Ok(n) => n,
                Err(e) => {
                    let _ = fs::remove_file(&tmp);
                    if cancelled(c) {
                        return Ok(ItemOutcome::Cancelled);
                    }
                    return Err(e);
                }
            };
            if n != src_len {
                let _ = fs::remove_file(&tmp);
                return Err(std::io::Error::other(format!(
                    "size mismatch: {} bytes written, expected {src_len}",
                    n
                )));
            }
            tmp
        }
    };
    // Commit. If the destination appeared (or the KeepBoth name got
    // taken) between resolution and now, resolve again rather than
    // clobbering whatever landed there.
    for _ in 0..8 {
        if !dst_exists(&final_dst) {
            match fs::rename(&tmp, &final_dst) {
                Ok(()) => {
                    preserve_metadata(src, &final_dst);
                    return Ok(ItemOutcome::Done);
                }
                Err(e) => {
                    if dst_exists(&final_dst) {
                        continue; // lost a race — re-resolve below
                    }
                    let _ = fs::remove_file(&tmp);
                    return Err(e);
                }
            }
        }
        if existed {
            // User already chose overwrite for this path.
            match rename_replace(&tmp, &final_dst) {
                Ok(()) => {
                    preserve_metadata(src, &final_dst);
                    return Ok(ItemOutcome::Done);
                }
                Err(e) => {
                    let _ = fs::remove_file(&tmp);
                    return Err(e);
                }
            }
        }
        // Something appeared at a path that was free — treat as a fresh
        // conflict instead of overwriting it.
        match (c.resolve)(src, &final_dst) {
            ConflictAction::Overwrite => {
                existed = true;
                continue;
            }
            ConflictAction::Skip => {
                let _ = fs::remove_file(&tmp);
                return Ok(ItemOutcome::Skipped);
            }
            ConflictAction::KeepBoth => {
                let Some(name) = final_dst.file_name() else {
                    let _ = fs::remove_file(&tmp);
                    return Ok(ItemOutcome::Skipped);
                };
                final_dst = keep_both_path(final_dst.parent().unwrap_or(Path::new("")), name);
                continue;
            }
            ConflictAction::CancelOp => {
                c.cancel.store(true, Ordering::Relaxed);
                let _ = fs::remove_file(&tmp);
                return Ok(ItemOutcome::Cancelled);
            }
        }
    }
    let _ = fs::remove_file(&tmp);
    Err(std::io::Error::other("destination kept changing under us"))
}

/// Recreate `src` symlink at `dst` (link contents preserved, never followed).
fn copy_symlink(src: &Path, dst: &Path, c: &CopyCtx<'_>) -> std::io::Result<ItemOutcome> {
    let Some((dst, _)) = resolve_dest(src, dst, c) else {
        return Ok(if cancelled(c) {
            ItemOutcome::Cancelled
        } else {
            ItemOutcome::Skipped
        });
    };
    let target = fs::read_link(src)?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &dst)?;
    #[cfg(windows)]
    {
        if src.is_dir() {
            std::os::windows::fs::symlink_dir(&target, &dst)?;
        } else {
            std::os::windows::fs::symlink_file(&target, &dst)?;
        }
    }
    Ok(ItemOutcome::Done)
}

/// Copy file/dir/symlink `src` to `dst` (final path), recursing for dirs.
/// Returns `Partial` when children were skipped or failed — callers that
/// delete move sources must consult it (see [`prune_copied`]).
pub fn copy_tree(src: &Path, dst: &Path, c: &CopyCtx<'_>) -> std::io::Result<ItemOutcome> {
    copy_tree_inner(src, dst, c, true)
}

fn copy_tree_inner(
    src: &Path,
    dst: &Path,
    c: &CopyCtx<'_>,
    top_level: bool,
) -> std::io::Result<ItemOutcome> {
    if cancelled(c) {
        return Ok(ItemOutcome::Cancelled);
    }
    let md = fs::symlink_metadata(src)?;
    if md.file_type().is_symlink() {
        let r = copy_symlink(src, dst, c);
        return r.inspect(|&o| {
            if o == ItemOutcome::Done {
                tick_item(c, src);
            }
        });
    }
    if md.is_dir() {
        // A just-created dst dir inside src would be picked up by the
        // read_dir below and recursed into forever. The canonicalized
        // check runs once per top-level item; deeper levels only need the
        // lexical test because dst is built by joining under an already
        // validated parent.
        let self_copy = if top_level {
            is_descendant_or_same(src, dst)
        } else {
            dst == src || dst.starts_with(src)
        };
        if self_copy {
            return Err(std::io::Error::other(format!(
                "refusing to copy {} into itself",
                dst.display()
            )));
        }
        match resolve_dest(src, dst, c) {
            None => {
                return Ok(if cancelled(c) {
                    ItemOutcome::Cancelled
                } else {
                    ItemOutcome::Skipped
                });
            }
            Some((dst, _)) => {
                if !dst_exists(&dst) {
                    fs::create_dir(&dst)?;
                    c.created_dirs.lock().unwrap().push(dst.clone());
                }
                tick_item(c, src);
                let rd = fs::read_dir(src)?;
                let mut overall = ItemOutcome::Done;
                for e in rd.flatten() {
                    if cancelled(c) {
                        return Ok(ItemOutcome::Cancelled);
                    }
                    let child_src = e.path();
                    let child_dst = dst.join(e.file_name());
                    match copy_tree_inner(&child_src, &child_dst, c, false) {
                        Ok(ItemOutcome::Done) => {}
                        // Failed descendants are already in `failed`; the
                        // dir itself is kept only while it still holds them.
                        Ok(ItemOutcome::Partial) => {
                            overall = ItemOutcome::Partial;
                        }
                        Ok(ItemOutcome::Skipped) => {
                            mark_failed(c, &child_src);
                            overall = ItemOutcome::Partial;
                        }
                        Ok(ItemOutcome::Cancelled) => return Ok(ItemOutcome::Cancelled),
                        Err(err) => {
                            note_error(c, format!("{}: {err}", child_src.display()));
                            mark_failed(c, &child_src);
                            overall = ItemOutcome::Partial;
                        }
                    }
                }
                preserve_metadata(src, &dst);
                return Ok(overall);
            }
        }
    }
    // Regular files only. Fifos, sockets, devices and other specials must
    // not be opened (opening a fifo blocks forever) — report and move on.
    if !md.is_file() {
        note_error(c, format!("{}: unsupported special file", src.display()));
        mark_failed(c, src);
        return Ok(ItemOutcome::Partial);
    }
    let r = copy_file(src, dst, c)?;
    if r == ItemOutcome::Done {
        tick_item(c, src);
    }
    Ok(r)
}

/// Delete move sources — but only the parts whose transfer actually
/// succeeded. `ctx.failed` holds source paths that were skipped or errored;
/// they and their ancestors survive. A dir is removed only once emptied.
/// Never touches anything outside `path`'s own tree.
pub fn prune_copied(path: &Path, c: &CopyCtx<'_>) -> std::io::Result<()> {
    let failed = c.failed.lock().unwrap();
    prune_inner(path, &failed).map(|_| ())
}

fn prune_inner(p: &Path, failed: &FxHashSet<PathBuf>) -> std::io::Result<bool> {
    if failed.contains(p) {
        return Ok(true); // kept — failed or skipped
    }
    let md = fs::symlink_metadata(p)?;
    if md.is_dir() && !md.file_type().is_symlink() {
        let mut any_kept = false;
        if let Ok(rd) = fs::read_dir(p) {
            for e in rd.flatten() {
                if prune_inner(&e.path(), failed)? {
                    any_kept = true;
                }
            }
        }
        if !any_kept {
            fs::remove_dir(p)?;
            return Ok(false);
        }
        return Ok(true);
    }
    fs::remove_file(p)?;
    Ok(false)
}

thread_local! {
    /// Test hook: pretend `rename` can't handle this move so the
    /// copy+prune fallback is exercised on a single filesystem.
    static FORCE_COPY_PATH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Test-only fault injection for the cross-volume move path.
#[doc(hidden)]
pub fn set_force_copy_path(on: bool) {
    FORCE_COPY_PATH.with(|f| f.set(on));
}

/// What the rename fast-path decided for one move item.
enum MoveStep {
    /// Move finished via rename (or was skipped/cancelled/answered).
    Resolved(ItemOutcome),
    /// Rename was inappropriate or failed — transfer by copy+prune to
    /// this final destination instead.
    CopyTo(PathBuf),
}

/// Move one item: conflict resolution BEFORE any destructive rename.
/// Same-volume moves stay a single atomic rename; anything else falls
/// back to copy+verified prune. Never silently overwrites.
fn move_item_inner(src: &Path, dst: &Path, c: &CopyCtx<'_>) -> std::io::Result<MoveStep> {
    // Moving an item onto its own exact path is meaningless — reject.
    // This is a lexical compare: a different spelling of the same file
    // (Case.txt → CASE.txt on a case-insensitive fs) is a legitimate
    // rename handled by the `same_file` branch below, not an error.
    if src == dst {
        return Err(std::io::Error::other(format!(
            "cannot move {} onto itself",
            dst.display()
        )));
    }
    // Strict containment — a dir may never move into its own subtree.
    // Canonical equality alone does NOT reject: on case-insensitive
    // filesystems Case.txt and CASE.txt canonicalize to one path, yet
    // the rename between them must proceed (see `same_file` below).
    let s = normalize(src);
    let d = normalize(dst);
    if !paths_equal(&d, &s) && path_starts_with(&d, &s) {
        return Err(std::io::Error::other(format!(
            "cannot move {} — it is inside the source",
            dst.display()
        )));
    }
    if FORCE_COPY_PATH.with(|f| f.get()) {
        return Ok(MoveStep::CopyTo(dst.to_path_buf()));
    }
    if !dst_exists(dst) {
        // Free name: a single rename is atomic and instant on one volume.
        // On failure (cross-volume, perms, …) fall through to copy+prune,
        // which reports the real error.
        return Ok(match fs::rename(src, dst) {
            Ok(()) => MoveStep::Resolved(ItemOutcome::Done),
            Err(_) => MoveStep::CopyTo(dst.to_path_buf()),
        });
    }
    // Destination occupied.
    if same_file(src, dst) {
        // Same object under another name (hardlink, case-insensitive
        // alias): this is a pure rename, not a conflict. If src and dst
        // spell the same path the rename is a harmless no-op.
        return Ok(match fs::rename(src, dst) {
            Ok(()) => MoveStep::Resolved(ItemOutcome::Done),
            Err(_) => MoveStep::Resolved(ItemOutcome::Skipped),
        });
    }
    match (c.resolve)(src, dst) {
        ConflictAction::Skip => Ok(MoveStep::Resolved(ItemOutcome::Skipped)),
        ConflictAction::CancelOp => {
            c.cancel.store(true, Ordering::Relaxed);
            Ok(MoveStep::Resolved(ItemOutcome::Cancelled))
        }
        ConflictAction::Overwrite => match rename_replace(src, dst) {
            Ok(()) => Ok(MoveStep::Resolved(ItemOutcome::Done)),
            // Couldn't complete the swap (e.g. cross-volume): transfer
            // instead. copy_tree re-resolves the still-existing dst.
            Err(_) => Ok(MoveStep::CopyTo(dst.to_path_buf())),
        },
        ConflictAction::KeepBoth => {
            let name = dst
                .file_name()
                .ok_or_else(|| std::io::Error::other("destination has no filename"))?;
            let dst2 = keep_both_path(dst.parent().unwrap_or(Path::new("")), name);
            match fs::rename(src, &dst2) {
                Ok(()) => Ok(MoveStep::Resolved(ItemOutcome::Done)),
                Err(_) => Ok(MoveStep::CopyTo(dst2)),
            }
        }
    }
}

/// Move `src` to `dst`. Returns the outcome; on `Done`/`Partial` a
/// cross-volume fallback has already pruned the copied parts of the
/// source. Errors are top-level failures (e.g. source vanished).
pub fn move_item(src: &Path, dst: &Path, c: &CopyCtx<'_>) -> std::io::Result<ItemOutcome> {
    if cancelled(c) {
        return Ok(ItemOutcome::Cancelled);
    }
    fs::symlink_metadata(src)?; // source must exist
    match move_item_inner(src, dst, c)? {
        MoveStep::Resolved(o) => {
            if o == ItemOutcome::Done {
                tick_item(c, src);
            }
            Ok(o)
        }
        MoveStep::CopyTo(final_dst) => {
            let r = copy_tree(src, &final_dst, c)?;
            match r {
                // Only remove what verifiably transferred. `Partial`
                // keeps every skipped/failed source item in place.
                ItemOutcome::Done | ItemOutcome::Partial => {
                    if let Err(e) = prune_copied(src, c) {
                        note_error(c, format!("cleanup of {}: {e}", src.display()));
                    }
                }
                ItemOutcome::Skipped | ItemOutcome::Cancelled => {}
            }
            Ok(r)
        }
    }
}

/// Remove a tree we fully own, deepest-first, with cancel checks.
/// Used for permanent delete (and nothing else — moves use
/// [`prune_copied`] so partial transfers keep their sources).
pub fn remove_tree(path: &Path, c: &CopyCtx<'_>) -> std::io::Result<()> {
    let md = fs::symlink_metadata(path)?;
    if md.is_dir() && !md.file_type().is_symlink() {
        // Post-order iterative traversal.
        let mut stack = vec![(path.to_path_buf(), false)];
        while let Some((p, visited)) = stack.pop() {
            if cancelled(c) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "cancelled",
                ));
            }
            if visited {
                fs::remove_dir(&p)?;
                set_state(c, |p2| p2.done_items += 1);
                continue;
            }
            stack.push((p.clone(), true));
            if let Ok(rd) = fs::read_dir(&p) {
                for e in rd.flatten() {
                    let cp = e.path();
                    let ft = e.file_type();
                    match ft {
                        Ok(t) if t.is_dir() && !t.is_symlink() => stack.push((cp, false)),
                        _ => {
                            if let Err(err) = fs::remove_file(&cp) {
                                note_error(c, format!("{}: {err}", cp.display()));
                            }
                            set_state(c, |p2| p2.done_items += 1);
                        }
                    }
                }
            }
        }
        Ok(())
    } else {
        fs::remove_file(path)
    }
}

/// First free sibling path for duplication: "name copy.ext", then
/// "name copy 2.ext", … Dirs get no extension part: "name copy".
pub fn duplicate_dst(src: &Path) -> PathBuf {
    let parent = src.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let name = src.file_name().unwrap_or_default();
    // Dirs (and ext-less names): stem = whole name, ext = none.
    let (stem, ext) = if src.is_dir() {
        (name.to_string_lossy().into_owned(), String::new())
    } else {
        let p = Path::new(name);
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.to_string_lossy().into_owned());
        let ext = p
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        (stem, ext)
    };
    for n in 0..1000u32 {
        let candidate = if n == 0 {
            parent.join(format!("{stem} copy{ext}"))
        } else {
            parent.join(format!("{stem} copy {}{}", n + 1, ext))
        };
        if !candidate.try_exists().unwrap_or(true) {
            return candidate;
        }
    }
    parent.join(format!("{stem} copy {ext}"))
}

/// After a cancel/failure, drop in-flight temp dirs we created (deepest
/// first). Never removes anything that existed before the op.
pub fn cleanup_created(c: &CopyCtx<'_>) {
    let mut dirs = c.created_dirs.lock().unwrap().drain(..).collect::<Vec<_>>();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        // remove_dir_all only on dirs we created: they contain only our files.
        let _ = fs::remove_dir_all(&d);
    }
}
