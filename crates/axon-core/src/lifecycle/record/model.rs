use super::{EntityId, Kind, Lifecycle, Operation, Recorder, Result, StoreId};
use crate::lifecycle::{TITLE_LIMIT, invalid, validate_line, validate_reason};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

/// A record ID is the BLAKE3 hash of the record's canonical bytes, in lowercase hex.
pub const RECORD_ID_LENGTH: usize = 64;
/// A Note's nonce is 32 lowercase hex characters of randomness.
pub const NONCE_LENGTH: usize = 32;

fn lowercase_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

macro_rules! hex_identifier {
    ($name:ident, $length:expr, $what:literal) => {
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
        #[serde(try_from = "String")]
        pub struct $name(String);
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }
        impl TryFrom<String> for $name {
            type Error = super::Error;
            fn try_from(value: String) -> Result<Self> {
                if !lowercase_hex(&value, $length) {
                    return Err(invalid(format!(
                        concat!(
                            "invalid ",
                            $what,
                            " {:?}: expected {} lowercase hex characters"
                        ),
                        value, $length
                    )));
                }
                Ok(Self(value))
            }
        }
        impl TryFrom<&str> for $name {
            type Error = super::Error;
            fn try_from(value: &str) -> Result<Self> {
                Self::try_from(value.to_string())
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0)
            }
        }
    };
}
hex_identifier!(RecordId, RECORD_ID_LENGTH, "record ID");
hex_identifier!(Nonce, NONCE_LENGTH, "Note nonce");

impl RecordId {
    /// The ID of a record file with exactly these bytes.
    pub fn of(bytes: &[u8]) -> Self {
        Self(blake3::hash(bytes).to_hex().to_string())
    }
    /// The subdirectory under the record directory: the first two characters of the ID.
    pub fn subdirectory(&self) -> &str {
        &self.0[..2]
    }
}
impl Nonce {
    pub fn generate() -> Self {
        Self(format!("{:032x}", rand::random::<u128>()))
    }
}

/// The current value a record leaves behind. Every record except a Note carries one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Current {
    pub kind: Kind,
    pub lifecycle: Lifecycle,
    /// The actor that started an InProgress Issue, when the recorder was known. Always None in
    /// any other lifecycle; it makes concurrent starts visible as different values.
    pub owner: Option<String>,
    pub title: String,
    pub description: String,
    /// None means no condition. The core never executes a stored command.
    pub condition: Option<String>,
    pub parent: Option<EntityId>,
    /// Outgoing dependencies.
    pub needs: BTreeSet<EntityId>,
}
impl Current {
    pub fn is_terminal(&self) -> bool {
        matches!(self.lifecycle, Lifecycle::Completed | Lifecycle::Cancelled)
    }
    pub(super) fn validate(&self) -> Result<()> {
        validate_line("title", &self.title, TITLE_LIMIT)?;
        if self.condition.as_ref().is_some_and(|s| s.trim().is_empty()) {
            return Err(invalid("empty condition command"));
        }
        if self.kind == Kind::Group && self.lifecycle == Lifecycle::InProgress {
            return Err(invalid("a Group never stores InProgress"));
        }
        if self.owner.is_some() && self.lifecycle != Lifecycle::InProgress {
            return Err(invalid("owner is set only while InProgress"));
        }
        if self.owner.as_ref().is_some_and(|o| o.trim().is_empty()) {
            return Err(invalid("empty owner"));
        }
        Ok(())
    }
}

/// What a record documents. The JSON `record` key names the variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordKind {
    Created,
    Transition(Operation),
    Edit,
    Parent,
    Dependency,
    Condition,
    Convert,
    /// One record for every Entity that `axon import apply` changes: title, description,
    /// parent and needs move to their final values together.
    Import,
    /// Joins every head of a conflicted Entity and takes the value of `chosen`.
    Resolve {
        chosen: RecordId,
    },
}
impl RecordKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Transition(_) => "transition",
            Self::Edit => "edit",
            Self::Parent => "parent",
            Self::Dependency => "dependency",
            Self::Condition => "condition",
            Self::Convert => "convert",
            Self::Import => "import",
            Self::Resolve { .. } => "resolve",
        }
    }
}

