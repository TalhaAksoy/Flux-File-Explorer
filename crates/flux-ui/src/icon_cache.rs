//! Background icon fetching + texture cache.
//!
//! The OS icon call is synchronous and slow-ish (ms each), so it runs on a
//! dedicated worker. The UI requests icons for *visible rows only* and gets
//! `None` until the texture lands, falling back to painted glyphs. Cache is
//! keyed by file *kind* (folder / extension), not per-path — a `.png` icon
//! is the same for every png on macOS/Windows.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};

use flux_core::{DirEntry, EntryKind};
use rustc_hash::FxHashMap;

/// What makes an icon distinct. OsStr ext → lowercase String keeps the key
/// hashable and cheap.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum IconKey {
    Folder,
    Symlink,
    /// Lowercased extension; empty string = generic file.
    Ext(String),
    /// Items whose icon is genuinely per-path (e.g. ".app" bundles are dirs
    /// on disk but carry their own icon).
    Path(PathBuf),
}

impl IconKey {
    /// Cache key for an entry — O(1) for dirs, one small lowercase for files.
    /// `path` is the entry's absolute path (needed only for per-path keys).
    pub fn for_entry(e: &DirEntry, path: &std::path::Path) -> Self {
        match e.kind {
            EntryKind::Dir => {
                // ".app" bundles have unique icons per app. `DirEntry::extension`
                // returns None for dirs, so look at the name directly.
                let is_bundle = std::path::Path::new(&e.name)
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case(OsStr::new("app")));
                if is_bundle {
                    IconKey::Path(path.to_path_buf())
                } else {
                    IconKey::Folder
                }
            }
            EntryKind::Symlink => IconKey::Symlink,
            _ => IconKey::Ext(
                e.extension()
                    .map(|x| x.to_string_lossy().to_lowercase())
                    .unwrap_or_default(),
            ),
        }
    }
}

enum IconMsg {
    Result(IconKey, Option<flux_platform::FileIcon>),
}

pub struct IconCache {
    map: RwLock<FxHashMap<IconKey, egui::TextureHandle>>,
    in_flight: Mutex<HashSet<IconKey>>,
    res_rx: mpsc::Receiver<IconMsg>,
    /// Keep the sender for the worker.
    req_tx: mpsc::Sender<(IconKey, PathBuf)>,
    /// Set once at startup so the worker can wake a reactive egui loop.
    wake_ctx: Arc<Mutex<Option<egui::Context>>>,
}

const ICON_PX: u16 = 32;
/// Hard cap on cached textures; extension diversity is small in practice.
const MAX_CACHED: usize = 512;

impl Default for IconCache {
    fn default() -> Self {
        Self::new()
    }
}

impl IconCache {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<(IconKey, PathBuf)>();
        let (res_tx, res_rx) = mpsc::channel::<IconMsg>();
        let wake_ctx = Arc::new(Mutex::new(Option::<egui::Context>::None));
        let wake = wake_ctx.clone();
        std::thread::Builder::new()
            .name("flux-icons".into())
            .spawn(move || {
                while let Ok((key, path)) = req_rx.recv() {
                    let icon = flux_platform::file_icon(&path, ICON_PX);
                    if res_tx.send(IconMsg::Result(key, icon)).is_err() {
                        return;
                    }
                    if let Some(c) = wake.lock().unwrap().as_ref() {
                        c.request_repaint();
                    }
                }
            })
            .ok();
        Self {
            map: RwLock::new(FxHashMap::default()),
            in_flight: Mutex::new(HashSet::new()),
            res_rx,
            req_tx,
            wake_ctx,
        }
    }

    /// Hand the cache a Context so the worker can request repaints when
    /// results land (egui is reactive — without this icons stall).
    pub fn set_ctx(&self, ctx: egui::Context) {
        *self.wake_ctx.lock().unwrap() = Some(ctx);
    }

    /// Look up a texture; if missing, queue a fetch and return None.
    /// `path` is the absolute path used as the fetch's representative.
    pub fn texture(&self, key: &IconKey, path: &std::path::Path) -> Option<egui::TextureId> {
        if let Some(t) = self.map.read().unwrap().get(key) {
            return Some(t.id());
        }
        {
            let mut inflight = self.in_flight.lock().unwrap();
            if inflight.len() >= MAX_CACHED || !inflight.insert(key.clone()) {
                return None;
            }
        }
        let _ = self.req_tx.send((key.clone(), path.to_path_buf()));
        None
    }

    /// Drain completed fetches; upload textures on the UI thread (GL upload
    /// must happen here, not on the worker). Returns count of new icons.
    pub fn pump(&self, ctx: &egui::Context) -> usize {
        let mut n = 0;
        while let Ok(IconMsg::Result(key, icon)) = self.res_rx.try_recv() {
            self.in_flight.lock().unwrap().remove(&key);
            if let Some(icon) = icon {
                let img = egui::ColorImage::from_rgba_unmultiplied(
                    [icon.width as usize, icon.height as usize],
                    &icon.rgba,
                );
                let tex =
                    ctx.load_texture("icon", img, egui::TextureOptions::LINEAR);
                let mut map = self.map.write().unwrap();
                if map.len() >= MAX_CACHED {
                    // Evict: simple whole-cache reset — icons re-request on
                    // next visible frame; keeps eviction O(1) instead of LRU.
                    map.clear();
                }
                map.insert(key, tex);
            }
            n += 1;
        }
        n
    }
}

/// UI-thread-only (the receiver and textures are !Sync). Rc, not Arc.
pub type IconCacheHandle = std::rc::Rc<IconCache>;
