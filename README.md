# Flux Files

A native, cross-platform file explorer written in Rust — Windows File
Explorer familiarity, macOS Finder polish, Linux file-manager productivity,
and native-app responsiveness. Not Electron, not Tauri, not a webview.

**Status: early development.** Milestone 1 (fast navigation + virtualized
large-directory browsing) is implemented and benchmarked on macOS arm64.
Windows and Linux builds are written but not yet validated on those platforms.

## Architecture

Cargo workspace, Rust 2024 edition:

| Crate | Role |
|---|---|
| `flux-core` | GUI-independent model: directory entries, snapshot, streaming enumeration worker, background filter/sort compute |
| `flux-platform` | OS adapters: open-with-system, trash, known dirs, volumes, fs watcher |
| `flux-operations` | Cancellable copy/move/delete/trash engine, staged writes, conflict policies, cycle-safe batch rename |
| `flux-search` | Cancellable recursive search (explicit user action only — no indexer, no daemon) |
| `flux-settings` | Atomic JSON persistence (window, tabs, theme, recents) |
| `flux-ui` | egui/eframe 0.36 UI: virtualized `TableBuilder` list, sidebar, breadcrumbs, tabs, dialogs, palette |
| `flux-app` | `flux-files` executable |
| `flux-bench` | Dataset generator + real-measurement harness, criterion micro-benches |

Concurrency: dedicated worker threads + bounded channels. No tokio —
blocking filesystem calls stay on workers; the UI thread only renders.

## Build

```sh
cargo build --release -p flux-app    # binary: target/release/flux-files
./target/release/flux-files [dir]    # optional start directory
cargo test --workspace               # 34 tests (core, operations, search)
cargo bench -p flux-bench            # sort/view micro-benchmarks
```

## Benchmarks (real measurements, macOS arm64, Apple Silicon)

Generated datasets via `flux-bench gen`, measured via `flux-bench run`
and criterion. Numbers are from this machine; re-run to verify.

| Metric | 10,000 entries | 100,000 entries |
|---|---|---|
| Time to first visible batch | 2.5 ms | 2.5 ms |
| Full enumeration | 21.4 ms | 243 ms |
| Filter+sort view compute (background thread) | 15.5 ms | 206 ms |
| Snapshot model memory | 767 KiB | 7.5 MiB (~75 B/entry) |
| Process RSS delta during scan | 1.8 MiB | 11.9 MiB |
| Name comparator (natural, case-folded, zero-alloc) | ~69 ns/comparison | same |

Cold launch: window open in well under a second; ~110 MiB RSS including the
GPU context and font atlas; 0.0% CPU at idle (repaint is event-driven).
Live app smoke test with the 100k dir loaded: ~120 MiB RSS, stable, 0% CPU.

Copy throughput (APFS, clonefile-capable `fs::copy` path): 1.4 GiB/s on
200×512KiB files. Same-filesystem moves are metadata-only renames.

## Working today

- Sidebar (known dirs, volumes, recents), breadcrumbs, address edit, tabs
- Background enumeration with progressive batches; rapid navigation cancels
  stale scans via generation counters
- Virtualized row rendering — scroll cost is O(visible rows), not O(entries)
- Sort by name/size/modified/type, hidden-file toggle, substring filter
- Selection: click, shift-range, cmd/ctrl-toggle, keyboard nav, select-all
- Copy/cut/paste, trash, delete, rename, new folder/file, bulk rename with
  preview + conflict-safe ordering, mkdir, native open, drag-drop copy-in
- Conflict dialog (replace / keep-both / skip / apply-to-all), op progress
  with cancel in the status bar
- Recursive search on demand (⌘F-mode toggle), command palette (⌘K)
- fs watcher → debounced auto-refresh; settings persist across restarts
- **Native OS file icons** (macOS/Windows via `file_icon_provider`), fetched
  on a background worker, cached by extension — only visible rows trigger
  fetches; painted glyphs are the fallback (and Linux default)
- **Drag & drop**: rows drag onto folder rows / breadcrumbs / sidebar /
  volumes to move (hold Option/Alt to copy) with a cursor ghost; drag out
  of the window into Finder/other apps on macOS/Windows; drag files in
  from the OS to copy them into the current folder
