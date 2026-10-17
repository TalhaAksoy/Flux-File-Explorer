//! FluxApp: owns the engines, the tab set, and the per-frame event pump.
//! All heavy work happens on engine threads; `update` only drains message
//! queues and draws the current snapshot.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use flux_core::enumerate::{EnumEvent, MetaEvent};
use flux_core::snapshot::SnapshotHandle;
use flux_core::{
    ComputeEngine, ComputeEvent, EnumEngine, MetaEngine, SizeEngine, SortKey, SortSpec,
};
use flux_operations::engine::OpEvent;
use flux_operations::{ConflictQuery, OpEngine, OpKind, OpState};
use flux_platform::{FsWatcher, Volume, mounted_volumes};
use flux_search::{SearchEngine, SearchEvent, SearchHit};
use flux_settings::Settings;

use crate::pane::{DirectoryTab, Tab, TabContent};
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
    Properties {
        path: PathBuf,
        /// Filled by a background stat so the dialog never blocks the UI
        /// thread on slow/network filesystems.
        meta: Option<crate::dialogs::PropsMeta>,
    },
    /// A Bulk Rename workspace tab holds a live, unexecuted plan —
    /// confirm before discarding it.
    ConfirmCloseTab {
        idx: usize,
    },
}

/// Settings-tab state: which category is focused and the search query
/// that filters the registry (empty query = normal category view).
#[derive(Default)]
pub struct SettingsUi {
    pub category: crate::settings_page::Category,
    pub query: String,
    /// Two-step confirmation for "Reset all settings".
    pub reset_armed: bool,
}

/// Which rows the rename preview list shows. Display-only — operation
/// order/semantics come from `plan`, never from the filtered view.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum RenameFilter {
    #[default]
    All,
    Changed,
    Unchanged,
    /// Conflicts, duplicate targets, invalid names.
    Issues,
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
    /// A plan recompute is in flight on the worker; results are dropped
    /// unless their generation still matches `plan_gen` (stale-guard).
    pub plan_rx: Option<(
        u64,
        std::sync::mpsc::Receiver<Vec<flux_operations::RenameAction>>,
    )>,
    pub plan_gen: u64,
    /// True while the worker is crunching — the list shows the last plan
    /// dimmed so typing never blocks.
    pub plan_stale: bool,
    pub dirty: bool,
    /// Last keystroke time for debounce.
    pub last_edit: Instant,
    /// Preview list state — filter + filename search.
    pub filter: RenameFilter,
    pub query: String,
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
            plan_rx: None,
            plan_gen: 0,
            plan_stale: false,
            dirty: true,
            last_edit: Instant::now(),
            filter: RenameFilter::All,
            query: String::new(),
        }
    }

    /// Mark the plan dirty — the workspace's debounce submits a recompute.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_edit = Instant::now();
    }

    /// Kick the rename plan onto a worker thread. plan_renames is pure
    /// (name transforms + `exists()` checks), but with 10k+ sources the
    /// stat storm can stall a frame — off the UI thread it never does.
    /// Generations discard results superseded by a newer edit.
    pub fn recompute_async(&mut self) {
        self.dirty = false;
        self.plan_stale = true;
        self.plan_gen += 1;
        let generation = self.plan_gen;
        let sources = self.sources.clone();
        let rules = self.rules();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("flux-rename-plan".into())
            .spawn(move || {
                let _ = tx.send(flux_operations::plan_renames(&sources, &rules));
            })
            .expect("spawn rename planner");
        self.plan_rx = Some((generation, rx));
    }

    /// Accept a finished plan only if it answers the latest request.
    /// Returns true when a pending computation resolved (applied or dropped).
    pub fn poll_plan(&mut self) -> bool {
        let Some((generation, rx)) = &self.plan_rx else {
            return false;
        };
        let generation = *generation;
        match rx.try_recv() {
            Ok(plan) => {
                self.plan_rx = None;
                if generation == self.plan_gen {
                    self.plan = plan;
                    self.plan_stale = false;
                }
                true
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.plan_rx = None;
                self.plan_stale = false;
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
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
    Command {
        name: "Open Settings",
        id: "open_settings",
    },
];

pub struct FluxApp {
    pub enum_engine: EnumEngine,
    pub meta_engine: MetaEngine,
    /// Opt-in recursive folder sizes (context menu request).
    pub size_engine: SizeEngine,
    pub compute: ComputeEngine,
    pub ops: OpEngine,
    pub search_engine: SearchEngine,
    pub watcher: FsWatcher,
    pub settings: Settings,

    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Monotonic id source for new tabs.
    pub next_tab_id: u64,

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
    /// Per-file image thumbnails (bounded LRU worker cache).
    pub thumbs: crate::thumbs::ThumbCacheHandle,

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
    /// Docked embedded terminal panel (None = closed).
    pub terminal: Option<crate::terminal::EmbeddedTerm>,

    /// Preview worker + state for the right-side panel. `preview_req` is
    /// the path currently in focus; `preview`/`preview_tex` hold the last
    /// completed result (only applied while it still matches the request).
    pub preview_engine: crate::preview::PreviewEngine,
    pub preview_req: Option<PathBuf>,
    /// The visual preview — phase-2 worker delivery.
    pub preview: Option<crate::preview::Preview>,
    /// Filesystem metadata — phase-1 worker delivery (arrives first).
    pub preview_meta: Option<crate::preview::PreviewMeta>,
    pub preview_tex: Option<egui::TextureHandle>,
    /// Opt-in expensive metadata (SHA-256, recursive dir size) — only
    /// ever runs when the user clicks the panel button. `meta_op` is
    /// the last completed result; `meta_op_pending` shows the spinner.
    pub meta_ops: crate::preview::MetaOps,
    pub meta_op: Option<crate::preview::MetaOpMsg>,
    pub meta_op_pending: Option<(PathBuf, crate::preview::MetaOp)>,
    /// File row the pointer rests on, refreshed by the views each frame.
    /// Space resolves this first — preview the hovered file even when
    /// it isn't selected.
    pub hovered_path: Option<PathBuf>,
    /// Explicit Space-pinned preview target. Wins over focus while it
    /// stays valid; `pin_anchor` remembers the focused path at pin time
    /// so a real focus change can drop the pin.
    pub preview_pin: Option<PathBuf>,
    pub pin_anchor: Option<PathBuf>,
    /// Space key state last frame — rising-edge detection so key-repeat
    /// can't machine-gun the panel open/closed.
    pub space_was_down: bool,
    /// Debounce marker for settings writes: set when a frame mutates
    /// persisted state (e.g. column resize); flushed after a quiet period
    /// so resizing doesn't write settings.json every frame.
    pub settings_dirty_at: Option<Instant>,
    /// "Open With" app list for the file a context menu was opened on —
    /// resolved once per target (spawning xdg-mime per frame would be
    /// wasteful) and recomputed when the target path changes.
    pub open_with_cache: Option<(PathBuf, Vec<flux_platform::OpenWithApp>)>,
    /// Row context-menu path facts (is-dir / is-zip / has-real-dir),
    /// keyed by a hash of the item set — computed once per menu instead
    /// of statting the filesystem every frame the menu is open.
    pub row_menu_flags: Option<(u64, crate::dialogs::RowFlags)>,
    /// Cells-per-row the icon grid last rendered with — keyboard nav in
    /// grid/gallery mode moves Up/Down by this many entries.
    pub grid_cols: usize,
    /// Undo/redo journal. Ops that complete `Done` cleanly are inverted
    /// into entries here; ops this app submits itself as inverses are
    /// pre-marked in `journaled_ops` so they don't double-record.
    pub undo_stack: Vec<UndoEntry>,
    pub redo_stack: Vec<UndoEntry>,
    /// Op ids already journaled (or submitted BY undo/redo).
    pub journaled_ops: rustc_hash::FxHashSet<u64>,
    /// Ops-history popup visibility (status-bar button).
    pub show_ops_log: bool,
    /// Screenshot harness config — `Some` only when FLUX_SHOT_OUT is set.
    /// Dev/QA only: renders N frames, captures, exits.
    pub shot: Option<ShotCfg>,
}

/// Env-driven screenshot capture for visual QA. `FLUX_SHOT_OUT` enables
/// it; `FLUX_SHOT_SCREEN` = explorer|settings|rename, `FLUX_SHOT_DIR`
/// opens that folder, `FLUX_SHOT_THEME` = dark|light, `FLUX_SHOT_SIZE` =
/// WxH (parsed in `run`), `FLUX_SHOT_WAIT` = settle frames (default 45),
/// `FLUX_SHOT_MENU` = row|bg opens that context menu before capturing.
pub struct ShotCfg {
    pub out: PathBuf,
    pub screen: String,
    pub wait: u32,
    pub frames: u32,
    pub requested: bool,
    pub screen_done: bool,
    /// Re-requested at runtime — some Wayland compositors ignore the
    /// initial `with_inner_size` but honor post-map resizes.
    pub size: Option<[f32; 2]>,
    /// `row` opens the file context menu on the first row; `bg` the
    /// empty-space menu.
    pub menu: Option<String>,
    /// `FLUX_SHOT_PREVIEW` — filename to pin into the open details
    /// panel (simulates a Space-preview end state).
    pub preview_name: Option<String>,
    pub preview_done: bool,
}

impl ShotCfg {
    pub fn from_env() -> Option<Self> {
        let out = std::env::var_os("FLUX_SHOT_OUT").map(PathBuf::from)?;
        Some(Self {
            out,
            screen: std::env::var("FLUX_SHOT_SCREEN").unwrap_or_else(|_| "explorer".into()),
            wait: std::env::var("FLUX_SHOT_WAIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(45),
            frames: 0,
            requested: false,
            screen_done: false,
            size: shot_size_env(),
            menu: std::env::var("FLUX_SHOT_MENU").ok(),
            preview_name: std::env::var("FLUX_SHOT_PREVIEW").ok(),
            preview_done: false,
        })
    }
}

/// Parse `FLUX_SHOT_SIZE` (`WxH`) — shared by `run()` and the harness.
pub fn shot_size_env() -> Option<[f32; 2]> {
    std::env::var("FLUX_SHOT_SIZE").ok().and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?])
    })
}

