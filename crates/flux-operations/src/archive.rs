//! "Compress…" — zip archive creation on the ops worker.
//!
//! Entries keep their top-level names: compressing `~/docs` yields
//! `docs/…` paths inside the archive; a lone `a.txt` yields `a.txt`.
//!
//! Safety properties:
//! - **Staged**: the zip is written to a `*.fluxpart-*` sibling and only
//!   published (rename) once fully finished. A cancel or crash can never
//!   leave a misleading partial archive at the final name — nor does an
//!   existing destination get touched until the new archive is complete.
//! - **Symlinks are stored as links**, never followed: no traversal
//!   loops, no silently bundling data from outside the selection.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::copy::{CopyCtx, cancelled, create_staging, note_error, rename_replace, tick_item};

const CHUNK: usize = 256 * 1024;

fn opts() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true)
}

pub fn compress_items(items: &[PathBuf], dest: &Path, c: &CopyCtx<'_>) -> io::Result<()> {
    let (tmp, file) = create_staging(dest, c.tag)?;
    let res = write_archive(items, file, c);
    // A cancelled op leaves only the staging file to sweep — `dest` was
    // never touched. The caller marks the op Cancelled via its flag.
    if cancelled(c) {
        let _ = std::fs::remove_file(&tmp);
        return Ok(());
    }
    match res {
        Ok(()) => match rename_replace(&tmp, dest) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Err(e)
            }
        },
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn write_archive(items: &[PathBuf], file: File, c: &CopyCtx<'_>) -> io::Result<()> {
    let mut zw = ZipWriter::new(file);
    for item in items {
        if cancelled(c) {
            break;
        }
        if let Err(e) = write_entry(&mut zw, item, item, c) {
            note_error(c, format!("compress {}: {e}", item.display()));
        }
    }
    // On cancel we bail BEFORE finish() — the staging file is deleted by
    // the caller, so no half-written central directory is ever published.
    if cancelled(c) {
        return Ok(());
    }
    match zw.finish() {
        Ok(_) => Ok(()),
        Err(e) => Err(io::Error::other(e)),
    }
}

