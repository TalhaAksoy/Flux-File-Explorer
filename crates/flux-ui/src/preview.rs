//! Preview panel — right-side column for the focused item. All IO and
//! decoding happens on dedicated workers; the panel only renders the
//! last completed result, so a fast selection change can never flash a
//! stale preview and no metadata call ever blocks the UI thread.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::SystemTime;

use crate::app::FluxApp;
use crate::theme::Palette;

/// Bound a decoded image to fit comfortably inside the panel; thumbnails
/// and previews share this ceiling so huge raws can't OOM the decode.
const MAX_DIM: u32 = 1600;
/// Bytes sampled for the text sniff — UTF-8 check runs on this chunk.
const TEXT_SNIFF: usize = 64 * 1024;
/// Text preview line cap: enough for real files, bounded for paint cost.
const TEXT_MAX_LINES: usize = 500;
/// Child count shown for folders (stop counting beyond this).
const DIR_COUNT_CAP: usize = 10_000;
/// Recursive walks give up past this many entries — a pathological tree
/// must not keep the ops worker busy forever.
const WALK_CAP: u64 = 500_000;

/// What the worker produced for one path.
pub enum Preview {
    /// Decoded image (already size-bounded to [`MAX_DIM`]).
    Image(egui::ColorImage),
    /// First chunk of UTF-8-ish text.
    Text(String),
    /// Short status line for special entries (dir count, link target).
    Info(String),
    /// Nothing meaningful to render (binary, unreadable, gone).
    Empty,
}

/// Unix-specific fields from `MetadataExt` — all cheap stat fields.
pub struct UnixMeta {
    /// Permission bits only (file-type bits masked out).
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    /// Resolved names where the passwd/group databases know the id.
    pub owner: Option<String>,
    pub group: Option<String>,
    pub nlink: u64,
    pub inode: u64,
    pub dev: u64,
    /// `st_blocks * 512` — allocated disk usage where the fs reports it.
    pub allocated: Option<u64>,
    /// st_blksize — preferred IO block size.
    pub blksize: u64,
}

/// Windows `file_attributes()` decoded into the flag names that are set.
/// This is deliberately NOT an ACL dump — NTFS DACLs need dedicated APIs.
pub struct WinAttrs {
    pub flags: Vec<&'static str>,
}

/// Image header + EXIF information — gathered without a full decode
/// where the codec supports header-only inspection.
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    /// Container format ("PNG", "JPEG", …) detected from magic bytes.
    pub format: String,
    /// Pixel layout + bit depth ("RGB 8-bit", "RGBA 16-bit", …).
    pub color: Option<String>,
    pub alpha: Option<bool>,
    /// Curated EXIF rows — only fields the file actually carries.
    pub exif: Vec<(&'static str, String)>,
}

/// Volume information for the filesystem containing the path.
pub struct VolumeMeta {
    /// Filesystem type where the OS exposes it (Linux `/proc/mounts`).
    pub fs_type: Option<String>,
    /// Mount point the path lives under.
    pub mount: Option<PathBuf>,
    pub total: u64,
    pub available: u64,
}

/// Filesystem metadata for the previewed path, gathered on the worker
/// so the panel never stats the disk on the UI thread. Optional fields
/// are `None` when the platform or filesystem can't supply the value —
/// the UI renders "—" rather than inventing data.
pub struct PreviewMeta {
    /// Human-readable classification ("Folder", "PNG image", "Link").
    pub kind: String,
    /// MIME type guessed from the extension/content type table.
    pub mime: Option<String>,
    /// Logical file size; `None` for dirs (their size needs recursion).
    pub size: Option<u64>,
    /// Allocated size (blocks × 512) where the fs reports it — unix.
    pub allocated: Option<u64>,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub link_target: Option<PathBuf>,
    /// Symlink whose target doesn't resolve.
    pub link_broken: bool,
    /// Immediate child count for directories (capped at DIR_COUNT_CAP).
    pub dir_items: Option<u64>,
    pub modified: Option<SystemTime>,
    /// Birth time where the platform reports it (btime/st_birthtime).
    pub created: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    /// Unix st_ctime — last METADATA change, not creation time.
    pub changed: Option<SystemTime>,
    pub readonly: bool,
    pub hidden: bool,
    /// User-execute bit (unix) or executable extension (windows).
    pub executable: bool,
    pub unix: Option<UnixMeta>,
    pub windows: Option<WinAttrs>,
    pub volume: Option<VolumeMeta>,
    pub image: Option<ImageMeta>,
}

/// One worker delivery — `meta` ships alone in phase 1 (fast stat);
/// `preview` follows in phase 2 once pixels/text are ready. The app
/// applies each non-None field independently.
pub struct PreviewMsg {
    pub path: PathBuf,
    pub preview: Option<Preview>,
    pub meta: Option<PreviewMeta>,
}

/// Latest-request-wins worker. Results carry the requested path — the
/// app drops anything that doesn't match the item currently in focus.
pub struct PreviewEngine {
    req_tx: mpsc::Sender<PathBuf>,
    event_rx: mpsc::Receiver<PreviewMsg>,
    _worker: std::thread::JoinHandle<()>,
}

impl Default for PreviewEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewEngine {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<PathBuf>();
        let (ev_tx, event_rx) = mpsc::sync_channel::<PreviewMsg>(8);
        let worker = std::thread::Builder::new()
            .name("flux-preview".into())
            .spawn(move || {
                while let Ok(mut path) = req_rx.recv() {
                    // Coalesce bursts: only the newest request matters.
                    while let Ok(newer) = req_rx.try_recv() {
                        path = newer;
                    }
                    // Phase 1: stat + cheap probes — the panel fills its
                    // metadata sections while the visual still renders.
                    let (prior, mut meta) = gather(&path);
                    if let Some(m) = meta.as_mut()
                        && is_image_path(&path)
                        && !m.is_dir
                        && !m.is_symlink
                    {
                        m.image = image_probe(&path);
                    }
                    let done = matches!(prior, Preview::Info(_));
                    let _ = ev_tx.try_send(PreviewMsg {
                        path: path.clone(),
                        preview: done.then_some(prior),
                        meta,
                    });
                    if done {
                        continue;
                    }
                    // Phase 2: image decode / text sniff — potentially
                    // slow, delivered separately.
                    let preview = render(&path);
                    let _ = ev_tx.try_send(PreviewMsg {
                        path,
                        preview: Some(preview),
                        meta: None,
                    });
                }
            })
            .expect("spawn preview worker");
        Self {
            req_tx,
            event_rx,
            _worker: worker,
        }
    }

    pub fn request(&self, path: PathBuf) {
        let _ = self.req_tx.send(path);
    }

    pub fn drain_events(&self) -> impl Iterator<Item = PreviewMsg> + '_ {
        self.event_rx.try_iter()
    }
}

