//! Micro-benchmarks for the per-frame / per-scan hot paths:
//! entry sorting (`build_view`) and the comparator itself at 10k and 100k.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use flux_core::sort::{build_view, name_cmp};
use flux_core::{DirEntry, EntryKind, SortSpec};
use std::ffi::OsString;
use std::time::SystemTime;

fn make_entries(n: usize) -> Vec<DirEntry> {
    (0..n)
        .map(|i| DirEntry {
            name: OsString::from(format!("file_{}_{i:06}.txt", i % 997)),
            kind: if i % 101 == 0 {
                EntryKind::Dir
            } else {
                EntryKind::File
            },
            size: (i * 4096) as u64,
            mtime: Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(i as u64)),
            readonly: false,
            hidden: false,
        })
        .collect()
}

fn bench_view(c: &mut Criterion) {
    let spec = SortSpec::default();
    let mut group = c.benchmark_group("build_view");
    for n in [10_000usize, 100_000] {
        let entries = make_entries(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &entries, |b, entries| {
            b.iter(|| build_view(entries, &spec, None, false));
        });
    }
    group.finish();
}

fn bench_cmp(c: &mut Criterion) {
    let spec = SortSpec::default();
    c.bench_function("name_cmp pair", |b| {
        b.iter(|| {
            name_cmp(
                "file_123_000042.txt",
                "file_99_000123.txt",
                std::hint::black_box(&spec),
            )
        });
    });
}

/// Cost of the compute worker's input capture: cloning the entry list
/// under a read lock is the price for never holding a write lock during
/// sorting. Reported so the trade stays visible.
fn bench_entry_clone(c: &mut Criterion) {
    let mut group = c.benchmark_group("entry_clone");
    for n in [10_000usize, 100_000] {
        let entries = make_entries(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &entries, |b, entries| {
            b.iter(|| entries.to_vec());
        });
    }
    group.finish();
}

/// End-to-end directory enumeration on the real filesystem — measures the
/// eager-stat vs lazy-metadata difference. Directory creation is outside
/// the timed section; only the enum-to-finish time counts.
fn bench_enumerate(c: &mut Criterion) {
    let mut group = c.benchmark_group("enumerate");
    for n in [2_000usize, 10_000] {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..n {
            std::fs::write(dir.path().join(format!("f{i:06}.txt")), b"x").unwrap();
        }
        let path = dir.path().to_path_buf();
        group.bench_with_input(BenchmarkId::from_parameter(n), &path, |b, path| {
            b.iter(|| {
                let snap = flux_core::DirSnapshot::handle(path.clone());
                let engine = flux_core::EnumEngine::new();
                let gen_id0 = engine.navigate(&snap, path.clone());
                // Spin until Finished (or the snapshot reports Ready).
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                loop {
                    let done = engine.drain_events().any(|e| matches!(
                        e,
                        flux_core::EnumEvent::Finished { gen_id, .. } if gen_id == gen_id0
                    ));
                    if done || std::time::Instant::now() > deadline {
                        break;
                    }
                    std::hint::spin_loop();
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_view, bench_cmp, bench_entry_clone, bench_enumerate);
criterion_main!(benches);
