use std::cmp::Ordering;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use flux_core::entry::{DirEntry, EntryKind, contains_case_insensitive, is_hidden_name};
use flux_core::snapshot::DirSnapshot;
use flux_core::sort::{SortDir, SortKey, SortSpec, build_view, folded_cmp, natural_cmp};
use flux_core::{EnumEngine, EnumEvent, MetaEngine, MetaEvent, UNKNOWN_SIZE};

fn entry(name: &str, kind: EntryKind, size: u64) -> DirEntry {
    DirEntry {
        name: OsString::from(name),
        kind,
        size,
        mtime: None,
        readonly: false,
        hidden: is_hidden_name(&OsString::from(name)),
    }
}

fn names(snap: &DirSnapshot) -> Vec<String> {
    snap.view()
        .iter()
        .map(|&i| snap.entry(i).unwrap().name.to_string_lossy().into_owned())
        .collect()
}

// ---------- comparators ----------

#[test]
fn natural_sort_orders_digit_runs_numerically() {
    assert_eq!(natural_cmp("file2", "file10", true), Ordering::Less);
    assert_eq!(natural_cmp("file10", "file2", true), Ordering::Greater);
    assert_eq!(natural_cmp("a1b2", "a1b10", true), Ordering::Less);
    assert_eq!(natural_cmp("x", "x", true), Ordering::Equal);
}

#[test]
fn natural_sort_handles_leading_zeros() {
    // Numerically equal: "07" vs "7" — deterministic order, shorter run wins.
    assert_eq!(natural_cmp("f07", "f7", true), Ordering::Greater);
    assert_ne!(natural_cmp("f08", "f7", true), Ordering::Equal);
}

#[test]
fn folded_cmp_is_case_insensitive_when_requested() {
    assert_eq!(folded_cmp("ABC", "abc", true), Ordering::Equal);
    assert_eq!(folded_cmp("abc", "ABD", true), Ordering::Less);
    assert_eq!(folded_cmp("b", "A", false), Ordering::Greater);
    assert_eq!(folded_cmp("b", "A", true), Ordering::Greater); // 'b' > 'a'
    assert_eq!(folded_cmp("A", "b", true), Ordering::Less);
}

#[test]
fn substring_filter_is_case_insensitive() {
    assert!(contains_case_insensitive("Holiday_Photo.JPG", "photo"));
    assert!(contains_case_insensitive("report.pdf", ".PDF"));
    assert!(!contains_case_insensitive("a.txt", "xyz"));
    assert!(contains_case_insensitive("anything", ""));
}

#[test]
fn build_view_sorts_dirs_first_then_natural_names() {
    let entries = vec![
        entry("file10.txt", EntryKind::File, 10),
        entry("file2.txt", EntryKind::File, 5),
        entry("ZDir", EntryKind::Dir, UNKNOWN_SIZE),
        entry("aDir", EntryKind::Dir, UNKNOWN_SIZE),
        entry(".hidden", EntryKind::File, 1),
    ];
    let view = build_view(&entries, &SortSpec::default(), None, false);
    let got: Vec<&str> = view
        .iter()
        .map(|&i| entries[i as usize].name.to_str().unwrap())
        .collect();
    assert_eq!(got, ["aDir", "ZDir", "file2.txt", "file10.txt"]);
}

#[test]
fn build_view_hides_hidden_entries_until_enabled() {
    let entries = vec![
        entry(".secret", EntryKind::File, 1),
        entry("visible", EntryKind::File, 1),
    ];
    let hidden_off = build_view(&entries, &SortSpec::default(), None, false);
    let hidden_on = build_view(&entries, &SortSpec::default(), None, true);
    assert_eq!(hidden_off.len(), 1);
    assert_eq!(hidden_on.len(), 2);
}

#[test]
fn build_view_filters_and_sorts_by_size() {
    let entries = vec![
        entry("big.log", EntryKind::File, 900),
        entry("small.log", EntryKind::File, 3),
        entry("skip.txt", EntryKind::File, 50),
    ];
    let spec = SortSpec {
        key: SortKey::Size,
        dir: SortDir::Desc,
        ..SortSpec::default()
    };
    let view = build_view(&entries, &spec, Some(".log"), false);
    let got: Vec<&str> = view
        .iter()
        .map(|&i| entries[i as usize].name.to_str().unwrap())
        .collect();
    assert_eq!(got, ["big.log", "small.log"]);
}

#[test]
fn header_click_toggles_direction_for_same_key() {
    let spec = SortSpec::default();
    let next = SortSpec::for_header_click(&spec, SortKey::Name);
    assert_eq!(next.dir, SortDir::Desc);
    let other = SortSpec::for_header_click(&spec, SortKey::Size);
    assert_eq!(other.key, SortKey::Size);
    assert_eq!(other.dir, SortDir::Asc);
}

