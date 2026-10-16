# Known Limitations

Honest status of unfinished or unverified work. Updated on
`dev/stage-a-stabilization` after Stage A stabilization + the first
Stage B feature wave.

## Not implemented / partial

- **Dual-pane mode** — not implemented.
- **Undo coverage** — the journal only inverts cleanly-completed ops
  whose result is exactly known: renames, batch renames, mkdir,
  create-file, duplicate, compress (inverse = trash the result, which
  is recoverable). Copy/move are deliberately not journaled — after
  conflict resolution the real destination set can't be reconstructed
  from the submitted kind. Extract-archive is never undone (an
  "Extract Here" inverse could only trash the *parent* folder — too
  dangerous). Trash/delete are irreversible by design.
- **Gallery view** — currently a large-cell icon grid; a filmstrip /
  big-stage variant is not implemented.
- **Release packaging** — CI uploads bare binaries per OS; there is no
  AppImage, `.dmg`/`.app` bundle, or Windows installer. Artifacts are
  unsigned and not notarized.
- **Linux XDG icon themes** — file rows use vector glyph fallbacks; no
  icon-theme lookup is wired in. Image thumbnails DO work on Linux
  (decoder is bundled, not icon-theme based).
- **External drag-out on Linux** — dragging files from Flux Files into
  other apps is not supported by eframe today (the `drag` crate also
  lacks a Linux backend); internal drag-and-drop works. Blocked
  upstream, not a will-not-fix.
- **macOS Finder "cut"** — Finder has no file-cut verb; Flux cut-to-
  clipboard is a copy there (Cmd+X inside Flux still moves on Cmd+V).
- **Image formats** — thumbnails + preview decode via the bundled
  `image` crate build (png/jpeg/gif/webp/bmp/ico). RAW, HEIC, AVIF,
  TIFF, SVG and video have no decoder wired.
- **Open With** — Linux requires `xdg-utils` (xdg-mime) on PATH and
  scans `$XDG_DATA_{HOME,DIRS}/applications` + Flatpak exports (snap
  isn't scanned). macOS lists installed `.app` bundles without
  capability filtering. Windows delegates to the system's own
  OpenWith dialog — no custom list.
- **ZIP extraction** — symlinks are recreated on Unix only; on
  Windows/others they're skipped with a noted per-entry error.
  "Extract Here" (existing dest) merges in place — a cancelled direct
  extraction leaves already-written files (same as Explorer); the
  staged "Extract to X/" mode cleans up fully on cancel.

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

- **Windows hidden/system attributes** — `FILE_ATTRIBUTE_HIDDEN` and
  `FILE_ATTRIBUTE_SYSTEM` are read in enumeration + deferred metadata;
  dot-prefix still marks hidden on Unix. System files gate behind a
  separate "protected OS files" setting (snapshot `show_system`).
- **Column-width persistence** — `TableBody::widths()` captured per
  frame; settings writes debounced 1.2 s (`settings_dirty_at`).
- **Terminal `cd` quoting** — cmd (`cd /d "…"`), PowerShell
  (`Set-Location -LiteralPath '…'`) and POSIX (`cd '…'`) from the
  spawned shell's executable name.
- **Ops audit fixes** — symlink overwrite now stages+commits like file
  copies (was `AlreadyExists` even with Overwrite); permanent delete
  clears Windows read-only before remove.
- **Staged ZIP creation** — `*.fluxpart-*` sibling write, publish via
  `rename_replace`; symlinks archived as links (never followed);
  cancel/failure leaves zero partial output.
- **Safe ZIP extraction** — zip-slip (`enclosed_name`) + ancestor
  symlink-walk containment; staged when dest is new; conflict
  resolution when merging; Unix symlink recreation.
- **Opt-in folder sizes** — `SizeEngine` (lstat walk, symlink-safe,
  cancellable); row context-menu "Calculate Folder Size".
- **Preview panel** — Alt+P / status-bar toggle; async worker with
  latest-request-wins coalescing; images (bounded decode), text
  snippets, dir counts, link targets; persisted `show_preview`.
- **Thumbnails** — `ThumbCache` worker, bounded LRU (256 × ≤64 px),
  `image::Limits` decode guards; list + grid cells.
- **View modes** — `view_mode` setting: Details/Compact/Grid/Gallery;
  grid keyboard nav uses live column count.
- **Open With** — `.desktop` resolution + Exec field-code expansion on
  Linux, `.app` enumeration on macOS, `OpenAs_RunDLL` on Windows.
- **Undo/Redo + ops history** — journal of invertible ops (see
  coverage note above); Operations window lists every op.
- **`MetaEngine::set_priority`** — the egui view reports the rendered
  entry-index range each frame; background statting covers the
  viewport first.
- **Idle repaint on metadata progress** — `MetaEvent::Batch` repaints
  are coalesced to ~30 fps (`last_meta_repaint`); `Finished` still
  repaints immediately.

## Platform notes

- **Windows**: UNC paths, junctions, case-insensitive collisions and
  Recycle Bin paths are coded but only CI-verified; not runtime-smoked.
- **macOS**: system-appearance detection works via
  `ctx.system_theme()`; pasteboard file interop verified.
- **Linux**: tested on Wayland/X11; idle CPU ~0% after watcher fix.
