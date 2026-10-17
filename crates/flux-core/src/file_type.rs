//! Offline, allocation-free hot-path file identity. No IO, MIME guessing or
//! GUI dependencies. ASCII rules operate on OsStr bytes, never lossy paths.
use crate::EntryKind;
use rustc_hash::FxHashMap;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Object,
    Language,
    Web,
    Tool,
    Repository,
    Document,
    Text,
    Feed,
    Image,
    Audio,
    Video,
    Archive,
    Application,
    Config,
    Data,
    Font,
    Security,
    Specialized,
}

pub struct FileType {
    pub icon: IconId,
    pub label: &'static str,
    pub category: Category,
    pub extensions: &'static [&'static str],
}

macro_rules! catalog {
    ($( $id:ident, $cat:ident, $label:literal, [$($ext:literal),*]; )*) => {
        #[repr(u16)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum IconId { $($id,)* }
        pub const CATALOG: &[FileType] = &[$(FileType { icon: IconId::$id, label: $label,
            category: Category::$cat, extensions: &[$($ext),*] },)*];
    }
}

catalog! {
    Unknown, Object, "File", [];
    Folder, Object, "Folder", [];
    Symlink, Object, "Symbolic link", [];
    FolderLink, Object, "Folder symbolic link", [];
    Socket, Object, "Socket", [];
    Pipe, Object, "Named pipe", [];
    Device, Object, "Device", [];
    Special, Object, "Special file", [];
    JavaScript, Language, "JavaScript source", ["js", "mjs", "cjs"];
    TypeScript, Language, "TypeScript source", ["ts", "mts", "cts"];
    ReactJs, Web, "React JavaScript source", ["jsx"];
    ReactTs, Web, "React TypeScript source", ["tsx"];
    Python, Language, "Python source", ["py", "pyw", "pyi", "pyx", "pxd"];
    Notebook, Text, "Jupyter notebook", ["ipynb"];
    Rust, Language, "Rust source", ["rs"];
    C, Language, "C source", ["c", "h"];
    Cpp, Language, "C++ source", ["cpp", "cc", "cxx", "hpp", "hxx", "hh", "c++", "h++"];
    Csharp, Language, "C# source", ["cs", "csx"];
    Java, Language, "Java source", ["java", "class"];
    Kotlin, Language, "Kotlin source", ["kt", "kts"];
    Swift, Language, "Swift source", ["swift"];
    ObjectiveC, Language, "Objective-C source", ["m", "mm"];
    Matlab, Language, "MATLAB source", ["matlab"];
    Go, Language, "Go source", ["go"];
    Dart, Language, "Dart source", ["dart"];
    Ruby, Language, "Ruby source", ["rb", "rake", "gemspec"];
    Php, Language, "PHP source", ["php", "php3", "php4", "php5", "phtml"];
    Lua, Language, "Lua source", ["lua"];
    Perl, Language, "Perl source", ["pl", "pm", "pod", "t"];
    R, Language, "R source", ["r", "rmd", "rproj"];
    Julia, Language, "Julia source", ["jl"];
    Scala, Language, "Scala source", ["scala", "sc"];
    Elixir, Language, "Elixir source", ["ex", "exs"];
    Erlang, Language, "Erlang source", ["erl", "hrl"];
    Haskell, Language, "Haskell source", ["hs", "lhs"];
    Ocaml, Language, "OCaml source", ["ml", "mli"];
    Fsharp, Language, "F# source", ["fs", "fsx", "fsi"];
    Zig, Language, "Zig source", ["zig", "zon"];
    Nim, Language, "Nim source", ["nim", "nims", "nimble"];
    Crystal, Language, "Crystal source", ["cr"];
    Solidity, Language, "Solidity source", ["sol"];
    V, Language, "V source", ["v", "vsh"];
    Assembly, Language, "Assembly source", ["asm", "s", "inc"];
    Wasm, Language, "WebAssembly", ["wasm", "wat", "wast"];
    Sql, Data, "SQL source", ["sql"];
    Graphql, Web, "GraphQL schema", ["graphql", "gql"];
    Protobuf, Data, "Protocol Buffers schema", ["proto"];
    Latex, Document, "LaTeX document", ["tex", "bib", "sty", "cls", "dtx", "ins"];
    Html, Web, "HTML document", ["html", "htm", "xhtml"];
    Css, Web, "CSS stylesheet", ["css"];
    Scss, Web, "SCSS stylesheet", ["scss"];
    Sass, Web, "Sass stylesheet", ["sass"];
    Less, Web, "Less stylesheet", ["less"];
    Vue, Web, "Vue component", ["vue"];
    Svelte, Web, "Svelte component", ["svelte"];
    Astro, Web, "Astro component", ["astro"];
    Angular, Web, "Angular configuration", [];
    Next, Tool, "Next.js configuration", [];
    Vite, Tool, "Vite configuration", [];
    Webpack, Tool, "Webpack configuration", [];
    Tailwind, Tool, "Tailwind configuration", [];
    Postcss, Tool, "PostCSS configuration", [];
    Eslint, Tool, "ESLint configuration", [];
    Prettier, Tool, "Prettier configuration", [];
    Babel, Tool, "Babel configuration", [];
    Json, Data, "JSON document", ["json"];
    Json5, Data, "JSON5 document", ["json5"];
    Jsonc, Data, "JSON with comments", ["jsonc"];
    Yaml, Config, "YAML document", ["yaml", "yml"];
    Toml, Config, "TOML document", ["toml"];
    Xml, Data, "XML document", ["xml", "xsd", "xsl", "xslt", "dtd"];
    Svg, Image, "SVG image", ["svg", "svgz"];
    Manifest, Web, "Web manifest", ["webmanifest"];
    Make, Tool, "Make build file", ["mk", "mak"];
    Cmake, Tool, "CMake build file", ["cmake"];
    Meson, Tool, "Meson build file", [];
    Docker, Tool, "Container build/configuration", [];
    Just, Tool, "Just task file", [];
    Task, Tool, "Task build file", [];
    Process, Tool, "Process configuration", [];
    Vagrant, Tool, "Vagrant configuration", [];
    Jenkins, Tool, "Jenkins pipeline", [];
    Brew, Tool, "Homebrew dependencies", [];
    Cargo, Tool, "Cargo manifest/lockfile", [];
    Node, Tool, "Node package manifest/lockfile", [];
    Npm, Tool, "npm configuration", [];
    Yarn, Tool, "Yarn lock/configuration", [];
    Pnpm, Tool, "pnpm lock/configuration", [];
    Bun, Tool, "Bun lock/configuration", [];
    Pub, Tool, "Dart package manifest/lockfile", [];
    GoMod, Tool, "Go module manifest/checksums", [];
    PythonConfig, Tool, "Python dependencies/configuration", [];
    Gradle, Tool, "Gradle build/configuration", ["gradle"];
    Maven, Tool, "Maven build file", [];
    Sbt, Tool, "sbt build file", ["sbt"];
    Dotnet, Tool, ".NET project/solution", ["csproj", "fsproj", "vbproj", "sln", "slnx", "props", "targets"];
    Xcode, Tool, "Xcode project/workspace", ["xcodeproj", "xcworkspace", "pbxproj"];
    Github, Tool, "GitHub workflow", [];
    Gitlab, Tool, "GitLab CI configuration", [];
    Circle, Tool, "CircleCI configuration", [];
    Travis, Tool, "Travis CI configuration", [];
    Azure, Tool, "Azure pipeline", [];
    Terraform, Tool, "Terraform configuration/state", ["tf", "tfvars", "tfstate", "hcl"];
    Kubernetes, Tool, "Kubernetes manifest", [];
    Helm, Tool, "Helm chart", ["helm"];
    Ansible, Tool, "Ansible configuration", [];
    Nix, Tool, "Nix expression", ["nix"];
    Bazel, Tool, "Bazel build file", ["bzl", "bazel"];
    Buck, Tool, "Buck build file", [];
    Git, Repository, "Git metadata", [];
    EditorConfig, Config, "Editor configuration", [];
    Clang, Tool, "Clang configuration", [];
    RustConfig, Tool, "Rust tool configuration", [];
    Readme, Repository, "Readme documentation", [];
    Changelog, Repository, "Changelog", [];
    Contributing, Repository, "Contribution guide", [];
    License, Repository, "License", ["license"];
    Codeowners, Repository, "Repository ownership", [];
    SecurityPolicy, Repository, "Security policy", [];
    Word, Document, "Word document/template", ["doc", "docx", "docm", "dot", "dotx", "dotm"];
    Excel, Document, "Excel spreadsheet/template", ["xls", "xlsx", "xlsm", "xlsb", "xlt", "xltx", "xltm", "xlam"];
    Powerpoint, Document, "PowerPoint presentation/template", ["ppt", "pptx", "pptm", "pps", "ppsx", "ppsm", "pot", "potx", "potm"];
    Odt, Document, "OpenDocument text", ["odt", "ott", "fodt"];
    Ods, Document, "OpenDocument spreadsheet", ["ods", "ots", "fods"];
    Odp, Document, "OpenDocument presentation", ["odp", "otp", "fodp"];
    Odg, Document, "OpenDocument drawing", ["odg", "otg", "fodg"];
    Pdf, Document, "PDF document", ["pdf"];
    Rtf, Document, "Rich text", ["rtf"];
    Pages, Document, "Pages document", ["pages"];
    Numbers, Document, "Numbers spreadsheet", ["numbers"];
    Keynote, Document, "Keynote presentation", [];
    Ebook, Document, "E-book", ["epub", "mobi", "azw", "azw3", "fb2"];
    Djvu, Document, "DjVu document", ["djvu", "djv"];
    Markdown, Text, "Markdown", ["md", "markdown", "mdown", "mkd"];
    Mdx, Text, "MDX document", ["mdx"];
    Text, Text, "Plain text", ["txt", "text"];
    Asciidoc, Text, "AsciiDoc", ["adoc", "asciidoc", "ascdoc"];
    Rst, Text, "reStructuredText", ["rst", "rest"];
    Org, Text, "Org document", ["org"];
    Log, Text, "Log file", ["log", "out"];
    Csv, Data, "CSV data", ["csv"];
    Tsv, Data, "TSV data", ["tsv"];
    JsonLines, Data, "JSON Lines", ["jsonl", "ndjson"];
    Ini, Config, "INI configuration", ["ini"];
    Properties, Config, "Properties configuration", ["properties"];
    Diff, Text, "Diff/patch", ["diff", "patch", "rej"];
    Rss, Feed, "RSS feed", ["rss"];
    Atom, Feed, "Atom feed", ["atom"];
    Opml, Feed, "OPML subscriptions", ["opml"];
    Rdf, Feed, "RDF document", ["rdf"];
    JsonFeed, Feed, "JSON Feed", [];
    Png, Image, "PNG image", ["png", "apng"];
    Jpeg, Image, "JPEG image", ["jpg", "jpeg", "jpe", "jfif"];
    Gif, Image, "GIF image", ["gif"];
    Webp, Image, "WebP image", ["webp"];
    Avif, Image, "AVIF image", ["avif"];
    Bmp, Image, "Bitmap image", ["bmp", "dib"];
    Tiff, Image, "TIFF image", ["tif", "tiff"];
    Heif, Image, "HEIF image", ["heic", "heif", "hif"];
    Icon, Image, "Icon image", ["ico", "icns", "cur", "ani"];
    Photoshop, Image, "Photoshop image", ["psd", "psb"];
    Illustrator, Image, "Illustrator/PostScript artwork", ["ai", "eps", "ps"];
    Indesign, Image, "InDesign document", ["indd", "idml"];
    Figma, Image, "Figma design", ["fig"];
    Krita, Image, "Krita painting", ["kra", "ora"];
    Gimp, Image, "GIMP image", ["xcf"];
    Blender, Image, "Blender scene", ["blend", "blend1"];
    Sketch, Image, "Sketch design", ["sketch"];
    Mp3, Audio, "MP3 audio", ["mp3"];
    Wav, Audio, "WAV audio", ["wav", "wave"];
    Flac, Audio, "FLAC audio", ["flac"];
    Ogg, Audio, "Ogg/Opus audio", ["ogg", "oga", "opus"];
    Aac, Audio, "AAC audio", ["aac", "m4a"];
    Aiff, Audio, "AIFF audio", ["aiff", "aif", "aifc"];
    Midi, Audio, "MIDI sequence", ["mid", "midi", "kar"];
    Audio, Audio, "Audio file", ["wma", "amr", "alac", "ape", "au", "snd", "ac3", "dts"];
    Mp4, Video, "MP4 video", ["mp4", "m4v"];
    Mkv, Video, "Matroska video", ["mkv"];
    Mov, Video, "QuickTime movie", ["mov", "qt"];
    Avi, Video, "AVI video", ["avi"];
    Webm, Video, "WebM video", ["webm"];
    Mpeg, Video, "MPEG video", ["mpeg", "mpg", "mpe", "m2v"];
    TransportStream, Video, "MPEG transport stream", ["m2ts", "mtsvideo"];
    Video, Video, "Video file", ["wmv", "flv", "3gp", "3g2", "vob", "ogv", "mxf"];
    Zip, Archive, "Zip archive", ["zip", "zipx"];
    Rar, Archive, "RAR archive", ["rar"];
    SevenZip, Archive, "7-Zip archive", ["7z"];
    Tar, Archive, "Tar archive", ["tar"];
    TarGz, Archive, "Gzip tar archive", ["tgz"];
    TarXz, Archive, "XZ tar archive", ["txz"];
    TarBz, Archive, "Bzip2 tar archive", ["tbz", "tbz2"];
    TarZst, Archive, "Zstd tar archive", ["tzst"];
    Gzip, Archive, "Gzip archive", ["gz", "gzip"];
    Bzip, Archive, "Bzip2 archive", ["bz2", "bzip2"];
    Xz, Archive, "XZ archive", ["xz"];
    Zstd, Archive, "Zstd archive", ["zst", "zstd"];
    Lz4, Archive, "LZ4 archive", ["lz4", "lz", "lzma", "lzo"];
    Cab, Archive, "Cabinet archive", ["cab"];
    Iso, Specialized, "ISO disk image", ["iso"];
    Dmg, Specialized, "macOS disk image", ["dmg"];
    DiskImage, Specialized, "Disk image", ["img", "binimage", "vhd", "vhdx"];
    Exe, Application, "Windows application", ["exe", "com", "scr"];
    Msi, Application, "Windows installer", ["msi", "msp"];
    Msix, Application, "Windows application package", ["msix", "appx", "msixbundle", "appxbundle"];
    Library, Application, "Shared library", ["dll", "so", "dylib", "a", "lib", "o", "obj"];
    Driver, Application, "System driver", ["sys", "ko"];
    Batch, Application, "Windows batch script", ["bat", "cmd"];
    Powershell, Application, "PowerShell script", ["ps1", "psm1", "psd1"];
    AppImage, Application, "AppImage application", ["appimage"];
    Deb, Application, "Debian package", ["deb", "udeb"];
    Rpm, Application, "RPM package", ["rpm"];
    Elf, Application, "ELF binary", ["elf"];
    Desktop, Application, "Desktop entry", ["desktop"];
    Flatpak, Application, "Flatpak package/reference", ["flatpak", "flatpakref", "flatpakrepo"];
    App, Application, "macOS application bundle", ["app"];
    Pkg, Application, "macOS installer package", ["pkg", "mpkg"];
    Framework, Application, "Framework bundle", ["framework"];
    Plist, Config, "Property list", ["plist"];
    Shell, Config, "Shell script", ["sh", "bash", "zsh", "fish", "ksh", "csh", "command"];
    Env, Config, "Environment configuration", ["env"];
    Config, Config, "Configuration", ["cfg", "conf", "config", "cnf"];
    Service, Config, "System service", ["service", "socket", "target", "mount", "automount", "path"];
    Timer, Config, "System timer", ["timer"];
    Registry, Config, "Windows registry data", ["reg"];
    Sqlite, Data, "SQLite database", ["sqlite", "sqlite3", "db3"];
    Database, Data, "Database", ["db", "dbf"];
    Duckdb, Data, "DuckDB database", ["duckdb"];
    Access, Data, "Access database", ["mdb", "accdb"];
    ColumnData, Data, "Columnar data", ["parquet", "arrow", "feather", "avro", "orc"];
    Scientific, Data, "Scientific data", ["h5", "hdf5", "hdf", "nc", "npy", "npz", "fits", "fit", "mat", "rds", "rdata"];
    Font, Font, "Font", ["ttf", "otf", "woff", "woff2", "eot", "ttc", "pfb", "pfm"];
    Certificate, Security, "Certificate", ["pem", "crt", "cer", "der", "p12", "pfx", "p7b", "p7c"];
    Key, Security, "Cryptographic key", ["key"];
    PublicKey, Security, "Public key", ["pub"];
    Signature, Security, "Signature/armored data", ["sig", "asc"];
    Encrypted, Security, "Encrypted data", ["gpg", "pgp", "enc", "age", "kdbx"];
    Cad, Specialized, "CAD drawing", ["dwg", "dxf", "step", "stp", "iges", "igs", "fcstd", "scad"];
    Model3d, Specialized, "3D model", ["stl", "fbx", "gltf", "glb", "3ds", "dae", "usdz", "usd", "ply"];
    Game, Specialized, "Game asset/project", ["unity", "unitypackage", "uasset", "umap", "godot", "tscn", "tres", "pck", "pak"];
    Save, Specialized, "Saved game/state", ["sav", "save", "state"];
    Subtitle, Specialized, "Subtitles", ["srt", "vtt", "ass", "ssa", "sub", "sbv"];
    Torrent, Specialized, "Torrent metadata", ["torrent"];
    VmDisk, Specialized, "Virtual machine disk/configuration", ["vmdk", "vdi", "qcow", "qcow2", "ova", "ovf", "vmx"];
    Backup, Specialized, "Backup", ["bak", "backup", "old", "orig", "bkp"];
    Geo, Specialized, "Geographic data", ["geojson", "gpx", "kml", "kmz", "shp", "gpkg", "osm", "pbf"];
    Lock, Config, "Lockfile", ["lock"];
}