/// Opt-in expensive computations for the preview target — recursive
/// folder size and SHA-256. These NEVER run automatically; the panel
/// shows a "Calculate"/"Compute" button instead.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetaOp {
    /// Recursive walk: file/dir counts + logical bytes.
    DirSize,
    /// Streaming SHA-256 of the file's contents.
    Sha256,
}

#[derive(Debug)]
pub enum MetaOpOut {
    DirSize { files: u64, dirs: u64, bytes: u64 },
    Sha256(String),
}

#[derive(Debug)]
pub struct MetaOpMsg {
    pub path: PathBuf,
    pub op: MetaOp,
    pub out: Result<MetaOpOut, String>,
}

/// One sequential worker for opt-in ops, latest-wins + generation
/// cancellation (a stale hash aborts at the next 1 MiB chunk boundary).
pub struct MetaOps {
    req_tx: mpsc::Sender<(PathBuf, MetaOp)>,
    event_rx: mpsc::Receiver<MetaOpMsg>,
    generation: Arc<AtomicU64>,
    _worker: std::thread::JoinHandle<()>,
}

impl Default for MetaOps {
    fn default() -> Self {
        Self::new()
    }
}

impl MetaOps {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<(PathBuf, MetaOp)>();
        let (ev_tx, event_rx) = mpsc::sync_channel::<MetaOpMsg>(8);
        let generation = Arc::new(AtomicU64::new(0));
        let gen_w = Arc::clone(&generation);
        let worker = std::thread::Builder::new()
            .name("flux-meta-ops".into())
            .spawn(move || {
                while let Ok((mut path, mut op)) = req_rx.recv() {
                    while let Ok(newer) = req_rx.try_recv() {
                        (path, op) = newer;
                    }
                    let mine = gen_w.load(Ordering::SeqCst);
                    let out = match op {
                        MetaOp::DirSize => dir_size(&path, &gen_w, mine),
                        MetaOp::Sha256 => sha256(&path, &gen_w, mine),
                    };
                    let _ = ev_tx.try_send(MetaOpMsg { path, op, out });
                }
            })
            .expect("spawn meta-ops worker");
        Self {
            req_tx,
            event_rx,
            generation,
            _worker: worker,
        }
    }

    pub fn request(&self, path: PathBuf, op: MetaOp) {
        // Invalidate any in-flight op — it aborts at the next checkpoint.
        self.generation.fetch_add(1, Ordering::SeqCst);
        let _ = self.req_tx.send((path, op));
    }

    pub fn drain_events(&self) -> impl Iterator<Item = MetaOpMsg> + '_ {
        self.event_rx.try_iter()
    }
}

fn aborted(generation: &AtomicU64, mine: u64) -> bool {
    generation.load(Ordering::SeqCst) != mine
}

/// Iterative recursive walk — no symlink following, so a link cycle
/// can't loop forever; errors are skipped, not fatal.
fn dir_size(path: &Path, generation: &AtomicU64, mine: u64) -> Result<MetaOpOut, String> {
    let mut stack = vec![path.to_path_buf()];
    let (mut files, mut dirs, mut bytes) = (0u64, 0u64, 0u64);
    while let Some(dir) = stack.pop() {
        if aborted(generation, mine) {
            return Err("cancelled".into());
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            if aborted(generation, mine) {
                return Err("cancelled".into());
            }
            if files + dirs >= WALK_CAP {
                return Err("too many items".into());
            }
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                dirs += 1;
                stack.push(e.path());
            } else {
                files += 1;
                bytes += md.len();
            }
        }
    }
    Ok(MetaOpOut::DirSize { files, dirs, bytes })
}

/// Streaming SHA-256 — 1 MiB chunks with a stale-generation check per
/// chunk so a newer selection can't queue behind a multi-GB hash.
fn sha256(path: &Path, generation: &AtomicU64, mine: u64) -> Result<MetaOpOut, String> {
    use sha2::Digest;
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = sha2::Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        if aborted(generation, mine) {
            return Err("cancelled".into());
        }
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let digest = h.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    Ok(MetaOpOut::Sha256(hex))
}

/// Decode bounded the same way `thumbs::decode` does — dimension and
/// allocation caps so a decompression-bomb image can't OOM the worker.
fn decode_image(path: &Path) -> Option<image::RgbaImage> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(512 * 1024 * 1024);
    let mut reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    reader.limits(limits);
    let img = reader.decode().ok()?;
    Some(img.thumbnail(MAX_DIM, MAX_DIM).to_rgba8())
}

fn is_image_path(path: &Path) -> bool {
    path.extension()
        .map(|e| {
            matches!(
                e.to_string_lossy().to_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico"
            )
        })
        .unwrap_or(false)
}

fn format_name(f: image::ImageFormat) -> String {
    match f {
        image::ImageFormat::Png => "PNG",
        image::ImageFormat::Jpeg => "JPEG",
        image::ImageFormat::Gif => "GIF",
        image::ImageFormat::WebP => "WebP",
        image::ImageFormat::Bmp => "BMP",
        image::ImageFormat::Ico => "ICO",
        image::ImageFormat::Tiff => "TIFF",
        image::ImageFormat::Tga => "TGA",
        image::ImageFormat::Dds => "DDS",
        image::ImageFormat::Hdr => "HDR",
        image::ImageFormat::OpenExr => "OpenEXR",
        image::ImageFormat::Farbfeld => "Farbfeld",
        image::ImageFormat::Avif => "AVIF",
        image::ImageFormat::Qoi => "QOI",
        image::ImageFormat::Pnm => "PNM",
        _ => return format!("{f:?}"),
    }
    .to_string()
}

