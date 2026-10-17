//! Shared file presentation: original vector identities plus bounded,
//! asynchronous native artwork. No filesystem/native calls while painting.
use crate::{file_icons, icon_evidence::EvidenceCache, theme::Palette};
use flux_core::{
    DirEntry, EntryKind,
    file_type::{self, IconId},
};
use flux_settings::IconSource;
use rustc_hash::{FxHashMap, FxHashSet};
use std::{
    cell::{Cell, RefCell},
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum IconKey {
    Folder,
    Symlink,
    Type(IconId),
    Ext(String),
    Path(PathBuf),
}
impl IconKey {
    pub fn for_entry(e: &DirEntry, path: &Path) -> Self {
        Self::for_entry_path(e.kind, path)
    }
    pub fn for_entry_path(kind: EntryKind, path: &Path) -> Self {
        let id = file_type::resolve(kind, path);
        match id {
            IconId::Folder => Self::Folder,
            IconId::Symlink => Self::Symlink,
            IconId::App | IconId::Framework | IconId::Xcode => Self::Path(path.to_owned()),
            IconId::Exe if cfg!(target_os = "windows") => Self::Path(path.to_owned()),
            _ => Self::Type(id),
        }
    }
}
#[derive(Clone, PartialEq, Eq, Hash)]
struct Request {
    key: IconKey,
    pixels: u16,
    dark: bool,
    generation: u64,
}
struct Cached {
    texture: Option<egui::TextureHandle>,
    touched: u64,
    bytes: usize,
}
struct Message {
    request: Request,
    icon: Option<flux_platform::FileIcon>,
}

pub const MAX_CACHED: usize = 512;
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PENDING: usize = 32;
const UPLOADS_PER_FRAME: usize = 8;

#[derive(Clone, Copy, Default, Debug)]
pub struct IconStats {
    pub native_hits: u64,
    pub native_misses: u64,
    pub requests: u64,
    pub bundled_draws: u64,
    pub native_draws: u64,
    pub entries: usize,
    pub texture_bytes: usize,
    pub pending: usize,
}

pub struct IconCache {
    map: RefCell<FxHashMap<Request, Cached>>,
    pending: RefCell<FxHashSet<Request>>,
    tx: mpsc::SyncSender<(Request, PathBuf)>,
    rx: mpsc::Receiver<Message>,
    wake: Arc<Mutex<Option<egui::Context>>>,
    clock: Cell<u64>,
    source: Cell<IconSource>,
    path_generation: Cell<u64>,
    stats: Cell<IconStats>,
    evidence: EvidenceCache,
}
impl Default for IconCache {
    fn default() -> Self {
        Self::new()
    }
}
impl IconCache {
    pub fn new() -> Self {
        let (tx, jobs) = mpsc::sync_channel::<(Request, PathBuf)>(MAX_PENDING);
        let (results, rx) = mpsc::sync_channel(MAX_PENDING);
        let wake = Arc::new(Mutex::new(None::<egui::Context>));
        let w = wake.clone();
        std::thread::Builder::new()
            .name("flux-icons".into())
            .spawn(move || {
                while let Ok((request, path)) = jobs.recv() {
                    let icon = flux_platform::file_icon(&path, request.pixels).and_then(|icon| {
                        let expected = (icon.width as usize)
                            .checked_mul(icon.height as usize)?
                            .checked_mul(4)?;
                        if icon.width == 0
                            || icon.height == 0
                            || expected != icon.rgba.len()
                            || expected > 4 * 1024 * 1024
                        {
                            return None;
                        }
                        if icon.width > request.pixels as u32 || icon.height > request.pixels as u32
                        {
                            let img =
                                image::RgbaImage::from_raw(icon.width, icon.height, icon.rgba)?;
                            let factor = (request.pixels as f32 / icon.width as f32)
                                .min(request.pixels as f32 / icon.height as f32);
                            let resized = image::imageops::resize(
                                &img,
                                (icon.width as f32 * factor).max(1.0) as u32,
                                (icon.height as f32 * factor).max(1.0) as u32,
                                image::imageops::FilterType::Triangle,
                            );
                            Some(flux_platform::FileIcon {
                                width: resized.width(),
                                height: resized.height(),
                                rgba: resized.into_raw(),
                            })
                        } else {
                            Some(icon)
                        }
                    });
                    if results.send(Message { request, icon }).is_err() {
                        break;
                    }
                    if let Some(ctx) = w.lock().unwrap().as_ref() {
                        ctx.request_repaint();
                    }
                }
            })
            .expect("spawn native icon worker");
        Self {
            map: Default::default(),
            pending: Default::default(),
            tx,
            rx,
            wake,
            clock: Cell::new(0),
            source: Cell::new(IconSource::Automatic),
            path_generation: Cell::new(0),
            stats: Cell::new(IconStats::default()),
            evidence: EvidenceCache::new(),
        }
    }
    pub fn set_ctx(&self, ctx: egui::Context) {
        *self.wake.lock().unwrap() = Some(ctx.clone());
        self.evidence.set_ctx(ctx);
    }
    pub fn set_source(&self, source: IconSource) {
        self.source.set(source);
    }
    pub fn source(&self) -> IconSource {
        self.source.get()
    }
    pub fn invalidate_evidence(&self) {
        self.evidence.invalidate();
        self.path_generation
            .set(self.path_generation.get().wrapping_add(1));
        self.map
            .borrow_mut()
            .retain(|request, _| !matches!(request.key, IconKey::Path(_)));
    }
    fn request(&self, key: IconKey, pixels: u16, dark: bool) -> Request {
        let generation = if matches!(key, IconKey::Path(_)) {
            self.path_generation.get()
        } else {
            0
        };
        Request {
            key,
            pixels,
            dark,
            generation,
        }
    }
    pub fn file_type(&self, kind: EntryKind, path: &Path) -> IconId {
        self.evidence.resolved(kind, path, false)
    }
    pub fn stats(&self) -> IconStats {
        let mut s = self.stats.get();
        let map = self.map.borrow();
        s.entries = map.len();
        s.texture_bytes = map.values().map(|v| v.bytes).sum();
        s.pending = self.pending.borrow().len();
        s
    }
    fn tick(&self) -> u64 {
        let t = self.clock.get().wrapping_add(1);
        self.clock.set(t);
        t
    }
    fn native_key(key: &IconKey, path: &Path) -> IconKey {
        match key {
            IconKey::Type(_) => IconKey::Ext(
                path.extension()
                    .unwrap_or(OsStr::new(""))
                    .to_string_lossy()
                    .to_ascii_lowercase(),
            ),
            _ => key.clone(),
        }
    }
    fn lookup(&self, request: &Request, path: &Path, queue: bool) -> Option<egui::TextureId> {
        let tick = self.tick();
        let mut stats = self.stats.get();
        if let Some(c) = self.map.borrow_mut().get_mut(request) {
            c.touched = tick;
            stats.native_hits += 1;
            self.stats.set(stats);
            return c.texture.as_ref().map(|t| t.id());
        }
        stats.native_misses += 1;
        if queue {
            let mut pending = self.pending.borrow_mut();
            if pending.len() < MAX_PENDING && pending.insert(request.clone()) {
                if self
                    .tx
                    .try_send((request.clone(), path.to_owned()))
                    .is_err()
                {
                    pending.remove(request);
                } else {
                    stats.requests += 1;
                }
            }
        }
        self.stats.set(stats);
        None
    }
    /// Legacy native-cache entry point; shared renderer below handles policy.
    pub fn texture(&self, key: &IconKey, path: &Path) -> Option<egui::TextureId> {
        self.lookup(
            &self.request(Self::native_key(key, path), 32, false),
            path,
            true,
        )
    }
    pub fn peek(&self, key: &IconKey) -> Option<egui::TextureId> {
        self.map
            .borrow()
            .iter()
            .filter(|(r, _)| r.key == *key)
            .max_by_key(|(_, v)| v.touched)
            .and_then(|(_, v)| v.texture.as_ref().map(|t| t.id()))
    }
    pub fn paint(
        &self,
        p: &egui::Painter,
        rect: egui::Rect,
        kind: EntryKind,
        path: &Path,
        pal: &Palette,
    ) {
        self.paint_impl(p, rect, kind, path, pal, true);
    }
    pub fn paint_cached(
        &self,
        p: &egui::Painter,
        rect: egui::Rect,
        kind: EntryKind,
        path: &Path,
        pal: &Palette,
    ) {
        self.paint_impl(p, rect, kind, path, pal, false);
    }
    fn paint_impl(
        &self,
        p: &egui::Painter,
        rect: egui::Rect,
        kind: EntryKind,
        path: &Path,
        pal: &Palette,
        queue: bool,
    ) {
        let id = self.evidence.resolved(kind, path, queue);
        let native_available = cfg!(any(target_os = "windows", target_os = "macos"));
        let application = matches!(id, IconId::App | IconId::Framework | IconId::Xcode)
            || (cfg!(target_os = "windows") && id == IconId::Exe);
        if native_available
            && (self.source.get() == IconSource::System
                || (self.source.get() == IconSource::Automatic && application))
        {
            let key = IconKey::for_entry_path(kind, path);
            let pixels = size_bucket(rect.width().max(rect.height()) * p.ctx().pixels_per_point());
            let request = self.request(
                Self::native_key(&key, path),
                pixels,
                pal.bg_content.r() < 128,
            );
            if let Some(texture) = self.lookup(&request, path, queue) {
                p.image(
                    texture,
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                let mut s = self.stats.get();
                s.native_draws += 1;
                self.stats.set(s);
                return;
            }
        }
        file_icons::paint(p, rect, id, pal);
        let mut s = self.stats.get();
        s.bundled_draws += 1;
        self.stats.set(s);
    }
    /// Main-thread uploads are limited per frame. Both request and result
    /// queues are bounded; negative entries participate in the same LRU.
    pub fn pump(&self, ctx: &egui::Context) -> usize {
        let mut n = self.evidence.pump();
        for _ in 0..UPLOADS_PER_FRAME {
            let Ok(msg) = self.rx.try_recv() else {
                break;
            };
            self.pending.borrow_mut().remove(&msg.request);
            if matches!(msg.request.key, IconKey::Path(_))
                && msg.request.generation != self.path_generation.get()
            {
                continue;
            }
            let bytes = msg.icon.as_ref().map_or(0, |i| i.rgba.len());
            let mut map = self.map.borrow_mut();
            trim_cache(&mut map, bytes);
            let texture = msg.icon.map(|i| {
                ctx.load_texture(
                    "native-file-icon",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [i.width as usize, i.height as usize],
                        &i.rgba,
                    ),
                    egui::TextureOptions::LINEAR,
                )
            });
            map.insert(
                msg.request,
                Cached {
                    texture,
                    bytes,
                    touched: self.tick(),
                },
            );
            n += 1;
        }
        if !self.pending.borrow().is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        n
    }
}

fn trim_cache(map: &mut FxHashMap<Request, Cached>, incoming_bytes: usize) {
    let mut used: usize = map.values().map(|v| v.bytes).sum();
    while !map.is_empty() && (map.len() >= MAX_CACHED || used + incoming_bytes > MAX_BYTES) {
        let victim = map
            .iter()
            .min_by_key(|(_, v)| v.touched)
            .map(|(k, _)| k.clone())
            .unwrap();
        used = used.saturating_sub(map.remove(&victim).unwrap().bytes);
    }
}

pub fn size_bucket(pixels: f32) -> u16 {
    [16, 24, 32, 48, 64, 96, 128, 192, 256, 384, 512]
        .into_iter()
        .find(|&s| s as f32 >= pixels)
        .unwrap_or(512)
}
pub type IconCacheHandle = std::rc::Rc<IconCache>;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_sharing_and_native_application_keys() {
        assert_eq!(
            IconKey::for_entry_path(EntryKind::File, Path::new("a.ts")),
            IconKey::for_entry_path(EntryKind::File, Path::new("b.ts"))
        );
        assert_ne!(
            IconKey::for_entry_path(EntryKind::File, Path::new("Cargo.toml")),
            IconKey::for_entry_path(EntryKind::File, Path::new("generic.toml"))
        );
        assert_ne!(
            IconKey::for_entry_path(EntryKind::Dir, Path::new("A.app")),
            IconKey::for_entry_path(EntryKind::Dir, Path::new("B.app"))
        );
        for (pixels, want) in [
            (16.0, 16),
            (17.0, 24),
            (33.0, 48),
            (100.0, 128),
            (400.0, 512),
        ] {
            assert_eq!(size_bucket(pixels), want);
        }
    }
    #[test]
    fn bounded_queue_and_negative_cache() {
        let cache = IconCache::new();
        let ctx = egui::Context::default();
        let path = std::env::temp_dir();
        for i in 0..1000 {
            cache.texture(&IconKey::Ext(format!("unknown{i}")), &path);
        }
        assert!(cache.stats().pending <= MAX_PENDING);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while cache.stats().pending > 0 {
            cache.pump(&ctx);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(cache.stats().entries <= MAX_CACHED);
        assert!(cache.stats().texture_bytes <= MAX_BYTES);
        let key = IconKey::Folder;
        cache.texture(&key, &path);
        while cache.stats().pending > 0 {
            cache.pump(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let requests = cache.stats().requests;
        cache.texture(&key, &path);
        assert_eq!(cache.stats().requests, requests);
        cache.set_source(IconSource::Flux);
        assert!(
            cache.stats().entries > 0,
            "source preference flushed unrelated native entries"
        );
    }
    #[test]
    fn lru_does_not_clear_hot_entries() {
        let cache = IconCache::new();
        let hot = Request {
            key: IconKey::Folder,
            pixels: 32,
            dark: false,
            generation: 0,
        };
        cache.map.borrow_mut().insert(
            hot.clone(),
            Cached {
                texture: None,
                touched: 10000,
                bytes: 0,
            },
        );
        for i in 0..MAX_CACHED - 1 {
            cache.map.borrow_mut().insert(
                Request {
                    key: IconKey::Ext(i.to_string()),
                    pixels: 32,
                    dark: false,
                    generation: 0,
                },
                Cached {
                    texture: None,
                    touched: i as u64,
                    bytes: 0,
                },
            );
        }
        trim_cache(&mut cache.map.borrow_mut(), 0);
        assert!(cache.map.borrow().contains_key(&hot));
        assert_eq!(cache.stats().entries, MAX_CACHED - 1);
    }
}
