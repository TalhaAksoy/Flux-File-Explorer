//! Bounded, optional icon evidence. Never called synchronously by drawing.
//! No XML parser, entity expansion, shell execution or remote fetches.
use flux_core::{
    EntryKind,
    file_type::{self, Evidence, IconId},
};
use rustc_hash::FxHashMap;
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

pub const HEADER_BYTES: usize = 4096;
const CAPACITY: usize = 256;
const PENDING: usize = 16;
const TTL: Duration = Duration::from_secs(60);

pub fn sniff(kind: EntryKind, path: &Path) -> Option<Evidence> {
    if !file_type::needs_evidence(kind, path) {
        return None;
    }
    let file_type = std::fs::symlink_metadata(path).ok()?.file_type();
    if kind == EntryKind::Symlink {
        return (file_type.is_symlink() && std::fs::metadata(path).ok()?.is_dir())
            .then_some(Evidence::FolderLink);
    }
    if kind == EntryKind::Other {
        return {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                if file_type.is_socket() {
                    Some(Evidence::Socket)
                } else if file_type.is_fifo() {
                    Some(Evidence::Pipe)
                } else if file_type.is_char_device() || file_type.is_block_device() {
                    Some(Evidence::Device)
                } else {
                    None
                }
            }
            #[cfg(not(unix))]
            {
                None
            }
        };
    }
    // Don't follow a path that became a symlink after enumeration.
    if !file_type.is_file() {
        return None;
    }
    let mut bytes = Vec::with_capacity(HEADER_BYTES);
    std::fs::File::open(path)
        .ok()?
        .take(HEADER_BYTES as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    detect(path, &bytes)
}

pub fn detect(path: &Path, bytes: &[u8]) -> Option<Evidence> {
    let bytes = &bytes[..bytes.len().min(HEADER_BYTES)];
    let ext = path.extension()?.to_str()?;
    if ext.eq_ignore_ascii_case("ts") {
        // Three complete sync positions, not just a coincidental first 'G'.
        return (bytes.len() >= 564
            && [0, 188, 376]
                .into_iter()
                .all(|at| bytes[at] == 0x47 && ((bytes[at + 3] >> 4) & 3) != 0))
        .then_some(Evidence::TransportStream);
    }
    let text = std::str::from_utf8(bytes)
        .ok()?
        .trim_start_matches('\u{feff}')
        .trim_start();
    if ext.eq_ignore_ascii_case("m") {
        let first = text.lines().find(|line| !line.trim().is_empty())?.trim();
        return (first.starts_with("function ")
            || first.starts_with("classdef ")
            || first.starts_with("% MATLAB"))
        .then_some(Evidence::Matlab);
    }
    if ext.eq_ignore_ascii_case("json") {
        // Parsing the bounded buffer only: incomplete/oversize feeds simply
        // keep JSON. A nested string mentioning JSON Feed is not evidence.
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let object = value.as_object()?;
        let version = object.get("version")?.as_str()?;
        return (matches!(
            version,
            "https://jsonfeed.org/version/1" | "https://jsonfeed.org/version/1.1"
        ) && object.get("items").is_some_and(|v| v.is_array()))
        .then_some(Evidence::JsonFeed);
    }
    if !ext.eq_ignore_ascii_case("xml") {
        return None;
    }
    // Refuse DTD/entities altogether, even though we never expand them.
    if text.contains("<!DOCTYPE") || text.contains("<!ENTITY") {
        return None;
    }
    let mut root = text;
    loop {
        if root.starts_with("<?xml ") {
            root = root.split_once("?>")?.1.trim_start();
        } else if root.starts_with("<!--") {
            root = root.split_once("-->")?.1.trim_start();
        } else {
            break;
        }
    }
    let end = root.find('>')?;
    let tag = &root[..=end];
    let root_name = tag
        .strip_prefix('<')?
        .split(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
        .next()?;
    match root_name {
        "rss"
            if matches!(xml_attribute(tag, "version"), Some("2.0" | "0.91" | "0.92"))
                && root[end + 1..]
                    .trim_start()
                    .strip_prefix("<channel")
                    .is_some_and(|s| {
                        s.starts_with('>')
                            || s.starts_with('/')
                            || s.starts_with(char::is_whitespace)
                    }) =>
        {
            Some(Evidence::Rss)
        }
        "feed" if xml_attribute(tag, "xmlns") == Some("http://www.w3.org/2005/Atom") => {
            Some(Evidence::Atom)
        }
        "opml" if xml_attribute(tag, "version").is_some() => Some(Evidence::Opml),
        _ => None,
    }
}

/// Tiny bounded attribute lexer: values containing the target name do not
/// count as attributes. No entity expansion or document tree is constructed.
fn xml_attribute<'a>(tag: &'a str, wanted: &str) -> Option<&'a str> {
    let mut rest = tag.strip_prefix('<')?;
    let root_end = rest.find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')?;
    rest = &rest[root_end..];
    let mut found = None;
    loop {
        rest = rest.trim_start();
        if rest == ">" || rest == "/>" {
            return found;
        }
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.')))?;
        if end == 0 {
            return None;
        }
        let name = &rest[..end];
        rest = rest[end..].trim_start().strip_prefix('=')?.trim_start();
        let quote = rest.chars().next()?;
        if quote != '\'' && quote != '"' {
            return None;
        }
        rest = &rest[1..];
        let end = rest.find(quote)?;
        let value = &rest[..end];
        if name == wanted {
            if found.is_some() || value.contains('&') {
                return None;
            }
            found = Some(value);
        }
        rest = &rest[end + 1..];
        if !rest.starts_with(char::is_whitespace)
            && !rest.starts_with('>')
            && !rest.starts_with("/>")
        {
            return None;
        }
    }
}

