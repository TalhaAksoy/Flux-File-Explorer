use std::path::PathBuf;

#[derive(Debug)]
pub enum TrashError {
    /// The platform has no trash mechanism for this path (e.g. some network
    /// mounts). Caller must NOT silently fall back to permanent delete.
    Unsupported(String),
    Failed(String),
}

impl std::fmt::Display for TrashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrashError::Unsupported(m) | TrashError::Failed(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for TrashError {}

/// Move items to the OS Trash / Recycle Bin. Items are processed
/// independently; returns per-item failures so a partial trash keeps the
/// untouched sources intact and reported.
pub fn trash_items(items: &[PathBuf]) -> (usize, Vec<(PathBuf, TrashError)>) {
    let mut ok = 0usize;
    let mut errs = Vec::new();
    for p in items {
        match trash::delete(p) {
            Ok(()) => ok += 1,
            Err(e) => errs.push((
                p.clone(),
                TrashError::Failed(format!("trash failed: {e}")),
            )),
        }
    }
    (ok, errs)
}
