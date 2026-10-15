# Known Limitations

Honest status of unfinished or unverified work. Updated at commit `8013ced`.

## Not implemented

- **Native OS file clipboard** — copying files to/from Windows Explorer,
  macOS Finder, or Linux file managers (Dolphin, Nautilus) is not wired up.
  The internal clipboard works correctly (including repeated paste after
  copy); native interop needs platform-specific clipboard APIs
  (`CF_HDROP` / `NSPasteboard` file URLs / `text/uri-list` +
  `x-special/gnome-copied-files`).
- **Dual-pane mode** — not implemented.
- **Release packaging** — CI uploads bare binaries per OS; there is no
  AppImage, `.dmg`/`.app` bundle, or Windows installer. Artifacts are
  unsigned and not notarized.
- **Linux XDG icon themes** — file rows use vector glyph fallbacks; no
  icon-theme lookup is wired in.
- **External drag-out on Linux** — dragging files from Flux Files into
  other apps is not supported by eframe today; internal drag-and-drop
  works.

## Implemented but not fully wired

- **`MetaEngine::set_priority`** — the metadata filler accepts a priority
  index range (for visible-first statting), but the egui view layer does
  not report its visible row range back to the app, so it currently fills
  sequentially. Metadata still lands correctly, just not viewport-first.
- **Cross-platform verification** — Windows and macOS code paths exist and
  compile in CI, but have only been runtime-tested on Linux. CI runs
  fmt/clippy/test/build on all three OSes on every push.
- **Transfer copy APIs** — `std::fs::copy` is used, which already maps to
  `copy_file_range` on Linux. Native `CopyFile2`/`copyfile`/`clonefile`
  integration is not done.
- **Idle repaint on metadata progress** — each `MetaBatch` requests a
  repaint so sizes appear progressively; on very large slow-mount dirs
  this is bounded by batch size but not frame-throttled.

## Platform notes

- **Windows**: UNC paths, junctions, case-insensitive collisions and
  Recycle Bin paths are coded but only CI-verified; not runtime-smoked.
- **macOS**: pasteboard file-URL interop missing (see clipboard above);
  system-appearance detection works via `ctx.system_theme()`.
- **Linux**: tested on Wayland/X11; idle CPU ~0% after watcher fix.
