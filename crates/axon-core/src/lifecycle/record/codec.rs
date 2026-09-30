//! Canonical bytes of one record file: one line of JSON with keys in a fixed order, no
//! whitespace, minimal escapes, and a trailing LF. The record ID is the BLAKE3 hash of exactly
//! these bytes, so `decode` accepts only canonical input: the same content always has the same
//! bytes and therefore the same ID.
use super::model::{Current, Entry, Header, Note, Record, RecordId, RecordKind};
use super::{EntityId, Kind, Lifecycle, Nonce, Operation, Recorder, Result, StoreId};
use crate::lifecycle::invalid;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, ser::SerializeStruct};
use std::collections::BTreeSet;

pub const HEADER_FORMAT: &str = "axon-records/v1";

/// The spellings of kind and lifecycle are the declaration's.
use crate::declaration::{kind as kind_name, lifecycle as lifecycle_name};
fn kind_from(name: &str) -> Result<Kind> {
    [Kind::Issue, Kind::Group]
        .into_iter()
        .find(|kind| kind_name(*kind) == name)
        .ok_or_else(|| invalid(format!("unknown kind {name:?}")))
}
fn lifecycle_from(name: &str) -> Result<Lifecycle> {
    [
        Lifecycle::Undecided,
        Lifecycle::NotStarted,
        Lifecycle::InProgress,
        Lifecycle::Completed,
        Lifecycle::Cancelled,
    ]
    .into_iter()
    .find(|lifecycle| lifecycle_name(*lifecycle) == name)
    .ok_or_else(|| invalid(format!("unknown lifecycle {name:?}")))
}
fn operation_name(operation: Operation) -> &'static str {
    match operation {
        Operation::Accept => "accept",
        Operation::Withdraw => "withdraw",
        Operation::Start => "start",
        Operation::Release => "release",
        Operation::Complete => "complete",
        Operation::Cancel => "cancel",
        Operation::Reconsider => "reconsider",
        Operation::Reopen => "reopen",
    }
}
fn operation_from(name: &str) -> Result<Operation> {
    match name {
        "accept" => Ok(Operation::Accept),
        "withdraw" => Ok(Operation::Withdraw),
        "start" => Ok(Operation::Start),
        "release" => Ok(Operation::Release),
        "complete" => Ok(Operation::Complete),
        "cancel" => Ok(Operation::Cancel),
        "reconsider" => Ok(Operation::Reconsider),
        "reopen" => Ok(Operation::Reopen),
        _ => Err(invalid(format!("unknown operation {name:?}"))),
    }
}

/// `Option<Option<T>>` distinguishes an absent key (outer None) from an explicit null.
fn present<'de, D, T>(deserializer: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}
fn required<T>(field: &str, value: Option<T>) -> Result<T> {
    value.ok_or_else(|| invalid(format!("missing key {field:?}")))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AfterRow {
    kind: String,
    lifecycle: String,
    #[serde(default, deserialize_with = "present")]
    owner: Option<Option<String>>,
    title: String,
    description: String,
    #[serde(default, deserialize_with = "present")]
    condition: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    parent: Option<Option<EntityId>>,
    needs: Vec<EntityId>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    entity: EntityId,
    record: String,
    #[serde(default)]
    operation: Option<String>,
    parents: Vec<RecordId>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    chosen: Option<RecordId>,
    at: DateTime<Utc>,
    #[serde(default, deserialize_with = "present")]
    recorder: Option<Option<Recorder>>,
    #[serde(default, deserialize_with = "present")]
    reason: Option<Option<String>>,
    #[serde(default)]
    after: Option<AfterRow>,
    #[serde(default)]
    body: Option<String>,
}

struct CanonicalEntry<'a>(&'a Entry);
impl Serialize for CanonicalEntry<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        match self.0 {
            Entry::Record(record) => {
                #[derive(Serialize)]
                struct After<'a> {
                    kind: &'static str,
                    lifecycle: &'static str,
                    owner: &'a Option<String>,
                    title: &'a str,
                    description: &'a str,
                    condition: &'a Option<String>,
                    parent: &'a Option<EntityId>,
                    needs: &'a BTreeSet<EntityId>,
                }
                let optional = matches!(
                    record.kind,
                    RecordKind::Transition(_) | RecordKind::Resolve { .. }
                );
                let mut row = serializer.serialize_struct("Row", 7 + usize::from(optional))?;
                row.serialize_field("entity", &record.entity)?;
                row.serialize_field("record", record.kind.name())?;
                if let RecordKind::Transition(operation) = record.kind {
                    row.serialize_field("operation", operation_name(operation))?;
                }
                row.serialize_field("parents", &record.parents)?;
                if let RecordKind::Resolve { chosen } = &record.kind {
                    row.serialize_field("chosen", chosen)?;
                }
                row.serialize_field("at", &record.at)?;
                row.serialize_field("recorder", &record.recorder)?;
                row.serialize_field("reason", &record.reason)?;
                row.serialize_field(
                    "after",
                    &After {
                        kind: kind_name(record.after.kind),
                        lifecycle: lifecycle_name(record.after.lifecycle),
                        owner: &record.after.owner,
                        title: &record.after.title,
                        description: &record.after.description,
                        condition: &record.after.condition,
                        parent: &record.after.parent,
                        needs: &record.after.needs,
                    },
                )?;
                row.end()
            }
            Entry::Note(note) => {
                let mut row = serializer.serialize_struct("Row", 8)?;
                row.serialize_field("entity", &note.entity)?;
                row.serialize_field("record", "note")?;
                row.serialize_field("parents", &[] as &[RecordId])?;
                row.serialize_field("nonce", &note.nonce)?;
                row.serialize_field("at", &note.at)?;
                row.serialize_field("recorder", &note.recorder)?;
                row.serialize_field("reason", &note.reason)?;
                row.serialize_field("body", &note.body)?;
                row.end()
            }
        }
    }
}

