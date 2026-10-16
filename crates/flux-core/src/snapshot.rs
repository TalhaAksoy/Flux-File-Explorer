use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::EntryIdx;
use crate::entry::DirEntry;
use crate::sort::SortSpec;

#[derive(Clone, Debug)]
pub enum LoadState {
    /// No enumeration running (fresh snapshot).
    Idle,
    /// Enumeration in progress; `received` entries arrived so far.
    Loading { received: usize },
    /// Enumeration complete; `view` is globally sorted per `sort`.
    Ready { total: usize },
    /// Enumeration failed.
    Failed(String),
    /// Superseded by a newer navigation request.
    Cancelled,
}

impl LoadState {
    pub fn is_loading(&self) -> bool {
        matches!(self, LoadState::Loading { .. })
    }
}

/// One directory's contents plus the ordered, filtered view into them.
///
/// `entries` is an append-only arena: indices are stable [`EntryIdx`]s and are
/// used as identities by selection. `view` holds entry indices in display
/// order. During loading, `view` is in arrival order (filtered); when the scan
/// finishes the compute engine rebuilds it fully sorted.
#[derive(Debug)]
pub struct DirSnapshot {
    pub path: PathBuf,
    entries: Vec<DirEntry>,
    view: Vec<EntryIdx>,
    pub state: LoadState,
    pub sort: SortSpec,
    pub filter: Option<String>,
    pub show_hidden: bool,
    /// Show hidden+system "protected operating system files" (Windows).
    /// Independent of `show_hidden`, mirroring Explorer's separate option.
    pub show_system: bool,
    /// Generation of the enumeration filling this snapshot.
    pub generation: u64,
    /// Bumped on every mutation; lets the UI detect changes cheaply.
    version: u64,
}

pub type SnapshotHandle = Arc<RwLock<DirSnapshot>>;

/// One entry's refreshed metadata, produced by the deferred stat worker.
/// `hidden`/`system` come along for free on Windows (file attributes ride
/// in the same metadata call); elsewhere `system` is always false.
pub struct MetaFill {
    pub idx: EntryIdx,
    pub size: u64,
    pub mtime: Option<std::time::SystemTime>,
    pub readonly: bool,
    pub hidden: bool,
    pub system: bool,
}

impl DirSnapshot {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            entries: Vec::new(),
            view: Vec::new(),
            state: LoadState::Idle,
            sort: SortSpec::default(),
            filter: None,
            show_hidden: false,
            show_system: false,
            generation: 0,
            version: 0,
        }
    }

    pub fn handle(path: PathBuf) -> SnapshotHandle {
        Arc::new(RwLock::new(Self::new(path)))
    }

    /// Reset for a new enumeration of `path`.
    pub fn begin_load(&mut self, path: PathBuf, generation: u64) {
        self.path = path;
        self.entries.clear();
        self.view.clear();
        self.state = LoadState::Loading { received: 0 };
        self.generation = generation;
        self.version += 1;
    }

    /// Append a batch of freshly enumerated entries. Their indices are
    /// appended to the view when they pass the current filter/hidden policy
    /// (arrival order while loading).
    pub fn push_batch(&mut self, batch: &mut Vec<DirEntry>) {
        let base = self.entries.len() as EntryIdx;
        self.entries.append(batch);
        let filter = self.filter.as_deref();
        let show_hidden = self.show_hidden;
        let show_system = self.show_system;
        self.view
            .extend((base..self.entries.len() as EntryIdx).filter(|&i| {
                let e = &self.entries[i as usize];
                e.visible(show_hidden, show_system) && e.matches_filter(filter)
            }));
        self.state = LoadState::Loading {
            received: self.entries.len(),
        };
        self.version += 1;
    }

    /// Replace the view (called by the compute engine).
    /// `done` marks the scan finished → the view is now globally sorted.
    pub fn set_view(&mut self, view: Vec<EntryIdx>) {
        self.view = view;
        self.version += 1;
    }

    /// Apply background-statted metadata to entries in place. Indices are
    /// stable (entries are append-only), so this is safe mid-view. The
    /// version bump lets in-flight compute work notice fresher data.
    pub fn apply_meta(&mut self, items: &[MetaFill]) {
        for it in items {
            if let Some(e) = self.entries.get_mut(it.idx as usize) {
                e.size = it.size;
                e.mtime = it.mtime;
                e.readonly = it.readonly;
                e.hidden = it.hidden;
                e.system = it.system;
            }
        }
        self.version += 1;
    }

    pub fn finish(&mut self) {
        self.state = LoadState::Ready {
            total: self.entries.len(),
        };
        self.version += 1;
    }

    pub fn fail(&mut self, message: String) {
        self.state = LoadState::Failed(message);
        self.version += 1;
    }

    pub fn cancel(&mut self) {
        self.state = LoadState::Cancelled;
        self.version += 1;
    }

    pub fn entry(&self, idx: EntryIdx) -> Option<&DirEntry> {
        self.entries.get(idx as usize)
    }

    /// Entry at a visible row (view position).
    pub fn row(&self, row: usize) -> Option<(EntryIdx, &DirEntry)> {
        self.view.get(row).map(|&i| (i, &self.entries[i as usize]))
    }

    pub fn row_index(&self, row: usize) -> Option<EntryIdx> {
        self.view.get(row).copied()
    }

    pub fn entries(&self) -> &[DirEntry] {
        &self.entries
    }

    pub fn view(&self) -> &[EntryIdx] {
        &self.view
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn view_len(&self) -> usize {
        self.view.len()
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Absolute path of an entry without allocating a `PathBuf` for display:
    /// `path` + `name`. Used by operations, never by pure rendering.
    pub fn entry_path(&self, idx: EntryIdx) -> Option<PathBuf> {
        self.entries
            .get(idx as usize)
            .map(|e| self.path.join(&e.name))
    }

    /// Estimated heap+struct footprint in bytes (for diagnostics/benchmarks).
    pub fn memory_estimate(&self) -> usize {
        let names: usize = self
            .entries
            .iter()
            .map(|e| std::mem::size_of_val(e) + e.name.len())
            .sum();
        names + self.view.len() * std::mem::size_of::<EntryIdx>() + std::mem::size_of::<Self>()
    }
}
