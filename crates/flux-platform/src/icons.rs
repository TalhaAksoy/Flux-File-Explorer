//! Native file icons and human-readable type names.
//!
//! Icons come from the OS (`file_icon_provider` → NSWorkspace on macOS,
//! SHGetFileInfo on Windows). On targets without it we return `None` and
//! the UI falls back to painted glyphs. MUST be called off the UI thread —
//! the OS call is a few ms each; the UI caches aggressively by extension.

use std::path::Path;

/// Decoded icon pixels in RGBA, or None when unavailable (Linux, errors).
pub struct FileIcon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Fetch the OS-native icon for `path` at `size` px.
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

/// Human-readable file type for the "Type" column ("PNG image",
/// "Rust source"). Pure static table — deterministic and identical across
/// OSes; directories/links handled by the caller.
pub fn type_label(ext: Option<&str>) -> &'static str {
    match ext.map(|e| e.to_ascii_lowercase()).as_deref() {
        None | Some("") => "File",
        Some("rs") => "Rust source",
        Some("py") => "Python source",
        Some("js") | Some("mjs") | Some("cjs") => "JavaScript source",
        Some("ts") | Some("mts") | Some("cts") => "TypeScript source",
        Some("jsx") | Some("tsx") => "React source",
        Some("c") | Some("h") => "C source",
        Some("cpp") | Some("cxx") | Some("hpp") => "C++ source",
        Some("go") => "Go source",
        Some("java") => "Java source",
        Some("swift") => "Swift source",
        Some("kt") | Some("kts") => "Kotlin source",
        Some("rb") => "Ruby source",
        Some("sh") | Some("bash") | Some("zsh") => "Shell script",
        Some("html") | Some("htm") => "HTML document",
        Some("css") => "CSS stylesheet",
        Some("json") => "JSON document",
        Some("jsonl") => "JSON Lines",
        Some("toml") => "TOML document",
        Some("yaml") | Some("yml") => "YAML document",
        Some("xml") => "XML document",
        Some("md") => "Markdown",
        Some("txt") | Some("text") => "Plain text",
        Some("rtf") => "Rich text",
        Some("pdf") => "PDF document",
        Some("doc") | Some("docx") => "Word document",
        Some("xls") | Some("xlsx") | Some("csv") | Some("tsv") => "Spreadsheet",
        Some("ppt") | Some("pptx") => "Presentation",
        Some("png") => "PNG image",
        Some("jpg") | Some("jpeg") => "JPEG image",
        Some("gif") => "GIF image",
        Some("webp") => "WebP image",
        Some("svg") => "SVG image",
        Some("heic") | Some("heif") => "HEIF image",
        Some("tiff") | Some("tif") => "TIFF image",
        Some("bmp") => "BMP image",
        Some("ico") | Some("icns") => "Icon",
        Some("mp4") | Some("m4v") => "MP4 video",
        Some("mov") => "QuickTime movie",
        Some("mkv") => "Matroska video",
        Some("webm") => "WebM video",
        Some("avi") => "AVI video",
        Some("mp3") => "MP3 audio",
        Some("m4a") | Some("aac") => "AAC audio",
        Some("flac") => "FLAC audio",
        Some("wav") => "WAV audio",
        Some("ogg") | Some("oga") => "Ogg audio",
        Some("zip") => "Zip archive",
        Some("tar") => "Tar archive",
        Some("gz") | Some("tgz") => "Gzip archive",
        Some("bz2") => "Bzip2 archive",
        Some("xz") => "XZ archive",
        Some("zst") => "Zstd archive",
        Some("7z") => "7-Zip archive",
        Some("rar") => "RAR archive",
        Some("dmg") => "Disk image",
        Some("iso") => "ISO image",
        Some("app") => "Application",
        Some("exe") | Some("msi") => "Windows executable",
        Some("deb") | Some("rpm") => "Package",
        Some("so") | Some("dylib") | Some("dll") => "Library",
        Some("wasm") => "WebAssembly",
        Some("lock") => "Lockfile",
        Some("log") => "Log file",
        Some("db") | Some("sqlite") | Some("sqlite3") => "Database",
        Some("env") => "Environment file",
        Some("pem") | Some("crt") | Some("cer") | Some("key") => "Certificate/Key",
        Some("ttf") | Some("otf") | Some("woff") | Some("woff2") => "Font",
        Some(_) => "File",
    }
}