fn color_name(c: image::ColorType) -> (&'static str, bool) {
    use image::ColorType::*;
    match c {
        L8 => ("Grayscale 8-bit", false),
        La8 => ("Grayscale+Alpha 8-bit", true),
        Rgb8 => ("RGB 8-bit", false),
        Rgba8 => ("RGBA 8-bit", true),
        L16 => ("Grayscale 16-bit", false),
        La16 => ("Grayscale+Alpha 16-bit", true),
        Rgb16 => ("RGB 16-bit", false),
        Rgba16 => ("RGBA 16-bit", true),
        Rgb32F => ("RGB 32-bit float", false),
        Rgba32F => ("RGBA 32-bit float", true),
        _ => ("Other", false),
    }
}

/// Header-only image probe — `into_decoder` parses the container header
/// without decoding pixels, so dimension/color metadata is cheap even
/// for very large files. EXIF is attempted for JPEG/TIFF containers.
fn image_probe(path: &Path) -> Option<ImageMeta> {
    use image::ImageDecoder;
    let mut reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    reader.no_limits(); // dims only — no pixel buffer is allocated
    let format = reader.format().map(format_name)?;
    let decoder = reader.into_decoder().ok()?;
    let (width, height) = decoder.dimensions();
    let (color, alpha) = {
        let (n, a) = color_name(decoder.color_type());
        (Some(n.to_string()), Some(a))
    };
    let exif = exif_fields(path);
    Some(ImageMeta {
        width,
        height,
        format,
        color,
        alpha,
        exif,
    })
}

