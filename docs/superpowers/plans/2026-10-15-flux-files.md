# Flux Files — Milestone Implementation Plan

> Executed inline per user's explicit autonomous-execution instruction ("Start implementing",
> "make reasonable engineering decisions independently"). Spec = user's project brief.

**Goal:** Native, high-performance cross-platform file explorer in Rust/egui that browses
100k+ entry directories without UI freezes, with a safe background file-operation engine.

**Architecture:** Cargo workspace. `flux-core` (entries, snapshot, streaming enumeration,
sort/filter — no GUI deps), `flux-platform` (open/trash/volumes/known dirs/terminal),
`flux-operations` (queued copy/move/rename/trash/delete, cancellable, progress,
conflict policy, safe staged batch rename), `flux-search` (in-view filter + recursive
filename search), `flux-settings` (serde JSON, atomic writes), `flux-ui` (eframe/egui,
virtualized `egui_extras::TableBuilder` list, tabs, sidebar, breadcrumbs, dialogs),
`flux-app` (binary), `flux-bench` (dataset generator + JSON benchmark harness +
criterion benches).

**Tech stack:** eframe/egui 0.36 + egui_extras, glow renderer; std threads + bounded
mpsc (no tokio); trash, notify, fs2, directories, filetime, regex, serde_json, jiff.

## Global constraints

- No blocking filesystem work on the UI thread; rendering cost ∝ visible rows.
- Generation tokens invalidate stale enumeration; selection survives sort/refresh by id/name.
- Copy uses temp-file-then-rename; cancel leaves no complete-looking partial file.
- Move: same-volume `rename` first; cross-volume copy → verify size → delete source.
- Never overwrite by default; conflict dialog with apply-to-all.
- Snapshot held as `Arc<RwLock<DirSnapshot>>`; workers append batches, compute thread
  rebuilds `view: Vec<u32>` (sort+filter) — UI only reads visible indices.
- Sort: zero-alloc natural/case-insensitive comparator; view sorted on compute thread.
- Rust 2024 edition, `std` first; no `unsafe` except documented platform FFI (none yet).

## Tasks

1. **Workspace + skeletons compile** — all crate manifests, empty libs, `cargo check` green.
2. **flux-core entry model + comparators** — `DirEntry`, `EntryKind`, `SortSpec`,
   `natural_cmp` (zero-alloc), `cmp_entries`; TDD: ordering, natural sort, dirs-first.
3. **flux-core snapshot + streaming enumeration** — `DirSnapshot`, `EnumEngine`
   (bounded channel, generation cancel, batch 512), `ComputeEngine` (view rebuild);
   TDD: batch append, generation invalidation, filter matching, view rebuild.
4. **flux-platform** — open/trash/volumes/known-dirs/free-space/open-terminal;
   TDD where testable (volumes list non-empty on macOS), else smoke-tested.
5. **flux-operations engine** — job queue, progress registry, cancel, conflict
   resolution channel, copy/move/delete/trash/rename/mkdir; TDD on real tempdirs:
   copy tree + verify, move same-volume, cancel mid-copy leaves source intact and no
   complete dest, conflict keep-both naming, trash on macOS (or skip-if).
6. **Batch rename engine** — rule chain → preview plan (dup/conflict/invalid/case
   detection) → collision-safe two-phase execution; TDD: cycles A↔B, case-only,
   duplicate detection, token expansion.
7. **flux-search** — `search` worker: iterative DFS, no symlink follow, cancel,
   batched hits; TDD on generated tree incl. symlink loop.
8. **flux-settings** — load/save atomic, defaults, round-trip test.
9. **flux-ui app shell** — theme, layout (toolbar/breadcrumb/sidebar/table/status),
   tab model, selection (ctrl/shift/range), keyboard nav, sort-on-header,
   filter box, hidden toggle, refresh, open file/dir, context menu.
10. **flux-ui file ops wiring** — dialogs (rename/new folder/delete confirm/conflict),
    op progress UI with cancel, bulk-rename dialog with preview.
11. **flux-bench** — dataset generator (1k/10k/100k/1M option), enumeration +
    view-build + memory benchmarks writing JSON; criterion benches for comparators.
12. **Verification + docs** — fmt/clippy/test green, release build, run bench,
    README/ARCHITECTURE/PERFORMANCE/TESTING with real numbers.

## Review focus (spec-implied, not unit-covered)

- Non-UTF-8 filename on disk displays lossy but operations still work (OsString paths).
- 100k-entry dir: rapid A→B→C→D navigation within 1s never shows wrong dir (gen check).
- Cancel mid-copy of large file leaves *no* complete-looking destination.
- Sort/filter while enumeration still streaming: no panic, consistent final view.
- Case-only rename on case-insensitive APFS/NTFS (FILE→file) succeeds via staging.