fn entry(mut row: Row) -> Result<Entry> {
    let recorder = required("recorder", row.recorder)?;
    let reason = required("reason", row.reason)?;
    let parent_count = row.parents.len();
    let parents: BTreeSet<RecordId> = row.parents.into_iter().collect();
    if parents.len() != parent_count {
        return Err(invalid("duplicate parent"));
    }
    if row.record == "note" {
        if row.operation.is_some() || row.chosen.is_some() || row.after.is_some() {
            return Err(invalid("a note carries no operation, chosen head or after"));
        }
        if !parents.is_empty() {
            return Err(invalid("a note has no parents"));
        }
        return Ok(Entry::Note(Note {
            entity: row.entity,
            nonce: Nonce::try_from(required("nonce", row.nonce)?)?,
            at: row.at,
            recorder,
            reason,
            body: required("body", row.body)?,
        }));
    }
    if row.nonce.is_some() || row.body.is_some() {
        return Err(invalid("only a note carries a nonce and a body"));
    }
    let kind = match row.record.as_str() {
        "created" => RecordKind::Created,
        "transition" => RecordKind::Transition(operation_from(&required(
            "operation",
            row.operation.take(),
        )?)?),
        "edit" => RecordKind::Edit,
        "parent" => RecordKind::Parent,
        "dependency" => RecordKind::Dependency,
        "condition" => RecordKind::Condition,
        "convert" => RecordKind::Convert,
        "import" => RecordKind::Import,
        "resolve" => RecordKind::Resolve {
            chosen: required("chosen", row.chosen.take())?,
        },
        other => return Err(invalid(format!("unknown record kind {other:?}"))),
    };
    if row.operation.is_some() {
        return Err(invalid("only a transition carries an operation"));
    }
    if row.chosen.is_some() {
        return Err(invalid("only a resolve record carries a chosen head"));
    }
    let after = required("after", row.after)?;
    let need_count = after.needs.len();
    let needs: BTreeSet<EntityId> = after.needs.into_iter().collect();
    if needs.len() != need_count {
        return Err(invalid("duplicate dependency"));
    }
    Ok(Entry::Record(Record {
        entity: row.entity,
        kind,
        parents,
        at: row.at,
        recorder,
        reason,
        after: Current {
            kind: kind_from(&after.kind)?,
            lifecycle: lifecycle_from(&after.lifecycle)?,
            owner: required("owner", after.owner)?,
            title: after.title,
            description: after.description,
            condition: required("condition", after.condition)?,
            parent: required("parent", after.parent)?,
            needs,
        },
    }))
}

fn to_line<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(value).map_err(|error| invalid(error.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn from_line<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T> {
    if bytes.is_empty() {
        return Err(invalid("empty record"));
    }
    let Some(line) = bytes.strip_suffix(b"\n") else {
        return Err(invalid("record does not end with a line feed"));
    };
    if line.contains(&b'\n') {
        return Err(invalid("record spans more than one line"));
    }
    serde_json::from_slice(line).map_err(|error| invalid(error.to_string()))
}

/// The canonical bytes of one record after validating it.
pub fn encode(entry: &Entry) -> Result<Vec<u8>> {
    entry.validate()?;
    to_line(&CanonicalEntry(entry))
}

/// Decodes one record file. Rejects unknown, missing or kind-mismatched keys, values outside
/// the rules, truncated or empty input, and bytes that are not the canonical encoding of their
/// content. Returns the record ID computed from the bytes.
pub fn decode(bytes: &[u8]) -> Result<(RecordId, Entry)> {
    let entry = entry(from_line::<Row>(bytes)?)?;
    let canonical = encode(&entry)?;
    if canonical != bytes {
        return Err(invalid("record bytes are not canonical"));
    }
    Ok((RecordId::of(bytes), entry))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderRow {
    format: String,
    store: StoreId,
    prefix: String,
}

pub fn encode_header(header: &Header) -> Result<Vec<u8>> {
    header.validate()?;
    to_line(&HeaderRow {
        format: HEADER_FORMAT.into(),
        store: header.store.clone(),
        prefix: header.prefix.clone(),
    })
}

/// Rejects an unknown format instead of converting it.
pub fn decode_header(bytes: &[u8]) -> Result<Header> {
    let row: HeaderRow = from_line(bytes).map_err(|error| invalid(format!("header: {error}")))?;
    if row.format != HEADER_FORMAT {
        return Err(invalid(format!(
            "unsupported store format {:?}",
            row.format
        )));
    }
    let header = Header {
        store: row.store,
        prefix: row.prefix,
    };
    header.validate()?;
    Ok(header)
}