/// Curated EXIF fields — never fatal, empty Vec when absent.
fn exif_fields(path: &Path) -> Vec<(&'static str, String)> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    // kamadak-exif handles JPEG containers and raw TIFF data.
    if !matches!(ext.as_str(), "jpg" | "jpeg" | "tif" | "tiff") {
        return Vec::new();
    }
    let Ok(f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut br = std::io::BufReader::new(f);
    let x = match exif::Reader::new().read_from_container(&mut br) {
        Ok(x) => x,
        Err(_) => return Vec::new(),
    };
    use exif::{In, Tag, Value};
    let mut out: Vec<(&'static str, String)> = Vec::new();
    // kamadak flattens all linked sub-IFDs (Exif/GPS/Interop) under
    // In::PRIMARY — one lookup covers them all. Rational fields are
    // rendered as real values (f/2.8, 35 mm, 1/250 s), ASCII unquoted.
    let rat = |v: &exif::Rational| v.num as f64 / (v.denom.max(1) as f64);
    let val = |x: &exif::Exif, tag: Tag| -> Option<String> {
        let f = x.get_field(tag, In::PRIMARY)?;
        let s = match &f.value {
            Value::Ascii(vv) => vv
                .iter()
                .map(|v| {
                    String::from_utf8_lossy(v)
                        .trim_matches('\0')
                        .trim()
                        .to_string()
                })
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
            Value::Rational(vv) => vv.first().map(|v| format!("{:.1}", rat(v)))?,
            Value::Short(vv) => vv.first().map(|v| v.to_string())?,
            Value::Long(vv) => vv.first().map(|v| v.to_string())?,
            Value::SRational(vv) => vv
                .first()
                .map(|v| format!("{:.1}", v.num as f64 / (v.denom.max(1) as f64)))?,
            _ => f.display_value().with_unit(x).to_string(),
        };
        (!s.trim().is_empty()).then_some(s)
    };
    fn push(
        x: &exif::Exif,
        out: &mut Vec<(&'static str, String)>,
        label: &'static str,
        tag: Tag,
        val: &dyn Fn(&exif::Exif, Tag) -> Option<String>,
    ) {
        if let Some(v) = val(x, tag) {
            out.push((label, v));
        }
    }
    push(&x, &mut out, "Captured", Tag::DateTimeOriginal, &val);
    push(&x, &mut out, "Digitized", Tag::DateTimeDigitized, &val);
    // Make+Model joined into one Camera row.
    let cam = match (val(&x, Tag::Make), val(&x, Tag::Model)) {
        (Some(mk), Some(mo)) => Some(format!("{mk} {mo}")),
        (a, b) => a.or(b),
    };
    if let Some(c) = cam {
        out.push(("Camera", c));
    }
    push(&x, &mut out, "Lens", Tag::LensModel, &val);
    // Some writers store rationals as Short[num, den] pairs — accept
    // both forms rather than dropping a present value.
    let rat_f64 = |f: &exif::Field| -> Option<f64> {
        match &f.value {
            Value::Rational(vv) => vv.first().map(rat),
            Value::SRational(vv) => vv.first().map(|v| v.num as f64 / (v.denom.max(1) as f64)),
            Value::Short(vv) if vv.len() >= 2 => Some(vv[0] as f64 / (vv[1].max(1) as f64)),
            _ => None,
        }
    };
    // f/N aperture — Rational rendered as a real number.
    if let Some(v) = x.get_field(Tag::FNumber, In::PRIMARY).and_then(rat_f64) {
        out.push(("Aperture", format!("f/{v:.1}")));
    }
    // Shutter: 1/N for sub-second, N.N s otherwise.
    if let Some(secs) = x
        .get_field(Tag::ExposureTime, In::PRIMARY)
        .and_then(rat_f64)
        && secs > 0.0
    {
        out.push((
            "Shutter",
            if secs < 1.0 {
                format!("1/{:.0} s", 1.0 / secs)
            } else {
                format!("{secs:.1} s")
            },
        ));
    }
    push(&x, &mut out, "ISO", Tag::PhotographicSensitivity, &val);
    if let Some(mm) = x.get_field(Tag::FocalLength, In::PRIMARY).and_then(rat_f64) {
        out.push(("Focal length", format!("{mm:.0} mm")));
    }
    push(&x, &mut out, "Software", Tag::Software, &val);
    if let Some(f) = x.get_field(Tag::Orientation, In::PRIMARY)
        && let Some(v) = f.value.get_uint(0)
    {
        let label = match v {
            1 => "Normal",
            2 => "Mirrored",
            3 => "Rotated 180°",
            4 => "Mirrored 180°",
            5 => "Transposed",
            6 => "Rotated 90° CW",
            7 => "Mirrored 90°",
            8 => "Rotated 90° CCW",
            _ => "Other",
        };
        if v != 1 {
            out.push(("Orientation", label.into()));
        }
    }
    out
}

/// `st_ctime` → SystemTime (unix only). Negative times (pre-epoch) are
/// reported as None rather than clamped to a misleading 1970 value.
#[cfg(unix)]
fn ctime(md: &std::fs::Metadata) -> Option<SystemTime> {
    use std::os::unix::fs::MetadataExt;
    let secs = md.ctime();
    let nanos = md.ctime_nsec();
    if secs >= 0 {
        SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::new(secs as u64, nanos as u32))
    } else {
        SystemTime::UNIX_EPOCH
            .checked_sub(std::time::Duration::new(secs.unsigned_abs(), nanos as u32))
    }
}

#[cfg(unix)]
fn unix_meta(md: &std::fs::Metadata) -> UnixMeta {
    use std::os::unix::fs::MetadataExt;
    let uid = md.uid();
    let gid = md.gid();
    // passwd/group lookups are cached by the OS (nscd/NSS) — cheap
    // enough per selection, and they run on the worker regardless.
    let owner = uzers::get_user_by_uid(uid).map(|u| u.name().to_string_lossy().into_owned());
    let group = uzers::get_group_by_gid(gid).map(|g| g.name().to_string_lossy().into_owned());
    let allocated = (md.blocks() > 0).then(|| md.blocks().saturating_mul(512));
    UnixMeta {
        mode: md.mode() & 0o7777,
        uid,
        gid,
        owner,
        group,
        nlink: md.nlink(),
        inode: md.ino(),
        dev: md.dev(),
        allocated,
        blksize: md.blksize(),
    }
}

#[cfg(windows)]
fn win_attrs(md: &std::fs::Metadata) -> WinAttrs {
    use std::os::windows::fs::MetadataExt;
    let a = md.file_attributes();
    let mut flags = Vec::new();
    // Win32 FILE_ATTRIBUTE_* constants.
    if a & 0x20 != 0 {
        flags.push("Archive");
    }
    if a & 0x4 != 0 {
        flags.push("System");
    }
    if a & 0x800 != 0 {
        flags.push("Compressed");
    }
    if a & 0x4000 != 0 {
        flags.push("Encrypted");
    }
    if a & 0x2000 != 0 {
        flags.push("Not indexed");
    }
    if a & 0x400 != 0 {
        flags.push("Reparse point");
    }
    if a & 0x200 != 0 {
        flags.push("Sparse");
    }
    if a & 0x100 != 0 {
        flags.push("Temporary");
    }
    if a & 0x1000 != 0 {
        flags.push("Offline");
    }
    WinAttrs { flags }
}

fn is_executable(_path: &Path, md: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        md.is_file() && md.mode() & 0o111 != 0
    }
    #[cfg(windows)]
    {
        let _ = md;
        _path
            .extension()
            .map(|e| {
                matches!(
                    e.to_string_lossy().to_lowercase().as_str(),
                    "exe" | "bat" | "cmd" | "com" | "msi" | "ps1" | "scr"
                )
            })
            .unwrap_or(false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (_path, md);
        false
    }
}

/// Filesystem type + mount point via /proc/mounts longest-prefix match
/// (Linux). Other platforms report `None` honestly rather than guess.
#[cfg(target_os = "linux")]
fn fs_info(path: &Path) -> (Option<String>, Option<PathBuf>) {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let Ok(mtab) = std::fs::read_to_string("/proc/mounts") else {
        return (None, None);
    };
    let mut best: Option<(String, PathBuf)> = None;
    for line in mtab.lines() {
        let mut f = line.split_whitespace();
        let (Some(_dev), Some(mnt), Some(fst)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        // /proc/mounts escapes spaces as \040.
        let mnt = mnt.replace("\\040", " ");
        let mp = PathBuf::from(&mnt);
        if canonical.starts_with(&mp)
            && best
                .as_ref()
                .is_none_or(|(_, b)| mp.as_os_str().len() > b.as_os_str().len())
        {
            best = Some((fst.to_string(), mp));
        }
    }
    (best.as_ref().map(|b| b.0.clone()), best.map(|b| b.1))
}

#[cfg(not(target_os = "linux"))]
fn fs_info(_path: &Path) -> (Option<String>, Option<PathBuf>) {
    (None, None)
}

fn volume_meta(path: &Path) -> Option<VolumeMeta> {
    let anchor = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    let total = fs2::total_space(&anchor).ok()?;
    let available = fs2::available_space(&anchor).ok()?;
    let (fs_type, mount) = fs_info(&anchor);
    Some(VolumeMeta {
        fs_type,
        mount,
        total,
        available,
    })
}

/// Both phases in one call — used by tests; the worker calls the
/// phases directly so metadata can ship before pixels decode.
#[cfg(test)]
fn load(path: &Path) -> (Preview, Option<PreviewMeta>) {
    let (prior, mut meta) = gather(path);
    if let Some(m) = meta.as_mut()
        && is_image_path(path)
        && !m.is_dir
        && !m.is_symlink
    {
        m.image = image_probe(path);
    }
    if matches!(prior, Preview::Info(_)) {
        return (prior, meta);
    }
    (render(path), meta)
}

/// Phase 1 — filesystem metadata only (fast: one stat + cheap probes).
/// Phase 2 (`render`) produces the visual preview and may be slow
/// (image decode) — the worker ships phase 1 first so the panel shows
/// real metadata while pixels are still decoding.
fn gather(path: &Path) -> (Preview, Option<PreviewMeta>) {
    let Ok(md) = std::fs::symlink_metadata(path) else {
        return (Preview::Empty, None);
    };
    let is_symlink = md.file_type().is_symlink();
    let (link_target, link_broken, target_is_dir) = if is_symlink {
        let t = std::fs::read_link(path).ok();
        let resolved = t.as_ref().map(|t| path.parent().unwrap_or(path).join(t));
        let target_md = resolved.as_ref().and_then(|r| std::fs::metadata(r).ok());
        (
            t,
            target_md.is_none(),
            target_md.is_some_and(|m| m.is_dir()),
        )
    } else {
        (None, false, false)
    };
    let is_dir = md.is_dir() || target_is_dir;
    let name = path.file_name().unwrap_or_default();

    #[cfg(unix)]
    let unix = Some(unix_meta(&md));
    #[cfg(not(unix))]
    let unix: Option<UnixMeta> = None;
    #[cfg(windows)]
    let windows = Some(win_attrs(&md));
    #[cfg(not(windows))]
    let windows: Option<WinAttrs> = None;

    let win_bits = {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            md.file_attributes()
        }
        #[cfg(not(windows))]
        {
            0
        }
    };

    let mut meta = PreviewMeta {
        kind: if is_symlink {
            "Symbolic link".to_string()
        } else if is_dir {
            "Folder".to_string()
        } else {
            flux_core::file_type::resolve(flux_core::EntryKind::File, path)
                .info()
                .label
                .to_string()
        },
        mime: (!md.is_dir() && !is_symlink)
            .then(|| mime_guess::from_path(path).first_raw())
            .flatten()
            .map(str::to_string),
        size: md.is_file().then_some(md.len()),
        allocated: unix.as_ref().and_then(|u| u.allocated),
        is_dir,
        is_symlink,
        link_target,
        link_broken,
        dir_items: None,
        modified: md.modified().ok(),
        created: md.created().ok(),
        accessed: md.accessed().ok(),
        changed: {
            #[cfg(unix)]
            {
                ctime(&md)
            }
            #[cfg(not(unix))]
            {
                None
            }
        },
        readonly: md.permissions().readonly(),
        hidden: flux_core::entry::is_hidden(name, win_bits),
        executable: is_executable(path, &md),
        unix,
        windows,
        volume: volume_meta(path),
        image: None,
    };

    if is_symlink {
        let target = meta
            .link_target
            .as_ref()
            .map(|t| t.display().to_string())
            .unwrap_or_else(|| "?".into());
        let status = if meta.link_broken {
            format!("Symbolic link → {target} (broken)")
        } else {
            format!("Symbolic link → {target}")
        };
        return (Preview::Info(status), Some(meta));
    }
    if md.is_dir() {
        let mut n = 0u64;
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd {
                if e.is_err() || n >= DIR_COUNT_CAP as u64 {
                    break;
                }
                n += 1;
            }
        }
        meta.dir_items = Some(n);
        return (
            Preview::Info(if n >= DIR_COUNT_CAP as u64 {
                format!("Folder — {DIR_COUNT_CAP}+ items")
            } else {
                format!("Folder — {n} item{}", if n == 1 { "" } else { "s" })
            }),
            Some(meta),
        );
    }
    // Regular file — preview decided in phase 2.
    (Preview::Empty, Some(meta))
}

/// Phase 2 — the visual preview only. Image decode for known formats,
/// UTF-8 sniff + first-chunk text, Empty for anything else.
fn render(path: &Path) -> Preview {
    if is_image_path(path)
        && let Some(img) = decode_image(path)
    {
        let size = [img.width() as usize, img.height() as usize];
        return Preview::Image(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()));
    }
    // Text sniff: NUL in the first chunk ⇒ binary.
    let Ok(mut f) = std::fs::File::open(path) else {
        return Preview::Empty;
    };
    let mut buf = vec![0u8; TEXT_SNIFF];
    let Ok(n) = f.read(&mut buf) else {
        return Preview::Empty;
    };
    buf.truncate(n);
    if buf[..n.min(8192)].contains(&0) {
        return Preview::Empty;
    }
    let text = String::from_utf8_lossy(&buf);
    let capped: String = text
        .lines()
        .take(TEXT_MAX_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    Preview::Text(
        if n >= TEXT_SNIFF || text.lines().count() > TEXT_MAX_LINES {
            format!("{capped}\n\n· · ·\n(truncated — {TEXT_SNIFF} KiB max)")
        } else {
            capped
        },
    )
}

// ─── Panel rendering ────────────────────────────────────────────────

/// One metadata row: muted label in a fixed-width column, value on the
/// right; long values wrap; `copyable` appends a small copy button.
fn meta_row(ui: &mut egui::Ui, pal: &Palette, label: &str, value: &str) {
    meta_row_copy(ui, pal, label, value, false);
}

fn meta_row_copy(ui: &mut egui::Ui, pal: &Palette, label: &str, value: &str, copyable: bool) {
    // Reserve the value and copy target before laying out wrapping text.
    // At the minimum panel width use a stacked row, not a squeezed grid.
    let narrow = ui.available_width() < 240.0;
    ui.push_id(label, |ui| {
        let value_ui = |ui: &mut egui::Ui| {
            ui.horizontal_top(|ui| {
                let copy_width = if copyable {
                    ui.spacing().interact_size.y + ui.spacing().item_spacing.x
                } else {
                    0.0
                };
                let width = (ui.available_width() - copy_width).max(1.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(width);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(value)
                                    .color(pal.text_primary)
                                    .size(12.5),
                            )
                            .wrap(),
                        )
                        .on_hover_text(value);
                    },
                );
                if copyable
                    && crate::glyphs::icon_button(
                        ui,
                        crate::glyphs::Glyph::Copy,
                        "Copy value",
                        true,
                    )
                    .clicked()
                {
                    ui.ctx().copy_text(value.to_owned());
                }
            });
        };
        if narrow {
            ui.label(
                egui::RichText::new(label)
                    .color(pal.text_secondary)
                    .size(12.5),
            );
            value_ui(ui);
        } else {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(84.0, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(84.0);
                        ui.label(
                            egui::RichText::new(label)
                                .color(pal.text_secondary)
                                .size(12.5),
                        );
                    },
                );
                value_ui(ui);
            });
        }
    });
    ui.add_space(4.0);
}

