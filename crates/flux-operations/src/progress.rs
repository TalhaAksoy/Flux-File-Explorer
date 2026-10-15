use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub enum OpState {
    Queued,
    /// Walking sources to learn total items/bytes.
    Discovering,
    Running,
    /// Blocked until the UI answers a conflict query.
    WaitingConflict,
    Done,
    /// Finished but some items failed; `errors` holds a bounded sample.
    DoneWithErrors,
    Cancelled,
    Failed(String),
}

impl OpState {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            OpState::Queued | OpState::Discovering | OpState::Running | OpState::WaitingConflict
        )
    }
    pub fn is_finished(&self) -> bool {
        !self.is_active()
    }
}

const MAX_RECORDED_ERRORS: usize = 64;

#[derive(Debug)]
pub struct OpProgress {
    pub state: OpState,
    /// Totals are 0/unknown while `state == Discovering` and stay None
    /// for operations where a byte total is meaningless (rename, trash).
    pub total_items: u64,
    pub done_items: u64,
    pub total_bytes: u64,
    pub done_bytes: u64,
    pub current_item: PathBuf,
    pub errors: Vec<String>,
    pub error_count: u64,
    pub started: Option<Instant>,
    pub finished: Option<Instant>,
    /// Set when the operation does byte-wise progress (copy/move).
    pub tracks_bytes: bool,
}

impl Default for OpProgress {
    fn default() -> Self {
        Self {
            state: OpState::Queued,
            total_items: 0,
            done_items: 0,
            total_bytes: 0,
            done_bytes: 0,
            current_item: PathBuf::new(),
            errors: Vec::new(),
            error_count: 0,
            started: None,
            finished: None,
            tracks_bytes: false,
        }
    }
}

impl OpProgress {
    pub fn record_error(&mut self, msg: String) {
        self.error_count += 1;
        if self.errors.len() < MAX_RECORDED_ERRORS {
            self.errors.push(msg);
        }
    }

    /// 0.0..=1.0 for display; None when indeterminate.
    pub fn fraction(&self) -> Option<f32> {
        if self.tracks_bytes && self.total_bytes > 0 {
            Some((self.done_bytes as f64 / self.total_bytes as f64) as f32)
        } else if self.total_items > 0 {
            Some((self.done_items as f64 / self.total_items as f64) as f32)
        } else {
            None
        }
    }

    pub fn elapsed(&self) -> Duration {
        match (self.started, self.finished) {
            (Some(s), Some(f)) => f.duration_since(s),
            (Some(s), None) => s.elapsed(),
            _ => Duration::ZERO,
        }
    }

    /// Bytes/sec over the operation lifetime; None when not meaningful.
    pub fn throughput_bps(&self) -> Option<f64> {
        if !self.tracks_bytes || self.done_bytes == 0 {
            return None;
        }
        let secs = self.elapsed().as_secs_f64();
        (secs > 0.05).then(|| self.done_bytes as f64 / secs)
    }
}
