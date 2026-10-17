//! Native artwork adapters. File classification lives in flux-core so
//! native and bundled views cannot disagree about format names.
use std::path::Path;

pub struct FileIcon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// OS calls are synchronous; callers MUST use a background worker.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn file_icon(path: &Path, size: u16) -> Option<FileIcon> {
    file_icon_provider::get_file_icon(path, size.max(16))
        .ok()
        .map(|i| FileIcon {
            rgba: i.pixels,
            width: i.width,
            height: i.height,
        })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn file_icon(_path: &Path, _size: u16) -> Option<FileIcon> {
    None
}

/// Compatibility adapter for callers that only know an extension.
/// File-aware callers should use `file_type::resolve(kind, path)`.
pub fn type_label(ext: Option<&str>) -> &'static str {
    flux_core::file_type::by_extension(ext.unwrap_or_default())
        .info()
        .label
}