fn section(ui: &mut egui::Ui, pal: &Palette, title: &str) {
    ui.add_space(16.0);
    ui.label(
        egui::RichText::new(title)
            .color(pal.text_primary)
            .size(14.0)
            .strong(),
    );
    ui.add_space(8.0);
}

fn fmt_time(t: Option<SystemTime>) -> Option<String> {
    let t = t?;
    let ts = jiff::Timestamp::try_from(t).ok()?;
    let z = ts.to_zoned(jiff::tz::TimeZone::system());
    Some(z.strftime("%Y-%m-%d %H:%M:%S %Z").to_string())
}

fn mode_string(mode: u32) -> String {
    let mut s = String::with_capacity(10);
    let flags = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    for (bit, ch) in flags {
        s.push(if mode & bit != 0 { ch } else { '-' });
    }
    // setuid/setgid/sticky replace the matching execute slot
    // (user x @2, group x @5, other x @8).
    if mode & 0o4000 != 0 {
        s.replace_range(2..3, if mode & 0o100 != 0 { "s" } else { "S" });
    }
    if mode & 0o2000 != 0 {
        s.replace_range(5..6, if mode & 0o010 != 0 { "s" } else { "S" });
    }
    if mode & 0o1000 != 0 {
        s.replace_range(8..9, if mode & 0o001 != 0 { "t" } else { "T" });
    }
    s
}

