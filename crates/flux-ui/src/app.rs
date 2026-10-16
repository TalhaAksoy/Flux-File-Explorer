//! FluxApp: owns the engines, the tab set, and the per-frame event pump.
//! All heavy work happens on engine threads; `update` only drains message
//! queues and draws the current snapshot.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use flux_core::enumerate::{EnumEvent, MetaEvent};
use flux_core::snapshot::SnapshotHandle;
use flux_core::{ComputeEngine, ComputeEvent, EnumEngine, MetaEngine, SortKey, SortSpec};
use flux_operations::engine::OpEvent;
use flux_operations::{ConflictQuery, OpEngine, OpKind, OpState};
use flux_platform::{FsWatcher, Volume, mounted_volumes};
use flux_search::{SearchEngine, SearchEvent, SearchHit};
use flux_settings::Settings;

use crate::pane::Tab;
use crate::theme::Density;

/// Internal file clipboard. Writes are also mirrored to the OS clipboard
/// (`flux_platform::clipboard`) so files copy between Flux and the native
/// file manager; plain-text paths are mirrored for convenience too.
#[derive(Default)]
pub struct Clipboard {
    pub items: Vec<PathBuf>,
    pub cut: bool,
}

pub enum Dialog {
    None,
    Rename {
        src: PathBuf,
        input: String,
    },
    NewFolder {
        input: String,
    },
    NewFile {
        input: String,
    },
    ConfirmPermanentDelete {
        items: Vec<PathBuf>,
    },
    ConfirmTrashUnsupported {
        items: Vec<PathBuf>,
    },
    Conflict {
        query: ConflictQuery,
        apply_all: bool,
    },
    BulkRename(BulkRenameState),
    Properties {
        path: PathBuf,
        /// Filled by a background stat so the dialog never blocks the UI
        /// thread on slow/network filesystems.
        meta: Option<crate::dialogs::PropsMeta>,
    },
    About,
}

pub struct BulkRenameState {
    pub sources: Vec<PathBuf>,
    pub find: String,
    pub replace: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub numbering: bool,
    pub num_start: u32,
    pub num_pad: usize,
    pub num_prefix: bool,
    pub case_mode: usize, // 0 none, 1 upper, 2 lower, 3 title
    pub set_ext: Option<String>,
    pub template: String,
    pub plan: Vec<flux_operations::RenameAction>,
    pub dirty: bool,
    /// Last keystroke time for debounce.
    pub last_edit: Instant,
}

impl BulkRenameState {
    pub fn new(sources: Vec<PathBuf>) -> Self {
        Self {
            sources,
            find: String::new(),
            replace: String::new(),
            regex: false,
            case_sensitive: false,
            numbering: true,
            num_start: 1,
            num_pad: 3,
            num_prefix: false,
            case_mode: 0,
            set_ext: None,
            template: String::new(),
            plan: Vec::new(),
            dirty: true,
            last_edit: Instant::now(),
        }
    }

    pub fn rules(&self) -> Vec<flux_operations::RenameRule> {
        use flux_operations::RenameRule as R;
        use flux_operations::rename::CaseKind;
        let mut rules = Vec::new();
        if !self.find.is_empty() {
            rules.push(R::FindReplace {
                find: self.find.clone(),
                replace: self.replace.clone(),
                regex: self.regex,
                case_sensitive: self.case_sensitive,
            });
        }
        match self.case_mode {
            1 => rules.push(R::ChangeCase(CaseKind::Upper)),
            2 => rules.push(R::ChangeCase(CaseKind::Lower)),
            3 => rules.push(R::ChangeCase(CaseKind::Title)),
            _ => {}
        }
        if let Some(ext) = &self.set_ext {
            rules.push(R::SetExtension(ext.clone()));
        }
        if !self.template.is_empty() {
            rules.push(R::Template(self.template.clone()));
        }
        if self.numbering {
            rules.push(R::Numbering {
                start: self.num_start,
                pad: self.num_pad,
                prefix: self.num_prefix,
            });
        }
        rules
    }

    pub fn recompute(&mut self) {
        let rules = self.rules();
        self.plan = flux_operations::plan_renames(&self.sources, &rules);
        self.dirty = false;
    }
}

/// Recursive-search UI state; when `active`, the center panel shows hits
/// instead of the directory table.
pub struct SearchUi {
    pub active: bool,
    pub hits: Vec<SearchHit>,
    /// A query is in flight on the search worker.
    pub running: bool,
    /// Text changed since the last issued query (debounce flag).
    pub dirty: bool,
    pub last_edit: Instant,
}