impl IconId {
    pub fn info(self) -> &'static FileType {
        &CATALOG[self as usize]
    }
}

/// Exact names are checked before patterns and extensions.
pub const FILENAMES: &[(&str, IconId)] = &[
    ("makefile", IconId::Make),
    ("gnumakefile", IconId::Make),
    ("bsdmakefile", IconId::Make),
    ("cmakelists.txt", IconId::Cmake),
    ("meson.build", IconId::Meson),
    ("meson_options.txt", IconId::Meson),
    ("dockerfile", IconId::Docker),
    ("containerfile", IconId::Docker),
    (".dockerignore", IconId::Docker),
    ("docker-compose.yml", IconId::Docker),
    ("docker-compose.yaml", IconId::Docker),
    ("compose.yaml", IconId::Docker),
    ("compose.yml", IconId::Docker),
    ("justfile", IconId::Just),
    (".justfile", IconId::Just),
    ("taskfile.yml", IconId::Task),
    ("taskfile.yaml", IconId::Task),
    ("procfile", IconId::Process),
    ("vagrantfile", IconId::Vagrant),
    ("jenkinsfile", IconId::Jenkins),
    ("brewfile", IconId::Brew),
    ("gemfile", IconId::Ruby),
    ("gemfile.lock", IconId::Ruby),
    ("rakefile", IconId::Ruby),
    ("podfile", IconId::Ruby),
    ("podfile.lock", IconId::Ruby),
    ("cargo.toml", IconId::Cargo),
    ("cargo.lock", IconId::Cargo),
    ("package.json", IconId::Node),
    ("package-lock.json", IconId::Node),
    ("npm-shrinkwrap.json", IconId::Node),
    ("pnpm-lock.yaml", IconId::Pnpm),
    ("pnpm-workspace.yaml", IconId::Pnpm),
    ("yarn.lock", IconId::Yarn),
    (".yarnrc", IconId::Yarn),
    (".yarnrc.yml", IconId::Yarn),
    ("bun.lock", IconId::Bun),
    ("bun.lockb", IconId::Bun),
    ("bunfig.toml", IconId::Bun),
    ("pubspec.yaml", IconId::Pub),
    ("pubspec.lock", IconId::Pub),
    ("go.mod", IconId::GoMod),
    ("go.sum", IconId::GoMod),
    ("go.work", IconId::GoMod),
    ("requirements.txt", IconId::PythonConfig),
    ("pipfile", IconId::PythonConfig),
    ("pipfile.lock", IconId::PythonConfig),
    ("pyproject.toml", IconId::PythonConfig),
    ("poetry.lock", IconId::PythonConfig),
    ("uv.lock", IconId::PythonConfig),
    ("setup.py", IconId::PythonConfig),
    ("setup.cfg", IconId::PythonConfig),
    ("gradle.properties", IconId::Gradle),
    ("build.gradle", IconId::Gradle),
    ("build.gradle.kts", IconId::Gradle),
    ("settings.gradle", IconId::Gradle),
    ("settings.gradle.kts", IconId::Gradle),
    ("gradlew", IconId::Gradle),
    ("pom.xml", IconId::Maven),
    ("build.sbt", IconId::Sbt),
    ("angular.json", IconId::Angular),
    ("tsconfig.json", IconId::TypeScript),
    ("jsconfig.json", IconId::JavaScript),
    (".gitignore", IconId::Git),
    (".gitattributes", IconId::Git),
    (".gitmodules", IconId::Git),
    (".gitkeep", IconId::Git),
    (".gitconfig", IconId::Git),
    (".gitmessage", IconId::Git),
    (".editorconfig", IconId::EditorConfig),
    (".npmrc", IconId::Npm),
    (".nvmrc", IconId::Node),
    (".node-version", IconId::Node),
    (".prettierrc", IconId::Prettier),
    (".eslintrc", IconId::Eslint),
    (".babelrc", IconId::Babel),
    (".clang-format", IconId::Clang),
    (".clang-tidy", IconId::Clang),
    (".rustfmt.toml", IconId::RustConfig),
    ("rustfmt.toml", IconId::RustConfig),
    ("rust-toolchain.toml", IconId::RustConfig),
    ("rust-toolchain", IconId::RustConfig),
    ("codeowners", IconId::Codeowners),
    (".gitlab-ci.yml", IconId::Gitlab),
    (".travis.yml", IconId::Travis),
    ("azure-pipelines.yml", IconId::Azure),
    ("azure-pipelines.yaml", IconId::Azure),
    ("chart.yaml", IconId::Helm),
    ("chart.lock", IconId::Helm),
    ("ansible.cfg", IconId::Ansible),
    ("playbook.yml", IconId::Ansible),
    ("flake.nix", IconId::Nix),
    ("flake.lock", IconId::Nix),
    ("build", IconId::Bazel),
    ("build.bazel", IconId::Bazel),
    ("workspace", IconId::Bazel),
    ("workspace.bazel", IconId::Bazel),
    ("module.bazel", IconId::Bazel),
    ("buck", IconId::Buck),
    (".buckconfig", IconId::Buck),
    (".env", IconId::Env),
    (".bashrc", IconId::Shell),
    (".bash_profile", IconId::Shell),
    (".zshrc", IconId::Shell),
    (".zprofile", IconId::Shell),
    (".profile", IconId::Shell),
];

