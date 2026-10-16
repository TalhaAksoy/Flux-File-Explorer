# Known Limitations

Honest status of unfinished or unverified work. Updated at commit
`8013ced` + clipboard/priority/clone work.

## Not implemented

- **Dual-pane mode** — not implemented.
- **Release packaging** — CI uploads bare binaries per OS; there is no
  AppImage, `.dmg`/`.app` bundle, or Windows installer. Artifacts are
  unsigned and not notarized.
- **Linux XDG icon themes** — file rows use vector glyph fallbacks; no
  icon-theme lookup is wired in.
- **External drag-out on Linux** — dragging files from Flux Files into
  other apps is not supported by eframe today (the `drag` crate also
  lacks a Linux backend); internal drag-and-drop works. Blocked
  upstream, not a will-not-fix.
- **macOS Finder "cut"** — Finder has no file-cut verb; Flux cut-to-
  clipboard is a copy there (Cmd+X inside Flux still moves on Cmd+V).

## Implemented but only partially verified

- **Native OS file clipboard** — wired on all three platforms:
  - macOS: `NSPasteboard` file URLs, write **and** read verified at
    runtime (`clipboard_round_trip` test hits the real pasteboard).
  - Windows: `CF_HDROP` + `Preferred DropEffect` (copy/cut verbs),
    compile-checked (`cargo check --target x86_64-pc-windows-msvc`),
    not yet runtime-tested.
  - Linux: `wl-copy`/`wl-paste`/`xclip` subprocesses offering
    `x-special/gnome-copied-files` + `text/uri-list`, compile-checked
    (`x86_64-unknown-linux-gnu`); needs a runtime pass under Wayland
    and X11 (xclip's `-t` on input varies by version).
- **APFS `clonefile` fast-path** — file copies on macOS try a CoW clone
  into the staged temp name first (same atomic commit, cancel honored
  before commit); falls back to the byte-copy on any error. Runtime
  verified on macOS; Windows `CopyFile2`/`copy_file_range` explicitly
  not wired (`std::fs::copy` already maps to `copy_file_range` on
  Linux).
- **Transfer copy APIs** — `std::fs::copy` remains the fallback on all
  platforms; only macOS gained a native fast path (above).
- **Cross-platform verification** — Windows and Linux code paths are
  compile-checked from macOS via `--target`; CI runs fmt/clippy/test/
  build on all three OSes, but only macOS has been runtime-smoked in
  this iteration.

## Wired since last update

- **`MetaEngine::set_priority`** — the egui view now reports the
  rendered entry-index range each frame; background statting covers
  the viewport first.
- **Idle repaint on metadata progress** — `MetaEvent::Batch` repaints
  are coalesced to ~30 fps (`last_meta_repaint`); `Finished` still
  repaints immediately.

## Platform notes

- **Windows**: UNC paths, junctions, case-insensitive collisions and
  Recycle Bin paths are coded but only CI-verified; not runtime-smoked.
- **macOS**: system-appearance detection works via
  `ctx.system_theme()`; pasteboard file interop verified.
- **Linux**: tested on Wayland/X11; idle CPU ~0% after watcher fix.