impl Default for SearchUi {
    fn default() -> Self {
        Self {
            active: false,
            hits: Vec::new(),
            running: false,
            dirty: false,
            last_edit: Instant::now(),
        }
    }
}

pub struct Command {
    pub name: &'static str,
    pub id: &'static str,
}

pub const COMMANDS: &[Command] = &[
    Command {
        name: "Go to Home",
        id: "go_home",
    },
    Command {
        name: "New Tab",
        id: "new_tab",
    },
    Command {
        name: "Close Tab",
        id: "close_tab",
    },
    Command {
        name: "Toggle Hidden Files",
        id: "toggle_hidden",
    },
    Command {
        name: "Refresh",
        id: "refresh",
    },
    Command {
        name: "Focus Address Bar",
        id: "focus_address",
    },
    Command {
        name: "Focus Filter",
        id: "focus_filter",
    },
    Command {
        name: "New Folder",
        id: "new_folder",
    },
    Command {
        name: "New File",
        id: "new_file",
    },
    Command {
        name: "Bulk Rename Selected",
        id: "bulk_rename",
    },
    Command {
        name: "Duplicate Selected",
        id: "duplicate",
    },
    Command {
        name: "Copy Path(s)",
        id: "copy_paths",
    },
    Command {
        name: "Open in Terminal",
        id: "open_terminal",
    },
    Command {
        name: "Toggle Recursive Search",
        id: "toggle_search",
    },
    Command {
        name: "Toggle Density",
        id: "toggle_density",
    },
    Command {
        name: "Toggle Dark/Light",
        id: "toggle_theme",
    },
];

pub struct FluxApp {
    pub enum_engine: EnumEngine,
    pub meta_engine: MetaEngine,
    pub compute: ComputeEngine,
    pub ops: OpEngine,
    pub search_engine: SearchEngine,
    pub watcher: FsWatcher,
    pub settings: Settings,

    pub tabs: Vec<Tab>,
    pub active: usize,

    pub clipboard: Clipboard,
    pub dialog: Dialog,
    pub address_edit: Option<String>,
    pub focus_address: bool,
    pub focus_filter: bool,
    /// Live text of the filter/search box; mirrored into the snapshot on apply.
    pub filter_text: String,
    pub search_ui: SearchUi,
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_sel: usize,
    /// Pointer position where a row context menu should appear.
    pub context_menu_pos: Option<egui::Pos2>,
    /// Same for the background (empty-area) context menu.
    pub bg_menu_pos: Option<egui::Pos2>,
    pub density: Density,
    pub dark: bool,

    pub volumes: Vec<Volume>,
    pub known: Vec<(&'static str, PathBuf)>,
    pub free_space: Option<flux_platform::FreeSpace>,
    pub status_msg: Option<(String, Instant)>,
    /// Paths text to hand to the OS clipboard on the next frame.
    pub pending_copy_text: Option<String>,
    /// File payload the OS clipboard held when a menu last opened —
    /// refreshed on context-menu open and consulted when the internal
    /// clipboard is empty (files copied in Finder/Explorer/a Linux FM).
    pub ext_clipboard: Option<flux_platform::clipboard::ClipPayload>,

    /// Native icon cache (background worker; glyph fallback until ready).
    pub icons: crate::icon_cache::IconCacheHandle,

    /// In-progress internal drag: absolute paths being dragged, and the
    /// pointer position where the drag began.
    pub dragging: Option<DragState>,

    /// Newest-dir events we haven't matched to a tab yet are dropped safely
    /// because snapshots carry their own generation.
    pub last_watch_hit: Option<Instant>,
    pub last_gc: Instant,
    /// Last time a MetaBatch triggered a repaint — progressive size/date
    /// updates are coalesced to ~30 fps instead of one frame per batch.
    pub last_meta_repaint: Instant,
    /// Op ids whose failure was already toasted — report once, not per frame.
    pub reported_ops: rustc_hash::FxHashSet<u64>,
    /// Whether the filesystem watcher has been handed a wake callback yet.
    pub wake_installed: bool,
    /// In-flight sidebar reorder drag — the dragged row's path plus the
    /// section it came from (kept separate from `dragging`, which is
    /// file-row drags into directories).
    pub sidebar_drag: Option<SidebarDrag>,
}

/// Which sidebar section a reorder drag belongs to — a path that exists in
/// both Places and Bookmarks must only reorder the section it was grabbed
/// in, so we remember the origin instead of re-deriving membership.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SidebarSection {
    Places,
    Bookmarks,
}