const COMPOUNDS: &[(&str, IconId)] = &[
    (".tar.gz", IconId::TarGz),
    (".tar.xz", IconId::TarXz),
    (".tar.bz2", IconId::TarBz),
    (".tar.zst", IconId::TarZst),
    (".tf.json", IconId::Terraform),
    (".tfvars.json", IconId::Terraform),
    (".d.ts", IconId::TypeScript),
    (".component.html", IconId::Angular),
    (".component.ts", IconId::Angular),
    (".blade.php", IconId::Php),
    (".so.1", IconId::Library),
    (".so.2", IconId::Library),
];

fn extensions() -> &'static FxHashMap<&'static str, IconId> {
    static MAP: OnceLock<FxHashMap<&'static str, IconId>> = OnceLock::new();
    MAP.get_or_init(|| {
        CATALOG
            .iter()
            .flat_map(|t| t.extensions.iter().map(move |e| (*e, t.icon)))
            .collect()
    })
}
fn filenames() -> &'static FxHashMap<&'static str, IconId> {
    static MAP: OnceLock<FxHashMap<&'static str, IconId>> = OnceLock::new();
    MAP.get_or_init(|| FILENAMES.iter().copied().collect())
}
fn ascii_lower<'a>(bytes: &[u8], out: &'a mut [u8]) -> Option<&'a str> {
    if bytes.len() > out.len() || !bytes.is_ascii() {
        return None;
    }
    for (dst, src) in out.iter_mut().zip(bytes) {
        *dst = src.to_ascii_lowercase();
    }
    std::str::from_utf8(&out[..bytes.len()]).ok()
}
fn ends(bytes: &[u8], suffix: &str) -> bool {
    bytes
        .get(bytes.len().saturating_sub(suffix.len())..)
        .is_some_and(|b| b.eq_ignore_ascii_case(suffix.as_bytes()))
}
fn starts(bytes: &[u8], prefix: &str) -> bool {
    bytes
        .get(..prefix.len())
        .is_some_and(|b| b.eq_ignore_ascii_case(prefix.as_bytes()))
}

