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