- Duplicate (⌘D / context menu) → "name copy.ext" via the ops engine
- **Context menus** (Explorer/Finder-style): row menu with Open / Open in
  New Tab / Reveal in File Manager / Cut / Copy / Paste Into / Duplicate /
  Compress to .zip / Rename / Add to Sidebar / Move to Trash / Delete
  Permanently / Copy Path / Copy Name / Properties; empty-area menu with
  New Folder / New File / Paste / Sort By / Row Density / Refresh /
  Open Terminal Here / Properties; sidebar menu with Open / Open in New
  Tab / Remove from Sidebar
- Compress to .zip on the ops engine — staged `*.fluxpart-*` write
  published by rename only when complete; symlinks stored as links,
  never followed; a cancel/failure never leaves a misleading archive
- **Safe ZIP extraction** — `enclosed_name` zip-slip guard plus a
  per-component symlink walk (writes can't escape through planted
  links); "Extract to X/" is staged so cancel leaves no partial dir;
  "Extract Here" merges with per-entry conflict resolution; symlink
  entries are recreated on Unix, never written through
- **View modes** — Details (classic table), Compact (names-only dense
  list), Grid and Gallery icon cells, virtualized; persisted + keyboard
  nav adapts to the grid's column count
- **Preview panel** (Alt+P / status-bar toggle) — async worker decodes
  bounded images and text snippets; stale results for a moved selection
  are dropped; persisted setting
- **Image thumbnails** — dedicated decode worker, bounded LRU (256),
  pixel/allocation caps against decompression bombs; grid/gallery and
  list rows show them inline
- **Open With** — Linux resolves `.desktop` handlers via `xdg-mime`
  (default marked); macOS lists installed app bundles; Windows calls
  the system's own OpenWith dialog
- **Opt-in folder sizes** — right-click "Calculate Folder Size" runs a
  cancellable worker; symlinks are never recursed
- **Undo/Redo** (Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y) — a journal inverts
  cleanly-finished renames, batch renames, mkdir/create-file, duplicate
  and compress ops (inverse = rename-back or trash-the-result)
- **Operation history** — status-bar clock opens a session log of every
  op with state + progress, plus a one-click Undo entry point
- Windows hidden/system file attributes (`FILE_ATTRIBUTE_HIDDEN`,
  `FILE_ATTRIBUTE_SYSTEM`) drive the hidden toggle; system files gate
  behind a separate "protected OS files" setting
- Column widths persist (debounced writes); embedded terminal `cd`
  quoting handles cmd / PowerShell / POSIX shells

## Known limitations (honest list)

- **Windows / Linux untested** — platform code is written (open, trash,
  volumes, watcher) but has only been run on macOS.
- Undo covers only fully-invertible ops (renames, mkdir, create-file,
  duplicate, compress). Copy/move aren't journaled — the post-conflict
  destination set isn't recoverable from the op description alone.
  Extract isn't undone (an "Extract Here" inverse could only trash the
  parent dir). Trash and permanent delete are irreversible by nature.
- Gallery mode is a large-cell icon grid — a filmstrip-style variant
  with a big preview stage isn't implemented.
- Linux "Open With" needs `xdg-utils` (`xdg-mime`) installed; Flatpak
  exports are scanned, snap handlers aren't.
- Thumbnails/previews decode the bundled `image` formats
  (png/jpeg/gif/webp/bmp/ico); RAW, HEIC, AVIF and video aren't covered.
- ZIP extraction recreates symlinks on Unix only; elsewhere they're
  skipped with a noted error.
- Copy engine uses `fs::copy` (APFS clonefile on macOS) and a chunked
  fallback; it does not yet use `copy_file_range`/`sendfile` special-cases
  on Linux or `CopyFile2` flags on Windows.
- Dragging files **out** to the OS is macOS/Windows-only (`drag` crate's
  Linux backend needs GTK, incompatible with winit). Icon fetching is
  macOS/Windows-only for the same reason; Linux shows painted glyphs.
- Drag-out preview shows the item's own contents when it's an image file;
  other types may get a blank drag image on macOS.
- No accessibility-tree audit beyond what egui/accesskit provides.

## Notes

- Filenames are `OsString` end-to-end; display uses lossy conversion but all
  filesystem operations go through the original OsString — non-UTF-8 names
  work correctly.
- Bulk rename plans detect cycles (a→b, b→a swaps) and execute them via a
  temp name, and detect same-inode/case-only renames.
- Delete is two-stage: trash first; permanent delete requires an explicit
  confirm and is never a silent fallback.