struct Cached {
    evidence: Option<Evidence>,
    touched: u64,
    at: Instant,
}
struct ResultMsg {
    path: PathBuf,
    generation: u64,
    evidence: Option<Evidence>,
}

pub struct EvidenceCache {
    map: RefCell<FxHashMap<PathBuf, Cached>>,
    pending: RefCell<HashSet<(PathBuf, u64)>>,
    clock: Cell<u64>,
    generation: Cell<u64>,
    tx: mpsc::SyncSender<(PathBuf, u64, EntryKind)>,
    rx: mpsc::Receiver<ResultMsg>,
    wake: Arc<Mutex<Option<egui::Context>>>,
}
impl Default for EvidenceCache {
    fn default() -> Self {
        Self::new()
    }
}

impl EvidenceCache {
    pub fn new() -> Self {
        let (tx, jobs) = mpsc::sync_channel::<(PathBuf, u64, EntryKind)>(PENDING);
        let (results, rx) = mpsc::sync_channel(PENDING);
        let wake = Arc::new(Mutex::new(None::<egui::Context>));
        let w = wake.clone();
        std::thread::Builder::new()
            .name("flux-icon-evidence".into())
            .spawn(move || {
                while let Ok((path, generation, kind)) = jobs.recv() {
                    let evidence = sniff(kind, &path);
                    if results
                        .send(ResultMsg {
                            path,
                            generation,
                            evidence,
                        })
                        .is_err()
                    {
                        break;
                    }
                    if let Some(ctx) = w.lock().unwrap().as_ref() {
                        ctx.request_repaint();
                    }
                }
            })
            .expect("spawn icon evidence worker");
        Self {
            map: Default::default(),
            pending: Default::default(),
            clock: Cell::new(0),
            generation: Cell::new(0),
            tx,
            rx,
            wake,
        }
    }
    pub fn set_ctx(&self, ctx: egui::Context) {
        *self.wake.lock().unwrap() = Some(ctx);
    }
    pub fn lookup(&self, path: &Path, request: bool) -> Option<Evidence> {
        self.lookup_kind(EntryKind::File, path, request)
    }
    fn lookup_kind(&self, kind: EntryKind, path: &Path, request: bool) -> Option<Evidence> {
        let tick = self.clock.get().wrapping_add(1);
        self.clock.set(tick);
        if let Some(c) = self.map.borrow_mut().get_mut(path)
            && c.at.elapsed() < TTL
        {
            c.touched = tick;
            return c.evidence;
        }
        if request && file_type::needs_evidence(kind, path) {
            let key = (path.to_owned(), self.generation.get());
            let mut pending = self.pending.borrow_mut();
            if pending.len() < PENDING
                && pending.insert(key.clone())
                && self.tx.try_send((key.0.clone(), key.1, kind)).is_err()
            {
                pending.remove(&key);
            }
        }
        None
    }
    pub fn pump(&self) -> usize {
        let mut n = 0;
        while let Ok(result) = self.rx.try_recv() {
            self.pending
                .borrow_mut()
                .remove(&(result.path.clone(), result.generation));
            if result.generation != self.generation.get() {
                continue;
            }
            let mut map = self.map.borrow_mut();
            if map.len() >= CAPACITY
                && !map.contains_key(&result.path)
                && let Some(victim) = map
                    .iter()
                    .min_by_key(|(_, v)| v.touched)
                    .map(|(p, _)| p.clone())
            {
                map.remove(&victim);
            }
            map.insert(
                result.path,
                Cached {
                    evidence: result.evidence,
                    touched: self.clock.get(),
                    at: Instant::now(),
                },
            );
            n += 1;
        }
        n
    }
    pub fn invalidate(&self) {
        self.map.borrow_mut().clear();
        self.generation.set(self.generation.get().wrapping_add(1));
    }
    pub fn resolved(&self, kind: EntryKind, path: &Path, request: bool) -> IconId {
        let base = file_type::resolve(kind, path);
        if !file_type::needs_evidence(kind, path) {
            return base;
        }
        file_type::refine(kind, path, self.lookup_kind(kind, path, request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_conservative_signatures() {
        let cases: &[(&str,&[u8],Option<Evidence>)]=&[
            ("feed.xml",b"<?xml version=\"1.0\"?><!--x--><rss version=\"2.0\"><channel></channel></rss>",Some(Evidence::Rss)),
            ("feed.xml",b"<feed xmlns='http://www.w3.org/2005/Atom'></feed>",Some(Evidence::Atom)),
            ("plain.xml",b"<configuration><rss version=\"2.0\"/></configuration>",None),
            ("plain.xml",b"<rssish version=\"2.0\"><channel/></rssish>",None),
            ("bad.xml",b"<!DOCTYPE rss [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><rss version=\"2.0\"><channel/>",None),
            ("bad.xml",b"<feed xmlns=",None),
            ("feed.json",br#"{"version":"https://jsonfeed.org/version/1.1","items":[]}"#,Some(Evidence::JsonFeed)),
            ("bad.json",br#"{"nested":{"version":"https://jsonfeed.org/version/1.1"},"items":[]}"#,None),
            ("bad.json",br#"{"version":"https://jsonfeed.org/version/1.1","items":[]"#,None),
            ("private.key",b"<rss version=\"2.0\"><channel/>",None),
            ("app.ts",b"const G = 'not a video';",None),
            ("math.m",b"function result = transform(x)\n",Some(Evidence::Matlab)),
        ];
        for (path, bytes, want) in cases {
            assert_eq!(detect(Path::new(path), bytes), *want, "{path}");
        }
        let mut ts = vec![0; 4096];
        for offset in [0, 188, 376] {
            ts[offset] = 0x47;
            ts[offset + 3] = 0x10;
        }
        assert_eq!(
            detect(Path::new("video.ts"), &ts),
            Some(Evidence::TransportStream)
        );
        assert_eq!(detect(Path::new("app.ts"), &ts[..376]), None);
        let mut late = vec![b' '; HEADER_BYTES];
        late.extend_from_slice(b"<rss version=\"2.0\"><channel/>");
        assert_eq!(detect(Path::new("late.xml"), &late), None);
    }
    #[test]
    fn negative_results_are_cached_and_refresh_invalidates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed.xml");
        std::fs::write(&path, b"plain").unwrap();
        let cache = EvidenceCache::new();
        cache.lookup(&path, true);
        let deadline = Instant::now() + Duration::from_secs(3);
        while cache.pump() == 0 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        cache.lookup(&path, true);
        assert!(cache.pending.borrow().is_empty());
        cache.invalidate();
        assert!(cache.map.borrow().is_empty());
        cache.lookup(&path, true);
        assert_eq!(cache.pending.borrow().len(), 1);
    }
}
