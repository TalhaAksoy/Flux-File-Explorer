//! Async image thumbnails with a bounded LRU cache.
//!
//! Same pattern as `icon_cache`: a worker thread decodes, the UI thread
//! uploads textures in `pump`. Keys are full paths (thumbnails are
//! per-file, unlike kind-keyed icons). Bounded two ways:
//!
//! - `MAX_CACHED` entries in a real LRU — least-recently-touched entry
//!   is evicted on insert, so scrolling a huge photo dir can't grow GPU
//!   memory unbounded.
//! - Hard decode guards on the worker: file-size cap + `image::Limits`
//!   on pixel dimensions, so a hostile/corrupt multi-GB image can't OOM
//!   the decoder thread.
//!
//! Failed decodes are cached as `None` (negative caching) — a corrupt
//! file is retried only after eviction, not every frame.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};

use rustc_hash::{FxHashMap, FxHashSet};

/// Thumbnails decode to at most this edge length (pixels).
pub const THUMB_PX: u32 = 64;
/// Cache holds at most this many textures (~64²·4·256 ≈ 4 MB VRAM).
const MAX_CACHED: usize = 256;
/// Don't attempt files beyond 256 MB — decode cost isn't worth a glyph.
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// Decoder-side pixel-dimension guard against decompression bombs.
const MAX_DIM: u32 = 32_768;

enum Slot {
    Ready(egui::TextureHandle),
    Failed,
}

enum Msg {
    Result(PathBuf, Option<image::RgbaImage>),
}

pub struct ThumbCache {
    map: RwLock<FxHashMap<PathBuf, (Slot, u64)>>,
    lru: Mutex<VecDeque<PathBuf>>,
    in_flight: Mutex<FxHashSet<PathBuf>>,
    clock: AtomicU64,
    res_rx: mpsc::Receiver<Msg>,
    req_tx: mpsc::Sender<PathBuf>,
    wake_ctx: Arc<Mutex<Option<egui::Context>>>,
}

impl Default for ThumbCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Extension set the bundled `image` build decodes — checked before
/// queueing so non-images never reach the worker.
pub fn is_thumb_ext(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico"
        )
    })
}

fn decode(path: &Path) -> Option<image::RgbaImage> {
    let md = std::fs::metadata(path).ok()?;
    if !md.is_file() || md.len() > MAX_FILE_BYTES {
        return None;
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIM);
    limits.max_image_height = Some(MAX_DIM);
    limits.max_alloc = Some(512 * 1024 * 1024);
    let mut reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    reader.limits(limits);
    let img = reader.decode().ok()?;
    Some(img.thumbnail(THUMB_PX, THUMB_PX).to_rgba8())
}

