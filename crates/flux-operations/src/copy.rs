//! Safe copy/move primitives.
//!
//! Invariants:
//! - file copies go to a sibling `*.fluxpart-*` temp then rename into place —
//!   a cancelled/failed copy never leaves a complete-looking destination.
//! - `Move` tries `rename` first (atomic, instant on one volume); on any
//!   failure it copies, verifies sizes, then removes the source.
//! - cancelling removes in-flight temp files and dirs this op created.
//! - symlink sources are recreated as symlinks, never followed into.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemOutcome {
    Done,
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

pub fn tick_item(c: &CopyCtx<'_>, path: &Path, bytes: u64) {
    set_state(c, |p| {
        p.done_items += 1;
        p.done_bytes += bytes;
        p.current_item = path.to_path_buf();
    });
}

pub fn note_error(c: &CopyCtx<'_>, msg: String) {
    set_state(c, |p| p.record_error(msg));
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

/// A free staging path next to `dst` for a same-volume temp write.
fn staging_path(dst: &Path, tag: u64) -> PathBuf {
    let mut name = dst.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".fluxpart-{tag}"));
    dst.with_file_name(name)
}

/// Copy one file to `tmp`, chunk-loop with cancel + byte progress.
/// On failure/cancel the caller removes `tmp`.
fn copy_file_chunks(src: &Path, tmp: &Path, c: &CopyCtx<'_>) -> std::io::Result<u64> {
    let mut input = fs::File::open(src)?;
    let mut output = fs::File::create(tmp)?;
    let mut buf = vec![0u8; COPY_BUF];
    let mut written = 0u64;
    loop {
        if cancelled(c) {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        output.write_all(&buf[..n])?;
        written += n as u64;
        set_state(c, |p| p.done_bytes += n as u64);
    }
    output.sync_all()?;
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

/// Resolve where `dst` should actually be written given the conflict policy.
/// Returns `None` for skip/cancel (caller distinguishes via `cancelled`).
fn resolve_dest(src: &Path, dst: &Path, c: &CopyCtx<'_>) -> Option<PathBuf> {
    if !dst.exists() {
        return Some(dst.to_path_buf());
    }
    let action = (c.resolve)(src, dst);
    match action {
        ConflictAction::Overwrite => Some(dst.to_path_buf()),
        ConflictAction::Skip => None,
        ConflictAction::KeepBoth => {
            let name = dst.file_name()?;
            Some(keep_both_path(dst.parent()?, name))
        }
        ConflictAction::CancelOp => {
            c.cancel.store(true, Ordering::Relaxed);
            None
        }
    }
}

/// Copy a single file `src` to `dst` (final path). Handles conflict
/// resolution, staging, verification metadata.
pub fn copy_file(src: &Path, dst: &Path, c: &CopyCtx<'_>) -> std::io::Result<ItemOutcome> {
    let Some(dst) = resolve_dest(src, dst, c) else {
        return Ok(if cancelled(c) {
            ItemOutcome::Cancelled
        } else {
            ItemOutcome::Skipped
        });
    };
    let src_len = fs::symlink_metadata(src).map(|m| m.len()).unwrap_or(0);
    let tmp = staging_path(&dst, c.tag);
    let res = if src_len <= SMALL_FILE_FAST_PATH {
        fs::copy(src, &tmp).inspect(|&n| {
            set_state(c, |p| p.done_bytes += n);
        })
    } else {
        copy_file_chunks(src, &tmp, c)
    };
    match res {
        Ok(n) => {
            if n != src_len {
                let _ = fs::remove_file(&tmp);
                return Err(std::io::Error::other(format!(
                    "size mismatch: {} bytes written, expected {src_len}",
                    n
                )));
            }
            fs::rename(&tmp, &dst)?;
            preserve_metadata(src, &dst);
            Ok(ItemOutcome::Done)
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Recreate `src` symlink at `dst` (link contents preserved, never followed).
fn copy_symlink(src: &Path, dst: &Path, c: &CopyCtx<'_>) -> std::io::Result<ItemOutcome> {
    let Some(dst) = resolve_dest(src, dst, c) else {
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
/// Returns Ok(ItemOutcome) or Err for a hard failure of the top-level item.
pub fn copy_tree(src: &Path, dst: &Path, c: &CopyCtx<'_>) -> std::io::Result<ItemOutcome> {
    if cancelled(c) {
        return Ok(ItemOutcome::Cancelled);
    }
    let md = fs::symlink_metadata(src)?;
    if md.file_type().is_symlink() {
        let r = copy_symlink(src, dst, c);
        return r.inspect(|&o| {
            if o == ItemOutcome::Done {
                tick_item(c, src, 0);
            }
        });
    }
    if md.is_dir() {
        match resolve_dest(src, dst, c) {
            None => {
                return Ok(if cancelled(c) {
                    ItemOutcome::Cancelled
                } else {
                    ItemOutcome::Skipped
                });
            }
            Some(dst) => {
                if !dst.exists() {
                    fs::create_dir(&dst)?;
                    c.created_dirs.lock().unwrap().push(dst.clone());
                }
                tick_item(c, src, 0);
                let rd = fs::read_dir(src)?;
                let mut overall = ItemOutcome::Done;
                for e in rd.flatten() {
                    if cancelled(c) {
                        return Ok(ItemOutcome::Cancelled);
                    }
                    let child_src = e.path();
                    let child_dst = dst.join(e.file_name());
                    match copy_tree(&child_src, &child_dst, c) {
                        Ok(ItemOutcome::Done) | Ok(ItemOutcome::Skipped) => {}
                        Ok(ItemOutcome::Cancelled) => return Ok(ItemOutcome::Cancelled),
                        Err(err) => {
                            note_error(c, format!("{}: {err}", child_src.display()));
                            overall = ItemOutcome::Done; // keep going; counted as error
                        }
                    }
                }
                preserve_metadata(src, &dst);
                return Ok(overall);
            }
        }
    }
    // Regular file / other.
    let r = copy_file(src, dst, c)?;
    if r == ItemOutcome::Done {
        tick_item(c, src, md.len());
    }
    Ok(r)
}

/// Remove a tree we fully own, deepest-first, with cancel checks.
/// Used to delete move sources after verified copies, and for permanent delete.
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
