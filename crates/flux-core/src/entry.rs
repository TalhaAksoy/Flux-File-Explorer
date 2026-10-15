use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::time::SystemTime;


/// What kind of filesystem object an entry is. Determined without following
/// symlinks: a symlink reports [`EntryKind::Symlink`], not its target's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    Dir,
    File,
    Symlink,
    /// Fifo, socket, device, or undetermined.
    Other,
}

/// One directory entry. `size` is [`UNKNOWN_SIZE`] until fetched; directory
/// sizes are never computed implicitly.
#[derive(Clone, Debug)]
pub struct DirEntry {
    pub name: OsString,
    pub kind: EntryKind,
    pub size: u64,
    pub mtime: Option<SystemTime>,
    pub readonly: bool,
    pub hidden: bool,
}

impl DirEntry {
    pub fn extension(&self) -> Option<&OsStr> {
        if self.kind == EntryKind::Dir {
            return None;
        }
        Path::new(&self.name).extension()
    }

    /// Display name, lossy for non-UTF-8 names. Filesystem operations must use
    /// `name` (OsString), never this string.
    pub fn display_name(&self) -> std::borrow::Cow<'_, str> {
        self.name.to_string_lossy()
    }

    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }

    /// Whether the name passes the current substring filter. `None` filter
    /// matches everything.
    pub fn matches_filter(&self, filter: Option<&str>) -> bool {
        let Some(f) = filter else { return true };
        if f.is_empty() {
            return true;
        }
        contains_case_insensitive(&self.name.to_string_lossy(), f)
    }
}

/// Case-insensitive substring check with no allocation in the common path.
/// Falls back to a lowercase copy of `hay` only when `needle` contains no
/// bytes that can match literally — i.e., when a plain `contains` misses but
/// a case-folded one might hit.
pub fn contains_case_insensitive(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if hay.contains(needle) {
        return true;
    }
    // Fast path: if needle is pure ASCII, compare without allocating a
    // lowercased copy of the whole haystack when hay is short; for long
    // haystacks one allocation is cheaper than a custom fold loop.
    if needle.is_ascii() && hay.is_ascii() {
        return hay.to_ascii_lowercase().contains(&needle.to_ascii_lowercase());
    }
    let hay_l = hay.to_lowercase();
    let needle_l = needle.to_lowercase();
    hay_l.contains(&needle_l)
}

/// True if a filesystem name should be considered hidden.
/// Unix: leading '.'. Windows hidden attribute is detected via metadata.
pub fn is_hidden_name(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}