fn metadata_sections(app: &mut FluxApp, ui: &mut egui::Ui, path: &Path) {
    let pal = Palette::of(&ui.style().visuals);
    let Some(meta) = app.preview_meta.as_ref() else {
        return;
    };

    // ── General ──
    section(ui, &pal, "General");
    let kind = if meta.is_symlink {
        flux_core::EntryKind::Symlink
    } else if meta.is_dir {
        flux_core::EntryKind::Dir
    } else {
        flux_core::EntryKind::File
    };
    meta_row(
        ui,
        &pal,
        "Type",
        app.icons.file_type(kind, path).info().label,
    );
    if let Some(mime) = &meta.mime {
        meta_row(ui, &pal, "MIME type", mime);
    }
    let path_s = path.display().to_string();
    meta_row_copy(ui, &pal, "Path", &path_s, true);
    if let Some(parent) = path.parent() {
        let ps = parent.display().to_string();
        meta_row_copy(ui, &pal, "Location", &ps, true);
    }
    if let Some(sz) = meta.size {
        let s = if sz == 0 {
            "0 B".to_string()
        } else {
            format!("{} ({} bytes)", crate::format::format_size(sz), sz)
        };
        meta_row(ui, &pal, "Size", &s);
    }
    if let Some(a) = meta.allocated
        && a != meta.size.unwrap_or(0)
    {
        let s = format!("{} on disk ({} bytes)", crate::format::format_size(a), a);
        meta_row(ui, &pal, "Allocated", &s);
    }
    if let Some(n) = meta.dir_items {
        let s = if n >= DIR_COUNT_CAP as u64 {
            format!("{DIR_COUNT_CAP}+")
        } else {
            n.to_string()
        };
        meta_row(ui, &pal, "Items", &s);
    }
    if meta.is_symlink
        && let Some(t) = &meta.link_target
    {
        let mut ts = t.display().to_string();
        if meta.link_broken {
            ts.push_str("  (broken)");
        }
        meta_row(ui, &pal, "Target", &ts);
    }

    // ── Dates ──
    let dates = [
        ("Modified", meta.modified),
        ("Created", meta.created),
        ("Accessed", meta.accessed),
        // st_ctime is status-change time — labelled honestly, never
        // confused with creation/birth time.
        ("Changed", meta.changed),
    ];
    if dates.iter().any(|(_, t)| t.is_some()) {
        section(ui, &pal, "Dates");
        for (label, t) in dates {
            if let Some(v) = fmt_time(t) {
                meta_row(ui, &pal, label, &v);
            }
        }
    }

    // ── Permissions & ownership ──
    let has_perms = meta.unix.is_some()
        || meta.windows.is_some()
        || meta.readonly
        || meta.hidden
        || meta.executable;
    if has_perms {
        section(
            ui,
            &pal,
            if meta.windows.is_some() {
                "Attributes"
            } else {
                "Permissions & ownership"
            },
        );
        if let Some(u) = &meta.unix {
            meta_row(
                ui,
                &pal,
                "Mode",
                &format!("{} ({:04o})", mode_string(u.mode), u.mode),
            );
            let owner = u.owner.clone().unwrap_or_else(|| u.uid.to_string());
            meta_row(ui, &pal, "Owner", &format!("{owner} ({})", u.uid));
            let group = u.group.clone().unwrap_or_else(|| u.gid.to_string());
            meta_row(ui, &pal, "Group", &format!("{group} ({})", u.gid));
        }
        if let Some(w) = &meta.windows
            && !w.flags.is_empty()
        {
            meta_row(ui, &pal, "Flags", &w.flags.join(", "));
        }
        meta_row(
            ui,
            &pal,
            "Read-only",
            if meta.readonly { "Yes" } else { "No" },
        );
        meta_row(ui, &pal, "Hidden", if meta.hidden { "Yes" } else { "No" });
        if !meta.is_dir {
            meta_row(
                ui,
                &pal,
                "Executable",
                if meta.executable { "Yes" } else { "No" },
            );
        }
    }

    // ── Image ──
    if let Some(img) = &meta.image {
        section(ui, &pal, "Image");
        meta_row(
            ui,
            &pal,
            "Dimensions",
            &format!("{} × {} px", img.width, img.height),
        );
        let mp = img.width as f64 * img.height as f64 / 1_000_000.0;
        meta_row(ui, &pal, "Resolution", &format!("{mp:.1} MP"));
        if img.height > 0 {
            let ar = img.width as f64 / img.height as f64;
            let g = gcd(img.width, img.height);
            meta_row(
                ui,
                &pal,
                "Aspect ratio",
                &format!("{}:{} ({ar:.2})", img.width / g, img.height / g),
            );
        }
        meta_row(ui, &pal, "Format", &img.format);
        if let Some(c) = &img.color {
            meta_row(ui, &pal, "Color", c);
        }
        if let Some(a) = img.alpha {
            meta_row(ui, &pal, "Alpha", if a { "Yes" } else { "No" });
        }
        for (k, v) in &img.exif {
            meta_row(ui, &pal, k, v);
        }
    }

    // ── Filesystem ──
    let has_fs = meta.volume.is_some() || meta.unix.is_some();
    if has_fs {
        section(ui, &pal, "Filesystem");
        if let Some(v) = &meta.volume {
            if let Some(fst) = &v.fs_type {
                meta_row(ui, &pal, "Filesystem", fst);
            }
            if let Some(m) = &v.mount {
                meta_row(ui, &pal, "Mounted at", &m.display().to_string());
            }
            meta_row(
                ui,
                &pal,
                "Free space",
                &format!(
                    "{} of {}",
                    crate::format::format_size(v.available),
                    crate::format::format_size(v.total)
                ),
            );
        }
        if let Some(u) = &meta.unix {
            meta_row(ui, &pal, "Inode", &u.inode.to_string());
            meta_row(ui, &pal, "Device", &u.dev.to_string());
            meta_row(ui, &pal, "Hard links", &u.nlink.to_string());
        }
    }

    // ── Opt-in computations ──
    section(ui, &pal, "Details");
    render_ops(app, ui, &pal, path);
    ui.add_space(8.0);
}