/// A journal entry: submit `inverse` to undo, `forward` to redo.
pub struct UndoEntry {
    pub label: String,
    pub forward: OpKind,
    pub inverse: OpKind,
}

/// Build an inverse for a cleanly-completed op, or None when the op kind
/// can't be safely reversed.
///
/// Inverted here: renames (exact), mkdir/create-file (trash the result —
/// recoverable, unlike delete), duplicate (trash the copies), compress
/// (trash the archive). NOT inverted: copy/move (post-conflict dest set
/// isn't recoverable from the kind alone), trash (the OS trash API has
/// no portable restore), delete, extract (an "Extract Here" inverse
/// would have to trash the *parent* dir — dangerous).
fn undo_entry(kind: &OpKind, label: &str) -> Option<UndoEntry> {
    let inverse = match kind {
        OpKind::Rename { from, to } if to.exists() => OpKind::Rename {
            from: to.clone(),
            to: from.clone(),
        },
        OpKind::BatchRename { actions } => {
            let inv: Vec<flux_operations::RenameAction> = actions
                .iter()
                .filter(|a| a.dst.exists())
                .map(|a| flux_operations::RenameAction {
                    src: a.dst.clone(),
                    dst: a.src.clone(),
                    status: flux_operations::RenameStatus::Ready,
                })
                .collect();
            if inv.is_empty() {
                return None;
            }
            OpKind::BatchRename { actions: inv }
        }
        OpKind::Mkdir { path } if path.exists() => OpKind::Trash {
            items: vec![path.clone()],
        },
        OpKind::CreateFile { path } if path.exists() => OpKind::Trash {
            items: vec![path.clone()],
        },
        OpKind::Duplicate { items } => {
            let dsts: Vec<PathBuf> = items
                .iter()
                .map(|p| flux_operations::copy::duplicate_dst(p.as_path()))
                .filter(|p| p.exists())
                .collect();
            if dsts.is_empty() {
                return None;
            }
            OpKind::Trash { items: dsts }
        }
        OpKind::Compress { dest, .. } if dest.exists() => OpKind::Trash {
            items: vec![dest.clone()],
        },
        _ => return None,
    };
    Some(UndoEntry {
        label: label.to_string(),
        forward: kind.clone(),
        inverse,
    })
}

