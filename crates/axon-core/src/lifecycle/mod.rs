//! Lifecycle records and in-memory operations, independent of I/O.
//!
//! `record` derives every current value from the immutable record set and produces the one
//! record an ordinary operation adds; `candidates` selects and evaluates candidates. Durable
//! publication belongs to the storage adapters.
mod candidates;
mod model;
pub mod record;
mod refusal;

pub use candidates::{CandidateList, Surfacing, list_candidates};
pub use model::*;
pub use record::{
    Current, Entry, HEADER_FORMAT, Header, NONCE_LENGTH, Nonce, Note, RECORD_ID_LENGTH, Record,
    RecordId, RecordKind, Settled, Store, View, Violation, ViolationKind, decode, decode_header,
    encode, encode_header, new_entity_id,
};
pub use refusal::Refusal;

/// A rejected input or operation. The message is the diagnostic; a refusal that names the
/// Entities involved also carries the [`Refusal`] the message describes.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    message: String,
    refusal: Option<Refusal>,
}
impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            refusal: None,
        }
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    /// Why a structural operation was refused, when that is what the error is.
    pub fn refusal(&self) -> Option<&Refusal> {
        self.refusal.as_ref()
    }
}
impl From<Refusal> for Error {
    fn from(refusal: Refusal) -> Self {
        Self {
            message: refusal.to_string(),
            refusal: Some(refusal),
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::new(message)
}