fn render_ops(app: &mut FluxApp, ui: &mut egui::Ui, pal: &Palette, path: &Path) {
    use crate::widgets::{ghost_button, secondary_button};

    // Folder: recursive size; File: SHA-256. Results keyed to the path —
    // a stale result for a different path is dropped by the app pump.
    if meta_is_dir(app) {
        ui.horizontal(|ui| {
            if ghost_button(ui, "Calculate folder size").clicked() {
                app.meta_ops.request(path.to_path_buf(), MetaOp::DirSize);
                app.meta_op_pending = Some((path.to_path_buf(), MetaOp::DirSize));
            }
            op_status(app, ui, pal, path, MetaOp::DirSize);
        });
        if let Some(out) = app
            .meta_op
            .as_ref()
            .filter(|m| m.path == *path && m.op == MetaOp::DirSize)
            .and_then(|m| m.out.as_ref().ok())
            && let MetaOpOut::DirSize { files, dirs, bytes } = out
        {
            meta_row(
                ui,
                pal,
                "Contents",
                &format!("{files} files, {dirs} folders"),
            );
            meta_row(
                ui,
                pal,
                "Total size",
                &format!("{} ({} bytes)", crate::format::format_size(*bytes), bytes),
            );
        }
    } else {
        ui.horizontal(|ui| {
            if secondary_button(ui, "Compute SHA-256").clicked() {
                app.meta_ops.request(path.to_path_buf(), MetaOp::Sha256);
                app.meta_op_pending = Some((path.to_path_buf(), MetaOp::Sha256));
            }
            op_status(app, ui, pal, path, MetaOp::Sha256);
        });
        match app
            .meta_op
            .as_ref()
            .filter(|m| m.path == *path && m.op == MetaOp::Sha256)
            .map(|m| &m.out)
        {
            Some(Ok(MetaOpOut::Sha256(hex))) => {
                meta_row_copy(ui, pal, "SHA-256", hex, true);
            }
            Some(Err(e)) if e != "cancelled" => {
                meta_row(ui, pal, "SHA-256", &format!("failed: {e}"));
            }
            _ => {}
        }
    }
}

fn meta_is_dir(app: &FluxApp) -> bool {
    app.preview_meta.as_ref().map(|m| m.is_dir).unwrap_or(false)
}

fn op_status(app: &FluxApp, ui: &mut egui::Ui, pal: &Palette, path: &Path, op: MetaOp) {
    if app
        .meta_op_pending
        .as_ref()
        .is_some_and(|(p, o)| p == path && *o == op)
        && app
            .meta_op
            .as_ref()
            .is_none_or(|m| m.path != *path || m.op != op)
    {
        ui.add(egui::Spinner::new().size(12.0));
        ui.label(
            egui::RichText::new("computing…")
                .color(pal.text_muted)
                .size(11.0),
        );
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a.max(1) } else { gcd(b, a % b) }
}

