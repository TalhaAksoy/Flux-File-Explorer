//! "Compress…" — zip archive creation on the ops worker.
//!
//! Entries keep their top-level names: compressing `~/docs` yields
//! `docs/…` paths inside the archive; a lone `a.txt` yields `a.txt`.
//! Follows symlinks (stores target content), same as Finder/Explorer.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::copy::{cancelled, note_error, tick_item, CopyCtx};

const CHUNK: usize = 256 * 1024;

fn opts() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true)
}

pub fn compress_items(items: &[PathBuf], dest: &Path, c: &CopyCtx<'_>) -> io::Result<()> {
    let file = File::create(dest)?;
    let mut zw = ZipWriter::new(file);
    for item in items {
        if cancelled(c) {
            break;
        }
        if let Err(e) = write_entry(&mut zw, item, item, c) {
            note_error(c, format!("compress {}: {e}", item.display()));
        }
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
    // Follow symlinks — archives hold content, not links.
    let md = std::fs::metadata(path)?;
    if md.is_dir() {
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
        let mut wrote = 0u64;
        loop {
            if cancelled(c) {
                return Ok(());
            }
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            io::Write::write_all(&mut *zw, &buf[..n])?;
            wrote += n as u64;
            if let Ok(mut p) = c.progress.lock() {
                p.done_bytes += n as u64;
            }
        }
        tick_item(c, path, wrote);
    }
    Ok(())
}