pub fn by_extension(ext: &str) -> IconId {
    let mut lower = [0; 64];
    ascii_lower(ext.as_bytes(), &mut lower)
        .and_then(|s| extensions().get(s).copied())
        .unwrap_or(IconId::Unknown)
}

/// No IO. Special paths are matched by components, never directory scans.
pub fn resolve(kind: EntryKind, path: &Path) -> IconId {
    let name = path.file_name().unwrap_or_default().as_encoded_bytes();
    match kind {
        EntryKind::Symlink => return IconId::Symlink,
        EntryKind::Other => return IconId::Special,
        EntryKind::Dir => {
            return if ends(name, ".app") {
                IconId::App
            } else if ends(name, ".framework") {
                IconId::Framework
            } else if ends(name, ".xcodeproj") || ends(name, ".xcworkspace") {
                IconId::Xcode
            } else if ends(name, ".key") {
                IconId::Keynote
            } else if ends(name, ".pages") {
                IconId::Pages
            } else if ends(name, ".numbers") {
                IconId::Numbers
            } else {
                IconId::Folder
            };
        }
        EntryKind::File => {}
    }
    let mut lower = [0; 256];
    if let Some(id) = ascii_lower(name, &mut lower).and_then(|s| filenames().get(s)) {
        return *id;
    }
    for (prefix, id) in [
        ("readme", IconId::Readme),
        ("changelog", IconId::Changelog),
        ("changes", IconId::Changelog),
        ("contributing", IconId::Contributing),
        ("license", IconId::License),
        ("licence", IconId::License),
        ("copying", IconId::License),
        ("security", IconId::SecurityPolicy),
    ] {
        if starts(name, prefix)
            && (name.len() == prefix.len() || name.get(prefix.len()) == Some(&b'.'))
        {
            return id;
        }
    }
    for (prefix, id) in [
        ("dockerfile.", IconId::Docker),
        ("containerfile.", IconId::Docker),
        ("docker-compose.", IconId::Docker),
        (".env.", IconId::Env),
        ("next.config.", IconId::Next),
        ("vite.config.", IconId::Vite),
        ("vitest.config.", IconId::Vite),
        ("webpack.config.", IconId::Webpack),
        ("astro.config.", IconId::Astro),
        ("svelte.config.", IconId::Svelte),
        ("tailwind.config.", IconId::Tailwind),
        ("postcss.config.", IconId::Postcss),
        ("eslint.config.", IconId::Eslint),
        (".eslintrc.", IconId::Eslint),
        ("prettier.config.", IconId::Prettier),
        (".prettierrc.", IconId::Prettier),
        ("babel.config.", IconId::Babel),
        (".babelrc.", IconId::Babel),
        ("tsconfig.", IconId::TypeScript),
        ("requirements-", IconId::PythonConfig),
        ("jenkinsfile.", IconId::Jenkins),
    ] {
        if starts(name, prefix) {
            return id;
        }
    }
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml") {
        let parent = path.parent().and_then(Path::file_name);
        let grandparent = path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name);
        if parent.is_some_and(|c| c.eq_ignore_ascii_case(std::ffi::OsStr::new("workflows")))
            && grandparent.is_some_and(|c| c == ".github")
        {
            return IconId::Github;
        }
        if parent.is_some_and(|c| c == ".circleci") {
            return IconId::Circle;
        }
        if path
            .ancestors()
            .skip(1)
            .take(3)
            .filter_map(Path::file_name)
            .any(|c| c == "kubernetes" || c == "k8s")
        {
            return IconId::Kubernetes;
        }
        if starts(name, "azure-pipelines") {
            return IconId::Azure;
        }
    }
    // Longest matching compound suffix, not last-dot-only interpretation.
    if let Some((_, id)) = COMPOUNDS
        .iter()
        .filter(|(suffix, _)| ends(name, suffix))
        .max_by_key(|(suffix, _)| suffix.len())
    {
        return *id;
    }
    if let Some(at) = name
        .windows(4)
        .position(|s| s.eq_ignore_ascii_case(b".so."))
    {
        let tail = &name[at + 4..];
        if !tail.is_empty() && tail.iter().all(|b| b.is_ascii_digit() || *b == b'.') {
            return IconId::Library;
        }
    }
    by_extension(ext)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    FolderLink,
    Socket,
    Pipe,
    Device,
    Rss,
    Atom,
    Opml,
    JsonFeed,
    TransportStream,
    Matlab,
    Elf,
    Keynote,
}