/// The right-side docked panel. Called every frame while enabled; the
/// worker request for the focused path is issued here (idempotent).
pub fn panel(app: &mut FluxApp, ui: &mut egui::Ui) {
    let pal = Palette::of(&ui.style().visuals);
    let target = app.preview_target();
    let Some(path) = target else {
        crate::widgets::empty_state(
            ui,
            &pal,
            |p, r, c| crate::glyphs::paint_glyph(p, r, crate::glyphs::Glyph::Preview, c),
            "Nothing selected",
            Some("Select a file to preview it"),
        );
        return;
    };

    // Issue/refresh the worker request when the target changed. The
    // result is applied in `pump` only while this path is still wanted.
    if app.preview_req.as_ref() != Some(&path) {
        app.preview_req = Some(path.clone());
        app.preview = None;
        app.preview_meta = None;
        app.preview_tex = None;
        app.meta_op = None;
        app.meta_op_pending = None;
        app.preview_engine.request(path.clone());
    }

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        let kind = app
            .preview_meta
            .as_ref()
            .map(|m| {
                if m.is_symlink {
                    flux_core::EntryKind::Symlink
                } else if m.is_dir {
                    flux_core::EntryKind::Dir
                } else {
                    flux_core::EntryKind::File
                }
            })
            .unwrap_or(flux_core::EntryKind::File);
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(28.0), egui::Sense::hover());
        app.icons.paint(ui.painter(), rect, kind, &path, &pal);
        ui.label(
            egui::RichText::new(&name)
                .color(pal.text_primary)
                .size(16.0)
                .strong(),
        );
    });
    ui.add_space(12.0);

    // Preview leads (Finder Quick Look order); metadata sits below it.
    match app.preview.as_ref() {
        None => {
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(16.0));
            });
            ui.add_space(12.0);
        }
        Some(Preview::Empty) => {
            ui.label(
                egui::RichText::new("No preview available")
                    .color(pal.text_muted)
                    .size(12.0),
            );
            ui.add_space(12.0);
        }
        Some(Preview::Info(msg)) => {
            egui::Frame::default()
                .fill(pal.field_inset)
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(12, 9))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(msg)
                            .color(pal.text_secondary)
                            .size(12.0),
                    );
                });
            ui.add_space(12.0);
        }
        Some(Preview::Text(text)) => {
            let h = (ui.available_height() - 180.0).clamp(140.0, 320.0);
            egui::Frame::default()
                .fill(pal.field_inset)
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_max_height(h);
                    egui::ScrollArea::vertical()
                        .id_salt("preview_text")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(text)
                                        .monospace()
                                        .size(11.0)
                                        .color(pal.text_secondary),
                                )
                                .selectable(true)
                                .wrap(),
                            );
                        });
                });
            ui.add_space(12.0);
        }
        Some(Preview::Image(_)) => {
            if let Some(tex) = &app.preview_tex {
                let size = tex.size_vec2();
                let w = ui.available_width().max(1.0);
                let h = (ui.available_height() * 0.4).clamp(80.0, 280.0);
                let scaled = size * (w / size.x).min(h / size.y).min(1.0);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::from_texture(tex).max_size(scaled));
                });
            }
            ui.add_space(12.0);
        }
    }

    // Structured metadata — everything gathered on the worker, scrolled
    // vertically so the preview always stays in view at the top.
    egui::ScrollArea::vertical()
        .id_salt("preview_meta")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            metadata_sections(app, ui, &path);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_copy_buttons_do_not_grow_the_panel_each_frame() {
        for width in [180.0, 320.0, 520.0] {
            let ctx = egui::Context::default();
            crate::theme::apply_theme(&ctx, true);
            for _ in 0..60 {
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1440.0, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        let r = egui::Panel::right("preview_test")
                            .resizable(true)
                            .default_size(width)
                            .size_range(180.0..=520.0)
                            .show(ui, |ui| {
                                let pal = Palette::of(ui.visuals());
                                meta_row_copy(
                                    ui,
                                    &pal,
                                    "Path",
                                    "/home/user/İstanbul/a-long-file-name-with-unicode.txt",
                                    true,
                                );
                                meta_row_copy(
                                    ui,
                                    &pal,
                                    "SHA-256",
                                    &"abcdef0123456789".repeat(4),
                                    true,
                                );
                            });
                        assert!(
                            (r.response.rect.width() - width).abs() < 1.0,
                            "panel grew: {:?}, expected {width}",
                            r.response.rect
                        );
                    },
                );
                out.textures_delta.clear();
            }
        }
    }

    #[test]
    fn load_handles_exif_jpeg() {
        if !std::path::Path::new("/tmp/flux-meta-fixture/photo.jpg").exists() {
            return; // fixture may be absent in CI
        }
        let (_p, meta) = load(std::path::Path::new("/tmp/flux-meta-fixture/photo.jpg"));
        assert!(meta.is_some());
        let m = meta.unwrap();
        if let Some(i) = &m.image {
            eprintln!(
                "dims={}x{} fmt={} exif={:?}",
                i.width, i.height, i.format, i.exif
            );
        }
    }

    #[test]
    fn mode_string_renders_standard_bits() {
        assert_eq!(mode_string(0o644), "rw-r--r--");
        assert_eq!(mode_string(0o755), "rwxr-xr-x");
        assert_eq!(mode_string(0o4755), "rwsr-xr-x");
        assert_eq!(mode_string(0o1755), "rwxr-xr-t");
        assert_eq!(mode_string(0o000), "---------");
    }

    #[test]
    fn load_reports_real_fields() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, "hello").unwrap();
        let (_, meta) = load(&f);
        let m = meta.expect("meta");
        assert_eq!(m.size, Some(5));
        assert!(!m.is_dir);
        assert!(m.modified.is_some());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert!(m.unix.is_some());
            let u = m.unix.unwrap();
            assert!(u.inode > 0);
            assert!(u.nlink >= 1);
            assert_eq!(
                u.mode & 0o777,
                std::fs::metadata(&f).unwrap().mode() & 0o777
            );
        }
        let (_, meta) = load(dir.path());
        let m = meta.unwrap();
        assert!(m.is_dir);
        assert_eq!(m.dir_items, Some(1));
        assert!(m.size.is_none(), "dirs never report a fake size");
    }

    #[test]
    fn load_marks_broken_symlinks() {
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            let link = dir.path().join("dead");
            std::os::unix::fs::symlink(dir.path().join("nope"), &link).unwrap();
            let (_, meta) = load(&link);
            let m = meta.unwrap();
            assert!(m.is_symlink);
            assert!(m.link_broken);
        }
    }

    #[test]
    fn meta_ops_sha256_and_dirsize() {
        let ops = MetaOps::new();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.bin"), b"data").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/b.bin"), b"more data").unwrap();

        ops.request(dir.path().to_path_buf(), MetaOp::DirSize);
        let mut got = None;
        for _ in 0..100 {
            if let Some(m) = ops.drain_events().next() {
                got = Some(m);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let Some(MetaOpMsg {
            out: Ok(MetaOpOut::DirSize { files, dirs, bytes }),
            ..
        }) = got
        else {
            panic!("no dirsize result");
        };
        assert_eq!(files, 2);
        assert_eq!(dirs, 1);
        assert_eq!(bytes, 13);

        ops.request(dir.path().join("a.bin"), MetaOp::Sha256);
        let mut got = None;
        for _ in 0..100 {
            if let Some(m) = ops.drain_events().next() {
                got = Some(m);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let Some(MetaOpMsg {
            out: Ok(MetaOpOut::Sha256(h)),
            ..
        }) = got
        else {
            panic!("no sha256 result");
        };
        // sha256("data")
        assert_eq!(
            h,
            "3a6eb0790f39ac87c94f3856b2dd2c5d110e6811602261a9a923d3bb23adc8b7"
        );
    }
}