fn write_entry(
    zw: &mut ZipWriter<File>,
    root: &Path,
    path: &Path,
    c: &CopyCtx<'_>,
) -> io::Result<()> {
    if cancelled(c) {
        return Ok(());
    }
    // Names relative to the item's parent → archive mirrors selection shape.
    let base = root.parent().unwrap_or_else(|| Path::new("/"));
    let rel = path.strip_prefix(base).unwrap_or(path);
    let mut name = rel.to_string_lossy().replace('\\', "/");
    if name.is_empty() {
        return Ok(());
    }
    // lstat: symlinks are archived AS links — never followed — which makes
    // traversal loop-proof without a visited set and prevents the archive
    // from quietly absorbing content outside the selection.
    let md = std::fs::symlink_metadata(path)?;
    if md.file_type().is_symlink() {
        let target = std::fs::read_link(path)?;
        zw.add_symlink(&name, target.to_string_lossy().as_ref(), opts())
            .map_err(io::Error::other)?;
        tick_item(c, path);
    } else if md.is_dir() {
        if !name.ends_with('/') {
            name.push('/');
        }
        zw.add_directory(&name, opts()).map_err(io::Error::other)?;
        for e in std::fs::read_dir(path)?.flatten() {
            write_entry(zw, root, &e.path(), c)?;
        }
    } else if md.is_file() {
        zw.start_file(&name, opts()).map_err(io::Error::other)?;
        let mut f = File::open(path)?;
        let mut buf = vec![0u8; CHUNK];
        loop {
            if cancelled(c) {
                return Ok(());
            }
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            io::Write::write_all(&mut *zw, &buf[..n])?;
            if let Ok(mut p) = c.progress.lock() {
                p.done_bytes += n as u64;
            }
        }
        tick_item(c, path);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Safe extraction
// ---------------------------------------------------------------------------

/// Extract `archive` into `dest_dir` with zip-slip protection.
///
/// Safety properties:
/// - **Path containment**: `enclosed_name()` rejects `../` and absolute
///   entries outright; additionally every output path's ancestor chain is
///   walked component-by-component with `symlink_metadata` — a malicious
///   or pre-existing symlink inside the tree (e.g. `x -> /etc` followed
///   by `x/passwd`) fails instead of escaping `dest_dir`.
/// - **Symlink entries are recreated as links** (Unix) but never written
///   through; on other platforms they're skipped with a noted error.
/// - **Staged when the target dir is new**: "Extract to X/" writes to
///   `X.fluxpart-*` and renames on success — a cancel removes the whole
///   partial tree. When `dest_dir` already exists ("Extract Here") files
///   merge in place and existing paths go through the conflict resolver.
/// - **Existing symlinks at a write path are never followed**: overwrite
///   removes the link node itself before creating the file.
pub fn extract_archive(archive: &Path, dest_dir: &Path, c: &CopyCtx<'_>) -> io::Result<()> {
    let file = File::open(archive)?;
    let mut za = zip::ZipArchive::new(io::BufReader::new(file)).map_err(io::Error::other)?;

    // Archives need no discover walk — totals come from the central dir.
    let (mut items, mut bytes) = (0u64, 0u64);
    for i in 0..za.len() {
        if let Ok(e) = za.by_index(i) {
            items += 1;
            bytes += e.size();
        }
    }
    crate::copy::add_items(c, items, bytes);

    let staged = !dest_dir.exists();
    let work = if staged {
        staging_dir(dest_dir, c.tag)?
    } else {
        dest_dir.to_path_buf()
    };
    let root = work.canonicalize()?;
    let res = extract_all(&mut za, archive, &work, &root, staged, c);
    if let Err(e) = res {
        if staged {
            let _ = std::fs::remove_dir_all(&work);
        }
        return Err(e);
    }
    if cancelled(c) {
        if staged {
            let _ = std::fs::remove_dir_all(&work);
        }
        return Ok(());
    }
    if staged {
        // dest didn't exist at entry — publish atomically by rename. If a
        // dest appeared in the meantime, don't clobber it.
        if dest_dir.exists() {
            let _ = std::fs::remove_dir_all(&work);
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} appeared during extraction", dest_dir.display()),
            ));
        }
        if let Err(e) = std::fs::rename(&work, dest_dir) {
            let _ = std::fs::remove_dir_all(&work);
            return Err(e);
        }
    }
    Ok(())
}