/// Content can override only a genuinely ambiguous filename classification.
/// Exact tool filenames and special filesystem objects always retain precedence.
pub fn refine(kind: EntryKind, path: &Path, evidence: Option<Evidence>) -> IconId {
    let base = resolve(kind, path);
    if kind == EntryKind::Symlink {
        return if evidence == Some(Evidence::FolderLink) {
            IconId::FolderLink
        } else {
            base
        };
    }
    if kind == EntryKind::Other {
        return match evidence {
            Some(Evidence::Socket) => IconId::Socket,
            Some(Evidence::Pipe) => IconId::Pipe,
            Some(Evidence::Device) => IconId::Device,
            _ => base,
        };
    }
    if kind != EntryKind::File {
        return base;
    }
    match (base, evidence) {
        (IconId::Xml, Some(Evidence::Rss)) => IconId::Rss,
        (IconId::Xml, Some(Evidence::Atom)) => IconId::Atom,
        (IconId::Xml, Some(Evidence::Opml)) => IconId::Opml,
        (IconId::Json, Some(Evidence::JsonFeed)) => IconId::JsonFeed,
        (IconId::TypeScript, Some(Evidence::TransportStream))
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(std::ffi::OsStr::new("ts"))) =>
        {
            IconId::TransportStream
        }
        (IconId::ObjectiveC, Some(Evidence::Matlab)) => IconId::Matlab,
        (IconId::Unknown, Some(Evidence::Elf)) => IconId::Elf,
        (IconId::Key, Some(Evidence::Keynote)) => IconId::Keynote,
        _ => base,
    }
}

/// Only these candidates may be sampled by the optional background worker.
/// In particular .env, configuration secrets, certificates and .key are excluded.
pub fn needs_evidence(kind: EntryKind, path: &Path) -> bool {
    if matches!(kind, EntryKind::Symlink | EntryKind::Other) {
        return true;
    }
    if kind != EntryKind::File {
        return false;
    }
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let base = resolve(kind, path);
    let name = path.file_name().unwrap_or_default().as_encoded_bytes();
    match base {
        IconId::Xml => ext.eq_ignore_ascii_case("xml"),
        // Generic JSON is frequently configuration/secrets. Only plausible
        // feed filenames are sampled automatically; all others stay JSON.
        IconId::Json => name.windows(4).any(|s| s.eq_ignore_ascii_case(b"feed")),
        IconId::TypeScript => ext.eq_ignore_ascii_case("ts") && !ends(name, ".d.ts"),
        IconId::ObjectiveC => ext.eq_ignore_ascii_case("m"),
        _ => false,
    }
}
