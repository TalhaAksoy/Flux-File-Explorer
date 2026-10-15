//! Flux Files benchmark harness.
//!
//! Usage:
//!   flux-bench gen <dir> <count>      create <count> files (+ some dirs)
//!   flux-bench run <dir>              measure enumeration / view / memory
//!   flux-bench copy <src_dir> <dst>   measure copy throughput
//!
//! All numbers are real measurements; nothing is extrapolated or faked.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use flux_core::{ComputeEngine, ComputeEvent, DirSnapshot, EnumEngine, EnumEvent};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("gen"), Some(dir)) => {
            let count: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(10_000);
            gen_dataset(Path::new(&dir), count);
            ExitCode::SUCCESS
        }
        (Some("run"), Some(dir)) => run(Path::new(&dir)),
        (Some("copy"), Some(src)) => {
            let Some(dst) = args.next() else {
                eprintln!("usage: flux-bench copy <src_dir> <dst_dir>");
                return ExitCode::FAILURE;
            };
            copy_bench(Path::new(&src), Path::new(&dst))
        }
        _ => {
            eprintln!(
                "usage:\n  flux-bench gen <dir> <count>\n  flux-bench run <dir>\n  flux-bench copy <src_dir> <dst_dir>"
            );
            ExitCode::FAILURE
        }
    }
}

/// Create `count` empty files plus ~1% subdirectories.
fn gen_dataset(dir: &Path, count: usize) {
    let t0 = Instant::now();
    std::fs::create_dir_all(dir).expect("create bench dir");
    // Realistic name mix: prefixes + variable-width numbering stresses
    // natural sort; extensions spread the kind column.
    const EXTS: [&str; 8] = ["txt", "jpg", "png", "rs", "zip", "md", "bin", "dat"];
    for i in 0..count {
        let name = if i % 101 == 0 {
            format!("sub_{i:06}")
        } else {
            format!("file_{}_{:06}.{}", i % 997, i, EXTS[i % EXTS.len()])
        };
        let p = dir.join(name);
        if i % 101 == 0 {
            std::fs::create_dir_all(&p).expect("mkdir");
        } else {
            std::fs::File::create(&p).expect("create file");
        }
    }
    println!(
        "generated {} entries in {} ({})",
        count,
        fmt_dur(t0.elapsed()),
        dir.display()
    );
}

/// Process RSS in bytes via `ps`. macOS/Linux only; returns None elsewhere.
fn rss_bytes() -> Option<u64> {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    // ps reports KiB.
    s.trim().parse::<u64>().ok().map(|k| k * 1024)
}

fn run(dir: &Path) -> ExitCode {
    if !dir.is_dir() {
        eprintln!("not a directory: {}", dir.display());
        return ExitCode::FAILURE;
    }
    let rss0 = rss_bytes();

    // --- Enumeration ---
    let snap = DirSnapshot::handle(dir.to_path_buf());
    let engine = EnumEngine::new();
    let t0 = Instant::now();
    let g = engine.navigate(&snap, dir.to_path_buf());
    let mut first_batch: Option<Duration> = None;
    let mut total = 0usize;
    loop {
        let mut finished = false;
        for ev in engine.drain_events() {
            match ev {
                EnumEvent::Batch { gen_id, .. } if gen_id == g => {
                    if first_batch.is_none() {
                        first_batch = Some(t0.elapsed());
                    }
                }
                EnumEvent::Finished { gen_id, total: t } if gen_id == g => {
                    total = t;
                    finished = true;
                }
                EnumEvent::Error { message, .. } => {
                    eprintln!("enumeration error: {message}");
                    return ExitCode::FAILURE;
                }
                _ => {}
            }
        }
        if finished {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let enum_time = t0.elapsed();

    // --- View computation (filter+sort on compute thread) ---
    let compute = ComputeEngine::new();
    let t1 = Instant::now();
    compute.rebuild(&snap, g);
    loop {
        let done = compute
            .drain_events()
            .any(|ev| matches!(ev, ComputeEvent::ViewReady { gen_id } if gen_id == g));
        if done {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let view_time = t1.elapsed();

    let snap_read = snap.read().unwrap();
    let model_bytes = snap_read.memory_estimate();
    drop(snap_read);
    let rss1 = rss_bytes();

    println!("== flux-bench results ==");
    println!("dir:                 {}", dir.display());
    println!("entries:             {total}");
    println!(
        "time to first batch: {}",
        fmt_dur(first_batch.unwrap_or_default())
    );
    println!("full enumeration:    {}", fmt_dur(enum_time));
    println!("view compute:        {}", fmt_dur(view_time));
    println!("snapshot model mem:  {}", fmt_bytes(model_bytes as u64));
    if let (Some(a), Some(b)) = (rss0, rss1) {
        println!(
            "process RSS:         {} before, {} after (delta {})",
            fmt_bytes(a),
            fmt_bytes(b),
            fmt_bytes(b.saturating_sub(a))
        );
    }
    ExitCode::SUCCESS
}

/// Copy `src` recursively into `dst/src_name` via the real OpEngine and
/// report wall time + throughput.
fn copy_bench(src: &Path, dst: &Path) -> ExitCode {
    if !src.is_dir() {
        eprintln!("not a directory: {}", src.display());
        return ExitCode::FAILURE;
    }
    std::fs::create_dir_all(dst).expect("create dst");
    let engine = flux_operations::OpEngine::new();
    let t0 = Instant::now();
    let id = engine.submit(
        flux_operations::OpKind::Copy {
            sources: vec![src.to_path_buf()],
            dest_dir: dst.to_path_buf(),
        },
        flux_operations::ConflictPolicy::OverwriteAll,
    );
    // Poll progress until terminal state.
    let final_progress;
    loop {
        let progress = engine
            .all_ops()
            .iter()
            .find(|j| j.id == id)
            .map(|j| j.progress_snapshot());
        match progress.as_ref().map(|p| &p.state) {
            Some(flux_operations::OpState::Done)
            | Some(flux_operations::OpState::DoneWithErrors) => {
                final_progress = progress.unwrap();
                break;
            }
            Some(flux_operations::OpState::Failed(_))
            | Some(flux_operations::OpState::Cancelled)
            | None => {
                eprintln!("copy failed or cancelled");
                return ExitCode::FAILURE;
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(20));
        // drain to keep channel unblocked
        let _: Vec<_> = engine.drain_events().collect();
    }
    let dt = t0.elapsed();
    let bytes = final_progress.total_bytes;
    println!("== flux-bench copy ==");
    println!("items:     {}", final_progress.total_items);
    println!("bytes:     {}", fmt_bytes(bytes));
    println!("time:      {}", fmt_dur(dt));
    if dt.as_secs_f64() > 0.0 {
        println!(
            "throughput: {}/s",
            fmt_bytes((bytes as f64 / dt.as_secs_f64()) as u64)
        );
    }
    let _ = std::io::stdout().flush();
    ExitCode::SUCCESS
}

fn fmt_dur(d: Duration) -> String {
    if d.as_secs() >= 1 {
        format!("{:.2}s", d.as_secs_f64())
    } else {
        format!("{:.1}ms", d.as_secs_f64() * 1000.0)
    }
}

fn fmt_bytes(b: u64) -> String {
    const U: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", U[i])
}