/// A record other than a Note: one immutable fact about an Entity with the value after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub entity: EntityId,
    pub kind: RecordKind,
    /// Empty for `Created`; every head for `Resolve`; exactly one otherwise.
    pub parents: BTreeSet<RecordId>,
    pub at: DateTime<Utc>,
    pub recorder: Option<Recorder>,
    pub reason: Option<String>,
    pub after: Current,
}
impl Record {
    pub(super) fn validate(&self) -> Result<()> {
        validate_reason(&self.reason)?;
        self.after.validate()?;
        match &self.kind {
            RecordKind::Created => {
                if !self.parents.is_empty() {
                    return Err(invalid("a created record has no parents"));
                }
                if !matches!(
                    self.after.lifecycle,
                    Lifecycle::Undecided | Lifecycle::NotStarted
                ) {
                    return Err(invalid("creation requires Undecided or NotStarted"));
                }
            }
            RecordKind::Resolve { chosen } => {
                if self.parents.len() < 2 {
                    return Err(invalid("a resolve record joins at least two heads"));
                }
                if !self.parents.contains(chosen) {
                    return Err(invalid("a resolve record chooses one of its parents"));
                }
            }
            RecordKind::Transition(operation) => {
                if self.parents.len() != 1 {
                    return Err(invalid("a transition record has exactly one parent"));
                }
                // A transition keeps the kind, so the kind after it is the kind at its time.
                // Whether some source lifecycle leads here is decidable without the parent.
                let reachable = [
                    Lifecycle::Undecided,
                    Lifecycle::NotStarted,
                    Lifecycle::InProgress,
                    Lifecycle::Completed,
                    Lifecycle::Cancelled,
                ]
                .into_iter()
                .any(|before| {
                    operation.next_as(self.after.kind, before) == Some(self.after.lifecycle)
                });
                if !reachable {
                    return Err(invalid(format!(
                        "{operation:?} never leads a {:?} to {:?}",
                        self.after.kind, self.after.lifecycle
                    )));
                }
                let actor = self.recorder.as_ref().map(|r| r.actor.clone());
                if *operation == Operation::Start && self.after.owner != actor {
                    return Err(invalid("a start record's owner is its recorder's actor"));
                }
            }
            _ => {
                if self.parents.len() != 1 {
                    return Err(invalid(format!(
                        "a {} record has exactly one parent",
                        self.kind.name()
                    )));
                }
                // A conversion keeps the lifecycle, which must have allowed it.
                if self.kind == RecordKind::Convert
                    && !matches!(
                        self.after.lifecycle,
                        Lifecycle::Undecided | Lifecycle::NotStarted
                    )
                {
                    return Err(invalid(
                        "a conversion record of a started or terminal Entity",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// A Note: append-only information without causal links to other records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub entity: EntityId,
    /// Randomness that keeps two Notes with the same body, time and recorder distinct.
    pub nonce: Nonce,
    pub at: DateTime<Utc>,
    pub recorder: Option<Recorder>,
    pub reason: Option<String>,
    pub body: String,
}
impl Note {
    pub(super) fn validate(&self) -> Result<()> {
        validate_reason(&self.reason)?;
        if self.body.trim().is_empty() {
            return Err(invalid("empty Note"));
        }
        Ok(())
    }
}

/// One record file: a Note or any other record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Record(Record),
    Note(Note),
}
impl Entry {
    pub fn entity(&self) -> &EntityId {
        match self {
            Self::Record(record) => &record.entity,
            Self::Note(note) => &note.entity,
        }
    }
    pub fn at(&self) -> DateTime<Utc> {
        match self {
            Self::Record(record) => record.at,
            Self::Note(note) => note.at,
        }
    }
    pub fn recorder(&self) -> Option<&Recorder> {
        match self {
            Self::Record(record) => record.recorder.as_ref(),
            Self::Note(note) => note.recorder.as_ref(),
        }
    }
    pub fn as_record(&self) -> Option<&Record> {
        match self {
            Self::Record(record) => Some(record),
            Self::Note(_) => None,
        }
    }
    pub fn as_note(&self) -> Option<&Note> {
        match self {
            Self::Record(_) => None,
            Self::Note(note) => Some(note),
        }
    }
    pub(super) fn validate(&self) -> Result<()> {
        match self {
            Self::Record(record) => record.validate(),
            Self::Note(note) => note.validate(),
        }
    }
}

/// The header file next to the record directory: the mark of a store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub store: StoreId,
    pub prefix: String,
}
impl Header {
    pub fn new(prefix: &str) -> Result<Self> {
        let header = Self {
            store: StoreId::generate(),
            prefix: prefix.to_string(),
        };
        header.validate()?;
        Ok(header)
    }
    pub(super) fn validate(&self) -> Result<()> {
        let allowed = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-';
        if self.prefix.is_empty()
            || self.prefix.starts_with('-')
            || self.prefix.ends_with('-')
            || !self.prefix.bytes().all(allowed)
        {
            return Err(invalid(format!(
                "invalid ID prefix {:?}: expected ASCII lowercase letters, digits and hyphens, starting and ending with a letter or digit",
                self.prefix
            )));
        }
        Ok(())
    }
}
