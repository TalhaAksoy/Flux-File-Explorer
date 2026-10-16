//! Per-tab navigation state: snapshot handle, history, selection, filter.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;

use flux_core::snapshot::{DirSnapshot, SnapshotHandle};
use flux_core::{EntryIdx, SortSpec};
use rustc_hash::FxHashSet;

/// Selection is by entry index (stable within a snapshot's append-only
/// arena). `anchor`/`focus` are VIEW rows for range selection.
#[derive(Default)]
pub struct Selection {
    pub ids: FxHashSet<EntryIdx>,
    /// View row where the last non-shift selection began.
    pub anchor: Option<usize>,
    /// Keyboard focus row.
    pub focus: Option<usize>,
}

impl Selection {
    pub fn clear(&mut self) {
        self.ids.clear();
        self.anchor = None;
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn contains(&self, idx: EntryIdx) -> bool {
        self.ids.contains(&idx)
    }

    /// Plain click: single-select `row`'s entry.
    pub fn select_row(&mut self, snap: &DirSnapshot, row: usize) {
        self.ids.clear();
        if let Some(idx) = snap.row_index(row) {
            self.ids.insert(idx);
        }
        self.anchor = Some(row);
        self.focus = Some(row);
    }

    /// Ctrl/Cmd-click: toggle one entry.
    pub fn toggle_row(&mut self, snap: &DirSnapshot, row: usize) {
        if let Some(idx) = snap.row_index(row)
            && !self.ids.remove(&idx)
        {
            self.ids.insert(idx);
        }
        self.anchor = Some(row);
        self.focus = Some(row);
    }

    /// Shift-click: select the view range anchor..=row.
    pub fn range_to(&mut self, snap: &DirSnapshot, row: usize) {
        let Some(anchor) = self.anchor else {
            self.select_row(snap, row);
            return;
        };
        let (lo, hi) = if anchor <= row {
            (anchor, row)
        } else {
            (row, anchor)
        };
        self.ids.clear();
        for r in lo..=hi {
            if let Some(idx) = snap.row_index(r) {
                self.ids.insert(idx);
            }
        }
        self.focus = Some(row);
    }

    /// Extend selection by keyboard (shift+arrows): grows from anchor to row.
    pub fn extend_to(&mut self, snap: &DirSnapshot, row: usize) {
        let anchor = self.anchor.unwrap_or(row);
        let (lo, hi) = if anchor <= row {
            (anchor, row)
        } else {
            (row, anchor)
        };
        self.ids.clear();
        for r in lo..=hi {
            if let Some(idx) = snap.row_index(r) {
                self.ids.insert(idx);
            }
        }
        self.focus = Some(row);
    }

    /// Names of selected entries — used to re-select after a refresh (indices
    /// are not stable across enumerations).
    pub fn selected_names(&self, snap: &DirSnapshot) -> HashSet<OsString> {
        self.ids
            .iter()
            .filter_map(|&i| snap.entry(i).map(|e| e.name.clone()))
            .collect()
    }

    /// Restore selection by name after a re-enumeration.
    pub fn reselect_names(&mut self, snap: &DirSnapshot, names: &HashSet<OsString>) {
        self.ids.clear();
        for (row, &idx) in snap.view().iter().enumerate() {
            if let Some(e) = snap.entry(idx)
                && names.contains(&e.name)
            {
                self.ids.insert(idx);
                let _ = row;
            }
        }
    }

    pub fn select_all(&mut self, snap: &DirSnapshot) {
        self.ids = snap.view().iter().copied().collect();
    }
}

/// Back/forward navigation history.
#[derive(Default)]
pub struct NavHistory {
    stack: Vec<PathBuf>,
    pos: usize,
}

impl NavHistory {
    pub fn push(&mut self, path: PathBuf) {
        // Truncate forward entries then append.
        self.stack.truncate(self.pos + 1);
        if self.stack.last() != Some(&path) {
            self.stack.push(path);
            self.pos = self.stack.len() - 1;
        }
    }

    pub fn back(&mut self) -> Option<PathBuf> {
        if self.pos > 0 {
            self.pos -= 1;
            self.stack.get(self.pos).cloned()
        } else {
            None
        }
    }

    pub fn forward(&mut self) -> Option<PathBuf> {
        if self.pos + 1 < self.stack.len() {
            self.pos += 1;
            self.stack.get(self.pos).cloned()
        } else {
            None
        }
    }

    pub fn can_back(&self) -> bool {
        self.pos > 0
    }

    pub fn can_forward(&self) -> bool {
        self.pos + 1 < self.stack.len()
    }

    pub fn current(&self) -> Option<&PathBuf> {
        self.stack.get(self.pos)
    }
}

/// Everything a tab needs. The expensive part (entries) lives in the shared
/// snapshot; this struct is small.
pub struct Tab {
    pub snap: SnapshotHandle,
    pub sel: Selection,
    pub history: NavHistory,
    pub gen_id: u64,
    pub filter: String,
    /// Set on refresh: re-apply after enumeration finishes.
    pub reselect_after_load: Option<HashSet<OsString>>,
    /// Names awaiting reselection once the rebuilt view arrives.
    pub pending_reselect: Option<HashSet<OsString>>,
    /// Delay enumeration until first activation (session restore).
    pub deferred_load: bool,
    /// Scroll target for keyboard navigation.
    pub scroll_to_row: Option<usize>,
    /// A watcher change arrived while this tab's enumeration was still
    /// running. Restarting the scan on every burst would starve a large,
    /// actively-written directory forever, so one trailing refresh runs
    /// when the in-flight enumeration finishes instead.
    pub watcher_refresh_pending: bool,
}

impl Tab {
    pub fn new(path: PathBuf) -> Self {
        let mut history = NavHistory::default();
        history.push(path.clone());
        let mut snap = DirSnapshot::new(path);
        Self::apply_default_sort(&mut snap);
        Self {
            snap: SnapshotHandle::new(std::sync::RwLock::new(snap)),
            sel: Selection::default(),
            history,
            gen_id: 0,
            filter: String::new(),
            reselect_after_load: None,
            pending_reselect: None,
            deferred_load: false,
            scroll_to_row: None,
            watcher_refresh_pending: false,
        }
    }

    /// Lazy tab for session restore: knows its path, loads on activation.
    pub fn deferred(path: PathBuf, sort: SortSpec, show_hidden: bool, show_system: bool) -> Self {
        let mut t = Self::new(path);
        {
            let mut s = t.snap.write().unwrap();
            s.sort = sort;
            s.show_hidden = show_hidden;
            s.show_system = show_system;
        }
        t.deferred_load = true;
        t
    }

    fn apply_default_sort(snap: &mut DirSnapshot) {
        snap.sort = SortSpec::default();
    }

    pub fn title(&self) -> String {
        self.snap
            .read()
            .ok()
            .and_then(|s| {
                s.path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .or_else(|| Some(s.path.display().to_string()))
            })
            .unwrap_or_else(|| "…".into())
    }
}