#[test]
fn sort_by_modified_desc() {
    let now = SystemTime::now();
    let mut a = entry("old", EntryKind::File, 0);
    a.mtime = Some(now - Duration::from_secs(1000));
    let mut b = entry("new", EntryKind::File, 0);
    b.mtime = Some(now);
    let c = entry("unknown", EntryKind::File, 0);
    let entries = vec![a, b, c];
    let spec = SortSpec {
        key: SortKey::Modified,
        dir: SortDir::Desc,
        dirs_first: false,
        ..SortSpec::default()
    };
    let view = build_view(&entries, &spec, None, false);
    let got: Vec<&str> = view
        .iter()
        .map(|&i| entries[i as usize].name.to_str().unwrap())
        .collect();
    assert_eq!(got, ["new", "old", "unknown"]);
}

// ---------- snapshot ----------

#[test]
fn snapshot_view_is_arrival_order_while_loading() {
    let mut snap = DirSnapshot::new(PathBuf::from("/tmp"));
    snap.begin_load(PathBuf::from("/tmp/x"), 1);
    let mut b1 = vec![
        entry("b", EntryKind::File, 1),
        entry("a", EntryKind::File, 1),
    ];
    snap.push_batch(&mut b1);
    assert!(b1.is_empty()); // batch was drained
    assert_eq!(names(&snap), ["b", "a"]);
    assert!(snap.state.is_loading());
}

#[test]
fn snapshot_row_maps_view_to_entry() {
    let mut snap = DirSnapshot::new(PathBuf::from("/tmp"));
    snap.begin_load(PathBuf::from("/tmp"), 1);
    snap.push_batch(&mut vec![entry("x", EntryKind::File, 7)]);
    let (idx, e) = snap.row(0).unwrap();
    assert_eq!(idx, 0);
    assert_eq!(e.size, 7);
    assert_eq!(snap.entry_path(idx).unwrap(), PathBuf::from("/tmp/x"));
}

#[test]
fn snapshot_filter_applies_to_later_batches() {
    let mut snap = DirSnapshot::new(PathBuf::from("/tmp"));
    snap.filter = Some("keep".into());
    snap.begin_load(PathBuf::from("/tmp"), 1);
    snap.push_batch(&mut vec![
        entry("keepme", EntryKind::File, 1),
        entry("dropme", EntryKind::File, 1),
    ]);
    assert_eq!(snap.view_len(), 1);
}

// ---------- enumeration engine ----------

