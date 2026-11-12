//! Lifecycle records and in-memory operations, independent of I/O.
//!
//! `record` derives every current value from the immutable record set and produces the one
//! record an ordinary operation adds; `candidates` selects and evaluates candidates. Durable
//! publication belongs to the storage adapters.
mod candidates;
mod model;
pub mod record;
mod refusal;

pub use candidates::{CandidateList, Observation, Surfacing, list_candidates};
pub use model::*;
pub use record::{
    Current, Entry, HEADER_FORMAT, Header, NONCE_LENGTH, Nonce, Note, RECORD_ID_LENGTH, Record,
    RecordId, RecordKind, Settled, Store, View, Violation, ViolationKind, decode, decode_header,
    encode, encode_header, new_entity_id,
};
pub use refusal::{Line, LineProblem, Refusal};

/// A rejected input or operation. The message is the diagnostic; a refusal that names the
/// Entities involved also carries the [`Refusal`] the message describes.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    message: String,
    refusal: Option<Refusal>,
    newer_format: Option<String>,
}
impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            refusal: None,
            newer_format: None,
        }
    }
    pub(crate) fn for_newer_format(message: impl Into<String>, format: String) -> Self {
        Self {
            newer_format: Some(format),
            ..Self::new(message)
        }
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    /// Why an ordinary operation was refused, when that is what the error is.
    pub fn refusal(&self) -> Option<&Refusal> {
        self.refusal.as_ref()
    }
    /// The format a header names when a later axon wrote it, so the store needs a newer
    /// binary rather than a repair.
    pub fn newer_format(&self) -> Option<&str> {
        self.newer_format.as_deref()
    }
}
impl From<Refusal> for Error {
    fn from(refusal: Refusal) -> Self {
        Self {
            message: refusal.to_string(),
            refusal: Some(refusal),
            newer_format: None,
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::new(message)
}
