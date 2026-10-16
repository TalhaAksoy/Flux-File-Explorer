//! Preview panel — right-side column for the focused item. All IO and
//! decoding happens on a dedicated worker; the panel only renders the
//! last completed result, so a fast selection change can never flash a
//! stale preview.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

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

/// Filesystem metadata for the previewed path, gathered on the worker
/// so the panel never stats the disk on the UI thread.
pub struct PreviewMeta {
    pub kind: &'static str,
    /// File size; `None` for dirs/links (dir sizes need recursion).
    pub size: Option<u64>,
    pub modified: Option<std::time::SystemTime>,
}

pub struct PreviewMsg {
    pub path: PathBuf,
    pub preview: Preview,
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
                    let (preview, meta) = load(&path);
                    let msg = PreviewMsg {
                        preview,
                        meta,
                        path,
                    };
                    let _ = ev_tx.try_send(msg);
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

fn load(path: &Path) -> (Preview, Option<PreviewMeta>) {
    let Ok(md) = std::fs::symlink_metadata(path) else {
        return (Preview::Empty, None);
    };
    let meta = PreviewMeta {
        kind: if md.file_type().is_symlink() {
            "Link"
        } else if md.is_dir() {
            "Folder"
        } else {
            flux_platform::type_label(path.extension().and_then(|e| e.to_str()))
        },
        size: md.is_file().then_some(md.len()),
        modified: md.modified().ok(),
    };
    let meta = Some(meta);
    if md.file_type().is_symlink() {
        let target = std::fs::read_link(path)
            .map(|t| t.display().to_string())
            .unwrap_or_else(|_| "?".into());
        return (Preview::Info(format!("Symbolic link → {target}")), meta);
    }
    if md.is_dir() {
        let mut n = 0usize;
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd {
                if e.is_err() || n >= DIR_COUNT_CAP {
                    break;
                }
                n += 1;
            }
        }
        return (
            Preview::Info(if n >= DIR_COUNT_CAP {
                format!("Folder — {DIR_COUNT_CAP}+ items")
            } else {
                format!("Folder — {n} item{}", if n == 1 { "" } else { "s" })
            }),
            meta,
        );
    }
    if is_image_path(path)
        && let Some(img) = decode_image(path)
    {
        let size = [img.width() as usize, img.height() as usize];
        return (
            Preview::Image(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw())),
            meta,
        );
    }
    // Text sniff: NUL in the first chunk ⇒ binary.
    let Ok(mut f) = std::fs::File::open(path) else {
        return (Preview::Empty, meta);
    };
    let mut buf = vec![0u8; TEXT_SNIFF];
    let Ok(n) = f.read(&mut buf) else {
        return (Preview::Empty, meta);
    };
    buf.truncate(n);
    if buf[..n.min(8192)].contains(&0) {
        return (Preview::Empty, meta);
    }
    let text = String::from_utf8_lossy(&buf);
    let capped: String = text
        .lines()
        .take(TEXT_MAX_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    (
        Preview::Text(
            if n >= TEXT_SNIFF || text.lines().count() > TEXT_MAX_LINES {
                format!("{capped}\n\n· · ·\n(truncated — {TEXT_SNIFF} KiB max)")
            } else {
                capped
            },
        ),
        meta,
    )
}

/// The right-side docked panel. Called every frame while enabled; the
/// worker request for the focused path is issued here (idempotent).
pub fn panel(app: &mut FluxApp, ui: &mut egui::Ui) {
    let pal = Palette::of(&ui.style().visuals);
    let target = app.focused_path();
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

    // Issue/refresh the worker request when focus changed. The result is
    // applied in `pump` only while this path is still the requested one.
    if app.preview_req.as_ref() != Some(&path) {
        app.preview_req = Some(path.clone());
        app.preview = None;
        app.preview_tex = None;
        app.preview_engine.request(path.clone());
    }

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(&name)
            .color(pal.text_primary)
            .size(14.0)
            .strong(),
    );
    ui.add_space(8.0);

    // Preview leads (Finder Quick Look order); metadata sits below it.
    match app.preview.as_ref().map(|m| &m.preview) {
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
            let h = (ui.available_height() - 120.0).max(140.0);
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
                let scaled = size * (w / size.x).min(1.0);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::from_texture(tex).max_size(scaled));
                });
            }
            ui.add_space(12.0);
        }
    }

    // Metadata rows — gathered by the worker alongside the preview; no
    // synchronous filesystem access on the UI thread.
    if let Some(meta) = app.preview.as_ref().and_then(|m| m.meta.as_ref()) {
        let mut rows = vec![("Type", meta.kind.to_string())];
        if let Some(size) = meta.size {
            rows.push(("Size", crate::format::format_size(size)));
        }
        if let Some(m) = meta.modified {
            rows.push(("Modified", crate::format::format_mtime(Some(m))));
        }
        for (k, v) in rows {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(k).color(pal.text_muted).size(11.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(v).color(pal.text_secondary).size(11.5));
                });
            });
            ui.add_space(2.0);
        }
    }
}
