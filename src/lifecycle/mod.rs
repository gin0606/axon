//! Single-lifecycle records and in-memory operations, independent of SQL and I/O.
//!
//! Ordinary operations enforce lifecycle, containment and dependency constraints.
//! Three-way merging preserves records and validates whole-plan selections.
//! Durable publication belongs to the storage adapters.
mod candidates;
mod codec;
mod merge;
mod model;
mod relations;
mod snapshot;

pub use candidates::{CandidateList, candidates};
pub use codec::{decode, encode};
pub use merge::MergePlan;
pub use model::*;
pub use snapshot::Snapshot;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub String);
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error(message.into())
}

#[cfg(test)]
mod tests;
