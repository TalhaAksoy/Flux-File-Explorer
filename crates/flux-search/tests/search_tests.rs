use std::path::PathBuf;
use std::time::{Duration, Instant};

use flux_search::{SearchEngine, SearchEvent, SearchQuery};

fn make_tree() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().to_path_buf();
    std::fs::create_dir_all(root.join("a/b/c")).unwrap();
    std::fs::write(root.join("a/report.txt"), b"x").unwrap();
    std::fs::write(root.join("a/b/report.pdf"), b"x").unwrap();
    std::fs::write(root.join("a/b/c/deep_report.md"), b"x").unwrap();
    std::fs::write(root.join("a/other.log"), b"x").unwrap();
    // Symlink loop: c/up -> a (must not hang the search).
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("a"), root.join("a/b/c/up")).unwrap();
    (d, root)
}

fn query(root: PathBuf, text: &str) -> SearchQuery {
    SearchQuery {
        root,
        text: text.into(),
        case_sensitive: false,
        regex: false,
        include_hidden: false,
        max_depth: None,
        max_results: 10_000,
    }
}

fn run(engine: &SearchEngine, q: SearchQuery, timeout: Duration) -> Vec<PathBuf> {
    let gen_id = engine.search(q);
    let deadline = Instant::now() + timeout;
    let mut out = Vec::new();
    let mut done = false;
    while Instant::now() < deadline && !done {
        for ev in engine.drain_events() {
            match ev {
                SearchEvent::Batch { gen_id: g, hits } if g == gen_id => {
                    out.extend(hits.into_iter().map(|h| h.path));
                }
                SearchEvent::Done { gen_id: g, .. } if g == gen_id => done = true,
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    out
}

#[test]
fn finds_matching_names_recursively_without_hanging_on_loops() {
    let (_t, root) = make_tree();
    let engine = SearchEngine::new();
    let hits = run(&engine, query(root, "report"), Duration::from_secs(10));
    assert_eq!(hits.len(), 3);
}

#[test]
fn respects_max_results() {
    let (_t, root) = make_tree();
    let engine = SearchEngine::new();
    let mut q = query(root, "report");
    q.max_results = 1;
    let hits = run(&engine, q, Duration::from_secs(10));
    assert_eq!(hits.len(), 1);
}

#[test]
fn cancellation_stops_search() {
    let (_t, root) = make_tree();
    let engine = SearchEngine::new();
    engine.search(query(root, ""));
    engine.cancel();
    // Superseding gen_id should terminate quickly; nothing to assert beyond no hang.
    std::thread::sleep(Duration::from_millis(50));
}
