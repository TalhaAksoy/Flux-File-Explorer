//! Compute worker: rebuilds a snapshot's sorted+filtered view off the UI
//! thread. Jobs carry a generation so stale rebuilds are dropped.

use std::sync::mpsc;
use std::thread::JoinHandle;

use crate::snapshot::SnapshotHandle;
use crate::sort::build_view;

#[derive(Debug)]
pub struct ViewRequest {
    pub snap: SnapshotHandle,
    pub gen_id: u64,
}

#[derive(Debug)]
pub enum ComputeEvent {
    /// The snapshot's `view` was replaced; `gen_id` for staleness checking.
    ViewReady { gen_id: u64 },
    /// Generation was stale; nothing was written.
    Stale { gen_id: u64 },
}

pub struct ComputeEngine {
    req_tx: mpsc::Sender<ViewRequest>,
    event_rx: mpsc::Receiver<ComputeEvent>,
    _worker: JoinHandle<()>,
}

impl Default for ComputeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputeEngine {
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<ViewRequest>();
        let (ev_tx, event_rx) = mpsc::channel::<ComputeEvent>();
        let worker = std::thread::Builder::new()
            .name("flux-view".into())
            .spawn(move || {
                while let Ok(mut job) = req_rx.recv() {
                    // Drain to the newest request; a queued older rebuild of the
                    // same snapshot is wasted work.
                    while let Ok(newer) = req_rx.try_recv() {
                        job = newer;
                    }
                    let gen_id = job.gen_id;
                    // 1. Capture inputs under a READ lock — concurrent with
                    //    UI readers, so rendering never stalls on us.
                    let inputs = match job.snap.read() {
                        Ok(s) if s.generation == gen_id => Some((
                            s.entries().to_vec(),
                            s.sort,
                            s.filter.clone(),
                            s.show_hidden,
                            s.version(),
                        )),
                        _ => None,
                    };
                    // 2. Sort/filter OUTSIDE any lock — this is the
                    //    expensive part and it must not hold a write lock
                    //    that blocks UI snapshot reads.
                    let ok = match inputs {
                        Some((entries, sort, filter, show_hidden, version)) => {
                            let view = build_view(
                                &entries,
                                &sort,
                                filter.as_deref(),
                                show_hidden,
                            );
                            // 3. Apply under a short WRITE lock, re-checking
                            //    every input that could have changed while we
                            //    computed (entries appended → version bumps;
                            //    sort/filter edits are compared directly).
                            match job.snap.write() {
                                Ok(mut s)
                                    if s.generation == gen_id
                                        && s.version() == version
                                        && s.sort == sort
                                        && s.filter == filter
                                        && s.show_hidden == show_hidden =>
                                {
                                    s.set_view(view);
                                    true
                                }
                                _ => false,
                            }
                        }
                        None => false,
                    };
                    let _ = ev_tx.send(if ok {
                        ComputeEvent::ViewReady { gen_id }
                    } else {
                        ComputeEvent::Stale { gen_id }
                    });
                }
            })
            .expect("spawn compute worker");
        Self {
            req_tx,
            event_rx,
            _worker: worker,
        }
    }

    /// Queue a view rebuild; newest request wins, so callers may spam freely.
    pub fn rebuild(&self, snap: &SnapshotHandle, gen_id: u64) {
        let _ = self.req_tx.send(ViewRequest {
            snap: snap.clone(),
            gen_id,
        });
    }

    pub fn drain_events(&self) -> impl Iterator<Item = ComputeEvent> + '_ {
        self.event_rx.try_iter()
    }
}
