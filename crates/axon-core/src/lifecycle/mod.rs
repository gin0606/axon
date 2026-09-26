//! Lifecycle records and in-memory operations, independent of I/O.
//!
//! `record` derives every current value from the immutable record set and produces the one
//! record an ordinary operation adds; `candidates` selects and evaluates candidates. Durable
//! publication belongs to the storage adapters.
mod candidates;
mod model;
pub mod record;

pub use candidates::{CandidateList, Surfacing, candidates, candidates_filtered, list_candidates};
pub use model::*;
pub use record::{
    Current, Entry, HEADER_FORMAT, Header, NONCE_LENGTH, Nonce, Note, RECORD_ID_LENGTH, Record,
    RecordId, RecordKind, Settled, Store, View, Violation, ViolationKind, decode, decode_as,
    decode_header, encode, encode_header, new_entity_id,
};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub String);
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error(message.into())
}