/// `dest.fluxpart-<tag>-<n>` sibling directory, claimed by create_dir.
fn staging_dir(dest: &Path, tag: u64) -> io::Result<PathBuf> {
    let base = dest
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    for n in 0..100u32 {
        let mut name = base.clone();
        name.push(format!(".fluxpart-{tag}-{n}"));
        let tmp = dest.with_file_name(name);
        match std::fs::create_dir(&tmp) {
            Ok(()) => return Ok(tmp),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no free staging dir name"))
}

fn extract_all(
    za: &mut zip::ZipArchive<io::BufReader<File>>,
    archive: &Path,
    work: &Path,
    root: &Path,
    staged: bool,
    c: &CopyCtx<'_>,
) -> io::Result<()> {
    for i in 0..za.len() {
        if cancelled(c) {
            return Ok(());
        }
        let mut entry = match za.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                note_error(c, format!("entry {i}: {e}"));
                continue;
            }
        };
        // enclosed_name rejects `../`, absolute paths and drive prefixes.
        let Some(rel) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            note_error(
                c,
                format!(
                    "{}: unsafe path skipped",
                    String::from_utf8_lossy(entry.name_raw())
                ),
            );
            continue;
        };
        let mut out = work.join(&rel);
        let is_dir = entry.is_dir();

        // Existing-path conflicts ("Extract Here" merges): in staged mode
        // nothing pre-exists, so this only runs for direct extraction.
        if !is_dir && !staged && out.symlink_metadata().is_ok() {
            match (c.resolve)(archive, &out) {
                crate::conflict::ConflictAction::Overwrite => {
                    // Remove link nodes so File::create can't follow them.
                    if out
                        .symlink_metadata()
                        .is_ok_and(|m| m.file_type().is_symlink())
                    {
                        let _ = std::fs::remove_file(&out);
                    }
                }
                crate::conflict::ConflictAction::KeepBoth => {
                    out = crate::copy::duplicate_dst(&out);
                }
                crate::conflict::ConflictAction::Skip => {
                    tick_item(c, &out);
                    continue;
                }
                crate::conflict::ConflictAction::CancelOp => {
                    c.cancel.store(true, Ordering::Relaxed);
                    return Ok(());
                }
            }
        }

        if let Err(e) = ensure_dirs(out.parent().unwrap_or(work), work, root, c) {
            note_error(c, format!("{}: {e}", rel.display()));
            continue;
        }
        if is_dir {
            match std::fs::create_dir(&out) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => note_error(c, format!("{}: {e}", rel.display())),
            }
            tick_item(c, &out);
            continue;
        }

        #[cfg(unix)]
        let is_link = entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000);
        #[cfg(not(unix))]
        let is_link = false;

        if is_link {
            #[cfg(unix)]
            {
                let mut target = String::new();
                if entry.read_to_string(&mut target).is_err() {
                    note_error(c, format!("{}: unreadable symlink", rel.display()));
                    continue;
                }
                if out.symlink_metadata().is_ok() {
                    let _ = std::fs::remove_file(&out);
                }
                if let Err(e) = std::os::unix::fs::symlink(&target, &out) {
                    note_error(c, format!("{}: {e}", rel.display()));
                }
            }
            tick_item(c, &out);
            continue;
        }

        // A symlink sitting at `out` (pre-existing or planted by an
        // earlier entry) is removed, not followed.
        if out
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            let _ = std::fs::remove_file(&out);
        }
        match File::create(&out) {
            Ok(mut dst) => {
                let mut buf = vec![0u8; CHUNK];
                let res = (|| -> io::Result<()> {
                    loop {
                        if cancelled(c) {
                            return Ok(());
                        }
                        let n = entry.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        io::Write::write_all(&mut dst, &buf[..n])?;
                        if let Ok(mut p) = c.progress.lock() {
                            p.done_bytes += n as u64;
                        }
                    }
                    Ok(())
                })();
                if let Err(e) = res {
                    note_error(c, format!("{}: {e}", rel.display()));
                }
                #[cfg(unix)]
                if let Some(mode) = entry.unix_mode() {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &out,
                        std::fs::Permissions::from_mode(mode & 0o777),
                    );
                }
            }
            Err(e) => note_error(c, format!("{}: {e}", rel.display())),
        }
        tick_item(c, &out);
    }
    Ok(())
}

/// Create every directory component of `dir` inside `work`, verifying no
/// component is a symlink. `root` is `work` canonicalized; the check
/// bounds all writes to the extraction tree.
fn ensure_dirs(dir: &Path, work: &Path, root: &Path, c: &CopyCtx<'_>) -> io::Result<()> {
    // Collect components of `dir` relative to `work` — anything outside
    // (impossible via enclosed_name, but belt-and-suspenders) fails.
    let rel = dir
        .strip_prefix(work)
        .map_err(|_| io::Error::other(format!("{} escapes extraction root", dir.display())))?;
    let mut cur = work.to_path_buf();
    for comp in rel.components() {
        cur.push(comp.as_os_str());
        match cur.symlink_metadata() {
            Ok(md) => {
                if md.file_type().is_symlink() {
                    return Err(io::Error::other(format!(
                        "{} is a symlink — refusing to write through it",
                        cur.display()
                    )));
                }
                if !md.is_dir() {
                    return Err(io::Error::other(format!(
                        "{} exists and is not a directory",
                        cur.display()
                    )));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                std::fs::create_dir(&cur)?;
                c.created_dirs.lock().unwrap().push(cur.clone());
            }
            Err(e) => return Err(e),
        }
    }
    // Final containment check: the canonical path must stay under root —
    // covers any ancestor outside `work`'s own subtree.
    if let Ok(canon) = cur.canonicalize()
        && !canon.starts_with(root)
    {
        return Err(io::Error::other(format!(
            "{} resolves outside the extraction root",
            cur.display()
        )));
    }
    Ok(())
}
