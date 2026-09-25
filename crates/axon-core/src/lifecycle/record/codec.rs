//! Canonical bytes of one record file: one line of JSON with keys in a fixed order, no
//! whitespace, minimal escapes, and a trailing LF. The record ID is the BLAKE3 hash of exactly
//! these bytes, so `decode` accepts only canonical input: the same content always has the same
//! bytes and therefore the same ID.
use super::model::{Current, Entry, Header, Note, Record, RecordId, RecordKind};
use super::{EntityId, Kind, Lifecycle, Nonce, Operation, Recorder, Result, StoreId};
use crate::lifecycle::invalid;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
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

#[derive(Serialize, Deserialize)]
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
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    entity: EntityId,
    record: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    operation: Option<String>,
    parents: Vec<RecordId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chosen: Option<RecordId>,
    at: DateTime<Utc>,
    #[serde(default, deserialize_with = "present")]
    recorder: Option<Option<Recorder>>,
    #[serde(default, deserialize_with = "present")]
    reason: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after: Option<AfterRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    body: Option<String>,
}

fn row(entry: &Entry) -> Row {
    match entry {
        Entry::Record(record) => Row {
            entity: record.entity.clone(),
            record: record.kind.name().into(),
            operation: match &record.kind {
                RecordKind::Transition(operation) => Some(operation_name(*operation).into()),
                _ => None,
            },
            parents: record.parents.iter().cloned().collect(),
            nonce: None,
            chosen: match &record.kind {
                RecordKind::Resolve { chosen } => Some(chosen.clone()),
                _ => None,
            },
            at: record.at,
            recorder: Some(record.recorder.clone()),
            reason: Some(record.reason.clone()),
            after: Some(AfterRow {
                kind: kind_name(record.after.kind).into(),
                lifecycle: lifecycle_name(record.after.lifecycle).into(),
                owner: Some(record.after.owner.clone()),
                title: record.after.title.clone(),
                description: record.after.description.clone(),
                condition: Some(record.after.condition.clone()),
                parent: Some(record.after.parent.clone()),
                needs: record.after.needs.iter().cloned().collect(),
            }),
            body: None,
        },
        Entry::Note(note) => Row {
            entity: note.entity.clone(),
            record: "note".into(),
            operation: None,
            parents: Vec::new(),
            nonce: Some(note.nonce.to_string()),
            chosen: None,
            at: note.at,
            recorder: Some(note.recorder.clone()),
            reason: Some(note.reason.clone()),
            after: None,
            body: Some(note.body.clone()),
        },
    }
}

fn entry(mut row: Row) -> Result<Entry> {
    let recorder = required("recorder", row.recorder)?;
    let reason = required("reason", row.reason)?;
    let parents: BTreeSet<RecordId> = row.parents.iter().cloned().collect();
    if parents.len() != row.parents.len() {
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
    let needs: BTreeSet<EntityId> = after.needs.iter().cloned().collect();
    if needs.len() != after.needs.len() {
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
    to_line(&row(entry))
}

/// Decodes one record file. Rejects unknown, missing or kind-mismatched keys, values outside
/// the rules, truncated or empty input, and bytes that are not the canonical encoding of their
/// content. Returns the record ID computed from the bytes.
pub fn decode(bytes: &[u8]) -> Result<(RecordId, Entry)> {
    Ok((RecordId::of(bytes), decode_canonical(bytes)?))
}
fn decode_canonical(bytes: &[u8]) -> Result<Entry> {
    let entry = entry(from_line::<Row>(bytes)?)?;
    let canonical = encode(&entry)?;
    if canonical != bytes {
        return Err(invalid("record bytes are not canonical"));
    }
    Ok(entry)
}

/// Decodes a record file whose name is `id`, rejecting bytes whose hash is not the name.
pub fn decode_as(id: &RecordId, bytes: &[u8]) -> Result<Entry> {
    let actual = RecordId::of(bytes);
    if &actual != id {
        return Err(invalid(format!(
            "record {id} has content whose hash is {actual}"
        )));
    }
    decode_canonical(bytes)
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