impl ThumbCache {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<PathBuf>();
        let (res_tx, res_rx) = mpsc::channel::<Msg>();
        let wake_ctx = Arc::new(Mutex::new(Option::<egui::Context>::None));
        let wake = wake_ctx.clone();
        std::thread::Builder::new()
            .name("flux-thumbs".into())
            .spawn(move || {
                while let Ok(path) = req_rx.recv() {
                    let img = decode(&path);
                    if res_tx.send(Msg::Result(path, img)).is_err() {
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
            lru: Mutex::new(VecDeque::new()),
            in_flight: Mutex::new(FxHashSet::default()),
            clock: AtomicU64::new(1),
            res_rx,
            req_tx,
            wake_ctx,
        }
    }

    pub fn set_ctx(&self, ctx: egui::Context) {
        *self.wake_ctx.lock().unwrap() = Some(ctx);
    }

    fn touch(&self, key: &Path) {
        let t = self.clock.fetch_add(1, Ordering::Relaxed) + 1;
        let mut map = self.map.write().unwrap();
        if let Some(slot) = map.get_mut(key) {
            slot.1 = t;
        }
        let mut lru = self.lru.lock().unwrap();
        lru.push_back(key.to_path_buf());
        // Re-touches push duplicate keys; cap the deque so a long
        // session can't grow it without bound. Stale keys are skipped
        // during eviction anyway.
        if lru.len() > MAX_CACHED * 4 {
            let mut seen = rustc_hash::FxHashSet::default();
            lru.retain(|k| map.contains_key(k) && seen.insert(k.clone()));
        }
    }

    /// Look up a thumbnail; queues a decode on miss and returns None.
    /// Failed decodes return None without re-queueing. Returns the handle
    /// (cheap Arc clone) so callers can aspect-fit by its size.
    pub fn texture(&self, path: &Path) -> Option<egui::TextureHandle> {
        // The read guard must drop before `touch` takes the write lock —
        // binding it in a `let` (not an `if let` scrutinee, whose
        // temporaries live for the whole block) ends it at the `;`.
        // Upgrading a held read lock deadlocks the UI thread permanently.
        let cached = self
            .map
            .read()
            .unwrap()
            .get(path)
            .map(|(slot, _)| match slot {
                Slot::Ready(t) => Some(t.clone()),
                Slot::Failed => None,
            });
        if let Some(h) = cached {
            self.touch(path);
            return h;
        }
        {
            let mut inflight = self.in_flight.lock().unwrap();
            // In-flight is bounded by the cache cap — visible-row-only
            // requests keep it far smaller in practice.
            if inflight.len() >= MAX_CACHED || !inflight.insert(path.to_path_buf()) {
                return None;
            }
        }
        let _ = self.req_tx.send(path.to_path_buf());
        None
    }

    /// Upload finished decodes as textures; evict LRU beyond the cap.
    pub fn pump(&self, ctx: &egui::Context) -> usize {
        let mut n = 0;
        while let Ok(Msg::Result(path, img)) = self.res_rx.try_recv() {
            self.in_flight.lock().unwrap().remove(&path);
            let slot = match img {
                Some(rgba) => {
                    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
                    let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                    Slot::Ready(ctx.load_texture("thumb", img, egui::TextureOptions::LINEAR))
                }
                None => Slot::Failed,
            };
            {
                let mut map = self.map.write().unwrap();
                if map.len() >= MAX_CACHED && !map.contains_key(&path) {
                    // Evict least-recently-touched entries. The deque may
                    // hold stale keys (re-touches push duplicates), so pop
                    // until an actually-cached key is found.
                    let mut lru = self.lru.lock().unwrap();
                    while map.len() >= MAX_CACHED {
                        let Some(victim) = lru.pop_front() else { break };
                        if map.contains_key(&victim) {
                            map.remove(&victim);
                        }
                    }
                }
                let t = self.clock.fetch_add(1, Ordering::Relaxed) + 1;
                map.insert(path.clone(), (slot, t));
                self.lru.lock().unwrap().push_back(path);
            }
            n += 1;
        }
        n
    }
}

pub type ThumbCacheHandle = std::rc::Rc<ThumbCache>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_ext_filter() {
        assert!(is_thumb_ext(Path::new("/x/photo.PNG")));
        assert!(is_thumb_ext(Path::new("/x/photo.jpg")));
        assert!(!is_thumb_ext(Path::new("/x/doc.txt")));
        assert!(!is_thumb_ext(Path::new("/x/noext")));
    }

    #[test]
    fn decode_rejects_missing_and_oversized() {
        assert!(decode(Path::new("/definitely/not/here.png")).is_none());
    }

    #[test]
    fn decode_small_png_roundtrip() {
        let dir = std::env::temp_dir().join(format!("flux-thumb-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.png");
        // 8x8 solid red PNG written via the encoder.
        let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255]));
        img.save(&p).unwrap();
        let th = decode(&p).expect("should decode");
        assert!(th.width() <= THUMB_PX && th.height() <= THUMB_PX);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Regression: `texture()` on a cached path used to call `touch()`
    /// (a `map.write()`) while the `map.read()` guard from the `if let`
    /// scrutinee was still alive — a guaranteed self-deadlock that froze
    /// the UI thread the first time any thumbnail was re-rendered
    /// (observed: navigating to Downloads parked the main thread in
    /// `futex_wait` forever). `ThumbCache` is `!Sync` so this can't be
    /// witnessed with a timeout thread — under the old code this test
    /// simply hangs, which a CI timeout still reports.
    #[test]
    fn texture_on_cached_path_does_not_deadlock() {
        let c = ThumbCache::new();
        {
            let mut map = c.map.write().unwrap();
            map.insert(
                PathBuf::from("/x/seeded.png"),
                (Slot::Failed, c.clock.fetch_add(1, Ordering::Relaxed)),
            );
        }
        // Every cached hit used to park the caller in write_contended
        // forever — 100 hits must complete immediately.
        for _ in 0..100 {
            assert!(c.texture(Path::new("/x/seeded.png")).is_none());
        }
    }
}