impl FluxApp {
    /// Settings is a singleton workspace tab: focus it if it exists,
    /// otherwise open it alongside the directory tabs.
    pub fn open_settings(&mut self) {
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| matches!(t.content, TabContent::Settings(_)))
        {
            self.activate_tab(i);
            return;
        }
        let id = self.alloc_tab_id();
        self.push_tab(Tab {
            id,
            content: TabContent::Settings(SettingsUi::default()),
        });
    }

    /// Open the Bulk Rename workspace for the given sources as a new tab.
    pub fn open_bulk_rename(&mut self, sources: Vec<PathBuf>) {
        let mut st = BulkRenameState::new(sources);
        st.recompute_async();
        let id = self.alloc_tab_id();
        self.push_tab(Tab {
            id,
            content: TabContent::BulkRename(Box::new(st)),
        });
    }

    /// Leave the active workspace tab for the nearest directory tab —
    /// the workspace stays open (its tab keeps all state).
    pub fn leave_workspace(&mut self) {
        if self.current_tab().dir().is_some() {
            return;
        }
        if let Some(i) = self.tabs.iter().rposition(|t| t.dir().is_some()) {
            self.activate_tab(i);
        }
    }

    fn alloc_tab_id(&mut self) -> u64 {
        self.next_tab_id += 1;
        self.next_tab_id
    }

    /// Append a tab and activate it.
    fn push_tab(&mut self, tab: Tab) {
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
    }

    /// Cached `open_with_apps` for `path` (see field docs).
    pub fn open_with_apps(&mut self, path: &Path) -> Vec<flux_platform::OpenWithApp> {
        if self.open_with_cache.as_ref().map(|(p, _)| p.as_path()) != Some(path) {
            self.open_with_cache = Some((path.to_path_buf(), flux_platform::open_with_apps(path)));
        }
        self.open_with_cache.as_ref().unwrap().1.clone()
    }
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
        Self::from_settings(flux_settings::load())
    }

    /// Build the app around an explicit settings value — tests use this
    /// to avoid touching the real user config.
    pub fn from_settings(settings: Settings) -> Self {
        let icons = std::rc::Rc::new(crate::icon_cache::IconCache::new());
        icons.set_source(settings.icon_source);
        let mut next_id = 0u64;
        let mut alloc = || {
            next_id += 1;
            next_id
        };
        let mut tabs: Vec<Tab> = if settings.tabs.is_empty() {
            vec![Tab::directory(alloc(), flux_platform::home_dir())]
        } else {
            settings
                .tabs
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let mut d = DirectoryTab::new(p.clone());
                    d.deferred_load = i != 0; // only tab 0 scans at startup
                    Tab {
                        id: alloc(),
                        content: TabContent::Directory(Box::new(d)),
                    }
                })
                .collect()
        };
        if tabs.is_empty() {
            tabs.push(Tab::directory(alloc(), flux_platform::home_dir()));
        }
        if let Some(d0) = tabs[0].dir_mut() {
            let mut s = d0.snap.write().unwrap();
            s.show_hidden = settings.show_hidden;
            s.show_system = settings.show_system_files;
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
            size_engine: SizeEngine::new(),
            compute: ComputeEngine::new(),
            ops: OpEngine::new(),
            search_engine: SearchEngine::new(),
            watcher: FsWatcher::new(),
            settings,
            tabs,
            active: 0,
            next_tab_id: next_id,
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
            icons,
            thumbs: std::rc::Rc::new(crate::thumbs::ThumbCache::new()),
            dragging: None,
            last_watch_hit: None,
            last_gc: Instant::now(),
            last_meta_repaint: Instant::now(),
            reported_ops: rustc_hash::FxHashSet::default(),
            wake_installed: false,
            sidebar_drag: None,
            terminal: None,
            preview_engine: crate::preview::PreviewEngine::new(),
            preview_req: None,
            preview: None,
            preview_meta: None,
            preview_tex: None,
            meta_ops: crate::preview::MetaOps::new(),
            meta_op: None,
            meta_op_pending: None,
            hovered_path: None,
            preview_pin: None,
            pin_anchor: None,
            space_was_down: false,
            settings_dirty_at: None,
            open_with_cache: None,
            row_menu_flags: None,
            grid_cols: 1,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            journaled_ops: rustc_hash::FxHashSet::default(),
            show_ops_log: false,
            shot: ShotCfg::from_env(),
        }
    }

    /// Ctrl+Z — submit the inverse of the newest undoable op.
    pub fn undo(&mut self) {
        let Some(entry) = self.undo_stack.pop() else {
            self.toast("Nothing to undo".to_string());
            return;
        };
        let id = self
            .ops
            .submit(entry.inverse.clone(), flux_operations::ConflictPolicy::Ask);
        // Inverse ops don't re-journal (redo rides the explicit stack).
        self.journaled_ops.insert(id);
        self.toast(format!("Undoing {}", entry.label));
        self.redo_stack.push(entry);
    }

    /// Ctrl+Shift+Z / Ctrl+Y — re-run the last undone op.
    pub fn redo(&mut self) {
        let Some(entry) = self.redo_stack.pop() else {
            self.toast("Nothing to redo".to_string());
            return;
        };
        let id = self
            .ops
            .submit(entry.forward.clone(), flux_operations::ConflictPolicy::Ask);
        self.journaled_ops.insert(id);
        self.toast(format!("Redoing {}", entry.label));
        self.undo_stack.push(entry);
    }

    pub fn current_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn current_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    /// Directory workspace of the active tab — `None` for application
    /// tabs (Settings, Bulk Rename, …). Directory-only operations must
    /// go through this so a workspace tab can never fake a path.
    pub fn current_dir(&self) -> Option<&DirectoryTab> {
        self.current_tab().dir()
    }

    pub fn current_dir_mut(&mut self) -> Option<&mut DirectoryTab> {
        self.current_tab_mut().dir_mut()
    }

    pub fn current_snap(&self) -> Option<SnapshotHandle> {
        self.current_dir().map(|d| d.snap.clone())
    }

    pub fn current_path(&self) -> Option<PathBuf> {
        self.current_dir()
            .map(|d| d.snap.read().unwrap().path.clone())
    }

    /// Navigate a directory tab to `path`, pushing history. Cancels the
    /// in-flight scan implicitly via generation bump. When the active
    /// tab is an application workspace, navigation lands on the most
    /// recently used directory tab (creating one if none exists) —
    /// sidebar/drop actions stay usable from any workspace.
    pub fn navigate(&mut self, path: PathBuf) {
        self.icons.invalidate_evidence();
        if self.current_dir().is_none() {
            if let Some(i) = self.tabs.iter().rposition(|t| t.dir().is_some()) {
                self.active = i;
            } else {
                self.new_tab_at(path);
                return;
            }
        }
        let filter_text = self.filter_text.clone();
        let snap = {
            let Some(tab) = self.current_dir_mut() else {
                return;
            };
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
        if let Some(t) = self.current_dir_mut() {
            t.gen_id = g;
        }
        self.size_engine.cancel_current();
        self.watcher.watch(&path);
        self.free_space = flux_platform::free_space(&path);
        flux_settings::push_recent(&mut self.settings, path.clone());
        self.address_edit = None;
    }

    /// Re-enumerate the current directory, preserving selection by name.
    /// No-op on application tabs.
    pub fn refresh(&mut self) {
        self.icons.invalidate_evidence();
        let Some((snap, path)) = self.current_dir_mut().map(|tab| {
            let s = tab.snap.read().unwrap();
            let names = tab.sel.selected_names(&s);
            let path = s.path.clone();
            drop(s);
            tab.reselect_after_load = Some(names);
            (tab.snap.clone(), path)
        }) else {
            return;
        };
        let g = self.enum_engine.navigate(&snap, path.clone());
        if let Some(t) = self.current_dir_mut() {
            t.gen_id = g;
        }
        self.watcher.watch(&path);
    }

    pub fn go_up(&mut self) {
        let parent = self
            .current_path()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));
        if let Some(parent) = parent {
            self.navigate(parent);
        }
    }

    pub fn go_back(&mut self) {
        if let Some(p) = self.current_dir_mut().and_then(|t| t.history.back()) {
            self.navigate_no_history(p);
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(p) = self.current_dir_mut().and_then(|t| t.history.forward()) {
            self.navigate_no_history(p);
        }
    }

    fn navigate_no_history(&mut self, path: PathBuf) {
        self.icons.invalidate_evidence();
        let Some(snap) = self.current_dir_mut().map(|tab| {
            tab.sel.clear();
            tab.snap.clone()
        }) else {
            return;
        };
        let g = self.enum_engine.navigate(&snap, path.clone());
        if let Some(t) = self.current_dir_mut() {
            t.gen_id = g;
        }
        self.watcher.watch(&path);
        self.free_space = flux_platform::free_space(&path);
        self.address_edit = None;
    }

    pub fn open_entry(&mut self, idx: flux_core::EntryIdx) {
        let (path, is_dir) = {
            let Some(d) = self.current_dir() else {
                return;
            };
            let s = d.snap.read().unwrap();
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

    /// Selected entries as absolute paths — empty on workspace tabs.
    pub fn selected_paths(&self) -> Vec<PathBuf> {
        let Some(tab) = self.current_dir() else {
            return Vec::new();
        };
        let s = tab.snap.read().unwrap();
        tab.sel
            .ids
            .iter()
            .filter_map(|&i| s.entry_path(i))
            .collect()
    }

    /// Path of the focused row, or the sole selection, else None.
    pub fn focused_path(&self) -> Option<PathBuf> {
        let tab = self.current_dir()?;
        let s = tab.snap.read().unwrap();
        if let Some(row) = tab.sel.focus
            && let Some(idx) = s.row_index(row)
        {
            return s.entry_path(idx);
        }
        tab.sel.ids.iter().next().and_then(|&i| s.entry_path(i))
    }

    /// What the preview panel shows: an explicit Space pin wins while
    /// it's still inside the current directory; otherwise normal
    /// selection-driven preview applies.
    pub fn preview_target(&mut self) -> Option<PathBuf> {
        // Drop the pin when focus actually moved — pinning a hovered
        // file must not freeze the panel against real navigation.
        if self.preview_pin.is_some() && self.focused_path() != self.pin_anchor {
            self.preview_pin = None;
        }
        if let Some(pin) = &self.preview_pin {
            let under = self
                .current_path()
                .map(|cwd| pin.parent() == Some(cwd.as_path()));
            if under == Some(true) {
                return Some(pin.clone());
            }
            self.preview_pin = None;
        }
        self.focused_path()
    }

    /// Space key on a file row: preview the hovered file, else the
    /// keyboard-focused one, else the sole selection. Pressing Space on
    /// the same pinned file toggles the panel closed. Selection is
    /// never mutated — Space can't disturb copy/cut/rename targets.
    pub fn space_preview(&mut self) {
        let target = self.hovered_path.clone().or_else(|| self.focused_path());
        let Some(t) = target else { return };
        if self.settings.show_preview && self.preview_pin.as_ref() == Some(&t) {
            self.settings.show_preview = false;
            self.preview_pin = None;
            self.settings_dirty_at = Some(Instant::now());
            return;
        }
        self.pin_anchor = self.focused_path();
        self.preview_pin = Some(t);
        if !self.settings.show_preview {
            self.settings.show_preview = true;
            self.settings_dirty_at = Some(Instant::now());
        }
    }

    /// Apply `settings.ui_scale` and resize the OS window by the same
    /// ratio — egui zoom shrinks the logical space inside the same
    /// physical window, so the window must grow to keep content roomy.
    pub fn apply_ui_scale(&mut self, ctx: &egui::Context) {
        // Points are logical: physical = pts × ppp(zoom). Re-issuing the
        // current logical size after the zoom change scales the physical
        // window by exactly new/old.
        let size_pts = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
        ctx.set_zoom_factor(self.settings.ui_scale);
        if let Some(sz) = size_pts {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(sz));
        }
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.status_msg = Some((msg.into(), Instant::now()));
    }

    pub fn new_tab(&mut self) {
        // From a workspace tab, clone the most recent directory instead.
        let path = self.current_path().unwrap_or_else(|| {
            self.tabs
                .iter()
                .rev()
                .find_map(|t| t.dir().map(|d| d.snap.read().unwrap().path.clone()))
                .unwrap_or_else(flux_platform::home_dir)
        });
        self.new_tab_at(path);
    }

    pub fn new_tab_at(&mut self, path: PathBuf) {
        let mut t = Tab::directory(self.alloc_tab_id(), path.clone());
        if let Some(d) = t.dir_mut() {
            let mut s = d.snap.write().unwrap();
            s.show_hidden = self.settings.show_hidden;
            s.show_system = self.settings.show_system_files;
        }
        self.push_tab(t);
        self.navigate(path);
    }

    /// Close tab `idx`. A Bulk Rename workspace holding an unexecuted
    /// plan asks for confirmation first — closing must never silently
    /// discard a rename the user was mid-way through.
    pub fn close_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        if self.tabs[idx].has_pending_rename_plan() {
            self.dialog = Dialog::ConfirmCloseTab { idx };
            return;
        }
        self.force_close_tab(idx);
    }

    /// Close a tab unconditionally (post-confirmation / non-dirty).
    pub fn force_close_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        self.enum_engine.cancel_current();
        self.meta_engine.cancel_current();
        self.tabs.remove(idx);
        if self.tabs.is_empty() {
            // Never leave the window without a workspace — reopen Home.
            self.new_tab_at(flux_platform::home_dir());
            return;
        }
        if self.active > idx {
            self.active -= 1;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        self.arm_current_dir_tab();
    }

    /// After the active tab becomes a directory tab (activation or a
    /// neighboring close): kick deferred loads and re-arm the watcher.
    fn arm_current_dir_tab(&mut self) {
        let Some((snap, path, deferred)) = self.current_dir().map(|t| {
            (
                t.snap.clone(),
                t.snap.read().unwrap().path.clone(),
                t.deferred_load,
            )
        }) else {
            return;
        };
        if deferred {
            if let Some(t) = self.current_dir_mut() {
                t.deferred_load = false;
            }
            let g = self.enum_engine.navigate(&snap, path.clone());
            if let Some(t) = self.current_dir_mut() {
                t.gen_id = g;
            }
        }
        self.watcher.watch(&path);
        self.free_space = flux_platform::free_space(&path);
    }

    pub fn activate_tab(&mut self, idx: usize) {
        if idx == self.active || idx >= self.tabs.len() {
            return;
        }
        self.active = idx;
        self.arm_current_dir_tab();
    }

    pub fn set_filter(&mut self, text: String) {
        let Some(tab) = self.current_dir_mut() else {
            return;
        };
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
        let Some((snap, g)) = self.current_dir_mut().map(|tab| {
            tab.snap.write().unwrap().sort = spec;
            (tab.snap.clone(), tab.gen_id)
        }) else {
            return;
        };
        self.compute.rebuild(&snap, g);
    }

    /// Apply a sort spec to the most recently used directory tab —
    /// used by Settings, which has no snapshot of its own but should
    /// live-preview on the directory the user came from.
    pub fn set_sort_last_dir(&mut self, spec: SortSpec) {
        let Some((snap, g)) = self
            .tabs
            .iter_mut()
            .rev()
            .find_map(|t| t.dir_mut())
            .map(|d| {
                d.snap.write().unwrap().sort = spec;
                (d.snap.clone(), d.gen_id)
            })
        else {
            return;
        };
        self.compute.rebuild(&snap, g);
    }

    /// Path of the directory a workspace-agnostic action lands on — the
    /// active tab's dir, else the most recent directory tab's path.
    pub fn last_dir_path(&self) -> Option<PathBuf> {
        self.current_path().or_else(|| {
            self.tabs
                .iter()
                .rev()
                .find_map(|t| t.dir().map(|d| d.snap.read().unwrap().path.clone()))
        })
    }

    /// The directory a workspace-agnostic toggle should land on: the
    /// active tab if it's a directory, else the most recent dir tab
    /// (Settings changes should live-preview where the user will land).
    fn target_dir_mut(&mut self) -> Option<&mut DirectoryTab> {
        if self.current_dir().is_some() {
            return self.current_dir_mut();
        }
        self.tabs.iter_mut().rev().find_map(|t| t.dir_mut())
    }

    pub fn toggle_hidden(&mut self) {
        self.settings.show_hidden = !self.settings.show_hidden;
        let show_hidden = self.settings.show_hidden;
        if let Some((snap, g)) = self.target_dir_mut().map(|tab| {
            tab.snap.write().unwrap().show_hidden = show_hidden;
            (tab.snap.clone(), tab.gen_id)
        }) {
            self.compute.rebuild(&snap, g);
        }
        self.save_settings();
    }

    /// Toggle the right-side preview panel (Alt+P / status-bar button).
    pub fn toggle_preview(&mut self) {
        self.settings.show_preview = !self.settings.show_preview;
        self.save_settings();
    }

    /// Switch the file-pane presentation (persisted).
    pub fn set_view_mode(&mut self, mode: flux_settings::ViewMode) {
        if self.settings.view_mode != mode {
            self.settings.view_mode = mode;
            self.save_settings();
        }
    }

    /// Explorer's "hide protected operating system files" gate — a
    /// separate, stronger filter than `show_hidden` (Windows attributes;
    /// harmless no-op elsewhere). Used by the Settings dialog live preview.
    pub fn set_show_system_files(&mut self, show: bool) {
        self.settings.show_system_files = show;
        if let Some((snap, g)) = self.target_dir_mut().map(|tab| {
            tab.snap.write().unwrap().show_system = show;
            (tab.snap.clone(), tab.gen_id)
        }) {
            self.compute.rebuild(&snap, g);
        }
    }

    /// Flush debounced settings writes once they've been quiet long
    /// enough. Called every frame; `on_exit` persists unconditionally.
    pub fn flush_settings_debounce(&mut self) {
        const QUIET: std::time::Duration = std::time::Duration::from_millis(1200);
        if self.settings_dirty_at.is_some_and(|t| t.elapsed() >= QUIET) {
            self.settings_dirty_at = None;
            self.save_settings();
        }
    }

    pub fn save_settings(&mut self) {
        // Only directory tabs persist — workspace tabs (Settings, Bulk
        // Rename) are per-session state that must not resurrect stale.
        self.settings.tabs = self
            .tabs
            .iter()
            .filter_map(|t| t.dir())
            .map(|d| d.snap.read().unwrap().path.clone())
            .collect();
        self.settings.density = match self.density {
            Density::Compact => flux_settings::Density::Compact,
            Density::Standard => flux_settings::Density::Standard,
            Density::Comfortable => flux_settings::Density::Comfortable,
        };
        let _ = flux_settings::save(&self.settings);
    }

    /// Queue recursive size walks for the real directories among
    /// `items` (entries of the current snapshot only — the result lands
    /// in their size cell like deferred metadata).
    pub fn calculate_dir_sizes(&mut self, items: &[PathBuf]) {
        let Some((snap, gen_id)) = self.current_dir().map(|d| (d.snap.clone(), d.gen_id)) else {
            return;
        };
        let mut asked = 0u32;
        {
            let s = snap.read().unwrap();
            for it in items {
                let Some(idx) = s.entries().iter().position(|e| {
                    e.kind == flux_core::EntryKind::Dir
                        && e.size == flux_core::UNKNOWN_SIZE
                        && s.path.join(&e.name) == *it
                }) else {
                    continue;
                };
                self.size_engine
                    .request(&snap, gen_id, idx as u32, it.clone());
                asked += 1;
            }
        }
        if asked > 0 {
            self.toast(if asked == 1 {
                "Calculating folder size…".to_string()
            } else {
                format!("Calculating {asked} folder sizes…")
            });
        }
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
            let Some(tab) = self.current_dir_mut() else {
                return;
            };
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

    /// Extract `archive` into `dest_dir`. A fresh `dest_dir` is staged
    /// (cancel leaves nothing); an existing one merges with per-entry
    /// conflict resolution.
    pub fn extract_archive(&mut self, archive: PathBuf, dest_dir: PathBuf) {
        self.ops.submit(
            OpKind::ExtractArchive { archive, dest_dir },
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
        self.icons.set_source(self.settings.icon_source);
        // Enumeration events.
        let enum_events: Vec<_> = self.enum_engine.drain_events().collect();
        for ev in enum_events {
            match ev {
                EnumEvent::Finished { gen_id, .. } => {
                    let mut rescan_idx: Option<usize> = None;
                    for (i, tab) in self.tabs.iter_mut().enumerate() {
                        let Some(d) = tab.dir_mut() else {
                            continue;
                        };
                        if d.gen_id == gen_id {
                            // Final global sort on the compute thread.
                            self.compute.rebuild(&d.snap, gen_id);
                            // Deferred metadata: stat entries in background.
                            self.meta_engine.fill(&d.snap, gen_id);
                            if let Some(names) = d.reselect_after_load.take() {
                                d.pending_reselect = Some(names);
                            }
                            if let Ok(mut s) = d.snap.write() {
                                s.finish();
                            }
                            // A watcher event deferred during this scan —
                            // run exactly one trailing rescan.
                            if std::mem::take(&mut d.watcher_refresh_pending) {
                                rescan_idx = Some(i);
                            }
                        }
                    }
                    if let Some(i) = rescan_idx {
                        // Mirror refresh(): keep selection across the rescan.
                        let Some((snap, path)) = self.tabs[i].dir_mut().map(|d| {
                            let s = d.snap.read().unwrap();
                            let names = d.sel.selected_names(&s);
                            let path = s.path.clone();
                            drop(s);
                            d.reselect_after_load = Some(names);
                            (d.snap.clone(), path)
                        }) else {
                            continue;
                        };
                        let g = self.enum_engine.navigate(&snap, path);
                        if let Some(d) = self.tabs[i].dir_mut() {
                            d.gen_id = g;
                        }
                    }
                }
                EnumEvent::Error { gen_id, message } => {
                    let cur = self.current_dir().map(|d| d.gen_id);
                    if cur == Some(gen_id) {
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
                        let Some(d) = tab.dir_mut() else {
                            continue;
                        };
                        if d.gen_id == gen_id
                            && matches!(
                                d.snap.read().map(|s| s.sort.key),
                                Ok(SortKey::Size | SortKey::Modified)
                            )
                        {
                            self.compute.rebuild(&d.snap, gen_id);
                        }
                    }
                    ctx.request_repaint();
                }
                MetaEvent::Superseded { .. } => {}
            }
        }
        // Preview worker results — apply only while the path is still
        // the one in focus; uploads the texture for image previews.
        for msg in self.preview_engine.drain_events().collect::<Vec<_>>() {
            if self.preview_req.as_ref() == Some(&msg.path) {
                if let Some(crate::preview::Preview::Image(img)) = &msg.preview {
                    self.preview_tex = Some(ctx.load_texture(
                        "preview",
                        img.clone(),
                        egui::TextureOptions::LINEAR,
                    ));
                }
                if let Some(p) = msg.preview {
                    self.preview = Some(p);
                }
                if msg.meta.is_some() {
                    self.preview_meta = msg.meta;
                }
            }
            ctx.request_repaint();
        }
        // Opt-in metadata ops (SHA-256 / recursive folder size) — keep
        // only results for the path still being previewed.
        for msg in self.meta_ops.drain_events().collect::<Vec<_>>() {
            if self.preview_req.as_ref() == Some(&msg.path) {
                self.meta_op = Some(msg);
                ctx.request_repaint();
            }
        }
        // Opt-in folder sizes: apply_size already wrote under the snap
        // lock; events just drive repaint + resort when sorting by size.
        for ev in self.size_engine.drain_events().collect::<Vec<_>>() {
            if let flux_core::SizeEvent::Done { gen_id, .. } = ev {
                for tab in &mut self.tabs {
                    let Some(d) = tab.dir_mut() else {
                        continue;
                    };
                    if d.gen_id == gen_id
                        && matches!(d.snap.read().map(|s| s.sort.key), Ok(SortKey::Size))
                    {
                        self.compute.rebuild(&d.snap, gen_id);
                    }
                }
                ctx.request_repaint();
            }
        }
        // Compute events: apply deferred reselection + repaint.
        let compute_events: Vec<_> = self.compute.drain_events().collect();
        for ev in compute_events {
            if let ComputeEvent::ViewReady { gen_id } = ev {
                for tab in &mut self.tabs {
                    let Some(d) = tab.dir_mut() else {
                        continue;
                    };
                    if d.gen_id == gen_id
                        && let Some(names) = d.pending_reselect.take()
                    {
                        let s = d.snap.read().unwrap();
                        d.sel.reselect_names(&s, &names);
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
            // An enumeration already running for this tab will observe
            // the mid-write state; restarting it on every burst starves
            // a large, actively-written directory forever. Flag one
            // trailing refresh to run at Finished instead. Application
            // tabs have no watched dir — ignore the event entirely.
            let loading = self
                .current_dir()
                .and_then(|d| d.snap.read().ok().map(|s| s.state.is_loading()))
                .unwrap_or(false);
            if loading {
                if let Some(d) = self.current_dir_mut() {
                    d.watcher_refresh_pending = true;
                }
            } else {
                self.refresh();
            }
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
            // Journal cleanly-finished ops for undo. Only pure `Done`
            // counts — a partial or errored result can't be safely
            // inverted wholesale.
            for op in self.ops.all_ops() {
                if self.journaled_ops.contains(&op.id) {
                    continue;
                }
                let p = op.progress_snapshot();
                if !matches!(p.state, OpState::Done) {
                    continue;
                }
                self.journaled_ops.insert(op.id);
                if let Some(entry) = undo_entry(&op.kind, &op.label) {
                    if self.undo_stack.len() >= 50 {
                        self.undo_stack.remove(0);
                    }
                    self.undo_stack.push(entry);
                    self.redo_stack.clear();
                }
            }
            ctx.request_repaint();
        }
        // Icon fetches finished → upload + repaint.
        if self.icons.pump(ctx) > 0 {
            ctx.request_repaint();
        }
        // Thumbnail decodes finished → upload + repaint.
        if self.thumbs.pump(ctx) > 0 {
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
        // Bulk Rename previews: debounced recompute on the worker thread,
        // stale-generation results dropped. Every open workspace keeps
        // its worker alive while its tab exists — not just the active one.
        for tab in &mut self.tabs {
            let Some(br) = tab.bulk_rename_mut() else {
                continue;
            };
            if br.dirty
                && br.plan_rx.is_none()
                && br.last_edit.elapsed() > Duration::from_millis(220)
            {
                br.recompute_async();
            }
            if br.poll_plan() || br.plan_stale {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
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
                if let Some(p) = self.current_path() {
                    self.address_edit = Some(p.display().to_string());
                    self.focus_address = true;
                }
            }
            "focus_filter" if self.current_dir().is_some() => {
                self.focus_filter = true;
            }
            "new_folder" if self.current_dir().is_some() => {
                self.dialog = Dialog::NewFolder {
                    input: "New Folder".into(),
                };
            }
            "new_file" if self.current_dir().is_some() => {
                self.dialog = Dialog::NewFile {
                    input: "untitled".into(),
                };
            }
            "bulk_rename" => {
                let paths = self.actionable_paths();
                if !paths.is_empty() {
                    self.open_bulk_rename(paths);
                }
            }
            "open_settings" => self.open_settings(),
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
                if let Some(p) = self.current_path()
                    && let Err(e) = flux_platform::open_in_terminal(&p)
                {
                    self.toast(format!("Terminal: {e}"));
                }
            }
            "toggle_search" if self.current_dir().is_some() => {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Settings with zero restored tabs → one empty-state app.
    fn app() -> FluxApp {
        let mut s = Settings::default();
        s.tabs.clear();
        FluxApp::from_settings(s)
    }

    #[test]
    fn opens_one_dir_tab_at_home() {
        let a = app();
        assert_eq!(a.tabs.len(), 1);
        assert!(a.current_dir().is_some());
    }

    #[test]
    fn settings_tab_is_a_singleton() {
        let mut a = app();
        a.open_settings();
        assert_eq!(a.tabs.len(), 2);
        assert!(a.current_tab().settings().is_some());
        // Second open focuses the existing tab instead of duplicating.
        a.open_settings();
        assert_eq!(a.tabs.len(), 2);
        assert_eq!(a.active, 1);
    }

    #[test]
    fn workspace_state_survives_tab_switching() {
        let mut a = app();
        a.open_settings();
        a.current_tab_mut().settings_mut().unwrap().query = "theme".into();
        a.current_tab_mut().settings_mut().unwrap().category =
            crate::settings_page::Category::Appearance;
        a.activate_tab(0); // back to the directory
        assert!(a.current_dir().is_some());
        a.open_settings(); // singleton focus — same tab, same state
        let sui = a.current_tab().settings().unwrap();
        assert_eq!(sui.query, "theme");
        assert_eq!(sui.category, crate::settings_page::Category::Appearance);
    }

    #[test]
    fn bulk_rename_keeps_rules_and_sources_across_switches() {
        let mut a = app();
        a.open_bulk_rename(vec![PathBuf::from("/tmp/x"), PathBuf::from("/tmp/y")]);
        {
            let br = a.current_tab_mut().bulk_rename_mut().unwrap();
            br.find = "old".into();
            br.replace = "new".into();
        }
        a.activate_tab(0);
        a.activate_tab(1);
        let br = a.current_tab().bulk_rename().unwrap();
        assert_eq!(br.sources.len(), 2);
        assert_eq!(br.find, "old");
        assert_eq!(br.replace, "new");
    }

    #[test]
    fn dir_only_ops_are_inert_on_workspace_tabs() {
        let mut a = app();
        a.open_settings();
        assert!(a.current_dir().is_none());
        assert!(a.current_path().is_none());
        assert!(a.current_snap().is_none());
        assert!(a.selected_paths().is_empty());
        assert!(a.focused_path().is_none());
        // None of these may panic or fake a target on a workspace tab.
        a.refresh();
        a.set_filter("x".into());
        a.go_up();
        a.go_back();
        a.go_forward();
        a.open_entry(flux_core::EntryIdx::MAX);
        assert_eq!(a.active, 1); // still on the Settings tab
    }

    #[test]
    fn navigate_from_workspace_lands_on_a_dir_tab() {
        let mut a = app();
        let home = flux_platform::home_dir();
        a.open_settings();
        a.navigate(home.clone());
        // Navigation retargets the most recent directory tab, and the
        // workspace remains open behind it.
        assert_eq!(a.tabs.len(), 2);
        assert!(a.current_dir().is_some());
        assert!(a.tabs[1].settings().is_some());
    }

    #[test]
    fn esc_leaves_workspace_without_losing_it() {
        let mut a = app();
        a.open_settings();
        a.leave_workspace();
        assert!(a.current_dir().is_some());
        assert_eq!(a.tabs.len(), 2); // tab still open
    }

    #[test]
    fn close_workspace_tab_returns_to_dir() {
        let mut a = app();
        a.open_settings();
        a.close_tab(1);
        assert_eq!(a.tabs.len(), 1);
        assert!(a.current_dir().is_some());
    }

    #[test]
    fn closing_last_tab_reopens_home() {
        let mut a = app();
        a.close_tab(0);
        assert_eq!(a.tabs.len(), 1);
        assert!(a.current_dir().is_some());
        assert_eq!(a.current_path().unwrap(), flux_platform::home_dir());
    }

    #[test]
    fn pending_rename_plan_asks_before_closing() {
        let mut a = app();
        a.open_bulk_rename(vec![PathBuf::from("/tmp/x")]);
        {
            let br = a.current_tab_mut().bulk_rename_mut().unwrap();
            // Forge a ready action — close_tab only guards Ready plans.
            br.plan.push(flux_operations::RenameAction {
                src: PathBuf::from("/tmp/x"),
                dst: PathBuf::from("/tmp/y"),
                status: flux_operations::RenameStatus::Ready,
            });
        }
        let idx = a.active;
        a.close_tab(idx);
        assert!(matches!(a.dialog, Dialog::ConfirmCloseTab { .. }));
        assert_eq!(a.tabs.len(), 2); // still open
        a.force_close_tab(idx);
        assert_eq!(a.tabs.len(), 1);
    }

    #[test]
    fn only_dir_tabs_persist_to_settings() {
        let mut a = app();
        a.open_settings();
        a.open_bulk_rename(vec![PathBuf::from("/tmp/x")]);
        a.save_settings();
        assert_eq!(a.settings.tabs.len(), 1);
        assert!(a.settings.tabs[0].is_absolute() || a.settings.tabs[0] == PathBuf::new());
    }

    #[test]
    fn tab_titles_and_glyphs_distinguish_workspaces() {
        let mut a = app();
        a.open_settings();
        a.open_bulk_rename(vec![PathBuf::from("/tmp/x")]);
        assert_eq!(a.tabs[0].glyph(), crate::glyphs::Glyph::Folder);
        assert_eq!(a.tabs[1].glyph(), crate::glyphs::Glyph::Gear);
        assert_eq!(a.tabs[1].title(), "Settings");
        assert_eq!(a.tabs[2].glyph(), crate::glyphs::Glyph::Pencil);
        assert_eq!(a.tabs[2].title(), "Bulk Rename");
    }

    #[test]
    fn dir_tab_state_survives_workspace_switching() {
        let mut a = app();
        // Seed a marker into tab 0's selection state.
        if let Some(d) = a.current_dir_mut() {
            d.filter = "marker".into();
        }
        a.open_settings();
        a.activate_tab(0);
        assert_eq!(a.current_dir().unwrap().filter, "marker");
    }
}

#[cfg(test)]
mod space_tests {
    use super::*;
    use flux_core::DirEntry;

    fn file(name: &str) -> DirEntry {
        DirEntry {
            name: name.into(),
            kind: flux_core::EntryKind::File,
            size: 3,
            mtime: None,
            readonly: false,
            hidden: false,
            system: false,
        }
    }

    /// App with two files pushed into its snapshot; returns (app, cwd).
    fn app_with_files() -> (FluxApp, PathBuf) {
        let a = {
            let mut s = Settings::default();
            s.tabs.clear();
            FluxApp::from_settings(s)
        };
        let cwd = a.current_path().unwrap();
        a.current_dir()
            .unwrap()
            .snap
            .write()
            .unwrap()
            .push_batch(&mut vec![file("a.txt"), file("b.txt")]);
        (a, cwd)
    }

    #[test]
    fn space_pins_the_hovered_file() {
        let (mut a, cwd) = app_with_files();
        a.hovered_path = Some(cwd.join("b.txt"));
        a.space_preview();
        assert!(a.settings.show_preview);
        assert_eq!(a.preview_pin.as_deref(), Some(cwd.join("b.txt").as_path()));
        // Selection untouched — hover must not mutate copy/cut targets.
        assert!(a.current_dir().unwrap().sel.ids.is_empty());
    }

    #[test]
    fn space_toggles_same_pinned_file() {
        let (mut a, cwd) = app_with_files();
        a.hovered_path = Some(cwd.join("b.txt"));
        a.space_preview(); // open
        a.space_preview(); // same target → close
        assert!(!a.settings.show_preview);
        assert!(a.preview_pin.is_none());
        a.space_preview(); // and reopens
        assert!(a.settings.show_preview);
    }

    #[test]
    fn space_falls_back_to_focused_then_selection() {
        let (mut a, _cwd) = app_with_files();
        {
            let d = a.current_dir_mut().unwrap();
            let s = d.snap.read().unwrap();
            d.sel.select_row(&s, 1);
        }
        a.space_preview();
        let expect = a.focused_path();
        assert_eq!(a.preview_pin, expect);
    }

    #[test]
    fn pin_drops_when_focus_moves() {
        let (mut a, cwd) = app_with_files();
        a.hovered_path = Some(cwd.join("b.txt"));
        a.space_preview();
        assert_eq!(a.preview_target(), Some(cwd.join("b.txt")));
        // Focus moves to a.txt (click/arrow) → pin is stale, panel
        // follows the real selection again.
        a.hovered_path = None;
        {
            let d = a.current_dir_mut().unwrap();
            let s = d.snap.read().unwrap();
            d.sel.select_row(&s, 0);
        }
        assert_eq!(a.preview_target(), Some(cwd.join("a.txt")));
        assert!(a.preview_pin.is_none());
    }

    #[test]
    fn pin_outside_current_dir_is_ignored() {
        let (mut a, _cwd) = app_with_files();
        a.preview_pin = Some(PathBuf::from("/elsewhere/x.txt"));
        assert_eq!(a.preview_target(), a.focused_path());
        assert!(a.preview_pin.is_none());
    }
}
