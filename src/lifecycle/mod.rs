//! Single-lifecycle records and in-memory operations, independent of SQL and I/O.
//!
//! Ordinary operations enforce lifecycle, containment and dependency constraints.
//! Automatic three-way merging and durable publication belong to subsequent layers;
//! explicit integration here requires a choice for every Entity.
mod candidates;
mod codec;
mod model;
mod relations;
mod snapshot;

pub use candidates::{CandidateList, candidates};
pub use codec::{decode, encode};
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
