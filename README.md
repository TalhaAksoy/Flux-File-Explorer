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

## Known limitations (honest list)

- **Windows / Linux untested** — platform code is written (open, trash,
  volumes, watcher) but has only been run on macOS.
- Directory sizes always show "—"; computing them needs a recursive scan,
  which the spec forbids implicitly. (Planned: opt-in per-folder size.)
- Copy engine uses `fs::copy` (APFS clonefile on macOS) and a chunked
  fallback; it does not yet use `copy_file_range`/`sendfile` special-cases
  on Linux or `CopyFile2` flags on Windows.
- No undo for file operations yet.
- Icon rendering is painted glyphs, not OS-native file icons.
- Column widths don't persist yet.
- No accessibility-tree audit beyond what egui/accesskit provides.

## Notes

- Filenames are `OsString` end-to-end; display uses lossy conversion but all
  filesystem operations go through the original OsString — non-UTF-8 names
  work correctly.
- Bulk rename plans detect cycles (a→b, b→a swaps) and execute them via a
  temp name, and detect same-inode/case-only renames.
- Delete is two-stage: trash first; permanent delete requires an explicit
  confirm and is never a silent fallback.
