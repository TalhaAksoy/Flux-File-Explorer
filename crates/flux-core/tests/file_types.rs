use flux_core::{
    EntryKind,
    file_type::{self, Evidence as E, IconId as I},
};
use std::path::Path;

#[test]
fn requested_representative_names() {
    for (name, id) in [
        ("app.js", I::JavaScript),
        ("app.ts", I::TypeScript),
        ("App.tsx", I::ReactTs),
        ("main.py", I::Python),
        ("main.rs", I::Rust),
        ("main.go", I::Go),
        ("README.md", I::Readme),
        ("Makefile", I::Make),
        ("Dockerfile", I::Docker),
        ("Dockerfile.production", I::Docker),
        (".gitignore", I::Git),
        ("Cargo.toml", I::Cargo),
        ("package.json", I::Node),
        ("tsconfig.json", I::TypeScript),
        ("vite.config.ts", I::Vite),
        ("feed.rss", I::Rss),
        ("feed.atom", I::Atom),
        ("feeds.opml", I::Opml),
        ("report.docx", I::Word),
        ("budget.xlsx", I::Excel),
        ("slides.pptx", I::Powerpoint),
        ("document.pdf", I::Pdf),
        ("archive.tar.gz", I::TarGz),
        ("archive.tar.zst", I::TarZst),
        ("photo.webp", I::Webp),
        ("script.sh", I::Shell),
        ("app.AppImage", I::AppImage),
        ("unknown.customextension", I::Unknown),
        ("file.TAR.BZ2", I::TarBz),
        ("İstanbul/çalışma.RS", I::Rust),
        (".env.production", I::Env),
        (".github/workflows/build.yml", I::Github),
        (".circleci/config.yml", I::Circle),
        ("k8s/deploy.yaml", I::Kubernetes),
        ("normal/deploy.yaml", I::Yaml),
        ("README_not_a_readme.rs", I::Rust),
        ("Cargo.toml.backup", I::Backup),
        ("movie.m2ts", I::TransportStream),
        ("private.key", I::Key),
    ] {
        assert_eq!(
            file_type::resolve(EntryKind::File, Path::new(name)),
            id,
            "{name}"
        );
    }
}

#[test]
fn every_registry_extension_and_exact_filename_is_reachable() {
    let mut extensions = std::collections::HashSet::new();
    for ty in file_type::CATALOG {
        assert_eq!(ty.icon.info().label, ty.label);
        for ext in ty.extensions {
            assert!(extensions.insert(*ext), "duplicate extension: {ext}");
            let name = format!("sample.{ext}");
            assert_eq!(
                file_type::resolve(EntryKind::File, Path::new(&name)),
                ty.icon,
                "{name}"
            );
            assert_eq!(file_type::by_extension(&ext.to_ascii_uppercase()), ty.icon);
        }
    }
    let mut names = std::collections::HashSet::new();
    for (name, id) in file_type::FILENAMES {
        assert!(names.insert(*name), "duplicate filename: {name}");
        for name in [name.to_string(), name.to_ascii_uppercase()] {
            assert_eq!(
                file_type::resolve(EntryKind::File, Path::new(&name)),
                *id,
                "{name}"
            );
        }
    }
    assert!(extensions.len() > 300, "coverage unexpectedly reduced");
}

#[test]
fn object_and_evidence_precedence() {
    for kind in [EntryKind::Dir, EntryKind::Symlink, EntryKind::Other] {
        let expected = match kind {
            EntryKind::Dir => I::Folder,
            EntryKind::Symlink => I::Symlink,
            _ => I::Special,
        };
        assert_eq!(
            file_type::refine(kind, Path::new("file.ts"), Some(E::TransportStream)),
            expected
        );
    }
    assert_eq!(
        file_type::resolve(EntryKind::Dir, Path::new("Editor.APP")),
        I::App
    );
    assert_eq!(
        file_type::resolve(EntryKind::Dir, Path::new("Code.xcodeproj")),
        I::Xcode
    );
    assert_eq!(
        file_type::refine(
            EntryKind::File,
            Path::new("stream.ts"),
            Some(E::TransportStream)
        ),
        I::TransportStream
    );
    assert_eq!(
        file_type::refine(
            EntryKind::File,
            Path::new("vite.config.ts"),
            Some(E::TransportStream)
        ),
        I::Vite
    );
    assert_eq!(
        file_type::refine(EntryKind::File, Path::new("plain.xml"), Some(E::Rss)),
        I::Rss
    );
    assert_eq!(
        file_type::resolve(EntryKind::File, Path::new("plain.xml")),
        I::Xml
    );
    assert!(!file_type::needs_evidence(
        EntryKind::File,
        Path::new("private.key")
    ));
    assert!(!file_type::needs_evidence(
        EntryKind::File,
        Path::new(".env.production")
    ));
}

#[cfg(unix)]
#[test]
fn non_utf8_basename_keeps_ascii_extension_without_changing_path() {
    use std::os::unix::ffi::OsStringExt;
    let name = std::ffi::OsString::from_vec(b"bad\xff.RS".to_vec());
    assert_eq!(
        file_type::resolve(EntryKind::File, Path::new(&name)),
        I::Rust
    );
    let archive = std::ffi::OsString::from_vec(b"bad\xff.tar.gz".to_vec());
    assert_eq!(
        file_type::resolve(EntryKind::File, Path::new(&archive)),
        I::TarGz
    );
}
