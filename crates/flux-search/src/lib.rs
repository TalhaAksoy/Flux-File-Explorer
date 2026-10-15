//! flux-search: recursive filename search as a cancellable streaming worker.
//! Current-dir filtering lives in flux-core's view rebuild; this crate covers
//! explicit recursive search only. Never follows symlinks (loop-proof by
//! construction), never indexes anything.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use flux_core::entry::contains_case_insensitive;

#[derive(Clone, Debug)]
pub struct SearchQuery {
    pub root: PathBuf,
    /// Substring (or regex when `regex` is true) on the file NAME.
    pub text: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub include_hidden: bool,
    /// `None` = unbounded.
    pub max_depth: Option<u32>,
    pub max_results: usize,
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug)]
pub enum SearchEvent {
    Batch {
        gen_id: u64,
        hits: Vec<SearchHit>,
    },
    Done {
        gen_id: u64,
        total: usize,
        cancelled: bool,
    },
}

struct Job {
    gen_id: u64,
    query: SearchQuery,
}

pub struct SearchEngine {
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<SearchEvent>,
    cur_gen: Arc<AtomicU64>,
    _worker: JoinHandle<()>,
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

const BATCH: usize = 128;

impl SearchEngine {
    pub fn new() -> Self {
        let (tx, rx_job) = mpsc::channel::<Job>();
        let (ev_tx, rx) = mpsc::sync_channel::<SearchEvent>(1024);
        let cur_gen = Arc::new(AtomicU64::new(0));
        let worker_gen = Arc::clone(&cur_gen);
        let worker = std::thread::Builder::new()
            .name("flux-search".into())
            .spawn(move || {
                while let Ok(mut job) = rx_job.recv() {
                    while let Ok(newer) = rx_job.try_recv() {
                        job = newer;
                    }
                    run_search(&job, &ev_tx, &worker_gen);
                }
            })
            .expect("spawn search worker");
        Self {
            tx,
            rx,
            cur_gen,
            _worker: worker,
        }
    }

    /// Start a search; returns its generation. Supersedes any running one.
    pub fn search(&self, query: SearchQuery) -> u64 {
        let gen_id = self.cur_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.tx.send(Job { gen_id, query });
        gen_id
    }

    pub fn cancel(&self) {
        self.cur_gen.fetch_add(1, Ordering::SeqCst);
    }

    pub fn drain_events(&self) -> impl Iterator<Item = SearchEvent> + '_ {
        self.rx.try_iter()
    }
}

struct Matcher {
    re: Option<regex::Regex>,
    text: String,
    case_sensitive: bool,
}

impl Matcher {
    fn new(q: &SearchQuery) -> Self {
        let re = if q.regex {
            regex::RegexBuilder::new(&q.text)
                .case_insensitive(!q.case_sensitive)
                .build()
                .ok()
        } else {
            None
        };
        Self {
            re,
            text: q.text.clone(),
            case_sensitive: q.case_sensitive,
        }
    }

    fn matches(&self, name: &str) -> bool {
        if let Some(re) = &self.re {
            return re.is_match(name);
        }
        if self.text.is_empty() {
            return true;
        }
        if self.case_sensitive {
            name.contains(&self.text)
        } else {
            contains_case_insensitive(name, &self.text)
        }
    }
}

fn run_search(job: &Job, ev_tx: &mpsc::SyncSender<SearchEvent>, cur_gen: &Arc<AtomicU64>) {
    let gen_id = job.gen_id;
    let q = &job.query;
    let is_current = || cur_gen.load(Ordering::SeqCst) == gen_id;
    let matcher = Matcher::new(q);
    let mut stack: Vec<(PathBuf, u32)> = vec![(q.root.clone(), 0)];
    let mut hits = Vec::with_capacity(BATCH);
    let mut total = 0usize;
    let mut checks = 0usize;

    while let Some((dir, depth)) = stack.pop() {
        if checks.is_multiple_of(256) && !is_current() {
            let _ = ev_tx.try_send(SearchEvent::Done {
                gen_id,
                total,
                cancelled: true,
            });
            return;
        }
        checks += 1;
        if q.max_depth.is_some_and(|m| depth > m) {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue; // permission denied etc. — skip, keep searching
        };
        for e in rd.flatten() {
            if total >= q.max_results {
                if !hits.is_empty() {
                    let _ = ev_tx.try_send(SearchEvent::Batch {
                        gen_id,
                        hits: std::mem::take(&mut hits),
                    });
                }
                let _ = ev_tx.try_send(SearchEvent::Done {
                    gen_id,
                    total,
                    cancelled: false,
                });
                return;
            }
            let name = e.file_name();
            let hidden = name.as_encoded_bytes().first() == Some(&b'.');
            if hidden && !q.include_hidden {
                continue;
            }
            let ft = e.file_type().ok();
            let is_dir = ft.is_some_and(|t| t.is_dir());
            let is_link = ft.is_some_and(|t| t.is_symlink());
            if matcher.matches(&name.to_string_lossy()) {
                let size = if is_dir {
                    flux_core::UNKNOWN_SIZE
                } else {
                    e.metadata().map(|m| m.len()).unwrap_or(0)
                };
                hits.push(SearchHit {
                    path: e.path(),
                    is_dir,
                    size,
                });
                total += 1;
                if hits.len() >= BATCH {
                    let _ = ev_tx.try_send(SearchEvent::Batch {
                        gen_id,
                        hits: std::mem::take(&mut hits),
                    });
                    hits = Vec::with_capacity(BATCH);
                }
            }
            // Recurse into real directories only — never through symlinks,
            // which makes traversal loop-proof without a visited set.
            if is_dir && !is_link {
                stack.push((e.path(), depth + 1));
            }
        }
    }
    if !hits.is_empty() {
        let _ = ev_tx.try_send(SearchEvent::Batch { gen_id, hits });
    }
    let _ = ev_tx.try_send(SearchEvent::Done {
        gen_id,
        total,
        cancelled: !is_current(),
    });
}
