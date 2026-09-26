//! The record set: immutable records, one per file, from which every current value is derived.
//!
//! A `Store` holds records keyed by their content hash. A `View` derives heads, conflicts,
//! settled current values, gaps and structural violations from the whole set, independent of
//! the order the records were read in. Ordinary operations check their prerequisites against a
//! `View` and produce exactly one new record; resolution joins every head of a conflicted
//! Entity. `encode` / `decode` map one record to its canonical bytes and back.
//!
//! This module is the core of the storage layer. It shares the lifecycle vocabulary (`Kind`,
//! `Lifecycle`, `Operation`, `Recorder`, `EntityId`, `StoreId`) with the lifecycle model.
mod codec;
mod model;
mod ops;
mod store;
mod view;

pub use codec::{HEADER_FORMAT, decode, decode_as, decode_header, encode, encode_header};
pub use model::{
    Current, Entry, Header, NONCE_LENGTH, Nonce, Note, RECORD_ID_LENGTH, Record, RecordId,
    RecordKind,
};
pub use store::Store;
pub use view::{Settled, View, Violation, ViolationKind};

pub use super::{Context, EntityId, Error, Kind, Lifecycle, Operation, Recorder, Result, StoreId};

impl Entry {
    /// The ID this record has once encoded: the hash of its canonical bytes.
    pub fn id(&self) -> Result<RecordId> {
        Ok(RecordId::of(&encode(self)?))
    }
}

/// A new Entity ID with an 8-character random part in lowercase Crockford Base32 (`i`, `l`,
/// `o` and `u` excluded). Existing 6-character IDs stay valid; IDs are opaque strings. Fails
/// on a prefix outside the ID's character rules.
pub fn new_entity_id(prefix: &str) -> Result<EntityId> {
    const ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let suffix: String = (0..8)
        .map(|_| ALPHABET[rand::random_range(0..ALPHABET.len())] as char)
        .collect();
    EntityId::try_from(format!("{prefix}-{suffix}"))
}

#[cfg(test)]
mod tests;
