#!/usr/bin/env python3
"""Generate disposable real files and capture actual native egui windows.
Usage: python3 scripts/icon-qa.py <output-dir> [path/to/flux-files]
No browser, downloaded artwork or third-party fixture assets.
"""
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import time
import zlib

out = Path(sys.argv[1]).resolve()
binary = Path(sys.argv[2] if len(sys.argv) > 2 else "target/release/flux-files").resolve()
root = out / "fixtures"
groups = {
    "programming": ["01-main.rs", "02-app.ts", "03-App.tsx", "04-app.js", "05-App.jsx", "06-main.py", "07-main.go", "08-main.c", "09-main.cpp", "10-main.cs", "11-Main.java", "12-Main.kt", "13-App.swift", "14-main.rb", "15-index.php", "16-script.lua", "17-app.ex", "18-main.hs", "19-main.zig", "20-main.jl", "21-main.dart", "22-index.html", "23-style.css", "24-style.scss", "25-App.vue", "26-App.svelte", "27-index.astro", "28-query.graphql", "29-schema.proto", "30-main.wasm"],
    "office": ["Report.docx", "Budget.xlsx", "Slides.pptx", "Letter.odt", "Analysis.ods", "Talk.odp", "Document.pdf", "Notes.pages", "Accounts.numbers", "Book.epub", "Typography.tex"],
    "feeds-config": ["README.md", "CHANGELOG.md", "LICENSE", "Makefile", "Dockerfile", "Cargo.toml", "Cargo.lock", "package.json", "tsconfig.json", "vite.config.ts", ".gitignore", ".env.production", "feed.rss", "feed.atom", "feeds.opml", "feed.xml", "feed.json", "normal.xml", "settings.toml", "subscriptions.rdf"],
    "archives-apps": ["backup.tar.gz", "backup.tar.zst", "archive.zip", "archive.7z", "archive.rar", "image.iso", "image.dmg", "setup.exe", "setup.msi", "editor.AppImage", "package.deb", "package.rpm", "library.so.2", "script.sh", "script.ps1", "public.pub", "private.key", "certificate.crt", "disk.qcow2"],
    "images-media": ["photo.png", "unavailable.png", "design.psd", "drawing.ai", "painting.kra", "scene.blend", "vector.svg", "music.flac", "song.mp3", "score.mid", "movie.mp4", "stream.ts", "font.woff2", "data.sqlite", "data.parquet", "map.geojson", "model.stl", "unknown.customextension"],
}
for group, names in groups.items():
    folder = root / group
    folder.mkdir(parents=True, exist_ok=True)
    for name in names:
        (folder / name).write_bytes(b"")
(root / "feeds-config" / "feed.xml").write_text('<?xml version="1.0"?><rss version="2.0"><channel><title>Flux</title></channel></rss>')
(root / "feeds-config" / "feed.json").write_text('{"version":"https://jsonfeed.org/version/1.1","title":"Flux","items":[]}')
(root / "feeds-config" / "normal.xml").write_text('<configuration><value>not a feed</value></configuration>')
(root / "feeds-config" / "README.md").write_text('# Flux icon fixtures\nDisposable file-type samples.\n')
ts = bytearray(564)
for offset in [0, 188, 376]:
    ts[offset] = 0x47
    ts[offset+3] = 0x10
(root / "images-media" / "stream.ts").write_bytes(ts)
(root / "images-media" / "vector.svg").write_text('<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><circle cx="32" cy="32" r="24" fill="#4477aa"/></svg>')
# Original synthetic content image, not an interface mockup.
def chunk(kind, payload):
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload) & 0xffffffff)
w, h = 192, 128
pixels = b"".join(b"\0" + bytes(v for x in range(w) for v in (int(x * 255 / w), int(y * 255 / h), 160)) for y in range(h))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(pixels)) + chunk(b"IEND", b"")
(root / "images-media" / "photo.png").write_bytes(png)
(root / "archives-apps" / "Editor.app").mkdir(exist_ok=True)
(root / "office" / "Presentation.key").mkdir(exist_ok=True)

cases = [
    ("programming-details-dark", "programming", "List", "dark", 1),
    ("programming-grid-dark", "programming", "Grid", "dark", 1),
    ("programming-grid-light", "programming", "Grid", "light", 1),
    ("office-grid-light", "office", "Grid", "light", 1),
    ("office-details-dark", "office", "List", "dark", 1),
    ("feeds-config-grid-dark", "feeds-config", "Grid", "dark", 1),
    ("feeds-config-details-light", "feeds-config", "List", "light", 1),
    ("archives-apps-grid-dark", "archives-apps", "Grid", "dark", 1),
    ("images-media-grid-light", "images-media", "Grid", "light", 1),
    ("images-media-gallery-dark", "images-media", "Gallery", "dark", 1),
    ("programming-grid-200", "programming", "Grid", "dark", 2),
    ("programming-compact-light", "programming", "Compact", "light", 1),
]
for name, group, view, theme, scale in cases:
    env = {k: v for k, v in os.environ.items() if not k.startswith("FLUX_SHOT_")}
    config = out / f"{name}.json"
    (out / f"{name}.egui.ron").unlink(missing_ok=True)
    config.write_text(json.dumps(dict(theme=theme.title(), density="Standard", icon_source="Flux", show_hidden=True,
        show_system_files=False, show_preview=False, view_mode=view, confirm_permanent_delete=True,
        tabs=[], bookmarks=[], recents=[], places_order=[], sidebar_width=190, column_widths=[340,90,160,90],
        sort_key="name", sort_desc=False, dirs_first=True, ui_font="", mono_font="", terminal_font_size=12.5, ui_scale=scale)))
    env.update(FLUX_SETTINGS_PATH=str(config), FLUX_SHOT_OUT=str(out / f"{name}.png"),
        FLUX_SHOT_DIR=str(root / group), FLUX_SHOT_SIZE="1440x900", FLUX_SHOT_WAIT="120", FLUX_SHOT_THEME=theme,
        XDG_DATA_HOME=str(out / f"xdg-{name}"))
    start = time.monotonic()
    with (out / f"{name}.log").open("w") as log:
        subprocess.run([str(binary)], env=env, stdout=log, stderr=log, check=True, timeout=40)
    if not (out / f"{name}.png").exists():
        raise RuntimeError(f"No native capture: {name}")
    print(name, f"capture+exit={time.monotonic()-start:.3f}s", flush=True)