fn collect_until_done(engine: &EnumEngine, gen_id: u64, timeout: Duration) -> (usize, bool) {
    let deadline = Instant::now() + timeout;
    let mut total = 0usize;
    while Instant::now() < deadline {
        for ev in engine.drain_events() {
            match ev {
                EnumEvent::Batch {
                    gen_id: g,
                    total: t,
                } if g == gen_id => total = t,
                EnumEvent::Finished {
                    gen_id: g,
                    total: t,
                } if g == gen_id => {
                    return (t, true);
                }
                EnumEvent::Error { gen_id: g, .. } if g == gen_id => return (total, false),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    (total, false)
}

fn make_dir_with_files(n: usize) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    for i in 0..n {
        let mut f = std::fs::File::create(path.join(format!("f{i:06}.txt"))).unwrap();
        f.write_all(b"x").unwrap();
    }
    std::fs::create_dir(path.join("subdir")).unwrap();
    (dir, path)
}

#[test]
fn engine_enumerates_real_directory() {
    let (_tmp, path) = make_dir_with_files(2000);
    let engine = EnumEngine::new();
    let snap = DirSnapshot::handle(path.clone());
    let gen_id = engine.navigate(&snap, path);
    let (total, ok) = collect_until_done(&engine, gen_id, Duration::from_secs(10));
    assert!(ok, "enumeration did not finish");
    assert_eq!(total, 2001);
    let s = snap.read().unwrap();
    assert_eq!(s.entry_count(), 2001);
    assert!(s.entries().iter().any(|e| e.name == "subdir" && e.is_dir()));
}

#[test]
fn meta_engine_fills_sizes_and_mtimes() {
    let (_tmp, path) = make_dir_with_files(300);
    let enum_engine = EnumEngine::new();
    let meta = MetaEngine::new();
    let snap = DirSnapshot::handle(path.clone());
    let gen_id = enum_engine.navigate(&snap, path);
    let (_t, ok) = collect_until_done(&enum_engine, gen_id, Duration::from_secs(10));
    assert!(ok);
    // Lazy enumeration leaves metadata unknown.
    assert!(
        snap.read()
            .unwrap()
            .entries()
            .iter()
            .all(|e| { e.is_dir() || e.size == flux_core::UNKNOWN_SIZE })
    );
    meta.fill(&snap, gen_id);
    // Wait for MetaEvent::Finished.
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut finished = false;
    while Instant::now() < deadline && !finished {
        for ev in meta.drain_events() {
            if matches!(ev, MetaEvent::Finished { gen_id: g } if g == gen_id) {
                finished = true;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(finished, "meta fill did not finish");
    let s = snap.read().unwrap();
    let file = s
        .entries()
        .iter()
        .find(|e| e.name == "f000000.txt")
        .unwrap();
    assert_eq!(file.size, 1);
    assert!(file.mtime.is_some());
    assert!(
        s.entries()
            .iter()
            .all(|e| e.is_dir() || e.size != flux_core::UNKNOWN_SIZE)
    );
}

#[test]
fn rapid_navigation_only_last_generation_wins() {
    let (_t1, p1) = make_dir_with_files(50);
    let (_t2, p2) = make_dir_with_files(60);
    let (_t3, p3) = make_dir_with_files(70);
    let engine = EnumEngine::new();
    let snap = DirSnapshot::handle(p1.clone());
    let _g1 = engine.navigate(&snap, p1);
    let _g2 = engine.navigate(&snap, p2.clone());
    let g3 = engine.navigate(&snap, p3.clone());
    let (total, ok) = collect_until_done(&engine, g3, Duration::from_secs(10));
    assert!(ok);
    assert_eq!(total, 71, "final dir must win (70 files + 1 subdir)");
    assert_eq!(snap.read().unwrap().path, p3);
}

// ---------- compute worker locking ----------

/// A UI reader must never wait for a full sort of a huge directory.
/// The compute worker captures inputs under a read lock, sorts unlocked,
/// and only holds the write lock for the final swap — so read-acquire
/// latency stays at milliseconds, not the ~140ms a 100k sort takes.
#[test]
fn sorting_does_not_block_ui_snapshot_reads() {
    let snap = DirSnapshot::handle(PathBuf::from("/tmp/x"));
    {
        let mut s = snap.write().unwrap();
        s.begin_load(PathBuf::from("/tmp/x"), 7);
        for chunk in 0..25 {
            let mut batch: Vec<DirEntry> = (0..4_000)
                .map(|i| {
                    entry(
                        &format!("f{:06}.txt", chunk * 4_000 + i),
                        EntryKind::File,
                        1,
                    )
                })
                .collect();
            s.push_batch(&mut batch);
        }
        s.finish();
    }
    assert_eq!(snap.read().unwrap().entry_count(), 100_000);

    let engine = flux_core::ComputeEngine::new();
    let mut worst = Duration::ZERO;
    for _ in 0..40 {
        engine.rebuild(&snap, 7); // keep the worker saturated
        let t = Instant::now();
        {
            let s = snap.read().unwrap();
            std::hint::black_box(s.view_len());
        }
        worst = worst.max(t.elapsed());
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        worst < Duration::from_millis(120),
        "UI read stalled {worst:?} — the write lock was held during sorting"
    );
}

#[test]
fn stale_view_rebuild_is_discarded() {
    let snap = DirSnapshot::handle(PathBuf::from("/tmp/y"));
    {
        let mut s = snap.write().unwrap();
        s.begin_load(PathBuf::from("/tmp/y"), 3);
        s.push_batch(&mut vec![
            entry("b", EntryKind::File, 1),
            entry("a", EntryKind::File, 1),
        ]);
        s.finish();
    }
    let engine = flux_core::ComputeEngine::new();
    // Queue a rebuild then immediately mutate — the stale result must not
    // overwrite the newer snapshot state.
    engine.rebuild(&snap, 3);
    {
        let mut s = snap.write().unwrap();
        s.push_batch(&mut vec![entry("z-new", EntryKind::File, 1)]);
    }
    engine.rebuild(&snap, 3);
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if snap.read().unwrap().view_len() == 3 {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("sorted view never applied");
}

#[test]
fn enumeration_error_surfaces_as_failed_state() {
    let engine = EnumEngine::new();
    let snap = DirSnapshot::handle(PathBuf::from("/nonexistent-flux-path-xyz"));
    let gen_id = engine.navigate(&snap, PathBuf::from("/nonexistent-flux-path-xyz"));
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut failed = false;
    while Instant::now() < deadline {
        for ev in engine.drain_events() {
            if let EnumEvent::Error { gen_id: g, .. } = ev
                && g == gen_id
            {
                failed = true;
            }
        }
        if failed {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(failed);
    assert!(matches!(
        snap.read().unwrap().state,
        flux_core::LoadState::Failed(_)
    ));
}