pub struct SidebarDrag {
    pub path: PathBuf,
    pub section: SidebarSection,
}

/// An in-flight drag of file rows.
pub struct DragState {
    pub items: Vec<PathBuf>,
    /// Icon key + kind of the first item, captured at drag start so the
    /// ghost can paint the real icon without touching the filesystem.
    pub key: crate::icon_cache::IconKey,
    pub kind: flux_core::EntryKind,
    /// Set once the pointer left the window and the OS drag was started.
    pub handed_to_os: bool,
}

impl Default for FluxApp {
    fn default() -> Self {
        Self::new()
    }
}

impl FluxApp {
    pub fn new() -> Self {
        let settings = flux_settings::load();
        let mut tabs: Vec<Tab> = if settings.tabs.is_empty() {
            vec![Tab::new(flux_platform::home_dir())]
        } else {
            settings
                .tabs
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let mut t = Tab::new(p.clone());
                    t.deferred_load = i != 0; // only tab 0 scans at startup
                    t
                })
                .collect()
        };
        if tabs.is_empty() {
            tabs.push(Tab::new(flux_platform::home_dir()));
        }
        {
            let mut s = tabs[0].snap.write().unwrap();
            s.show_hidden = settings.show_hidden;
            s.sort = sort_from_settings(&settings);
        }
        let dark = match settings.theme {
            flux_settings::Theme::Dark => true,
            flux_settings::Theme::Light => false,
            flux_settings::Theme::System => true,
        };
        let density = match settings.density {
            flux_settings::Density::Compact => Density::Compact,
            flux_settings::Density::Comfortable => Density::Comfortable,
            flux_settings::Density::Standard => Density::Standard,
        };
        Self {
            enum_engine: EnumEngine::new(),
            meta_engine: MetaEngine::new(),
            compute: ComputeEngine::new(),
            ops: OpEngine::new(),
            search_engine: SearchEngine::new(),
            watcher: FsWatcher::new(),
            settings,
            tabs,
            active: 0,
            clipboard: Clipboard::default(),
            dialog: Dialog::None,
            address_edit: None,
            focus_address: false,
            focus_filter: false,
            filter_text: String::new(),
            search_ui: SearchUi::default(),
            palette_open: false,
            palette_query: String::new(),
            palette_sel: 0,
            context_menu_pos: None,
            bg_menu_pos: None,
            density,
            dark,
            volumes: mounted_volumes(),
            known: flux_platform::known_dirs(),
            free_space: None,
            status_msg: None,
            pending_copy_text: None,
            ext_clipboard: None,
            icons: std::rc::Rc::new(crate::icon_cache::IconCache::new()),
            dragging: None,
            last_watch_hit: None,
            last_gc: Instant::now(),
            last_meta_repaint: Instant::now(),
            reported_ops: rustc_hash::FxHashSet::default(),
            wake_installed: false,
            sidebar_drag: None,
        }
    }

    pub fn current_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn current_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    pub fn current_snap(&self) -> SnapshotHandle {
        self.current_tab().snap.clone()
    }

    pub fn current_path(&self) -> PathBuf {
        self.current_tab().snap.read().unwrap().path.clone()
    }

    /// Navigate a tab to `path`, pushing history. Cancels the in-flight scan
    /// implicitly via generation bump.
    pub fn navigate(&mut self, path: PathBuf) {
        let filter_text = self.filter_text.clone();
        let snap = {
            let tab = self.current_tab_mut();
            tab.history.push(path.clone());
            tab.sel.clear();
            let mut s = tab.snap.write().unwrap();
            s.filter = if filter_text.is_empty() {
                None
            } else {
                Some(filter_text.clone())
            };
            tab.filter = filter_text;
            tab.snap.clone()
        };
        let g = self.enum_engine.navigate(&snap, path.clone());
        self.current_tab_mut().gen_id = g;
        self.watcher.watch(&path);
        self.free_space = flux_platform::free_space(&path);
        flux_settings::push_recent(&mut self.settings, path.clone());
        self.address_edit = None;
    }

    /// Re-enumerate the current directory, preserving selection by name.
    pub fn refresh(&mut self) {
        let (snap, path) = {
            let tab = self.current_tab_mut();
            let s = tab.snap.read().unwrap();
            let names = tab.sel.selected_names(&s);
            let path = s.path.clone();
            drop(s);
            tab.reselect_after_load = Some(names);
            (tab.snap.clone(), path)
        };
        let g = self.enum_engine.navigate(&snap, path.clone());
        self.current_tab_mut().gen_id = g;
        self.watcher.watch(&path);
    }

    pub fn go_up(&mut self) {
        if let Some(parent) = self.current_path().parent().map(|p| p.to_path_buf()) {
            self.navigate(parent);
        }
    }

    pub fn go_back(&mut self) {
        if let Some(p) = self.current_tab_mut().history.back() {
            self.navigate_no_history(p);
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(p) = self.current_tab_mut().history.forward() {
            self.navigate_no_history(p);
        }
    }

    fn navigate_no_history(&mut self, path: PathBuf) {
        let snap = {
            let tab = self.current_tab_mut();
            tab.sel.clear();
            tab.snap.clone()
        };
        let g = self.enum_engine.navigate(&snap, path.clone());
        self.current_tab_mut().gen_id = g;
        self.watcher.watch(&path);
        self.free_space = flux_platform::free_space(&path);
        self.address_edit = None;
    }

    pub fn open_entry(&mut self, idx: flux_core::EntryIdx) {
        let (path, is_dir) = {
            let s = self.current_tab().snap.read().unwrap();
            match s.entry(idx) {
                Some(e) => (s.path.join(&e.name), e.is_dir()),
                None => return,
            }
        };
        if is_dir {
            self.navigate(path);
        } else if let Err(e) = flux_platform::open_path(&path) {
            self.toast(format!("Cannot open: {e}"));
        }
    }

    /// Selected entries as absolute paths.
    pub fn selected_paths(&self) -> Vec<PathBuf> {
        let tab = self.current_tab();
        let s = tab.snap.read().unwrap();
        tab.sel
            .ids
            .iter()
            .filter_map(|&i| s.entry_path(i))
            .collect()
    }

    /// Path of the focused row, or the sole selection, else None.
    pub fn focused_path(&self) -> Option<PathBuf> {
        let tab = self.current_tab();
        let s = tab.snap.read().unwrap();
        if let Some(row) = tab.sel.focus
            && let Some(idx) = s.row_index(row)
        {
            return s.entry_path(idx);
        }
        tab.sel.ids.iter().next().and_then(|&i| s.entry_path(i))
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.status_msg = Some((msg.into(), Instant::now()));
    }

    pub fn new_tab(&mut self) {
        let path = self.current_path();
        let t = Tab::new(path.clone());
        {
            let mut s = t.snap.write().unwrap();
            s.show_hidden = self.settings.show_hidden;
        }
        self.tabs.push(t);
        self.active = self.tabs.len() - 1;
        self.navigate(path);
    }

    pub fn new_tab_at(&mut self, path: PathBuf) {
        let t = Tab::new(path.clone());
        {
            let mut s = t.snap.write().unwrap();
            s.show_hidden = self.settings.show_hidden;
        }
        self.tabs.push(t);
        self.active = self.tabs.len() - 1;
        self.navigate(path);
    }

    pub fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() <= 1 {
            return;
        }
        self.enum_engine.cancel_current();
        self.meta_engine.cancel_current();
        self.tabs.remove(idx);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        let (snap, path, deferred) = {
            let t = self.current_tab();
            (
                t.snap.clone(),
                t.snap.read().unwrap().path.clone(),
                t.deferred_load,
            )
        };
        if deferred {
            self.current_tab_mut().deferred_load = false;
            let g = self.enum_engine.navigate(&snap, path.clone());
            self.current_tab_mut().gen_id = g;
        }
        self.watcher.watch(&path);
    }

    pub fn activate_tab(&mut self, idx: usize) {
        if idx == self.active || idx >= self.tabs.len() {
            return;
        }
        self.active = idx;
        let (snap, path, deferred) = {
            let t = self.current_tab();
            (
                t.snap.clone(),
                t.snap.read().unwrap().path.clone(),
                t.deferred_load,
            )
        };
        if deferred {
            self.current_tab_mut().deferred_load = false;
            let g = self.enum_engine.navigate(&snap, path.clone());
            self.current_tab_mut().gen_id = g;
        }
        self.watcher.watch(&path);
        self.free_space = flux_platform::free_space(&path);
    }

    pub fn set_filter(&mut self, text: String) {
        let tab = self.current_tab_mut();
        tab.filter = text;
        {
            let mut s = tab.snap.write().unwrap();
            s.filter = if tab.filter.is_empty() {
                None
            } else {
                Some(tab.filter.clone())
            };
        }
        let g = tab.gen_id;
        let snap = tab.snap.clone();
        self.compute.rebuild(&snap, g);
    }

    pub fn set_sort(&mut self, spec: SortSpec) {
        let tab = self.current_tab_mut();
        {
            let mut s = tab.snap.write().unwrap();
            s.sort = spec;
        }
        let g = tab.gen_id;
        let snap = tab.snap.clone();
        self.compute.rebuild(&snap, g);
    }

    pub fn toggle_hidden(&mut self) {
        self.settings.show_hidden = !self.settings.show_hidden;
        let show_hidden = self.settings.show_hidden;
        let tab = self.current_tab_mut();
        {
            let mut s = tab.snap.write().unwrap();
            s.show_hidden = show_hidden;
        }
        let g = tab.gen_id;
        let snap = tab.snap.clone();
        self.compute.rebuild(&snap, g);
        self.save_settings();
    }

    pub fn save_settings(&mut self) {
        self.settings.tabs = self
            .tabs
            .iter()
            .map(|t| t.snap.read().unwrap().path.clone())
            .collect();
        self.settings.density = match self.density {
            Density::Compact => flux_settings::Density::Compact,
            Density::Standard => flux_settings::Density::Standard,
            Density::Comfortable => flux_settings::Density::Comfortable,
        };
        let _ = flux_settings::save(&self.settings);
    }

    /// Selected-or-focused items as (path, is_dir).
    pub fn actionable_paths(&self) -> Vec<PathBuf> {
        let sel = self.selected_paths();
        if sel.is_empty() {
            self.focused_path().into_iter().collect()
        } else {
            sel
        }
    }

    pub fn submit_copy(&mut self, dest: PathBuf) {
        // Copy once → paste many times: only a Cut empties the clipboard
        // after the operation is submitted (Explorer semantics).
        let mut cut = self.clipboard.cut;
        let mut items = if cut {
            std::mem::take(&mut self.clipboard.items)
        } else {
            self.clipboard.items.clone()
        };
        if items.is_empty() {
            // Internal clipboard empty — pull files the user copied in the
            // OS file manager (Finder/Explorer/Dolphin…). Read the live
            // clipboard first; the menu-open snapshot is only a fallback
            // (it can be stale if the OS clipboard changed since).
            let ext = flux_platform::clipboard::read_files().or_else(|| self.ext_clipboard.take());
            if let Some(pl) = ext {
                items = pl.paths;
                cut = pl.cut;
            }
        }
        if items.is_empty() {
            return;
        }
        if cut {
            self.clipboard.cut = false;
        }
        let kind = if cut {
            OpKind::Move {
                sources: items,
                dest_dir: dest,
            }
        } else {
            OpKind::Copy {
                sources: items,
                dest_dir: dest,
            }
        };
        self.ops.submit(kind, flux_operations::ConflictPolicy::Ask);
    }

    /// Begin dragging `row`'s entry — or the whole selection if the row is
    /// already selected. Selects an unselected row first, matching Explorer/
    /// Finder behavior.
    pub fn start_drag_row(&mut self, row: usize) {
        let (items, key, kind) = {
            let tab = self.current_tab_mut();
            let s = tab.snap.read().unwrap();
            let Some(idx) = s.row_index(row) else {
                return;
            };
            if !tab.sel.contains(idx) {
                tab.sel.select_row(&s, row);
            }
            let first = s.entry(idx);
            let (key, kind) = first
                .map(|e| {
                    (
                        crate::icon_cache::IconKey::for_entry(e, &s.path.join(&e.name)),
                        e.kind,
                    )
                })
                .unwrap_or((
                    crate::icon_cache::IconKey::Ext(String::new()),
                    flux_core::EntryKind::File,
                ));
            let items = tab
                .sel
                .ids
                .iter()
                .filter_map(|&i| s.entry_path(i))
                .collect::<Vec<_>>();
            (items, key, kind)
        };
        if !items.is_empty() {
            self.dragging = Some(DragState {
                items,
                key,
                kind,
                handed_to_os: false,
            });
        }
    }

    /// Execute a completed drop: move (or copy, when `copy` — Option/Alt held)
    /// the dragged items into `target_dir`. Items whose parent IS the target
    /// (a no-op move) are filtered out.
    pub fn apply_drop(&mut self, target_dir: PathBuf, copy: bool) {
        let Some(drag) = self.dragging.take() else {
            return;
        };
        let items: Vec<PathBuf> = drag
            .items
            .into_iter()
            .filter(|p| {
                p.as_path() != target_dir.as_path() && p.parent() != Some(target_dir.as_path())
            })
            .collect();
        if items.is_empty() {
            return;
        }
        if !target_dir.is_dir() {
            self.toast(format!("Not a folder: {}", target_dir.display()));
            return;
        }
        let kind = if copy {
            OpKind::Copy {
                sources: items,
                dest_dir: target_dir,
            }
        } else {
            OpKind::Move {
                sources: items,
                dest_dir: target_dir,
            }
        };
        self.ops.submit(kind, flux_operations::ConflictPolicy::Ask);
    }

    /// ⌘D — duplicate selected items in place ("name copy.ext").
    pub fn duplicate_selected(&mut self) {
        let items = self.actionable_paths();
        if !items.is_empty() {
            self.ops.submit(
                OpKind::Duplicate { items },
                flux_operations::ConflictPolicy::SkipAll,
            );
        }
    }

    pub fn submit_trash(&mut self, items: Vec<PathBuf>) {
        if items.is_empty() {
            return;
        }
        self.ops.submit(
            OpKind::Trash { items },
            flux_operations::ConflictPolicy::Ask,
        );
    }

    /// Compress selected items to "<name>.zip" in their parent dir
    /// (multi-select → "Archive.zip"); auto-suffixes when taken.
    pub fn compress_items(&mut self, items: Vec<PathBuf>) {
        let Some(first) = items.first() else { return };
        let parent = first.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let stem = if items.len() == 1 {
            first
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Archive".into())
        } else {
            "Archive".to_string()
        };
        // Reserve a free name up-front; the op's resolver still guards races.
        let mut dest = parent.join(format!("{stem}.zip"));
        for n in 2..1000 {
            if !dest.exists() {
                break;
            }
            dest = parent.join(format!("{stem} {n}.zip"));
        }
        self.ops.submit(
            OpKind::Compress { items, dest },
            flux_operations::ConflictPolicy::Ask,
        );
    }

    /// Pin a directory to the sidebar bookmarks (idempotent).
    pub fn pin_to_sidebar(&mut self, path: PathBuf) {
        if path.is_dir() && !self.settings.bookmarks.contains(&path) {
            self.settings.bookmarks.push(path);
            let _ = flux_settings::save(&self.settings);
            self.toast("Added to sidebar".to_string());
        }
    }

    pub fn unpin_from_sidebar(&mut self, path: &std::path::Path) {
        self.settings.bookmarks.retain(|b| b != path);
        let _ = flux_settings::save(&self.settings);
    }

    /// Move bookmark `path` so it sits at `insert_at` in the current order.
    /// `insert_at` is an insert-BEFORE index into the pre-move list.
    pub fn move_bookmark(&mut self, path: &std::path::Path, insert_at: usize) {
        let Some(from) = self.settings.bookmarks.iter().position(|b| b == path) else {
            return;
        };
        let last = self.settings.bookmarks.len() - 1;
        let at = if insert_at > from {
            insert_at - 1
        } else {
            insert_at
        }
        .min(last);
        if at == from {
            return; // dropped onto itself — nothing to persist
        }
        let item = self.settings.bookmarks.remove(from);
        self.settings.bookmarks.insert(at, item);
        let _ = flux_settings::save(&self.settings);
    }

    /// Places rows in display order — `settings.places_order` positions
    /// win; dirs absent from it keep their default relative order.
    pub fn ordered_places(&self) -> Vec<(&'static str, PathBuf)> {
        let mut v = self.known.clone();
        v.sort_by_key(|(_, p)| {
            self.settings
                .places_order
                .iter()
                .position(|x| x == p)
                .unwrap_or(usize::MAX)
        });
        v
    }

    /// Persist a user-defined Places order — the full visible order with
    /// `path` moved to `insert_at`.
    pub fn move_place(&mut self, path: &std::path::Path, insert_at: usize) {
        let mut order: Vec<PathBuf> = self.ordered_places().into_iter().map(|(_, p)| p).collect();
        let Some(from) = order.iter().position(|p| p == path) else {
            return;
        };
        let last = order.len() - 1;
        let at = if insert_at > from {
            insert_at - 1
        } else {
            insert_at
        }
        .min(last);
        if at == from {
            return;
        }
        let item = order.remove(from);
        order.insert(at, item);
        self.settings.places_order = order;
        let _ = flux_settings::save(&self.settings);
    }

    /// Per-frame pump: engine events, watcher, ops, search. Returns whether
    /// anything needs a repaint (we request_repaint anyway on any event).
    pub fn pump(&mut self, ctx: &egui::Context) {
        // Enumeration events.
        let enum_events: Vec<_> = self.enum_engine.drain_events().collect();
        for ev in enum_events {
            match ev {
                EnumEvent::Finished { gen_id, .. } => {
                    for tab in &mut self.tabs {
                        if tab.gen_id == gen_id {
                            // Final global sort on the compute thread.
                            self.compute.rebuild(&tab.snap, gen_id);
                            // Deferred metadata: stat entries in background.
                            self.meta_engine.fill(&tab.snap, gen_id);
                            if let Some(names) = tab.reselect_after_load.take() {
                                tab.pending_reselect = Some(names);
                            }
                            if let Ok(mut s) = tab.snap.write() {
                                s.finish();
                            }
                        }
                    }
                }
                EnumEvent::Error { gen_id, message } => {
                    let cur = self.current_tab().gen_id;
                    if gen_id == cur {
                        self.toast(format!("Cannot open folder: {message}"));
                    }
                }
                _ => {}
            }
            ctx.request_repaint();
        }
        // Metadata-fill events: sizes/dates arrive progressively — repaint to
        // show them; on finish, rebuild the view if the sort depends on
        // metadata so the final order is exact.
        for ev in self.meta_engine.drain_events().collect::<Vec<_>>() {
            match ev {
                MetaEvent::Batch { .. } => {
                    // A fast disk can stream ~100 batches/sec for a huge
                    // dir — coalesce repaints to ~30 fps; the final
                    // Finished event always repaints regardless.
                    const FRAME: Duration = Duration::from_millis(33);
                    let elapsed = self.last_meta_repaint.elapsed();
                    if elapsed >= FRAME {
                        self.last_meta_repaint = Instant::now();
                        ctx.request_repaint();
                    } else {
                        ctx.request_repaint_after(FRAME - elapsed);
                    }
                }
                MetaEvent::Finished { gen_id } => {
                    for tab in &mut self.tabs {
                        if tab.gen_id == gen_id
                            && matches!(
                                tab.snap.read().map(|s| s.sort.key),
                                Ok(SortKey::Size | SortKey::Modified)
                            )
                        {
                            self.compute.rebuild(&tab.snap, gen_id);
                        }
                    }
                    ctx.request_repaint();
                }
                MetaEvent::Superseded { .. } => {}
            }
        }
        // Compute events: apply deferred reselection + repaint.
        let compute_events: Vec<_> = self.compute.drain_events().collect();
        for ev in compute_events {
            if let ComputeEvent::ViewReady { gen_id } = ev {
                for tab in &mut self.tabs {
                    if tab.gen_id == gen_id
                        && let Some(names) = tab.pending_reselect.take()
                    {
                        let s = tab.snap.read().unwrap();
                        tab.sel.reselect_names(&s, &names);
                    }
                }
                ctx.request_repaint();
            }
        }
        // Watcher debounce → refresh current dir. The watcher also wakes
        // the UI directly so an idle app notices external changes; while a
        // debounce is pending we schedule a repaint at its expiry.
        if !self.wake_installed {
            let c = ctx.clone();
            self.watcher.set_wake(move || c.request_repaint());
            self.wake_installed = true;
        }
        if self.watcher.poll_changed() {
            self.refresh();
        } else if self.watcher.pending() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // Op events.
        let mut dirty = false;
        for ev in self.ops.drain_events() {
            match ev {
                OpEvent::Dirty => dirty = true,
                OpEvent::Conflict(q) => {
                    self.dialog = Dialog::Conflict {
                        query: q,
                        apply_all: false,
                    };
                    ctx.request_repaint();
                }
            }
        }
        if dirty {
            // The status bar only renders *active* ops — a finished op's
            // errors would vanish silently, so toast them once each.
            for op in self.ops.all_ops() {
                if self.reported_ops.contains(&op.id) {
                    continue;
                }
                let p = op.progress_snapshot();
                let err = match &p.state {
                    OpState::Failed(m) => Some(m.clone()),
                    OpState::DoneWithErrors => p.errors.first().cloned(),
                    _ => None,
                };
                if let Some(err) = err {
                    self.reported_ops.insert(op.id);
                    self.toast(format!("{}: {err}", op.label));
                }
            }
            ctx.request_repaint();
        }
        // Icon fetches finished → upload + repaint.
        if self.icons.pump(ctx) > 0 {
            ctx.request_repaint();
        }
        // Search events.
        let mut got_hits = false;
        for ev in self.search_engine.drain_events() {
            match ev {
                SearchEvent::Batch { hits, .. } => {
                    self.search_ui.hits.extend(hits);
                    got_hits = true;
                }
                SearchEvent::Done { .. } => self.search_ui.running = false,
            }
        }
        if got_hits {
            ctx.request_repaint();
        }
        // Deferred bulk-rename preview recompute (debounced).
        if let Dialog::BulkRename(br) = &mut self.dialog
            && br.dirty
            && br.last_edit.elapsed() > Duration::from_millis(250)
        {
            br.recompute();
            ctx.request_repaint();
        }
        // GC finished ops once a minute.
        if self.last_gc.elapsed() > Duration::from_secs(60) {
            self.last_gc = Instant::now();
            self.ops.gc_finished(Duration::from_secs(300));
        }
    }

    /// Flip to an explicit light/dark theme (drops System following) and
    /// persist the choice — previously the toggle never reached settings,
    /// so restarts silently reverted.
    pub fn toggle_theme(&mut self, ctx: &egui::Context) {
        self.settings.theme = if self.dark {
            flux_settings::Theme::Light
        } else {
            flux_settings::Theme::Dark
        };
        self.dark = !self.dark;
        let _ = flux_settings::save(&self.settings);
        crate::theme::apply_theme(ctx, self.dark);
    }

    /// While the theme preference is System, follow the OS: egui keeps a
    /// cached `system_theme` updated by the OS (no polling on our side).
    /// Call once per frame; a change is applied immediately.
    pub fn sync_system_theme(&mut self, ctx: &egui::Context) {
        if self.settings.theme != flux_settings::Theme::System {
            return;
        }
        let dark = matches!(ctx.system_theme(), Some(egui::Theme::Dark));
        if dark != self.dark {
            self.dark = dark;
            crate::theme::apply_theme(ctx, dark);
        }
    }

    pub fn run_command(&mut self, id: &str, ctx: &egui::Context) {
        match id {
            "go_home" => self.navigate(flux_platform::home_dir()),
            "new_tab" => self.new_tab(),
            "close_tab" => {
                let i = self.active;
                self.close_tab(i);
            }
            "toggle_hidden" => self.toggle_hidden(),
            "refresh" => self.refresh(),
            "focus_address" => {
                self.address_edit = Some(self.current_path().display().to_string());
                self.focus_address = true;
            }
            "focus_filter" => self.focus_filter = true,
            "new_folder" => {
                self.dialog = Dialog::NewFolder {
                    input: "New Folder".into(),
                }
            }
            "new_file" => {
                self.dialog = Dialog::NewFile {
                    input: "untitled".into(),
                }
            }
            "bulk_rename" => {
                let paths = self.actionable_paths();
                if !paths.is_empty() {
                    self.dialog = Dialog::BulkRename(BulkRenameState::new(paths));
                }
            }
            "copy_paths" => {
                let text = self
                    .actionable_paths()
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    self.pending_copy_text = Some(text);
                }
            }
            "duplicate" => self.duplicate_selected(),
            "open_terminal" => {
                let p = self.current_path();
                if let Err(e) = flux_platform::open_in_terminal(&p) {
                    self.toast(format!("Terminal: {e}"));
                }
            }
            "toggle_search" => {
                self.search_ui.active = !self.search_ui.active;
                if self.search_ui.active {
                    self.focus_filter = true;
                }
            }
            "toggle_density" => self.density = self.density.next(),
            "toggle_theme" => self.toggle_theme(ctx),
            _ => {}
        }
    }
}

fn sort_from_settings(s: &Settings) -> SortSpec {
    SortSpec {
        dirs_first: s.dirs_first,
        dir: if s.sort_desc {
            flux_core::SortDir::Desc
        } else {
            flux_core::SortDir::Asc
        },
        key: match s.sort_key.as_str() {
            "size" => flux_core::SortKey::Size,
            "modified" => flux_core::SortKey::Modified,
            "extension" => flux_core::SortKey::Extension,
            "kind" => flux_core::SortKey::Kind,
            _ => flux_core::SortKey::Name,
        },
        ..SortSpec::default()
    }
}
