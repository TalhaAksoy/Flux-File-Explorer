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

criterion_group!(benches, bench_view, bench_cmp);
criterion_main!(benches);
