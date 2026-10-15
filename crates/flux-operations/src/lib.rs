//! flux-operations: cancellable, observable file operations.
//!
//! A single bounded worker executes jobs serially (disk-friendly default;
//! parallelism is a config knob, not a free-for-all). Progress lives in a
//! shared registry the UI polls; conflicts pause the worker on a per-op
//! reply channel until the user (or an apply-to-all policy) answers.

pub mod conflict;
pub mod copy;
pub mod engine;
pub mod progress;
pub mod rename;

pub use conflict::{ConflictDecision, ConflictPolicy, ConflictQuery};
pub use engine::{OpEngine, OpEvent, OpHandle, OpId, OpJob, OpKind};
pub use progress::{OpProgress, OpState};
pub use rename::{RenameAction, RenameStatus, RenameRule, plan_renames};
