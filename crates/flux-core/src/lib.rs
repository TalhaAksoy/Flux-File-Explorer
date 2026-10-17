//! flux-core: GUI-independent filesystem model.
//!
//! Provides the directory entry representation, a mutable snapshot of one
//! directory's contents, a streaming background enumerator, and a compute
//! worker that builds sorted/filtered views of the snapshot off the UI thread.

pub mod compute;
pub mod dirsize;
pub mod entry;
pub mod enumerate;
pub mod file_type;
pub mod snapshot;
pub mod sort;

pub use compute::{ComputeEngine, ComputeEvent, ViewRequest};
pub use dirsize::{SizeEngine, SizeEvent};
pub use entry::{DirEntry, EntryKind};
pub use enumerate::{EnumEngine, EnumEvent, MetaEngine, MetaEvent};
pub use snapshot::{DirSnapshot, LoadState, SnapshotHandle};
pub use sort::{SortDir, SortKey, SortSpec};

/// Sentinel for "size not fetched / not applicable".
pub const UNKNOWN_SIZE: u64 = u64::MAX;

/// Index of an entry inside a snapshot's arena. Stable for the lifetime of
/// the snapshot: entries are only ever appended, never removed or reordered;
/// view ordering lives in a separate index vector.
pub type EntryIdx = u32;
